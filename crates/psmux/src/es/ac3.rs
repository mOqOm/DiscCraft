use super::EsError;

/// One AC-3 access unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ac3Frame {
    /// Byte offset of the frame in the elementary stream.
    pub offset: usize,
    /// Frame length in bytes.
    pub len: usize,
    /// Presentation timestamp in 27 MHz units.
    pub pts: i64,
}

/// Parsed AC-3 elementary stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ac3Stream {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Bit rate in kbit/s.
    pub bit_rate_kbps: u32,
    /// Channel count including LFE.
    pub channels: u8,
    /// True when LFE is enabled.
    pub lfe: bool,
    /// AC-3 acmod value.
    pub acmod: u8,
    /// Frames in stream order.
    pub frames: Vec<Ac3Frame>,
}

const MAX_AC3_FRAMES: usize = 10_000_000;
const AC3_PTS_STEP: i64 = 864_000;

const BITRATES: [u32; 19] = [
    32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 576, 640,
];

/// Parse an AC-3 elementary stream.
pub fn parse_ac3(data: &[u8], start_pts: i64) -> Result<Ac3Stream, EsError> {
    let mut frames = Vec::new();
    let mut sample_rate = 0u32;
    let mut bit_rate_kbps = 0u32;
    let mut channels = 0u8;
    let mut lfe = false;
    let mut acmod = 0u8;

    let mut offset = 0usize;
    let mut index = 0usize;

    while offset < data.len() {
        if frames.len() >= MAX_AC3_FRAMES {
            return Err(es_err("too many AC-3 frames"));
        }

        let header = parse_ac3_header(data, offset)?;
        if index == 0 {
            sample_rate = header.sample_rate;
            bit_rate_kbps = header.bit_rate_kbps;
            channels = header.channels;
            lfe = header.lfe;
            acmod = header.acmod;
        } else if sample_rate != header.sample_rate
            || bit_rate_kbps != header.bit_rate_kbps
            || channels != header.channels
            || lfe != header.lfe
            || acmod != header.acmod
        {
            return Err(es_err("inconsistent AC-3 stream"));
        }

        let end = offset
            .checked_add(header.len)
            .ok_or_else(|| es_err("AC-3 frame length overflow"))?;
        if end > data.len() {
            return Err(es_err("truncated AC-3 frame"));
        }

        let idx = i64::try_from(index).map_err(|_| es_err("too many AC-3 frames"))?;
        let pts = start_pts
            .checked_add(
                idx.checked_mul(AC3_PTS_STEP)
                    .ok_or_else(|| es_err("AC-3 PTS overflow"))?,
            )
            .ok_or_else(|| es_err("AC-3 PTS overflow"))?;

        frames.push(Ac3Frame {
            offset,
            len: header.len,
            pts,
        });

        offset = end;
        index = index
            .checked_add(1)
            .ok_or_else(|| es_err("AC-3 frame count overflow"))?;
    }

    if frames.is_empty() {
        return Err(es_err("no AC-3 frames"));
    }

    Ok(Ac3Stream {
        sample_rate,
        bit_rate_kbps,
        channels,
        lfe,
        acmod,
        frames,
    })
}

fn es_err(msg: &str) -> EsError {
    EsError(msg.to_string())
}

struct BitReader<'a> {
    data: &'a [u8],
    byte: usize,
    bit: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8], byte: usize) -> Option<Self> {
        if byte > data.len() {
            None
        } else {
            Some(Self { data, byte, bit: 0 })
        }
    }

    fn read_bits(&mut self, n: u32) -> Option<u32> {
        if n > 32 {
            return None;
        }
        let mut value = 0u32;
        for _ in 0..n {
            let byte = self.data.get(self.byte)?;
            let mask = 1u8 << (7 - self.bit);
            let bit = if byte & mask != 0 { 1u32 } else { 0u32 };
            value = (value << 1) | bit;
            self.bit += 1;
            if self.bit == 8 {
                self.bit = 0;
                self.byte = self.byte.checked_add(1)?;
            }
        }
        Some(value)
    }
}

struct Ac3Header {
    len: usize,
    sample_rate: u32,
    bit_rate_kbps: u32,
    channels: u8,
    lfe: bool,
    acmod: u8,
}

fn acmod_channels(acmod: u8) -> u8 {
    match acmod {
        0 | 2 => 2,
        1 => 1,
        3 | 4 => 3,
        5 | 6 => 4,
        7 => 5,
        _ => 0,
    }
}

fn parse_ac3_header(data: &[u8], offset: usize) -> Result<Ac3Header, EsError> {
    let sync0 = data.get(offset).copied();
    let sync1 = offset.checked_add(1).and_then(|i| data.get(i).copied());
    if sync0 != Some(0x0B) || sync1 != Some(0x77) {
        return Err(es_err("AC-3 sync word not found"));
    }

    let mut br = BitReader::new(data, offset).ok_or_else(|| es_err("truncated AC-3 header"))?;
    let _sync = br
        .read_bits(16)
        .ok_or_else(|| es_err("truncated AC-3 header"))?;
    let _crc1 = br
        .read_bits(16)
        .ok_or_else(|| es_err("truncated AC-3 header"))?;
    let fscod = br
        .read_bits(2)
        .ok_or_else(|| es_err("truncated AC-3 header"))?;
    if fscod != 0 {
        return Err(es_err("only 48 kHz AC-3 is supported"));
    }
    let frmsizecod = br
        .read_bits(6)
        .ok_or_else(|| es_err("truncated AC-3 header"))?;
    if frmsizecod > 37 {
        return Err(es_err("invalid AC-3 frame size code"));
    }
    let bsid = br
        .read_bits(5)
        .ok_or_else(|| es_err("truncated AC-3 header"))?;
    if bsid > 8 {
        return Err(es_err("invalid AC-3 bsid"));
    }
    let _bsmod = br
        .read_bits(3)
        .ok_or_else(|| es_err("truncated AC-3 header"))?;
    let acmod = br
        .read_bits(3)
        .ok_or_else(|| es_err("truncated AC-3 header"))? as u8;

    if acmod & 1 != 0 && acmod != 1 {
        let _cmixlev = br
            .read_bits(2)
            .ok_or_else(|| es_err("truncated AC-3 header"))?;
    }
    if acmod & 4 != 0 {
        let _surmixlev = br
            .read_bits(2)
            .ok_or_else(|| es_err("truncated AC-3 header"))?;
    }
    if acmod == 2 {
        let _dsurmod = br
            .read_bits(2)
            .ok_or_else(|| es_err("truncated AC-3 header"))?;
    }
    let lfe = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated AC-3 header"))?
        != 0;

    let idx =
        usize::try_from(frmsizecod / 2).map_err(|_| es_err("invalid AC-3 frame size code"))?;
    let bit_rate_kbps = *BITRATES
        .get(idx)
        .ok_or_else(|| es_err("invalid AC-3 frame size code"))?;
    let len = 4usize
        .checked_mul(bit_rate_kbps as usize)
        .ok_or_else(|| es_err("AC-3 frame length overflow"))?;
    let channels = acmod_channels(acmod)
        .checked_add(if lfe { 1 } else { 0 })
        .ok_or_else(|| es_err("AC-3 channel count overflow"))?;

    Ok(Ac3Header {
        len,
        sample_rate: 48_000,
        bit_rate_kbps,
        channels,
        lfe,
        acmod,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_ac3;

    struct BitWriter {
        bytes: Vec<u8>,
        current: u8,
        bits_left: u8,
    }

    impl BitWriter {
        fn new() -> Self {
            Self {
                bytes: Vec::new(),
                current: 0,
                bits_left: 8,
            }
        }

        fn write_bits(&mut self, value: u32, n: u32) {
            for i in (0..n).rev() {
                let bit = ((value >> i) & 1) as u8;
                self.current |= bit << (self.bits_left - 1);
                self.bits_left -= 1;
                if self.bits_left == 0 {
                    self.bytes.push(self.current);
                    self.current = 0;
                    self.bits_left = 8;
                }
            }
        }

        fn finish(&mut self) {
            if self.bits_left != 8 {
                self.bytes.push(self.current);
                self.current = 0;
                self.bits_left = 8;
            }
        }
    }

    fn build_ac3_frame(fscod: u32, frmsizecod: u32, acmod: u32, lfe: u32) -> Vec<u8> {
        let mut bw = BitWriter::new();
        bw.write_bits(0x0B77, 16);
        bw.write_bits(0, 16);
        bw.write_bits(fscod, 2);
        bw.write_bits(frmsizecod, 6);
        bw.write_bits(8, 5);
        bw.write_bits(0, 3);
        bw.write_bits(acmod, 3);
        if acmod & 1 != 0 && acmod != 1 {
            bw.write_bits(0, 2);
        }
        if acmod & 4 != 0 {
            bw.write_bits(0, 2);
        }
        if acmod == 2 {
            bw.write_bits(0, 2);
        }
        bw.write_bits(lfe, 1);
        bw.finish();

        let mut frame = bw.bytes;
        frame.resize(768, 0);
        frame
    }

    #[test]
    fn parses_three_ac3_frames() {
        let mut data = Vec::new();
        for _ in 0..3 {
            data.extend_from_slice(&build_ac3_frame(0, 20, 2, 0));
        }

        let result = parse_ac3(&data, 0);
        assert!(result.is_ok());

        if let Ok(stream) = result {
            assert_eq!(stream.frames.len(), 3);
            assert_eq!(stream.frames[0].len, 768);
            assert_eq!(stream.frames[1].pts, 864_000);
            assert_eq!(stream.frames[2].pts, 1_728_000);
            assert_eq!(stream.channels, 2);
            assert_eq!(stream.bit_rate_kbps, 192);
            assert_eq!(stream.sample_rate, 48_000);
        }
    }

    #[test]
    fn rejects_non_48khz() {
        let frame = build_ac3_frame(1, 20, 2, 0);
        assert!(parse_ac3(&frame, 0).is_err());
    }

    #[test]
    fn rejects_trailing_partial_frame() {
        let mut data = build_ac3_frame(0, 20, 2, 0);
        data.extend_from_slice(&[0, 0, 0]);
        assert!(parse_ac3(&data, 0).is_err());
    }

    #[test]
    fn rejects_random_bytes() {
        let data = vec![1, 2, 3, 4, 5, 6, 7, 8];
        assert!(parse_ac3(&data, 0).is_err());
    }
}
