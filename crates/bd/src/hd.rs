//! HD titles (ROADMAP M10): a timeline's video, or a slideshow's pictures (M10.6), encoded to H.264 at the disc's
//! HD format (`BdVideo`), in place of its SD MPEG-2 video. The audio, chapters and timing stay those of the title's SD streams; subtitles are drawn
//! again at the HD size.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use disccraft_dvd::{SubtitleTrack, TitleSource, TitleStreams};
use disccraft_project::{Project, TimelineId};
use disccraft_transcode::hd::{AccessUnit, HdOptions, read_h264_index, transcode_h264};

use crate::{BdError, invalid, io_error};

/// Target bitrate of an HD title, kilobits per second.
const HD_TARGET_KBPS: u32 = 12_000;
/// Peak bitrate of an HD title, kilobits per second (Encore's default A/V maximum is 15 Mbps; a Blu-ray disc's
/// video may go to 40).
const HD_MAX_KBPS: u32 = 24_000;

/// A title's HD video.
pub(crate) struct HdTitle {
    /// The H.264 elementary stream (Annex B).
    pub(crate) data: Vec<u8>,
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
            let data = std::fs::read(&es).map_err(|error| io_error(&es, error))?;
            return Ok(Some(HdTitle {
                data,
                units,
                width: size(width)?,
                height: size(height)?,
                rate: (fps_num, fps_den),
                subtitles: Vec::new(),
            }));
        }
    };
    let timeline = project
        .timelines
        .get(&TimelineId(id))
        .ok_or_else(|| invalid("timeline does not exist"))?;
    let asset = project
        .assets
        .get(&timeline.video)
        .ok_or_else(|| invalid("timeline video does not exist"))?;
    let source = Path::new(&asset.path);
    let (es, index) = cached_paths(cache_dir, source, project.settings.bd_video.id())?;
    if !(es.is_file() && index.is_file()) {
        std::fs::create_dir_all(cache_dir).map_err(|error| io_error(cache_dir, error))?;
        let es_part = es.with_extension("h264.part");
        let index_part = es.with_extension("idx.part");
        let label = format!("HD: {}", asset.name);
        transcode_h264(source, &es_part, &index_part, &options, &mut |fraction| {
            progress(&label, fraction);
        })
        .map_err(|error| BdError::Prepare(error.to_string()))?;
        std::fs::rename(&es_part, &es).map_err(|error| io_error(&es, error))?;
        std::fs::rename(&index_part, &index).map_err(|error| io_error(&index, error))?;
    }
    let units = read_h264_index(&index).map_err(|error| BdError::Prepare(error.to_string()))?;
    let data = std::fs::read(&es).map_err(|error| io_error(&es, error))?;
    let subtitles = disccraft_dvd::subtitle_tracks_sized(
        project,
        timeline,
        title.first_pts,
        (size(width)?, size(height)?),
    )
    .map_err(|error| BdError::Prepare(error.to_string()))?;
    Ok(Some(HdTitle {
        data,
        units,
        width: size(width)?,
        height: size(height)?,
        rate: (fps_num, fps_den),
        subtitles,
    }))
}

/// Where the HD encode of `source` in `format` is kept.
fn cached_paths(
    cache_dir: &Path,
    source: &Path,
    format: &str,
) -> Result<(PathBuf, PathBuf), BdError> {
    let metadata = std::fs::metadata(source).map_err(|error| io_error(source, error))?;
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
            data: Vec::new(),
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
}
