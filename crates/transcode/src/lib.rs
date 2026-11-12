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

mod audio;
pub mod hd;
mod picture;
pub mod slideshow;
mod source;
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
}

impl Default for TranscodeOptions {
    /// Default DVD-oriented transcode options.
    fn default() -> Self {
        Self {
            standard: Standard::Ntsc,
            video_bitrate: 6_000_000,
            max_bitrate: 9_800_000,
            wide: false,
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
    let frames = picture::frame_count(duration, options.standard).ok_or_else(|| {
        TranscodeError::Unsupported("could not compute output frame count".to_string())
    })?;

    picture::encode_video(&source, video_out, options, max_bitrate, frames, progress)?;

    let audio = audio::extract_audio(&source, frames, options.standard, audio_out)?;

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
    let source = source::Source::open_audio(input)?;

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
    let source = source::Source::open(input)?;
    Ok(source.info().duration.0 as f64 / 254_016_000_000.0)
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
}

/// Probe a video file: duration, dimensions and frame rate.
pub fn probe_video(input: &Path) -> Result<VideoProbe, TranscodeError> {
    let source = source::Source::open(input)?;
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
    })
}

/// First frame of a video file as letterboxed RGB24 at the NTSC DVD frame size (for thumbnails).
pub fn first_frame(input: &Path) -> Result<(u32, u32, Vec<u8>), TranscodeError> {
    let source = source::Source::open(input)?;
    let mut rgb = Vec::new();
    picture::for_each_rgb_frame(&source, Standard::Ntsc, 1, &mut |_, frame| {
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

    /// The length of the video in seconds.
    pub fn duration_seconds(&self) -> f64 {
        self.source.info().duration.0 as f64 / TICKS_PER_SECOND as f64
    }

    /// The frame at `seconds` (clamped to the file) as letterboxed RGB24, 720 x 480.
    pub fn frame(&self, seconds: f64) -> Result<(u32, u32, Vec<u8>), TranscodeError> {
        let duration = self.source.info().duration.0;
        let wanted = if seconds.is_finite() && seconds > 0.0 {
            // `as` saturates at the i64 range.
            (seconds * TICKS_PER_SECOND as f64) as i64
        } else {
            0
        };
        let last = duration.saturating_sub(1).max(0);
        let rgb = picture::rgb_frame_at(&self.source, filmcraft_time::Tick(wanted.clamp(0, last)))?;
        Ok((720, 480, rgb))
    }
}

/// Duration of an audio-only media file in seconds. A raw AC-3 elementary stream (which the openers do not
/// read) is measured from its sync frames: 1536 samples each.
pub fn probe_audio_seconds(input: &Path) -> Result<f64, TranscodeError> {
    match source::Source::open_audio(input) {
        Ok(source) => Ok(source.info().duration.0 as f64 / 254_016_000_000.0),
        Err(error) => ac3_seconds(input).ok_or(error),
    }
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

/// Feeds the first `frames` frames of a video file (4:3 letterboxed RGB24 at the DVD frame size and rate) to `f` — motion menu backgrounds.
pub fn for_each_frame(
    input: &Path,
    standard: Standard,
    frames: u64,
    f: &mut dyn FnMut(u64, &[u8]) -> Result<(), String>,
) -> Result<(), TranscodeError> {
    let source = source::Source::open(input)?;
    picture::for_each_rgb_frame(&source, standard, frames, f)
}
