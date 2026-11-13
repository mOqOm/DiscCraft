//! Highlight layer construction.

use crate::MenuError;
use disccraft_project::{Menu, Rect};

/// A sparse highlight layer covering the union of button rectangles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HighlightLayer {
    /// Left edge.
    pub x: u16,
    /// Top edge.
    pub y: u16,
    /// Width.
    pub width: u16,
    /// Height.
    pub height: u16,
    /// Pixel values: 0 for empty, else the colour (1 to 3) of the colour set — 1 for a frame or a text pixel covered
    /// by half or more, a button's own subpicture its layers' colours.
    pub pixels: Vec<u8>,
    /// How much of each pixel the highlight covers, 0 to 255 (the smooth edges of a text highlight; a frame covers
    /// its pixels fully). Rich-colour Blu-ray highlights use it.
    pub coverage: Vec<u8>,
}

pub(crate) fn build(menu: &Menu, _width: u16, _height: u16) -> Result<HighlightLayer, MenuError> {
    // A menu without buttons (shown in an editor) has a 1 x 1 transparent highlight.
    if menu.buttons.is_empty() {
        return Ok(HighlightLayer {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            pixels: vec![0],
            coverage: vec![0],
        });
    }
    let (x, y, layer_w, layer_h) = button_bounds(menu)?;

    let w = usize::from(layer_w);
    let h = usize::from(layer_h);
    let len = w
        .checked_mul(h)
        .ok_or_else(|| MenuError::Invalid("highlight layer too large".to_string()))?;

    let mut pixels = vec![0u8; len];
    let mut coverage = vec![0u8; len];

    for button in &menu.buttons {
        // A button's own subpicture (a PSD button's subpicture layers) comes first.
        if let Some((values, sub_w, sub_h)) = button.subpicture.as_ref().and_then(|sub| {
            sub.values()
                .map(|values| (values, usize::from(sub.width), usize::from(sub.height)))
        }) {
            paint_subpicture(
                &mut pixels,
                (x, y, layer_w, layer_h),
                button.rect,
                (&values, sub_w, sub_h),
            );
            continue;
        }
        // Encore's "Create Text Subpicture": the highlight is the button's text; a button without text keeps the
        // frame.
        let painted = button.text_subpicture
            && crate::draw::label_mask(menu, button).is_some_and(|(mask, mask_x, mask_y)| {
                paint_mask(
                    (&mut pixels, &mut coverage),
                    (x, y, layer_w, layer_h),
                    button.rect,
                    &mask,
                    (mask_x, mask_y),
                )
            });
        if !painted {
            paint_frame(&mut pixels, x, y, layer_w, layer_h, button.rect);
        }
    }
    // A frame or a subpicture covers its pixels fully.
    for (cover, pixel) in coverage.iter_mut().zip(&pixels) {
        if *pixel != 0 {
            *cover = u8::MAX;
        }
    }

    Ok(HighlightLayer {
        x,
        y,
        width: layer_w,
        height: layer_h,
        pixels,
        coverage,
    })
}

fn button_bounds(menu: &Menu) -> Result<(u16, u16, u16, u16), MenuError> {
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut any = false;

    for button in &menu.buttons {
        let rect = button.rect;
        let x = u32::from(rect.x);
        let y = u32::from(rect.y);
        let x1 = x.saturating_add(u32::from(rect.w).saturating_sub(1));
        let y1 = y.saturating_add(u32::from(rect.h).saturating_sub(1));

        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x1);
        max_y = max_y.max(y1);
        any = true;
    }

    if !any {
        return Ok((0, 0, 1, 1));
    }

    let width = max_x
        .checked_sub(min_x)
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| MenuError::Invalid("highlight bounds too large".to_string()))?;
    let height = max_y
        .checked_sub(min_y)
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| MenuError::Invalid("highlight bounds too large".to_string()))?;

    let x = u16::try_from(min_x)
        .map_err(|_| MenuError::Invalid("highlight x too large".to_string()))?;
    let y = u16::try_from(min_y)
        .map_err(|_| MenuError::Invalid("highlight y too large".to_string()))?;
    let width = u16::try_from(width)
        .map_err(|_| MenuError::Invalid("highlight width too large".to_string()))?;
    let height = u16::try_from(height)
        .map_err(|_| MenuError::Invalid("highlight height too large".to_string()))?;

    Ok((x, y, width, height))
}

fn paint_frame(
    pixels: &mut [u8],
    layer_x: u16,
    layer_y: u16,
    layer_w: u16,
    layer_h: u16,
    rect: Rect,
) {
    if layer_w == 0 || layer_h == 0 {
        return;
    }

    let lx = u32::from(layer_x);
    let ly = u32::from(layer_y);
    let lw = u32::from(layer_w);
    let lh = u32::from(layer_h);

    let Some(lx1) = lx.checked_add(lw).and_then(|n| n.checked_sub(1)) else {
        return;
    };
    let Some(ly1) = ly.checked_add(lh).and_then(|n| n.checked_sub(1)) else {
        return;
    };

    let rx = u32::from(rect.x);
    let ry = u32::from(rect.y);
    let rx1 = rx.saturating_add(u32::from(rect.w).saturating_sub(1));
    let ry1 = ry.saturating_add(u32::from(rect.h).saturating_sub(1));

    let start_x = rx.max(lx);
    let end_x = rx1.min(lx1);
    let start_y = ry.max(ly);
    let end_y = ry1.min(ly1);

    if start_x > end_x || start_y > end_y {
        return;
    }

    for y in start_y..=end_y {
        for x in start_x..=end_x {
            if !is_frame(x, y, rx, ry, rx1, ry1) {
                continue;
            }

            let Some(rel_y) = y.checked_sub(ly) else {
                continue;
            };
            let Some(rel_x) = x.checked_sub(lx) else {
                continue;
            };
            let Some(idx) = rel_y
                .checked_mul(lw)
                .and_then(|n| n.checked_add(rel_x))
                .and_then(|n| usize::try_from(n).ok())
            else {
                continue;
            };

            if let Some(px) = pixels.get_mut(idx) {
                *px = 1;
            }
        }
    }
}

/// Sets the layer pixels the text `mask` (drawn at `origin`) covers by half or more, inside the button's `rect`, and
/// records every covered pixel's coverage. Returns whether any pixel was set.
fn paint_mask(
    (pixels, coverages): (&mut [u8], &mut [u8]),
    layer: (u16, u16, u16, u16),
    rect: Rect,
    mask: &filmcraft_text::Mask,
    origin: (i32, i32),
) -> bool {
    let (layer_x, layer_y, layer_w, layer_h) = layer;
    let inside = |value: i64, start: u16, length: u16| {
        value >= i64::from(start) && value < i64::from(start) + i64::from(length)
    };
    let mut any = false;
    for row in 0..mask.h {
        for col in 0..mask.w {
            let Some(coverage) = row
                .checked_mul(mask.w)
                .and_then(|n| n.checked_add(col))
                .and_then(|n| mask.a.get(n))
            else {
                continue;
            };
            if *coverage <= 0.0 {
                continue;
            }
            let (Ok(col), Ok(row)) = (i64::try_from(col), i64::try_from(row)) else {
                continue;
            };
            let fx = i64::from(origin.0).saturating_add(col);
            let fy = i64::from(origin.1).saturating_add(row);
            if !inside(fx, rect.x, rect.w)
                || !inside(fy, rect.y, rect.h)
                || !inside(fx, layer_x, layer_w)
                || !inside(fy, layer_y, layer_h)
            {
                continue;
            }
            let index = (fy - i64::from(layer_y))
                .checked_mul(i64::from(layer_w))
                .and_then(|n| n.checked_add(fx - i64::from(layer_x)))
                .and_then(|n| usize::try_from(n).ok());
            let Some(index) = index else {
                continue;
            };
            if let Some(cover) = coverages.get_mut(index) {
                // Coverage is 0 to 1; the product is well inside u8.
                let value = (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
                *cover = (*cover).max(value);
            }
            if *coverage < 0.5 {
                continue;
            }
            if let Some(pixel) = pixels.get_mut(index) {
                *pixel = 1;
                any = true;
            }
        }
    }
    any
}

/// Paints a subpicture of `sub_w` x `sub_h` values stretched over the button's `rect` (nearest pixel), where it
/// draws.
fn paint_subpicture(
    pixels: &mut [u8],
    (layer_x, layer_y, layer_w, layer_h): (u16, u16, u16, u16),
    rect: Rect,
    (values, sub_w, sub_h): (&[u8], usize, usize),
) {
    let (rect_w, rect_h) = (usize::from(rect.w), usize::from(rect.h));
    if rect_w == 0 || rect_h == 0 {
        return;
    }
    for row in 0..rect_h {
        let Some(y) = usize::from(rect.y)
            .checked_add(row)
            .and_then(|y| y.checked_sub(usize::from(layer_y)))
            .filter(|y| *y < usize::from(layer_h))
        else {
            continue;
        };
        let source_row = row.saturating_mul(sub_h) / rect_h;
        for col in 0..rect_w {
            let Some(x) = usize::from(rect.x)
                .checked_add(col)
                .and_then(|x| x.checked_sub(usize::from(layer_x)))
                .filter(|x| *x < usize::from(layer_w))
            else {
                continue;
            };
            let source_col = col.saturating_mul(sub_w) / rect_w;
            let value = source_row
                .checked_mul(sub_w)
                .and_then(|n| n.checked_add(source_col))
                .and_then(|n| values.get(n))
                .copied()
                .unwrap_or(0);
            if value == 0 {
                continue;
            }
            if let Some(pixel) = y
                .checked_mul(usize::from(layer_w))
                .and_then(|n| n.checked_add(x))
                .and_then(|n| pixels.get_mut(n))
            {
                *pixel = value;
            }
        }
    }
}

/// The highlight of button `index` alone over its rectangle (`width` x `height` values, row by row), as the disc
/// draws it; `None` without such a button.
pub(crate) fn button_values(menu: &Menu, index: usize) -> Option<Vec<u8>> {
    let button = menu.buttons.get(index)?;
    let alone = Menu {
        buttons: vec![button.clone()],
        ..menu.clone()
    };
    let layer = build(&alone, 0, 0).ok()?;
    let rect = button.rect;
    let (rect_w, rect_h) = (usize::from(rect.w), usize::from(rect.h));
    let mut values = vec![0_u8; rect_w.checked_mul(rect_h)?];
    for row in 0..rect_h {
        for col in 0..rect_w {
            let x = usize::from(rect.x) + col;
            let y = usize::from(rect.y) + row;
            let Some(lx) = x.checked_sub(usize::from(layer.x)) else {
                continue;
            };
            let Some(ly) = y.checked_sub(usize::from(layer.y)) else {
                continue;
            };
            if lx >= usize::from(layer.width) || ly >= usize::from(layer.height) {
                continue;
            }
            let value = ly
                .checked_mul(usize::from(layer.width))
                .and_then(|n| n.checked_add(lx))
                .and_then(|n| layer.pixels.get(n))
                .copied()
                .unwrap_or(0);
            if let Some(slot) = values.get_mut(row * rect_w + col) {
                *slot = value;
            }
        }
    }
    Some(values)
}

fn is_frame(x: u32, y: u32, rx: u32, ry: u32, rx1: u32, ry1: u32) -> bool {
    let left = rx.saturating_add(3);
    let right = rx1.saturating_sub(3);
    let top = ry.saturating_add(3);
    let bottom = ry1.saturating_sub(3);

    x <= left || x >= right || y <= top || y >= bottom
}
