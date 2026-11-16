//! HD titles (ROADMAP M10): a timeline's video, or a slideshow's pictures (M10.6), encoded to H.264 at the disc's
//! HD format (`BdVideo`), in place of its SD MPEG-2 video. The audio, chapters and timing stay those of the title's SD streams; subtitles are drawn
//! again at the HD size.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use disccraft_dvd::{SubtitleTrack, TitleSource, TitleStreams};
use disccraft_project::Tick;
use disccraft_project::{Project, TimelineId};
use disccraft_transcode::hd::{AccessUnit, HdOptions, read_h264_index, transcode_h264_range};

/// Ticks in a second (the project's time base).
const TICKS_PER_SECOND: i64 = 254_016_000_000;

use crate::{BdError, invalid, io_error};

/// Target bitrate of an HD title, kilobits per second.
const HD_TARGET_KBPS: u32 = 12_000;
/// Peak bitrate of an HD title, kilobits per second (Encore's default A/V maximum is 15 Mbps; a Blu-ray disc's
/// video may go to 40).
const HD_MAX_KBPS: u32 = 24_000;

/// Reads an HD title's access units from its cached stream one at a time, as the multiplexer writes them.
pub(crate) struct HdReader {
    file: std::sync::Mutex<std::fs::File>,
    /// Each unit's offset and length.
    units: Vec<(u64, u32)>,
}

impl disccraft_m2tsmux::UnitData for HdReader {
    fn read(&self, unit: usize) -> std::io::Result<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let (offset, len) = self.units.get(unit).copied().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("no unit {unit}"))
        })?;
        let mut file = self
            .file
            .lock()
            .map_err(|_| std::io::Error::other("the HD stream reader was poisoned"))?;
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0u8; len as usize];
        file.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    fn len(&self, unit: usize) -> usize {
        self.units.get(unit).map_or(0, |(_, len)| *len as usize)
    }
}

/// A title's HD video.
pub(crate) struct HdTitle {
    /// The cached H.264 elementary stream (Annex B), read a unit at a time as the clip is written.
    pub(crate) stream: PathBuf,
    /// Its access units, time stamps in 90 kHz from 0.
    pub(crate) units: Vec<AccessUnit>,
    /// Frame size.
    pub(crate) width: u16,
    /// Frame height.
    pub(crate) height: u16,
    /// Frame rate (numerator, denominator).
    pub(crate) rate: (u32, u32),
    /// Subtitle tracks drawn at the HD size.
    pub(crate) subtitles: Vec<SubtitleTrack>,
}

impl HdTitle {
    /// A reader of its access units for the multiplexer.
    pub(crate) fn reader(&self) -> Result<HdReader, BdError> {
        let file =
            std::fs::File::open(&self.stream).map_err(|error| io_error(&self.stream, error))?;
        Ok(HdReader {
            file: std::sync::Mutex::new(file),
            units: self
                .units
                .iter()
                .map(|unit| (unit.offset, unit.len))
                .collect(),
        })
    }

    /// The BD video format code (5 for 720p, 6 for 1080p).
    pub(crate) fn video_format(&self) -> u8 {
        if self.height >= 1080 { 6 } else { 5 }
    }

    /// The BD frame rate code (1 23.976, 2 24, 3 25, 6 50, 7 59.94).
    pub(crate) fn frame_rate(&self) -> u8 {
        match self.rate {
            (24_000, 1001) => 1,
            (24, 1) => 2,
            (25, 1) => 3,
            (50, 1) => 6,
            (60_000, 1001) => 7,
            _ => 4,
        }
    }

    /// One frame in 90 kHz units.
    pub(crate) fn frame_90k(&self) -> i64 {
        let (num, den) = self.rate;
        i64::from(den)
            .saturating_mul(90_000)
            .checked_div(i64::from(num.max(1)))
            .unwrap_or(3003)
    }
}

/// The HD video of `title` when the disc's titles are HD: a timeline's video or a slideshow's pictures, encoded
/// once and kept in `cache_dir` (a timeline's by the video file, its size and time, and the format).
pub(crate) fn hd_title(
    project: &Project,
    title: &TitleStreams,
    cache_dir: &Path,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<Option<HdTitle>, BdError> {
    let Some((width, height, fps_num, fps_den)) = project.settings.bd_video.hd() else {
        return Ok(None);
    };
    let options = HdOptions {
        width,
        height,
        fps_num,
        fps_den,
        target_kbps: HD_TARGET_KBPS,
        max_kbps: HD_MAX_KBPS,
    };
    let size = |value: u32| u16::try_from(value).map_err(|_| invalid("HD size too large"));
    let id = match title.source {
        TitleSource::Timeline(id) => id,
        TitleSource::Slideshow(id) => {
            let slideshow = project
                .slideshows
                .get(&id)
                .ok_or_else(|| invalid("slideshow does not exist"))?;
            let (es, index) = disccraft_dvd::encode_hd_slideshow(
                project, slideshow, cache_dir, &options, progress,
            )
            .map_err(|error| BdError::Prepare(error.to_string()))?;
            let units =
                read_h264_index(&index).map_err(|error| BdError::Prepare(error.to_string()))?;
            // The slides' subtitles (Create Subtitle), drawn for the HD frame.
            let subtitles = disccraft_dvd::slide_subtitles_sized(
                project,
                slideshow,
                title,
                Some((size(width)?, size(height)?)),
            )
            .map_err(|error| BdError::Prepare(error.to_string()))?;
            return Ok(Some(HdTitle {
                stream: es,
                units,
                width: size(width)?,
                height: size(height)?,
                rate: (fps_num, fps_den),
                subtitles,
            }));
        }
    };
    let timeline = project
        .timelines
        .get(&TimelineId(id))
        .ok_or_else(|| invalid("timeline does not exist"))?;
    // Each video clip is encoded by itself (its trimmed part only) and kept in the cache; the title is the clips
    // one after another. Every encode starts with an IDR picture and its parameter sets, so the join decodes.
    let standard_rate = if project.settings.standard == disccraft_project::VideoStandard::Ntsc {
        (30_000_u32, 1001_u32)
    } else {
        (25, 1)
    };
    let videos = timeline.videos();
    let mut parts = Vec::with_capacity(videos.len());
    let mut single = None;
    for (clip, video) in videos.iter().enumerate() {
        let asset = project
            .assets
            .get(video)
            .ok_or_else(|| invalid("timeline video does not exist"))?;
        let media = asset.media_file();
        let source: &Path = &media;
        let trim = timeline.video_trim(clip);
        // The empty stretch in front of the clip plays black.
        let gap = timeline.video_gap(clip);
        if gap > 0 {
            parts.push(black_part(
                cache_dir,
                gap_frames(gap, standard_rate, (fps_num, fps_den)),
                project.settings.bd_video.id(),
                &options,
            )?);
        }
        // A Blu-ray's own H.264 (an .m2ts) that fits the disc goes on it as it is (a clip used whole); anything else
        // is encoded.
        let whole = trim.start == 0 && trim.end.is_none();
        let passthrough = if whole {
            disccraft_dvd::demux_h264_for_bd(source, cache_dir, (width, height, fps_num, fps_den))
                .map_err(|error| BdError::Prepare(error.to_string()))?
        } else {
            None
        };
        let (es, index) = match passthrough {
            Some(paths) => paths,
            None => cached_paths(cache_dir, source, project.settings.bd_video.id(), trim)?,
        };
        if !(es.is_file() && index.is_file()) {
            std::fs::create_dir_all(cache_dir).map_err(|error| io_error(cache_dir, error))?;
            let es_part = es.with_extension("h264.part");
            let index_part = es.with_extension("idx.part");
            let label = format!("HD: {}", asset.name);
            let range = (
                trim_tick(trim.start, standard_rate)?,
                trim.end
                    .map(|end| trim_tick(end, standard_rate))
                    .transpose()?,
            );
            let _ = transcode_h264_range(
                source,
                &es_part,
                &index_part,
                &options,
                range,
                &mut |fraction| {
                    progress(&label, fraction);
                },
            )
            .map_err(|error| BdError::Prepare(error.to_string()))?;
            std::fs::rename(&es_part, &es).map_err(|error| io_error(&es, error))?;
            std::fs::rename(&index_part, &index).map_err(|error| io_error(&index, error))?;
        }
        let units = read_h264_index(&index).map_err(|error| BdError::Prepare(error.to_string()))?;
        // A title of one whole clip is read from its cache file as the clip is written (never held in memory).
        if videos.len() == 1 && gap == 0 {
            single = Some((es, units));
            break;
        }
        let data = std::fs::read(&es).map_err(|error| io_error(&es, error))?;
        parts.push((data, units));
    }
    let (es, units) = match single {
        Some(single) => single,
        None => {
            // Several clips (or gaps): joined into one stream, kept in the cache for the multiplexer to read.
            let (data, units) = join_hd_clips(parts, (fps_num, fps_den))?;
            let joined = cache_dir.join(format!(
                "hd-joined-{}-{}.h264",
                timeline.id.0,
                project.settings.bd_video.id()
            ));
            std::fs::create_dir_all(cache_dir).map_err(|error| io_error(cache_dir, error))?;
            std::fs::write(&joined, &data).map_err(|error| io_error(&joined, error))?;
            (joined, units)
        }
    };
    let subtitles = disccraft_dvd::subtitle_tracks_sized(
        project,
        timeline,
        title.first_pts,
        (size(width)?, size(height)?),
    )
    .map_err(|error| BdError::Prepare(error.to_string()))?;
    Ok(Some(HdTitle {
        stream: es,
        units,
        width: size(width)?,
        height: size(height)?,
        rate: (fps_num, fps_den),
        subtitles,
    }))
}

/// A gap of `gap` frames of the disc's standard rate, as frames of the HD rate (the nearest count, at least one).
fn gap_frames(gap: u32, standard: (u32, u32), hd: (u32, u32)) -> u64 {
    let numerator = u128::from(gap) * u128::from(standard.1) * u128::from(hd.0);
    let denominator = (u128::from(standard.0) * u128::from(hd.1)).max(1);
    u64::try_from((numerator + denominator / 2) / denominator)
        .unwrap_or(u64::MAX)
        .max(1)
}

/// `frames` black pictures in the HD format, encoded once and kept in the cache (by their number and the format).
fn black_part(
    cache_dir: &Path,
    frames: u64,
    format: &str,
    options: &HdOptions,
) -> Result<(Vec<u8>, Vec<AccessUnit>), BdError> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    "black".hash(&mut hasher);
    frames.hash(&mut hasher);
    format.hash(&mut hasher);
    HD_TARGET_KBPS.hash(&mut hasher);
    HD_MAX_KBPS.hash(&mut hasher);
    let key = hasher.finish();
    let es = cache_dir.join(format!("hd-{key:016x}.h264"));
    let index = cache_dir.join(format!("hd-{key:016x}.idx"));
    if !(es.is_file() && index.is_file()) {
        std::fs::create_dir_all(cache_dir).map_err(|error| io_error(cache_dir, error))?;
        let es_part = es.with_extension("h264.part");
        let index_part = es.with_extension("idx.part");
        let _ = disccraft_transcode::hd::encode_h264_black(frames, &es_part, &index_part, options)
            .map_err(|error| BdError::Prepare(error.to_string()))?;
        std::fs::rename(&es_part, &es).map_err(|error| io_error(&es, error))?;
        std::fs::rename(&index_part, &index).map_err(|error| io_error(&index, error))?;
    }
    let units = read_h264_index(&index).map_err(|error| BdError::Prepare(error.to_string()))?;
    let data = std::fs::read(&es).map_err(|error| io_error(&es, error))?;
    Ok((data, units))
}

/// The time of a trim point: `frame` frames of the disc's standard rate (a clip's trims count those).
fn trim_tick(frame: u32, rate: (u32, u32)) -> Result<Tick, BdError> {
    if rate.0 == 0 {
        return Err(invalid("frame rate is zero"));
    }
    let ticks = i128::from(frame)
        .checked_mul(i128::from(TICKS_PER_SECOND))
        .and_then(|value| value.checked_mul(i128::from(rate.1)))
        .map(|value| value / i128::from(rate.0))
        .and_then(|value| i64::try_from(value).ok())
        .ok_or_else(|| invalid("trim point out of range"))?;
    Ok(Tick(ticks))
}

/// The frame number of a 90 kHz time at `rate`, rounded to the nearest frame (times are whole frames rounded down).
fn frame_of_90k(time: i64, rate: (u32, u32)) -> Option<i64> {
    let denominator = i128::from(rate.1).checked_mul(90_000)?;
    if denominator == 0 {
        return None;
    }
    let numerator = i128::from(time).checked_mul(i128::from(rate.0))?;
    let frame = numerator
        .checked_mul(2)?
        .checked_add(denominator)?
        .div_euclid(denominator.checked_mul(2)?);
    i64::try_from(frame).ok()
}

/// The 90 kHz time of a frame number at `rate`, rounded down.
fn frame_to_90k(frame: i64, rate: (u32, u32)) -> Option<i64> {
    if rate.0 == 0 {
        return None;
    }
    let value = i128::from(frame)
        .checked_mul(90_000)?
        .checked_mul(i128::from(rate.1))?
        .div_euclid(i128::from(rate.0));
    i64::try_from(value).ok()
}

/// Encoded clips one after another as one stream: the bytes appended, each clip's access units moved behind the
/// pictures before it (times stay on the frame grid of `rate`).
fn join_hd_clips(
    parts: Vec<(Vec<u8>, Vec<AccessUnit>)>,
    rate: (u32, u32),
) -> Result<(Vec<u8>, Vec<AccessUnit>), BdError> {
    let mut data = Vec::new();
    let mut units = Vec::new();
    let mut frames_before = 0_i64;
    for (clip_data, clip_units) in parts {
        let base = u64::try_from(data.len()).map_err(|_| invalid("HD video too large"))?;
        let moved = |time: i64| {
            frame_of_90k(time, rate)
                .and_then(|frame| frame.checked_add(frames_before))
                .and_then(|frame| frame_to_90k(frame, rate))
                .ok_or_else(|| invalid("HD time stamp out of range"))
        };
        for unit in &clip_units {
            units.push(AccessUnit {
                offset: unit
                    .offset
                    .checked_add(base)
                    .ok_or_else(|| invalid("HD unit offset too large"))?,
                len: unit.len,
                pts: moved(unit.pts)?,
                dts: moved(unit.dts)?,
                keyframe: unit.keyframe,
            });
        }
        frames_before = i64::try_from(clip_units.len())
            .ok()
            .and_then(|count| frames_before.checked_add(count))
            .ok_or_else(|| invalid("too many HD pictures"))?;
        data.extend_from_slice(&clip_data);
    }
    Ok((data, units))
}

/// Where the HD encode of `source` in `format` is kept (of its trimmed part when `trim` is not the whole clip).
fn cached_paths(
    cache_dir: &Path,
    source: &Path,
    format: &str,
    trim: disccraft_project::ClipTrim,
) -> Result<(PathBuf, PathBuf), BdError> {
    let metadata =
        disccraft_transcode::document_metadata(source).map_err(|error| io_error(source, error))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_secs());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    modified.hash(&mut hasher);
    format.hash(&mut hasher);
    if trim != disccraft_project::ClipTrim::default() {
        trim.start.hash(&mut hasher);
        trim.end.hash(&mut hasher);
    }
    // The encode's version (2: frame counts round to the nearest frame).
    2u8.hash(&mut hasher);
    HD_TARGET_KBPS.hash(&mut hasher);
    HD_MAX_KBPS.hash(&mut hasher);
    let key = hasher.finish();
    Ok((
        cache_dir.join(format!("hd-{key:016x}.h264")),
        cache_dir.join(format!("hd-{key:016x}.idx")),
    ))
}

#[cfg(test)]
mod tests {
    use super::HdTitle;

    fn title(rate: (u32, u32), height: u16) -> HdTitle {
        HdTitle {
            stream: std::path::PathBuf::new(),
            units: Vec::new(),
            width: 1920,
            height,
            rate,
            subtitles: Vec::new(),
        }
    }

    #[test]
    fn every_hd_format_has_its_codes() {
        for video in disccraft_project::BdVideo::ALL {
            let Some((width, height, num, den)) = video.hd() else {
                continue;
            };
            let hd = HdTitle {
                width: u16::try_from(width).unwrap_or(0),
                ..title((num, den), u16::try_from(height).unwrap_or(0))
            };
            assert!(matches!(hd.video_format(), 5 | 6), "{video:?}");
            let expected = match (num, den) {
                (24_000, 1001) => 1,
                (24, 1) => 2,
                (25, 1) => 3,
                (30_000, 1001) => 4,
                (50, 1) => 6,
                _ => 7,
            };
            assert_eq!(hd.frame_rate(), expected, "{video:?}");
        }
    }

    #[test]
    fn formats_and_rates_have_their_codes() {
        assert_eq!(title((24_000, 1001), 1080).video_format(), 6);
        assert_eq!(title((24_000, 1001), 1080).frame_rate(), 1);
        assert_eq!(title((24_000, 1001), 1080).frame_90k(), 3753);
        assert_eq!(title((60_000, 1001), 720).video_format(), 5);
        assert_eq!(title((60_000, 1001), 720).frame_rate(), 7);
        assert_eq!(title((50, 1), 720).frame_rate(), 6);
        assert_eq!(title((24, 1), 1080).frame_90k(), 3750);
    }

    /// Clips of a timeline follow each other: offsets after the bytes before, times after the pictures before, on
    /// the frame grid (23.976: 3753.75 ticks a frame).
    #[test]
    fn joined_clips_keep_the_frame_grid() -> Result<(), Box<dyn std::error::Error>> {
        use disccraft_transcode::hd::AccessUnit;
        let rate = (24_000, 1001);
        let unit = |offset: u64, pts: i64, dts: i64, keyframe: bool| AccessUnit {
            offset,
            len: 4,
            pts,
            dts,
            keyframe,
        };
        // Three pictures (times 0, 3753, 7507; decode one frame early), then two.
        let first = vec![
            unit(0, 0, -3754, true),
            unit(4, 7507, 0, false),
            unit(8, 3753, 3753, false),
        ];
        let second = vec![unit(0, 0, -3754, true), unit(4, 3753, 0, false)];
        let (data, units) =
            super::join_hd_clips(vec![(vec![1; 12], first), (vec![2; 8], second)], rate)?;
        assert_eq!(data.len(), 20);
        let offsets: Vec<u64> = units.iter().map(|unit| unit.offset).collect();
        assert_eq!(offsets, [0, 4, 8, 12, 16]);
        let pts: Vec<i64> = units.iter().map(|unit| unit.pts).collect();
        assert_eq!(pts, [0, 7507, 3753, 11_261, 15_015]);
        let dts: Vec<i64> = units.iter().map(|unit| unit.dts).collect();
        assert_eq!(dts, [-3754, 0, 3753, 7507, 11_261]);
        assert!(units.get(3).is_some_and(|unit| unit.keyframe));
        Ok(())
    }

    /// A gap counted in the disc's frames becomes the nearest number of HD frames.
    #[test]
    fn a_gap_is_counted_in_hd_frames() {
        // 45 frames of 29.97 are 1.5015 s: 36 frames of 23.976.
        assert_eq!(super::gap_frames(45, (30_000, 1001), (24_000, 1001)), 36);
        assert_eq!(super::gap_frames(25, (25, 1), (50, 1)), 50);
        assert_eq!(super::gap_frames(1, (30_000, 1001), (24_000, 1001)), 1);
    }

    /// A trim point counts frames of the disc's standard rate.
    #[test]
    fn a_trim_point_is_a_time_of_the_standard_rate() -> Result<(), Box<dyn std::error::Error>> {
        let second = super::TICKS_PER_SECOND;
        assert_eq!(super::trim_tick(25, (25, 1))?.0, second);
        assert_eq!(super::trim_tick(30_000, (30_000, 1001))?.0, second * 1001);
        Ok(())
    }
}
