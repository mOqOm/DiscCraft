//! MPEG-2 video and AC-3 in program and transport streams (`.vob`, `.mpg`, `.ts`) that can go on a DVD as they are.
//!
//! A raw elementary stream (`.m2v`, `.ac3`) of DVD video or audio is passed through; the same streams in a container
//! were transcoded, so a DVD's own VOB lost quality and took the time of a full encode. [`probe`] reads a container's
//! index (no decoding) and tells what of it can be passed through; [`demux_to_cache`] writes those streams to the
//! build cache as elementary streams, which then take the elementary-stream path.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use craft_media::ReadSeekSource;
use craft_media::mpegts::{self, Codec, File, Kind};
use disccraft_project::{DiscKind, VideoStandard};
use disccraft_psmux::es::parse_ac3;

use crate::DvdError;
use crate::prepare::{audio_cache_key, first_sequence_header_matches, part_number};

/// The fastest AC-3 a DVD carries, kbit/s.
const DVD_MAX_AC3_KBPS: u32 = 448;
/// The fastest AC-3 a Blu-ray carries, kbit/s.
const BD_MAX_AC3_KBPS: u32 = 640;
/// 90 kHz ticks of an AC-3 frame (1536 samples at 48 kHz).
const AC3_FRAME_90K: i64 = 2_880;
/// The latest an AC-3 stream may start after the first picture and still be passed through, kept that far behind
/// (90 kHz: one second). Later than that, it is transcoded.
const MAX_AUDIO_DELAY_90K: i64 = 90_000;

/// What of a program or transport stream can go on a DVD as it is.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Probe {
    /// Its MPEG-2 video fits the disc's standard and starts with a closed GOP: its bytes, and its GOPs (when the
    /// first picture of each is shown, seconds from the video's first, and whether it is closed).
    pub(crate) video: Option<(u64, Vec<(f64, bool)>)>,
    /// Its first audio stream is AC-3 a DVD can carry: the bytes kept (the frames from the video's first picture on)
    /// and how long after the first picture the first of them starts (27 MHz; at most [`MAX_AUDIO_DELAY_90K`]).
    pub(crate) ac3: Option<(u64, i64)>,
}

/// What of `path` can go on a disc of `kind` and `standard` as it is (the video: on a DVD); `None` for a file that
/// is not a program or transport stream (or cannot be read as one). Read once per file and modification time.
pub(crate) fn probe(path: &Path, standard: VideoStandard, kind: DiscKind) -> Option<Arc<Probe>> {
    type Memo = Mutex<HashMap<(PathBuf, u64, u64, bool, bool), Option<Arc<Probe>>>>;
    static MEMO: OnceLock<Memo> = OnceLock::new();

    if !is_container(path) {
        return None;
    }
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos() as u64);
    let key = (
        path.to_path_buf(),
        metadata.len(),
        modified,
        standard == VideoStandard::Ntsc,
        kind == DiscKind::BluRay,
    );
    let memo = MEMO.get_or_init(Mutex::default);
    if let Some(known) = memo.lock().ok()?.get(&key) {
        return known.clone();
    }
    let max_ac3_kbps = match kind {
        DiscKind::BluRay => BD_MAX_AC3_KBPS,
        _ => DVD_MAX_AC3_KBPS,
    };
    let probed = probe_file(path, standard, max_ac3_kbps).map(Arc::new);
    memo.lock().ok()?.insert(key, probed.clone());
    probed
}

/// Whether the file starts like a program or transport stream.
fn is_container(path: &Path) -> bool {
    let mut head = vec![0u8; 64 * 1024];
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let read = std::io::Read::read(&mut file, &mut head).unwrap_or(0);
    head.truncate(read);
    mpegts::sniff(&head).is_some()
}

/// The file to read units from and its index, which is made once for a file and shared (`ts_index`: the index
/// takes a reading of the whole file).
fn open(path: &Path) -> Option<(ReadSeekSource<fs::File>, Arc<File>)> {
    disccraft_transcode::ts_index::open(path)
}

fn probe_file(path: &Path, standard: VideoStandard, max_ac3_kbps: u32) -> Option<Probe> {
    let (source, file) = open(path)?;
    let video = file.find(Kind::Video, |codec| *codec == Codec::Mpeg2Video);
    let video = video.and_then(|index| video_plan(&source, &file, index, standard));
    let ac3 = ac3_plan(&source, &file, video_origin(&file), max_ac3_kbps);
    Some(Probe { video, ac3 })
}

/// When the first picture of the file's video (MPEG-2 or H.264) is shown, 90 kHz: where its demuxed video starts, and
/// so the time its demuxed audio is kept in step with.
fn video_origin(file: &File) -> Option<i64> {
    let index = file.find(Kind::Video, |codec| {
        matches!(codec, Codec::Mpeg2Video | Codec::H264)
    })?;
    file.streams
        .get(index)?
        .units
        .iter()
        .filter_map(|unit| unit.pts)
        .min()
}

/// The video's size and GOPs, when it can be passed through: a sequence header for this standard and a closed GOP at
/// its first unit, and frame pictures only.
fn video_plan(
    source: &ReadSeekSource<fs::File>,
    file: &File,
    index: usize,
    standard: VideoStandard,
) -> Option<(u64, Vec<(f64, bool)>)> {
    let stream = file.streams.get(index)?;
    let first = stream.units.first()?;
    let picture = first.picture?;
    if !(first.key && picture.gop && picture.closed_gop) {
        return None;
    }
    let head = file.read_unit(source, index, 0).ok()?;
    if !first_sequence_header_matches(&head, standard) {
        return None;
    }
    if stream
        .units
        .iter()
        .any(|unit| unit.picture.is_none_or(|picture| picture.structure != 3))
    {
        return None;
    }
    let bytes = stream.units.iter().map(|unit| u64::from(unit.size)).sum();
    Some((bytes, gops(stream)))
}

/// When each GOP's first picture is shown (seconds from the stream's first shown picture) and whether it is closed:
/// the earliest presentation time among its pictures that carry one (a DVD stamps most, not all).
fn gops(stream: &mpegts::Stream) -> Vec<(f64, bool)> {
    let mut gops: Vec<(Option<i64>, bool)> = Vec::new();
    for unit in &stream.units {
        let Some(picture) = unit.picture else {
            continue;
        };
        if picture.gop || gops.is_empty() {
            gops.push((unit.pts, picture.closed_gop));
        } else if let (Some(last), Some(pts)) = (gops.last_mut(), unit.pts) {
            last.0 = Some(last.0.map_or(pts, |start| start.min(pts)));
        }
    }
    let start = gops.iter().filter_map(|gop| gop.0).min().unwrap_or(0);
    gops.into_iter()
        .filter_map(|(pts, closed)| Some(((pts? - start) as f64 / 90_000.0, closed)))
        .collect()
}

/// The bytes of the first audio stream's AC-3 kept for the disc (the frames from `video_start` on) and their delay
/// after it (27 MHz), when it is AC-3 the disc can carry (48 kHz, at most `max_kbps`).
fn ac3_plan(
    source: &ReadSeekSource<fs::File>,
    file: &File,
    video_start: Option<i64>,
    max_kbps: u32,
) -> Option<(u64, i64)> {
    let index = file.find(Kind::Audio, |_| true)?;
    let stream = file.streams.get(index)?;
    if stream.codec != Codec::Ac3 {
        return None;
    }
    let first = file.read_unit(source, index, 0).ok()?;
    let parsed = parse_ac3(&first, 0).ok()?;
    if parsed.sample_rate != 48_000 || parsed.bit_rate_kbps > max_kbps {
        return None;
    }
    let (kept, delay) = kept_audio_units(stream, video_start)?;
    let bytes = stream
        .units
        .get(kept..)?
        .iter()
        .map(|unit| u64::from(unit.size))
        .sum();
    Some((bytes, delay * 300))
}

/// The first audio unit kept so the audio starts with the video, and how long after `video_start` it starts (90 kHz,
/// 0 when about with it): the first unit that starts no earlier than half a frame before `video_start`, so audio
/// that starts first loses the frames ahead of the picture, and audio that starts later keeps that delay. `None`
/// when it starts more than [`MAX_AUDIO_DELAY_90K`] after the video, or no unit has a time.
fn kept_audio_units(stream: &mpegts::Stream, video_start: Option<i64>) -> Option<(usize, i64)> {
    let starts = audio_times(stream)?;
    let Some(video_start) = video_start else {
        return Some((0, 0));
    };
    let kept = starts.partition_point(|&pts| pts < video_start - AC3_FRAME_90K / 2);
    let delay = (*starts.get(kept)? - video_start).max(0);
    (delay <= MAX_AUDIO_DELAY_90K).then_some((kept, delay))
}

/// Each AC-3 frame's presentation time (90 kHz): a PES packet stamps only the first frame it starts, so the others are
/// counted on from the frame before at the frame's length (before the first stamped one, back from it). `None` when
/// no frame is stamped.
fn audio_times(stream: &mpegts::Stream) -> Option<Vec<i64>> {
    let (first, first_pts) = stream
        .units
        .iter()
        .enumerate()
        .find_map(|(index, unit)| Some((index, unit.pts?)))?;
    let mut times = Vec::with_capacity(stream.units.len());
    let mut previous = first_pts - AC3_FRAME_90K * first as i64 - AC3_FRAME_90K;
    for unit in &stream.units {
        let time = unit.pts.unwrap_or(previous + AC3_FRAME_90K);
        times.push(time);
        previous = time;
    }
    Some(times)
}

/// Which stream [`demux_to_cache`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    /// The MPEG-2 video, as `.m2v`.
    Video,
    /// The first audio stream's AC-3 from the video's first picture on, as `.ac3`.
    Ac3,
}

/// Writes `part` of the program or transport stream `path` to `cache_dir` as an elementary stream (once: a finished
/// one is reused) and returns its path.
///
/// # Errors
///
/// [`DvdError::Unsupported`] when the file cannot be demuxed, and I/O errors.
pub(crate) fn demux_to_cache(
    path: &Path,
    cache_dir: &Path,
    part: Part,
) -> Result<PathBuf, DvdError> {
    fs::create_dir_all(cache_dir).map_err(|err| crate::io_error(cache_dir, err))?;
    let key = audio_cache_key(path)?;
    let (name, extension) = match part {
        Part::Video => ("demux-video", "m2v"),
        Part::Ac3 => ("demux-ac3", "ac3"),
    };
    let out = cache_dir.join(format!("{key}.{name}.{extension}"));
    let marker = cache_dir.join(format!("{key}.{name}.ok"));
    if marker.is_file() && out.is_file() {
        return Ok(out);
    }

    let unsupported =
        |what: &str| DvdError::Unsupported(format!("demux {}: {what}", path.display()));
    let (source, file) =
        open(path).ok_or_else(|| unsupported("not a program or transport stream"))?;
    let video = file.find(Kind::Video, |codec| *codec == Codec::Mpeg2Video);
    let (stream, first) = match part {
        Part::Video => (video.ok_or_else(|| unsupported("no MPEG-2 video"))?, 0),
        Part::Ac3 => {
            let index = file
                .find(Kind::Audio, |codec| *codec == Codec::Ac3)
                .ok_or_else(|| unsupported("no AC-3"))?;
            let units = file
                .streams
                .get(index)
                .ok_or_else(|| unsupported("no AC-3"))?;
            let video_start = video_origin(&file);
            let (kept, _) = kept_audio_units(units, video_start)
                .ok_or_else(|| unsupported("the AC-3 starts long after the video"))?;
            (index, kept)
        }
    };
    let count = file
        .streams
        .get(stream)
        .map_or(0, |stream| stream.units.len());

    let partial = cache_dir.join(format!("{key}.{name}.{}.part", part_number()));
    let write = || -> Result<(), DvdError> {
        let created = fs::File::create(&partial).map_err(|err| crate::io_error(&partial, err))?;
        let mut writer = std::io::BufWriter::with_capacity(1 << 20, created);
        for unit in first..count {
            let bytes = file
                .read_unit(&source, stream, unit)
                .map_err(|err| unsupported(&err.to_string()))?;
            writer
                .write_all(&bytes)
                .map_err(|err| crate::io_error(&partial, err))?;
        }
        writer.flush().map_err(|err| crate::io_error(&partial, err))
    };
    if let Err(err) = write() {
        fs::remove_file(&partial).unwrap_or(());
        return Err(err);
    }
    fs::rename(&partial, &out).map_err(|err| crate::io_error(&partial, err))?;
    fs::write(&marker, b"ok").map_err(|err| crate::io_error(&marker, err))?;
    Ok(out)
}

/// The highest H.264 level a Blu-ray carries (4.1).
const BD_MAX_H264_LEVEL: u8 = 41;
/// The fastest video a Blu-ray carries, bit/s.
pub(crate) const BD_MAX_VIDEO_BITRATE: f64 = 40_000_000.0;

/// The video of a Blu-ray HD title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BdVideoFormat {
    /// Frame width, pixels.
    pub width: u32,
    /// Frame height, pixels.
    pub height: u32,
    /// Frames a second (numerator, denominator); an interlaced format's frames, not its fields.
    pub rate: (u32, u32),
    /// Interlaced (1080i).
    pub interlaced: bool,
}

impl BdVideoFormat {
    /// The HD video of format `video`; `None` for SD.
    #[must_use]
    pub fn of_video(video: disccraft_project::BdVideo) -> Option<Self> {
        let (_, height, num, den) = video.hd()?;
        Some(Self {
            // The width its video is coded at (M10.40: a 1440 x 1080 format's 1440).
            width: video.video_width()?,
            height,
            rate: (num, den),
            interlaced: video.interlaced(),
        })
    }

    /// The HD video the titles of the video file `path` are made in on `project`'s disc: its asset's own HD format
    /// (M6.325), else the project's; `None` when it is not a Blu-ray in an HD format.
    #[must_use]
    pub fn of_path(project: &disccraft_project::Project, path: &Path) -> Option<Self> {
        if project.settings.kind != DiscKind::BluRay {
            return None;
        }
        let video = project
            .assets
            .values()
            .find(|asset| asset.media_file().as_ref() == path)
            .map_or(project.settings.bd_video, |asset| {
                project.asset_bd_video(asset.id)
            });
        Self::of_video(video)
    }

    /// The video of `project`'s titles that the Blu-ray build encodes: an HD format's, or (M10.41) an SD H.264
    /// disc's 720 x 480 / 720 x 576, interlaced; `None` when it is not a Blu-ray, or an SD one in MPEG-2.
    #[must_use]
    pub fn of(project: &disccraft_project::Project) -> Option<Self> {
        if project.settings.kind != DiscKind::BluRay {
            return None;
        }
        if project.settings.bd_sd_h264() {
            let (height, rate) = match project.settings.standard {
                disccraft_project::VideoStandard::Ntsc => (480, (30_000, 1001)),
                disccraft_project::VideoStandard::Pal => (576, (25, 1)),
            };
            return Some(Self {
                width: 720,
                height,
                rate,
                interlaced: true,
            });
        }
        Self::of_video(project.settings.bd_video)
    }

    /// A name for cache files.
    pub(crate) fn key(self) -> String {
        format!(
            "{}x{}{}-{}-{}",
            self.width,
            self.height,
            if self.interlaced { "i" } else { "" },
            self.rate.0,
            self.rate.1
        )
    }
}

/// Whether pictures `width` wide are a Blu-ray's video of `format`: the format's width, or for the 1080-line
/// formats also 1440 (the anamorphic 1440 x 1080 of HDV and AVCHD camcorders, shown 16:9 as 1920 x 1080 is).
pub(crate) fn width_fits(width: u32, format: BdVideoFormat) -> bool {
    width == format.width || (width == 1440 && (format.width, format.height) == (1920, 1080))
}

/// Whether H.264 with this sequence parameter set is a Blu-ray's video of `format`: Main or High profile up to
/// level 4.1, 4:2:0 at 8 bits, the format's size (in whole macroblocks; 1440 wide too for 1080 lines), and coded
/// as the format is scanned (an interlaced format's stream may code fields, a progressive format's codes frames
/// only).
pub(crate) fn sps_fits(sps: &disccraft_transcode::h264_sps::Sps, format: BdVideoFormat) -> bool {
    matches!(sps.profile, 77 | 100)
        && sps.level <= BD_MAX_H264_LEVEL
        && sps.yuv420_8bit
        && width_fits(sps.width, format)
        && sps.height == format.height.div_ceil(16).saturating_mul(16)
        && sps.frame_mbs_only != format.interlaced
}

/// Whether `path`'s H.264 would go on a Blu-ray title of `format` as it is ([`demux_h264_for_bd`]), told from the
/// file's headers without reading it through.
#[must_use]
pub fn h264_fits_bd(path: &Path, format: BdVideoFormat) -> bool {
    // Asked again and again as a project's size is estimated: answered once per file (its size and time).
    type Memo = Mutex<HashMap<(PathBuf, u64, u64, BdVideoFormat), bool>>;
    static MEMO: OnceLock<Memo> = OnceLock::new();

    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos() as u64);
    let key = (path.to_path_buf(), metadata.len(), modified, format);
    let memo = MEMO.get_or_init(Mutex::default);
    if let Some(known) = memo.lock().ok().and_then(|memo| memo.get(&key).copied()) {
        return known;
    }
    let fits = h264_fits_bd_now(path, format);
    if let Ok(mut memo) = memo.lock() {
        let _ = memo.insert(key, fits);
    }
    fits
}

fn h264_fits_bd_now(path: &Path, format: BdVideoFormat) -> bool {
    if crate::demux_mkv::is_matroska(path) {
        return crate::demux_mkv::fits(path, format);
    }
    if crate::demux_mp4::is_mp4(path) {
        return crate::demux_mp4::fits(path, format);
    }
    if !is_container(path) {
        return false;
    }
    let Some((source, file)) = open(path) else {
        return false;
    };
    file.find(Kind::Video, |codec| *codec == Codec::H264)
        .is_some_and(|index| h264_fits(path, &source, &file, index, format))
        && video_origin(&file).is_some()
}

/// The H.264 pictures of the transport stream `path` as its index tells them (times in 90 kHz; offsets and
/// decode times are not worked out), and a test of whether one is an IDR picture, read from the file. `None`
/// when the file is not a transport stream with H.264 video.
pub(crate) fn ts_h264_keys(
    path: &Path,
) -> Option<(
    Vec<disccraft_transcode::hd::AccessUnit>,
    Box<dyn Fn(usize) -> bool + Send>,
)> {
    if !is_container(path) {
        return None;
    }
    let (source, file) = open(path)?;
    let index = file.find(Kind::Video, |codec| *codec == Codec::H264)?;
    let units = file
        .streams
        .get(index)?
        .units
        .iter()
        .map(|unit| {
            Some(disccraft_transcode::hd::AccessUnit {
                offset: 0,
                len: unit.size,
                pts: unit.pts?,
                dts: unit.pts?,
                keyframe: unit.key,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let clean = move |unit: usize| {
        file.read_unit(&source, index, unit)
            .is_ok_and(|bytes| crate::h264_cut::is_idr(&bytes))
    };
    Some((units, Box::new(clean)))
}

/// A file's H.264 that goes on a Blu-ray as it is ([`demux_h264_for_bd`]).
#[derive(Debug)]
pub enum Passthrough {
    /// H.264 left in its transport stream (M6.367), read from there through the stream's index as the clip is
    /// written.
    Transport(TsUnits),
    /// HD MPEG-2 left in its transport stream (M6.380), read from there in the same way.
    TransportMpeg2(TsUnits),
    /// Taken out of a transport stream into the build cache: an Annex B stream and its access-unit index.
    Stream {
        /// The Annex B stream.
        es: PathBuf,
        /// Its access-unit index (`disccraft_transcode::hd::read_h264_index`).
        index: PathBuf,
    },
    /// Left in its Matroska or MP4 file, and read from there as the clip is written.
    Samples(crate::H264Samples),
    /// HD MPEG-2 video (M10.22) taken out of a transport stream into the build cache: the elementary stream and
    /// its access-unit index (the same form as an H.264 stream's).
    Mpeg2 {
        /// The MPEG-2 video elementary stream.
        es: PathBuf,
        /// Its access-unit index.
        index: PathBuf,
    },
}

/// Video left in a transport stream (H.264 M6.367, HD MPEG-2 M6.380): each access unit is read from the file
/// through the stream's index
/// (held for the build, so it is not made again) as the disc's clip is written, so the video is not copied into
/// the build cache first.
#[derive(Clone)]
pub struct TsUnits {
    path: PathBuf,
    index: Arc<File>,
    stream: usize,
    /// For each access unit, the unit of the stream it is.
    picks: Vec<usize>,
    /// The access units: lengths, times (90 kHz from the first shown picture), key frames; offsets as in one
    /// stream of them.
    units: Vec<disccraft_transcode::hd::AccessUnit>,
}

impl std::fmt::Debug for TsUnits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TsUnits")
            .field("path", &self.path)
            .field("stream", &self.stream)
            .field("units", &self.units.len())
            .finish_non_exhaustive()
    }
}

impl PartialEq for TsUnits {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.stream == other.stream
            && self.picks == other.picks
            && self.units == other.units
    }
}

impl TsUnits {
    /// The transport stream.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The access units, in decoding order.
    #[must_use]
    pub fn units(&self) -> &[disccraft_transcode::hd::AccessUnit] {
        &self.units
    }

    /// The units `indices` alone, as the access units `units` (the same pictures, timed anew): a trimmed clip.
    #[must_use]
    pub(crate) fn keep(
        &self,
        indices: &[usize],
        units: Vec<disccraft_transcode::hd::AccessUnit>,
    ) -> Self {
        Self {
            picks: indices
                .iter()
                .filter_map(|index| self.picks.get(*index).copied())
                .collect(),
            units,
            ..self.clone()
        }
    }

    /// Access unit `unit` as Annex B bytes, read from `file` (the transport stream, opened).
    ///
    /// # Errors
    ///
    /// When the file cannot be read, or the unit is not as long as when the stream was indexed (the file changed).
    pub fn read(&self, file: &mut fs::File, unit: usize) -> std::io::Result<Vec<u8>> {
        let invalid = |what: String| std::io::Error::new(std::io::ErrorKind::InvalidData, what);
        let (Some(pick), Some(expected)) = (self.picks.get(unit), self.units.get(unit)) else {
            return Err(invalid(format!("no unit {unit}")));
        };
        let source = ReadSeekSource::new(file)?;
        let bytes = self
            .index
            .read_unit(&source, self.stream, *pick)
            .map_err(|error| invalid(error.to_string()))?;
        if u32::try_from(bytes.len()).ok() != Some(expected.len) {
            return Err(invalid(format!(
                "{}: the file changed since it was read",
                self.path.display()
            )));
        }
        Ok(bytes)
    }
}

/// The frame rate code of an MPEG-2 sequence header for `rate` (ISO/IEC 13818-2 table 6-4), 0 for none.
fn mpeg2_rate_code(rate: (u32, u32)) -> u8 {
    match rate {
        (24_000, 1001) => 1,
        (24, 1) => 2,
        (25, 1) => 3,
        (30_000, 1001) => 4,
        (30, 1) => 5,
        (50, 1) => 6,
        (60_000, 1001) => 7,
        (60, 1) => 8,
        _ => 0,
    }
}

/// Whether MPEG-2 video with this sequence header is a Blu-ray's video of `format`: Main profile at High or
/// High-1440 level, the format's size (1440 wide too for 1080 lines), frame rate and scan, at no more than
/// 40 Mbit/s.
pub(crate) fn mpeg2_sequence_fits(
    sequence: &disccraft_psmux::es::SequenceInfo,
    format: BdVideoFormat,
) -> bool {
    let height = u32::from(sequence.height);
    width_fits(u32::from(sequence.width), format)
        && (height == format.height || height == format.height.div_ceil(16).saturating_mul(16))
        && sequence.frame_rate_code != 0
        && sequence.frame_rate_code == mpeg2_rate_code(format.rate)
        && sequence.progressive_sequence != format.interlaced
        // profile_and_level_indication: Main profile (100), level High (0100) or High-1440 (0110).
        && matches!(sequence.profile_level, 0x44 | 0x46)
        && u64::from(sequence.bit_rate_400) * 400 <= BD_MAX_VIDEO_BPS
}

/// The fastest video a Blu-ray carries, bit/s.
const BD_MAX_VIDEO_BPS: u64 = 40_000_000;

/// Stream `index` of `file` when it is HD MPEG-2 a Blu-ray title of `format` carries as it is: its pictures
/// that play (the leading B pictures of an open first GOP are left out: what they refer to is not there), each
/// with its number in the stream. `None` when it is not: another size, rate or level, a picture without a time, or
/// a stream that does not start with a sequence header and an I picture. Frames coded as two field pictures
/// (M10.45, as some broadcast and recorder video has them) go as they are when each unit holds a frame: the first
/// unit holds both fields of its frame, and every unit starts with a frame picture or a first field of the same
/// parity as the first unit's (a unit a field would alternate).
fn mpeg2_plan(
    source: &ReadSeekSource<fs::File>,
    file: &File,
    index: usize,
    format: BdVideoFormat,
) -> Option<Vec<usize>> {
    let stream = file.streams.get(index)?;
    let first = stream.units.first()?;
    if !first.key || !first.picture?.gop {
        return None;
    }
    let first_field = first.picture?.structure;
    if stream.units.iter().any(|unit| {
        unit.pts.is_none()
            || unit
                .picture
                .is_none_or(|picture| picture.structure != 3 && picture.structure != first_field)
    }) {
        return None;
    }
    // The ES parser takes a unit of one field picture for a frame without its second field.
    let head = file.read_unit(source, index, 0).ok()?;
    let parsed = disccraft_psmux::es::parse_video(&head, 0).ok()?;
    if !mpeg2_sequence_fits(&parsed.sequence, format) {
        return None;
    }
    // The pictures shown before the first I picture are the open GOP's leading ones.
    let start = first.pts?;
    Some(
        stream
            .units
            .iter()
            .enumerate()
            .filter(|(_, unit)| unit.pts.is_some_and(|pts| pts >= start))
            .map(|(number, _)| number)
            .collect(),
    )
}

/// The head of an MPEG-2 video elementary stream (it starts with a sequence header), when `path` is one.
fn mpeg2_es_head(path: &Path) -> Option<Vec<u8>> {
    let mut head = vec![0u8; 512 * 1024];
    let mut file = fs::File::open(path).ok()?;
    let read = std::io::Read::read(&mut file, &mut head).ok()?;
    head.truncate(read);
    head.starts_with(&[0, 0, 1, 0xB3]).then_some(head)
}

/// The picture_structure of the first picture coding extension in `data` (3: a frame picture).
fn first_picture_structure(data: &[u8]) -> Option<u8> {
    data.windows(7)
        .find(|window| {
            window.starts_with(&[0, 0, 1, 0xB5]) && window.get(4).is_some_and(|byte| byte >> 4 == 8)
        })
        .and_then(|window| window.get(6))
        .map(|byte| byte & 0x03)
}

/// Whether the head of the elementary stream `path` says its video goes on a Blu-ray title of `format` as it is:
/// a sequence that fits, starting with an I picture after a GOP header (a frame picture or, M10.45, a frame of two
/// field pictures).
fn mpeg2_es_head_fits(path: &Path, format: BdVideoFormat) -> bool {
    let Some(head) = mpeg2_es_head(path) else {
        return false;
    };
    let Ok(parsed) = disccraft_psmux::es::parse_video_head(&head, 0) else {
        return false;
    };
    mpeg2_sequence_fits(&parsed.sequence, format)
        && parsed.pictures.first().is_some_and(|picture| {
            picture.gop_start && picture.kind == disccraft_psmux::es::PictureType::I
        })
        && first_picture_structure(&head).is_some_and(|structure| structure != 0)
}

/// An HD MPEG-2 video elementary stream (`.m2v`, M6.370) as it is, for an HD title of `format`: the stream is read
/// from the file itself as the clip is written, so only its access-unit index (timed from its first shown
/// picture; an open GOP's leading pictures left out) goes in `cache_dir`. `None` when it does not fit
/// ([`mpeg2_es_head_fits`]).
fn mpeg2_es_for_bd(
    path: &Path,
    cache_dir: &Path,
    format: BdVideoFormat,
) -> Result<Option<Passthrough>, DvdError> {
    if !mpeg2_es_head_fits(path, format) {
        return Ok(None);
    }
    fs::create_dir_all(cache_dir).map_err(|err| crate::io_error(cache_dir, err))?;
    let key = format!("{}-{}", audio_cache_key(path)?, format.key());
    let index_path = cache_dir.join(format!("{key}.passthrough.es.idx"));
    let marker = cache_dir.join(format!("{key}.passthrough.es.ok"));
    if marker.is_file() && index_path.is_file() {
        return Ok(Some(Passthrough::Mpeg2 {
            es: path.to_path_buf(),
            index: index_path,
        }));
    }
    let unsupported = |what: String| DvdError::Unsupported(format!("{}: {what}", path.display()));
    let file = fs::File::open(path).map_err(|err| crate::io_error(path, err))?;
    let video = disccraft_psmux::es::parse_video_reader(std::io::BufReader::new(file), 0)
        .map_err(|err| unsupported(err.to_string()))?;
    let Some(start) = video.pictures.first().map(|picture| picture.pts) else {
        return Ok(None);
    };
    // 27 MHz to 90 kHz.
    let units: Vec<disccraft_transcode::hd::AccessUnit> = video
        .pictures
        .iter()
        .filter(|picture| picture.pts >= start)
        .map(|picture| {
            Ok(disccraft_transcode::hd::AccessUnit {
                offset: u64::try_from(picture.offset)
                    .map_err(|_| unsupported("a picture is too far in".to_string()))?,
                len: u32::try_from(picture.len)
                    .map_err(|_| unsupported("a picture is too large".to_string()))?,
                pts: (picture.pts - start) / 300,
                dts: (picture.dts - start) / 300,
                keyframe: picture.kind == disccraft_psmux::es::PictureType::I,
            })
        })
        .collect::<Result<_, DvdError>>()?;
    let index_part = cache_dir.join(format!("{key}.{}.es.idx.part", part_number()));
    disccraft_transcode::hd::write_h264_index(&index_part, &units)
        .map_err(|err| unsupported(err.to_string()))?;
    fs::rename(&index_part, &index_path).map_err(|err| crate::io_error(&index_part, err))?;
    fs::write(&marker, b"ok").map_err(|err| crate::io_error(&marker, err))?;
    Ok(Some(Passthrough::Mpeg2 {
        es: path.to_path_buf(),
        index: index_path,
    }))
}

/// The narrower width a 1080-line Blu-ray title of `timeline` is made in (M6.388 / M6.393): HDV's 1440, when more
/// of the title's clips that go on as they are (HD MPEG-2 that fits `format`, without a Blu-ray preset of its own)
/// are 1440 x 1080 than are the disc's width. The title's other clips and its gaps are then encoded at 1440 x 1080
/// (shown 16:9), so the HDV clips join them as they are (pictures of two widths do not share one stream). `None`:
/// the disc's width.
#[must_use]
pub fn narrow_mpeg2_width(
    project: &disccraft_project::Project,
    timeline: &disccraft_project::Timeline,
    format: BdVideoFormat,
) -> Option<u32> {
    const HDV_WIDTH: u32 = 1440;
    if (format.width, format.height) != (1920, 1080) {
        return None;
    }
    let (mut narrow, mut full) = (0_usize, 0_usize);
    for video in timeline.videos() {
        let Some(asset) = project.assets.get(&video) else {
            continue;
        };
        if asset.bluray_transcode.is_some() {
            continue;
        }
        let media = asset.media_file();
        let path: &Path = &media;
        if !mpeg2_fits_bd(path, format) {
            continue;
        }
        match disccraft_transcode::probe_video(path).map(|probe| (probe.width, probe.height)) {
            Ok((HDV_WIDTH, 1080)) => narrow += 1,
            Ok((width, _)) if width == format.width => full += 1,
            _ => {}
        }
    }
    (narrow > full).then_some(HDV_WIDTH)
}

/// Whether `path`'s MPEG-2 video would go on a Blu-ray title of `format` as it is ([`demux_mpeg2_for_bd`]):
/// HD MPEG-2 in a transport stream (HDV, a broadcast recording).
#[must_use]
pub fn mpeg2_fits_bd(path: &Path, format: BdVideoFormat) -> bool {
    type Memo = Mutex<HashMap<(PathBuf, u64, u64, BdVideoFormat), bool>>;
    static MEMO: OnceLock<Memo> = OnceLock::new();

    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos() as u64);
    let key = (path.to_path_buf(), metadata.len(), modified, format);
    let memo = MEMO.get_or_init(Mutex::default);
    if let Some(known) = memo.lock().ok().and_then(|memo| memo.get(&key).copied()) {
        return known;
    }
    let fits = if is_container(path) {
        open(path).is_some_and(|(source, file)| {
            file.find(Kind::Video, |codec| *codec == Codec::Mpeg2Video)
                .is_some_and(|index| mpeg2_plan(&source, &file, index, format).is_some())
        })
    } else {
        mpeg2_es_head_fits(path, format)
    };
    if let Ok(mut memo) = memo.lock() {
        let _ = memo.insert(key, fits);
    }
    fits
}

/// The first reason the video of a file does not go on a Blu-ray title as it is (M6.376), for the asset's
/// properties: what a tester asked about (why a large MKV became a much smaller title).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BdMismatch {
    /// Not H.264 (nor HD MPEG-2 that fits): HEVC, VP9, MPEG-4 part 2 and so on.
    Codec,
    /// An H.264 profile other than Main and High (`profile_idc`: 110 High 10, 122 High 4:2:2, …).
    Profile(u8),
    /// Not 4:2:0 with 8 bits.
    Chroma,
    /// An H.264 level above 4.1 (`level_idc`, 51 for 5.1).
    Level(u8),
    /// Another picture size.
    Size(u32, u32),
    /// Interlaced on a progressive format (`true`), or progressive on an interlaced one.
    Scan(bool),
    /// Another frame rate (frames a second).
    Rate(f64),
    /// Faster than a Blu-ray's video on average (Mbit/s, the whole file counted).
    Bitrate(f64),
}

/// Why the video of `path` is converted rather than going on a Blu-ray title of `format` as it is: the first
/// thing that does not fit, read from the file's headers. `None` when it goes on as it is, or nothing is known.
#[must_use]
pub fn bd_mismatch(path: &Path, format: BdVideoFormat) -> Option<BdMismatch> {
    if video_fits_bd(path, format) {
        return None;
    }
    let sps = if crate::demux_mkv::is_matroska(path) {
        crate::demux_mkv::sps_of(path)
    } else if crate::demux_mp4::is_mp4(path) {
        crate::demux_mp4::sps_of(path)
    } else if is_container(path) {
        open(path).and_then(|(source, file)| {
            let index = file.find(Kind::Video, |codec| *codec == Codec::H264)?;
            disccraft_transcode::h264_sps::first_in_annex_b(
                &file.read_unit(&source, index, 0).ok()?,
            )
        })
    } else {
        None
    };
    let Some(sps) = sps else {
        return Some(BdMismatch::Codec);
    };
    if !matches!(sps.profile, 77 | 100) {
        return Some(BdMismatch::Profile(sps.profile));
    }
    if !sps.yuv420_8bit {
        return Some(BdMismatch::Chroma);
    }
    if sps.level > BD_MAX_H264_LEVEL {
        return Some(BdMismatch::Level(sps.level));
    }
    if !width_fits(sps.width, format) || sps.height != format.height.div_ceil(16).saturating_mul(16)
    {
        let height = if sps.height == 1088 { 1080 } else { sps.height };
        return Some(BdMismatch::Size(sps.width, height));
    }
    if sps.frame_mbs_only == format.interlaced {
        return Some(BdMismatch::Scan(!sps.frame_mbs_only));
    }
    let probe = disccraft_transcode::probe_video(path).ok()?;
    let (num, den) = format.rate;
    let rate = f64::from(num) / f64::from(den.max(1));
    if (probe.fps - rate).abs() > rate * 0.005 {
        return Some(BdMismatch::Rate(probe.fps));
    }
    let bytes = fs::metadata(path).ok()?.len();
    if probe.seconds > 0.0 {
        let bits = bytes as f64 * 8.0 / probe.seconds;
        if bits > BD_MAX_VIDEO_BITRATE {
            return Some(BdMismatch::Bitrate(bits / 1e6));
        }
    }
    None
}

/// Whether `path`'s video goes on a Blu-ray title of `format` as it is: H.264 ([`h264_fits_bd`]) or HD MPEG-2
/// ([`mpeg2_fits_bd`]).
#[must_use]
pub fn video_fits_bd(path: &Path, format: BdVideoFormat) -> bool {
    h264_fits_bd(path, format) || mpeg2_fits_bd(path, format)
}

/// A file's video that goes on a Blu-ray title of `format` as it is: its H.264 ([`demux_h264_for_bd`]), else its
/// HD MPEG-2 ([`demux_mpeg2_for_bd`]).
///
/// # Errors
///
/// As those.
pub fn demux_video_for_bd(
    path: &Path,
    cache_dir: &Path,
    format: BdVideoFormat,
    progress: &mut dyn FnMut(f32),
) -> Result<Option<Passthrough>, DvdError> {
    if let Some(found) = demux_h264_for_bd(path, cache_dir, format, progress)? {
        return Ok(Some(found));
    }
    demux_mpeg2_for_bd(path, cache_dir, format)
}

/// A transport stream's HD MPEG-2 video (HDV, a broadcast recording) as it is, for an HD title of `format`,
/// timed from its first shown picture: written once to `cache_dir` as an elementary stream and its access-unit
/// index. `None` when the file has no such video ([`mpeg2_fits_bd`]).
///
/// # Errors
///
/// I/O errors, and [`DvdError::Unsupported`] when a picture cannot be read.
pub fn demux_mpeg2_for_bd(
    path: &Path,
    cache_dir: &Path,
    format: BdVideoFormat,
) -> Result<Option<Passthrough>, DvdError> {
    if !is_container(path) {
        return mpeg2_es_for_bd(path, cache_dir, format);
    }
    let Some((source, file)) = open(path) else {
        return Ok(None);
    };
    let Some(index) = file.find(Kind::Video, |codec| *codec == Codec::Mpeg2Video) else {
        return Ok(None);
    };
    let Some(kept) = mpeg2_plan(&source, &file, index, format) else {
        return Ok(None);
    };
    let Some(stream) = file.streams.get(index) else {
        return Ok(None);
    };
    let Some(origin) = kept
        .iter()
        .filter_map(|number| stream.units.get(*number)?.pts)
        .min()
    else {
        return Ok(None);
    };

    // M6.380: left in the file; what an earlier version copied into the cache goes.
    let key = format!("{}-{}", audio_cache_key(path)?, format.key());
    for extension in ["m2v", "m2v.idx", "m2v.ok"] {
        let _ = fs::remove_file(cache_dir.join(format!("{key}.passthrough.{extension}")));
    }
    let unsupported =
        |what: String| DvdError::Unsupported(format!("demux {}: {what}", path.display()));
    let mut units = Vec::with_capacity(kept.len());
    let mut offset = 0u64;
    for number in &kept {
        let Some(unit) = stream.units.get(*number) else {
            continue;
        };
        let pts = unit
            .pts
            .ok_or_else(|| unsupported("a picture has no time".to_string()))?;
        units.push(disccraft_transcode::hd::AccessUnit {
            offset,
            len: unit.size,
            pts: pts - origin,
            // A B picture is decoded when it is shown; the others say when.
            dts: unit.dts.unwrap_or(pts).min(pts) - origin,
            keyframe: unit.key,
        });
        offset += u64::from(unit.size);
    }
    drop(source);
    Ok(Some(Passthrough::TransportMpeg2(TsUnits {
        path: path.to_path_buf(),
        index: Arc::clone(&file),
        stream: index,
        picks: kept,
        units,
    })))
}

/// A Blu-ray's H.264 from a transport stream (`.m2ts`, `.ts`), a Matroska file (`.mkv`) or an MP4 / QuickTime file
/// (`.mp4`, `.m4v`, `.mov`) as it is, for an HD title
/// of `format`, timed from its first shown picture, as an encode of the title would be. A transport stream's is
/// written once to `cache_dir` as an Annex B stream and its access-unit index; a Matroska or MP4 file's stays in
/// the file, and only where its pictures are is kept in `cache_dir`. `None` when the file has no such video: not one of those
/// containers, not H.264, another size or rate, progressive for an interlaced format (or the reverse), a profile
/// other than High or Main or a level above 4.1, faster than 40 Mbit/s on average, not starting at a key frame, or
/// a picture without a time. `progress` is told how far the file has been read (0 to 1).
///
/// # Errors
///
/// I/O errors, and [`DvdError::Unsupported`] when a unit cannot be read.
pub fn demux_h264_for_bd(
    path: &Path,
    cache_dir: &Path,
    format: BdVideoFormat,
    progress: &mut dyn FnMut(f32),
) -> Result<Option<Passthrough>, DvdError> {
    if crate::demux_mkv::is_matroska(path) {
        let samples = crate::demux_mkv::demux_h264(path, cache_dir, format, progress)?;
        return Ok(samples.map(Passthrough::Samples));
    }
    if crate::demux_mp4::is_mp4(path) {
        let samples = crate::demux_mp4::demux_h264(path, cache_dir, format, progress)?;
        return Ok(samples.map(Passthrough::Samples));
    }
    if !is_container(path) {
        return Ok(None);
    }
    let Some((source, file)) = open(path) else {
        return Ok(None);
    };
    let Some(index) = file.find(Kind::Video, |codec| *codec == Codec::H264) else {
        return Ok(None);
    };
    let Some(stream) = file.streams.get(index) else {
        return Ok(None);
    };
    if !h264_fits(path, &source, &file, index, format) {
        return Ok(None);
    }
    let Some(origin) = video_origin(&file) else {
        return Ok(None);
    };

    // M6.367: left in the file; what an earlier version copied into the cache goes.
    let key = format!("{}-{}", audio_cache_key(path)?, format.key());
    for extension in ["h264", "idx", "ok"] {
        let _ = fs::remove_file(cache_dir.join(format!("{key}.passthrough.{extension}")));
    }
    let unsupported =
        |what: String| DvdError::Unsupported(format!("demux {}: {what}", path.display()));
    let mut units = Vec::with_capacity(stream.units.len());
    let mut offset = 0u64;
    for unit in &stream.units {
        let pts = unit
            .pts
            .ok_or_else(|| unsupported("a picture has no time".to_string()))?;
        units.push(disccraft_transcode::hd::AccessUnit {
            offset,
            len: unit.size,
            pts: pts - origin,
            dts: unit.dts.unwrap_or(pts) - origin,
            keyframe: unit.key,
        });
        offset += u64::from(unit.size);
    }
    progress(1.0);
    let picks = (0..stream.units.len()).collect();
    drop(source);
    Ok(Some(Passthrough::Transport(TsUnits {
        path: path.to_path_buf(),
        index: Arc::clone(&file),
        stream: index,
        picks,
        units,
    })))
}

/// [`demux_h264_for_bd`] as it was before M6.367, copying the stream into the cache: kept for the tests that
/// compare the two.
#[cfg(test)]
fn copy_h264_for_bd(path: &Path, cache_dir: &Path) -> Result<(PathBuf, PathBuf), DvdError> {
    let (source, file) = open(path).ok_or_else(|| DvdError::Invalid("not a stream".to_string()))?;
    let index = file
        .find(Kind::Video, |codec| *codec == Codec::H264)
        .ok_or_else(|| DvdError::Invalid("no H.264".to_string()))?;
    let stream = file
        .streams
        .get(index)
        .ok_or_else(|| DvdError::Invalid("no H.264".to_string()))?;
    let origin = video_origin(&file).ok_or_else(|| DvdError::Invalid("no origin".to_string()))?;
    fs::create_dir_all(cache_dir).map_err(|err| crate::io_error(cache_dir, err))?;
    let key = audio_cache_key(path)?;
    let es = cache_dir.join(format!("{key}.copied.h264"));
    let index_path = cache_dir.join(format!("{key}.copied.idx"));
    let marker = cache_dir.join(format!("{key}.copied.ok"));
    let unsupported =
        |what: String| DvdError::Unsupported(format!("demux {}: {what}", path.display()));
    let number = part_number();
    let es_part = cache_dir.join(format!("{key}.{number}.h264.part"));
    let index_part = cache_dir.join(format!("{key}.{number}.idx.part"));
    let write = || -> Result<(), DvdError> {
        let created = fs::File::create(&es_part).map_err(|err| crate::io_error(&es_part, err))?;
        let mut writer = std::io::BufWriter::with_capacity(1 << 20, created);
        let mut units = Vec::with_capacity(stream.units.len());
        let mut offset = 0u64;
        for (number, unit) in stream.units.iter().enumerate() {
            let bytes = file
                .read_unit(&source, index, number)
                .map_err(|err| unsupported(err.to_string()))?;
            writer
                .write_all(&bytes)
                .map_err(|err| crate::io_error(&es_part, err))?;
            let pts = unit
                .pts
                .ok_or_else(|| unsupported("a picture has no time".to_string()))?;
            let len = u32::try_from(bytes.len())
                .map_err(|_| unsupported("a picture is too large".to_string()))?;
            units.push(disccraft_transcode::hd::AccessUnit {
                offset,
                len,
                pts: pts - origin,
                dts: unit.dts.unwrap_or(pts) - origin,
                keyframe: unit.key,
            });
            offset += u64::from(len);
        }
        writer
            .flush()
            .map_err(|err| crate::io_error(&es_part, err))?;
        disccraft_transcode::hd::write_h264_index(&index_part, &units)
            .map_err(|err| unsupported(err.to_string()))
    };
    if let Err(err) = write() {
        fs::remove_file(&es_part).unwrap_or(());
        fs::remove_file(&index_part).unwrap_or(());
        return Err(err);
    }
    fs::rename(&es_part, &es).map_err(|err| crate::io_error(&es_part, err))?;
    fs::rename(&index_part, &index_path).map_err(|err| crate::io_error(&index_part, err))?;
    fs::write(&marker, b"ok").map_err(|err| crate::io_error(&marker, err))?;
    Ok((es, index_path))
}

/// Whether stream `index` of `file` is H.264 a Blu-ray HD title of `format` can carry as it is.
fn h264_fits(
    path: &Path,
    source: &ReadSeekSource<fs::File>,
    file: &File,
    index: usize,
    format: BdVideoFormat,
) -> bool {
    let Some(stream) = file.streams.get(index) else {
        return false;
    };
    let Some(first) = stream.units.first() else {
        return false;
    };
    if !first.key || stream.units.iter().any(|unit| unit.pts.is_none()) {
        return false;
    }
    // Profile and level from the sequence parameter set at the start.
    let Some(sps) = file
        .read_unit(source, index, 0)
        .ok()
        .and_then(|bytes| disccraft_transcode::h264_sps::first_in_annex_b(&bytes))
    else {
        return false;
    };
    if !sps_fits(&sps, format) {
        return false;
    }
    // Size, rate and average bitrate as the media reader sees them.
    let Ok(probe) = disccraft_transcode::probe_video(path) else {
        return false;
    };
    let rate = f64::from(format.rate.0) / f64::from(format.rate.1.max(1));
    let bytes: u64 = stream.units.iter().map(|unit| u64::from(unit.size)).sum();
    width_fits(probe.width, format)
        && probe.height == format.height
        && (probe.fps - rate).abs() <= rate * 0.005
        && probe.seconds > 0.0
        && bytes as f64 * 8.0 / probe.seconds <= BD_MAX_VIDEO_BITRATE
}

#[cfg(test)]
mod direct_tests {
    use super::{BdVideoFormat, Passthrough, copy_h264_for_bd, demux_h264_for_bd};
    use std::path::PathBuf;

    /// M6.367: H.264 left in its transport stream gives, unit for unit, the access units and bytes that the copy
    /// into the cache had; a trimmed part reads its own units.
    #[test]
    fn units_read_from_the_stream_are_the_copied_ones() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/fixtures/h264_1080i_tones.m2ts");
        if !fixture.is_file() {
            eprintln!("h264_1080i_tones.m2ts fixture not found; skipping");
            return Ok(());
        }
        let dir = std::env::temp_dir().join(format!("disccraft-ts-direct-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let format =
            BdVideoFormat::of_video(disccraft_project::BdVideo::Hd1080i59).ok_or("no format")?;
        let Some(Passthrough::Transport(ts)) =
            demux_h264_for_bd(&fixture, &dir, format, &mut |_| {})?
        else {
            return Err("not left in the transport stream".into());
        };
        // Nothing of the video was written to the cache.
        let written = std::fs::read_dir(&dir).map_or(0, |entries| entries.count());
        assert_eq!(written, 0);
        let (es, index) = copy_h264_for_bd(&fixture, &dir)?;
        let copied = disccraft_transcode::hd::read_h264_index(&index)?;
        assert_eq!(ts.units(), copied.as_slice());
        assert!(copied.len() >= 60, "{}", copied.len());
        let data = std::fs::read(&es)?;
        let mut file = std::fs::File::open(&fixture)?;
        let bytes_of = |unit: &disccraft_transcode::hd::AccessUnit| {
            let start = usize::try_from(unit.offset).unwrap_or(usize::MAX);
            data.get(start..start.saturating_add(unit.len as usize))
                .map(<[u8]>::to_vec)
        };
        for (number, unit) in copied.iter().enumerate() {
            assert_eq!(
                Some(ts.read(&mut file, number)?),
                bytes_of(unit),
                "{number}"
            );
        }
        let (second, third) = (
            copied.get(2).cloned().ok_or("short")?,
            copied.get(3).cloned().ok_or("short")?,
        );
        let part = ts.keep(&[2, 3], vec![second, third.clone()]);
        assert_eq!(Some(part.read(&mut file, 1)?), bytes_of(&third));
        assert!(part.read(&mut file, 2).is_err());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
