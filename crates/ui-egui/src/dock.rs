//! 画面のパネル（[docs/ui-parity.md](../../../docs/ui-parity.md) §2）。
//!
//! メニューバーの下にツールバー、いちばん下にステータスバーを置き、その間を craft-libs の craft-ui のドッキング
//! （FilmCraft などの Craft と同じもの）に任せる。craft-ui がタブグループ、タブの並べ替え・移動・分割・
//! フローティング、閉じる操作を描いて受け持ち、DiscCraft は各パネルの中身とタブの名前を描く。並びは
//! ワークスペース（[`crate::workspace`]）として保存する。

use crate::DiscCraftApp;
use crate::app::Selection;
use crate::i18n::{tl, tlf};
use crate::icons::{self, Icon};
use crate::theme::bar_frame;
use crate::workspace::{Panel, WorkspaceId, WorkspacePreset, permissions};

use craft_ui::docking::{Action, DockArea, DockContent, DockStyle, PanelState};
use disccraft_project::{MenuId, TimelineId};
use egui::{Sense, Ui};

/// ドッキングの領域の egui の id。
fn area_id() -> egui::Id {
    egui::Id::unique("disccraft_dock")
}

/// タブの名前の前に空ける幅（つまみを描く）。
const GRIP_SPACE: f32 = 12.0;
/// タブの名前の後ろに空ける幅（× を描く）。
const CLOSE_SPACE: f32 = 14.0;
/// 前面のビューアのタブで、名前と × の間に空ける幅（区切りと ▼ を描く）。
const CHOICE_SPACE: f32 = 24.0;
/// craft-ui がタブの左端から名前（つまみの場所を含む）を描き始めるまでの幅。
const TAB_TEXT_INSET: f32 = 12.0;

impl DiscCraftApp {
    /// メニューバーの下のすべてを描く。
    pub(crate) fn draw_groups(&mut self, ui: &mut Ui) {
        egui::Panel::top("toolbar")
            .frame(bar_frame())
            .show_separator_line(false)
            .show(ui, |ui| self.toolbar(ui));

        egui::Panel::bottom("status_bar")
            .frame(bar_frame())
            .show_separator_line(false)
            .show(ui, |ui| {
                // While a build runs, its progress (wherever the Build panel is).
                let text = self
                    .build_status_text()
                    .unwrap_or_else(|| self.status.clone());
                ui.horizontal(|ui| ui.label(text));
            });

        // パネルの間の暗い溝の上にタブグループを並べる。
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(crate::theme::GUTTER))
            .show(ui, |ui| self.dock_area(ui));
    }

    /// タブグループを描き、ドラッグや × でされた並びの変更をワークスペースに当てる。
    fn dock_area(&mut self, ui: &mut Ui) {
        let style = dock_style(ui);
        let layout = self.dock.layout.clone();
        let mut content = DiscContent { app: self };
        let output = DockArea::new(area_id()).show_content(ui, &layout, &style, &mut content);
        let close = self.tab_chrome(ui);

        let mut actions = output.actions;
        if let Some(panel) = close {
            actions.push(Action::Close { panel });
        }
        if let Some(error) = output.error {
            self.status = tlf!(
                "Cannot arrange the panels: {error}",
                error = format!("{error:?}")
            );
        }
        if !actions.is_empty() {
            self.apply_dock(actions);
        }
        self.save_dock_when_idle(ui.ctx());
    }

    /// Encore のタブと同じく、並び替えられることが見て分かるように: グループの前面のタブの名前の左につまみ
    /// （点の並び）、右にそのタブを閉じる ×（後ろのタブは名前だけ、カーソルは普通の矢印）。タブの場所は craft-ui
    /// が前のフレームで記録したもの（ドッキングされたタブだけ。ドラッグ中は描かない）。押された × のパネルを
    /// 返す。
    fn tab_chrome(&mut self, ui: &mut Ui) -> Option<Panel> {
        use craft_ui::docking::Hit;
        let ctx = ui.ctx().clone();
        if ctx.dragged_id().is_some() {
            return None;
        }
        let mut close = None;
        let mut chosen = None;
        for (hit, rect) in DockArea::<Panel>::hit_rects(&ctx, area_id()) {
            let Hit::Tab(panel) = hit else {
                continue;
            };
            // フローティングの窓のタブは窓のレイヤーにある（そこには描かない）。
            if self
                .dock
                .layout
                .floating
                .iter()
                .any(|window| window.contains(&panel))
            {
                continue;
            }
            // 後ろのタブは名前だけ（Encore と同じ）。
            if !self.tab_has_chrome(panel) {
                continue;
            }
            // つまみ: 2 列 × 3 段の点。
            let grip = egui::Rect::from_center_size(
                egui::pos2(rect.left() + 9.0, rect.center().y),
                egui::vec2(4.0, 9.0),
            );
            let dot = crate::theme::TEXT;
            for column in 0..2 {
                for row in 0..3 {
                    let center = egui::pos2(
                        grip.left() + column as f32 * 3.5,
                        grip.top() + row as f32 * 3.5 + 1.0,
                    );
                    ui.painter().circle_filled(center, 0.9, dot);
                }
            }
            // Encore と同じく、前面のビューアのタブは名前が切り替えのリスト（くぼんだ箱に名前と | ▼）。
            if self.tab_choice_box(panel) {
                let choices = self.viewer_choices(panel);
                let current = self.viewer_selection(panel);
                chosen = viewer_tab_box(ui, rect, panel, &choices, current).or(chosen);
            }
            // ×: そのタブを閉じる（ウィンドウメニューで開き直せる）。
            let close_rect = egui::Rect::from_center_size(
                egui::pos2(rect.right() - 11.0, rect.center().y),
                egui::vec2(14.0, 14.0),
            );
            let response = ui
                .interact(
                    close_rect,
                    egui::Id::unique("tab_close").with(panel),
                    Sense::click(),
                )
                .on_hover_text(tl!("Close Panel"));
            if response.hovered() {
                ui.painter()
                    .rect_filled(close_rect, 2.0, crate::craft_timeline::style::HOVER);
            }
            let color = if response.hovered() {
                crate::theme::TEXT_STRONG
            } else {
                crate::theme::TEXT
            };
            icons::paint(ui.painter(), close_rect.shrink(3.0), Icon::Close, color);
            if response.clicked() {
                close = Some(panel);
            }
        }
        if let Some(selection) = chosen.filter(|chosen| *chosen != self.selection) {
            self.select(selection);
        }
        close
    }

    /// タブに切り替えのリストの箱を出すか: 前面に出ているビューアで、選べる項目があるとき。
    fn tab_choice_box(&self, panel: Panel) -> bool {
        self.tab_has_chrome(panel) && !self.viewer_choices(panel).is_empty()
    }

    /// タブにつまみと × を出すか: グループの前面に出ているタブ（Encore と同じ）。フローティングの窓のタブは
    /// craft-ui の描くまま。
    fn tab_has_chrome(&self, panel: Panel) -> bool {
        self.panel_front(panel)
            && !self
                .dock
                .layout
                .floating
                .iter()
                .any(|window| window.contains(&panel))
    }

    /// ビューアのタブの ▼ で選べる項目（選んだときの選択と名前、プロジェクトの順）。ビューアでなければ空。
    fn viewer_choices(&self, panel: Panel) -> Vec<(Selection, String)> {
        let project = &self.session.project;
        match panel {
            Panel::TimelineViewer => project
                .timelines
                .iter()
                .map(|(id, timeline)| (Selection::Timeline(*id), timeline.name.clone()))
                .collect(),
            Panel::MenuViewer => project
                .menus
                .iter()
                .map(|(id, menu)| (Selection::Menu(*id), menu.name.clone()))
                .collect(),
            Panel::SlideshowViewer => project
                .slideshows
                .iter()
                .map(|(id, slideshow)| (Selection::Slideshow(*id), slideshow.name.clone()))
                .collect(),
            Panel::ChapterPlaylist => project
                .chapter_playlists
                .iter()
                .map(|(id, list)| (Selection::ChapterPlaylist(*id), list.name.clone()))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// ビューアに出ている項目（▼ のリストで印を付ける）。
    fn viewer_selection(&self, panel: Panel) -> Option<Selection> {
        match panel {
            Panel::TimelineViewer => self.viewer_timeline().map(Selection::Timeline),
            Panel::MenuViewer => self.viewer_menu().map(Selection::Menu),
            Panel::SlideshowViewer => self.viewer_slideshow().map(Selection::Slideshow),
            Panel::ChapterPlaylist => self
                .viewer_chapter_playlist()
                .map(Selection::ChapterPlaylist),
            _ => None,
        }
    }

    /// 並びの変更をまとめて当てる（全部か、何もしないか）。
    pub(crate) fn apply_dock(&mut self, actions: Vec<Action<Panel>>) {
        match self.dock.apply_all(actions, permissions) {
            Ok(_) => self.dock_dirty = true,
            Err(error) => {
                self.status = tlf!(
                    "Cannot arrange the panels: {error}",
                    error = format!("{error:?}")
                );
            }
        }
    }

    /// `panel` がタブグループの前面に出ているか。
    pub(crate) fn panel_front(&self, panel: Panel) -> bool {
        self.dock.layout.panel_state(&panel) == PanelState::Front
    }

    /// `panel` を前面に出す。閉じていれば開く: 閉じる前の場所へ、なければ初期設定の配置で近いパネルの隣へ。
    pub(crate) fn show_panel(&mut self, panel: Panel) {
        if self.panel_front(panel) {
            return;
        }
        if self.dock.layout.contains(&panel) {
            self.apply_dock(vec![Action::Activate { panel }]);
            return;
        }
        let fallback = panel
            .neighbours()
            .iter()
            .copied()
            .find(|other| *other != panel && self.dock.layout.contains(other));
        match self.dock.set_visible(panel, true, fallback, permissions) {
            Ok(()) => self.dock_dirty = true,
            Err(error) => {
                self.status = tlf!(
                    "Cannot arrange the panels: {error}",
                    error = format!("{error:?}")
                );
            }
        }
    }

    /// ウィンドウメニュー: 前面に出ているパネルなら閉じ、そうでなければ前面に出す。
    pub(crate) fn toggle_panel(&mut self, panel: Panel) {
        if !self.panel_front(panel) {
            self.show_panel(panel);
            return;
        }
        match self.dock.set_visible(panel, false, None, permissions) {
            Ok(()) => self.dock_dirty = true,
            Err(error) => {
                self.status = tlf!(
                    "Cannot arrange the panels: {error}",
                    error = format!("{error:?}")
                );
            }
        }
    }

    /// 並びが変わっていれば設定に保存する。仕切りやタブをドラッグしている間は待つ（離してから 1 回）。
    fn save_dock_when_idle(&mut self, ctx: &egui::Context) {
        if !self.dock_dirty || ctx.input(|input| input.pointer.any_down()) {
            return;
        }
        self.dock_dirty = false;
        if let Err(error) =
            crate::workspace::save_workspace(&self.workspace, &self.dock, &self.user_workspaces)
        {
            self.status = tlf!(
                "Workspace: {name} (save failed: {error})",
                name = self.workspace.label(),
                error = error
            );
        }
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
                    for id in self.workspace_ids() {
                        if ui
                            .selectable_label(self.workspace == id, id.label())
                            .clicked()
                        {
                            chosen = Some(id);
                        }
                    }
                });
            ui.label(tl!("Workspace:"));
            if let Some(id) = chosen {
                self.set_workspace(id);
            }
        });
    }

    /// 選べるワークスペース: 組み込みのもの、名前を付けて保存したもの（作った順）。
    pub(crate) fn workspace_ids(&self) -> Vec<WorkspaceId> {
        WorkspacePreset::ALL
            .into_iter()
            .map(WorkspaceId::Preset)
            .chain(self.user_workspaces.iter().cloned().map(WorkspaceId::User))
            .collect()
    }

    /// パネルの中身。
    fn panel_body(&mut self, ui: &mut Ui, panel: Panel) {
        match panel {
            Panel::Project => self.project_tab(ui),
            // メニューのタブは上下に分かれ、それぞれがスクロールする。
            Panel::Menus => self.menus_tab(ui),
            Panel::Timelines => {
                fill_scroll("timelines_tab").show(ui, |ui| self.timelines_tab(ui));
            }
            Panel::Build => {
                fill_scroll("build_tab").show(ui, |ui| self.build_tab(ui));
            }
            Panel::Monitor => self.monitor_view(ui),
            Panel::MenuViewer => match self.viewer_menu() {
                Some(id) => self.menu_editor(ui, id),
                None => {
                    ui.label(tl!("Choose a menu"));
                }
            },
            Panel::Flowchart => self.flowchart_panel(ui),
            Panel::ChapterPlaylist => match self.viewer_chapter_playlist() {
                Some(id) => self.chapter_playlist_view(ui, id),
                None => {
                    ui.label(tl!("Choose a chapter playlist"));
                }
            },
            Panel::Properties => {
                fill_scroll("properties_tab").show(ui, |ui| self.properties_tab(ui));
            }
            Panel::Disc => {
                fill_scroll("disc_tab").show(ui, |ui| self.disc_sheet(ui));
            }
            Panel::Library => {
                fill_scroll("library_tab").show(ui, |ui| self.library_tab(ui));
            }
            Panel::Layers => {
                fill_scroll("layers_tab").show(ui, |ui| self.layers_tab(ui));
            }
            // タイムラインビューアは自分でスクロールする（下にズームのバー）。
            Panel::TimelineViewer => match self.viewer_timeline() {
                Some(id) => self.timeline_view(ui, id),
                None => {
                    ui.label(tl!("Choose a timeline"));
                }
            },
            Panel::SlideshowViewer => match self.viewer_slideshow() {
                Some(id) => {
                    fill_scroll("slideshow_tab").show(ui, |ui| self.slideshow_view(ui, id));
                }
                None => {
                    ui.label(tl!("Choose a slideshow"));
                }
            },
        }
    }

    /// タブの名前。ビューアは開いている項目の名前（Encore と同じ）。
    pub(crate) fn panel_label(&self, panel: Panel) -> String {
        match panel {
            Panel::MenuViewer => match self.viewer_menu() {
                Some(id) => self.menu_name(id),
                None => tl!("(No menu)").to_owned(),
            },
            Panel::TimelineViewer => match self.viewer_timeline() {
                Some(id) => self.timeline_name(id),
                None => tl!("(No timeline)").to_owned(),
            },
            Panel::SlideshowViewer => match self.viewer_slideshow() {
                Some(id) => self.slideshow_name(id),
                None => tl!("(No slideshow)").to_owned(),
            },
            // Encore はチャプター再生リストごとにビューアのタブが開く。DiscCraft は最後に開いたものを 1 つのタブで。
            Panel::ChapterPlaylist => match self.viewer_chapter_playlist() {
                Some(id) => {
                    let name = self
                        .session
                        .project
                        .chapter_playlist(id)
                        .map(|list| list.name.clone())
                        .unwrap_or_default();
                    tlf!("{name}: Chapter Playlist", name = name)
                }
                None => panel.title().to_owned(),
            },
            _ => panel.title().to_owned(),
        }
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
            self.show_panel(Panel::MenuViewer);
        }
        if let Some(index) = clicked_graphic {
            self.select_menu_graphic(id, index);
            self.show_panel(Panel::MenuViewer);
        }

        // Shift / Ctrl を押しながらのクリックは選択に加える / 外す。
        match clicked {
            Some((index, true)) => self.toggle_button(id, index),
            Some((index, false)) => self.select_only_button(id, index),
            None => {}
        }
        if clicked.is_some() {
            self.show_panel(Panel::MenuViewer);
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

/// DiscCraft の見た目のドッキング: Encore と同じ色のタブ、タブの帯の高さ、中クリックとタブの右クリックで閉じる、
/// 文字は日本語に引く。
fn dock_style(ui: &Ui) -> DockStyle {
    use crate::theme::{BORDER, PANEL, TAB_HEIGHT, TAB_STRIP, TEXT_STRONG, TEXT_WEAK};
    let mut style = DockStyle::from_ui(ui);
    style.tab_height = TAB_HEIGHT;
    style.gap = 4.0;
    style.background = PANEL;
    style.tab_background = TAB_STRIP;
    style.active_background = PANEL;
    style.floating_background = PANEL;
    style.text = TEXT_STRONG;
    style.inactive_text = TEXT_WEAK;
    style.border = egui::Stroke::new(1.0, BORDER);
    // Encore と同じく、ドラッグ中はポインタの下のグループの上下左右に台形の区画の枠を出し、落とす先の区画
    // （端の台形なら分割、中ならタブとして入る）を青で塗る（FilmCraft と同じ設定）。
    style.side_edge_zones = true;
    style.accent = crate::craft_timeline::style::PLAYHEAD;
    style.middle_click_closes_tabs = true;
    style.wheel_selects_tabs = true;
    style.close_others_item = true;
    style.maximize_item = true;
    style.float_label = tl!("Undock Panel").to_owned();
    style.close_label = tl!("Close Panel").to_owned();
    style.move_panel_to_label = tl!("Move Panel to Group").to_owned();
    style.panels_label = tl!("Panels").to_owned();
    style.move_group_label = tl!("Move Tab Group").to_owned();
    style.close_group_label = tl!("Close Tab Group").to_owned();
    style.minimize_label = tl!("Minimize").to_owned();
    style.expand_label = tl!("Expand").to_owned();
    style.float_group_label = tl!("Undock Tab Group").to_owned();
    style.move_group_to_label = tl!("Move Tab Group To").to_owned();
    style.close_others_label = tl!("Close Other Panels in Group").to_owned();
    style.maximize_label = tl!("Maximize Frame").to_owned();
    style.restore_frame_label = tl!("Restore Frame Size").to_owned();
    style.close_window_label = tl!("Close Panel Window").to_owned();
    style
}

/// DiscCraft の側のドッキング（1 フレームぶん）。
struct DiscContent<'a> {
    app: &'a mut DiscCraftApp,
}

impl DockContent<Panel> for DiscContent<'_> {
    fn body(&mut self, ui: &mut Ui, panel: &Panel) {
        let rect = ui.max_rect();
        let inner = rect.shrink(f32::from(crate::theme::GROUP_MARGIN));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .id_salt(("panel", *panel)),
        );
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        self.app.panel_body(&mut child, *panel);
        // 中身は枠いっぱい: フローティングにしたパネルも枠の大きさになる。
        ui.allocate_rect(rect, Sense::hover());
    }

    fn logical_label(&self, panel: &Panel) -> String {
        self.app.panel_label(*panel)
    }

    /// 前面のタブの名前に、左のつまみ（グリップ）と右の × の場所を空ける。前面のビューアのタブは、名前と × の
    /// 間に区切りと ▼ の場所も（描くのは `tab_chrome`）。後ろのタブは名前だけ。
    fn label(
        &mut self,
        _ui: &Ui,
        panel: &Panel,
        logical: &str,
        font: &egui::FontId,
        color: egui::Color32,
    ) -> egui::WidgetText {
        let mut job = egui::text::LayoutJob::default();
        let format = egui::TextFormat::simple(font.clone(), color);
        if !self.app.tab_has_chrome(*panel) {
            job.append(logical, 0.0, format);
            return job.into();
        }
        let trailing = if self.app.tab_choice_box(*panel) {
            CHOICE_SPACE + CLOSE_SPACE
        } else {
            CLOSE_SPACE
        };
        job.append(logical, GRIP_SPACE, format.clone());
        job.append(" ", trailing, format);
        job.into()
    }
}

/// 前面のビューアのタブの、Encore と同じくぼんだ箱: 名前（craft-ui が描いたもの）を囲み、その右に区切りと
/// ▼。押すと `choices` のリストが開き（今の項目に印）、選んだものを返す。
fn viewer_tab_box(
    ui: &mut Ui,
    tab: egui::Rect,
    panel: Panel,
    choices: &[(Selection, String)],
    current: Option<Selection>,
) -> Option<Selection> {
    let rect = egui::Rect::from_min_max(
        egui::pos2(
            tab.left() + TAB_TEXT_INSET + GRIP_SPACE - 6.0,
            tab.top() + 3.0,
        ),
        egui::pos2(
            tab.right() - TAB_TEXT_INSET - CLOSE_SPACE + 2.0,
            tab.bottom() - 3.0,
        ),
    );
    if rect.width() < 40.0 {
        return None;
    }
    let response = ui.interact(
        rect,
        egui::Id::unique("viewer_tab_choices").with(panel),
        Sense::click(),
    );
    let painter = ui.painter();
    // 名前の文字はそのまま見えるように、塗りはごく薄く。
    let fill = if response.hovered() {
        egui::Color32::from_white_alpha(16)
    } else {
        egui::Color32::from_white_alpha(6)
    };
    painter.rect(
        rect,
        2.0,
        fill,
        egui::Stroke::new(1.0, crate::theme::GUTTER),
        egui::StrokeKind::Inside,
    );
    let divider = rect.right() - 20.0;
    painter.vline(
        divider,
        egui::Rangef::new(rect.top() + 3.0, rect.bottom() - 3.0),
        egui::Stroke::new(1.0, crate::theme::TEXT_WEAK),
    );
    let arrow = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 10.0, rect.center().y),
        egui::vec2(8.0, 5.0),
    );
    let color = if response.hovered() {
        crate::theme::TEXT_STRONG
    } else {
        crate::theme::TEXT
    };
    painter.add(egui::Shape::convex_polygon(
        vec![arrow.left_top(), arrow.right_top(), arrow.center_bottom()],
        color,
        egui::Stroke::NONE,
    ));
    let mut chosen = None;
    let _ = egui::Popup::menu(&response).show(|ui| {
        for (selection, name) in choices {
            if ui
                .selectable_label(current == Some(*selection), name)
                .clicked()
            {
                chosen = Some(*selection);
            }
        }
    });
    chosen
}

/// パネルいっぱいに広がるスクロール領域（中身が小さいとその大きさまで縮まないように）。
fn fill_scroll(id: &str) -> egui::ScrollArea {
    egui::ScrollArea::both()
        .id_salt(id)
        .auto_shrink([false, false])
}
