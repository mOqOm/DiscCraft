//! DTS Coherent Acoustics core frames (ETSI TS 102 114 V1.2.1, clause 5.3: the frame header).
//!
//! Only what repackaging needs is read: where each frame starts and ends, how long it plays, and what the stream
//! says of itself (sampling frequency, channels, bit rate). The audio data is not decoded.

use super::{Ac3Frame, EsError};

/// The sync word of a core frame in its 16-bit big-endian form (the form a DVD carries).
const SYNC: [u8; 4] = [0x7F, 0xFE, 0x80, 0x01];
/// The bytes of a frame that hold the sync word and the header fields read here (55 bits after the sync word).
const HEADER_BYTES: usize = 11;
/// The most frames taken from one stream.
const MAX_FRAMES: usize = 20_000_000;
/// The 27 MHz clock.
const CLOCK: i64 = 27_000_000;

/// Table 5.7: the targeted bit rate in kbit/s by `RATE` (0 for open, variable and loss-less).
const RATES: [u32; 32] = [
    32, 56, 64, 96, 112, 128, 192, 224, 256, 320, 384, 448, 512, 576, 640, 768, 960, 1024, 1152,
    1280, 1344, 1408, 1411, 1472, 1536, 1920, 2048, 3072, 3840, 0, 0, 0,
];

/// Table 5.4: the number of channels by `AMODE` (the standard arrangements).
const CHANNELS: [u8; 16] = [1, 2, 2, 2, 2, 3, 3, 4, 4, 5, 6, 6, 6, 7, 8, 8];

/// A parsed DTS elementary stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DtsStream {
    /// Sampling frequency in Hz.
    pub sample_rate: u32,
    /// The targeted bit rate in kbit/s (0 when the stream does not name one).
    pub bit_rate_kbps: u32,
    /// Channel count including the low frequency effects channel.
    pub channels: u8,
    /// Whether there is a low frequency effects channel.
    pub lfe: bool,
    /// Samples a frame plays, per channel.
    pub frame_samples: u32,
    /// Frames in stream order (27 MHz time stamps).
    pub frames: Vec<Ac3Frame>,
}

/// The header fields of one frame.
struct Header {
    len: usize,
    samples: u32,
    sample_rate: u32,
    bit_rate_kbps: u32,
    channels: u8,
    lfe: bool,
}

fn es_err(message: &str) -> EsError {
    EsError(message.to_string())
}

/// Table 5.5: the sampling frequency by `SFREQ`.
fn sample_rate(code: u64) -> Option<u32> {
    match code {
        1 => Some(8_000),
        2 => Some(16_000),
        3 => Some(32_000),
        6 => Some(11_025),
        7 => Some(22_050),
        8 => Some(44_100),
        11 => Some(12_000),
        12 => Some(24_000),
        13 => Some(48_000),
        _ => None,
    }
}

fn parse_header(data: &[u8], offset: usize) -> Result<Header, EsError> {
    let end = offset
        .checked_add(HEADER_BYTES)
        .ok_or_else(|| es_err("DTS offset overflow"))?;
    let bytes = data
        .get(offset..end)
        .ok_or_else(|| es_err("truncated DTS frame header"))?;
    if !bytes.starts_with(&SYNC) {
        return Err(es_err("DTS sync word not found"));
    }
    // The 56 bits after the sync word, most significant first.
    let bits = bytes
        .iter()
        .skip(SYNC.len())
        .fold(0u64, |bits, byte| (bits << 8) | u64::from(*byte));
    let mut left = 56u32;
    let mut take = |count: u32| {
        left = left.saturating_sub(count);
        (bits >> left) & ((1u64 << count) - 1)
    };
    let normal = take(1) == 1; // FTYPE
    let short = take(5); // SHORT
    let _crc = take(1); // CPF
    let blocks = take(7); // NBLKS
    let size = take(14); // FSIZE
    let arrangement = take(6); // AMODE
    let frequency = take(4); // SFREQ
    let rate = take(5); // RATE
    let _flags = take(5); // MIX, DYNF, TIMEF, AUXF, HDCD
    let _extension = take(5); // EXT_AUDIO_ID, EXT_AUDIO, ASPF
    let lfe = take(2); // LFF

    if !normal || short != 31 {
        return Err(es_err("DTS termination frames are not supported"));
    }
    if blocks < 5 {
        return Err(es_err("invalid DTS block count"));
    }
    if size < 95 {
        return Err(es_err("invalid DTS frame size"));
    }
    let sample_rate =
        sample_rate(frequency).ok_or_else(|| es_err("invalid DTS sampling frequency"))?;
    let channels = usize::try_from(arrangement)
        .ok()
        .and_then(|index| CHANNELS.get(index).copied())
        .ok_or_else(|| es_err("unsupported DTS channel arrangement"))?;
    let has_lfe = lfe == 1 || lfe == 2;
    Ok(Header {
        len: usize::try_from(size + 1).map_err(|_| es_err("invalid DTS frame size"))?,
        samples: u32::try_from((blocks + 1) * 32).map_err(|_| es_err("invalid DTS block count"))?,
        sample_rate,
        bit_rate_kbps: usize::try_from(rate)
            .ok()
            .and_then(|index| RATES.get(index).copied())
            .unwrap_or(0),
        channels: channels.saturating_add(u8::from(has_lfe)),
        lfe: has_lfe,
    })
}

/// The bit rate (kbit/s) a DTS stream uses, from its first frame alone: the frame's bytes over the time it plays.
#[must_use]
pub fn first_frame_bit_rate_kbps(data: &[u8]) -> Option<u32> {
    let header = parse_header(data, 0).ok()?;
    let bits = u64::try_from(header.len).ok()?.checked_mul(8)?;
    let rate = bits
        .checked_mul(u64::from(header.sample_rate))?
        .checked_div(u64::from(header.samples))?;
    u32::try_from(rate / 1000).ok()
}

/// How long a DTS stream of `total_bytes` plays, in seconds, from its first frame alone (every frame of a stream
/// has the same size and length).
#[must_use]
pub fn seconds_of(head: &[u8], total_bytes: u64) -> Option<f64> {
    let header = parse_header(head, 0).ok()?;
    let frames = total_bytes.checked_div(u64::try_from(header.len).ok()?)?;
    // Frame counts and sample counts are far inside what a float holds exactly.
    Some(frames as f64 * f64::from(header.samples) / f64::from(header.sample_rate))
}

/// Parses a DTS elementary stream (core frames in the 16-bit big-endian form, one after another).
///
/// # Errors
///
/// When a frame header is missing or invalid, a frame is cut short, or the frames differ in their format.
pub fn parse_dts(data: &[u8], start_pts: i64) -> Result<DtsStream, EsError> {
    let mut frames = Vec::new();
    let mut first: Option<Header> = None;
    let mut offset = 0usize;
    while offset < data.len() {
        if frames.len() >= MAX_FRAMES {
            return Err(es_err("too many DTS frames"));
        }
        let header = parse_header(data, offset)?;
        if let Some(first) = &first
            && (first.samples != header.samples
                || first.sample_rate != header.sample_rate
                || first.channels != header.channels
                || first.lfe != header.lfe)
        {
            return Err(es_err("inconsistent DTS stream"));
        }
        let end = offset
            .checked_add(header.len)
            .ok_or_else(|| es_err("DTS frame length overflow"))?;
        if end > data.len() {
            return Err(es_err("truncated DTS frame"));
        }
        // Frame n starts n frames of samples after the first.
        let index = i64::try_from(frames.len()).map_err(|_| es_err("too many DTS frames"))?;
        let pts = index
            .checked_mul(i64::from(header.samples))
            .and_then(|samples| samples.checked_mul(CLOCK))
            .and_then(|ticks| ticks.checked_div(i64::from(header.sample_rate)))
            .and_then(|ticks| ticks.checked_add(start_pts))
            .ok_or_else(|| es_err("DTS PTS overflow"))?;
        frames.push(Ac3Frame {
            offset,
            len: header.len,
            pts,
        });
        let _ = first.get_or_insert(header);
        offset = end;
    }
    let first = first.ok_or_else(|| es_err("no DTS frames"))?;
    Ok(DtsStream {
        sample_rate: first.sample_rate,
        bit_rate_kbps: first.bit_rate_kbps,
        channels: first.channels,
        lfe: first.lfe,
        frame_samples: first.samples,
        frames,
    })
}

#[cfg(test)]
mod tests {
    use super::{first_frame_bit_rate_kbps, parse_dts};

    /// A frame of `len` bytes: 48 kHz, 512 samples, `amode`, `rate`, with or without LFE.
    fn frame(len: usize, amode: u64, rate: u64, lfe: u64) -> Vec<u8> {
        let size = u64::try_from(len).unwrap_or(0) - 1;
        // FTYPE 1, SHORT 31, CPF 0, NBLKS 15, FSIZE, AMODE, SFREQ 13, RATE, 5 flags 0, 5 extension bits 0, LFF.
        let mut bits = 1u64;
        for (value, width) in [
            (31, 5),
            (0, 1),
            (15, 7),
            (size, 14),
            (amode, 6),
            (13, 4),
            (rate, 5),
            (0, 5),
            (0, 5),
            (lfe, 2),
        ] {
            bits = (bits << width) | value;
        }
        // 55 bits; the 56th is the first bit after the header.
        bits <<= 1;
        let mut out = vec![0x7F, 0xFE, 0x80, 0x01];
        out.extend_from_slice(bits.to_be_bytes().get(1..).unwrap_or(&[]));
        out.resize(len, 0);
        out
    }

    #[test]
    fn frames_are_found_with_their_times() {
        // 5.1 at 1 536 kbit/s (on a DVD: 2 013 bytes a frame, 1 509.75 kbit/s).
        let mut data = frame(2013, 9, 24, 2);
        data.extend(frame(2013, 9, 24, 2));
        data.extend(frame(2013, 9, 24, 2));
        let stream = parse_dts(&data, 1000).unwrap();
        assert_eq!(stream.sample_rate, 48_000);
        assert_eq!(stream.bit_rate_kbps, 1536);
        assert_eq!((stream.channels, stream.lfe), (6, true));
        assert_eq!(stream.frame_samples, 512);
        let frames: Vec<(usize, usize, i64)> = stream
            .frames
            .iter()
            .map(|frame| (frame.offset, frame.len, frame.pts))
            .collect();
        // 512 samples at 48 kHz are 288 000 ticks of 27 MHz (960 of 90 kHz).
        assert_eq!(
            frames,
            [
                (0, 2013, 1000),
                (2013, 2013, 289_000),
                (4026, 2013, 577_000)
            ]
        );
        assert_eq!(first_frame_bit_rate_kbps(&data), Some(1509));
        // 3 frames of 512 samples at 48 kHz.
        assert_eq!(super::seconds_of(&data, 6039), Some(0.032));
    }

    #[test]
    fn bad_streams_are_refused() {
        let good = frame(1006, 2, 15, 0);
        assert_eq!(parse_dts(&good, 0).map(|stream| stream.channels), Ok(2));
        // Cut short, no sync word, another format in the second frame, nothing at all.
        assert!(parse_dts(good.get(..500).unwrap_or(&[]), 0).is_err());
        assert!(parse_dts(&[0u8; 2000], 0).is_err());
        let mut mixed = good.clone();
        mixed.extend(frame(1006, 9, 15, 0));
        assert!(parse_dts(&mixed, 0).is_err());
        assert!(parse_dts(&[], 0).is_err());
        assert_eq!(first_frame_bit_rate_kbps(&[1, 2, 3]), None);
    }
}
