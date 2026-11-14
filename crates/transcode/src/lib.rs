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
    /// Output frames (at the standard's rate, from 0) where a closed GOP must start: the chapters and the layer
    /// break of the titles that play the video, so their DVD cells begin exactly there. Between them the GOPs keep
    /// their regular length.
    pub gop_starts: Vec<u64>,
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
        wide: is_wide(video.width, video.height, video.par),
        has_audio: source.has_audio(),
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

/// How far apart [`FrameReader::first_bright_time`] looks.
pub const BRIGHT_STEP_SECONDS: f64 = 0.25;
/// How far [`FrameReader::first_bright_time`] looks.
pub const BRIGHT_SEARCH_SECONDS: f64 = 10.0;

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
        Ok(Self {
            source: source::Source::open_audio(input)?,
        })
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

    /// The picture at `seconds` (clamped to the file) without letterbox or pillarbox bars, as RGB24: (width,
    /// height, pixels). A video thumbnail stretches it over its area, as Encore shows the whole picture there.
    pub fn picture(&self, seconds: f64) -> Result<(u32, u32, Vec<u8>), TranscodeError> {
        picture::rgb_picture_at(&self.source, self.tick_at(seconds))
    }

    /// `seconds` as a time inside the file.
    fn tick_at(&self, seconds: f64) -> filmcraft_time::Tick {
        let duration = self.source.info().duration.0;
        let wanted = if seconds.is_finite() && seconds > 0.0 {
            // `as` saturates at the i64 range.
            (seconds * TICKS_PER_SECOND as f64) as i64
        } else {
            0
        };
        let last = duration.saturating_sub(1).max(0);
        filmcraft_time::Tick(wanted.clamp(0, last))
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

#[cfg(test)]
mod tests {
    use super::{is_bright, is_wide};

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
}
