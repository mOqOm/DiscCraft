//! Subtitles inside a video file (M6.422): a Matroska file's subtitle tracks, taken out as subtitle files DiscCraft
//! reads — text (SubRip `S_TEXT/UTF8`, WebVTT, ASS / SSA) as `.srt`, a Blu-ray's PGS (`S_HDMV/PGS`) as `.sup`, a
//! DVD's subpictures (`S_VOBSUB`, M6.542) as a VobSub `.idx` + `.sub` — and an MP4 / MOV file's text tracks (3GPP
//! timed text `tx3g` and QuickTime text, M6.542) as `.srt`, so they become subtitle assets like any other (edited,
//! checked and built the same way).
//!
//! VobSub in Matroska (the Matroska codec mappings): the track's private data is the `.idx` file's header (size,
//! palette and the rest) and each block one subpicture unit. The `.sub` written is an MPEG-2 program stream of
//! 2048-byte packs, each unit in private stream 1 packets of substream 0x20 (the DVD-Video layout; ISO/IEC 13818-1
//! for the pack and PES headers), with the `.idx` lines `id:` and `timestamp: …, filepos: …` that VobSub files have.
//!
//! An MP4 / MOV text track (3GPP TS 26.245; QuickTime text): each sample a 16-bit length and the text (UTF-8, or
//! UTF-16 behind a byte order mark), read with craft-media's `parse_text_sample`; style boxes after it are left out.
//! A text track another track names as its chapters (`tref` `chap`) is not a subtitle track.
//!
//! An MP4's VobSub (M6.547): an `mp4s` sample entry whose `esds` (the ISO/IEC 14496-1 descriptors: ES, decoder
//! configuration, decoder specific info, each a tag and a size of 7 bits a byte) names object type 0xE0; each sample
//! one subpicture unit. Its decoder specific info holds the 16 colours, each `00 Y Cr Cb` (BT.601, video range): read
//! from an MP4 ffmpeg wrote from a VobSub of known colours (the outside oracle, as a black box). The frame is the
//! track's size.
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
use craft_media::isobmff;
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
    /// A DVD's subpictures (M6.542): taken out as a VobSub `.idx` + `.sub`.
    VobSub,
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
/// Matroska, MP4 or MOV, or has none. Only the file's headers are read.
#[must_use]
pub fn embedded_subtitles(path: &Path) -> Vec<EmbeddedSubtitle> {
    let Some(demuxer) = open(path, false) else {
        return mp4_subtitles(path);
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
/// `<file>.<stream + 1>.<language>.srt`, `.sup` or `.idx` (with its `.sub`), and gives the file's path. The whole
/// file is read (its frames' places).
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
    let Some(demuxer) = open(path, true) else {
        return extract_mp4_text(path, stream, folder);
    };
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
    let language = language(track);
    let (content, extension) = match format {
        EmbeddedFormat::Text => (srt(&frames, &track.codec_id).into_bytes(), "srt"),
        EmbeddedFormat::Pgs => (sup(&frames), "sup"),
        EmbeddedFormat::VobSub => {
            let private = undone(&track.encodings, track.codec_private.clone(), 2)
                .ok_or_else(|| fail("the VobSub header cannot be read".to_owned()))?;
            let units: Vec<(f64, Vec<u8>)> = frames
                .into_iter()
                .map(|(start, _, bytes)| (start, bytes))
                .collect();
            let (idx, sub) = vobsub_files(
                &String::from_utf8_lossy(&private),
                language.as_deref(),
                &units,
            );
            if units.is_empty() {
                return Err(fail(format!(
                    "subtitle stream {} holds nothing",
                    stream + 1
                )));
            }
            let _ = write_beside(path, folder, (stream, language.as_deref(), "sub"), &sub)?;
            (idx.into_bytes(), "idx")
        }
    };
    if content.is_empty() {
        return Err(fail(format!(
            "subtitle stream {} holds nothing",
            stream + 1
        )));
    }
    write_beside(
        path,
        folder,
        (stream, language.as_deref(), extension),
        &content,
    )
}

/// Writes `content` into `folder` as `<file>.<stream + 1>.<language>.<extension>` (`und` for no language), named
/// after the video file `path`, and gives its path.
fn write_beside(
    path: &Path,
    folder: &Path,
    (stream, language, extension): (usize, Option<&str>, &str),
    content: &[u8],
) -> Result<PathBuf, TranscodeError> {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("video");
    let language: String = language
        .unwrap_or("und")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(16)
        .collect();
    std::fs::create_dir_all(folder)
        .map_err(|error| TranscodeError::Decode(format!("{}: {error}", folder.display())))?;
    let out = folder.join(format!("{stem}.{}.{language}.{extension}", stream + 1));
    std::fs::write(&out, content)
        .map_err(|error| TranscodeError::Decode(format!("{}: {error}", out.display())))?;
    Ok(out)
}

/// The subtitle tracks of an MP4 / MOV file that can be taken out: text (M6.542) and VobSub (M6.547). Their places
/// among the file's tracks and their formats.
fn mp4_subtitle_tracks(file: &isobmff::Mp4File) -> Vec<(usize, EmbeddedFormat)> {
    let text = mp4_text_tracks(file);
    file.tracks
        .iter()
        .enumerate()
        .filter_map(|(index, track)| {
            if text.contains(&index) {
                Some((index, EmbeddedFormat::Text))
            } else {
                mp4_vobsub_palette(track).map(|_| (index, EmbeddedFormat::VobSub))
            }
        })
        .collect()
}

/// One ISO/IEC 14496-1 descriptor at the start of `data`: its tag, its body and what follows it.
fn descriptor(data: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, mut rest) = data.split_first()?;
    let mut size = 0usize;
    for _ in 0..4 {
        let (&byte, after) = rest.split_first()?;
        rest = after;
        size = (size << 7) | usize::from(byte & 0x7F);
        if byte & 0x80 == 0 {
            break;
        }
    }
    Some((tag, rest.get(..size)?, rest.get(size..)?))
}

/// The decoder specific info of an `esds` body (after its version and flags) whose decoder configuration names
/// object type 0xE0 (VobSub).
fn vobsub_specific_info(es: &[u8]) -> Option<&[u8]> {
    let (tag, body, _) = descriptor(es)?;
    if tag != 0x03 {
        return None;
    }
    // ES_ID, then the flags: a dependency (2 bytes), a URL (its length and itself), an OCR stream (2 bytes).
    let flags = *body.get(2)?;
    let mut at = 3usize;
    if flags & 0x80 != 0 {
        at += 2;
    }
    if flags & 0x40 != 0 {
        at += 1 + usize::from(*body.get(at)?);
    }
    if flags & 0x20 != 0 {
        at += 2;
    }
    let (tag, config, _) = descriptor(body.get(at..)?)?;
    if tag != 0x04 || *config.first()? != 0xE0 {
        return None;
    }
    // Object type, stream type, buffer size (3), maximum and average bit rate (4 each).
    let (tag, info, _) = descriptor(config.get(13..)?)?;
    (tag == 0x05).then_some(info)
}

/// The 16 colours (RGB) of an MP4 VobSub track (M6.547); `None` for any other track.
fn mp4_vobsub_palette(track: &isobmff::Track) -> Option<[[u8; 3]; 16]> {
    let entry = track.entries.first()?;
    if entry.format.0 != *b"mp4s" {
        return None;
    }
    let isobmff::CodecConfig::Unknown { raw, .. } = &entry.codec else {
        return None;
    };
    // The sample entry's six reserved bytes and data reference index, then its boxes.
    let esds = isobmff::boxes::find(raw.get(8..)?, b"esds")?;
    let info = vobsub_specific_info(esds.get(4..)?)?;
    let mut palette = [[0u8; 3]; 16];
    for (slot, entry) in palette.iter_mut().zip(info.chunks(4)) {
        let &[_, y, cr, cb] = entry else {
            return None;
        };
        *slot = rgb_601([y, cr, cb]);
    }
    (info.len() >= 64).then_some(palette)
}

/// A BT.601 video-range colour (Y, Cr, Cb) as RGB.
fn rgb_601([y, cr, cb]: [u8; 3]) -> [u8; 3] {
    let y = 1.164 * (f64::from(y) - 16.0);
    let (cr, cb) = (f64::from(cr) - 128.0, f64::from(cb) - 128.0);
    // `as` after clamping to a byte's range.
    let byte = |value: f64| value.round().clamp(0.0, 255.0) as u8;
    [
        byte(y + 1.596 * cr),
        byte(y - 0.813 * cr - 0.392 * cb),
        byte(y + 2.017 * cb),
    ]
}

/// The text tracks of an MP4 / MOV file (M6.542): `tx3g` or QuickTime `text` samples, not a track another names as
/// its chapters. Their places among the file's tracks.
fn mp4_text_tracks(file: &isobmff::Mp4File) -> Vec<usize> {
    let chapters: Vec<u32> = file
        .tracks
        .iter()
        .flat_map(|track| track.references.iter())
        .filter(|(kind, _)| kind.0 == *b"chap")
        .flat_map(|(_, ids)| ids.iter().copied())
        .collect();
    file.tracks
        .iter()
        .enumerate()
        .filter(|(_, track)| {
            !chapters.contains(&track.id)
                && track
                    .entries
                    .first()
                    .is_some_and(|entry| matches!(&entry.format.0, b"tx3g" | b"text"))
        })
        .map(|(index, _)| index)
        .collect()
}

fn open_mp4(path: &Path) -> Option<(ReadSeekSource<std::fs::File>, isobmff::Mp4File)> {
    let source = ReadSeekSource::new(std::fs::File::open(path).ok()?).ok()?;
    let file = isobmff::open(&source).ok()?;
    Some((source, file))
}

/// The language an MP4 track names (ISO 639-2/T); `None` for none or `und`.
fn mp4_language(track: &isobmff::Track) -> Option<String> {
    let code = track.language.trim();
    (!code.is_empty() && !code.eq_ignore_ascii_case("und")).then(|| code.to_owned())
}

/// The text (M6.542) and VobSub (M6.547) tracks of an MP4 / MOV file as subtitles that can be taken out.
fn mp4_subtitles(path: &Path) -> Vec<EmbeddedSubtitle> {
    let Some((_, file)) = open_mp4(path) else {
        return Vec::new();
    };
    mp4_subtitle_tracks(&file)
        .into_iter()
        .filter_map(|(index, format)| Some((file.tracks.get(index)?, format)))
        .enumerate()
        .map(|(stream, (track, format))| EmbeddedSubtitle {
            stream,
            format,
            language: mp4_language(track),
            name: String::new(),
            forced: false,
        })
        .collect()
}

/// Takes the MP4 / MOV subtitle track `stream` out: text as a `.srt` (M6.542), each sample's text from its
/// presentation time for its duration; VobSub as a `.idx` + `.sub` (M6.547), each sample a subpicture unit at its
/// presentation time.
fn extract_mp4_text(path: &Path, stream: usize, folder: &Path) -> Result<PathBuf, TranscodeError> {
    let fail = |message: String| TranscodeError::Decode(format!("{}: {message}", path.display()));
    let (source, file) =
        open_mp4(path).ok_or_else(|| fail("not a Matroska, MP4 or MOV file".to_owned()))?;
    let (index, format) = *mp4_subtitle_tracks(&file)
        .get(stream)
        .ok_or_else(|| fail(format!("no subtitle stream {}", stream + 1)))?;
    if format == EmbeddedFormat::VobSub {
        return extract_mp4_vobsub(path, (&source, &file, index), (stream, folder));
    }
    let track = file
        .tracks
        .get(index)
        .ok_or_else(|| fail(format!("no subtitle stream {}", stream + 1)))?;
    if track.timescale == 0 {
        return Err(fail("the text track has no timescale".to_owned()));
    }
    let scale = f64::from(track.timescale);
    let mut frames: Vec<(f64, f64, Vec<u8>)> = Vec::new();
    let mut total = 0usize;
    for (at, sample) in track.samples.iter().enumerate().take(MAX_CUES) {
        if usize::try_from(sample.size).unwrap_or(usize::MAX) > MAX_FRAME {
            continue;
        }
        let data = file
            .read_sample(&source, index, at)
            .map_err(|error| fail(error.to_string()))?;
        let Some(text) = isobmff::chapters::parse_text_sample(&data) else {
            continue;
        };
        total = total.saturating_add(text.len());
        if total > MAX_TRACK {
            return Err(fail("the subtitle track is too large".to_owned()));
        }
        // `as` is exact enough for subtitle times.
        let start = track.presentation_pts(at).unwrap_or(sample.pts) as f64 / scale;
        frames.push((
            start.max(0.0),
            f64::from(sample.duration) / scale,
            text.into_bytes(),
        ));
    }
    frames.sort_by(|a, b| a.0.total_cmp(&b.0));
    let content = srt(&frames, "S_TEXT/UTF8");
    if content.is_empty() {
        return Err(fail(format!(
            "subtitle stream {} holds nothing",
            stream + 1
        )));
    }
    write_beside(
        path,
        folder,
        (stream, mp4_language(track).as_deref(), "srt"),
        content.as_bytes(),
    )
}

/// Takes the MP4 VobSub track at `index` of `file` out as a `.idx` + `.sub` (M6.547), named for subtitle stream
/// `stream`.
fn extract_mp4_vobsub(
    path: &Path,
    (source, file, index): (&ReadSeekSource<std::fs::File>, &isobmff::Mp4File, usize),
    (stream, folder): (usize, &Path),
) -> Result<PathBuf, TranscodeError> {
    let fail = |message: String| TranscodeError::Decode(format!("{}: {message}", path.display()));
    let track = file
        .tracks
        .get(index)
        .ok_or_else(|| fail(format!("no subtitle stream {}", stream + 1)))?;
    let palette =
        mp4_vobsub_palette(track).ok_or_else(|| fail("the VobSub has no palette".to_owned()))?;
    if track.timescale == 0 {
        return Err(fail("the VobSub track has no timescale".to_owned()));
    }
    let scale = f64::from(track.timescale);
    let mut units: Vec<(f64, Vec<u8>)> = Vec::new();
    let mut total = 0usize;
    for (at, sample) in track.samples.iter().enumerate().take(MAX_CUES) {
        if usize::try_from(sample.size).unwrap_or(usize::MAX) > MAX_FRAME {
            continue;
        }
        let data = file
            .read_sample(source, index, at)
            .map_err(|error| fail(error.to_string()))?;
        total = total.saturating_add(data.len());
        if total > MAX_TRACK {
            return Err(fail("the subtitle track is too large".to_owned()));
        }
        // `as` is exact enough for subtitle times.
        let start = track.presentation_pts(at).unwrap_or(sample.pts) as f64 / scale;
        units.push((start.max(0.0), data));
    }
    units.sort_by(|a, b| a.0.total_cmp(&b.0));
    if units.iter().all(|(_, unit)| unit.is_empty()) {
        return Err(fail(format!(
            "subtitle stream {} holds nothing",
            stream + 1
        )));
    }
    let (width, height) = match (track.width, track.height) {
        (0, _) | (_, 0) => (720, 480),
        size => size,
    };
    let colours: Vec<String> = palette
        .iter()
        .map(|[r, g, b]| format!("{r:02x}{g:02x}{b:02x}"))
        .collect();
    let header = format!(
        "# VobSub index file, v7 (do not modify this line!)\nsize: {width}x{height}\npalette: {}\n",
        colours.join(", ")
    );
    let language = mp4_language(track);
    let (idx, sub) = vobsub_files(&header, language.as_deref(), &units);
    let _ = write_beside(path, folder, (stream, language.as_deref(), "sub"), &sub)?;
    write_beside(
        path,
        folder,
        (stream, language.as_deref(), "idx"),
        idx.as_bytes(),
    )
}

/// The palette a VobSub without one gets (M6.550): black, white, black and grey in turn, so that the usual
/// subpicture of a subtitle (background, letters, edge and anti-alias in entries 0 to 3, as DiscCraft's own DVDs and
/// most others number them) shows white letters with a black edge.
pub const DEFAULT_PALETTE: [&str; 16] = [
    "000000", "ffffff", "000000", "808080", "000000", "ffffff", "000000", "808080", "000000",
    "ffffff", "000000", "808080", "000000", "ffffff", "000000", "808080",
];

/// The bytes of a pack of a VobSub's `.sub`.
const PACK: usize = 2048;
/// The pack header's bytes (MPEG-2, no stuffing).
const PACK_HEADER: usize = 14;
/// The `.sub`'s program mux rate, in 50 bytes a second (10.08 Mbit/s, a DVD's).
const MUX_RATE: u32 = 25_200;

/// A VobSub (M6.542) as its two files: the `.idx` (the stream's `header` — size, palette and the rest, as an MKV
/// keeps it in the track's private data, without `id:` or `timestamp:` lines — then the stream's `id:` line in
/// `language`'s two letters, `--` for none, and each unit's `timestamp:` and `filepos:`) and the `.sub` (an MPEG-2
/// program stream of 2048-byte packs, each unit of `units` — its time in seconds and its bytes — in private stream 1
/// packets of substream 0x20, its time in the first one's PTS). Empty units are left out.
///
/// A header without the VobSub's first line gets it, and one without a palette (M6.550: an MKV that ffmpeg made from
/// a VOB, which holds no palette, has an empty header) gets [`DEFAULT_PALETTE`], so that the files can be used.
#[must_use]
pub fn vobsub_files(
    header: &str,
    language: Option<&str>,
    units: &[(f64, Vec<u8>)],
) -> (String, Vec<u8>) {
    let mut idx = String::new();
    if !header.trim_start().starts_with("# VobSub index file") {
        idx.push_str("# VobSub index file, v7 (do not modify this line!)\r\n");
    }
    for line in header.lines().map(str::trim_end) {
        if line.starts_with("id:") || line.starts_with("timestamp:") {
            continue;
        }
        idx.push_str(line);
        idx.push_str("\r\n");
    }
    if !header
        .lines()
        .any(|line| line.trim_start().starts_with("palette:"))
    {
        let _ = write!(idx, "palette: {}\r\n", DEFAULT_PALETTE.join(", "));
    }
    let two = language
        .filter(|code| code.len() == 2 && code.chars().all(|c| c.is_ascii_alphabetic()))
        .map_or_else(|| "--".to_owned(), str::to_ascii_lowercase);
    let _ = write!(idx, "id: {two}, index: 0\r\n");
    let mut sub = Vec::new();
    for (start, spu) in units.iter().filter(|(_, spu)| !spu.is_empty()) {
        // `as` saturates; times are short of a day.
        let millis = (start.max(0.0) * 1000.0).round() as u64;
        let _ = write!(
            idx,
            "timestamp: {:02}:{:02}:{:02}:{:03}, filepos: {:09x}\r\n",
            millis / 3_600_000,
            millis / 60_000 % 60,
            millis / 1000 % 60,
            millis % 1000,
            sub.len()
        );
        push_unit(&mut sub, spu, millis.saturating_mul(90));
    }
    (idx, sub)
}

/// Appends the subpicture unit `spu` to the `.sub` `out` in 2048-byte packs, its time `pts` (90 kHz) in the first
/// packet's PTS and the packs' clock. A pack's room left is a padding packet, or (when too small for one) stuffing
/// in the packet's header.
fn push_unit(out: &mut Vec<u8>, spu: &[u8], pts: u64) {
    let mut rest = spu;
    let mut first = true;
    while !rest.is_empty() {
        let pts_bytes = if first { 5 } else { 0 };
        // Pack header, packet start and length, three header bytes, the PTS, the substream number.
        let room = PACK - PACK_HEADER - 6 - 3 - pts_bytes - 1;
        let take = rest.len().min(room);
        let left = room - take;
        let (stuffing, padding) = if left >= 6 { (0, left) } else { (left, 0) };
        push_pack_header(out, pts);
        let length = 3 + pts_bytes + stuffing + 1 + take;
        out.extend_from_slice(&[0, 0, 1, 0xBD]);
        out.extend_from_slice(&u16::try_from(length).unwrap_or(u16::MAX).to_be_bytes());
        // '10', not scrambled, original; the PTS flag; the header's own length.
        out.push(0x81);
        out.push(if first { 0x80 } else { 0x00 });
        out.push(u8::try_from(pts_bytes + stuffing).unwrap_or(0));
        if first {
            push_pts(out, pts);
        }
        out.extend(std::iter::repeat_n(0xFF, stuffing));
        out.push(0x20);
        out.extend_from_slice(rest.get(..take).unwrap_or_default());
        if padding >= 6 {
            out.extend_from_slice(&[0, 0, 1, 0xBE]);
            out.extend_from_slice(&u16::try_from(padding - 6).unwrap_or(0).to_be_bytes());
            out.extend(std::iter::repeat_n(0xFF, padding - 6));
        }
        rest = rest.get(take..).unwrap_or_default();
        first = false;
    }
}

/// An MPEG-2 pack header with the system clock `scr` (90 kHz, no extension).
fn push_pack_header(out: &mut Vec<u8>, scr: u64) {
    // `as` keeps the bits each byte holds.
    let base = scr & 0x1_FFFF_FFFF;
    out.extend_from_slice(&[0, 0, 1, 0xBA]);
    out.push(0x44 | ((base >> 27) & 0x38) as u8 | ((base >> 28) & 0x03) as u8);
    out.push((base >> 20) as u8);
    out.push(0x04 | ((base >> 12) & 0xF8) as u8 | ((base >> 13) & 0x03) as u8);
    out.push((base >> 5) as u8);
    out.push(0x04 | ((base << 3) & 0xF8) as u8);
    out.push(0x01);
    let rate = (MUX_RATE << 2) | 3;
    out.extend_from_slice(&rate.to_be_bytes()[1..]);
    // Reserved bits, no stuffing.
    out.push(0xF8);
}

/// A PTS field ('0010' and the 33-bit time with its marker bits).
fn push_pts(out: &mut Vec<u8>, pts: u64) {
    // `as` keeps the bits each byte holds.
    let pts = pts & 0x1_FFFF_FFFF;
    out.push(0x21 | ((pts >> 29) & 0x0E) as u8);
    out.push((pts >> 22) as u8);
    out.push(0x01 | ((pts >> 14) & 0xFE) as u8);
    out.push((pts >> 7) as u8);
    out.push(0x01 | ((pts << 1) & 0xFE) as u8);
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
            "S_VOBSUB" => EmbeddedFormat::VobSub,
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
fn decoded(encodings: &[ContentEncoding], bytes: Vec<u8>) -> Option<Vec<u8>> {
    undone(encodings, bytes, 1)
}

/// `bytes` as they were before those of the track's encodings whose scope has the bit `scope` (1: the frames, 2: the
/// codec's private data, 4: the next encoding's settings); `None` when they cannot be undone.
fn undone(encodings: &[ContentEncoding], mut bytes: Vec<u8>, bit: u64) -> Option<Vec<u8>> {
    for encoding in encodings.iter().rev() {
        match encoding {
            ContentEncoding::HeaderStripping {
                bytes: stripped,
                scope,
            } => {
                if scope & bit != 0 {
                    let mut whole = stripped.clone();
                    whole.extend_from_slice(&bytes);
                    bytes = whole;
                }
            }
            ContentEncoding::Compression { algo, scope, .. } => {
                if scope & bit != 0 {
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
    use super::{
        ContentEncoding, ass_text, decoded, rgb_601, srt, sup, timecode, vobsub_files,
        vobsub_specific_info,
    };

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

    /// M6.542: a VobSub's two files: the header kept (without its own `id:` and `timestamp:` lines), the stream's
    /// id, each unit's time and place; the `.sub` in 2048-byte packs, a unit of more than one pack in packets of
    /// substream 0x20 whose payloads add up to it, its time in the first packet's PTS.
    #[test]
    fn subpicture_units_become_a_vobsub() {
        let palette = "palette: 000000, ffffff, 808080, 000000, 000000, 000000, 000000, 000000, 000000, 000000, \
                       000000, 000000, 000000, 000000, 000000, 000000";
        let header = format!(
            "# VobSub index file, v7\nsize: 720x480\n{palette}\nid: en, index: 0\n\
             timestamp: 00:00:00:000, filepos: 000000000\n"
        );
        let small = vec![0, 8, 0, 4, 1, 2, 3, 0xFF];
        let mut large = vec![0u8; 3000];
        large[..2].copy_from_slice(&3000u16.to_be_bytes());
        for (at, byte) in large.iter_mut().enumerate().skip(2) {
            *byte = (at % 251) as u8;
        }
        let units = [
            (1.5, small.clone()),
            (62.25, large.clone()),
            (70.0, Vec::new()),
        ];
        let (idx, sub) = vobsub_files(&header, Some("ja"), &units);
        assert_eq!(
            idx,
            format!(
                "# VobSub index file, v7\r\nsize: 720x480\r\n{palette}\r\nid: ja, index: 0\r\n\
                 timestamp: 00:00:01:500, filepos: 000000000\r\n\
                 timestamp: 00:01:02:250, filepos: 000000800\r\n"
            )
        );
        // One pack for the small unit, two for the large.
        assert_eq!(sub.len(), 3 * 2048);
        // Each pack: its header, then the private stream 1 packet; the payloads after the substream byte.
        let mut units = Vec::new();
        for pack in sub.chunks(2048) {
            assert_eq!(&pack[..4], &[0, 0, 1, 0xBA]);
            assert_eq!(&pack[14..18], &[0, 0, 1, 0xBD]);
            let length = usize::from(u16::from_be_bytes([pack[18], pack[19]]));
            let header = usize::from(pack[22]);
            assert_eq!(pack[23 + header], 0x20);
            let payload = &pack[24 + header..20 + length];
            if pack[21] & 0x80 != 0 {
                // A PTS: a new unit.
                let p = &pack[23..28];
                let pts = (u64::from(p[0] & 0x0E) << 29)
                    | (u64::from(p[1]) << 22)
                    | (u64::from(p[2] >> 1) << 15)
                    | (u64::from(p[3]) << 7)
                    | u64::from(p[4] >> 1);
                units.push((pts, payload.to_vec()));
            } else if let Some(last) = units.last_mut() {
                last.1.extend_from_slice(payload);
            }
        }
        assert_eq!(units, vec![(135_000, small), (5_602_500, large)]);
        // Without a two-letter language: `--`; without the first line, it is put in.
        let first = "# VobSub index file, v7 (do not modify this line!)\r\n";
        let (idx, _) = vobsub_files("palette: 0", Some("jpn"), &[]);
        assert_eq!(idx, format!("{first}palette: 0\r\nid: --, index: 0\r\n"));
        // M6.550: an empty header (an MKV ffmpeg made from a VOB) gets the default palette.
        let (idx, _) = vobsub_files("", None, &[]);
        assert_eq!(
            idx,
            format!(
                "{first}palette: {}\r\nid: --, index: 0\r\n",
                super::DEFAULT_PALETTE.join(", ")
            )
        );
        assert!(idx.contains("palette: 000000, ffffff, 000000, 808080, 000000"));
    }

    /// M6.547: an MP4 VobSub's `esds` (as ffmpeg writes it, the bytes after its version and flags): the ES
    /// descriptor, the decoder configuration of object type 0xE0, and its 64 bytes of colours; red is `00 51 EF 5A`.
    #[test]
    fn an_mp4_vobsubs_colours_are_read_from_its_esds() {
        let mut es = vec![0x03, 0x80, 0x80, 0x80, 0x60, 0x00, 0x02, 0x00];
        es.extend_from_slice(&[
            0x04, 0x80, 0x80, 0x80, 0x52, 0xE0, 0xE1, 0, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0,
        ]);
        es.extend_from_slice(&[0x05, 0x80, 0x80, 0x80, 0x40]);
        let mut colours = vec![0u8; 60];
        colours.extend_from_slice(&[0x00, 0x51, 0xEF, 0x5A]);
        es.extend_from_slice(&colours);
        es.extend_from_slice(&[0x06, 0x80, 0x80, 0x80, 0x01, 0x02]);
        let info = vobsub_specific_info(&es).unwrap();
        assert_eq!(info.len(), 64);
        // Red, within the rounding of the stored colour (Cr 239, not 240).
        let red = rgb_601([info[61], info[62], info[63]]);
        assert!(red[0] >= 252 && red[1] <= 2 && red[2] <= 2, "{red:?}");
        // Another object type is not a VobSub.
        let mut other = es.clone();
        other[13] = 0x40;
        assert_eq!(vobsub_specific_info(&other), None);
        assert_eq!(vobsub_specific_info(&es[..20]), None);
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
