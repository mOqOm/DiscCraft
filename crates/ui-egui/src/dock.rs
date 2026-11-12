//! 画面のタブグループ（Encore の既定の配置。[docs/ui-parity.md](../../../docs/ui-parity.md) §2）。
//!
//! egui のパネルは先に置いたものが端を取るので、ツールバー → ステータスバー → 右の列 → 下のグループ → 左の
//! グループ → 中央の順に置く。右の列はさらに上（プロパティ / ディスク）と下（レイヤー / ライブラリ）に分ける。

use crate::DiscCraftApp;
use crate::app::Selection;
use crate::i18n::{tl, tlf};
use crate::theme::{bar_frame, container_frame, group_frame, tab_bar};
use crate::workspace::{BottomTab, CenterTab, LeftTab, LowerTab, RightTab, WorkspacePreset};

use disccraft_project::{MenuId, TimelineId};
use egui::Ui;

/// 左のグループが占める横幅の割合（Encore の初期の配置）。
const LEFT_SHARE: f32 = 0.35;
/// 右の列が占める横幅の割合。
const RIGHT_SHARE: f32 = 0.22;
/// 下のグループ（タイムラインビューアなど）が占める高さの割合。
const BOTTOM_SHARE: f32 = 0.36;
/// 右の列のうち、右下のグループ（レイヤー / ライブラリ）が占める高さの割合。
const LOWER_SHARE: f32 = 0.49;
/// 横のグループのいちばん狭い幅。
const MIN_SIDE: f32 = 260.0;
/// 下のグループのいちばん低い高さ。
const MIN_BOTTOM: f32 = 160.0;
/// これより大きくウィンドウの大きさが変わったら、パネルを割合どおりに並べ直す。
const RESIZE_SLACK: f32 = 24.0;

impl DiscCraftApp {
    /// メニューバーの下のすべてを描く。
    pub(crate) fn draw_groups(&mut self, ui: &mut Ui) {
        // パネルの間の暗い溝。各パネルは外側の余白を空けて、この上に置く。
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 0.0, crate::theme::GUTTER);

        // Encore と同じ割合で分ける（左 35%、右 22%、下 36%）。ウィンドウの大きさが変わったら割合に戻す。
        let screen = area.size();
        if (screen - self.layout_size).length() > RESIZE_SLACK {
            self.layout_size = screen;
            self.layout_generation = self.layout_generation.wrapping_add(1);
        }
        let generation = self.layout_generation;
        let left_size = (screen.x * LEFT_SHARE).max(MIN_SIDE);
        let right_size = (screen.x * RIGHT_SHARE).max(MIN_SIDE);
        let bottom_size = (screen.y * BOTTOM_SHARE).max(MIN_BOTTOM);

        egui::Panel::top("toolbar")
            .frame(bar_frame())
            .show_separator_line(false)
            .show(ui, |ui| self.toolbar(ui));

        egui::Panel::bottom("status_bar")
            .frame(bar_frame())
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| ui.label(self.status.clone()));
            });

        if self.layout.right || self.layout.lower {
            egui::Panel::right(egui::Id::unique("right_column").with(generation))
                .frame(container_frame())
                .show_separator_line(false)
                .resizable(true)
                .default_size(right_size)
                .show(ui, |ui| self.right_column(ui));
        }

        if self.layout.bottom {
            egui::Panel::bottom(egui::Id::unique("bottom_group").with(generation))
                .frame(group_frame())
                .show_separator_line(false)
                .resizable(true)
                .default_size(bottom_size)
                .show(ui, |ui| self.bottom_group(ui));
        }

        if self.layout.left {
            egui::Panel::left(egui::Id::unique("left_group").with(generation))
                .frame(group_frame())
                .show_separator_line(false)
                .resizable(true)
                .default_size(left_size)
                .show(ui, |ui| self.left_group(ui));
        }

        egui::CentralPanel::default()
            .frame(group_frame())
            .show(ui, |ui| self.center_group(ui));
    }

    /// ツールバー。左にツール（選択 / テキスト / ズーム）とプレビュー、右端にワークスペースの選択を置く
    /// （Encore と同じ位置）。
    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            self.tool_buttons(ui);
            self.workspace_choice(ui);
        });
    }

    fn workspace_choice(&mut self, ui: &mut Ui) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut chosen = None;
            egui::ComboBox::from_id_salt("workspace_preset")
                .selected_text(self.workspace.label())
                .show_ui(ui, |ui| {
                    for preset in WorkspacePreset::ALL {
                        if ui
                            .selectable_label(self.workspace == preset, preset.label())
                            .clicked()
                        {
                            chosen = Some(preset);
                        }
                    }
                });
            ui.label(tl!("Workspace:"));
            if let Some(preset) = chosen {
                self.set_workspace(preset);
            }
        });
    }

    fn left_group(&mut self, ui: &mut Ui) {
        let tabs = [
            (LeftTab::Project, tl!("Project").to_owned()),
            (LeftTab::Menus, tl!("Menus").to_owned()),
            (LeftTab::Timelines, tl!("Timelines").to_owned()),
            (LeftTab::Build, tl!("Build").to_owned()),
        ];
        tab_bar(ui, &mut self.layout.left_tab, &tabs);

        match self.layout.left_tab {
            LeftTab::Project => self.project_tab(ui),
            // メニューのタブは上下に分かれ、それぞれがスクロールする。
            LeftTab::Menus => self.menus_tab(ui),
            LeftTab::Timelines => {
                fill_scroll("timelines_tab").show(ui, |ui| self.timelines_tab(ui));
            }
            LeftTab::Build => {
                fill_scroll("build_tab").show(ui, |ui| self.build_tab(ui));
            }
        }
    }

    fn center_group(&mut self, ui: &mut Ui) {
        let menu = self.viewer_menu();
        let menu_label = match menu {
            Some(id) => self.menu_name(id),
            None => tl!("(No menu)").to_owned(),
        };
        // Encore はチャプター再生リストごとにビューアのタブが開く。DiscCraft は最後に開いたものを 1 つのタブで。
        let chapter_playlist = self.viewer_chapter_playlist();
        let mut tabs = vec![
            (CenterTab::Monitor, tl!("Monitor").to_owned()),
            (CenterTab::Menu, menu_label),
            (CenterTab::Flowchart, tl!("Flowchart").to_owned()),
        ];
        if let Some(id) = chapter_playlist {
            let name = self
                .session
                .project
                .chapter_playlist(id)
                .map(|list| list.name.clone())
                .unwrap_or_default();
            tabs.push((
                CenterTab::ChapterPlaylist,
                tlf!("{name}: Chapter Playlist", name = name),
            ));
        } else if self.tab == CenterTab::ChapterPlaylist {
            self.tab = CenterTab::Monitor;
        }
        tab_bar(ui, &mut self.tab, &tabs);

        match self.tab {
            CenterTab::Monitor => self.monitor_view(ui),
            CenterTab::Menu => match menu {
                Some(id) => self.menu_editor(ui, id),
                None => {
                    ui.label(tl!("Choose a menu"));
                }
            },
            CenterTab::Flowchart => self.flowchart_panel(ui),
            CenterTab::ChapterPlaylist => match chapter_playlist {
                Some(id) => self.chapter_playlist_view(ui, id),
                None => {
                    ui.label(tl!("Choose a chapter playlist"));
                }
            },
        }
    }

    fn right_column(&mut self, ui: &mut Ui) {
        if self.layout.lower && self.layout.right {
            // Encore と同じく、右の列は上下ほぼ半分ずつ。
            let lower_size = (ui.available_height() * LOWER_SHARE).max(MIN_BOTTOM);
            egui::Panel::bottom(egui::Id::unique("lower_group").with(self.layout_generation))
                .frame(group_frame())
                .show_separator_line(false)
                .resizable(true)
                .default_size(lower_size)
                .show(ui, |ui| self.lower_group(ui));
        }

        // 右上を閉じたときは、右の列全体を右下のグループに使う。
        egui::CentralPanel::default()
            .frame(group_frame())
            .show(ui, |ui| {
                if self.layout.right {
                    self.right_group(ui);
                } else {
                    self.lower_group(ui);
                }
            });
    }

    fn right_group(&mut self, ui: &mut Ui) {
        let tabs = [
            (RightTab::Properties, tl!("Properties").to_owned()),
            (RightTab::Disc, tl!("Disc").to_owned()),
        ];
        tab_bar(ui, &mut self.layout.right_tab, &tabs);

        fill_scroll("right_group").show(ui, |ui| match self.layout.right_tab {
            RightTab::Properties => self.properties_tab(ui),
            RightTab::Disc => self.disc_sheet(ui),
        });
    }

    fn lower_group(&mut self, ui: &mut Ui) {
        // Encore と同じ並び（ライブラリ、レイヤー）。
        let tabs = [
            (LowerTab::Library, tl!("Library").to_owned()),
            (LowerTab::Layers, tl!("Layers").to_owned()),
        ];
        tab_bar(ui, &mut self.layout.lower_tab, &tabs);

        fill_scroll("lower_group").show(ui, |ui| match self.layout.lower_tab {
            LowerTab::Layers => self.layers_tab(ui),
            LowerTab::Library => self.library_tab(ui),
        });
    }

    fn bottom_group(&mut self, ui: &mut Ui) {
        let timeline = self.viewer_timeline();
        let slideshow = self.viewer_slideshow();
        let timeline_label = match timeline {
            Some(id) => self.timeline_name(id),
            None => tl!("(No timeline)").to_owned(),
        };
        let slideshow_label = match slideshow {
            Some(id) => self.slideshow_name(id),
            None => tl!("(No slideshow)").to_owned(),
        };
        let tabs = [
            (BottomTab::Timeline, timeline_label),
            (BottomTab::Slideshow, slideshow_label),
        ];
        tab_bar(ui, &mut self.layout.bottom_tab, &tabs);

        // タイムラインビューアは自分でスクロールする（下にズームのバー）。
        if let (BottomTab::Timeline, Some(id)) = (self.layout.bottom_tab, timeline) {
            self.timeline_view(ui, id);
            return;
        }
        fill_scroll("bottom_group").show(ui, |ui| {
            match (self.layout.bottom_tab, timeline, slideshow) {
                (BottomTab::Timeline, Some(id), _) => self.timeline_view(ui, id),
                (BottomTab::Slideshow, _, Some(id)) => self.slideshow_view(ui, id),
                (BottomTab::Timeline, None, _) => {
                    ui.label(tl!("Choose a timeline"));
                }
                (BottomTab::Slideshow, _, None) => {
                    ui.label(tl!("Choose a slideshow"));
                }
            }
        });
    }

    /// プロパティのタブ。再生リストは項目の編集もここに出す（Encore では専用のダイアログ）。
    fn properties_tab(&mut self, ui: &mut Ui) {
        self.property_sheet(ui);
    }

    /// レイヤーのタブ。メニュービューアのメニューの背景とボタンを、重なりの上から並べる。
    fn layers_tab(&mut self, ui: &mut Ui) {
        let Some(id) = self.viewer_menu() else {
            ui.label(tl!("Open a menu to see its layers."));
            return;
        };
        let Some(menu) = self.session.project.menu(id).cloned() else {
            return;
        };

        let mut clicked = None;
        let mut clicked_text = None;
        for (index, button) in menu.buttons.iter().enumerate().rev() {
            let number = index.saturating_add(1);
            let name = if button.label.is_empty() {
                button.name.clone()
            } else {
                button.label.clone()
            };
            let text = tlf!("Button {n}: {name}", n = number, name = name);
            let selected = self.selection == Selection::Menu(id) && self.button_selected(index);
            let response = ui.selectable_label(selected, text);
            if response.clicked() {
                let modifiers = ui.input(|i| i.modifiers);
                clicked = Some((index, modifiers.shift || modifiers.command));
            }
        }
        // テキストはボタンの下、背景の上。
        for (index, text) in menu.texts.iter().enumerate().rev() {
            let first = text.text.lines().next().unwrap_or_default();
            let selected = self.selected_menu_text(id) == Some(index);
            if ui
                .selectable_label(selected, tlf!("Text: {text}", text = first))
                .clicked()
            {
                clicked_text = Some(index);
            }
        }
        // 画像 / シェイプはテキストの下、背景の上。
        let mut clicked_graphic = None;
        for (index, graphic) in menu.graphics.iter().enumerate().rev() {
            let selected = self.selected_menu_graphic(id) == Some(index);
            if ui
                .selectable_label(selected, self.graphic_layer_name(graphic))
                .clicked()
            {
                clicked_graphic = Some(index);
            }
        }
        ui.label(tl!("Background"));
        if let Some(index) = clicked_text {
            self.select_menu_text(id, index);
            self.tab = CenterTab::Menu;
        }
        if let Some(index) = clicked_graphic {
            self.select_menu_graphic(id, index);
            self.tab = CenterTab::Menu;
        }

        // Shift / Ctrl を押しながらのクリックは選択に加える / 外す。
        match clicked {
            Some((index, true)) => self.toggle_button(id, index),
            Some((index, false)) => self.select_only_button(id, index),
            None => {}
        }
        if clicked.is_some() {
            self.tab = CenterTab::Menu;
        }
    }

    /// メニュービューアに出すメニュー（選択中のメニュー、なければ最後に開いたメニュー）。
    pub(crate) fn viewer_menu(&self) -> Option<MenuId> {
        if let Selection::Menu(id) = self.selection {
            return Some(id);
        }
        self.last_menu
            .filter(|id| self.session.project.menu(*id).is_some())
    }

    /// タイムラインビューアに出すタイムライン。
    pub(crate) fn viewer_timeline(&self) -> Option<TimelineId> {
        if let Selection::Timeline(id) = self.selection {
            return Some(id);
        }
        self.last_timeline
            .filter(|id| self.session.project.timeline(*id).is_some())
    }

    /// チャプター再生リストビューアに出す再生リスト（選択中のもの、なければ最後に開いたもの）。
    pub(crate) fn viewer_chapter_playlist(&self) -> Option<u64> {
        if let Selection::ChapterPlaylist(id) = self.selection {
            return Some(id);
        }
        self.last_chapter_playlist
            .filter(|id| self.session.project.chapter_playlists.contains_key(id))
    }

    /// スライドショービューアに出すスライドショー。
    pub(crate) fn viewer_slideshow(&self) -> Option<u64> {
        if let Selection::Slideshow(id) = self.selection {
            return Some(id);
        }
        self.last_slideshow
            .filter(|id| self.session.project.slideshows.contains_key(id))
    }

    fn menu_name(&self, id: MenuId) -> String {
        self.session
            .project
            .menu(id)
            .map(|menu| menu.name.clone())
            .unwrap_or_default()
    }

    fn timeline_name(&self, id: TimelineId) -> String {
        self.session
            .project
            .timeline(id)
            .map(|timeline| timeline.name.clone())
            .unwrap_or_default()
    }

    fn slideshow_name(&self, id: u64) -> String {
        self.session
            .project
            .slideshow(id)
            .map(|slideshow| slideshow.name.clone())
            .unwrap_or_default()
    }
}

/// グループいっぱいに広がるスクロール領域（egui のパネルは中身が小さいとその大きさまで縮むため）。
fn fill_scroll(id: &str) -> egui::ScrollArea {
    egui::ScrollArea::both()
        .id_salt(id)
        .auto_shrink([false, false])
}
