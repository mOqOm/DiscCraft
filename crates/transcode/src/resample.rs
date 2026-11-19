//! Encore's "Use Maximum Render Quality" (M6.327): a picture scaled down is averaged over the source pixels each
//! output pixel covers (an area filter), so fine detail turns into its average instead of aliasing as the usual
//! two-by-two bilinear sampling lets it; a picture scaled up is interpolated linearly between pixel centres. The
//! rows of both passes are shared out between the CPU's cores.

/// The weights of an output pixel's source pixels add up to this.
const ONE: u32 = 1 << 14;

/// The extra bits the horizontal pass keeps for the vertical one.
const EXTRA: u32 = 8;

/// The fewest rows worth a thread of their own.
const ROWS_PER_WORKER: usize = 48;

/// One output pixel's source pixels along an axis: the first one and their weights (adding up to [`ONE`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Taps {
    pub(crate) first: usize,
    pub(crate) weights: Vec<u32>,
}

/// The taps of each of `out` output pixels along an axis of `source` source pixels: an area filter when the axis
/// is scaled down (each output pixel the average of the source pixels it covers, the ones at its ends in part),
/// linear interpolation between pixel centres when it is scaled up or kept.
pub(crate) fn axis_taps(source: usize, out: usize) -> Vec<Taps> {
    if source == 0 || out == 0 {
        return Vec::new();
    }
    let last = source - 1;
    (0..out)
        .map(|index| {
            if source > out {
                // Output pixel `index` covers [index * source, (index + 1) * source) and source pixel j covers
                // [j * out, (j + 1) * out), both in units of 1 / (source * out) of the axis.
                let start = index * source;
                let end = start + source;
                let first = start / out;
                let past = end.div_ceil(out).min(source);
                let mut weights: Vec<u32> = (first..past)
                    .map(|j| {
                        let overlap = (end.min((j + 1) * out)).saturating_sub(start.max(j * out));
                        u32::try_from(overlap as u64 * u64::from(ONE) / source as u64).unwrap_or(0)
                    })
                    .collect();
                settle(&mut weights);
                Taps { first, weights }
            } else {
                // The output pixel's centre in source pixels: ((2 index + 1) source - out) / (2 out).
                let numerator = (2 * index + 1) * source;
                let denominator = 2 * out;
                if numerator < out {
                    return Taps {
                        first: 0,
                        weights: vec![ONE],
                    };
                }
                let at = numerator - out;
                let first = at / denominator;
                if first >= last {
                    return Taps {
                        first: last,
                        weights: vec![ONE],
                    };
                }
                let fraction =
                    u32::try_from((at % denominator) as u64 * u64::from(ONE) / denominator as u64)
                        .unwrap_or(0);
                Taps {
                    first,
                    weights: vec![ONE - fraction, fraction],
                }
            }
        })
        .collect()
}

/// Makes `weights` add up to [`ONE`] exactly (what rounding down left goes to the largest).
fn settle(weights: &mut [u32]) {
    let total: u32 = weights.iter().sum();
    let largest = weights
        .iter()
        .enumerate()
        .max_by_key(|(_, weight)| **weight)
        .map(|(index, _)| index);
    if let Some(weight) = largest.and_then(|index| weights.get_mut(index)) {
        *weight = weight.saturating_add(ONE.saturating_sub(total));
    }
}

/// The source picture (RGBA, composited onto black) scaled with [`axis_taps`] into the rectangle `(x, y, width,
/// height)` of an RGB24 output `output` large, black around it. `None` when the sizes do not fit.
pub(crate) fn render_rgb_filtered(
    rgba: &[u8],
    (source_width, source_height): (u32, u32),
    (x, y, width, height): (u32, u32, u32, u32),
    (output_width, output_height): (u32, u32),
) -> Option<Vec<u8>> {
    let ow = usize::try_from(output_width).ok()?;
    let oh = usize::try_from(output_height).ok()?;
    let mut rgb = vec![0u8; ow.checked_mul(oh)?.checked_mul(3)?];
    if width == 0 || height == 0 {
        return Some(rgb);
    }
    if x.checked_add(width)? > output_width || y.checked_add(height)? > output_height {
        return None;
    }
    let sw = usize::try_from(source_width).ok()?;
    let sh = usize::try_from(source_height).ok()?;
    if sw == 0 || sh == 0 {
        return None;
    }
    let (pixels, _) = rgba.as_chunks::<4>();
    let pixels = pixels.get(..sw.checked_mul(sh)?)?;
    let (cw, ch) = (usize::try_from(width).ok()?, usize::try_from(height).ok()?);
    let columns = axis_taps(sw, cw);
    let rows = axis_taps(sh, ch);

    // Across: every source row to the picture's width, with EXTRA more bits.
    let mut across = vec![0u16; sh.checked_mul(cw)?.checked_mul(3)?];
    let across_row = |out: &mut [u16], source_row: usize| {
        let Some(row) = pixels.get(source_row * sw..(source_row + 1) * sw) else {
            return;
        };
        let (out, _) = out.as_chunks_mut::<3>();
        for (slot, taps) in out.iter_mut().zip(&columns) {
            let mut sum = [0u32; 3];
            for (offset, weight) in taps.weights.iter().enumerate() {
                if let Some(pixel) = row.get(taps.first + offset) {
                    let alpha = u32::from(pixel[3]);
                    for (total, channel) in sum.iter_mut().zip(pixel.iter().take(3)) {
                        // Composited onto black.
                        let value = (u32::from(*channel) * alpha + 127) / 255;
                        *total += value * weight;
                    }
                }
            }
            for (slot, total) in slot.iter_mut().zip(sum) {
                *slot = u16::try_from((total + (1 << (14 - EXTRA - 1))) >> (14 - EXTRA))
                    .unwrap_or(u16::MAX);
            }
        }
    };
    in_rows(&mut across, cw * 3, |index, row| across_row(row, index));

    // Down: the picture's rows from those, into the output.
    let stride = ow * 3;
    let left = usize::try_from(x).ok()? * 3;
    let top = usize::try_from(y).ok()?;
    let content = rgb.get_mut(top * stride..(top + ch) * stride)?;
    let down_row = |out: &mut [u8], taps: &Taps| {
        let Some(out) = out.get_mut(left..left + cw * 3) else {
            return;
        };
        for (column, slot) in out.iter_mut().enumerate() {
            let mut total = 0u32;
            for (offset, weight) in taps.weights.iter().enumerate() {
                if let Some(value) = across.get((taps.first + offset) * cw * 3 + column) {
                    total += u32::from(*value) * weight;
                }
            }
            let shift = 14 + EXTRA;
            *slot = u8::try_from((total + (1 << (shift - 1))) >> shift).unwrap_or(u8::MAX);
        }
    };
    in_rows(content, stride, |index, row| {
        if let Some(taps) = rows.get(index) {
            down_row(row, taps);
        }
    });
    Some(rgb)
}

/// Runs `fill` on each `stride`-long row of `buffer` (with its index), the rows shared out between threads.
fn in_rows<T: Send>(buffer: &mut [T], stride: usize, fill: impl Fn(usize, &mut [T]) + Sync) {
    if stride == 0 {
        return;
    }
    let count = buffer.len() / stride;
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .clamp(1, (count / ROWS_PER_WORKER).max(1));
    if workers == 1 {
        for (index, row) in buffer.chunks_mut(stride).enumerate() {
            fill(index, row);
        }
        return;
    }
    let per_worker = count.div_ceil(workers).max(1);
    std::thread::scope(|scope| {
        for (block, rows) in buffer.chunks_mut(per_worker * stride).enumerate() {
            let fill = &fill;
            let _ = scope.spawn(move || {
                for (offset, row) in rows.chunks_mut(stride).enumerate() {
                    fill(block * per_worker + offset, row);
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing)]
    use super::{ONE, Taps, axis_taps, render_rgb_filtered};

    #[test]
    fn taps_cover_the_source_and_add_up() {
        // 1920 to 720: each output pixel covers 2 2/3 source pixels.
        let taps = axis_taps(1920, 720);
        assert_eq!(taps.len(), 720);
        assert!(
            taps.iter()
                .all(|tap| tap.weights.iter().sum::<u32>() == ONE)
        );
        assert_eq!(taps[0].first, 0);
        assert_eq!(taps[0].weights.len(), 3);
        assert_eq!(taps[719].first + taps[719].weights.len(), 1920);
        // Halving: two source pixels each, half and half.
        assert_eq!(
            axis_taps(4, 2),
            [
                Taps {
                    first: 0,
                    weights: vec![ONE / 2, ONE / 2]
                },
                Taps {
                    first: 2,
                    weights: vec![ONE / 2, ONE / 2]
                }
            ]
        );
        // Doubling: between pixel centres, the ends held.
        let up = axis_taps(2, 4);
        assert_eq!(up[0].weights, [ONE]);
        // Output pixel 1's centre is a quarter of the way from source pixel 0 to 1.
        assert_eq!(
            (up[1].first, up[1].weights.clone()),
            (0, vec![3 * ONE / 4, ONE / 4])
        );
        assert_eq!(up[3].weights, [ONE]);
        assert!(axis_taps(0, 4).is_empty());
    }

    /// A one-pixel checkerboard scaled down averages to grey (the bilinear sampler picks pixels of it and shows
    /// a pattern); a flat colour stays itself.
    #[test]
    fn fine_detail_averages_out() {
        let (sw, sh) = (1920u32, 1080u32);
        let mut rgba = Vec::with_capacity((sw * sh * 4) as usize);
        for y in 0..sh {
            for x in 0..sw {
                let value = if (x + y) % 2 == 0 { 255 } else { 0 };
                rgba.extend_from_slice(&[value, value, value, 255]);
            }
        }
        let rgb = render_rgb_filtered(&rgba, (sw, sh), (0, 0, 720, 480), (720, 480)).unwrap();
        let (low, high) = rgb.iter().fold((255u8, 0u8), |(low, high), value| {
            (low.min(*value), high.max(*value))
        });
        assert!(low >= 100 && high <= 155, "{low}..{high}");
        let bilinear =
            crate::hd::render_rgb_at(&rgba, (sw, sh), (0, 0, 720, 480), (720, 480)).unwrap();
        let (low, high) = bilinear.iter().fold((255u8, 0u8), |(low, high), value| {
            (low.min(*value), high.max(*value))
        });
        assert!(high - low > 150, "{low}..{high}");

        let flat = [200u8, 100, 50, 255].repeat(64 * 48);
        let rgb = render_rgb_filtered(&flat, (64, 48), (4, 2, 20, 10), (30, 14)).unwrap();
        assert_eq!(&rgb[(2 * 30 + 4) * 3..(2 * 30 + 5) * 3], [200, 100, 50]);
        assert_eq!(&rgb[..3], [0, 0, 0]);
        // Upscaled, too.
        let rgb = render_rgb_filtered(&flat, (64, 48), (0, 0, 640, 480), (640, 480)).unwrap();
        assert!(rgb.chunks(3).all(|pixel| pixel == [200, 100, 50]));
        // Out of the output: refused.
        assert_eq!(
            render_rgb_filtered(&flat, (64, 48), (0, 0, 31, 14), (30, 14)),
            None
        );
    }
}
