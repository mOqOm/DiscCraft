//! The bitrate of a Blu-ray project's HD video (ROADMAP M10.15): the format's usual rate, or less when the titles
//! would not fit the disc at it. Every title's video is encoded in HD (a timeline's clips with their trims and
//! gaps, a slideshow's slides), so the video's time is the titles' time; the sound keeps its own size. A clip
//! whose own H.264 goes on the disc as it is (M10.16) is not encoded: it takes its file's size.

use std::path::Path;

use disccraft_project::{DiscKind, Project, Timeline, TimelineId, VideoStandard};

use super::{
    AC3_STEREO_BITRATE, DvdError, asset_path, audio_bitrate_for_title, capacity_bits,
    positive_seconds, slide_seconds,
};

/// The usual target and peak bitrate of H.264 HD video, kilobits per second (Encore's default A/V maximum is
/// 15 Mbps; a Blu-ray's video may go to 40).
const H264_KBPS: (u32, u32) = (12_000, 24_000);
/// The usual target and peak bitrate of MPEG-2 HD video (1080i; I and P pictures), kilobits per second: MPEG-2
/// needs about twice H.264's rate. A BD-25 holds a little over two hours at the target.
const MPEG2_KBPS: (u32, u32) = (25_000, 35_000);
/// The lowest target an HD title is encoded at, kilobits per second: below it the titles are too long for the
/// disc.
const H264_FLOOR_KBPS: u32 = 3_000;
/// The lowest target of MPEG-2 HD video, kilobits per second.
const MPEG2_FLOOR_KBPS: u32 = 8_000;
/// What a transport stream adds to its elementary streams (the 192-byte source packets, PES headers, tables),
/// as parts in a thousand.
const MUX_OVERHEAD_PER_MILLE: f64 = 1040.0;
/// Targets are whole multiples of this, kilobits per second, so a small change of a title's length keeps the
/// cached encodes.
const STEP_KBPS: u32 = 500;

/// The bitrates of a Blu-ray project's HD video and what they were worked out from.
#[derive(Debug, Clone, PartialEq)]
pub struct HdBudget {
    /// Target (average) video bitrate, kilobits per second.
    pub target_kbps: u32,
    /// Peak video bitrate, kilobits per second.
    pub max_kbps: u32,
    /// The titles' video that is encoded, seconds.
    pub video_seconds: f64,
    /// The titles' sound, bits, with the video that goes on the disc as it is (`passthrough_bits`).
    pub audio_bits: f64,
    /// The video that goes on the disc as it is, bits (its files' sizes).
    pub passthrough_bits: f64,
    /// Each timeline's encoded seconds and its other bits (sound, video kept as it is), in timeline order.
    pub timelines: Vec<(TimelineId, f64, f64)>,
}

impl HdBudget {
    /// The bytes the titles take on the disc at the target bitrate (video and sound, multiplexed).
    #[must_use]
    pub fn estimated_bytes(&self) -> u64 {
        multiplexed_bytes(self.video_seconds, self.audio_bits, self.target_kbps)
    }

    /// The bytes one timeline of `seconds` and `audio_bits` takes.
    #[must_use]
    pub fn timeline_bytes(&self, seconds: f64, audio_bits: f64) -> u64 {
        multiplexed_bytes(seconds, audio_bits, self.target_kbps)
    }
}

fn multiplexed_bytes(seconds: f64, audio_bits: f64, target_kbps: u32) -> u64 {
    let bits = seconds * f64::from(target_kbps) * 1000.0 + audio_bits;
    let bytes = bits * MUX_OVERHEAD_PER_MILLE / 1000.0 / 8.0;
    if bytes.is_finite() && bytes > 0.0 {
        bytes as u64
    } else {
        0
    }
}

/// The usual target and peak bitrate (kilobits per second) of HD video: MPEG-2 (`mpeg2`, the interlaced formats)
/// or H.264.
#[must_use]
pub fn hd_default_kbps(mpeg2: bool) -> (u32, u32) {
    if mpeg2 { MPEG2_KBPS } else { H264_KBPS }
}

/// The bitrates of `project`'s HD video: the format's usual ones, or a lower target when the titles would not fit
/// the disc at it. `None` when the project is not a Blu-ray in an HD format.
///
/// # Errors
///
/// Unreadable media, and [`DvdError::Invalid`] when the titles do not fit the disc even at the lowest bitrate.
pub fn hd_budget(project: &Project) -> Result<Option<HdBudget>, DvdError> {
    let probe = |path: &Path| {
        super::probe_seconds(path)
            .map_err(|err| DvdError::Unsupported(format!("probe {}: {err}", path.display())))
    };
    hd_budget_with_probe(project, &probe)
}

/// [`hd_budget`] with the media durations from `probe`.
pub(crate) fn hd_budget_with_probe(
    project: &Project,
    probe: &dyn Fn(&Path) -> Result<f64, DvdError>,
) -> Result<Option<HdBudget>, DvdError> {
    let Some(format) = crate::BdVideoFormat::of(project) else {
        return Ok(None);
    };
    // A file whose H.264 goes on the disc as it is: its size.
    let kept = |path: &Path| {
        crate::h264_fits_bd(path, format)
            .then(|| std::fs::metadata(path).ok().map(|metadata| metadata.len()))
            .flatten()
    };
    hd_budget_with(project, probe, &kept)
}

/// [`hd_budget_with_probe`] with the size of a file whose video goes on the disc as it is from `kept`.
fn hd_budget_with(
    project: &Project,
    probe: &dyn Fn(&Path) -> Result<f64, DvdError>,
    kept: &dyn Fn(&Path) -> Option<u64>,
) -> Result<Option<HdBudget>, DvdError> {
    if project.settings.kind != DiscKind::BluRay || project.settings.bd_video.hd().is_none() {
        return Ok(None);
    }
    let mpeg2 = project.settings.bd_video.interlaced();
    let mut video_seconds = 0.0_f64;
    let mut audio_bits = 0.0_f64;
    let mut passthrough_bits = 0.0_f64;
    let mut timelines = Vec::with_capacity(project.timelines.len());
    for timeline in project.timelines.values() {
        let (seconds, encoded, kept_bytes) =
            timeline_seconds(project, timeline, probe, kept, mpeg2)?;
        let kept_bits = kept_bytes as f64 * 8.0;
        let bits =
            f64::from(audio_bitrate_for_title(project, timeline, seconds)?) * seconds + kept_bits;
        video_seconds += encoded;
        audio_bits += bits;
        passthrough_bits += kept_bits;
        timelines.push((timeline.id, encoded, bits));
    }
    for slideshow in project.slideshows.values() {
        let mut seconds = 0.0_f64;
        for slide in &slideshow.slides {
            seconds += slide_seconds(slide.duration)?;
        }
        video_seconds += seconds;
        audio_bits += f64::from(AC3_STEREO_BITRATE) * seconds;
    }
    if !(video_seconds.is_finite() && audio_bits.is_finite()) {
        return Err(DvdError::Invalid("title duration overflow".to_string()));
    }

    let (usual, max_kbps) = hd_default_kbps(mpeg2);
    let target_kbps = if video_seconds > 0.0 {
        let capacity = capacity_bits(project)? as f64;
        // What is left for the video once the sound is on the disc, both multiplexed.
        let fit_kbps =
            (capacity * 1000.0 / MUX_OVERHEAD_PER_MILLE - audio_bits) / video_seconds / 1000.0;
        let floor = if mpeg2 {
            MPEG2_FLOOR_KBPS
        } else {
            H264_FLOOR_KBPS
        };
        if !fit_kbps.is_finite() || fit_kbps < f64::from(floor) {
            return Err(DvdError::Invalid(format!(
                "the titles are too long for the disc capacity in HD ({video_seconds:.0} s at {:.0} kbit/s; \
                 at least {floor} kbit/s is needed)",
                fit_kbps.max(0.0)
            )));
        }
        if fit_kbps >= f64::from(usual) {
            usual
        } else {
            // Rounded down to a step, never below the floor.
            let steps = (fit_kbps as u32) / STEP_KBPS;
            steps.saturating_mul(STEP_KBPS).max(floor)
        }
    } else {
        usual
    };
    Ok(Some(HdBudget {
        target_kbps,
        max_kbps: max_kbps.max(target_kbps),
        video_seconds,
        audio_bits,
        passthrough_bits,
        timelines,
    }))
}

/// How long `timeline` plays (its clips, each with its trim in frames of the disc's standard rate, and the empty
/// stretches in front of them), how much of that is encoded, and the bytes of the clips whose video goes on the
/// disc as it is: a clip used whole that `kept` knows, on a disc whose encoded video is MPEG-2 (`mpeg2`) only
/// when it is the whole title.
fn timeline_seconds(
    project: &Project,
    timeline: &Timeline,
    probe: &dyn Fn(&Path) -> Result<f64, DvdError>,
    kept: &dyn Fn(&Path) -> Option<u64>,
    mpeg2: bool,
) -> Result<(f64, f64, u64), DvdError> {
    let frame = if project.settings.standard == VideoStandard::Ntsc {
        1001.0 / 30_000.0
    } else {
        1.0 / 25.0
    };
    let mut seconds = 0.0_f64;
    let mut encoded = 0.0_f64;
    let mut kept_bytes = 0u64;
    let videos = timeline.videos();
    let count = videos.len();
    for (clip, video) in videos.into_iter().enumerate() {
        let path = asset_path(project, video)?;
        let path: &Path = &path;
        let whole = positive_seconds(probe(path)?)?;
        let trim = timeline.video_trim(clip);
        let start = (f64::from(trim.start) * frame).min(whole);
        let end = trim
            .end
            .map_or(whole, |end| (f64::from(end) * frame).clamp(start, whole));
        let gap = f64::from(timeline.video_gap(clip)) * frame;
        seconds += end - start + gap;
        let untrimmed = trim.start == 0 && trim.end.is_none();
        let alone = count == 1 && timeline.video_gap(clip) == 0;
        match kept(path).filter(|_| untrimmed && (!mpeg2 || alone)) {
            Some(bytes) => {
                kept_bytes = kept_bytes.saturating_add(bytes);
                encoded += gap;
            }
            None => encoded += end - start + gap,
        }
    }
    Ok((positive_seconds(seconds)?, encoded, kept_bytes))
}

#[cfg(test)]
mod tests {
    use super::{HdBudget, hd_budget_with_probe};
    use crate::DvdError;
    use disccraft_project::{BdVideo, Capacity, ClipTrim, DiscKind, Project};
    use std::path::Path;

    /// A Blu-ray project in `video` with one timeline of `movie.mp4`.
    fn project(video: BdVideo) -> Result<Project, Box<dyn std::error::Error>> {
        let mut project = Project::default();
        project.settings.kind = DiscKind::BluRay;
        project.settings.capacity = Capacity::Bd25;
        project.settings.bd_video = video;
        let movie = project.add_asset(Path::new("movie.mp4"))?;
        let _ = project.add_timeline(movie, None)?;
        Ok(project)
    }

    /// A title whose own H.264 goes on the disc is not encoded: it takes its file's size, and the rest of the
    /// disc is what the other titles' bitrate is fitted to.
    #[test]
    fn video_kept_as_it_is_counts_by_its_size() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = project(BdVideo::Hd1080i59)?;
        let kept = project.add_asset(Path::new("kept.mkv"))?;
        let _ = project.add_timeline(kept, None)?;
        let probe = |_path: &Path| Ok::<f64, DvdError>(3600.0);
        let size = |path: &Path| path.ends_with("kept.mkv").then_some(15_000_000_000_u64);
        let budget = super::hd_budget_with(&project, &probe, &size)?.ok_or("no budget")?;
        // Only movie.mp4's hour is encoded.
        assert!((budget.video_seconds - 3600.0).abs() < 1e-6, "{budget:?}");
        assert!((budget.passthrough_bits - 120e9).abs() < 1.0);
        // 15 GB of the 25 are taken: the other hour no longer gets the usual 25 Mbps.
        assert!(
            budget.target_kbps < 25_000 && budget.target_kbps >= 15_000,
            "{budget:?}"
        );
        assert!(budget.estimated_bytes() > 15_000_000_000);
        assert!(budget.estimated_bytes() <= Capacity::Bd25.bytes());
        // A trimmed clip is encoded.
        let second = *project.timelines.keys().nth(1).ok_or("no timeline")?;
        let timeline = project.timelines.get_mut(&second).ok_or("no timeline")?;
        timeline.video_trims = vec![ClipTrim {
            start: 30,
            end: None,
        }];
        let budget = super::hd_budget_with(&project, &probe, &size)?.ok_or("no budget")?;
        assert!(budget.passthrough_bits.abs() < 1.0, "{budget:?}");
        Ok(())
    }

    fn budget(project: &Project, seconds: f64) -> Result<Option<HdBudget>, DvdError> {
        hd_budget_with_probe(project, &|_path: &Path| Ok::<f64, DvdError>(seconds))
    }

    #[test]
    fn only_an_hd_blu_ray_has_an_hd_budget() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(budget(&project(BdVideo::Sd)?, 60.0)?, None);
        let mut dvd = project(BdVideo::Hd1080p23)?;
        dvd.settings.kind = DiscKind::Dvd;
        assert_eq!(budget(&dvd, 60.0)?, None);
        Ok(())
    }

    #[test]
    fn titles_that_fit_get_the_usual_bitrate() -> Result<(), Box<dyn std::error::Error>> {
        // An hour: 5.4 GB at 12 Mbps, 11 GB at 25 Mbps.
        let h264 = budget(&project(BdVideo::Hd1080p23)?, 3600.0)?.ok_or("no budget")?;
        assert_eq!((h264.target_kbps, h264.max_kbps), (12_000, 24_000));
        let mpeg2 = budget(&project(BdVideo::Hd1080i59)?, 3600.0)?.ok_or("no budget")?;
        assert_eq!((mpeg2.target_kbps, mpeg2.max_kbps), (25_000, 35_000));
        // Video and the AC-3 soundtrack (192 kbit/s), with the transport stream's overhead.
        let expected = (3600.0 * (25_000_000.0 + 192_000.0) * 1.04 / 8.0) as u64;
        assert_eq!(mpeg2.estimated_bytes(), expected);
        assert!(mpeg2.estimated_bytes() < Capacity::Bd25.bytes());
        Ok(())
    }

    #[test]
    fn long_titles_get_a_bitrate_that_fits() -> Result<(), Box<dyn std::error::Error>> {
        // Three hours of 1080i would be 35 GB at 25 Mbps: more than a BD-25.
        let project_1080i = project(BdVideo::Hd1080i59)?;
        let fitted = budget(&project_1080i, 3.0 * 3600.0)?.ok_or("no budget")?;
        assert!(
            fitted.target_kbps < 25_000 && fitted.target_kbps >= 16_000,
            "{fitted:?}"
        );
        assert_eq!(fitted.target_kbps % 500, 0);
        assert!(fitted.max_kbps >= fitted.target_kbps);
        // Inside the 96 % of the disc the budget plans with.
        assert!(fitted.estimated_bytes() <= Capacity::Bd25.bytes() / 100 * 96);
        // The same three hours fit a BD-50 at the usual rate.
        let mut bd50 = project_1080i;
        bd50.settings.capacity = Capacity::Bd50;
        assert_eq!(
            budget(&bd50, 3.0 * 3600.0)?.map(|budget| budget.target_kbps),
            Some(25_000)
        );
        Ok(())
    }

    #[test]
    fn titles_too_long_for_the_disc_are_refused() -> Result<(), Box<dyn std::error::Error>> {
        // Eight hours of 1080i on a BD-25 would need less than 8 Mbps.
        let error = budget(&project(BdVideo::Hd1080i59)?, 8.0 * 3600.0);
        assert!(
            matches!(&error, Err(DvdError::Invalid(text)) if text.contains("too long")),
            "{error:?}"
        );
        // H.264 goes lower: the same eight hours fit.
        let h264 = budget(&project(BdVideo::Hd1080p23)?, 8.0 * 3600.0)?.ok_or("no budget")?;
        assert!((3_000..12_000).contains(&h264.target_kbps), "{h264:?}");
        Ok(())
    }

    #[test]
    fn trims_and_gaps_count() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = project(BdVideo::Hd1080i59)?;
        let id = *project.timelines.keys().next().ok_or("no timeline")?;
        // 100 s of the 3600 s clip (frames 300 to 3297 of 29.97), then 30 frames of black before nothing more.
        let timeline = project.timeline_mut(id).ok_or("no timeline")?;
        timeline.video_trims = vec![ClipTrim {
            start: 300,
            end: Some(3297),
        }];
        timeline.video_gaps = vec![30];
        let budget = budget(&project, 3600.0)?.ok_or("no budget")?;
        let expected = f64::from(3297 - 300 + 30) * 1001.0 / 30_000.0;
        assert!((budget.video_seconds - expected).abs() < 1e-6, "{budget:?}");
        assert_eq!(budget.timelines.len(), 1);
        Ok(())
    }

    /// The disc usage of an HD Blu-ray is its HD video's, whatever the sources would take on a DVD.
    #[test]
    fn the_estimate_of_an_hd_blu_ray_uses_the_hd_bitrate() -> Result<(), Box<dyn std::error::Error>>
    {
        let project = project(BdVideo::Hd1080i59)?;
        let probe = |_path: &Path| Ok::<f64, DvdError>(3600.0);
        let estimate = crate::prepare::estimate_with_probe(&project, &probe)?;
        assert_eq!(estimate.video_bitrate, 25_000_000);
        assert_eq!(estimate.capacity_bytes, Capacity::Bd25.bytes());
        let expected = (3600.0 * (25_000_000.0 + 192_000.0) * 1.04 / 8.0) as u64;
        assert_eq!(estimate.estimated_bytes, expected);
        assert_eq!(
            estimate.timelines.first().map(|(_, bytes)| *bytes),
            Some(expected)
        );
        Ok(())
    }
}
