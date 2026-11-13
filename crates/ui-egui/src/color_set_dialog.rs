//! Encore's Menu ▸ Edit Menu Color Set… dialog: the menu's colour set (automatic, or the menu's own), with the
//! normal group and each highlight group's selected / activated colour and opacity, and a live preview.

use crate::app::{DiscCraftApp, rgb_from_f32, rgb_to_f32};
use crate::i18n::{tl, tlf};
use disccraft_project::{ColorSet, DEFAULT_NORMAL_MORE, Menu, MenuId};
use egui::{Context, Id, Modal, Ui};
use serde_json::json;

/// The open dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ColorSetDialog {
    pub(crate) menu: MenuId,
    /// The menu's own set (Encore's メニューの初期設定); off is the automatic set.
    pub(crate) own: bool,
    /// The set being edited (the automatic one while `own` is off).
    pub(crate) draft: ColorSet,
    /// Whether the viewer shows the draft.
    pub(crate) preview: bool,
}

impl DiscCraftApp {
    /// Opens the dialog for the menu being edited (a status message when no menu is open).
    pub(crate) fn open_color_set_dialog(&mut self) {
        let Some(menu) = self
            .viewer_menu()
            .and_then(|id| self.session.project.menu(id))
        else {
            self.status = tl!("Open a menu first").to_string();
            return;
        };
        self.color_set_dialog = Some(ColorSetDialog {
            menu: menu.id,
            own: menu.color_set.is_some(),
            draft: menu.color_set_or_default(),
            preview: false,
        });
    }

    /// `menu` with the dialog's draft, when the dialog previews this menu.
    pub(crate) fn color_set_preview(&self, menu: &Menu) -> Option<Menu> {
        let dialog = self.color_set_dialog.as_ref()?;
        if !dialog.preview || dialog.menu != menu.id {
            return None;
        }
        let mut shown = menu.clone();
        shown.color_set = dialog.own.then_some(dialog.draft);
        Some(shown)
    }

    /// Draws the dialog while it is open.
    pub(crate) fn color_set_window(&mut self, ctx: &Context) {
        let Some(mut state) = self.color_set_dialog.clone() else {
            return;
        };
        let Some(menu) = self.session.project.menu(state.menu).cloned() else {
            self.color_set_dialog = None;
            return;
        };
        let mut done = None;
        let response = Modal::new(Id::unique("color_set_dialog")).show(ctx, |ui| {
            ui.heading(tl!("Menu Color Set"));
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_max_width(440.0);
                    ui.horizontal(|ui| {
                        crate::properties::prop_label(ui, tl!("Menu:"));
                        ui.label(&menu.name);
                    });
                    ui.horizontal(|ui| {
                        crate::properties::prop_label(ui, tl!("Color Set:"));
                        let shown = if state.own {
                            tl!("Menu Default")
                        } else {
                            tl!("Automatic")
                        };
                        egui::ComboBox::from_id_salt("color_set_kind")
                            .width(200.0)
                            .selected_text(shown)
                            .show_ui(ui, |ui| {
                                if ui.selectable_label(!state.own, tl!("Automatic")).clicked() {
                                    state.own = false;
                                    // The automatic set is made from the highlight colour.
                                    let mut automatic = menu.clone();
                                    automatic.color_set = None;
                                    state.draft = automatic.color_set_or_default();
                                }
                                if ui
                                    .selectable_label(state.own, tl!("Menu Default"))
                                    .clicked()
                                {
                                    state.own = true;
                                }
                            });
                    });
                    ui.add_enabled_ui(state.own, |ui| color_set_fields(ui, &mut state.draft));
                });
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    // Encore と同じく右の列に OK / キャンセル / プレビュー（折り返さない）。
                    ui.set_min_width(110.0);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    let size = egui::vec2(100.0, 0.0);
                    if ui
                        .add(egui::Button::new(tl!("OK")).min_size(size))
                        .clicked()
                    {
                        done = Some(true);
                    }
                    if ui
                        .add(egui::Button::new(tl!("Cancel")).min_size(size))
                        .clicked()
                    {
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
                self.color_set_dialog = None;
                let set = if state.own {
                    serde_json::to_value(state.draft).unwrap_or(serde_json::Value::Null)
                } else {
                    serde_json::Value::Null
                };
                let _ = self.run_command(
                    "menu.replaceColorSet",
                    json!({ "menu": state.menu.0, "colorSet": set }),
                );
            }
            Some(false) => self.color_set_dialog = None,
            None => self.color_set_dialog = Some(state),
        }
    }
}

/// The normal group and the highlight groups, in Encore's layout.
fn color_set_fields(ui: &mut Ui, set: &mut ColorSet) {
    ui.group(|ui| {
        ui.label(tl!("Normal Group"));
        egui::Grid::new("color_set_normal")
            .num_columns(3)
            .show(ui, |ui| {
                ui.label("");
                ui.label(tl!("Color"));
                ui.label(tl!("Opacity"));
                ui.end_row();
                ui.label("1:");
                color_button(ui, &mut set.normal.color);
                opacity_value(ui, &mut set.normal.opacity);
                ui.end_row();
                // Colours 2 and 3 (a subpicture's `(=2)` / `(=3)` layers).
                let mut more = set.normal_more.unwrap_or(DEFAULT_NORMAL_MORE);
                for (n, color) in (2..).zip(more.iter_mut()) {
                    ui.label(format!("{n}:"));
                    color_button(ui, &mut color.color);
                    opacity_value(ui, &mut color.opacity);
                    ui.end_row();
                }
                if more != set.normal_more.unwrap_or(DEFAULT_NORMAL_MORE) {
                    set.normal_more = Some(more);
                }
            });
    });
    for (index, group) in set.groups.iter_mut().enumerate() {
        ui.group(|ui| {
            ui.label(tlf!("Highlight Group {n}", n = index.saturating_add(1)));
            egui::Grid::new(("color_set_group", index))
                .num_columns(5)
                .show(ui, |ui| {
                    ui.label("");
                    ui.label(tl!("Selected Color"));
                    ui.label("");
                    ui.label(tl!("Activated Color"));
                    ui.label("");
                    ui.end_row();
                    ui.label("1:");
                    color_button(ui, &mut group.selected);
                    opacity_value(ui, &mut group.selected_opacity);
                    ui.add_enabled_ui(!group.same_activated, |ui| {
                        color_button(ui, &mut group.activated);
                    });
                    ui.add_enabled_ui(!group.same_activated, |ui| {
                        opacity_value(ui, &mut group.activated_opacity);
                    });
                    ui.end_row();
                    // Colours 2 and 3 (a subpicture's `(=2)` / `(=3)` layers): tints of colour 1 until changed.
                    let before = group.more_colors();
                    let mut more = before;
                    for (n, color) in (2..).zip(more.iter_mut()) {
                        ui.label(format!("{n}:"));
                        color_button(ui, &mut color.selected);
                        opacity_value(ui, &mut color.selected_opacity);
                        ui.add_enabled_ui(!group.same_activated, |ui| {
                            color_button(ui, &mut color.activated);
                        });
                        ui.add_enabled_ui(!group.same_activated, |ui| {
                            opacity_value(ui, &mut color.activated_opacity);
                        });
                        ui.end_row();
                    }
                    if more != before {
                        group.more = Some(more);
                    }
                });
            ui.checkbox(
                &mut group.same_activated,
                tl!("Use Selected Colors as Activated Colors"),
            );
        });
    }
}

fn color_button(ui: &mut Ui, color: &mut [u8; 3]) {
    let mut rgb = rgb_to_f32(*color);
    if ui.color_edit_button_rgb(&mut rgb).changed() {
        *color = rgb_from_f32(rgb);
    }
}

fn opacity_value(ui: &mut Ui, opacity: &mut u8) {
    let mut value = u16::from(*opacity);
    ui.add(egui::DragValue::new(&mut value).range(0..=100).suffix(" %"));
    *opacity = u8::try_from(value).unwrap_or(100);
}
