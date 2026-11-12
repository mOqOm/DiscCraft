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

fn sad_block(
    cur: &Frame420,
    reference: &Frame420,
    mbx: usize,
    mby: usize,
    dx: i32,
    dy: i32,
) -> u32 {
    let mut sum = 0u32;

    for row in 0..16 {
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
    fn new(reference: &Frame420, mbx: usize, mby: usize, range: i32) -> Option<Self> {
        let (w, h) = (usize::from(reference.width), usize::from(reference.height));
        if w < 16 || h < 16 {
            return None;
        }
        let (x, y) = (i32::try_from(mbx).ok()?, i32::try_from(mby).ok()?);
        let win = Window {
            dx_min: (-x).max(-range),
            dy_min: (-y).max(-range),
            dx_max: i32::try_from(w - 16).ok()?.saturating_sub(x).min(range),
            dy_max: i32::try_from(h - 16).ok()?.saturating_sub(y).min(range),
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

/// Predictive diamond search (full-pel): start from the better of (0, 0) and `pred`, walk the large diamond until its
/// centre is best, then refine with the small diamond. About 20–40 SAD evaluations instead of (2·range + 1)².
pub(crate) fn search_fast(
    cur: &Frame420,
    reference: &Frame420,
    mbx: usize,
    mby: usize,
    range: i32,
    pred: (i32, i32),
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
    let Some(win) = Window::new(reference, mbx, mby, range.max(0)) else {
        return (0, 0, 0);
    };
    let mut best = (0, 0, sad_block(cur, reference, mbx, mby, 0, 0));
    if win.contains(pred.0, pred.1) && pred != (0, 0) {
        let sad = sad_block(cur, reference, mbx, mby, pred.0, pred.1);
        if better(best, pred.0, pred.1, sad) {
            best = (pred.0, pred.1, sad);
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
                    let sad = sad_block(cur, reference, mbx, mby, dx, dy);
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
    width: usize,
    height: usize,
    x: i32,
    y: i32,
    fx: i32,
    fy: i32,
) -> [u8; 64] {
    let mut out = [0u8; 64];

    for row in 0..8 {
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

/// Predict a macroblock from `reference` using a motion vector in luma half-pel units.
///
/// Luma prediction is full-pel in this encoder. Chroma vectors are luma vectors divided by
/// two, in chroma half-pel units, with half-pel chroma averaging as required by MPEG-2.
pub(crate) fn predict(
    reference: &Frame420,
    mbx: usize,
    mby: usize,
    mv_x_half: i32,
    mv_y_half: i32,
) -> ([u8; 256], [u8; 64], [u8; 64]) {
    let mut luma = [0u8; 256];

    let luma_x = mv_x_half / 2;
    let luma_y = mv_y_half / 2;
    let ref_x = i32::try_from(mbx).unwrap_or(0).saturating_add(luma_x);
    let ref_y = i32::try_from(mby).unwrap_or(0).saturating_add(luma_y);

    for row in 0..16 {
        for col in 0..16 {
            let x = ref_x.saturating_add(i32::try_from(col).unwrap_or(0));
            let y = ref_y.saturating_add(i32::try_from(row).unwrap_or(0));

            if let Some(slot) = luma.get_mut(row * 16 + col) {
                *slot = sample_y(reference, x, y);
            }
        }
    }

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
        chroma_width,
        chroma_height,
        chroma_x,
        chroma_y,
        fx,
        fy,
    );
    let cr = predict_chroma(
        &reference.cr,
        chroma_width,
        chroma_height,
        chroma_x,
        chroma_y,
        fx,
        fy,
    );

    (luma, cb, cr)
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
}
