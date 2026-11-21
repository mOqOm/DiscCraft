use crate::Frame420;

fn clamp_i32(value: i32, len: usize) -> i32 {
    if len == 0 {
        return 0;
    }

    let max = i32::try_from(len - 1).unwrap_or(0);

    if value < 0 {
        0
    } else if value > max {
        max
    } else {
        value
    }
}

fn sample_plane(plane: &[u8], width: usize, height: usize, x: i32, y: i32) -> u8 {
    if width == 0 || height == 0 {
        return 0;
    }

    let x = clamp_i32(x, width);
    let y = clamp_i32(y, height);
    let xi = usize::try_from(x).unwrap_or(0);
    let yi = usize::try_from(y).unwrap_or(0);
    let index = yi.saturating_mul(width).saturating_add(xi);

    plane.get(index).copied().unwrap_or(0)
}

fn sample_y(frame: &Frame420, x: i32, y: i32) -> u8 {
    sample_plane(
        &frame.y,
        usize::from(frame.width),
        usize::from(frame.height),
        x,
        y,
    )
}

fn closer_to_zero(dx: i32, dy: i32, best_dx: i32, best_dy: i32) -> bool {
    let distance = dx.abs().saturating_add(dy.abs());
    let best_distance = best_dx.abs().saturating_add(best_dy.abs());

    if distance != best_distance {
        return distance < best_distance;
    }

    if dx.abs() != best_dx.abs() {
        return dx.abs() < best_dx.abs();
    }

    if dy.abs() != best_dy.abs() {
        return dy.abs() < best_dy.abs();
    }

    if dx != best_dx {
        return dx == 0 || dx > best_dx;
    }

    if dy != best_dy {
        return dy == 0 || dy > best_dy;
    }

    false
}

#[cfg(test)] // the 16-row block of the reference full search
fn sad_block(
    cur: &Frame420,
    reference: &Frame420,
    mbx: usize,
    mby: usize,
    dx: i32,
    dy: i32,
) -> u32 {
    sad_rows(cur, reference, (mbx, mby, 16), dx, dy)
}

/// [`sad_block`] of a block `rows` high (16, or 8 for a field of a macroblock).
fn sad_rows(
    cur: &Frame420,
    reference: &Frame420,
    (mbx, mby, rows): (usize, usize, usize),
    dx: i32,
    dy: i32,
) -> u32 {
    let mut sum = 0u32;

    for row in 0..rows {
        for col in 0..16 {
            let cur_x = i32::try_from(mbx + col).unwrap_or(i32::MAX);
            let cur_y = i32::try_from(mby + row).unwrap_or(i32::MAX);
            let ref_x = cur_x.saturating_add(dx);
            let ref_y = cur_y.saturating_add(dy);

            let a = sample_y(cur, cur_x, cur_y);
            let b = sample_y(reference, ref_x, ref_y);

            sum += if a > b {
                u32::from(a - b)
            } else {
                u32::from(b - a)
            };
        }
    }

    sum
}

/// Search window for one macroblock, clipped to the reference frame.
struct Window {
    dx_min: i32,
    dx_max: i32,
    dy_min: i32,
    dy_max: i32,
}

impl Window {
    /// The window of a block `rows` high.
    fn rows(
        reference: &Frame420,
        (mbx, mby, rows): (usize, usize, usize),
        range: i32,
    ) -> Option<Self> {
        let (w, h) = (usize::from(reference.width), usize::from(reference.height));
        if w < 16 || h < rows {
            return None;
        }
        let (x, y) = (i32::try_from(mbx).ok()?, i32::try_from(mby).ok()?);
        let win = Window {
            dx_min: (-x).max(-range),
            dy_min: (-y).max(-range),
            dx_max: i32::try_from(w - 16).ok()?.saturating_sub(x).min(range),
            dy_max: i32::try_from(h - rows).ok()?.saturating_sub(y).min(range),
        };
        (win.dx_min <= win.dx_max && win.dy_min <= win.dy_max).then_some(win)
    }

    fn contains(&self, dx: i32, dy: i32) -> bool {
        (self.dx_min..=self.dx_max).contains(&dx) && (self.dy_min..=self.dy_max).contains(&dy)
    }
}

/// Best of the current candidate and `(dx, dy)`, preferring the vector closer to zero on ties.
fn better(best: (i32, i32, u32), dx: i32, dy: i32, sad: u32) -> bool {
    sad < best.2 || (sad == best.2 && closer_to_zero(dx, dy, best.0, best.1))
}

/// Predictive diamond search (full-pel): start from the best of (0, 0) and `preds` (the vector before, the last
/// picture's here), walk the large diamond until its centre is best, then refine with the small diamond. About 20–40
/// SAD evaluations instead of (2·range + 1)².
pub(crate) fn search_fast(
    cur: &Frame420,
    reference: &Frame420,
    (mbx, mby): (usize, usize),
    range: i32,
    preds: &[(i32, i32)],
) -> (i32, i32, u32) {
    search_fast_rows(cur, reference, (mbx, mby, 16), range, preds)
}

/// [`search_fast`] of a block `rows` high (8: one field of a macroblock, in field pictures made by [`field`]).
pub(crate) fn search_fast_rows(
    cur: &Frame420,
    reference: &Frame420,
    (mbx, mby, rows): (usize, usize, usize),
    range: i32,
    preds: &[(i32, i32)],
) -> (i32, i32, u32) {
    const LARGE: [(i32, i32); 8] = [
        (0, -2),
        (0, 2),
        (-2, 0),
        (2, 0),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ];
    const SMALL: [(i32, i32); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];
    let Some(win) = Window::rows(reference, (mbx, mby, rows), range.max(0)) else {
        return (0, 0, 0);
    };
    let mut best = (0, 0, sad_rows(cur, reference, (mbx, mby, rows), 0, 0));
    for (index, &pred) in preds.iter().enumerate() {
        if win.contains(pred.0, pred.1)
            && pred != (0, 0)
            && !preds
                .get(..index)
                .is_some_and(|before| before.contains(&pred))
        {
            let sad = sad_rows(cur, reference, (mbx, mby, rows), pred.0, pred.1);
            if better(best, pred.0, pred.1, sad) {
                best = (pred.0, pred.1, sad);
            }
        }
    }
    for pattern in [&LARGE[..], &SMALL[..]] {
        // The large diamond repeats while it moves; the small one runs once.
        let rounds = if pattern.len() == LARGE.len() {
            2 * range.max(1)
        } else {
            1
        };
        for _ in 0..rounds {
            let centre = best;
            for &(ox, oy) in pattern {
                let (dx, dy) = (centre.0 + ox, centre.1 + oy);
                if win.contains(dx, dy) {
                    let sad = sad_rows(cur, reference, (mbx, mby, rows), dx, dy);
                    if better(best, dx, dy, sad) {
                        best = (dx, dy, sad);
                    }
                }
            }
            if best == centre {
                break;
            }
        }
    }
    best
}

/// Full-pel full search of a 16x16 luma block in +/- `range` pixels.
///
/// Returns `(dx, dy, SAD)`, with ties broken towards `(0, 0)`.
#[cfg(test)] // reference implementation for the tests of search_fast
pub(crate) fn search(
    cur: &Frame420,
    reference: &Frame420,
    mbx: usize,
    mby: usize,
    range: i32,
) -> (i32, i32, u32) {
    let range = range.max(0);
    let ref_w = usize::from(reference.width);
    let ref_h = usize::from(reference.height);

    if ref_w < 16 || ref_h < 16 {
        return (0, 0, 0);
    }

    let mbx_i = i32::try_from(mbx).unwrap_or(i32::MAX);
    let mby_i = i32::try_from(mby).unwrap_or(i32::MAX);

    let dx_min = (-mbx_i).max(-range);
    let dy_min = (-mby_i).max(-range);

    let dx_max = i32::try_from(ref_w - 16)
        .unwrap_or(0)
        .saturating_sub(mbx_i)
        .min(range);
    let dy_max = i32::try_from(ref_h - 16)
        .unwrap_or(0)
        .saturating_sub(mby_i)
        .min(range);

    if dx_min > dx_max || dy_min > dy_max {
        return (0, 0, 0);
    }

    let mut best_dx = 0i32;
    let mut best_dy = 0i32;
    let mut best_sad = u32::MAX;

    for dy in dy_min..=dy_max {
        for dx in dx_min..=dx_max {
            let sad = sad_block(cur, reference, mbx, mby, dx, dy);

            if sad < best_sad || (sad == best_sad && closer_to_zero(dx, dy, best_dx, best_dy)) {
                best_dx = dx;
                best_dy = dy;
                best_sad = sad;
            }
        }
    }

    if best_sad == u32::MAX {
        (0, 0, 0)
    } else {
        (best_dx, best_dy, best_sad)
    }
}

fn predict_chroma(
    plane: &[u8],
    (width, height, rows): (usize, usize, usize),
    x: i32,
    y: i32,
    fx: i32,
    fy: i32,
) -> [u8; 64] {
    let mut out = [0u8; 64];

    for row in 0..rows.min(8) {
        for col in 0..8 {
            let sx = x.saturating_add(i32::try_from(col).unwrap_or(0));
            let sy = y.saturating_add(i32::try_from(row).unwrap_or(0));

            let value = if fx == 0 && fy == 0 {
                sample_plane(plane, width, height, sx, sy)
            } else if fx != 0 && fy == 0 {
                let ax = if fx < 0 { sx - 1 } else { sx };
                let bx = if fx > 0 { sx + 1 } else { sx };
                let a = u32::from(sample_plane(plane, width, height, ax, sy));
                let b = u32::from(sample_plane(plane, width, height, bx, sy));
                u8::try_from((a + b + 1) >> 1).unwrap_or(255)
            } else if fx == 0 && fy != 0 {
                let ay = if fy < 0 { sy - 1 } else { sy };
                let by = if fy > 0 { sy + 1 } else { sy };
                let a = u32::from(sample_plane(plane, width, height, sx, ay));
                let b = u32::from(sample_plane(plane, width, height, sx, by));
                u8::try_from((a + b + 1) >> 1).unwrap_or(255)
            } else {
                let ax = if fx < 0 { sx - 1 } else { sx };
                let bx = if fx > 0 { sx + 1 } else { sx };
                let ay = if fy < 0 { sy - 1 } else { sy };
                let by = if fy > 0 { sy + 1 } else { sy };

                let a = u32::from(sample_plane(plane, width, height, ax, ay));
                let b = u32::from(sample_plane(plane, width, height, bx, ay));
                let c = u32::from(sample_plane(plane, width, height, ax, by));
                let d = u32::from(sample_plane(plane, width, height, bx, by));

                u8::try_from((a + b + c + d + 2) >> 2).unwrap_or(255)
            };

            if let Some(slot) = out.get_mut(row * 8 + col) {
                *slot = value;
            }
        }
    }

    out
}

/// The 16 x 16 luma prediction of the macroblock at (`mbx`, `mby`) for a vector in half pixels (13818-2 7.6.4):
/// the whole part of the vector moves the block, and an odd component averages each sample with its right or
/// lower neighbour (all four when both are odd), rounding up.
pub(crate) fn predict_luma(
    reference: &Frame420,
    mbx: usize,
    mby: usize,
    mv_x_half: i32,
    mv_y_half: i32,
) -> [u8; 256] {
    predict_luma_rows(reference, (mbx, mby, 16), mv_x_half, mv_y_half)
}

/// [`predict_luma`] of a block `rows` high (the first `rows` × 16 samples of the result).
pub(crate) fn predict_luma_rows(
    reference: &Frame420,
    (mbx, mby, rows): (usize, usize, usize),
    mv_x_half: i32,
    mv_y_half: i32,
) -> [u8; 256] {
    let rows = rows.min(16);
    let mut luma = [0u8; 256];
    let (width, height) = (usize::from(reference.width), usize::from(reference.height));
    // Floor division: -3 half pixels is 2 whole pixels back and a half forward.
    let ref_x = i32::try_from(mbx)
        .unwrap_or(0)
        .saturating_add(mv_x_half >> 1);
    let ref_y = i32::try_from(mby)
        .unwrap_or(0)
        .saturating_add(mv_y_half >> 1);
    let (half_x, half_y) = (mv_x_half & 1 != 0, mv_y_half & 1 != 0);

    // The usual case, all samples inside the picture: rows are read as slices.
    let inside = usize::try_from(ref_x)
        .ok()
        .zip(usize::try_from(ref_y).ok())
        .filter(|(x, y)| {
            x + 16 + usize::from(half_x) <= width && y + rows + usize::from(half_y) <= height
        });
    if let Some((x, y)) = inside {
        let step = usize::from(half_x);
        for row in 0..rows {
            let top = (y + row) * width + x;
            let bottom = top + if half_y { width } else { 0 };
            let (Some(a), Some(b), Some(out)) = (
                reference.y.get(top..top + 16 + step),
                reference.y.get(bottom..bottom + 16 + step),
                luma.get_mut(row * 16..row * 16 + 16),
            ) else {
                continue;
            };
            for (col, slot) in out.iter_mut().enumerate() {
                let sum = match (half_x, half_y) {
                    (false, false) => u16::from(a[col]),
                    (true, false) => (u16::from(a[col]) + u16::from(a[col + 1]) + 1) >> 1,
                    (false, true) => (u16::from(a[col]) + u16::from(b[col]) + 1) >> 1,
                    (true, true) => {
                        (u16::from(a[col])
                            + u16::from(a[col + 1])
                            + u16::from(b[col])
                            + u16::from(b[col + 1])
                            + 2)
                            >> 2
                    }
                };
                *slot = u8::try_from(sum).unwrap_or(255);
            }
        }
        return luma;
    }

    let (step_x, step_y) = (i32::from(half_x), i32::from(half_y));
    for row in 0..rows {
        for col in 0..16 {
            let x = ref_x.saturating_add(i32::try_from(col).unwrap_or(0));
            let y = ref_y.saturating_add(i32::try_from(row).unwrap_or(0));
            let a = u16::from(sample_y(reference, x, y));
            let b = u16::from(sample_y(reference, x + step_x, y));
            let c = u16::from(sample_y(reference, x, y + step_y));
            let d = u16::from(sample_y(reference, x + step_x, y + step_y));
            if let Some(slot) = luma.get_mut(row * 16 + col) {
                *slot = u8::try_from((a + b + c + d + 2) >> 2).unwrap_or(255);
            }
        }
    }
    luma
}

/// The sum of absolute differences between the macroblock of `cur` at (`mbx`, `mby`) and a prediction of it.
pub(crate) fn sad_prediction(
    cur: &Frame420,
    mbx: usize,
    mby: usize,
    prediction: &[u8; 256],
) -> u32 {
    sad_prediction_rows(cur, (mbx, mby, 16), prediction)
}

/// [`sad_prediction`] of a block `rows` high.
pub(crate) fn sad_prediction_rows(
    cur: &Frame420,
    (mbx, mby, rows): (usize, usize, usize),
    prediction: &[u8; 256],
) -> u32 {
    let width = usize::from(cur.width);
    let mut sum = 0u32;
    for row in 0..rows.min(16) {
        let start = (mby + row) * width + mbx;
        let (Some(line), Some(predicted)) = (
            cur.y.get(start..start + 16),
            prediction.get(row * 16..row * 16 + 16),
        ) else {
            return u32::MAX;
        };
        for (a, b) in line.iter().zip(predicted) {
            sum += u32::from(a.abs_diff(*b));
        }
    }
    sum
}

/// Refines a whole-pixel vector (`best`: dx, dy and its SAD, from [`search_fast`]) to half a pixel: the eight
/// half-pixel places around it that stay inside the picture and inside +/- `range` pixels. Returns the vector in
/// half pixels and its SAD.
pub(crate) fn refine_half(
    cur: &Frame420,
    reference: &Frame420,
    (mbx, mby): (usize, usize),
    best: (i32, i32, u32),
    range: i32,
) -> (i32, i32, u32) {
    refine_half_rows(cur, reference, (mbx, mby, 16), best, range)
}

/// [`refine_half`] of a block `rows` high.
pub(crate) fn refine_half_rows(
    cur: &Frame420,
    reference: &Frame420,
    (mbx, mby, rows): (usize, usize, usize),
    best: (i32, i32, u32),
    range: i32,
) -> (i32, i32, u32) {
    let centre = (best.0.saturating_mul(2), best.1.saturating_mul(2));
    let mut found = (centre.0, centre.1, best.2);
    let Some(win) = Window::rows(reference, (mbx, mby, rows), range.max(0)) else {
        return found;
    };
    // A half-pixel component reads one sample further right or down.
    let fits = |half: i32, min: i32, max: i32| {
        let whole = half >> 1;
        whole >= min && whole + (half & 1) <= max
    };
    for oy in -1..=1 {
        for ox in -1..=1 {
            let (vx, vy) = (centre.0 + ox, centre.1 + oy);
            if (ox, oy) == (0, 0)
                || !fits(vx, win.dx_min, win.dx_max)
                || !fits(vy, win.dy_min, win.dy_max)
            {
                continue;
            }
            let prediction = predict_luma_rows(reference, (mbx, mby, rows), vx, vy);
            let sad = sad_prediction_rows(cur, (mbx, mby, rows), &prediction);
            if sad < found.2 {
                found = (vx, vy, sad);
            }
        }
    }
    found
}

/// Predict a macroblock from `reference` using a motion vector in luma half-pel units.
///
/// Chroma vectors are luma vectors divided by two, in chroma half-pel units, with half-pel chroma averaging as
/// required by MPEG-2.
pub(crate) fn predict(
    reference: &Frame420,
    mbx: usize,
    mby: usize,
    mv_x_half: i32,
    mv_y_half: i32,
) -> ([u8; 256], [u8; 64], [u8; 64]) {
    let luma = predict_luma(reference, mbx, mby, mv_x_half, mv_y_half);

    let chroma_width = usize::from(reference.width) / 2;
    let chroma_height = usize::from(reference.height) / 2;

    let chroma_vx = mv_x_half / 2;
    let chroma_vy = mv_y_half / 2;

    let chroma_x = i32::try_from(mbx / 2)
        .unwrap_or(0)
        .saturating_add(chroma_vx / 2);
    let chroma_y = i32::try_from(mby / 2)
        .unwrap_or(0)
        .saturating_add(chroma_vy / 2);

    let fx = chroma_vx % 2;
    let fy = chroma_vy % 2;

    let cb = predict_chroma(
        &reference.cb,
        (chroma_width, chroma_height, 8),
        chroma_x,
        chroma_y,
        fx,
        fy,
    );
    let cr = predict_chroma(
        &reference.cr,
        (chroma_width, chroma_height, 8),
        chroma_x,
        chroma_y,
        fx,
        fy,
    );

    (luma, cb, cr)
}

/// One field of `frame` as a picture of its own (13818-2 6.1.1.4): its lines `parity`, `parity` + 2, … of each plane
/// (0: the top field), half as high.
pub(crate) fn field(frame: &Frame420, parity: usize) -> Frame420 {
    let (w, h) = (usize::from(frame.width), usize::from(frame.height));
    let lines = |plane: &[u8], width: usize, height: usize| -> Vec<u8> {
        (parity..height)
            .step_by(2)
            .flat_map(|row| {
                plane
                    .get(row * width..(row + 1) * width)
                    .unwrap_or(&[])
                    .iter()
                    .copied()
            })
            .collect()
    };
    Frame420 {
        width: frame.width,
        height: frame.height / 2,
        y: lines(&frame.y, w, h),
        cb: lines(&frame.cb, w / 2, h / 2),
        cr: lines(&frame.cr, w / 2, h / 2),
    }
}

/// The 16 x 8 luma and 8 x 4 chroma prediction of one field of the macroblock at (`mbx`, `mby`) of a frame picture
/// (`mby` in the frame's lines) from the reference field `reference` (a [`field`] picture), for a field vector in
/// half pixels (its vertical part in field lines): chroma from the vector halved, toward zero (13818-2 7.6.3.7).
pub(crate) fn predict_field(
    reference: &Frame420,
    (mbx, mby): (usize, usize),
    (mv_x_half, mv_y_half): (i32, i32),
) -> ([u8; 256], [u8; 64], [u8; 64]) {
    let luma = predict_luma_rows(reference, (mbx, mby / 2, 8), mv_x_half, mv_y_half);
    let (chroma_width, chroma_height) = (
        usize::from(reference.width) / 2,
        usize::from(reference.height) / 2,
    );
    let (chroma_vx, chroma_vy) = (mv_x_half / 2, mv_y_half / 2);
    let chroma_x = i32::try_from(mbx / 2)
        .unwrap_or(0)
        .saturating_add(chroma_vx / 2);
    let chroma_y = i32::try_from(mby / 4)
        .unwrap_or(0)
        .saturating_add(chroma_vy / 2);
    let chroma = |plane: &[u8]| {
        predict_chroma(
            plane,
            (chroma_width, chroma_height, 4),
            chroma_x,
            chroma_y,
            chroma_vx % 2,
            chroma_vy % 2,
        )
    };
    (luma, chroma(&reference.cb), chroma(&reference.cr))
}

/// Intra cost of a 16x16 luma block: sum of absolute deviations from the block mean.
pub(crate) fn intra_cost(cur: &Frame420, mbx: usize, mby: usize) -> u32 {
    let mut sum = 0u32;

    for row in 0..16 {
        for col in 0..16 {
            let x = i32::try_from(mbx + col).unwrap_or(i32::MAX);
            let y = i32::try_from(mby + row).unwrap_or(i32::MAX);
            sum += u32::from(sample_y(cur, x, y));
        }
    }

    let mean = sum / 256;
    let mut cost = 0u32;

    for row in 0..16 {
        for col in 0..16 {
            let x = i32::try_from(mbx + col).unwrap_or(i32::MAX);
            let y = i32::try_from(mby + row).unwrap_or(i32::MAX);
            let value = u32::from(sample_y(cur, x, y));

            cost += value.abs_diff(mean);
        }
    }

    cost
}

#[cfg(test)]
mod tests {
    use super::{predict, search};
    use crate::Frame420;

    fn blank_frame(width: u16, height: u16) -> Frame420 {
        let w = usize::from(width);
        let h = usize::from(height);
        let cw = w / 2;
        let ch = h / 2;

        Frame420 {
            width,
            height,
            y: vec![0u8; w * h],
            cb: vec![0u8; cw * ch],
            cr: vec![0u8; cw * ch],
        }
    }

    #[test]
    fn search_finds_shifted_block() {
        let mut cur = blank_frame(32, 32);
        let mut reference = blank_frame(32, 32);

        for row in 0..16 {
            for col in 0..16 {
                let value = (row * 16 + col) as u8;
                if let Some(slot) = cur.y.get_mut(row * 32 + col) {
                    *slot = value;
                }
                if let Some(slot) = reference.y.get_mut((row + 2) * 32 + (col + 3)) {
                    *slot = value;
                }
            }
        }

        let (dx, dy, sad) = search(&cur, &reference, 0, 0, 15);
        assert_eq!(dx, 3);
        assert_eq!(dy, 2);
        assert_eq!(sad, 0);
    }

    #[test]
    fn predict_odd_luma_vector_averages_chroma() {
        let mut reference = blank_frame(16, 16);

        if let Some(slot) = reference.cb.get_mut(0) {
            *slot = 10;
        }
        if let Some(slot) = reference.cb.get_mut(1) {
            *slot = 20;
        }

        let (_, cb_pred, _) = predict(&reference, 0, 0, 2, 0);
        assert_eq!(cb_pred.first().copied(), Some(15));
    }

    /// A frame whose luma is `f(x, y)`.
    fn frame_of(width: u16, height: u16, f: impl Fn(usize, usize) -> u8) -> Frame420 {
        let mut frame = blank_frame(width, height);
        for y in 0..usize::from(height) {
            for x in 0..usize::from(width) {
                frame.y[y * usize::from(width) + x] = f(x, y);
            }
        }
        frame
    }

    /// M12.12: an odd vector component averages a sample with its right (lower) neighbour, rounding up; the whole
    /// part is the floor, so -3 half pixels is two pixels back and half a pixel forward.
    #[test]
    fn predict_luma_averages_at_half_pixels() {
        let reference = frame_of(64, 64, |x, y| (x * 3 + y * 5) as u8);
        let at = |x: usize, y: usize| u16::from(reference.y[y * 64 + x]);
        let whole = super::predict_luma(&reference, 16, 16, 4, -2);
        assert_eq!(u16::from(whole[0]), at(18, 15));
        let right = super::predict_luma(&reference, 16, 16, 1, 0);
        assert_eq!(u16::from(right[0]), (at(16, 16) + at(17, 16) + 1) >> 1);
        let back = super::predict_luma(&reference, 16, 16, -3, 0);
        assert_eq!(u16::from(back[0]), (at(14, 16) + at(15, 16) + 1) >> 1);
        let both = super::predict_luma(&reference, 16, 16, 1, -1);
        assert_eq!(
            u16::from(both[17]),
            (at(17, 16) + at(18, 16) + at(17, 17) + at(18, 17) + 2) >> 2
        );
        // At the picture's edge the samples outside repeat the edge (never read by a vector the search gives).
        let edge = super::predict_luma(&reference, 48, 48, 1, 1);
        assert_eq!(u16::from(edge[255]), at(63, 63));
    }

    /// The refinement finds a picture moved by half a pixel, and stays inside the picture and the range.
    #[test]
    fn refine_half_finds_half_pixel_motion() {
        let texture = |x: usize, y: usize| ((x * 37 + y * 11 + (x * y) % 13) % 200) as u8 + 20;
        let reference = frame_of(64, 64, texture);
        // The current picture is the reference half a pixel to the right and down.
        let current = frame_of(64, 64, |x, y| {
            let sum = u16::from(texture(x, y))
                + u16::from(texture(x + 1, y))
                + u16::from(texture(x, y + 1))
                + u16::from(texture(x + 1, y + 1));
            ((sum + 2) >> 2) as u8
        });
        let whole = (0, 0, super::sad_block(&current, &reference, 16, 16, 0, 0));
        assert!(whole.2 > 0);
        assert_eq!(
            super::refine_half(&current, &reference, (16, 16), whole, 15),
            (1, 1, 0)
        );

        // In the last macroblock there is nothing to the right or below to average with: only vectors back.
        let corner = (0, 0, u32::MAX);
        let (vx, vy, _) = super::refine_half(&current, &reference, (48, 48), corner, 15);
        assert!(vx <= 0 && vy <= 0, "({vx}, {vy})");
        // With no range the vector stays.
        assert_eq!(
            super::refine_half(&current, &reference, (16, 16), (0, 0, 7), 0),
            (0, 0, 7)
        );
    }
}
