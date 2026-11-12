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
    /// Big-endian interleaved sample data.
    pub data: Vec<u8>,
    /// Bytes per 1/600 s LPCM frame.
    pub frame_bytes: usize,
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
const LPCM_FRAME_SAMPLES: usize = 80;
const LPCM_FRAME_PTS: i64 = 45_000;

struct FmtInfo {
    channels: u8,
    sample_rate: u32,
    bits: u8,
}

struct DataChunk {
    start: usize,
    len: usize,
}

/// Parse a WAV file into DVD LPCM data.
pub fn parse_wav(data: &[u8]) -> Result<LpcmStream, EsError> {
    let limit = parse_riff_header(data)?;
    let (fmt, chunk) = parse_chunks(data, limit)?;
    let fmt = fmt.ok_or_else(|| EsError("missing fmt chunk".to_string()))?;
    let chunk = chunk.ok_or_else(|| EsError("missing data chunk".to_string()))?;
    let samples = convert_samples(data, &chunk, fmt.channels)?;
    let frame_bytes = frame_bytes(fmt.channels)?;
    Ok(LpcmStream {
        sample_rate: fmt.sample_rate,
        channels: fmt.channels,
        bits: fmt.bits,
        data: samples,
        frame_bytes,
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
            fmt = Some(parse_fmt(body)?);
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

fn parse_fmt(body: &[u8]) -> Result<FmtInfo, EsError> {
    let tag = read_u16_le(body, 0).ok_or_else(|| EsError("missing format tag".to_string()))?;
    let channels =
        read_u16_le(body, 2).ok_or_else(|| EsError("missing channel count".to_string()))?;
    let sample_rate =
        read_u32_le(body, 4).ok_or_else(|| EsError("missing sample rate".to_string()))?;
    let block_align =
        read_u16_le(body, 12).ok_or_else(|| EsError("missing block align".to_string()))?;
    let bits =
        read_u16_le(body, 14).ok_or_else(|| EsError("missing bits per sample".to_string()))?;

    if tag == EXTENSIBLE {
        let sub = body
            .get(24..40)
            .ok_or_else(|| EsError("missing extensible subformat".to_string()))?;
        if sub != PCM_SUBFORMAT {
            return Err(EsError("extensible subformat is not PCM".to_string()));
        }
    } else if tag != PCM {
        return Err(EsError("unsupported WAV format tag".to_string()));
    }

    if channels == 0 || channels > 8 {
        return Err(EsError("channel count must be 1..=8".to_string()));
    }
    if bits != 16 {
        return Err(EsError("bits per sample must be 16".to_string()));
    }
    if sample_rate != 48_000 {
        return Err(EsError("sample rate must be 48000 Hz".to_string()));
    }
    let expected_block = channels
        .checked_mul(2)
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
    })
}

fn convert_samples(data: &[u8], chunk: &DataChunk, channels: u8) -> Result<Vec<u8>, EsError> {
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

fn frame_bytes(channels: u8) -> Result<usize, EsError> {
    LPCM_FRAME_SAMPLES
        .checked_mul(usize::from(channels))
        .and_then(|v| v.checked_mul(2))
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
    use super::{EsError, lpcm_frame_pts, parse_wav};

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
