use crate::{EncodeError, Frame420};

/// Convert RGB24 row-major data to a studio-range BT.601 4:2:0 frame.
pub(crate) fn rgb_to_frame420(
    width: u16,
    height: u16,
    rgb: &[u8],
) -> Result<Frame420, EncodeError> {
    let w = usize::from(width);
    let h = usize::from(height);
    let expected = w
        .checked_mul(h)
        .and_then(|n| n.checked_mul(3))
        .ok_or_else(|| error("invalid frame size"))?;

    if rgb.len() != expected {
        return Err(error("rgb length mismatch"));
    }

    let mut y = vec![0u8; expected / 3];
    fill_luma(rgb, &mut y);

    let cw = w.div_ceil(2);
    let ch = h.div_ceil(2);
    let c_len = cw
        .checked_mul(ch)
        .ok_or_else(|| error("invalid chroma size"))?;

    let mut cb = vec![0u8; c_len];
    let mut cr = vec![0u8; c_len];
    fill_chroma(rgb, w, h, &mut cb, &mut cr);

    Ok(Frame420 {
        width,
        height,
        y,
        cb,
        cr,
    })
}

fn error(message: &str) -> EncodeError {
    EncodeError(message.to_string())
}

fn fill_luma(rgb: &[u8], y: &mut [u8]) {
    for (index, pixel) in rgb.as_chunks::<3>().0.iter().enumerate() {
        if let [r, g, b] = pixel
            && let Some(slot) = y.get_mut(index)
        {
            *slot = luma_from_rgb(*r, *g, *b);
        }
    }
}

fn fill_chroma(rgb: &[u8], width: usize, height: usize, cb: &mut [u8], cr: &mut [u8]) {
    let cw = width.div_ceil(2);
    let ch = height.div_ceil(2);

    for cy in 0..ch {
        for cx in 0..cw {
            if let Some((cb_value, cr_value)) = chroma_block(rgb, width, height, cx, cy) {
                let index = cy * cw + cx;
                if let Some(slot) = cb.get_mut(index) {
                    *slot = cb_value;
                }
                if let Some(slot) = cr.get_mut(index) {
                    *slot = cr_value;
                }
            }
        }
    }
}

fn chroma_block(rgb: &[u8], width: usize, height: usize, cx: usize, cy: usize) -> Option<(u8, u8)> {
    let mut sum_cb = 0i32;
    let mut sum_cr = 0i32;
    let mut count = 0i32;

    for dy in 0..2 {
        let py = cy.checked_mul(2)?.checked_add(dy)?;
        if py >= height {
            continue;
        }

        for dx in 0..2 {
            let px = cx.checked_mul(2)?.checked_add(dx)?;
            if px >= width {
                continue;
            }

            let start = py.checked_mul(width)?.checked_add(px)?.checked_mul(3)?;
            let end = start.checked_add(3)?;

            if let Some(pixel) = rgb.get(start..end)
                && let [r, g, b] = pixel
            {
                let (cb, cr) = chroma_from_rgb(*r, *g, *b);
                sum_cb += cb;
                sum_cr += cr;
                count += 1;
            }
        }
    }

    if count == 0 {
        return None;
    }

    let cb = (sum_cb + count / 2) / count;
    let cr = (sum_cr + count / 2) / count;
    Some((clamp_u8(cb, 16, 240), clamp_u8(cr, 16, 240)))
}

fn luma_from_rgb(r: u8, g: u8, b: u8) -> u8 {
    let value = ((66 * i32::from(r) + 129 * i32::from(g) + 25 * i32::from(b) + 128) >> 8) + 16;
    clamp_u8(value, 16, 235)
}

fn chroma_from_rgb(r: u8, g: u8, b: u8) -> (i32, i32) {
    let cb = ((-38 * i32::from(r) - 74 * i32::from(g) + 112 * i32::from(b) + 128) >> 8) + 128;
    let cr = ((112 * i32::from(r) - 94 * i32::from(g) - 18 * i32::from(b) + 128) >> 8) + 128;
    (cb, cr)
}

fn clamp_u8(value: i32, min: u8, max: u8) -> u8 {
    let min = i32::from(min);
    let max = i32::from(max);
    if value < min {
        min as u8
    } else if value > max {
        max as u8
    } else {
        value as u8
    }
}

#[cfg(test)]
mod tests {
    use super::rgb_to_frame420;

    #[test]
    fn white_is_studio_range() -> Result<(), String> {
        let rgb = vec![255u8; 12];
        let frame = rgb_to_frame420(2, 2, &rgb).map_err(|e| e.to_string())?;
        let y = frame.y.first().copied().ok_or("empty luma")?;
        let cb = frame.cb.first().copied().ok_or("empty cb")?;
        let cr = frame.cr.first().copied().ok_or("empty cr")?;
        assert_eq!(y, 235);
        assert_eq!(cb, 128);
        assert_eq!(cr, 128);
        Ok(())
    }

    #[test]
    fn black_is_studio_range() -> Result<(), String> {
        let rgb = vec![0u8; 12];
        let frame = rgb_to_frame420(2, 2, &rgb).map_err(|e| e.to_string())?;
        let y = frame.y.first().copied().ok_or("empty luma")?;
        let cb = frame.cb.first().copied().ok_or("empty cb")?;
        let cr = frame.cr.first().copied().ok_or("empty cr")?;
        assert_eq!(y, 16);
        assert_eq!(cb, 128);
        assert_eq!(cr, 128);
        Ok(())
    }

    #[test]
    fn red_is_studio_range() -> Result<(), String> {
        let mut rgb = Vec::with_capacity(12);
        rgb.extend_from_slice(&[255, 0, 0]);
        rgb.extend_from_slice(&[255, 0, 0]);
        rgb.extend_from_slice(&[255, 0, 0]);
        rgb.extend_from_slice(&[255, 0, 0]);

        let frame = rgb_to_frame420(2, 2, &rgb).map_err(|e| e.to_string())?;
        let y = frame.y.first().copied().ok_or("empty luma")?;
        let cb = frame.cb.first().copied().ok_or("empty cb")?;
        let cr = frame.cr.first().copied().ok_or("empty cr")?;

        assert!((i32::from(y) - 81).abs() <= 2);
        assert!((i32::from(cb) - 90).abs() <= 2);
        assert!((i32::from(cr) - 240).abs() <= 2);
        Ok(())
    }
}
