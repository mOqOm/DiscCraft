//! The few fields of an H.264 sequence parameter set (ITU-T H.264 7.3.2.1.1) that decide whether a stream can go on a
//! Blu-ray as it is.

/// What a sequence parameter set says about its stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sps {
    /// profile_idc (77 Main, 100 High).
    pub(crate) profile: u8,
    /// level_idc (41 is level 4.1).
    pub(crate) level: u8,
    /// Coded width, pixels (whole macroblocks).
    pub(crate) width: u32,
    /// Coded height, pixels (whole macroblocks of a frame).
    pub(crate) height: u32,
    /// Every picture is a frame of frame macroblocks (progressive coding); else fields or MBAFF may be used.
    pub(crate) frame_mbs_only: bool,
    /// 4:2:0 at 8 bits.
    pub(crate) yuv420_8bit: bool,
}

/// The first sequence parameter set (NAL type 7) in Annex B `data`.
pub(crate) fn first_in_annex_b(data: &[u8]) -> Option<Sps> {
    let start = data
        .array_windows()
        .position(|&[a, b, c, nal]| [a, b, c] == [0, 0, 1] && nal & 0x1F == 7)?;
    let body = data.get(start.checked_add(4)?..)?;
    // To the next start code (or the end).
    let end = body
        .array_windows()
        .position(|&[a, b, c]| [a, b, c] == [0, 0, 1])
        .unwrap_or(body.len());
    parse(body.get(..end)?)
}

/// Parses a sequence parameter set's payload (the bytes after its NAL header, emulation prevention still in).
pub(crate) fn parse(payload: &[u8]) -> Option<Sps> {
    let mut bits = Bits::new(payload);
    let profile = u8::try_from(bits.read(8)?).ok()?;
    let _constraints = bits.read(8)?;
    let level = u8::try_from(bits.read(8)?).ok()?;
    let _id = bits.ue()?;
    let yuv420_8bit = if matches!(
        profile,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        high_profile_fields(&mut bits)?
    } else {
        true
    };
    let _log2_max_frame_num = bits.ue()?;
    skip_picture_order(&mut bits)?;
    let _ref_frames = bits.ue()?;
    let _gaps = bits.read(1)?;
    let width_mbs = bits.ue()?.checked_add(1)?;
    let height_units = bits.ue()?.checked_add(1)?;
    let frame_mbs_only = bits.read(1)? == 1;
    let width = width_mbs.checked_mul(16)?;
    let height = height_units
        .checked_mul(16)?
        .checked_mul(if frame_mbs_only { 1 } else { 2 })?;
    Some(Sps {
        profile,
        level,
        width,
        height,
        frame_mbs_only,
        yuv420_8bit,
    })
}

/// Reads the chroma format, bit depths and scaling matrix of a High-family profile: whether it is 4:2:0 at 8 bits.
fn high_profile_fields(bits: &mut Bits<'_>) -> Option<bool> {
    let chroma = bits.ue()?;
    if chroma == 3 {
        let _separate = bits.read(1)?;
    }
    let luma_depth = bits.ue()?;
    let chroma_depth = bits.ue()?;
    let _bypass = bits.read(1)?;
    if bits.read(1)? == 1 {
        let lists = if chroma == 3 { 12 } else { 8 };
        for list in 0..lists {
            if bits.read(1)? == 1 {
                skip_scaling_list(bits, if list < 6 { 16 } else { 64 })?;
            }
        }
    }
    Some(chroma == 1 && luma_depth == 0 && chroma_depth == 0)
}

/// Skips the picture order count fields.
fn skip_picture_order(bits: &mut Bits<'_>) -> Option<()> {
    match bits.ue()? {
        0 => {
            let _log2_max_poc = bits.ue()?;
        }
        1 => {
            let _always_zero = bits.read(1)?;
            let _non_ref = bits.se()?;
            let _top_to_bottom = bits.se()?;
            let cycle = bits.ue()?;
            if cycle > 255 {
                return None;
            }
            for _ in 0..cycle {
                let _offset = bits.se()?;
            }
        }
        _ => {}
    }
    Some(())
}

/// Skips a scaling list of `size` entries (7.3.2.1.1.1).
fn skip_scaling_list(bits: &mut Bits<'_>, size: u32) -> Option<()> {
    let (mut last, mut next) = (8i64, 8i64);
    for _ in 0..size {
        if next != 0 {
            let delta = i64::from(bits.se()?);
            next = (last + delta).rem_euclid(256);
        }
        if next != 0 {
            last = next;
        }
    }
    Some(())
}

/// A bit reader over a NAL payload that steps over emulation prevention bytes (00 00 03).
struct Bits<'a> {
    data: &'a [u8],
    /// The next byte.
    at: usize,
    /// Bits of the current byte already read.
    used: u8,
    /// Zero bytes just before `at`.
    zeros: u8,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            at: 0,
            used: 0,
            zeros: 0,
        }
    }

    fn bit(&mut self) -> Option<u32> {
        if self.used == 0 && self.zeros >= 2 && self.data.get(self.at) == Some(&3) {
            self.at = self.at.checked_add(1)?;
            self.zeros = 0;
        }
        let byte = *self.data.get(self.at)?;
        let bit = (byte >> (7 - self.used)) & 1;
        self.used += 1;
        if self.used == 8 {
            self.used = 0;
            self.at = self.at.checked_add(1)?;
            self.zeros = if byte == 0 {
                self.zeros.saturating_add(1)
            } else {
                0
            };
        }
        Some(u32::from(bit))
    }

    fn read(&mut self, count: u8) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..count.min(32) {
            value = value << 1 | self.bit()?;
        }
        Some(value)
    }

    /// An unsigned Exp-Golomb code (9.1).
    fn ue(&mut self) -> Option<u32> {
        let mut zeros = 0u8;
        while self.bit()? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        let rest = self.read(zeros)?;
        (1u32 << zeros).checked_sub(1)?.checked_add(rest)
    }

    /// A signed Exp-Golomb code (9.1.1).
    fn se(&mut self) -> Option<i32> {
        let code = self.ue()?;
        let magnitude = i32::try_from(code.div_ceil(2)).ok()?;
        Some(if code % 2 == 1 { magnitude } else { -magnitude })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sequence parameter set of a 1080i High@4.1 stream (frame_mbs_only_flag 0), with emulation prevention.
    const SPS_1080I: [u8; 41] = [
        0x64, 0x00, 0x29, 0xac, 0xd1, 0x40, 0x78, 0x04, 0x4f, 0xde, 0x02, 0x84, 0x04, 0x04, 0x05,
        0x00, 0x00, 0x03, 0x03, 0xe9, 0x00, 0x00, 0xea, 0x60, 0xe6, 0xa0, 0x00, 0x17, 0xa6, 0xb0,
        0x00, 0x0b, 0xd3, 0x5a, 0x4c, 0x30, 0x0f, 0x8b, 0x16, 0x00, 0x00,
    ];

    #[test]
    fn reads_an_interlaced_1080_sps() {
        let sps = parse(&SPS_1080I);
        assert_eq!(
            sps,
            Some(Sps {
                profile: 100,
                level: 41,
                width: 1920,
                height: 1088,
                frame_mbs_only: false,
                yuv420_8bit: true,
            })
        );
    }

    #[test]
    fn finds_the_sps_in_annex_b() {
        let mut data = vec![0, 0, 0, 1, 9, 0xF0, 0, 0, 0, 1, 0x67];
        data.extend_from_slice(&SPS_1080I[..24]);
        data.extend_from_slice(&[0, 0, 1, 0x68, 0xEE]);
        let sps = first_in_annex_b(&data);
        assert_eq!(sps.map(|sps| (sps.width, sps.height)), Some((1920, 1088)));
    }

    #[test]
    fn hostile_bytes_do_not_panic() {
        for len in 0..SPS_1080I.len() {
            let _ = parse(&SPS_1080I[..len]);
        }
        let _ = parse(&[0xFF; 64]);
        let _ = parse(&[0x00; 64]);
        let _ = first_in_annex_b(&[0, 0, 1, 7]);
    }
}
