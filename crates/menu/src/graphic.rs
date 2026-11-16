//! Drawing the graphic layers of a menu (images and shapes) into a frame.
//!
//! Every graphic is painted through one pixel sink, so the same code fills the RGB frame of a built menu and the
//! transparent RGBA layer of a PSD export.

use crate::draw::{SourceImage, load_image};
use disccraft_project::{
    GraphicKind, Menu, MenuGraphic, Orientation, Project, Rect, ShapeKind, rotated_bounds,
};
use filmcraft_text::Mask;

/// A pixel the painter wants drawn: frame position, colour and coverage (0 to 1, the graphic's opacity included).
pub(crate) type Sink<'a> = dyn FnMut(usize, usize, [u8; 3], f32) + 'a;

/// Paints every graphic of `menu`, bottom to top, into `sink`. An image that cannot be read is skipped (the
/// project check reports it).
pub(crate) fn paint_graphics(
    project: &Project,
    menu: &Menu,
    width: u16,
    height: u16,
    sink: &mut Sink<'_>,
) {
    for graphic in menu.graphics.iter().filter(|graphic| !graphic.hidden) {
        paint_graphic(project, graphic, width, height, sink);
    }
}

/// Paints one graphic into `sink`, its drop shadow (when it has one) first.
pub(crate) fn paint_graphic(
    project: &Project,
    graphic: &MenuGraphic,
    width: u16,
    height: u16,
    sink: &mut Sink<'_>,
) {
    if let Some(shadow) = &graphic.shadow
        && let Some((mask, x, y)) = graphic_mask(project, graphic, width, height)
        && let Some((shade, sx, sy)) = crate::shadow::shadow_mask(&mask, x, y, shadow)
    {
        crate::shadow::paint_mask(&shade, sx, sy, shadow.color, width, height, sink);
    }
    paint_body(project, graphic, width, height, sink);
}

/// The coverage of a graphic inside the frame (for its shadow), with the frame position of the mask's corner.
fn graphic_mask(
    project: &Project,
    graphic: &MenuGraphic,
    width: u16,
    height: u16,
) -> Option<(Mask, i32, i32)> {
    let (x0, y0, x1, y1) = frame_area(drawn_rect(graphic), width, height)?;
    let w = x1.checked_sub(x0)?;
    let h = y1.checked_sub(y0)?;
    if w == 0 || h == 0 {
        return None;
    }
    let mut mask = Mask::new(w, h);
    {
        let mut sink = |x: usize, y: usize, _color: [u8; 3], coverage: f32| {
            if let Some(slot) = y
                .checked_sub(y0)
                .and_then(|row| row.checked_mul(w))
                .and_then(|n| n.checked_add(x.checked_sub(x0)?))
                .and_then(|n| mask.a.get_mut(n))
            {
                *slot = slot.max(coverage.clamp(0.0, 1.0));
            }
        };
        paint_body(project, graphic, width, height, &mut sink);
    }
    Some((mask, i32::try_from(x0).ok()?, i32::try_from(y0).ok()?))
}

/// Paints the graphic itself (no shadow).
fn paint_body(
    project: &Project,
    graphic: &MenuGraphic,
    width: u16,
    height: u16,
    sink: &mut Sink<'_>,
) {
    let opacity = f32::from(graphic.opacity.clamp(1, 100)) / 100.0;
    let Some(area) = frame_area(drawn_rect(graphic), width, height) else {
        return;
    };
    let place = Placement::new(graphic.rect, graphic.orientation);
    match graphic.kind {
        GraphicKind::Image { asset } => {
            // A turned image is read at its upright size, then drawn on its side.
            let (w, h) = if graphic.orientation.swaps_sides() {
                (graphic.rect.h, graphic.rect.w)
            } else {
                (graphic.rect.w, graphic.rect.h)
            };
            if let Ok(source) = load_image(project, asset, w, h) {
                paint_image(&source, &place, graphic.orientation, area, opacity, sink);
            }
        }
        GraphicKind::Shape { shape, color } => {
            paint_shape(shape, color, &place, area, opacity, sink);
        }
    }
}

/// The frame rectangle a graphic covers: its box, turned by its free angle about its centre.
pub(crate) fn drawn_rect(graphic: &MenuGraphic) -> Rect {
    rotated_bounds(graphic.rect, graphic.orientation.angle)
}

/// Where a frame pixel falls in a graphic's box, which may be turned by a free angle about its centre.
struct Placement {
    rect: Rect,
    /// The free turn's sine and cosine; `None` when it is not turned.
    turn: Option<(f32, f32)>,
}

impl Placement {
    fn new(rect: Rect, orientation: Orientation) -> Self {
        let turn = (disccraft_project::normalized_angle(i32::from(orientation.angle)) != 0)
            .then(|| orientation.angle_radians().sin_cos());
        Self { rect, turn }
    }

    /// The centre of frame pixel (`x`, `y`) in the box's own coordinates (0 to its width and height), and how much
    /// of the pixel the box covers at a turned edge (1 inside); `None` outside.
    fn local(&self, x: usize, y: usize) -> Option<(f32, f32, f32)> {
        let (w, h) = (f32::from(self.rect.w), f32::from(self.rect.h));
        // `as` is exact for frame coordinates.
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let Some((sin, cos)) = self.turn else {
            return Some((
                px - f32::from(self.rect.x),
                py - f32::from(self.rect.y),
                1.0,
            ));
        };
        // Turn the pixel back about the box's centre.
        let (cx, cy) = (
            f32::from(self.rect.x) + w * 0.5,
            f32::from(self.rect.y) + h * 0.5,
        );
        let (qx, qy) = (px - cx, py - cy);
        let (lx, ly) = (
            qx * cos + qy * sin + w * 0.5,
            -qx * sin + qy * cos + h * 0.5,
        );
        let edge = (lx.min(w - lx).min(ly).min(h - ly) + 0.5).clamp(0.0, 1.0);
        (edge > 0.0).then_some((lx, ly, edge))
    }
}

/// The part of `rect` inside the frame, as `(x0, y0, x1, y1)` (exclusive ends).
fn frame_area(rect: Rect, width: u16, height: u16) -> Option<(usize, usize, usize, usize)> {
    let x0 = usize::from(rect.x);
    let y0 = usize::from(rect.y);
    let x1 = usize::from(rect.x.saturating_add(rect.w)).min(usize::from(width));
    let y1 = usize::from(rect.y.saturating_add(rect.h)).min(usize::from(height));
    (x1 > x0 && y1 > y0 && rect.w > 0 && rect.h > 0).then_some((x0, y0, x1, y1))
}

/// Scales `source` into the graphic's box (bilinear, alpha aware) and paints the part inside `area`.
fn paint_image(
    source: &SourceImage,
    place: &Placement,
    orientation: Orientation,
    area: (usize, usize, usize, usize),
    opacity: f32,
    sink: &mut Sink<'_>,
) {
    let (x0, y0, x1, y1) = area;
    let dst_w = usize::from(place.rect.w);
    let dst_h = usize::from(place.rect.h);
    for y in y0..y1 {
        for x in x0..x1 {
            let Some((lx, ly, edge)) = place.local(x, y) else {
                continue;
            };
            // The box's pixel (`as` saturates; clamped into the box).
            let dx = (lx.max(0.0) as usize).min(dst_w.saturating_sub(1));
            let dy = (ly.max(0.0) as usize).min(dst_h.saturating_sub(1));
            let (ux, uy) = orientation.upright_pixel(dx, dy, dst_w, dst_h);
            let (up_w, up_h) = if orientation.swaps_sides() {
                (dst_h, dst_w)
            } else {
                (dst_w, dst_h)
            };
            let (color, alpha) = bilinear_rgba(source, ux, uy, up_w, up_h);
            let coverage = alpha * opacity * edge;
            if coverage > 0.0 {
                sink(x, y, color, coverage);
            }
        }
    }
}

/// Paints `shape` filling the graphic's box with `color`, anti-aliased at its edge.
fn paint_shape(
    shape: ShapeKind,
    color: [u8; 3],
    place: &Placement,
    area: (usize, usize, usize, usize),
    opacity: f32,
    sink: &mut Sink<'_>,
) {
    let (x0, y0, x1, y1) = area;
    let w = f32::from(place.rect.w);
    let h = f32::from(place.rect.h);
    for y in y0..y1 {
        for x in x0..x1 {
            // The pixel centre, in the box.
            let Some((px, py, edge)) = place.local(x, y) else {
                continue;
            };
            let coverage = shape_coverage(shape, px, py, w, h) * opacity * edge;
            if coverage > 0.0 {
                sink(x, y, color, coverage);
            }
        }
    }
}

/// How much of the pixel centred at `(px, py)` (relative to a `w` x `h` rectangle) the shape covers: 1 inside,
/// 0 outside, in between within a pixel of the edge.
fn shape_coverage(shape: ShapeKind, px: f32, py: f32, w: f32, h: f32) -> f32 {
    match shape {
        ShapeKind::Rectangle => 1.0,
        ShapeKind::RoundedRectangle => {
            let radius = (w.min(h) / 8.0).max(0.5);
            // Distance outside the rounded box (negative inside), via the corner circle.
            let cx = (px - w / 2.0).abs() - (w / 2.0 - radius);
            let cy = (py - h / 2.0).abs() - (h / 2.0 - radius);
            let outside = (cx.max(0.0).hypot(cy.max(0.0))) + cx.max(cy).min(0.0) - radius;
            (0.5 - outside).clamp(0.0, 1.0)
        }
        ShapeKind::Ellipse => {
            let a = (w / 2.0).max(0.5);
            let b = (h / 2.0).max(0.5);
            let nx = (px - w / 2.0) / a;
            let ny = (py - h / 2.0) / b;
            let d = (nx * nx + ny * ny).sqrt();
            // Distance to the edge in pixels, approximated along the shorter radius.
            let edge = (1.0 - d) * a.min(b);
            (edge + 0.5).clamp(0.0, 1.0)
        }
    }
}

/// Bilinear sample of `src` scaled to `dst_w` x `dst_h`, at destination pixel `(dx, dy)`: straight colour and
/// alpha (0 to 1).
fn bilinear_rgba(
    src: &SourceImage,
    dx: usize,
    dy: usize,
    dst_w: usize,
    dst_h: usize,
) -> ([u8; 3], f32) {
    let sx = dest_to_source(dx, dst_w, src.width);
    let sy = dest_to_source(dy, dst_h, src.height);
    let x0 = sx.floor().max(0.0) as usize;
    let y0 = sy.floor().max(0.0) as usize;
    let x1 = x0.saturating_add(1).min(src.width.saturating_sub(1));
    let y1 = y0.saturating_add(1).min(src.height.saturating_sub(1));
    let tx = (sx - sx.floor()).clamp(0.0, 1.0);
    let ty = (sy - sy.floor()).clamp(0.0, 1.0);

    let samples = [
        (sample_rgba(src, x0, y0), (1.0 - tx) * (1.0 - ty)),
        (sample_rgba(src, x1, y0), tx * (1.0 - ty)),
        (sample_rgba(src, x0, y1), (1.0 - tx) * ty),
        (sample_rgba(src, x1, y1), tx * ty),
    ];
    // Average premultiplied colour and alpha, then un-premultiply.
    let mut premul = [0.0f32; 3];
    let mut alpha = 0.0f32;
    for (([r, g, b], a), weight) in samples {
        premul[0] += r * a * weight;
        premul[1] += g * a * weight;
        premul[2] += b * a * weight;
        alpha += a * weight;
    }
    if alpha <= 0.0 {
        return ([0, 0, 0], 0.0);
    }
    let color = [
        to_u8(premul[0] / alpha),
        to_u8(premul[1] / alpha),
        to_u8(premul[2] / alpha),
    ];
    (color, alpha.clamp(0.0, 1.0))
}

/// One source pixel as straight colour (0 to 255 floats) and alpha (0 to 1).
fn sample_rgba(src: &SourceImage, x: usize, y: usize) -> ([f32; 3], f32) {
    let Some(idx) = y
        .checked_mul(src.width)
        .and_then(|row| row.checked_add(x))
        .and_then(|pixel| pixel.checked_mul(src.channels))
    else {
        return ([0.0; 3], 0.0);
    };
    let Some(data) = idx
        .checked_add(src.channels)
        .and_then(|end| src.data.get(idx..end))
    else {
        return ([0.0; 3], 0.0);
    };
    let channel = |i: usize| data.get(i).copied().map_or(0.0, f32::from);
    let alpha = if src.channels >= 4 {
        channel(3) / 255.0
    } else {
        1.0
    };
    ([channel(0), channel(1), channel(2)], alpha)
}

/// The source coordinate (pixel centres aligned) of destination pixel `dest`.
fn dest_to_source(dest: usize, dst_len: usize, src_len: usize) -> f32 {
    if dst_len == 0 || src_len == 0 {
        return 0.0;
    }
    let scale = (src_len as f32) / (dst_len as f32);
    (((dest as f32) + 0.5) * scale - 0.5).clamp(0.0, (src_len.saturating_sub(1)) as f32)
}

fn to_u8(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::{ShapeKind, shape_coverage};

    #[test]
    fn shapes_cover_their_middle_and_not_their_outside() {
        for shape in ShapeKind::ALL {
            assert!((shape_coverage(shape, 50.0, 25.0, 100.0, 50.0) - 1.0).abs() < 1e-6);
        }
        // The corners of the rectangle are inside; the ellipse's and the rounded box's corners are outside.
        assert!((shape_coverage(ShapeKind::Rectangle, 0.5, 0.5, 100.0, 50.0) - 1.0).abs() < 1e-6);
        assert!(shape_coverage(ShapeKind::Ellipse, 0.5, 0.5, 100.0, 50.0) < 0.01);
        assert!(shape_coverage(ShapeKind::RoundedRectangle, 0.5, 0.5, 100.0, 50.0) < 0.01);
        // Just inside the ellipse's rightmost point is covered.
        assert!(shape_coverage(ShapeKind::Ellipse, 97.0, 25.0, 100.0, 50.0) > 0.9);
    }
}
