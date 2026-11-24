//! Sound played faster or slower than real time without its pitch changing (M6.427: the Monitor's playback
//! speed), by WSOLA: waveform-similarity overlap-add (W. Verhelst and M. Roelands, "An overlap-add technique
//! based on waveform similarity (WSOLA) for high quality time-scale modification of speech", ICASSP 1993).
//!
//! The output is made of windowed stretches of the input laid half over one another at a fixed step. Each
//! stretch is taken near where the speed puts it in the input, at the offset where its start looks most like
//! the input that followed the stretch before it, so the joins keep the waves in step instead of beating.

/// Interleaved stereo.
const CHANNELS: usize = 2;
/// The length of one stretch, seconds.
const WINDOW_SECONDS: f64 = 0.03;
/// How far from its place a stretch may be taken, seconds.
const TOLERANCE_SECONDS: f64 = 0.01;
/// The search looks at every this many offsets first, then at the ones around the best.
const COARSE: usize = 4;
/// Speeds this close to 1 play as they are.
const UNCHANGED: f64 = 1e-6;

/// The state of one stretched sound: interleaved stereo in, interleaved stereo out at `1 / speed` its length.
#[derive(Debug)]
pub(crate) struct TimeStretch {
    speed: f64,
    /// The window of a stretch (Hann, periodic: halves laid over each other add up to 1).
    window: Vec<f32>,
    /// Half a stretch, in frames: the output step and the overlap.
    half: usize,
    /// How far from its place a stretch may be taken, in frames.
    tolerance: usize,
    /// Input still needed, interleaved; its first frame is input frame `start`.
    input: Vec<f32>,
    start: u64,
    /// The second half of the last stretch, windowed, waiting for the first half of the next.
    tail: Vec<f32>,
    /// Stretches laid so far.
    laid: u64,
    /// The input frame the last stretch was taken from.
    last: Option<u64>,
}

impl TimeStretch {
    /// A stretcher for `speed` (input seconds per output second) at `rate` Hz; `None` when the speed is 1 (the
    /// sound plays as it is) or not a usable number.
    pub(crate) fn new(speed: f64, rate: u32) -> Option<Self> {
        if !speed.is_finite() || speed <= 0.0 || (speed - 1.0).abs() < UNCHANGED || rate == 0 {
            return None;
        }
        // `as` saturates; the rates are at most hundreds of kHz.
        let half = ((WINDOW_SECONDS * f64::from(rate) / 2.0).round() as usize).max(8);
        let tolerance = ((TOLERANCE_SECONDS * f64::from(rate)).round() as usize).max(1);
        let length = half * 2;
        let window = (0..length)
            .map(|index| {
                let phase = std::f64::consts::TAU * index as f64 / length as f64;
                (0.5 - 0.5 * phase.cos()) as f32
            })
            .collect();
        Some(Self {
            speed,
            window,
            half,
            tolerance,
            input: Vec::new(),
            start: 0,
            tail: vec![0.0; half * CHANNELS],
            laid: 0,
            last: None,
        })
    }

    /// Takes in `samples` (interleaved stereo; a dangling half frame is dropped) and returns the output that is
    /// ready.
    pub(crate) fn push(&mut self, samples: &[f32]) -> Vec<f32> {
        let whole = samples.len() / CHANNELS * CHANNELS;
        self.input
            .extend_from_slice(samples.get(..whole).unwrap_or_default());
        let mut out = Vec::new();
        while self.lay(&mut out) {}
        out
    }

    /// The end of the input: what is left of the output.
    pub(crate) fn finish(&mut self) -> Vec<f32> {
        // Enough silence after the input for the stretches that still reach into it.
        let pad = (self.half * 2 + self.tolerance * 2) * CHANNELS;
        let end = self.end();
        self.input.extend(std::iter::repeat_n(0.0, pad));
        let mut out = Vec::new();
        while self.nominal(self.laid) < end && self.lay(&mut out) {}
        out.append(&mut self.tail);
        out
    }

    /// The input frame after the last one taken in.
    fn end(&self) -> u64 {
        self.start
            .saturating_add(u64::try_from(self.input.len() / CHANNELS).unwrap_or(u64::MAX))
    }

    /// Where stretch `index` belongs in the input, in frames.
    fn nominal(&self, index: u64) -> u64 {
        // `as` saturates; the positions are frames of at most hours of sound.
        (index as f64 * self.half as f64 * self.speed).round() as u64
    }

    /// The mono mix of input frames `from..from + count` (all of them taken in and not dropped).
    fn mono(&self, from: u64, count: usize) -> Vec<f32> {
        let offset = usize::try_from(from.saturating_sub(self.start)).unwrap_or(usize::MAX);
        self.input
            .as_chunks::<CHANNELS>()
            .0
            .iter()
            .skip(offset)
            .take(count)
            .map(|frame| frame.iter().sum::<f32>())
            .collect()
    }

    /// Lays the next stretch when the input reaches far enough, adding half a stretch to `out`.
    fn lay(&mut self, out: &mut Vec<f32>) -> bool {
        let length = self.half * 2;
        let place = self.nominal(self.laid);
        let tolerance = self.tolerance as u64;
        let reach = place
            .saturating_add(tolerance)
            .saturating_add(length as u64);
        if self.end() < reach {
            return false;
        }
        let taken = match self.last {
            None => place,
            Some(last) => self.best_offset(last.saturating_add(self.half as u64), place),
        };
        // Overlap-add: the stretch's first half onto the last one's second half.
        let offset = usize::try_from(taken.saturating_sub(self.start)).unwrap_or(usize::MAX);
        let stretch: Vec<f32> = self
            .input
            .as_chunks::<CHANNELS>()
            .0
            .iter()
            .skip(offset)
            .take(length)
            .zip(&self.window)
            .flat_map(|(frame, weight)| frame.iter().map(move |sample| sample * weight))
            .collect();
        if stretch.len() != length * CHANNELS {
            return false;
        }
        let (first, second) = stretch.split_at(self.half * CHANNELS);
        out.extend(self.tail.iter().zip(first).map(|(a, b)| a + b));
        self.tail = second.to_vec();
        self.last = Some(taken);
        self.laid = self.laid.saturating_add(1);
        // Drop the input no later stretch looks at.
        let next = self.nominal(self.laid).saturating_sub(tolerance);
        let keep = next.min(taken.saturating_add(self.half as u64));
        let drop = usize::try_from(keep.saturating_sub(self.start)).unwrap_or(0);
        let drop = drop.min(self.input.len() / CHANNELS);
        self.input.drain(..drop * CHANNELS);
        self.start = self.start.saturating_add(drop as u64);
        true
    }

    /// The input frame near `place` (within the tolerance) whose following half stretch looks most like the half
    /// stretch at `natural` (the input that followed the last stretch): the highest normalized correlation.
    fn best_offset(&self, natural: u64, place: u64) -> u64 {
        let half = self.half;
        let tolerance = self.tolerance as u64;
        let lowest = place.saturating_sub(tolerance).max(self.start);
        let highest = place.saturating_add(tolerance);
        let reference = self.mono(natural, half);
        let span = usize::try_from(highest.saturating_sub(lowest)).unwrap_or(0);
        let candidates = self.mono(lowest, span + half);
        if reference.len() < half || candidates.len() < span + half {
            return place.max(self.start);
        }
        let score = |shift: usize, stride: usize| -> f32 {
            let Some(window) = candidates.get(shift..shift + half) else {
                return f32::MIN;
            };
            let (mut product, mut energy) = (0.0f32, 0.0f32);
            for (a, b) in reference.iter().zip(window).step_by(stride) {
                product += a * b;
                energy += b * b;
            }
            product / (energy + 1e-9).sqrt()
        };
        // The place itself unless another offset is better.
        let at_place = usize::try_from(place.saturating_sub(lowest))
            .unwrap_or(0)
            .min(span);
        let mut best = (at_place, score(at_place, 2));
        for shift in (0..=span).step_by(COARSE) {
            let value = score(shift, 2);
            if value > best.1 {
                best = (shift, value);
            }
        }
        let around = best.0;
        let mut best = (around, score(around, 1));
        for shift in around.saturating_sub(COARSE - 1)..=(around + COARSE - 1).min(span) {
            let value = score(shift, 1);
            if value > best.1 {
                best = (shift, value);
            }
        }
        lowest.saturating_add(best.0 as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::TimeStretch;

    const RATE: u32 = 48_000;

    /// `seconds` of a sine of `frequency` Hz and amplitude `amplitude`, on both channels.
    fn sine(frequency: f64, amplitude: f32, seconds: f64) -> Vec<f32> {
        let frames = (seconds * f64::from(RATE)) as usize;
        (0..frames)
            .flat_map(|index| {
                let value = amplitude
                    * (std::f64::consts::TAU * frequency * index as f64 / f64::from(RATE)).sin()
                        as f32;
                [value, value]
            })
            .collect()
    }

    fn stretched(speed: f64, input: &[f32], chunk: usize) -> Vec<f32> {
        let mut stretch = TimeStretch::new(speed, RATE).unwrap();
        let mut out = Vec::new();
        for part in input.chunks(chunk * 2) {
            out.extend(stretch.push(part));
        }
        out.extend(stretch.finish());
        out
    }

    /// The left channel of the middle half of `out`.
    fn middle(out: &[f32]) -> Vec<f32> {
        let left: Vec<f32> = out
            .as_chunks::<2>()
            .0
            .iter()
            .map(|frame| frame[0])
            .collect();
        left[left.len() / 4..left.len() * 3 / 4].to_vec()
    }

    /// Hz, from the rising zero crossings.
    fn frequency(samples: &[f32]) -> f64 {
        let rising = samples
            .windows(2)
            .filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0)
            .count();
        rising as f64 * f64::from(RATE) / samples.len() as f64
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    #[test]
    fn the_sound_takes_its_time_over_the_speed_and_keeps_its_pitch() {
        let input = sine(440.0, 0.5, 3.0);
        for speed in [2.0, 1.5, 0.5] {
            let out = stretched(speed, &input, 4_800);
            let expected = 3.0 * f64::from(RATE) / speed;
            let frames = (out.len() / 2) as f64;
            // Within a stretch and the tolerance of the length the speed asks for.
            assert!(
                (frames - expected).abs() < 0.05 * f64::from(RATE),
                "{speed}: {frames} frames for {expected}"
            );
            let part = middle(&out);
            let heard = frequency(&part);
            assert!((heard - 440.0).abs() < 440.0 * 0.02, "{speed}: {heard} Hz");
            let level = rms(&part);
            assert!(
                (level - 0.5 / 2.0f32.sqrt()).abs() < 0.06,
                "{speed}: rms {level}"
            );
            assert!(out.iter().all(|sample| sample.is_finite()));
        }
    }

    #[test]
    fn a_steady_level_and_silence_come_out_as_they_went_in() {
        let steady = vec![0.25f32; 2 * 48_000];
        let out = stretched(2.0, &steady, 1_000);
        assert!(
            middle(&out)
                .iter()
                .all(|sample| (sample - 0.25).abs() < 1e-3),
            "steady"
        );
        let silence = vec![0.0f32; 2 * 48_000];
        let out = stretched(0.5, &silence, 1_000);
        assert!(!out.is_empty() && out.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn the_output_does_not_depend_on_how_the_input_is_cut() {
        let input = sine(300.0, 0.3, 1.0);
        assert_eq!(stretched(1.5, &input, 333), stretched(1.5, &input, 48_000));
    }

    #[test]
    fn speed_one_and_nonsense_need_no_stretcher() {
        for speed in [1.0, 0.0, -2.0, f64::NAN, f64::INFINITY] {
            assert!(TimeStretch::new(speed, RATE).is_none(), "{speed}");
        }
        assert!(TimeStretch::new(2.0, 0).is_none());
        // A dangling half frame is dropped, not kept.
        let mut stretch = TimeStretch::new(2.0, RATE).unwrap();
        let _ = stretch.push(&[0.1, 0.2, 0.3]);
        assert_eq!(stretch.input.len() % 2, 0);
    }
}
