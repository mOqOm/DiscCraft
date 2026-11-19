//! Program stream demultiplexer: takes the video elementary stream and the first AC-3 stream out of an MPEG-2
//! program stream (a `.mpg` / `.vob` file), so that video which already fits a DVD is repackaged as it is instead
//! of being encoded again.
//!
//! Written from ISO/IEC 13818-1 (MPEG-2 Systems: pack header §2.5.3.3, system header §2.5.3.5, PES packet §2.4.3.6)
//! and the DVD-Video use of private stream 1 (substream ids 0x80–0x87 for AC-3, each packet's payload starting with
//! the substream id, a frame count and a two-byte pointer to the first access unit). An MPEG-1 system stream (a VCD
//! or other MPEG-1 `.mpg`, M6.350) is read too, from ISO/IEC 11172-1 (pack header §2.4.3.2: `0010`, then the system
//! clock reference and mux rate, eight bytes in all; packet §2.4.3.3: stuffing bytes, an optional STD buffer field,
//! then the time stamps or `0000 1111`).

use std::io::{self, Read, Write};

/// Pack start code (`00 00 01 BA`).
const PACK: u8 = 0xBA;
/// Program end code.
const PROGRAM_END: u8 = 0xB9;
/// Private stream 1 (DVD: AC-3, DTS, LPCM and subpictures by substream id).
const PRIVATE_1: u8 = 0xBD;
/// The bytes of a private stream 1 payload in front of the AC-3 data: substream id, frame count, pointer.
const AC3_HEADER: usize = 4;
/// How many bytes may be skipped looking for the next start code before the file is given up as damaged.
const MAX_RESYNC: u64 = 16 * 1024 * 1024;

/// What a demultiplexing pass found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Demuxed {
    /// Bytes of video elementary stream written.
    pub video_bytes: u64,
    /// Bytes of AC-3 written.
    pub ac3_bytes: u64,
    /// Whether the stream carries sound other than the AC-3 taken out (MPEG audio, LPCM, DTS, more AC-3 streams).
    pub other_audio: bool,
}

/// Whether `head` (the start of a file) is a program stream: a pack start code whose next bits are `01` (MPEG-2)
/// or `0010` (an MPEG-1 system stream, M6.350).
#[must_use]
pub fn is_program_stream(head: &[u8]) -> bool {
    head.starts_with(&[0, 0, 1, PACK])
        && head
            .get(4)
            .is_some_and(|byte| byte & 0xC0 == 0x40 || byte & 0xF0 == 0x20)
}

/// Reads the program stream `input` and writes the payload of its first video stream to `video` and of its first
/// AC-3 stream to `ac3` (either may be left out). With `video_limit`, reading stops once that many video bytes are
/// out (a look at the start of the video).
///
/// # Errors
///
/// When reading or writing fails, or the stream is too damaged to follow.
pub fn demux(
    input: &mut dyn Read,
    video: Option<&mut dyn Write>,
    ac3: Option<&mut dyn Write>,
    video_limit: Option<u64>,
) -> io::Result<Demuxed> {
    demux_with(input, video, ac3, None, video_limit)
}

/// Reads the program stream `input` and writes the payload of its first MPEG audio stream (stream ids 0xC0–0xDF:
/// a VCD's MPEG-1 Layer II, M6.350) to `audio`; how many bytes.
///
/// # Errors
///
/// When reading or writing fails, or the stream is too damaged to follow.
pub fn demux_mpeg_audio(input: &mut dyn Read, audio: &mut dyn Write) -> io::Result<u64> {
    let mut written = 0_u64;
    let mut counting = CountingWriter {
        inner: audio,
        written: &mut written,
    };
    demux_with(input, None, None, Some(&mut counting), None)?;
    Ok(written)
}

/// A writer that counts what goes through it.
struct CountingWriter<'a> {
    inner: &'a mut dyn Write,
    written: &'a mut u64,
}

impl Write for CountingWriter<'_> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let count = self.inner.write(data)?;
        *self.written = self
            .written
            .saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// [`demux`], also writing the first MPEG audio stream to `mpeg_audio`.
fn demux_with(
    input: &mut dyn Read,
    mut video: Option<&mut dyn Write>,
    mut ac3: Option<&mut dyn Write>,
    mut mpeg_audio: Option<&mut dyn Write>,
    video_limit: Option<u64>,
) -> io::Result<Demuxed> {
    let mut found = Demuxed::default();
    let mut video_id: Option<u8> = None;
    let mut ac3_id: Option<u8> = None;
    let mut mpeg_audio_id: Option<u8> = None;
    let mut window = [0xFF_u8; 3];
    let mut skipped = 0_u64;
    let mut packet = Vec::new();
    loop {
        // The next start code: `00 00 01`, then the stream id.
        let mut byte = [0_u8; 1];
        if !read_or_end(input, &mut byte)? {
            break;
        }
        if window != [0, 0, 1] {
            window = [window[1], window[2], byte[0]];
            skipped = skipped.saturating_add(1);
            if skipped > MAX_RESYNC {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "no start code found in the program stream",
                ));
            }
            continue;
        }
        window = [0xFF; 3];
        skipped = 0;
        let id = byte[0];
        match id {
            PROGRAM_END => break,
            PACK => {
                let mut first = [0_u8; 1];
                if !read_or_end(input, &mut first)? {
                    break;
                }
                // MPEG-1: seven more bytes (clock reference and mux rate).
                if first[0] & 0xF0 == 0x20 {
                    let mut rest = [0_u8; 7];
                    if !read_or_end(input, &mut rest)? {
                        break;
                    }
                    continue;
                }
                // MPEG-2: nine more bytes, the last one's low three bits counting the stuffing bytes after it.
                let mut header = [0_u8; 9];
                if !read_or_end(input, &mut header)? {
                    break;
                }
                let stuffing = usize::from(header[8] & 0x07);
                let mut stuffed = [0_u8; 7];
                if let Some(buffer) = stuffed.get_mut(..stuffing)
                    && !read_or_end(input, buffer)?
                {
                    break;
                }
            }
            // System header, program stream map, padding, private stream 2, and every PES packet: a two-byte
            // length, then that many bytes.
            0xBB..=0xFF => {
                let mut length = [0_u8; 2];
                if !read_or_end(input, &mut length)? {
                    break;
                }
                packet.resize(usize::from(u16::from_be_bytes(length)), 0);
                if !read_or_end(input, &mut packet)? {
                    break;
                }
                match id {
                    0xE0..=0xEF => {
                        if *video_id.get_or_insert(id) != id {
                            continue;
                        }
                        let data = pes_payload(&packet);
                        if let Some(out) = video.as_mut() {
                            out.write_all(data)?;
                        }
                        found.video_bytes = found
                            .video_bytes
                            .saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
                        if video_limit.is_some_and(|limit| found.video_bytes >= limit) {
                            break;
                        }
                    }
                    PRIVATE_1 => {
                        let data = pes_payload(&packet);
                        match data.first().copied() {
                            Some(substream @ 0x80..=0x87) => {
                                if *ac3_id.get_or_insert(substream) != substream {
                                    found.other_audio = true;
                                    continue;
                                }
                                let sound = data.get(AC3_HEADER..).unwrap_or_default();
                                if let Some(out) = ac3.as_mut() {
                                    out.write_all(sound)?;
                                }
                                found.ac3_bytes = found
                                    .ac3_bytes
                                    .saturating_add(u64::try_from(sound.len()).unwrap_or(u64::MAX));
                            }
                            // DTS (0x88–0x8F) and LPCM (0xA0–0xA7); subpictures (0x20–0x3F) are not sound.
                            Some(0x88..=0x8F | 0xA0..=0xA7) => found.other_audio = true,
                            _ => {}
                        }
                    }
                    // MPEG audio.
                    0xC0..=0xDF => {
                        found.other_audio = true;
                        if *mpeg_audio_id.get_or_insert(id) == id
                            && let Some(out) = mpeg_audio.as_mut()
                        {
                            out.write_all(pes_payload(&packet))?;
                        }
                    }
                    _ => {}
                }
            }
            // A start code of the video layer inside a damaged stretch: look on.
            _ => {}
        }
    }
    Ok(found)
}

/// The elementary stream bytes of a PES packet's body (what follows the packet length). MPEG-2: past the two flag
/// bytes and the header data whose length the third byte gives. MPEG-1 (M6.350): past the stuffing bytes, the STD
/// buffer field (`01`) and the time stamps (`0010`: PTS, `0011`: PTS and DTS, `0000 1111`: none). Empty when the
/// body is neither.
fn pes_payload(body: &[u8]) -> &[u8] {
    let Some(first) = body.first() else {
        return &[];
    };
    if first & 0xC0 == 0x80 {
        let Some(header_length) = body.get(2) else {
            return &[];
        };
        return body
            .get(usize::from(*header_length).saturating_add(3)..)
            .unwrap_or_default();
    }
    // MPEG-1: at most 16 stuffing bytes.
    let mut at = body
        .iter()
        .take(16)
        .take_while(|byte| **byte == 0xFF)
        .count();
    if body.get(at).is_some_and(|byte| byte & 0xC0 == 0x40) {
        at += 2;
    }
    let stamps = match body.get(at) {
        Some(byte) if byte & 0xF0 == 0x20 => 5,
        Some(byte) if byte & 0xF0 == 0x30 => 10,
        Some(0x0F) => 1,
        _ => return &[],
    };
    body.get(at + stamps..).unwrap_or_default()
}

/// Fills `buffer` from `input`; `false` when the input ends first.
fn read_or_end(input: &mut dyn Read, buffer: &mut [u8]) -> io::Result<bool> {
    let mut filled = 0;
    while let Some(rest) = buffer.get_mut(filled..).filter(|rest| !rest.is_empty()) {
        match input.read(rest) {
            Ok(0) => return Ok(false),
            Ok(read) => filled = filled.saturating_add(read),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{Demuxed, demux, is_program_stream};

    fn pack() -> Vec<u8> {
        // Pack header with two stuffing bytes.
        let mut bytes = vec![
            0, 0, 1, 0xBA, 0x44, 0, 4, 0, 4, 1, 1, 0x89, 0xC3, 0xFA, 0xFF, 0xFF,
        ];
        bytes.truncate(16);
        bytes
    }

    fn pes(id: u8, header: &[u8], payload: &[u8]) -> Vec<u8> {
        let length = u16::try_from(3 + header.len() + payload.len()).unwrap_or(0);
        let mut bytes = vec![0, 0, 1, id];
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(&[0x81, 0x80, u8::try_from(header.len()).unwrap_or(0)]);
        bytes.extend_from_slice(header);
        bytes.extend_from_slice(payload);
        bytes
    }

    fn stream() -> Vec<u8> {
        let mut bytes = pack();
        // System header and a navigation packet (private stream 2), as a VOB has them.
        bytes.extend_from_slice(&[0, 0, 1, 0xBB, 0, 3, 9, 9, 9]);
        bytes.extend_from_slice(&[0, 0, 1, 0xBF, 0, 2, 7, 7]);
        bytes.extend(pes(0xE0, &[0x21, 0, 1, 0, 1], &[0, 0, 1, 0xB3, 1, 2]));
        bytes.extend(pack());
        bytes.extend(pes(0xBD, &[], &[0x80, 1, 0, 1, 0x0B, 0x77, 5]));
        // A second AC-3 stream and MPEG audio: noticed, not taken.
        bytes.extend(pes(0xBD, &[], &[0x81, 1, 0, 1, 0x0B, 0x77, 9]));
        bytes.extend(pes(0xC0, &[], &[0xFF, 0xFD]));
        // Another video stream is left alone.
        bytes.extend(pes(0xE1, &[], &[0xEE]));
        bytes.extend(pes(0xE0, &[], &[3, 4]));
        // Padding.
        bytes.extend_from_slice(&[0, 0, 1, 0xBE, 0, 2, 0xFF, 0xFF]);
        bytes.extend_from_slice(&[0, 0, 1, 0xB9]);
        bytes
    }

    #[test]
    fn the_video_and_the_first_ac3_stream_come_out() -> Result<(), std::io::Error> {
        let bytes = stream();
        assert!(is_program_stream(&bytes));
        let (mut video, mut sound) = (Vec::new(), Vec::new());
        let found = demux(
            &mut bytes.as_slice(),
            Some(&mut video),
            Some(&mut sound),
            None,
        )?;
        assert_eq!(video, [0, 0, 1, 0xB3, 1, 2, 3, 4]);
        assert_eq!(sound, [0x0B, 0x77, 5]);
        assert_eq!(
            found,
            Demuxed {
                video_bytes: 8,
                ac3_bytes: 3,
                other_audio: true
            }
        );
        Ok(())
    }

    #[test]
    fn a_look_at_the_start_stops_early() -> Result<(), std::io::Error> {
        let bytes = stream();
        let mut video = Vec::new();
        let found = demux(&mut bytes.as_slice(), Some(&mut video), None, Some(4))?;
        assert_eq!(video, [0, 0, 1, 0xB3, 1, 2]);
        assert_eq!(found.ac3_bytes, 0);
        Ok(())
    }

    /// Cut-off and foreign data end the pass or are skipped; nothing panics.
    #[test]
    fn damaged_streams_are_followed_as_far_as_they_go() -> Result<(), std::io::Error> {
        let bytes = stream();
        for cut in 0..bytes.len() {
            let mut video = Vec::new();
            let _ = demux(&mut &bytes[..cut], Some(&mut video), None, None)?;
            assert!(video.len() <= 8);
        }
        // Not a program stream: an elementary stream, a pack of neither MPEG, nothing.
        assert!(!is_program_stream(&[0, 0, 1, 0xB3, 0x2D]));
        assert!(!is_program_stream(&[0, 0, 1, 0xBA, 0x11]));
        assert!(!is_program_stream(&[]));
        let mut video = Vec::new();
        let found = demux(&mut [9_u8; 64].as_slice(), Some(&mut video), None, None)?;
        assert_eq!(found, Demuxed::default());
        Ok(())
    }

    /// M6.350: an MPEG-1 system stream (a VCD's) gives its video too: eight-byte packs, packets with stuffing, an STD
    /// buffer field and time stamps (or none).
    #[test]
    fn an_mpeg1_system_stream_gives_its_video() -> Result<(), std::io::Error> {
        let mut bytes = vec![0, 0, 1, 0xBA, 0x21, 0, 1, 0, 1, 0x80, 0x1C, 0x61];
        // System header.
        bytes.extend_from_slice(&[0, 0, 1, 0xBB, 0, 3, 9, 9, 9]);
        // Video: two stuffing bytes, the STD buffer, a PTS and DTS.
        let header = [0xFF, 0xFF, 0x60, 0x2E, 0x31, 0, 1, 0, 1, 0x11, 0, 1, 0, 1];
        let payload = [0, 0, 1, 0xB3, 0x16, 0];
        let length = u16::try_from(header.len() + payload.len()).unwrap_or(0);
        bytes.extend_from_slice(&[0, 0, 1, 0xE0]);
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(&payload);
        bytes.extend_from_slice(&[0, 0, 1, 0xBA, 0x21, 0, 1, 0, 1, 0x80, 0x1C, 0x61]);
        // MPEG audio, then video with a PTS only, then with no time stamp.
        bytes.extend_from_slice(&[0, 0, 1, 0xC0, 0, 3, 0x0F, 0xFF, 0xFD]);
        bytes.extend_from_slice(&[0, 0, 1, 0xE0, 0, 7, 0x21, 0, 1, 0, 1, 7, 8]);
        bytes.extend_from_slice(&[0, 0, 1, 0xE0, 0, 2, 0x0F, 9]);
        bytes.extend_from_slice(&[0, 0, 1, 0xB9]);
        assert!(is_program_stream(&bytes));
        let mut video = Vec::new();
        let found = demux(&mut bytes.as_slice(), Some(&mut video), None, None)?;
        assert_eq!(video, [0, 0, 1, 0xB3, 0x16, 0, 7, 8, 9]);
        assert!(found.other_audio);
        // Its MPEG audio, for the sound to be converted from.
        let mut audio = Vec::new();
        let written = super::demux_mpeg_audio(&mut bytes.as_slice(), &mut audio)?;
        assert_eq!((written, audio), (2, vec![0xFF, 0xFD]));
        Ok(())
    }
}
