//! SoundCraft 連携: `.scraft` セッションのミックスダウン（音声素材として使う）と、ITU-R BS.1770 のラウドネス測定。
//!
//! 層: L0（docs/architecture.md §5）。
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::collections::VecDeque;
use std::convert::TryFrom;
use std::path::Path;

use serde_json::{Value, json};
use soundcraft_engine::Engine;

/// Errors returned by SoundCraft audio helpers.
#[derive(Debug, thiserror::Error)]
pub enum SoundError {
    /// Filesystem or path-related failure.
    #[error("{path}: {message}")]
    Io {
        /// Path associated with the failure.
        path: String,
        /// Human-readable failure message.
        message: String,
    },
    /// SoundCraft engine failure.
    #[error("SoundCraft: {0}")]
    Engine(String),
    /// Invalid argument or unsupported audio layout.
    #[error("{0}")]
    Invalid(String),
}

/// Result of bouncing a SoundCraft session to WAV.
#[derive(Debug, Clone, Copy)]
pub struct Mixdown {
    /// Duration of the bounced mix in seconds.
    pub seconds: f64,
    /// Integrated loudness in LUFS when measurable.
    pub lufs: Option<f64>,
    /// Peak level in dBFS when measurable.
    pub peak_db: Option<f64>,
}

const SAMPLE_RATE_HZ: u32 = 48_000;
const BLOCK_SAMPLES: usize = 1_920;
const STEP_SAMPLES: usize = 480;
const BLOCK_SAMPLES_F64: f64 = 1_920.0;
const ABSOLUTE_GATE_LUFS: f64 = -70.0;
const RELATIVE_GATE_LU: f64 = -10.0;
const LOUDNESS_OFFSET: f64 = -0.691;
const FULL_SCALE: f64 = 32_768.0;

const PRE_B0: f64 = 1.53512485958697;
const PRE_B1: f64 = -2.69169618940638;
const PRE_B2: f64 = 1.19839281085285;
const PRE_A1: f64 = -1.69065929318241;
const PRE_A2: f64 = 0.73248077421585;

const HIGH_B0: f64 = 1.0;
const HIGH_B1: f64 = -2.0;
const HIGH_B2: f64 = 1.0;
const HIGH_A1: f64 = -1.99004745483398;
const HIGH_A2: f64 = 0.99007225036621;

/// Whether the path is a SoundCraft session (`.scraft`, case-insensitive).
pub fn is_session(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("scraft"))
}

/// Open a SoundCraft session and bounce its main mix to a 16-bit WAV at `wav_out` (session sample rate; callers resample).
#[allow(unused_mut)]
pub fn mixdown(session: &Path, wav_out: &Path) -> Result<Mixdown, SoundError> {
    if !session.exists() {
        return Err(io_err(session, "session not found"));
    }

    let session_path = absolute_path(session)?;
    let out_path = absolute_path(wav_out)?;

    let mut engine = Engine::default();
    engine
        .execute("session.open", &json!({ "path": session_path }))
        .map_err(engine_err)?;

    let reply = engine
        .execute(
            "file.bounce_mix",
            &json!({
                "path": out_path,
                "bit_depth": "16",
                "dither": true,
            }),
        )
        .map_err(engine_err)?;

    if !wav_out.exists() {
        return Err(io_err(wav_out, "bounce output missing"));
    }

    parse_mixdown(&reply)
}

/// Integrated loudness (ITU-R BS.1770-4, LUFS) of interleaved 16-bit big-endian PCM (the DVD/BD LPCM layout) at `sample_rate`.
pub fn integrated_loudness_be16(
    samples: &[u8],
    channels: u8,
    sample_rate: u32,
) -> Result<Option<f64>, SoundError> {
    if sample_rate != SAMPLE_RATE_HZ {
        return Err(invalid("loudness needs 48 kHz"));
    }

    let channels = usize::from(channels);
    if channels == 0 {
        return Err(invalid("channels must be positive"));
    }

    let frame_bytes = channels
        .checked_mul(2)
        .ok_or_else(|| invalid("channels too large"))?;

    if !samples.len().is_multiple_of(frame_bytes) {
        return Err(invalid("sample count must be a multiple of channels"));
    }

    let mut meters = make_meters(channels);
    let z_values = collect_block_z(samples, frame_bytes, &mut meters)?;
    Ok(integrated_from_z(&z_values))
}

/// Scale interleaved 16-bit big-endian PCM in place by `gain_db`, clamping to the 16-bit range; returns the number of clipped samples.
pub fn apply_gain_be16(samples: &mut [u8], gain_db: f64) -> Result<u64, SoundError> {
    if !samples.len().is_multiple_of(2) {
        return Err(invalid("byte count must be even"));
    }
    if !gain_db.is_finite() {
        return Err(invalid("gain_db must be finite"));
    }

    let gain = 10f64.powf(gain_db / 20.0);
    if !gain.is_finite() {
        return Err(invalid("gain overflow"));
    }

    let mut clipped = 0u64;
    for pair in samples.as_chunks_mut::<2>().0 {
        let Some(sample) = i16_from_pair(pair) else {
            return Err(invalid("missing sample bytes"));
        };

        let scaled = f64::from(sample) * gain;
        let rounded = scaled.round();
        if rounded < -32768.0 || rounded > 32767.0 {
            clipped = clipped.saturating_add(1);
        }

        let value = i16_from_rounded(rounded);
        pair.copy_from_slice(&value.to_be_bytes());
    }

    Ok(clipped)
}

/// The gain that brings `measured` LUFS to `target` LUFS, limited so the result stays under `ceiling_db` true... use sample peak:
/// `min(target − measured, ceiling_db − peak_db)`; `None` measured → 0 dB.
pub fn normalization_gain(
    measured: Option<f64>,
    peak_db: f64,
    target: f64,
    ceiling_db: f64,
) -> f64 {
    let Some(measured) = measured else {
        return 0.0;
    };

    if !measured.is_finite() || !target.is_finite() {
        return 0.0;
    }

    let loudness_gain = target - measured;
    let peak_gain = if peak_db.is_finite() && ceiling_db.is_finite() {
        ceiling_db - peak_db
    } else {
        f64::INFINITY
    };

    let gain = loudness_gain.min(peak_gain);
    if gain.is_finite() { gain } else { 0.0 }
}

/// Sample peak in dBFS of interleaved 16-bit big-endian PCM (−inf → `f64::NEG_INFINITY`).
pub fn peak_db_be16(samples: &[u8]) -> f64 {
    let mut peak = 0.0f64;

    for pair in samples.as_chunks::<2>().0 {
        if let Some(sample) = i16_from_pair(pair) {
            let abs = f64::from(sample).abs();
            if abs > peak {
                peak = abs;
            }
        }
    }

    if peak <= 0.0 {
        return f64::NEG_INFINITY;
    }

    20.0 * (peak / FULL_SCALE).log10()
}

#[derive(Clone, Copy)]
struct Biquad {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    fn new() -> Self {
        Self {
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process_pre(&mut self, x: f64) -> f64 {
        let y =
            PRE_B0 * x + PRE_B1 * self.x1 + PRE_B2 * self.x2 - PRE_A1 * self.y1 - PRE_A2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    fn process_high(&mut self, x: f64) -> f64 {
        let y = HIGH_B0 * x + HIGH_B1 * self.x1 + HIGH_B2 * self.x2
            - HIGH_A1 * self.y1
            - HIGH_A2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

struct ChannelMeter {
    pre: Biquad,
    high: Biquad,
    buffer: VecDeque<f64>,
    sum_sq: f64,
    weight: f64,
}

impl ChannelMeter {
    fn new(weight: f64) -> Self {
        Self {
            pre: Biquad::new(),
            high: Biquad::new(),
            buffer: VecDeque::with_capacity(BLOCK_SAMPLES),
            sum_sq: 0.0,
            weight,
        }
    }

    fn process(&mut self, sample: f64) {
        let filtered = self.high.process_high(self.pre.process_pre(sample));
        self.update_buffer(filtered);
    }

    fn update_buffer(&mut self, filtered: f64) {
        if self.buffer.len() == BLOCK_SAMPLES
            && let Some(old) = self.buffer.pop_front()
        {
            self.sum_sq -= old * old;
        }

        self.buffer.push_back(filtered);
        self.sum_sq += filtered * filtered;
    }
}

fn make_meters(channels: usize) -> Vec<ChannelMeter> {
    (0..channels)
        .map(|index| ChannelMeter::new(channel_weight(channels, index)))
        .collect()
}

fn channel_weight(channels: usize, index: usize) -> f64 {
    if channels == 6 && index == 3 {
        return 0.0;
    }
    if index < 3 {
        return 1.0;
    }
    if index < 6 {
        return 1.41;
    }
    1.0
}

fn collect_block_z(
    samples: &[u8],
    frame_bytes: usize,
    meters: &mut [ChannelMeter],
) -> Result<Vec<f64>, SoundError> {
    let mut z_values = Vec::new();
    let mut frames = 0usize;

    for frame in samples.chunks_exact(frame_bytes) {
        process_frame(frame, meters)?;
        frames = frames.saturating_add(1);

        if frames >= BLOCK_SAMPLES
            && frames.is_multiple_of(STEP_SAMPLES)
            && let Some(z) = block_z(meters)
        {
            z_values.push(z);
        }
    }

    Ok(z_values)
}

fn process_frame(frame: &[u8], meters: &mut [ChannelMeter]) -> Result<(), SoundError> {
    let mut pairs = frame.as_chunks::<2>().0.iter();

    for meter in meters.iter_mut() {
        let Some(pair) = pairs.next() else {
            return Err(invalid("missing sample bytes"));
        };
        let Some(sample) = sample_from_pair(pair) else {
            return Err(invalid("missing sample bytes"));
        };
        meter.process(sample);
    }

    Ok(())
}

fn block_z(meters: &[ChannelMeter]) -> Option<f64> {
    if !meters
        .iter()
        .all(|meter| meter.buffer.len() == BLOCK_SAMPLES)
    {
        return None;
    }

    let mut sum = 0.0;
    let mut weight_sum = 0.0;

    for meter in meters {
        if meter.weight == 0.0 {
            continue;
        }

        weight_sum += meter.weight;
        let mean_sq = if meter.sum_sq > 0.0 {
            meter.sum_sq / BLOCK_SAMPLES_F64
        } else {
            0.0
        };
        sum += meter.weight * mean_sq;
    }

    if weight_sum > 0.0 && sum > 0.0 {
        Some(sum / weight_sum)
    } else {
        None
    }
}

fn integrated_from_z(z_values: &[f64]) -> Option<f64> {
    let mean_z_abs = absolute_gated_mean_z(z_values)?;
    let mean_loudness = loudness_from_z(mean_z_abs)?;
    let threshold = mean_loudness + RELATIVE_GATE_LU;
    let mean_z_rel = relative_gated_mean_z(z_values, threshold)?;
    loudness_from_z(mean_z_rel)
}

fn absolute_gated_mean_z(z_values: &[f64]) -> Option<f64> {
    let mut sum = 0.0;
    let mut count = 0usize;

    for &z in z_values {
        if let Some(loudness) = loudness_from_z(z)
            && loudness > ABSOLUTE_GATE_LUFS
        {
            sum += z;
            count = count.saturating_add(1);
        }
    }

    mean_z(sum, count)
}

fn relative_gated_mean_z(z_values: &[f64], threshold: f64) -> Option<f64> {
    let mut sum = 0.0;
    let mut count = 0usize;

    for &z in z_values {
        if let Some(loudness) = loudness_from_z(z)
            && loudness > threshold
        {
            sum += z;
            count = count.saturating_add(1);
        }
    }

    mean_z(sum, count)
}

fn mean_z(sum: f64, count: usize) -> Option<f64> {
    if count == 0 {
        return None;
    }

    let count_f64 = f64::from(u32::try_from(count).ok()?);
    Some(sum / count_f64)
}

fn loudness_from_z(z: f64) -> Option<f64> {
    if z.is_finite() && z > 0.0 {
        Some(LOUDNESS_OFFSET + 10.0 * z.log10())
    } else {
        None
    }
}

fn sample_from_pair(pair: &[u8]) -> Option<f64> {
    Some(f64::from(i16_from_pair(pair)?) / FULL_SCALE)
}

fn i16_from_pair(pair: &[u8]) -> Option<i16> {
    let mut bytes = pair.iter();
    let b0 = bytes.next().copied()?;
    let b1 = bytes.next().copied()?;
    Some(i16::from_be_bytes([b0, b1]))
}

fn i16_from_rounded(rounded: f64) -> i16 {
    let clamped = rounded.clamp(-32768.0, 32767.0);
    i16::try_from(clamped as i32).unwrap_or_default()
}

fn parse_mixdown(reply: &Value) -> Result<Mixdown, SoundError> {
    let seconds =
        optional_finite_f64(reply, "seconds").ok_or_else(|| invalid("missing seconds"))?;
    let lufs = optional_finite_f64(reply, "lufs");
    let peak_db = optional_finite_f64(reply, "peak_db");

    Ok(Mixdown {
        seconds,
        lufs,
        peak_db,
    })
}

fn optional_finite_f64(reply: &Value, key: &str) -> Option<f64> {
    reply
        .get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

fn absolute_path(path: &Path) -> Result<String, SoundError> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|err| io_err(path, &err.to_string()))?
            .join(path)
    };

    Ok(abs.to_string_lossy().to_string())
}

fn io_err(path: &Path, message: &str) -> SoundError {
    SoundError::Io {
        path: path.display().to_string(),
        message: message.to_string(),
    }
}

fn engine_err(err: impl std::fmt::Display) -> SoundError {
    SoundError::Engine(err.to_string())
}

fn invalid(message: &str) -> SoundError {
    SoundError::Invalid(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        SAMPLE_RATE_HZ, SoundError, apply_gain_be16, i16_from_rounded, integrated_loudness_be16,
        invalid, is_session, mixdown, normalization_gain, peak_db_be16,
    };
    use std::convert::TryFrom;
    use std::f64::consts::TAU;
    use std::path::Path;

    fn push_be16(bytes: &mut Vec<u8>, sample: i16) {
        bytes.extend_from_slice(&sample.to_be_bytes());
    }

    fn sine_be16(frames: usize, channels: usize, amplitude: f64) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(frames.saturating_mul(channels.saturating_mul(2)));

        for frame in 0..frames {
            let frame_u32 = u32::try_from(frame).unwrap_or_default();
            let t = f64::from(frame_u32) / 48000.0;
            let value = amplitude * (TAU * 1000.0 * t).sin();
            let sample = i16_from_rounded(value.round());

            for _ in 0..channels {
                push_be16(&mut bytes, sample);
            }
        }

        bytes
    }

    fn lufs_of(bytes: &[u8], channels: u8) -> Result<f64, SoundError> {
        integrated_loudness_be16(bytes, channels, SAMPLE_RATE_HZ)?
            .ok_or_else(|| invalid("expected loudness"))
    }

    #[test]
    fn is_session_checks_extension() {
        assert!(is_session(Path::new("song.Scraft")));
        assert!(!is_session(Path::new("song.wav")));
        assert!(!is_session(Path::new("song")));
    }

    #[test]
    fn full_scale_stereo_sine_is_minus_three_lufs() -> Result<(), SoundError> {
        let bytes = sine_be16(3 * 48000, 2, 32767.0);
        let lufs = lufs_of(&bytes, 2)?;
        assert!((lufs + 3.0).abs() < 0.3);
        Ok(())
    }

    #[test]
    fn quiet_sine_is_twenty_lu_lower() -> Result<(), SoundError> {
        let full = sine_be16(3 * 48000, 2, 32767.0);
        let low = sine_be16(3 * 48000, 2, 32767.0 * 10f64.powf(-20.0 / 20.0));

        let full_lufs = lufs_of(&full, 2)?;
        let low_lufs = lufs_of(&low, 2)?;

        assert!((full_lufs - low_lufs - 20.0).abs() < 0.3);
        Ok(())
    }

    #[test]
    fn silence_has_no_loudness() -> Result<(), SoundError> {
        let bytes = vec![0u8; 48000 * 2 * 2];
        let result = integrated_loudness_be16(&bytes, 2, SAMPLE_RATE_HZ)?;
        assert!(result.is_none());
        Ok(())
    }

    #[test]
    fn non_48khz_is_invalid() {
        let bytes = vec![0u8; 4];
        let result = integrated_loudness_be16(&bytes, 2, 44100);
        assert!(matches!(result, Err(SoundError::Invalid(_))));
    }

    #[test]
    fn apply_gain_doubles_small_and_clips_large() -> Result<(), SoundError> {
        let mut bytes = Vec::new();
        push_be16(&mut bytes, 1000);
        push_be16(&mut bytes, 32767);

        let clipped = apply_gain_be16(&mut bytes, 6.0)?;
        assert_eq!(clipped, 1);

        let first = i16::from_be_bytes([bytes[0], bytes[1]]);
        assert!((f64::from(first) - 2000.0).abs() < 10.0);

        let second = i16::from_be_bytes([bytes[2], bytes[3]]);
        assert_eq!(second, 32767);

        Ok(())
    }

    #[test]
    fn normalization_gain_is_limited_by_ceiling() {
        let limited = normalization_gain(Some(-20.0), -1.0, -14.0, -1.0);
        assert!((limited - 0.0).abs() < 1e-9);

        let loudness_limited = normalization_gain(Some(-20.0), -10.0, -14.0, -1.0);
        assert!((loudness_limited - 6.0).abs() < 1e-9);

        let none = normalization_gain(None, -10.0, -14.0, -1.0);
        assert!((none - 0.0).abs() < 1e-9);
    }

    #[test]
    fn peak_db_full_scale_is_zero() {
        let mut bytes = Vec::new();
        push_be16(&mut bytes, 32767);

        let peak = peak_db_be16(&bytes);
        assert!(peak.abs() < 0.01);
        assert!(peak_db_be16(&[]) == f64::NEG_INFINITY);
    }

    #[test]
    fn mixdown_missing_session_is_io() {
        let result = mixdown(
            Path::new("definitely-missing-soundcraft-session.scraft"),
            Path::new("out.wav"),
        );
        assert!(matches!(result, Err(SoundError::Io { .. })));
    }
}
