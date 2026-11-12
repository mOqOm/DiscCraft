//! MPEG-2 quantisation, dequantisation, and inverse DCT helpers.
#![allow(dead_code)]

use std::sync::OnceLock;

const INV_SQRT2: f64 = 1.0 / std::f64::consts::SQRT_2;

static COS_TABLE: OnceLock<[[f64; 8]; 8]> = OnceLock::new();

/// Default MPEG-2 intra quantiser matrix in row-major order.
pub(crate) const DEFAULT_INTRA_MATRIX: [u8; 64] = [
    8, 16, 19, 22, 26, 27, 29, 34, 16, 16, 22, 24, 27, 29, 34, 37, 19, 22, 26, 27, 29, 34, 34, 38,
    22, 22, 26, 27, 29, 34, 37, 40, 22, 26, 27, 29, 32, 35, 40, 48, 26, 27, 29, 32, 35, 40, 48, 58,
    26, 27, 29, 34, 38, 46, 56, 69, 27, 29, 35, 38, 46, 56, 69, 83,
];

/// Default MPEG-2 non-intra quantiser matrix in row-major order.
pub(crate) const DEFAULT_NON_INTRA: [u8; 64] = [16; 64];

/// Zig-zag scan table: scan index to row-major coefficient position.
pub(crate) const ZIGZAG: [u8; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// Quantise intra coefficients. Output levels are in zig-zag scan order.
pub(crate) fn quantise_intra(coef: &[f64; 64], qs: u16) -> [i16; 64] {
    let mut levels = [0i16; 64];

    let dc = coef.first().copied().unwrap_or(0.0);
    if let Some(slot) = levels.get_mut(0) {
        *slot = clamp_round_i16(dc / 8.0, 0, 255);
    }

    if qs == 0 {
        return levels;
    }

    for scan in 1usize..64 {
        if let Some(level) = quantise_intra_ac(coef, qs, scan)
            && let Some(slot) = levels.get_mut(scan)
        {
            *slot = level;
        }
    }

    levels
}

/// Quantise non-intra coefficients. Output levels are in zig-zag scan order.
pub(crate) fn quantise_non_intra(coef: &[f64; 64], qs: u16) -> [i16; 64] {
    let mut levels = [0i16; 64];

    if qs == 0 {
        return levels;
    }

    for scan in 0usize..64 {
        if let Some(level) = quantise_non_intra_level(coef, qs, scan)
            && let Some(slot) = levels.get_mut(scan)
        {
            *slot = level;
        }
    }

    levels
}

/// Dequantise intra levels from zig-zag scan order to row-major frequency coefficients.
pub(crate) fn dequantise_intra(levels: &[i16; 64], qs: u16) -> [i32; 64] {
    let mut f = [0i32; 64];

    let dc = levels.first().copied().unwrap_or(0);
    if let Some(slot) = f.get_mut(0) {
        *slot = saturate_i32(i64::from(dc) * 8);
    }

    for scan in 1usize..64 {
        if let Some((pos, value)) = dequantise_intra_slot(levels, qs, scan)
            && let Some(slot) = f.get_mut(pos)
        {
            *slot = value;
        }
    }

    apply_mismatch_control(&mut f);
    f
}

/// Dequantise non-intra levels from zig-zag scan order to row-major frequency coefficients.
pub(crate) fn dequantise_non_intra(levels: &[i16; 64], qs: u16) -> [i32; 64] {
    let mut f = [0i32; 64];

    for scan in 0usize..64 {
        if let Some((pos, value)) = dequantise_non_intra_slot(levels, qs, scan)
            && let Some(slot) = f.get_mut(pos)
        {
            *slot = value;
        }
    }

    apply_mismatch_control(&mut f);
    f
}

/// Separable orthonormal inverse 8x8 DCT. Input and output are row-major.
pub(crate) fn idct8x8(coef: &[i32; 64]) -> [i32; 64] {
    let temp = idct_horizontal(coef);
    idct_vertical(&temp)
}

/// Reconstruct an intra block from zig-zag levels.
pub(crate) fn reconstruct_intra(levels: &[i16; 64], qs: u16) -> [u8; 64] {
    let f = dequantise_intra(levels, qs);
    let samples = idct8x8(&f);
    let mut out = [0u8; 64];

    for (i, &value) in samples.iter().enumerate() {
        let clamped = value.clamp(0, 255);

        if let Some(slot) = out.get_mut(i) {
            *slot = u8::try_from(clamped).unwrap_or(0);
        }
    }

    out
}

/// Reconstruct a residual block from zig-zag levels.
pub(crate) fn reconstruct_residual(levels: &[i16; 64], qs: u16) -> [i32; 64] {
    let f = dequantise_non_intra(levels, qs);
    idct8x8(&f)
}

fn quantise_intra_ac(coef: &[f64; 64], qs: u16, scan: usize) -> Option<i16> {
    let pos = usize::from(ZIGZAG.get(scan).copied()?);
    let coeff = coef.get(pos).copied()?;
    let weight = f64::from(DEFAULT_INTRA_MATRIX.get(pos).copied().unwrap_or(1));
    let denominator = weight * f64::from(qs);

    if denominator == 0.0 {
        return Some(0);
    }

    Some(clamp_round_i16(16.0 * coeff / denominator, -2047, 2047))
}

fn quantise_non_intra_level(coef: &[f64; 64], qs: u16, scan: usize) -> Option<i16> {
    let pos = usize::from(ZIGZAG.get(scan).copied()?);
    let coeff = coef.get(pos).copied()?;
    let weight = f64::from(DEFAULT_NON_INTRA.get(pos).copied().unwrap_or(1));
    let denominator = weight * f64::from(qs);

    if denominator == 0.0 {
        return Some(0);
    }

    Some(clamp_trunc_i16(16.0 * coeff / denominator, -2047, 2047))
}

fn dequantise_intra_slot(levels: &[i16; 64], qs: u16, scan: usize) -> Option<(usize, i32)> {
    let pos = usize::from(ZIGZAG.get(scan).copied()?);
    let level = levels.get(scan).copied()?;
    let weight = i64::from(DEFAULT_INTRA_MATRIX.get(pos).copied().unwrap_or(1));
    let numerator = 2 * i64::from(level) * weight * i64::from(qs);

    Some((pos, saturate_i32(numerator / 32)))
}

fn dequantise_non_intra_slot(levels: &[i16; 64], qs: u16, scan: usize) -> Option<(usize, i32)> {
    let pos = usize::from(ZIGZAG.get(scan).copied()?);
    let level = levels.get(scan).copied()?;

    if level == 0 {
        return Some((pos, 0));
    }

    let weight = i64::from(DEFAULT_NON_INTRA.get(pos).copied().unwrap_or(1));
    let sign = if level > 0 { 1i64 } else { -1i64 };
    let numerator = (2 * i64::from(level) + sign) * weight * i64::from(qs);

    Some((pos, saturate_i32(numerator / 32)))
}

fn apply_mismatch_control(f: &mut [i32; 64]) {
    let sum = f.iter().fold(0i64, |acc, &value| acc + i64::from(value));

    if sum % 2 != 0 {
        return;
    }

    if let Some(last) = f.get_mut(63) {
        *last = if *last % 2 != 0 { *last - 1 } else { *last + 1 };
    }
}

fn saturate_i32(value: i64) -> i32 {
    if value <= -2048 {
        -2048
    } else if value >= 2047 {
        2047
    } else {
        value as i32
    }
}

fn clamp_round_i16(value: f64, min: i32, max: i32) -> i16 {
    if !value.is_finite() {
        return 0;
    }

    let rounded = value.round();

    if rounded <= f64::from(min) {
        to_i16(min)
    } else if rounded >= f64::from(max) {
        to_i16(max)
    } else {
        to_i16(rounded as i32)
    }
}

fn clamp_trunc_i16(value: f64, min: i32, max: i32) -> i16 {
    if !value.is_finite() {
        return 0;
    }

    let truncated = value.trunc();

    if truncated <= f64::from(min) {
        to_i16(min)
    } else if truncated >= f64::from(max) {
        to_i16(max)
    } else {
        to_i16(truncated as i32)
    }
}

fn to_i16(value: i32) -> i16 {
    i16::try_from(value).unwrap_or(0)
}

fn idct_horizontal(coef: &[i32; 64]) -> [f64; 64] {
    let cos = cos_table();
    let mut temp = [0.0f64; 64];

    for row in 0usize..8 {
        for sample in 0usize..8 {
            let sum = inverse_row_sum(cos, coef, row, sample);
            if let Some(slot) = temp.get_mut(row * 8 + sample) {
                *slot = sum;
            }
        }
    }

    temp
}

fn idct_vertical(temp: &[f64; 64]) -> [i32; 64] {
    let cos = cos_table();
    let mut out = [0i32; 64];

    for sample_row in 0usize..8 {
        for sample_col in 0usize..8 {
            let sum = inverse_column_sum(cos, temp, sample_row, sample_col);
            if let Some(slot) = out.get_mut(sample_row * 8 + sample_col) {
                *slot = clamp_idct(sum);
            }
        }
    }

    out
}

fn inverse_row_sum(cos: &[[f64; 8]; 8], coef: &[i32; 64], row: usize, sample: usize) -> f64 {
    let mut sum = 0.0f64;

    for freq in 0usize..8 {
        let value = f64::from(coef.get(row * 8 + freq).copied().unwrap_or(0));
        if let Some(term) = inverse_term(cos, freq, sample, value) {
            sum += term;
        }
    }

    sum
}

fn inverse_column_sum(
    cos: &[[f64; 8]; 8],
    temp: &[f64; 64],
    sample_row: usize,
    sample_col: usize,
) -> f64 {
    let mut sum = 0.0f64;

    for freq in 0usize..8 {
        let value = temp.get(freq * 8 + sample_col).copied().unwrap_or(0.0);
        if let Some(term) = inverse_term(cos, freq, sample_row, value) {
            sum += term;
        }
    }

    sum
}

fn inverse_term(cos: &[[f64; 8]; 8], freq: usize, sample: usize, value: f64) -> Option<f64> {
    let c = cos.get(freq).and_then(|row| row.get(sample)).copied()?;
    let scale = if freq == 0 { 0.5 * INV_SQRT2 } else { 0.5 };

    Some(value * scale * c)
}

fn clamp_idct(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }

    let rounded = value.round();

    if rounded <= -256.0 {
        -256
    } else if rounded >= 255.0 {
        255
    } else {
        rounded as i32
    }
}

fn build_cos_table() -> [[f64; 8]; 8] {
    let mut table = [[0.0f64; 8]; 8];

    for freq in 0usize..8 {
        for sample in 0usize..8 {
            let angle = (2 * sample + 1) as f64 * freq as f64 * std::f64::consts::PI / 16.0;
            if let Some(row) = table.get_mut(freq)
                && let Some(cell) = row.get_mut(sample)
            {
                *cell = angle.cos();
            }
        }
    }

    table
}

fn cos_table() -> &'static [[f64; 8]; 8] {
    COS_TABLE.get_or_init(build_cos_table)
}

#[cfg(test)]
mod tests {
    use super::{
        dequantise_intra, dequantise_non_intra, idct8x8, quantise_intra, quantise_non_intra,
        reconstruct_intra, reconstruct_residual,
    };
    use crate::dct::forward_dct8x8;

    #[test]
    fn intra_mismatch_toggles_even_sum() {
        let levels = [0i16; 64];
        let f = dequantise_intra(&levels, 2);

        assert_eq!(f.get(63).copied(), Some(1));

        let sum = f.iter().fold(0i64, |acc, &value| acc + i64::from(value));
        assert_eq!(sum % 2, 1);
    }

    #[test]
    fn non_intra_mismatch_toggles_even_sum() {
        let levels = [0i16; 64];
        let f = dequantise_non_intra(&levels, 2);

        assert_eq!(f.get(63).copied(), Some(1));

        let sum = f.iter().fold(0i64, |acc, &value| acc + i64::from(value));
        assert_eq!(sum % 2, 1);
    }

    #[test]
    fn non_intra_odd_sum_does_not_toggle() {
        let mut levels = [0i16; 64];
        if let Some(slot) = levels.get_mut(1) {
            *slot = 1;
        }

        let f = dequantise_non_intra(&levels, 2);

        assert_eq!(f.get(1).copied(), Some(3));
        assert_eq!(f.get(63).copied(), Some(0));
    }

    #[test]
    fn idct_dc_only_is_flat() {
        let mut coef = [0i32; 64];
        if let Some(slot) = coef.get_mut(0) {
            *slot = 40;
        }

        let out = idct8x8(&coef);
        assert!(out.iter().all(|&value| value == 5));
    }

    #[test]
    fn intra_round_trip_constant() {
        let samples = [128u8; 64];
        let coeffs = forward_dct8x8(&samples);
        let levels = quantise_intra(&coeffs, 2);
        let recon = reconstruct_intra(&levels, 2);

        for (i, (&orig, &rec)) in samples.iter().zip(recon.iter()).enumerate() {
            let diff = i32::from(orig) - i32::from(rec);
            assert!(diff.abs() <= 3, "index {i} diff {diff}");
        }
    }

    #[test]
    fn non_intra_residual_round_trip() {
        let residual = [40i32; 64];
        let mut sample = [0u8; 64];

        for (i, &r) in residual.iter().enumerate() {
            let value = 128 + r;
            if let Some(slot) = sample.get_mut(i) {
                *slot = u8::try_from(value).unwrap_or(128);
            }
        }

        let sample_coeffs = forward_dct8x8(&sample);
        let base_coeffs = forward_dct8x8(&[128u8; 64]);
        let mut coeffs = [0.0f64; 64];

        for (i, (s, b)) in sample_coeffs.iter().zip(base_coeffs.iter()).enumerate() {
            if let Some(slot) = coeffs.get_mut(i) {
                *slot = *s - *b;
            }
        }

        let levels = quantise_non_intra(&coeffs, 2);
        let recon = reconstruct_residual(&levels, 2);

        for (i, (&orig, &rec)) in residual.iter().zip(recon.iter()).enumerate() {
            let diff = orig - rec;
            assert!(diff.abs() <= 4, "index {i} diff {diff}");
        }
    }
}
