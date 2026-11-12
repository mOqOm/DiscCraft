//! MPEG-2 P-picture variable-length codes and bitstream helpers.
#![allow(dead_code)]

use crate::bitwriter::BitWriter;

/// A variable-length code: bits and length.
pub(crate) type VlcCode = (u32, u8);

/// P-picture macroblock type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PType {
    /// Motion compensated, coded.
    McCoded,
    /// No motion compensation, coded.
    NoMcCoded,
    /// Motion compensated, not coded.
    McNotCoded,
    /// Intra macroblock.
    Intra,
}

const MACROBLOCK_ADDRESS_INCREMENT: [(u8, u32, u8); 33] = [
    (1, 0b1, 1),
    (2, 0b011, 3),
    (3, 0b010, 3),
    (4, 0b0011, 4),
    (5, 0b0010, 4),
    (6, 0b00011, 5),
    (7, 0b00010, 5),
    (8, 0b0000111, 7),
    (9, 0b0000110, 7),
    (10, 0b00001011, 8),
    (11, 0b00001010, 8),
    (12, 0b00001001, 8),
    (13, 0b00001000, 8),
    (14, 0b00000111, 8),
    (15, 0b00000110, 8),
    (16, 0b0000010111, 10),
    (17, 0b0000010110, 10),
    (18, 0b0000010101, 10),
    (19, 0b0000010100, 10),
    (20, 0b0000010011, 10),
    (21, 0b0000010010, 10),
    (22, 0b00000100011, 11),
    (23, 0b00000100010, 11),
    (24, 0b00000100001, 11),
    (25, 0b00000100000, 11),
    (26, 0b00000011111, 11),
    (27, 0b00000011110, 11),
    (28, 0b00000011101, 11),
    (29, 0b00000011100, 11),
    (30, 0b00000011011, 11),
    (31, 0b00000011010, 11),
    (32, 0b00000011001, 11),
    (33, 0b00000011000, 11),
];

const CODED_BLOCK_PATTERN: [(u8, u32, u8); 64] = [
    (60, 0b111, 3),
    (4, 0b1101, 4),
    (8, 0b1100, 4),
    (16, 0b1011, 4),
    (32, 0b1010, 4),
    (12, 0b10011, 5),
    (48, 0b10010, 5),
    (20, 0b10001, 5),
    (40, 0b10000, 5),
    (28, 0b01111, 5),
    (44, 0b01110, 5),
    (52, 0b01101, 5),
    (56, 0b01100, 5),
    (1, 0b01011, 5),
    (61, 0b01010, 5),
    (2, 0b01001, 5),
    (62, 0b01000, 5),
    (24, 0b001111, 6),
    (36, 0b001110, 6),
    (3, 0b001101, 6),
    (63, 0b001100, 6),
    (5, 0b0010111, 7),
    (9, 0b0010110, 7),
    (17, 0b0010101, 7),
    (33, 0b0010100, 7),
    (6, 0b0010011, 7),
    (10, 0b0010010, 7),
    (18, 0b0010001, 7),
    (34, 0b0010000, 7),
    (7, 0b00011111, 8),
    (11, 0b00011110, 8),
    (19, 0b00011101, 8),
    (35, 0b00011100, 8),
    (13, 0b00011011, 8),
    (49, 0b00011010, 8),
    (21, 0b00011001, 8),
    (41, 0b00011000, 8),
    (14, 0b00010111, 8),
    (50, 0b00010110, 8),
    (22, 0b00010101, 8),
    (42, 0b00010100, 8),
    (15, 0b00010011, 8),
    (51, 0b00010010, 8),
    (23, 0b00010001, 8),
    (43, 0b00010000, 8),
    (25, 0b00001111, 8),
    (37, 0b00001110, 8),
    (26, 0b00001101, 8),
    (38, 0b00001100, 8),
    (29, 0b00001011, 8),
    (45, 0b00001010, 8),
    (53, 0b00001001, 8),
    (57, 0b00001000, 8),
    (30, 0b00000111, 8),
    (46, 0b00000110, 8),
    (54, 0b00000101, 8),
    (58, 0b00000100, 8),
    (31, 0b000000111, 9),
    (47, 0b000000110, 9),
    (55, 0b000000101, 9),
    (59, 0b000000100, 9),
    (27, 0b000000011, 9),
    (39, 0b000000010, 9),
    (0, 0b000000001, 9),
];

const MOTION_PREFIX: [(u8, u32, u8); 16] = [
    (1, 0b01, 2),
    (2, 0b001, 3),
    (3, 0b0001, 4),
    (4, 0b000011, 6),
    (5, 0b0000101, 7),
    (6, 0b0000100, 7),
    (7, 0b0000011, 7),
    (8, 0b000001011, 9),
    (9, 0b000001010, 9),
    (10, 0b000001001, 9),
    (11, 0b0000010001, 10),
    (12, 0b0000010000, 10),
    (13, 0b0000001111, 10),
    (14, 0b0000001110, 10),
    (15, 0b0000001101, 10),
    (16, 0b0000001100, 10),
];

const LUMA_DC_SIZE: [(u8, u32, u8); 12] = [
    (0, 0b100, 3),
    (1, 0b00, 2),
    (2, 0b01, 2),
    (3, 0b101, 3),
    (4, 0b110, 3),
    (5, 0b1110, 4),
    (6, 0b11110, 5),
    (7, 0b111110, 6),
    (8, 0b1111110, 7),
    (9, 0b11111110, 8),
    (10, 0b111111110, 9),
    (11, 0b111111111, 9),
];

const CHROMA_DC_SIZE: [(u8, u32, u8); 12] = [
    (0, 0b00, 2),
    (1, 0b01, 2),
    (2, 0b10, 2),
    (3, 0b110, 3),
    (4, 0b1110, 4),
    (5, 0b11110, 5),
    (6, 0b111110, 6),
    (7, 0b1111110, 7),
    (8, 0b11111110, 8),
    (9, 0b111111110, 9),
    (10, 0b1111111110, 10),
    (11, 0b1111111111, 10),
];

/// Return the macroblock address increment code for `n` in 1..=33.
pub(crate) fn macroblock_address_increment(n: u8) -> Option<VlcCode> {
    lookup(&MACROBLOCK_ADDRESS_INCREMENT, n)
}

/// Return the P-picture macroblock type code.
pub(crate) fn p_macroblock_type(kind: PType) -> VlcCode {
    match kind {
        PType::McCoded => (1, 1),
        PType::NoMcCoded => (0b01, 2),
        PType::McNotCoded => (0b001, 3),
        PType::Intra => (0b00011, 5),
    }
}

/// Return the coded block pattern code for `cbp` in 0..=63.
pub(crate) fn coded_block_pattern(cbp: u8) -> Option<VlcCode> {
    lookup(&CODED_BLOCK_PATTERN, cbp)
}

/// Return the motion code for `m` in -16..=16.
pub(crate) fn motion_code(m: i8) -> Option<VlcCode> {
    if m == 0 {
        return Some((1, 1));
    }

    let magnitude = m.unsigned_abs();
    if magnitude > 16 {
        return None;
    }

    let (prefix, len) = lookup(&MOTION_PREFIX, magnitude)?;
    let sign = if m < 0 { 1 } else { 0 };
    Some(((prefix << 1) | sign, len.saturating_add(1)))
}

/// Return the luma DCT DC size code for `size` in 0..=11.
pub(crate) fn dct_dc_size_luma(size: u8) -> Option<VlcCode> {
    lookup(&LUMA_DC_SIZE, size)
}

/// Return the chroma DCT DC size code for `size` in 0..=11.
pub(crate) fn dct_dc_size_chroma(size: u8) -> Option<VlcCode> {
    lookup(&CHROMA_DC_SIZE, size)
}

/// Write a macroblock address increment code.
pub(crate) fn write_macroblock_address_increment(writer: &mut BitWriter, n: u8) {
    if let Some(code) = macroblock_address_increment(n) {
        writer.write_bits(code.0, code.1);
    }
}

/// Write a P-picture macroblock type code.
pub(crate) fn write_p_macroblock_type(writer: &mut BitWriter, kind: PType) {
    let code = p_macroblock_type(kind);
    writer.write_bits(code.0, code.1);
}

/// Write a coded block pattern code.
pub(crate) fn write_coded_block_pattern(writer: &mut BitWriter, cbp: u8) {
    if let Some(code) = coded_block_pattern(cbp) {
        writer.write_bits(code.0, code.1);
    }
}

/// Write a motion code.
pub(crate) fn write_motion_code(writer: &mut BitWriter, m: i8) {
    if let Some(code) = motion_code(m) {
        writer.write_bits(code.0, code.1);
    }
}

/// Write a motion delta using the MPEG-2 motion vector range code.
pub(crate) fn write_motion_delta(writer: &mut BitWriter, delta: i32, f_code: u8) {
    let Some(r_size) = motion_residual_size(f_code) else {
        return;
    };

    if delta == 0 {
        write_motion_code(writer, 0);
        return;
    }

    let abs = delta.unsigned_abs().saturating_sub(1);
    let magnitude = (abs >> r_size) + 1;
    let magnitude = if magnitude > 16 { 16 } else { magnitude };

    write_motion_code_magnitude(writer, magnitude, delta < 0);

    if r_size > 0 {
        let mask = (1u32 << r_size) - 1;
        let residual = abs & mask;
        writer.write_bits(residual, u8::try_from(r_size).unwrap_or(0));
    }
}

/// Wrap a motion delta into the MPEG-2 range for `f_code`.
pub(crate) fn wrap_motion_delta(delta: i32, f_code: u8) -> i32 {
    let Some(f) = motion_factor(f_code) else {
        return 0;
    };

    let modulus = i64::from(f) * 32;
    let max = i64::from(f) * 16 - 1;
    let mut rem = i64::from(delta) % modulus;

    if rem < 0 {
        rem += modulus;
    }

    if rem > max {
        rem -= modulus;
    }

    i32::try_from(rem).unwrap_or(0)
}

/// Write an escape coefficient: `000001`, run, and 12-bit two's-complement level.
pub(crate) fn write_escape_coefficient(writer: &mut BitWriter, run: u8, level: i32) {
    writer.write_bits(0b000001, 6);
    writer.write_bits(u32::from(run & 0x3F), 6);
    writer.write_bits(level_to_twos_complement_12(level), 12);
}

/// Write the end-of-block code.
pub(crate) fn write_end_of_block(writer: &mut BitWriter) {
    writer.write_bits(0b10, 2);
}

fn lookup(table: &[(u8, u32, u8)], key: u8) -> Option<VlcCode> {
    for &(entry_key, bits, len) in table {
        if entry_key == key {
            return Some((bits, len));
        }
    }

    None
}

fn motion_residual_size(f_code: u8) -> Option<u32> {
    if f_code == 0 || f_code > 10 {
        return None;
    }

    Some(u32::from(f_code - 1))
}

fn motion_factor(f_code: u8) -> Option<u32> {
    let r_size = motion_residual_size(f_code)?;
    Some(1u32 << r_size)
}

fn write_motion_code_magnitude(writer: &mut BitWriter, magnitude: u32, negative: bool) {
    if magnitude == 0 {
        write_motion_code(writer, 0);
        return;
    }

    let value = i8::try_from(magnitude).unwrap_or(16);
    let signed = if negative { -value } else { value };
    write_motion_code(writer, signed);
}

fn level_to_twos_complement_12(level: i32) -> u32 {
    if level < 0 {
        let abs = level.unsigned_abs();
        (1u32 << 12).wrapping_sub(abs) & 0xFFF
    } else {
        u32::try_from(level).unwrap_or(0) & 0xFFF
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PType, VlcCode, coded_block_pattern, dct_dc_size_chroma, dct_dc_size_luma,
        macroblock_address_increment, motion_code, p_macroblock_type, wrap_motion_delta,
        write_end_of_block, write_escape_coefficient, write_motion_delta,
    };
    use crate::bitwriter::BitWriter;

    fn is_prefix(short: VlcCode, long: VlcCode) -> bool {
        let (a, la) = short;
        let (b, lb) = long;

        if la > lb {
            return false;
        }

        let shift = u32::from(lb - la);
        (b >> shift) == a
    }

    fn codes_are_prefix_free(codes: &[VlcCode]) {
        for (i, &a) in codes.iter().enumerate() {
            for (j, &b) in codes.iter().enumerate() {
                if i != j {
                    assert!(!is_prefix(a, b));
                }
            }
        }
    }

    fn macroblock_codes() -> Vec<VlcCode> {
        let mut codes = Vec::new();
        for n in 1u8..=33 {
            if let Some(code) = macroblock_address_increment(n) {
                codes.push(code);
            }
        }
        codes
    }

    fn coded_block_pattern_codes() -> Vec<VlcCode> {
        let mut codes = Vec::new();
        for cbp in 0u8..=63 {
            if let Some(code) = coded_block_pattern(cbp) {
                codes.push(code);
            }
        }
        codes
    }

    fn motion_codes() -> Vec<VlcCode> {
        let mut codes = Vec::new();
        for m in -16i8..=16 {
            if let Some(code) = motion_code(m) {
                codes.push(code);
            }
        }
        codes
    }

    fn p_macroblock_type_codes() -> Vec<VlcCode> {
        vec![
            p_macroblock_type(PType::McCoded),
            p_macroblock_type(PType::NoMcCoded),
            p_macroblock_type(PType::McNotCoded),
            p_macroblock_type(PType::Intra),
        ]
    }

    fn luma_dc_codes() -> Vec<VlcCode> {
        let mut codes = Vec::new();
        for size in 0u8..=11 {
            if let Some(code) = dct_dc_size_luma(size) {
                codes.push(code);
            }
        }
        codes
    }

    fn chroma_dc_codes() -> Vec<VlcCode> {
        let mut codes = Vec::new();
        for size in 0u8..=11 {
            if let Some(code) = dct_dc_size_chroma(size) {
                codes.push(code);
            }
        }
        codes
    }

    fn write_delta(delta: i32, f_code: u8) -> Vec<u8> {
        let mut writer = BitWriter::new();
        write_motion_delta(&mut writer, delta, f_code);
        writer.finish()
    }

    fn write_escape(run: u8, level: i32) -> Vec<u8> {
        let mut writer = BitWriter::new();
        write_escape_coefficient(&mut writer, run, level);
        writer.finish()
    }

    #[test]
    fn tables_have_expected_sizes() {
        assert_eq!(macroblock_codes().len(), 33);
        assert_eq!(coded_block_pattern_codes().len(), 64);
        assert_eq!(motion_codes().len(), 33);
        assert_eq!(p_macroblock_type_codes().len(), 4);
        assert_eq!(luma_dc_codes().len(), 12);
        assert_eq!(chroma_dc_codes().len(), 12);
    }

    #[test]
    fn tables_are_prefix_free() {
        codes_are_prefix_free(&macroblock_codes());
        codes_are_prefix_free(&coded_block_pattern_codes());
        codes_are_prefix_free(&motion_codes());
        codes_are_prefix_free(&p_macroblock_type_codes());
        codes_are_prefix_free(&luma_dc_codes());
        codes_are_prefix_free(&chroma_dc_codes());
    }

    #[test]
    fn exact_table_values() {
        assert_eq!(macroblock_address_increment(1), Some((1, 1)));
        assert_eq!(macroblock_address_increment(2), Some((0b011, 3)));
        assert_eq!(macroblock_address_increment(33), Some((0b00000011000, 11)));

        assert_eq!(p_macroblock_type(PType::McCoded), (1, 1));
        assert_eq!(p_macroblock_type(PType::NoMcCoded), (0b01, 2));
        assert_eq!(p_macroblock_type(PType::McNotCoded), (0b001, 3));
        assert_eq!(p_macroblock_type(PType::Intra), (0b00011, 5));

        assert_eq!(coded_block_pattern(60), Some((0b111, 3)));
        assert_eq!(coded_block_pattern(0), Some((0b000000001, 9)));
        assert_eq!(coded_block_pattern(63), Some((0b001100, 6)));

        assert_eq!(motion_code(0), Some((1, 1)));
        assert_eq!(motion_code(1), Some((0b010, 3)));
        assert_eq!(motion_code(-1), Some((0b011, 3)));
        assert_eq!(motion_code(3), Some((0b00010, 5)));
        assert_eq!(motion_code(-3), Some((0b00011, 5)));

        assert_eq!(dct_dc_size_luma(0), Some((0b100, 3)));
        assert_eq!(dct_dc_size_luma(11), Some((0b111111111, 9)));
        assert_eq!(dct_dc_size_chroma(0), Some((0b00, 2)));
        assert_eq!(dct_dc_size_chroma(11), Some((0b1111111111, 10)));
    }

    #[test]
    fn motion_delta_f_code_one() {
        assert_eq!(write_delta(0, 1), vec![0x80]);
        assert_eq!(write_delta(1, 1), vec![0x40]);
        assert_eq!(write_delta(-1, 1), vec![0x60]);
    }

    #[test]
    fn motion_delta_f_code_two() {
        assert_eq!(write_delta(5, 2), vec![0x10]);
        assert_eq!(write_delta(-1, 2), vec![0x60]);
    }

    #[test]
    fn motion_delta_wrapping() {
        assert_eq!(wrap_motion_delta(16, 1), -16);
        assert_eq!(wrap_motion_delta(-17, 1), 15);
        assert_eq!(wrap_motion_delta(32, 2), -32);
        assert_eq!(wrap_motion_delta(-33, 2), 31);
    }

    #[test]
    fn escape_coefficient_exact_values() {
        assert_eq!(write_escape(0, 1), vec![0x04, 0x00, 0x01]);
        // 000001 000000 111111111111
        assert_eq!(write_escape(0, -1), vec![0x04, 0x0F, 0xFF]);
    }

    #[test]
    fn end_of_block_exact_value() {
        let mut writer = BitWriter::new();
        write_end_of_block(&mut writer);
        assert_eq!(writer.finish(), vec![0x80]);
    }
}
