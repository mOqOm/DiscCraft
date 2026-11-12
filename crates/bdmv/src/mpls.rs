//! Movie PlayList (MPLS) files.
//!
//! This module implements the subset of `PLAYLIST/*.mpls` needed by DiscCraft:
//! sequential playlists with one clip per play item, no sub-paths, and entry
//! marks.

use crate::{BdmvError, Reader, Stream, StreamCoding, VERSION, len_u32, patch_u32};

/// One play item: a span of one clip.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayItem {
    /// Clip_Information_file_name, e.g. `*b"00001"`.
    pub clip: [u8; 5],
    /// Start time in 45 kHz units.
    pub in_time: u32,
    /// End time in 45 kHz units.
    pub out_time: u32,
    /// Streams selectable while the play item plays.
    pub streams: StnTable,
    /// Still at the end of the item: 0 none, 1 for `still_time` seconds, 2 until the user acts (menus).
    pub still_mode: u8,
    /// Still time in seconds when `still_mode` is 1.
    pub still_time: u16,
}

/// Streams selectable while the play item plays (STN table).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StnTable {
    /// Primary video streams.
    pub video: Vec<Stream>,
    /// Primary audio streams.
    pub audio: Vec<Stream>,
    /// Primary presentation graphics streams.
    pub pg: Vec<Stream>,
    /// Primary interactive graphics streams.
    pub ig: Vec<Stream>,
}

/// Entry mark (chapter).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayListMark {
    /// Zero-based index of the referenced play item.
    pub play_item: u16,
    /// Mark time stamp in 45 kHz units.
    pub time: u32,
}

/// A whole playlist.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayList {
    /// Play items in playback order.
    pub items: Vec<PlayItem>,
    /// Entry marks.
    pub marks: Vec<PlayListMark>,
}

/// Write a playlist file.
pub fn write_mpls(playlist: &PlayList) -> Result<Vec<u8>, BdmvError> {
    let mut buf = Vec::new();
    write_header(&mut buf)?;

    write_app_info(&mut buf)?;

    let playlist_addr = len_u32(buf.len())?;
    patch_u32(&mut buf, 8, playlist_addr)?;

    write_play_list(&mut buf, &playlist.items)?;

    let marks_addr = len_u32(buf.len())?;
    patch_u32(&mut buf, 12, marks_addr)?;

    write_marks(&mut buf, &playlist.marks)?;
    Ok(buf)
}

/// Read a playlist file written by [`write_mpls`].
pub fn read_mpls(data: &[u8]) -> Result<PlayList, BdmvError> {
    let mut r = Reader::at(data, 0);
    r.skip(4)?;
    r.skip(4)?;

    let playlist_addr = r.u32()?;
    let marks_addr = r.u32()?;
    let _ext_addr = r.u32()?;
    r.skip(20)?;

    let app_len = usize_from_u32(r.u32()?)?;
    r.skip(app_len)?;

    let items = read_playlist(data, playlist_addr)?;
    let marks = read_marks(data, marks_addr)?;
    Ok(PlayList { items, marks })
}

fn write_header(buf: &mut Vec<u8>) -> Result<(), BdmvError> {
    buf.extend_from_slice(b"MPLS");
    buf.extend_from_slice(VERSION);
    buf.extend_from_slice(&[0; 4]);
    buf.extend_from_slice(&[0; 4]);
    buf.extend_from_slice(&[0; 4]);
    buf.extend_from_slice(&[0; 20]);
    Ok(())
}

fn write_app_info(buf: &mut Vec<u8>) -> Result<(), BdmvError> {
    let mut body = Vec::new();
    body.push(0);
    body.push(1);
    body.extend_from_slice(&[0; 2]);
    body.extend_from_slice(&[0; 8]);
    body.extend_from_slice(&[0; 2]);

    let len = len_u32(body.len())?;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&body);
    Ok(())
}

fn write_play_list(buf: &mut Vec<u8>, items: &[PlayItem]) -> Result<(), BdmvError> {
    let mut body = Vec::new();
    body.extend_from_slice(&[0; 2]);

    let n_items =
        u16::try_from(items.len()).map_err(|_| BdmvError("too many play items".to_string()))?;
    body.extend_from_slice(&n_items.to_be_bytes());
    body.extend_from_slice(&0u16.to_be_bytes());

    for item in items {
        write_play_item(&mut body, item)?;
    }

    let len = len_u32(body.len())?;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&body);
    Ok(())
}

fn write_play_item(buf: &mut Vec<u8>, item: &PlayItem) -> Result<(), BdmvError> {
    let mut body = Vec::new();
    body.extend_from_slice(&item.clip);
    body.extend_from_slice(b"M2TS");

    // 11 reserved bits, 1 IsMultiAngle bit, 4 ConnectionCondition bits.
    body.extend_from_slice(&0x0001u16.to_be_bytes());
    body.push(0);
    body.extend_from_slice(&item.in_time.to_be_bytes());
    body.extend_from_slice(&item.out_time.to_be_bytes());
    // UO mask, then random access flag (0), still mode and still time.
    body.extend_from_slice(&[0; 8]);
    body.push(0);
    body.push(item.still_mode);
    let still_time = if item.still_mode == 1 {
        item.still_time
    } else {
        0
    };
    body.extend_from_slice(&still_time.to_be_bytes());

    write_stn_table(&mut body, &item.streams)?;

    let len =
        u16::try_from(body.len()).map_err(|_| BdmvError("play item too large".to_string()))?;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&body);
    Ok(())
}

fn write_stn_table(buf: &mut Vec<u8>, streams: &StnTable) -> Result<(), BdmvError> {
    let mut body = Vec::new();
    body.extend_from_slice(&[0; 2]);

    body.push(count_u8(streams.video.len())?);
    body.push(count_u8(streams.audio.len())?);
    body.push(count_u8(streams.pg.len())?);
    body.push(count_u8(streams.ig.len())?);
    body.extend_from_slice(&[0; 4]);
    body.extend_from_slice(&[0; 4]);

    for stream in &streams.video {
        write_stream(&mut body, stream)?;
    }
    for stream in &streams.audio {
        write_stream(&mut body, stream)?;
    }
    for stream in &streams.pg {
        write_stream(&mut body, stream)?;
    }
    for stream in &streams.ig {
        write_stream(&mut body, stream)?;
    }

    let len =
        u16::try_from(body.len()).map_err(|_| BdmvError("STN table too large".to_string()))?;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&body);
    Ok(())
}

fn write_stream(buf: &mut Vec<u8>, stream: &Stream) -> Result<(), BdmvError> {
    buf.push(9);
    buf.push(1);
    buf.extend_from_slice(&stream.pid.to_be_bytes());
    buf.extend_from_slice(&[0; 6]);

    write_stream_attributes(buf, &stream.coding)?;
    Ok(())
}

fn write_stream_attributes(buf: &mut Vec<u8>, coding: &StreamCoding) -> Result<(), BdmvError> {
    buf.push(5);
    buf.push(coding.coding_type());

    match coding {
        StreamCoding::Mpeg2Video {
            video_format,
            frame_rate,
            aspect,
        }
        | StreamCoding::H264Video {
            video_format,
            frame_rate,
            aspect,
        } => {
            buf.push(pack_nibble_pair(*video_format, *frame_rate)?);
            // Aspect is not part of the BD-ROM StreamAttributes video block.
            // Store it in the first reserved byte so StreamCoding round-trips.
            buf.push(pack_high_nibble(*aspect)?);
            buf.extend_from_slice(&[0; 2]);
        }
        StreamCoding::Lpcm {
            audio_format,
            sample_rate,
            language,
        }
        | StreamCoding::Ac3 {
            audio_format,
            sample_rate,
            language,
        } => {
            buf.push(pack_nibble_pair(*audio_format, *sample_rate)?);
            buf.extend_from_slice(language);
        }
        StreamCoding::Pgs { language } | StreamCoding::Igs { language } => {
            buf.extend_from_slice(language);
            buf.push(0);
        }
    }

    Ok(())
}

fn write_marks(buf: &mut Vec<u8>, marks: &[PlayListMark]) -> Result<(), BdmvError> {
    let mut body = Vec::new();
    let n_marks =
        u16::try_from(marks.len()).map_err(|_| BdmvError("too many marks".to_string()))?;
    body.extend_from_slice(&n_marks.to_be_bytes());

    for mark in marks {
        body.push(0);
        body.push(1);
        body.extend_from_slice(&mark.play_item.to_be_bytes());
        body.extend_from_slice(&mark.time.to_be_bytes());
        body.extend_from_slice(&0xFFFFu16.to_be_bytes());
        body.extend_from_slice(&0u32.to_be_bytes());
    }

    let len = len_u32(body.len())?;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&body);
    Ok(())
}

fn read_playlist(data: &[u8], addr: u32) -> Result<Vec<PlayItem>, BdmvError> {
    let mut r = Reader::at(data, usize_from_u32(addr)?);
    let _len = r.u32()?;
    r.skip(2)?;

    let n_items = r.u16()?;
    let n_subpaths = r.u16()?;
    if n_subpaths != 0 {
        return Err(BdmvError("sub-paths are not supported".to_string()));
    }

    let mut items = Vec::new();
    for _ in 0..u32::from(n_items) {
        items.push(read_play_item(&mut r)?);
    }
    Ok(items)
}

fn read_play_item(r: &mut Reader<'_>) -> Result<PlayItem, BdmvError> {
    let _len = r.u16()?;
    let clip = read_clip_name(r)?;
    r.skip(4)?;

    let flags = r.u16()?;
    if (flags >> 4) & 1 != 0 {
        return Err(BdmvError(
            "multi-angle play items are not supported".to_string(),
        ));
    }

    r.skip(1)?;
    let in_time = r.u32()?;
    let out_time = r.u32()?;
    r.skip(8)?;
    r.skip(1)?;
    let still_mode = r.u8()?;
    let still_time = r.u16()?;

    let streams = read_stn_table(r)?;
    Ok(PlayItem {
        clip,
        in_time,
        out_time,
        streams,
        still_mode,
        still_time,
    })
}

fn read_stn_table(r: &mut Reader<'_>) -> Result<StnTable, BdmvError> {
    let _len = r.u16()?;
    r.skip(2)?;

    let n_video = r.u8()?;
    let n_audio = r.u8()?;
    let n_pg = r.u8()?;
    let n_ig = r.u8()?;
    let n_sec_audio = r.u8()?;
    let n_sec_video = r.u8()?;
    let n_pip_pg = r.u8()?;
    let n_dv = r.u8()?;

    if n_sec_audio != 0 || n_sec_video != 0 || n_pip_pg != 0 || n_dv != 0 {
        return Err(BdmvError("secondary streams are not supported".to_string()));
    }

    r.skip(4)?;

    let mut video = Vec::new();
    let mut audio = Vec::new();
    let mut pg = Vec::new();
    let mut ig = Vec::new();

    for _ in 0..u32::from(n_video) {
        video.push(read_stream(r)?);
    }
    for _ in 0..u32::from(n_audio) {
        audio.push(read_stream(r)?);
    }
    for _ in 0..u32::from(n_pg) {
        pg.push(read_stream(r)?);
    }
    for _ in 0..u32::from(n_ig) {
        ig.push(read_stream(r)?);
    }

    Ok(StnTable {
        video,
        audio,
        pg,
        ig,
    })
}

fn read_stream(r: &mut Reader<'_>) -> Result<Stream, BdmvError> {
    let entry_len = r.u8()?;
    if entry_len != 9 {
        return Err(BdmvError("unexpected StreamEntry length".to_string()));
    }

    let stream_type = r.u8()?;
    if stream_type != 1 {
        return Err(BdmvError("unsupported StreamEntry type".to_string()));
    }

    let pid = r.u16()?;
    r.skip(6)?;

    let attr_len = r.u8()?;
    if attr_len != 5 {
        return Err(BdmvError("unexpected StreamAttributes length".to_string()));
    }

    let coding_type = r.u8()?;
    let coding = read_stream_attributes(r, coding_type)?;
    Ok(Stream { pid, coding })
}

fn read_stream_attributes(r: &mut Reader<'_>, coding_type: u8) -> Result<StreamCoding, BdmvError> {
    match coding_type {
        0x02 => {
            let b0 = r.u8()?;
            let b1 = r.u8()?;
            r.skip(2)?;
            Ok(StreamCoding::Mpeg2Video {
                video_format: b0 >> 4,
                frame_rate: b0 & 0x0F,
                aspect: b1 >> 4,
            })
        }
        0x1B => {
            let b0 = r.u8()?;
            let b1 = r.u8()?;
            r.skip(2)?;
            Ok(StreamCoding::H264Video {
                video_format: b0 >> 4,
                frame_rate: b0 & 0x0F,
                aspect: b1 >> 4,
            })
        }
        0x80 => {
            let b = r.u8()?;
            let language = read_lang(r)?;
            Ok(StreamCoding::Lpcm {
                audio_format: b >> 4,
                sample_rate: b & 0x0F,
                language,
            })
        }
        0x81 => {
            let b = r.u8()?;
            let language = read_lang(r)?;
            Ok(StreamCoding::Ac3 {
                audio_format: b >> 4,
                sample_rate: b & 0x0F,
                language,
            })
        }
        0x90 => {
            let language = read_lang(r)?;
            r.skip(1)?;
            Ok(StreamCoding::Pgs { language })
        }
        0x91 => {
            let language = read_lang(r)?;
            r.skip(1)?;
            Ok(StreamCoding::Igs { language })
        }
        _ => Err(BdmvError("unsupported stream coding type".to_string())),
    }
}

fn read_marks(data: &[u8], addr: u32) -> Result<Vec<PlayListMark>, BdmvError> {
    let mut r = Reader::at(data, usize_from_u32(addr)?);
    let _len = r.u32()?;
    let n_marks = r.u16()?;

    let mut marks = Vec::new();
    for _ in 0..u32::from(n_marks) {
        r.skip(1)?;
        let mark_type = r.u8()?;
        if mark_type != 1 {
            return Err(BdmvError("unsupported mark type".to_string()));
        }

        let play_item = r.u16()?;
        let time = r.u32()?;
        r.skip(6)?;
        marks.push(PlayListMark { play_item, time });
    }

    Ok(marks)
}

fn read_clip_name(r: &mut Reader<'_>) -> Result<[u8; 5], BdmvError> {
    r.bytes(5)?
        .try_into()
        .map_err(|_| BdmvError("clip name truncated".to_string()))
}

fn read_lang(r: &mut Reader<'_>) -> Result<[u8; 3], BdmvError> {
    r.bytes(3)?
        .try_into()
        .map_err(|_| BdmvError("language code truncated".to_string()))
}

fn count_u8(len: usize) -> Result<u8, BdmvError> {
    u8::try_from(len).map_err(|_| BdmvError("stream count too large".to_string()))
}

fn pack_nibble_pair(high: u8, low: u8) -> Result<u8, BdmvError> {
    let value = (u32::from(high) << 4) | u32::from(low);
    u8::try_from(value).map_err(|_| BdmvError("4-bit field overflow".to_string()))
}

fn pack_high_nibble(value: u8) -> Result<u8, BdmvError> {
    pack_nibble_pair(value, 0)
}

fn usize_from_u32(value: u32) -> Result<usize, BdmvError> {
    usize::try_from(value).map_err(|_| BdmvError("address out of range".to_string()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::{Stream, StreamCoding};

    fn sample_playlist() -> PlayList {
        let streams = StnTable {
            video: vec![Stream {
                pid: 0x1011,
                coding: StreamCoding::Mpeg2Video {
                    video_format: 1,
                    frame_rate: 4,
                    aspect: 2,
                },
            }],
            audio: vec![Stream {
                pid: 0x1100,
                coding: StreamCoding::Lpcm {
                    audio_format: 3,
                    sample_rate: 1,
                    language: *b"jpn",
                },
            }],
            pg: vec![Stream {
                pid: 0x1200,
                coding: StreamCoding::Pgs { language: *b"eng" },
            }],
            ig: vec![],
        };

        PlayList {
            items: vec![PlayItem {
                clip: *b"00001",
                in_time: 0,
                out_time: 45_000,
                streams,
                still_mode: 0,
                still_time: 0,
            }],
            marks: vec![
                PlayListMark {
                    play_item: 0,
                    time: 0,
                },
                PlayListMark {
                    play_item: 0,
                    time: 15_000,
                },
                PlayListMark {
                    play_item: 0,
                    time: 30_000,
                },
            ],
        }
    }

    #[test]
    fn round_trip() {
        let playlist = sample_playlist();
        let bytes = write_mpls(&playlist).unwrap();
        assert_eq!(read_mpls(&bytes).unwrap(), playlist);
    }

    #[test]
    fn header_and_playlist_length() {
        let bytes = write_mpls(&sample_playlist()).unwrap();
        assert_eq!(&bytes[0..8], b"MPLS0200");

        let playlist_addr = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        let marks_addr = u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);

        let a = usize::try_from(playlist_addr).unwrap();
        let len = u32::from_be_bytes([bytes[a], bytes[a + 1], bytes[a + 2], bytes[a + 3]]);
        let expected = marks_addr
            .checked_sub(playlist_addr)
            .and_then(|v| v.checked_sub(4))
            .unwrap();

        assert_eq!(len, expected);
    }

    #[test]
    fn truncated_input_is_error() {
        let bytes = write_mpls(&sample_playlist()).unwrap();
        assert!(read_mpls(&bytes[..bytes.len() - 1]).is_err());
    }
}
