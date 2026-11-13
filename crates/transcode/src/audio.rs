use std::path::{Path, PathBuf};

use filmcraft_frame::AudioBuffer;

use crate::source::Source;
use crate::wav::{MAX_DATA_BYTES, WavWriter};
use crate::{Standard, TranscodeError};

pub(crate) const SAMPLE_RATE: u32 = 48_000;
const CHUNK_FRAMES: usize = 48_000;
const CENTER_WEIGHT: f32 = 0.707;
const CENTER_SCALE: f32 = 1.0 / 1.707;

/// Extract and downmix source audio to a 48 kHz stereo WAV file.
pub(crate) fn extract_audio(
    source: &Source,
    frames: u64,
    standard: Standard,
    output_path: &Path,
) -> Result<Option<PathBuf>, TranscodeError> {
    if !source.has_audio() {
        return Ok(None);
    }

    let target_samples = target_sample_frames(frames, standard)
        .ok_or_else(|| TranscodeError::Unsupported("audio duration overflow".to_string()))?;

    if target_samples == 0 {
        return Ok(None);
    }

    write_audio_samples(source, target_samples, output_path)?;
    Ok(Some(output_path.to_path_buf()))
}

/// Extract and downmix exactly `sample_frames` audio frames to a WAV file.
pub(crate) fn extract_audio_samples(
    source: &Source,
    sample_frames: u64,
    output_path: &Path,
) -> Result<u64, TranscodeError> {
    if !source.has_audio() {
        return Err(TranscodeError::Unsupported(
            "input has no audio stream".to_string(),
        ));
    }

    write_audio_samples(source, sample_frames, output_path)
}

/// Convert a duration in seconds to 48 kHz sample frames, rounding up.
pub(crate) fn sample_frames_from_seconds(seconds: f64) -> Option<u64> {
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }

    let max_samples = MAX_DATA_BYTES / 4;
    let max_seconds = max_samples as f64 / f64::from(SAMPLE_RATE);

    if seconds > max_seconds {
        return None;
    }

    let scaled = seconds * f64::from(SAMPLE_RATE);
    if scaled <= 0.0 {
        return Some(0);
    }

    let rounded = scaled.ceil();
    if rounded > max_samples as f64 {
        return None;
    }

    // 0 ≤ rounded ≤ max_samples here, so the cast is exact.
    Some(rounded as u64)
}

/// Convert a FilmCraft tick duration to 48 kHz sample frames, rounding up.
pub(crate) fn sample_frames_from_ticks(ticks: i64) -> Option<u64> {
    if ticks <= 0 {
        return Some(0);
    }

    let ticks = u64::try_from(ticks).ok()?;
    let rate = u64::from(SAMPLE_RATE);
    let denominator = u64::try_from(crate::TICKS_PER_SECOND).ok()?;
    let numerator = ticks.checked_mul(rate)?;

    let whole = numerator / denominator;
    let remainder = numerator % denominator;
    let samples = if remainder == 0 {
        whole
    } else {
        whole.checked_add(1)?
    };

    let max_samples = MAX_DATA_BYTES / 4;
    if samples > max_samples {
        return None;
    }

    Some(samples)
}

fn write_audio_samples(
    source: &Source,
    sample_frames: u64,
    output_path: &Path,
) -> Result<u64, TranscodeError> {
    let data_bytes = sample_frames
        .checked_mul(4)
        .ok_or_else(|| TranscodeError::Unsupported("audio size overflow".to_string()))?;

    if data_bytes > MAX_DATA_BYTES {
        return Err(TranscodeError::Unsupported(
            "audio output exceeds 4 GiB".to_string(),
        ));
    }

    let mut writer = WavWriter::new(output_path)?;
    let mut produced: u64 = 0;

    while produced < sample_frames {
        let remaining = sample_frames
            .checked_sub(produced)
            .ok_or_else(|| TranscodeError::Unsupported("audio remaining overflow".to_string()))?;
        let request = usize::try_from(remaining.min(CHUNK_FRAMES as u64))
            .map_err(|_| TranscodeError::Unsupported("audio chunk too large".to_string()))?;
        let start = i64::try_from(produced)
            .map_err(|_| TranscodeError::Unsupported("audio start too large".to_string()))?;

        let buffer = source.audio(start, request, SAMPLE_RATE)?;
        let written = write_audio_buffer(&mut writer, &buffer, remaining)?;

        if written == 0 {
            write_silence(&mut writer, remaining)?;
            produced = sample_frames;
            break;
        }

        produced = produced
            .checked_add(written)
            .ok_or_else(|| TranscodeError::Unsupported("audio produced overflow".to_string()))?;
    }

    writer.finish()?;
    Ok(produced)
}

/// Writes exactly `sample_frames` frames made of `sources` played one after another (a slideshow's audio clips);
/// with `looped` the sequence starts over until the length is filled, otherwise silence pads the rest.
pub(crate) fn write_audio_sequence(
    sources: &[Source],
    sample_frames: u64,
    looped: bool,
    output_path: &Path,
) -> Result<u64, TranscodeError> {
    let data_bytes = sample_frames
        .checked_mul(4)
        .ok_or_else(|| TranscodeError::Unsupported("audio size overflow".to_string()))?;
    if data_bytes > MAX_DATA_BYTES {
        return Err(TranscodeError::Unsupported(
            "audio output exceeds 4 GiB".to_string(),
        ));
    }
    let mut writer = WavWriter::new(output_path)?;
    let mut produced: u64 = 0;
    let clips: Vec<&Source> = sources.iter().filter(|source| source.has_audio()).collect();

    'passes: while produced < sample_frames && !clips.is_empty() {
        let before_pass = produced;
        for source in &clips {
            let mut position: u64 = 0;
            loop {
                let remaining = sample_frames.saturating_sub(produced);
                if remaining == 0 {
                    break 'passes;
                }
                let request =
                    usize::try_from(remaining.min(CHUNK_FRAMES as u64)).map_err(|_| {
                        TranscodeError::Unsupported("audio chunk too large".to_string())
                    })?;
                let start = i64::try_from(position).map_err(|_| {
                    TranscodeError::Unsupported("audio start too large".to_string())
                })?;
                let buffer = source.audio(start, request, SAMPLE_RATE)?;
                let written = write_audio_buffer(&mut writer, &buffer, remaining)?;
                if written == 0 {
                    // The end of this clip.
                    break;
                }
                position = position.saturating_add(written);
                produced = produced.saturating_add(written);
            }
        }
        // Not looping, or a pass that produced nothing (empty clips): stop here.
        if !looped || produced == before_pass {
            break;
        }
    }
    if produced < sample_frames {
        write_silence(&mut writer, sample_frames.saturating_sub(produced))?;
        produced = sample_frames;
    }
    writer.finish()?;
    Ok(produced)
}

fn target_sample_frames(frames: u64, standard: Standard) -> Option<u64> {
    let (num, den) = if standard == Standard::Ntsc {
        (48_000u64.checked_mul(1_001)?, 30_000u64)
    } else {
        (48_000u64, 25u64)
    };

    let numerator = frames.checked_mul(num)?;
    let whole = numerator / den;
    let remainder = numerator % den;

    if remainder == 0 {
        Some(whole)
    } else {
        whole.checked_add(1)
    }
}

fn write_audio_buffer(
    writer: &mut WavWriter,
    buffer: &AudioBuffer,
    limit: u64,
) -> Result<u64, TranscodeError> {
    let available = u64::try_from(buffer.frames())
        .map_err(|_| TranscodeError::Unsupported("audio frame count too large".to_string()))?;
    let count = available.min(limit);

    if count == 0 || buffer.channels.is_empty() {
        return Ok(0);
    }

    let count_usize = usize::try_from(count)
        .map_err(|_| TranscodeError::Unsupported("audio chunk too large".to_string()))?;
    let capacity = count_usize
        .checked_mul(2)
        .ok_or_else(|| TranscodeError::Unsupported("audio sample count overflow".to_string()))?;

    let mut samples = Vec::with_capacity(capacity);
    for frame in 0..count_usize {
        let [left, right] = stereo_at(buffer, frame);
        samples.push(sample_to_i16(left));
        samples.push(sample_to_i16(right));
    }

    writer.write_samples(&samples)?;
    Ok(count)
}

fn write_silence(writer: &mut WavWriter, frames: u64) -> Result<(), TranscodeError> {
    let mut remaining = frames;

    while remaining > 0 {
        let chunk = usize::try_from(remaining.min(CHUNK_FRAMES as u64))
            .map_err(|_| TranscodeError::Unsupported("silence chunk too large".to_string()))?;
        let sample_count = chunk.checked_mul(2).ok_or_else(|| {
            TranscodeError::Unsupported("silence sample count overflow".to_string())
        })?;

        let samples = vec![0i16; sample_count];
        writer.write_samples(&samples)?;

        let consumed = u64::try_from(chunk)
            .map_err(|_| TranscodeError::Unsupported("silence chunk too large".to_string()))?;
        remaining = remaining.checked_sub(consumed).ok_or_else(|| {
            TranscodeError::Unsupported("silence remaining underflow".to_string())
        })?;
    }

    Ok(())
}

/// The buffer as interleaved stereo (left, right), downmixed as the DVD audio is.
pub(crate) fn interleaved_stereo(buffer: &AudioBuffer) -> Vec<f32> {
    if buffer.channels.is_empty() {
        return Vec::new();
    }
    let frames = buffer.frames();
    let mut samples = Vec::with_capacity(frames.saturating_mul(2));
    for frame in 0..frames {
        let [left, right] = stereo_at(buffer, frame);
        samples.push(left);
        samples.push(right);
    }
    samples
}

fn stereo_at(buffer: &AudioBuffer, frame: usize) -> [f32; 2] {
    match buffer.channels.len() {
        0 => [0.0, 0.0],
        1 => {
            let value = channel(buffer, 0, frame);
            [value, value]
        }
        2 => [channel(buffer, 0, frame), channel(buffer, 1, frame)],
        _ => {
            let left = channel(buffer, 0, frame) + CENTER_WEIGHT * channel(buffer, 2, frame);
            let right = channel(buffer, 1, frame) + CENTER_WEIGHT * channel(buffer, 2, frame);
            [left * CENTER_SCALE, right * CENTER_SCALE]
        }
    }
}

fn channel(buffer: &AudioBuffer, index: usize, frame: usize) -> f32 {
    buffer
        .channels
        .get(index)
        .and_then(|data| data.get(frame).copied())
        .unwrap_or(0.0)
}

fn sample_to_i16(value: f32) -> i16 {
    if !value.is_finite() {
        return 0;
    }

    let clamped = value.clamp(-1.0, 1.0);
    let rounded = (clamped * 32767.0).round();

    if rounded >= 32767.0 {
        32767
    } else if rounded <= -32767.0 {
        -32767
    } else {
        rounded as i16
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{sample_to_i16, stereo_at};
    use filmcraft_frame::AudioBuffer;

    #[test]
    fn mono_duplicates() -> Result<(), String> {
        let buffer = AudioBuffer {
            sample_rate: 48_000,
            channels: vec![vec![0.5, -0.25]],
        };

        let [left, right] = stereo_at(&buffer, 0);
        assert!((left - 0.5).abs() < 1e-6);
        assert!((right - 0.5).abs() < 1e-6);

        let [left, right] = stereo_at(&buffer, 1);
        assert!((left + 0.25).abs() < 1e-6);
        assert!((right + 0.25).abs() < 1e-6);
        Ok(())
    }

    #[test]
    fn stereo_passthrough() -> Result<(), String> {
        let buffer = AudioBuffer {
            sample_rate: 48_000,
            channels: vec![vec![0.25], vec![-0.75]],
        };

        let [left, right] = stereo_at(&buffer, 0);
        assert!((left - 0.25).abs() < 1e-6);
        assert!((right + 0.75).abs() < 1e-6);
        Ok(())
    }

    #[test]
    fn center_downmix_scales() -> Result<(), String> {
        let buffer = AudioBuffer {
            sample_rate: 48_000,
            channels: vec![vec![1.0], vec![0.0], vec![1.0]],
        };

        let [left, right] = stereo_at(&buffer, 0);
        assert!((left - 1.0).abs() < 1e-3);
        assert!((right - 0.414).abs() < 1e-3);
        Ok(())
    }

    #[test]
    fn extra_channels_ignore_surround() -> Result<(), String> {
        let buffer = AudioBuffer {
            sample_rate: 48_000,
            channels: vec![vec![1.0], vec![1.0], vec![1.0], vec![1.0]],
        };

        let [left, right] = stereo_at(&buffer, 0);
        assert!((left - 1.0).abs() < 1e-3);
        assert!((right - 1.0).abs() < 1e-3);
        Ok(())
    }

    #[test]
    fn sample_clamping() {
        assert_eq!(sample_to_i16(1.0), 32767);
        assert_eq!(sample_to_i16(2.0), 32767);
        assert_eq!(sample_to_i16(-1.0), -32767);
        assert_eq!(sample_to_i16(-2.0), -32767);
        assert_eq!(sample_to_i16(f32::NAN), 0);
        assert_eq!(sample_to_i16(f32::INFINITY), 0);
    }
}
