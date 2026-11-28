//! The two ways FFV1 stores symbols (RFC 9043 §3.8): the range coder (§3.8.1) with its state transition tables, and
//! Golomb-Rice codes read bit by bit (§3.8.2), with the per-context adaptive state of §3.8.2.4.

use crate::Error;

/// The default state transition table (RFC 9043 Figure 24): the state after a 1 for each state.
const DEFAULT_TRANSITION: [u8; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 20, 21, 22, 23, 24, 25, 26, 27, //
    28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 37, 38, 39, 40, 41, 42, //
    43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 56, 57, //
    58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, //
    74, 75, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, //
    89, 90, 91, 92, 93, 94, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, //
    104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 114, 115, 116, 117, 118, //
    119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133, 133, //
    134, 135, 136, 137, 138, 139, 140, 141, 142, 143, 144, 145, 146, 147, 148, 149, //
    150, 151, 152, 152, 153, 154, 155, 156, 157, 158, 159, 160, 161, 162, 163, 164, //
    165, 166, 167, 168, 169, 170, 171, 171, 172, 173, 174, 175, 176, 177, 178, 179, //
    180, 181, 182, 183, 184, 185, 186, 187, 188, 189, 190, 190, 191, 192, 194, 194, //
    195, 196, 197, 198, 199, 200, 201, 202, 202, 204, 205, 206, 207, 208, 209, 209, //
    210, 211, 212, 213, 215, 215, 216, 217, 218, 219, 220, 220, 222, 223, 224, 225, //
    226, 227, 227, 229, 229, 230, 231, 232, 234, 234, 235, 236, 237, 238, 239, 240, //
    241, 242, 243, 244, 245, 246, 247, 248, 248, 0, 0, 0, 0, 0, 0, 0, //
];

/// The states after a 1 and after a 0 (RFC 9043 §3.8.1.4): the default table, or one with the `deltas` of a custom
/// table (coder type 2) added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Transitions {
    one: [u8; 256],
    zero: [u8; 256],
}

impl Transitions {
    /// The tables with `deltas` (index 1 to 255; the first is unused) added to the default one.
    pub(crate) fn new(deltas: &[i32; 256]) -> Self {
        let mut one = [0u8; 256];
        for (index, (state, delta)) in one.iter_mut().zip(deltas).enumerate() {
            let base = DEFAULT_TRANSITION.get(index).copied().map_or(0, i32::from);
            let value = if index == 0 { base } else { base + delta };
            *state = u8::try_from(value.clamp(0, 255)).unwrap_or(0);
        }
        // zero_state_i = 256 - one_state_(256 - i); state 0 (no table entry 256) stays 0.
        let mut zero = [0u8; 256];
        for (index, state) in zero.iter_mut().enumerate().skip(1) {
            let mirrored = one.get(256 - index).copied().map_or(0, i32::from);
            *state = u8::try_from((256 - mirrored).clamp(0, 255)).unwrap_or(0);
        }
        Self { one, zero }
    }

    /// The default tables (coder type 1).
    pub(crate) fn default_tables() -> Self {
        Self::new(&[0; 256])
    }
}

/// A range decoder over a byte stream of known length (RFC 9043 §3.8.1.1, closed mode: bytes past the end read as 0).
pub(crate) struct RangeDecoder<'a> {
    data: &'a [u8],
    /// The next byte to read.
    pos: usize,
    low: u32,
    range: u32,
    end: bool,
}

impl<'a> RangeDecoder<'a> {
    /// A decoder reading `data` from its start (Figure 18).
    pub(crate) fn new(data: &'a [u8]) -> Self {
        let first = |at: usize| u32::from(data.get(at).copied().unwrap_or(0));
        let mut decoder = Self {
            data,
            pos: 2,
            low: (first(0) << 8) | first(1),
            range: 0xFF00,
            end: data.len() <= 2,
        };
        if decoder.low >= decoder.range {
            decoder.low = decoder.range;
            decoder.end = true;
        }
        decoder
    }

    /// How many bytes of the stream have been read.
    pub(crate) fn bytes_read(&self) -> usize {
        self.pos
    }

    fn refill(&mut self) {
        if self.range < 0x100 {
            self.range <<= 8;
            self.low <<= 8;
            if !self.end {
                self.low += u32::from(self.data.get(self.pos).copied().unwrap_or(0));
                self.pos += 1;
                if self.pos >= self.data.len() {
                    self.end = true;
                }
            }
        }
    }

    /// One binary value with the adaptive `state` (Figure 20).
    pub(crate) fn bit(&mut self, state: &mut u8, tables: &Transitions) -> bool {
        let split = (self.range * u32::from(*state)) >> 8;
        self.range -= split;
        if self.low < self.range {
            *state = tables.zero.get(usize::from(*state)).copied().unwrap_or(0);
            self.refill();
            false
        } else {
            self.low -= self.range;
            *state = tables.one.get(usize::from(*state)).copied().unwrap_or(0);
            self.range = split;
            self.refill();
            true
        }
    }

    /// A scalar with its 32 adaptive states (Figure 21): 0, or an exponent, mantissa and (when `signed`) sign.
    pub(crate) fn symbol(
        &mut self,
        states: &mut [u8; 32],
        signed: bool,
        tables: &Transitions,
    ) -> Result<i32, Error> {
        if self.bit(&mut states[0], tables) {
            return Ok(0);
        }
        let mut exponent = 0usize;
        while self.bit(&mut states[1 + exponent.min(9)], tables) {
            exponent += 1;
            if exponent > 31 {
                return Err(Error::Corrupt("a range-coded number too long"));
            }
        }
        let mut value: i64 = 1;
        for bit in (0..exponent).rev() {
            value = value * 2 + i64::from(self.bit(&mut states[22 + bit.min(9)], tables));
        }
        let value =
            i32::try_from(value).map_err(|_| Error::Corrupt("a range-coded number too large"))?;
        if signed && self.bit(&mut states[11 + exponent.min(10)], tables) {
            Ok(-value)
        } else {
            Ok(value)
        }
    }
}

/// Bits read most significant first (Golomb-Rice coded slices); past the end, zeros.
pub(crate) struct BitReader<'a> {
    data: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, bit: 0 }
    }

    pub(crate) fn bits(&mut self, count: u32) -> u32 {
        let mut value = 0u32;
        for _ in 0..count.min(32) {
            let byte = self.data.get(self.bit / 8).copied().unwrap_or(0);
            let one = (byte >> (7 - (self.bit % 8))) & 1;
            value = (value << 1) | u32::from(one);
            self.bit += 1;
        }
        value
    }

    /// Whether the reader is past the end of its bytes (a hostile stream running on).
    pub(crate) fn exhausted(&self) -> bool {
        self.bit > self.data.len() * 8
    }

    /// An unsigned Golomb-Rice number with parameter `k`, or, after 12 zeros, `bits` bits plus 11 (Figure 26).
    pub(crate) fn unsigned_golomb(&mut self, k: u32, bits: u32) -> u32 {
        for prefix in 0..12u32 {
            if self.bits(1) == 1 {
                return self.bits(k).wrapping_add(prefix << k.min(31));
            }
        }
        self.bits(bits).wrapping_add(11)
    }

    /// A signed Golomb-Rice number (Figure 27).
    pub(crate) fn signed_golomb(&mut self, k: u32, bits: u32) -> i32 {
        let value = self.unsigned_golomb(k, bits);
        let half = i32::try_from(value >> 1).unwrap_or(i32::MAX);
        if value & 1 == 1 { -half - 1 } else { half }
    }
}

/// The adaptive state of one Golomb-Rice context (§3.8.2.4, initial values §3.8.2.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VlcState {
    drift: i32,
    error_sum: i32,
    bias: i32,
    count: i32,
}

impl Default for VlcState {
    fn default() -> Self {
        Self {
            drift: 0,
            error_sum: 4,
            bias: 0,
            count: 1,
        }
    }
}

impl VlcState {
    /// A sample difference of `bits` bits with this context's mean taken out and its own `k` (`get_vlc_symbol`).
    pub(crate) fn symbol(&mut self, reader: &mut BitReader<'_>, bits: u32) -> i32 {
        let mut k = 0u32;
        let mut i = self.count;
        while i < self.error_sum && k < 31 {
            k += 1;
            i = i.saturating_add(i);
        }
        let mut v = reader.signed_golomb(k, bits);
        if 2 * self.drift < -self.count {
            v = -1 - v;
        }
        let value = sign_extend(v.wrapping_add(self.bias), bits);
        self.error_sum = self.error_sum.saturating_add(v.saturating_abs());
        self.drift = self.drift.saturating_add(v);
        if self.count == 128 {
            self.count >>= 1;
            self.drift >>= 1;
            self.error_sum >>= 1;
        }
        self.count += 1;
        if self.drift <= -self.count {
            self.bias = (self.bias - 1).max(-128);
            self.drift = (self.drift + self.count).max(-self.count + 1);
        } else if self.drift > 0 {
            self.bias = (self.bias + 1).min(127);
            self.drift = (self.drift - self.count).min(0);
        }
        value
    }
}

/// `value` as a signed number of `bits` bits (§3.8.2.3).
pub(crate) fn sign_extend(value: i32, bits: u32) -> i32 {
    if bits == 0 || bits >= 32 {
        return value;
    }
    let negative = 1i32 << (bits - 1);
    let out = value & (negative - 1);
    if value & negative != 0 {
        out - negative
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::{BitReader, Transitions, sign_extend};

    /// RFC 9043 Table 3: signed Golomb-Rice codes as the RFC decodes them.
    #[test]
    fn golomb_codes_decode_as_the_rfc_shows() {
        let cases: [(&[u8], u32, i32); 5] = [
            (&[0b1000_0000], 0, 0),
            (&[0b0010_0000], 0, 1),
            (&[0b1000_0000], 2, 0),
            (&[0b1100_0000], 2, 1),
            (&[0b0101_0000], 2, -3),
        ];
        // Table 3 lists the unsigned values 0, 2, 0, 2, 5; signed they are 0, 1, 0, 1, -3.
        for (bytes, k, expected) in cases {
            assert_eq!(
                BitReader::new(bytes).signed_golomb(k, 8),
                expected,
                "{bytes:?} k={k}"
            );
        }
        // The escape: 12 zeros, then 8 bits of 128, so 139 (unsigned).
        let mut reader = BitReader::new(&[0, 0b0000_1000, 0b0000_0000]);
        assert_eq!(reader.unsigned_golomb(3, 8), 139);
    }

    #[test]
    fn sign_extension_keeps_the_sign() {
        assert_eq!(sign_extend(0xFF, 8), -1);
        assert_eq!(sign_extend(0x7F, 8), 127);
        assert_eq!(sign_extend(0x180, 8), -128);
    }

    /// The tables are inverses where the RFC makes them so: after a 0 the state mirrors the state after a 1.
    #[test]
    fn the_zero_table_mirrors_the_one_table() {
        let tables = Transitions::default_tables();
        assert_eq!(tables.one[8], 20);
        assert_eq!(u32::from(tables.zero[256 - 8]), 256 - 20);
    }
}
