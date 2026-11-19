//! The sound of a transport stream the media reader cannot open (M6.366): an AVCHD `.m2ts` / `.mts` whose
//! interlaced H.264 the reader refuses as a whole, so its sound could not be heard in the monitor, drawn as a
//! waveform or converted. Its first audio stream is taken out once with the stream's own index
//! (`crate::ts_index`) into a file the reader opens: AC-3, E-AC-3, AAC (ADTS) and MPEG audio as they are, Blu-ray
//! LPCM as a WAV (its four-byte headers read with `disccraft_m2tsmux`). The units before the video's first picture
//! are left out, so the sound starts with the picture.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use craft_media::mpegts::{Codec, Kind};

/// The most bytes of sound taken out of one file (about 2 hours of 8-channel 24-bit 48 kHz LPCM).
const MAX_BYTES: u64 = 8 << 30;

/// One extraction at a time, so two callers do not write the same file.
static EXTRACTING: Mutex<()> = Mutex::new(());

/// The sound of the transport stream `path` as a file the media reader opens, taken out the first time it is
/// asked for and kept (in the probe cache's folder, else the temporary one) by the file's path, size and time.
/// `None` for a file that is not a transport stream with a sound this can take out.
pub(crate) fn extracted(path: &Path) -> Option<PathBuf> {
    let (source, file) = crate::ts_index::open(path)?;
    let stream = file.find(Kind::Audio, |codec| {
        matches!(
            codec,
            Codec::Ac3 | Codec::Eac3 | Codec::AacAdts | Codec::MpegAudio | Codec::LpcmBluray
        )
    })?;
    let audio = file.streams.get(stream)?;
    let extension = match audio.codec {
        Codec::Ac3 => "ac3",
        Codec::Eac3 => "eac3",
        Codec::AacAdts => "aac",
        Codec::MpegAudio => "mpa",
        _ => "wav",
    };
    let out = cache_dir()?.join(format!("ts-audio-{:016x}.{extension}", file_hash(path)?));
    let _held = EXTRACTING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if out.is_file() {
        return Some(out);
    }
    // The sound from the video's first picture on.
    let video_start = file
        .find(Kind::Video, |_| true)
        .and_then(|video| file.streams.get(video))
        .and_then(|video| video.units.iter().filter_map(|unit| unit.pts).min());
    let first = video_start.map_or(0, |start| {
        audio
            .units
            .iter()
            .position(|unit| unit.pts.is_none_or(|pts| pts >= start))
            .unwrap_or(audio.units.len())
    });
    let partial = out.with_extension(format!("{extension}.part"));
    let written = (|| -> Option<()> {
        let mut writer = std::io::BufWriter::new(fs::File::create(&partial).ok()?);
        let mut total = 0u64;
        let mut wav: Option<WavOut> = None;
        for unit in first..audio.units.len() {
            let bytes = file.read_unit(&source, stream, unit).ok()?;
            total = total.saturating_add(u64::try_from(bytes.len()).ok()?);
            if total > MAX_BYTES {
                return None;
            }
            if audio.codec == Codec::LpcmBluray {
                let format = disccraft_m2tsmux::read_bd_lpcm_header(&bytes)?;
                let out = wav.get_or_insert_with(|| WavOut::new(format.clone()));
                if out.format != format {
                    return None;
                }
                out.push(bytes.get(4..)?);
            } else {
                writer.write_all(&bytes).ok()?;
            }
        }
        if let Some(wav) = wav {
            writer.write_all(&wav.finish()?).ok()?;
        }
        writer.flush().ok()
    })();
    if written.is_none() || fs::rename(&partial, &out).is_err() {
        let _ = fs::remove_file(&partial);
        return None;
    }
    Some(out)
}

/// Where taken-out sound is kept: the probe cache's folder, else a folder of the temporary one.
fn cache_dir() -> Option<PathBuf> {
    let dir = crate::media_cache::probe_cache_dir()
        .unwrap_or_else(|| std::env::temp_dir().join("disccraft-ts-audio"));
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// A name for the file `path` as it is now (its path, size and time).
fn file_hash(path: &Path) -> Option<u64> {
    let metadata = fs::metadata(path).ok()?;
    let mut hasher = DefaultHasher::new();
    std::path::absolute(path).ok()?.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    metadata.modified().ok().hash(&mut hasher);
    Some(hasher.finish())
}

/// Blu-ray LPCM samples (big-endian, the stream's channel order, an even count stored) as a WAV's (little-endian,
/// the WAV's order, the channels the sound has).
struct WavOut {
    format: disccraft_m2tsmux::BdLpcmFormat,
    data: Vec<u8>,
}

impl WavOut {
    fn new(format: disccraft_m2tsmux::BdLpcmFormat) -> Self {
        Self {
            format,
            data: Vec::new(),
        }
    }

    fn push(&mut self, samples: &[u8]) {
        let size = usize::from(self.format.sample_bytes);
        let channels = usize::from(self.format.channels);
        let frame = usize::from(self.format.stored) * size;
        if frame == 0 {
            return;
        }
        for stored in samples.chunks_exact(frame) {
            let mut out = vec![0u8; channels * size];
            for (channel, wav) in self.format.wav_order.iter().enumerate() {
                let (Some(sample), Some(slot)) = (
                    stored.get(channel * size..(channel + 1) * size),
                    out.get_mut(wav * size..(wav + 1) * size),
                ) else {
                    continue;
                };
                // Big-endian to little-endian.
                for (to, from) in slot.iter_mut().zip(sample.iter().rev()) {
                    *to = *from;
                }
            }
            self.data.extend_from_slice(&out);
        }
    }

    /// The whole WAV: a PCM header (an extensible one naming the speakers past two channels), then the samples.
    fn finish(self) -> Option<Vec<u8>> {
        let format = &self.format;
        let size = u16::from(format.sample_bytes);
        let channels = u16::from(format.channels);
        let block = channels.checked_mul(size)?;
        let rate = format.rate;
        let byte_rate = rate.checked_mul(u32::from(block))?;
        let bits = size * 8;
        let extensible = channels > 2;
        let fmt_len: u32 = if extensible { 40 } else { 16 };
        let data_len = u32::try_from(self.data.len()).ok()?;
        let mut out = Vec::with_capacity(self.data.len() + 68);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(4 + 8 + fmt_len + 8 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&fmt_len.to_le_bytes());
        out.extend_from_slice(&(if extensible { 0xFFFE_u16 } else { 1 }).to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&byte_rate.to_le_bytes());
        out.extend_from_slice(&block.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        if extensible {
            out.extend_from_slice(&22u16.to_le_bytes());
            out.extend_from_slice(&u16::from(format.bits).to_le_bytes());
            out.extend_from_slice(&format.mask.to_le_bytes());
            // KSDATAFORMAT_SUBTYPE_PCM.
            out.extend_from_slice(&[
                0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38,
                0x9B, 0x71,
            ]);
        }
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.extend_from_slice(&self.data);
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::WavOut;

    /// Stereo 16-bit LPCM: each sample's bytes turned around, the frames as they were.
    #[test]
    fn lpcm_samples_become_a_wav() -> Result<(), Box<dyn std::error::Error>> {
        let format =
            disccraft_m2tsmux::read_bd_lpcm_header(&[0, 8, 0x31, 0x40]).ok_or("no header")?;
        let mut wav = WavOut::new(format);
        wav.push(&[0x12, 0x34, 0x56, 0x78, 0xAB, 0xCD, 0xEF, 0x01]);
        let bytes = wav.finish().ok_or("no wav")?;
        assert_eq!(bytes.get(..4), Some(&b"RIFF"[..]));
        assert_eq!(bytes.get(22..24), Some(&[2u8, 0][..]));
        assert_eq!(bytes.get(24..28), Some(&48_000u32.to_le_bytes()[..]));
        assert_eq!(
            bytes.get(44..),
            Some(&[0x34, 0x12, 0x78, 0x56, 0xCD, 0xAB, 0x01, 0xEF][..])
        );
        Ok(())
    }
}
