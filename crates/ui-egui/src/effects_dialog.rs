//! The Layer Effects dialog of an image or a shape (M6.332), a text object or a button's text (M6.342): its stroke,
//! outer and inner glow, bevel and colour
//! overlay, each switched on with its box, with a live preview in the menu viewer while the dialog is open. Encore
//! takes layer effects from Photoshop layer styles (and its Styles panel); DiscCraft edits them here too.

use crate::app::{DiscCraftApp, rgb_from_f32, rgb_to_f32};
use crate::i18n::tl;
use disccraft_project::{
    BevelEffect, GlowEffect, LayerEffects, MAX_EFFECT_SIZE, Menu, MenuId, OverlayEffect,
    StrokeEffect,
};
use egui::{Context, Id, Modal, Ui};
use serde_json::json;

/// What the dialog's effects are on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EffectsTarget {
    /// An image or a shape (its index).
    Graphic(usize),
    /// A text object.
    Text(usize),
    /// A button's text.
    Button(usize),
}

impl EffectsTarget {
    /// The `menu.setEffects` parameter that names it, and its index.
    fn param(self) -> (&'static str, usize) {
        match self {
            Self::Graphic(index) => ("graphic", index),
            Self::Text(index) => ("text", index),
            Self::Button(index) => ("button", index),
        }
    }

    /// Its effects in `menu`.
    fn effects(self, menu: &Menu) -> Option<LayerEffects> {
        match self {
            Self::Graphic(index) => menu.graphics.get(index).map(|graphic| graphic.effects),
            Self::Text(index) => menu.texts.get(index).map(|text| text.effects),
            Self::Button(index) => menu.buttons.get(index).map(|button| button.effects),
        }
    }

    /// Its effects in `menu`, to change.
    fn effects_mut(self, menu: &mut Menu) -> Option<&mut LayerEffects> {
        match self {
            Self::Graphic(index) => menu
                .graphics
                .get_mut(index)
                .map(|graphic| &mut graphic.effects),
            Self::Text(index) => menu.texts.get_mut(index).map(|text| &mut text.effects),
            Self::Button(index) => menu
                .buttons
                .get_mut(index)
                .map(|button| &mut button.effects),
        }
    }
}

/// The open dialog: what it is for, which effects are on, and each effect's values (kept while it is off, so turning
/// it on again brings them back).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EffectsDialog {
    menu: MenuId,
    target: EffectsTarget,
    on: [bool; 5],
    stroke: StrokeEffect,
    outer_glow: GlowEffect,
    inner_glow: GlowEffect,
    bevel: BevelEffect,
    overlay: OverlayEffect,
    preview: bool,
}

impl EffectsDialog {
    /// The dialog for `effects` of `target` of `menu`: each effect it has, the others at their defaults.
    fn of(menu: MenuId, target: EffectsTarget, effects: LayerEffects) -> Self {
        Self {
            menu,
            target,
            on: [
                effects.stroke.is_some(),
                effects.outer_glow.is_some(),
                effects.inner_glow.is_some(),
                effects.bevel.is_some(),
                effects.overlay.is_some(),
            ],
            stroke: effects.stroke.unwrap_or(StrokeEffect {
                size: 3,
                color: [255, 255, 255],
                opacity: 100,
            }),
            outer_glow: effects.outer_glow.unwrap_or(GlowEffect {
                size: 10,
                spread: 0,
                color: [255, 255, 190],
                opacity: 75,
            }),
            inner_glow: effects.inner_glow.unwrap_or(GlowEffect {
                size: 8,
                spread: 0,
                color: [255, 255, 255],
                opacity: 75,
            }),
            bevel: effects.bevel.unwrap_or(BevelEffect {
                size: 5,
                angle: 120,
                highlight: 75,
                shadow: 75,
            }),
            overlay: effects.overlay.unwrap_or(OverlayEffect {
                color: [255, 255, 255],
                opacity: 50,
            }),
            preview: true,
        }
    }

    /// The effects the dialog stands for.
    pub(crate) fn effects(&self) -> LayerEffects {
        let [stroke, outer, inner, bevel, overlay] = self.on;
        LayerEffects {
            stroke: stroke.then_some(self.stroke),
            outer_glow: outer.then_some(self.outer_glow),
            inner_glow: inner.then_some(self.inner_glow),
            bevel: bevel.then_some(self.bevel),
            overlay: overlay.then_some(self.overlay),
        }
        .sane()
    }
}

impl DiscCraftApp {
    /// Opens the dialog for the selected image or shape.
    pub(crate) fn open_effects_dialog(&mut self, menu: MenuId, graphic: usize) {
        self.open_effects_for(menu, EffectsTarget::Graphic(graphic));
    }

    /// Opens the dialog for `target` of `menu` (an image or a shape, a text object, a button's text).
    pub(crate) fn open_effects_for(&mut self, menu: MenuId, target: EffectsTarget) {
        let Some(effects) = self
            .session
            .project
            .menu(menu)
            .and_then(|found| target.effects(found))
        else {
            return;
        };
        self.effects_dialog = Some(EffectsDialog::of(menu, target, effects));
    }

    /// The Layer Effects row of a property sheet: the button that opens the dialog for `target`, and what it has.
    pub(crate) fn effects_row(&mut self, ui: &mut Ui, menu: MenuId, target: EffectsTarget) {
        let Some(effects) = self
            .session
            .project
            .menu(menu)
            .and_then(|found| target.effects(found))
        else {
            return;
        };
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Effects:"));
            let named = crate::menu_graphic::effects_summary(&effects);
            if ui
                .button(tl!("Layer Effects…"))
                .on_hover_text(named.clone())
                .clicked()
            {
                self.open_effects_for(menu, target);
            }
            ui.label(egui::RichText::new(named).weak());
        });
    }

    /// `menu` with the dialog's effects on its graphic, while the dialog previews them on this menu.
    pub(crate) fn effects_preview(&self, menu: &Menu) -> Option<Menu> {
        let dialog = self.effects_dialog.as_ref()?;
        if !dialog.preview || dialog.menu != menu.id {
            return None;
        }
        let mut shown = menu.clone();
        *dialog.target.effects_mut(&mut shown)? = dialog.effects();
        Some(shown)
    }

    /// Draws the dialog while it is open.
    pub(crate) fn effects_window(&mut self, ctx: &Context) {
        let Some(mut state) = self.effects_dialog.clone() else {
            return;
        };
        let mut done = None;
        let response = Modal::new(Id::unique("effects_dialog")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading(tl!("Layer Effects"));
            ui.add_space(4.0);
            effect_fields(ui, &mut state);
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(tl!("OK")).clicked() {
                    done = Some(true);
                }
                if ui.button(tl!("Cancel")).clicked() {
                    done = Some(false);
                }
                ui.checkbox(&mut state.preview, tl!("Preview"));
            });
        });
        if response.should_close() {
            done = Some(false);
        }
        match done {
            Some(true) => {
                self.effects_dialog = None;
                let effects = state.effects();
                let value = if effects.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::to_value(effects).unwrap_or(serde_json::Value::Null)
                };
                let (key, index) = state.target.param();
                let _ = self.run_command(
                    "menu.setEffects",
                    json!({ "menu": state.menu.0, key: index, "effects": value }),
                );
            }
            Some(false) => self.effects_dialog = None,
            None => self.effects_dialog = Some(state),
        }
    }
}

/// Each effect: its box, then its values (greyed while it is off).
fn effect_fields(ui: &mut Ui, state: &mut EffectsDialog) {
    let [stroke_on, outer_on, inner_on, bevel_on, overlay_on] = &mut state.on;
    ui.checkbox(stroke_on, tl!("Stroke"));
    ui.add_enabled_ui(*stroke_on, |ui| {
        color_row(ui, tl!("Color:"), &mut state.stroke.color);
        number_row(
            ui,
            tl!("Size:"),
            &mut state.stroke.size,
            (1, MAX_EFFECT_SIZE),
            tl!("pixels"),
        );
        number_row(
            ui,
            tl!("Opacity:"),
            &mut state.stroke.opacity,
            (0, 100),
            "%",
        );
    });
    ui.separator();
    for (on, glow, label) in [
        (outer_on, &mut state.outer_glow, tl!("Outer Glow")),
        (inner_on, &mut state.inner_glow, tl!("Inner Glow")),
    ] {
        ui.checkbox(on, label);
        ui.add_enabled_ui(*on, |ui| {
            color_row(ui, tl!("Color:"), &mut glow.color);
            number_row(
                ui,
                tl!("Size:"),
                &mut glow.size,
                (1, MAX_EFFECT_SIZE),
                tl!("pixels"),
            );
            number_row(ui, tl!("Spread:"), &mut glow.spread, (0, 100), "%");
            number_row(ui, tl!("Opacity:"), &mut glow.opacity, (0, 100), "%");
        });
        ui.separator();
    }
    ui.checkbox(bevel_on, tl!("Bevel"));
    ui.add_enabled_ui(*bevel_on, |ui| {
        number_row(
            ui,
            tl!("Size:"),
            &mut state.bevel.size,
            (1, MAX_EFFECT_SIZE),
            tl!("pixels"),
        );
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Angle:"));
            ui.add(egui::DragValue::new(&mut state.bevel.angle).range(0..=359));
            ui.label(tl!("degrees"));
        });
        number_row(
            ui,
            tl!("Highlight:"),
            &mut state.bevel.highlight,
            (0, 100),
            "%",
        );
        number_row(ui, tl!("Shadow:"), &mut state.bevel.shadow, (0, 100), "%");
    });
    ui.separator();
    ui.checkbox(overlay_on, tl!("Color Overlay"));
    ui.add_enabled_ui(*overlay_on, |ui| {
        color_row(ui, tl!("Color:"), &mut state.overlay.color);
        number_row(
            ui,
            tl!("Opacity:"),
            &mut state.overlay.opacity,
            (0, 100),
            "%",
        );
    });
}

fn color_row(ui: &mut Ui, label: &str, color: &mut [u8; 3]) {
    ui.horizontal(|ui| {
        crate::properties::prop_label(ui, label);
        let mut rgb = rgb_to_f32(*color);
        if ui.color_edit_button_rgb(&mut rgb).changed() {
            *color = rgb_from_f32(rgb);
        }
    });
}

/// One row with a small whole number from `range.0` to `range.1` and its unit.
fn number_row(ui: &mut Ui, label: &str, value: &mut u8, range: (u8, u8), unit: &str) {
    ui.horizontal(|ui| {
        crate::properties::prop_label(ui, label);
        let _ = ui.add(egui::DragValue::new(value).range(range.0..=range.1));
        ui.label(unit);
    });
}

#[cfg(test)]
mod tests {
    use super::EffectsDialog;
    use disccraft_project::{LayerEffects, MenuId, StrokeEffect};

    /// The dialog keeps an effect's values while it is off and gives only the ones that are on.
    #[test]
    fn only_the_effects_that_are_on_are_given() {
        let stroke = StrokeEffect {
            size: 4,
            color: [1, 2, 3],
            opacity: 90,
        };
        let mut dialog = EffectsDialog::of(
            MenuId(1),
            super::EffectsTarget::Graphic(0),
            LayerEffects {
                stroke: Some(stroke),
                ..LayerEffects::default()
            },
        );
        assert_eq!(dialog.effects().stroke, Some(stroke));
        assert_eq!(dialog.effects().bevel, None);
        dialog.on[0] = false;
        dialog.on[3] = true;
        let effects = dialog.effects();
        assert_eq!(effects.stroke, None);
        assert_eq!(effects.bevel.map(|bevel| bevel.angle), Some(120));
        // Turned on again: its own values.
        dialog.on[0] = true;
        assert_eq!(dialog.effects().stroke, Some(stroke));
    }

    /// M6.342: the dialog's effects go to an image, a text object or a button, by the parameter that names it.
    #[test]
    fn the_effects_go_to_their_target() {
        use super::EffectsTarget;
        let mut menu = disccraft_project::Menu::default();
        menu.texts.push(disccraft_project::MenuText {
            effects: LayerEffects::default(),
            box_width: None,
            box_height: None,
            text: "Title".to_owned(),
            x: 0,
            y: 0,
            size: 30,
            color: None,
            font: None,
            font_style: None,
            format: Default::default(),
            shadow: None,
            orientation: Default::default(),
            vertical: false,
            hidden: false,
            locked: false,
        });
        let glow = LayerEffects {
            outer_glow: Some(disccraft_project::GlowEffect {
                size: 6,
                spread: 0,
                color: [255, 255, 0],
                opacity: 80,
            }),
            ..LayerEffects::default()
        };
        if let Some(slot) = EffectsTarget::Text(0).effects_mut(&mut menu) {
            *slot = glow;
        }
        assert_eq!(EffectsTarget::Text(0).effects(&menu), Some(glow));
        assert_eq!(EffectsTarget::Button(0).effects(&menu), None);
        assert_eq!(EffectsTarget::Button(2).param(), ("button", 2));
        assert_eq!(EffectsTarget::Text(1).param(), ("text", 1));
        assert_eq!(EffectsTarget::Graphic(0).param(), ("graphic", 0));
    }
}
