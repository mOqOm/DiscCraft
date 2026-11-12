//! Encore's Object ▸ Drop Shadow… dialog: a drop shadow for the selected text, graphic or buttons, with a live
//! preview in the menu viewer while the dialog is open.

use crate::app::{DiscCraftApp, rgb_from_f32, rgb_to_f32};
use crate::i18n::tl;
use disccraft_project::{DropShadow, MAX_SHADOW_DISTANCE, MAX_SHADOW_SIZE, Menu, MenuId};
use egui::{Context, Id, Modal, Ui};
use serde_json::json;

/// What the dialog's shadow belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShadowTarget {
    /// A text object.
    Text(MenuId, usize),
    /// An image or shape.
    Graphic(MenuId, usize),
    /// The selected buttons (their text).
    Buttons(MenuId, Vec<usize>),
}

impl ShadowTarget {
    /// The menu the target is on.
    pub(crate) fn menu(&self) -> MenuId {
        match self {
            ShadowTarget::Text(menu, _)
            | ShadowTarget::Graphic(menu, _)
            | ShadowTarget::Buttons(menu, _) => *menu,
        }
    }
}

/// The open dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ShadowDialog {
    pub(crate) target: ShadowTarget,
    /// Whether the object gets a shadow (the dialog's first check box).
    pub(crate) enabled: bool,
    /// The shadow being edited (kept even while disabled, like Encore's greyed fields).
    pub(crate) draft: DropShadow,
    /// Whether the viewer shows the draft while the dialog is open.
    pub(crate) preview: bool,
}

impl ShadowDialog {
    /// The shadow the dialog stands for: the draft when enabled, otherwise none.
    pub(crate) fn shadow(&self) -> Option<DropShadow> {
        self.enabled.then_some(self.draft)
    }
}

impl DiscCraftApp {
    /// What Object ▸ Drop Shadow… would edit: the selected text, else the selected graphic, else the selected
    /// buttons of the menu being edited.
    pub(crate) fn shadow_dialog_target(&self) -> Option<ShadowTarget> {
        let menu = self.edit_menu_target()?;
        if let Some(index) = self.selected_menu_text(menu) {
            return Some(ShadowTarget::Text(menu, index));
        }
        if let Some(index) = self.selected_menu_graphic(menu) {
            return Some(ShadowTarget::Graphic(menu, index));
        }
        if self.selected_buttons.is_empty() {
            return None;
        }
        let mut buttons = self.selected_buttons.clone();
        buttons.sort_unstable();
        buttons.dedup();
        Some(ShadowTarget::Buttons(menu, buttons))
    }

    /// The shadow the target has now (for several buttons, the first one's).
    fn current_shadow(&self, target: &ShadowTarget) -> Option<DropShadow> {
        let menu = self.session.project.menu(target.menu())?;
        match target {
            ShadowTarget::Text(_, index) => menu.texts.get(*index)?.shadow,
            ShadowTarget::Graphic(_, index) => menu.graphics.get(*index)?.shadow,
            ShadowTarget::Buttons(_, buttons) => menu.buttons.get(*buttons.first()?)?.shadow,
        }
    }

    /// Opens the dialog for the current selection (a status message when nothing is selected).
    pub(crate) fn open_shadow_dialog(&mut self) {
        let Some(target) = self.shadow_dialog_target() else {
            self.status = tl!("Select a text, button, image or shape first").to_string();
            return;
        };
        let current = self.current_shadow(&target);
        self.shadow_dialog = Some(ShadowDialog {
            target,
            enabled: current.is_some(),
            draft: current.unwrap_or_default(),
            preview: true,
        });
    }

    /// `menu` with the dialog's draft laid over it, when the dialog previews a shadow on this menu.
    pub(crate) fn shadow_preview(&self, menu: &Menu) -> Option<Menu> {
        let dialog = self.shadow_dialog.as_ref()?;
        if !dialog.preview || dialog.target.menu() != menu.id {
            return None;
        }
        let shadow = dialog.shadow();
        let mut shown = menu.clone();
        match &dialog.target {
            ShadowTarget::Text(_, index) => {
                shown.texts.get_mut(*index)?.shadow = shadow;
            }
            ShadowTarget::Graphic(_, index) => {
                shown.graphics.get_mut(*index)?.shadow = shadow;
            }
            ShadowTarget::Buttons(_, buttons) => {
                for index in buttons {
                    if let Some(button) = shown.buttons.get_mut(*index) {
                        button.shadow = shadow;
                    }
                }
            }
        }
        Some(shown)
    }

    /// Draws the dialog while it is open.
    pub(crate) fn shadow_window(&mut self, ctx: &Context) {
        let Some(mut state) = self.shadow_dialog.clone() else {
            return;
        };
        let mut done = None;
        let response = Modal::new(Id::unique("shadow_dialog")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading(tl!("Drop Shadow"));
            ui.add_space(4.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.checkbox(&mut state.enabled, tl!("Drop Shadow"));
                    ui.add_enabled_ui(state.enabled, |ui| shadow_fields(ui, &mut state.draft));
                });
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    if ui.button(tl!("OK")).clicked() {
                        done = Some(true);
                    }
                    if ui.button(tl!("Cancel")).clicked() {
                        done = Some(false);
                    }
                    ui.checkbox(&mut state.preview, tl!("Preview"));
                });
            });
        });
        if response.should_close() {
            done = Some(false);
        }
        match done {
            Some(true) => {
                self.shadow_dialog = None;
                self.apply_shadow_dialog(&state);
            }
            Some(false) => self.shadow_dialog = None,
            None => self.shadow_dialog = Some(state),
        }
    }

    /// Writes the dialog's shadow to its target through `menu.setShadow` (one command per button).
    fn apply_shadow_dialog(&mut self, state: &ShadowDialog) {
        let menu = state.target.menu().0;
        let shadow = state.shadow().map_or(serde_json::Value::Null, shadow_json);
        let targets: Vec<(&str, usize)> = match &state.target {
            ShadowTarget::Text(_, index) => vec![("text", *index)],
            ShadowTarget::Graphic(_, index) => vec![("graphic", *index)],
            ShadowTarget::Buttons(_, buttons) => {
                buttons.iter().map(|index| ("button", *index)).collect()
            }
        };
        for (kind, index) in targets {
            let _ = self.run_command(
                "menu.setShadow",
                json!({ "menu": menu, kind: index, "shadow": shadow }),
            );
        }
    }
}

/// The dialog's fields in Encore's order: colour, opacity, angle, distance, size, spread (right-aligned labels,
/// the value, then its unit).
fn shadow_fields(ui: &mut Ui, draft: &mut DropShadow) {
    ui.horizontal(|ui| {
        crate::properties::prop_label(ui, tl!("Shadow color:"));
        let mut rgb = rgb_to_f32(draft.color);
        if ui.color_edit_button_rgb(&mut rgb).changed() {
            draft.color = rgb_from_f32(rgb);
        }
    });
    percent_row(ui, tl!("Opacity:"), &mut draft.opacity, 100, "%");
    ui.horizontal(|ui| {
        crate::properties::prop_label(ui, tl!("Angle:"));
        ui.add(egui::DragValue::new(&mut draft.angle).range(0..=359));
        ui.label(tl!("degrees"));
    });
    percent_row(
        ui,
        tl!("Distance:"),
        &mut draft.distance,
        MAX_SHADOW_DISTANCE,
        tl!("pixels"),
    );
    percent_row(
        ui,
        tl!("Size:"),
        &mut draft.size,
        MAX_SHADOW_SIZE,
        tl!("pixels"),
    );
    percent_row(ui, tl!("Spread:"), &mut draft.spread, 100, "%");
}

/// One row with a small whole number from 0 to `max` and its unit.
fn percent_row(ui: &mut Ui, label: &str, value: &mut u8, max: u8, unit: &str) {
    ui.horizontal(|ui| {
        crate::properties::prop_label(ui, label);
        let mut number = u16::from(*value);
        ui.add(egui::DragValue::new(&mut number).range(0..=u16::from(max)));
        *value = u8::try_from(number).unwrap_or(max);
        ui.label(unit);
    });
}

/// A shadow as the `menu.setShadow` parameter.
fn shadow_json(shadow: DropShadow) -> serde_json::Value {
    json!({
        "color": format!("#{:02X}{:02X}{:02X}", shadow.color[0], shadow.color[1], shadow.color[2]),
        "opacity": shadow.opacity,
        "angle": shadow.angle,
        "distance": shadow.distance,
        "size": shadow.size,
        "spread": shadow.spread,
    })
}

#[cfg(test)]
mod tests {
    use super::shadow_json;
    use disccraft_project::DropShadow;

    #[test]
    fn the_command_parameter_carries_every_field() {
        let shadow = DropShadow {
            color: [1, 2, 255],
            opacity: 30,
            angle: 45,
            distance: 7,
            size: 3,
            spread: 50,
        };
        let value = shadow_json(shadow);
        assert_eq!(value.get("color").and_then(|v| v.as_str()), Some("#0102FF"));
        assert_eq!(value.get("angle").and_then(|v| v.as_u64()), Some(45));
        assert_eq!(value.get("spread").and_then(|v| v.as_u64()), Some(50));
    }
}
