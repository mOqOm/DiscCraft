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

    let cw = w.div_ceil(2);
    let ch = h.div_ceil(2);
    let c_len = cw
        .checked_mul(ch)
        .ok_or_else(|| error("invalid chroma size"))?;
    let mut y = vec![0u8; expected / 3];
    let mut cb = vec![0u8; c_len];
    let mut cr = vec![0u8; c_len];
    if w == 0 || h == 0 {
        return Ok(Frame420 {
            width,
            height,
            y,
            cb,
            cr,
        });
    }

    // Bands of whole chroma rows (two picture rows each), each on a thread of its own: at least
    // [`BAND_ROWS`] picture rows a band, as more threads cost more to start than they save (M6.365).
    let bands = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(ch.div_ceil(BAND_ROWS / 2))
        .max(1);
    let band = ch.div_ceil(bands);
    let jobs = y
        .chunks_mut(band * 2 * w)
        .zip(cb.chunks_mut(band * cw))
        .zip(cr.chunks_mut(band * cw))
        .enumerate();
    if bands == 1 {
        for (index, ((y_band, cb_band), cr_band)) in jobs {
            convert_band(rgb, (w, h), index * band, y_band, (cb_band, cr_band));
        }
    } else {
        std::thread::scope(|scope| {
            for (index, ((y_band, cb_band), cr_band)) in jobs {
                let _ = scope.spawn(move || {
                    convert_band(rgb, (w, h), index * band, y_band, (cb_band, cr_band));
                });
            }
        });
    }

    Ok(Frame420 {
        width,
        height,
        y,
        cb,
        cr,
    })
}

/// The fewest picture rows a thread of [`rgb_to_frame420`] converts.
const BAND_ROWS: usize = 48;

/// Converts the picture rows of the chroma rows from `first_cy` on: their luma into `y_band` (whole rows of
/// `width`), their chroma into `cb_band` / `cr_band` (whole rows of half the width). A chroma sample is the mean
/// of the chroma of the (up to four) pixels it covers, each worked out on its own, rounded half up.
fn convert_band(
    rgb: &[u8],
    (width, height): (usize, usize),
    first_cy: usize,
    y_band: &mut [u8],
    (cb_band, cr_band): (&mut [u8], &mut [u8]),
) {
    let cw = width.div_ceil(2);
    let row_bytes = width * 3;
    for (offset, (cb_row, cr_row)) in cb_band
        .chunks_mut(cw)
        .zip(cr_band.chunks_mut(cw))
        .enumerate()
    {
        let cy = first_cy + offset;
        let top = cy * 2;
        let source = |py: usize| {
            (py < height)
                .then(|| rgb.get(py * row_bytes..(py + 1) * row_bytes))
                .flatten()
        };
        let rows = [source(top), source(top + 1)];
        for (dy, row) in rows.iter().enumerate() {
            let Some(row) = row else {
                continue;
            };
            let at = (offset * 2 + dy) * width;
            if let Some(out) = y_band.get_mut(at..at + width) {
                for (slot, pixel) in out.iter_mut().zip(row.as_chunks::<3>().0) {
                    let [r, g, b] = *pixel;
                    *slot = luma_from_rgb(r, g, b);
                }
            }
        }
        for (cx, (cb_slot, cr_slot)) in cb_row.iter_mut().zip(cr_row.iter_mut()).enumerate() {
            let (mut sum_cb, mut sum_cr, mut count) = (0i32, 0i32, 0i32);
            for row in rows.iter().flatten() {
                for px in [cx * 2, cx * 2 + 1] {
                    if px >= width {
                        continue;
                    }
                    if let Some([r, g, b]) = row
                        .get(px * 3..px * 3 + 3)
                        .and_then(|p| <[u8; 3]>::try_from(p).ok())
                    {
                        let (cb, cr) = chroma_from_rgb(r, g, b);
                        sum_cb += cb;
                        sum_cr += cr;
                        count += 1;
                    }
                }
            }
            if count > 0 {
                *cb_slot = clamp_u8((sum_cb + count / 2) / count, 16, 240);
                *cr_slot = clamp_u8((sum_cr + count / 2) / count, 16, 240);
            }
        }
    }
}

fn error(message: &str) -> EncodeError {
    EncodeError(message.to_string())
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

    /// The conversion as it was before M6.365 (a pixel at a time): the new one gives the same bytes.
    fn reference(width: usize, height: usize, rgb: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let y: Vec<u8> = rgb
            .as_chunks::<3>()
            .0
            .iter()
            .map(|[r, g, b]| super::luma_from_rgb(*r, *g, *b))
            .collect();
        let (cw, ch) = (width.div_ceil(2), height.div_ceil(2));
        let (mut cb, mut cr) = (vec![0u8; cw * ch], vec![0u8; cw * ch]);
        for cy in 0..ch {
            for cx in 0..cw {
                let (mut sum_cb, mut sum_cr, mut count) = (0i32, 0i32, 0i32);
                for py in [cy * 2, cy * 2 + 1] {
                    for px in [cx * 2, cx * 2 + 1] {
                        if px < width
                            && py < height
                            && let Some([r, g, b]) = rgb
                                .get((py * width + px) * 3..(py * width + px) * 3 + 3)
                                .and_then(|p| <[u8; 3]>::try_from(p).ok())
                        {
                            let (c_b, c_r) = super::chroma_from_rgb(r, g, b);
                            sum_cb += c_b;
                            sum_cr += c_r;
                            count += 1;
                        }
                    }
                }
                if let Some(slot) = cb.get_mut(cy * cw + cx) {
                    *slot = super::clamp_u8((sum_cb + count / 2) / count, 16, 240);
                }
                if let Some(slot) = cr.get_mut(cy * cw + cx) {
                    *slot = super::clamp_u8((sum_cr + count / 2) / count, 16, 240);
                }
            }
        }
        (y, cb, cr)
    }

    #[test]
    fn bands_give_the_pixel_at_a_time_bytes() -> Result<(), String> {
        for (width, height) in [
            (720, 480),
            (720, 576),
            (1920, 1080),
            (7, 5),
            (1, 1),
            (2, 97),
            (353, 241),
        ] {
            let mut seed = 0x1234_5678_u32 ^ (width as u32 * 31 + height as u32);
            let rgb: Vec<u8> = (0..width * height * 3)
                .map(|_| {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (seed >> 24) as u8
                })
                .collect();
            let frame =
                rgb_to_frame420(width as u16, height as u16, &rgb).map_err(|e| e.to_string())?;
            let (y, cb, cr) = reference(width, height, &rgb);
            assert!(
                frame.y == y && frame.cb == cb && frame.cr == cr,
                "{width}x{height}"
            );
        }
        Ok(())
    }

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
