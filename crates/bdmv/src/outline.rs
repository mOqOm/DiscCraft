//! The outline of any disc's playlist (M6.427 の別件、`disccraft-cli bd-chapters`): its play items' clips and
//! times, and its marks, the chapters among them.
//!
//! [`crate::mpls::read_mpls`] reads back what DiscCraft writes (one clip per play item, no sub-paths, entry marks
//! only). Discs made by other authoring tools have more: sub-paths, multi-angle play items, link points, newer
//! versions. This reader takes only the fields a chapter list needs and steps over the rest by the lengths the
//! file gives (the PlayList, PlayItem and PlayListMark tables of the public BluRay wiki, docs/formats/bdmv.md).

use crate::{BdmvError, Reader};

/// The most play items and marks read from one playlist (a disc allows 999 of each).
const MAX_ENTRIES: u16 = 999;

/// One play item: a stretch of one clip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineItem {
    /// The clip's file name without its extension, e.g. `00001`.
    pub clip: String,
    /// Start in the clip, 45 kHz.
    pub in_time: u32,
    /// End in the clip, 45 kHz.
    pub out_time: u32,
    /// The play item has several angles.
    pub multi_angle: bool,
    /// Its first primary video stream's coding type and its video format and frame rate byte (M6.429).
    pub video: Option<(u8, u8)>,
    /// Its primary audio streams, in their order (M6.429).
    pub audio: Vec<OutlineStream>,
    /// Its presentation graphics and text subtitle streams, in their order (M6.429).
    pub subtitles: Vec<OutlineStream>,
}

/// One stream a play item lists (M6.429).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutlineStream {
    /// The stream coding type: audio 0x80 LPCM, 0x81 AC-3, 0x82 DTS, 0x83 Dolby TrueHD, 0x84 E-AC-3, 0x85 / 0x86
    /// DTS-HD; subtitles 0x90 PG, 0x92 text.
    pub coding: u8,
    /// The language, ISO 639-2 as the file gives it (`und` when it gives none).
    pub language: [u8; 3],
    /// Its PID in the play item's clip (M6.491), for a stream of the main clip (stream entry type 1); `None` for one
    /// in a sub-path.
    pub pid: Option<u16>,
}

impl OutlineStream {
    /// The language as text (`und` when it is not text).
    #[must_use]
    pub fn language_code(&self) -> String {
        std::str::from_utf8(&self.language).map_or_else(|_| "und".to_owned(), str::to_owned)
    }
}

/// The picture size and frame rate (frames per second) of a video format and frame rate byte of a stream's
/// attributes (M6.429): format 1 480i, 2 576i, 3 480p, 4 1080i, 5 720p, 6 1080p, 7 576p; rate 1 23.976, 2 24, 3 25,
/// 4 29.97, 6 50, 7 59.94.
#[must_use]
pub fn video_format(byte: u8) -> Option<((u32, u32), f64)> {
    let size = match byte >> 4 {
        1 | 3 => (720, 480),
        2 | 7 => (720, 576),
        4 | 6 => (1920, 1080),
        5 => (1280, 720),
        _ => return None,
    };
    let rate = match byte & 0x0F {
        1 => 24_000.0 / 1_001.0,
        2 => 24.0,
        3 => 25.0,
        4 => 30_000.0 / 1_001.0,
        6 => 50.0,
        7 => 60_000.0 / 1_001.0,
        _ => return None,
    };
    Some((size, rate))
}

impl OutlineItem {
    /// Its length, 45 kHz (none when the end is not after the start).
    #[must_use]
    pub fn length(&self) -> u32 {
        self.out_time.saturating_sub(self.in_time)
    }
}

/// One mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutlineMark {
    /// 1 an entry mark (a chapter), 2 a link point (a place for navigation commands, not a chapter).
    pub kind: u8,
    /// The play item it is in, from 0.
    pub play_item: u16,
    /// Where, as a time of that play item's clip, 45 kHz.
    pub time: u32,
}

/// A playlist's play items and marks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outline {
    /// The `MPLS` version, e.g. `0200`.
    pub version: String,
    /// Play items, in order.
    pub items: Vec<OutlineItem>,
    /// Marks, in the file's order.
    pub marks: Vec<OutlineMark>,
    /// Sub-paths: each its type and the clip of its first sub-play item (M6.430).
    pub sub_paths: Vec<OutlineSubPath>,
}

/// A playlist's sub-path (M6.430).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineSubPath {
    /// The sub-path type: 3 interactive graphics (a menu out of the main clip), 4 text subtitles, 5 to 7
    /// picture-in-picture and secondary audio.
    pub kind: u8,
    /// The clip of its first sub-play item, e.g. `00013`.
    pub clip: String,
}

/// A chapter of a playlist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutlineChapter {
    /// Where it starts in the playlist (the play items one after the other), 45 kHz.
    pub start: u64,
    /// How long until the next chapter or the playlist's end, 45 kHz.
    pub length: u64,
}

impl Outline {
    /// The playlist's length: its play items one after the other, 45 kHz.
    #[must_use]
    pub fn length(&self) -> u64 {
        self.items.iter().map(|item| u64::from(item.length())).sum()
    }

    /// The chapters: the entry marks, as times of the playlist, in order; marks outside their play item or in
    /// a play item the playlist does not have are left out, and so are marks at the same time.
    #[must_use]
    pub fn chapters(&self) -> Vec<OutlineChapter> {
        let mut offsets = Vec::with_capacity(self.items.len());
        let mut at = 0u64;
        for item in &self.items {
            offsets.push(at);
            at = at.saturating_add(u64::from(item.length()));
        }
        let total = at;
        let mut starts: Vec<u64> = self
            .marks
            .iter()
            .filter(|mark| mark.kind == 1)
            .filter_map(|mark| {
                let index = usize::from(mark.play_item);
                let item = self.items.get(index)?;
                let offset = *offsets.get(index)?;
                (item.in_time..=item.out_time)
                    .contains(&mark.time)
                    .then(|| offset.saturating_add(u64::from(mark.time - item.in_time)))
            })
            .filter(|start| *start < total.max(1))
            .collect();
        starts.sort_unstable();
        starts.dedup();
        starts
            .iter()
            .enumerate()
            .map(|(index, start)| {
                let end = starts.get(index + 1).copied().unwrap_or(total);
                OutlineChapter {
                    start: *start,
                    length: end.saturating_sub(*start),
                }
            })
            .collect()
    }
}

/// Reads the outline of a playlist file (`BDMV/PLAYLIST/*.mpls`) of any disc.
pub fn read_outline(data: &[u8]) -> Result<Outline, BdmvError> {
    let mut r = Reader::at(data, 0);
    if r.bytes(4)? != b"MPLS" {
        return Err(BdmvError("not a playlist file (no MPLS)".to_string()));
    }
    let version = String::from_utf8_lossy(r.bytes(4)?).into_owned();
    let playlist_at = address(r.u32()?)?;
    let marks_at = address(r.u32()?)?;
    let (items, sub_paths) = read_items(data, playlist_at)?;
    Ok(Outline {
        version,
        items,
        marks: read_marks(data, marks_at)?,
        sub_paths,
    })
}

fn address(value: u32) -> Result<usize, BdmvError> {
    usize::try_from(value).map_err(|_| BdmvError("address out of range".to_string()))
}

/// The PlayList: length (4), reserved (2), number of play items (2), number of sub-paths (2), the play items
/// (each its length (2) and that many bytes), then the sub-paths (not needed here).
fn read_items(
    data: &[u8],
    at: usize,
) -> Result<(Vec<OutlineItem>, Vec<OutlineSubPath>), BdmvError> {
    let mut r = Reader::at(data, at);
    let _length = r.u32()?;
    r.skip(2)?;
    let count = r.u16()?;
    let sub_path_count = r.u16()?;
    if count > MAX_ENTRIES {
        return Err(BdmvError(format!("{count} play items")));
    }
    let mut items = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let length = usize::from(r.u16()?);
        let body = r.bytes(length)?;
        // Clip_Information_file_name (5), Clip_codec_identifier (4), reserved (11 bits), is_multi_angle (1
        // bit), connection_condition (4 bits), ref_to_STC_id (1), IN_time (4), OUT_time (4), and the rest.
        let mut item = Reader::at(body, 0);
        let clip = String::from_utf8_lossy(item.bytes(5)?).into_owned();
        item.skip(4)?;
        let flags = item.u16()?;
        item.skip(1)?;
        let in_time = item.u32()?;
        let out_time = item.u32()?;
        let multi_angle = flags & 0x0010 != 0;
        // The streams are not needed for the times: a play item whose table cannot be read has none listed.
        let (video, audio, subtitles) = streams(body, multi_angle).unwrap_or_default();
        items.push(OutlineItem {
            clip,
            in_time,
            out_time,
            multi_angle,
            video,
            audio,
            subtitles,
        });
    }
    // The sub-paths follow; one that cannot be read leaves out those after it, not the play items.
    let mut sub_paths = Vec::new();
    for _ in 0..sub_path_count.min(MAX_ENTRIES) {
        let Some(sub_path) = read_sub_path(&mut r) else {
            break;
        };
        sub_paths.push(sub_path);
    }
    Ok((items, sub_paths))
}

/// One SubPath: length (4), reserved (1), SubPath_type (1), reserved and is_repeat_SubPath (2), reserved (1), number
/// of sub-play items (1), then the sub-play items, each its length (2) and that many bytes, starting with its
/// Clip_Information_file_name (5). The reader goes past the whole sub-path by its length.
fn read_sub_path(r: &mut Reader<'_>) -> Option<OutlineSubPath> {
    let length = usize::try_from(r.u32().ok()?).ok()?;
    let body = r.bytes(length).ok()?;
    let mut sub = Reader::at(body, 0);
    sub.skip(1).ok()?;
    let kind = sub.u8().ok()?;
    sub.skip(3).ok()?;
    let count = sub.u8().ok()?;
    let clip = if count > 0 {
        let _length = sub.u16().ok()?;
        String::from_utf8_lossy(sub.bytes(5).ok()?).into_owned()
    } else {
        String::new()
    };
    Some(OutlineSubPath { kind, clip })
}

/// The most streams of one kind a play item lists.
const MAX_STREAMS: u8 = 32;

/// A play item's first video stream's coding and format, and its primary audio and PG / text subtitle streams, from
/// its STN table. The table
/// follows the play item's fixed fields (32 bytes: up to the still time) and, for a multi-angle item, the number of
/// angles, a flags byte and 10 bytes for each angle after the first. The table: length (2), reserved (2), the
/// numbers of primary video, primary audio, PG and text subtitle, IG, secondary audio, secondary video and PiP PG
/// streams (1 each), reserved (5); then the streams in that order, each a stream entry and its attributes, both
/// a length byte and that many bytes. Video attributes: coding type, format and rate. Audio: coding type, format and
/// rate, language (3). PG (coding type 0x90): language (3); text subtitles (0x92): character code, language (3).
type Streams = (Option<(u8, u8)>, Vec<OutlineStream>, Vec<OutlineStream>);
fn streams(body: &[u8], multi_angle: bool) -> Result<Streams, BdmvError> {
    let mut r = Reader::at(body, 32);
    if multi_angle {
        let angles = r.u8()?;
        r.skip(1)?;
        r.skip(usize::from(angles.saturating_sub(1)) * 10)?;
    }
    let _length = r.u16()?;
    r.skip(2)?;
    let video = r.u8()?;
    let audio = r.u8()?;
    let pg = r.u8()?;
    r.skip(4)?;
    r.skip(5)?;
    if video > MAX_STREAMS || audio > MAX_STREAMS || pg > MAX_STREAMS {
        return Err(BdmvError("too many streams".to_string()));
    }
    // One stream entry and its attributes: the main clip's PID when the entry names one (type 1: the type, then the
    // PID), and the attributes' bytes.
    let stream = |r: &mut Reader<'_>| -> Result<(Option<u16>, Vec<u8>), BdmvError> {
        let entry = usize::from(r.u8()?);
        let bytes = r.bytes(entry)?;
        let pid = match bytes {
            [1, high, low, ..] => Some(u16::from_be_bytes([*high, *low])),
            _ => None,
        };
        let attributes = usize::from(r.u8()?);
        Ok((pid, r.bytes(attributes)?.to_vec()))
    };
    let language = |bytes: Option<&[u8]>| -> [u8; 3] {
        bytes
            .and_then(|bytes| <[u8; 3]>::try_from(bytes).ok())
            .unwrap_or(*b"und")
    };
    let mut first_video = None;
    for _ in 0..video {
        let (_, attributes) = stream(&mut r)?;
        if first_video.is_none()
            && let (Some(coding), Some(format)) = (attributes.first(), attributes.get(1))
        {
            first_video = Some((*coding, *format));
        }
    }
    let mut audio_streams = Vec::with_capacity(usize::from(audio));
    for _ in 0..audio {
        let (pid, attributes) = stream(&mut r)?;
        audio_streams.push(OutlineStream {
            coding: attributes.first().copied().unwrap_or(0),
            language: language(attributes.get(2..5)),
            pid,
        });
    }
    let mut pg_streams = Vec::with_capacity(usize::from(pg));
    for _ in 0..pg {
        let (pid, attributes) = stream(&mut r)?;
        let coding = attributes.first().copied().unwrap_or(0);
        let at = if coding == 0x92 { 2..5 } else { 1..4 };
        pg_streams.push(OutlineStream {
            coding,
            language: language(attributes.get(at)),
            pid,
        });
    }
    Ok((first_video, audio_streams, pg_streams))
}

/// Where a clip's presentation starts and ends (M6.429), 45 kHz: the presentation_start_time and _end_time of the
/// first STC sequence of the first ATC sequence in its clip information file (`BDMV/CLIPINF/*.clpi`). A play item's IN and OUT times are
/// on that time line. The file: `HDMV`, version (4), SequenceInfo start address (4), …; the SequenceInfo: length
/// (4), reserved (1), number of ATC sequences (1), then for each: SPN_ATC_start (4), number of STC sequences (1),
/// offset_STC_id (1), and for each STC sequence: PCR_PID (2), SPN_STC_start (4), presentation_start_time (4),
/// presentation_end_time (4).
pub fn clip_presentation(data: &[u8]) -> Result<(u32, u32), BdmvError> {
    let mut r = Reader::at(data, 0);
    if r.bytes(4)? != b"HDMV" {
        return Err(BdmvError(
            "not a clip information file (no HDMV)".to_string(),
        ));
    }
    r.skip(4)?;
    let at = address(r.u32()?)?;
    let mut r = Reader::at(data, at);
    let _length = r.u32()?;
    r.skip(1)?;
    if r.u8()? == 0 {
        return Err(BdmvError("no ATC sequence".to_string()));
    }
    r.skip(4)?;
    if r.u8()? == 0 {
        return Err(BdmvError("no STC sequence".to_string()));
    }
    r.skip(1)?;
    r.skip(2)?;
    r.skip(4)?;
    let start = r.u32()?;
    let end = r.u32()?;
    Ok((start, end))
}

/// The PlayListMark: length (4), number of marks (2), each 14 bytes: reserved (1), mark_type (1),
/// ref_to_PlayItem_id (2), mark_time_stamp (4), entry_ES_PID (2), duration (4).
fn read_marks(data: &[u8], at: usize) -> Result<Vec<OutlineMark>, BdmvError> {
    let mut r = Reader::at(data, at);
    let _length = r.u32()?;
    let count = r.u16()?;
    if count > MAX_ENTRIES {
        return Err(BdmvError(format!("{count} marks")));
    }
    let mut marks = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        r.skip(1)?;
        let kind = r.u8()?;
        let play_item = r.u16()?;
        let time = r.u32()?;
        r.skip(6)?;
        marks.push(OutlineMark {
            kind,
            play_item,
            time,
        });
    }
    Ok(marks)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{OutlineChapter, read_outline};
    use crate::mpls::{PlayItem, PlayList, PlayListMark, write_mpls};

    /// 45 kHz.
    const S: u32 = 45_000;

    #[test]
    fn a_playlist_disccraft_writes_has_its_chapters_on_the_playlists_time() {
        let item = |clip: &[u8; 5], from: u32, to: u32| PlayItem {
            clip: *clip,
            in_time: from,
            out_time: to,
            ..PlayItem::default()
        };
        let playlist = PlayList {
            items: vec![item(b"00001", 10 * S, 70 * S), item(b"00002", 0, 30 * S)],
            marks: vec![
                PlayListMark {
                    play_item: 0,
                    time: 10 * S,
                },
                PlayListMark {
                    play_item: 0,
                    time: 40 * S,
                },
                PlayListMark {
                    play_item: 1,
                    time: 5 * S,
                },
            ],
            ..PlayList::default()
        };
        let outline = read_outline(&write_mpls(&playlist).unwrap()).unwrap();
        assert_eq!(outline.version, "0200");
        assert_eq!(outline.items.len(), 2);
        assert_eq!(outline.items[1].clip, "00002");
        assert_eq!(outline.length(), u64::from(90 * S));
        let s = u64::from(S);
        assert_eq!(
            outline.chapters(),
            [
                OutlineChapter {
                    start: 0,
                    length: 30 * s
                },
                OutlineChapter {
                    start: 30 * s,
                    length: 35 * s
                },
                OutlineChapter {
                    start: 65 * s,
                    length: 25 * s
                },
            ]
        );
    }

    /// A playlist as other authoring tools make it, built by hand from the field tables: a sub-path, a
    /// multi-angle play item with a second angle, a link point, a mark in a play item there is not, and version
    /// 0300.
    #[test]
    fn sub_paths_angles_and_link_points_are_stepped_over() {
        let mut file = Vec::new();
        file.extend_from_slice(b"MPLS0300");
        file.extend_from_slice(&[0; 12]);
        file.extend_from_slice(&[0; 20]);
        // AppInfoPlayList: length 14 and its body.
        file.extend_from_slice(&14u32.to_be_bytes());
        file.extend_from_slice(&[0; 14]);
        let playlist_at = file.len() as u32;
        let mut body = Vec::new();
        body.extend_from_slice(&[0; 2]);
        body.extend_from_slice(&2u16.to_be_bytes());
        body.extend_from_slice(&1u16.to_be_bytes());
        let play_item = |clip: &[u8; 5], from: u32, to: u32, angles: bool| {
            let mut item = Vec::new();
            item.extend_from_slice(clip);
            item.extend_from_slice(b"M2TS");
            item.extend_from_slice(&(if angles { 0x0011u16 } else { 0x0001 }).to_be_bytes());
            item.push(0);
            item.extend_from_slice(&from.to_be_bytes());
            item.extend_from_slice(&to.to_be_bytes());
            item.extend_from_slice(&[0; 12]);
            if angles {
                // Number of angles, flags, and the second angle's clip, codec and STC id.
                item.extend_from_slice(&[2, 0]);
                item.extend_from_slice(b"00009M2TS");
                item.push(0);
            }
            // An STN table of some length.
            item.extend_from_slice(&16u16.to_be_bytes());
            item.extend_from_slice(&[0xAB; 16]);
            let mut out = (item.len() as u16).to_be_bytes().to_vec();
            out.extend(item);
            out
        };
        body.extend(play_item(b"00005", 0, 600 * S, false));
        body.extend(play_item(b"00006", 100 * S, 400 * S, true));
        // A sub-path (its contents are not read).
        body.extend_from_slice(&8u32.to_be_bytes());
        body.extend_from_slice(&[0xCD; 8]);
        file.extend_from_slice(&(body.len() as u32).to_be_bytes());
        file.extend(body);
        let marks_at = file.len() as u32;
        let marks = [
            (1u8, 0u16, 0u32),
            (2, 0, 30 * S),
            (1, 0, 300 * S),
            (1, 1, 100 * S),
            (1, 1, 250 * S),
            (1, 7, 0),
            (1, 1, 500 * S),
        ];
        let mut body = (marks.len() as u16).to_be_bytes().to_vec();
        for (kind, item, time) in marks {
            body.push(0);
            body.push(kind);
            body.extend_from_slice(&item.to_be_bytes());
            body.extend_from_slice(&time.to_be_bytes());
            body.extend_from_slice(&[0xFF, 0xFF, 0, 0, 0, 0]);
        }
        file.extend_from_slice(&(body.len() as u32).to_be_bytes());
        file.extend(body);
        file[8..12].copy_from_slice(&playlist_at.to_be_bytes());
        file[12..16].copy_from_slice(&marks_at.to_be_bytes());

        let outline = read_outline(&file).unwrap();
        assert_eq!(outline.version, "0300");
        assert_eq!(
            outline
                .items
                .iter()
                .map(|item| (item.clip.as_str(), item.multi_angle))
                .collect::<Vec<_>>(),
            [("00005", false), ("00006", true)]
        );
        assert_eq!(outline.marks.len(), 7);
        // Entry marks only, within their play items: 0, 300 s, 600 s (the second item's start), 750 s.
        let starts: Vec<u64> = outline
            .chapters()
            .iter()
            .map(|chapter| chapter.start / u64::from(S))
            .collect();
        assert_eq!(starts, [0, 300, 600, 750]);
        assert_eq!(outline.length(), u64::from(900 * S));
    }

    /// M6.429: a play item's audio and subtitle languages, and where its clip's presentation starts.
    #[test]
    fn stream_languages_and_the_clips_start_are_read() {
        use crate::clpi::{ClipInfo, write_clpi};
        use crate::mpls::StnTable;
        use crate::{Stream, StreamCoding};
        let audio = |pid: u16, language: &[u8; 3]| Stream {
            pid,
            coding: StreamCoding::Ac3 {
                audio_format: 3,
                sample_rate: 1,
                language: *language,
            },
        };
        let playlist = PlayList {
            items: vec![PlayItem {
                clip: *b"00003",
                in_time: 600 * S,
                out_time: 700 * S,
                streams: StnTable {
                    video: vec![Stream {
                        pid: 0x1011,
                        coding: StreamCoding::H264Video {
                            video_format: 4,
                            frame_rate: 4,
                            aspect: 3,
                        },
                    }],
                    audio: vec![audio(0x1100, b"jpn"), audio(0x1101, b"eng")],
                    pg: vec![Stream {
                        pid: 0x1200,
                        coding: StreamCoding::Pgs { language: *b"eng" },
                    }],
                    ig: Vec::new(),
                },
                ..PlayItem::default()
            }],
            ..PlayList::default()
        };
        let outline = read_outline(&write_mpls(&playlist).unwrap()).unwrap();
        let item = &outline.items[0];
        assert_eq!(
            item.audio
                .iter()
                .map(|stream| (stream.coding, stream.language_code()))
                .collect::<Vec<_>>(),
            [(0x81, "jpn".to_owned()), (0x81, "eng".to_owned())]
        );
        assert_eq!(item.subtitles[0].coding, 0x90);
        assert_eq!(item.subtitles[0].language_code(), "eng");
        // M6.491: each stream's PID in the main clip.
        assert_eq!(
            item.audio
                .iter()
                .map(|stream| stream.pid)
                .collect::<Vec<_>>(),
            [Some(0x1100), Some(0x1101)]
        );
        assert_eq!(item.subtitles[0].pid, Some(0x1200));
        assert_eq!(item.video, Some((0x1B, 0x44)));
        assert_eq!(
            super::video_format(0x44),
            Some(((1920, 1080), 30_000.0 / 1_001.0))
        );
        assert_eq!(super::video_format(0x00), None);

        let clip = write_clpi(&ClipInfo {
            presentation_start: 600 * S,
            presentation_end: 700 * S,
            ..ClipInfo::default()
        })
        .unwrap();
        assert_eq!(super::clip_presentation(&clip).unwrap(), (600 * S, 700 * S));
        assert!(super::clip_presentation(b"HDMV0200").is_err());
        for cut in 0..clip.len() {
            let _ = super::clip_presentation(&clip[..cut]);
        }
    }

    #[test]
    fn broken_files_are_refused_without_panicking() {
        assert!(read_outline(b"").is_err());
        assert!(read_outline(b"XPLS0200").is_err());
        let mut file = b"MPLS0200".to_vec();
        file.extend_from_slice(&u32::MAX.to_be_bytes());
        file.extend_from_slice(&u32::MAX.to_be_bytes());
        assert!(read_outline(&file).is_err());
        // Every cut of a real playlist is refused or read, never a panic.
        let whole = write_mpls(&PlayList {
            items: vec![PlayItem {
                clip: *b"00001",
                out_time: S,
                ..PlayItem::default()
            }],
            marks: vec![PlayListMark::default()],
            ..PlayList::default()
        })
        .unwrap();
        for cut in 0..whole.len() {
            let _ = read_outline(&whole[..cut]);
        }
    }
}
