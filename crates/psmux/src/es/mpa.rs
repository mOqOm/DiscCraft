//! MPEG-1 Audio Layer II frames (ISO/IEC 11172-3, 2.4.1 and 2.4.2: the frame header).
//!
//! Only what repackaging needs is read: where each frame starts and ends, how long it plays, and what the stream
//! says of itself (sampling frequency, bit rate, mono or two channels). The audio data is not decoded.

use super::{Ac3Frame, EsError};

/// The bytes of a frame header.
const HEADER_BYTES: usize = 4;
/// Samples a channel in a Layer II frame.
pub const FRAME_SAMPLES: u32 = 1152;
/// The most frames taken from one stream.
const MAX_FRAMES: usize = 20_000_000;
/// The 27 MHz clock.
const CLOCK: i64 = 27_000_000;

/// The bit rate in kbit/s by `bitrate_index` for Layer II (0: free format, 15: forbidden; neither is taken).
const RATES: [u32; 16] = [
    0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 0,
];

/// A parsed MPEG-1 Layer II elementary stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpaStream {
    /// Sampling frequency in Hz.
    pub sample_rate: u32,
    /// Bit rate in kbit/s.
    pub bit_rate_kbps: u32,
    /// 1 for single channel, else 2 (stereo, joint stereo or dual channel).
    pub channels: u8,
    /// Frames in stream order (27 MHz time stamps).
    pub frames: Vec<Ac3Frame>,
}

/// The header fields of one frame.
struct Header {
    len: usize,
    sample_rate: u32,
    bit_rate_kbps: u32,
    channels: u8,
}

fn es_err(message: &str) -> EsError {
    EsError(message.to_string())
}

/// Reads the frame header at `offset`: the syncword, MPEG-1 (ID 1), Layer II, a bit rate from the table and a
/// sampling frequency.
fn parse_header(data: &[u8], offset: usize) -> Result<Header, EsError> {
    let header: [u8; HEADER_BYTES] = offset
        .checked_add(HEADER_BYTES)
        .and_then(|end| data.get(offset..end))
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| es_err("truncated MPEG audio header"))?;
    let [sync, second, third, fourth] = header;
    if sync != 0xFF || second & 0xF0 != 0xF0 {
        return Err(es_err("MPEG audio syncword not found"));
    }
    // ID (1: MPEG-1) and layer ('10': Layer II).
    if second & 0x08 == 0 || (second >> 1) & 0x03 != 0b10 {
        return Err(es_err("not MPEG-1 Layer II audio"));
    }
    let bit_rate_kbps = RATES
        .get(usize::from(third >> 4))
        .copied()
        .filter(|rate| *rate != 0)
        .ok_or_else(|| es_err("unsupported MPEG audio bit rate"))?;
    let sample_rate = match (third >> 2) & 0x03 {
        0 => 44_100,
        1 => 48_000,
        2 => 32_000,
        _ => return Err(es_err("reserved MPEG audio sampling frequency")),
    };
    let padding = usize::from((third >> 1) & 1);
    // A Layer II frame is 144 * bit rate / sampling frequency bytes, one more when padded.
    let len = usize::try_from(144_000 * u64::from(bit_rate_kbps) / u64::from(sample_rate))
        .ok()
        .and_then(|len| len.checked_add(padding))
        .filter(|len| *len > HEADER_BYTES)
        .ok_or_else(|| es_err("invalid MPEG audio frame size"))?;
    Ok(Header {
        len,
        sample_rate,
        bit_rate_kbps,
        channels: if fourth >> 6 == 0b11 { 1 } else { 2 },
    })
}

/// What the first frame of an MPEG-1 Layer II stream states, from the stream's first bytes alone: sampling
/// frequency (Hz), bit rate (kbit/s) and channels.
#[must_use]
pub fn first_frame_format(data: &[u8]) -> Option<(u32, u32, u8)> {
    parse_header(data, 0)
        .ok()
        .map(|header| (header.sample_rate, header.bit_rate_kbps, header.channels))
}

/// How long a stream of `total_bytes` plays, in seconds, from its first frame alone (at one bit rate the frames
/// of a stream are of one size, give or take a padding byte).
#[must_use]
pub fn seconds_of(head: &[u8], total_bytes: u64) -> Option<f64> {
    let header = parse_header(head, 0).ok()?;
    // Bytes over the bit rate: exact at 48 kHz, where no frame is padded.
    Some(total_bytes as f64 * 8.0 / (f64::from(header.bit_rate_kbps) * 1000.0))
}

/// Parses an MPEG-1 Layer II elementary stream (frames one after another).
///
/// # Errors
///
/// When a frame header is missing or invalid, a frame is cut short, or the frames differ in their format.
pub fn parse_mpa(data: &[u8], start_pts: i64) -> Result<MpaStream, EsError> {
    let mut frames = Vec::new();
    let mut first: Option<Header> = None;
    let mut offset = 0usize;
    while offset < data.len() {
        if frames.len() >= MAX_FRAMES {
            return Err(es_err("too many MPEG audio frames"));
        }
        let header = parse_header(data, offset)?;
        if let Some(first) = &first
            && (first.sample_rate != header.sample_rate
                || first.bit_rate_kbps != header.bit_rate_kbps
                || first.channels != header.channels)
        {
            return Err(es_err("inconsistent MPEG audio stream"));
        }
        let end = offset
            .checked_add(header.len)
            .filter(|end| *end <= data.len())
            .ok_or_else(|| es_err("truncated MPEG audio frame"))?;
        // Each frame plays 1152 samples.
        let index =
            i64::try_from(frames.len()).map_err(|_| es_err("too many MPEG audio frames"))?;
        let ticks = index
            .checked_mul(i64::from(FRAME_SAMPLES))
            .and_then(|samples| samples.checked_mul(CLOCK))
            .map(|ticks| ticks / i64::from(header.sample_rate))
            .ok_or_else(|| es_err("MPEG audio time overflow"))?;
        frames.push(Ac3Frame {
            offset,
            len: header.len,
            pts: start_pts.saturating_add(ticks),
        });
        if first.is_none() {
            first = Some(header);
        }
        offset = end;
    }
    let first = first.ok_or_else(|| es_err("no MPEG audio frames"))?;
    Ok(MpaStream {
        sample_rate: first.sample_rate,
        bit_rate_kbps: first.bit_rate_kbps,
        channels: first.channels,
        frames,
    })
}

#[cfg(test)]
mod tests {
    use super::{first_frame_format, parse_mpa, seconds_of};

    /// A Layer II frame header at 48 kHz with `rate_index` and `mode`, and the frame's bytes behind it.
    fn frame(rate_index: u8, mode: u8) -> Vec<u8> {
        let kbps = super::RATES[usize::from(rate_index)];
        let mut bytes = vec![0xFF, 0xFD, (rate_index << 4) | 0x04, mode << 6];
        bytes.resize(3 * kbps as usize, 0x55);
        bytes
    }

    #[test]
    fn frames_are_found_one_after_another() {
        // 192 kbit/s stereo at 48 kHz: 576 bytes and 24 ms a frame.
        let mut data = frame(10, 0);
        data.extend(frame(10, 0));
        data.extend(frame(10, 0));
        let stream = parse_mpa(&data, 1000).unwrap();
        assert_eq!(
            (stream.sample_rate, stream.bit_rate_kbps, stream.channels),
            (48_000, 192, 2)
        );
        let frames: Vec<(usize, usize, i64)> = stream
            .frames
            .iter()
            .map(|frame| (frame.offset, frame.len, frame.pts))
            .collect();
        assert_eq!(
            frames,
            [(0, 576, 1000), (576, 576, 649_000), (1152, 576, 1_297_000)]
        );
        assert_eq!(first_frame_format(&data), Some((48_000, 192, 2)));
        assert_eq!(seconds_of(&data, 576 * 1000), Some(24.0));
        // Mono.
        assert_eq!(parse_mpa(&frame(4, 3), 0).unwrap().channels, 1);
    }

    #[test]
    fn what_is_not_layer_two_is_refused() {
        // Cut short; another bit rate in the second frame; Layer III; free format; no syncword.
        let whole = frame(10, 0);
        assert!(parse_mpa(&whole[..500], 0).is_err());
        let mut mixed = frame(10, 0);
        mixed.extend(frame(11, 0));
        assert!(parse_mpa(&mixed, 0).is_err());
        let mut layer3 = frame(10, 0);
        layer3[1] = 0xFB;
        assert!(parse_mpa(&layer3, 0).is_err());
        let mut free = frame(10, 0);
        free[2] = 0x04;
        assert!(parse_mpa(&free, 0).is_err());
        assert!(parse_mpa(b"RIFF....WAVE", 0).is_err());
        assert!(parse_mpa(&[], 0).is_err());
        assert!(first_frame_format(&[0xFF]).is_none());
    }
}
