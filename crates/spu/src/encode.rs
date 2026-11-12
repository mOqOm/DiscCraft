//! SPU encoder.

use crate::{
    Bitmap, MAX_HEIGHT, MAX_SPU_SIZE, MAX_WIDTH, SpuError, SpuParams, rle::encode_line, spu_error,
};

const HEADER_SIZE: usize = 4;
const DCSQ1_LEN: usize = 24;
const DCSQ2_LEN: usize = 6;

struct Layout {
    total_size: usize,
    dcsq1_offset: usize,
    dcsq2_offset: usize,
    top_offset: usize,
    bottom_offset: usize,
}

/// Encode a bitmap and parameters into an SPU byte string.
pub fn encode_spu(bitmap: &Bitmap, params: &SpuParams) -> Result<Vec<u8>, SpuError> {
    validate_inputs(bitmap, params)?;
    let (top_data, bottom_data) = encode_fields(bitmap)?;
    let layout = compute_layout(
        top_data.len(),
        bottom_data.len(),
        params.stop_delay.is_some(),
    )?;

    let mut out = Vec::with_capacity(layout.total_size);
    push_header(&mut out, layout.total_size, layout.dcsq1_offset)?;
    out.extend_from_slice(&top_data);
    out.extend_from_slice(&bottom_data);
    push_dcsq1(&mut out, bitmap, params, &layout)?;

    if let Some(delay) = params.stop_delay {
        push_dcsq2(&mut out, delay, layout.dcsq2_offset)?;
    }
    Ok(out)
}

fn validate_inputs(bitmap: &Bitmap, params: &SpuParams) -> Result<(), SpuError> {
    validate_area(bitmap, params)?;
    validate_pixels(bitmap)?;
    validate_params(params)?;
    Ok(())
}

fn validate_area(bitmap: &Bitmap, params: &SpuParams) -> Result<(), SpuError> {
    if bitmap.width == 0 || bitmap.height == 0 {
        return Err(spu_error("empty bitmap"));
    }
    let x_end = params
        .x
        .checked_add(bitmap.width)
        .ok_or_else(|| spu_error("x overflow"))?;
    let y_end = params
        .y
        .checked_add(bitmap.height)
        .ok_or_else(|| spu_error("y overflow"))?;
    if x_end > MAX_WIDTH || y_end > MAX_HEIGHT {
        return Err(spu_error("area too large"));
    }
    Ok(())
}

fn validate_pixels(bitmap: &Bitmap) -> Result<(), SpuError> {
    let len = usize::from(bitmap.width)
        .checked_mul(usize::from(bitmap.height))
        .ok_or_else(|| spu_error("pixel overflow"))?;
    if bitmap.pixels.len() != len {
        return Err(spu_error("pixel length mismatch"));
    }
    for &p in &bitmap.pixels {
        if p > 3 {
            return Err(spu_error("pixel value out of range"));
        }
    }
    Ok(())
}

fn validate_params(params: &SpuParams) -> Result<(), SpuError> {
    validate_nibbles(params.colors)?;
    validate_nibbles(params.alpha)?;
    Ok(())
}

fn validate_nibbles(values: [u8; 4]) -> Result<(), SpuError> {
    let [a, b, c, d] = values;
    if a > 15 || b > 15 || c > 15 || d > 15 {
        return Err(spu_error("nibble out of range"));
    }
    Ok(())
}

fn encode_fields(bitmap: &Bitmap) -> Result<(Vec<u8>, Vec<u8>), SpuError> {
    let height = usize::from(bitmap.height);
    let top_count = height.div_ceil(2);
    let bottom_count = height / 2;
    let top = encode_field(bitmap, 0, top_count)?;
    let bottom = encode_field(bitmap, 1, bottom_count)?;
    Ok((top, bottom))
}

fn encode_field(
    bitmap: &Bitmap,
    start_line: usize,
    line_count: usize,
) -> Result<Vec<u8>, SpuError> {
    let mut out = Vec::new();
    let width = usize::from(bitmap.width);

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
        let row = bitmap
            .pixels
            .get(start..end)
            .ok_or_else(|| spu_error("pixel out of range"))?;
        let encoded = encode_line(row)?;
        out.extend_from_slice(&encoded);
        if out.len() > MAX_SPU_SIZE {
            return Err(spu_error("SPU too large"));
        }
    }
    Ok(out)
}

fn compute_layout(top_len: usize, bottom_len: usize, has_stop: bool) -> Result<Layout, SpuError> {
    let top_offset = HEADER_SIZE;
    let bottom_offset = top_offset
        .checked_add(top_len)
        .ok_or_else(|| spu_error("size overflow"))?;
    let pixel_end = bottom_offset
        .checked_add(bottom_len)
        .ok_or_else(|| spu_error("size overflow"))?;
    let dcsq1_offset = pixel_end;

    let dcsq2_offset = if has_stop {
        dcsq1_offset
            .checked_add(DCSQ1_LEN)
            .ok_or_else(|| spu_error("size overflow"))?
    } else {
        dcsq1_offset
    };

    let total_size = if has_stop {
        dcsq2_offset
            .checked_add(DCSQ2_LEN)
            .ok_or_else(|| spu_error("size overflow"))?
    } else {
        dcsq1_offset
            .checked_add(DCSQ1_LEN)
            .ok_or_else(|| spu_error("size overflow"))?
    };

    if total_size > MAX_SPU_SIZE {
        return Err(spu_error("SPU too large"));
    }

    Ok(Layout {
        total_size,
        dcsq1_offset,
        dcsq2_offset,
        top_offset,
        bottom_offset,
    })
}

fn push_header(out: &mut Vec<u8>, total_size: usize, first_dcsq: usize) -> Result<(), SpuError> {
    push_u16_usize(out, total_size)?;
    push_u16_usize(out, first_dcsq)?;
    Ok(())
}

fn push_dcsq1(
    out: &mut Vec<u8>,
    bitmap: &Bitmap,
    params: &SpuParams,
    layout: &Layout,
) -> Result<(), SpuError> {
    push_u16(out, 0);
    let next = if params.stop_delay.is_some() {
        layout.dcsq2_offset
    } else {
        layout.dcsq1_offset
    };
    push_u16_usize(out, next)?;

    push_set_color(out, params);
    push_set_contr(out, params);
    push_set_darea(out, params.x, params.y, bitmap.width, bitmap.height)?;
    push_set_dspxa(out, layout.top_offset, layout.bottom_offset)?;
    push_u8(out, if params.forced { 0x00 } else { 0x01 });
    push_u8(out, 0xFF);
    Ok(())
}

fn push_dcsq2(out: &mut Vec<u8>, delay: u16, offset: usize) -> Result<(), SpuError> {
    push_u16(out, delay);
    push_u16_usize(out, offset)?;
    push_u8(out, 0x02);
    push_u8(out, 0xFF);
    Ok(())
}

fn push_set_color(out: &mut Vec<u8>, params: &SpuParams) {
    let [c0, c1, c2, c3] = params.colors;
    push_u8(out, 0x03);
    push_u8(out, (c3 << 4) | c2);
    push_u8(out, (c1 << 4) | c0);
}

fn push_set_contr(out: &mut Vec<u8>, params: &SpuParams) {
    let [a0, a1, a2, a3] = params.alpha;
    push_u8(out, 0x04);
    push_u8(out, (a3 << 4) | a2);
    push_u8(out, (a1 << 4) | a0);
}

fn push_set_darea(
    out: &mut Vec<u8>,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
) -> Result<(), SpuError> {
    let x2 = x
        .checked_add(width)
        .and_then(|v| v.checked_sub(1))
        .ok_or_else(|| spu_error("area overflow"))?;
    let y2 = y
        .checked_add(height)
        .and_then(|v| v.checked_sub(1))
        .ok_or_else(|| spu_error("area overflow"))?;

    push_u8(out, 0x05);
    out.extend_from_slice(&area_bytes(x, x2, y, y2));
    Ok(())
}

fn push_set_dspxa(out: &mut Vec<u8>, top: usize, bottom: usize) -> Result<(), SpuError> {
    push_u8(out, 0x06);
    push_u16_usize(out, top)?;
    push_u16_usize(out, bottom)?;
    Ok(())
}

fn area_bytes(x1: u16, x2: u16, y1: u16, y2: u16) -> [u8; 6] {
    [
        (x1 >> 4) as u8,
        (((x1 & 0xF) << 4) | (x2 >> 8)) as u8,
        (x2 & 0xFF) as u8,
        (y1 >> 4) as u8,
        (((y1 & 0xF) << 4) | (y2 >> 8)) as u8,
        (y2 & 0xFF) as u8,
    ]
}

fn push_u16_usize(out: &mut Vec<u8>, value: usize) -> Result<(), SpuError> {
    let v = u16::try_from(value).map_err(|_| spu_error("offset too large"))?;
    push_u16(out, v);
    Ok(())
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    push_u8(out, (value >> 8) as u8);
    push_u8(out, (value & 0xFF) as u8);
}

fn push_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

#[cfg(test)]
mod tests {
    use crate::{Bitmap, SpuError, SpuParams, decode_spu, encode_spu};

    fn test_params(forced: bool, stop: Option<u16>) -> SpuParams {
        SpuParams {
            x: 0,
            y: 0,
            colors: [0, 1, 2, 3],
            alpha: [0, 5, 10, 15],
            forced,
            stop_delay: stop,
        }
    }

    fn lcg(state: &mut u32) -> u32 {
        *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *state
    }

    fn test_bitmap(width: u16, height: u16) -> Bitmap {
        let mut pixels = Vec::new();
        let mut state = 0x1234_5678u32;
        for _ in 0..height {
            for _ in 0..width {
                let v = ((lcg(&mut state) >> 28) & 3) as u8;
                pixels.push(v);
            }
        }
        Bitmap {
            width,
            height,
            pixels,
        }
    }

    fn roundtrip(width: u16, height: u16, forced: bool, stop: Option<u16>) -> Result<(), SpuError> {
        let bitmap = test_bitmap(width, height);
        let params = test_params(forced, stop);
        let data = encode_spu(&bitmap, &params)?;
        let decoded = decode_spu(&data)?;

        assert_eq!(decoded.bitmap.width, width);
        assert_eq!(decoded.bitmap.height, height);
        assert_eq!(decoded.bitmap.pixels, bitmap.pixels);
        assert_eq!(decoded.params.x, params.x);
        assert_eq!(decoded.params.y, params.y);
        assert_eq!(decoded.params.colors, params.colors);
        assert_eq!(decoded.params.alpha, params.alpha);
        assert_eq!(decoded.params.forced, forced);
        assert_eq!(decoded.params.stop_delay, stop);
        Ok(())
    }

    #[test]
    fn roundtrip_sizes() -> Result<(), SpuError> {
        roundtrip(1, 1, false, None)?;
        roundtrip(720, 1, true, Some(0))?;
        roundtrip(35, 5, false, Some(123))?;
        roundtrip(1, 576, true, None)?;
        roundtrip(720, 3, false, Some(65_535))?;
        Ok(())
    }

    #[test]
    fn rejects_pixel_value() -> Result<(), SpuError> {
        let bitmap = Bitmap {
            width: 1,
            height: 1,
            pixels: vec![4],
        };
        assert!(encode_spu(&bitmap, &test_params(false, None)).is_err());
        Ok(())
    }

    #[test]
    fn rejects_area() -> Result<(), SpuError> {
        let bitmap = Bitmap {
            width: 720,
            height: 1,
            pixels: vec![0; 720],
        };
        let mut params = test_params(false, None);
        params.x = 1;
        assert!(encode_spu(&bitmap, &params).is_err());
        Ok(())
    }

    #[test]
    fn rejects_oversized() -> Result<(), SpuError> {
        let len = 720usize * 576usize;
        let mut pixels = Vec::with_capacity(len);
        for i in 0..len {
            pixels.push((i % 2) as u8);
        }
        let bitmap = Bitmap {
            width: 720,
            height: 576,
            pixels,
        };
        assert!(encode_spu(&bitmap, &test_params(false, None)).is_err());
        Ok(())
    }
}
