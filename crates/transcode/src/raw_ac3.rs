//! A raw Dolby Digital file (`.ac3`, the sound a DVD carries as it is) as something that can be heard and drawn:
//! the media reader opens containers and WAV, not a bare AC-3 stream, so the stream is decoded once to a stereo
//! WAV that is kept with the Craft documents' renders (`crate::session`) and opened in its place for the monitor's
//! sound and the timeline's waveform. The disc still gets the file itself.
//!
//! Decoding is craft-audio's (ATSC A/52), through its public API; the downmix to two channels is A/52 7.8.2's
//! Lo/Ro with the usual -3 dB centre and surround levels.
//!
//! A raw Dolby Digital Plus file (E-AC-3, `.eac3` / `.ec3`, M6.565) goes the same way: craft-audio's decoder reads
//! its independent substream 0 (A/52 Annex E); the frames of dependent substreams (the channels beyond 5.1) and of
//! further programs are left out. A disc never gets it as it is: a DVD cannot carry it, so it is converted to Dolby
//! Digital like the other sound files.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use craft_audio::ac3::{Decoder, parse_header};

use crate::{TranscodeError, io_error};

/// The most bytes of one syncframe: AC-3's is 1920 words at 32 kHz (Table 5.18), E-AC-3's `frmsiz` counts up to
/// 2048 words (Annex E, §E2.3.1.3).
const MAX_FRAME_BYTES: usize = 4096;
/// A WAV file's limit for its sound.
const MAX_WAV_BYTES: u64 = u32::MAX as u64 - 36;

/// Whether `path` is named as a raw AC-3 or E-AC-3 file.
pub(crate) fn is_raw_ac3(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["ac3", "eac3", "ec3"]
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

/// The length in seconds of a raw AC-3 or E-AC-3 file; `None` when it does not start with a syncframe. An AC-3
/// stream has one frame size throughout, so its first frame and its size tell it. An E-AC-3 stream's frames may
/// change size and interleave dependent substreams, so its frame headers are walked and the samples of the frames
/// that are heard counted.
pub(crate) fn seconds(path: &Path) -> Option<f64> {
    let mut file = File::open(path).ok()?;
    let mut head = [0u8; 16];
    file.read_exact(&mut head).ok()?;
    let header = parse_header(&head).ok()?;
    if header.frame_bytes == 0 || header.sample_rate == 0 {
        return None;
    }
    if !header.is_eac3() {
        let frames = file.metadata().ok()?.len() / header.frame_bytes as u64;
        return Some(frames as f64 * header.samples() as f64 / f64::from(header.sample_rate));
    }
    let rate = header.sample_rate;
    let mut reader = BufReader::with_capacity(1 << 16, file);
    reader.seek(SeekFrom::Start(0)).ok()?;
    let mut samples = 0u64;
    let mut at = [0u8; 8];
    // Each step passes at least 8 bytes, so the walk ends with the file.
    while reader.read_exact(&mut at).is_ok() {
        let Ok(header) = parse_header(&at) else {
            break;
        };
        if header.frame_bytes < 8 {
            break;
        }
        if header.is_primary() && header.sample_rate == rate {
            samples += header.samples() as u64;
        }
        let rest = i64::try_from(header.frame_bytes - 8).ok()?;
        if reader.seek_relative(rest).is_err() {
            break;
        }
    }
    Some(samples as f64 / f64::from(rate))
}

/// How much of each decoded channel goes to the left and to the right of the two-channel mix. The channels come
/// in WAV order: the front ones, the LFE (left out of the mix), the surrounds.
fn mix(acmod: u8, lfe: bool) -> Vec<(f32, f32)> {
    const HALF: f32 = std::f32::consts::FRAC_1_SQRT_2;
    let (left, right, centre) = ((1.0, 0.0), (0.0, 1.0), (HALF, HALF));
    let (mut front, surround): (Vec<(f32, f32)>, Vec<(f32, f32)>) = match acmod {
        // Two programmes: one a side.
        0 | 2 => (vec![left, right], Vec::new()),
        1 => (vec![(1.0, 1.0)], Vec::new()),
        3 => (vec![left, right, centre], Vec::new()),
        4 => (vec![left, right], vec![(0.5, 0.5)]),
        5 => (vec![left, right, centre], vec![(0.5, 0.5)]),
        6 => (vec![left, right], vec![(HALF, 0.0), (0.0, HALF)]),
        _ => (vec![left, right, centre], vec![(HALF, 0.0), (0.0, HALF)]),
    };
    if lfe {
        front.push((0.0, 0.0));
    }
    front.extend(surround);
    // More than two channels are turned down together, so that their sum does not clip.
    if acmod >= 3 {
        for gains in &mut front {
            gains.0 *= HALF;
            gains.1 *= HALF;
        }
    }
    front
}

/// Decodes the raw AC-3 file `input` to the 16-bit stereo WAV `out`.
///
/// # Errors
///
/// [`TranscodeError::Decode`] when the file does not start with an AC-3 frame or would pass a WAV's size, and I/O
/// errors. A frame that cannot be decoded is silent; what follows the last whole frame is left out.
pub(crate) fn decode_to_wav(input: &Path, out: &Path) -> Result<(), TranscodeError> {
    let not_ac3 = || TranscodeError::Decode(format!("{}: not an AC-3 stream", input.display()));
    let mut reader =
        BufReader::with_capacity(1 << 20, File::open(input).map_err(|e| io_error(input, e))?);
    let mut writer =
        BufWriter::with_capacity(1 << 20, File::create(out).map_err(|e| io_error(out, e))?);
    // The header is written last, when the length is known.
    writer.write_all(&[0u8; 44]).map_err(|e| io_error(out, e))?;

    let mut decoder = Decoder::new();
    let mut frame = vec![0u8; MAX_FRAME_BYTES];
    let mut sample_rate = 0u32;
    let mut written = 0u64;
    let mut pcm = Vec::new();
    // The start of a frame says how long it is.
    while let Some(head) = frame.get_mut(..8) {
        if reader.read_exact(head).is_err() {
            break;
        }
        let Ok(header) = parse_header(head) else {
            if sample_rate == 0 {
                return Err(not_ac3());
            }
            break;
        };
        let Some(rest) = frame.get_mut(8..header.frame_bytes.clamp(8, MAX_FRAME_BYTES)) else {
            break;
        };
        if header.frame_bytes < 8 || reader.read_exact(rest).is_err() {
            break;
        }
        // An E-AC-3 dependent substream or further program: not heard (see the module's notes).
        if !header.is_primary() {
            continue;
        }
        if sample_rate == 0 {
            sample_rate = header.sample_rate;
        } else if header.sample_rate != sample_rate {
            break;
        }
        let samples = header.samples();
        pcm.clear();
        pcm.resize(samples * 4, 0u8);
        let decoded = frame
            .get(..header.frame_bytes)
            .map(|bytes| decoder.decode(bytes));
        if let Some(Ok(decoded)) = decoded {
            let gains = mix(header.acmod, header.lfeon);
            for (index, out) in pcm.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let (mut left, mut right) = (0.0_f32, 0.0_f32);
                for (channel, (to_left, to_right)) in decoded.channels.iter().zip(&gains) {
                    let sample = channel.get(index).copied().unwrap_or(0.0);
                    left += sample * to_left;
                    right += sample * to_right;
                }
                // `as` saturates.
                let left = ((left.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes();
                let right = ((right.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes();
                *out = [left[0], left[1], right[0], right[1]];
            }
        }
        written += pcm.len() as u64;
        if written > MAX_WAV_BYTES {
            return Err(TranscodeError::Decode(format!(
                "{}: longer than a WAV file holds",
                input.display()
            )));
        }
        writer.write_all(&pcm).map_err(|e| io_error(out, e))?;
    }
    if sample_rate == 0 {
        return Err(not_ac3());
    }
    let mut file = writer
        .into_inner()
        .map_err(|e| io_error(out, e.into_error()))?;
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + written as u32).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&2u16.to_le_bytes());
    header.extend_from_slice(&sample_rate.to_le_bytes());
    header.extend_from_slice(&sample_rate.saturating_mul(4).to_le_bytes());
    header.extend_from_slice(&4u16.to_le_bytes());
    header.extend_from_slice(&16u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&(written as u32).to_le_bytes());
    file.seek(SeekFrom::Start(0))
        .map_err(|e| io_error(out, e))?;
    file.write_all(&header).map_err(|e| io_error(out, e))?;
    file.flush().map_err(|e| io_error(out, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_fold_down_to_two() {
        // Stereo passes; mono goes to both sides.
        assert_eq!(mix(2, false), vec![(1.0, 0.0), (0.0, 1.0)]);
        assert_eq!(mix(1, false), vec![(1.0, 1.0)]);
        // 5.1: L R C LFE Ls Rs. The LFE is left out, and each side stays within full scale.
        let gains = mix(7, true);
        assert_eq!(gains.len(), 6);
        assert_eq!(gains.get(3), Some(&(0.0, 0.0)));
        let left: f32 = gains.iter().map(|gain| gain.0).sum();
        let right: f32 = gains.iter().map(|gain| gain.1).sum();
        assert!(
            (left - right).abs() < 1e-6 && left > 1.0 && left < 1.75,
            "{left}"
        );
        // A mono surround goes to both sides.
        assert_eq!(mix(4, false).len(), 3);
        assert_eq!(mix(4, true).len(), 4);
    }

    #[test]
    fn a_file_that_is_no_ac3_is_refused() {
        let dir = std::env::temp_dir().join(format!("disccraft-rawac3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let junk = dir.join("junk.ac3");
        std::fs::write(&junk, b"this is not dolby digital at all").unwrap();
        assert!(is_raw_ac3(&junk) && !is_raw_ac3(Path::new("a.wav")));
        assert!(is_raw_ac3(Path::new("a.EAC3")) && is_raw_ac3(Path::new("a.ec3")));
        assert!(seconds(&junk).is_none());
        assert!(decode_to_wav(&junk, &dir.join("out.wav")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
