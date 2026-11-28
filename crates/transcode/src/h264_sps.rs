//! The few fields of an H.264 sequence parameter set (ITU-T H.264 7.3.2.1.1, and its VUI parameters: E.1.1) that
//! decide whether a stream can go on a Blu-ray as it is, and that tell a stream's picture without decoding it.

/// What a sequence parameter set says about its stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sps {
    /// profile_idc (77 Main, 100 High).
    pub profile: u8,
    /// level_idc (41 is level 4.1).
    pub level: u8,
    /// Coded width, pixels (whole macroblocks).
    pub width: u32,
    /// Coded height, pixels (whole macroblocks of a frame).
    pub height: u32,
    /// Every picture is a frame of frame macroblocks (progressive coding); else fields or MBAFF may be used.
    pub frame_mbs_only: bool,
    /// 4:2:0 at 8 bits.
    pub yuv420_8bit: bool,
    /// The size shown: the coded size less its cropping (1080 lines of a coded 1088).
    pub shown: (u32, u32),
    /// The shape of a pixel (width : height), when the stream says it.
    pub pixel_aspect: Option<(u32, u32)>,
    /// `num_units_in_tick` and `time_scale`, when the stream says them: a frame lasts two ticks.
    pub timing: Option<(u32, u32)>,
    /// What a slice header's first fields depend on (M12.42, [`top_field_first_in_annex_b`]).
    pub slice: SliceSyntax,
    /// M6.556: `video_full_range_flag` (E.2.1): the samples use the whole 0–255 rather than 16–235.
    pub full_range: bool,
    /// M6.556: `matrix_coefficients` (Table E-5: 1 BT.709, 2 unspecified, 5 and 6 BT.601), when the stream says it.
    pub matrix: Option<u8>,
}

/// The fields of a sequence parameter set that a slice header's first fields depend on (7.3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SliceSyntax {
    /// `log2_max_frame_num_minus4 + 4`: the bits of `frame_num`.
    pub log2_max_frame_num: u8,
    /// `pic_order_cnt_type` (0, 1 or 2).
    pub poc_type: u8,
    /// `log2_max_pic_order_cnt_lsb_minus4 + 4` (picture order count type 0): the bits of `pic_order_cnt_lsb`.
    pub log2_max_poc_lsb: u8,
    /// `separate_colour_plane_flag` (4:4:4 coded as three planes): a slice says its `colour_plane_id`.
    pub separate_colour_plane: bool,
}

impl Sps {
    /// Frames a second, when the stream says its timing.
    #[must_use]
    pub fn frame_rate(&self) -> Option<f64> {
        let (tick, scale) = self.timing?;
        (tick > 0 && scale > 0).then(|| f64::from(scale) / (2.0 * f64::from(tick)))
    }
}

/// What follows `frame_mbs_only_flag`: the cropping and, of the VUI parameters, the pixel shape and the timing.
/// Each is `None` (the size: uncropped) when the stream leaves it out or the bytes end first.
#[derive(Default)]
struct Extras {
    crop: Option<(u32, u32, u32, u32)>,
    pixel_aspect: Option<(u32, u32)>,
    timing: Option<(u32, u32)>,
    full_range: bool,
    matrix: Option<u8>,
}

/// The pixel shapes of `aspect_ratio_idc` 1 to 16 (Table E-1).
const PIXEL_ASPECTS: [(u32, u32); 16] = [
    (1, 1),
    (12, 11),
    (10, 11),
    (16, 11),
    (40, 33),
    (24, 11),
    (20, 11),
    (32, 11),
    (80, 33),
    (18, 11),
    (15, 11),
    (64, 33),
    (160, 99),
    (4, 3),
    (3, 2),
    (2, 1),
];

/// Reads on from `frame_mbs_only_flag` (already read), filling `extras` as far as the bytes go.
fn read_extras(bits: &mut Bits<'_>, frame_mbs_only: bool, extras: &mut Extras) -> Option<()> {
    if !frame_mbs_only {
        let _mbaff = bits.read(1)?;
    }
    let _direct_8x8 = bits.read(1)?;
    if bits.read(1)? == 1 {
        extras.crop = Some((bits.ue()?, bits.ue()?, bits.ue()?, bits.ue()?));
    }
    if bits.read(1)? == 0 {
        return Some(());
    }
    // VUI parameters.
    if bits.read(1)? == 1 {
        let idc = bits.read(8)?;
        extras.pixel_aspect = if idc == 255 {
            Some((bits.read(16)?, bits.read(16)?)).filter(|(w, h)| *w > 0 && *h > 0)
        } else {
            usize::try_from(idc)
                .ok()
                .and_then(|idc| idc.checked_sub(1))
                .and_then(|index| PIXEL_ASPECTS.get(index).copied())
        };
    }
    if bits.read(1)? == 1 {
        let _overscan_appropriate = bits.read(1)?;
    }
    if bits.read(1)? == 1 {
        // video_format (3 bits), video_full_range_flag.
        extras.full_range = bits.read(4)? & 1 == 1;
        if bits.read(1)? == 1 {
            // colour_primaries, transfer_characteristics, matrix_coefficients.
            extras.matrix = u8::try_from(bits.read(24)? & 0xFF).ok();
        }
    }
    if bits.read(1)? == 1 {
        let _top = bits.ue()?;
        let _bottom = bits.ue()?;
    }
    if bits.read(1)? == 1 {
        extras.timing = Some((bits.read(32)?, bits.read(32)?));
    }
    Some(())
}

/// The first sequence parameter set (NAL type 7) in Annex B `data`.
pub fn first_in_annex_b(data: &[u8]) -> Option<Sps> {
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
pub fn parse(payload: &[u8]) -> Option<Sps> {
    let mut bits = Bits::new(payload);
    let profile = u8::try_from(bits.read(8)?).ok()?;
    let _constraints = bits.read(8)?;
    let level = u8::try_from(bits.read(8)?).ok()?;
    let _id = bits.ue()?;
    let (yuv420_8bit, separate_colour_plane) = if matches!(
        profile,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        high_profile_fields(&mut bits)?
    } else {
        (true, false)
    };
    let log2_max_frame_num = u8::try_from(bits.ue()?.checked_add(4)?).ok()?;
    let (poc_type, log2_max_poc_lsb) = picture_order(&mut bits)?;
    let _ref_frames = bits.ue()?;
    let _gaps = bits.read(1)?;
    let width_mbs = bits.ue()?.checked_add(1)?;
    let height_units = bits.ue()?.checked_add(1)?;
    let frame_mbs_only = bits.read(1)? == 1;
    let width = width_mbs.checked_mul(16)?;
    let height = height_units
        .checked_mul(16)?
        .checked_mul(if frame_mbs_only { 1 } else { 2 })?;
    let mut extras = Extras::default();
    let _ = read_extras(&mut bits, frame_mbs_only, &mut extras);
    // Cropping counts chroma samples: two pixels across, and two lines (four of an interlaced frame) down, in
    // 4:2:0. Other chroma formats are left uncropped.
    let shown = extras
        .crop
        .filter(|_| yuv420_8bit)
        .and_then(|(left, right, top, bottom)| {
            let lines = if frame_mbs_only { 2 } else { 4 };
            let across = left.checked_add(right)?.checked_mul(2)?;
            let down = top.checked_add(bottom)?.checked_mul(lines)?;
            Some((
                width.checked_sub(across).filter(|shown| *shown > 0)?,
                height.checked_sub(down).filter(|shown| *shown > 0)?,
            ))
        })
        .unwrap_or((width, height));
    Some(Sps {
        profile,
        level,
        width,
        height,
        frame_mbs_only,
        yuv420_8bit,
        shown,
        pixel_aspect: extras.pixel_aspect,
        timing: extras.timing,
        slice: SliceSyntax {
            log2_max_frame_num,
            poc_type,
            log2_max_poc_lsb,
            separate_colour_plane,
        },
        full_range: extras.full_range,
        matrix: extras.matrix,
    })
}

/// Reads the chroma format, bit depths and scaling matrix of a High-family profile: whether it is 4:2:0 at 8 bits,
/// and whether its colour planes are coded apart.
fn high_profile_fields(bits: &mut Bits<'_>) -> Option<(bool, bool)> {
    let chroma = bits.ue()?;
    let separate = chroma == 3 && bits.read(1)? == 1;
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
    Some((
        chroma == 1 && luma_depth == 0 && chroma_depth == 0,
        separate,
    ))
}

/// Reads the picture order count fields: the type, and (type 0) the bits of `pic_order_cnt_lsb`.
fn picture_order(bits: &mut Bits<'_>) -> Option<(u8, u8)> {
    let kind = bits.ue()?;
    let mut lsb_bits = 0;
    match kind {
        0 => {
            lsb_bits = u8::try_from(bits.ue()?.checked_add(4)?).ok()?;
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
    Some((u8::try_from(kind).ok()?, lsb_bits))
}

/// The NAL units of Annex B `data`: each one's type and payload (the bytes after its header, to the next start
/// code).
fn nal_units(data: &[u8]) -> impl Iterator<Item = (u8, &[u8])> + '_ {
    let starts: Vec<usize> = data
        .array_windows()
        .enumerate()
        .filter(|(_, window)| **window == [0, 0, 1])
        .map(|(at, _)| at)
        .collect();
    (0..starts.len()).filter_map(move |index| {
        let header = starts.get(index)?.checked_add(3)?;
        let end = starts.get(index + 1).copied().unwrap_or(data.len());
        let kind = data.get(header)? & 0x1F;
        Some((kind, data.get(header.checked_add(1)?..end)?))
    })
}

/// Whether the first picture of interlaced Annex B `data` (with its sequence parameter set `sps`) shows its top
/// field first (M12.42): a field picture's `bottom_field_flag` (the field coded first), or a frame's
/// `delta_pic_order_cnt_bottom` (7.3.3; 8.2.1: the bottom field's order count less the top field's, negative when
/// the bottom field comes first), the picture parameter set saying whether frames carry it (7.3.2.2). `None` when
/// the stream does not say so this way (no picture parameter set or slice in `data`, a picture order count of type
/// 1 or 2, a frame whose two fields have the same order count).
pub fn top_field_first_in_annex_b(data: &[u8], sps: &Sps) -> Option<bool> {
    // Each picture parameter set's id and whether frames say their bottom field's order count.
    let mut parameter_sets: Vec<(u32, bool)> = Vec::new();
    for (kind, payload) in nal_units(data) {
        match kind {
            8 => {
                let mut bits = Bits::new(payload);
                let id = bits.ue()?;
                let _sps_id = bits.ue()?;
                let _entropy = bits.read(1)?;
                let bottom_present = bits.read(1)? == 1;
                parameter_sets.push((id, bottom_present));
            }
            1 | 5 => return slice_top_field_first(payload, kind == 5, sps, &parameter_sets),
            _ => {}
        }
    }
    None
}

/// [`top_field_first_in_annex_b`] from a slice header's first fields (7.3.3).
fn slice_top_field_first(
    payload: &[u8],
    idr: bool,
    sps: &Sps,
    parameter_sets: &[(u32, bool)],
) -> Option<bool> {
    let syntax = sps.slice;
    let mut bits = Bits::new(payload);
    let _first_mb = bits.ue()?;
    let _slice_type = bits.ue()?;
    let pps_id = bits.ue()?;
    if syntax.separate_colour_plane {
        let _plane = bits.read(2)?;
    }
    let _frame_num = bits.read(syntax.log2_max_frame_num)?;
    let mut field_picture = false;
    if !sps.frame_mbs_only {
        field_picture = bits.read(1)? == 1;
        if field_picture {
            return Some(bits.read(1)? == 0);
        }
    }
    if idr {
        let _idr_pic_id = bits.ue()?;
    }
    let (_, bottom_present) = parameter_sets.iter().find(|(id, _)| *id == pps_id)?;
    if syntax.poc_type != 0 || !*bottom_present || field_picture {
        return None;
    }
    let _poc_lsb = bits.read(syntax.log2_max_poc_lsb)?;
    let delta = bits.se()?;
    (delta != 0).then_some(delta > 0)
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
                shown: (1920, 1080),
                pixel_aspect: Some((1, 1)),
                timing: Some((1001, 60_000)),
                slice: SliceSyntax {
                    log2_max_frame_num: 4,
                    poc_type: 0,
                    log2_max_poc_lsb: 5,
                    separate_colour_plane: false,
                },
                full_range: false,
                matrix: Some(1),
            })
        );
        let rate = sps.and_then(|sps| sps.frame_rate()).unwrap_or_default();
        assert!((rate - 30_000.0 / 1001.0).abs() < 1e-9, "{rate}");
    }

    /// M6.556: a 1080p High@4.1 stream's sequence parameter set (libx264, 23.976) that says full range and the
    /// BT.601 matrix (6, SMPTE 170M) in its VUI.
    const SPS_1080P_FULL_601: [u8; 28] = [
        0x64, 0x00, 0x29, 0xac, 0xd9, 0x40, 0x78, 0x02, 0x27, 0xe5, 0xc0, 0x5b, 0x81, 0x01, 0x03,
        0x20, 0x00, 0x00, 0x7d, 0x20, 0x00, 0x17, 0x70, 0x01, 0xe3, 0x06, 0x32, 0xc0,
    ];

    /// M6.556: the VUI's video signal type is read: full range and the matrix.
    #[test]
    fn reads_the_range_and_matrix() {
        let sps = parse(&SPS_1080P_FULL_601);
        assert_eq!(
            sps.map(|sps| (sps.width, sps.shown, sps.full_range, sps.matrix)),
            Some((1920, (1920, 1080), true, Some(6)))
        );
    }

    #[test]
    fn finds_the_sps_in_annex_b() {
        let mut data = vec![0, 0, 0, 1, 9, 0xF0, 0, 0, 0, 1, 0x67];
        data.extend_from_slice(SPS_1080I.get(..24).unwrap_or_default());
        data.extend_from_slice(&[0, 0, 1, 0x68, 0xEE]);
        let sps = first_in_annex_b(&data);
        assert_eq!(sps.map(|sps| (sps.width, sps.height)), Some((1920, 1088)));
    }

    /// Bytes from a string of bits (spaces ignored), the last byte padded with zeros.
    fn bits(text: &str) -> Vec<u8> {
        let digits: Vec<u8> = text
            .bytes()
            .filter(|b| *b != b' ')
            .map(|b| b - b'0')
            .collect();
        digits
            .chunks(8)
            .map(|chunk| {
                chunk
                    .iter()
                    .chain(std::iter::repeat(&0))
                    .take(8)
                    .fold(0u8, |byte, bit| byte << 1 | bit)
            })
            .collect()
    }

    /// An Annex B stream: the 1080i SPS, a PPS (`bottom_present`: frames say their bottom field's order count), and
    /// an IDR slice whose header continues with `slice_rest` (after `first_mb`, `slice_type` 7, `pps_id` 0 and a
    /// four-bit `frame_num`).
    fn stream(bottom_present: bool, slice_rest: &str) -> Vec<u8> {
        let mut data = vec![0, 0, 0, 1, 0x67];
        data.extend_from_slice(&SPS_1080I);
        data.extend_from_slice(&[0, 0, 0, 1, 0x68]);
        data.extend(bits(&format!("1 1 0 {} 1", u8::from(bottom_present))));
        data.extend_from_slice(&[0, 0, 0, 1, 0x65]);
        data.extend(bits(&format!("1 0001000 1 0000 {slice_rest} 1")));
        data
    }

    /// M12.42: the first picture says which field comes first: a frame by the sign of `delta_pic_order_cnt_bottom`
    /// (when the PPS says frames carry it), a field picture by `bottom_field_flag`.
    #[test]
    fn reads_which_field_comes_first() {
        let sps = parse(&SPS_1080I).expect("the 1080i SPS");
        // Frames (field_pic_flag 0), idr_pic_id 0, a five-bit pic_order_cnt_lsb, then se(v) -1 or +1.
        assert_eq!(
            top_field_first_in_annex_b(&stream(true, "0 1 00000 011"), &sps),
            Some(false)
        );
        assert_eq!(
            top_field_first_in_annex_b(&stream(true, "0 1 00000 010"), &sps),
            Some(true)
        );
        assert_eq!(
            top_field_first_in_annex_b(&stream(true, "0 1 00000 1"), &sps),
            None
        );
        assert_eq!(
            top_field_first_in_annex_b(&stream(false, "0 1 00000"), &sps),
            None
        );
        // Field pictures: the bottom field coded first, or the top.
        assert_eq!(
            top_field_first_in_annex_b(&stream(false, "1 1"), &sps),
            Some(false)
        );
        assert_eq!(
            top_field_first_in_annex_b(&stream(false, "1 0"), &sps),
            Some(true)
        );
        // Cut anywhere, or nothing like a stream: no answer, no panic.
        let whole = stream(true, "0 1 00000 011");
        for len in 0..whole.len() {
            let _ = top_field_first_in_annex_b(whole.get(..len).unwrap_or_default(), &sps);
        }
        assert_eq!(top_field_first_in_annex_b(&[0xFF; 64], &sps), None);
        assert_eq!(
            top_field_first_in_annex_b(&[0, 0, 1, 0x65, 0, 0, 1], &sps),
            None
        );
    }

    #[test]
    fn hostile_bytes_do_not_panic() {
        for len in 0..SPS_1080I.len() {
            let _ = parse(SPS_1080I.get(..len).unwrap_or_default());
        }
        let _ = parse(&[0xFF; 64]);
        let _ = parse(&[0x00; 64]);
        let _ = first_in_annex_b(&[0, 0, 1, 7]);
    }
}
