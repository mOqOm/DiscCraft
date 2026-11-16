//! Where the GOPs of an MPEG-2 video elementary stream start, for placing chapters as Encore CS6 does.
//!
//! Encore puts a chapter on the first displayed frame of a GOP (docs/encore-behavior.md 7.1 and 9.9): the last
//! GOP start at or before the playhead, or the next one when a chapter is already there. A GOP's first displayed
//! frame is the number of pictures coded before its header: an open GOP's leading B pictures follow the I
//! picture in the stream but are shown before it, and after every picture of the GOPs before.
//!
//! Written from ISO/IEC 13818-2 (start codes: sequence header `00 00 01 B3`, group `00 00 01 B8`, picture
//! `00 00 01 00`). Only streams that are used as they are have GOPs to respect; an asset that is transcoded gets
//! its GOPs from the transcode (which starts one at every chapter).

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// The most GOP starts kept for one file (hours of video at one GOP every few frames).
const MAX_GOPS: usize = 4_000_000;
/// How much of the file is read at a time.
const CHUNK: usize = 1 << 20;

/// The GOPs of one MPEG-2 video elementary stream.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Gops {
    /// The display frame each GOP starts on, in order.
    pub(crate) starts: Vec<i64>,
    /// How many pictures the stream has (its length in frames).
    pub(crate) pictures: i64,
}

/// GOP starts of the files scanned so far, by path; a file is scanned again when its size or time changes.
#[derive(Default)]
pub(crate) struct GopCache {
    files: HashMap<PathBuf, (u64, Option<SystemTime>, Option<Arc<Gops>>)>,
}

impl GopCache {
    /// The display frame each GOP of the MPEG-2 video at `path` starts on, in order (an elementary stream, or the
    /// video of a program stream), or `None` when the file is neither (or cannot be read).
    pub(crate) fn starts(&mut self, path: &Path) -> Option<Arc<Gops>> {
        let metadata = std::fs::metadata(path).ok()?;
        let stamp = (metadata.len(), metadata.modified().ok());
        if let Some((len, modified, starts)) = self.files.get(path)
            && (*len, *modified) == stamp
        {
            return starts.clone();
        }
        let starts = File::open(path).ok().and_then(scan_file).map(Arc::new);
        let _ = self
            .files
            .insert(path.to_path_buf(), (stamp.0, stamp.1, starts.clone()));
        starts
    }
}

/// Follows an MPEG-2 video elementary stream as its bytes come, noting where its GOPs start.
struct Scanner {
    starts: Vec<i64>,
    pictures: i64,
    /// The last three bytes seen, to find start codes across the borders of what is fed.
    window: [u8; 3],
    first_code: bool,
    /// The stream is not one whose GOPs can be told (it does not begin with a sequence header, or has too many).
    failed: bool,
}

impl Scanner {
    fn new() -> Self {
        Self {
            starts: Vec::new(),
            pictures: 0,
            window: [0xFF; 3],
            first_code: true,
            failed: false,
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        if self.failed {
            return;
        }
        for &byte in bytes {
            if self.window == [0, 0, 1] {
                if self.first_code && byte != 0xB3 {
                    self.failed = true;
                    return;
                }
                self.first_code = false;
                match byte {
                    0x00 => self.pictures = self.pictures.saturating_add(1),
                    0xB8 => {
                        if self.starts.len() >= MAX_GOPS {
                            self.failed = true;
                            return;
                        }
                        self.starts.push(self.pictures);
                    }
                    _ => {}
                }
            }
            self.window = [self.window[1], self.window[2], byte];
        }
    }

    fn finish(self) -> Option<Gops> {
        (!self.failed && !self.starts.is_empty()).then_some(Gops {
            starts: self.starts,
            pictures: self.pictures,
        })
    }
}

impl std::io::Write for Scanner {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.feed(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Scans an MPEG-2 video elementary stream for its GOP starts. `None` unless the input begins with a sequence
/// header and has at least one GOP.
pub(crate) fn scan(mut input: impl Read) -> Option<Gops> {
    let mut scanner = Scanner::new();
    let mut buffer = vec![0_u8; CHUNK];
    loop {
        let read = match input.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        };
        scanner.feed(buffer.get(..read)?);
        if scanner.failed {
            return None;
        }
    }
    scanner.finish()
}

/// Scans the video of an MPEG-2 program stream (a `.mpg` / `.vob`, which goes on a disc as it is when it fits:
/// M12.6) for its GOP starts, the same way.
pub(crate) fn scan_program_stream(input: impl Read) -> Option<Gops> {
    let mut scanner = Scanner::new();
    let mut input = std::io::BufReader::with_capacity(CHUNK, input);
    disccraft_psmux::demux::demux(&mut input, Some(&mut scanner), None, None).ok()?;
    scanner.finish()
}

/// Scans a file: a program stream by its video, anything else as an elementary stream.
fn scan_file(mut file: File) -> Option<Gops> {
    use std::io::{Seek, SeekFrom};
    let mut head = [0_u8; 16];
    let mut filled = 0;
    while let Some(rest) = head.get_mut(filled..).filter(|rest| !rest.is_empty()) {
        match file.read(rest) {
            Ok(0) => break,
            Ok(read) => filled = filled.saturating_add(read),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    let _ = file.seek(SeekFrom::Start(0)).ok()?;
    if disccraft_psmux::demux::is_program_stream(head.get(..filled)?) {
        scan_program_stream(file)
    } else {
        scan(file)
    }
}

/// The frame a chapter asked for at `frame` goes to: the last GOP start at or before it, or — when a chapter
/// already sits there (`taken`) — the next GOP start. `None` when no GOP start is free.
pub(crate) fn chapter_frame(
    starts: &[i64],
    frame: i64,
    taken: impl Fn(i64) -> bool,
) -> Option<i64> {
    let after = starts.partition_point(|start| *start <= frame);
    let at = after
        .checked_sub(1)
        .and_then(|index| starts.get(index).copied());
    match at {
        Some(start) if !taken(start) => Some(start),
        _ => starts.get(after).copied().filter(|start| !taken(*start)),
    }
}

#[cfg(test)]
mod tests {
    use super::{chapter_frame, scan};

    /// A stream of start codes: sequence header, then per GOP a group header and `pictures` picture headers.
    fn stream(gops: &[usize]) -> Vec<u8> {
        let mut out = vec![0, 0, 1, 0xB3, 0x2D, 0x01, 0xE0];
        for pictures in gops {
            out.extend_from_slice(&[0, 0, 1, 0xB8, 0x00, 0x08]);
            for _ in 0..*pictures {
                out.extend_from_slice(&[
                    0, 0, 1, 0x00, 0x00, 0x0F, 0xFF, 0xF8, 0, 0, 1, 0x01, 0x12,
                ]);
            }
        }
        out
    }

    #[test]
    fn gop_starts_are_the_pictures_before_each_group() {
        // Encore CS6, PAL clip with GOPs of 12 whose first GOP is closed (I P B B P B B P B B = 10 pictures) and
        // the others open: chapters went to frames 10, 22, 34, … (two frames before each I picture).
        let gops = scan(stream(&[10, 12, 12]).as_slice());
        assert_eq!(
            gops.as_ref().map(|gops| gops.starts.clone()),
            Some(vec![0, 10, 22])
        );
        assert_eq!(gops.map(|gops| gops.pictures), Some(34));
        // Not a video elementary stream, and one without a GOP header.
        assert_eq!(scan(&[0_u8, 0, 1, 0xBA, 0, 0, 1, 0xB8][..]), None);
        assert_eq!(scan(&[0_u8, 0, 1, 0xB3, 0, 0, 1, 0x00][..]), None);
        assert_eq!(scan(&[][..]), None);
    }

    #[test]
    fn a_chapter_goes_to_the_gop_before_it_or_the_next_free_one() {
        // NTSC clip with a GOP every 13 frames, as measured in Encore CS6.
        let starts: Vec<i64> = (0..10).map(|gop| gop * 13).collect();
        let free = |_: i64| false;
        assert_eq!(chapter_frame(&starts, 13, free), Some(13));
        assert_eq!(chapter_frame(&starts, 25, free), Some(13));
        assert_eq!(chapter_frame(&starts, 26, free), Some(26));
        // The GOP before is taken (the first chapter sits on frame 0): the next one.
        assert_eq!(chapter_frame(&starts, 5, |frame| frame == 0), Some(13));
        assert_eq!(chapter_frame(&starts, 30, |frame| frame == 26), Some(39));
        // Both taken, or past the last GOP with it taken: nowhere.
        assert_eq!(
            chapter_frame(&starts, 30, |frame| frame == 26 || frame == 39),
            None
        );
        assert_eq!(chapter_frame(&starts, 500, |frame| frame == 117), None);
        assert_eq!(chapter_frame(&starts, 500, free), Some(117));
    }

    /// The video packed in a program stream (a `.mpg` / `.vob`) has the same GOP starts as the stream alone.
    #[test]
    fn a_program_streams_video_is_scanned_like_the_stream_alone() {
        let video = stream(&[3, 2, 4]);
        // Pack the stream into PES packets of five bytes, each after a pack header.
        let mut packed = Vec::new();
        for chunk in video.chunks(5) {
            packed.extend_from_slice(&[0, 0, 1, 0xBA, 0x44, 0, 4, 0, 4, 1, 1, 0x89, 0xC3, 0xF8]);
            let length = u16::try_from(3 + chunk.len()).unwrap_or(0);
            packed.extend_from_slice(&[0, 0, 1, 0xE0]);
            packed.extend_from_slice(&length.to_be_bytes());
            packed.extend_from_slice(&[0x81, 0, 0]);
            packed.extend_from_slice(chunk);
        }
        assert_eq!(
            super::scan_program_stream(packed.as_slice()),
            scan(video.as_slice())
        );
        assert!(scan(video.as_slice()).is_some());
    }
}
