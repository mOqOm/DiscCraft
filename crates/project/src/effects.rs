//! Layer effects on a menu's images and shapes (Encore's Photoshop layer styles, which its Styles panel puts on an
//! image or a shape, M6.332): a stroke around the layer, an outer and an inner glow, a bevel that lights its edges,
//! and a colour laid over it. The drop shadow is [`crate::DropShadow`], kept beside these.

use serde::{Deserialize, Serialize};

/// The widest an effect reaches, in pixels.
pub const MAX_EFFECT_SIZE: u8 = 50;

/// A line around the layer's outside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrokeEffect {
    /// Its width in pixels (1 to [`MAX_EFFECT_SIZE`]).
    pub size: u8,
    /// Its colour.
    pub color: [u8; 3],
    /// Its opacity in per cent.
    pub opacity: u8,
}

/// A soft light around the layer's outside (outer glow) or along its edges inside (inner glow).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlowEffect {
    /// How far it reaches in pixels (1 to [`MAX_EFFECT_SIZE`]).
    pub size: u8,
    /// How much of that is solid before it fades, in per cent.
    pub spread: u8,
    /// Its colour.
    pub color: [u8; 3],
    /// Its opacity in per cent.
    pub opacity: u8,
}

/// Raised edges (an inner bevel): the edges facing the light lighter, those facing away darker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BevelEffect {
    /// How far in from the edge it rises, in pixels (1 to [`MAX_EFFECT_SIZE`]).
    pub size: u8,
    /// Where the light comes from, in degrees counter-clockwise from the right (120: the upper left).
    pub angle: u16,
    /// The light side's opacity in per cent (white).
    pub highlight: u8,
    /// The dark side's opacity in per cent (black).
    pub shadow: u8,
}

/// A colour over the whole layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayEffect {
    /// The colour.
    pub color: [u8; 3],
    /// Its opacity in per cent.
    pub opacity: u8,
}

/// The layer effects of an image or a shape; none by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LayerEffects {
    /// A stroke around it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<StrokeEffect>,
    /// A glow around it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outer_glow: Option<GlowEffect>,
    /// A glow along its edges, inside.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inner_glow: Option<GlowEffect>,
    /// Raised edges.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bevel: Option<BevelEffect>,
    /// A colour over it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay: Option<OverlayEffect>,
}

impl LayerEffects {
    /// None of them.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The effects with every value inside its range (sizes 1 to [`MAX_EFFECT_SIZE`], per cents to 100, angles below
    /// 360); one with nothing to draw (no opacity) is left out.
    #[must_use]
    pub fn sane(self) -> Self {
        let size = |size: u8| size.clamp(1, MAX_EFFECT_SIZE);
        let cent = |value: u8| value.min(100);
        let glow = |glow: GlowEffect| GlowEffect {
            size: size(glow.size),
            spread: cent(glow.spread),
            color: glow.color,
            opacity: cent(glow.opacity),
        };
        Self {
            stroke: self
                .stroke
                .map(|stroke| StrokeEffect {
                    size: size(stroke.size),
                    color: stroke.color,
                    opacity: cent(stroke.opacity),
                })
                .filter(|stroke| stroke.opacity > 0),
            outer_glow: self.outer_glow.map(glow).filter(|glow| glow.opacity > 0),
            inner_glow: self.inner_glow.map(glow).filter(|glow| glow.opacity > 0),
            bevel: self
                .bevel
                .map(|bevel| BevelEffect {
                    size: size(bevel.size),
                    angle: bevel.angle % 360,
                    highlight: cent(bevel.highlight),
                    shadow: cent(bevel.shadow),
                })
                .filter(|bevel| bevel.highlight > 0 || bevel.shadow > 0),
            overlay: self
                .overlay
                .map(|overlay| OverlayEffect {
                    color: overlay.color,
                    opacity: cent(overlay.opacity),
                })
                .filter(|overlay| overlay.opacity > 0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BevelEffect, GlowEffect, LayerEffects, MAX_EFFECT_SIZE, StrokeEffect};

    #[test]
    fn effects_are_kept_in_range_and_written_only_when_there() -> Result<(), serde_json::Error> {
        assert!(LayerEffects::default().is_empty());
        assert_eq!(serde_json::to_string(&LayerEffects::default())?, "{}");
        let wild = LayerEffects {
            stroke: Some(StrokeEffect {
                size: 200,
                color: [255, 255, 255],
                opacity: 250,
            }),
            outer_glow: Some(GlowEffect {
                size: 0,
                spread: 120,
                color: [255, 255, 0],
                opacity: 0,
            }),
            bevel: Some(BevelEffect {
                size: 5,
                angle: 480,
                highlight: 75,
                shadow: 0,
            }),
            ..LayerEffects::default()
        }
        .sane();
        assert_eq!(
            wild.stroke.map(|stroke| (stroke.size, stroke.opacity)),
            Some((MAX_EFFECT_SIZE, 100))
        );
        // No opacity: nothing to draw.
        assert_eq!(wild.outer_glow, None);
        assert_eq!(wild.bevel.map(|bevel| bevel.angle), Some(120));
        let text = serde_json::to_string(&wild)?;
        assert!(
            text.contains("\"stroke\"") && !text.contains("outerGlow"),
            "{text}"
        );
        let back: LayerEffects = serde_json::from_str(&text)?;
        assert_eq!(back, wild);
        Ok(())
    }
}
