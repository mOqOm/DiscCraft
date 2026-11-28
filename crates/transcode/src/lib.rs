//! Transcode supported media files to DVD-compliant MPEG-2 video and LPCM WAV audio.
//!
//! This milestone produces one MPEG-2 I-picture video elementary stream and one
//! 48 kHz 16-bit stereo WAV file from a decoded source.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing,
    clippy::cognitive_complexity
)]

mod field_type;
pub use field_type::{FieldType, field_type};
mod audio;
mod avi;
mod avi_mpeg;
mod blend;
mod dv_decoder;
mod dv_source;
mod file_chapters;
pub use file_chapters::file_chapters;
mod stream_languages;
pub use stream_languages::audio_stream_languages;
mod bd_subtitles;
pub use bd_subtitles::{BdSubtitlePart, extract_bd_subtitle};
mod embedded_subtitles;
pub use embedded_subtitles::{
    EmbeddedFormat, EmbeddedSubtitle, embedded_subtitles, extract_embedded_subtitle, vobsub_files,
};
pub mod h264_sps;
pub mod hd;
mod media_cache;
mod mkv_probe;
mod own_source;
mod own_video;
pub use media_cache::{
    cached_blob, clear_probe_cache, enable_probe_cache, media_open, probe_cache_dir, probe_ready,
    store_blob, warm, warming,
};
mod picture;
mod pixel_aspect;
mod plane_colour;
mod pulldown;
mod raw_ac3;
mod resample;
mod session;
pub use pixel_aspect::{
    chosen_pixel_aspect, pixel_aspect, set_pixel_aspects, still_default_aspect, still_own_aspect,
    still_pixel_aspect,
};
pub use session::{document_metadata, document_of, set_compositions};
pub mod slideshow;
mod source;
mod ts_audio;
pub mod ts_index;
mod ts_probe;
mod ts_streams;
pub use ts_streams::ts_stream_types;
mod wav;

use std::path::{Path, PathBuf};

pub use disccraft_mpeg2venc::Standard;

/// Error returned by the transcoder.
#[derive(Debug, thiserror::Error)]
pub enum TranscodeError {
    /// Filesystem or stream I/O failure.
    #[error("io error at {path}: {message}")]
    Io {
        /// Path associated with the failure.
        path: String,
        /// Human-readable I/O message.
        message: String,
    },
    /// Source decoding failure.
    #[error("decode error: {0}")]
    Decode(String),
    /// MPEG-2 encoding failure.
    #[error("encode error: {0}")]
    Encode(String),
    /// Unsupported input, output, or option.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// The transcode was stopped ([`request_stop`]): what it had written is not a transcode.
    #[error("{STOPPED}")]
    Stopped,
}

/// What a transcode that was stopped says of itself ([`TranscodeError::Stopped`]); a caller tells a stop from a
/// failure by it.
pub const STOPPED: &str = "the transcode was stopped";

/// Set while the transcodes are asked to stop.
static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Asks every running transcode to stop (Encore's Progress panel ▸ Stop): each gives up at its next picture
/// with [`TranscodeError::Stopped`]. It stays asked until [`clear_stop`].
pub fn request_stop() {
    STOP.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Lets transcodes run again: called before a build or a Transcode Now starts.
pub fn clear_stop() {
    STOP.store(false, std::sync::atomic::Ordering::Relaxed);
}

/// Whether the transcodes are asked to stop.
#[must_use]
pub fn stop_requested() -> bool {
    STOP.load(std::sync::atomic::Ordering::Relaxed)
}

/// [`TranscodeError::Stopped`] when the transcodes are asked to stop: called once a picture by each of them.
pub(crate) fn check_stop() -> Result<(), TranscodeError> {
    if stop_requested() {
        Err(TranscodeError::Stopped)
    } else {
        Ok(())
    }
}

/// Options for transcoding one media file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscodeOptions {
    /// Target broadcast standard.
    pub standard: Standard,
    /// Target video bitrate in bits per second.
    pub video_bitrate: u32,
    /// Maximum video bitrate in bits per second.
    pub max_bitrate: u32,
    /// 16:9 anamorphic output instead of 4:3.
    pub wide: bool,
    /// Output frames (at the standard's rate, from 0) where a closed GOP must start: the chapters and the layer
    /// break of the titles that play the video, so their DVD cells begin exactly there. Between them the GOPs keep
    /// their regular length.
    pub gop_starts: Vec<u64>,
    /// How the bit rate is kept: one-pass VBR, two-pass VBR or CBR.
    pub mode: RateMode,
    /// Film (NTSC only; Encore's 23.976 presets): the video is made of the source's pictures at 23.976 a second,
    /// progressive, shown by 3:2 pulldown in the DVD's 29.97 sequence (soft pulldown). `gop_starts` are then
    /// counted in those pictures.
    pub film: bool,
    /// How hard the encoder searches for motion: Encore's quality (1 to 5; its "Low quality" presets' 2, the
    /// others' 5). See [`disccraft_mpeg2venc::Encoder::set_quality`].
    pub quality: u8,
    /// How the pictures are made (Encore's rendering checkboxes).
    pub rendering: Rendering,
}

/// How a transcode makes its pictures: Encore's Transcode Settings checkboxes "Use Maximum Render Quality" and
/// "Use Frame Blending". Both off is how DiscCraft always made them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rendering {
    /// Scale with an area filter (scaling down) or between pixel centres (scaling up) instead of sampling two by
    /// two pixels bilinearly: fine detail averages out instead of aliasing.
    pub max_quality: bool,
    /// Where an output picture falls between two of the source's (another frame rate), mix the two by how near
    /// it is to each instead of showing the one before it.
    pub frame_blend: bool,
}

/// The best quality (Encore's 5): the encoder's whole motion search. See [`TranscodeOptions::quality`].
pub const BEST_QUALITY: u8 = disccraft_mpeg2venc::encoder::BEST_QUALITY;

/// How a transcode keeps its video bit rate (Encore's quality presets: VBR 1 pass, VBR 2 pass, CBR 1 pass).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RateMode {
    /// Variable, in one pass: the rate averages the target, up to the maximum.
    #[default]
    Vbr,
    /// Variable, in two passes: a first pass measures every picture (writing nothing), the second shares the bits
    /// by what it measured. Twice the time.
    Vbr2Pass,
    /// Constant: the target is also the maximum.
    Cbr,
}

impl Default for TranscodeOptions {
    /// Default DVD-oriented transcode options.
    fn default() -> Self {
        Self {
            standard: Standard::Ntsc,
            video_bitrate: 6_000_000,
            max_bitrate: 9_800_000,
            wide: false,
            gop_starts: Vec::new(),
            mode: RateMode::Vbr,
            film: false,
            quality: BEST_QUALITY,
            rendering: Rendering::default(),
        }
    }
}

/// Files produced by a successful transcode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscodeOutput {
    /// MPEG-2 video elementary stream path.
    pub video: PathBuf,
    /// Optional LPCM WAV path.
    pub audio: Option<PathBuf>,
    /// Number of encoded video frames.
    pub frames: u64,
}

pub(crate) const TICKS_PER_SECOND: i64 = 254_016_000_000;

/// Transcode `input` into DVD-compliant MPEG-2 video and optional LPCM WAV audio.
pub fn transcode(
    input: &Path,
    video_out: &Path,
    audio_out: &Path,
    options: &TranscodeOptions,
    progress: &mut dyn FnMut(f32),
) -> Result<TranscodeOutput, TranscodeError> {
    if options.video_bitrate == 0 {
        return Err(TranscodeError::Unsupported(
            "video bitrate must be non-zero".to_string(),
        ));
    }

    let max_bitrate = if options.max_bitrate == 0 {
        options.video_bitrate
    } else {
        options.max_bitrate.max(options.video_bitrate)
    };

    let source = source::Source::open(input)?;
    let duration = source.info().duration;
    let film = options.film && options.standard == Standard::Ntsc;
    let frames = picture::frame_count(duration, options.standard, film).ok_or_else(|| {
        TranscodeError::Unsupported("could not compute output frame count".to_string())
    })?;

    picture::encode_video(&source, video_out, options, max_bitrate, frames, progress)?;

    let audio = audio::extract_audio(&source, frames, (options.standard, film), audio_out)?;

    Ok(TranscodeOutput {
        video: video_out.to_path_buf(),
        audio,
        frames,
    })
}

/// Transcode an audio-only input to a 48 kHz stereo 16-bit WAV file.
///
/// If `seconds` is `None`, the source duration is used. Returns the number of
/// seconds actually written.
pub fn transcode_audio(
    input: &Path,
    wav_out: &Path,
    seconds: Option<f64>,
) -> Result<f64, TranscodeError> {
    transcode_audio_stream(input, 0, wav_out, seconds)
}

/// [`transcode_audio`] of sound stream `stream` of the file (M6.418: 0 the first).
///
/// # Errors
///
/// When the file cannot be read, has no such stream, or the WAV cannot be written.
pub fn transcode_audio_stream(
    input: &Path,
    stream: usize,
    wav_out: &Path,
    seconds: Option<f64>,
) -> Result<f64, TranscodeError> {
    let source = source::Source::open_audio_or_extracted(input)?.with_stream(stream);

    let sample_frames = match seconds {
        Some(seconds) => audio::sample_frames_from_seconds(seconds).ok_or_else(|| {
            TranscodeError::Unsupported("audio duration out of range".to_string())
        })?,
        None => audio::sample_frames_from_ticks(source.info().duration.0).ok_or_else(|| {
            TranscodeError::Unsupported("audio duration out of range".to_string())
        })?,
    };

    let written = audio::extract_audio_samples(&source, sample_frames, wav_out)?;
    Ok(written as f64 / f64::from(audio::SAMPLE_RATE))
}

pub(crate) fn io_error(path: &Path, error: std::io::Error) -> TranscodeError {
    TranscodeError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}

/// Duration of a media file in seconds (opens it with the same FilmCraft openers as [`transcode`]).
pub fn probe_seconds(input: &Path) -> Result<f64, TranscodeError> {
    // Remembered from an earlier look at the file, when the application keeps a probe cache.
    if let Some(video) = remembered_video(input) {
        return Ok(video.seconds);
    }
    probe_video(input).map(|probe| probe.seconds)
}

/// The facts of a video file remembered in the probe cache (`media_cache`), if any.
fn remembered_video(input: &Path) -> Option<VideoProbe> {
    let text = media_cache::fact("video", input).or_else(|| told("video", input))?;
    let mut parts = text.split(',');
    let probe = VideoProbe {
        seconds: parts.next()?.parse().ok()?,
        width: parts.next()?.parse().ok()?,
        height: parts.next()?.parse().ok()?,
        fps: parts.next()?.parse().ok()?,
        wide: parts.next()? == "1",
        has_audio: parts.next()? == "1",
    };
    (probe.seconds.is_finite() && probe.fps.is_finite()).then_some(probe)
}

/// The GOPs of an MPEG-2 video elementary stream (`.m2v`), read without decoding: when each one's first picture is
/// shown, in seconds from the stream's first shown picture (soft pulldown counted), and whether it is closed. A DVD
/// cell, so a chapter, can start only at a closed GOP of video that is passed through. `None` when the stream cannot
/// be read so (not an elementary stream, field pictures, a progressive sequence, an unknown frame rate).
///
/// # Errors
///
/// When the file cannot be read.
pub fn mpeg2_gop_starts(input: &Path) -> Result<Option<Vec<(f64, bool)>>, TranscodeError> {
    let Some(scanned) = pulldown::scan(input).map_err(|error| io_error(input, error))? else {
        return Ok(None);
    };
    let Some(rate) = pulldown::frame_rate_of_code(scanned.frame_rate_code) else {
        return Ok(None);
    };
    Ok(Some(
        pulldown::gop_starts(&scanned.pictures, rate)
            .into_iter()
            .map(|(start, closed)| (start.0 as f64 / TICKS_PER_SECOND as f64, closed))
            .collect(),
    ))
}

/// Video stream facts for asset views: duration, dimensions and frame rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoProbe {
    /// Duration in seconds.
    pub seconds: f64,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Frames per second (0.0 when the file does not say).
    pub fps: f64,
    /// Whether the picture is wide: shown at 16:9 or wider (its frame size times its pixel aspect), not 4:3.
    pub wide: bool,
    /// Whether the file also has sound (at least one audio stream).
    pub has_audio: bool,
}

/// Whether a `width`×`height` picture with pixel aspect `par` is shown wide (16:9 or wider) rather than 4:3: its
/// display aspect is above 1.55, between 4:3 (1.33) and 16:9 (1.78). A 720×480 frame of square pixels (1.5) is
/// 4:3; a pixel aspect of 0 counts as square.
pub fn is_wide(width: u32, height: u32, par: (u32, u32)) -> bool {
    let par_num = if par.0 == 0 { 1 } else { par.0 };
    let par_den = if par.1 == 0 { 1 } else { par.1 };
    // width·par_num / (height·par_den) > 1.55, in integers: 100·width·par_num > 155·height·par_den.
    u128::from(width) * u128::from(par_num) * 100 > u128::from(height) * u128::from(par_den) * 155
}

/// Probe a video file: duration, dimensions and frame rate.
pub fn probe_video(input: &Path) -> Result<VideoProbe, TranscodeError> {
    let mut probe = probe_video_as_it_is(input)?;
    // Interpret Footage: whether the picture is wide follows the pixel aspect ratio chosen for the file.
    if let Some(aspect) = chosen_pixel_aspect(input) {
        probe.wide = is_wide(probe.width, probe.height, aspect);
    }
    Ok(probe)
}

/// [`probe_video`] as the file itself says (what is remembered of a file is the file's own).
fn probe_video_as_it_is(input: &Path) -> Result<VideoProbe, TranscodeError> {
    if let Some(probe) = remembered_video(input) {
        return Ok(probe);
    }
    let probe = probe_video_now(input)?;
    media_cache::remember(
        "video",
        input,
        &format!(
            "{},{},{},{},{},{}",
            probe.seconds,
            probe.width,
            probe.height,
            probe.fps,
            u8::from(probe.wide),
            u8::from(probe.has_audio)
        ),
    );
    Ok(probe)
}

/// Remembers what video file `input` is as something other than the file tells it (M6.429: a Blu-ray's playlist and
/// clip information give a clip's length, picture size, rate and sound), so that [`probe_video`] answers without
/// reading the file through (indexing a transport stream reads all of it: minutes for a Blu-ray's clips). A file
/// already probed keeps what it said; what is remembered goes when the file changes.
pub fn tell_video(input: &Path, probe: &VideoProbe) {
    if remembered_video(input).is_some()
        || !probe.seconds.is_finite()
        || probe.seconds <= 0.0
        || !probe.fps.is_finite()
        || probe.fps <= 0.0
    {
        return;
    }
    let text = format!(
        "{},{},{},{},{},{}",
        probe.seconds,
        probe.width,
        probe.height,
        probe.fps,
        u8::from(probe.wide),
        u8::from(probe.has_audio)
    );
    tell("video", input, &text);
}

/// Remembers how long the sound of media file `input` is (seconds) as something other than the file tells it
/// (M6.429: a Blu-ray clip's presentation time), so that [`probe_audio_seconds`] answers without reading the file
/// through. A file already measured keeps what it said.
pub fn tell_audio_seconds(input: &Path, seconds: f64) {
    if !seconds.is_finite() || seconds <= 0.0 || told("audio", input).is_some() {
        return;
    }
    if media_cache::fact("audio", input).is_some() {
        return;
    }
    tell("audio", input, &seconds.to_string());
}

/// Keeps fact `kind` of `input` for this run (whether or not the media cache keeps facts between runs) and in the
/// media cache.
fn tell(kind: &str, input: &Path, text: &str) {
    if let (Some(key), Ok(mut told)) = (told_key(kind, input), TOLD.lock()) {
        if told.len() >= MAX_TOLD {
            told.clear();
        }
        told.retain(|(known, _)| *known != key);
        told.push((key, text.to_owned()));
    }
    media_cache::remember(kind, input, text);
}

/// What [`tell_video`] and [`tell_audio_seconds`] were told in this run, by kind and file (its path, size and
/// modification time).
static TOLD: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());
/// The most files told of in one run.
const MAX_TOLD: usize = 1_000;

/// The key of fact `kind` of a file as it is now for [`TOLD`].
fn told_key(kind: &str, input: &Path) -> Option<String> {
    let metadata = std::fs::metadata(input).ok()?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos());
    let path = std::path::absolute(input).unwrap_or_else(|_| input.to_path_buf());
    Some(format!(
        "{kind}	{}	{}	{modified}",
        path.display(),
        metadata.len()
    ))
}

/// What was told of fact `kind` of `input` in this run, as the file is now.
fn told(kind: &str, input: &Path) -> Option<String> {
    let key = told_key(kind, input)?;
    TOLD.lock()
        .ok()?
        .iter()
        .find(|(told, _)| *told == key)
        .map(|(_, text)| text.clone())
}

/// [`probe_video`] by opening the file.
fn probe_video_now(input: &Path) -> Result<VideoProbe, TranscodeError> {
    // A Matroska file says it all in its header: it is not read through to be asked.
    if let Some(probe) = mkv_probe::quick(input) {
        return Ok(probe);
    }
    let source = match source::Source::open(input) {
        Ok(source) => source,
        // A transport stream the media reader refuses (H.264 coded as fields or MBAFF) still tells its length,
        // size and rate: enough to put it on a Blu-ray as it is.
        Err(error) => return ts_probe::quick(input).ok_or(error),
    };
    let info = source.info();
    let seconds = info.duration.0 as f64 / 254_016_000_000.0;
    let video = info
        .video
        .as_ref()
        .ok_or_else(|| TranscodeError::Unsupported("input has no video stream".to_string()))?;
    let fps = if video.frame_rate.den == 0 {
        0.0
    } else {
        video.frame_rate.num as f64 / video.frame_rate.den as f64
    };

    Ok(VideoProbe {
        seconds,
        width: video.width,
        height: video.height,
        fps,
        wide: is_wide(video.width, video.height, video.par),
        has_audio: source.has_audio(),
    })
}

/// Whether the pictures of the video file `input` can be decoded, when that is known without a wait: found out
/// before (and remembered with the file's other facts), or tried now on a file that is already open. `None`
/// while the file is not open yet. A file whose picture cannot be decoded cannot be converted either.
#[must_use]
pub fn picture_decodes(input: &Path) -> Option<bool> {
    // That it decodes stays true; that it does not is only this build's verdict (a later build's decoder may
    // read it), so it is remembered with the build and tried again by another.
    let not_by_this_build = format!("0:{}", build_tag());
    match media_cache::fact("picture", input) {
        Some(known) if known == "1" => return Some(true),
        Some(known) if known == not_by_this_build => return Some(false),
        _ => {}
    }
    if !media_open(input) {
        return None;
    }
    let decodes = first_frame(input).is_ok();
    media_cache::remember(
        "picture",
        input,
        if decodes { "1" } else { &not_by_this_build },
    );
    Some(decodes)
}

/// A name for this build of the program: its executable's size and modification time. What a build's decoders
/// could not read is remembered under it, so that a newer build tries again instead of trusting the old answer.
#[must_use]
pub fn build_tag() -> &'static str {
    static TAG: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TAG.get_or_init(|| {
        let metadata = std::env::current_exe()
            .ok()
            .and_then(|exe| std::fs::metadata(exe).ok());
        let modified = metadata
            .as_ref()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_secs());
        let len = metadata.map_or(0, |metadata| metadata.len());
        format!("{len:x}-{modified:x}")
    })
}

/// First frame of a video file as letterboxed RGB24 at the NTSC DVD frame size (for thumbnails).
pub fn first_frame(input: &Path) -> Result<(u32, u32, Vec<u8>), TranscodeError> {
    let source = source::Source::open(input)?;
    let mut rgb = Vec::new();
    picture::for_each_rgb_frame(&source, Standard::Ntsc, 1, false, &mut |_, frame| {
        rgb.extend_from_slice(frame);
        Ok(())
    })?;

    let expected = 720_usize.checked_mul(480).and_then(|n| n.checked_mul(3));
    if Some(rgb.len()) != expected {
        return Err(TranscodeError::Decode(
            "unexpected thumbnail frame size".to_string(),
        ));
    }
    Ok((720, 480, rgb))
}

/// The frame of a video file at `seconds` (clamped to the file) as letterboxed RGB24, 720 x 480 (monitor view).
pub fn frame_at(input: &Path, seconds: f64) -> Result<(u32, u32, Vec<u8>), TranscodeError> {
    FrameReader::open(input)?.frame(seconds)
}

/// How far apart [`FrameReader::first_bright_time`] looks.
pub const BRIGHT_STEP_SECONDS: f64 = 0.25;
/// How far [`FrameReader::first_bright_time`] looks.
pub const BRIGHT_SEARCH_SECONDS: f64 = 10.0;

/// How far apart [`FrameReader::first_chapter_poster`] looks: half a second, as Encore picks a new timeline's poster
/// frame (1;15 or 2;00 for clips that fade in from black).
pub const POSTER_STEP_SECONDS: f64 = 0.5;

/// Whether an RGB24 picture shows something to stand for a new timeline (Encore's first-chapter poster frame): more
/// than 0.1 % of its pixels have a channel above 180. Measured on Encore CS6's own picks (2026-11-23, a test
/// disc's clips): a fade from black whose brightest part has reached 165 is still passed over, one at 199 is taken,
/// whatever the average (a dim picture with a bright title counts; a mid-grey fade does not).
pub fn is_poster_lit(rgb: &[u8]) -> bool {
    const LEVEL: u8 = 180;
    let pixels = rgb.len() / 3;
    if pixels == 0 {
        return false;
    }
    let lit = rgb
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|pixel| pixel.iter().any(|channel| *channel > LEVEL))
        .count();
    lit.saturating_mul(1000) > pixels
}

/// Whether an RGB24 picture is bright (not black): more than 2 % of its pixels have a channel above 40 (a fade from
/// black, letterbox bars and a dark scene with something in it are told apart from a black frame).
pub fn is_bright(rgb: &[u8]) -> bool {
    const LEVEL: u8 = 40;
    let pixels = rgb.len() / 3;
    if pixels == 0 {
        return false;
    }
    let lit = rgb
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|pixel| pixel.iter().any(|channel| *channel > LEVEL))
        .count();
    lit.saturating_mul(50) > pixels
}

/// An audio file (or a video file's sound) held open for playback: read in order as interleaved stereo at the
/// output's rate.
pub struct AudioReader {
    source: source::Source,
}

impl AudioReader {
    /// The most frames one read returns (ten seconds at 48 kHz), so a read's buffer is bounded.
    pub const MAX_READ_FRAMES: usize = 480_000;

    /// Opens a file with sound.
    ///
    /// # Errors
    ///
    /// When the file cannot be read or has no audio stream.
    pub fn open(input: &Path) -> Result<Self, TranscodeError> {
        Self::open_stream(input, 0)
    }

    /// Opens sound stream `stream` of a file (M6.418: 0 the first).
    ///
    /// # Errors
    ///
    /// When the file cannot be read or has no such stream.
    pub fn open_stream(input: &Path, stream: usize) -> Result<Self, TranscodeError> {
        let source = source::Source::open_audio_or_extracted(input)?.with_stream(stream);
        if !source.has_audio() {
            return Err(TranscodeError::Unsupported(format!(
                "{}: no sound stream {}",
                input.display(),
                stream + 1
            )));
        }
        Ok(Self { source })
    }

    /// The length of the file in seconds.
    pub fn duration_seconds(&self) -> f64 {
        self.source.info().duration.0 as f64 / TICKS_PER_SECOND as f64
    }

    /// Up to `frames` stereo frames from frame `start` (counted at `rate`), interleaved left and right and
    /// downmixed as the disc's audio is; fewer (none) at the end.
    ///
    /// # Errors
    ///
    /// When the audio cannot be decoded.
    pub fn read_stereo(
        &self,
        start: u64,
        frames: usize,
        rate: u32,
    ) -> Result<Vec<f32>, TranscodeError> {
        let start = i64::try_from(start)
            .map_err(|_| TranscodeError::Unsupported("audio position too large".to_string()))?;
        let frames = frames.min(Self::MAX_READ_FRAMES);
        if frames == 0 || rate == 0 {
            return Ok(Vec::new());
        }
        let buffer = self.source.audio(start, frames, rate)?;
        let mut samples = audio::interleaved_stereo(&buffer);
        samples.truncate(frames.saturating_mul(2));
        Ok(samples)
    }
}

/// How much later than asked a picture is taken (M6.449). A time carried in seconds (f64) can land a hair before
/// the frame it names: a test disc's chapter 7, tick 264 762 891 993 600 (frame 31 238 at 29.97), comes back as
/// tick …599.97, and the frame before a cut there was shown in its place (about 1 frame start in 40 at 29.97). A
/// microsecond is far past such error and far inside any frame.
pub const FRAME_SNAP_SECONDS: f64 = 1e-6;

/// The tick whose picture shows at `seconds` (0 for a bad or negative time): [`FRAME_SNAP_SECONDS`] later.
fn picture_tick(seconds: f64) -> i64 {
    if !seconds.is_finite() || seconds <= 0.0 {
        return 0;
    }
    // `as` saturates at the i64 range.
    ((seconds + FRAME_SNAP_SECONDS) * TICKS_PER_SECOND as f64) as i64
}

/// The widest frame [`FrameReader::frame_sized`] renders (UHD).
pub const MAX_FRAME_WIDTH: u32 = 3840;
/// The tallest frame [`FrameReader::frame_sized`] renders (UHD).
pub const MAX_FRAME_HEIGHT: u32 = 2160;

/// A video file held open for playback in the monitor: asking for the frames in order reuses the decoder, so the
/// next frame is cheap.
pub struct FrameReader {
    source: source::Source,
}

impl FrameReader {
    /// Opens a video file.
    pub fn open(input: &Path) -> Result<Self, TranscodeError> {
        Ok(Self {
            source: source::Source::open(input)?,
        })
    }

    /// Opens a video file with a decoder of its own, not the one shared with every other reader of the file
    /// (M6.486): asking it for pictures far apart does not move the shared decoder (the monitor's playback) away.
    pub fn open_own(input: &Path) -> Result<Self, TranscodeError> {
        Ok(Self {
            source: source::Source::open_own(input)?,
        })
    }

    /// The length of the video in seconds.
    pub fn duration_seconds(&self) -> f64 {
        self.source.info().duration.0 as f64 / TICKS_PER_SECOND as f64
    }

    /// When the first bright (non-black) picture from `seconds` is (Encore's still video thumbnail without a poster
    /// frame), looking every [`BRIGHT_STEP_SECONDS`] for up to [`BRIGHT_SEARCH_SECONDS`]; `seconds` when none is
    /// bright.
    pub fn first_bright_time(&self, seconds: f64) -> Result<f64, TranscodeError> {
        let start = if seconds.is_finite() {
            seconds.max(0.0)
        } else {
            0.0
        };
        let end = (start + BRIGHT_SEARCH_SECONDS).min(self.duration_seconds());
        let mut step = 0_u32;
        loop {
            let time = start + f64::from(step) * BRIGHT_STEP_SECONDS;
            if step > 0 && time >= end {
                return Ok(start);
            }
            if is_bright(&self.picture(time)?.2) {
                return Ok(time);
            }
            step = step.saturating_add(1);
        }
    }

    /// The poster frame Encore gives a new timeline's first chapter: none when the picture at the start shows
    /// something ([`is_poster_lit`]); otherwise the first half second (0.5 s, 1 s, 1.5 s…, up to
    /// [`BRIGHT_SEARCH_SECONDS`] and the end of the file) that does, or none when nothing does.
    pub fn first_chapter_poster(&self) -> Result<Option<f64>, TranscodeError> {
        if is_poster_lit(&self.picture(0.0)?.2) {
            return Ok(None);
        }
        let end = BRIGHT_SEARCH_SECONDS.min(self.duration_seconds());
        let mut step = 1_u32;
        loop {
            let time = f64::from(step) * POSTER_STEP_SECONDS;
            if time >= end {
                return Ok(None);
            }
            if is_poster_lit(&self.picture(time)?.2) {
                return Ok(Some(time));
            }
            step = step.saturating_add(1);
        }
    }

    /// The picture at `seconds` (clamped to the file) without letterbox or pillarbox bars, as RGB24: (width,
    /// height, pixels). A video thumbnail stretches it over its area, as Encore shows the whole picture there.
    pub fn picture(&self, seconds: f64) -> Result<(u32, u32, Vec<u8>), TranscodeError> {
        picture::rgb_picture_at(&self.source, self.tick_at(seconds))
    }

    /// `seconds` as a time inside the file.
    fn tick_at(&self, seconds: f64) -> filmcraft_time::Tick {
        let duration = self.source.info().duration.0;
        let last = duration.saturating_sub(1).max(0);
        filmcraft_time::Tick(picture_tick(seconds).clamp(0, last))
    }

    /// The frame at `seconds` (clamped to the file) as letterboxed RGB24, 720 x 480.
    pub fn frame(&self, seconds: f64) -> Result<(u32, u32, Vec<u8>), TranscodeError> {
        let rgb = picture::rgb_frame_at(&self.source, self.tick_at(seconds))?;
        Ok((720, 480, rgb))
    }

    /// The frame at `seconds` (clamped to the file) as RGB24 at `width` x `height` with square pixels, the picture
    /// fitted in with bars as the HD transcode renders it (a Blu-ray HD title in the preview): (width, height,
    /// pixels). The size is kept within 16 x 16 and [`MAX_FRAME_WIDTH`] x [`MAX_FRAME_HEIGHT`].
    pub fn frame_sized(
        &self,
        seconds: f64,
        width: u32,
        height: u32,
    ) -> Result<(u32, u32, Vec<u8>), TranscodeError> {
        let width = width.clamp(16, MAX_FRAME_WIDTH);
        let height = height.clamp(16, MAX_FRAME_HEIGHT);
        let frame = self.source.video_frame(self.tick_at(seconds))?;
        let rgb = hd::render_fit(&frame, width, height)?;
        Ok((width, height, rgb))
    }
}

/// How many sound streams the file at `input` has (M6.418): an MKV's languages, a camcorder's two stereo pairs, …;
/// 0 when it cannot be read as media (a raw AC-3 or DTS file, which has one, reads as 0 here too).
#[must_use]
pub fn audio_stream_count(input: &Path) -> usize {
    source::Source::open(input)
        .or_else(|_| source::Source::open_audio_or_extracted(input))
        .map_or(0, |source| source.audio_streams())
}

/// Duration of an audio-only media file in seconds. A raw AC-3 elementary stream (which the openers do not
/// read) is measured from its sync frames: 1536 samples each.
pub fn probe_audio_seconds(input: &Path) -> Result<f64, TranscodeError> {
    if let Some(seconds) = media_cache::fact("audio", input)
        .or_else(|| told("audio", input))
        .and_then(|text| text.parse::<f64>().ok())
        .filter(|seconds| seconds.is_finite())
    {
        return Ok(seconds);
    }
    // A raw AC-3 file tells its length from its first frame and its size: it is not decoded for that.
    let raw_ac3 = raw_ac3::is_raw_ac3(input)
        .then(|| raw_ac3::seconds(input))
        .flatten();
    let seconds = match mkv_probe::quick(input).filter(|probe| probe.has_audio) {
        // A Matroska video's sound is as long as the file.
        Some(probe) => probe.seconds,
        None if raw_ac3.is_some() => raw_ac3.unwrap_or_default(),
        None => match source::Source::open_audio(input) {
            Ok(source) => source.info().duration.0 as f64 / 254_016_000_000.0,
            Err(error) => match ac3_seconds(input) {
                Some(seconds) => seconds,
                // A transport stream whose video the media reader refuses: its sound is as long as the file.
                None => ts_probe::quick(input)
                    .filter(|probe| probe.has_audio)
                    .map(|probe| probe.seconds)
                    .ok_or(error)?,
            },
        },
    };
    media_cache::remember("audio", input, &seconds.to_string());
    Ok(seconds)
}

/// The length of a raw AC-3 file from its frame count, `None` when it is not AC-3. Files past 64 MiB are
/// not read (a sanity bound; DVD audio is far smaller).
fn ac3_seconds(input: &Path) -> Option<f64> {
    const MAX_AC3_BYTES: u64 = 64 * 1024 * 1024;
    let metadata = std::fs::metadata(input).ok()?;
    if metadata.len() > MAX_AC3_BYTES {
        return None;
    }
    let data = std::fs::read(input).ok()?;
    let stream = disccraft_psmux::es::parse_ac3(&data, 0).ok()?;
    if stream.sample_rate == 0 || stream.frames.is_empty() {
        return None;
    }
    // Frame counts are small; the float holds them exactly.
    Some(stream.frames.len() as f64 * 1536.0 / f64::from(stream.sample_rate))
}

/// Feeds the first `frames` frames of a video file (RGB24 at the DVD frame size and rate, letterboxed for a 16:9
/// screen when `wide`, M6.406, else a 4:3 one) to `f` — motion menu backgrounds.
pub fn for_each_frame(
    input: &Path,
    standard: Standard,
    frames: u64,
    wide: bool,
    f: &mut dyn FnMut(u64, &[u8]) -> Result<(), String>,
) -> Result<(), TranscodeError> {
    let source = source::Source::open(input)?;
    picture::for_each_rgb_frame(&source, standard, frames, wide, f)
}

#[cfg(test)]
mod tests {
    use super::{TICKS_PER_SECOND, is_bright, is_poster_lit, is_wide, picture_tick};

    #[test]
    fn a_frame_start_in_seconds_shows_that_frame() {
        // Every frame start of three hours at 29.97 and 23.976, carried as seconds, lands inside its own frame (a
        // test disc's chapter 7, frame 31 238, showed the frame before a cut).
        for (numerator, denominator) in [(30_000_i64, 1001_i64), (24_000, 1001)] {
            let frame_ticks = TICKS_PER_SECOND / numerator * denominator;
            for frame in 0..(3 * 3600 * numerator / denominator) {
                let start = frame * frame_ticks;
                let tick = picture_tick(start as f64 / TICKS_PER_SECOND as f64);
                assert!(
                    (start..start + frame_ticks).contains(&tick),
                    "frame {frame} at {numerator}/{denominator}: tick {tick}, frame from {start}"
                );
            }
        }
        assert_eq!(picture_tick(-1.0), 0);
        assert_eq!(picture_tick(f64::NAN), 0);
    }

    #[test]
    fn a_picture_is_wide_from_its_display_aspect() {
        // 16:9 in square pixels (HD), anamorphic DVD and HDV.
        assert!(is_wide(1280, 720, (1, 1)));
        assert!(is_wide(1920, 1080, (1, 1)));
        assert!(is_wide(720, 480, (32, 27)));
        assert!(is_wide(720, 576, (64, 45)));
        assert!(is_wide(1440, 1080, (4, 3)));
        // 4:3 DVD, and a 720×480 frame that does not say (1.5: not 16:9).
        assert!(!is_wide(720, 480, (8, 9)));
        assert!(!is_wide(720, 576, (16, 15)));
        assert!(!is_wide(720, 480, (1, 1)));
        assert!(!is_wide(720, 480, (0, 0)));
        assert!(!is_wide(640, 480, (1, 1)));
        // Hostile sizes do not overflow.
        assert!(is_wide(u32::MAX, 1, (u32::MAX, 1)));
        assert!(!is_wide(0, 0, (0, 0)));
    }

    #[test]
    fn a_black_frame_is_not_bright_but_a_dark_scene_with_something_in_it_is() {
        // 100 pixels: all black (video black may be a little above 0).
        let black = vec![16_u8; 300];
        assert!(!is_bright(&black));
        // One lit pixel in a hundred: still black (noise, a logo's corner).
        let mut speck = black.clone();
        if let Some(pixel) = speck.get_mut(0..3) {
            pixel.copy_from_slice(&[200, 200, 200]);
        }
        assert!(!is_bright(&speck));
        // Three lit pixels in a hundred: something is there.
        let mut lit = black.clone();
        for index in [0_usize, 30, 60] {
            if let Some(pixel) = lit.get_mut(index..index + 3) {
                pixel.copy_from_slice(&[0, 90, 0]);
            }
        }
        assert!(is_bright(&lit));
        assert!(!is_bright(&[]));
    }

    /// Encore's first-chapter poster takes a picture with something near white in it, not a dim one (a test disc's
    /// clips: brightest 165 passed over, 199 taken), and not a speck.
    #[test]
    fn a_poster_needs_something_near_white() {
        // 2000 pixels of video black.
        let black = vec![16_u8; 6000];
        assert!(!is_poster_lit(&black));
        // A fade whose brightest part is at 165 everywhere: still passed over.
        assert!(!is_poster_lit(&vec![165_u8; 6000]));
        // A dim picture with a bright title over 0.15 % of it: taken.
        let mut title = black.clone();
        for index in 0..3 {
            if let Some(pixel) = title.get_mut(index * 3..index * 3 + 3) {
                pixel.copy_from_slice(&[199, 199, 199]);
            }
        }
        assert!(is_poster_lit(&title));
        // Two bright pixels in 2000 (0.1 %): a speck, not taken.
        let mut speck = black;
        for index in 0..2 {
            if let Some(pixel) = speck.get_mut(index * 3..index * 3 + 3) {
                pixel.copy_from_slice(&[255, 255, 255]);
            }
        }
        assert!(!is_poster_lit(&speck));
        assert!(!is_poster_lit(&[]));
    }
}
