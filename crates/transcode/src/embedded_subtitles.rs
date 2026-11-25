//! Subtitles inside a video file (M6.422): a Matroska file's subtitle tracks, taken out as subtitle files DiscCraft
//! reads — text (SubRip `S_TEXT/UTF8`, WebVTT, ASS / SSA) as `.srt`, a Blu-ray's PGS (`S_HDMV/PGS`) as `.sup` — so
//! they become subtitle assets like any other (edited, checked and built the same way).
//!
//! From the Matroska specifications (the codec mappings: a SubRip block holds the text of one subtitle; an ASS / SSA
//! block holds `ReadOrder, Layer, Style, Name, MarginL, MarginR, MarginV, Effect, Text`; a PGS block holds the
//! presentation graphics segments of one display set, which a `.sup` stores each behind `PG`, its presentation and
//! decoding times at 90 kHz). Frames stored with header stripping get their bytes back, and frames stored compressed
//! with zlib (as some muxers store subtitle tracks) are inflated; tracks stored otherwise are not offered.

use std::fmt::Write as _;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use craft_media::ReadSeekSource;
use craft_media::matroska::{ContentEncoding, Demuxer, OpenOptions, Track, TrackKind};

use crate::TranscodeError;

/// The most subtitles taken from one track.
const MAX_CUES: usize = 100_000;
/// The most bytes of one stored subtitle frame, and of one inflated.
const MAX_FRAME: usize = 4 << 20;
/// The most bytes of a whole extracted track.
const MAX_TRACK: usize = 512 << 20;
/// How long a text subtitle with no duration of its own is shown, at most (until the next one otherwise), seconds.
const DEFAULT_SECONDS: f64 = 5.0;

/// What a subtitle track inside a video file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddedFormat {
    /// Text: taken out as a `.srt`.
    Text,
    /// A Blu-ray's presentation graphics: taken out as a `.sup`.
    Pgs,
}

/// A subtitle track inside a video file that DiscCraft can take out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedSubtitle {
    /// Its place among the file's subtitle tracks that can be taken out (from 0).
    pub stream: usize,
    pub format: EmbeddedFormat,
    /// The language the file names for it (ISO 639-2 or BCP 47, as written); `None` for none, or `und`.
    pub language: Option<String>,
    /// The track's name, as the file has it.
    pub name: String,
    /// The file marks it to be shown even with subtitles off (signs and foreign dialogue).
    pub forced: bool,
}

/// The subtitle tracks of the video file `path` that can be taken out, in file order; empty for a file that is not
/// Matroska or has none. Only the file's headers are read.
#[must_use]
pub fn embedded_subtitles(path: &Path) -> Vec<EmbeddedSubtitle> {
    let Some(demuxer) = open(path, false) else {
        return Vec::new();
    };
    usable_tracks(demuxer.tracks())
        .enumerate()
        .map(|(stream, (_, track, format))| EmbeddedSubtitle {
            stream,
            format,
            language: language(track),
            name: track.name.trim().to_owned(),
            forced: track.forced,
        })
        .collect()
}

/// Takes subtitle track `stream` ([`embedded_subtitles`]) of the video file `path` out into the folder `folder`, as
/// `<file>.<stream + 1>.<language>.srt` or `.sup`, and gives the file's path. The whole file is read (its frames'
/// places).
///
/// # Errors
///
/// When the file has no such track, cannot be read, or the track holds nothing.
pub fn extract_embedded_subtitle(
    path: &Path,
    stream: usize,
    folder: &Path,
) -> Result<PathBuf, TranscodeError> {
    let fail = |message: String| TranscodeError::Decode(format!("{}: {message}", path.display()));
    let demuxer = open(path, true).ok_or_else(|| fail("not a Matroska file".to_owned()))?;
    let (_, track, format) = usable_tracks(demuxer.tracks())
        .nth(stream)
        .ok_or_else(|| fail(format!("no subtitle stream {}", stream + 1)))?;
    let mut file = std::fs::File::open(path).map_err(|error| fail(error.to_string()))?;
    let (num, den) = track.timebase;
    let seconds = |ticks: i64| -> f64 {
        if den == 0 {
            0.0
        } else {
            // `as` is exact enough for subtitle times.
            ticks as f64 * num as f64 / den as f64
        }
    };
    // Each frame's start and duration (seconds) and its bytes, as stored before compression.
    let mut frames: Vec<(f64, f64, Vec<u8>)> = Vec::new();
    let mut total = 0usize;
    for sample in track.samples.iter().take(MAX_CUES) {
        let size = usize::try_from(sample.size).unwrap_or(usize::MAX);
        if size > MAX_FRAME {
            continue;
        }
        let mut stored = vec![0u8; size];
        file.seek(SeekFrom::Start(sample.offset))
            .and_then(|_| file.read_exact(&mut stored))
            .map_err(|error| fail(error.to_string()))?;
        let Some(bytes) = decoded(&track.encodings, stored) else {
            continue;
        };
        total = total.saturating_add(bytes.len());
        if total > MAX_TRACK {
            return Err(fail("the subtitle track is too large".to_owned()));
        }
        let duration = i64::try_from(sample.duration).unwrap_or(i64::MAX);
        frames.push((
            seconds(sample.pts).max(0.0),
            seconds(duration).max(0.0),
            bytes,
        ));
    }
    frames.sort_by(|a, b| a.0.total_cmp(&b.0));
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("video");
    let language = language(track).unwrap_or_else(|| "und".to_owned());
    let language: String = language
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(16)
        .collect();
    let (content, extension) = match format {
        EmbeddedFormat::Text => (srt(&frames, &track.codec_id).into_bytes(), "srt"),
        EmbeddedFormat::Pgs => (sup(&frames), "sup"),
    };
    if content.is_empty() {
        return Err(fail(format!(
            "subtitle stream {} holds nothing",
            stream + 1
        )));
    }
    std::fs::create_dir_all(folder).map_err(|error| fail(error.to_string()))?;
    let out = folder.join(format!("{stem}.{}.{language}.{extension}", stream + 1));
    std::fs::write(&out, content)
        .map_err(|error| TranscodeError::Decode(format!("{}: {error}", out.display())))?;
    Ok(out)
}

fn open(path: &Path, index: bool) -> Option<Demuxer<ReadSeekSource<std::fs::File>>> {
    let mut head = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut head))
        .ok()?;
    if head != [0x1A, 0x45, 0xDF, 0xA3] {
        return None;
    }
    let source = ReadSeekSource::new(std::fs::File::open(path).ok()?).ok()?;
    let options = OpenOptions {
        index,
        verify_crc: false,
        index_limit: None,
    };
    Demuxer::with_options(source, options).ok()
}

/// The subtitle tracks that can be taken out, with their index among all tracks and their format.
fn usable_tracks(tracks: &[Track]) -> impl Iterator<Item = (usize, &Track, EmbeddedFormat)> {
    tracks.iter().enumerate().filter_map(|(index, track)| {
        if track.kind != TrackKind::Subtitle || !readable(track) {
            return None;
        }
        let format = match track.codec_id.as_str() {
            "S_TEXT/UTF8" | "S_TEXT/ASCII" | "S_TEXT/WEBVTT" | "S_TEXT/ASS" | "S_TEXT/SSA"
            | "S_ASS" | "S_SSA" => EmbeddedFormat::Text,
            id if id.starts_with("D_WEBVTT/") => EmbeddedFormat::Text,
            "S_HDMV/PGS" => EmbeddedFormat::Pgs,
            _ => return None,
        };
        Some((index, track, format))
    })
}

/// Whether the track's frames can be read: stored as they are, with header stripping, or zlib-compressed.
fn readable(track: &Track) -> bool {
    track.encodings.iter().all(|encoding| match encoding {
        ContentEncoding::HeaderStripping { .. } => true,
        ContentEncoding::Compression { algo, .. } => *algo == 0,
        ContentEncoding::Encryption { .. } => false,
    })
}

/// A frame's bytes as they were before the track's encodings (applied in the file's order, undone the other way);
/// `None` when they cannot be undone.
fn decoded(encodings: &[ContentEncoding], mut bytes: Vec<u8>) -> Option<Vec<u8>> {
    // Scope bit 1: the frames (2 is the codec's private data, 4 the next encoding's settings).
    for encoding in encodings.iter().rev() {
        match encoding {
            ContentEncoding::HeaderStripping {
                bytes: stripped,
                scope,
            } => {
                if scope & 1 != 0 {
                    let mut whole = stripped.clone();
                    whole.extend_from_slice(&bytes);
                    bytes = whole;
                }
            }
            ContentEncoding::Compression { algo, scope, .. } => {
                if scope & 1 != 0 {
                    if *algo != 0 {
                        return None;
                    }
                    bytes =
                        miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&bytes, MAX_FRAME)
                            .ok()?;
                }
            }
            ContentEncoding::Encryption { .. } => return None,
        }
    }
    Some(bytes)
}

/// The language the track names, as written; `None` for none or `und`. Matroska's `Language` is `eng` when the file
/// does not say, so English counts only from `LanguageBCP47`, or when the track says so and another track of the
/// file is not English (left to the caller: here `eng` is kept).
fn language(track: &Track) -> Option<String> {
    let code = track
        .language_bcp47
        .clone()
        .filter(|tag| !tag.trim().is_empty())
        .unwrap_or_else(|| track.language.clone());
    let code = code.trim().to_owned();
    (!code.is_empty() && !code.eq_ignore_ascii_case("und")).then_some(code)
}

/// The text of one ASS / SSA event as Matroska stores it (`ReadOrder, Layer, Style, Name, MarginL, MarginR, MarginV,
/// Effect, Text`): its text without the override blocks (`{…}`) and without what a `\p1` (or higher) block turns into
/// a drawing until `\p0`, `\N` and `\n` as line breaks, `\h` as a space.
fn ass_text(event: &str) -> String {
    let text = event.splitn(9, ',').nth(8).unwrap_or_default();
    let mut plain = String::with_capacity(text.len());
    let mut block: Option<String> = None;
    let mut drawing = false;
    for c in text.chars() {
        match (&mut block, c) {
            (None, '{') => block = Some(String::new()),
            (Some(tags), '}') => {
                // The last `\p<n>` of the block sets the drawing mode.
                if let Some(level) = tags
                    .rsplit("\\p")
                    .next()
                    .filter(|_| tags.contains("\\p"))
                    .map(|rest| {
                        rest.chars()
                            .take_while(char::is_ascii_digit)
                            .collect::<String>()
                    })
                    .filter(|digits| !digits.is_empty())
                {
                    drawing = !level.trim_start_matches('0').is_empty();
                }
                block = None;
            }
            (Some(tags), _) => tags.push(c),
            (None, _) if !drawing => plain.push(c),
            (None, _) => {}
        }
    }
    plain
        .replace("\\N", "\n")
        .replace("\\n", "\n")
        .replace("\\h", " ")
}

/// Text frames as a SubRip file: each frame a subtitle from its start for its duration (else until the next one,
/// at most [`DEFAULT_SECONDS`]); empty ones left out.
fn srt(frames: &[(f64, f64, Vec<u8>)], codec: &str) -> String {
    let ass = matches!(codec, "S_TEXT/ASS" | "S_TEXT/SSA" | "S_ASS" | "S_SSA");
    let mut out = String::new();
    let mut number = 0usize;
    for (at, (start, duration, bytes)) in frames.iter().enumerate() {
        let raw = String::from_utf8_lossy(bytes);
        let text = if ass {
            ass_text(&raw)
        } else {
            raw.into_owned()
        };
        let lines: Vec<&str> = text
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.trim().is_empty())
            .collect();
        if lines.is_empty() {
            continue;
        }
        let next = frames.get(at + 1).map(|frame| frame.0);
        let end = if *duration > 0.0 {
            start + duration
        } else {
            next.filter(|next| *next > *start)
                .map_or(start + DEFAULT_SECONDS, |next| {
                    next.min(start + DEFAULT_SECONDS)
                })
        };
        number += 1;
        let _ = write!(
            out,
            "{number}\r\n{} --> {}\r\n{}\r\n\r\n",
            timecode(*start),
            timecode(end),
            lines.join("\r\n")
        );
    }
    out
}

/// A SubRip time: `HH:MM:SS,mmm`.
fn timecode(seconds: f64) -> String {
    // `as` saturates; times are short of a day.
    let millis = (seconds.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        millis / 3_600_000,
        millis / 60_000 % 60,
        millis / 1000 % 60,
        millis % 1000
    )
}

/// PGS frames (each a display set's segments: type, length, data) as a `.sup`: each segment behind `PG` and its
/// frame's time at 90 kHz, as presentation and decoding time. A frame whose segments do not add up stops at the
/// first that does not.
fn sup(frames: &[(f64, f64, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (start, _, bytes) in frames {
        // `as` saturates; the time wraps at 2^32 ticks as the field does.
        let pts = (start * 90_000.0).round() as i64;
        let _ = push_sup(&mut out, bytes, (pts, pts));
    }
    out
}

/// Appends the presentation graphics segments in `bytes` (each a type byte, a two-byte length and its data) to the
/// `.sup` `out`, each behind `PG` and the times `(pts, dts)` (90 kHz, kept to the field's 32 bits): how many.
pub(crate) fn push_sup(out: &mut Vec<u8>, bytes: &[u8], (pts, dts): (i64, i64)) -> usize {
    // The field holds 32 bits; times wrap as it does.
    let field = |time: i64| (time & 0xFFFF_FFFF) as u32;
    let mut at = 0usize;
    let mut count = 0usize;
    while let (Some(&high), Some(&low)) = (bytes.get(at + 1), bytes.get(at + 2)) {
        let length = usize::from(u16::from_be_bytes([high, low]));
        let Some(segment) = bytes.get(at..at + 3 + length) else {
            break;
        };
        out.extend_from_slice(b"PG");
        out.extend_from_slice(&field(pts).to_be_bytes());
        out.extend_from_slice(&field(dts).to_be_bytes());
        out.extend_from_slice(segment);
        at += 3 + length;
        count += 1;
    }
    count
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::{ContentEncoding, ass_text, decoded, srt, sup, timecode};

    /// M6.422: frames stored zlib-compressed (as mkvmerge stores subtitle tracks) or header-stripped come back as
    /// they were; other compression and encryption are not undone.
    #[test]
    fn stored_frames_come_back_as_they_were() {
        let frame =
            b"PG segments or subtitle text, long enough to compress: aaaaaaaaaaaaaaaaaaaa".to_vec();
        let zlib = ContentEncoding::Compression {
            algo: 0,
            settings: Vec::new(),
            scope: 1,
        };
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&frame, 6);
        assert_eq!(
            decoded(std::slice::from_ref(&zlib), compressed),
            Some(frame.clone())
        );
        let stripped = ContentEncoding::HeaderStripping {
            bytes: frame[..3].to_vec(),
            scope: 1,
        };
        assert_eq!(
            decoded(std::slice::from_ref(&stripped), frame[3..].to_vec()),
            Some(frame.clone())
        );
        // Only the codec's private data compressed (scope 2): the frames are as stored.
        let private = ContentEncoding::Compression {
            algo: 0,
            settings: Vec::new(),
            scope: 2,
        };
        assert_eq!(decoded(&[private], frame.clone()), Some(frame.clone()));
        let lzo = ContentEncoding::Compression {
            algo: 2,
            settings: Vec::new(),
            scope: 1,
        };
        assert_eq!(decoded(&[lzo], frame.clone()), None);
        assert_eq!(
            decoded(&[ContentEncoding::Encryption { scope: 1 }], frame.clone()),
            None
        );
        // Not zlib at all: refused, not a panic.
        assert_eq!(decoded(&[zlib], frame), None);
    }

    #[test]
    fn ass_events_become_plain_text() {
        assert_eq!(
            ass_text("0,0,Default,,0,0,0,,{\\i1}Hello{\\i0},\\Nworld\\hagain"),
            "Hello,\nworld again"
        );
        assert_eq!(ass_text("1,0,Sign,,0,0,0,,{\\p1}m 0 0 l 10 10{\\p0}"), "");
        assert_eq!(
            ass_text("2,0,Sign,,0,0,0,,{\\pos(10,20)\\p2}m 0 0{\\p0}Text"),
            "Text"
        );
        assert_eq!(ass_text("no fields"), "");
    }

    #[test]
    fn text_frames_become_subrip_with_their_times() {
        let frames = vec![
            (1.0, 2.5, b"First\r\nline two".to_vec()),
            (4.0, 0.0, b"No duration".to_vec()),
            (6.0, 0.0, b"  ".to_vec()),
            (7.0, 0.0, b"Last".to_vec()),
        ];
        assert_eq!(
            srt(&frames, "S_TEXT/UTF8"),
            "1\r\n00:00:01,000 --> 00:00:03,500\r\nFirst\r\nline two\r\n\r\n\
             2\r\n00:00:04,000 --> 00:00:06,000\r\nNo duration\r\n\r\n\
             3\r\n00:00:07,000 --> 00:00:12,000\r\nLast\r\n\r\n"
        );
        assert_eq!(timecode(3_725.004_9), "01:02:05,005");
    }

    #[test]
    fn pgs_frames_become_a_sup_with_each_segment_timed() {
        // A display set of two segments at 2 s, and a truncated one that stops.
        let frames = vec![
            (2.0, 0.0, vec![0x16, 0, 2, 9, 9, 0x80, 0, 0]),
            (3.0, 0.0, vec![0x16, 0, 5, 1]),
        ];
        let out = sup(&frames);
        let pts = 180_000u32.to_be_bytes();
        let mut expected = b"PG".to_vec();
        expected.extend_from_slice(&pts);
        expected.extend_from_slice(&pts);
        expected.extend_from_slice(&[0x16, 0, 2, 9, 9]);
        expected.extend_from_slice(b"PG");
        expected.extend_from_slice(&pts);
        expected.extend_from_slice(&pts);
        expected.extend_from_slice(&[0x80, 0, 0]);
        assert_eq!(out, expected);
    }
}
