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

/// Write the address increment of a macroblock that follows `skipped` skipped ones: a `macroblock_escape` for each
/// 33, then the code of the rest plus one.
pub(crate) fn write_macroblock_address_increment_after(writer: &mut BitWriter, skipped: u32) {
    let mut increment = skipped.saturating_add(1);
    while increment > 33 {
        writer.write_bits(8, 11); // macroblock_escape: 0000 0001 000
        increment -= 33;
    }
    write_macroblock_address_increment(writer, u8::try_from(increment).unwrap_or(1));
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

/// Which DCT coefficient table codes a block's coefficients (H.262 7.2.2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DctTable {
    /// B.14, table zero: non-intra blocks, and intra blocks when `intra_vlc_format` is 0.
    Zero,
    /// B.15, table one: intra blocks when `intra_vlc_format` is 1.
    One,
}

/// Write one coefficient (`run` zeros, then `level`): its run/level code and sign bit when the table has one, else
/// an escape. `first` marks the first coefficient of a non-intra block, where table zero codes run 0 level ±1 as
/// `1s`.
pub(crate) fn write_coefficient(
    writer: &mut BitWriter,
    table: DctTable,
    run: u8,
    level: i32,
    first: bool,
) {
    let magnitude = level.unsigned_abs();
    let sign = u32::from(level < 0);
    if first && table == DctTable::Zero && run == 0 && magnitude == 1 {
        writer.write_bits(0b10 | sign, 2);
        return;
    }
    match run_level_code(table, run, magnitude) {
        Some((bits, len)) => {
            writer.write_bits(bits, len);
            writer.write_bits(sign, 1);
        }
        None => write_escape_coefficient(writer, run, level),
    }
}

/// Write the end-of-block code of `table`.
pub(crate) fn write_end_of_block_in(writer: &mut BitWriter, table: DctTable) {
    match table {
        DctTable::Zero => writer.write_bits(0b10, 2),
        DctTable::One => writer.write_bits(0b0110, 4),
    }
}

/// The run/level code (sign bit excluded) of `table`, if it has one.
pub(crate) fn run_level_code(table: DctTable, run: u8, level: u32) -> Option<VlcCode> {
    let tables = dct_tables();
    let codes = match table {
        DctTable::Zero => &tables.zero,
        DctTable::One => &tables.one,
    };
    let level = usize::try_from(level).ok()?;
    codes.get(usize::from(run))?.get(level).copied().flatten()
}

/// Longest run, and largest level, that the tables code.
const DCT_MAX_RUN: usize = 31;
const DCT_MAX_LEVEL: usize = 40;

type RunLevelCodes = [[Option<VlcCode>; DCT_MAX_LEVEL + 1]; DCT_MAX_RUN + 1];

struct DctTables {
    zero: RunLevelCodes,
    one: RunLevelCodes,
}

static DCT_TABLES: std::sync::OnceLock<DctTables> = std::sync::OnceLock::new();

fn dct_tables() -> &'static DctTables {
    DCT_TABLES.get_or_init(|| {
        let build = |lists: &[&[(&str, u8, u8)]]| {
            let mut codes: RunLevelCodes = [[None; DCT_MAX_LEVEL + 1]; DCT_MAX_RUN + 1];
            for list in lists {
                for &(code, run, level) in *list {
                    if let Some(slot) = codes
                        .get_mut(usize::from(run))
                        .and_then(|levels| levels.get_mut(usize::from(level)))
                    {
                        *slot = Some(parse_code(code));
                    }
                }
            }
            codes
        };
        DctTables {
            zero: build(&[DCT_ZERO, DCT_ZERO_RUN0_LONG, DCT_MID_SHARED, DCT_LONG]),
            one: build(&[DCT_ONE, DCT_MID_SHARED, DCT_LONG]),
        }
    })
}

/// A code written as a string of `0` and `1`, spaces ignored.
fn parse_code(code: &str) -> VlcCode {
    code.bytes()
        .filter(|byte| *byte != b' ')
        .fold((0u32, 0u8), |(bits, len), byte| {
            ((bits << 1) | u32::from(byte == b'1'), len.saturating_add(1))
        })
}

/// B.14, table zero: (code without the sign bit, run, level). End of block (`10`), escape (`0000 01`) and the `1s`
/// first coefficient are written separately.
const DCT_ZERO: &[(&str, u8, u8)] = &[
    ("11", 0, 1),
    ("011", 1, 1),
    ("0100", 0, 2),
    ("0101", 2, 1),
    ("0010 1", 0, 3),
    ("0011 1", 3, 1),
    ("0011 0", 4, 1),
    ("0001 10", 1, 2),
    ("0001 11", 5, 1),
    ("0001 01", 6, 1),
    ("0001 00", 7, 1),
    ("0000 110", 0, 4),
    ("0000 100", 2, 2),
    ("0000 111", 8, 1),
    ("0000 101", 9, 1),
    ("0010 0110", 0, 5),
    ("0010 0001", 0, 6),
    ("0010 0101", 1, 3),
    ("0010 0100", 3, 2),
    ("0010 0111", 10, 1),
    ("0010 0011", 11, 1),
    ("0010 0010", 12, 1),
    ("0010 0000", 13, 1),
    ("0000 0010 10", 0, 7),
    ("0000 0011 00", 1, 4),
    ("0000 0010 11", 2, 3),
    ("0000 0011 11", 4, 2),
    ("0000 0010 01", 5, 2),
    ("0000 0011 10", 14, 1),
    ("0000 0011 01", 15, 1),
    ("0000 0010 00", 16, 1),
    ("0000 0001 1101", 0, 8),
    ("0000 0001 1000", 0, 9),
    ("0000 0001 0011", 0, 10),
    ("0000 0001 0000", 0, 11),
    ("0000 0001 1011", 1, 5),
    ("0000 0001 0100", 2, 4),
];

/// Table zero's codes for run 0, levels 12-15 (table one has shorter ones of its own).
const DCT_ZERO_RUN0_LONG: &[(&str, u8, u8)] = &[
    ("0000 0000 1101 0", 0, 12),
    ("0000 0000 1100 1", 0, 13),
    ("0000 0000 1100 0", 0, 14),
    ("0000 0000 1011 1", 0, 15),
];

/// B.15, table one: (code without the sign bit, run, level). End of block is `0110`, escape `0000 01`.
const DCT_ONE: &[(&str, u8, u8)] = &[
    ("10", 0, 1),
    ("010", 1, 1),
    ("110", 0, 2),
    ("0010 1", 2, 1),
    ("0111", 0, 3),
    ("0011 1", 3, 1),
    ("0001 10", 4, 1),
    ("0011 0", 1, 2),
    ("0001 11", 5, 1),
    ("0000 110", 6, 1),
    ("0000 100", 7, 1),
    ("1110 0", 0, 4),
    ("0000 111", 2, 2),
    ("0000 101", 8, 1),
    ("1111 000", 9, 1),
    ("1110 1", 0, 5),
    ("0001 01", 0, 6),
    ("1111 001", 1, 3),
    ("0010 0110", 3, 2),
    ("1111 010", 10, 1),
    ("0010 0001", 11, 1),
    ("0010 0101", 12, 1),
    ("0010 0100", 13, 1),
    ("0001 00", 0, 7),
    ("0010 0111", 1, 4),
    ("1111 1100", 2, 3),
    ("1111 1101", 4, 2),
    ("0000 0010 0", 5, 2),
    ("0000 0010 1", 14, 1),
    ("0000 0011 1", 15, 1),
    ("0000 0011 01", 16, 1),
    ("1111 011", 0, 8),
    ("1111 100", 0, 9),
    ("0010 0011", 0, 10),
    ("0010 0010", 0, 11),
    ("0010 0000", 1, 5),
    ("0000 0011 00", 2, 4),
    ("1111 1010", 0, 12),
    ("1111 1011", 0, 13),
    ("1111 1110", 0, 14),
    ("1111 1111", 0, 15),
];

/// `0000 0001 xxxx` codes the two tables share.
const DCT_MID_SHARED: &[(&str, u8, u8)] = &[
    ("0000 0001 1100", 3, 3),
    ("0000 0001 0010", 4, 3),
    ("0000 0001 1110", 6, 2),
    ("0000 0001 0101", 7, 2),
    ("0000 0001 0001", 8, 2),
    ("0000 0001 1111", 17, 1),
    ("0000 0001 1010", 18, 1),
    ("0000 0001 1001", 19, 1),
    ("0000 0001 0111", 20, 1),
    ("0000 0001 0110", 21, 1),
];

/// Codes from `0000 0000 1` on, which the two tables share.
const DCT_LONG: &[(&str, u8, u8)] = &[
    ("0000 0000 1011 0", 1, 6),
    ("0000 0000 1010 1", 1, 7),
    ("0000 0000 1010 0", 2, 5),
    ("0000 0000 1001 1", 3, 4),
    ("0000 0000 1001 0", 5, 3),
    ("0000 0000 1000 1", 9, 2),
    ("0000 0000 1000 0", 10, 2),
    ("0000 0000 1111 1", 22, 1),
    ("0000 0000 1111 0", 23, 1),
    ("0000 0000 1110 1", 24, 1),
    ("0000 0000 1110 0", 25, 1),
    ("0000 0000 1101 1", 26, 1),
    ("0000 0000 0111 11", 0, 16),
    ("0000 0000 0111 10", 0, 17),
    ("0000 0000 0111 01", 0, 18),
    ("0000 0000 0111 00", 0, 19),
    ("0000 0000 0110 11", 0, 20),
    ("0000 0000 0110 10", 0, 21),
    ("0000 0000 0110 01", 0, 22),
    ("0000 0000 0110 00", 0, 23),
    ("0000 0000 0101 11", 0, 24),
    ("0000 0000 0101 10", 0, 25),
    ("0000 0000 0101 01", 0, 26),
    ("0000 0000 0101 00", 0, 27),
    ("0000 0000 0100 11", 0, 28),
    ("0000 0000 0100 10", 0, 29),
    ("0000 0000 0100 01", 0, 30),
    ("0000 0000 0100 00", 0, 31),
    ("0000 0000 0011 000", 0, 32),
    ("0000 0000 0010 111", 0, 33),
    ("0000 0000 0010 110", 0, 34),
    ("0000 0000 0010 101", 0, 35),
    ("0000 0000 0010 100", 0, 36),
    ("0000 0000 0010 011", 0, 37),
    ("0000 0000 0010 010", 0, 38),
    ("0000 0000 0010 001", 0, 39),
    ("0000 0000 0010 000", 0, 40),
    ("0000 0000 0011 111", 1, 8),
    ("0000 0000 0011 110", 1, 9),
    ("0000 0000 0011 101", 1, 10),
    ("0000 0000 0011 100", 1, 11),
    ("0000 0000 0011 011", 1, 12),
    ("0000 0000 0011 010", 1, 13),
    ("0000 0000 0011 001", 1, 14),
    ("0000 0000 0001 0011", 1, 15),
    ("0000 0000 0001 0010", 1, 16),
    ("0000 0000 0001 0001", 1, 17),
    ("0000 0000 0001 0000", 1, 18),
    ("0000 0000 0001 0100", 6, 3),
    ("0000 0000 0001 1010", 11, 2),
    ("0000 0000 0001 1001", 12, 2),
    ("0000 0000 0001 1000", 13, 2),
    ("0000 0000 0001 0111", 14, 2),
    ("0000 0000 0001 0110", 15, 2),
    ("0000 0000 0001 0101", 16, 2),
    ("0000 0000 0001 1111", 27, 1),
    ("0000 0000 0001 1110", 28, 1),
    ("0000 0000 0001 1101", 29, 1),
    ("0000 0000 0001 1100", 30, 1),
    ("0000 0000 0001 1011", 31, 1),
];

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
        DctTable, PType, VlcCode, coded_block_pattern, dct_dc_size_chroma, dct_dc_size_luma,
        macroblock_address_increment, motion_code, p_macroblock_type, run_level_code,
        wrap_motion_delta, write_coefficient, write_end_of_block, write_end_of_block_in,
        write_escape_coefficient, write_macroblock_address_increment_after, write_motion_delta,
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

    /// Every run/level code of a table, with its end of block and escape.
    fn dct_codes(table: DctTable) -> Vec<VlcCode> {
        let mut codes: Vec<VlcCode> = (0u8..64)
            .flat_map(|run| (1u32..=64).filter_map(move |level| run_level_code(table, run, level)))
            .collect();
        codes.push(match table {
            DctTable::Zero => (0b10, 2),
            DctTable::One => (0b0110, 4),
        });
        codes.push((0b000001, 6));
        codes
    }

    #[test]
    fn dct_tables_are_complete_and_prefix_free() {
        for table in [DctTable::Zero, DctTable::One] {
            let codes = dct_codes(table);
            // 111 run/level codes, end of block and escape in each of B.14 and B.15.
            assert_eq!(codes.len(), 113, "{table:?}");
            codes_are_prefix_free(&codes);
            // (The sign bit after a run/level code splits its share of the code space, not adds to it.) Table zero
            // leaves only "0000 0000 0000 xxxx" unused: a Kraft sum of 1 - 2^-12. Table one also leaves free the
            // codes its short ones replaced.
            let kraft: f64 = codes
                .iter()
                .map(|&(_, len)| 2f64.powi(-i32::from(len)))
                .sum();
            assert!(kraft <= 1.0, "{table:?}: Kraft sum {kraft}");
            if table == DctTable::Zero {
                assert!(
                    (kraft - (1.0 - 2f64.powi(-12))).abs() < 1e-12,
                    "Kraft sum {kraft}"
                );
            }
        }
    }

    fn coefficient(table: DctTable, run: u8, level: i32, first: bool) -> Vec<u8> {
        let mut writer = BitWriter::new();
        write_coefficient(&mut writer, table, run, level, first);
        writer.finish()
    }

    #[test]
    fn coefficient_exact_values() {
        // Table zero, first coefficient of a non-intra block: 1s.
        assert_eq!(coefficient(DctTable::Zero, 0, 1, true), vec![0x80]);
        assert_eq!(coefficient(DctTable::Zero, 0, -1, true), vec![0xC0]);
        // Otherwise 11s.
        assert_eq!(coefficient(DctTable::Zero, 0, 1, false), vec![0xC0]);
        assert_eq!(coefficient(DctTable::Zero, 0, -1, false), vec![0xE0]);
        // run 1 level -1: 011 1.
        assert_eq!(coefficient(DctTable::Zero, 1, -1, false), vec![0x70]);
        // Table one, run 0 level 2: 110 0; "1s" is not used there.
        assert_eq!(coefficient(DctTable::One, 0, 2, false), vec![0xC0]);
        assert_eq!(coefficient(DctTable::One, 0, 1, true), vec![0x80]);
        // Not in the tables (run 0 level 41, run 32 level 1): escape.
        assert_eq!(
            coefficient(DctTable::Zero, 0, 41, false),
            write_escape(0, 41)
        );
        assert_eq!(
            coefficient(DctTable::One, 32, 1, false),
            write_escape(32, 1)
        );
    }

    #[test]
    fn end_of_block_per_table() {
        let mut writer = BitWriter::new();
        write_end_of_block_in(&mut writer, DctTable::One);
        assert_eq!(writer.finish(), vec![0x60]);
    }

    fn address_increment(skipped: u32) -> Vec<u8> {
        let mut writer = BitWriter::new();
        write_macroblock_address_increment_after(&mut writer, skipped);
        writer.finish()
    }

    #[test]
    fn address_increment_after_skips() {
        // No skip: increment 1 is "1".
        assert_eq!(address_increment(0), vec![0x80]);
        // 32 skipped: increment 33, "0000 0011 000".
        assert_eq!(address_increment(32), vec![0x03, 0x00]);
        // 33 skipped: escape "0000 0001 000", then increment 1 "1".
        assert_eq!(address_increment(33), vec![0x01, 0x10]);
    }
}
