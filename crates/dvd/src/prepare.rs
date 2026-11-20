//! Prepare DVD title media by choosing passthrough or cached transcodes.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use disccraft_project::{
    AssetId, AssetKind, BdVideo, Capacity, DiscKind, Project, Slideshow, Tick, Timeline,
    TimelineId, VideoStandard,
};
use disccraft_transcode::slideshow::{
    Motion, SlideFit, SlideInput, SlideTransition, Transition, render_slideshow,
    render_slideshow_audio, render_slideshow_hd,
};
use disccraft_transcode::{Standard, TranscodeOptions, probe_seconds, transcode, transcode_audio};

use crate::DvdError;

mod hd_budget;
pub use hd_budget::{HdBudget, hd_budget, hd_default_kbps};

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
/// What a Blu-ray's transport stream carries of its elementary streams, bits a second: its 48 Mbit/s less the
/// packets' overhead (M6.347).
pub(crate) const BD_STREAMS_BITRATE: u32 = 46_000_000;

const HEAD_BYTES: usize = 65_536;
const SEQUENCE_HEADER_BYTES: usize = 11;
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const CACHE_VERSION: &str = "disccraft-mpeg2venc-2";
const SLIDE_RENDER_VERSION: &str = "slides-3";
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
    /// DTS elementary stream passed through.
    Dts(PathBuf),
    /// MPEG-1 Layer II elementary stream passed through (a PAL DVD).
    Mp2(PathBuf),
    /// AC-3 demuxed from a program or transport stream, starting `delay` (27 MHz) after the title's first picture
    /// as it did there.
    DelayedAc3 { path: PathBuf, delay: i64 },
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
    /// How long after the first picture `transcoded_audio` starts (27 MHz): an AC-3 demuxed as it is keeps the delay
    /// it had in its program stream.
    pub(crate) soundtrack_delay: i64,
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
    Dts,
    Wav,
    Transcode,
}

/// Return whether a video asset can be passed through unchanged: an MPEG-2 video elementary stream (`.m2v` /
/// `.mpv`) or the video of an MPEG-2 program stream (`.mpg`, `.vob` and the like, whatever the name) whose sequence
/// header fits the disc. Such a video is repackaged, not encoded again.
pub(crate) fn is_passthrough(path: &Path, standard: VideoStandard) -> bool {
    let Some(data) = read_head(path) else {
        return false;
    };
    if disccraft_psmux::demux::is_program_stream(&data) {
        return program_stream_fits(path, standard);
    }
    has_passthrough_extension(path) && first_sequence_header_matches(&data, standard)
}

/// Whether the file at `path` is an MPEG-2 program stream (a `.mpg` / `.vob`): video and sound packed together.
fn is_program_stream_file(path: &Path) -> bool {
    read_head(path).is_some_and(|head| disccraft_psmux::demux::is_program_stream(&head))
}

/// Whether the program stream at `path` carries video that goes on a disc of `standard` as it is: the sequence
/// header of the start of its video is looked at, as for an elementary stream.
fn program_stream_fits(path: &Path, standard: VideoStandard) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut head = Vec::new();
    let limit = u64::try_from(HEAD_BYTES).unwrap_or(u64::MAX);
    disccraft_psmux::demux::demux(&mut file, Some(&mut head), None, Some(limit)).is_ok()
        && first_sequence_header_matches(&head, standard)
}

/// The video elementary stream and the AC-3 sound (when it has any) of the program stream at `path`, taken out
/// once into the cache: the video goes on the disc as it is (repackaged, not encoded again), and so does the
/// sound. `None` for the sound when the stream has no AC-3.
fn demuxed_streams(path: &Path, cache_dir: &Path) -> Result<(PathBuf, Option<PathBuf>), DvdError> {
    let metadata =
        disccraft_transcode::document_metadata(path).map_err(|err| crate::io_error(path, err))?;
    let (secs, nanos) = modified_time(&metadata)?;
    let mut seed = fnv1a64(absolute_path_string(path).as_bytes(), FNV_OFFSET);
    seed = fnv1a64(metadata.len().to_string().as_bytes(), seed);
    seed = fnv1a64(secs.to_string().as_bytes(), seed);
    seed = fnv1a64(nanos.to_string().as_bytes(), seed);
    seed = fnv1a64(b"demux", seed);
    seed = fnv1a64(CACHE_VERSION.as_bytes(), seed);
    let video = cache_dir.join(format!("{seed:016x}.demux.m2v"));
    let sound = cache_dir.join(format!("{seed:016x}.demux.ac3"));
    let marker = cache_dir.join(format!("{seed:016x}.demux.ok"));
    if !(marker.is_file() && video.is_file()) {
        fs::create_dir_all(cache_dir).map_err(|err| crate::io_error(cache_dir, err))?;
        let number = part_number();
        let video_part = cache_dir.join(format!("{seed:016x}.demux.{number}.m2v.part"));
        let sound_part = cache_dir.join(format!("{seed:016x}.demux.{number}.ac3.part"));
        let taken = (|| -> Result<disccraft_psmux::demux::Demuxed, DvdError> {
            use std::io::Write;
            let input = fs::File::open(path).map_err(|err| crate::io_error(path, err))?;
            let mut input = std::io::BufReader::with_capacity(1 << 20, input);
            let create = |part: &Path| {
                fs::File::create(part)
                    .map(|file| std::io::BufWriter::with_capacity(1 << 20, file))
                    .map_err(|err| crate::io_error(part, err))
            };
            let (mut video_out, mut sound_out) = (create(&video_part)?, create(&sound_part)?);
            let found = disccraft_psmux::demux::demux(
                &mut input,
                Some(&mut video_out),
                Some(&mut sound_out),
                None,
            )
            .map_err(|err| crate::io_error(path, err))?;
            video_out
                .flush()
                .map_err(|err| crate::io_error(&video_part, err))?;
            sound_out
                .flush()
                .map_err(|err| crate::io_error(&sound_part, err))?;
            Ok(found)
        })();
        let found = match taken {
            Ok(found) if found.video_bytes > 0 => found,
            other => {
                let _ = fs::remove_file(&video_part);
                let _ = fs::remove_file(&sound_part);
                return Err(other.err().unwrap_or_else(|| {
                    DvdError::Unsupported(format!(
                        "{}: no video in the program stream",
                        path.display()
                    ))
                }));
            }
        };
        fs::rename(&video_part, &video).map_err(|err| crate::io_error(&video, err))?;
        if found.ac3_bytes > 0 {
            fs::rename(&sound_part, &sound).map_err(|err| crate::io_error(&sound, err))?;
        } else {
            let _ = fs::remove_file(&sound_part);
            let _ = fs::remove_file(&sound);
        }
        fs::write(&marker, b"ok").map_err(|err| crate::io_error(&marker, err))?;
    }
    Ok((video.clone(), sound.is_file().then_some(sound)))
}

/// The file whose bytes are the video elementary stream of a video that goes on the disc as it is: the file itself,
/// or for a program stream its video taken out into the cache.
fn passthrough_video(path: &Path, cache_dir: &Path) -> Result<PathBuf, DvdError> {
    if is_program_stream_file(path) {
        Ok(demuxed_streams(path, cache_dir)?.0)
    } else {
        Ok(path.to_path_buf())
    }
}

/// The source's own sound as AC-3, for a title whose video is not transcoded with its sound: a program stream's
/// AC-3 as it is, any other sound decoded and encoded (kept in the cache). `None` when the video has no sound.
fn own_sound(input: &Path, cache_dir: &Path) -> Result<Option<PathBuf>, DvdError> {
    if is_program_stream_file(input)
        && let (_, Some(sound)) = demuxed_streams(input, cache_dir)?
    {
        return Ok(Some(sound));
    }
    let metadata =
        disccraft_transcode::document_metadata(input).map_err(|err| crate::io_error(input, err))?;
    let (secs, nanos) = modified_time(&metadata)?;
    let mut seed = fnv1a64(absolute_path_string(input).as_bytes(), FNV_OFFSET);
    seed = fnv1a64(metadata.len().to_string().as_bytes(), seed);
    seed = fnv1a64(secs.to_string().as_bytes(), seed);
    seed = fnv1a64(nanos.to_string().as_bytes(), seed);
    seed = fnv1a64(b"own sound", seed);
    seed = fnv1a64(CACHE_VERSION.as_bytes(), seed);
    let wav = cache_dir.join(format!("{seed:016x}.sound.wav"));
    let silent = cache_dir.join(format!("{seed:016x}.sound.none"));
    if wav.is_file() {
        return Ok(Some(ac3_soundtrack(&wav)?));
    }
    if silent.is_file() {
        return Ok(None);
    }
    fs::create_dir_all(cache_dir).map_err(|err| crate::io_error(cache_dir, err))?;
    let part = cache_dir.join(format!("{seed:016x}.sound.{}.wav.part", part_number()));
    // A program stream the media readers cannot open (a VCD's MPEG-1 system stream, M6.350): its MPEG audio taken
    // out by our own demultiplexer, and that converted.
    let decoded = transcode_audio(input, &part, None).or_else(|error| {
        if !is_program_stream_file(input) {
            return Err(error);
        }
        let mp2 = cache_dir.join(format!("{seed:016x}.sound.{}.mp2", part_number()));
        let taken = (|| -> std::io::Result<u64> {
            let mut reader = std::io::BufReader::new(fs::File::open(input)?);
            let mut writer = std::io::BufWriter::new(fs::File::create(&mp2)?);
            let written = disccraft_psmux::demux::demux_mpeg_audio(&mut reader, &mut writer)?;
            std::io::Write::flush(&mut writer)?;
            Ok(written)
        })();
        let converted = match taken {
            Ok(written) if written > 0 => transcode_audio(&mp2, &part, None),
            _ => Err(error),
        };
        let _ = fs::remove_file(&mp2);
        converted
    });
    match decoded {
        Ok(_) => {
            fs::rename(&part, &wav).map_err(|err| crate::io_error(&wav, err))?;
            Ok(Some(ac3_soundtrack(&wav)?))
        }
        // A video without sound (or one whose sound cannot be read): the title has no soundtrack of its own, as
        // after a transcode of such a video. Remembered, so the next build does not look again.
        Err(_) => {
            let _ = fs::remove_file(&part);
            fs::write(&silent, b"").map_err(|err| crate::io_error(&silent, err))?;
            Ok(None)
        }
    }
}

/// Whether a video asset's video goes on the disc as it is: a raw elementary stream ([`is_passthrough`]), or the
/// MPEG-2 of a program or transport stream that fits the standard (demuxed, not transcoded).
pub(crate) fn video_passes_through(path: &Path, standard: VideoStandard) -> bool {
    is_passthrough(path, standard) || container_video(path, standard).is_some()
}

/// The bytes and GOPs of a program or transport stream's MPEG-2 video that goes on the disc as it is.
pub(crate) fn container_video(
    path: &Path,
    standard: VideoStandard,
) -> Option<(u64, Vec<(f64, bool)>)> {
    crate::demux::probe(path, standard, DiscKind::Dvd)?
        .video
        .clone()
}

/// The bytes of a program or transport stream's AC-3 that goes on the disc as it is, and how long after the first
/// picture it starts (27 MHz).
fn container_ac3(path: &Path, kind: DiscKind) -> Option<(u64, i64)> {
    crate::demux::probe(path, VideoStandard::Ntsc, kind)?.ac3
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
    prepare_all_of(project, cache_dir, budget, progress, true, None)
}

/// Whether `timeline` plays `asset`: as one of its video clips or on an audio track.
pub(crate) fn timeline_uses(timeline: &Timeline, asset: AssetId) -> bool {
    timeline.videos().contains(&asset) || timeline.audio.iter().any(|track| track.asset == asset)
}

/// [`prepare_all`], the timelines' videos only when `timelines` (a Blu-ray in HD encodes them on its own). With
/// `only`, just the timelines that play that asset (Transcode Now of one asset), and no slideshows.
pub(crate) fn prepare_all_of(
    project: &Project,
    cache_dir: &Path,
    budget: &Budget,
    progress: &mut dyn FnMut(&str, f32),
    timelines: bool,
    only: Option<AssetId>,
) -> Result<(), DvdError> {
    // The titles' video is made elsewhere (HD Blu-ray titles): neither the timelines' nor the slideshows' DVD video
    // is wanted, but their sound is made here side by side (M6.381), so the titles loaded one by one afterwards
    // find it in the cache.
    if !timelines {
        let jobs: Vec<&Timeline> = project
            .timelines
            .values()
            .filter(|timeline| only.is_none_or(|asset| timeline_uses(timeline, asset)))
            .collect();
        let results = crate::parallel::run(&jobs, progress, |timeline, _report| {
            prepare_title_sound(project, timeline, cache_dir).map(|_| ())
        });
        for result in results {
            result?;
        }
        return Ok(());
    }
    enum Job<'a> {
        Timeline(&'a Timeline),
        Slideshow(&'a Slideshow),
    }
    // One job per video (timelines of the same video share its transcode) and per slideshow picture.
    let mut videos = std::collections::BTreeSet::new();
    let mut jobs: Vec<Job<'_>> = project
        .timelines
        .values()
        // Not the timelines when their video is made elsewhere (HD Blu-ray titles).
        .filter(|timeline| only.is_none_or(|asset| timeline_uses(timeline, asset)))
        .filter(|timeline| timelines && videos.insert(timeline.video))
        .map(Job::Timeline)
        .collect();
    let mut shows = std::collections::BTreeSet::new();
    for id in crate::slideshow_ids(project) {
        if let Some(slideshow) = project.slideshows.get(&id)
            && only.is_none()
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

/// File ▸ Render ▸ Slideshow: transcodes the slideshows (`only`: one of them) into `cache_dir` at the rate the
/// whole project gives them, as a build would. Returns how many.
///
/// # Errors
///
/// As [`prepare_slideshow`].
pub(crate) fn prepare_slideshows(
    project: &Project,
    cache_dir: &Path,
    budget: &Budget,
    progress: &mut dyn FnMut(&str, f32),
    only: Option<u64>,
) -> Result<usize, DvdError> {
    let shows: Vec<&Slideshow> = crate::slideshow_ids(project)
        .into_iter()
        .filter(|id| only.is_none_or(|wanted| *id == wanted))
        .filter_map(|id| project.slideshows.get(&id))
        .collect();
    let results = crate::parallel::run(&shows, progress, |slideshow, report| {
        prepare_slideshow(project, slideshow, cache_dir, budget, report)
    });
    for result in results {
        let _ = result?;
    }
    Ok(shows.len())
}

/// How a slide's pan and zoom (Encore's slide Effect) moves its picture in the build, and in the preview.
#[must_use]
pub fn slide_motion(effect: disccraft_project::SlideEffect) -> Motion {
    Motion {
        pan: match effect.pan {
            disccraft_project::SlidePan::None => (0, 0),
            disccraft_project::SlidePan::Left => (-1, 0),
            disccraft_project::SlidePan::Right => (1, 0),
            disccraft_project::SlidePan::Up => (0, -1),
            disccraft_project::SlidePan::Down => (0, 1),
        },
        zoom: match effect.zoom {
            disccraft_project::SlideZoom::In => 1,
            disccraft_project::SlideZoom::Out => -1,
            disccraft_project::SlideZoom::None => 0,
        },
    }
}

/// Stops a rendering step that is not a transcode when the build or Transcode Now was asked to stop.
pub(crate) fn check_stop() -> Result<(), DvdError> {
    if disccraft_transcode::stop_requested() {
        return Err(DvdError::Unsupported(
            disccraft_transcode::STOPPED.to_string(),
        ));
    }
    Ok(())
}

/// A number for a transcode's part files, so two transcodes running at once never write the same file (the one
/// that finishes last publishes its result; both are the same).
pub(crate) fn part_number() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// A short black MPEG-2 video in the cache: the DVD video of a title whose pictures are made elsewhere.
fn stand_in_video(
    cache_dir: &Path,
    standard: VideoStandard,
    wide: bool,
) -> Result<PathBuf, DvdError> {
    fs::create_dir_all(cache_dir).map_err(|err| crate::io_error(cache_dir, err))?;
    let stand_in = cache_dir.join(format!(
        "stand-in.{CACHE_VERSION}.{}.{}.m2v",
        standard_tag(standard),
        if wide { "wide" } else { "full" }
    ));
    if !stand_in.is_file() {
        let frames = match standard {
            VideoStandard::Ntsc => 15,
            VideoStandard::Pal => 12,
        };
        let part = cache_dir.join(format!("stand-in.{}.part", part_number()));
        fs::write(&part, black_video(frames, standard, wide)?)
            .map_err(|err| crate::io_error(&part, err))?;
        fs::rename(&part, &stand_in).map_err(|err| crate::io_error(&stand_in, err))?;
    }
    Ok(stand_in)
}

/// Prepares a slideshow whose pictures are made elsewhere (an HD Blu-ray title is encoded from the slides as
/// H.264): its sound as usual, and in place of the DVD video a short black stand-in. No MPEG-2 encode of the slides runs, where [`prepare_slideshow`] would spend
/// the time on one that is thrown away.
///
/// # Errors
///
/// I/O or invalid-project errors when cache or asset access fails, [`DvdError::Unsupported`] when the sound
/// cannot be made.
pub(crate) fn prepare_slideshow_without_video(
    project: &Project,
    slideshow: &Slideshow,
    cache_dir: &Path,
) -> Result<PreparedTitle, DvdError> {
    let standard = project.settings.standard;
    let stand_in = stand_in_video(cache_dir, standard, project.settings.widescreen)?;

    let mut seed = fnv1a64(b"slideshow-sound", FNV_OFFSET);
    seed = fnv1a64(CACHE_VERSION.as_bytes(), seed);
    seed = fnv1a64(standard_tag(standard).as_bytes(), seed);
    seed = slideshow_seed(project, slideshow, seed)?;
    let audio_out = cache_dir.join(format!("{seed:016x}.wav"));
    if !audio_out.is_file() {
        let part = cache_dir.join(format!("{seed:016x}.{}.wav.part", part_number()));
        let slides = slideshow_inputs(project, slideshow)?;
        let audio = slideshow_audio_paths(project, slideshow)?;
        if let Err(err) = render_slideshow_audio(
            &slides,
            &audio,
            slideshow.loop_audio,
            &part,
            transcode_standard(standard),
        ) {
            let _ = fs::remove_file(&part);
            return Err(DvdError::Unsupported(format!(
                "slideshow {} audio: {err}",
                slideshow.name
            )));
        }
        fs::rename(&part, &audio_out).map_err(|err| crate::io_error(&audio_out, err))?;
    }
    Ok(PreparedTitle {
        video_path: stand_in,
        transcoded_audio: Some(ac3_soundtrack(&audio_out)?),
        transcoded: true,
        soundtrack_delay: 0,
        audio_tracks: Vec::new(),
    })
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
    let mut prepared = prepare_title_as_made(project, timeline, cache_dir, budget, progress)?;
    // Video used as it is says the title's shape (M6.323): the title is 16:9 or 4:3 (the timeline's, as Interpret
    // Footage makes a new one), so its sequence headers say so too.
    if !prepared.transcoded {
        prepared.video_path = with_display_aspect(
            &prepared.video_path,
            timeline.wide(&project.settings),
            cache_dir,
        )?;
    }
    Ok(prepared)
}

/// The aspect_ratio_information (ISO/IEC 13818-2 6.2.2.1: 2 is 4:3, 3 is 16:9) and the width of the first sequence
/// header of the MPEG-2 video `path`, when one is in its first megabyte.
fn first_display_aspect(path: &Path) -> Result<Option<(u8, u16)>, DvdError> {
    use std::io::Read;
    let file = fs::File::open(path).map_err(|err| crate::io_error(path, err))?;
    let mut head = Vec::new();
    let _ = file
        .take(1 << 20)
        .read_to_end(&mut head)
        .map_err(|err| crate::io_error(path, err))?;
    Ok(head
        .windows(8)
        .find(|bytes| bytes.starts_with(&[0, 0, 1, 0xB3]))
        .and_then(|bytes| {
            let width = (u16::from(*bytes.get(4)?) << 4) | u16::from(*bytes.get(5)? >> 4);
            Some((*bytes.get(7)? >> 4, width))
        }))
}

/// The MPEG-2 video `path` with every sequence header saying the title's display aspect (`wide`: 16:9, else 4:3):
/// the file itself when it already does (or says neither, or is narrower than a 16:9 DVD picture may be), else a copy
/// in `cache_dir` with those four bits of each sequence header changed, named after the file and the aspect.
fn with_display_aspect(path: &Path, wide: bool, cache_dir: &Path) -> Result<PathBuf, DvdError> {
    let wanted: u8 = if wide { 3 } else { 2 };
    let Some((said, width)) = first_display_aspect(path)? else {
        return Ok(path.to_path_buf());
    };
    // Only 4:3 and 16:9 are a DVD's; a 352-wide picture cannot be 16:9.
    if said == wanted || !matches!(said, 2 | 3) || (wide && width < 704) {
        return Ok(path.to_path_buf());
    }
    let metadata = fs::metadata(path).map_err(|err| crate::io_error(path, err))?;
    let (secs, nanos) = modified_time(&metadata)?;
    let mut seed = fnv1a64(absolute_path_string(path).as_bytes(), FNV_OFFSET);
    seed = fnv1a64(metadata.len().to_string().as_bytes(), seed);
    seed = fnv1a64(secs.to_string().as_bytes(), seed);
    seed = fnv1a64(nanos.to_string().as_bytes(), seed);
    seed = fnv1a64(format!("display aspect {wanted}").as_bytes(), seed);
    let out = cache_dir.join(format!("{seed:016x}.aspect.m2v"));
    if out.is_file() {
        return Ok(out);
    }
    fs::create_dir_all(cache_dir).map_err(|err| crate::io_error(cache_dir, err))?;
    let part = cache_dir.join(format!("{seed:016x}.{}.aspect.part", part_number()));
    let written = rewrite_display_aspect(path, &part, wanted);
    if let Err(error) = written {
        let _ = fs::remove_file(&part);
        return Err(error);
    }
    fs::rename(&part, &out).map_err(|err| crate::io_error(&out, err))?;
    Ok(out)
}

/// Copies the MPEG-2 video `from` to `to` with the aspect_ratio_information of every sequence header set to
/// `aspect`, a megabyte at a time (a header split between two reads is carried over).
fn rewrite_display_aspect(from: &Path, to: &Path, aspect: u8) -> Result<(), DvdError> {
    use std::io::{Read, Write};
    // A sequence header's start code and the three bytes before its aspect: the fourth byte after them has it.
    const REACH: usize = 7;
    let mut input = fs::File::open(from).map_err(|err| crate::io_error(from, err))?;
    let mut output =
        std::io::BufWriter::new(fs::File::create(to).map_err(|err| crate::io_error(to, err))?);
    let mut window: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 1 << 20];
    loop {
        let read = input
            .read(&mut chunk)
            .map_err(|err| crate::io_error(from, err))?;
        window.extend_from_slice(chunk.get(..read).unwrap_or_default());
        let done = read == 0;
        let mut at = 0;
        while at + REACH < window.len() {
            if window.get(at..at + 4) == Some(&[0, 0, 1, 0xB3])
                && let Some(byte) = window.get_mut(at + REACH)
            {
                *byte = (aspect << 4) | (*byte & 0x0F);
            }
            at += 1;
        }
        // Everything but the last few bytes (a header may begin there) is done.
        let keep = if done { 0 } else { REACH.min(window.len()) };
        let ready = window.len() - keep;
        output
            .write_all(window.get(..ready).unwrap_or_default())
            .map_err(|err| crate::io_error(to, err))?;
        let _ = window.drain(..ready);
        if done {
            break;
        }
    }
    output.flush().map_err(|err| crate::io_error(to, err))
}

/// [`prepare_title`] as the video is made: transcoded, joined or taken as it is.
fn prepare_title_as_made(
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
    // The asset's own transcode setting (M6.251) takes the place of the rate that fits the disc.
    let budget = &match fixed_video_bitrate(project, timeline.video) {
        Some(fixed) => Budget {
            video_bitrate: fixed.min(budget.max_bitrate),
            ..*budget
        },
        None => *budget,
    };
    let media = asset.media_file();
    let input: &Path = &media;

    let audio_tracks = prepare_audio_tracks(project, timeline, cache_dir)?;

    if !timeline.more_video.is_empty()
        || !timeline.video_trims.is_empty()
        || timeline.has_video_gaps()
    {
        return Ok(PreparedTitle {
            video_path: joined_clips(project, timeline, cache_dir, budget, progress)?,
            transcoded_audio: None,
            transcoded: false,
            soundtrack_delay: 0,
            audio_tracks,
        });
    }

    // A program or transport stream (a DVD's own VOB, an .mpg) whose MPEG-2 fits the disc: demuxed, not transcoded
    // (unless a DVD preset of its own transcodes it all the same, M6.322). Its soundtrack (for a timeline without
    // audio tracks) is its AC-3 as it is when a DVD can carry it, else transcoded alone.
    if container_video(input, project.settings.standard).is_some()
        && !transcode_forced(project, input)
    {
        let video_path = crate::demux::demux_to_cache(input, cache_dir, crate::demux::Part::Video)?;
        let (transcoded_audio, soundtrack_delay) = match (
            timeline.audio.is_empty(),
            container_ac3(input, project.settings.kind),
        ) {
            (false, _) => (None, 0),
            (true, Some((_, delay))) => (
                Some(crate::demux::demux_to_cache(
                    input,
                    cache_dir,
                    crate::demux::Part::Ac3,
                )?),
                delay,
            ),
            (true, None) => (
                prepare_title_sound(project, timeline, cache_dir)?.transcoded_audio,
                0,
            ),
        };
        return Ok(PreparedTitle {
            video_path,
            transcoded_audio,
            transcoded: false,
            soundtrack_delay,
            audio_tracks,
        });
    }

    if passes_through(project, input) {
        // A program stream's video is taken out and used as it is; its sound comes along when the timeline has no
        // audio track of its own (as the sound of a transcoded video does).
        let transcoded_audio = if is_program_stream_file(input) && timeline.audio.is_empty() {
            own_sound(input, cache_dir)?
        } else {
            None
        };
        return Ok(PreparedTitle {
            video_path: passthrough_video(input, cache_dir)?,
            transcoded_audio,
            transcoded: false,
            soundtrack_delay: 0,
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
            true => Some(soundtrack_of(project, timeline.video, &audio_out)?),
            false => None,
        };
        return Ok(PreparedTitle {
            video_path: video_out,
            transcoded_audio,
            transcoded: true,
            soundtrack_delay: 0,
            audio_tracks,
        });
    }

    let options = TranscodeOptions {
        standard: transcode_standard(project.settings.standard),
        video_bitrate: budget.video_bitrate,
        max_bitrate: budget.max_bitrate,
        wide,
        gop_starts,
        mode: rate_mode(project, input),
        film: film_of(project, input),
        quality: quality_of(project, input),
        rendering: rendering_of(project, input),
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
        .map(|wav| soundtrack_of(project, timeline.video, &wav))
        .transpose()?;

    Ok(PreparedTitle {
        video_path,
        transcoded_audio,
        transcoded: true,
        soundtrack_delay: 0,
        audio_tracks,
    })
}

/// Whether asset `asset`'s converted sound goes on the disc as PCM (its transcode setting; a DVD's).
fn wants_pcm(project: &Project, asset: disccraft_project::AssetId) -> bool {
    project.settings.kind == DiscKind::Dvd
        && project
            .assets
            .get(&asset)
            .is_some_and(|asset| asset.transcode_audio == disccraft_project::TranscodeAudio::Pcm)
}

/// The clips of `track` joined as Dolby Digital without decoding them, when they are all AC-3 files of one form
/// (48 kHz, one bit rate and channel layout): each clip's part of its file cut at its frames (as a trimmed clip is),
/// placed where the clip starts by silent frames of the same form in between (to the nearest frame, 32 ms).
/// `None` when they are not (they are decoded and joined then). Kept in `cache_dir`, named by the clips.
///
/// # Errors
///
/// [`DvdError::Unsupported`] when a file cannot be read as AC-3, and I/O errors.
fn ac3_clip_sequence(
    project: &Project,
    track: &disccraft_project::AudioTrack,
    cache_dir: &Path,
) -> Result<Option<PathBuf>, DvdError> {
    use std::hash::{Hash, Hasher};
    use std::io::Write;
    const SAMPLES_PER_FRAME: u64 = 1536;
    let clips = track.clips();
    let mut files = Vec::with_capacity(clips.len());
    let mut form = None;
    for (_, clip) in &clips {
        let Some(asset) = project.assets.get(&clip.asset) else {
            return Ok(None);
        };
        let path = asset.media_file().to_path_buf();
        if audio_kind(&path) != AudioKind::Ac3 {
            return Ok(None);
        }
        let Some(own) =
            read_head(&path).and_then(|head| disccraft_psmux::es::ac3::first_frame_format(&head))
        else {
            return Ok(None);
        };
        if own.0 != 48_000 || form.is_some_and(|form| form != own) {
            return Ok(None);
        }
        form = Some(own);
        files.push((*clip, path));
    }
    let Some((_, kbps, acmod, lfe)) = form else {
        return Ok(None);
    };
    let frame = if project.settings.standard == VideoStandard::Ntsc {
        1001.0 / 30_000.0
    } else {
        1.0 / 25.0
    };
    fs::create_dir_all(cache_dir).map_err(|err| crate::io_error(cache_dir, err))?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (clip, path) in &files {
        (
            audio_cache_key(path)?,
            clip.start,
            clip.trim_start,
            clip.trim_end,
        )
            .hash(&mut hasher);
    }
    let out = cache_dir.join(format!("ac3seq{:016x}.ac3", hasher.finish()));
    if out.is_file() {
        return Ok(Some(out));
    }
    let silent = disccraft_ac3enc::silent_frame(acmod, lfe, kbps)
        .map_err(|err| DvdError::Unsupported(err.to_string()))?;
    let part = cache_dir.join(format!("ac3seq.{}.ac3.part", part_number()));
    let mut writer = std::io::BufWriter::with_capacity(
        1 << 20,
        fs::File::create(&part).map_err(|err| crate::io_error(&part, err))?,
    );
    // Where the stream has got to, in samples.
    let mut at = 0u64;
    let mut write = |writer: &mut std::io::BufWriter<fs::File>| -> Result<(), DvdError> {
        for (clip, path) in &files {
            let trimmed = clip.trim_start > 0 || clip.trim_end.is_some();
            let (cut, late) = if trimmed {
                trimmed_ac3(
                    path,
                    cache_dir,
                    &audio_cache_key(path)?,
                    0,
                    (
                        f64::from(clip.trim_start) * frame,
                        clip.trim_end.map(|end| f64::from(end) * frame),
                    ),
                )?
            } else {
                (path.clone(), 0)
            };
            let data = fs::read(&cut).map_err(|err| crate::io_error(&cut, err))?;
            let frames = disccraft_psmux::es::parse_ac3(&data, 0)
                .map_err(|err| DvdError::Unsupported(format!("{}: {err}", cut.display())))?
                .frames
                .len();
            // The first frame kept starts a little after the in point (less than a frame).
            let late = u64::try_from(late.max(0))
                .unwrap_or(0)
                .saturating_mul(48_000)
                / 27_000_000;
            let start = delay_samples(clip.start, project.settings.standard).saturating_add(late);
            let gap = start.saturating_sub(at);
            let silence = (gap + SAMPLES_PER_FRAME / 2) / SAMPLES_PER_FRAME;
            for _ in 0..silence {
                writer
                    .write_all(&silent)
                    .map_err(|err| crate::io_error(&part, err))?;
            }
            writer
                .write_all(&data)
                .map_err(|err| crate::io_error(&part, err))?;
            let written = silence.saturating_add(u64::try_from(frames).unwrap_or(0));
            at = at.saturating_add(written.saturating_mul(SAMPLES_PER_FRAME));
        }
        writer.flush().map_err(|err| crate::io_error(&part, err))
    };
    if let Err(err) = write(&mut writer) {
        drop(writer);
        fs::remove_file(&part).unwrap_or(());
        return Err(err);
    }
    drop(writer);
    fs::rename(&part, &out).map_err(|err| crate::io_error(&part, err))?;
    Ok(Some(out))
}

/// Whether every clip of `track` is a WAV file: its sound then stays LPCM when it is moved, trimmed or joined.
fn all_wav(project: &Project, track: &disccraft_project::AudioTrack) -> bool {
    track.clips().iter().all(|(_, clip)| {
        project
            .assets
            .get(&clip.asset)
            .is_some_and(|asset| audio_kind(&asset.media_file()) == AudioKind::Wav)
    })
}

/// An audio track made of the converted sound `wav` of asset `asset`: LPCM as it is when the asset asks for PCM
/// (and it is 48 kHz 16-bit), else Dolby Digital made from it.
fn converted_sound(
    project: &Project,
    asset: disccraft_project::AssetId,
    wav: PathBuf,
) -> Result<PreparedAudio, DvdError> {
    if wants_pcm(project, asset) && wav_is_disc_lpcm(&wav) {
        Ok(PreparedAudio::Wav(wav))
    } else {
        Ok(PreparedAudio::Ac3(ac3_soundtrack(&wav)?))
    }
}

/// The soundtrack a title takes from the converted sound `wav` of video asset `video`: the WAV itself as LPCM when
/// the asset asks for PCM (and it is 48 kHz 16-bit), else Dolby Digital made from it.
fn soundtrack_of(
    project: &Project,
    video: disccraft_project::AssetId,
    wav: &Path,
) -> Result<PathBuf, DvdError> {
    if wants_pcm(project, video) && wav_is_disc_lpcm(wav) {
        Ok(wav.to_path_buf())
    } else {
        ac3_soundtrack(wav)
    }
}

/// Prepare one timeline's sound only, for a Blu-ray title whose video is encoded in HD on its own: its audio tracks
/// and, when it has none, the video asset's soundtrack (AC-3) that the SD transcode would have brought.
///
/// # Errors
///
/// [`DvdError::Unsupported`] when the audio cannot be transcoded, and I/O or invalid-project errors.
pub(crate) fn prepare_title_sound(
    project: &Project,
    timeline: &Timeline,
    cache_dir: &Path,
) -> Result<PreparedTitle, DvdError> {
    let asset = project
        .assets
        .get(&timeline.video)
        .ok_or_else(|| DvdError::Invalid("referenced asset does not exist".to_string()))?;
    let media = asset.media_file();
    let input: &Path = &media;
    let audio_tracks = prepare_audio_tracks(project, timeline, cache_dir)?;
    // The source's own sound, as a transcode of the whole title would bring it: only a timeline of one clip.
    let has_sound = || {
        timeline.more_video.is_empty()
            && disccraft_transcode::probe_video(input).is_ok_and(|probe| probe.has_audio)
    };
    // A trimmed clip's sound is the same part of the file's (M10.21): a program or transport stream's AC-3 by
    // its frames (M12.22), any other decoded, cut and made AC-3.
    let trim = timeline.video_trim(0);
    let trimmed = timeline.audio.is_empty()
        && timeline.more_video.is_empty()
        && trim != disccraft_project::ClipTrim::default();
    let frame = if project.settings.standard == VideoStandard::Ntsc {
        1001.0 / 30_000.0
    } else {
        1.0 / 25.0
    };
    let part = (
        f64::from(trim.start) * frame,
        trim.end.map(|end| f64::from(end) * frame),
    );
    if trimmed && let Some((_, delay)) = container_ac3(input, project.settings.kind) {
        let ac3 = crate::demux::demux_to_cache(input, cache_dir, crate::demux::Part::Ac3)?;
        let key = audio_cache_key(input)?;
        let (cut, delay) = trimmed_ac3(&ac3, cache_dir, &key, delay, part)?;
        return Ok(PreparedTitle {
            video_path: input.to_path_buf(),
            transcoded_audio: Some(cut),
            transcoded: false,
            soundtrack_delay: delay,
            audio_tracks,
        });
    }
    if trimmed && has_sound() {
        let key = format!("{}-soundtrack", audio_cache_key(input)?);
        let wav = transcode_audio_to_cache(input, cache_dir, &key)?;
        let cut = trimmed_wav(&wav, cache_dir, &key, part.0, part.1)?;
        return Ok(PreparedTitle {
            video_path: input.to_path_buf(),
            transcoded_audio: Some(soundtrack_of(project, timeline.video, &cut)?),
            transcoded: false,
            soundtrack_delay: 0,
            audio_tracks,
        });
    }
    // A program or transport stream's AC-3 the disc can carry goes on it as it is, keeping its delay.
    let container = container_ac3(input, project.settings.kind);
    let (transcoded_audio, soundtrack_delay) = match (timeline.audio.is_empty(), container) {
        (true, Some((_, delay))) => (
            Some(crate::demux::demux_to_cache(
                input,
                cache_dir,
                crate::demux::Part::Ac3,
            )?),
            delay,
        ),
        (true, None) if has_sound() => {
            let key = format!("{}-soundtrack", audio_cache_key(input)?);
            (
                Some(soundtrack_of(
                    project,
                    timeline.video,
                    &transcode_audio_to_cache(input, cache_dir, &key)?,
                )?),
                0,
            )
        }
        _ => (None, 0),
    };
    Ok(PreparedTitle {
        video_path: input.to_path_buf(),
        transcoded_audio,
        transcoded: false,
        soundtrack_delay,
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
            soundtrack_delay: 0,
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
        soundtrack_delay: 0,
        audio_tracks: Vec::new(),
    })
}

/// Allocate bitrate using an injectable duration probe.
fn budget_with_probe(
    project: &Project,
    probe: &dyn Fn(&Path) -> Result<f64, DvdError>,
) -> Result<Budget, DvdError> {
    let capacity_bits = capacity_bits(project)?;
    let scan = scan_project(project, probe)?;

    let audio_bitrate = if scan.transcode_count == 0 {
        LPCM_STEREO_BITRATE
    } else {
        scan.largest_audio_bitrate
    };

    let max_bitrate = max_bitrate_for_audio_bitrate(audio_bitrate, project.settings.kind)?;

    if scan.transcode_count == 0 {
        return Ok(Budget {
            video_bitrate: 0,
            max_bitrate,
            wide: project.settings.widescreen,
            audio_bitrate,
        });
    }

    // Every transcoded title has a bit rate of its own: nothing is left to share out.
    let video_bitrate = if scan.transcode_seconds > 0.0 {
        allocate_video_bitrate(capacity_bits, &scan, max_bitrate)?
    } else {
        0
    };

    Ok(Budget {
        video_bitrate,
        max_bitrate,
        wide: project.settings.widescreen,
        audio_bitrate,
    })
}

/// The video bit rate (bit/s) asset `video` is transcoded at by its own transcode setting for the disc's kind (the
/// DVD setting on a DVD, the Blu-ray one on a Blu-ray); `None`: the rate that fits the disc.
pub(crate) fn fixed_video_bitrate(
    project: &Project,
    video: disccraft_project::AssetId,
) -> Option<u32> {
    let asset = project.assets.get(&video)?;
    let kbps = match project.settings.kind {
        DiscKind::Dvd => asset.transcode_kbps?,
        DiscKind::BluRay => asset.bluray_transcode.as_ref()?.kbps,
    };
    kbps.clamp(
        disccraft_project::MIN_TRANSCODE_KBPS,
        disccraft_project::MAX_TRANSCODE_KBPS,
    )
    .checked_mul(1000)
}

/// How the video of `input` is transcoded: its asset's own setting (Encore's quality presets), one-pass VBR for
/// anything else.
pub(crate) fn rate_mode(project: &Project, input: &Path) -> disccraft_transcode::RateMode {
    use disccraft_project::TranscodeMode;
    use disccraft_transcode::RateMode;
    let asset = project
        .assets
        .values()
        .find(|asset| asset.media_file().as_ref() == input);
    // A Blu-ray's: its own Blu-ray setting's (Encore's presets are one-pass VBR; a preset of the user's may take
    // two passes or keep the rate constant, M6.328).
    let mode = if project.settings.kind == DiscKind::BluRay {
        asset
            .and_then(|asset| asset.bluray_transcode.as_ref())
            .map(|setting| setting.mode)
    } else {
        asset.map(|asset| asset.transcode_mode)
    };
    match mode.unwrap_or_default() {
        TranscodeMode::Vbr => RateMode::Vbr,
        TranscodeMode::Vbr2Pass => RateMode::Vbr2Pass,
        TranscodeMode::Cbr => RateMode::Cbr,
    }
}

/// Whether the video of `input` is transcoded as film (Encore's NTSC DV 23.976 presets: 23.976 progressive pictures
/// shown by 3:2 pulldown): its asset's own setting, on an NTSC DVD.
pub(crate) fn film_of(project: &Project, input: &Path) -> bool {
    project.settings.kind == DiscKind::Dvd
        && project.settings.standard == VideoStandard::Ntsc
        && project
            .assets
            .values()
            .find(|asset| asset.media_file().as_ref() == input)
            .is_some_and(|asset| asset.transcode_film && asset.transcode_kbps.is_some())
}

/// How hard the transcode of `input` searches for motion (M6.326, Encore's quality): its asset's DVD preset's on a
/// DVD (with a rate of its own), its Blu-ray preset's on a Blu-ray (an SD one's MPEG-2); else the best.
pub(crate) fn quality_of(project: &Project, input: &Path) -> u8 {
    use disccraft_project::{BEST_QUALITY, MIN_QUALITY};
    let asset = project
        .assets
        .values()
        .find(|asset| asset.media_file().as_ref() == input);
    let quality = match project.settings.kind {
        DiscKind::Dvd => asset
            .filter(|asset| asset.transcode_kbps.is_some())
            .map(|asset| asset.transcode_quality),
        DiscKind::BluRay => asset
            .and_then(|asset| asset.bluray_transcode.as_ref())
            .map(|setting| setting.quality),
    };
    quality
        .unwrap_or(BEST_QUALITY)
        .clamp(MIN_QUALITY, BEST_QUALITY)
}

/// How the transcode of `input` makes its pictures (M6.327): its asset's Encore rendering checkboxes, for either
/// kind of disc.
pub(crate) fn rendering_of(project: &Project, input: &Path) -> disccraft_transcode::Rendering {
    project
        .assets
        .values()
        .find(|asset| asset.media_file().as_ref() == input)
        .map_or_else(Default::default, |asset| disccraft_transcode::Rendering {
            max_quality: asset.max_render_quality,
            frame_blend: asset.frame_blend,
        })
}

/// The bits of the disc the titles may take: its capacity less a 4 percent reserve and the ROM content (the files
/// that go on the disc beside the video).
fn capacity_bits(project: &Project) -> Result<u64, DvdError> {
    let bits = project
        .settings
        .capacity
        .bytes()
        .checked_mul(8)
        .and_then(|bits| bits.checked_mul(96))
        .and_then(|bits| bits.checked_div(100))
        .ok_or_else(|| DvdError::Invalid("disc capacity overflow".to_string()))?;
    let rom = rom_bytes(project)?.saturating_mul(8);
    bits.checked_sub(rom).ok_or_else(|| {
        DvdError::Invalid("the ROM content is larger than the disc capacity".to_string())
    })
}

/// The bytes the project's ROM content takes on the disc.
fn rom_bytes(project: &Project) -> Result<u64, DvdError> {
    let files = project.rom_files().map_err(DvdError::Invalid)?;
    Ok(disccraft_project::rom_disc_bytes(&files))
}

/// Return the muxer video bitrate ceiling after an audio bitrate: a DVD's 10.08 Mbit/s, or on a Blu-ray (M6.347)
/// what its transport stream leaves, at most the SD video's 9.8 Mbit/s.
fn max_bitrate_for_audio_bitrate(audio_bitrate: u32, kind: DiscKind) -> Result<u32, DvdError> {
    if kind == DiscKind::BluRay {
        return BD_STREAMS_BITRATE
            .checked_sub(audio_bitrate)
            .filter(|room| *room >= MIN_VIDEO_BITRATE)
            .map(|room| room.min(MAX_MUX_VIDEO_BITRATE))
            .ok_or_else(|| {
                DvdError::Invalid(format!(
                    "the sound ({audio_bitrate} bit/s) takes too much of the Blu-ray's stream"
                ))
            });
    }
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
    let video_path: &Path = &video_path;

    // More than one clip: joined as they are (the build refuses clips that would need a transcode).
    if !timeline.more_video.is_empty()
        || !timeline.video_trims.is_empty()
        || timeline.has_video_gaps()
        || passes_through(project, video_path)
    {
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
    let video_path: &Path = &video_path;
    for video in timeline.videos() {
        let path = asset_path(project, video)?;
        let path: &Path = &path;
        if passes_through(project, path) {
            // A container's video is its MPEG-2 alone.
            let video_bytes = match container_video(path, project.settings.standard) {
                Some((bytes, _)) => bytes,
                None => file_size(path)?,
            };
            scan.passthrough_bits = add_bits(scan.passthrough_bits, video_bytes)?;
        } else {
            // A clip of a joined timeline that needs a transcode: its picture takes part of the bitrate budget.
            scan.transcode_seconds += positive_seconds(probe(path)?)?;
            scan.transcode_count = scan
                .transcode_count
                .checked_add(1)
                .ok_or_else(|| DvdError::Invalid("too many timelines".to_string()))?;
        }
    }

    let mut seconds: Option<f64> = None;

    for track in &timeline.audio {
        let audio_path = asset_path(project, track.asset)?;
        let audio_path: &Path = &audio_path;

        match audio_kind(audio_path) {
            AudioKind::Ac3 | AudioKind::Dts | AudioKind::Wav => {
                scan.passthrough_bits = add_bits(scan.passthrough_bits, file_size(audio_path)?)?;
            }
            AudioKind::Transcode if container_ac3(audio_path, project.settings.kind).is_some() => {
                let bytes =
                    container_ac3(audio_path, project.settings.kind).map_or(0, |(bytes, _)| bytes);
                scan.passthrough_bits = add_bits(scan.passthrough_bits, bytes)?;
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
    scan.transcode_count = scan
        .transcode_count
        .checked_add(1)
        .ok_or_else(|| DvdError::Invalid("too many timelines".to_string()))?;

    let audio_bitrate = audio_bitrate_for_title(project, timeline, seconds)?;
    scan.largest_audio_bitrate = scan.largest_audio_bitrate.max(audio_bitrate);
    // A video with a bit rate of its own (M6.251) takes that much of the disc, whatever is left for the others.
    if let Some(fixed) = fixed_video_bitrate(project, timeline.video) {
        // `as` saturates; bits of a title.
        let bits = (seconds * (f64::from(fixed) + f64::from(audio_bitrate))).ceil() as u64;
        scan.passthrough_bits = scan.passthrough_bits.saturating_add(bits);
        return Ok(());
    }
    scan.transcode_seconds += seconds;
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
    // The video's own soundtrack, transcoded to AC-3 (or kept as PCM, as its asset asks).
    if timeline.audio.is_empty() {
        return Ok(if wants_pcm(project, timeline.video) {
            LPCM_STEREO_BITRATE
        } else {
            AC3_STEREO_BITRATE
        });
    }

    let mut total = 0u64;

    for track in &timeline.audio {
        let path = asset_path(project, track.asset)?;
        let path: &Path = &path;
        // A moved clip is made Dolby Digital; 24-bit / 96 kHz PCM goes on a DVD as it is, at its own rate. A sound
        // that is converted is LPCM when its asset asks for PCM.
        let converted = !track.is_whole()
            || !track.more.is_empty()
            || (audio_kind(path) == AudioKind::Transcode
                && container_ac3(path, project.settings.kind).is_none()
                && wav_is_fine_lpcm(path, project.settings.kind).is_none());
        // A track of several clips that are not all WAV is joined as Dolby Digital (M6.308): it counts so
        // (M6.346), not as the LPCM of its first clip's WAV.
        let mixed = !track.more.is_empty() && !all_wav(project, track);
        let bitrate = match wav_is_fine_lpcm(path, project.settings.kind)
            .or_else(|| dvd_mp2(path, project))
        {
            _ if converted && wants_pcm(project, track.asset) => LPCM_STEREO_BITRATE,
            _ if mixed => AC3_STEREO_BITRATE,
            Some(bitrate) if track.is_whole() => bitrate,
            _ => track_audio_bitrate(path, seconds)?,
        };
        total = total
            .checked_add(u64::from(bitrate))
            .ok_or_else(|| DvdError::Invalid("audio bitrate overflow".to_string()))?;
    }

    u32::try_from(total).map_err(|_| DvdError::Invalid("audio bitrate exceeds u32".to_string()))
}

/// Compute the bitrate contribution of one audio track.
fn track_audio_bitrate(path: &Path, seconds: f64) -> Result<u32, DvdError> {
    // A program or transport stream's AC-3 that goes on the disc as it is counts at its own rate.
    let ac3_bytes = match audio_kind(path) {
        AudioKind::Ac3 | AudioKind::Dts => Some(file_size(path)?),
        AudioKind::Transcode => container_ac3(path, DiscKind::Dvd).map(|(bytes, _)| bytes),
        AudioKind::Wav => None,
    };
    match (audio_kind(path), ac3_bytes) {
        (AudioKind::Ac3 | AudioKind::Dts, Some(bytes)) | (AudioKind::Transcode, Some(bytes)) => {
            let bits = bytes
                .checked_mul(8)
                .ok_or_else(|| DvdError::Invalid("audio size overflow".to_string()))?;
            let per_second_f64 = bits as f64 / seconds;

            if !per_second_f64.is_finite() || per_second_f64 > f64::from(u32::MAX) {
                return Err(DvdError::Invalid("audio bitrate overflow".to_string()));
            }

            u32::try_from(per_second_f64 as u64)
                .map_err(|_| DvdError::Invalid("audio bitrate exceeds u32".to_string()))
        }
        (AudioKind::Wav, _) => Ok(LPCM_STEREO_BITRATE),
        (AudioKind::Ac3 | AudioKind::Dts | AudioKind::Transcode, None) => Ok(AC3_STEREO_BITRATE),
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

/// The video of a timeline with more than one clip: the clips' elementary streams one after another, as one file
/// in the cache (a clip's sequence end code is left out, so the stream runs on into the next clip). Every clip
/// must be an MPEG-2 elementary stream of the disc's standard that is used as it is.
///
/// # Errors
///
/// [`DvdError::Unsupported`] when a clip would need a transcode.
fn joined_clips(
    project: &Project,
    timeline: &Timeline,
    cache_dir: &Path,
    budget: &Budget,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<PathBuf, DvdError> {
    let mut seed = fnv1a64(b"joined clips", FNV_OFFSET);
    let mut clips = Vec::new();
    let mut joined_size: Option<(u16, bool)> = None;
    for (index, video) in timeline.videos().into_iter().enumerate() {
        let source = asset_path(project, video)?;
        let source: &Path = &source;
        let trim = timeline.video_trim(index);
        // A clip that needs a transcode is transcoded on its own (as a timeline of that one video would be), and
        // its result is joined. Only a stream that is used as it is can be trimmed: its GOPs are known.
        let path: PathBuf = if passes_through(project, source) {
            passthrough_video(source, cache_dir)?
        } else {
            if trim != disccraft_project::ClipTrim::default() {
                return Err(DvdError::Unsupported(format!(
                    "timeline \"{}\": only an MPEG-2 video stream that needs no transcode can be trimmed \
                     ({} is not)",
                    timeline.name,
                    source.display()
                )));
            }
            let mut one = timeline.clone();
            one.video = video;
            one.more_video.clear();
            one.video_trims.clear();
            prepare_title(project, &one, cache_dir, budget, progress)?.video_path
        };
        let metadata = fs::metadata(&path).map_err(|err| crate::io_error(&path, err))?;
        let (secs, nanos) = modified_time(&metadata)?;
        seed = fnv1a64(absolute_path_string(&path).as_bytes(), seed);
        seed = fnv1a64(metadata.len().to_string().as_bytes(), seed);
        seed = fnv1a64(secs.to_string().as_bytes(), seed);
        seed = fnv1a64(nanos.to_string().as_bytes(), seed);
        seed = fnv1a64(
            format!("trim:{}:{:?}", trim.start, trim.end).as_bytes(),
            seed,
        );
        // The clips of one title share a picture size and coding (a title has one: MPEG-1 video is joined only to
        // MPEG-1, M6.350); a gap's black is made at full size.
        let (size, mpeg1) = if passes_through(project, source) {
            (
                dvd_picture_size(source, project.settings.standard).unwrap_or((720, 0)),
                is_mpeg1_video(source),
            )
        } else {
            ((720, 0), false)
        };
        let first_size = *joined_size.get_or_insert((size.0, mpeg1));
        if first_size != (size.0, mpeg1) || (timeline.has_video_gaps() && size.0 != 720) {
            return Err(DvdError::Unsupported(format!(
                "timeline \"{}\": its video clips must have the same picture width and coding (MPEG-1 or \
                 MPEG-2; 720 wide when there are gaps between them)",
                timeline.name
            )));
        }
        let gap = timeline.video_gap(index);
        if gap > 0 {
            seed = fnv1a64(
                format!("gap:{gap}:{}", timeline.wide(&project.settings)).as_bytes(),
                seed,
            );
        }
        clips.push((path, trim, gap));
    }
    seed = fnv1a64(CACHE_VERSION.as_bytes(), seed);
    let joined = cache_dir.join(format!("{seed:016x}.joined.m2v"));
    if joined.is_file() {
        return Ok(joined);
    }
    fs::create_dir_all(cache_dir).map_err(|err| crate::io_error(cache_dir, err))?;
    let part = cache_dir.join(format!("{seed:016x}.joined.{}.part", part_number()));
    let last = clips.len().saturating_sub(1);
    let written = (|| -> Result<(), DvdError> {
        use std::io::Write;
        let file = fs::File::create(&part).map_err(|err| crate::io_error(&part, err))?;
        let mut out = std::io::BufWriter::new(file);
        for (index, (path, trim, gap)) in clips.iter().enumerate() {
            // The empty stretch in front of the clip plays black.
            if *gap > 0 {
                let black = black_video(
                    *gap,
                    project.settings.standard,
                    timeline.wide(&project.settings),
                )?;
                out.write_all(&black)
                    .map_err(|err| crate::io_error(&part, err))?;
            }
            write_clip(path, *trim, index == last, &mut out)?;
        }
        out.flush().map_err(|err| crate::io_error(&part, err))
    })();
    if let Err(error) = written {
        let _ = fs::remove_file(&part);
        return Err(error);
    }
    fs::rename(&part, &joined).map_err(|err| crate::io_error(&joined, err))?;
    Ok(joined)
}

/// The most frames of black one gap is encoded as (three hours of NTSC; a longer gap is refused).
const MAX_GAP_FRAMES: u32 = 30 * 60 * 60 * 3;

/// `frames` pictures of black as an MPEG-2 elementary stream of the disc's standard (closed GOPs, its own sequence
/// header, no sequence end code): what plays in a gap between video clips.
fn black_video(frames: u32, standard: VideoStandard, wide: bool) -> Result<Vec<u8>, DvdError> {
    use disccraft_mpeg2venc::Frame420;
    use disccraft_mpeg2venc::encoder::{Encoder, EncoderConfig};
    if frames > MAX_GAP_FRAMES {
        return Err(DvdError::Unsupported(
            "a gap between video clips is too long".to_string(),
        ));
    }
    let (standard, height, gop_size) = match standard {
        VideoStandard::Ntsc => (disccraft_mpeg2venc::Standard::Ntsc, 480_u16, 15_u16),
        VideoStandard::Pal => (disccraft_mpeg2venc::Standard::Pal, 576, 12),
    };
    let encode = |error: disccraft_mpeg2venc::EncodeError| DvdError::Invalid(error.0);
    let mut encoder = Encoder::new(EncoderConfig {
        standard,
        gop_size,
        bitrate: 1_000_000,
        max_bitrate: 8_000_000,
        wide,
        format: disccraft_mpeg2venc::encoder::Format::Sd,
        pulldown: false,
    })
    .map_err(encode)?;
    let (width, luma) = (720_u16, usize::from(height) * 720);
    let black = Frame420 {
        width,
        height,
        y: vec![16; luma],
        cb: vec![128; luma / 4],
        cr: vec![128; luma / 4],
    };
    let mut stream = Vec::new();
    for _ in 0..frames {
        stream.extend_from_slice(&encoder.encode_frame(&black).map_err(encode)?);
    }
    Ok(stream)
}

/// Writes the trimmed part of the MPEG-2 elementary stream at `path` to `out`: the headers before its first GOP
/// (every clip brings its own sequence header), then the GOPs from the trim's start up to its end. A sequence end
/// code is left out unless this is the last clip (it would end the stream there), and a GOP that starts a trimmed
/// clip without being closed is marked as a broken link (its leading B pictures need the GOP before it, which is
/// gone).
fn write_clip(
    path: &Path,
    trim: disccraft_project::ClipTrim,
    is_last: bool,
    out: &mut impl std::io::Write,
) -> Result<(), DvdError> {
    use std::io::{Seek, SeekFrom};
    let io = |err: std::io::Error| crate::io_error(path, err);
    let mut input = fs::File::open(path).map_err(io)?;
    let mut length = input.metadata().map_err(io)?.len();
    if length >= 4 {
        let mut tail = [0_u8; 4];
        let _ = input.seek(SeekFrom::End(-4)).map_err(io)?;
        input.read_exact(&mut tail).map_err(io)?;
        let ends = tail == [0, 0, 1, 0xB7];
        // The end code goes when another clip follows, or when the clip's end is trimmed off.
        if ends && (!is_last || trim.end.is_some()) {
            length = length.saturating_sub(4);
        }
    }
    let _ = input.seek(SeekFrom::Start(0)).map_err(io)?;
    if trim == disccraft_project::ClipTrim::default() {
        let _ = std::io::copy(&mut input.take(length), out).map_err(io)?;
        return Ok(());
    }

    let gops = gop_offsets(path)?;
    let offset_of = |frame: u32| {
        gops.iter()
            .find(|(_, pictures, _)| *pictures == frame)
            .map(|(offset, _, _)| *offset)
    };
    // Where the clip stops: in front of the headers of the GOP that starts on the end frame.
    let end_of = |frame: u32| {
        gops.iter()
            .find(|(_, pictures, _)| *pictures == frame)
            .map(|(_, _, headers)| *headers)
    };
    let invalid = || {
        DvdError::Invalid(format!(
            "{}: the clip's trim is not on a GOP start",
            path.display()
        ))
    };
    let first = gops
        .first()
        .map(|(offset, _, _)| *offset)
        .ok_or_else(invalid)?;
    let from = offset_of(trim.start).ok_or_else(invalid)?;
    let to = match trim.end {
        Some(end) => end_of(end).unwrap_or(length).min(length),
        None => length,
    };
    if to <= from {
        return Err(invalid());
    }
    // The headers before the first GOP.
    let _ = std::io::copy(&mut (&mut input).take(first), out).map_err(io)?;
    let _ = input.seek(SeekFrom::Start(from)).map_err(io)?;
    let mut head = [0_u8; 8];
    let head_len = usize::try_from(to.saturating_sub(from).min(8)).unwrap_or(8);
    let head = head.get_mut(..head_len).ok_or_else(invalid)?;
    input.read_exact(head).map_err(io)?;
    // Group header: start code, 25 bits of time code, closed_gop, broken_link.
    if trim.start > 0
        && let Some(flags) = head.get_mut(7)
        && *flags & 0x40 == 0
    {
        *flags |= 0x20;
    }
    out.write_all(head).map_err(io)?;
    let rest = to
        .saturating_sub(from)
        .saturating_sub(u64::try_from(head_len).unwrap_or(8));
    let _ = std::io::copy(&mut input.take(rest), out).map_err(io)?;
    Ok(())
}

/// The GOPs of the MPEG-2 video elementary stream at `path`: the byte offset of each group start code, the
/// number of pictures before it (the frame the GOP starts on, in display order), and where the GOP's own headers
/// begin (the sequence header in front of the group start code when there is one, else the group start code).
fn gop_offsets(path: &Path) -> Result<Vec<(u64, u32, u64)>, DvdError> {
    let io = |err: std::io::Error| crate::io_error(path, err);
    let mut input = std::io::BufReader::new(fs::File::open(path).map_err(io)?);
    let mut gops = Vec::new();
    let mut pictures = 0_u32;
    // The sequence header seen since the last picture began.
    let mut header: Option<u64> = None;
    let mut window = [0xFF_u8; 3];
    let mut position = 0_u64;
    let mut buffer = vec![0_u8; 1 << 20];
    loop {
        let read = match input.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(io(error)),
        };
        for &byte in buffer.get(..read).unwrap_or_default() {
            if window == [0, 0, 1] {
                match byte {
                    0x00 => {
                        pictures = pictures.saturating_add(1);
                        header = None;
                    }
                    0xB3 => header = header.or(Some(position.saturating_sub(3))),
                    // The start code began three bytes back.
                    0xB8 => {
                        let start = position.saturating_sub(3);
                        gops.push((start, pictures, header.take().unwrap_or(start)));
                    }
                    _ => {}
                }
            }
            window = [window[1], window[2], byte];
            position = position.saturating_add(1);
        }
    }
    Ok(gops)
}

/// Return the file size in bytes.
fn file_size(path: &Path) -> Result<u64, DvdError> {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|err| crate::io_error(path, err))
}

/// Borrow the filesystem path for an asset id.
fn asset_path(project: &Project, id: AssetId) -> Result<std::borrow::Cow<'_, Path>, DvdError> {
    project
        .assets
        .get(&id)
        .map(disccraft_project::Asset::media_file)
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
        let media = asset.media_file();
        let path: &Path = &media;

        // Several clips on the track (M6.269): each is decoded and cut, and they are written one after the
        // other with silence between, as one Dolby Digital stream.
        if !track.more.is_empty() {
            // Clips that are all Dolby Digital of one form are joined by their frames, not decoded (M6.309).
            if let Some(ac3) = ac3_clip_sequence(project, track, cache_dir)? {
                out.push(PreparedAudio::Ac3(ac3));
                continue;
            }
            let wav = audio_clip_sequence(project, track, cache_dir)?;
            // Clips that are all PCM stay PCM, joined (M6.308).
            out.push(if all_wav(project, track) && wav_is_disc_lpcm(&wav) {
                PreparedAudio::Wav(wav)
            } else {
                converted_sound(project, track.asset, wav)?
            });
            continue;
        }

        // A Blu-ray's LPCM takes more than two channels on the speakers of a channel assignment (M6.347): a WAV
        // of others is converted like other sound files.
        let kind = match audio_kind(path) {
            // A disc also carries 24-bit and 96 kHz PCM (a Blu-ray 192 kHz) as LPCM, as it is.
            AudioKind::Transcode if wav_is_fine_lpcm(path, project.settings.kind).is_some() => {
                AudioKind::Wav
            }
            AudioKind::Wav
                if project.settings.kind != DiscKind::Dvd
                    && !read_head(path)
                        .and_then(|head| wav_format(&head))
                        .is_some_and(|(_, channels, _, _, mask)| bd_layout(channels, mask)) =>
            {
                AudioKind::Transcode
            }
            kind => kind,
        };
        // MPEG-1 Layer II is a PAL DVD's own audio: the file goes on the disc as it is.
        if track.is_whole() && dvd_mp2(path, project).is_some() {
            out.push(PreparedAudio::Mp2(path.to_path_buf()));
            continue;
        }
        // A clip moved along its track starts with silence, and a trimmed one (M6.254) is a part of its file:
        // the sound is decoded, cut, the silence put in front, and the whole made Dolby Digital (whatever it was:
        // a stream used as it is cannot start late by minutes).
        if !track.is_whole() {
            let key = audio_cache_key(path)?;
            let mut samples = delay_samples(track.start, project.settings.standard);
            let frame = if project.settings.standard == VideoStandard::Ntsc {
                1001.0 / 30_000.0
            } else {
                1.0 / 25.0
            };
            let part = (
                f64::from(track.trim_start) * frame,
                track.trim_end.map(|end| f64::from(end) * frame),
            );
            // The trim names the files made of the part.
            let part_key = match (track.is_trimmed(), track.trim_end) {
                (false, _) => key.clone(),
                (true, Some(end)) => format!("{key}.t{}-{end}", track.trim_start),
                (true, None) => format!("{key}.t{}-", track.trim_start),
            };
            let delayed = match kind {
                // Dolby Digital stays as it is: its frames of the part, and silent frames of its own form in
                // front.
                AudioKind::Ac3 => PreparedAudio::Ac3({
                    let cut = if track.is_trimmed() {
                        let (cut, late) = trimmed_ac3(path, cache_dir, &key, 0, part)?;
                        // The first frame kept starts a little after the in point (less than a frame).
                        let late = u64::try_from(late.max(0)).unwrap_or(0);
                        samples = samples.saturating_add(late.saturating_mul(48_000) / 27_000_000);
                        cut
                    } else {
                        path.to_path_buf()
                    };
                    if track.start > 0 {
                        delayed_ac3(&cut, cache_dir, &part_key, samples)?
                    } else {
                        cut
                    }
                }),
                AudioKind::Dts => {
                    return Err(DvdError::Unsupported(format!(
                        "{}: a DTS track cannot be moved or trimmed (it goes on the disc whole, from the start \
                         of its timeline)",
                        path.display()
                    )));
                }
                AudioKind::Wav | AudioKind::Transcode => {
                    let mut wav = transcode_audio_to_cache(path, cache_dir, &key)?;
                    if track.is_trimmed() {
                        wav = trimmed_wav(&wav, cache_dir, &key, part.0, part.1)?;
                    }
                    if track.start > 0 {
                        wav = delayed_wav(&wav, cache_dir, &part_key, samples)?;
                    }
                    // A WAV stays PCM, cut and with its silence in front (M6.308); other sound is made Dolby
                    // Digital (or PCM, as its asset asks).
                    if kind == AudioKind::Wav && wav_is_disc_lpcm(&wav) {
                        PreparedAudio::Wav(wav)
                    } else {
                        converted_sound(project, track.asset, wav)?
                    }
                }
            };
            out.push(delayed);
            continue;
        }
        let prepared = match kind {
            AudioKind::Ac3 => PreparedAudio::Ac3(path.to_path_buf()),
            AudioKind::Dts => PreparedAudio::Dts(path.to_path_buf()),
            AudioKind::Wav => PreparedAudio::Wav(path.to_path_buf()),
            AudioKind::Transcode => match container_ac3(path, project.settings.kind) {
                // The AC-3 of a program or transport stream, as it is, when a DVD can carry it.
                Some((_, delay)) => PreparedAudio::DelayedAc3 {
                    path: crate::demux::demux_to_cache(path, cache_dir, crate::demux::Part::Ac3)?,
                    delay,
                },
                // The AC-3 sound of a program stream read by the muxer's own demuxer.
                None if is_program_stream_file(path)
                    && let (_, Some(sound)) = demuxed_streams(path, cache_dir)? =>
                {
                    PreparedAudio::Ac3(sound)
                }
                None => {
                    let key = audio_cache_key(path)?;
                    let wav = transcode_audio_to_cache(path, cache_dir, &key)?;
                    converted_sound(project, track.asset, wav)?
                }
            },
        };

        out.push(prepared);
    }

    Ok(out)
}

/// The clips of `track` as one WAV file in `cache_dir`: each clip's part of its file where the clip is on the
/// timeline, silence where there is none. Where two clips overlap (the commands do not let them), the earlier
/// one sounds to its end.
fn audio_clip_sequence(
    project: &Project,
    track: &disccraft_project::AudioTrack,
    cache_dir: &Path,
) -> Result<PathBuf, DvdError> {
    use std::hash::{Hash, Hasher};
    let frame = if project.settings.standard == VideoStandard::Ntsc {
        1001.0 / 30_000.0
    } else {
        1.0 / 25.0
    };
    let mut parts = Vec::new();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut first_key = String::new();
    for (_, clip) in track.clips() {
        let asset = project
            .assets
            .get(&clip.asset)
            .ok_or_else(|| DvdError::Invalid("referenced asset does not exist".to_string()))?;
        let media = asset.media_file();
        let path: &Path = &media;
        if audio_kind(path) == AudioKind::Dts {
            return Err(DvdError::Unsupported(format!(
                "{}: a DTS file cannot be one of several clips on a track (it goes on the disc whole)",
                path.display()
            )));
        }
        let key = audio_cache_key(path)?;
        let mut wav = transcode_audio_to_cache(path, cache_dir, &key)?;
        let trimmed = clip.trim_start > 0 || clip.trim_end.is_some();
        if trimmed {
            wav = trimmed_wav(
                &wav,
                cache_dir,
                &key,
                f64::from(clip.trim_start) * frame,
                clip.trim_end.map(|end| f64::from(end) * frame),
            )?;
        }
        let part_key = match (trimmed, clip.trim_end) {
            (false, _) => key.clone(),
            (true, Some(end)) => format!("{key}.t{}-{end}", clip.trim_start),
            (true, None) => format!("{key}.t{}-", clip.trim_start),
        };
        // With its silence in front (none for a clip at the start: the file is then only written in the
        // plain form the joining reads).
        let samples = delay_samples(clip.start, project.settings.standard);
        parts.push(delayed_wav(&wav, cache_dir, &part_key, samples)?);
        (part_key.as_str(), samples).hash(&mut hasher);
        if first_key.is_empty() {
            first_key = key;
        }
    }
    joined_wav(
        &parts,
        cache_dir,
        &format!("{first_key}.seq{:016x}", hasher.finish()),
    )
}

/// The bytes in front of the samples of a WAV file as [`reshaped_wav`] writes it.
const PLAIN_WAV_HEAD: u64 = 44;

/// WAV files that all start at the same moment (as [`delayed_wav`] makes them), laid over one another as
/// `{name}.wav` in `cache_dir`: each sounds from where the ones before it have ended.
fn joined_wav(parts: &[PathBuf], cache_dir: &Path, name: &str) -> Result<PathBuf, DvdError> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let out = cache_dir.join(format!("{name}.wav"));
    if out.is_file() {
        return Ok(out);
    }
    let mut format: Option<[u8; 16]> = None;
    // Each part's file and how many bytes of samples it holds.
    let mut inputs = Vec::with_capacity(parts.len());
    for part in parts {
        let unsupported = |what: &str| DvdError::Unsupported(format!("{}: {what}", part.display()));
        let mut input = fs::File::open(part).map_err(|err| crate::io_error(part, err))?;
        let mut head = [0u8; 44];
        input
            .read_exact(&mut head)
            .map_err(|err| crate::io_error(part, err))?;
        let own: [u8; 16] = head
            .get(20..36)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| unsupported("no format"))?;
        if head.get(0..4) != Some(b"RIFF")
            || head.get(12..16) != Some(b"fmt ")
            || head.get(36..40) != Some(b"data")
        {
            return Err(unsupported("not a plain WAV file"));
        }
        if *format.get_or_insert(own) != own {
            return Err(unsupported(
                "its samples are stored another way than the other clips'",
            ));
        }
        let block = u64::from(u16::from_le_bytes([own[12], own[13]])).max(1);
        let size = input
            .metadata()
            .map_err(|err| crate::io_error(part, err))?
            .len()
            .saturating_sub(PLAIN_WAV_HEAD);
        inputs.push((part, input, size - size % block));
    }
    let format =
        format.ok_or_else(|| DvdError::Invalid("an audio track without clips".to_string()))?;
    let total = inputs.iter().map(|(_, _, size)| *size).max().unwrap_or(0);
    if total > u64::from(u32::MAX) - 36 {
        return Err(DvdError::Unsupported(
            "the track's sound is longer than a WAV file holds".to_string(),
        ));
    }
    let partial = cache_dir.join(format!("{name}.{}.wav.part", part_number()));
    let mut write = || -> std::io::Result<()> {
        let mut writer = std::io::BufWriter::with_capacity(1 << 20, fs::File::create(&partial)?);
        writer.write_all(b"RIFF")?;
        writer.write_all(&(36 + total as u32).to_le_bytes())?;
        writer.write_all(b"WAVEfmt ")?;
        writer.write_all(&16u32.to_le_bytes())?;
        writer.write_all(&format)?;
        writer.write_all(b"data")?;
        writer.write_all(&(total as u32).to_le_bytes())?;
        let mut written = 0u64;
        for (_, input, size) in &mut inputs {
            if *size <= written {
                continue;
            }
            input.seek(SeekFrom::Start(PLAIN_WAV_HEAD + written))?;
            let more = *size - written;
            let copied = std::io::copy(&mut input.take(more), &mut writer)?;
            if copied != more {
                return Err(std::io::Error::other("the sound ended early"));
            }
            written = *size;
        }
        writer.flush()
    };
    if let Err(err) = write() {
        fs::remove_file(&partial).unwrap_or(());
        return Err(crate::io_error(&partial, err));
    }
    fs::rename(&partial, &out).map_err(|err| crate::io_error(&partial, err))?;
    Ok(out)
}

/// The 48 kHz samples of `frames` frames of the disc's standard rate, to the nearest sample.
pub(crate) fn delay_samples(frames: u32, standard: VideoStandard) -> u64 {
    let frames = u64::from(frames);
    match standard {
        // 48000 * 1001 / 30000 samples a frame.
        VideoStandard::Ntsc => (frames * 48_000 * 1001 + 15_000) / 30_000,
        VideoStandard::Pal => frames * 1920,
    }
}

/// The AC-3 stream `path` with silence in front for `samples` samples (to the nearest frame of 1536), kept in
/// `cache_dir` as `{key}.delay{frames}f.ac3`. The silent frames are in the stream's own form (coding mode, LFE,
/// bit rate), so the stream is not decoded.
///
/// # Errors
///
/// [`DvdError::Unsupported`] when the file is not 48 kHz AC-3 or its form cannot be made, and I/O errors.
pub(crate) fn delayed_ac3(
    path: &Path,
    cache_dir: &Path,
    key: &str,
    samples: u64,
) -> Result<PathBuf, DvdError> {
    use std::io::Write;
    let frames = (samples + 768) / 1536;
    fs::create_dir_all(cache_dir).map_err(|err| crate::io_error(cache_dir, err))?;
    let out = cache_dir.join(format!("{key}.delay{frames}f.ac3"));
    if out.is_file() {
        return Ok(out);
    }
    let unsupported = |what: String| DvdError::Unsupported(format!("{}: {what}", path.display()));
    let head = read_head(path).ok_or_else(|| unsupported("cannot be read".to_string()))?;
    let (rate, kbps, acmod, lfe) = disccraft_psmux::es::ac3::first_frame_format(&head)
        .ok_or_else(|| unsupported("not an AC-3 stream".to_string()))?;
    if rate != 48_000 {
        return Err(unsupported(format!("AC-3 at {rate} Hz cannot start later")));
    }
    let silent = disccraft_ac3enc::silent_frame(acmod, lfe, kbps)
        .map_err(|err| unsupported(err.to_string()))?;
    let part = cache_dir.join(format!("{key}.delay{frames}f.{}.ac3.part", part_number()));
    let write = || -> std::io::Result<()> {
        let mut writer = std::io::BufWriter::with_capacity(1 << 20, fs::File::create(&part)?);
        for _ in 0..frames {
            writer.write_all(&silent)?;
        }
        let _ = std::io::copy(&mut fs::File::open(path)?, &mut writer)?;
        writer.flush()
    };
    if let Err(err) = write() {
        fs::remove_file(&part).unwrap_or(());
        return Err(crate::io_error(&part, err));
    }
    fs::rename(&part, &out).map_err(|err| crate::io_error(&part, err))?;
    Ok(out)
}

/// `wav` (a cached transcode: 16-bit PCM) with `samples` sample frames of silence in front, kept next to it in
/// `cache_dir` as `{key}.delay{samples}.wav`.
///
/// # Errors
///
/// [`DvdError::Unsupported`] when the file is not a PCM WAV or would pass a WAV's 4 GiB, and I/O errors.
pub(crate) fn delayed_wav(
    wav: &Path,
    cache_dir: &Path,
    key: &str,
    samples: u64,
) -> Result<PathBuf, DvdError> {
    reshaped_wav(
        wav,
        cache_dir,
        &format!("{key}.delay{samples}"),
        WavShape::Delay(samples),
    )
}

/// The part of `wav` (a cached transcode: 16-bit PCM) from `start` seconds up to `end` seconds (`None`: its end),
/// kept next to it in `cache_dir`: the sound of a video clip trimmed to that part.
///
/// # Errors
///
/// As [`delayed_wav`].
pub(crate) fn trimmed_wav(
    wav: &Path,
    cache_dir: &Path,
    key: &str,
    start: f64,
    end: Option<f64>,
) -> Result<PathBuf, DvdError> {
    // The times in milliseconds name the file.
    let ms = |seconds: f64| (seconds * 1000.0).round() as u64;
    let name = match end {
        Some(end) => format!("{key}.cut{}-{}", ms(start), ms(end)),
        None => format!("{key}.cut{}-", ms(start)),
    };
    reshaped_wav(wav, cache_dir, &name, WavShape::Cut(start, end))
}

/// The frames of the AC-3 stream `ac3`, whose first frame sounds `delay` (27 MHz) after the file's first
/// picture, that start from `start` seconds up to `end` seconds of the file (`None`: to its end): kept in
/// `cache_dir`, with how long after the trimmed clip's first picture the first of them starts (less than a
/// frame: 32 ms).
///
/// # Errors
///
/// [`DvdError::Unsupported`] when the stream is not AC-3 or no frame is left, and I/O errors.
fn trimmed_ac3(
    ac3: &Path,
    cache_dir: &Path,
    key: &str,
    delay: i64,
    (start, end): (f64, Option<f64>),
) -> Result<(PathBuf, i64), DvdError> {
    const CLOCK: f64 = 27_000_000.0;
    let data = fs::read(ac3).map_err(|err| crate::io_error(ac3, err))?;
    let stream = disccraft_psmux::es::parse_ac3(&data, 0)
        .map_err(|err| DvdError::Unsupported(format!("{}: {err}", ac3.display())))?;
    // `as` saturates; times are far inside the range.
    let from = (start.max(0.0) * CLOCK).round() as i64;
    let to = end.map(|end| (end.max(0.0) * CLOCK).round() as i64);
    let at = |frame: &disccraft_psmux::es::Ac3Frame| frame.pts.saturating_add(delay);
    let kept: Vec<&disccraft_psmux::es::Ac3Frame> = stream
        .frames
        .iter()
        .filter(|frame| at(frame) >= from && to.is_none_or(|to| at(frame) < to))
        .collect();
    let first = kept.first().ok_or_else(|| {
        DvdError::Unsupported(format!(
            "{}: the trimmed clip has no sound left",
            ac3.display()
        ))
    })?;
    let new_delay = at(first).saturating_sub(from);
    // The times in milliseconds name the file.
    let ms = |ticks: i64| ticks / 27_000;
    let out = cache_dir.join(match to {
        Some(to) => format!("{key}.cut{}-{}.ac3", ms(from), ms(to)),
        None => format!("{key}.cut{}-.ac3", ms(from)),
    });
    if out.is_file() {
        return Ok((out, new_delay));
    }
    let mut bytes = Vec::new();
    for frame in kept {
        let end = frame.offset.saturating_add(frame.len);
        bytes.extend_from_slice(data.get(frame.offset..end).unwrap_or_default());
    }
    let part = cache_dir.join(format!("{key}.cut.{}.ac3.part", part_number()));
    fs::write(&part, &bytes).map_err(|err| crate::io_error(&part, err))?;
    fs::rename(&part, &out).map_err(|err| crate::io_error(&part, err))?;
    Ok((out, new_delay))
}

/// What [`reshaped_wav`] makes of a WAV file.
#[derive(Clone, Copy)]
enum WavShape {
    /// Sample frames of silence in front.
    Delay(u64),
    /// Only the part between these seconds (to the end when `None`).
    Cut(f64, Option<f64>),
}

/// `wav` reshaped ([`WavShape`]), kept in `cache_dir` as `{name}.wav`.
fn reshaped_wav(
    wav: &Path,
    cache_dir: &Path,
    name: &str,
    shape: WavShape,
) -> Result<PathBuf, DvdError> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let out = cache_dir.join(format!("{name}.wav"));
    if out.is_file() {
        return Ok(out);
    }
    let unsupported = |what: &str| DvdError::Unsupported(format!("{}: {what}", wav.display()));
    let io = |err: std::io::Error| crate::io_error(wav, err);
    let mut input = fs::File::open(wav).map_err(io)?;
    let mut head = vec![0u8; 64 * 1024];
    let mut filled = 0usize;
    while filled < head.len() {
        let got = input
            .read(head.get_mut(filled..).unwrap_or_default())
            .map_err(io)?;
        if got == 0 {
            break;
        }
        filled += got;
    }
    head.truncate(filled);
    // The chunks of the RIFF file: "fmt " (how the samples are stored) and "data" (where they start).
    if head.get(0..4) != Some(b"RIFF") || head.get(8..12) != Some(b"WAVE") {
        return Err(unsupported("not a WAV file"));
    }
    let mut at = 12usize;
    let mut format: Option<[u8; 16]> = None;
    let (data_start, data_len) = loop {
        let name = head
            .get(at..at + 4)
            .ok_or_else(|| unsupported("no sound data"))?;
        let len = head
            .get(at + 4..at + 8)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
            .ok_or_else(|| unsupported("no sound data"))?;
        let body = at + 8;
        if name == b"data" {
            break (body as u64, u64::from(len));
        }
        if name == b"fmt " {
            format = head
                .get(body..body + 16)
                .and_then(|bytes| bytes.try_into().ok());
        }
        at = body
            .checked_add(len as usize)
            .and_then(|end| end.checked_add(len as usize & 1))
            .ok_or_else(|| unsupported("no sound data"))?;
    };
    let format = format.ok_or_else(|| unsupported("no format"))?;
    let block = u64::from(u16::from_le_bytes([format[12], format[13]]));
    if u16::from_le_bytes([format[0], format[1]]) != 1 || block == 0 {
        return Err(unsupported("not PCM"));
    }
    // What is really there (a file cut short has less than its header says).
    let size = input.metadata().map_err(io)?.len();
    let data_len = data_len.min(size.saturating_sub(data_start));
    let data_len = data_len - data_len % block;
    let sample_rate = f64::from(u32::from_le_bytes([
        format[4], format[5], format[6], format[7],
    ]));
    // Sample frames of silence in front, sample frames left out at the head, and how many are kept.
    let whole = data_len / block;
    // `as` saturates; times are far inside the range.
    let at = |seconds: f64| ((seconds.max(0.0) * sample_rate).round() as u64).min(whole);
    let (samples, skip, keep) = match shape {
        WavShape::Delay(samples) => (samples, 0, whole),
        WavShape::Cut(start, end) => {
            let skip = at(start);
            let keep = end.map_or(whole, at).saturating_sub(skip);
            (0, skip, keep)
        }
    };
    let data_start = data_start.saturating_add(skip.saturating_mul(block));
    let data_len = keep.saturating_mul(block);
    let silence = samples
        .checked_mul(block)
        .ok_or_else(|| unsupported("the clip is too far along"))?;
    let total = silence
        .checked_add(data_len)
        .filter(|total| *total <= u64::from(u32::MAX) - 36)
        .ok_or_else(|| unsupported("the sound with its silence is longer than a WAV file holds"))?;

    let part = cache_dir.join(format!("{name}.{}.wav.part", part_number()));
    let mut write = || -> std::io::Result<()> {
        let mut writer = std::io::BufWriter::with_capacity(1 << 20, fs::File::create(&part)?);
        writer.write_all(b"RIFF")?;
        writer.write_all(&(36 + total as u32).to_le_bytes())?;
        writer.write_all(b"WAVEfmt ")?;
        writer.write_all(&16u32.to_le_bytes())?;
        writer.write_all(&format)?;
        writer.write_all(b"data")?;
        writer.write_all(&(total as u32).to_le_bytes())?;
        let zeros = vec![0u8; 1 << 16];
        let mut left = silence;
        while left > 0 {
            let now = left.min(zeros.len() as u64) as usize;
            writer.write_all(zeros.get(..now).unwrap_or_default())?;
            left -= now as u64;
        }
        input.seek(SeekFrom::Start(data_start))?;
        let copied = std::io::copy(&mut (&mut input).take(data_len), &mut writer)?;
        if copied != data_len {
            return Err(std::io::Error::other("the sound ended early"));
        }
        writer.flush()
    };
    if let Err(err) = write() {
        fs::remove_file(&part).unwrap_or(());
        return Err(crate::io_error(&part, err));
    }
    fs::rename(&part, &out).map_err(|err| crate::io_error(&part, err))?;
    Ok(out)
}

/// Classify an audio asset by extension.
fn audio_kind(path: &Path) -> AudioKind {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("ac3") => AudioKind::Ac3,
        Some(ext) if ext.eq_ignore_ascii_case("dts") => AudioKind::Dts,
        // A WAV goes on the disc as LPCM only as 16-bit 48 kHz PCM; any other (44.1 kHz, 24-bit, float) is
        // converted like the other sound files.
        Some(ext) if ext.eq_ignore_ascii_case("wav") => {
            if !path.is_file() || wav_is_disc_lpcm(path) {
                AudioKind::Wav
            } else {
                AudioKind::Transcode
            }
        }
        _ => AudioKind::Transcode,
    }
}

/// The bitrate of a WAV file a disc carries as LPCM as it is although it is not 16-bit 48 kHz: integer PCM of 20
/// or 24 bits (20 carried as 24) or at 96 kHz (or both), with no more channels than a DVD stream's 6.144 Mbit/s
/// allow (96 kHz 24-bit is stereo). A Blu-ray's (M6.347) is also 192 kHz, up to 27.648 Mbit/s, on the speakers
/// of a channel assignment, an odd count of channels stored with one more. `None` for any other file.
pub(crate) fn wav_is_fine_lpcm(path: &Path, kind: DiscKind) -> Option<u32> {
    if !path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
    {
        return None;
    }
    let head = read_head(path)?;
    let (tag, channels, rate, bits, mask) = wav_format(&head)?;
    let bits = if bits == 20 { 24 } else { bits };
    let rates: &[u32] = match kind {
        DiscKind::Dvd => &[48_000, 96_000],
        DiscKind::BluRay => &[48_000, 96_000, 192_000],
    };
    let hi_res = tag == 1
        && rates.contains(&rate)
        && matches!(bits, 16 | 24)
        && !(rate == 48_000 && bits == 16);
    let bitrate = match kind {
        DiscKind::Dvd => {
            let bitrate = u64::from(rate) * u64::from(bits) * u64::from(channels);
            ((1..=8).contains(&channels) && bitrate <= 6_144_000).then_some(bitrate)?
        }
        DiscKind::BluRay => {
            let bitrate = bd_lpcm_bitrate(channels, rate, bits);
            (bd_layout(channels, mask) && bitrate <= disccraft_psmux::es::BD_LPCM_MAX_BITRATE)
                .then_some(bitrate)?
        }
    };
    hi_res.then(|| u32::try_from(bitrate).ok()).flatten()
}

/// Whether a Blu-ray's LPCM has a channel assignment for a WAV's `channels` on speakers `mask` (M6.347).
fn bd_layout(channels: u16, mask: u32) -> bool {
    u8::try_from(channels)
        .ok()
        .and_then(|channels| disccraft_m2tsmux::bd_lpcm_layout(channels, mask))
        .is_some()
}

/// The bits a second of a Blu-ray's LPCM stream of `channels` (an odd count stored with one more, mono as two).
fn bd_lpcm_bitrate(channels: u16, rate: u32, bits: u16) -> u64 {
    u64::from(rate) * u64::from(bits) * u64::from(channels.max(2).next_multiple_of(2))
}

/// The bit rate of an MPEG-1 Layer II file (`.mp2`, `.mpa`) that goes on the disc as it is: on a PAL DVD (whose
/// players all decode MPEG audio; an NTSC disc's need not), at 48 kHz. `None` for any other file or disc: the
/// file is then converted like other sound.
pub(crate) fn dvd_mp2(path: &Path, project: &Project) -> Option<u32> {
    if project.settings.kind != DiscKind::Dvd
        || project.settings.standard != VideoStandard::Pal
        || !path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("mp2") || ext.eq_ignore_ascii_case("mpa"))
    {
        return None;
    }
    let (rate, kbps, _) = disccraft_psmux::es::mpa::first_frame_format(&read_head(path)?)?;
    (rate == 48_000).then(|| kbps.checked_mul(1000)).flatten()
}

/// The bitrate of a WAV file's PCM (sampling frequency x word length x channels), from its `fmt ` chunk.
fn wav_bitrate(path: &Path) -> Option<u32> {
    let (_, channels, rate, bits, _) = wav_format(&read_head(path)?)?;
    u32::try_from(u64::from(rate) * u64::from(bits) * u64::from(channels)).ok()
}

/// Whether a WAV file holds what a disc carries as LPCM without conversion: integer PCM, 16 bits, 48 kHz, up to
/// 8 channels (read from its `fmt ` chunk).
fn wav_is_disc_lpcm(path: &Path) -> bool {
    let Some(head) = read_head(path) else {
        return false;
    };
    wav_format(&head).is_some_and(|(tag, channels, rate, bits, _)| {
        tag == 1 && (1..=8).contains(&channels) && rate == 48_000 && bits == 16
    })
}

/// The format tag (1 = integer PCM; the PCM sub-format of an extensible header counts as 1), channel count,
/// sample rate, bits per sample and speakers (an extensible header's channel mask, else 0) of a RIFF WAVE file's
/// `fmt ` chunk.
fn wav_format(data: &[u8]) -> Option<(u16, u16, u32, u16, u32)> {
    if data.get(..4)? != b"RIFF" || data.get(8..12)? != b"WAVE" {
        return None;
    }
    let u16_at = |at: usize| -> Option<u16> {
        Some(u16::from_le_bytes(
            data.get(at..at.checked_add(2)?)?.try_into().ok()?,
        ))
    };
    let u32_at = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(
            data.get(at..at.checked_add(4)?)?.try_into().ok()?,
        ))
    };
    let mut at = 12usize;
    loop {
        let id = data.get(at..at.checked_add(4)?)?;
        let size = usize::try_from(u32_at(at.checked_add(4)?)?).ok()?;
        let body = at.checked_add(8)?;
        if id == b"fmt " {
            let mut tag = u16_at(body)?;
            let mut mask = 0;
            // WAVE_FORMAT_EXTENSIBLE: the real format is the first two bytes of the sub-format.
            if tag == 0xFFFE && size >= 26 {
                tag = u16_at(body.checked_add(24)?)?;
                mask = u32_at(body.checked_add(20)?)?;
            }
            return Some((
                tag,
                u16_at(body.checked_add(2)?)?,
                u32_at(body.checked_add(4)?)?,
                u16_at(body.checked_add(14)?)?,
                mask,
            ));
        }
        // Chunks are padded to an even size.
        at = body.checked_add(size)?.checked_add(size % 2)?;
    }
}

/// Return whether the extension is an MPEG video elementary stream.
fn has_passthrough_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ["m2v", "mpv", "m1v"]
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
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
pub(crate) fn first_sequence_header_matches(data: &[u8], standard: VideoStandard) -> bool {
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
    sequence_header_size(slice, standard).is_some_and(|(width, _)| width == 720)
}

/// The picture size of a sequence header that goes on a DVD of `standard` as it is: one of the DVD sizes (720 or
/// 704 wide at full height; 352 wide at full or half height, 4:3 only), at the standard's frame rate, 4:3 or 16:9,
/// within the bit rate a DVD plays. `None` for anything else.
fn sequence_header_size(slice: &[u8], standard: VideoStandard) -> Option<(u16, u16)> {
    let (ifo_standard, want_rate) = match standard {
        VideoStandard::Ntsc => (disccraft_ifo::Standard::Ntsc, 4u8),
        VideoStandard::Pal => (disccraft_ifo::Standard::Pal, 3u8),
    };
    let byte = |index: usize| slice.get(index).copied();
    let (b0, b1, b2, b3) = (byte(4)?, byte(5)?, byte(6)?, byte(7)?);
    let (b4, b5, b6) = (byte(8)?, byte(9)?, byte(10)?);

    let width = (u16::from(b0) << 4) | (u16::from(b1) >> 4);
    let height = (u16::from(b1 & 0x0F) << 8) | u16::from(b2);
    let aspect = b3 >> 4;
    let rate = b3 & 0x0F;
    let bit_rate = (u32::from(b4) << 10) | (u32::from(b5) << 2) | (u32::from(b6) >> 6);

    let resolution = disccraft_ifo::Resolution::of_picture(width, height, ifo_standard)?;
    let small = matches!(
        resolution,
        disccraft_ifo::Resolution::Half | disccraft_ifo::Resolution::Quarter
    );
    let fits = (aspect == 2 || (aspect == 3 && !small))
        && rate == want_rate
        && bit_rate
            .checked_mul(400)
            .is_some_and(|bits| bits <= MAX_MUX_VIDEO_BITRATE);
    fits.then_some((width, height))
}

/// The picture size in the first sequence header of `data`, when it goes on a DVD as it is (MPEG-1 video too,
/// M6.350).
fn first_sequence_header_size(data: &[u8], standard: VideoStandard) -> Option<(u16, u16)> {
    let pos = data
        .windows(4)
        .position(|window| window == [0x00, 0x00, 0x01, 0xB3])?;
    let slice = data.get(pos..pos.saturating_add(SEQUENCE_HEADER_BYTES))?;
    if is_mpeg1_at(data, pos) {
        return mpeg1_header_size(slice, standard);
    }
    sequence_header_size(slice, standard)
}

/// Whether the sequence header at `pos` of `data` is MPEG-1's: MPEG-2's is followed at once by its sequence
/// extension (start code `00 00 01 B5`), MPEG-1's by anything else (ISO/IEC 13818-2 §6.2.2: a sequence header
/// without its extension is ISO/IEC 11172-2). Its quantiser matrices hold no zero byte, so the next start code is
/// the next `00 00 01`.
fn is_mpeg1_at(data: &[u8], pos: usize) -> bool {
    let after = pos.saturating_add(SEQUENCE_HEADER_BYTES + 1);
    data.get(after..)
        .and_then(|rest| {
            rest.windows(4)
                .find(|window| window.starts_with(&[0, 0, 1]))
        })
        .is_some_and(|code| code.get(3) != Some(&0xB5))
}

/// The picture size of an MPEG-1 sequence header that goes on a DVD of `standard` as it is (M6.350): 352x240 at
/// 29.97 or 352x288 at 25 frames a second, up to 1.856 Mbit/s (any pixel aspect ratio: the title is 4:3).
fn mpeg1_header_size(slice: &[u8], standard: VideoStandard) -> Option<(u16, u16)> {
    mpeg1_header(slice, standard)
        .filter(|(_, bit_rate)| u64::from(*bit_rate) * 400 <= crate::source::MPEG1_MAX_BITRATE)
        .map(|(size, _)| size)
}

/// The picture size and bit rate field (units of 400 bit/s) of an MPEG-1 sequence header whose picture goes on a
/// DVD of `standard` (352x240 at 29.97 or 352x288 at 25 frames a second), whatever its rate.
fn mpeg1_header(slice: &[u8], standard: VideoStandard) -> Option<((u16, u16), u32)> {
    let (height, want_rate) = match standard {
        VideoStandard::Ntsc => (240, 4u8),
        VideoStandard::Pal => (288, 3u8),
    };
    let byte = |index: usize| slice.get(index).copied();
    let (b0, b1, b2, b3) = (byte(4)?, byte(5)?, byte(6)?, byte(7)?);
    let (b4, b5, b6) = (byte(8)?, byte(9)?, byte(10)?);
    let size = (
        (u16::from(b0) << 4) | (u16::from(b1) >> 4),
        (u16::from(b1 & 0x0F) << 8) | u16::from(b2),
    );
    let bit_rate = (u32::from(b4) << 10) | (u32::from(b5) << 2) | (u32::from(b6) >> 6);
    let fits = size == (352, height) && b3 & 0x0F == want_rate && b3 >> 4 != 0;
    fits.then_some((size, bit_rate))
}

/// The picture size of an MPEG-1 video marked variable (M6.362) whose first sequence header in `head` (its head)
/// fits a DVD of `standard`: `None` for anything else. Whether its rate fits is measured over the whole stream
/// ([`measured_mpeg1_rate`]).
fn variable_mpeg1_size(head: &[u8], standard: VideoStandard) -> Option<(u16, u16)> {
    let pos = head
        .windows(4)
        .position(|window| window == [0x00, 0x00, 0x01, 0xB3])?;
    if !is_mpeg1_at(head, pos) {
        return None;
    }
    let slice = head.get(pos..pos.saturating_add(SEQUENCE_HEADER_BYTES))?;
    mpeg1_header(slice, standard)
        .filter(|(_, bit_rate)| *bit_rate == crate::source::MPEG1_VARIABLE_RATE)
        .map(|(size, _)| size)
}

/// Counts the bytes of each picture of an MPEG-1 / MPEG-2 video elementary stream written to it (from one picture
/// start code `00 00 01 00` to the next).
struct PictureMeter {
    /// The last four bytes seen.
    tail: u32,
    /// The bytes since the current picture's start code began.
    since: u64,
    started: bool,
    sizes: Vec<u64>,
}

impl PictureMeter {
    fn new() -> Self {
        Self {
            tail: u32::MAX,
            since: 0,
            started: false,
            sizes: Vec::new(),
        }
    }

    /// Each picture's bytes, once the whole stream is written.
    fn sizes(mut self) -> Vec<u64> {
        if self.started {
            self.sizes.push(self.since);
        }
        self.sizes
    }
}

impl std::io::Write for PictureMeter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        for byte in data {
            self.since = self.since.saturating_add(1);
            self.tail = (self.tail << 8) | u32::from(*byte);
            if self.tail == 0x0000_0100 {
                if self.started {
                    self.sizes.push(self.since.saturating_sub(4));
                }
                self.started = true;
                self.since = 4;
            }
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The measured peak rate (bit/s) of the video at `path` (an elementary stream, or the video of a program
/// stream), read whole once and remembered by the file's path, size and time while the program runs.
fn measured_mpeg1_rate(path: &Path, standard: VideoStandard) -> Option<u64> {
    type Key = (PathBuf, u64, Option<std::time::SystemTime>, bool);
    static MEASURED: std::sync::Mutex<Option<std::collections::HashMap<Key, Option<u64>>>> =
        std::sync::Mutex::new(None);
    let metadata = fs::metadata(path).ok()?;
    let key = (
        path.to_path_buf(),
        metadata.len(),
        metadata.modified().ok(),
        standard == VideoStandard::Ntsc,
    );
    if let Some(known) = MEASURED
        .lock()
        .ok()
        .and_then(|cache| cache.as_ref().and_then(|cache| cache.get(&key).copied()))
    {
        return known;
    }
    let measure = || -> Option<u64> {
        let mut file = fs::File::open(path).ok()?;
        let mut meter = PictureMeter::new();
        let head = read_head(path)?;
        if disccraft_psmux::demux::is_program_stream(&head) {
            let _ = disccraft_psmux::demux::demux(&mut file, Some(&mut meter), None, None).ok()?;
        } else {
            let _ = std::io::copy(&mut std::io::BufReader::new(file), &mut meter).ok()?;
        }
        Some(crate::source::peak_bit_rate(meter.sizes(), standard))
    };
    let measured = measure();
    if let Ok(mut cache) = MEASURED.lock() {
        let cache = cache.get_or_insert_with(std::collections::HashMap::new);
        if cache.len() > 256 {
            cache.clear();
        }
        let _ = cache.insert(key, measured);
    }
    measured
}

/// Whether the video at `path` (an elementary stream, or the video of a program stream) is MPEG-1 (M6.350).
fn is_mpeg1_video(path: &Path) -> bool {
    let Some(mut data) = read_head(path) else {
        return false;
    };
    if disccraft_psmux::demux::is_program_stream(&data) {
        let Ok(mut file) = fs::File::open(path) else {
            return false;
        };
        let mut head = Vec::new();
        let limit = u64::try_from(HEAD_BYTES).unwrap_or(u64::MAX);
        if disccraft_psmux::demux::demux(&mut file, Some(&mut head), None, Some(limit)).is_err() {
            return false;
        }
        data = head;
    }
    data.windows(4)
        .position(|window| window == [0x00, 0x00, 0x01, 0xB3])
        .is_some_and(|pos| is_mpeg1_at(&data, pos))
}

/// The bit rate (bit/s) in the first sequence header of the video at `path` (an elementary stream, or the video of
/// a program stream): the most the stream says it uses. An MPEG-1 video marked variable is measured (M6.362).
fn header_bit_rate(path: &Path, standard: VideoStandard) -> Option<u32> {
    let mut data = read_head(path)?;
    if disccraft_psmux::demux::is_program_stream(&data) {
        let mut file = fs::File::open(path).ok()?;
        let mut head = Vec::new();
        let limit = u64::try_from(HEAD_BYTES).unwrap_or(u64::MAX);
        disccraft_psmux::demux::demux(&mut file, Some(&mut head), None, Some(limit)).ok()?;
        data = head;
    }
    let at = data
        .windows(4)
        .position(|window| window == [0x00, 0x00, 0x01, 0xB3])?;
    let byte = |index: usize| data.get(at.checked_add(index)?).copied().map(u32::from);
    let rate = (byte(8)? << 10) | (byte(9)? << 2) | (byte(10)? >> 6);
    if rate == crate::source::MPEG1_VARIABLE_RATE && is_mpeg1_at(&data, at) {
        return measured_mpeg1_rate(path, standard).and_then(|rate| u32::try_from(rate).ok());
    }
    rate.checked_mul(400)
}

/// What the streams of a timeline ask of a DVD each second, for a timeline whose video goes on the disc as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelineRate {
    /// The timeline's id.
    pub timeline: u64,
    /// The video's bit rate (the highest of its clips' sequence headers), in bit/s.
    pub video: u32,
    /// The audio tracks' bit rates added up, in bit/s.
    pub audio: u32,
}

impl TimelineRate {
    /// The most a DVD's video, audio and subtitles may use together, in bit/s.
    pub const LIMIT: u32 = MAX_MUX_VIDEO_BITRATE;

    /// Video and audio together.
    #[must_use]
    pub fn total(&self) -> u32 {
        self.video.saturating_add(self.audio)
    }
}

/// The stream rates of the timelines whose video goes on a DVD as it is (the transcoded ones get a rate that
/// fits, from the disc's budget). Timelines whose files cannot be read are left out.
#[must_use]
pub fn timeline_rates(project: &Project) -> Vec<TimelineRate> {
    project
        .timelines
        .values()
        .filter_map(|timeline| {
            let video = timeline
                .videos()
                .iter()
                .filter_map(|id| {
                    let path = asset_path(project, *id).ok()?;
                    let path: &Path = &path;
                    passes_through(project, path)
                        .then(|| header_bit_rate(path, project.settings.standard))
                        .flatten()
                })
                .max()?;
            let audio = if timeline.audio.is_empty() {
                // The sound a program stream brings along; an elementary stream has none.
                let own = asset_path(project, timeline.video).is_ok_and(|path| {
                    timeline.more_video.is_empty() && is_program_stream_file(&path)
                });
                if own { AC3_STEREO_BITRATE } else { 0 }
            } else {
                timeline
                    .audio
                    .iter()
                    .filter_map(|track| asset_path(project, track.asset).ok())
                    .map(|path| match audio_kind(&path) {
                        AudioKind::Ac3 => read_head(&path)
                            .and_then(|head| {
                                disccraft_psmux::es::ac3::first_frame_bit_rate_kbps(&head)
                            })
                            .and_then(|kbps| kbps.checked_mul(1000))
                            .unwrap_or(AC3_STEREO_BITRATE),
                        AudioKind::Dts => read_head(&path)
                            .and_then(|head| {
                                disccraft_psmux::es::dts::first_frame_bit_rate_kbps(&head)
                            })
                            .and_then(|kbps| kbps.checked_mul(1000))
                            .unwrap_or(1_509_750),
                        // LPCM at its own rate (its channels, word length and sampling frequency).
                        AudioKind::Wav => wav_bitrate(&path).unwrap_or(LPCM_STEREO_BITRATE),
                        AudioKind::Transcode => wav_is_fine_lpcm(&path, project.settings.kind)
                            .or_else(|| dvd_mp2(&path, project))
                            .unwrap_or(AC3_STEREO_BITRATE),
                    })
                    .fold(0u32, u32::saturating_add)
            };
            Some(TimelineRate {
                timeline: timeline.id.0,
                video,
                audio,
            })
        })
        .collect()
}

/// The picture size of a video that goes on a DVD as it is (an elementary stream, or the video of a program
/// stream): 720 wide, or one of the smaller DVD sizes. `None` when the video would be transcoded.
fn dvd_picture_size(path: &Path, standard: VideoStandard) -> Option<(u16, u16)> {
    let data = read_head(path)?;
    if disccraft_psmux::demux::is_program_stream(&data) {
        let mut file = fs::File::open(path).ok()?;
        let mut head = Vec::new();
        let limit = u64::try_from(HEAD_BYTES).unwrap_or(u64::MAX);
        disccraft_psmux::demux::demux(&mut file, Some(&mut head), None, Some(limit)).ok()?;
        return first_sequence_header_size(&head, standard)
            .or_else(|| variable_mpeg1_fits(path, &head, standard));
    }
    if !has_passthrough_extension(path) {
        return None;
    }
    first_sequence_header_size(&data, standard)
        .or_else(|| variable_mpeg1_fits(path, &data, standard))
}

/// The picture size of an MPEG-1 video marked variable ([`variable_mpeg1_size`]) whose measured rate a DVD
/// carries (M6.362).
fn variable_mpeg1_fits(path: &Path, head: &[u8], standard: VideoStandard) -> Option<(u16, u16)> {
    let size = variable_mpeg1_size(head, standard)?;
    measured_mpeg1_rate(path, standard)
        .filter(|rate| *rate <= crate::source::MPEG1_MAX_BITRATE)
        .map(|_| size)
}

/// Whether the video at `path` goes on the project's disc as it is: it fits ([`fits_as_is`]) and is not transcoded
/// all the same (a DVD quality preset of its own, as Encore transcodes a DVD-compliant asset a preset is picked for).
pub(crate) fn passes_through(project: &Project, path: &Path) -> bool {
    fits_as_is(project, path) && !transcode_forced(project, path)
}

/// Whether the video at `path` would go on the disc transcoded although it fits as it is: its asset has a setting
/// of its own for the disc's kind (a DVD or a Blu-ray quality preset; Encore's "Don't Transcode" is no setting).
pub(crate) fn transcode_forced(project: &Project, path: &Path) -> bool {
    project.assets.values().any(|asset| {
        asset.media_file().as_ref() == path
            && match project.settings.kind {
                DiscKind::Dvd => asset.transcode_kbps.is_some(),
                DiscKind::BluRay => asset.bluray_transcode.is_some(),
            }
    })
}

/// Whether video asset `asset` fits the project's disc as it is (so its setting may be "Don't Transcode", or a
/// preset that transcodes it all the same): an SD stream, or on an HD Blu-ray video of the disc's format.
#[must_use]
pub fn video_fits_as_is(project: &Project, asset: AssetId) -> bool {
    video_fits_disc(project, asset, project.settings.kind)
}

/// Whether video asset `asset` would fit a disc of `kind` as it is, with the project's TV standard and Blu-ray
/// format: on a DVD an MPEG-2 stream of a DVD picture size; on an HD Blu-ray video of its format; on an SD
/// Blu-ray a 720-wide MPEG-2 stream.
#[must_use]
pub fn video_fits_disc(project: &Project, asset: AssetId, kind: DiscKind) -> bool {
    let Some(found) = project
        .assets
        .get(&asset)
        .filter(|found| found.kind == AssetKind::Video)
    else {
        return false;
    };
    let media = found.media_file();
    let path: &Path = &media;
    let standard = project.settings.standard;
    match kind {
        DiscKind::Dvd => {
            video_passes_through(path, standard) || dvd_picture_size(path, standard).is_some()
        }
        // In the asset's own HD format (M6.325), else the project's.
        DiscKind::BluRay => video_fits_bluray_as(project, asset, project.asset_bd_video(asset)),
    }
}

/// Whether video asset `asset` would fit a Blu-ray title in `video` as it is (the Transcode Settings dialog asks
/// for a format being chosen, M6.325): an SD format takes a 720-wide MPEG-2 stream of the TV standard.
#[must_use]
pub fn video_fits_bluray_as(project: &Project, asset: AssetId, video: BdVideo) -> bool {
    let Some(found) = project
        .assets
        .get(&asset)
        .filter(|found| found.kind == AssetKind::Video)
    else {
        return false;
    };
    let media = found.media_file();
    match crate::BdVideoFormat::of_video(video) {
        Some(format) => crate::video_fits_bd(&media, format),
        None => video_passes_through(&media, project.settings.standard),
    }
}

/// Whether the video at `path` fits the project's disc as it is. A DVD takes the smaller DVD picture sizes too
/// (704x480, 352x480, 352x240 and their 576 / 288 line counterparts); a Blu-ray's SD video is 720 wide only.
pub(crate) fn fits_as_is(project: &Project, path: &Path) -> bool {
    let standard = project.settings.standard;
    // An elementary stream, or the MPEG-2 of a program or transport stream that fits the standard.
    if video_passes_through(path, standard) {
        return true;
    }
    match project.settings.kind {
        disccraft_project::DiscKind::Dvd => dvd_picture_size(path, standard).is_some(),
        _ => false,
    }
}

/// Compute a 16 hex digit cache key for a transcode (`wide`: the title's aspect, and `gop_starts`: the frames its
/// GOPs must start at; both part of the output).
fn cache_key(
    project: &Project,
    input: &Path,
    budget: &Budget,
    (wide, gop_starts): (bool, &[u64]),
) -> Result<String, DvdError> {
    // At the asset's own bit rate (M6.251), also when asked for the file alone (its transcode status).
    let budget = &project
        .assets
        .iter()
        .find(|(id, asset)| {
            asset.media_file().as_ref() == input && fixed_video_bitrate(project, **id).is_some()
        })
        .and_then(|(id, _)| fixed_video_bitrate(project, *id))
        .map_or(*budget, |fixed| Budget {
            video_bitrate: fixed.min(budget.max_bitrate),
            ..*budget
        });
    let metadata =
        disccraft_transcode::document_metadata(input).map_err(|err| crate::io_error(input, err))?;
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
    // Two passes or a constant rate make another stream (one-pass VBR leaves the older keys as they were).
    match rate_mode(project, input) {
        disccraft_transcode::RateMode::Vbr => {}
        disccraft_transcode::RateMode::Vbr2Pass => seed = fnv1a64(b"vbr2pass", seed),
        disccraft_transcode::RateMode::Cbr => seed = fnv1a64(b"cbr", seed),
    }
    // Film (23.976 by 3:2 pulldown) makes another stream (the keys of the others stay as they were).
    if film_of(project, input) {
        seed = fnv1a64(b"film", seed);
    }
    // A lower quality (Encore's "Low quality" presets) makes another stream (the best leaves the keys as they were).
    let quality = quality_of(project, input);
    if quality != disccraft_transcode::BEST_QUALITY {
        seed = fnv1a64(format!("quality:{quality}").as_bytes(), seed);
    }
    // So do Encore's rendering checkboxes (both off leaves the keys as they were).
    let rendering = rendering_of(project, input);
    if rendering.max_quality {
        seed = fnv1a64(b"max-render-quality", seed);
    }
    if rendering.frame_blend {
        seed = fnv1a64(b"frame-blend", seed);
    }
    // Interpret Footage: pixels of another shape make another picture (the keys of the others stay as they were).
    if let Some((width, height)) = disccraft_transcode::chosen_pixel_aspect(input) {
        seed = fnv1a64(format!("par:{width}:{height}").as_bytes(), seed);
    }
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
    nearest_frame(ticks, frame_ticks(standard))
}

/// Ticks of one film picture (1001/24000 s), as a film transcode makes them.
const FILM_FRAME_TICKS: u64 = 10_594_584_000;

/// The frame `per_frame` ticks long nearest to `ticks`.
fn nearest_frame(ticks: i64, per_frame: u64) -> u64 {
    u64::try_from(ticks).map_or(0, |ticks| {
        ticks.saturating_add(per_frame / 2) / per_frame.max(1)
    })
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
    // A film transcode's pictures are 23.976 a second.
    let per_frame = if film_of(project, path) {
        FILM_FRAME_TICKS
    } else {
        frame_ticks(project.settings.standard)
    };
    let mut frames: Vec<u64> = project
        .timelines
        .values()
        .filter(|timeline| {
            project
                .assets
                .get(&timeline.video)
                .is_some_and(|asset| asset.media_file().as_ref() == path)
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
        .map(|time| nearest_frame(time.0, per_frame))
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
                .is_some_and(|asset| asset.media_file().as_ref() == path)
        })
        .map_or(project.settings.widescreen, |timeline| {
            timeline.wide(&project.settings)
        })
}

/// Compute a 16 hex digit cache key for an audio transcode.
pub(crate) fn audio_cache_key(input: &Path) -> Result<String, DvdError> {
    let metadata =
        disccraft_transcode::document_metadata(input).map_err(|err| crate::io_error(input, err))?;
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
    // The slide renderer's version (2: M10.6 gave tall slides their right width; 3: M6.320 a DVD-sized still has SD
    // pixels).
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

        let path: &Path = &path;
        let metadata = disccraft_transcode::document_metadata(path)
            .map_err(|err| crate::io_error(path, err))?;
        let (secs, nanos) = modified_time(&metadata)?;

        seed = fnv1a64(absolute_path_string(path).as_bytes(), seed);
        seed = fnv1a64(metadata.len().to_string().as_bytes(), seed);
        seed = fnv1a64(secs.to_string().as_bytes(), seed);
        seed = fnv1a64(nanos.to_string().as_bytes(), seed);
        seed = fnv1a64(slide.duration.0.to_string().as_bytes(), seed);
        seed = fnv1a64(slide.scale.as_str().as_bytes(), seed);
        // A picture whose alpha is ignored is another picture (none ignored leaves older keys as they were).
        if project
            .assets
            .get(&slide.asset)
            .is_some_and(|asset| asset.ignore_alpha)
        {
            seed = fnv1a64(b"opaque", seed);
        }
        // Interpret Footage: pixels of another shape make another picture (as a video's transcode key has it; the
        // keys of the others stay as they were).
        if let Some((width, height)) = disccraft_transcode::chosen_pixel_aspect(path) {
            seed = fnv1a64(format!("par:{width}:{height}").as_bytes(), seed);
        }
        match slide.transition {
            Some(own) => {
                seed = fnv1a64(own.kind.as_str().as_bytes(), seed);
                seed = fnv1a64(own.duration.0.to_string().as_bytes(), seed);
            }
            None => seed = fnv1a64(b"show", seed),
        }
    }
    // The pan and zoom each slide plays with (none at all leaves the keys of older builds as they were).
    for index in 0..slideshow.slides.len() {
        if let Some(effect) = slideshow.slide_effect(index) {
            seed = fnv1a64(format!("effect {index}").as_bytes(), seed);
            seed = fnv1a64(effect.pan.as_str().as_bytes(), seed);
            seed = fnv1a64(effect.zoom.as_str().as_bytes(), seed);
        }
    }

    let clips = slideshow.audio_list();
    if clips.is_empty() {
        seed = fnv1a64(b"none", seed);
    }
    for asset in clips {
        let path = asset_path(project, asset)?;
        let path: &Path = &path;
        let metadata = disccraft_transcode::document_metadata(path)
            .map_err(|err| crate::io_error(path, err))?;
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
        mode: disccraft_transcode::RateMode::Vbr,
        film: false,
        quality: disccraft_transcode::BEST_QUALITY,
        rendering: disccraft_transcode::Rendering::default(),
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
    // 1080i coded as H.264 (M12.28) is another stream than the format's MPEG-2 (the keys of the others as they were).
    if options.interlaced {
        seed = fnv1a64(b"h264-interlaced", seed);
    }
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
    for (index, (slide, duration)) in slideshow.slides.iter().zip(durations).enumerate() {
        let path = asset_path(project, slide.asset)?;
        let path: &Path = &path;
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
            // The slide's own pan and zoom, or a random one when the show asks for them.
            motion: slideshow.slide_effect(index).map(slide_motion),
            opaque: project
                .assets
                .get(&slide.asset)
                .is_some_and(|asset| asset.ignore_alpha),
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
        .map(|asset| asset_path(project, asset).map(std::borrow::Cow::into_owned))
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
    /// A clip moved along its track: its WAV gets that much silence in front, sample for sample.
    #[test]
    fn a_moved_audio_clip_starts_with_silence() {
        assert_eq!(super::delay_samples(0, VideoStandard::Ntsc), 0);
        // 30 NTSC frames are 1.001 s; 25 PAL frames one second.
        assert_eq!(super::delay_samples(30, VideoStandard::Ntsc), 48_048);
        assert_eq!(super::delay_samples(1, VideoStandard::Ntsc), 1602);
        assert_eq!(super::delay_samples(25, VideoStandard::Pal), 48_000);

        let dir = std::env::temp_dir().join(format!("disccraft-delay-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A stereo WAV with a "LIST" chunk before its data, and a data length that says more than is there.
        let sound: Vec<u8> = (1..=40u8).collect();
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF\0\0\0\0WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&[1, 0, 2, 0, 0x80, 0xBB, 0, 0, 0, 0xEE, 2, 0, 4, 0, 16, 0]);
        wav.extend_from_slice(b"LIST");
        wav.extend_from_slice(&3u32.to_le_bytes());
        wav.extend_from_slice(b"abc\0");
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&1000u32.to_le_bytes());
        wav.extend_from_slice(&sound);
        // A torn last frame is left out.
        wav.extend_from_slice(&[9, 9]);
        let input = dir.join("in.wav");
        std::fs::write(&input, &wav).unwrap();
        let out = super::delayed_wav(&input, &dir, "key", 3).unwrap();
        assert_eq!(out.file_name().unwrap(), "key.delay3.wav");
        let made = std::fs::read(&out).unwrap();
        assert_eq!(&made[0..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes(made[4..8].try_into().unwrap()),
            36 + 12 + 40
        );
        assert_eq!(&made[20..36], &wav[20..36]);
        assert_eq!(&made[36..40], b"data");
        assert_eq!(
            u32::from_le_bytes(made[40..44].try_into().unwrap()),
            12 + 40
        );
        assert_eq!(&made[44..56], &[0u8; 12]);
        assert_eq!(&made[56..], &sound[..]);
        // Asked again, it is there already.
        std::fs::remove_file(&input).unwrap();
        assert_eq!(super::delayed_wav(&input, &dir, "key", 3).unwrap(), out);
        // Not a WAV: refused, nothing left behind.
        let junk = dir.join("junk.wav");
        std::fs::write(&junk, b"not a wave file at all").unwrap();
        assert!(super::delayed_wav(&junk, &dir, "junk", 3).is_err());
        assert!(!dir.join("junk.delay3.wav").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::{
        AC3_STEREO_BITRATE, AudioKind, Budget, LPCM_STEREO_BITRATE, MAX_MUX_VIDEO_BITRATE,
        TICKS_PER_SECOND, TranscodeStatus, VideoStandard, audio_cache_key, audio_kind,
        budget_with_probe, estimate_with_probe, fnv1a64, is_passthrough,
        max_bitrate_for_audio_bitrate, passes_through, slideshow_cache_key, transcode_status,
        video_fits_as_is,
    };
    use crate::DvdError;
    use disccraft_project::{Capacity, DiscKind, Project, Tick};
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
        // The smaller DVD picture sizes: sequence headers of 704x480, 352x480 and 352x240 at 29.97, 4:3, 4 Mbps;
        // 352 wide is 4:3 only, and other sizes are not DVD sizes.
        let header = |width: u16, height: u16, aspect: u8| {
            let [w_hi, w_lo] = (width << 4).to_be_bytes();
            let [h_hi, h_lo] = height.to_be_bytes();
            vec![
                0,
                0,
                1,
                0xB3,
                w_hi,
                w_lo | h_hi,
                h_lo,
                (aspect << 4) | 4,
                0x09,
                0xC4,
                0x20,
            ]
        };
        let size = |width, height, aspect| {
            super::first_sequence_header_size(&header(width, height, aspect), VideoStandard::Ntsc)
        };
        assert_eq!(size(720, 480, 2), Some((720, 480)));
        assert_eq!(size(704, 480, 3), Some((704, 480)));
        assert_eq!(size(352, 480, 2), Some((352, 480)));
        assert_eq!(size(352, 240, 2), Some((352, 240)));
        assert_eq!(size(352, 480, 3), None);
        assert_eq!(size(640, 480, 2), None);
        assert_eq!(size(352, 288, 2), None);
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

        // M6.315: a DVD quality preset of its own transcodes a video that fits all the same (Encore's preset picked
        // for a DVD-compliant asset); it still fits, so "Don't Transcode" (no setting) puts it back.
        assert!(video_fits_as_is(&project, compliant));
        assert!(!video_fits_as_is(&project, other_video));
        assert!(!video_fits_as_is(&project, still));
        // M6.324: asked for each kind of disc, whatever the project's.
        assert!(super::video_fits_disc(
            &project,
            compliant,
            disccraft_project::DiscKind::Dvd
        ));
        assert!(!super::video_fits_disc(
            &project,
            other_video,
            disccraft_project::DiscKind::BluRay
        ));
        assert!(!super::video_fits_disc(
            &project,
            still,
            disccraft_project::DiscKind::Dvd
        ));
        // M6.325: and for a Blu-ray format being chosen: the SD MPEG-2 fits an SD title, not an HD one.
        assert!(super::video_fits_bluray_as(
            &project,
            compliant,
            super::BdVideo::Sd
        ));
        assert!(!super::video_fits_bluray_as(
            &project,
            compliant,
            super::BdVideo::Hd1080p23
        ));
        assert!(!super::video_fits_bluray_as(
            &project,
            still,
            super::BdVideo::Sd
        ));
        project.set_asset_transcode(compliant, Some(4_000))?;
        let status = transcode_status(&project, None);
        assert_eq!(status.get(&compliant), Some(&TranscodeStatus::Untranscoded));
        assert!(video_fits_as_is(&project, compliant));
        assert!(!passes_through(&project, &m2v));
        // On a Blu-ray the DVD setting does not count.
        project.settings.kind = disccraft_project::DiscKind::BluRay;
        assert!(passes_through(&project, &m2v));
        project.settings.kind = disccraft_project::DiscKind::Dvd;
        project.set_asset_transcode(compliant, None)?;
        assert!(passes_through(&project, &m2v));

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

    /// M6.251: a video with a bit rate of its own is transcoded at it, and the others share what it leaves.
    #[test]
    fn an_assets_own_bit_rate_is_taken_off_the_budget() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        project.settings.capacity = Capacity::Dvd5;
        let first = project.add_asset(Path::new("first.mp4"))?;
        let second = project.add_asset(Path::new("second.mp4"))?;
        let one = project.add_timeline(first, None)?;
        let two = project.add_timeline(second, None)?;
        // Two titles of 80 minutes: 3.5 Mbit/s each fits.
        let probe = |_path: &Path| Ok::<f64, DvdError>(4800.0);
        let shared = budget_with_probe(&project, &probe)?.video_bitrate;
        assert!((3_400_000..3_700_000).contains(&shared), "{shared}");
        let even = estimate_with_probe(&project, &probe)?;

        // The first at 2 Mbit/s: the second gets what that leaves.
        project.set_asset_transcode(first, Some(2_000))?;
        assert_eq!(super::fixed_video_bitrate(&project, first), Some(2_000_000));
        assert_eq!(super::fixed_video_bitrate(&project, second), None);
        let budget = budget_with_probe(&project, &probe)?;
        assert!(
            (4_900_000..5_300_000).contains(&budget.video_bitrate),
            "{budget:?}"
        );
        let estimate = estimate_with_probe(&project, &probe)?;
        let bytes = |estimate: &super::Estimate, id| {
            estimate
                .timelines
                .iter()
                .find(|(timeline, _)| *timeline == id)
                .map_or(0, |(_, bytes)| *bytes)
        };
        // 80 minutes at 2 Mbit/s and 192 kbit/s of sound.
        let expected = 4800 * (2_000_000 + u64::from(AC3_STEREO_BITRATE)) / 8;
        assert!(bytes(&estimate, one).abs_diff(expected) < 1_000);
        assert!(bytes(&estimate, one) < bytes(&even, one));
        assert!(bytes(&estimate, two) > bytes(&even, two));
        assert!(estimate.estimated_bytes.abs_diff(even.estimated_bytes) < 10_000_000);

        // Both with a rate of their own: nothing is left to share out, and the disc may overflow.
        project.set_asset_transcode(second, Some(8_000))?;
        let budget = budget_with_probe(&project, &probe)?;
        assert_eq!(budget.video_bitrate, 0);
        let estimate = estimate_with_probe(&project, &probe)?;
        assert!(estimate.estimated_bytes > Capacity::Dvd5.bytes());
        // A rate outside what a disc carries is refused.
        assert!(project.set_asset_transcode(first, Some(500)).is_err());
        assert!(project.set_asset_transcode(first, Some(50_000)).is_err());
        project.set_asset_transcode(first, None)?;
        assert_eq!(super::fixed_video_bitrate(&project, first), None);
        Ok(())
    }

    /// M6.350: an MPEG-1 sequence header (no sequence extension after it) fits a DVD at 352x240 / 352x288, the
    /// standard's rate and up to 1.856 Mbit/s; MPEG-2's is checked as before.
    #[test]
    fn mpeg1_video_fits_a_dvd_at_its_own_size() {
        // 352x240, pixel aspect 12, 29.97, 1150 kbit/s (2875 x 400), then a GOP header.
        let header = |width: u16, height: u16, rate: u8, bits_400: u32, next: u8| -> Vec<u8> {
            let mut data = vec![0, 0, 1, 0xB3];
            data.push((width >> 4) as u8);
            data.push((((width & 0x0F) << 4) as u8) | ((height >> 8) as u8));
            data.push((height & 0xFF) as u8);
            data.push(0xC0 | rate);
            data.push((bits_400 >> 10) as u8);
            data.push((bits_400 >> 2) as u8);
            data.push((((bits_400 & 0x03) << 6) as u8) | 0x20);
            data.extend_from_slice(&[0x10, 0x00]);
            data.extend_from_slice(&[0, 0, 1, next, 0, 0, 0, 0]);
            data
        };
        let ntsc = VideoStandard::Ntsc;
        let vcd = header(352, 240, 4, 2875, 0xB8);
        assert!(super::is_mpeg1_at(&vcd, 0));
        assert_eq!(
            super::first_sequence_header_size(&vcd, ntsc),
            Some((352, 240))
        );
        // On a PAL disc it is 352x288 at 25.
        assert_eq!(
            super::first_sequence_header_size(&vcd, VideoStandard::Pal),
            None
        );
        let pal = header(352, 288, 3, 2875, 0x00);
        assert_eq!(
            super::first_sequence_header_size(&pal, VideoStandard::Pal),
            Some((352, 288))
        );
        // Too fast (2 Mbit/s), marked variable (0x3FFFF), or full size: not as it is.
        assert_eq!(
            super::first_sequence_header_size(&header(352, 240, 4, 5000, 0xB8), ntsc),
            None
        );
        assert_eq!(
            super::first_sequence_header_size(&header(352, 240, 4, 0x3FFFF, 0xB8), ntsc),
            None
        );
        assert_eq!(
            super::first_sequence_header_size(&header(720, 480, 4, 2875, 0xB8), ntsc),
            None
        );
        // With its sequence extension it is MPEG-2 (whose 352x240 needs a 4:3 aspect code, which 12 is not).
        let mpeg2 = header(352, 240, 4, 2875, 0xB5);
        assert!(!super::is_mpeg1_at(&mpeg2, 0));
        assert_eq!(super::first_sequence_header_size(&mpeg2, ntsc), None);
        // M6.362: marked variable, its size is known from the head and its rate is measured.
        assert_eq!(
            super::variable_mpeg1_size(&header(352, 240, 4, 0x3FFFF, 0xB8), ntsc),
            Some((352, 240))
        );
        assert_eq!(super::variable_mpeg1_size(&vcd, ntsc), None);
        assert_eq!(
            super::variable_mpeg1_size(&header(352, 240, 4, 0x3FFFF, 0xB5), ntsc),
            None
        );
    }

    /// M6.362: the pictures of a stream are counted from one picture start code to the next, written in any
    /// pieces; the peak is the most of a second's run of them.
    #[test]
    fn a_streams_pictures_are_measured() -> Result<(), Box<dyn std::error::Error>> {
        use std::io::Write;
        let mut stream = vec![0, 0, 1, 0xB3, 9, 9, 9, 9];
        let mut sizes = Vec::new();
        for picture in 0..40u32 {
            // Pictures of 1000 bytes, and 4000 for ten of them.
            let size = if (10..20).contains(&picture) {
                4000
            } else {
                1000
            };
            stream.extend_from_slice(&[0, 0, 1, 0]);
            stream.resize(stream.len() + size - 4, 0x55);
            sizes.push(size as u64);
        }
        let mut meter = super::PictureMeter::new();
        for piece in stream.chunks(7) {
            meter.write_all(piece)?;
        }
        let measured = meter.sizes();
        // The header's bytes go with no picture; the first picture starts at its start code.
        assert_eq!(measured, sizes);
        // 30 pictures: 20 of 1000 and 10 of 4000 bytes at 29.97 a second.
        let peak = crate::source::peak_bit_rate(measured.iter().copied(), VideoStandard::Ntsc);
        assert_eq!(peak, 60_000 * 8 * 1000 / 1001);
        assert_eq!(
            crate::source::peak_bit_rate([1000; 25], VideoStandard::Pal),
            200_000
        );
        assert_eq!(crate::source::peak_bit_rate([], VideoStandard::Pal), 0);
        Ok(())
    }

    /// A WAV is taken as LPCM only as 16-bit 48 kHz integer PCM; the header is read, also an extensible one.
    #[test]
    fn a_wav_header_tells_whether_it_is_disc_lpcm() {
        fn wav(tag: u16, channels: u16, rate: u32, bits: u16, extensible: bool) -> Vec<u8> {
            let mut fmt = Vec::new();
            fmt.extend_from_slice(&(if extensible { 0xFFFE } else { tag }).to_le_bytes());
            fmt.extend_from_slice(&channels.to_le_bytes());
            fmt.extend_from_slice(&rate.to_le_bytes());
            fmt.extend_from_slice(&[0; 6]);
            fmt.extend_from_slice(&bits.to_le_bytes());
            if extensible {
                // 5.1 with side surrounds.
                fmt.extend_from_slice(&[22, 0, 0, 0, 0x0F, 0x06, 0, 0]);
                fmt.extend_from_slice(&tag.to_le_bytes());
                fmt.extend_from_slice(&[0; 14]);
            }
            let mut out = b"RIFF\0\0\0\0WAVE".to_vec();
            // An odd-sized chunk in front of the format, to check the padding.
            out.extend_from_slice(b"LIST\x03\0\0\0abc\0");
            out.extend_from_slice(b"fmt ");
            out.extend_from_slice(&u32::try_from(fmt.len()).unwrap_or(0).to_le_bytes());
            out.extend_from_slice(&fmt);
            out
        }
        assert_eq!(
            super::wav_format(&wav(1, 2, 48_000, 16, false)),
            Some((1, 2, 48_000, 16, 0))
        );
        assert_eq!(
            super::wav_format(&wav(1, 6, 48_000, 24, true)),
            Some((1, 6, 48_000, 24, 0x60F))
        );
        assert_eq!(
            super::wav_format(&wav(3, 2, 44_100, 32, false)),
            Some((3, 2, 44_100, 32, 0))
        );
        assert_eq!(super::wav_format(b"RIFF....AVI "), None);
        assert_eq!(super::wav_format(&[]), None);
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
        assert_eq!(
            max_bitrate_for_audio_bitrate(audio_bitrate, DiscKind::Dvd)?,
            6_728_000
        );
        Ok(())
    }

    #[test]
    fn max_bitrate_five_lpcm_tracks_errors() {
        let audio_bitrate = 5 * LPCM_STEREO_BITRATE;
        assert!(max_bitrate_for_audio_bitrate(audio_bitrate, DiscKind::Dvd).is_err());
    }

    /// M6.347: a Blu-ray keeps as LPCM a WAV of 192 kHz, of 20 bits (as 24) and of more than two channels on the
    /// speakers of a channel assignment, up to 27.648 Mbit/s as stored; a DVD none of these past its 6.144.
    #[test]
    fn a_blu_ray_keeps_more_wavs_as_lpcm() -> Result<(), Box<dyn Error>> {
        let wav =
            |channels: u16, rate: u32, bits: u16, mask: Option<u32>| -> Result<_, Box<dyn Error>> {
                let mut fmt = Vec::new();
                fmt.extend_from_slice(&(if mask.is_some() { 0xFFFE_u16 } else { 1 }).to_le_bytes());
                fmt.extend_from_slice(&channels.to_le_bytes());
                fmt.extend_from_slice(&rate.to_le_bytes());
                fmt.extend_from_slice(&[0; 6]);
                fmt.extend_from_slice(&bits.to_le_bytes());
                if let Some(mask) = mask {
                    fmt.extend_from_slice(&22_u16.to_le_bytes());
                    fmt.extend_from_slice(&bits.to_le_bytes());
                    fmt.extend_from_slice(&mask.to_le_bytes());
                    fmt.extend_from_slice(&1_u16.to_le_bytes());
                    fmt.extend_from_slice(&[0; 14]);
                }
                let mut out = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
                out.extend_from_slice(&u32::try_from(fmt.len())?.to_le_bytes());
                out.extend_from_slice(&fmt);
                let path = temp_path("wav");
                fs::write(&path, out)?;
                Ok(path)
            };
        let bd = |path: &Path| super::wav_is_fine_lpcm(path, DiscKind::BluRay);
        let dvd = |path: &Path| super::wav_is_fine_lpcm(path, DiscKind::Dvd);
        // 5.1 at 192 kHz, 24 bits: the Blu-ray's most.
        let most = wav(6, 192_000, 24, Some(0x60F))?;
        assert_eq!(bd(&most), Some(27_648_000));
        assert_eq!(dvd(&most), None);
        // Five channels are stored as six; seven of 24 bits at 192 kHz as eight, too many.
        let five = wav(5, 96_000, 24, None)?;
        assert_eq!(bd(&five), Some(13_824_000));
        let seven = wav(7, 192_000, 24, Some(0x637))?;
        assert_eq!(bd(&seven), None);
        // 20 bits count as 24.
        let twenty = wav(2, 48_000, 20, None)?;
        assert_eq!(bd(&twenty), Some(2_304_000));
        assert_eq!(dvd(&twenty), Some(2_304_000));
        // 2.1 has no channel assignment; seven channels that name no speakers neither.
        let two_one = wav(3, 96_000, 24, Some(0b1011))?;
        assert_eq!(bd(&two_one), None);
        let unnamed = wav(7, 96_000, 16, None)?;
        assert_eq!(bd(&unnamed), None);
        // The 16-bit 48 kHz ones are the disc's plain LPCM (not "fine"): 5.1 stays LPCM on a Blu-ray, 2.1 is
        // converted.
        let plain = wav(6, 48_000, 16, None)?;
        assert_eq!(bd(&plain), None);
        assert!(super::bd_layout(6, 0));
        assert!(!super::bd_layout(3, 0b1011));
        for path in [most, five, seven, twenty, two_one, unnamed, plain] {
            fs::remove_file(&path).unwrap_or(());
        }
        Ok(())
    }

    /// M6.347: a Blu-ray's stream leaves the SD video its 9.8 Mbit/s beside much more sound (six channels of 24-bit
    /// 192 kHz LPCM), and refuses only sound that leaves it too little.
    #[test]
    fn a_blu_ray_leaves_the_video_its_rate_beside_more_sound() -> Result<(), Box<dyn Error>> {
        let audio_bitrate = 27_648_000;
        assert!(max_bitrate_for_audio_bitrate(audio_bitrate, DiscKind::Dvd).is_err());
        assert_eq!(
            max_bitrate_for_audio_bitrate(audio_bitrate, DiscKind::BluRay)?,
            MAX_MUX_VIDEO_BITRATE
        );
        assert_eq!(
            max_bitrate_for_audio_bitrate(40_000_000, DiscKind::BluRay)?,
            6_000_000
        );
        assert!(max_bitrate_for_audio_bitrate(45_000_000, DiscKind::BluRay).is_err());
        Ok(())
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

    /// M6.300: a video transcoded in two passes or at a constant rate is another stream: its cache key changes, and
    /// one-pass VBR keeps the key it had.
    #[test]
    fn the_cache_key_follows_the_encoding() -> Result<(), Box<dyn Error>> {
        use disccraft_project::TranscodeMode;
        let path = temp_path("mpg");
        fs::write(&path, b"video")?;
        let mut project = Project::default();
        let asset = project.add_asset(&path)?;
        let budget = Budget {
            video_bitrate: 6_000_000,
            max_bitrate: 9_800_000,
            wide: false,
            audio_bitrate: LPCM_STEREO_BITRATE,
        };
        let key = |project: &Project| super::cache_key(project, &path, &budget, (false, &[]));
        let vbr = key(&project)?;
        let mut keys = vec![vbr.clone()];
        for mode in [TranscodeMode::Vbr2Pass, TranscodeMode::Cbr] {
            if let Some(found) = project.assets.get_mut(&asset) {
                found.transcode_mode = mode;
            }
            assert_eq!(
                super::rate_mode(&project, &path),
                match mode {
                    TranscodeMode::Vbr2Pass => disccraft_transcode::RateMode::Vbr2Pass,
                    _ => disccraft_transcode::RateMode::Cbr,
                }
            );
            keys.push(key(&project)?);
        }
        if let Some(found) = project.assets.get_mut(&asset) {
            found.transcode_mode = TranscodeMode::Vbr;
        }
        assert_eq!(key(&project)?, vbr);
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), 3);
        fs::remove_file(&path).unwrap_or(());
        Ok(())
    }

    /// M6.326: the quality is the DVD preset's (with a rate of its own) on a DVD, the Blu-ray preset's on a Blu-ray,
    /// else the best; a lower one is another stream (another cache key), the best keeps the key it had.
    #[test]
    fn the_cache_key_follows_the_quality() -> Result<(), Box<dyn Error>> {
        use disccraft_project::BEST_QUALITY;
        let path = temp_path("mpg");
        fs::write(&path, b"video")?;
        let mut project = Project::default();
        let asset = project.add_asset(&path)?;
        let budget = Budget {
            video_bitrate: 6_000_000,
            max_bitrate: 9_800_000,
            wide: false,
            audio_bitrate: LPCM_STEREO_BITRATE,
        };
        let key = |project: &Project| super::cache_key(project, &path, &budget, (false, &[]));
        let best = key(&project)?;
        let set = |project: &mut Project, kbps: Option<u32>, quality: u8| {
            if let Some(found) = project.assets.get_mut(&asset) {
                found.transcode_kbps = kbps;
                found.transcode_quality = quality;
            }
        };
        // Automatic has no quality of its own.
        set(&mut project, None, 2);
        assert_eq!(super::quality_of(&project, &path), BEST_QUALITY);
        set(&mut project, Some(4_000), 2);
        assert_eq!(super::quality_of(&project, &path), 2);
        let low = key(&project)?;
        set(&mut project, Some(4_000), BEST_QUALITY);
        let own = key(&project)?;
        assert_ne!(low, own);
        set(&mut project, None, BEST_QUALITY);
        assert_eq!(key(&project)?, best);
        // On a Blu-ray, the Blu-ray preset's (an SD Blu-ray's MPEG-2).
        project.settings.kind = disccraft_project::DiscKind::BluRay;
        if let Some(found) = project.assets.get_mut(&asset) {
            found.transcode_kbps = Some(4_000);
            found.transcode_quality = 2;
            found.bluray_transcode = Some(disccraft_project::BlurayTranscode {
                kbps: 9_000,
                max_kbps: 11_000,
                preset: String::new(),
                quality: 3,
                mode: disccraft_project::TranscodeMode::Vbr,
            });
        }
        assert_eq!(super::quality_of(&project, &path), 3);
        fs::remove_file(&path).unwrap_or(());
        Ok(())
    }

    /// M6.328: on a Blu-ray the rate is kept as its Blu-ray setting says (one-pass VBR without one), not as the
    /// DVD setting does; another way is another cache key.
    #[test]
    fn a_blu_ray_keeps_its_rate_as_its_setting_says() -> Result<(), Box<dyn Error>> {
        use disccraft_project::TranscodeMode;
        use disccraft_transcode::RateMode;
        let path = temp_path("mpg");
        fs::write(&path, b"video")?;
        let mut project = Project::default();
        project.settings.kind = disccraft_project::DiscKind::BluRay;
        let asset = project.add_asset(&path)?;
        let budget = Budget {
            video_bitrate: 6_000_000,
            max_bitrate: 9_800_000,
            wide: false,
            audio_bitrate: LPCM_STEREO_BITRATE,
        };
        if let Some(found) = project.assets.get_mut(&asset) {
            found.transcode_mode = TranscodeMode::Cbr;
        }
        assert_eq!(super::rate_mode(&project, &path), RateMode::Vbr);
        let one = super::cache_key(&project, &path, &budget, (false, &[]))?;
        if let Some(found) = project.assets.get_mut(&asset) {
            found.bluray_transcode = Some(disccraft_project::BlurayTranscode {
                kbps: 6_000,
                max_kbps: 9_000,
                preset: String::new(),
                quality: disccraft_project::BEST_QUALITY,
                mode: TranscodeMode::Vbr2Pass,
            });
        }
        assert_eq!(super::rate_mode(&project, &path), RateMode::Vbr2Pass);
        assert_ne!(
            super::cache_key(&project, &path, &budget, (false, &[]))?,
            one
        );
        fs::remove_file(&path).unwrap_or(());
        Ok(())
    }

    /// M6.327: Encore's rendering checkboxes make another stream each (another cache key); both off keeps the key
    /// it had.
    #[test]
    fn the_cache_key_follows_the_rendering() -> Result<(), Box<dyn Error>> {
        let path = temp_path("mpg");
        fs::write(&path, b"video")?;
        let mut project = Project::default();
        let asset = project.add_asset(&path)?;
        let budget = Budget {
            video_bitrate: 6_000_000,
            max_bitrate: 9_800_000,
            wide: false,
            audio_bitrate: LPCM_STEREO_BITRATE,
        };
        let mut keys = Vec::new();
        for flags in [(false, false), (true, false), (false, true), (true, true)] {
            if let Some(found) = project.assets.get_mut(&asset) {
                (found.max_render_quality, found.frame_blend) = flags;
            }
            let rendering = super::rendering_of(&project, &path);
            assert_eq!((rendering.max_quality, rendering.frame_blend), flags);
            keys.push(super::cache_key(&project, &path, &budget, (false, &[]))?);
        }
        let plain = keys.first().cloned();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), 4);
        if let Some(found) = project.assets.get_mut(&asset) {
            (found.max_render_quality, found.frame_blend) = (false, false);
        }
        assert_eq!(
            Some(super::cache_key(&project, &path, &budget, (false, &[]))?),
            plain
        );
        fs::remove_file(&path).unwrap_or(());
        Ok(())
    }

    /// M6.306: a DVD title whose video asks for PCM keeps its converted sound as LPCM (counted at 1.536 Mbit/s),
    /// Dolby Digital otherwise; a Blu-ray's stays Dolby Digital.
    #[test]
    fn a_pcm_soundtrack_is_counted_as_lpcm() -> Result<(), Box<dyn Error>> {
        use disccraft_project::TranscodeAudio;
        let path = temp_path("mp4");
        fs::write(&path, b"video")?;
        let mut project = Project::default();
        let video = project.add_asset(&path)?;
        let timeline_id = project.add_timeline(video, None)?;
        let rate = |project: &Project| -> Result<u32, crate::DvdError> {
            let timeline = project
                .timeline(timeline_id)
                .ok_or_else(|| crate::DvdError::Invalid("no timeline".to_string()))?;
            super::audio_bitrate_for_title(project, timeline, 60.0)
        };
        assert_eq!(rate(&project)?, AC3_STEREO_BITRATE);
        if let Some(asset) = project.assets.get_mut(&video) {
            asset.transcode_audio = TranscodeAudio::Pcm;
        }
        assert_eq!(rate(&project)?, LPCM_STEREO_BITRATE);
        assert!(super::wants_pcm(&project, video));
        project.settings.kind = disccraft_project::DiscKind::BluRay;
        assert!(!super::wants_pcm(&project, video));
        fs::remove_file(&path).unwrap_or(());
        Ok(())
    }

    /// M6.346: a track of WAV clips stays LPCM and counts so; one that mixes a WAV with a Dolby Digital clip is
    /// joined as Dolby Digital and counts as that.
    #[test]
    fn a_track_mixing_wav_and_dolby_digital_counts_as_dolby_digital() -> Result<(), Box<dyn Error>>
    {
        use disccraft_project::AudioClip;
        let video_path = temp_path("mp4");
        fs::write(&video_path, b"video")?;
        let wav_path = temp_path("wav");
        // A second of 48 kHz 16-bit stereo silence: a WAV that goes on the disc as LPCM.
        let samples = 48_000u32;
        let mut wav_bytes = Vec::new();
        wav_bytes.extend_from_slice(b"RIFF");
        wav_bytes.extend_from_slice(&(36 + samples * 4).to_le_bytes());
        wav_bytes.extend_from_slice(b"WAVEfmt ");
        wav_bytes.extend_from_slice(&16u32.to_le_bytes());
        wav_bytes.extend_from_slice(&1u16.to_le_bytes());
        wav_bytes.extend_from_slice(&2u16.to_le_bytes());
        wav_bytes.extend_from_slice(&48_000u32.to_le_bytes());
        wav_bytes.extend_from_slice(&(48_000u32 * 4).to_le_bytes());
        wav_bytes.extend_from_slice(&4u16.to_le_bytes());
        wav_bytes.extend_from_slice(&16u16.to_le_bytes());
        wav_bytes.extend_from_slice(b"data");
        wav_bytes.extend_from_slice(&(samples * 4).to_le_bytes());
        wav_bytes.resize(wav_bytes.len() + (samples * 4) as usize, 0);
        fs::write(&wav_path, &wav_bytes)?;
        let ac3_path = temp_path("ac3");
        fs::write(&ac3_path, b"ac3")?;
        let mut project = Project::default();
        let video = project.add_asset(&video_path)?;
        let wav = project.add_asset(&wav_path)?;
        let ac3 = project.add_asset(&ac3_path)?;
        let timeline_id = project.add_timeline(video, None)?;
        let rate = |project: &Project| -> Result<u32, crate::DvdError> {
            let timeline = project
                .timeline(timeline_id)
                .ok_or_else(|| crate::DvdError::Invalid("no timeline".to_string()))?;
            super::audio_bitrate_for_title(project, timeline, 60.0)
        };
        let set_track = |project: &mut Project, more: Vec<AudioClip>| {
            if let Some(timeline) = project.timeline_mut(timeline_id) {
                let mut track = disccraft_project::AudioTrack::of(wav);
                track.more = more;
                timeline.audio = vec![track];
            }
        };
        let clip = |asset| AudioClip {
            asset,
            start: 3000,
            trim_start: 0,
            trim_end: None,
        };
        // Two WAV clips: LPCM.
        set_track(&mut project, vec![clip(wav)]);
        assert_eq!(rate(&project)?, LPCM_STEREO_BITRATE);
        // A WAV and a Dolby Digital clip: Dolby Digital.
        set_track(&mut project, vec![clip(ac3)]);
        assert_eq!(rate(&project)?, AC3_STEREO_BITRATE);
        for path in [video_path, wav_path, ac3_path] {
            fs::remove_file(&path).unwrap_or(());
        }
        Ok(())
    }

    /// M6.309: Dolby Digital clips of one form on a track are joined by their frames: the first clip's frames as
    /// they are, silent frames of the same form up to where the second starts, then its frames as they are.
    #[test]
    fn dolby_digital_clips_are_joined_without_decoding() -> Result<(), Box<dyn Error>> {
        use disccraft_project::{AudioClip, AudioTrack};
        let dir = temp_path("ac3join");
        fs::create_dir_all(&dir)?;
        // A second of a 440 Hz tone, encoded as AC-3.
        let wav = dir.join("tone.wav");
        let samples = 48_000u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + samples * 4).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&48_000u32.to_le_bytes());
        bytes.extend_from_slice(&(48_000u32 * 4).to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(samples * 4).to_le_bytes());
        for n in 0..samples {
            let value =
                ((f64::from(n) * 440.0 * std::f64::consts::TAU / 48_000.0).sin() * 8_000.0) as i16;
            for _ in 0..2 {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        fs::write(&wav, &bytes)?;
        let ac3 = dir.join("tone.ac3");
        disccraft_ac3enc::encode_wav(&wav, &ac3).map_err(|err| err.to_string())?;
        let source = fs::read(&ac3)?;
        let source_frames = disccraft_psmux::es::parse_ac3(&source, 0)
            .map_err(|err| err.to_string())?
            .frames
            .len();

        let mut project = Project::default();
        let sound = project.add_asset(&ac3)?;
        // The whole file at the start, and again 2 seconds (60 NTSC frames) in.
        let track = AudioTrack {
            more: vec![AudioClip {
                asset: sound,
                start: 60,
                trim_start: 0,
                trim_end: None,
            }],
            ..AudioTrack::of(sound)
        };
        let joined =
            super::ac3_clip_sequence(&project, &track, &dir.join("cache"))?.ok_or("not joined")?;
        let data = fs::read(&joined)?;
        let stream = disccraft_psmux::es::parse_ac3(&data, 0).map_err(|err| err.to_string())?;
        // 2.002 s = 62.56 frames of 1536 samples: 63 frames before the second clip.
        let gap = 63usize.saturating_sub(source_frames);
        assert_eq!(stream.frames.len(), source_frames * 2 + gap);
        // The clips' frames are the file's own bytes.
        assert_eq!(data.get(..source.len()), Some(source.as_slice()));
        assert_eq!(
            data.get(data.len() - source.len()..),
            Some(source.as_slice())
        );
        // A WAV among them: not joined this way.
        let other = project.add_asset(&wav)?;
        let mixed = AudioTrack {
            more: vec![AudioClip {
                asset: other,
                start: 60,
                trim_start: 0,
                trim_end: None,
            }],
            ..AudioTrack::of(sound)
        };
        assert!(super::ac3_clip_sequence(&project, &mixed, &dir.join("cache"))?.is_none());
        fs::remove_dir_all(&dir).unwrap_or(());
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
    /// Estimated bytes of the finished disc: the title VOBs and the ROM content.
    pub estimated_bytes: u64,
    /// Bytes of the ROM content (the files beside the video; 0 without any).
    pub rom_bytes: u64,
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
    // Counted as the build makes it: with the clips' own sound (M6.378).
    estimate_with_probe(&with_clip_sounds(project), &probe)
}

/// The project as a build plays it (M6.378): a title of several clips (or with an empty stretch before a clip) and
/// no audio tracks gets an audio track of its clips' own sounds, each where its clip is on the timeline and cut as
/// its clip is, as the preview plays it. Clips without sound leave silence. A title of one clip keeps playing its
/// file's own sound as before (trimmed with it, M10.21). Titles whose files cannot be read are left as they are.
#[must_use]
pub fn with_clip_sounds(project: &Project) -> Project {
    let mut out = project.clone();
    let rate = project.settings.frame_rate();
    for timeline in out.timelines.values_mut() {
        if !timeline.audio.is_empty()
            || (timeline.more_video.is_empty() && !timeline.has_video_gaps())
        {
            continue;
        }
        let mut frames = 0_i64;
        let mut clips = Vec::new();
        let mut readable = true;
        for (index, video) in timeline.videos().into_iter().enumerate() {
            frames = frames.saturating_add(i64::from(timeline.video_gap(index)));
            let Some(probe) = asset_path(project, video)
                .ok()
                .and_then(|path| disccraft_transcode::probe_video(&path).ok())
            else {
                readable = false;
                break;
            };
            // `as` saturates; a clip's length is far inside the range.
            let length = (probe.seconds * rate.as_f64()).round() as i64;
            let trim = timeline.video_trim(index);
            let end = trim.end.map_or(length, |end| i64::from(end).min(length));
            let kept = end.saturating_sub(i64::from(trim.start)).max(0);
            if probe.has_audio
                && let (Ok(start), Ok(trim_end)) = (u32::try_from(frames), u32::try_from(end))
            {
                clips.push(disccraft_project::AudioClip {
                    asset: video,
                    start,
                    trim_start: trim.start,
                    trim_end: Some(trim_end),
                });
            }
            frames = frames.saturating_add(kept);
        }
        if !readable {
            continue;
        }
        if let Some(first) = clips.first().copied() {
            timeline.audio.push(disccraft_project::AudioTrack {
                more: clips.get(1..).unwrap_or(&[]).to_vec(),
                asset: first.asset,
                language: String::new(),
                start: first.start,
                trim_start: first.trim_start,
                trim_end: first.trim_end,
            });
        }
    }
    out
}

/// [`estimate`] with the media durations from `probe`.
fn estimate_with_probe(
    project: &Project,
    probe: &dyn Fn(&Path) -> Result<f64, DvdError>,
) -> Result<Estimate, DvdError> {
    let rom_bytes = rom_bytes(project)?;
    // A Blu-ray in HD encodes every title's video at the HD bitrate, whatever the sources are.
    if let Some(hd) = hd_budget::hd_budget_with_probe(project, probe)? {
        return Ok(Estimate {
            capacity_bytes: project.settings.capacity.bytes(),
            passthrough_bytes: (hd.passthrough_bits / 8.0) as u64,
            transcode_seconds: hd.video_seconds,
            video_bitrate: hd.target_kbps.saturating_mul(1000),
            estimated_bytes: hd.estimated_bytes().saturating_add(rom_bytes),
            rom_bytes,
            timelines: hd
                .timelines
                .iter()
                .map(|(id, seconds, audio_bits)| (*id, hd.timeline_bytes(*seconds, *audio_bits)))
                .collect(),
        });
    }
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
        estimated_bytes: (scan.passthrough_bits / 8)
            .saturating_add(transcode_bytes)
            .saturating_add(rom_bytes),
        rom_bytes,
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
            let media = asset.media_file();
            let path: &Path = &media;
            let status = match asset.kind {
                AssetKind::Video => video_status(project, path, cache_dir, budget.as_ref()),
                // 24-bit / 96 kHz PCM is a DVD's LPCM as it is.
                AssetKind::Audio
                    if wav_is_fine_lpcm(path, project.settings.kind).is_some()
                        || dvd_mp2(path, project).is_some() =>
                {
                    TranscodeStatus::Unneeded
                }
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
    if passes_through(project, path) {
        return TranscodeStatus::Unneeded;
    }
    // On an HD Blu-ray, H.264 the disc carries goes on it as it is (unless a Blu-ray preset of its own transcodes
    // it all the same, M6.324).
    if !transcode_forced(project, path)
        && crate::BdVideoFormat::of_path(project, path)
            .is_some_and(|format| crate::video_fits_bd(path, format))
    {
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

/// The video assets the build would convert whose pictures are known not to decode (a codec form the decoder does
/// not read, such as interlaced H.264): the build would stop at them. Each as its asset id and name. Only what is
/// known without a wait is told (`disccraft_transcode::picture_decodes`).
#[must_use]
pub fn undecodable_videos(project: &Project) -> Vec<(AssetId, String)> {
    let mut found = Vec::new();
    for asset in project.assets.values() {
        if asset.kind != AssetKind::Video
            || !project
                .timelines
                .values()
                .any(|timeline| timeline.videos().contains(&asset.id))
        {
            continue;
        }
        let media = asset.media_file();
        let path: &Path = &media;
        if video_status(project, path, None, None) == TranscodeStatus::Untranscoded
            && disccraft_transcode::picture_decodes(path) == Some(false)
        {
            found.push((asset.id, asset.name.clone()));
        }
    }
    found
}

/// Encore's Revert to Original: removes what the cache in `cache_dir` holds of `asset`'s transcodes for a DVD (its
/// MPEG-2 video for the project's bitrates, its converted sound and what was made from them), so the next build
/// (or Transcode Now) makes them again. Returns how many files went.
///
/// # Errors
///
/// When the cache folder cannot be read.
pub fn revert_transcodes(
    project: &Project,
    cache_dir: &Path,
    asset: AssetId,
) -> Result<usize, DvdError> {
    let Some(found) = project.assets.get(&asset) else {
        return Err(DvdError::Invalid("asset does not exist".to_string()));
    };
    let media = found.media_file();
    let path: &Path = &media;
    if !cache_dir.is_dir() {
        return Ok(0);
    }
    // The names of its cache files start with its keys: the sound's, and the video's for the project's budget.
    let mut keys = Vec::new();
    if let Ok(key) = audio_cache_key(path) {
        keys.push(key);
    }
    if found.kind == AssetKind::Video
        && let Ok(budget) = budget(project)
    {
        let gop_starts = asset_gop_starts(project, path);
        if let Ok(key) = cache_key(
            project,
            path,
            &budget,
            (asset_wide(project, path), &gop_starts),
        ) {
            keys.push(key);
        }
    }
    let mut removed = 0usize;
    let entries = fs::read_dir(cache_dir).map_err(|err| crate::io_error(cache_dir, err))?;
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let ours = keys.iter().any(|key| {
            name.strip_prefix(key.as_str())
                .is_some_and(|rest| rest.starts_with('.'))
        });
        if ours && entry.path().is_file() && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Status of an audio asset: AC-3 and WAV are used as is, anything else is converted to LPCM.
fn audio_status(path: &Path, cache_dir: Option<&Path>) -> TranscodeStatus {
    if audio_kind(path) != AudioKind::Transcode || container_ac3(path, DiscKind::Dvd).is_some() {
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

#[cfg(test)]
mod clip_tests {
    use super::write_clip;
    use disccraft_project::ClipTrim;

    /// A clip whose end is trimmed stops before the sequence header that leads the cut-off GOP: left at the very
    /// end of a title, that header had nothing after it and the stream no longer parsed.
    #[test]
    fn a_trimmed_end_leaves_no_header_behind() -> Result<(), Box<dyn std::error::Error>> {
        let sequence = [0, 0, 1, 0xB3, 9, 9, 0, 0, 1, 0xB5, 8, 8];
        let group = [0, 0, 1, 0xB8, 0, 0, 0, 0x40];
        let picture = [0, 0, 1, 0x00, 7, 7, 0, 0, 1, 0x01, 6, 6];
        let mut stream = Vec::new();
        for _ in 0..3 {
            stream.extend_from_slice(&sequence);
            stream.extend_from_slice(&group);
            stream.extend_from_slice(&picture);
        }
        stream.extend_from_slice(&[0, 0, 1, 0xB7]);
        let path =
            std::env::temp_dir().join(format!("disccraft-clip-trim-{}.m2v", std::process::id()));
        std::fs::write(&path, &stream)?;
        let run = |trim: ClipTrim, is_last: bool| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
            let mut out = Vec::new();
            write_clip(&path, trim, is_last, &mut out)?;
            Ok(out)
        };
        let one: Vec<u8> = [&sequence[..], &group[..], &picture[..]].concat();
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            // The first GOP only.
            let trim = ClipTrim {
                start: 0,
                end: Some(1),
            };
            assert_eq!(run(trim, true)?, one);
            assert_eq!(run(trim, false)?, one);
            // The middle GOP: the file's headers, then the GOP (closed, so no broken link).
            let trim = ClipTrim {
                start: 1,
                end: Some(2),
            };
            assert_eq!(run(trim, true)?, one);
            // From the last GOP to the end: the end code stays on the last clip only.
            let trim = ClipTrim {
                start: 2,
                end: None,
            };
            let mut ended = one.clone();
            ended.extend_from_slice(&[0, 0, 1, 0xB7]);
            assert_eq!(run(trim, true)?, ended);
            assert_eq!(run(trim, false)?, one);
            Ok(())
        })();
        let _ = std::fs::remove_file(&path);
        result
    }
}
