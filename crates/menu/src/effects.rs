//! Drawing the layer effects of an image or a shape (M6.332, Encore's layer styles): each one a coverage mask made
//! from the layer's own mask (a [`Mask`] of the frame with its corner at `x`, `y`), painted in its colour. The outer
//! glow goes under the layer; the colour overlay, the inner glow, the bevel and the stroke over it.

use disccraft_project::{BevelEffect, DropShadow, GlowEffect, LayerEffects, StrokeEffect};
use filmcraft_text::Mask;

/// Most pixels an effect's mask may hold.
const MAX_EFFECT_PIXELS: usize = 4096 * 4096;

/// How steep the bevel's light falls: the light's height above the surface, in degrees.
const LIGHT_ALTITUDE: f32 = 30.0;

/// One effect to paint: its mask, where its corner lands and its colour.
pub(crate) type Painted = (Mask, i32, i32, [u8; 3]);

/// `mask` with `pad` empty pixels around it, and its corner moved out by as much.
fn padded(mask: &Mask, x: i32, y: i32, pad: usize) -> Option<(Mask, i32, i32)> {
    let w = mask.w.checked_add(pad.checked_mul(2)?)?;
    let h = mask.h.checked_add(pad.checked_mul(2)?)?;
    if w.checked_mul(h)? > MAX_EFFECT_PIXELS {
        return None;
    }
    let mut out = Mask::new(w, h);
    for row in 0..mask.h {
        let from = mask.a.get(row * mask.w..(row + 1) * mask.w)?;
        let start = (row + pad) * w + pad;
        out.a.get_mut(start..start + mask.w)?.copy_from_slice(from);
    }
    let pad = i32::try_from(pad).ok()?;
    Some((out, x.checked_sub(pad)?, y.checked_sub(pad)?))
}

/// What goes under the layer: its outer glow.
pub(crate) fn under(mask: &Mask, x: i32, y: i32, effects: &LayerEffects) -> Vec<Painted> {
    effects
        .outer_glow
        .and_then(|glow| outer_glow(mask, x, y, &glow))
        .into_iter()
        .collect()
}

/// What goes over the layer, in order: its colour overlay, inner glow, bevel (light, then dark) and stroke.
pub(crate) fn over(mask: &Mask, x: i32, y: i32, effects: &LayerEffects) -> Vec<Painted> {
    let mut painted = Vec::new();
    if let Some(overlay) = effects.overlay {
        let opacity = f32::from(overlay.opacity.min(100)) / 100.0;
        let mut tint = mask.clone();
        for value in &mut tint.a {
            *value = (*value * opacity).clamp(0.0, 1.0);
        }
        painted.push((tint, x, y, overlay.color));
    }
    if let Some(glow) = effects.inner_glow
        && let Some(found) = inner_glow(mask, x, y, &glow)
    {
        painted.push(found);
    }
    if let Some(bevel) = effects.bevel {
        painted.extend(bevel_masks(mask, x, y, &bevel));
    }
    if let Some(stroke) = effects.stroke
        && let Some(found) = stroke_mask(mask, x, y, &stroke)
    {
        painted.push(found);
    }
    painted
}

/// A glow around the layer: its mask spread and blurred like a drop shadow that does not move.
fn outer_glow(mask: &Mask, x: i32, y: i32, glow: &GlowEffect) -> Option<Painted> {
    let shadow = DropShadow {
        color: glow.color,
        opacity: glow.opacity,
        angle: 0,
        distance: 0,
        size: glow.size,
        spread: glow.spread,
    };
    let (glowing, gx, gy) = crate::shadow::shadow_mask(mask, x, y, &shadow)?;
    Some((glowing, gx, gy, glow.color))
}

/// A line `size` pixels wide around the outside: the mask grown by that much, less the mask itself.
fn stroke_mask(mask: &Mask, x: i32, y: i32, stroke: &StrokeEffect) -> Option<Painted> {
    let size = usize::from(stroke.size);
    let (inside, sx, sy) = padded(mask, x, y, size + 1)?;
    let mut grown = inside.clone();
    crate::shadow::dilate(&mut grown, size);
    let opacity = f32::from(stroke.opacity.min(100)) / 100.0;
    for (value, own) in grown.a.iter_mut().zip(&inside.a) {
        *value = ((*value - own).max(0.0) * opacity).clamp(0.0, 1.0);
    }
    Some((grown, sx, sy, stroke.color))
}

/// A glow along the edges inside: how near each covered pixel is to the outside (the outside spread and blurred
/// inwards), on the mask.
fn inner_glow(mask: &Mask, x: i32, y: i32, glow: &GlowEffect) -> Option<Painted> {
    let size = usize::from(glow.size);
    let (inside, gx, gy) = padded(mask, x, y, size + 1)?;
    let mut outside = inside.clone();
    for value in &mut outside.a {
        *value = 1.0 - value.clamp(0.0, 1.0);
    }
    let solid = size * usize::from(glow.spread.min(100)) / 100;
    if solid > 0 {
        crate::shadow::dilate(&mut outside, solid);
    }
    let blur = size.saturating_sub(solid).div_ceil(2);
    if blur > 0 {
        crate::shadow::box_blur(&mut outside, blur);
        crate::shadow::box_blur(&mut outside, blur);
    }
    let opacity = f32::from(glow.opacity.min(100)) / 100.0;
    for (value, own) in outside.a.iter_mut().zip(&inside.a) {
        // Half way at the very edge after the blur: twice that is the full glow there.
        *value = ((*value * 2.0).min(1.0) * own * opacity).clamp(0.0, 1.0);
    }
    Some((outside, gx, gy, glow.color))
}

/// An inner bevel: the mask blurred over `size` is a height that rises from the edge; each covered pixel is lit by
/// how its slope faces the light (white where it faces it, black where it faces away).
fn bevel_masks(mask: &Mask, x: i32, y: i32, bevel: &BevelEffect) -> Vec<Painted> {
    let size = usize::from(bevel.size);
    let Some((inside, bx, by)) = padded(mask, x, y, size + 1) else {
        return Vec::new();
    };
    let mut height = inside.clone();
    let radius = size.div_ceil(2);
    crate::shadow::box_blur(&mut height, radius);
    crate::shadow::box_blur(&mut height, radius);
    let (w, h) = (height.w, height.h);
    let at = |col: usize, row: usize| height.a.get(row * w + col).copied().unwrap_or(0.0);
    // The light: from `angle` (counter-clockwise from the right, y down on screen), LIGHT_ALTITUDE up.
    let (sin, cos) = f32::from(bevel.angle).to_radians().sin_cos();
    let (alt_sin, alt_cos) = LIGHT_ALTITUDE.to_radians().sin_cos();
    let light = (cos * alt_cos, -sin * alt_cos, alt_sin);
    // A slope of one height unit over the bevel's size is 45 degrees.
    let steepness = f32::from(bevel.size);
    let mut light_side = Mask::new(w, h);
    let mut dark_side = Mask::new(w, h);
    let (high, low) = (
        f32::from(bevel.highlight.min(100)) / 100.0,
        f32::from(bevel.shadow.min(100)) / 100.0,
    );
    for row in 1..h.saturating_sub(1) {
        for col in 1..w.saturating_sub(1) {
            let own = inside.a.get(row * w + col).copied().unwrap_or(0.0);
            if own <= 0.0 {
                continue;
            }
            let dx = (at(col + 1, row) - at(col - 1, row)) * 0.5 * steepness;
            let dy = (at(col, row + 1) - at(col, row - 1)) * 0.5 * steepness;
            // The surface's normal is (-dx, -dy, 1); how much more or less light it gets than flat ground.
            let length = (dx * dx + dy * dy + 1.0).sqrt();
            let lit = (-dx * light.0 - dy * light.1 + light.2) / length - light.2;
            let index = row * w + col;
            if lit > 0.0 {
                if let Some(slot) = light_side.a.get_mut(index) {
                    *slot = (lit * 2.0 * high * own).clamp(0.0, 1.0);
                }
            } else if let Some(slot) = dark_side.a.get_mut(index) {
                *slot = (-lit * 2.0 * low * own).clamp(0.0, 1.0);
            }
        }
    }
    vec![
        (light_side, bx, by, [255, 255, 255]),
        (dark_side, bx, by, [0, 0, 0]),
    ]
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::{over, under};
    use disccraft_project::{BevelEffect, GlowEffect, LayerEffects, StrokeEffect};
    use filmcraft_text::Mask;

    /// A 20 x 20 square, fully covered.
    fn square() -> Mask {
        let mut mask = Mask::new(20, 20);
        mask.a.fill(1.0);
        mask
    }

    /// The value of a painted mask at frame position (`fx`, `fy`).
    fn value(painted: &(Mask, i32, i32, [u8; 3]), fx: i32, fy: i32) -> f32 {
        let (mask, x, y, _) = painted;
        let (Ok(col), Ok(row)) = (usize::try_from(fx - x), usize::try_from(fy - y)) else {
            return 0.0;
        };
        if col >= mask.w || row >= mask.h {
            return 0.0;
        }
        mask.a.get(row * mask.w + col).copied().unwrap_or(0.0)
    }

    #[test]
    fn a_stroke_is_outside_the_layer_only() {
        let effects = LayerEffects {
            stroke: Some(StrokeEffect {
                size: 3,
                color: [255, 255, 255],
                opacity: 100,
            }),
            ..LayerEffects::default()
        };
        let painted = over(&square(), 100, 100, &effects);
        let stroke = painted.first().expect("a stroke");
        assert_eq!(stroke.3, [255, 255, 255]);
        // Inside the square: none; 1 to 3 pixels out: all; 4 out: none.
        assert_eq!(value(stroke, 110, 110), 0.0);
        assert_eq!(value(stroke, 99, 110), 1.0);
        assert_eq!(value(stroke, 97, 110), 1.0);
        assert_eq!(value(stroke, 96, 110), 0.0);
        assert_eq!(value(stroke, 122, 122), 1.0);
    }

    #[test]
    fn a_glow_fades_outward_and_an_inner_one_lights_the_edges() {
        let effects = LayerEffects {
            outer_glow: Some(GlowEffect {
                size: 8,
                spread: 0,
                color: [255, 255, 0],
                opacity: 100,
            }),
            inner_glow: Some(GlowEffect {
                size: 6,
                spread: 0,
                color: [255, 255, 255],
                opacity: 100,
            }),
            ..LayerEffects::default()
        };
        let under = under(&square(), 100, 100, &effects);
        let glow = under.first().expect("a glow");
        // Strongest next to the square, fading out, gone past its size.
        assert!(value(glow, 99, 110) > value(glow, 96, 110));
        assert!(value(glow, 96, 110) > 0.0);
        assert_eq!(value(glow, 90, 110), 0.0);
        let over = over(&square(), 100, 100, &effects);
        let inner = over.first().expect("an inner glow");
        // Strong at the edge, nothing in the middle, nothing outside.
        assert!(value(inner, 100, 110) > 0.5);
        assert!(value(inner, 110, 110) < 0.05);
        assert_eq!(value(inner, 99, 110), 0.0);
    }

    #[test]
    fn a_bevel_lights_the_edges_facing_the_light() {
        // Light from the upper left (135 degrees).
        let effects = LayerEffects {
            bevel: Some(BevelEffect {
                size: 6,
                angle: 135,
                highlight: 100,
                shadow: 100,
            }),
            ..LayerEffects::default()
        };
        let painted = over(&square(), 100, 100, &effects);
        let (light, dark) = (
            painted.first().expect("light"),
            painted.get(1).expect("dark"),
        );
        assert_eq!((light.3, dark.3), ([255, 255, 255], [0, 0, 0]));
        // The upper left edges are lit, the lower right ones dark; the flat middle neither.
        assert!(value(light, 101, 101) > 0.2);
        assert!(value(dark, 118, 118) > 0.2);
        assert!(value(light, 118, 118) < 0.01 && value(dark, 101, 101) < 0.01);
        assert!(value(light, 110, 110) < 0.05 && value(dark, 110, 110) < 0.05);
    }
}
