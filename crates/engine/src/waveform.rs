//! Waveform peaks of an audio file for the timeline (ROADMAP M9.10), as the Craft apps keep them (FilmCraft's
//! `source_peaks`): the lowest and highest sample of each 256 samples at 48 kHz, both channels together.

use std::path::Path;

use disccraft_transcode::AudioReader;

/// Samples a peak covers, at [`PEAK_RATE`].
pub const SAMPLES_PER_PEAK: usize = 256;
/// The rate the peaks are taken at.
pub const PEAK_RATE: u32 = 48_000;
/// Most peaks one waveform keeps (16 Mi pairs: over 24 hours of audio); the length comes from the file's header,
/// so it must not size an allocation unchecked.
const MAX_PEAKS: usize = 1 << 24;
/// Peaks reserved up front.
const INITIAL_PEAKS: usize = 1 << 16;

/// The waveform peaks of `path`: (lowest, highest) per [`SAMPLES_PER_PEAK`] samples at [`PEAK_RATE`].
///
/// # Errors
///
/// When the file cannot be opened or has no sound.
pub fn audio_peaks(path: &Path) -> Result<Vec<(f32, f32)>, String> {
    let reader = AudioReader::open(path).map_err(|error| error.to_string())?;
    // `as` saturates; a file of at most hours.
    let total = (reader.duration_seconds().max(0.0) * f64::from(PEAK_RATE)) as usize;
    let total = total.min(MAX_PEAKS.saturating_mul(SAMPLES_PER_PEAK));
    let mut peaks = Vec::with_capacity((total / SAMPLES_PER_PEAK + 1).min(INITIAL_PEAKS));
    let chunk = SAMPLES_PER_PEAK * 750;
    let mut start = 0usize;
    while start < total {
        let frames = chunk.min(total - start);
        let samples = reader
            .read_stereo(u64::try_from(start).unwrap_or(u64::MAX), frames, PEAK_RATE)
            .map_err(|error| error.to_string())?;
        if samples.len() < 2 {
            break;
        }
        // Interleaved stereo: a peak covers SAMPLES_PER_PEAK frames, two samples each.
        for block in samples.chunks(SAMPLES_PER_PEAK * 2) {
            let (lo, hi) = block
                .iter()
                .fold((0f32, 0f32), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
            peaks.push((lo, hi));
        }
        start = start.saturating_add(samples.len() / 2);
        if peaks.len() >= MAX_PEAKS {
            break;
        }
    }
    Ok(peaks)
}

#[cfg(test)]
mod tests {
    use super::{SAMPLES_PER_PEAK, audio_peaks};
    use std::path::PathBuf;

    #[test]
    fn a_tone_has_one_peak_per_256_samples() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("fixtures")
            .join("ntsc.wav");
        if !fixture.is_file() {
            println!("skipped: no fixture");
            return;
        }
        let peaks = audio_peaks(&fixture).unwrap_or_default();
        // About 20 s at 48 kHz.
        let expected = 20.0 * 48_000.0 / SAMPLES_PER_PEAK as f64;
        assert!(
            (peaks.len() as f64 - expected).abs() < expected * 0.05,
            "{} peaks",
            peaks.len()
        );
        // A tone swings both ways, within full scale.
        let top = peaks.iter().fold(0f32, |m, (_, hi)| m.max(*hi));
        let bottom = peaks.iter().fold(0f32, |m, (lo, _)| m.min(*lo));
        assert!((0.01..=1.0).contains(&top), "{top}");
        assert!((-1.0..=-0.01).contains(&bottom), "{bottom}");
    }

    #[test]
    fn a_file_without_sound_has_no_peaks() {
        assert!(audio_peaks(std::path::Path::new("no-such-file.wav")).is_err());
    }
}
