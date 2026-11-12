use crate::bitwriter::BitWriter;
use crate::dct::forward_dct8x8;
use crate::{EncodeError, Frame420, Standard, StillOptions};

const INTRA_MATRIX: [[u16; 8]; 8] = [
    [8, 16, 19, 22, 26, 27, 29, 34],
    [16, 16, 22, 24, 27, 29, 34, 37],
    [19, 22, 26, 27, 29, 34, 34, 38],
    [22, 22, 26, 27, 29, 34, 37, 40],
    [22, 26, 27, 29, 32, 35, 40, 48],
    [26, 27, 29, 32, 35, 40, 48, 58],
    [26, 27, 29, 34, 38, 46, 56, 69],
    [27, 29, 35, 38, 46, 56, 69, 83],
];

const ZIGZAG: [(usize, usize); 64] = [
    (0, 0),
    (0, 1),
    (1, 0),
    (2, 0),
    (1, 1),
    (0, 2),
    (0, 3),
    (1, 2),
    (2, 1),
    (3, 0),
    (4, 0),
    (3, 1),
    (2, 2),
    (1, 3),
    (0, 4),
    (0, 5),
    (1, 4),
    (2, 3),
    (3, 2),
    (4, 1),
    (5, 0),
    (6, 0),
    (5, 1),
    (4, 2),
    (3, 3),
    (2, 4),
    (1, 5),
    (0, 6),
    (0, 7),
    (1, 6),
    (2, 5),
    (3, 4),
    (4, 3),
    (5, 2),
    (6, 1),
    (7, 0),
    (7, 1),
    (6, 2),
    (5, 3),
    (4, 4),
    (3, 5),
    (2, 6),
    (1, 7),
    (2, 7),
    (3, 6),
    (4, 5),
    (5, 4),
    (6, 3),
    (7, 2),
    (7, 3),
    (6, 4),
    (5, 5),
    (4, 6),
    (3, 7),
    (4, 7),
    (5, 6),
    (6, 5),
    (7, 4),
    (7, 5),
    (6, 6),
    (5, 7),
    (6, 7),
    (7, 6),
    (7, 7),
];

const LUMA_DC_SIZE: [(u32, u8); 12] = [
    (0b100, 3),
    (0b00, 2),
    (0b01, 2),
    (0b101, 3),
    (0b110, 3),
    (0b1110, 4),
    (0b11110, 5),
    (0b111110, 6),
    (0b1111110, 7),
    (0b11111110, 8),
    (0b111111110, 9),
    (0b111111111, 9),
];

const CHROMA_DC_SIZE: [(u32, u8); 12] = [
    (0b00, 2),
    (0b01, 2),
    (0b10, 2),
    (0b110, 3),
    (0b1110, 4),
    (0b11110, 5),
    (0b111110, 6),
    (0b1111110, 7),
    (0b11111110, 8),
    (0b111111110, 9),
    (0b1111111110, 10),
    (0b1111111111, 10),
];

struct Predictors {
    y: u16,
    cb: u16,
    cr: u16,
}

impl Predictors {
    fn new() -> Self {
        Self {
            y: 128,
            cb: 128,
            cr: 128,
        }
    }
}

/// Encode one still picture, retrying with coarser quantisation if needed.
pub(crate) fn encode_still(
    frame: &Frame420,
    options: &StillOptions,
) -> Result<Vec<u8>, EncodeError> {
    validate(frame, options)?;

    let mut code = options.quantiser_scale_code;
    loop {
        let data = encode_with(frame, options.standard, options.wide, code);
        if data.len() <= options.max_bytes {
            return Ok(data);
        }

        if code >= 31 {
            return Err(EncodeError(format!(
                "encoded size {} exceeds max_bytes {}",
                data.len(),
                options.max_bytes
            )));
        }

        code = code.saturating_add(2);
        if code > 31 {
            code = 31;
        }
    }
}

fn validate(frame: &Frame420, options: &StillOptions) -> Result<(), EncodeError> {
    if options.quantiser_scale_code == 0 || options.quantiser_scale_code > 31 {
        return Err(EncodeError(
            "quantiser_scale_code must be 1..=31".to_string(),
        ));
    }

    let w = usize::from(frame.width);
    let h = usize::from(frame.height);
    let expected_h = match options.standard {
        Standard::Ntsc => 480,
        Standard::Pal => 576,
    };

    if w != 720 || h != expected_h {
        return Err(EncodeError(format!(
            "expected 720x{expected_h}, got {w}x{h}"
        )));
    }

    let y_len = w
        .checked_mul(h)
        .ok_or_else(|| EncodeError("invalid frame size".to_string()))?;
    let cw = w / 2;
    let ch = h / 2;
    let c_len = cw
        .checked_mul(ch)
        .ok_or_else(|| EncodeError("invalid chroma size".to_string()))?;

    if frame.y.len() != y_len || frame.cb.len() != c_len || frame.cr.len() != c_len {
        return Err(EncodeError("plane length mismatch".to_string()));
    }

    Ok(())
}

fn encode_with(frame: &Frame420, standard: Standard, wide: bool, code: u8) -> Vec<u8> {
    let mut writer = BitWriter::new();

    write_sequence_header(&mut writer, frame, standard, wide);
    write_sequence_extension(&mut writer);
    write_gop_header(&mut writer);
    write_picture_header(&mut writer);
    write_picture_coding_extension(&mut writer);
    write_slices(&mut writer, frame, code);

    writer.write_start_code(0xB7);
    writer.finish()
}

fn write_sequence_header(writer: &mut BitWriter, frame: &Frame420, standard: Standard, wide: bool) {
    let frame_rate_code = match standard {
        Standard::Ntsc => 4,
        Standard::Pal => 3,
    };

    writer.write_start_code(0xB3);
    writer.write_bits(u32::from(frame.width), 12);
    writer.write_bits(u32::from(frame.height), 12);
    // aspect_ratio_information: 2 = 4:3, 3 = 16:9.
    writer.write_bits(if wide { 3 } else { 2 }, 4);
    writer.write_bits(frame_rate_code, 4);
    writer.write_bits(24500, 18);
    writer.write_bits(1, 1);
    writer.write_bits(112, 10);
    writer.write_bits(0, 1);
    writer.write_bits(0, 1);
    writer.write_bits(0, 1);
}

fn write_sequence_extension(writer: &mut BitWriter) {
    writer.write_start_code(0xB5);
    writer.write_bits(1, 4);
    writer.write_bits(0x48, 8);
    writer.write_bits(0, 1);
    writer.write_bits(1, 2);
    writer.write_bits(0, 2);
    writer.write_bits(0, 2);
    writer.write_bits(0, 12);
    writer.write_bits(1, 1);
    writer.write_bits(0, 8);
    writer.write_bits(0, 1);
    writer.write_bits(0, 2);
    writer.write_bits(0, 5);
}

fn write_gop_header(writer: &mut BitWriter) {
    writer.write_start_code(0xB8);
    writer.write_bits(0, 1);
    writer.write_bits(0, 5);
    writer.write_bits(0, 6);
    writer.write_bits(1, 1);
    writer.write_bits(0, 6);
    writer.write_bits(0, 6);
    writer.write_bits(1, 1);
    writer.write_bits(0, 1);
}

fn write_picture_header(writer: &mut BitWriter) {
    writer.write_start_code(0x00);
    writer.write_bits(0, 10);
    writer.write_bits(1, 3);
    writer.write_bits(0xFFFF, 16);
    writer.write_bits(0, 1);
}

fn write_picture_coding_extension(writer: &mut BitWriter) {
    writer.write_start_code(0xB5);
    writer.write_bits(8, 4);
    writer.write_bits(15, 4);
    writer.write_bits(15, 4);
    writer.write_bits(15, 4);
    writer.write_bits(15, 4);
    writer.write_bits(0, 2);
    writer.write_bits(3, 2);
    writer.write_bits(1, 1);
    writer.write_bits(1, 1);
    writer.write_bits(0, 1);
    writer.write_bits(0, 1);
    writer.write_bits(0, 1);
    writer.write_bits(0, 1);
    writer.write_bits(0, 1);
    writer.write_bits(1, 1);
    writer.write_bits(1, 1);
    writer.write_bits(0, 1);
}

fn write_slices(writer: &mut BitWriter, frame: &Frame420, code: u8) {
    let mb_rows = usize::from(frame.height) / 16;
    let mb_cols = usize::from(frame.width) / 16;
    let quantiser_scale = f64::from(code) * 2.0;

    for row in 0..mb_rows {
        write_slice_header(writer, row, code);
        let mut predictors = Predictors::new();

        for col in 0..mb_cols {
            encode_macroblock(writer, frame, row, col, quantiser_scale, &mut predictors);
        }
    }
}

fn write_slice_header(writer: &mut BitWriter, row: usize, code: u8) {
    let slice_code = u8::try_from(row + 1).unwrap_or(1);
    writer.write_start_code(slice_code);
    writer.write_bits(u32::from(code), 5);
    writer.write_bits(0, 1);
}

fn encode_macroblock(
    writer: &mut BitWriter,
    frame: &Frame420,
    row: usize,
    col: usize,
    quantiser_scale: f64,
    predictors: &mut Predictors,
) {
    writer.write_bits(1, 1);
    writer.write_bits(1, 1);

    let x = col * 16;
    let y = row * 16;

    encode_y_block(writer, frame, x, y, quantiser_scale, &mut predictors.y);
    encode_y_block(writer, frame, x + 8, y, quantiser_scale, &mut predictors.y);
    encode_y_block(writer, frame, x, y + 8, quantiser_scale, &mut predictors.y);
    encode_y_block(
        writer,
        frame,
        x + 8,
        y + 8,
        quantiser_scale,
        &mut predictors.y,
    );

    let chroma_width = usize::from(frame.width) / 2;
    encode_chroma_block(
        writer,
        &frame.cb,
        chroma_width,
        x / 2,
        y / 2,
        quantiser_scale,
        &mut predictors.cb,
    );
    encode_chroma_block(
        writer,
        &frame.cr,
        chroma_width,
        x / 2,
        y / 2,
        quantiser_scale,
        &mut predictors.cr,
    );
}

fn encode_y_block(
    writer: &mut BitWriter,
    frame: &Frame420,
    x: usize,
    y: usize,
    quantiser_scale: f64,
    predictor: &mut u16,
) {
    let block = block8x8(&frame.y, usize::from(frame.width), x, y);
    encode_block(writer, &block, quantiser_scale, true, predictor);
}

fn encode_chroma_block(
    writer: &mut BitWriter,
    plane: &[u8],
    width: usize,
    x: usize,
    y: usize,
    quantiser_scale: f64,
    predictor: &mut u16,
) {
    let block = block8x8(plane, width, x, y);
    encode_block(writer, &block, quantiser_scale, false, predictor);
}

fn encode_block(
    writer: &mut BitWriter,
    samples: &[u8; 64],
    quantiser_scale: f64,
    is_luma: bool,
    predictor: &mut u16,
) {
    let coeffs = forward_dct8x8(samples);
    let dc = quantise_dc(&coeffs);
    let diff = i32::from(dc) - i32::from(*predictor);
    *predictor = dc;

    write_dc_diff(writer, diff, is_luma);
    write_ac(writer, &coeffs, quantiser_scale);
}

fn block8x8(plane: &[u8], width: usize, x: usize, y: usize) -> [u8; 64] {
    let mut block = [0u8; 64];

    for row in 0..8 {
        let start = (y + row).saturating_mul(width).saturating_add(x);
        let end = start.saturating_add(8);

        if let Some(chunk) = plane.get(start..end) {
            for (col, &value) in chunk.iter().enumerate() {
                if let Some(slot) = block.get_mut(row * 8 + col) {
                    *slot = value;
                }
            }
        }
    }

    block
}

fn quantise_dc(coeffs: &[f64; 64]) -> u16 {
    let dc = coeffs.first().copied().unwrap_or(0.0);
    round_to_i32(dc / 8.0, 0, 255) as u16
}

fn write_dc_diff(writer: &mut BitWriter, diff: i32, is_luma: bool) {
    let size = diff_size(diff);
    write_dc_size(writer, size, is_luma);

    if size == 0 {
        return;
    }

    let abs = diff.unsigned_abs();
    let value = if diff > 0 {
        abs
    } else {
        (1u32 << u32::from(size)) - 1 - abs
    };

    writer.write_bits(value, size);
}

fn write_dc_size(writer: &mut BitWriter, size: u8, is_luma: bool) {
    let table = if is_luma {
        &LUMA_DC_SIZE
    } else {
        &CHROMA_DC_SIZE
    };

    if let Some(&(code, len)) = table.get(size as usize) {
        writer.write_bits(code, len);
    }
}

fn diff_size(diff: i32) -> u8 {
    if diff == 0 {
        return 0;
    }

    let mut value = diff.unsigned_abs();
    let mut size = 0u8;
    while value != 0 {
        size += 1;
        value >>= 1;
    }
    size
}

fn write_ac(writer: &mut BitWriter, coeffs: &[f64; 64], quantiser_scale: f64) {
    let mut run = 0u32;

    for index in 1..64 {
        if let Some(&(u, v)) = ZIGZAG.get(index) {
            let level = ac_level(coeffs, u, v, quantiser_scale);
            if level != 0 {
                write_escape(writer, run, level);
                run = 0;
            } else {
                run += 1;
            }
        }
    }

    writer.write_bits(2, 2);
}

fn ac_level(coeffs: &[f64; 64], u: usize, v: usize, quantiser_scale: f64) -> i32 {
    let coeff = coeffs.get(u * 8 + v).copied().unwrap_or(0.0);
    let weight = intra_weight(u, v);
    let denominator = weight * quantiser_scale;

    if denominator == 0.0 {
        return 0;
    }

    round_to_i32(16.0 * coeff / denominator, -2047, 2047)
}

fn intra_weight(u: usize, v: usize) -> f64 {
    INTRA_MATRIX
        .get(u)
        .and_then(|row| row.get(v))
        .copied()
        .map(f64::from)
        .unwrap_or(1.0)
}

fn round_to_i32(value: f64, min: i32, max: i32) -> i32 {
    let rounded = value.round();
    if rounded <= f64::from(min) {
        min
    } else if rounded >= f64::from(max) {
        max
    } else {
        rounded as i32
    }
}

fn write_escape(writer: &mut BitWriter, run: u32, level: i32) {
    writer.write_bits(1, 6);
    writer.write_bits(run & 0x3F, 6);
    writer.write_bits((level as u32) & 0xFFF, 12);
}

#[cfg(test)]
mod tests {
    use super::{diff_size, write_dc_diff};
    use crate::bitwriter::BitWriter;

    fn encode_dc(diff: i32, is_luma: bool) -> Vec<u8> {
        let mut writer = BitWriter::new();
        write_dc_diff(&mut writer, diff, is_luma);
        writer.finish()
    }

    #[test]
    fn diff_sizes() {
        assert_eq!(diff_size(0), 0);
        assert_eq!(diff_size(1), 1);
        assert_eq!(diff_size(-1), 1);
        assert_eq!(diff_size(255), 8);
        assert_eq!(diff_size(-255), 8);
    }

    #[test]
    fn luminance_dc_codes() {
        assert_eq!(encode_dc(0, true), vec![0x80]);
        assert_eq!(encode_dc(1, true), vec![0x20]);
        assert_eq!(encode_dc(-1, true), vec![0x00]);
        assert_eq!(encode_dc(255, true), vec![0xFD, 0xFE]);
        assert_eq!(encode_dc(-255, true), vec![0xFC, 0x00]);
    }

    #[test]
    fn chroma_dc_codes() {
        assert_eq!(encode_dc(0, false), vec![0x00]);
        assert_eq!(encode_dc(1, false), vec![0x60]);
        assert_eq!(encode_dc(-1, false), vec![0x40]);
        assert_eq!(encode_dc(255, false), vec![0xFE, 0xFF]);
        assert_eq!(encode_dc(-255, false), vec![0xFE, 0x00]);
    }
}
