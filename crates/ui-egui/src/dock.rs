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

use craft_ui::docking::{Action, DockArea, DockContent, DockEvent, DockStyle, PanelState, Target};
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
/// タブの帯の右端の、パネルのメニューのボタン（≡）の幅。
const MENU_WIDTH: f32 = 24.0;
/// パネルのメニューの ドッキングを解除 で、浮かべる窓を元の場所からずらす量（craft-ui のタブのメニューと同じ）。
const FLOAT_OFFSET: f32 = 24.0;

impl DiscCraftApp {
    /// メニューバーの下のすべてを描く。
    pub(crate) fn draw_groups(&mut self, ui: &mut Ui) {
        // Encore's Tools panel; Window ▸ Tools (Ctrl+F4) hides it.
        if self.show_tools {
            egui::Panel::top("toolbar")
                .frame(bar_frame())
                .show_separator_line(false)
                .show(ui, |ui| self.toolbar(ui));
        }

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
            .frame(egui::Frame::NONE.fill(crate::theme::GUTTER.get()))
            .show(ui, |ui| self.dock_area(ui));
    }

    /// タブグループを描き、ドラッグや × でされた並びの変更をワークスペースに当てる。
    fn dock_area(&mut self, ui: &mut Ui) {
        let style = dock_style(ui);
        let layout = self.dock.layout.clone();
        // A maximized frame that was closed, undocked or replaced by a workspace is not maximized any more.
        self.maximized_panel = self
            .maximized_panel
            .filter(|panel| layout.contains(panel) && !self.floating(*panel));
        let before = std::mem::take(&mut self.panel_bodies);
        let maximized = self.maximized_panel;
        let mut content = DiscContent {
            app: self,
            before,
            actions: Vec::new(),
            maximize: None,
        };
        let output = DockArea::new(area_id()).maximized(maximized).show_content(
            ui,
            &layout,
            &style,
            &mut content,
        );
        let DiscContent {
            actions: menu_actions,
            maximize,
            ..
        } = content;
        // A press on a tab or its strip makes that panel the one being worked in, as a press in its body does.
        if let Some(panel) = output.focused {
            self.active_panel = Some(panel);
        }
        let close = self.tab_chrome(ui);
        self.focus_outline(ui);

        let mut actions = output.actions;
        actions.extend(menu_actions);
        if let Some(panel) = close {
            actions.push(Action::Close { panel });
        }
        // フレームを最大化（タブの右クリックのメニューかパネルのメニュー）: もう一度で元の大きさ。
        for event in output.events {
            if let DockEvent::ToggleMaximize { panel } = event {
                self.toggle_maximize(panel);
            }
        }
        if let Some(panel) = maximize {
            self.toggle_maximize(panel);
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
            if self.floating(panel) {
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
                crate::theme::TEXT_STRONG.get()
            } else {
                crate::theme::TEXT.get()
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

    /// Encore と同じく、操作中のパネル（最後に押したパネル）を 1 本の線で囲む: 前面のタブの上と左右、中身の上と
    /// 左右と下（タブの下には線がない。M6.474）。ドッキングしたパネルだけ（浮かんだ窓は窓のレイヤーにある）、
    /// ドラッグ中は描かない。タブの場所は craft-ui が前のフレームで記録したもの、中身の場所はこのフレームのもの。
    fn focus_outline(&self, ui: &Ui) {
        use craft_ui::docking::Hit;
        let Some(panel) = self.active_panel else {
            return;
        };
        let ctx = ui.ctx();
        if ctx.dragged_id().is_some() || self.floating(panel) || !self.panel_front(panel) {
            return;
        }
        let Some(body) = self
            .panel_bodies
            .iter()
            .find(|(drawn, _)| *drawn == panel)
            .map(|(_, rect)| *rect)
        else {
            return;
        };
        let tab = DockArea::<Panel>::hit_rects(ctx, area_id())
            .into_iter()
            .find_map(|(hit, rect)| matches!(hit, Hit::Tab(p) if p == panel).then_some(rect));
        let painter = ui.painter();
        let half = 0.5 / painter.pixels_per_point();
        let points: Vec<egui::Pos2> = focus_path(body, tab, half)
            .into_iter()
            .map(|point| {
                egui::pos2(
                    painter.round_to_pixel_center(point.x),
                    painter.round_to_pixel_center(point.y),
                )
            })
            .collect();
        painter.add(egui::Shape::closed_line(
            points,
            egui::Stroke::new(1.0, crate::theme::FOCUS_OUTLINE),
        ));
    }

    /// タブに切り替えのリストの箱を出すか: 前面に出ているビューアで、選べる項目があるとき。
    fn tab_choice_box(&self, panel: Panel) -> bool {
        self.tab_has_chrome(panel) && !self.viewer_choices(panel).is_empty()
    }

    /// タブにつまみと × を出すか: グループの前面に出ているタブ（Encore と同じ）。フローティングの窓のタブは
    /// craft-ui の描くまま。
    fn tab_has_chrome(&self, panel: Panel) -> bool {
        self.panel_front(panel) && !self.floating(panel)
    }

    /// `panel` がフローティングの窓にあるか。
    fn floating(&self, panel: Panel) -> bool {
        self.dock
            .layout
            .floating
            .iter()
            .any(|window| window.contains(&panel))
    }

    /// フレームを最大化: `panel` のフレームを最大化する。そのパネルのフレームが最大化されていれば元の大きさに戻す。
    fn toggle_maximize(&mut self, panel: Panel) {
        self.maximized_panel = if self.maximized_panel == Some(panel) {
            None
        } else {
            Some(panel)
        };
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
                .keys()
                .map(|id| {
                    (
                        Selection::Menu(*id),
                        crate::menu_editor::menu_title(project, *id),
                    )
                })
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

    /// パネルの中身。押されたパネルを覚える（Delete キーなどの対象）。
    fn panel_body(&mut self, ui: &mut Ui, panel: Panel) {
        let area = ui.max_rect();
        if ui.input(|i| {
            i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|at| area.contains(at))
        }) {
            self.active_panel = Some(panel);
        }
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
            Panel::Progress => {
                fill_scroll("progress_tab").show(ui, |ui| self.progress_panel(ui));
            }
            Panel::Character => {
                fill_scroll("character_tab").show(ui, |ui| self.character_panel(ui));
            }
            Panel::Metadata => {
                fill_scroll("metadata_tab").show(ui, |ui| self.metadata_panel(ui));
            }
            Panel::Styles => {
                fill_scroll("styles_tab").show(ui, |ui| self.styles_panel(ui));
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
                // A later page says which page it is.
                Some(id) => crate::menu_editor::menu_title(&self.session.project, id),
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

    /// レイヤーのタブ（Encore と同じ形。layers_panel）。
    fn layers_tab(&mut self, ui: &mut Ui) {
        self.layers_panel(ui);
    }

    /// メニュービューアに出すメニュー（選択中のメニュー、なければ最後に開いたメニュー）。
    pub(crate) fn viewer_menu(&self) -> Option<MenuId> {
        if let Selection::Menu(id) = self.selection {
            return Some(id);
        }
        self.last_menu
            .filter(|id| self.session.project.menu(*id).is_some())
    }

    /// File ▸ Close: the viewer of the selected kind of item shows nothing any more (the item stays in the
    /// project; opening it again brings it back).
    pub(crate) fn close_viewer(&mut self) {
        match self.selection {
            Selection::Menu(_) => self.last_menu = None,
            Selection::Timeline(_) => self.last_timeline = None,
            Selection::Slideshow(_) => self.last_slideshow = None,
            Selection::ChapterPlaylist(_) => self.last_chapter_playlist = None,
            Selection::Asset(_) | Selection::Playlist(_) | Selection::None => return,
        }
        self.select(Selection::None);
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
    style.background = PANEL.get();
    style.tab_background = TAB_STRIP.get();
    style.active_background = PANEL.get();
    style.floating_background = PANEL.get();
    style.text = TEXT_STRONG.get();
    style.inactive_text = TEXT_WEAK.get();
    style.border = egui::Stroke::new(1.0, BORDER);
    // Encore と同じく、ドラッグ中はポインタの下のグループの上下左右に台形の区画の枠を出し、落とす先の区画
    // （端の台形なら分割、中ならタブとして入る）を青で塗る（FilmCraft と同じ設定）。
    style.side_edge_zones = true;
    style.accent = crate::craft_timeline::style::PLAYHEAD.get();
    style.middle_click_closes_tabs = true;
    style.wheel_selects_tabs = true;
    style.close_others_item = true;
    style.maximize_item = true;
    style.float_label = tl!("Undock Panel").to_owned();
    style.close_label = tl!("Close Panel").to_owned();
    style.move_panel_to_label = tl!("Move Panel to Frame").to_owned();
    style.panels_label = tl!("Panels").to_owned();
    style.move_group_label = tl!("Move Frame").to_owned();
    // Encore calls a tab group a frame (its panel menu: Undock Frame, Close Frame, Maximize Frame).
    style.close_group_label = tl!("Close Frame").to_owned();
    style.minimize_label = tl!("Minimize").to_owned();
    style.expand_label = tl!("Expand").to_owned();
    style.float_group_label = tl!("Undock Frame").to_owned();
    style.move_group_to_label = tl!("Move Frame To").to_owned();
    style.close_others_label = tl!("Close Other Panels in Frame").to_owned();
    style.maximize_label = tl!("Maximize Frame").to_owned();
    style.restore_frame_label = tl!("Restore Frame Size").to_owned();
    style.close_window_label = tl!("Close Panel Window").to_owned();
    style
}

/// DiscCraft の側のドッキング（1 フレームぶん）。
struct DiscContent<'a> {
    app: &'a mut DiscCraftApp,
    /// 前のフレームで描いた各パネルの中身の場所（ドッキングを解除 で浮かべる窓の場所と大きさ）。
    before: Vec<(Panel, egui::Rect)>,
    /// パネルのメニューで選ばれた並びの変更。
    actions: Vec<Action<Panel>>,
    /// パネルのメニューで フレームを最大化 / 元に戻す が選ばれたパネル。
    maximize: Option<Panel>,
}

impl DiscContent<'_> {
    /// Encore のパネルのメニュー（タブの帯の右端の ≡）: パネルのドッキングを解除 / フレームのドッキングを解除 /
    /// パネルを閉じる / フレームを閉じる / フレームを最大化、そのあとにパネルごとの項目（フローチャート、ライブラリ）。
    /// フローティングの窓では、ドッキングを解除 と 最大化 は選べない。Encore と同じく、どの項目の名前もチェックの
    /// 印の列の後ろから始まる。
    fn panel_menu_items(
        &mut self,
        ui: &mut Ui,
        button: egui::Rect,
        (panels, active, floating): (&[Panel], Panel, bool),
    ) {
        let item = |ui: &mut Ui, enabled: bool, label: &str| {
            let clicked = ui
                .add_enabled(
                    enabled,
                    egui::Button::new(crate::actions::indented(label.to_owned())),
                )
                .clicked();
            if clicked {
                ui.close();
            }
            clicked
        };
        let float = self.float_rect(active, button);
        if item(ui, !floating, tl!("Undock Panel")) {
            self.actions.push(Action::Float {
                panel: active,
                rect: float,
            });
        }
        if item(ui, !floating, tl!("Undock Frame")) {
            self.actions.push(Action::MoveGroup {
                panel: active,
                to: Target::Float { rect: Some(float) },
            });
        }
        if item(ui, true, tl!("Close Panel")) {
            self.actions.push(Action::Close { panel: active });
        }
        if item(ui, true, tl!("Close Frame")) {
            self.actions.push(Action::CloseGroup { panel: active });
        }
        let maximized = self
            .app
            .maximized_panel
            .filter(|panel| panels.contains(panel));
        let label = if maximized.is_some() {
            tl!("Restore Frame Size")
        } else {
            tl!("Maximize Frame")
        };
        if item(ui, !floating, label) {
            self.maximize = Some(maximized.unwrap_or(active));
        }
        if has_panel_items(active) {
            ui.separator();
            self.app.panel_items(ui, active);
        }
    }

    /// ドッキングを解除 で浮かべる窓の場所と大きさ（`[x, y, 幅, 高さ]`）: 前のフレームのパネルの中身の場所から
    /// 少しずらして同じ大きさ。中身を描いていなかった（最小化していた）ときは、メニューのボタンの下に 400×300。
    fn float_rect(&self, panel: Panel, button: egui::Rect) -> [f32; 4] {
        let rect = self
            .before
            .iter()
            .find(|(drawn, _)| *drawn == panel)
            .map(|(_, rect)| rect.translate(egui::vec2(FLOAT_OFFSET, FLOAT_OFFSET)))
            .unwrap_or_else(|| {
                egui::Rect::from_min_size(
                    egui::pos2(button.right() - 400.0, button.bottom()),
                    egui::vec2(400.0, 300.0),
                )
            });
        [rect.left(), rect.top(), rect.width(), rect.height()]
    }
}

/// パネルのメニューにパネルごとの項目があるか（Encore では フローチャート と ライブラリ。文字 と メタデータ の
/// 項目は DiscCraft にない機能なので出さない）。
fn has_panel_items(panel: Panel) -> bool {
    matches!(panel, Panel::Flowchart | Panel::Library)
}

impl DiscCraftApp {
    /// パネルのメニューとタブの右クリックのメニューの、パネルごとの項目。
    fn panel_items(&mut self, ui: &mut Ui, panel: Panel) {
        match panel {
            Panel::Flowchart => self.flow_panel_menu(ui),
            Panel::Library => self.library_menu(ui),
            _ => {}
        }
    }
}

impl DockContent<Panel> for DiscContent<'_> {
    fn body(&mut self, ui: &mut Ui, panel: &Panel) {
        let rect = ui.max_rect();
        self.app.panel_bodies.push((*panel, rect));
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

    /// タブの右クリックのメニューの頭に、パネルのメニューと同じパネルごとの項目（そのあとに craft-ui の
    /// ドッキングを解除・移動・閉じる など）。
    fn panel_menu(&mut self, ui: &mut Ui, panel: &Panel) {
        if has_panel_items(*panel) {
            self.app.panel_items(ui, *panel);
            ui.separator();
        }
    }

    /// タブの帯の右端に、パネルのメニューのボタン（Encore と同じ場所）。
    fn group_header_width(
        &self,
        _panels: &[Panel],
        _active: &Panel,
        _floating: bool,
        strip_width: f32,
    ) -> f32 {
        MENU_WIDTH.min(strip_width)
    }

    /// パネルのメニューのボタン（Craft の ≡ のアイコン）。押すと前面のパネルのメニューが開く。
    fn group_header(&mut self, ui: &mut Ui, panels: &[Panel], active: &Panel, floating: bool) {
        let header = ui.max_rect();
        let rect = egui::Rect::from_center_size(header.center(), egui::vec2(18.0, 16.0));
        let response = ui.interact(
            rect,
            egui::Id::unique("panel_menu_button").with(*active),
            Sense::click(),
        );
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tl!("Panel Menu")));
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
        if response.hovered() || open {
            ui.painter()
                .rect_filled(rect, 2.0, crate::craft_timeline::style::HOVER);
        }
        let color = if response.hovered() || open {
            crate::theme::TEXT_STRONG.get()
        } else {
            crate::theme::TEXT.get()
        };
        icons::paint(
            ui.painter(),
            egui::Rect::from_center_size(rect.center(), egui::vec2(14.0, 14.0)),
            Icon::Hamburger,
            color,
        );
        let _ = egui::Popup::menu(&response).show(|ui| {
            ui.set_min_width(180.0);
            self.panel_menu_items(ui, rect, (panels, *active, floating));
        });
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
        crate::theme::Shade::new(
            egui::Color32::from_white_alpha(16),
            egui::Color32::from_black_alpha(18),
        )
        .get()
    } else {
        crate::theme::Shade::new(
            egui::Color32::from_white_alpha(6),
            egui::Color32::from_black_alpha(7),
        )
        .get()
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

/// 操作中のパネルを囲む線の点（閉じた折れ線）: 中身（`body`）の上の行から始まり、前面のタブ（`tab`。グループの
/// 幅に収める）があればその左の辺を上がって上を通り右の辺を下り、中身の右上、右下、左下へ。点は端の画素の中心
/// （`half` は半画素）。同じ点が続くところ（タブがグループの左端にあるとき）は 1 つにまとめる。
fn focus_path(body: egui::Rect, tab: Option<egui::Rect>, half: f32) -> Vec<egui::Pos2> {
    let (left, right) = (body.left() + half, body.right() - half);
    let (top, bottom) = (body.top() + half, body.bottom() - half);
    let mut points = vec![egui::pos2(left, top)];
    if let Some(tab) = tab.filter(|tab| tab.is_positive()) {
        let tab_left = (tab.left() + half).clamp(left, right);
        let tab_right = (tab.right() - half).clamp(left, right);
        let tab_top = (tab.top() + half).min(top);
        points.extend([
            egui::pos2(tab_left, top),
            egui::pos2(tab_left, tab_top),
            egui::pos2(tab_right, tab_top),
            egui::pos2(tab_right, top),
        ]);
    }
    points.extend([
        egui::pos2(right, top),
        egui::pos2(right, bottom),
        egui::pos2(left, bottom),
    ]);
    points.dedup();
    if points.len() > 1 && points.first() == points.last() {
        let _ = points.pop();
    }
    points
}

/// パネルいっぱいに広がるスクロール領域（中身が小さいとその大きさまで縮まないように）。
fn fill_scroll(id: &str) -> egui::ScrollArea {
    egui::ScrollArea::both()
        .id_salt(id)
        .auto_shrink([false, false])
}

#[cfg(test)]
mod tests {
    use super::{DiscContent, FLOAT_OFFSET, Panel, focus_path};
    use crate::DiscCraftApp;
    use egui::{Rect, pos2};

    /// The outline of the panel being worked in runs over its front tab and round its body, as Encore's does: up
    /// the tab's sides and over it, along the body's top beside the tab, and round the rest, on the edge pixels.
    #[test]
    fn the_focus_outline_goes_over_the_front_tab() {
        let body = Rect::from_min_max(pos2(0.0, 20.0), pos2(100.0, 80.0));
        let at_left = Rect::from_min_max(pos2(0.0, 0.0), pos2(30.0, 20.0));
        assert_eq!(
            focus_path(body, Some(at_left), 0.5),
            vec![
                pos2(0.5, 20.5),
                pos2(0.5, 0.5),
                pos2(29.5, 0.5),
                pos2(29.5, 20.5),
                pos2(99.5, 20.5),
                pos2(99.5, 79.5),
                pos2(0.5, 79.5),
            ]
        );
        let in_middle = Rect::from_min_max(pos2(40.0, 0.0), pos2(60.0, 20.0));
        assert_eq!(
            focus_path(body, Some(in_middle), 0.5),
            vec![
                pos2(0.5, 20.5),
                pos2(40.5, 20.5),
                pos2(40.5, 0.5),
                pos2(59.5, 0.5),
                pos2(59.5, 20.5),
                pos2(99.5, 20.5),
                pos2(99.5, 79.5),
                pos2(0.5, 79.5),
            ]
        );
        // No tab recorded (or an empty one): round the body only.
        for tab in [None, Some(Rect::NOTHING)] {
            assert_eq!(
                focus_path(body, tab, 0.5),
                vec![
                    pos2(0.5, 20.5),
                    pos2(99.5, 20.5),
                    pos2(99.5, 79.5),
                    pos2(0.5, 79.5),
                ]
            );
        }
    }

    /// Maximize Frame (the panel menu's or the tab menu's) maximizes the frame; the same again restores it.
    #[test]
    fn maximize_frame_toggles() {
        let mut app = DiscCraftApp::new(&egui::Context::default());
        app.toggle_maximize(Panel::Library);
        assert_eq!(app.maximized_panel, Some(Panel::Library));
        app.toggle_maximize(Panel::Flowchart);
        assert_eq!(app.maximized_panel, Some(Panel::Flowchart));
        app.toggle_maximize(Panel::Flowchart);
        assert_eq!(app.maximized_panel, None);
    }

    /// Undock opens the window where the panel was, a little lower right and as large; a panel not drawn in the
    /// last frame (a minimized group) gets a 400 × 300 window under the menu button.
    #[test]
    fn an_undocked_panel_floats_beside_where_it_was() {
        let mut app = DiscCraftApp::new(&egui::Context::default());
        let body = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(300.0, 200.0));
        let content = DiscContent {
            app: &mut app,
            before: vec![(Panel::Library, body)],
            actions: Vec::new(),
            maximize: None,
        };
        let button = egui::Rect::from_min_size(egui::pos2(900.0, 10.0), egui::vec2(18.0, 16.0));
        assert_eq!(
            content.float_rect(Panel::Library, button),
            [100.0 + FLOAT_OFFSET, 50.0 + FLOAT_OFFSET, 300.0, 200.0]
        );
        assert_eq!(
            content.float_rect(Panel::Styles, button),
            [518.0, 26.0, 400.0, 300.0]
        );
    }
}
