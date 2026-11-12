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
    /// Pixel values, 0 for empty and 1 for highlight.
    pub pixels: Vec<u8>,
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
        });
    }
    let (x, y, layer_w, layer_h) = button_bounds(menu)?;

    let w = usize::from(layer_w);
    let h = usize::from(layer_h);
    let len = w
        .checked_mul(h)
        .ok_or_else(|| MenuError::Invalid("highlight layer too large".to_string()))?;

    let mut pixels = vec![0u8; len];

    for button in &menu.buttons {
        paint_frame(&mut pixels, x, y, layer_w, layer_h, button.rect);
    }

    Ok(HighlightLayer {
        x,
        y,
        width: layer_w,
        height: layer_h,
        pixels,
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

fn is_frame(x: u32, y: u32, rx: u32, ry: u32, rx1: u32, ry1: u32) -> bool {
    let left = rx.saturating_add(3);
    let right = rx1.saturating_sub(3);
    let top = ry.saturating_add(3);
    let bottom = ry1.saturating_sub(3);

    x <= left || x >= right || y <= top || y >= bottom
}
