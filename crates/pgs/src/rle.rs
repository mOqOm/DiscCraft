//! RLE codec for PGS object data.

use std::cmp::min;

use crate::PgsError;

const MAX_RUN: usize = 16383;

fn err(message: &str) -> PgsError {
    PgsError(message.to_string())
}

fn pixel_count(width: u16, height: u16) -> Result<usize, PgsError> {
    let w = usize::from(width);
    let h = usize::from(height);
    w.checked_mul(h).ok_or_else(|| err("image size overflow"))
}

fn push_usize_u8(out: &mut Vec<u8>, value: usize) -> Result<(), PgsError> {
    let value = u8::try_from(value).map_err(|_| err("value out of range"))?;
    out.push(value);
    Ok(())
}

fn push_long_zero(out: &mut Vec<u8>, len: usize) -> Result<(), PgsError> {
    let high = u8::try_from(len >> 8).map_err(|_| err("run length too large"))?;
    let low = u8::try_from(len & 0xFF).map_err(|_| err("run length too large"))?;

    out.push(0x00);
    out.push(0x40 | high);
    out.push(low);
    Ok(())
}

fn push_long_color(out: &mut Vec<u8>, color: u8, len: usize) -> Result<(), PgsError> {
    let high = u8::try_from(len >> 8).map_err(|_| err("run length too large"))?;
    let low = u8::try_from(len & 0xFF).map_err(|_| err("run length too large"))?;

    out.push(0x00);
    out.push(0xC0 | high);
    out.push(low);
    out.push(color);
    Ok(())
}

fn encode_zero_run(out: &mut Vec<u8>, mut len: usize) -> Result<(), PgsError> {
    while len > 0 {
        let chunk = min(len, MAX_RUN);

        if chunk <= 63 {
            out.push(0x00);
            push_usize_u8(out, chunk)?;
        } else {
            push_long_zero(out, chunk)?;
        }

        len = len
            .checked_sub(chunk)
            .ok_or_else(|| err("run length underflow"))?;
    }

    Ok(())
}

fn encode_color_run(out: &mut Vec<u8>, color: u8, mut len: usize) -> Result<(), PgsError> {
    while len > 0 {
        if len <= 2 {
            for _ in 0..len {
                out.push(color);
            }
            return Ok(());
        }

        let chunk = min(len, MAX_RUN);

        if chunk <= 63 {
            out.push(0x00);
            out.push(0x80 | u8::try_from(chunk).map_err(|_| err("run length out of range"))?);
            out.push(color);
        } else {
            push_long_color(out, color, chunk)?;
        }

        len = len
            .checked_sub(chunk)
            .ok_or_else(|| err("run length underflow"))?;
    }

    Ok(())
}

fn encode_run(out: &mut Vec<u8>, color: u8, len: usize) -> Result<(), PgsError> {
    if len == 0 {
        return Ok(());
    }

    if color == 0 {
        encode_zero_run(out, len)
    } else {
        encode_color_run(out, color, len)
    }
}

fn encode_row(row: &[u8], out: &mut Vec<u8>) -> Result<(), PgsError> {
    let mut current = 0u8;
    let mut run_len = 0usize;
    let mut started = false;

    for value in row.iter().copied() {
        if !started {
            current = value;
            run_len = 1;
            started = true;
        } else if value == current {
            run_len = run_len
                .checked_add(1)
                .ok_or_else(|| err("run length overflow"))?;
        } else {
            encode_run(out, current, run_len)?;
            current = value;
            run_len = 1;
        }
    }

    if started {
        encode_run(out, current, run_len)?;
    }

    out.push(0x00);
    out.push(0x00);
    Ok(())
}

/// Encode a palette-index bitmap as PGS RLE data.
pub fn rle_encode(width: u16, height: u16, pixels: &[u8]) -> Result<Vec<u8>, PgsError> {
    let total = pixel_count(width, height)?;
    if pixels.len() != total {
        return Err(err("pixel count mismatch"));
    }

    let mut out = Vec::new();

    if height == 0 {
        return Ok(out);
    }

    if width == 0 {
        for _ in 0..height {
            out.push(0x00);
            out.push(0x00);
        }
        return Ok(out);
    }

    let row_len = usize::from(width);
    for row in pixels.chunks_exact(row_len) {
        encode_row(row, &mut out)?;
    }

    Ok(out)
}

fn next_byte(data: &[u8], cursor: &mut usize) -> Result<u8, PgsError> {
    let value = data
        .get(*cursor)
        .copied()
        .ok_or_else(|| err("unexpected end of RLE data"))?;
    *cursor = cursor
        .checked_add(1)
        .ok_or_else(|| err("RLE cursor overflow"))?;
    Ok(value)
}

fn read_long_length(data: &[u8], cursor: &mut usize, high: u8) -> Result<usize, PgsError> {
    let low = next_byte(data, cursor)?;
    let len = (usize::from(high) << 8) | usize::from(low);

    if !(64..=MAX_RUN).contains(&len) {
        return Err(err("invalid long run length"));
    }

    Ok(len)
}

fn push_pixels(
    out: &mut Vec<u8>,
    color: u8,
    len: usize,
    row_len: usize,
    start_len: usize,
) -> Result<(), PgsError> {
    if len == 0 {
        return Err(err("zero length run"));
    }

    let line_len = out
        .len()
        .checked_sub(start_len)
        .ok_or_else(|| err("RLE output underflow"))?;
    let new_line_len = line_len
        .checked_add(len)
        .ok_or_else(|| err("RLE line overflow"))?;

    if new_line_len > row_len {
        return Err(err("RLE run exceeds line"));
    }

    let new_total = out
        .len()
        .checked_add(len)
        .ok_or_else(|| err("RLE output overflow"))?;
    out.resize(new_total, color);
    Ok(())
}

fn finish_line(out: &mut [u8], start_len: usize, row_len: usize) -> Result<(), PgsError> {
    let line_len = out
        .len()
        .checked_sub(start_len)
        .ok_or_else(|| err("RLE output underflow"))?;

    if line_len != row_len {
        return Err(err("RLE line length mismatch"));
    }

    Ok(())
}

fn decode_line(
    data: &[u8],
    cursor: &mut usize,
    row_len: usize,
    out: &mut Vec<u8>,
) -> Result<(), PgsError> {
    let start_len = out.len();

    while *cursor < data.len() {
        let first = next_byte(data, cursor)?;

        if first != 0 {
            push_pixels(out, first, 1, row_len, start_len)?;
            continue;
        }

        let second = next_byte(data, cursor)?;

        if second == 0 {
            finish_line(out, start_len, row_len)?;
            return Ok(());
        }

        let high = second & 0xC0;

        if high == 0x00 {
            let len = usize::from(second & 0x3F);
            push_pixels(out, 0, len, row_len, start_len)?;
        } else if high == 0x40 {
            let len = read_long_length(data, cursor, second & 0x3F)?;
            push_pixels(out, 0, len, row_len, start_len)?;
        } else if high == 0x80 {
            let len = usize::from(second & 0x3F);
            let color = next_byte(data, cursor)?;
            push_pixels(out, color, len, row_len, start_len)?;
        } else {
            let len = read_long_length(data, cursor, second & 0x3F)?;
            let color = next_byte(data, cursor)?;
            push_pixels(out, color, len, row_len, start_len)?;
        }
    }

    Err(err("missing end of line"))
}

/// Decode PGS RLE data into a palette-index bitmap.
pub fn rle_decode(width: u16, height: u16, data: &[u8]) -> Result<Vec<u8>, PgsError> {
    let total = pixel_count(width, height)?;
    let mut out = Vec::new();
    let mut cursor = 0usize;
    let row_len = usize::from(width);

    for _ in 0..height {
        decode_line(data, &mut cursor, row_len, &mut out)?;
    }

    if cursor != data.len() {
        return Err(err("trailing RLE data"));
    }

    if out.len() != total {
        return Err(err("decoded pixel count mismatch"));
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{rle_decode, rle_encode};

    #[test]
    fn exact_short_line() {
        let pixels = vec![0u8, 0, 0, 5, 5, 5, 1];
        let encoded = rle_encode(7, 1, &pixels).unwrap_or_default();

        let expected = vec![0x00u8, 0x03, 0x00, 0x83, 0x05, 0x01, 0x00, 0x00];
        assert_eq!(encoded, expected);
    }

    #[test]
    fn round_trip_10x3_short_runs() {
        let pixels = vec![
            0u8, 0, 1, 2, 2, 3, 3, 3, 4, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1,
        ];

        let encoded = rle_encode(10, 3, &pixels).unwrap_or_default();
        let decoded = rle_decode(10, 3, &encoded).unwrap_or_default();

        assert_eq!(decoded, pixels);
    }

    #[test]
    fn round_trip_all_run_kinds_including_70() {
        let mut row = vec![0u8; 10];
        row.push(1);
        row.extend(vec![0u8; 70]);
        row.push(2);
        row.extend(vec![5u8; 10]);
        row.push(3);
        row.extend(vec![6u8; 70]);
        row.push(4);

        let width = u16::try_from(row.len()).unwrap_or_default();

        let encoded = rle_encode(width, 1, &row).unwrap_or_default();
        let decoded = rle_decode(width, 1, &encoded).unwrap_or_default();

        assert_eq!(decoded, row);
    }
}
