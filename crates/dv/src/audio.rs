//! The sound of a DV frame (ITU-R BT.1618-1 Annex 1 §1.6; SMPTE 314M-2005 §4.6): each sample's bytes are shuffled
//! over the audio DIF blocks of half the frame's DIF sequences (§1.6.2.2).
//!
//! - 16 bits linear at 48 kHz (the recommendation), 44.1 kHz and 32 kHz (IEC 61834): channel 1 in the first half of
//!   the DIF sequences, channel 2 in the second, two bytes a sample (most significant first).
//! - 12 bits non-linear at 32 kHz (IEC 61834's four-channel mode, a camcorder's "12 bit"): a stereo pair in each half
//!   (the first half sound 1, the second sound 2, which a camcorder keeps for audio dubbing), three bytes a pair of
//!   samples, placed by the same shuffle; each 12-bit code is widened to 16 bits by the table of IETF
//!   draft-kobayashi-dv-audio12-00.
//!
//! The AAUX source pack says how many samples a frame holds as AF SIZE above a least number. The 48 kHz ones are the
//! recommendation's (1 580 for 525/60, 1 896 for 625/50); for 44.1 and 32 kHz, and for how the three bytes hold two
//! 12-bit samples, no public document was found: they were found by making DV frames in each mode and seeing what
//! ffmpeg, as an outside oracle, reads from them (AGENTS.md rule 9); ffmpeg's own DV files at 44.1 / 32 kHz (625/50)
//! agree.

use crate::{DIF_BLOCK, DvError, SEQUENCE, System};

/// The sound of a frame: one list of samples a channel (two: left and right).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Audio {
    pub sample_rate: u32,
    /// Bits a sample as recorded: 16, or 12 (widened to 16 here).
    pub bits: u32,
    pub channels: Vec<Vec<i16>>,
}

/// The 16-bit audio error code: a sample that was not reproduced.
const ERROR_SAMPLE: i16 = i16::MIN;
/// The 12-bit one.
const ERROR_CODE: u16 = 0x800;

/// The byte offset of audio DIF block `block` (0–8) of DIF sequence `sequence`.
fn audio_block(sequence: usize, block: usize) -> usize {
    sequence * SEQUENCE + (6 + 16 * block) * DIF_BLOCK
}

/// The audio modes decoded: sample rate, and the least samples a frame holds (AF SIZE adds to it), by system.
fn mode(frequency: u8, system: System) -> Option<(u32, usize)> {
    let lines525 = system == System::Lines525;
    match frequency {
        0 => Some((48_000, if lines525 { 1_580 } else { 1_896 })),
        1 => Some((44_100, if lines525 { 1_452 } else { 1_742 })),
        2 => Some((32_000, if lines525 { 1_053 } else { 1_264 })),
        _ => None,
    }
}

/// Decodes the sound of the DV frame `frame` (for 12 bits, sound 1; at 50 Mb/s CH1 and CH2): `None` when it has none,
/// or it is in a mode not decoded.
///
/// # Errors
/// As [`crate::header`], and when the frame says it holds more samples than it can.
pub fn decode_audio(frame: &[u8]) -> Result<Option<Audio>, DvError> {
    decode_audio_pair(frame, 0)
}

/// Decodes stereo pair `pair` of the sound of the DV frame `frame` (M6.418): 0 as [`decode_audio`]; 1 the second
/// pair, where a frame has one: a camcorder's 12-bit sound 2 (the second half of the DIF sequences, kept for audio
/// dubbing), or DVCPRO50's CH3 and CH4 (its second channel). `None` when there is no such pair, or its source pack
/// says its audio is not valid (audio mode 1111).
///
/// # Errors
/// As [`decode_audio`].
pub fn decode_audio_pair(frame: &[u8], pair: usize) -> Result<Option<Audio>, DvError> {
    let header = crate::header(frame)?;
    if frame.len() < header.frame_bytes() {
        return Err(DvError(
            "the frame is shorter than its DIF sequences".to_owned(),
        ));
    }
    let system = header.system;
    // Five DIF sequences a channel (six for 625/50), nine audio blocks each, 72 bytes of samples in a block.
    let sequences = system.sequences();
    let group = sequences / 2;
    // The first DIF sequence of the pair's channel (the second at 50 Mb/s for pair 1), and which half of it holds
    // the pair's 12-bit samples.
    let (base, half) = match (header.channels(), pair) {
        (2, 0 | 1) => (pair * sequences, 0),
        (1, 0 | 1) => (0, pair),
        _ => return Ok(None),
    };
    // The AAUX source pack of the pair (pack 3 of the even DIF sequences, 0 of the odd ones).
    let first = base + half * group;
    let Some(source) = (first..first + group).find_map(|sequence| {
        (0..9).find_map(|block| {
            let at = audio_block(sequence, block) + 3;
            frame.get(at..at + 5).filter(|pack| pack[0] == 0x50)
        })
    }) else {
        return Ok(None);
    };
    if pair > 0 && source[2] & 0x0F == 0x0F {
        return Ok(None);
    }
    let samples_code = usize::from(source[1] & 0x3F);
    let frequency = (source[4] >> 3) & 0x07;
    let quantisation = source[4] & 0x07;
    let Some((sample_rate, least)) = mode(frequency, system) else {
        return Ok(None);
    };
    // Bytes a sample (16 bits) or a pair of samples (12 bits).
    let (bits, unit) = match quantisation {
        0 => (16, 2),
        1 if sample_rate == 32_000 => (12, 3),
        _ => return Ok(None),
    };
    // At 25 Mb/s, 16-bit sound is one pair (its left in the first half, its right in the second).
    if bits == 16 && header.channels() == 1 && pair > 0 {
        return Ok(None);
    }
    let count = least + samples_code;
    let most = 9 * group * (72 / unit);
    if count > most {
        return Err(DvError(format!(
            "the frame says it has {count} audio samples, more than {most}"
        )));
    }
    // Where sample (or pair) `n` of the half `half` of the pair's channel is.
    let place = |n: usize, half: usize| {
        let sequence = base + (n / 3 + 2 * (n % 3)) % group + half * group;
        let block = 3 * (n % 3) + (n % (9 * group)) / (3 * group);
        audio_block(sequence, block) + 8 + unit * (n / (9 * group))
    };
    let mut channels: Vec<Vec<i16>> = (0..2).map(|_| Vec::with_capacity(count)).collect();
    if bits == 16 {
        for (half, samples) in channels.iter_mut().enumerate() {
            let mut last = 0i16;
            for n in 0..count {
                let at = place(n, half);
                let sample = frame
                    .get(at..at + 2)
                    .map_or(ERROR_SAMPLE, |pair| i16::from_be_bytes([pair[0], pair[1]]));
                // A sample not reproduced repeats the one before it.
                if sample != ERROR_SAMPLE {
                    last = sample;
                }
                samples.push(last);
            }
        }
    } else {
        let mut last = [0i16; 2];
        for n in 0..count {
            let at = place(n, half);
            // Left: the first byte and the upper half of the third; right: the second and the lower half.
            let codes = frame.get(at..at + 3).map_or([ERROR_CODE; 2], |b| {
                [
                    (u16::from(b[0]) << 4) | u16::from(b[2] >> 4),
                    (u16::from(b[1]) << 4) | u16::from(b[2] & 0x0F),
                ]
            });
            for ((code, held), samples) in codes.into_iter().zip(&mut last).zip(&mut channels) {
                if code != ERROR_CODE {
                    *held = widen(code);
                }
                samples.push(*held);
            }
        }
    }
    Ok(Some(Audio {
        sample_rate,
        bits,
        channels,
    }))
}

/// A 12-bit non-linear sample (two's complement) as 16 bits linear, by the table of draft-kobayashi-dv-audio12-00:
/// codes within ±512 are the value itself; above, each further 256 codes stand for a range twice as wide as the one
/// before (steps of 2, 4, … 64); below, the same mirrored around −1.
fn widen(code: u16) -> i16 {
    let value = i32::from(code & 0x0FFF);
    let value = if value >= 0x800 {
        value - 0x1000
    } else {
        value
    };
    if (-512..=511).contains(&value) {
        return value as i16;
    }
    // The magnitude on the positive side: v for v > 0, −1 − v for v < 0 (the table is symmetric about −1/2).
    let (magnitude, negative) = if value > 0 {
        (value, false)
    } else {
        (-1 - value, true)
    };
    // Segment 1 (512–767) steps by 2 from 512, segment 2 (768–1023) by 4 from 1 024, … segment 6 by 64 from 16 384.
    let segment = (magnitude - 256) / 256;
    let start = 256 * (segment + 1);
    let widened = (256 << segment) + (magnitude - start) * (1 << segment);
    let widened = if negative { -1 - widened } else { widened };
    widened.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn twelve_bit_codes_widen_by_the_table() {
        // The rows of the draft's table: each segment's ends.
        for (code, value) in [
            (0x000u16, 0i16),
            (0x1FF, 511),
            (0x200, 512),
            (0x2FF, 1_022),
            (0x300, 1_024),
            (0x500, 4_096),
            (0x600, 8_192),
            (0x700, 16_384),
            (0x7FF, 32_704),
            (0xFFF, -1),
            (0xE00, -512),
            (0xDFF, -513),
            (0xD00, -1_023),
            (0xCFF, -1_025),
            (0x801, -32_641),
        ] {
            assert_eq!(widen(code), value, "{code:#x}");
        }
        // Monotonic over all codes but the error code.
        let mut values: Vec<i16> = (0x801..=0xFFFu16).chain(0..=0x7FF).map(widen).collect();
        let sorted = {
            let mut v = values.clone();
            v.sort_unstable();
            v
        };
        assert_eq!(values, sorted);
        values.dedup();
        assert_eq!(values.len(), 4_095);
    }
}
