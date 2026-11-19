//! The Library's layer sets and replacement layers (Encore's Library kinds "Layer Set" and "Replacement Layer",
//! M6.331): DiscCraft's own, made of plain shapes in code. A layer set is a few layers placed together (a band, a
//! frame); a replacement layer is a placeholder a still dropped on it replaces (see [`MenuGraphic::replacement`]).
//! No shape, name or layout is taken from another product's library.

use crate::graphic::{FULL_OPACITY, GraphicKind, MenuGraphic, ShapeKind};
use crate::menu::{Orientation, Rect};
use crate::model::DiscSettings;

/// One layer of a [`LayerPreset`], laid out in a 720x480 frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresetLayer {
    /// Its shape.
    pub shape: ShapeKind,
    /// Its fill colour.
    pub color: [u8; 3],
    /// Its opacity in per cent.
    pub opacity: u8,
    /// Left edge.
    pub x: u16,
    /// Top edge.
    pub y: u16,
    /// Width.
    pub w: u16,
    /// Height.
    pub h: u16,
}

/// A built-in layer set or replacement layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerPreset {
    /// Stable id, e.g. "layers.band-top".
    pub id: &'static str,
    /// English display name (the GUI translates it).
    pub name: &'static str,
    /// A replacement layer (its one layer is a `(!)` placeholder), else a layer set.
    pub replacement: bool,
    /// Its layers, bottom first.
    pub layers: &'static [PresetLayer],
}

const fn layer(shape: ShapeKind, color: [u8; 3], opacity: u8, rect: [u16; 4]) -> PresetLayer {
    PresetLayer {
        shape,
        color,
        opacity,
        x: rect[0],
        y: rect[1],
        w: rect[2],
        h: rect[3],
    }
}

const PLACEHOLDER: [u8; 3] = [128, 128, 128];
const LINE: [u8; 3] = [230, 230, 230];
const BAND: [u8; 3] = [0, 0, 0];

/// The built-in layer sets, then the replacement layers.
pub const LAYER_PRESETS: &[LayerPreset] = &[
    LayerPreset {
        id: "layers.band-top",
        name: "Title Band",
        replacement: false,
        layers: &[
            layer(ShapeKind::Rectangle, BAND, 60, [0, 40, 720, 70]),
            layer(ShapeKind::Rectangle, LINE, 100, [0, 110, 720, 3]),
        ],
    },
    LayerPreset {
        id: "layers.band-bottom",
        name: "Bottom Band",
        replacement: false,
        layers: &[
            layer(ShapeKind::Rectangle, BAND, 60, [0, 360, 720, 80]),
            layer(ShapeKind::Rectangle, LINE, 100, [0, 357, 720, 3]),
        ],
    },
    LayerPreset {
        id: "layers.frame",
        name: "Thin Frame",
        replacement: false,
        layers: &[
            layer(ShapeKind::Rectangle, LINE, 100, [60, 40, 600, 3]),
            layer(ShapeKind::Rectangle, LINE, 100, [60, 437, 600, 3]),
            layer(ShapeKind::Rectangle, LINE, 100, [60, 40, 3, 400]),
            layer(ShapeKind::Rectangle, LINE, 100, [657, 40, 3, 400]),
        ],
    },
    LayerPreset {
        id: "layers.panel",
        name: "Side Panel",
        replacement: false,
        layers: &[
            layer(ShapeKind::RoundedRectangle, BAND, 50, [400, 60, 260, 360]),
            layer(ShapeKind::Rectangle, LINE, 100, [420, 110, 220, 2]),
        ],
    },
    LayerPreset {
        id: "replace.photo-4x3",
        name: "Photo 4:3",
        replacement: true,
        layers: &[layer(
            ShapeKind::Rectangle,
            PLACEHOLDER,
            100,
            [240, 150, 240, 180],
        )],
    },
    LayerPreset {
        id: "replace.photo-16x9",
        name: "Photo 16:9",
        replacement: true,
        layers: &[layer(
            ShapeKind::Rectangle,
            PLACEHOLDER,
            100,
            [200, 165, 320, 150],
        )],
    },
    LayerPreset {
        id: "replace.photo-round",
        name: "Round Photo",
        replacement: true,
        layers: &[layer(
            ShapeKind::Ellipse,
            PLACEHOLDER,
            100,
            [270, 150, 180, 180],
        )],
    },
];

/// The built-in layer set or replacement layer `id`.
pub fn layer_preset(id: &str) -> Option<&'static LayerPreset> {
    LAYER_PRESETS.iter().find(|preset| preset.id == id)
}

impl LayerPreset {
    /// Its layers as menu graphics in the frame of a project with `settings` (laid out as the button presets are:
    /// an SD frame's lines scaled to its height, an HD frame's 4:3 design in its middle).
    pub fn graphics(&self, settings: &DiscSettings) -> Vec<MenuGraphic> {
        self.layers
            .iter()
            .map(|preset| MenuGraphic {
                rect: crate::library::design_rect_of(
                    Rect {
                        x: preset.x,
                        y: preset.y,
                        w: preset.w,
                        h: preset.h,
                    },
                    settings,
                ),
                kind: GraphicKind::Shape {
                    shape: preset.shape,
                    color: preset.color,
                },
                opacity: preset.opacity.clamp(1, FULL_OPACITY),
                shadow: None,
                orientation: Orientation::default(),
                hidden: false,
                locked: false,
                replacement: self.replacement,
                effects: crate::effects::LayerEffects::default(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{LAYER_PRESETS, layer_preset};
    use crate::model::DiscSettings;

    #[test]
    fn presets_make_their_layers_in_the_frame() {
        let settings = DiscSettings::default();
        let mut ids: Vec<&str> = LAYER_PRESETS.iter().map(|preset| preset.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), LAYER_PRESETS.len());
        for preset in LAYER_PRESETS {
            let graphics = preset.graphics(&settings);
            assert_eq!(graphics.len(), preset.layers.len(), "{}", preset.id);
            assert!(
                graphics
                    .iter()
                    .all(|graphic| graphic.replacement == preset.replacement)
            );
            assert!(graphics.iter().all(|graphic| {
                u32::from(graphic.rect.x) + u32::from(graphic.rect.w) <= 720
                    && u32::from(graphic.rect.y) + u32::from(graphic.rect.h) <= 480
            }));
        }
        // A replacement layer is one placeholder.
        let photo = layer_preset("replace.photo-4x3").map(|preset| preset.graphics(&settings));
        assert_eq!(photo.as_ref().map(Vec::len), Some(1));
        assert!(layer_preset("missing").is_none());
    }
}
