//! Drop shadows (Encore's Object ▸ Drop Shadow…): a blurred, offset copy of an object's coverage mask, drawn
//! behind the object in the shadow's colour.

use disccraft_project::{DropShadow, MAX_SHADOW_SIZE};
use filmcraft_text::Mask;

/// Most pixels a shadow mask may hold (a frame-sized object with the widest blur fits well inside).
const MAX_SHADOW_PIXELS: usize = 4096 * 4096;

/// The shadow of `mask` (an object drawn with its top-left at `x`, `y`): the mask spread and blurred by the
/// shadow's size, moved by its distance and angle, with its opacity already multiplied in. Returns the shadow
/// mask and where to draw it. `None` when there is nothing to draw.
pub(crate) fn shadow_mask(
    mask: &Mask,
    x: i32,
    y: i32,
    shadow: &DropShadow,
) -> Option<(Mask, i32, i32)> {
    if mask.w == 0 || mask.h == 0 || shadow.opacity == 0 {
        return None;
    }
    let size = usize::from(shadow.size.min(MAX_SHADOW_SIZE));
    let spread = usize::from(shadow.spread.min(100));
    // The solid part of the size, then the blurred rest.
    let solid = size.checked_mul(spread)? / 100;
    let blur = size.saturating_sub(solid);
    // Two box blurs of half the radius each reach about the whole blur; one more pixel of room for rounding.
    let blur_radius = blur.div_ceil(2);
    let pad = solid
        .checked_add(blur_radius.checked_mul(2)?)?
        .checked_add(1)?;
    let w = mask.w.checked_add(pad.checked_mul(2)?)?;
    let h = mask.h.checked_add(pad.checked_mul(2)?)?;
    if w.checked_mul(h)? > MAX_SHADOW_PIXELS {
        return None;
    }
    let mut out = Mask::new(w, h);
    for row in 0..mask.h {
        for col in 0..mask.w {
            let Some(source) = row
                .checked_mul(mask.w)
                .and_then(|n| n.checked_add(col))
                .and_then(|n| mask.a.get(n))
            else {
                continue;
            };
            if let Some(target) = row
                .checked_add(pad)
                .and_then(|r| r.checked_mul(w))
                .and_then(|n| n.checked_add(col.checked_add(pad)?))
                .and_then(|n| out.a.get_mut(n))
            {
                *target = source.clamp(0.0, 1.0);
            }
        }
    }
    if solid > 0 {
        dilate(&mut out, solid);
    }
    if blur_radius > 0 {
        box_blur(&mut out, blur_radius);
        box_blur(&mut out, blur_radius);
    }
    let opacity = f32::from(shadow.opacity.min(100)) / 100.0;
    for value in &mut out.a {
        *value = (*value * opacity).clamp(0.0, 1.0);
    }
    let (dx, dy) = shadow.offset();
    let pad = i32::try_from(pad).ok()?;
    Some((
        out,
        x.checked_sub(pad)?.checked_add(dx)?,
        y.checked_sub(pad)?.checked_add(dy)?,
    ))
}

/// Grows every covered pixel by `radius` pixels in each direction (a square maximum filter, run as two passes).
fn dilate(mask: &mut Mask, radius: usize) {
    let w = mask.w;
    let h = mask.h;
    let mut temp = vec![0.0f32; mask.a.len()];
    // Horizontal pass.
    for row in 0..h {
        for col in 0..w {
            let from = col.saturating_sub(radius);
            let to = col.saturating_add(radius).min(w.saturating_sub(1));
            let mut best = 0.0f32;
            for k in from..=to {
                if let Some(value) = row
                    .checked_mul(w)
                    .and_then(|n| n.checked_add(k))
                    .and_then(|n| mask.a.get(n))
                {
                    best = best.max(*value);
                }
            }
            if let Some(target) = row
                .checked_mul(w)
                .and_then(|n| n.checked_add(col))
                .and_then(|n| temp.get_mut(n))
            {
                *target = best;
            }
        }
    }
    // Vertical pass.
    for col in 0..w {
        for row in 0..h {
            let from = row.saturating_sub(radius);
            let to = row.saturating_add(radius).min(h.saturating_sub(1));
            let mut best = 0.0f32;
            for k in from..=to {
                if let Some(value) = k
                    .checked_mul(w)
                    .and_then(|n| n.checked_add(col))
                    .and_then(|n| temp.get(n))
                {
                    best = best.max(*value);
                }
            }
            if let Some(target) = row
                .checked_mul(w)
                .and_then(|n| n.checked_add(col))
                .and_then(|n| mask.a.get_mut(n))
            {
                *target = best;
            }
        }
    }
}

/// Averages every pixel with its neighbours within `radius` (a box blur, run as two passes over prefix sums).
fn box_blur(mask: &mut Mask, radius: usize) {
    let w = mask.w;
    let h = mask.h;
    let mut temp = vec![0.0f32; mask.a.len()];
    blur_lines(&mask.a, &mut temp, w, h, radius, true);
    blur_lines(&temp, &mut mask.a, w, h, radius, false);
}

/// One blur pass: along rows when `horizontal`, else along columns.
fn blur_lines(
    source: &[f32],
    target: &mut [f32],
    w: usize,
    h: usize,
    radius: usize,
    horizontal: bool,
) {
    let (lines, length) = if horizontal { (h, w) } else { (w, h) };
    let index = |line: usize, k: usize| -> Option<usize> {
        if horizontal {
            line.checked_mul(w)?.checked_add(k)
        } else {
            k.checked_mul(w)?.checked_add(line)
        }
    };
    let mut prefix = vec![0.0f32; length.saturating_add(1)];
    for line in 0..lines {
        let mut total = 0.0f32;
        for k in 0..length {
            total += index(line, k)
                .and_then(|n| source.get(n))
                .copied()
                .unwrap_or(0.0);
            if let Some(slot) = prefix.get_mut(k.saturating_add(1)) {
                *slot = total;
            }
        }
        let window = f32::from(
            u16::try_from(radius.saturating_mul(2).saturating_add(1)).unwrap_or(u16::MAX),
        );
        for k in 0..length {
            let from = k.saturating_sub(radius);
            let to = k.saturating_add(radius).saturating_add(1).min(length);
            let sum =
                prefix.get(to).copied().unwrap_or(0.0) - prefix.get(from).copied().unwrap_or(0.0);
            if let Some(slot) = index(line, k).and_then(|n| target.get_mut(n)) {
                *slot = sum / window;
            }
        }
    }
}

/// Sends every covered pixel of `mask` (drawn at `x`, `y`) that lies inside the `width` x `height` frame to
/// `sink`, as the shadow's colour with the mask's value as coverage.
pub(crate) fn paint_mask(
    mask: &Mask,
    x: i32,
    y: i32,
    color: [u8; 3],
    width: u16,
    height: u16,
    sink: &mut crate::graphic::Sink<'_>,
) {
    let width = i64::from(width);
    let height = i64::from(height);
    for row in 0..mask.h {
        for col in 0..mask.w {
            let Some(coverage) = row
                .checked_mul(mask.w)
                .and_then(|n| n.checked_add(col))
                .and_then(|n| mask.a.get(n))
            else {
                continue;
            };
            let coverage = coverage.clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }
            let (Ok(col), Ok(row)) = (i64::try_from(col), i64::try_from(row)) else {
                continue;
            };
            let fx = i64::from(x).saturating_add(col);
            let fy = i64::from(y).saturating_add(row);
            if fx < 0 || fy < 0 || fx >= width || fy >= height {
                continue;
            }
            if let (Ok(fx), Ok(fy)) = (usize::try_from(fx), usize::try_from(fy)) {
                sink(fx, fy, color, coverage);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::shadow_mask;
    use disccraft_project::DropShadow;
    use filmcraft_text::Mask;

    fn dot() -> Mask {
        let mut mask = Mask::new(3, 3);
        mask.a[4] = 1.0;
        mask
    }

    #[test]
    fn a_sharp_shadow_is_the_mask_moved_by_the_distance() {
        let shadow = DropShadow {
            opacity: 100,
            angle: 90,
            distance: 4,
            size: 0,
            ..DropShadow::default()
        };
        let (mask, x, y) =
            shadow_mask(&dot(), 10, 20, &shadow).unwrap_or_else(|| (Mask::new(0, 0), 0, 0));
        // Light from above: the shadow falls straight down.
        let (col, row) = (0..mask.w * mask.h)
            .find(|i| mask.a[*i] > 0.99)
            .map(|i| (i % mask.w, i / mask.w))
            .unwrap_or((usize::MAX, usize::MAX));
        assert_eq!((x + col as i32, y + row as i32), (10 + 1, 20 + 1 + 4));
    }

    #[test]
    fn blur_spreads_the_coverage_and_opacity_scales_it() {
        let shadow = DropShadow {
            opacity: 50,
            distance: 0,
            size: 2,
            ..DropShadow::default()
        };
        let (mask, _, _) =
            shadow_mask(&dot(), 0, 0, &shadow).unwrap_or_else(|| (Mask::new(0, 0), 0, 0));
        let covered = mask.a.iter().filter(|a| **a > 0.0).count();
        assert!(covered > 1, "the blur covers more than the dot");
        let total: f32 = mask.a.iter().sum();
        assert!(
            (total - 0.5).abs() < 0.01,
            "a blur keeps the coverage, opacity halves it: {total}"
        );
    }

    #[test]
    fn spread_makes_a_solid_core() {
        let shadow = DropShadow {
            opacity: 100,
            distance: 0,
            size: 2,
            spread: 100,
            ..DropShadow::default()
        };
        let (mask, _, _) =
            shadow_mask(&dot(), 0, 0, &shadow).unwrap_or_else(|| (Mask::new(0, 0), 0, 0));
        let solid = mask.a.iter().filter(|a| **a > 0.99).count();
        assert_eq!(solid, 25, "a 2-pixel spread makes a 5x5 solid square");
    }

    #[test]
    fn no_shadow_when_fully_transparent_or_empty() {
        let shadow = DropShadow {
            opacity: 0,
            ..DropShadow::default()
        };
        assert!(shadow_mask(&dot(), 0, 0, &shadow).is_none());
        assert!(shadow_mask(&Mask::new(0, 0), 0, 0, &DropShadow::default()).is_none());
    }
}
