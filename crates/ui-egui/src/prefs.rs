//! 環境設定ダイアログ（Encore と同じ形。編集 ▸ 環境設定、Ctrl+K。[docs/ui-parity.md](../../../docs/ui-parity.md) §3.5）。
//!
//! 左に分類の一覧、右にその設定を置く。「一般」はテレビ方式の初期設定（新規プロジェクトの既定）、
//! 「表示方法」は表示言語。設定は `workspace.json` に保存する。

use crate::DiscCraftApp;
use crate::i18n::{Language, tl};

use disccraft_project::VideoStandard;
use egui::{Context, Id, Ui};

/// 環境設定の分類。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum PrefsCategory {
    /// 一般（テレビ方式の初期設定）。
    #[default]
    General,
    /// メニュー（ボタンのルートとボタン同期の初期設定。プロジェクトに保存）。
    Menus,
    /// 表示方法（表示言語）。
    Interface,
}

impl PrefsCategory {
    /// 左の一覧に並べる順。
    pub(crate) const ALL: [PrefsCategory; 3] = [
        PrefsCategory::General,
        PrefsCategory::Menus,
        PrefsCategory::Interface,
    ];

    /// 表示名。
    pub(crate) fn label(self) -> &'static str {
        match self {
            PrefsCategory::General => tl!("General"),
            PrefsCategory::Menus => tl!("Menus"),
            PrefsCategory::Interface => tl!("Interface"),
        }
    }
}

/// 環境設定ダイアログの状態。
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PrefsWindow {
    /// 表示中かどうか。
    pub(crate) open: bool,
    /// 選択中の分類。
    pub(crate) category: PrefsCategory,
}

impl DiscCraftApp {
    /// 環境設定ダイアログを描く（開いているときだけ）。
    pub(crate) fn prefs_window(&mut self, ctx: &Context) {
        if !self.prefs.open {
            return;
        }

        let mut open = self.prefs.open;
        let _ = egui::Window::new(tl!("Preferences"))
            .id(Id::unique("prefs_window"))
            .open(&mut open)
            .resizable(false)
            .default_size([480.0, 220.0])
            .show(ctx, |ui| self.prefs_contents(ui));
        self.prefs.open = open;
    }

    /// 左の分類と右の設定。
    fn prefs_contents(&mut self, ui: &mut Ui) {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(110.0);
                for category in PrefsCategory::ALL {
                    if ui
                        .selectable_label(self.prefs.category == category, category.label())
                        .clicked()
                    {
                        self.prefs.category = category;
                    }
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_min_height(170.0);
                match self.prefs.category {
                    PrefsCategory::General => self.prefs_general(ui),
                    PrefsCategory::Menus => self.prefs_menus(ui),
                    PrefsCategory::Interface => self.prefs_interface(ui),
                }
            });
        });
    }

    /// 「一般」: テレビ方式の初期設定（新規プロジェクトの既定）。
    fn prefs_general(&mut self, ui: &mut Ui) {
        ui.strong(tl!("General"));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(tl!("Default TV standard:"));
            let current = crate::workspace::load_default_standard();
            let mut chosen = None;
            egui::ComboBox::from_id_salt("prefs_standard")
                .selected_text(standard_label(current))
                .show_ui(ui, |ui| {
                    for standard in [VideoStandard::Ntsc, VideoStandard::Pal] {
                        if ui
                            .selectable_label(current == standard, standard_label(standard))
                            .clicked()
                        {
                            chosen = Some(standard);
                        }
                    }
                });
            if let Some(standard) = chosen
                && let Err(error) = crate::workspace::save_default_standard(standard)
            {
                self.status = error;
            }
        });
        ui.small(tl!("Used as the TV standard of new projects."));
    }

    /// 「メニュー」（Encore と同じ並び）: ボタンのルートと、ボタン同期の初期設定。DiscCraft はプロジェクトに保存する
    /// （どのパソコンでビルドしても同じルートになるように）。
    fn prefs_menus(&mut self, ui: &mut Ui) {
        ui.strong(tl!("Menus"));
        ui.add_space(6.0);
        let settings = &self.session.project.settings;
        let mut routing = settings.routing;
        let mut defaults = settings.button_defaults;
        let mut change: Option<serde_json::Value> = None;
        ui.group(|ui| {
            ui.label(tl!("Button Routing"));
            for vertical in [true, false] {
                let (moves, wraps, mode) = if vertical {
                    (
                        &mut routing.vertical,
                        &mut routing.vertical_wrap,
                        &mut routing.vertical_wrap_mode,
                    )
                } else {
                    (
                        &mut routing.horizontal,
                        &mut routing.horizontal_wrap,
                        &mut routing.horizontal_wrap_mode,
                    )
                };
                let (move_label, wrap_label, key) = if vertical {
                    (
                        tl!("Route Buttons Up / Down"),
                        tl!("Wrap Up / Down"),
                        "vertical",
                    )
                } else {
                    (
                        tl!("Route Buttons Left / Right"),
                        tl!("Wrap Left / Right"),
                        "horizontal",
                    )
                };
                if ui.checkbox(moves, move_label).changed() {
                    change = Some(serde_json::json!({ key: *moves }));
                }
                ui.add_enabled_ui(*moves, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        if ui.checkbox(wraps, wrap_label).changed() {
                            change = Some(serde_json::json!({ format!("{key}Wrap"): *wraps }));
                        }
                        let name = |mode: disccraft_project::WrapMode| match (mode, vertical) {
                            (disccraft_project::WrapMode::SameLine, true) => {
                                tl!("Wrap within Same Column")
                            }
                            (disccraft_project::WrapMode::NextLine, true) => {
                                tl!("Wrap to Next Column")
                            }
                            (disccraft_project::WrapMode::SameLine, false) => {
                                tl!("Wrap within Same Row")
                            }
                            (disccraft_project::WrapMode::NextLine, false) => {
                                tl!("Wrap to Next Row")
                            }
                        };
                        ui.add_enabled_ui(*wraps, |ui| {
                            egui::ComboBox::from_id_salt(("prefs_wrap", key))
                                .width(180.0)
                                .selected_text(name(*mode))
                                .show_ui(ui, |ui| {
                                    for choice in [
                                        disccraft_project::WrapMode::SameLine,
                                        disccraft_project::WrapMode::NextLine,
                                    ] {
                                        if ui
                                            .selectable_label(*mode == choice, name(choice))
                                            .clicked()
                                            && *mode != choice
                                        {
                                            change = Some(serde_json::json!({
                                                format!("{key}WrapMode"): choice.as_str()
                                            }));
                                        }
                                    }
                                });
                        });
                    });
                });
            }
        });
        ui.group(|ui| {
            ui.label(tl!("Button Sync Defaults"));
            if ui
                .checkbox(&mut defaults.sync_name, tl!("Sync Button Text and Name"))
                .changed()
            {
                change = Some(serde_json::json!({ "syncButtonNames": defaults.sync_name }));
            }
            if ui
                .checkbox(&mut defaults.name_from_link, tl!("Set Name from Link"))
                .changed()
            {
                change =
                    Some(serde_json::json!({ "nameButtonsFromLinks": defaults.name_from_link }));
            }
        });
        ui.small(tl!(
            "Saved with this project, so every computer builds the same routes."
        ));
        if let Some(params) = change {
            let _ = self.run_command("disc.setMenuOptions", params);
        }
    }

    /// 「表示方法」: 表示言語。
    fn prefs_interface(&mut self, ui: &mut Ui) {
        ui.strong(tl!("Interface"));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(tl!("Language:"));
            let mut chosen = None;
            egui::ComboBox::from_id_salt("prefs_language")
                .selected_text(self.language.name())
                .show_ui(ui, |ui| {
                    for language in Language::ALL {
                        if ui
                            .selectable_label(self.language == language, language.name())
                            .clicked()
                        {
                            chosen = Some(language);
                        }
                    }
                });
            if let Some(language) = chosen {
                self.set_language(language);
            }
        });
    }
}

fn standard_label(standard: VideoStandard) -> &'static str {
    match standard {
        VideoStandard::Ntsc => "NTSC",
        VideoStandard::Pal => "PAL",
    }
}
