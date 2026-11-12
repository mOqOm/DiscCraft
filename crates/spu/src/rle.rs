//! Run-length coding for SPU pixel fields.

use crate::{SpuError, spu_error};

/// Encode one run as a vector of nibbles.
pub(crate) fn run_nibbles(run: u16, value: u8, at_line_end: bool) -> Result<Vec<u8>, SpuError> {
    if value > 3 {
        return Err(spu_error("pixel value out of range"));
    }
    if run == 0 {
        return Err(spu_error("zero run"));
    }
    if at_line_end && run > 255 {
        return Ok(vec![0, 0, 0, value]);
    }
    encode_normal_run(run, value)
}

fn encode_normal_run(run: u16, value: u8) -> Result<Vec<u8>, SpuError> {
    if run <= 3 {
        return Ok(vec![((run << 2) | u16::from(value)) as u8]);
    }
    if run <= 15 {
        let first = (run >> 2) as u8;
        let second = (((run & 3) << 2) | u16::from(value)) as u8;
        return Ok(vec![first, second]);
    }
    if run <= 63 {
        let second = (run >> 2) as u8;
        let third = (((run & 3) << 2) | u16::from(value)) as u8;
        return Ok(vec![0, second, third]);
    }
    if run <= 255 {
        let second = (run >> 6) as u8;
        let third = ((run >> 2) & 0xF) as u8;
        let fourth = (((run & 3) << 2) | u16::from(value)) as u8;
        return Ok(vec![0, second, third, fourth]);
    }
    Err(spu_error("run too long"))
}

/// Encode one pixel line into bytes, padding to a byte boundary.
pub(crate) fn encode_line(line: &[u8]) -> Result<Vec<u8>, SpuError> {
    let mut nibbles = Vec::new();
    encode_runs(line, &mut nibbles)?;
    Ok(pack_nibbles(&nibbles))
}

fn encode_runs(line: &[u8], nibbles: &mut Vec<u8>) -> Result<(), SpuError> {
    let mut i = 0usize;
    while let Some(&value) = line.get(i) {
        let run = count_run(line, i, value)?;
        let run16 = u16::try_from(run).map_err(|_| spu_error("run too long"))?;
        let next = i
            .checked_add(run)
            .ok_or_else(|| spu_error("line overflow"))?;
        let at_end = next == line.len();
        push_run(nibbles, run16, value, at_end)?;
        i = next;
    }
    Ok(())
}

fn count_run(line: &[u8], start: usize, value: u8) -> Result<usize, SpuError> {
    let mut run = 1usize;
    while let Some(&next) = start.checked_add(run).and_then(|idx| line.get(idx)) {
        if next != value {
            break;
        }
        run = run
            .checked_add(1)
            .ok_or_else(|| spu_error("run overflow"))?;
    }
    Ok(run)
}

fn push_run(nibbles: &mut Vec<u8>, run: u16, value: u8, at_end: bool) -> Result<(), SpuError> {
    if at_end && run > 255 {
        nibbles.extend(run_nibbles(run, value, true)?);
        return Ok(());
    }

    let mut remaining = run;
    while remaining > 255 {
        nibbles.extend(run_nibbles(255, value, false)?);
        remaining = remaining
            .checked_sub(255)
            .ok_or_else(|| spu_error("run underflow"))?;
    }
    if remaining > 0 {
        nibbles.extend(run_nibbles(remaining, value, at_end)?);
    }
    Ok(())
}

fn pack_nibbles(nibbles: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(nibbles.len().div_ceil(2));
    let mut iter = nibbles.iter();
    while let Some(&a) = iter.next() {
        let b = match iter.next() {
            Some(&b) => b,
            None => 0,
        };
        bytes.push(((a & 0xF) << 4) | (b & 0xF));
    }
    bytes
}

pub(crate) struct NibbleReader<'a> {
    data: &'a [u8],
    pos: usize,
    nibble: usize,
    limit: usize,
}

impl<'a> NibbleReader<'a> {
    pub(crate) fn new(data: &'a [u8], pos: usize, limit: usize) -> Option<Self> {
        if pos > limit || limit > data.len() {
            return None;
        }
        Some(Self {
            data,
            pos,
            nibble: 0,
            limit,
        })
    }

    fn read(&mut self) -> Option<u8> {
        if self.pos >= self.limit {
            return None;
        }
        let byte = self.data.get(self.pos)?;
        let value = if self.nibble == 0 {
            byte >> 4
        } else {
            byte & 0xF
        };

        if self.nibble == 0 {
            self.nibble = 1;
        } else {
            self.nibble = 0;
            self.pos = self.pos.checked_add(1)?;
        }
        Some(value)
    }

    fn consume_padding(&mut self) -> Result<(), SpuError> {
        if self.nibble != 0 {
            let pad = self.read().ok_or_else(|| spu_error("missing padding"))?;
            if pad != 0 {
                return Err(spu_error("bad padding"));
            }
        }
        Ok(())
    }
}

/// Decode one interlaced field into `pixels`.
pub(crate) fn decode_field(
    data: &[u8],
    limit: usize,
    offset: usize,
    start_line: usize,
    line_count: usize,
    width: usize,
    pixels: &mut [u8],
) -> Result<(), SpuError> {
    if line_count == 0 {
        return Ok(());
    }

    let mut reader =
        NibbleReader::new(data, offset, limit).ok_or_else(|| spu_error("bad field offset"))?;
    for i in 0..line_count {
        let doubled = i.checked_mul(2).ok_or_else(|| spu_error("line overflow"))?;
        let line_index = start_line
            .checked_add(doubled)
            .ok_or_else(|| spu_error("line overflow"))?;
        let start = line_index
            .checked_mul(width)
            .ok_or_else(|| spu_error("pixel overflow"))?;
        let end = start
            .checked_add(width)
            .ok_or_else(|| spu_error("pixel overflow"))?;
        let row = pixels
            .get_mut(start..end)
            .ok_or_else(|| spu_error("pixel out of range"))?;
        decode_line(&mut reader, row)?;
    }
    Ok(())
}

pub(crate) fn decode_line(reader: &mut NibbleReader<'_>, row: &mut [u8]) -> Result<(), SpuError> {
    if row.is_empty() {
        return Ok(());
    }

    let width = row.len();
    let mut written = 0usize;
    while written < width {
        let (run, color, rest) = decode_run(reader)?;
        if rest {
            fill_rest(row, written, color)?;
            written = width;
        } else {
            written = fill_run(row, written, run, color)?;
        }
    }
    reader.consume_padding()
}

fn decode_run(reader: &mut NibbleReader<'_>) -> Result<(u16, u8, bool), SpuError> {
    let a = reader.read().ok_or_else(|| spu_error("unexpected end"))?;
    if a & 0x0C != 0 {
        return decode_short(a);
    }
    if a != 0 {
        return decode_medium(reader, a);
    }
    decode_long(reader)
}

fn decode_short(a: u8) -> Result<(u16, u8, bool), SpuError> {
    let run = u16::from(a >> 2);
    let color = a & 3;
    if run == 0 || run > 3 {
        return Err(spu_error("bad short run"));
    }
    Ok((run, color, false))
}

fn decode_medium(reader: &mut NibbleReader<'_>, a: u8) -> Result<(u16, u8, bool), SpuError> {
    let b = reader.read().ok_or_else(|| spu_error("unexpected end"))?;
    let run = (u16::from(a & 3) << 2) | u16::from(b >> 2);
    let color = b & 3;
    if !(4..=15).contains(&run) {
        return Err(spu_error("bad medium run"));
    }
    Ok((run, color, false))
}

fn decode_long(reader: &mut NibbleReader<'_>) -> Result<(u16, u8, bool), SpuError> {
    let b = reader.read().ok_or_else(|| spu_error("unexpected end"))?;
    if b == 0 {
        return decode_rest(reader);
    }
    if b >= 4 {
        return decode_12(reader, b);
    }
    decode_16(reader, b)
}

fn decode_rest(reader: &mut NibbleReader<'_>) -> Result<(u16, u8, bool), SpuError> {
    let c = reader.read().ok_or_else(|| spu_error("unexpected end"))?;
    let d = reader.read().ok_or_else(|| spu_error("unexpected end"))?;
    if c != 0 {
        return Err(spu_error("bad long code"));
    }
    Ok((0, d, true))
}

fn decode_12(reader: &mut NibbleReader<'_>, b: u8) -> Result<(u16, u8, bool), SpuError> {
    let c = reader.read().ok_or_else(|| spu_error("unexpected end"))?;
    let run = (u16::from(b) << 2) | u16::from(c >> 2);
    let color = c & 3;
    if !(16..=63).contains(&run) {
        return Err(spu_error("bad 12-bit run"));
    }
    Ok((run, color, false))
}

fn decode_16(reader: &mut NibbleReader<'_>, b: u8) -> Result<(u16, u8, bool), SpuError> {
    let c = reader.read().ok_or_else(|| spu_error("unexpected end"))?;
    let d = reader.read().ok_or_else(|| spu_error("unexpected end"))?;
    let run = (u16::from(b & 3) << 6) | (u16::from(c) << 2) | u16::from(d >> 2);
    let color = d & 3;
    if !(64..=255).contains(&run) {
        return Err(spu_error("bad 16-bit run"));
    }
    Ok((run, color, false))
}

fn fill_run(row: &mut [u8], written: usize, run: u16, color: u8) -> Result<usize, SpuError> {
    if run == 0 {
        return Err(spu_error("zero run"));
    }
    let run = usize::from(run);
    let end = written
        .checked_add(run)
        .ok_or_else(|| spu_error("run overflow"))?;
    if end > row.len() {
        return Err(spu_error("run exceeds line"));
    }
    let slice = row
        .get_mut(written..end)
        .ok_or_else(|| spu_error("row range"))?;
    slice.fill(color);
    Ok(end)
}

fn fill_rest(row: &mut [u8], written: usize, color: u8) -> Result<(), SpuError> {
    let slice = row
        .get_mut(written..row.len())
        .ok_or_else(|| spu_error("rest range"))?;
    slice.fill(color);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{NibbleReader, decode_line, encode_line, run_nibbles};
    use crate::{SpuError, spu_error};

    #[test]
    fn exact_short_run() -> Result<(), SpuError> {
        assert_eq!(run_nibbles(2, 1, true)?, vec![0b1001]);
        assert_eq!(encode_line(&[1, 1])?, vec![0x90]);
        Ok(())
    }

    #[test]
    fn exact_medium_run() -> Result<(), SpuError> {
        assert_eq!(run_nibbles(10, 3, true)?, vec![0x2, 0xB]);
        assert_eq!(encode_line(&[3u8; 10])?, vec![0x2B]);
        Ok(())
    }

    #[test]
    fn exact_12bit_run() -> Result<(), SpuError> {
        assert_eq!(run_nibbles(40, 2, true)?, vec![0x0, 0xA, 0x2]);
        assert_eq!(encode_line(&[2u8; 40])?, vec![0x0A, 0x20]);
        Ok(())
    }

    #[test]
    fn exact_16bit_run() -> Result<(), SpuError> {
        assert_eq!(run_nibbles(100, 0, true)?, vec![0x0, 0x1, 0x9, 0x0]);
        assert_eq!(encode_line(&[0u8; 100])?, vec![0x01, 0x90]);
        Ok(())
    }

    #[test]
    fn exact_rest_of_line() -> Result<(), SpuError> {
        assert_eq!(encode_line(&vec![1u8; 720])?, vec![0x00, 0x01]);
        Ok(())
    }

    #[test]
    fn line_padding() -> Result<(), SpuError> {
        assert_eq!(encode_line(&[1])?, vec![0x50]);
        Ok(())
    }

    #[test]
    fn decode_short_run() -> Result<(), SpuError> {
        let mut reader = NibbleReader::new(&[0x90], 0, 1).ok_or_else(|| spu_error("reader"))?;
        let mut row = vec![0u8; 2];
        decode_line(&mut reader, &mut row)?;
        assert_eq!(row, vec![1, 1]);
        Ok(())
    }
}
