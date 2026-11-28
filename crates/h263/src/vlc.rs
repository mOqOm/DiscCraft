//! The bit reader and the variable-length code tables of ITU-T H.263 (01/2005) §5: MCBPC (Tables 7 and 8), CBPY
//! (Table 12), MVD (Table 14) and TCOEF (Table 16), with the fixed-length escape of Table 17.

/// Bits read most significant first; past the end, zeros (and [`BitReader::overrun`] says so).
pub(crate) struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// The next `count` bits (at most 32) without taking them.
    pub(crate) fn peek(&self, count: u32) -> u32 {
        let mut value = 0u32;
        for index in 0..count.min(32) as usize {
            let at = self.pos + index;
            let byte = self.data.get(at / 8).copied().unwrap_or(0);
            value = (value << 1) | u32::from((byte >> (7 - at % 8)) & 1);
        }
        value
    }

    pub(crate) fn skip(&mut self, count: u32) {
        self.pos = self.pos.saturating_add(count as usize);
    }

    pub(crate) fn bits(&mut self, count: u32) -> u32 {
        let value = self.peek(count);
        self.skip(count);
        value
    }

    pub(crate) fn bit(&mut self) -> bool {
        self.bits(1) == 1
    }

    /// The bit position.
    pub(crate) fn position(&self) -> usize {
        self.pos
    }

    pub(crate) fn set_position(&mut self, pos: usize) {
        self.pos = pos;
    }

    /// Whether more bits have been read than there are.
    pub(crate) fn overrun(&self) -> bool {
        self.pos > self.data.len() * 8
    }
}

/// A prefix code: for each code length, a table of every code of that length.
pub(crate) struct Vlc<T: Copy> {
    by_length: Vec<Vec<Option<T>>>,
}

impl<T: Copy> Vlc<T> {
    /// The code of `entries`: each a code written as '0' and '1' (spaces ignored) and its value.
    pub(crate) fn new(entries: &[(&str, T)]) -> Self {
        let longest = entries
            .iter()
            .map(|(code, _)| code.chars().filter(|c| *c == '0' || *c == '1').count())
            .max()
            .unwrap_or(0);
        let mut by_length: Vec<Vec<Option<T>>> = (0..=longest)
            .map(|length| vec![None; 1 << length])
            .collect();
        for (code, value) in entries {
            let bits: Vec<u32> = code.chars().filter_map(|c| c.to_digit(2)).collect();
            let index = bits
                .iter()
                .fold(0usize, |acc, bit| (acc << 1) | *bit as usize);
            if let Some(slot) = by_length
                .get_mut(bits.len())
                .and_then(|table| table.get_mut(index))
            {
                *slot = Some(*value);
            }
        }
        Self { by_length }
    }

    /// The next value, or `None` when the bits are no code.
    pub(crate) fn read(&self, reader: &mut BitReader<'_>) -> Option<T> {
        for (length, table) in self.by_length.iter().enumerate().skip(1) {
            let code = reader.peek(length as u32) as usize;
            if let Some(value) = table.get(code).copied().flatten() {
                reader.skip(length as u32);
                return Some(value);
            }
        }
        None
    }
}

/// An MCBPC value: a macroblock type and its chroma coded block pattern (bit 1 Cb, bit 0 Cr), or stuffing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mcbpc {
    Type { kind: u8, cbpc: u8 },
    Stuffing,
}

/// Table 7: MCBPC for I-pictures.
pub(crate) fn mcbpc_intra() -> Vlc<Mcbpc> {
    let t = |kind, cbpc| Mcbpc::Type { kind, cbpc };
    Vlc::new(&[
        ("1", t(3, 0)),
        ("001", t(3, 1)),
        ("010", t(3, 2)),
        ("011", t(3, 3)),
        ("0001", t(4, 0)),
        ("0000 01", t(4, 1)),
        ("0000 10", t(4, 2)),
        ("0000 11", t(4, 3)),
        ("0000 0000 1", Mcbpc::Stuffing),
    ])
}

/// Table 8: MCBPC for P-pictures.
pub(crate) fn mcbpc_inter() -> Vlc<Mcbpc> {
    let t = |kind, cbpc| Mcbpc::Type { kind, cbpc };
    Vlc::new(&[
        ("1", t(0, 0)),
        ("0011", t(0, 1)),
        ("0010", t(0, 2)),
        ("0001 01", t(0, 3)),
        ("011", t(1, 0)),
        ("0000 111", t(1, 1)),
        ("0000 110", t(1, 2)),
        ("0000 0010 1", t(1, 3)),
        ("010", t(2, 0)),
        ("0000 101", t(2, 1)),
        ("0000 100", t(2, 2)),
        ("0000 0101", t(2, 3)),
        ("0001 1", t(3, 0)),
        ("0000 0100", t(3, 1)),
        ("0000 0011", t(3, 2)),
        ("0000 011", t(3, 3)),
        ("0001 00", t(4, 0)),
        ("0000 0010 0", t(4, 1)),
        ("0000 0001 1", t(4, 2)),
        ("0000 0001 0", t(4, 3)),
        ("0000 0000 1", Mcbpc::Stuffing),
        ("0000 0000 010", t(5, 0)),
        ("0000 0000 0110 0", t(5, 1)),
        ("0000 0000 0111 0", t(5, 2)),
        ("0000 0000 0111 1", t(5, 3)),
    ])
}

/// Table 12: CBPY, as the pattern of an INTRA macroblock (blocks 1 to 4 from the high bit); an INTER macroblock's
/// is its complement.
pub(crate) fn cbpy() -> Vlc<u8> {
    Vlc::new(&[
        ("0011", 0),
        ("0010 1", 1),
        ("0010 0", 2),
        ("1001", 3),
        ("0001 1", 4),
        ("0111", 5),
        ("0000 10", 6),
        ("1011", 7),
        ("0001 0", 8),
        ("0000 11", 9),
        ("0101", 10),
        ("1010", 11),
        ("0100", 12),
        ("1000", 13),
        ("0110", 14),
        ("11", 15),
    ])
}

/// Table 14: MVD, as a difference in half pixels (index - 32; the other value of each pair is 64 away).
pub(crate) fn mvd() -> Vlc<i8> {
    const CODES: [&str; 64] = [
        "0000 0000 0010 1",
        "0000 0000 0011 1",
        "0000 0000 0101",
        "0000 0000 0111",
        "0000 0000 1001",
        "0000 0000 1011",
        "0000 0000 1101",
        "0000 0000 1111",
        "0000 0001 001",
        "0000 0001 011",
        "0000 0001 101",
        "0000 0001 111",
        "0000 0010 001",
        "0000 0010 011",
        "0000 0010 101",
        "0000 0010 111",
        "0000 0011 001",
        "0000 0011 011",
        "0000 0011 101",
        "0000 0011 111",
        "0000 0100 001",
        "0000 0100 011",
        "0000 0100 11",
        "0000 0101 01",
        "0000 0101 11",
        "0000 0111",
        "0000 1001",
        "0000 1011",
        "0000 111",
        "0001 1",
        "0011",
        "011",
        "1",
        "010",
        "0010",
        "0001 0",
        "0000 110",
        "0000 1010",
        "0000 1000",
        "0000 0110",
        "0000 0101 10",
        "0000 0101 00",
        "0000 0100 10",
        "0000 0100 010",
        "0000 0100 000",
        "0000 0011 110",
        "0000 0011 100",
        "0000 0011 010",
        "0000 0011 000",
        "0000 0010 110",
        "0000 0010 100",
        "0000 0010 010",
        "0000 0010 000",
        "0000 0001 110",
        "0000 0001 100",
        "0000 0001 010",
        "0000 0001 000",
        "0000 0000 1110",
        "0000 0000 1100",
        "0000 0000 1010",
        "0000 0000 1000",
        "0000 0000 0110",
        "0000 0000 0100",
        "0000 0000 0011 0",
    ];
    let entries: Vec<(&str, i8)> = CODES
        .iter()
        .enumerate()
        .map(|(index, code)| (*code, i8::try_from(index as i32 - 32).unwrap_or(0)))
        .collect();
    Vlc::new(&entries)
}

/// A TCOEF event: the last coefficient of the block or not, the zeros before it and its level's magnitude; or the
/// escape to Table 17.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Event {
    Coefficient { last: bool, run: u8, level: u8 },
    Escape,
}

/// Table 16: TCOEF, the codes without their sign bit.
pub(crate) fn tcoef() -> Vlc<Event> {
    // (code, run, |level|) for LAST = 0, then for LAST = 1.
    const NOT_LAST: [(&str, u8, u8); 58] = [
        ("10", 0, 1),
        ("1111", 0, 2),
        ("0101 01", 0, 3),
        ("0010 111", 0, 4),
        ("0001 1111", 0, 5),
        ("0001 0010 1", 0, 6),
        ("0001 0010 0", 0, 7),
        ("0000 1000 01", 0, 8),
        ("0000 1000 00", 0, 9),
        ("0000 0000 111", 0, 10),
        ("0000 0000 110", 0, 11),
        ("0000 0100 000", 0, 12),
        ("110", 1, 1),
        ("0101 00", 1, 2),
        ("0001 1110", 1, 3),
        ("0000 0011 11", 1, 4),
        ("0000 0100 001", 1, 5),
        ("0000 0101 0000", 1, 6),
        ("1110", 2, 1),
        ("0001 1101", 2, 2),
        ("0000 0011 10", 2, 3),
        ("0000 0101 0001", 2, 4),
        ("0110 1", 3, 1),
        ("0001 0001 1", 3, 2),
        ("0000 0011 01", 3, 3),
        ("0110 0", 4, 1),
        ("0001 0001 0", 4, 2),
        ("0000 0101 0010", 4, 3),
        ("0101 1", 5, 1),
        ("0000 0011 00", 5, 2),
        ("0000 0101 0011", 5, 3),
        ("0100 11", 6, 1),
        ("0000 0010 11", 6, 2),
        ("0000 0101 0100", 6, 3),
        ("0100 10", 7, 1),
        ("0000 0010 10", 7, 2),
        ("0100 01", 8, 1),
        ("0000 0010 01", 8, 2),
        ("0100 00", 9, 1),
        ("0000 0010 00", 9, 2),
        ("0010 110", 10, 1),
        ("0000 0101 0101", 10, 2),
        ("0010 101", 11, 1),
        ("0010 100", 12, 1),
        ("0001 1100", 13, 1),
        ("0001 1011", 14, 1),
        ("0001 0000 1", 15, 1),
        ("0001 0000 0", 16, 1),
        ("0000 1111 1", 17, 1),
        ("0000 1111 0", 18, 1),
        ("0000 1110 1", 19, 1),
        ("0000 1110 0", 20, 1),
        ("0000 1101 1", 21, 1),
        ("0000 1101 0", 22, 1),
        ("0000 0100 010", 23, 1),
        ("0000 0100 011", 24, 1),
        ("0000 0101 0110", 25, 1),
        ("0000 0101 0111", 26, 1),
    ];
    const LAST: [(&str, u8, u8); 44] = [
        ("0111", 0, 1),
        ("0000 1100 1", 0, 2),
        ("0000 0000 101", 0, 3),
        ("0011 11", 1, 1),
        ("0000 0000 100", 1, 2),
        ("0011 10", 2, 1),
        ("0011 01", 3, 1),
        ("0011 00", 4, 1),
        ("0010 011", 5, 1),
        ("0010 010", 6, 1),
        ("0010 001", 7, 1),
        ("0010 000", 8, 1),
        ("0001 1010", 9, 1),
        ("0001 1001", 10, 1),
        ("0001 1000", 11, 1),
        ("0001 0111", 12, 1),
        ("0001 0110", 13, 1),
        ("0001 0101", 14, 1),
        ("0001 0100", 15, 1),
        ("0001 0011", 16, 1),
        ("0000 1100 0", 17, 1),
        ("0000 1011 1", 18, 1),
        ("0000 1011 0", 19, 1),
        ("0000 1010 1", 20, 1),
        ("0000 1010 0", 21, 1),
        ("0000 1001 1", 22, 1),
        ("0000 1001 0", 23, 1),
        ("0000 1000 1", 24, 1),
        ("0000 0001 11", 25, 1),
        ("0000 0001 10", 26, 1),
        ("0000 0001 01", 27, 1),
        ("0000 0001 00", 28, 1),
        ("0000 0100 100", 29, 1),
        ("0000 0100 101", 30, 1),
        ("0000 0100 110", 31, 1),
        ("0000 0100 111", 32, 1),
        ("0000 0101 1000", 33, 1),
        ("0000 0101 1001", 34, 1),
        ("0000 0101 1010", 35, 1),
        ("0000 0101 1011", 36, 1),
        ("0000 0101 1100", 37, 1),
        ("0000 0101 1101", 38, 1),
        ("0000 0101 1110", 39, 1),
        ("0000 0101 1111", 40, 1),
    ];
    let mut entries: Vec<(&str, Event)> = NOT_LAST
        .iter()
        .map(|&(code, run, level)| {
            (
                code,
                Event::Coefficient {
                    last: false,
                    run,
                    level,
                },
            )
        })
        .chain(LAST.iter().map(|&(code, run, level)| {
            (
                code,
                Event::Coefficient {
                    last: true,
                    run,
                    level,
                },
            )
        }))
        .collect();
    entries.push(("0000 011", Event::Escape));
    Vlc::new(&entries)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::{BitReader, Event, cbpy, mcbpc_inter, mvd, tcoef};

    /// The tables are prefix codes: no code is the start of another, so each reads back as itself.
    #[test]
    fn every_code_reads_back_as_its_value() {
        // The lengths the tables give (Table 16 counts the sign bit too).
        let tcoef = tcoef();
        let mut reader = BitReader::new(&[0b1000_0000]);
        assert_eq!(
            tcoef.read(&mut reader),
            Some(Event::Coefficient {
                last: false,
                run: 0,
                level: 1
            })
        );
        assert_eq!(reader.position(), 2);
        let mut reader = BitReader::new(&[0b0000_0110]);
        assert_eq!(tcoef.read(&mut reader), Some(Event::Escape));
        let mut reader = BitReader::new(&[0b1000_0000]);
        assert_eq!(mvd().read(&mut reader), Some(0));
        let mut reader = BitReader::new(&[0b0100_0000]);
        assert_eq!(mvd().read(&mut reader), Some(1));
        let mut reader = BitReader::new(&[0b1100_0000]);
        assert_eq!(cbpy().read(&mut reader), Some(15));
        let mut reader = BitReader::new(&[0b0000_0000, 0b1000_0000]);
        assert_eq!(
            mcbpc_inter().read(&mut reader),
            Some(super::Mcbpc::Stuffing)
        );
    }
}
