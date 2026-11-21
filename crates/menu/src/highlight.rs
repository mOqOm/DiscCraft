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

/// The highlight of `menu`'s buttons; `artwork`: a menu made from a layered document (a PSD), whose buttons show
/// only their own subpicture layers, as Encore's do: one without them (a picture laid out as a button) has no
/// highlight, not the frame DiscCraft's own buttons get (GitHub issue #13).
pub(crate) fn build(menu: &Menu, artwork: bool) -> Result<HighlightLayer, MenuError> {
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
        // Over the content box, or its own area of it (a pasted layer that reaches past the button, M6.329); turned
        // with the content about the box's centre.
        if let Some((values, sub_w, sub_h, area)) = button.subpicture.as_ref().and_then(|sub| {
            sub.values().map(|values| {
                (
                    values,
                    usize::from(sub.width),
                    usize::from(sub.height),
                    sub.rect_in(button.content_box()),
                )
            })
        }) {
            paint_subpicture(
                &mut pixels,
                (x, y, layer_w, layer_h),
                (area, button.orientation.angle, button.content_centre()),
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
        if !painted && !artwork {
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
    (rect, angle, pivot): (Rect, i16, (f32, f32)),
    (values, sub_w, sub_h): (&[u8], usize, usize),
) {
    let (rect_w, rect_h) = (usize::from(rect.w), usize::from(rect.h));
    if rect_w == 0 || rect_h == 0 {
        return;
    }
    if disccraft_project::normalized_angle(i32::from(angle)) != 0 {
        paint_turned_subpicture(
            pixels,
            (layer_x, layer_y, layer_w, layer_h),
            (rect, angle, pivot),
            (values, sub_w, sub_h),
        );
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

/// [`paint_subpicture`] for a button whose content is turned by a free angle about the content box's centre: each
/// pixel of the turned box's bounds takes the subpicture value it shows (nearest, so the four colours stay).
fn paint_turned_subpicture(
    pixels: &mut [u8],
    (layer_x, layer_y, layer_w, layer_h): (u16, u16, u16, u16),
    (rect, angle, (cx, cy)): (Rect, i16, (f32, f32)),
    (values, sub_w, sub_h): (&[u8], usize, usize),
) {
    let bounds = disccraft_project::rotated_bounds_about(rect, (cx, cy), angle);
    let (sin, cos) = (f32::from(angle) / 10.0).to_radians().sin_cos();
    let (w, h) = (f32::from(rect.w), f32::from(rect.h));
    for y in bounds.y..bounds.y.saturating_add(bounds.h) {
        for x in bounds.x..bounds.x.saturating_add(bounds.w) {
            // The pixel centre turned back about the pivot, into the box.
            let (px, py) = (f32::from(x) + 0.5 - cx, f32::from(y) + 0.5 - cy);
            let (lx, ly) = (
                px * cos + py * sin + cx - f32::from(rect.x),
                -px * sin + py * cos + cy - f32::from(rect.y),
            );
            if lx < 0.0 || ly < 0.0 || lx >= w || ly >= h {
                continue;
            }
            // Inside the box; `as` truncates the non-negative position.
            let source_col = (lx / w * sub_w as f32) as usize;
            let source_row = (ly / h * sub_h as f32) as usize;
            let value = source_row
                .checked_mul(sub_w)
                .and_then(|n| n.checked_add(source_col))
                .and_then(|n| values.get(n))
                .copied()
                .unwrap_or(0);
            if value == 0 {
                continue;
            }
            let (Some(lx), Some(ly)) = (x.checked_sub(layer_x), y.checked_sub(layer_y)) else {
                continue;
            };
            if lx >= layer_w || ly >= layer_h {
                continue;
            }
            if let Some(pixel) =
                pixels.get_mut(usize::from(ly) * usize::from(layer_w) + usize::from(lx))
            {
                *pixel = value;
            }
        }
    }
}

/// A PSD button's subpicture layers in their own colours as a rich-colour Blu-ray highlight shows them (M6.335):
/// the normal, selected and activated pictures ([`disccraft_project::Subpicture::color_states`]) laid over the
/// button's rectangle as its subpicture is (stretched over its area, turned with the content), each straight-alpha
/// RGBA, `rect.w * rect.h * 4` bytes; `None` for a button without colour layers.
pub(crate) fn color_states(button: &disccraft_project::Button) -> Option<[Option<Vec<u8>>; 3]> {
    let sub = button.subpicture.as_ref()?;
    let states = sub.color_states()?;
    let over = layer_over_button(button)?;
    Some(states.map(|state| over(state?)))
}

/// A PSD button's `(=1)`, `(=2)` and `(=3)` layers in their own colours over the button's rectangle, laid as
/// [`color_states`] lays them; `None` for a button without colour layers.
pub(crate) fn color_layers(button: &disccraft_project::Button) -> Option<[Option<Vec<u8>>; 3]> {
    let sub = button.subpicture.as_ref()?;
    sub.color_states()?;
    let over = layer_over_button(button)?;
    Some(
        sub.color_layers
            .each_ref()
            .map(|layer| over(layer.as_ref()?)),
    )
}

/// Lays one of the button's colour layers over its rectangle (RGBA, `rect.w * rect.h * 4` bytes).
fn layer_over_button(
    button: &disccraft_project::Button,
) -> Option<impl Fn(&disccraft_project::ColorLayer) -> Option<Vec<u8>>> {
    let sub = button.subpicture.as_ref()?;
    let area = sub.rect_in(button.content_box());
    let sources = subpicture_sources(
        button.rect,
        (area, button.orientation.angle, button.content_centre()),
        (usize::from(sub.width), usize::from(sub.height)),
    )?;
    let (width, height) = (sub.width, sub.height);
    Some(move |layer: &disccraft_project::ColorLayer| {
        let rgba = layer.rgba(width, height)?;
        let colours = rgba.as_chunks::<4>().0;
        let mut out = vec![0_u8; sources.len().checked_mul(4)?];
        for (pixel, source) in out.as_chunks_mut::<4>().0.iter_mut().zip(&sources) {
            if let Some(colour) = source.and_then(|source| colours.get(source)) {
                *pixel = *colour;
            }
        }
        Some(out)
    })
}

/// For each pixel of `rect` (row by row), the subpicture pixel (`sub_w` x `sub_h`, row by row) drawn there when
/// the subpicture is stretched over `area` and turned by `angle` about `pivot`, as [`paint_subpicture`] draws it.
fn subpicture_sources(
    rect: Rect,
    (area, angle, (cx, cy)): (Rect, i16, (f32, f32)),
    (sub_w, sub_h): (usize, usize),
) -> Option<Vec<Option<usize>>> {
    let (rect_w, rect_h) = (usize::from(rect.w), usize::from(rect.h));
    let mut sources = vec![None; rect_w.checked_mul(rect_h)?];
    if area.w == 0 || area.h == 0 || sub_w == 0 || sub_h == 0 {
        return Some(sources);
    }
    let turned = disccraft_project::normalized_angle(i32::from(angle)) != 0;
    let (sin, cos) = (f32::from(angle) / 10.0).to_radians().sin_cos();
    let (w, h) = (f32::from(area.w), f32::from(area.h));
    for row in 0..rect_h {
        for col in 0..rect_w {
            let x = i64::from(rect.x) + i64::try_from(col).ok()?;
            let y = i64::from(rect.y) + i64::try_from(row).ok()?;
            let source = if turned {
                // The pixel centre turned back about the pivot, into the area.
                let (px, py) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                let (lx, ly) = (
                    px * cos + py * sin + cx - f32::from(area.x),
                    -px * sin + py * cos + cy - f32::from(area.y),
                );
                if lx < 0.0 || ly < 0.0 || lx >= w || ly >= h {
                    continue;
                }
                // Inside the area; `as` truncates the non-negative position.
                let (source_col, source_row) = (
                    (lx / w * sub_w as f32) as usize,
                    (ly / h * sub_h as f32) as usize,
                );
                source_row
                    .checked_mul(sub_w)?
                    .checked_add(source_col.min(sub_w - 1))
            } else {
                let (Ok(ax), Ok(ay)) = (
                    usize::try_from(x - i64::from(area.x)),
                    usize::try_from(y - i64::from(area.y)),
                ) else {
                    continue;
                };
                if ax >= usize::from(area.w) || ay >= usize::from(area.h) {
                    continue;
                }
                let source_row = ay.saturating_mul(sub_h) / usize::from(area.h);
                let source_col = ax.saturating_mul(sub_w) / usize::from(area.w);
                source_row.checked_mul(sub_w)?.checked_add(source_col)
            };
            if let Some(slot) = row
                .checked_mul(rect_w)
                .and_then(|n| n.checked_add(col))
                .and_then(|n| sources.get_mut(n))
            {
                *slot = source;
            }
        }
    }
    Some(sources)
}

/// The highlight of button `index` alone over its rectangle (`width` x `height` values, row by row), as the disc
/// draws it (`artwork`: as [`build`]); `None` without such a button.
pub(crate) fn button_values(menu: &Menu, index: usize, artwork: bool) -> Option<Vec<u8>> {
    let button = menu.buttons.get(index)?;
    let alone = Menu {
        buttons: vec![button.clone()],
        ..menu.clone()
    };
    let layer = build(&alone, artwork).ok()?;
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
