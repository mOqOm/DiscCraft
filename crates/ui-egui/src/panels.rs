use crate::app::{DiscCraftApp, Selection};
use crate::i18n::{tl, tlf};
use crate::menu_editor::link_label;

use disccraft_project::{AssetId, AssetKind, MenuId};

use egui::{self, Ui};

impl DiscCraftApp {
    /// フローチャートとリンク一覧。
    pub(crate) fn flowchart_panel(&mut self, ui: &mut Ui) {
        self.flowchart_view(ui);
        ui.separator();
        ui.collapsing(tl!("Link list"), |ui| self.link_overview(ui));
    }

    pub(crate) fn rename_field(&mut self, ui: &mut Ui) {
        let Some(key) = self.selection_key() else {
            return;
        };

        let current = self.selection_name().unwrap_or_default();

        // 選択項目が変わったときだけ入力欄を同期する（入力中の文字を消さない）。
        if self.rename_key != Some(key) {
            self.rename_key = Some(key);
            self.rename_value.clone_from(&current);
        }

        let response =
            ui.add(egui::TextEdit::singleline(&mut self.rename_value).desired_width(200.0));
        if std::mem::take(&mut self.focus_rename) {
            response.request_focus();
        }

        let committed = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

        if committed && self.rename_value.trim() != current.trim() {
            let name = self.rename_value.clone();
            self.rename_selection(&name);
        }
    }

    fn selection_name(&self) -> Option<String> {
        let project = &self.session.project;

        match self.selection {
            Selection::Asset(id) => project.assets.get(&id).map(|asset| asset.name.clone()),
            Selection::Timeline(id) => project.timeline(id).map(|timeline| timeline.name.clone()),
            Selection::Menu(id) => project.menu(id).map(|menu| menu.name.clone()),
            Selection::Slideshow(id) => project.slideshow(id).map(|show| show.name.clone()),
            Selection::Playlist(id) => project.playlist(id).map(|list| list.name.clone()),
            Selection::ChapterPlaylist(id) => {
                project.chapter_playlist(id).map(|list| list.name.clone())
            }
            Selection::None => None,
        }
    }

    /// 「再生リストの名前」ダイアログ（Encore の 新規 ▸ 再生リスト…）: 名前を入れて OK（Enter）で作る。
    pub(crate) fn playlist_name_window(&mut self, ctx: &egui::Context) {
        let Some(mut name) = self.playlist_name_dialog.clone() else {
            return;
        };
        let mut done = None;
        let response = egui::Modal::new(egui::Id::unique("playlist_name_dialog")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.heading(tl!("Playlist Name"));
            ui.label(tl!("Enter a name for the playlist:"));
            let field = ui.add(egui::TextEdit::singleline(&mut name).desired_width(300.0));
            if !field.has_focus() && !field.lost_focus() {
                field.request_focus();
            }
            if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = Some(true);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button(tl!("OK")).clicked() {
                    done = Some(true);
                }
                if ui.button(tl!("Cancel")).clicked() {
                    done = Some(false);
                }
            });
        });
        if response.should_close() {
            done = Some(false);
        }
        match done {
            Some(true) => {
                self.playlist_name_dialog = None;
                self.create_playlist_named(&name);
            }
            Some(false) => self.playlist_name_dialog = None,
            None => self.playlist_name_dialog = Some(name),
        }
    }

    /// リンクの総覧。フローチャート実装までの代替表示。
    fn link_overview(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let project = &self.session.project;

            ui.label(tlf!(
                "First play: {target}",
                target = link_label(project, &project.settings.first_play)
            ));

            let menus: Vec<(MenuId, String, Vec<(String, String)>)> = project
                .menus
                .iter()
                .map(|(id, menu)| {
                    let buttons = menu
                        .buttons
                        .iter()
                        .map(|button| {
                            (
                                if button.label.is_empty() {
                                    tl!("(unnamed)").to_string()
                                } else {
                                    button.label.clone()
                                },
                                link_label(project, &button.link),
                            )
                        })
                        .collect();

                    (*id, menu.name.clone(), buttons)
                })
                .collect();

            for (_, name, buttons) in menus {
                ui.strong(tlf!("Menu: {name}", name = name));

                for (label, target) in buttons {
                    ui.label(format!("  {label} → {target}"));
                }
            }

            let timelines: Vec<(String, String)> = project
                .timelines
                .values()
                .map(|timeline| {
                    (
                        timeline.name.clone(),
                        link_label(project, &timeline.end_action),
                    )
                })
                .collect();

            for (name, target) in timelines {
                ui.label(tlf!(
                    "Timeline: {name} → end action: {target}",
                    name = name,
                    target = target
                ));
            }
        });
    }

    pub(crate) fn image_assets(&self) -> Vec<(AssetId, String)> {
        self.session
            .project
            .assets
            .iter()
            .filter(|(_, asset)| asset.kind == AssetKind::Image)
            .map(|(id, asset)| (*id, asset.name.clone()))
            .collect()
    }

    pub(crate) fn asset_name(&self, id: AssetId) -> String {
        self.session
            .project
            .assets
            .get(&id)
            .map(|asset| asset.name.clone())
            .unwrap_or_else(|| tl!("Unknown").to_string())
    }
}
