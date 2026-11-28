//! 画面のパネル（[docs/ui-parity.md](../../../docs/ui-parity.md) §2）。
//!
//! メニューバーの下にツールバー、いちばん下にステータスバーを置き、その間を craft-libs の craft-ui のドッキング
//! （`DockArea`。FilmCraft などの Craft と同じもの）に任せる。craft-ui がタブグループ、タブの並べ替え・移動・分割・
//! フローティング、閉じる操作を描いて受け持ち、DiscCraft は各パネルの中身とタブの名前、グループの頭のパネルの
//! メニュー（≡）、ビューアのタブの切り替えのリストを描く。並びはワークスペース（[`crate::workspace`]）として保存する。

use crate::DiscCraftApp;
use crate::app::Selection;
use crate::i18n::{tl, tlf};
use crate::icons::{self, Icon};
use crate::theme::bar_frame;
use crate::workspace::{Panel, WorkspaceId, WorkspacePreset, permissions};

use craft_behavior::dock::{Action, Node, PanelState, Permissions, Target};
use craft_ui::contract::{Bind, BodyCx};
use craft_ui::docking::{DockConfig, DockEvent, DockPanels, PanelSpec, PanelSpecs, SourceKind};
use craft_ui::patterns::dock::DockArea;
use disccraft_project::{MenuId, TimelineId};
use egui::{Sense, Ui};

/// ビューアのタブの ▼ のリストで選んだもの（M6.539。Encore: 閉じる / すべて閉じる、区切り、そのビューアで開いた項目）。
#[derive(Clone, Copy, PartialEq)]
enum TabChoice {
    /// 閉じる: ビューアに出ている項目を閉じる。
    Close,
    /// すべて閉じる: そのビューアで開いた項目をすべて閉じる。
    CloseAll,
    /// その項目をビューアに出す（選択も移る）。
    Show(Selection),
}

/// `selection` を出すビューア（メニュー / タイムライン / スライドショー / チャプター再生リスト）。ほかは `None`。
pub(crate) fn viewer_of(selection: Selection) -> Option<Panel> {
    match selection {
        Selection::Menu(_) => Some(Panel::MenuViewer),
        Selection::Timeline(_) => Some(Panel::TimelineViewer),
        Selection::Slideshow(_) => Some(Panel::SlideshowViewer),
        Selection::ChapterPlaylist(_) => Some(Panel::ChapterPlaylist),
        Selection::Playlist(_) | Selection::Asset(_) | Selection::None => None,
    }
}

/// 前面のタブで、名前の前に空ける文字（つまみの場所。描くのは `tab_chrome`）。
const GRIP_PADDING: &str = "\u{2002}";
/// 前面のビューアのタブで、名前の後ろに空ける文字（区切りと ▼ の場所。描くのは `tab_chrome`）。
const CHOICE_PADDING: &str = "\u{2003}\u{2003}\u{2003}";
/// タブの左の端から、つまみの点の真ん中までの幅。
const GRIP_INSET: f32 = 9.0;
/// タブの左の端から、切り替えのリストの箱までの幅（つまみの右）。
const CHOICE_INSET: f32 = 14.0;
/// タブの閉じるボタン（craft-ui が描く）の場所として、箱の右に空ける幅。
const TAB_CLOSE_SPACE: f32 = 27.0;
/// グループの頭の、パネルのメニューのボタン（≡）の幅。
const MENU_WIDTH: f32 = 24.0;
/// パネルのメニューの ドッキングを解除 で、浮かべる窓を元の場所からずらす量。
const FLOAT_OFFSET: f32 = 24.0;

/// ドッキングの領域が読む各パネルの名前（タブの文字）。ビューアは開いている項目の名前、前面のビューアで切り替えの
/// リストを出すものは、その場所を後ろに空ける。中身が変わったときだけ書き換える（領域はその版で読み直す）。
#[derive(Clone, Debug, Default, PartialEq, craft_ui::contract::PlainData)]
pub(crate) struct DiscSpecs {
    /// `Panel::ALL` の順の、タブの文字。
    labels: Vec<String>,
    /// `Panel::ALL` の順の、グループの前面に出ているか（Encore と同じく、× は前面のタブにだけ出す）。
    front: Vec<bool>,
}

impl PanelSpecs<Panel> for DiscSpecs {
    fn spec(&self, panel: &Panel) -> PanelSpec {
        let label = Panel::ALL
            .iter()
            .position(|other| other == panel)
            .and_then(|at| self.labels.get(at))
            .cloned()
            .unwrap_or_else(|| panel.title().to_owned());
        // 中身は DiscCraft の egui のまま描く。浮かべた窓は、Encore と同じく OS の窓（メインの窓の外へ、ほかの
        // モニターへも出せる。M9.55）。
        PanelSpec::new(label).host_body().allow_native(true)
    }

    /// どのパネルも動かし、浮かべられる。タブの × は前面のタブにだけ出す（後ろのタブは名前だけ。Encore と同じ）。
    /// 閉じる操作そのものは DiscCraft が当てる（パネルのメニュー、ウィンドウメニュー）。
    fn permissions(&self, panel: &Panel) -> Permissions {
        let front = Panel::ALL
            .iter()
            .position(|other| other == panel)
            .and_then(|at| self.front.get(at))
            .copied()
            .unwrap_or(true);
        let mut permissions = Permissions::ALL;
        permissions.closable = front;
        permissions
    }
}

/// DiscCraft のドッキングの設定: ドラッグ中はポインタの下のグループの上下左右に台形の区画（Encore と同じく端なら
/// 分割、中ならタブとして入る）、閉じられるタブに × と中クリックで閉じる、ホイールでタブを選ぶ、タブのメニューに
/// フレームを最大化 と ほかのパネルを閉じる。
fn dock_config() -> DockConfig {
    DockConfig::default()
        .side_edge_zones(true)
        .new_dock_zones(false)
        .maximize_item(true)
        .close_others_item(true)
        .wheel_selects_tabs(true)
        .tab_close_buttons(true)
        .native_windows(true)
}

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
        // A maximized frame that was closed, undocked or replaced by a workspace is not maximized any more.
        let layout = self.dock.layout().clone();
        self.maximized_panel = self
            .maximized_panel
            .filter(|panel| layout.contains(panel) && !self.floating(*panel));
        let labels: Vec<String> = Panel::ALL
            .iter()
            .map(|panel| self.tab_label(*panel))
            .collect();
        let front: Vec<bool> = Panel::ALL
            .iter()
            .map(|panel| self.panel_front(*panel))
            .collect();
        if self.dock_specs.labels != labels || self.dock_specs.front != front {
            self.dock_specs.edit(|specs| {
                specs.labels = labels;
                specs.front = front;
            });
        }
        let specs = std::mem::take(&mut self.dock_specs);
        let before = std::mem::take(&mut self.panel_bodies);
        let before_menus = before.clone();
        let maximized = self.maximized_panel;
        let workspace = self.dock.live();
        let mut content = DiscContent {
            app: self,
            before,
            actions: Vec::new(),
            maximize: None,
            windows: Vec::new(),
        };
        let mut area = DockArea::new(
            tl!("Panels"),
            Bind::given(workspace),
            &specs,
            &mut content,
            dock_config(),
        );
        if let Some(panel) = maximized {
            area = area.maximized(panel);
        }
        let output = area.show(ui);
        let DiscContent {
            actions: menu_actions,
            maximize,
            ..
        } = content;
        self.dock_specs = specs;
        // A press in a panel, on its tab or its strip makes that panel the one being worked in.
        if let Some(panel) = output.pressed.or(output.picked) {
            self.active_panel = Some(panel);
        }
        // The docked tabs this pass drew, for the viewer's list and the outline of the panel being worked in.
        let tabs: Vec<(Panel, egui::Rect)> = output
            .sources
            .iter()
            .filter(|source| source.viewport == egui::ViewportId::ROOT)
            .filter_map(|source| match source.kind {
                SourceKind::Tab(panel) if !self.floating(panel) => Some((panel, source.rect)),
                _ => None,
            })
            .collect();
        self.tab_chrome(ui, &tabs);
        self.focus_outline(ui, &tabs);
        // Every tab of every window, floating ones too: a right click on one opens DiscCraft's menu of its panel.
        crate::framework::set_tabs(
            ui.ctx(),
            output
                .sources
                .iter()
                .filter_map(|source| match source.kind {
                    SourceKind::Tab(panel) => Some((source.viewport, panel, source.rect)),
                    _ => None,
                })
                .collect(),
        );
        // Not over a modal dialog: the tabs under it take nothing.
        if let Some((panel, at)) = crate::framework::take_tab_menu(ui.ctx(), egui::ViewportId::ROOT)
            && !self.modal_open
        {
            self.tab_menu = Some((egui::ViewportId::ROOT, panel, at, true));
        }
        // Encore と同じく、ドッキングしたグループのパネルのメニュー（≡）はタブの帯の右端: craft-ui がグループを
        // ドラッグする帯を描くところ。
        let bars: Vec<(Panel, egui::Rect)> = output
            .sources
            .iter()
            .filter(|source| source.viewport == egui::ViewportId::ROOT)
            .filter_map(|source| match source.kind {
                SourceKind::GroupBar(panel) if !self.floating(panel) => Some((panel, source.rect)),
                _ => None,
            })
            .collect();
        let mut bar_menus = DiscContent {
            app: self,
            before: before_menus,
            actions: Vec::new(),
            maximize: None,
            windows: Vec::new(),
        };
        for (panel, bar) in bars {
            bar_menus.bar_menu(ui, panel, bar);
        }
        bar_menus.tab_menu(ui);
        let DiscContent {
            actions: bar_actions,
            maximize: bar_maximize,
            ..
        } = bar_menus;

        let mut actions = output.actions;
        actions.extend(output.requested);
        actions.extend(menu_actions);
        actions.extend(bar_actions);
        // フレームを最大化（タブの右クリックのメニューかパネルのメニュー）: もう一度で元の大きさ。
        for event in output.events {
            match event {
                DockEvent::ToggleMaximize { panel, .. } => self.toggle_maximize(panel),
                // Move Panel To and Move Frame To: a tab or a frame moves by dragging it.
                DockEvent::PanelCommand { .. } => {
                    self.status = tl!("Drag the tab to move the panel").to_owned();
                }
                _ => {}
            }
        }
        if let Some(panel) = maximize.or(bar_maximize) {
            self.toggle_maximize(panel);
        }
        if let Some(error) = output.error {
            self.status = tlf!(
                "Cannot arrange the panels: {error}",
                error = format!("{:?}", error.error)
            );
        }
        if !actions.is_empty() {
            self.apply_dock(actions);
        }
        self.save_dock_when_idle(ui.ctx());
    }

    /// タブの文字: パネルの名前。前面のタブはつまみの場所を前に、前面のビューアで切り替えのリストを出すものは
    /// その場所を後ろに空けて。
    fn tab_label(&self, panel: Panel) -> String {
        let label = self.panel_label(panel);
        if !self.tab_has_chrome(panel) {
            return label;
        }
        let choice = if self.tab_choice_box(panel) {
            CHOICE_PADDING
        } else {
            ""
        };
        format!("{GRIP_PADDING}{label}{choice}")
    }

    /// Encore と同じく、グループの前面のタブの名前の左につまみ（点の並び）、前面のビューアのタブの名前は切り替えの
    /// リスト（くぼんだ箱に名前と | ▼）。タブの場所は craft-ui がこのパスで描いたもの（ドッキングされたタブ。
    /// 浮かべた窓のタブは `floating_chrome`。ドラッグ中は描かない）。
    fn tab_chrome(&mut self, ui: &mut Ui, tabs: &[(Panel, egui::Rect)]) {
        if ui.ctx().dragged_id().is_some() {
            return;
        }
        let mut chosen: Option<(Panel, TabChoice)> = None;
        let painter = ui.painter().clone();
        for (panel, rect) in tabs {
            chosen = self.chrome_on_tab(ui, &painter, *panel, *rect).or(chosen);
        }
        self.apply_tab_choice(chosen);
    }

    /// 浮かべた窓のグループの前面のタブにも、ドッキングしたものと同じつまみ・切り替えのリスト・操作中のパネルの枠
    /// （M9.58）。その窓のパスの中、パネルの中身（`body`）を描いたあと。絵は窓のレイヤーのすぐ上の層（タブより上）
    /// に描くが、その層には部品を置かない: egui はポインターの下のいちばん上の層の部品しか相手にしないので、中身や
    /// タブの操作を遮ってしまう（切り替えのリストの箱も部品を置かない。`viewer_tab_box`）。タブの場所は前のパスで
    /// 描いたもの（このパスのものは中身を描き終えるまで分からない）。ドラッグ中は描かない。
    fn floating_chrome(&mut self, ui: &mut Ui, panel: Panel, body: egui::Rect) {
        let ctx = ui.ctx().clone();
        if ctx.dragged_id().is_some() {
            return;
        }
        let Some(tab) = crate::framework::tab_rect(&ctx, panel) else {
            return;
        };
        let layer = egui::LayerId::new(
            ui.layer_id().order,
            egui::Id::unique("disccraft_floating_chrome").with(panel),
        );
        ctx.set_sublayer(ui.layer_id(), layer);
        let painter = ctx.layer_painter(layer).with_clip_rect(body.union(tab));
        let chosen = self.chrome_on_tab(ui, &painter, panel, tab);
        if self.active_panel == Some(panel) && self.panel_front(panel) {
            paint_focus(&painter, body, Some(tab));
        }
        self.apply_tab_choice(chosen);
    }

    /// `panel` のタブ（`rect`）のつまみと、ビューアなら切り替えのリストの箱（`painter` に描く。`ui` はタブのある
    /// 層）。グループの前面のタブだけ。リストで選ばれたものを返す。
    fn chrome_on_tab(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        panel: Panel,
        rect: egui::Rect,
    ) -> Option<(Panel, TabChoice)> {
        if !self.tab_has_chrome(panel) {
            return None;
        }
        // つまみ: 2 列 × 3 段の点（Encore と同じ。グループの前面のタブ）。
        let grip = egui::Rect::from_center_size(
            egui::pos2(rect.left() + GRIP_INSET, rect.center().y),
            egui::vec2(4.0, 9.0),
        );
        for column in 0..2 {
            for row in 0..3 {
                let center = egui::pos2(
                    grip.left() + column as f32 * 3.5,
                    grip.top() + row as f32 * 3.5 + 1.0,
                );
                painter.circle_filled(center, 0.9, crate::theme::TEXT);
            }
        }
        if !self.tab_choice_box(panel) {
            return None;
        }
        let choices = self.viewer_choices(panel);
        let current = self.viewer_selection(panel);
        viewer_tab_box(ui, painter, rect, panel, &choices, current).map(|choice| (panel, choice))
    }

    /// 切り替えのリストで選ばれたものを当てる。
    fn apply_tab_choice(&mut self, chosen: Option<(Panel, TabChoice)>) {
        match chosen {
            Some((_, TabChoice::Show(selection))) if selection != self.selection => {
                self.select(selection);
            }
            Some((panel, TabChoice::Close)) => self.close_in_viewer(panel),
            Some((panel, TabChoice::CloseAll)) => self.close_all_in_viewer(panel),
            Some((_, TabChoice::Show(_))) | None => {}
        }
    }

    /// Encore と同じく、操作中のパネル（最後に押したパネル）を 1 本の線で囲む: 前面のタブの上と左右、中身の上と
    /// 左右と下（タブの下には線がない。M6.474）。ドッキングしたパネル（浮かべた窓のものは `floating_chrome`）、
    /// ドラッグ中は描かない。タブと中身の場所はこのパスで描いたもの。
    fn focus_outline(&self, ui: &Ui, tabs: &[(Panel, egui::Rect)]) {
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
        let tab = tabs
            .iter()
            .find_map(|(drawn, rect)| (*drawn == panel).then_some(*rect));
        paint_focus(ui.painter(), body, tab);
    }

    /// タブに切り替えのリストの箱を出すか: 前面に出ているビューアで、選べる項目があるとき。
    fn tab_choice_box(&self, panel: Panel) -> bool {
        self.tab_has_chrome(panel) && !self.viewer_choices(panel).is_empty()
    }

    /// タブにつまみと切り替えのリストを出せるか: グループの前面に出ているタブ（Encore と同じ。ドッキングした
    /// グループも浮かべた窓のグループも）。
    fn tab_has_chrome(&self, panel: Panel) -> bool {
        self.panel_front(panel)
    }

    /// `panel` がフローティングの窓にあるか。
    fn floating(&self, panel: Panel) -> bool {
        self.dock
            .layout()
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

    /// ビューアのタブの ▼ で選べる項目: そのビューアで開いた項目（M6.539、Encore と同じ）の、選んだときの選択と
    /// 名前（プロジェクトの順）。ビューアでなければ空。
    fn viewer_choices(&self, panel: Panel) -> Vec<(Selection, String)> {
        let mut choices = self.viewer_items(panel);
        choices.retain(|(selection, _)| self.opened.contains(selection));
        choices
    }

    /// ビューア `panel` に出せる項目すべて（選択と名前、プロジェクトの順）。ビューアでなければ空。
    fn viewer_items(&self, panel: Panel) -> Vec<(Selection, String)> {
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
        self.dock.layout().panel_state(&panel) == PanelState::Front
    }

    /// `panel` を前面に出す。閉じていれば開く: 閉じる前の場所へ、なければ初期設定の配置で近いパネルの隣へ。
    pub(crate) fn show_panel(&mut self, panel: Panel) {
        if self.panel_front(panel) {
            return;
        }
        if self.dock.layout().contains(&panel) {
            self.apply_dock(vec![Action::activate(panel)]);
            return;
        }
        let fallback = panel
            .neighbours()
            .iter()
            .copied()
            .find(|other| *other != panel && self.dock.layout().contains(other));
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

    /// File ▸ Close: closes the item the viewer of the selected kind of item shows, as its tab's ▼ 閉じる does
    /// (M6.539), else the one the viewer last clicked shows. The item stays in the project; opening it again brings it
    /// back.
    pub(crate) fn close_viewer(&mut self) {
        let panel = viewer_of(self.selection).or_else(|| {
            self.active_panel
                .filter(|panel| !self.viewer_items(*panel).is_empty())
        });
        if let Some(panel) = panel {
            self.close_in_viewer(panel);
        }
    }

    /// Note `selection` as opened in its viewer (M6.539): once, in the order opened.
    pub(crate) fn note_opened(&mut self, selection: Selection) {
        if viewer_of(selection).is_some() && !self.opened.contains(&selection) {
            self.opened.push(selection);
        }
    }

    /// Close what the viewer `panel` shows (M6.539; Encore: its tab's ▼ 閉じる): it is no longer opened there, and
    /// the viewer shows the item opened there last of the others, else nothing. The item stays in the project.
    pub(crate) fn close_in_viewer(&mut self, panel: Panel) {
        let Some(current) = self.viewer_selection(panel) else {
            return;
        };
        self.opened.retain(|opened| *opened != current);
        self.show_last_opened(panel);
        if self.selection == current {
            self.select(Selection::None);
        }
    }

    /// Close everything opened in the viewer `panel` (M6.539; Encore: its tab's ▼ すべて閉じる): it shows nothing.
    pub(crate) fn close_all_in_viewer(&mut self, panel: Panel) {
        self.opened
            .retain(|opened| viewer_of(*opened) != Some(panel));
        self.show_last_opened(panel);
        if viewer_of(self.selection) == Some(panel) {
            self.select(Selection::None);
        }
    }

    /// The viewer `panel` shows the item still in the project that was opened there last, else nothing.
    fn show_last_opened(&mut self, panel: Panel) {
        let items = self.viewer_items(panel);
        let last = self.opened.iter().rev().copied().find(|opened| {
            viewer_of(*opened) == Some(panel) && items.iter().any(|(item, _)| item == opened)
        });
        match panel {
            Panel::MenuViewer => {
                self.last_menu = last.and_then(|item| match item {
                    Selection::Menu(id) => Some(id),
                    _ => None,
                });
            }
            Panel::TimelineViewer => {
                self.last_timeline = last.and_then(|item| match item {
                    Selection::Timeline(id) => Some(id),
                    _ => None,
                });
            }
            Panel::SlideshowViewer => {
                self.last_slideshow = last.and_then(|item| match item {
                    Selection::Slideshow(id) => Some(id),
                    _ => None,
                });
            }
            Panel::ChapterPlaylist => {
                self.last_chapter_playlist = last.and_then(|item| match item {
                    Selection::ChapterPlaylist(id) => Some(id),
                    _ => None,
                });
            }
            _ => {}
        }
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

/// DiscCraft の側のドッキング（1 フレームぶん）。
struct DiscContent<'a> {
    app: &'a mut DiscCraftApp,
    /// 前のフレームで描いた各パネルの中身の場所（ドッキングを解除 で浮かべる窓の場所と大きさ）。
    before: Vec<(Panel, egui::Rect)>,
    /// パネルのメニューで選ばれた並びの変更。
    actions: Vec<Action<Panel>>,
    /// パネルのメニューで フレームを最大化 / 元に戻す が選ばれたパネル。
    maximize: Option<Panel>,
    /// このパスで中身を描いた、パネルの OS の窓（M9.55）。
    windows: Vec<egui::ViewportId>,
}

impl DiscContent<'_> {
    /// ドッキングしたグループ（craft-ui は帯をグループの先頭のパネル `first` で名指す）のパネルのメニューのボタン:
    /// craft-ui の描いたグループをドラッグする帯（`bar`。タブの帯の右端）の右寄せ。メニューは前面のパネルのもの。
    fn bar_menu(&mut self, ui: &mut Ui, first: Panel, bar: egui::Rect) {
        let (_, front) = group_of(self.app.dock.layout(), first);
        let width = MENU_WIDTH.min(bar.width());
        let rect = egui::Rect::from_min_max(egui::pos2(bar.right() - width, bar.top()), bar.max);
        self.menu_button(ui, rect, front);
    }

    /// パネルを浮かべた OS の窓の、そのパスの最初に（M9.55）: メインの窓と同じく、その窓のキーは DiscCraft のもの
    /// （キャンバスの登録、フレームワークが取らなかった Esc）、そこで押したショートカットはメインの窓の次のパスで
    /// 実行し、タブの右クリックは DiscCraft のメニュー。
    fn own_window(&mut self, ui: &mut Ui) {
        crate::framework::host_window(ui);
        crate::framework::give_back_escape(ui.ctx());
        let ctx = ui.ctx().clone();
        if !self.app.modal_open {
            let pasted = crate::actions::pasted_text(&ctx);
            let pressed = crate::actions::pressed_actions(&ctx);
            if pressed.contains(&crate::actions::Action::Paste) {
                self.app.pasted_text = pasted;
            }
            self.app.queued_actions.extend(pressed);
        }
        let window = ctx.viewport_id();
        if let Some((panel, at)) = crate::framework::take_tab_menu(&ctx, window)
            && !self.app.modal_open
        {
            self.app.tab_menu = Some((window, panel, at, true));
        }
        self.tab_menu(ui);
    }

    /// タブの右クリックのメニュー（`WindowInputs` が取った右クリックで開く）: そのパネルのパネルのメニューと同じ
    /// 項目に、Encore のタブのメニューと同じく フレームのほかのパネルを閉じる。craft-ui のタブのメニューは項目の名前が
    /// 英語だけなので、DiscCraft が出す（M9.54）。
    fn tab_menu(&mut self, ui: &mut Ui) {
        let window = ui.ctx().viewport_id();
        let Some((shown_in, panel, at, opening)) = self.app.tab_menu else {
            return;
        };
        if shown_in != window {
            return;
        }
        let id = egui::Id::unique("disccraft_tab_menu").with(window);
        if opening {
            self.app.tab_menu = Some((window, panel, at, false));
        }
        let (panels, _) = group_of(self.app.dock.layout(), panel);
        let floating = self.app.floating(panel);
        let place = egui::Rect::from_min_size(at, egui::Vec2::ZERO);
        let shown = egui::Popup::new(
            id,
            ui.ctx().clone(),
            egui::PopupAnchor::Position(at),
            egui::LayerId::new(egui::Order::Foreground, id),
        )
        .kind(egui::PopupKind::Menu)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .style(egui::containers::menu::menu_style)
        .open_memory(opening.then_some(egui::SetOpenCommand::Bool(true)))
        .show(|ui| {
            ui.set_min_width(180.0);
            self.panel_menu_items(ui, place, (&panels, panel, floating), true);
        });
        if shown.is_none() && !opening {
            self.app.tab_menu = None;
        }
    }

    /// `area` の真ん中のパネルのメニューのボタン（Craft の ≡ のアイコン）。押すと前面のパネル `active` のメニューが
    /// 開く。
    fn menu_button(&mut self, ui: &mut Ui, area: egui::Rect, active: Panel) {
        let rect = egui::Rect::from_center_size(area.center(), egui::vec2(18.0, 16.0));
        let response = ui.interact(
            rect,
            egui::Id::unique("panel_menu_button").with(active),
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
        let (panels, _) = group_of(self.app.dock.layout(), active);
        let floating = self.app.floating(active);
        let _ = egui::Popup::menu(&response).show(|ui| {
            ui.set_min_width(180.0);
            self.panel_menu_items(ui, rect, (&panels, active, floating), false);
        });
    }

    /// Encore のパネルのメニュー（タブの帯の右端の ≡）: パネルのドッキングを解除 / フレームのドッキングを解除 /
    /// パネルを閉じる / フレームを閉じる / フレームを最大化、そのあとにパネルごとの項目（フローチャート、ライブラリ）。
    /// フローティングの窓では、ドッキングを解除 と 最大化 は選べない。Encore と同じく、どの項目の名前もチェックの
    /// 印の列の後ろから始まる。
    fn panel_menu_items(
        &mut self,
        ui: &mut Ui,
        button: egui::Rect,
        (panels, active, floating): (&[Panel], Panel, bool),
        tab: bool,
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
            self.actions.push(Action::float(active, float));
        }
        if item(ui, !floating, tl!("Undock Frame")) {
            self.actions
                .push(Action::move_group(active, Target::float(float)));
        }
        if item(ui, true, tl!("Close Panel")) {
            self.actions.push(Action::close(active));
        }
        if item(ui, true, tl!("Close Frame")) {
            self.actions.push(Action::close_group(active));
        }
        // The tab's menu: the frame's other panels close, as Encore's tab menu does.
        if tab && item(ui, panels.len() > 1, tl!("Close Other Panels in Frame")) {
            self.actions.push(Action::close_others(active));
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

    /// ドッキングを解除 で浮かべる窓の場所と大きさ: 前のフレームのパネルの中身の場所から少しずらして同じ大きさ。
    /// 中身を描いていなかった（最小化していた）ときは、メニューのボタンの下に 400×300。
    fn float_rect(&self, panel: Panel, button: egui::Rect) -> egui::Rect {
        self.before
            .iter()
            .find(|(drawn, _)| *drawn == panel)
            .map(|(_, rect)| rect.translate(egui::vec2(FLOAT_OFFSET, FLOAT_OFFSET)))
            .unwrap_or_else(|| {
                egui::Rect::from_min_size(
                    egui::pos2(button.left(), button.bottom()),
                    egui::vec2(400.0, 300.0),
                )
            })
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

impl DockPanels<Panel> for DiscContent<'_> {
    fn body(&mut self, panel: &Panel, ui: &mut Ui, _cx: &mut BodyCx<'_>) {
        let window = ui.ctx().viewport_id();
        if window != egui::ViewportId::ROOT && !self.windows.contains(&window) {
            self.windows.push(window);
            self.own_window(ui);
        }
        if window != egui::ViewportId::ROOT && self.app.modal_open {
            ui.disable();
        }
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
        if self.app.floating(*panel) {
            self.app.floating_chrome(ui, *panel, rect);
        }
    }

    /// 浮かべた窓のグループは、頭（タブの帯の前）にパネルのメニューのボタン（≡）。ドッキングしたグループのボタンは
    /// タブの帯の右端（`bar_menu`）。
    fn group_header(&mut self, panels: &[Panel], active: usize, ui: &mut Ui, _cx: &mut BodyCx<'_>) {
        let Some(active) = panels.get(active).copied() else {
            return;
        };
        if !self.app.floating(active) {
            return;
        }
        let height = ui.available_height().max(crate::theme::TAB_HEIGHT);
        let (header, _) = ui.allocate_exact_size(egui::vec2(MENU_WIDTH, height), Sense::hover());
        self.menu_button(ui, header, active);
    }
}

/// `panel` のいるタブグループのパネルと、前面に出ているパネル（ドッキングした木、ドックの列、浮かべた窓）。
/// 見つからなければ `panel` だけ。
fn group_of(layout: &crate::workspace::DockLayout, panel: Panel) -> (Vec<Panel>, Panel) {
    let front = |panels: &[Panel], active: usize| panels.get(active).copied().unwrap_or(panel);
    fn find(node: &Node<Panel>, panel: Panel) -> Option<(Vec<Panel>, usize)> {
        match node {
            Node::Tabs { panels, active, .. } => {
                panels.contains(&panel).then(|| (panels.clone(), *active))
            }
            Node::Split { first, second, .. } => find(first, panel).or_else(|| find(second, panel)),
            Node::Dock { columns, .. } => columns
                .iter()
                .flat_map(|column| column.groups.iter())
                .find(|group| group.panels.contains(&panel))
                .map(|group| (group.panels.clone(), group.active)),
            _ => None,
        }
    }
    layout
        .root
        .as_ref()
        .and_then(|root| find(root, panel))
        .or_else(|| {
            layout
                .floating
                .iter()
                .flat_map(|window| window.groups().iter())
                .find(|group| group.panels.contains(&panel))
                .map(|group| (group.panels.clone(), group.active))
        })
        .map(|(panels, active)| {
            let shown = front(&panels, active);
            (panels, shown)
        })
        .unwrap_or_else(|| (vec![panel], panel))
}

/// 前面のビューアのタブの、Encore と同じくぼんだ箱: 名前（craft-ui が描いたもの）を囲み、その右に区切りと
/// ▼。クリックするとリストが開き（Encore と同じく 閉じる / すべて閉じる、区切り、`choices`。今の項目に印。
/// M6.539）、選んだものを返す。箱は `painter` に描く。M9.60: 箱は押しを取らない（部品を置かない）: craft-ui は
/// ポインターの下でいちばん上の部品に押しを渡すので、箱が押しを取ると下のタブがドラッグを始められなかった。押しは
/// いつもタブのもので、箱の上から始めたドラッグもタブを動かし、箱の中のクリック（動かさずに離した押し）だけが
/// リストを開け閉めする。ほかの層（浮かんだ窓、ダイアログ）が箱に重なっているところは箱のものではない。
fn viewer_tab_box(
    ui: &mut Ui,
    painter: &egui::Painter,
    tab: egui::Rect,
    panel: Panel,
    choices: &[(Selection, String)],
    current: Option<Selection>,
) -> Option<TabChoice> {
    let rect = egui::Rect::from_min_max(
        egui::pos2(tab.left() + CHOICE_INSET, tab.top() + 3.0),
        egui::pos2(tab.right() - TAB_CLOSE_SPACE, tab.bottom() - 3.0),
    );
    if rect.width() < 40.0 {
        return None;
    }
    let ctx = ui.ctx().clone();
    let id = egui::Id::unique("viewer_tab_choices").with(panel);
    let was_open = egui::Popup::is_id_open(&ctx, id);
    let layer = ui.layer_id();
    let (over, clicked) = ctx.input(|input| {
        let at = input
            .pointer
            .hover_pos()
            .or(input.pointer.interact_pos())
            .filter(|at| rect.contains(*at));
        (at, input.pointer.primary_clicked())
    });
    let over = over.is_some_and(|at| ctx.layer_id_at(at).is_none_or(|top| top == layer))
        && ctx.dragged_id().is_none();
    let opening = over && clicked && !was_open;
    let hovered = over || was_open;
    // 名前の文字はそのまま見えるように、塗りはごく薄く。
    let fill = if hovered {
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
    let color = if hovered {
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
    let _ = egui::Popup::new(id, ctx, rect, layer)
        .open_memory(opening.then_some(egui::SetOpenCommand::Bool(true)))
        .kind(egui::PopupKind::Menu)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .style(egui::containers::menu::menu_style)
        .gap(0.0)
        .show(|ui| {
            if ui.button(tl!("Close")).clicked() {
                chosen = Some(TabChoice::Close);
            }
            if ui.button(tl!("Close All")).clicked() {
                chosen = Some(TabChoice::CloseAll);
            }
            let _ = ui.separator();
            for (selection, name) in choices {
                if ui
                    .selectable_label(current == Some(*selection), name)
                    .clicked()
                {
                    chosen = Some(TabChoice::Show(*selection));
                }
            }
        });
    chosen
}

/// 操作中のパネルを囲む 1 本の線（`focus_path`）を、端の画素の中心に描く。
fn paint_focus(painter: &egui::Painter, body: egui::Rect, tab: Option<egui::Rect>) {
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

    /// M6.539: a viewer's tab lists what was opened in it (Encore: its ▼ shows 閉じる, すべて閉じる and the menus open
    /// there); closing one shows the one opened before it, closing all empties it.
    #[test]
    fn a_viewer_lists_and_closes_what_was_opened_in_it() -> Result<(), Box<dyn std::error::Error>> {
        use crate::app::Selection;
        let mut app = DiscCraftApp::new(&egui::Context::default());
        let first = app.session.project.add_menu(Some("A".to_owned()))?;
        let second = app.session.project.add_menu(Some("B".to_owned()))?;
        let third = app.session.project.add_menu(Some("C".to_owned()))?;
        let names = |app: &DiscCraftApp| -> Vec<String> {
            app.viewer_choices(Panel::MenuViewer)
                .into_iter()
                .map(|(_, name)| name)
                .collect()
        };
        // Nothing opened: no list (the tab shows only its name).
        assert!(names(&app).is_empty());
        app.select(Selection::Menu(second));
        app.select(Selection::Menu(first));
        // The menus opened, in the project's order; the third was never opened.
        assert_eq!(names(&app), ["A", "B"]);
        // 閉じる: the viewer shows the menu opened before it, and nothing is selected.
        app.close_in_viewer(Panel::MenuViewer);
        assert_eq!(names(&app), ["B"]);
        assert_eq!(app.viewer_menu(), Some(second));
        assert!(app.selection == Selection::None);
        // File ▸ Close does the same for the selected menu's viewer.
        app.select(Selection::Menu(third));
        app.close_viewer();
        assert_eq!(app.viewer_menu(), Some(second));
        // すべて閉じる: nothing opened, the viewer is empty; the menus stay in the project.
        app.select(Selection::Menu(third));
        app.close_all_in_viewer(Panel::MenuViewer);
        assert!(names(&app).is_empty());
        assert_eq!(app.viewer_menu(), None);
        assert_eq!(app.session.project.menus.len(), 3);
        Ok(())
    }

    /// M9.54: craft-ui names a group's drag bar by the group's first panel; the panel menu over it is the menu of the
    /// panel the group shows (Undock Panel undocks that one).
    #[test]
    fn a_groups_menu_is_its_front_panels() {
        let mut dock = crate::workspace::WorkspacePreset::Standard.dock();
        assert!(
            dock.apply_all(
                [craft_behavior::dock::Action::activate(Panel::Layers)],
                crate::workspace::permissions
            )
            .is_ok()
        );
        let (panels, front) = super::group_of(dock.layout(), Panel::Library);
        assert_eq!(panels, [Panel::Library, Panel::Styles, Panel::Layers]);
        assert_eq!(front, Panel::Layers);
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
            windows: Vec::new(),
        };
        let button = egui::Rect::from_min_size(egui::pos2(900.0, 10.0), egui::vec2(18.0, 16.0));
        assert_eq!(
            content.float_rect(Panel::Library, button),
            egui::Rect::from_min_size(
                egui::pos2(100.0 + FLOAT_OFFSET, 50.0 + FLOAT_OFFSET),
                egui::vec2(300.0, 200.0)
            )
        );
        assert_eq!(
            content.float_rect(Panel::Styles, button),
            egui::Rect::from_min_size(egui::pos2(900.0, 26.0), egui::vec2(400.0, 300.0))
        );
    }

    /// M9.60: the viewer tab's box takes no press, so the tab under it keeps them all: a click in the box (a press and
    /// its release without a drag) opens its list and another closes it, a drag that starts in the box (the tab's
    /// gesture) opens nothing, and the box registers no widget of its own for egui to give the press to.
    #[test]
    fn the_viewer_box_opens_on_a_click_and_leaves_drags_to_the_tab() {
        let ctx = egui::Context::default();
        let tab = Rect::from_min_size(pos2(100.0, 20.0), egui::vec2(200.0, 24.0));
        let inside = pos2(160.0, 32.0);
        let id = egui::Id::unique("viewer_tab_choices").with(Panel::TimelineViewer);
        let frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    pos2(0.0, 0.0),
                    egui::vec2(800.0, 600.0),
                )),
                events,
                ..egui::RawInput::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let painter = ui.painter().clone();
                let _ = super::viewer_tab_box(ui, &painter, tab, Panel::TimelineViewer, &[], None);
            });
            output.textures_delta.clear();
        };
        let button = |at: egui::Pos2, pressed: bool| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let click = |at: egui::Pos2| {
            frame(vec![egui::Event::PointerMoved(at)]);
            frame(vec![button(at, true)]);
            frame(vec![button(at, false)]);
            frame(Vec::new());
        };
        click(inside);
        assert!(egui::Popup::is_id_open(&ctx, id), "a click opens the list");
        let registered = ctx.viewport(|viewport| {
            viewport
                .prev_pass
                .widgets
                .layers()
                .flat_map(|(_, widgets)| widgets.iter())
                .any(|widget| widget.interact_rect.contains(inside) && widget.sense.senses_click())
        });
        assert!(!registered, "the box takes no press");
        click(inside);
        assert!(
            !egui::Popup::is_id_open(&ctx, id),
            "another click closes it"
        );

        // A drag from the box to the right: the tab's, not a click.
        frame(vec![egui::Event::PointerMoved(inside)]);
        frame(vec![button(inside, true)]);
        for step in 1..=5 {
            frame(vec![egui::Event::PointerMoved(
                inside + egui::vec2(20.0 * step as f32, 0.0),
            )]);
        }
        frame(vec![button(inside + egui::vec2(100.0, 0.0), false)]);
        frame(Vec::new());
        assert!(!egui::Popup::is_id_open(&ctx, id), "a drag opens nothing");
    }

    /// M9.58: a floating panel's front tab gets the grip a docked one has.
    #[test]
    fn a_floating_tab_has_its_grip() {
        let ctx = egui::Context::default();
        crate::framework::install_headless(&ctx);
        let mut app = DiscCraftApp::new(&ctx);
        app.dock = crate::workspace::WorkspacePreset::Standard.dock();
        app.welcome.open = false;
        app.apply_dock(vec![super::Action::float(
            Panel::Library,
            Rect::from_min_size(pos2(300.0, 300.0), egui::vec2(400.0, 300.0)),
        )]);
        let mut shapes = Vec::new();
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    pos2(0.0, 0.0),
                    egui::vec2(1600.0, 1000.0),
                )),
                ..egui::RawInput::default()
            };
            app.framework_input(&ctx);
            let mut output = ctx.run_ui(input, |ui| app.ui(ui));
            output.textures_delta.clear();
            shapes = output.shapes;
        }
        assert!(app.floating(Panel::Library), "the panel floats");
        let Some(tab) = crate::framework::tab_rect(&ctx, Panel::Library) else {
            panic!("the floating tab was not drawn");
        };
        let grip = pos2(tab.left() + super::GRIP_INSET, tab.center().y);
        let dots = shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::Circle(circle) => (circle.center - grip).length() < 6.0,
                _ => false,
            })
            .count();
        assert_eq!(dots, 6, "the grip's dots on the floating tab");
    }
}
