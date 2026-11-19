use super::EsError;
use std::convert::TryInto;

/// DVD LPCM stream parsed from a WAV file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LpcmStream {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Number of channels.
    pub channels: u8,
    /// Bits per sample.
    pub bits: u8,
    /// The samples as a DVD's LPCM stores them: 16-bit samples big-endian and interleaved; 24-bit samples in
    /// groups of two samples a channel, the 16 high bits of each sample first, then their low bytes.
    pub data: Vec<u8>,
    /// Bytes per 1/600 s LPCM frame.
    pub frame_bytes: usize,
    /// The speakers of the channels (a `WAVE_FORMAT_EXTENSIBLE` file's `dwChannelMask`); 0 when the file names
    /// none.
    pub channel_mask: u32,
}

const RIFF: &[u8] = b"RIFF";
const WAVE: &[u8] = b"WAVE";
const FMT: &[u8] = b"fmt ";
const DATA: &[u8] = b"data";
const PCM: u16 = 1;
const EXTENSIBLE: u16 = 0xFFFE;
const PCM_SUBFORMAT: &[u8] = &[
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
];
/// LPCM frames a second: a frame is 1/600 s (80 samples at 48 kHz, 160 at 96 kHz).
const LPCM_FRAMES_PER_SECOND: u32 = 600;
const LPCM_FRAME_PTS: i64 = 45_000;
/// The most a DVD's LPCM stream carries, bits a second.
const LPCM_MAX_BITRATE: u64 = 6_144_000;
/// The most a Blu-ray's LPCM stream carries, bits a second (six channels of 24 bits at 192 kHz), counting the
/// channels as stored (an odd count takes one more, silent).
pub const BD_LPCM_MAX_BITRATE: u64 = 27_648_000;

/// The disc whose LPCM a WAV file is read for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disc {
    Dvd,
    BluRay,
}

struct FmtInfo {
    channels: u8,
    sample_rate: u32,
    bits: u8,
    channel_mask: u32,
}

struct DataChunk {
    start: usize,
    len: usize,
}

/// Parse a WAV file into DVD LPCM data: 16, 20 or 24 bits (20 carried as 24), 48 or 96 kHz, up to 8 channels
/// within the stream's 6.144 Mbit/s.
pub fn parse_wav(data: &[u8]) -> Result<LpcmStream, EsError> {
    parse_wav_for(data, Disc::Dvd)
}

/// Parse a WAV file into LPCM data for a Blu-ray (M6.347): as [`parse_wav`], also at 192 kHz, within the stream's
/// 27.648 Mbit/s ([`BD_LPCM_MAX_BITRATE`]).
pub fn parse_wav_for_bd(data: &[u8]) -> Result<LpcmStream, EsError> {
    parse_wav_for(data, Disc::BluRay)
}

fn parse_wav_for(data: &[u8], disc: Disc) -> Result<LpcmStream, EsError> {
    let limit = parse_riff_header(data)?;
    let (fmt, chunk) = parse_chunks(data, limit, disc)?;
    let fmt = fmt.ok_or_else(|| EsError("missing fmt chunk".to_string()))?;
    let chunk = chunk.ok_or_else(|| EsError("missing data chunk".to_string()))?;
    let samples = convert_samples(data, &chunk, &fmt)?;
    let frame_bytes = frame_bytes(&fmt)?;
    Ok(LpcmStream {
        sample_rate: fmt.sample_rate,
        channels: fmt.channels,
        bits: fmt.bits,
        data: samples,
        frame_bytes,
        channel_mask: fmt.channel_mask,
    })
}

/// Compute the 27 MHz PTS for an LPCM frame.
pub fn lpcm_frame_pts(start_pts: i64, frame_index: i64) -> i64 {
    start_pts.saturating_add(frame_index.saturating_mul(LPCM_FRAME_PTS))
}

fn parse_riff_header(data: &[u8]) -> Result<usize, EsError> {
    let header = data
        .get(0..12)
        .ok_or_else(|| EsError("WAV shorter than RIFF header".to_string()))?;
    if header.get(0..4) != Some(RIFF) || header.get(8..12) != Some(WAVE) {
        return Err(EsError("not a RIFF/WAVE file".to_string()));
    }
    let riff_size = read_u32_le(data, 4).ok_or_else(|| EsError("missing RIFF size".to_string()))?;
    let total = (riff_size as u64)
        .checked_add(8)
        .ok_or_else(|| EsError("RIFF size overflow".to_string()))?;
    if total > data.len() as u64 {
        return Err(EsError("RIFF size exceeds input".to_string()));
    }
    let limit = total as usize;
    if limit < 12 {
        return Err(EsError("RIFF size too small".to_string()));
    }
    Ok(limit)
}

fn parse_chunks(
    data: &[u8],
    limit: usize,
    disc: Disc,
) -> Result<(Option<FmtInfo>, Option<DataChunk>), EsError> {
    let mut fmt = None;
    let mut chunk = None;
    let mut offset = 12usize;
    while offset < limit {
        let id_end = offset
            .checked_add(4)
            .ok_or_else(|| EsError("chunk offset overflow".to_string()))?;
        let body_start = id_end
            .checked_add(4)
            .ok_or_else(|| EsError("chunk offset overflow".to_string()))?;
        if body_start > limit {
            return Err(EsError("truncated chunk header".to_string()));
        }
        let id = data
            .get(offset..id_end)
            .ok_or_else(|| EsError("missing chunk id".to_string()))?;
        let size =
            read_u32_le(data, id_end).ok_or_else(|| EsError("missing chunk size".to_string()))?;
        let body_end = body_start
            .checked_add(size as usize)
            .ok_or_else(|| EsError("chunk size overflow".to_string()))?;
        if body_end > limit {
            return Err(EsError("chunk extends beyond RIFF size".to_string()));
        }
        let body = data
            .get(body_start..body_end)
            .ok_or_else(|| EsError("missing chunk body".to_string()))?;
        if id == FMT && fmt.is_none() {
            fmt = Some(parse_fmt(body, disc)?);
        } else if id == DATA && chunk.is_none() {
            chunk = Some(DataChunk {
                start: body_start,
                len: size as usize,
            });
        }
        offset = body_end;
        if size % 2 == 1 && offset < limit {
            offset += 1;
        }
    }
    Ok((fmt, chunk))
}

fn parse_fmt(body: &[u8], disc: Disc) -> Result<FmtInfo, EsError> {
    let tag = read_u16_le(body, 0).ok_or_else(|| EsError("missing format tag".to_string()))?;
    let channels =
        read_u16_le(body, 2).ok_or_else(|| EsError("missing channel count".to_string()))?;
    let sample_rate =
        read_u32_le(body, 4).ok_or_else(|| EsError("missing sample rate".to_string()))?;
    let block_align =
        read_u16_le(body, 12).ok_or_else(|| EsError("missing block align".to_string()))?;
    let bits =
        read_u16_le(body, 14).ok_or_else(|| EsError("missing bits per sample".to_string()))?;

    let mut channel_mask = 0;
    if tag == EXTENSIBLE {
        let sub = body
            .get(24..40)
            .ok_or_else(|| EsError("missing extensible subformat".to_string()))?;
        if sub != PCM_SUBFORMAT {
            return Err(EsError("extensible subformat is not PCM".to_string()));
        }
        channel_mask = read_u32_le(body, 20).unwrap_or(0);
    } else if tag != PCM {
        return Err(EsError("unsupported WAV format tag".to_string()));
    }

    if channels == 0 || channels > 8 {
        return Err(EsError("channel count must be 1..=8".to_string()));
    }
    // 20-bit samples fill the top of three bytes: they are carried as 24-bit ones (M6.347).
    let bits = if bits == 20 { 24 } else { bits };
    if bits != 16 && bits != 24 {
        return Err(EsError("bits per sample must be 16, 20 or 24".to_string()));
    }
    let rates: &[u32] = match disc {
        Disc::Dvd => &[48_000, 96_000],
        Disc::BluRay => &[48_000, 96_000, 192_000],
    };
    if !rates.contains(&sample_rate) {
        return Err(EsError(match disc {
            Disc::Dvd => "sample rate must be 48000 or 96000 Hz".to_string(),
            Disc::BluRay => "sample rate must be 48000, 96000 or 192000 Hz".to_string(),
        }));
    }
    // Rate, word length and channels together stay within what the stream may carry (on a DVD 96 kHz 24-bit is
    // stereo; a Blu-ray stores an odd count of channels with one more).
    let (stored, max) = match disc {
        Disc::Dvd => (channels, LPCM_MAX_BITRATE),
        Disc::BluRay => (channels.max(2).next_multiple_of(2), BD_LPCM_MAX_BITRATE),
    };
    if u64::from(sample_rate) * u64::from(bits) * u64::from(stored) > max {
        return Err(EsError(
            "too many channels for this sample rate and word length".to_string(),
        ));
    }
    let expected_block = channels
        .checked_mul(bits / 8)
        .ok_or_else(|| EsError("block align overflow".to_string()))?;
    if block_align != expected_block {
        return Err(EsError("block align mismatch".to_string()));
    }
    let channels =
        u8::try_from(channels).map_err(|_| EsError("channel count out of range".to_string()))?;
    let bits = u8::try_from(bits).map_err(|_| EsError("bits out of range".to_string()))?;
    Ok(FmtInfo {
        channels,
        sample_rate,
        bits,
        channel_mask,
    })
}

fn convert_samples(data: &[u8], chunk: &DataChunk, fmt: &FmtInfo) -> Result<Vec<u8>, EsError> {
    let channels = fmt.channels;
    if fmt.bits == 24 {
        return convert_samples_24(data, chunk, channels);
    }
    let end = chunk
        .start
        .checked_add(chunk.len)
        .ok_or_else(|| EsError("data chunk overflow".to_string()))?;
    let raw = data
        .get(chunk.start..end)
        .ok_or_else(|| EsError("missing data chunk body".to_string()))?;
    let frame = usize::from(channels)
        .checked_mul(2)
        .ok_or_else(|| EsError("frame size overflow".to_string()))?;
    let usable = raw.len() - (raw.len() % frame);
    let raw = raw
        .get(..usable)
        .ok_or_else(|| EsError("data length mismatch".to_string()))?;
    let mut out = Vec::with_capacity(usable);
    for pair in raw.as_chunks::<2>().0 {
        let lo = pair
            .first()
            .ok_or_else(|| EsError("missing sample byte".to_string()))?;
        let hi = pair
            .get(1)
            .ok_or_else(|| EsError("missing sample byte".to_string()))?;
        out.push(*hi);
        out.push(*lo);
    }
    Ok(out)
}

/// 24-bit samples (little-endian in the WAV) in the DVD's order: for every two samples a channel (that is,
/// `2 * channels` samples in the order they play), the two high bytes of each sample, then the low byte of each.
/// A last single sample frame that fills no group is left out.
fn convert_samples_24(data: &[u8], chunk: &DataChunk, channels: u8) -> Result<Vec<u8>, EsError> {
    let end = chunk
        .start
        .checked_add(chunk.len)
        .ok_or_else(|| EsError("data chunk overflow".to_string()))?;
    let raw = data
        .get(chunk.start..end)
        .ok_or_else(|| EsError("missing data chunk body".to_string()))?;
    let group = usize::from(channels)
        .checked_mul(2 * 3)
        .filter(|group| *group > 0)
        .ok_or_else(|| EsError("frame size overflow".to_string()))?;
    let usable = raw.len() - (raw.len() % group);
    let mut out = Vec::with_capacity(usable);
    for samples in raw.get(..usable).unwrap_or_default().chunks_exact(group) {
        let (samples, _) = samples.as_chunks::<3>();
        for [_, mid, high] in samples {
            out.push(*high);
            out.push(*mid);
        }
        for [low, _, _] in samples {
            out.push(*low);
        }
    }
    Ok(out)
}

fn frame_bytes(fmt: &FmtInfo) -> Result<usize, EsError> {
    usize::try_from(fmt.sample_rate / LPCM_FRAMES_PER_SECOND)
        .ok()
        .and_then(|samples| samples.checked_mul(usize::from(fmt.channels)))
        .and_then(|v| v.checked_mul(usize::from(fmt.bits / 8)))
        .ok_or_else(|| EsError("frame size overflow".to_string()))
}

fn read_u16_le(data: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let bytes: [u8; 2] = data.get(offset..end)?.try_into().ok()?;
    Some(u16::from_le_bytes(bytes))
}

fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let bytes: [u8; 4] = data.get(offset..end)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::{EsError, lpcm_frame_pts, parse_wav, parse_wav_for_bd};

    /// A WAV of `rate` Hz, `bits` bits and `channels` channels holding `samples` (one value a sample, in the
    /// order they play).
    fn pcm_wav(rate: u32, bits: u16, channels: u16, samples: &[i32]) -> Vec<u8> {
        let width = usize::from(bits / 8);
        let mut data = Vec::new();
        for sample in samples {
            data.extend_from_slice(&sample.to_le_bytes()[..width]);
        }
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * u32::from(channels) * u32::from(bits / 8)).to_le_bytes());
        wav.extend_from_slice(&(channels * (bits / 8)).to_le_bytes());
        wav.extend_from_slice(&bits.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        wav
    }

    /// 24-bit samples go in groups of two samples a channel: the high 16 bits of each, then their low bytes.
    #[test]
    fn twenty_four_bit_samples_are_grouped() -> Result<(), EsError> {
        // Stereo, two sample frames and a third that fills no group.
        let samples = [
            0x11_22_33, 0x44_55_66, 0x77_08_99, 0x0A_BB_CC, 0x01_02_03, 0x04_05_06,
        ];
        let stream = parse_wav(&pcm_wav(96_000, 24, 2, &samples))?;
        assert_eq!(
            (stream.sample_rate, stream.bits, stream.channels),
            (96_000, 24, 2)
        );
        assert_eq!(
            stream.data,
            [
                0x11, 0x22, 0x44, 0x55, 0x77, 0x08, 0x0A,
                0xBB, // the high 16 bits of four samples
                0x33, 0x66, 0x99, 0xCC, // their low bytes
            ]
        );
        // A 1/600 s frame: 160 samples a channel of 3 bytes.
        assert_eq!(stream.frame_bytes, 160 * 2 * 3);
        // 48 kHz 24-bit mono: 80 samples of 3 bytes.
        assert_eq!(
            parse_wav(&pcm_wav(48_000, 24, 1, &[1, 2]))?.frame_bytes,
            240
        );
        // 16-bit at 96 kHz is plain big-endian.
        let plain = parse_wav(&pcm_wav(96_000, 16, 1, &[0x0102, 0x0304]))?;
        assert_eq!(plain.data, [1, 2, 3, 4]);
        assert_eq!(plain.frame_bytes, 320);
        Ok(())
    }

    /// What a DVD's LPCM cannot carry is refused: other rates and word lengths, and too many channels for the
    /// rate and word length (6.144 Mbit/s).
    #[test]
    fn rates_and_sizes_past_the_stream_are_refused() {
        assert!(parse_wav(&pcm_wav(44_100, 16, 2, &[0, 0])).is_err());
        assert!(parse_wav(&pcm_wav(48_000, 8, 2, &[0, 0])).is_err());
        assert!(parse_wav(&pcm_wav(96_000, 24, 3, &[0; 6])).is_err());
        assert!(parse_wav(&pcm_wav(48_000, 24, 6, &[0; 12])).is_err());
        assert!(parse_wav(&pcm_wav(48_000, 24, 5, &[0; 10])).is_ok());
        assert!(parse_wav(&pcm_wav(96_000, 16, 4, &[0; 8])).is_ok());
    }

    /// M6.347: a Blu-ray's LPCM is also 192 kHz and up to 27.648 Mbit/s (an odd count of channels stored with
    /// one more); 20-bit samples are carried as 24-bit ones; an extensible file's speakers are kept.
    #[test]
    fn a_blu_ray_takes_192_khz_and_more_channels() -> Result<(), EsError> {
        assert!(parse_wav(&pcm_wav(192_000, 16, 2, &[0, 0])).is_err());
        let stream = parse_wav_for_bd(&pcm_wav(192_000, 24, 6, &[0; 12]))?;
        assert_eq!(
            (stream.sample_rate, stream.channels, stream.bits),
            (192_000, 6, 24)
        );
        assert_eq!(stream.frame_bytes, 320 * 6 * 3);
        // Five channels at 192 kHz 24-bit are stored as six: just within; seven as eight are not.
        assert!(parse_wav_for_bd(&pcm_wav(192_000, 24, 5, &[0; 10])).is_ok());
        assert!(parse_wav_for_bd(&pcm_wav(192_000, 24, 7, &[0; 14])).is_err());
        assert!(parse_wav_for_bd(&pcm_wav(192_000, 16, 8, &[0; 16])).is_ok());
        assert!(parse_wav_for_bd(&pcm_wav(44_100, 16, 2, &[0, 0])).is_err());
        // 20 bits in three bytes: as 24-bit samples, on either disc.
        let mut twenty = pcm_wav(
            48_000,
            24,
            2,
            &[0x12_34_50, 0x65_43_20, 0x0A_BC_D0, 0x0F_ED_C0],
        );
        twenty[34] = 20;
        let stream = parse_wav(&twenty)?;
        assert_eq!(stream.bits, 24);
        assert_eq!(
            stream.data,
            [
                0x12, 0x34, 0x65, 0x43, 0x0A, 0xBC, 0x0F, 0xED, 0x50, 0x20, 0xD0, 0xC0
            ]
        );
        assert_eq!(parse_wav_for_bd(&twenty)?.data, stream.data);
        // The speakers of an extensible file (5.1 with side surrounds).
        let mut wav = pcm_wav(48_000, 16, 6, &[0; 6]);
        let fmt = [
            &1u16.to_le_bytes()[..],
            &6u16.to_le_bytes(),
            &48_000u32.to_le_bytes(),
            &(48_000u32 * 12).to_le_bytes(),
            &12u16.to_le_bytes(),
            &16u16.to_le_bytes(),
        ]
        .concat();
        let mut extensible = fmt.clone();
        extensible[0..2].copy_from_slice(&0xFFFEu16.to_le_bytes());
        extensible.extend_from_slice(&22u16.to_le_bytes());
        extensible.extend_from_slice(&16u16.to_le_bytes());
        extensible.extend_from_slice(&0x60Fu32.to_le_bytes());
        extensible.extend_from_slice(&[
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38,
            0x9b, 0x71,
        ]);
        let data = wav.split_off(36);
        assert_eq!(
            parse_wav_for_bd(&wav_with(&extensible, &data))?.channel_mask,
            0x60F
        );
        assert_eq!(parse_wav(&wav_with(&fmt, &data))?.channel_mask, 0);
        Ok(())
    }

    /// A RIFF WAVE file of `fmt` and the `data` chunk (header included).
    fn wav_with(fmt: &[u8], data: &[u8]) -> Vec<u8> {
        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&(4 + 8 + fmt.len() as u32 + data.len() as u32).to_le_bytes());
        file.extend_from_slice(b"WAVEfmt ");
        file.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        file.extend_from_slice(fmt);
        file.extend_from_slice(data);
        file
    }

    fn build_wav(
        sample_rate: u32,
        channels: u16,
        bits: u16,
        samples: &[u16],
        include_list: bool,
    ) -> Vec<u8> {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&[0, 0, 0, 0]);
        wav.extend_from_slice(b"WAVE");

        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        let block_align = channels.saturating_mul(bits / 8);
        let byte_rate = sample_rate.saturating_mul(u32::from(block_align));
        wav.extend_from_slice(&byte_rate.to_le_bytes());
        wav.extend_from_slice(&block_align.to_le_bytes());
        wav.extend_from_slice(&bits.to_le_bytes());

        if include_list {
            let list = b"INFO";
            wav.extend_from_slice(b"LIST");
            wav.extend_from_slice(&(list.len() as u32).to_le_bytes());
            wav.extend_from_slice(list);
        }

        let mut data_bytes = Vec::new();
        for sample in samples {
            data_bytes.extend_from_slice(&sample.to_le_bytes());
        }
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data_bytes.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data_bytes);

        let riff_size = (wav.len() as u32).saturating_sub(8);
        if let Some(slice) = wav.get_mut(4..8) {
            slice.copy_from_slice(&riff_size.to_le_bytes());
        }
        wav
    }

    #[test]
    fn parses_valid_wav_with_list_chunk() -> Result<(), EsError> {
        let samples = [0x0102u16, 0x0304u16];
        let wav = build_wav(48_000, 2, 16, &samples, true);
        let lpcm = parse_wav(&wav)?;
        assert_eq!(lpcm.sample_rate, 48_000);
        assert_eq!(lpcm.channels, 2);
        assert_eq!(lpcm.bits, 16);
        assert_eq!(lpcm.frame_bytes, 320);
        assert_eq!(lpcm.data, vec![0x01, 0x02, 0x03, 0x04]);
        Ok(())
    }

    #[test]
    fn rejects_44100() {
        let wav = build_wav(44_100, 2, 16, &[0x0000], true);
        assert!(parse_wav(&wav).is_err());
    }

    #[test]
    fn rejects_truncated() {
        let wav = build_wav(48_000, 2, 16, &[0x0000], true);
        let truncated = wav.get(..wav.len().saturating_sub(1)).unwrap_or(&[]);
        assert!(parse_wav(truncated).is_err());
    }

    #[test]
    fn prefixes_do_not_panic() {
        let wav = build_wav(48_000, 2, 16, &[0x0000, 0x0001], true);
        for i in 0..=wav.len() {
            let prefix = wav.get(..i).unwrap_or(&[]);
            let _ = parse_wav(prefix);
        }
    }

    #[test]
    fn frame_pts_saturates() {
        assert_eq!(lpcm_frame_pts(100, 2), 90_100);
        assert_eq!(lpcm_frame_pts(i64::MAX, 1), i64::MAX);
        assert_eq!(lpcm_frame_pts(i64::MIN, -1), i64::MIN);
    }
}
