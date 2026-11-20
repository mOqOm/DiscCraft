//! The bitrate of a Blu-ray project's HD video (ROADMAP M10.15): the format's usual rate, or less when the titles
//! would not fit the disc at it, or more (up to a cap) when the disc has room to spare (M6.377). Every title's video is encoded in HD (a timeline's clips with their trims and
//! gaps, a slideshow's slides), so the video's time is the titles' time; the sound keeps its own size. A clip
//! whose own H.264 goes on the disc as it is (M10.16) is not encoded: it takes its file's size.

use std::path::Path;

use disccraft_project::{DiscKind, Project, Timeline, TimelineId, VideoStandard};

use super::{
    AC3_STEREO_BITRATE, BD_STREAMS_BITRATE, DvdError, asset_path, audio_bitrate_for_title,
    capacity_bits, positive_seconds, slide_seconds,
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
/// The highest target an HD title is raised to when the disc has room (M6.377), kilobits per second: H.264.
const H264_RAISED_KBPS: u32 = 25_000;
/// The highest raised target of MPEG-2 HD video, kilobits per second.
const MPEG2_RAISED_KBPS: u32 = 32_000;
/// The share of the disc's room a raised target plans with: the rate control lands near the target, not on it.
const RAISE_SHARE: f64 = 0.95;
/// The fastest a Blu-ray's video peaks, kilobits per second.
const BD_PEAK_KBPS: u32 = 40_000;

/// The highest target of `mpeg2` (else H.264) HD video when the disc has room.
fn raised_cap(mpeg2: bool) -> u32 {
    if mpeg2 {
        MPEG2_RAISED_KBPS
    } else {
        H264_RAISED_KBPS
    }
}

/// The peak of `mpeg2` (else H.264) HD video at `target`: the usual peak up to the usual target, twice a raised
/// target (no more than a Blu-ray's 40 Mbit/s), and never more than `room` nor less than the target.
fn peak_for(mpeg2: bool, target: u32, room: u32) -> u32 {
    let (usual_target, usual_peak) = hd_default_kbps(mpeg2);
    let peak = if target <= usual_target {
        usual_peak
    } else {
        target.saturating_mul(2).min(BD_PEAK_KBPS).max(usual_peak)
    };
    peak.min(room).max(target)
}

/// The bitrates of a Blu-ray project's HD video and what they were worked out from.
#[derive(Debug, Clone, PartialEq)]
pub struct HdBudget {
    /// Target (average) video bitrate, kilobits per second, of the project's format's codec.
    pub target_kbps: u32,
    /// Peak video bitrate, kilobits per second.
    pub max_kbps: u32,
    /// The project's format is coded as MPEG-2 (the rates above are MPEG-2's), else H.264.
    pub mpeg2: bool,
    /// The disc's codec setting, which chooses each title's codec from its format (M12.28).
    pub codec: disccraft_project::BdCodec,
    /// The titles' video that is encoded, seconds (a title in a format of the other codec counted by how much more
    /// or less it takes at its codec's usual rate: M6.325).
    pub video_seconds: f64,
    /// The titles' sound, bits, with the video that goes on the disc as it is (`passthrough_bits`).
    pub audio_bits: f64,
    /// The video that goes on the disc as it is, bits (its files' sizes).
    pub passthrough_bits: f64,
    /// Each timeline's encoded seconds and its other bits (sound, video kept as it is), in timeline order.
    pub timelines: Vec<(TimelineId, f64, f64)>,
    /// What the transport stream leaves the video beside the sound of the title with the most, kilobits per
    /// second (M6.347): no peak goes over it.
    pub room_kbps: u32,
}

impl HdBudget {
    /// The target and peak bitrates (kilobits per second) of a title in format `video`: the budget's, or for the
    /// other codec its usual rates scaled as the budget's are (a title's own HD format, M6.325).
    #[must_use]
    pub fn rates_for(&self, video: disccraft_project::BdVideo) -> (u32, u32) {
        let mpeg2 = self.codec.mpeg2(video);
        if mpeg2 == self.mpeg2 {
            return (self.target_kbps, self.max_kbps);
        }
        let (usual, _) = hd_default_kbps(mpeg2);
        let (disc_usual, _) = hd_default_kbps(self.mpeg2);
        let floor = if mpeg2 {
            MPEG2_FLOOR_KBPS
        } else {
            H264_FLOOR_KBPS
        };
        let scaled = u64::from(self.target_kbps) * u64::from(usual) / u64::from(disc_usual.max(1));
        let target = (u32::try_from(scaled).unwrap_or(usual) / STEP_KBPS * STEP_KBPS)
            .clamp(floor, raised_cap(mpeg2))
            .min(self.room_kbps.max(floor));
        (target, peak_for(mpeg2, target, self.room_kbps))
    }

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
    // A file whose H.264 goes on the disc as it is (in its title's format): its size (one a Blu-ray preset of its
    // own transcodes is not).
    let kept = |path: &Path| {
        let format = crate::BdVideoFormat::of_path(project, path).unwrap_or(format);
        (crate::video_fits_bd(path, format) && !super::transcode_forced(project, path))
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
    // MPEG-2 or H.264 as the disc's codec setting gives it (M12.28: 1080i may be H.264).
    let codec = project.settings.bd_codec;
    let mpeg2 = codec.mpeg2(project.settings.bd_video);
    let mut video_seconds = 0.0_f64;
    let mut audio_bits = 0.0_f64;
    let mut passthrough_bits = 0.0_f64;
    let mut most_sound = 0_u32;
    let mut timelines = Vec::with_capacity(project.timelines.len());
    for timeline in project.timelines.values() {
        // A title in a format of the other codec counts as it takes at that codec's usual rate (M6.325).
        let title_mpeg2 = codec.mpeg2(project.title_bd_video(timeline));
        let weight =
            f64::from(hd_default_kbps(title_mpeg2).0) / f64::from(hd_default_kbps(mpeg2).0);
        let (seconds, encoded, kept_bytes) =
            timeline_seconds(project, timeline, probe, kept, title_mpeg2)?;
        let encoded = encoded * weight;
        let kept_bits = kept_bytes as f64 * 8.0;
        let sound = audio_bitrate_for_title(project, timeline, seconds)?;
        most_sound = most_sound.max(sound);
        let bits = f64::from(sound) * seconds + kept_bits;
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

    let (usual, _) = hd_default_kbps(mpeg2);
    let floor = if mpeg2 {
        MPEG2_FLOOR_KBPS
    } else {
        H264_FLOOR_KBPS
    };
    // The video's peak and the sound go in the transport stream together (M6.347): much sound (many channels of
    // 24-bit 192 kHz LPCM) leaves the video less.
    let room_kbps = BD_STREAMS_BITRATE.saturating_sub(most_sound) / 1000;
    if room_kbps < floor {
        return Err(DvdError::Invalid(format!(
            "the sound ({most_sound} bit/s) leaves the HD video too little of the Blu-ray's stream ({room_kbps} \
             kbit/s; at least {floor} kbit/s is needed)"
        )));
    }
    let usual = usual.min(room_kbps / STEP_KBPS * STEP_KBPS).max(floor);
    let target_kbps = if video_seconds > 0.0 {
        let capacity = capacity_bits(project)? as f64;
        // What is left for the video once the sound is on the disc, both multiplexed.
        let fit_kbps =
            (capacity * 1000.0 / MUX_OVERHEAD_PER_MILLE - audio_bits) / video_seconds / 1000.0;
        if !fit_kbps.is_finite() || fit_kbps < f64::from(floor) {
            return Err(DvdError::Invalid(format!(
                "the titles are too long for the disc capacity in HD ({video_seconds:.0} s at {:.0} kbit/s; \
                 at least {floor} kbit/s is needed)",
                fit_kbps.max(0.0)
            )));
        }
        if fit_kbps >= f64::from(usual) {
            // Room to spare (M6.377): the rate goes up to use it, planning with a share of it, as far as the
            // codec's cap and the stream's room.
            let share = fit_kbps * RAISE_SHARE;
            // Inside `u32` once capped.
            let raised = (share.min(f64::from(raised_cap(mpeg2))) as u32) / STEP_KBPS * STEP_KBPS;
            raised.min(room_kbps / STEP_KBPS * STEP_KBPS).max(usual)
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
        max_kbps: peak_for(mpeg2, target_kbps, room_kbps),
        mpeg2,
        codec,
        video_seconds,
        audio_bits,
        passthrough_bits,
        timelines,
        room_kbps,
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
    // M6.388 / M6.393: a title made at HDV's 1440 keeps its 1440 clips (and encodes the others).
    let narrow = if mpeg2 {
        crate::BdVideoFormat::of(project)
            .and_then(|format| crate::narrow_mpeg2_width(project, timeline, format))
    } else {
        None
    };
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
        let alone = count == 1 && timeline.video_gap(clip) == 0;
        // A clip encoded at a bit rate of its own (M6.251) takes that much, whatever is left for the others.
        let fixed = super::fixed_video_bitrate(project, video);
        // HD MPEG-2 kept as it is (M10.22) is its title's only clip, as H.264 is on a disc that encodes MPEG-2.
        let own_mpeg2 = crate::BdVideoFormat::of_path(project, path)
            .is_some_and(|format| crate::mpeg2_fits_bd(path, format));
        // M6.385: HD MPEG-2 of the disc's size is kept beside the other clips of an MPEG-2 title.
        let joins = mpeg2
            && own_mpeg2
            && crate::BdVideoFormat::of_path(project, path).is_some_and(|format| {
                disccraft_transcode::probe_video(path)
                    .is_ok_and(|probe| probe.width == narrow.unwrap_or(format.width))
            });
        match (
            kept(path).filter(|_| !(mpeg2 || own_mpeg2) || alone || joins),
            fixed,
        ) {
            (None, Some(rate)) => {
                // `as` saturates; the bytes of a clip.
                let bytes = ((end - start) * f64::from(rate) / 8.0).round() as u64;
                kept_bytes = kept_bytes.saturating_add(bytes);
                encoded += gap;
            }
            (Some(bytes), _) => {
                // A trimmed clip keeps its part of the file (cut at key pictures: about this much).
                let share = ((end - start) / whole).clamp(0.0, 1.0);
                // `as` saturates; a share of a file's size.
                let bytes = (bytes as f64 * share).round() as u64;
                kept_bytes = kept_bytes.saturating_add(bytes);
                encoded += gap;
            }
            (None, None) => encoded += end - start + gap,
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
        // A trimmed clip keeps its part of the file (M10.21): the second half hour is half the bytes.
        let second = *project.timelines.keys().nth(1).ok_or("no timeline")?;
        let timeline = project.timelines.get_mut(&second).ok_or("no timeline")?;
        timeline.video_trims = vec![ClipTrim {
            start: 53_946,
            end: None,
        }];
        let budget = super::hd_budget_with(&project, &probe, &size)?.ok_or("no budget")?;
        assert!(
            (budget.passthrough_bits - 60e9).abs() < 60e9 * 1e-4,
            "{budget:?}"
        );
        Ok(())
    }

    /// M6.251: a clip encoded at a bit rate of its own takes that much of the disc, and is not shared out.
    #[test]
    fn an_assets_own_bit_rate_counts_as_bytes() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = project(BdVideo::Hd1080p23)?;
        let own = project.add_asset(Path::new("own.mp4"))?;
        let _ = project.add_timeline(own, None)?;
        let probe = |_path: &Path| Ok::<f64, DvdError>(3600.0);
        let size = |_path: &Path| None::<u64>;
        let both = super::hd_budget_with(&project, &probe, &size)?.ok_or("no budget")?;
        assert!((both.video_seconds - 7200.0).abs() < 1e-6, "{both:?}");
        // M6.307: a Blu-ray's own setting (the DVD one does not count on a Blu-ray).
        project.set_asset_transcode(own, Some(9_000))?;
        let dvd_only = super::hd_budget_with(&project, &probe, &size)?.ok_or("no budget")?;
        assert!(
            (dvd_only.video_seconds - 7200.0).abs() < 1e-6,
            "{dvd_only:?}"
        );
        if let Some(asset) = project.assets.get_mut(&own) {
            asset.bluray_transcode = Some(disccraft_project::BlurayTranscode {
                kbps: 30_000,
                max_kbps: 30_000,
                preset: String::new(),
                quality: disccraft_project::BEST_QUALITY,
                mode: disccraft_project::TranscodeMode::Vbr,
            });
        }
        let budget = super::hd_budget_with(&project, &probe, &size)?.ok_or("no budget")?;
        assert!((budget.video_seconds - 3600.0).abs() < 1e-6, "{budget:?}");
        // An hour at 30 Mbit/s.
        assert!((budget.passthrough_bits - 108e9).abs() < 1e6, "{budget:?}");
        assert!(budget.target_kbps <= both.target_kbps, "{budget:?}");
        Ok(())
    }

    fn budget(project: &Project, seconds: f64) -> Result<Option<HdBudget>, DvdError> {
        hd_budget_with_probe(project, &|_path: &Path| Ok::<f64, DvdError>(seconds))
    }

    /// M6.325: a title in a format of its own of the other codec (1080i MPEG-2 on a 1080p H.264 disc) counts as
    /// it takes at that codec's usual rate, and its rates are that codec's scaled as the budget's are.
    #[test]
    fn a_title_in_another_codec_counts_at_its_rate() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = project(BdVideo::Hd1080p23)?;
        let probe = |_path: &Path| Ok::<f64, DvdError>(3600.0);
        let size = |_path: &Path| None::<u64>;
        let alone = super::hd_budget_with(&project, &probe, &size)?.ok_or("no budget")?;
        // An hour alone on a BD-25 has room: raised to the caps (M6.377).
        assert_eq!(alone.rates_for(BdVideo::Hd1080p23), (25_000, 40_000));
        assert_eq!(alone.rates_for(BdVideo::Hd1080i59), (32_000, 40_000));
        let interlaced = project.add_asset(Path::new("interlaced.mp4"))?;
        let _ = project.add_timeline(interlaced, None)?;
        if let Some(asset) = project.assets.get_mut(&interlaced) {
            asset.bluray_video = Some(BdVideo::Hd1080i59);
        }
        let mixed = super::hd_budget_with(&project, &probe, &size)?.ok_or("no budget")?;
        // The MPEG-2 hour counts as 25/12 of an hour of H.264.
        assert!(
            (mixed.video_seconds - 3600.0 * (1.0 + 25.0 / 12.0)).abs() < 1e-6,
            "{mixed:?}"
        );
        assert!(!mixed.mpeg2);
        // Fitted: both codecs' rates come down together.
        let mut tight = project.clone();
        tight.settings.capacity = Capacity::Bd25;
        let long = |_path: &Path| Ok::<f64, DvdError>(3.0 * 3600.0);
        let fitted = super::hd_budget_with(&tight, &long, &size)?.ok_or("no budget")?;
        let (h264, _) = fitted.rates_for(BdVideo::Hd1080p23);
        let (mpeg2, peak) = fitted.rates_for(BdVideo::Hd1080i59);
        assert!(h264 < 12_000 && mpeg2 < 25_000, "{fitted:?}");
        assert!(mpeg2 >= h264 * 2 - super::STEP_KBPS, "{h264} {mpeg2}");
        assert_eq!(mpeg2 % super::STEP_KBPS, 0);
        assert!(peak >= mpeg2);
        Ok(())
    }

    /// M12.28: with the disc's codec set to H.264, 1080i is budgeted at H.264's rates (and a 1080i title on a
    /// 1080p disc counts as H.264, not at MPEG-2's rate).
    #[test]
    fn the_disc_codec_chooses_the_rates_of_1080i() -> Result<(), Box<dyn std::error::Error>> {
        let mut interlaced = project(BdVideo::Hd1080i59)?;
        interlaced.settings.bd_codec = disccraft_project::BdCodec::H264;
        let h264 = budget(&interlaced, 3600.0)?.ok_or("no budget")?;
        assert!(!h264.mpeg2, "{h264:?}");
        assert_eq!((h264.target_kbps, h264.max_kbps), (25_000, 40_000));
        assert_eq!(h264.rates_for(BdVideo::Hd1080i59), (25_000, 40_000));
        assert!((h264.video_seconds - 3600.0).abs() < 1e-6, "{h264:?}");
        // The same hour as MPEG-2 (the automatic choice) takes MPEG-2's rate.
        let mpeg2 = budget(&project(BdVideo::Hd1080i59)?, 3600.0)?.ok_or("no budget")?;
        assert!(mpeg2.mpeg2);
        let mut mixed = project(BdVideo::Hd1080p23)?;
        mixed.settings.bd_codec = disccraft_project::BdCodec::H264;
        let asset = mixed.add_asset(Path::new("interlaced.mp4"))?;
        let _ = mixed.add_timeline(asset, None)?;
        if let Some(asset) = mixed.assets.get_mut(&asset) {
            asset.bluray_video = Some(BdVideo::Hd1080i59);
        }
        let size = |_path: &Path| None::<u64>;
        let probe = |_path: &Path| Ok::<f64, DvdError>(3600.0);
        let budget = super::hd_budget_with(&mixed, &probe, &size)?.ok_or("no budget")?;
        // Two hours of H.264, not one and 25/12.
        assert!(
            (budget.video_seconds - 2.0 * 3600.0).abs() < 1e-6,
            "{budget:?}"
        );
        Ok(())
    }

    #[test]
    fn only_an_hd_blu_ray_has_an_hd_budget() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(budget(&project(BdVideo::Sd)?, 60.0)?, None);
        let mut dvd = project(BdVideo::Hd1080p23)?;
        dvd.settings.kind = DiscKind::Dvd;
        assert_eq!(budget(&dvd, 60.0)?, None);
        Ok(())
    }

    /// M6.377: titles with room to spare are raised, up to the codec's cap (the peak twice the target, at most
    /// 40 Mbit/s), planning with 95 % of the room; between the usual rate and the cap, as far as the room goes.
    #[test]
    fn titles_with_room_get_a_raised_bitrate() -> Result<(), Box<dyn std::error::Error>> {
        // An hour on a BD-25: room for about 51 Mbps.
        let h264 = budget(&project(BdVideo::Hd1080p23)?, 3600.0)?.ok_or("no budget")?;
        assert_eq!((h264.target_kbps, h264.max_kbps), (25_000, 40_000));
        let mpeg2 = budget(&project(BdVideo::Hd1080i59)?, 3600.0)?.ok_or("no budget")?;
        assert_eq!((mpeg2.target_kbps, mpeg2.max_kbps), (32_000, 40_000));
        // Video and the AC-3 soundtrack (192 kbit/s), with the transport stream's overhead.
        let expected = (3600.0 * (32_000_000.0 + 192_000.0) * 1.04 / 8.0) as u64;
        assert_eq!(mpeg2.estimated_bytes(), expected);
        assert!(mpeg2.estimated_bytes() < Capacity::Bd25.bytes());
        // Three and a half hours of 1080i on a BD-50: room for about 29 Mbps, 95 % of it.
        let mut bd50 = project(BdVideo::Hd1080i59)?;
        bd50.settings.capacity = Capacity::Bd50;
        let between = budget(&bd50, 3.5 * 3600.0)?.ok_or("no budget")?;
        assert!(
            (25_500..32_000).contains(&between.target_kbps),
            "{between:?}"
        );
        assert_eq!(between.target_kbps % 500, 0);
        assert!(between.estimated_bytes() <= Capacity::Bd50.bytes() / 100 * 96);
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
        // The same three hours fit a BD-50 above the usual rate (M6.377: raised to the cap).
        let mut bd50 = project_1080i;
        bd50.settings.capacity = Capacity::Bd50;
        assert_eq!(
            budget(&bd50, 3.0 * 3600.0)?.map(|budget| budget.target_kbps),
            Some(32_000)
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
        assert_eq!(estimate.video_bitrate, 32_000_000);
        assert_eq!(estimate.capacity_bytes, Capacity::Bd25.bytes());
        let expected = (3600.0 * (32_000_000.0 + 192_000.0) * 1.04 / 8.0) as u64;
        assert_eq!(estimate.estimated_bytes, expected);
        assert_eq!(
            estimate.timelines.first().map(|(_, bytes)| *bytes),
            Some(expected)
        );
        Ok(())
    }
}
