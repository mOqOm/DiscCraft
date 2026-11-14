//! Prepare DVD title media by choosing passthrough or cached transcodes.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use disccraft_project::{
    AssetId, AssetKind, Capacity, Project, Slideshow, Tick, Timeline, TimelineId, VideoStandard,
};
use disccraft_transcode::slideshow::{
    SlideFit, SlideInput, SlideTransition, Transition, render_slideshow, render_slideshow_hd,
};
use disccraft_transcode::{Standard, TranscodeOptions, probe_seconds, transcode, transcode_audio};

use crate::DvdError;

/// LPCM stereo bitrate (a WAV audio track, used as it is).
pub(crate) const LPCM_STEREO_BITRATE: u32 = 1_536_000;
/// The bitrate of transcoded audio: AC-3 stereo at 192 kbit/s (Dolby Digital, as Encore transcodes a soundtrack).
pub(crate) const AC3_STEREO_BITRATE: u32 = disccraft_ac3enc::STEREO_KBPS * 1000;
/// The version of the AC-3 soundtracks in the cache (made from the cached WAVs).
const AC3_CACHE_VERSION: &str = "ac3-1";

/// Maximum video bitrate accepted by the program stream muxer.
pub(crate) const MAX_MUX_VIDEO_BITRATE: u32 = 9_800_000;

const MIN_VIDEO_BITRATE: u32 = 2_000_000;
const MAX_LPCM_AUDIO_BITRATE: u32 = 5 * LPCM_STEREO_BITRATE;

const HEAD_BYTES: usize = 65_536;
const SEQUENCE_HEADER_BYTES: usize = 11;
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const CACHE_VERSION: &str = "disccraft-mpeg2venc-2";
const SLIDE_RENDER_VERSION: &str = "slides-2";
const TICKS_PER_SECOND: i64 = 254_016_000_000;

/// Bitrate budget for transcoded titles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Budget {
    /// Target video bitrate in bits per second.
    pub(crate) video_bitrate: u32,
    /// Maximum video bitrate in bits per second.
    pub(crate) max_bitrate: u32,
    /// 16:9 anamorphic output (from the project settings).
    pub(crate) wide: bool,
    /// Largest audio bitrate among transcoded titles.
    pub(crate) audio_bitrate: u32,
}

/// One prepared audio track for muxing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PreparedAudio {
    /// AC-3 elementary stream passed through.
    Ac3(PathBuf),
    /// LPCM WAV stream passed through or transcoded.
    Wav(PathBuf),
}

/// Prepared video and optional audio paths for one title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedTitle {
    /// Path to the video elementary stream used for muxing.
    pub(crate) video_path: PathBuf,
    /// Optional path to transcoded LPCM audio.
    pub(crate) transcoded_audio: Option<PathBuf>,
    /// Whether the title was transcoded rather than passed through.
    pub(crate) transcoded: bool,
    /// Prepared explicit audio tracks, in timeline order.
    pub(crate) audio_tracks: Vec<PreparedAudio>,
}

/// Internal scan result used to allocate bitrate.
#[derive(Default)]
struct Scan {
    passthrough_bits: u64,
    transcode_seconds: f64,
    audio_bits: f64,
    transcode_count: usize,
    largest_audio_bitrate: u32,
}

/// Internal classification of an audio asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioKind {
    Ac3,
    Wav,
    Transcode,
}

/// Return whether a video asset can be passed through unchanged.
pub(crate) fn is_passthrough(path: &Path, standard: VideoStandard) -> bool {
    if !has_passthrough_extension(path) {
        return false;
    }

    let Some(data) = read_head(path) else {
        return false;
    };

    first_sequence_header_matches(&data, standard)
}

/// Allocate a transcode bitrate budget for a project.
///
/// # Errors
///
/// Returns [`DvdError::Invalid`] when durations or capacity are unusable, and
/// I/O errors when passthrough asset sizes cannot be read.
pub(crate) fn budget(project: &Project) -> Result<Budget, DvdError> {
    budget_with_probe(project, &|path| {
        probe_seconds(path)
            .map_err(|err| DvdError::Invalid(format!("probe {}: {err}", path.display())))
    })
}

/// Prepare one title for muxing, using the transcode cache when needed.
///
/// Transcodes every title that needs it (timelines and slideshows) into `cache_dir`, several at a time (one per
/// CPU core: the MPEG-2 encoder uses one core each), so the builds that follow find them in the cache. The output
/// is the same as transcoding them one by one. `progress` hears from every transcode, on the calling thread.
///
/// # Errors
///
/// The first title that cannot be prepared (in project order).
pub(crate) fn prepare_all(
    project: &Project,
    cache_dir: &Path,
    budget: &Budget,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<(), DvdError> {
    enum Job<'a> {
        Timeline(&'a Timeline),
        Slideshow(&'a Slideshow),
    }
    // One job per video (timelines of the same video share its transcode) and per slideshow picture.
    let mut videos = std::collections::BTreeSet::new();
    let mut jobs: Vec<Job<'_>> = project
        .timelines
        .values()
        .filter(|timeline| videos.insert(timeline.video))
        .map(Job::Timeline)
        .collect();
    let mut shows = std::collections::BTreeSet::new();
    for id in crate::slideshow_ids(project) {
        if let Some(slideshow) = project.slideshows.get(&id)
            && slideshow_cache_key(project, slideshow, budget).map_or(true, |key| shows.insert(key))
        {
            jobs.push(Job::Slideshow(slideshow));
        }
    }
    let results = crate::parallel::run(&jobs, progress, |job, report| match job {
        Job::Timeline(timeline) => prepare_title(project, timeline, cache_dir, budget, report),
        Job::Slideshow(slideshow) => {
            prepare_slideshow(project, slideshow, cache_dir, budget, report)
        }
    });
    for result in results {
        result?;
    }
    Ok(())
}

/// A number for a transcode's part files, so two transcodes running at once never write the same file (the one
/// that finishes last publishes its result; both are the same).
fn part_number() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// # Errors
///
/// Returns [`DvdError::Unsupported`] when transcoding fails, and I/O or
/// invalid-project errors when cache or asset access fails.
pub(crate) fn prepare_title(
    project: &Project,
    timeline: &Timeline,
    cache_dir: &Path,
    budget: &Budget,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<PreparedTitle, DvdError> {
    let asset = project
        .assets
        .get(&timeline.video)
        .ok_or_else(|| DvdError::Invalid("referenced asset does not exist".to_string()))?;
    let input = Path::new(asset.path.as_str());

    let audio_tracks = prepare_audio_tracks(project, timeline, cache_dir)?;

    if is_passthrough(input, project.settings.standard) {
        return Ok(PreparedTitle {
            video_path: input.to_path_buf(),
            transcoded_audio: None,
            transcoded: false,
            audio_tracks,
        });
    }

    let wide = timeline.wide(&project.settings);
    let gop_starts = asset_gop_starts(project, input);
    let key = cache_key(project, input, budget, (wide, &gop_starts))?;
    let video_out = cache_dir.join(format!("{key}.m2v"));
    let marker = cache_dir.join(format!("{key}.ok"));

    if marker.is_file() && video_out.is_file() {
        let audio_out = cache_dir.join(format!("{key}.wav"));
        let transcoded_audio = match audio_out.is_file() {
            true => Some(ac3_soundtrack(&audio_out)?),
            false => None,
        };
        return Ok(PreparedTitle {
            video_path: video_out,
            transcoded_audio,
            transcoded: true,
            audio_tracks,
        });
    }

    let options = TranscodeOptions {
        standard: transcode_standard(project.settings.standard),
        video_bitrate: budget.video_bitrate,
        max_bitrate: budget.max_bitrate,
        wide,
        gop_starts,
    };
    let (video_path, transcoded_audio) = transcode_to_cache(
        asset.name.as_str(),
        input,
        cache_dir,
        &key,
        &options,
        progress,
    )?;
    let transcoded_audio = transcoded_audio
        .map(|wav| ac3_soundtrack(&wav))
        .transpose()?;

    Ok(PreparedTitle {
        video_path,
        transcoded_audio,
        transcoded: true,
        audio_tracks,
    })
}

/// Prepare one slideshow title for muxing, using the transcode cache when needed.
///
/// # Errors
///
/// Returns [`DvdError::Unsupported`] when transcoding fails, and I/O or
/// invalid-project errors when cache or asset access fails.
pub(crate) fn prepare_slideshow(
    project: &Project,
    slideshow: &Slideshow,
    cache_dir: &Path,
    budget: &Budget,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<PreparedTitle, DvdError> {
    let key = slideshow_cache_key(project, slideshow, budget)?;
    let video_out = cache_dir.join(format!("{key}.m2v"));
    let audio_out = cache_dir.join(format!("{key}.wav"));
    let marker = cache_dir.join(format!("{key}.ok"));

    if marker.is_file() && video_out.is_file() && audio_out.is_file() {
        return Ok(PreparedTitle {
            video_path: video_out,
            transcoded_audio: Some(ac3_soundtrack(&audio_out)?),
            transcoded: true,
            audio_tracks: Vec::new(),
        });
    }

    let (video_path, transcoded_audio) =
        transcode_slideshow_to_cache(project, slideshow, cache_dir, &key, budget, progress)?;
    let transcoded_audio = transcoded_audio
        .map(|wav| ac3_soundtrack(&wav))
        .transpose()?;

    Ok(PreparedTitle {
        video_path,
        transcoded_audio,
        transcoded: true,
        audio_tracks: Vec::new(),
    })
}

/// Allocate bitrate using an injectable duration probe.
fn budget_with_probe(
    project: &Project,
    probe: &dyn Fn(&Path) -> Result<f64, DvdError>,
) -> Result<Budget, DvdError> {
    let capacity_bits = capacity_bits(project.settings.capacity)?;
    let scan = scan_project(project, probe)?;

    let audio_bitrate = if scan.transcode_count == 0 {
        LPCM_STEREO_BITRATE
    } else {
        scan.largest_audio_bitrate
    };

    let max_bitrate = max_bitrate_for_audio_bitrate(audio_bitrate)?;

    if scan.transcode_count == 0 {
        return Ok(Budget {
            video_bitrate: 0,
            max_bitrate,
            wide: project.settings.widescreen,
            audio_bitrate,
        });
    }

    let video_bitrate = allocate_video_bitrate(capacity_bits, &scan, max_bitrate)?;

    Ok(Budget {
        video_bitrate,
        max_bitrate,
        wide: project.settings.widescreen,
        audio_bitrate,
    })
}

/// Convert usable disc capacity to bits with 4 percent overhead reserve.
fn capacity_bits(capacity: Capacity) -> Result<u64, DvdError> {
    capacity
        .bytes()
        .checked_mul(8)
        .and_then(|bits| bits.checked_mul(96))
        .and_then(|bits| bits.checked_div(100))
        .ok_or_else(|| DvdError::Invalid("disc capacity overflow".to_string()))
}

/// Return the muxer video bitrate ceiling after an audio bitrate.
fn max_bitrate_for_audio_bitrate(audio_bitrate: u32) -> Result<u32, DvdError> {
    let max_bitrate = MAX_MUX_VIDEO_BITRATE
        .checked_sub(audio_bitrate)
        .ok_or_else(|| {
            DvdError::Invalid("too many audio tracks for the DVD mux rate".to_string())
        })?;

    if audio_bitrate >= MAX_LPCM_AUDIO_BITRATE || max_bitrate < MIN_VIDEO_BITRATE {
        return Err(DvdError::Invalid(format!(
            "too many audio tracks for the DVD mux rate ({max_bitrate} bit/s video headroom)"
        )));
    }

    Ok(max_bitrate)
}

/// Scan all timelines and slideshows and accumulate bitrate requirements.
fn scan_project(
    project: &Project,
    probe: &dyn Fn(&Path) -> Result<f64, DvdError>,
) -> Result<Scan, DvdError> {
    let mut scan = scan_timelines(project, probe)?;
    scan_slideshows(project, &mut scan)?;
    Ok(scan)
}

/// Scan all timelines and accumulate passthrough and transcode requirements.
fn scan_timelines(
    project: &Project,
    probe: &dyn Fn(&Path) -> Result<f64, DvdError>,
) -> Result<Scan, DvdError> {
    let mut scan = Scan::default();

    for timeline in project.timelines.values() {
        scan_timeline(project, timeline, probe, &mut scan)?;
    }

    Ok(scan)
}

/// Scan one timeline as either passthrough or transcode.
fn scan_timeline(
    project: &Project,
    timeline: &Timeline,
    probe: &dyn Fn(&Path) -> Result<f64, DvdError>,
    scan: &mut Scan,
) -> Result<(), DvdError> {
    let video_path = asset_path(project, timeline.video)?;

    if is_passthrough(video_path, project.settings.standard) {
        scan_passthrough(project, timeline, probe, scan)
    } else {
        scan_transcode(project, timeline, video_path, probe, scan)
    }
}

/// Add passthrough video and audio file sizes to the scan.
fn scan_passthrough(
    project: &Project,
    timeline: &Timeline,
    probe: &dyn Fn(&Path) -> Result<f64, DvdError>,
    scan: &mut Scan,
) -> Result<(), DvdError> {
    let video_path = asset_path(project, timeline.video)?;
    scan.passthrough_bits = add_bits(scan.passthrough_bits, file_size(video_path)?)?;

    let mut seconds: Option<f64> = None;

    for track in &timeline.audio {
        let audio_path = asset_path(project, track.asset)?;

        match audio_kind(audio_path) {
            AudioKind::Ac3 | AudioKind::Wav => {
                scan.passthrough_bits = add_bits(scan.passthrough_bits, file_size(audio_path)?)?;
            }
            AudioKind::Transcode => {
                let secs = match seconds {
                    Some(secs) => secs,
                    None => {
                        let secs = positive_seconds(probe(video_path)?)?;
                        seconds = Some(secs);
                        secs
                    }
                };

                let bits = ac3_bits_for_seconds(secs)?;
                scan.passthrough_bits = add_bits(scan.passthrough_bits, bits)?;
            }
        }
    }

    Ok(())
}

/// Add transcode duration and audio bitrate to the scan.
fn scan_transcode(
    project: &Project,
    timeline: &Timeline,
    video_path: &Path,
    probe: &dyn Fn(&Path) -> Result<f64, DvdError>,
    scan: &mut Scan,
) -> Result<(), DvdError> {
    let seconds = positive_seconds(probe(video_path)?)?;
    scan.transcode_seconds += seconds;
    scan.transcode_count = scan
        .transcode_count
        .checked_add(1)
        .ok_or_else(|| DvdError::Invalid("too many timelines".to_string()))?;

    let audio_bitrate = audio_bitrate_for_title(project, timeline, seconds)?;
    scan.largest_audio_bitrate = scan.largest_audio_bitrate.max(audio_bitrate);
    scan.audio_bits += f64::from(audio_bitrate) * seconds;
    Ok(())
}

/// Add slideshow durations to the scan as transcoded material with one LPCM track.
fn scan_slideshows(project: &Project, scan: &mut Scan) -> Result<(), DvdError> {
    for slideshow in project.slideshows.values() {
        if slideshow.slides.is_empty() {
            return Err(DvdError::Invalid("slideshow has no slides".to_string()));
        }

        let mut seconds = 0f64;

        for slide in &slideshow.slides {
            let slide_seconds = slide_seconds(slide.duration)?;
            seconds += slide_seconds;

            if !seconds.is_finite() {
                return Err(DvdError::Invalid("slideshow duration overflow".to_string()));
            }
        }

        scan.transcode_seconds += seconds;
        if !scan.transcode_seconds.is_finite() {
            return Err(DvdError::Invalid("slideshow duration overflow".to_string()));
        }

        scan.transcode_count = scan
            .transcode_count
            .checked_add(1)
            .ok_or_else(|| DvdError::Invalid("too many slideshows".to_string()))?;

        scan.largest_audio_bitrate = scan.largest_audio_bitrate.max(AC3_STEREO_BITRATE);
        scan.audio_bits += f64::from(AC3_STEREO_BITRATE) * seconds;

        if !scan.audio_bits.is_finite() {
            return Err(DvdError::Invalid("audio bitrate overflow".to_string()));
        }
    }

    Ok(())
}

/// Convert a slide duration to seconds.
fn slide_seconds(duration: Tick) -> Result<f64, DvdError> {
    if duration.0 < 0 {
        return Err(DvdError::Invalid("slide duration is negative".to_string()));
    }

    let seconds = duration.0 as f64 / TICKS_PER_SECOND as f64;
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err(DvdError::Invalid("slide duration out of range".to_string()));
    }

    Ok(seconds)
}

/// Reject non-positive or non-finite durations.
fn positive_seconds(seconds: f64) -> Result<f64, DvdError> {
    if seconds.is_finite() && seconds > 0.0 {
        Ok(seconds)
    } else {
        Err(DvdError::Invalid(
            "video duration must be positive".to_string(),
        ))
    }
}

/// Compute the average audio bitrate for one transcoded title.
fn audio_bitrate_for_title(
    project: &Project,
    timeline: &Timeline,
    seconds: f64,
) -> Result<u32, DvdError> {
    // The video's own soundtrack, transcoded to AC-3.
    if timeline.audio.is_empty() {
        return Ok(AC3_STEREO_BITRATE);
    }

    let mut total = 0u64;

    for track in &timeline.audio {
        let path = asset_path(project, track.asset)?;
        let bitrate = track_audio_bitrate(path, seconds)?;
        total = total
            .checked_add(u64::from(bitrate))
            .ok_or_else(|| DvdError::Invalid("audio bitrate overflow".to_string()))?;
    }

    u32::try_from(total).map_err(|_| DvdError::Invalid("audio bitrate exceeds u32".to_string()))
}

/// Compute the bitrate contribution of one audio track.
fn track_audio_bitrate(path: &Path, seconds: f64) -> Result<u32, DvdError> {
    match audio_kind(path) {
        AudioKind::Ac3 => {
            let bits = file_size(path)?
                .checked_mul(8)
                .ok_or_else(|| DvdError::Invalid("audio size overflow".to_string()))?;
            let per_second_f64 = bits as f64 / seconds;

            if !per_second_f64.is_finite() || per_second_f64 > f64::from(u32::MAX) {
                return Err(DvdError::Invalid("audio bitrate overflow".to_string()));
            }

            u32::try_from(per_second_f64 as u64)
                .map_err(|_| DvdError::Invalid("audio bitrate exceeds u32".to_string()))
        }
        AudioKind::Wav => Ok(LPCM_STEREO_BITRATE),
        AudioKind::Transcode => Ok(AC3_STEREO_BITRATE),
    }
}

/// Estimate the bits of transcoded (AC-3) audio for a duration in seconds.
fn ac3_bits_for_seconds(seconds: f64) -> Result<u64, DvdError> {
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(DvdError::Invalid(
            "audio duration must be finite".to_string(),
        ));
    }

    let bits = f64::from(AC3_STEREO_BITRATE) * seconds;
    if !bits.is_finite() || bits > u64::MAX as f64 {
        return Err(DvdError::Invalid("audio bitrate overflow".to_string()));
    }

    // Finite and within u64 (checked above), so the cast is exact up to rounding.
    Ok(bits.ceil() as u64)
}

/// Allocate the remaining bitrate to video after passthrough and audio.
fn allocate_video_bitrate(
    capacity_bits: u64,
    scan: &Scan,
    max_bitrate: u32,
) -> Result<u32, DvdError> {
    if !scan.transcode_seconds.is_finite() || scan.transcode_seconds <= 0.0 {
        return Err(DvdError::Invalid("transcode duration overflow".to_string()));
    }
    if !scan.audio_bits.is_finite() {
        return Err(DvdError::Invalid("audio bitrate overflow".to_string()));
    }

    let remaining_bits = capacity_bits
        .checked_sub(scan.passthrough_bits)
        .ok_or_else(|| DvdError::Invalid("passthrough media exceeds disc capacity".to_string()))?;

    let total_bits_per_second = remaining_bits as f64 / scan.transcode_seconds;
    let average_audio_bitrate = scan.audio_bits / scan.transcode_seconds;
    let video_bitrate = total_bits_per_second - average_audio_bitrate;

    if !video_bitrate.is_finite() || video_bitrate < 1_000_000.0 {
        return Err(DvdError::Invalid(format!(
            "the timelines are too long for the disc capacity ({:.0} s at {:.0} kbit/s)",
            scan.transcode_seconds,
            video_bitrate / 1_000.0
        )));
    }

    let capped = video_bitrate.min(f64::from(max_bitrate));

    if capped >= 8_000_000.0 {
        return Ok(8_000_000);
    }

    u32::try_from(capped as u64)
        .map_err(|_| DvdError::Invalid("video bitrate overflow".to_string()))
}

/// Add a byte size converted to bits with checked arithmetic.
fn add_bits(total: u64, bytes: u64) -> Result<u64, DvdError> {
    let bits = bytes
        .checked_mul(8)
        .ok_or_else(|| DvdError::Invalid("asset size overflow".to_string()))?;

    total
        .checked_add(bits)
        .ok_or_else(|| DvdError::Invalid("passthrough size overflow".to_string()))
}

/// Return the file size in bytes.
fn file_size(path: &Path) -> Result<u64, DvdError> {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|err| crate::io_error(path, err))
}

/// Borrow the filesystem path for an asset id.
fn asset_path(project: &Project, id: AssetId) -> Result<&Path, DvdError> {
    project
        .assets
        .get(&id)
        .map(|asset| Path::new(asset.path.as_str()))
        .ok_or_else(|| DvdError::Invalid("referenced asset does not exist".to_string()))
}

/// Prepare explicit audio tracks, transcoding unsupported formats into cache.
fn prepare_audio_tracks(
    project: &Project,
    timeline: &Timeline,
    cache_dir: &Path,
) -> Result<Vec<PreparedAudio>, DvdError> {
    let mut out = Vec::with_capacity(timeline.audio.len());

    for track in &timeline.audio {
        let asset = project
            .assets
            .get(&track.asset)
            .ok_or_else(|| DvdError::Invalid("referenced asset does not exist".to_string()))?;
        let path = Path::new(asset.path.as_str());

        let prepared = match audio_kind(path) {
            AudioKind::Ac3 => PreparedAudio::Ac3(path.to_path_buf()),
            AudioKind::Wav => PreparedAudio::Wav(path.to_path_buf()),
            AudioKind::Transcode => {
                let key = audio_cache_key(path)?;
                let wav = transcode_audio_to_cache(path, cache_dir, &key)?;
                PreparedAudio::Ac3(ac3_soundtrack(&wav)?)
            }
        };

        out.push(prepared);
    }

    Ok(out)
}

/// Classify an audio asset by extension.
fn audio_kind(path: &Path) -> AudioKind {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("ac3") => AudioKind::Ac3,
        Some(ext) if ext.eq_ignore_ascii_case("wav") => AudioKind::Wav,
        _ => AudioKind::Transcode,
    }
}

/// Return whether the extension is an MPEG video elementary stream.
fn has_passthrough_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("m2v") || ext.eq_ignore_ascii_case("mpv"))
}

/// Read at most the first 64 KiB of a file.
fn read_head(path: &Path) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).ok()?;
    let mut buf = vec![0u8; HEAD_BYTES];
    let mut total = 0usize;

    while total < buf.len() {
        let (_, rest) = buf.split_at_mut(total);

        match file.read(rest) {
            Ok(0) => break,
            Ok(n) => {
                total = total.checked_add(n)?;
            }
            Err(_) => return None,
        }
    }

    buf.truncate(total);
    Some(buf)
}

/// Validate the first MPEG-2 sequence header in the buffer.
fn first_sequence_header_matches(data: &[u8], standard: VideoStandard) -> bool {
    let mut pos = 0usize;

    while let Some(slice) = data.get(pos..pos.saturating_add(SEQUENCE_HEADER_BYTES)) {
        if slice.starts_with(&[0x00, 0x00, 0x01, 0xB3]) {
            return sequence_header_matches(slice, standard);
        }

        pos = pos.saturating_add(1);
    }

    false
}

/// Check whether a sequence header matches DVD passthrough constraints.
fn sequence_header_matches(slice: &[u8], standard: VideoStandard) -> bool {
    let (want_height, want_rate) = match standard {
        VideoStandard::Ntsc => (480u16, 4u8),
        VideoStandard::Pal => (576u16, 3u8),
    };

    let Some(b0) = slice.get(4) else {
        return false;
    };
    let Some(b1) = slice.get(5) else {
        return false;
    };
    let Some(b2) = slice.get(6) else {
        return false;
    };
    let Some(b3) = slice.get(7) else {
        return false;
    };
    let Some(b4) = slice.get(8) else {
        return false;
    };
    let Some(b5) = slice.get(9) else {
        return false;
    };
    let Some(b6) = slice.get(10) else {
        return false;
    };

    let width = (u16::from(*b0) << 4) | (u16::from(*b1) >> 4);
    let height = (u16::from(*b1 & 0x0F) << 8) | u16::from(*b2);
    let aspect = *b3 >> 4;
    let rate = *b3 & 0x0F;
    let bit_rate = (u32::from(*b4) << 10) | (u32::from(*b5) << 2) | (u32::from(*b6) >> 6);

    width == 720
        && height == want_height
        && (aspect == 2 || aspect == 3)
        && rate == want_rate
        && bit_rate
            .checked_mul(400)
            .is_some_and(|bits| bits <= MAX_MUX_VIDEO_BITRATE)
}

/// Compute a 16 hex digit cache key for a transcode (`wide`: the title's aspect, and `gop_starts`: the frames its
/// GOPs must start at; both part of the output).
fn cache_key(
    project: &Project,
    input: &Path,
    budget: &Budget,
    (wide, gop_starts): (bool, &[u64]),
) -> Result<String, DvdError> {
    let metadata = fs::metadata(input).map_err(|err| crate::io_error(input, err))?;
    let (secs, nanos) = modified_time(&metadata)?;
    let path = absolute_path_string(input);

    let mut seed = fnv1a64(path.as_bytes(), FNV_OFFSET);
    seed = fnv1a64(metadata.len().to_string().as_bytes(), seed);
    seed = fnv1a64(secs.to_string().as_bytes(), seed);
    seed = fnv1a64(nanos.to_string().as_bytes(), seed);
    seed = fnv1a64(standard_tag(project.settings.standard).as_bytes(), seed);
    seed = fnv1a64(budget.video_bitrate.to_string().as_bytes(), seed);
    seed = fnv1a64(budget.max_bitrate.to_string().as_bytes(), seed);
    seed = fnv1a64(if wide { b"16:9" } else { b"4:3" }, seed);
    // Only a video with chapters past its start has GOP starts: the keys of the others stay as they were.
    if !gop_starts.is_empty() {
        let frames: Vec<String> = gop_starts.iter().map(u64::to_string).collect();
        seed = fnv1a64(format!("gop:{}", frames.join(",")).as_bytes(), seed);
    }
    seed = fnv1a64(CACHE_VERSION.as_bytes(), seed);

    Ok(format!("{seed:016x}"))
}

/// Ticks (254,016,000,000 a second) of one frame at the disc's rate: 1001/30000 s for NTSC, 1/25 s for PAL.
fn frame_ticks(standard: VideoStandard) -> u64 {
    match standard {
        VideoStandard::Ntsc => 8_475_667_200,
        VideoStandard::Pal => 10_160_640_000,
    }
}

/// The frame (at the disc's rate) nearest to `ticks` from the start of a title.
pub(crate) fn frame_at(ticks: i64, standard: VideoStandard) -> u64 {
    let per_frame = frame_ticks(standard);
    u64::try_from(ticks).map_or(0, |ticks| ticks.saturating_add(per_frame / 2) / per_frame)
}

/// The manual layer break's time in `timeline`, when the disc is a dual-layer DVD (DVD9) and the break is in it.
pub(crate) fn layer_break_time(project: &Project, timeline: &Timeline) -> Option<Tick> {
    project
        .settings
        .layer_break
        .filter(|point| point.timeline == timeline.id)
        .filter(|_| project.settings.capacity == Capacity::Dvd9)
        .map(|point| point.time)
}

/// The output frames where the transcode of video `path` must start a GOP: every chapter past the start of every
/// timeline that plays it, so each chapter's cell begins exactly at its frame (the GOPs otherwise fall every 15 or 12
/// frames, and a cell would start at the nearest of them).
fn asset_gop_starts(project: &Project, path: &Path) -> Vec<u64> {
    let mut frames: Vec<u64> = project
        .timelines
        .values()
        .filter(|timeline| {
            project
                .assets
                .get(&timeline.video)
                .is_some_and(|asset| Path::new(&asset.path) == path)
        })
        .flat_map(|timeline| {
            // The manual layer break of a dual-layer DVD gets its own cell, so its own GOP.
            let layer_break = layer_break_time(project, timeline);
            timeline
                .chapters
                .iter()
                .map(|chapter| chapter.time)
                .chain(layer_break)
        })
        .map(|time| frame_at(time.0, project.settings.standard))
        .filter(|frame| *frame > 0)
        .collect();
    frames.sort_unstable();
    frames.dedup();
    frames
}

/// The aspect a video asset is transcoded with: the first timeline that plays it (else the disc's).
fn asset_wide(project: &Project, path: &Path) -> bool {
    project
        .timelines
        .values()
        .find(|timeline| {
            project
                .assets
                .get(&timeline.video)
                .is_some_and(|asset| Path::new(&asset.path) == path)
        })
        .map_or(project.settings.widescreen, |timeline| {
            timeline.wide(&project.settings)
        })
}

/// Compute a 16 hex digit cache key for an audio transcode.
fn audio_cache_key(input: &Path) -> Result<String, DvdError> {
    let metadata = fs::metadata(input).map_err(|err| crate::io_error(input, err))?;
    let (secs, nanos) = modified_time(&metadata)?;
    let path = absolute_path_string(input);

    let mut seed = fnv1a64(path.as_bytes(), FNV_OFFSET);
    seed = fnv1a64(metadata.len().to_string().as_bytes(), seed);
    seed = fnv1a64(secs.to_string().as_bytes(), seed);
    seed = fnv1a64(nanos.to_string().as_bytes(), seed);
    seed = fnv1a64(b"audio-lpcm", seed);
    seed = fnv1a64(CACHE_VERSION.as_bytes(), seed);

    Ok(format!("{seed:016x}"))
}

/// Compute a 16 hex digit cache key for a slideshow transcode.
fn slideshow_cache_key(
    project: &Project,
    slideshow: &Slideshow,
    budget: &Budget,
) -> Result<String, DvdError> {
    let mut seed = fnv1a64(b"slideshow", FNV_OFFSET);
    seed = fnv1a64(CACHE_VERSION.as_bytes(), seed);
    // The slide renderer's version (2: M10.6 gave tall slides their right width).
    seed = fnv1a64(SLIDE_RENDER_VERSION.as_bytes(), seed);
    seed = fnv1a64(standard_tag(project.settings.standard).as_bytes(), seed);
    seed = fnv1a64(budget.video_bitrate.to_string().as_bytes(), seed);
    seed = fnv1a64(budget.max_bitrate.to_string().as_bytes(), seed);
    seed = fnv1a64(if budget.wide { b"16:9" } else { b"4:3" }, seed);
    seed = slideshow_seed(project, slideshow, seed)?;

    Ok(format!("{seed:016x}"))
}

/// `seed` with what a slideshow's pictures and audio are made of: the slides (files, lengths, fits, transitions),
/// the show's transition and its audio clips.
fn slideshow_seed(project: &Project, slideshow: &Slideshow, seed: u64) -> Result<u64, DvdError> {
    let mut seed = seed;
    seed = fnv1a64(slideshow.slides.len().to_string().as_bytes(), seed);
    seed = fnv1a64(slideshow.transition.as_str().as_bytes(), seed);
    seed = fnv1a64(slideshow.transition_duration.0.to_string().as_bytes(), seed);

    for slide in &slideshow.slides {
        if slide.duration.0 < 0 {
            return Err(DvdError::Invalid("slide duration is negative".to_string()));
        }

        let path = asset_path(project, slide.asset)?;
        let metadata = fs::metadata(path).map_err(|err| crate::io_error(path, err))?;
        let (secs, nanos) = modified_time(&metadata)?;

        seed = fnv1a64(absolute_path_string(path).as_bytes(), seed);
        seed = fnv1a64(metadata.len().to_string().as_bytes(), seed);
        seed = fnv1a64(secs.to_string().as_bytes(), seed);
        seed = fnv1a64(nanos.to_string().as_bytes(), seed);
        seed = fnv1a64(slide.duration.0.to_string().as_bytes(), seed);
        seed = fnv1a64(slide.scale.as_str().as_bytes(), seed);
        match slide.transition {
            Some(own) => {
                seed = fnv1a64(own.kind.as_str().as_bytes(), seed);
                seed = fnv1a64(own.duration.0.to_string().as_bytes(), seed);
            }
            None => seed = fnv1a64(b"show", seed),
        }
    }

    let clips = slideshow.audio_list();
    if clips.is_empty() {
        seed = fnv1a64(b"none", seed);
    }
    for asset in clips {
        let path = asset_path(project, asset)?;
        let metadata = fs::metadata(path).map_err(|err| crate::io_error(path, err))?;
        let (secs, nanos) = modified_time(&metadata)?;

        seed = fnv1a64(absolute_path_string(path).as_bytes(), seed);
        seed = fnv1a64(metadata.len().to_string().as_bytes(), seed);
        seed = fnv1a64(secs.to_string().as_bytes(), seed);
        seed = fnv1a64(nanos.to_string().as_bytes(), seed);
    }
    seed = fnv1a64(
        if slideshow.loop_audio {
            b"loop"
        } else {
            b"once"
        },
        seed,
    );
    seed = fnv1a64(
        if slideshow.fit_to_audio {
            b"fit"
        } else {
            b"own"
        },
        seed,
    );

    Ok(seed)
}

/// Return an absolute path string, falling back to the original path.
fn absolute_path_string(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Return modified time as seconds and nanoseconds since the UNIX epoch.
fn modified_time(metadata: &fs::Metadata) -> Result<(u64, u64), DvdError> {
    let time = metadata
        .modified()
        .map_err(|err| DvdError::Invalid(format!("cannot read modified time: {err}")))?;
    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();

    Ok((duration.as_secs(), u64::from(duration.subsec_nanos())))
}

/// Return the cache standard tag.
fn standard_tag(standard: VideoStandard) -> &'static str {
    match standard {
        VideoStandard::Ntsc => "ntsc",
        VideoStandard::Pal => "pal",
    }
}

/// Convert the project standard to the transcode standard.
fn transcode_standard(standard: VideoStandard) -> Standard {
    match standard {
        VideoStandard::Ntsc => Standard::Ntsc,
        VideoStandard::Pal => Standard::Pal,
    }
}

/// Transcode into cache part files and publish them atomically.
fn transcode_to_cache(
    asset_name: &str,
    input: &Path,
    cache_dir: &Path,
    key: &str,
    options: &TranscodeOptions,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<(PathBuf, Option<PathBuf>), DvdError> {
    fs::create_dir_all(cache_dir).map_err(|err| crate::io_error(cache_dir, err))?;

    let video_part = cache_dir.join(format!("{key}.{}.m2v.part", part_number()));
    let audio_part = cache_dir.join(format!("{key}.{}.wav.part", part_number()));

    let mut progress_cb = |fraction: f32| progress(asset_name, fraction);

    if let Err(err) = transcode(input, &video_part, &audio_part, options, &mut progress_cb) {
        remove_parts(&video_part, &audio_part);
        return Err(DvdError::Unsupported(format!(
            "transcode {}: {err}",
            input.display()
        )));
    }

    let video_out = cache_dir.join(format!("{key}.m2v"));
    let audio_out = cache_dir.join(format!("{key}.wav"));
    let marker = cache_dir.join(format!("{key}.ok"));

    if let Err(err) = fs::rename(&video_part, &video_out) {
        remove_parts(&video_part, &audio_part);
        return Err(crate::io_error(&video_part, err));
    }

    let transcoded_audio = if audio_part.is_file() {
        if let Err(err) = fs::rename(&audio_part, &audio_out) {
            remove_parts(&video_part, &audio_part);
            fs::remove_file(&video_out).unwrap_or(());
            return Err(crate::io_error(&audio_part, err));
        }
        Some(audio_out)
    } else {
        None
    };

    if let Err(err) = fs::write(&marker, b"ok") {
        cleanup_outputs(&video_out, transcoded_audio.as_deref());
        return Err(crate::io_error(&marker, err));
    }

    Ok((video_out, transcoded_audio))
}

/// Transcode one slideshow into cache part files and publish them atomically.
fn transcode_slideshow_to_cache(
    project: &Project,
    slideshow: &Slideshow,
    cache_dir: &Path,
    key: &str,
    budget: &Budget,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<(PathBuf, Option<PathBuf>), DvdError> {
    fs::create_dir_all(cache_dir).map_err(|err| crate::io_error(cache_dir, err))?;

    let video_part = cache_dir.join(format!("{key}.{}.m2v.part", part_number()));
    let audio_part = cache_dir.join(format!("{key}.{}.wav.part", part_number()));
    let options = TranscodeOptions {
        standard: transcode_standard(project.settings.standard),
        video_bitrate: budget.video_bitrate,
        max_bitrate: budget.max_bitrate,
        wide: budget.wide,
        gop_starts: Vec::new(),
    };

    let slide_inputs = slideshow_inputs(project, slideshow)?;
    let audio_paths = slideshow_audio_paths(project, slideshow)?;
    let name = slideshow.name.as_str();
    let transition = Transition {
        kind: transition_kind(slideshow.transition),
        seconds: slide_seconds(slideshow.transition_duration)?,
    };

    let mut progress_cb = |fraction: f32| progress(name, fraction);

    if let Err(err) = render_slideshow(
        &slide_inputs,
        transition,
        &audio_paths,
        slideshow.loop_audio,
        &video_part,
        &audio_part,
        &options,
        &mut progress_cb,
    ) {
        remove_parts(&video_part, &audio_part);
        return Err(DvdError::Unsupported(format!(
            "transcode slideshow {name}: {err}"
        )));
    }

    let video_out = cache_dir.join(format!("{key}.m2v"));
    let audio_out = cache_dir.join(format!("{key}.wav"));
    let marker = cache_dir.join(format!("{key}.ok"));

    if let Err(err) = fs::rename(&video_part, &video_out) {
        remove_parts(&video_part, &audio_part);
        return Err(crate::io_error(&video_part, err));
    }

    if !audio_part.is_file() {
        cleanup_outputs(&video_out, None);
        return Err(DvdError::Unsupported("slideshow audio missing".to_string()));
    }

    if let Err(err) = fs::rename(&audio_part, &audio_out) {
        remove_parts(&video_part, &audio_part);
        cleanup_outputs(&video_out, None);
        return Err(crate::io_error(&audio_part, err));
    }

    if let Err(err) = fs::write(&marker, b"ok") {
        cleanup_outputs(&video_out, Some(&audio_out));
        return Err(crate::io_error(&marker, err));
    }

    Ok((video_out, Some(audio_out)))
}

/// An HD Blu-ray disc's slideshow title: its pictures as H.264 of `options` (ROADMAP M10.6), encoded once and
/// kept in `cache_dir` (by the slides, the show's settings and the format). Returns the elementary stream's file
/// and its access-unit index's ([`disccraft_transcode::hd::read_h264_index`]).
///
/// # Errors
///
/// When a slide cannot be read or encoded, or the cache cannot be written.
pub fn encode_hd_slideshow(
    project: &Project,
    slideshow: &Slideshow,
    cache_dir: &Path,
    options: &disccraft_transcode::hd::HdOptions,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<(PathBuf, PathBuf), DvdError> {
    let mut seed = fnv1a64(b"hd-slideshow", FNV_OFFSET);
    seed = fnv1a64(SLIDE_RENDER_VERSION.as_bytes(), seed);
    let format = format!(
        "{}x{} {}/{} {} {}",
        options.width,
        options.height,
        options.fps_num,
        options.fps_den,
        options.target_kbps,
        options.max_kbps
    );
    seed = fnv1a64(format.as_bytes(), seed);
    seed = slideshow_seed(project, slideshow, seed)?;
    let es = cache_dir.join(format!("hd-{seed:016x}.h264"));
    let index = cache_dir.join(format!("hd-{seed:016x}.idx"));
    if es.is_file() && index.is_file() {
        return Ok((es, index));
    }

    fs::create_dir_all(cache_dir).map_err(|err| crate::io_error(cache_dir, err))?;
    let es_part = cache_dir.join(format!("hd-{seed:016x}.{}.h264.part", part_number()));
    let index_part = cache_dir.join(format!("hd-{seed:016x}.{}.idx.part", part_number()));
    let slides = slideshow_inputs(project, slideshow)?;
    let transition = Transition {
        kind: transition_kind(slideshow.transition),
        seconds: slide_seconds(slideshow.transition_duration)?,
    };
    let label = format!("HD: {}", slideshow.name);
    if let Err(err) = render_slideshow_hd(
        &slides,
        transition,
        (&es_part, &index_part),
        options,
        &mut |fraction| progress(&label, fraction),
    ) {
        remove_parts(&es_part, &index_part);
        return Err(DvdError::Unsupported(format!(
            "transcode slideshow {}: {err}",
            slideshow.name
        )));
    }
    // The index last: the stream is only taken with its index.
    if let Err(err) = fs::rename(&es_part, &es) {
        remove_parts(&es_part, &index_part);
        return Err(crate::io_error(&es_part, err));
    }
    if let Err(err) = fs::rename(&index_part, &index) {
        remove_parts(&es_part, &index_part);
        return Err(crate::io_error(&index_part, err));
    }
    Ok((es, index))
}

/// The renderer's transition kind for the project's.
fn transition_kind(kind: disccraft_project::TransitionKind) -> SlideTransition {
    match kind {
        disccraft_project::TransitionKind::None => SlideTransition::None,
        disccraft_project::TransitionKind::CrossDissolve => SlideTransition::CrossDissolve,
        disccraft_project::TransitionKind::DipToBlack => SlideTransition::DipToBlack,
        disccraft_project::TransitionKind::Wipe => SlideTransition::Wipe,
    }
}

fn slideshow_inputs(project: &Project, slideshow: &Slideshow) -> Result<Vec<SlideInput>, DvdError> {
    let mut inputs = Vec::with_capacity(slideshow.slides.len());

    let durations = effective_slide_durations(project, slideshow)?;
    for (slide, duration) in slideshow.slides.iter().zip(durations) {
        let path = asset_path(project, slide.asset)?;
        let seconds = slide_seconds(duration)?;
        inputs.push(SlideInput {
            image: path.to_path_buf(),
            seconds,
            fit: match slide.scale {
                disccraft_project::SlideScale::ScaleAndMatte => SlideFit::ScaleAndMatte,
                disccraft_project::SlideScale::ScaleAndCrop => SlideFit::ScaleAndCrop,
                disccraft_project::SlideScale::NoScale => SlideFit::NoScale,
            },
            transition: match slide.transition {
                Some(own) => Some(Transition {
                    kind: transition_kind(own.kind),
                    seconds: slide_seconds(own.duration)?,
                }),
                None => None,
            },
        });
    }

    Ok(inputs)
}

fn slideshow_audio_paths(
    project: &Project,
    slideshow: &Slideshow,
) -> Result<Vec<PathBuf>, DvdError> {
    slideshow
        .audio_list()
        .into_iter()
        .map(|asset| asset_path(project, asset).map(Path::to_path_buf))
        .collect()
}

/// The slides' durations as built: their own, or — with "Fit Slideshow to Audio Duration" — stretched or shrunk
/// so the show lasts as long as the audio clips together.
pub(crate) fn effective_slide_durations(
    project: &Project,
    slideshow: &Slideshow,
) -> Result<Vec<Tick>, DvdError> {
    let own: Vec<Tick> = slideshow
        .slides
        .iter()
        .map(|slide| slide.duration)
        .collect();
    if !slideshow.fit_to_audio {
        return Ok(own);
    }
    let clips = slideshow_audio_paths(project, slideshow)?;
    if clips.is_empty() {
        return Ok(own);
    }
    let mut audio_seconds = 0.0_f64;
    for clip in &clips {
        audio_seconds += probe_seconds(clip)
            .map_err(|err| DvdError::Invalid(format!("probe {}: {err}", clip.display())))?;
    }
    let total: f64 = own.iter().map(|tick| tick.0 as f64).sum();
    if !(audio_seconds.is_finite() && audio_seconds > 0.0) || total <= 0.0 {
        return Ok(own);
    }
    let scale = audio_seconds * TICKS_PER_SECOND_F64 / total;
    Ok(own
        .iter()
        .map(|tick| {
            // Scaled whole ticks; `as` saturates.
            Tick((tick.0 as f64 * scale).round() as i64)
        })
        .collect())
}

/// Ticks per second of [`Tick`], as a float for scaling.
const TICKS_PER_SECOND_F64: f64 = 254_016_000_000.0;

/// Transcode one audio asset into cache part files and publish it atomically.
fn transcode_audio_to_cache(
    input: &Path,
    cache_dir: &Path,
    key: &str,
) -> Result<PathBuf, DvdError> {
    fs::create_dir_all(cache_dir).map_err(|err| crate::io_error(cache_dir, err))?;

    let wav_out = cache_dir.join(format!("{key}.wav"));
    let marker = cache_dir.join(format!("{key}.ok"));

    if marker.is_file() && wav_out.is_file() {
        return Ok(wav_out);
    }

    let part = cache_dir.join(format!("{key}.{}.wav.part", part_number()));

    if let Err(err) = transcode_audio(input, &part, None) {
        fs::remove_file(&part).unwrap_or(());
        return Err(DvdError::Unsupported(format!(
            "transcode audio {}: {err}",
            input.display()
        )));
    }

    if let Err(err) = fs::rename(&part, &wav_out) {
        fs::remove_file(&part).unwrap_or(());
        return Err(crate::io_error(&part, err));
    }

    if let Err(err) = fs::write(&marker, b"ok") {
        fs::remove_file(&wav_out).unwrap_or(());
        return Err(crate::io_error(&marker, err));
    }

    Ok(wav_out)
}

/// The AC-3 soundtrack made from a cached transcode's WAV (Dolby Digital, as Encore transcodes audio), cached next to
/// it: `{wav stem}.ac3-1.ac3`, written through a part file so an interrupted encode is never taken for a finished one.
///
/// # Errors
///
/// [`DvdError::Unsupported`] when the WAV cannot be encoded, and I/O errors.
pub(crate) fn ac3_soundtrack(wav: &Path) -> Result<PathBuf, DvdError> {
    let stem = wav
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let out = wav.with_file_name(format!("{stem}.{AC3_CACHE_VERSION}.ac3"));
    if out.is_file() {
        return Ok(out);
    }
    let part = wav.with_file_name(format!(
        "{stem}.{AC3_CACHE_VERSION}.{}.ac3.part",
        part_number()
    ));
    if let Err(err) = disccraft_ac3enc::encode_wav(wav, &part) {
        fs::remove_file(&part).unwrap_or(());
        return Err(DvdError::Unsupported(format!(
            "AC-3 {}: {err}",
            wav.display()
        )));
    }
    if let Err(err) = fs::rename(&part, &out) {
        fs::remove_file(&part).unwrap_or(());
        return Err(crate::io_error(&part, err));
    }
    Ok(out)
}

/// Remove transcode part files, ignoring missing files.
fn remove_parts(video_part: &Path, audio_part: &Path) {
    fs::remove_file(video_part).unwrap_or(());
    fs::remove_file(audio_part).unwrap_or(());
}

/// Remove published cache outputs after a late failure.
fn cleanup_outputs(video_out: &Path, audio_out: Option<&Path>) {
    fs::remove_file(video_out).unwrap_or(());
    if let Some(path) = audio_out {
        fs::remove_file(path).unwrap_or(());
    }
}

/// Compute FNV-1a 64 over bytes starting from a seed.
fn fnv1a64(bytes: &[u8], seed: u64) -> u64 {
    let mut hash = seed;

    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}

#[cfg(test)]
mod tests {
    use super::{
        AC3_STEREO_BITRATE, AudioKind, Budget, LPCM_STEREO_BITRATE, MAX_MUX_VIDEO_BITRATE,
        TICKS_PER_SECOND, TranscodeStatus, VideoStandard, audio_cache_key, audio_kind,
        budget_with_probe, estimate_with_probe, fnv1a64, is_passthrough,
        max_bitrate_for_audio_bitrate, slideshow_cache_key, transcode_status,
    };
    use crate::DvdError;
    use disccraft_project::{Capacity, Project, Tick};
    use std::error::Error;
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::time::SystemTime;

    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

    #[test]
    fn fnv1a64_known_values() {
        assert_eq!(fnv1a64(b"", FNV_OFFSET), FNV_OFFSET);
        assert_eq!(fnv1a64(b"a", FNV_OFFSET), 0xaf63dc4c8601ec8c);
    }

    #[test]
    fn is_passthrough_checks_sequence_header() -> Result<(), Box<dyn Error>> {
        let path = temp_path("m2v");
        write_sequence_header(&path)?;

        assert!(is_passthrough(&path, VideoStandard::Ntsc));
        assert!(!is_passthrough(&path, VideoStandard::Pal));
        fs::remove_file(&path).unwrap_or(());

        let mp4 = temp_path("mp4");
        write_sequence_header(&mp4)?;

        assert!(!is_passthrough(&mp4, VideoStandard::Ntsc));
        fs::remove_file(&mp4).unwrap_or(());

        Ok(())
    }

    #[test]
    fn estimate_lists_the_size_of_each_timeline() -> Result<(), Box<dyn Error>> {
        let m2v = temp_path("m2v");
        write_sequence_header(&m2v)?;
        let size = fs::metadata(&m2v)?.len();

        let mut project = Project::default();
        let compliant = project.add_asset(&m2v)?;
        let movie = project.add_asset(Path::new("movie.mp4"))?;
        let as_is = project.add_timeline(compliant, None)?;
        let transcoded = project.add_timeline(movie, None)?;

        let estimate = estimate_with_probe(&project, &|_path: &Path| Ok::<f64, DvdError>(60.0));
        fs::remove_file(&m2v).unwrap_or(());
        let estimate = estimate?;

        let sizes: Vec<_> = estimate.timelines.clone();
        assert_eq!(sizes.first(), Some(&(as_is, size)));
        // 60 seconds at the video bitrate plus its AC-3 soundtrack.
        let expected =
            (60 * (u64::from(estimate.video_bitrate) + u64::from(AC3_STEREO_BITRATE))) / 8;
        assert_eq!(sizes.get(1), Some(&(transcoded, expected)));
        Ok(())
    }

    #[test]
    fn transcode_status_classifies_assets() -> Result<(), Box<dyn Error>> {
        let m2v = temp_path("m2v");
        write_sequence_header(&m2v)?;
        let mp3 = temp_path("mp3");
        fs::write(&mp3, b"not really mp3")?;
        let cache = temp_path("cache");
        fs::create_dir_all(&cache)?;

        let mut project = Project::default();
        let compliant = project.add_asset(&m2v)?;
        let other_video = project.add_asset(Path::new("missing-source.mp4"))?;
        let wav = project.add_asset(Path::new("narration.wav"))?;
        let music = project.add_asset(&mp3)?;
        let still = project.add_asset(Path::new("cover.png"))?;

        let status = transcode_status(&project, None);
        assert_eq!(status.get(&compliant), Some(&TranscodeStatus::Unneeded));
        assert_eq!(
            status.get(&other_video),
            Some(&TranscodeStatus::Untranscoded)
        );
        assert_eq!(status.get(&wav), Some(&TranscodeStatus::Unneeded));
        assert_eq!(status.get(&music), Some(&TranscodeStatus::Untranscoded));
        assert_eq!(status.get(&still), Some(&TranscodeStatus::NotApplicable));

        // A finished audio transcode (the file and its marker) in the cache.
        let key = audio_cache_key(&mp3)?;
        fs::write(cache.join(format!("{key}.wav")), b"")?;
        fs::write(cache.join(format!("{key}.ok")), b"")?;
        let status = transcode_status(&project, Some(&cache));
        assert_eq!(status.get(&music), Some(&TranscodeStatus::Transcoded));

        fs::remove_dir_all(&cache).unwrap_or(());
        fs::remove_file(&m2v).unwrap_or(());
        fs::remove_file(&mp3).unwrap_or(());
        Ok(())
    }

    #[test]
    fn budget_clamps_single_long_timeline() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        project.settings.capacity = Capacity::Dvd5;

        let asset = project.add_asset(Path::new("fake.mp4"))?;
        project.add_timeline(asset, None)?;

        let budget = budget_with_probe(&project, &|_path: &Path| Ok::<f64, DvdError>(3600.0))?;

        assert_eq!(budget.video_bitrate, 8_000_000);
        assert_eq!(
            budget.max_bitrate,
            MAX_MUX_VIDEO_BITRATE - AC3_STEREO_BITRATE
        );

        Ok(())
    }

    #[test]
    fn prepared_audio_kind_by_extension() {
        assert!(matches!(audio_kind(Path::new("track.ac3")), AudioKind::Ac3));
        assert!(matches!(audio_kind(Path::new("track.WAV")), AudioKind::Wav));
        assert!(matches!(
            audio_kind(Path::new("track.mp3")),
            AudioKind::Transcode
        ));
    }

    #[test]
    fn max_bitrate_two_lpcm_tracks() -> Result<(), Box<dyn Error>> {
        let audio_bitrate = 2 * LPCM_STEREO_BITRATE;
        assert_eq!(max_bitrate_for_audio_bitrate(audio_bitrate)?, 6_728_000);
        Ok(())
    }

    #[test]
    fn max_bitrate_five_lpcm_tracks_errors() {
        let audio_bitrate = 5 * LPCM_STEREO_BITRATE;
        assert!(max_bitrate_for_audio_bitrate(audio_bitrate).is_err());
    }

    #[test]
    fn slideshow_cache_key_changes_with_slide_duration() -> Result<(), Box<dyn Error>> {
        let path = temp_path("png");
        fs::write(&path, b"")?;

        let mut project = Project::default();
        let asset = project.add_asset(&path)?;
        let id = project.add_slideshow(None)?;
        project.add_slide(id, asset, Tick(2 * TICKS_PER_SECOND))?;

        let budget = Budget {
            video_bitrate: 6_000_000,
            max_bitrate: 9_800_000,
            wide: false,
            audio_bitrate: LPCM_STEREO_BITRATE,
        };

        let key1 = {
            let slideshow = project.slideshow(id).ok_or("missing slideshow")?;
            slideshow_cache_key(&project, slideshow, &budget)?
        };

        {
            let slideshow = project.slideshow_mut(id).ok_or("missing slideshow")?;
            let slide = slideshow.slides.first_mut().ok_or("missing slide")?;
            slide.duration = Tick(3 * TICKS_PER_SECOND);
        }

        let key2 = {
            let slideshow = project.slideshow(id).ok_or("missing slideshow")?;
            slideshow_cache_key(&project, slideshow, &budget)?
        };

        assert_ne!(key1, key2);
        fs::remove_file(&path).unwrap_or(());

        Ok(())
    }

    fn temp_path(ext: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();

        std::env::temp_dir().join(format!(
            "disccraft-prepare-{}-{}.{ext}",
            std::process::id(),
            nanos
        ))
    }

    fn write_sequence_header(path: &Path) -> Result<(), Box<dyn Error>> {
        let mut file = fs::File::create(path)?;
        file.write_all(&sequence_header_bytes())?;
        Ok(())
    }

    fn sequence_header_bytes() -> Vec<u8> {
        let mut bytes = vec![0x00, 0x00, 0x01, 0xB3];
        let width = 720u32;
        let height = 480u32;
        let aspect = 2u32;
        let rate = 4u32;
        let bit_rate = 24_500u32;

        push_u8(&mut bytes, width >> 4);
        push_u8(&mut bytes, ((width & 0x0F) << 4) | ((height >> 8) & 0x0F));
        push_u8(&mut bytes, height & 0xFF);
        push_u8(&mut bytes, (aspect << 4) | rate);
        push_u8(&mut bytes, bit_rate >> 10);
        push_u8(&mut bytes, (bit_rate >> 2) & 0xFF);
        push_u8(&mut bytes, (bit_rate & 0x03) << 6);

        bytes
    }

    fn push_u8(bytes: &mut Vec<u8>, value: u32) {
        bytes.push(u8::try_from(value).unwrap_or(0));
    }
}

/// Size estimate of a DVD build, without transcoding anything.
#[derive(Debug, Clone, PartialEq)]
pub struct Estimate {
    /// Disc capacity in bytes.
    pub capacity_bytes: u64,
    /// Bytes of material that is used as is (passthrough video and explicit audio files).
    pub passthrough_bytes: u64,
    /// Seconds of material that will be transcoded.
    pub transcode_seconds: f64,
    /// Video bitrate the transcoder will use (0 when nothing is transcoded).
    pub video_bitrate: u32,
    /// Estimated bytes of the finished title VOBs.
    pub estimated_bytes: u64,
    /// Estimated encoded bytes of each timeline (video and audio), in timeline order.
    pub timelines: Vec<(TimelineId, u64)>,
}

/// Estimate the disc usage of `project` (probes media durations; fails like a build would when the content cannot fit).
///
/// # Errors
///
/// Returns the same errors as the bitrate allocation of a build: unreadable or unsupported media, or content too long for the capacity.
pub fn estimate(project: &Project) -> Result<Estimate, DvdError> {
    let probe = |path: &Path| {
        probe_seconds(path)
            .map_err(|err| DvdError::Unsupported(format!("probe {}: {err}", path.display())))
    };
    estimate_with_probe(project, &probe)
}

/// [`estimate`] with the media durations from `probe`.
fn estimate_with_probe(
    project: &Project,
    probe: &dyn Fn(&Path) -> Result<f64, DvdError>,
) -> Result<Estimate, DvdError> {
    let scan = scan_project(project, probe)?;
    let budget = budget_with_probe(project, probe)?;
    let audio_bps = if scan.transcode_seconds > 0.0 {
        scan.audio_bits / scan.transcode_seconds
    } else {
        0.0
    };
    let transcode_bytes =
        (scan.transcode_seconds * (f64::from(budget.video_bitrate) + audio_bps) / 8.0) as u64;
    let mut timelines = Vec::with_capacity(project.timelines.len());
    for timeline in project.timelines.values() {
        let mut one = Scan::default();
        scan_timeline(project, timeline, probe, &mut one)?;
        let transcoded =
            (one.transcode_seconds * f64::from(budget.video_bitrate) + one.audio_bits) / 8.0;
        timelines.push((
            timeline.id,
            (one.passthrough_bits / 8).saturating_add(transcoded as u64),
        ));
    }
    Ok(Estimate {
        capacity_bytes: project.settings.capacity.bytes(),
        passthrough_bytes: scan.passthrough_bits / 8,
        transcode_seconds: scan.transcode_seconds,
        video_bitrate: budget.video_bitrate,
        estimated_bytes: (scan.passthrough_bits / 8).saturating_add(transcode_bytes),
        timelines,
    })
}

/// Whether a build uses an asset as is, transcodes it, or already has its transcode in the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscodeStatus {
    /// Not transcodable media (images, subtitles, other files).
    NotApplicable,
    /// Already DVD-compliant: used as is.
    Unneeded,
    /// The build will transcode it.
    Untranscoded,
    /// The transcode for the current settings is in the cache.
    Transcoded,
}

/// The transcode status of every asset in `project`. With `cache_dir` (the build's transcode cache), assets
/// whose transcode for the current bitrate budget is already cached are [`TranscodeStatus::Transcoded`].
#[must_use]
pub fn transcode_status(
    project: &Project,
    cache_dir: Option<&Path>,
) -> std::collections::BTreeMap<AssetId, TranscodeStatus> {
    // The video cache key depends on the bitrate budget of the whole project; without it nothing is "cached".
    let budget = cache_dir.and_then(|_| budget(project).ok());
    project
        .assets
        .values()
        .map(|asset| {
            let path = Path::new(asset.path.as_str());
            let status = match asset.kind {
                AssetKind::Video => video_status(project, path, cache_dir, budget.as_ref()),
                AssetKind::Audio => audio_status(path, cache_dir),
                AssetKind::Image | AssetKind::Subtitle | AssetKind::Other => {
                    TranscodeStatus::NotApplicable
                }
            };
            (asset.id, status)
        })
        .collect()
}

/// Status of a video asset: passthrough, or the cached `<key>.m2v` with its `.ok` marker.
fn video_status(
    project: &Project,
    path: &Path,
    cache_dir: Option<&Path>,
    budget: Option<&Budget>,
) -> TranscodeStatus {
    if is_passthrough(path, project.settings.standard) {
        return TranscodeStatus::Unneeded;
    }
    let cached = cache_dir
        .zip(budget)
        .and_then(|(dir, budget)| {
            let gop_starts = asset_gop_starts(project, path);
            let key = cache_key(
                project,
                path,
                budget,
                (asset_wide(project, path), &gop_starts),
            )
            .ok()?;
            Some(cached(dir, &key, "m2v"))
        })
        .unwrap_or(false);
    if cached {
        TranscodeStatus::Transcoded
    } else {
        TranscodeStatus::Untranscoded
    }
}

/// Status of an audio asset: AC-3 and WAV are used as is, anything else is converted to LPCM.
fn audio_status(path: &Path, cache_dir: Option<&Path>) -> TranscodeStatus {
    if audio_kind(path) != AudioKind::Transcode {
        return TranscodeStatus::Unneeded;
    }
    let cached = cache_dir
        .and_then(|dir| {
            let key = audio_cache_key(path).ok()?;
            Some(cached(dir, &key, "wav"))
        })
        .unwrap_or(false);
    if cached {
        TranscodeStatus::Transcoded
    } else {
        TranscodeStatus::Untranscoded
    }
}

/// Whether the cache holds a finished `<key>.<extension>` (written with its `<key>.ok` marker).
fn cached(dir: &Path, key: &str, extension: &str) -> bool {
    dir.join(format!("{key}.ok")).is_file() && dir.join(format!("{key}.{extension}")).is_file()
}
