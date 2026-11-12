use std::sync::OnceLock;

const INV_SQRT2: f64 = 1.0 / std::f64::consts::SQRT_2;

static COS_TABLE: OnceLock<[[f64; 8]; 8]> = OnceLock::new();

fn build_cos_table() -> [[f64; 8]; 8] {
    let mut table = [[0.0f64; 8]; 8];
    for freq in 0..8 {
        for sample in 0..8 {
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

/// Forward orthonormal 8x8 type-II DCT.
pub(crate) fn forward_dct8x8(samples: &[u8; 64]) -> [f64; 64] {
    let cos = cos_table();
    let mut temp = [0.0f64; 64];

    for (row, row_samples) in samples.as_chunks::<8>().0.iter().enumerate() {
        for freq_h in 0..8 {
            let sum = row_samples
                .iter()
                .enumerate()
                .fold(0.0f64, |acc, (x, &sample)| {
                    let c = cos
                        .get(freq_h)
                        .and_then(|r| r.get(x))
                        .copied()
                        .unwrap_or(0.0);
                    acc + f64::from(sample) * c
                });
            // Orthonormal 8-point DCT-II: X_k = 1/2 · c_k · Σ x_n cos(...), c_0 = 1/√2.
            let scale = if freq_h == 0 { 0.5 * INV_SQRT2 } else { 0.5 };
            if let Some(slot) = temp.get_mut(row * 8 + freq_h) {
                *slot = sum * scale;
            }
        }
    }

    let mut out = [0.0f64; 64];
    for col in 0..8 {
        for freq_v in 0..8 {
            let sum = (0..8).fold(0.0f64, |acc, y| {
                let t = temp.get(y * 8 + col).copied().unwrap_or(0.0);
                let c = cos
                    .get(freq_v)
                    .and_then(|r| r.get(y))
                    .copied()
                    .unwrap_or(0.0);
                acc + t * c
            });
            let scale = if freq_v == 0 { 0.5 * INV_SQRT2 } else { 0.5 };
            if let Some(slot) = out.get_mut(freq_v * 8 + col) {
                *slot = sum * scale;
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::forward_dct8x8;

    #[test]
    fn flat_block_has_only_dc() {
        let samples = [123u8; 64];
        let coeffs = forward_dct8x8(&samples);
        let dc = coeffs.first().copied().unwrap_or(0.0);
        assert!((dc - 8.0 * 123.0).abs() < 1e-6);
        for &coeff in coeffs.iter().skip(1) {
            assert!(coeff.abs() < 1e-6);
        }
    }
}
