//! 左のグループの中身: プロジェクト（すべての項目の表）、メニューの一覧、タイムラインの一覧、ビルド。
//!
//! Encore のプロジェクトパネルと同じく、上に選択中の項目の概要、その下に「名前 / 種類 / デュレーション」の表、
//! いちばん下に読み込みと新規作成のボタンを置く。表の行はピックウィップの落とし先にもなる。

use crate::DiscCraftApp;
use crate::app::Selection;
use crate::build_panel::format_bytes;
use crate::flowchart::NodeId;
use crate::i18n::{tl, tlf};
use crate::menu_editor::link_label;
use crate::timeline_view::{frame_rate, ticks_to_seconds, timecode};

use disccraft_build::OutputKind;
use disccraft_project::{AssetId, AssetKind, DiscKind, MenuId, TimelineId};
use egui::Ui;
use serde_json::{Value, json};

/// アセットの `asset.probe` の結果（アセットごとに 1 回だけ問い合わせて覚える）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ProbeInfo {
    /// 長さ（秒）。分からないときは `None`。
    pub(crate) seconds: Option<f64>,
    /// 映像の寸法（幅, 高さ）。
    pub(crate) dimensions: Option<(u32, u32)>,
    /// 映像のフレームレート。
    pub(crate) fps: Option<f64>,
    /// ファイルの大きさ（バイト）。
    pub(crate) size: Option<u64>,
    /// ファイルの最終変更日時（UNIX 秒）。
    pub(crate) modified: Option<i64>,
}

/// プロジェクトの表の列（Encore の「列の表示 / 非表示」の並び。名前は常に出る）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProjectColumn {
    Name,
    Type,
    Duration,
    Dimensions,
    DvdStatus,
    DvdSettings,
    BlurayStatus,
    BluraySettings,
    Size,
    Category,
    Description,
    Modified,
    Path,
}

impl ProjectColumn {
    /// 全列（表と「列の表示 / 非表示」の並び）。
    pub(crate) const ALL: [ProjectColumn; 13] = [
        ProjectColumn::Name,
        ProjectColumn::Type,
        ProjectColumn::Duration,
        ProjectColumn::Dimensions,
        ProjectColumn::DvdStatus,
        ProjectColumn::DvdSettings,
        ProjectColumn::BlurayStatus,
        ProjectColumn::BluraySettings,
        ProjectColumn::Size,
        ProjectColumn::Category,
        ProjectColumn::Description,
        ProjectColumn::Modified,
        ProjectColumn::Path,
    ];

    /// 見出し。
    pub(crate) fn label(self) -> &'static str {
        match self {
            ProjectColumn::Name => tl!("Name"),
            ProjectColumn::Type => tl!("Type"),
            ProjectColumn::Duration => tl!("Duration"),
            ProjectColumn::Dimensions => tl!("Dimensions"),
            ProjectColumn::DvdStatus => tl!("DVD Transcode Status"),
            ProjectColumn::DvdSettings => tl!("DVD Transcode Settings"),
            ProjectColumn::BlurayStatus => tl!("Blu-ray Transcode Status"),
            ProjectColumn::BluraySettings => tl!("Blu-ray Transcode Settings"),
            ProjectColumn::Size => tl!("Size"),
            ProjectColumn::Category => tl!("Media Category"),
            ProjectColumn::Description => tl!("Description"),
            ProjectColumn::Modified => tl!("Last Modified"),
            ProjectColumn::Path => tl!("File Path"),
        }
    }

    /// 初期状態で出す列（Encore と同じ。寸法・メディアカテゴリ・説明・ファイルパスは隠れている）。
    pub(crate) fn shown_by_default(self) -> bool {
        !matches!(
            self,
            ProjectColumn::Dimensions
                | ProjectColumn::Category
                | ProjectColumn::Description
                | ProjectColumn::Path
        )
    }

    /// `ALL` の中の位置（並べ替えの列番号）。
    fn index(self) -> usize {
        Self::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }
}

/// アセットのトランスコードステータス（`asset.transcodeStatus`）。Encore の「トランスコードステータス」の列に当たる。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TranscodeState {
    /// トランスコードできない素材（画像、字幕など）。
    NotApplicable,
    /// ディスクの形式に合っていて、そのまま使う。
    Unneeded,
    /// ビルドでトランスコードする。
    Untranscoded,
    /// 今の設定でのトランスコードがビルドのキャッシュにある。
    Transcoded,
}

impl TranscodeState {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "n/a" => Some(Self::NotApplicable),
            "unneeded" => Some(Self::Unneeded),
            "untranscoded" => Some(Self::Untranscoded),
            "transcoded" => Some(Self::Transcoded),
            _ => None,
        }
    }

    /// ステータスの列の文字。
    pub(crate) fn status_label(self) -> &'static str {
        match self {
            Self::NotApplicable => tl!("N/A"),
            Self::Unneeded => tl!("Don't Transcode"),
            Self::Untranscoded => tl!("Untranscoded"),
            Self::Transcoded => tl!("Transcoded"),
        }
    }

    /// 設定の列の文字。形式に合わない素材はビルドが自動でトランスコードする。
    pub(crate) fn setting_label(self) -> &'static str {
        match self {
            Self::NotApplicable => tl!("N/A"),
            Self::Unneeded => tl!("Don't Transcode"),
            Self::Untranscoded | Self::Transcoded => tl!("Automatic"),
        }
    }

    /// もう一方のディスクの種類での状態。容量が変わるとビットレートも変わるので、キャッシュは使えない。
    fn for_other_kind(self) -> Self {
        match self {
            Self::Transcoded => Self::Untranscoded,
            other => other,
        }
    }

    /// （DVD, Blu-ray）の列に出す状態。キャッシュがあるのは今のディスクの種類だけ。
    pub(crate) fn by_kind(self, kind: DiscKind) -> (Self, Self) {
        match kind {
            DiscKind::Dvd => (self, self.for_other_kind()),
            DiscKind::BluRay => (self.for_other_kind(), self),
        }
    }
}

/// 値がないときの表示（Encore と同じ）。
const NO_VALUE: &str = "--";

/// プロジェクトパネルに出す項目の種類（Encore の「異なる種類の項目の表示を切り替え」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProjectFilter {
    pub(crate) assets: bool,
    pub(crate) menus: bool,
    pub(crate) timelines: bool,
    pub(crate) slideshows: bool,
    pub(crate) playlists: bool,
    pub(crate) chapter_playlists: bool,
}

impl Default for ProjectFilter {
    fn default() -> Self {
        Self {
            assets: true,
            menus: true,
            timelines: true,
            slideshows: true,
            playlists: true,
            chapter_playlists: true,
        }
    }
}

impl ProjectFilter {
    /// Whether a row of this kind is shown.
    fn shows(self, selection: Selection) -> bool {
        match selection {
            Selection::Asset(_) => self.assets,
            Selection::Menu(_) => self.menus,
            Selection::Timeline(_) => self.timelines,
            Selection::Slideshow(_) => self.slideshows,
            Selection::Playlist(_) => self.playlists,
            Selection::ChapterPlaylist(_) => self.chapter_playlists,
            Selection::None => true,
        }
    }

    /// Whether every kind is shown.
    fn shows_all(self) -> bool {
        self == Self::default()
    }
}

/// タイムラインの表の 1 行。
struct TimelineRow {
    id: TimelineId,
    name: String,
    video: AssetId,
    chapters: usize,
    end_action: String,
    override_link: String,
    remote: String,
    /// 長さ（秒）。分からないときは `None`。
    seconds: Option<f64>,
}

/// プロジェクトの表の 1 行。
struct ProjectRow {
    /// 行を押したときの選択。
    selection: Selection,
    /// ピックウィップの落とし先（アセットは `None`）。
    node: Option<NodeId>,
    /// 名前。
    name: String,
    /// 種類。
    kind: &'static str,
    /// 名前の前の小さな絵。
    icon: RowIcon,
    /// デュレーション（秒）。分からないときは `None`。
    seconds: Option<f64>,
    /// 素材のフレームレート（デュレーションをその素材のタイムコードで出す。Encore と同じ）。
    fps: Option<f64>,
    /// トランスコードステータス（アセットとタイムラインの本編。ほかは `None`）。
    transcode: Option<TranscodeState>,
    /// 寸法（映像・画像のアセットは素材の、メニューとタイムラインはディスクの）。
    dimensions: Option<(u32, u32)>,
    /// ファイルの大きさ（アセットだけ）。
    size: Option<u64>,
    /// メディアのカテゴリ（アセットだけ。ほかは ""）。
    category: &'static str,
    /// 説明の 1 行目。
    description: String,
    /// ファイルパス（アセットだけ）。
    path: String,
    /// フォルダーの行ならそのフォルダー（項目の行は `None`）。
    folder: Option<u64>,
    /// 入れ子の深さ（root は 0）。
    depth: usize,
    /// ファイルの最終変更日時（UNIX 秒。アセットだけ）。
    modified: Option<i64>,
}

/// プロジェクトの表の行のドラッグ（項目かフォルダーの id）。フォルダーの行に落とすとその中へ入る。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DragRow(pub(crate) u64);

/// 選択が指す項目の id（アセット・タイムライン・メニュー・スライドショー・再生リスト）。
pub(crate) fn selection_item_id(selection: Selection) -> Option<u64> {
    match selection {
        Selection::Asset(id) => Some(id.0),
        Selection::Timeline(id) => Some(id.0),
        Selection::Menu(id) => Some(id.0),
        Selection::Slideshow(id) | Selection::Playlist(id) | Selection::ChapterPlaylist(id) => {
            Some(id)
        }
        Selection::None => None,
    }
}

/// フォルダーの名前を root からつないだもの（"写真 / 2026"）。
pub(crate) fn folder_path(project: &disccraft_project::Project, folder: u64) -> String {
    let mut parts = Vec::new();
    let mut current = Some(folder);
    for _ in 0..=project.folders.len() {
        let Some(id) = current else {
            break;
        };
        let Some(item) = project.folders.get(&id) else {
            break;
        };
        parts.push(item.name.clone());
        current = item.parent;
    }
    parts.reverse();
    parts.join(" / ")
}

impl DiscCraftApp {
    /// プロジェクトのタブ。
    pub(crate) fn project_tab(&mut self, ui: &mut Ui) {
        egui::Panel::bottom("project_tools").show(ui, |ui| self.project_tools(ui));

        self.project_summary(ui);
        ui.separator();

        let mut rows = self.project_rows();
        // Encore と同じく、見出しで並べ替える（初めは名前の昇順）。
        let sort = self.table_sort("project_grid");
        let kind = self.session.project.settings.kind;
        crate::theme::sort_rows(&mut rows, sort, |row, column| {
            project_sort_key(row, column, kind)
        });
        let filter = self.project_filter;
        rows.retain(|row| filter.shows(row.selection));
        let rows = self.folder_tree(rows);
        egui::ScrollArea::both()
            .id_salt("project_table")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.project_table(ui, &rows);
                // 表の下の何もないところ: 押すと選択を外し、右クリックで 読み込み ▸ / 新規 ▸（Encore と同じ）。
                let rest = ui.available_size().max(egui::vec2(1.0, 1.0));
                let response = ui.allocate_response(rest, egui::Sense::click());
                if response.clicked() {
                    self.select(Selection::None);
                }
                let _ = response.context_menu(|ui| self.project_background_menu(ui));
            });
    }

    /// 上の概要（選択中の項目のサムネール、名前、種類、デュレーション、場所）。
    fn project_summary(&mut self, ui: &mut Ui) {
        let texture = match self.selection {
            // メニューはメニューの絵を縮めて出す。
            Selection::Menu(id) => self.menu_thumbnail(ui.ctx(), id),
            _ => self
                .summary_asset()
                .and_then(|id| self.asset_thumbnail(ui.ctx(), id)),
        };

        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(96.0, 54.0), egui::Sense::hover());
            let visuals = ui.visuals();
            ui.painter()
                .rect_filled(rect, 2.0, visuals.extreme_bg_color);
            if let Some(texture) = &texture {
                draw_fitted(ui.painter(), rect, texture);
            }
            ui.painter().rect_stroke(
                rect,
                2.0,
                visuals.widgets.noninteractive.bg_stroke,
                egui::StrokeKind::Inside,
            );

            ui.vertical(|ui| {
                let Some(row) = self.selected_row() else {
                    ui.label(tl!("Nothing selected"));
                    return;
                };
                ui.strong(row.name.clone());

                // 種類と寸法（例: ビデオ, 1280x720）、デュレーションとフレームレート（例: 00:00:06:00 24 fps）。
                let probe = match row.selection {
                    Selection::Asset(id) => self.asset_probe(id),
                    // タイムラインはディスクに書く映像の寸法とフレームレート（素材のものではなく）。
                    Selection::Timeline(_) => {
                        let standard = self.session.project.settings.standard;
                        let (num, den) = crate::timeline_view::frame_rate(standard);
                        ProbeInfo {
                            dimensions: Some(crate::properties::frame_size(standard)),
                            fps: Some(f64::from(num) / f64::from(den.max(1))),
                            ..ProbeInfo::default()
                        }
                    }
                    _ => ProbeInfo::default(),
                };
                let kind_line = match probe.dimensions {
                    Some((width, height)) => format!("{}, {width}x{height}", row.kind),
                    None => row.kind.to_owned(),
                };
                ui.label(kind_line);

                if let Some(seconds) = row.seconds {
                    let mut line = self.duration_text_at(seconds, row.fps);
                    if let Some(fps) = probe.fps {
                        line.push(' ');
                        line.push_str(&crate::properties::fps_text(fps));
                    }
                    ui.label(line);
                }
                if let Selection::Asset(id) = row.selection
                    && let Some(asset) = self.session.project.assets.get(&id)
                {
                    ui.small(asset.path.clone());
                }
            });
        });
    }

    /// 項目の行をフォルダーの木に並べ直す: 各階層でフォルダー（名前順）とその中身、そのあとに項目。畳んだフォルダーの
    /// 中は出さない。
    fn folder_tree(&self, items: Vec<ProjectRow>) -> Vec<ProjectRow> {
        let project = &self.session.project;
        if project.folders.is_empty() {
            return items;
        }
        let mut by_folder: std::collections::HashMap<Option<u64>, Vec<ProjectRow>> =
            std::collections::HashMap::new();
        for row in items {
            let folder = selection_item_id(row.selection).and_then(|id| project.folder_of(id));
            by_folder.entry(folder).or_default().push(row);
        }
        let mut children: std::collections::HashMap<Option<u64>, Vec<(String, u64)>> =
            std::collections::HashMap::new();
        for folder in project.folders.values() {
            let parent = folder
                .parent
                .filter(|parent| project.folders.contains_key(parent));
            children
                .entry(parent)
                .or_default()
                .push((folder.name.to_lowercase(), folder.id));
        }
        for list in children.values_mut() {
            list.sort();
        }
        let mut seen = std::collections::HashSet::new();
        let mut out = self.folder_level(None, 0, &mut by_folder, &children, &mut seen);
        // どのフォルダーにも出なかった項目（輪になったフォルダーの中など）は root に。
        for (_, mut rows) in by_folder.drain() {
            for row in &mut rows {
                row.depth = 0;
            }
            out.extend(rows);
        }
        out
    }

    /// `parent` の階層: 子フォルダー（名前順。畳んでいなければその中身が続く）、そのあとに項目。
    fn folder_level(
        &self,
        parent: Option<u64>,
        depth: usize,
        by_folder: &mut std::collections::HashMap<Option<u64>, Vec<ProjectRow>>,
        children: &std::collections::HashMap<Option<u64>, Vec<(String, u64)>>,
        seen: &mut std::collections::HashSet<u64>,
    ) -> Vec<ProjectRow> {
        let project = &self.session.project;
        let mut out = Vec::new();
        if let Some(kids) = children.get(&parent) {
            for (_, id) in kids {
                // 訪れたフォルダーを覚えて、壊れたファイルの輪でも止まる。
                if !seen.insert(*id) {
                    continue;
                }
                let name = project
                    .folders
                    .get(id)
                    .map(|folder| folder.name.clone())
                    .unwrap_or_default();
                out.push(ProjectRow {
                    selection: Selection::None,
                    node: None,
                    name,
                    kind: tl!("Folder"),
                    icon: RowIcon::Folder,
                    seconds: None,
                    fps: None,
                    transcode: None,
                    dimensions: None,
                    size: None,
                    category: "",
                    description: String::new(),
                    path: String::new(),
                    folder: Some(*id),
                    depth,
                    modified: None,
                });
                if !self.collapsed_folders.contains(id) {
                    let mut inner = self.folder_level(
                        Some(*id),
                        depth.saturating_add(1),
                        by_folder,
                        children,
                        seen,
                    );
                    out.append(&mut inner);
                } else {
                    // 畳んだフォルダーの中身は出さない（root に落とさないよう取り除く）。
                    self.drop_hidden(Some(*id), by_folder, children, seen);
                }
            }
        }
        if let Some(mut rows) = by_folder.remove(&parent) {
            for row in &mut rows {
                row.depth = depth;
            }
            out.extend(rows);
        }
        out
    }

    /// 畳んだフォルダーの中の項目と子フォルダーを（出さずに）片づける。
    fn drop_hidden(
        &self,
        parent: Option<u64>,
        by_folder: &mut std::collections::HashMap<Option<u64>, Vec<ProjectRow>>,
        children: &std::collections::HashMap<Option<u64>, Vec<(String, u64)>>,
        seen: &mut std::collections::HashSet<u64>,
    ) {
        let _ = by_folder.remove(&parent);
        if let Some(kids) = children.get(&parent) {
            for (_, id) in kids {
                if seen.insert(*id) {
                    self.drop_hidden(Some(*id), by_folder, children, seen);
                }
            }
        }
    }

    /// 表の本体。
    fn project_table(&mut self, ui: &mut Ui, rows: &[ProjectRow]) {
        let kind = self.session.project.settings.kind;
        let selected = rows.iter().position(|row| {
            row.selection != Selection::None && row.selection == self.listed_selection()
        });
        let fill = crate::theme::TableFill::new(ui, "project_grid");
        let mut sort = self.table_sort("project_grid");
        let columns: Vec<ProjectColumn> = ProjectColumn::ALL
            .into_iter()
            .filter(|column| self.project_column_shown(*column))
            .collect();
        let table = crate::theme::table_selecting("project_grid", selected)
            .num_columns(columns.len().saturating_add(1))
            .min_col_width(64.0)
            .show(ui, |ui| {
                for column in &columns {
                    let response =
                        crate::theme::sort_header(ui, &mut sort, column.index(), column.label());
                    // Encore の見出しの右クリック: 表示しない / 列の表示 / 非表示 ▸。
                    let _ = response.context_menu(|ui| self.column_menu(ui, *column));
                }
                fill.cell(ui);
                ui.end_row();

                for (index, row) in rows.iter().enumerate() {
                    if let Some(folder) = row.folder {
                        self.folder_row(ui, &fill, &columns, row, folder, index);
                        continue;
                    }
                    // 行のどこを押しても選べる（名前の文字だけでなく、種類やデュレーションの列も）。
                    // Encore と同じく、名前の前に種類の小さな絵。フォルダーの中は字下げ。
                    let first = ui
                        .horizontal(|ui| {
                            ui.add_space(indent(row.depth));
                            row_glyph(ui, row.icon);
                            ui.label(&row.name);
                        })
                        .response
                        .rect;
                    let mut last = first;
                    for column in columns.iter().skip(1) {
                        last = self.project_cell(ui, row, *column, kind);
                    }
                    let id = egui::Id::unique("project_row").with(index);
                    let response =
                        crate::theme::table_row_draggable(ui, id, first, fill.row_end(first, last));
                    // 行はフォルダーへドラッグできる（Encore と同じ）。
                    if let Some(item) = selection_item_id(row.selection) {
                        response.dnd_set_drag_payload(DragRow(item));
                    }
                    // 右クリックでもその行を選ぶ（Encore と同じ）。
                    if response.clicked() || response.secondary_clicked() {
                        self.select(row.selection);
                    }
                    let _ = response.context_menu(|ui| self.object_menu_items(ui, row.selection));
                    if let Some(node) = row.node {
                        let _ = self.pick_whip_target(ui, &response, node);
                    }
                    ui.end_row();
                }
            });
        fill.store(ui, table.response.rect);
        let _ = self.table_sorts.insert("project_grid", sort);
    }

    /// フォルダーの行: ▸ / ▾ で畳む、名前（入力中なら欄）、右クリックで 新規 ▸ / 新規フォルダー / 名前の変更… / 消去。
    fn folder_row(
        &mut self,
        ui: &mut Ui,
        fill: &crate::theme::TableFill,
        columns: &[ProjectColumn],
        row: &ProjectRow,
        folder: u64,
        index: usize,
    ) {
        let collapsed = self.collapsed_folders.contains(&folder);
        let mut toggle = false;
        let mut committed_name: Option<String> = None;
        let mut cancel_rename = false;
        let first = ui
            .horizontal(|ui| {
                ui.add_space(indent(row.depth));
                let arrow = if collapsed { "▸" } else { "▾" };
                if ui
                    .add(egui::Label::new(arrow).sense(egui::Sense::click()))
                    .clicked()
                {
                    toggle = true;
                }
                row_glyph(ui, RowIcon::Folder);
                match self.renaming_folder.as_mut() {
                    Some((renaming, text)) if *renaming == folder => {
                        let response = ui.add(
                            egui::TextEdit::singleline(text)
                                .desired_width(160.0)
                                .id(egui::Id::unique("folder_rename").with(folder)),
                        );
                        // 初めの 1 回だけフォーカスを渡す（毎フレーム渡すと Enter で外れたことが分からない）。
                        if !response.has_focus() && !response.lost_focus() {
                            response.request_focus();
                        }
                        if response.lost_focus() {
                            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                cancel_rename = true;
                            } else {
                                committed_name = Some(text.clone());
                            }
                        }
                    }
                    _ => {
                        ui.label(&row.name);
                    }
                }
            })
            .response
            .rect;
        let mut last = first;
        for _ in columns.iter().skip(1) {
            last = ui.label("").rect;
        }
        if let Some(name) = committed_name {
            self.renaming_folder = None;
            if !name.trim().is_empty() && name.trim() != row.name {
                let _ = self.run_command(
                    "project.renameFolder",
                    json!({ "folder": folder, "name": name.trim() }),
                );
            }
        }
        if cancel_rename {
            self.renaming_folder = None;
        }
        let id = egui::Id::unique("project_folder_row").with(index);
        let response = crate::theme::table_row_draggable(ui, id, first, fill.row_end(first, last));
        // フォルダー自身もドラッグでほかのフォルダーへ入れられる。
        response.dnd_set_drag_payload(DragRow(folder));
        // 行をこのフォルダーに落とす。
        if response
            .dnd_hover_payload::<DragRow>()
            .is_some_and(|payload| payload.0 != folder)
        {
            let _ = ui.painter().rect_stroke(
                response.rect,
                0.0,
                ui.visuals().selection.stroke,
                egui::StrokeKind::Inside,
            );
        }
        if let Some(payload) = response.dnd_release_payload::<DragRow>()
            && payload.0 != folder
        {
            let _ = self.run_command(
                "project.moveToFolder",
                json!({ "id": payload.0, "folder": folder }),
            );
        }
        if response.double_clicked() {
            self.renaming_folder = Some((folder, row.name.clone()));
        } else if response.clicked() {
            toggle = true;
        }
        if toggle {
            if collapsed {
                self.collapsed_folders.retain(|id| *id != folder);
            } else {
                self.collapsed_folders.push(folder);
            }
        }
        let _ = response.context_menu(|ui| {
            // ここで作った項目はこのフォルダーに入る。
            self.new_item_folder = Some(folder);
            let timeline = match self.selection {
                Selection::Timeline(id) => Some(id),
                _ => None,
            };
            ui.menu_button(tl!("New"), |ui| self.new_items(ui, None, timeline));
            self.new_item_folder = None;
            ui.separator();
            if ui.button(tl!("Rename…")).clicked() {
                self.renaming_folder = Some((folder, row.name.clone()));
                ui.close();
            }
            if ui.button(tl!("Clear")).clicked() {
                let _ = self.run_command("project.removeFolder", json!({ "folder": folder }));
                ui.close();
            }
        });
        ui.end_row();
    }

    /// 名前以外の 1 セル。描いた範囲を返す（行の右端を知るため）。
    fn project_cell(
        &mut self,
        ui: &mut Ui,
        row: &ProjectRow,
        column: ProjectColumn,
        kind: DiscKind,
    ) -> egui::Rect {
        let transcode = row.transcode.map(|state| state.by_kind(kind));
        let text = match column {
            ProjectColumn::Name => row.name.clone(),
            ProjectColumn::Type => row.kind.to_owned(),
            ProjectColumn::Duration => row
                .seconds
                .map(|seconds| self.duration_text_at(seconds, row.fps))
                .unwrap_or_default(),
            ProjectColumn::Dimensions => row
                .dimensions
                .map(|(width, height)| format!("{width}x{height}"))
                .unwrap_or_default(),
            ProjectColumn::DvdStatus => transcode
                .map(|(dvd, _)| dvd.status_label().to_owned())
                .unwrap_or_default(),
            ProjectColumn::DvdSettings => transcode
                .map(|(dvd, _)| dvd.setting_label().to_owned())
                .unwrap_or_default(),
            ProjectColumn::BlurayStatus => transcode
                .map(|(_, bluray)| bluray.status_label().to_owned())
                .unwrap_or_default(),
            ProjectColumn::BluraySettings => transcode
                .map(|(_, bluray)| bluray.setting_label().to_owned())
                .unwrap_or_default(),
            ProjectColumn::Size => row
                .size
                .map(crate::build_panel::format_bytes)
                .unwrap_or_default(),
            ProjectColumn::Category => row.category.to_owned(),
            ProjectColumn::Description => row.description.clone(),
            ProjectColumn::Modified => row.modified.map(modified_text).unwrap_or_default(),
            ProjectColumn::Path => row.path.clone(),
        };
        ui.label(text).rect
    }

    /// その列を出しているか（名前は常に）。
    pub(crate) fn project_column_shown(&self, column: ProjectColumn) -> bool {
        column == ProjectColumn::Name || !self.hidden_project_columns.contains(&column.index())
    }

    /// 列を出す / 隠す。
    fn set_project_column_shown(&mut self, column: ProjectColumn, shown: bool) {
        if column == ProjectColumn::Name {
            return;
        }
        let index = column.index();
        if shown {
            self.hidden_project_columns
                .retain(|hidden| *hidden != index);
        } else if !self.hidden_project_columns.contains(&index) {
            self.hidden_project_columns.push(index);
        }
    }

    /// 見出しの右クリック: 表示しない / 列の表示 / 非表示 ▸（Encore と同じ）。
    fn column_menu(&mut self, ui: &mut Ui, column: ProjectColumn) {
        if ui
            .add_enabled(
                column != ProjectColumn::Name,
                egui::Button::new(tl!("Hide")),
            )
            .clicked()
        {
            self.set_project_column_shown(column, false);
            ui.close();
        }
        let _ = ui.menu_button(tl!("Show / Hide Columns"), |ui| {
            for candidate in ProjectColumn::ALL {
                let mut shown = self.project_column_shown(candidate);
                if ui
                    .add_enabled(
                        candidate != ProjectColumn::Name,
                        egui::Checkbox::new(&mut shown, candidate.label()),
                    )
                    .changed()
                {
                    self.set_project_column_shown(candidate, shown);
                }
            }
        });
    }

    /// 表の並べ替え（まだ押していなければ名前の昇順）。
    fn table_sort(&self, table: &'static str) -> crate::theme::TableSort {
        self.table_sorts.get(table).copied().unwrap_or_default()
    }

    /// `asset.transcodeStatus` を問い合わせ直す。ビルドの出力先が決まっていれば、そのキャッシュにある
    /// トランスコードを「トランスコード済み」にする。
    pub(crate) fn refresh_transcode(&mut self) {
        self.transcode_dirty = false;
        let path = self.build_path.trim().to_owned();
        let output = match self.build_output {
            OutputKind::Folder => "folder",
            OutputKind::Iso => "iso",
        };
        self.transcode_for = Some((path.clone(), self.build_output));
        let Some(value) = self.run_query(
            "asset.transcodeStatus",
            json!({ "output": output, "path": path }),
        ) else {
            return;
        };
        self.transcode = value
            .get("assets")
            .and_then(Value::as_array)
            .map(|assets| {
                assets
                    .iter()
                    .filter_map(|entry| {
                        let asset = entry.get("asset").and_then(Value::as_u64)?;
                        let state = entry
                            .get("status")
                            .and_then(Value::as_str)
                            .and_then(TranscodeState::parse)?;
                        Some((asset, state))
                    })
                    .collect()
            })
            .unwrap_or_default();
    }

    /// アセットのトランスコードステータス。
    pub(crate) fn transcode_state(&self, id: AssetId) -> Option<TranscodeState> {
        self.transcode.get(&id.0).copied()
    }

    /// いちばん下のボタン（読み込み、新規作成）。
    fn project_tools(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            // Encore の左下の絞り込み: 出す項目の種類を選ぶ（何か外していると印を付ける）。
            let filter_label = if self.project_filter.shows_all() {
                "▽".to_owned()
            } else {
                "▼".to_owned()
            };
            let mut filter = self.project_filter;
            let _ = egui::containers::menu::MenuButton::new(filter_label)
                .config(
                    egui::containers::menu::MenuConfig::new()
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
                )
                .ui(ui, |ui| {
                    ui.checkbox(&mut filter.assets, tl!("Assets"));
                    ui.checkbox(&mut filter.menus, tl!("Menus"));
                    ui.checkbox(&mut filter.timelines, tl!("Timelines"));
                    ui.checkbox(&mut filter.slideshows, tl!("Slideshows"));
                    ui.checkbox(&mut filter.playlists, tl!("Playlists"));
                    ui.checkbox(&mut filter.chapter_playlists, tl!("Chapter Playlists"));
                })
                .0
                .on_hover_text(tl!("Toggle the kinds of items shown"));
            self.project_filter = filter;
            if ui.button(tl!("Import…")).clicked() {
                self.import_assets();
            }
            let selection = self.selection;
            let delete = ui
                .add_enabled(selection != Selection::None, egui::Button::new(tl!("Delete")))
                .on_hover_text(tl!(
                    "Delete the selected item. Links to it become not set. An asset that is in use cannot be deleted."
                ));
            if delete.clicked() {
                self.remove_object(selection);
            }
            let asset = match self.selection {
                Selection::Asset(id) => Some(id),
                _ => None,
            };
            ui.menu_button(tl!("New"), |ui| self.new_items(ui, asset, None));
        });
    }

    /// 表の行（アセット、タイムライン、メニュー、再生リスト、スライドショーの順）。
    fn project_rows(&mut self) -> Vec<ProjectRow> {
        let mut rows = Vec::new();
        self.asset_rows(&mut rows);
        self.timeline_rows(&mut rows);

        let project = &self.session.project;
        let (frame_w, frame_h) = disccraft_project::frame_size(&project.settings);
        let frame = (u32::from(frame_w), u32::from(frame_h));
        // Encore と同じく、ページのあるメニューは 1 行（後のページは出さない）。
        for (id, menu) in project
            .menus
            .iter()
            .filter(|(id, _)| !project.is_menu_page(**id))
        {
            rows.push(ProjectRow {
                selection: Selection::Menu(*id),
                node: Some(NodeId::Menu(*id)),
                name: menu.name.clone(),
                kind: tl!("Menu"),
                icon: RowIcon::Menu,
                seconds: None,
                fps: None,
                transcode: None,
                dimensions: Some(frame),
                size: None,
                category: "",
                description: first_line(&menu.description),
                path: String::new(),
                folder: None,
                depth: 0,
                modified: None,
            });
        }
        for (id, playlist) in &project.playlists {
            rows.push(ProjectRow {
                selection: Selection::Playlist(*id),
                node: Some(NodeId::Playlist(*id)),
                name: playlist.name.clone(),
                kind: tl!("Playlist"),
                icon: RowIcon::Playlist,
                seconds: None,
                fps: None,
                transcode: None,
                dimensions: None,
                size: None,
                category: "",
                description: String::new(),
                path: String::new(),
                folder: None,
                depth: 0,
                modified: None,
            });
        }
        let chapter_playlists: Vec<(u64, String, String)> = project
            .chapter_playlists
            .values()
            .map(|list| (list.id, list.name.clone(), first_line(&list.description)))
            .collect();
        for (id, name, description) in chapter_playlists {
            let seconds = self.chapter_playlist_seconds(id);
            rows.push(ProjectRow {
                selection: Selection::ChapterPlaylist(id),
                node: Some(NodeId::ChapterPlaylist(id)),
                name,
                kind: tl!("Chapter Playlist"),
                icon: RowIcon::ChapterPlaylist,
                seconds,
                fps: None,
                transcode: None,
                dimensions: None,
                size: None,
                category: "",
                description,
                path: String::new(),
                folder: None,
                depth: 0,
                modified: None,
            });
        }
        let project = &self.session.project;
        for (id, slideshow) in &project.slideshows {
            let ticks = slideshow
                .slides
                .iter()
                .fold(0_i64, |sum, slide| sum.saturating_add(slide.duration.0));
            rows.push(ProjectRow {
                selection: Selection::Slideshow(*id),
                node: Some(NodeId::Slideshow(*id)),
                name: slideshow.name.clone(),
                kind: tl!("Slideshow"),
                icon: RowIcon::Slideshow,
                seconds: Some(ticks_to_seconds(ticks)),
                fps: None,
                transcode: None,
                dimensions: Some(frame),
                size: None,
                category: "",
                description: String::new(),
                path: String::new(),
                folder: None,
                depth: 0,
                modified: None,
            });
        }
        rows
    }

    fn asset_rows(&mut self, rows: &mut Vec<ProjectRow>) {
        let assets: Vec<(AssetId, String, AssetKind, bool, String, String)> = self
            .session
            .project
            .assets
            .iter()
            .map(|(id, asset)| {
                (
                    *id,
                    asset.name.clone(),
                    asset.kind,
                    asset.offline,
                    first_line(&asset.description),
                    asset.path.clone(),
                )
            })
            .collect();

        for (id, name, kind, offline, description, path) in assets {
            let mut name = name;
            if offline {
                name.push_str(tl!(" [offline]"));
            }
            let probe = self.asset_probe(id);
            rows.push(ProjectRow {
                selection: Selection::Asset(id),
                node: None,
                name,
                kind: asset_kind_label(kind),
                icon: RowIcon::of_asset(kind),
                seconds: probe.seconds,
                fps: probe.fps,
                transcode: self.transcode_state(id),
                dimensions: probe.dimensions,
                size: probe.size,
                category: crate::properties::media_category(kind),
                description,
                path,
                folder: None,
                depth: 0,
                modified: probe.modified,
            });
        }
    }

    fn timeline_rows(&mut self, rows: &mut Vec<ProjectRow>) {
        let timelines: Vec<(TimelineId, String, AssetId, String)> = self
            .session
            .project
            .timelines
            .iter()
            .map(|(id, timeline)| {
                (
                    *id,
                    timeline.name.clone(),
                    timeline.video,
                    first_line(&timeline.description),
                )
            })
            .collect();
        let (frame_w, frame_h) = disccraft_project::frame_size(&self.session.project.settings);
        let frame = (u32::from(frame_w), u32::from(frame_h));

        for (id, name, video, description) in timelines {
            rows.push(ProjectRow {
                selection: Selection::Timeline(id),
                node: Some(NodeId::Timeline(id)),
                name,
                kind: tl!("Timeline"),
                icon: RowIcon::Timeline,
                seconds: self.asset_seconds(video),
                fps: self.asset_probe(video).fps,
                transcode: self.transcode_state(video),
                dimensions: Some(frame),
                size: None,
                category: "",
                description,
                path: String::new(),
                folder: None,
                depth: 0,
                modified: None,
            });
        }
    }

    /// 表の行としての選択: メニューの後のページを開いているときは、そのメニュー（最初のページ）の行。
    fn listed_selection(&self) -> Selection {
        match self.selection {
            Selection::Menu(menu) => Selection::Menu(self.session.project.menu_root(menu)),
            other => other,
        }
    }

    /// 選択中の項目の行。
    fn selected_row(&mut self) -> Option<ProjectRow> {
        let selection = self.listed_selection();
        if selection == Selection::None {
            return None;
        }
        self.project_rows()
            .into_iter()
            .find(|row| row.selection == selection)
    }

    /// アセットの長さ（秒）。
    pub(crate) fn asset_seconds(&mut self, id: AssetId) -> Option<f64> {
        self.asset_probe(id).seconds
    }

    /// 概要のサムネールに使うアセット（映像か画像のアセット、またはタイムラインの本編）。
    fn summary_asset(&self) -> Option<AssetId> {
        match self.selection {
            Selection::Asset(id) => self
                .session
                .project
                .assets
                .get(&id)
                .filter(|asset| matches!(asset.kind, AssetKind::Video | AssetKind::Image))
                .map(|_| id),
            Selection::Timeline(id) => self
                .session
                .project
                .timeline(id)
                .map(|timeline| timeline.video),
            // スライドショーは 1 枚目のスライド。
            Selection::Slideshow(id) => self
                .session
                .project
                .slideshow(id)
                .and_then(|show| show.slides.first())
                .map(|slide| slide.asset),
            // チャプター再生リストはマスタータイムラインの本編。
            Selection::ChapterPlaylist(id) => self
                .session
                .project
                .chapter_playlist(id)
                .and_then(|list| self.session.project.timeline(list.timeline))
                .map(|timeline| timeline.video),
            _ => None,
        }
    }

    /// メニューの絵のサムネール（メニューが変わったときだけ描き直す）。ボタンのないメニューは `None`。
    fn menu_thumbnail(&mut self, ctx: &egui::Context, id: MenuId) -> Option<egui::TextureHandle> {
        let menu = self.session.project.menu(id)?.clone();
        let hash = self.picture_hash(&menu);
        if let Some((cached_hash, texture)) = self.menu_thumbnails.get(&id.0)
            && *cached_hash == hash
        {
            return texture.clone();
        }
        let texture = self
            .render_menu_picture(&menu)
            .ok()
            .filter(|rendered| {
                rendered.rgb.len() == usize::from(rendered.width) * usize::from(rendered.height) * 3
            })
            .map(|rendered| {
                let image = egui::ColorImage::from_rgb(
                    [usize::from(rendered.width), usize::from(rendered.height)],
                    &rendered.rgb,
                );
                ctx.load_texture(
                    format!("menu_thumbnail_{}", id.0),
                    image,
                    egui::TextureOptions::LINEAR,
                )
            });
        let _ = self.menu_thumbnails.insert(id.0, (hash, texture.clone()));
        texture
    }

    /// アセットのサムネール（`asset.thumbnail` の結果をテクスチャにして覚えておく）。
    pub(crate) fn asset_thumbnail(
        &mut self,
        ctx: &egui::Context,
        id: AssetId,
    ) -> Option<egui::TextureHandle> {
        if let Some(cached) = self.thumbnails.get(&id.0) {
            return cached.clone();
        }

        let value = self.run_query("asset.thumbnail", serde_json::json!({ "asset": id.0 }));
        let texture = value
            .as_ref()
            .and_then(|value| decode_thumbnail(ctx, id.0, value));
        let _ = self.thumbnails.insert(id.0, texture.clone());
        texture
    }

    /// アセットの `asset.probe` の結果（長さ、寸法、フレームレート）。アセットごとに覚えておく。
    pub(crate) fn asset_probe(&mut self, id: AssetId) -> ProbeInfo {
        if let Some(probe) = self.durations.get(&id.0) {
            return *probe;
        }

        let value = self.run_query("asset.probe", serde_json::json!({ "asset": id.0 }));
        let metadata = self
            .session
            .project
            .assets
            .get(&id)
            .and_then(|asset| std::fs::metadata(&asset.path).ok());
        let size = metadata.as_ref().map(std::fs::Metadata::len);
        let modified = metadata
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok());
        let get_u32 = |value: &serde_json::Value, key: &str| {
            value
                .get(key)
                .and_then(serde_json::Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
        };
        let probe = match &value {
            Some(value) => ProbeInfo {
                seconds: value
                    .get("seconds")
                    .and_then(serde_json::Value::as_f64)
                    .filter(|seconds| seconds.is_finite() && *seconds > 0.0),
                dimensions: get_u32(value, "width").zip(get_u32(value, "height")),
                fps: value
                    .get("fps")
                    .and_then(serde_json::Value::as_f64)
                    .filter(|fps| fps.is_finite() && *fps > 0.0),
                size,
                modified,
            },
            None => ProbeInfo {
                size,
                modified,
                ..ProbeInfo::default()
            },
        };
        let _ = self.durations.insert(id.0, probe);
        probe
    }

    /// 秒をタイムコード（プロジェクトのテレビ方式のフレームレート）で表す。
    pub(crate) fn duration_text(&self, seconds: f64) -> String {
        let (num, den) = frame_rate(self.session.project.settings.standard);
        timecode(seconds, num, den)
    }

    /// デュレーションを素材のフレームレートのタイムコードで（Encore と同じく、24 fps の素材は "00:00:06:00"、
    /// 29.97 の素材はドロップフレームの "00;00;06;00"）。フレームレートが分からなければディスクのもの。
    pub(crate) fn duration_text_at(&self, seconds: f64, fps: Option<f64>) -> String {
        match fps.filter(|fps| fps.is_finite() && *fps > 0.0) {
            Some(fps) => {
                let rate = disccraft_project::FrameRate::from_f64(fps).sane();
                // The nearest frame; `as` saturates.
                let frame = (seconds * rate.as_f64()).round() as i64;
                disccraft_project::format_frames(frame, rate)
            }
            None => self.duration_text(seconds),
        }
    }

    /// メニューのタブ: メニューの表と、選んだメニューのボタンの表。
    pub(crate) fn menus_tab(&mut self, ui: &mut Ui) {
        let project = &self.session.project;
        let menus: Vec<MenuRow> = project
            .menus
            .iter()
            .filter(|(id, _)| !project.is_menu_page(**id))
            .map(|(id, menu)| MenuRow {
                id: *id,
                name: menu.name.clone(),
                end_action: link_label(project, &menu.end_action),
                override_link: menu.override_link.as_ref().map_or_else(
                    || tl!("Not set").to_owned(),
                    |link| link_label(project, link),
                ),
                description: first_line(&menu.description),
                aspect: if menu.widescreen { "16:9" } else { "4:3" },
            })
            .collect();

        let mut menus = menus;
        let sort = self.table_sort("menus_grid");
        crate::theme::sort_rows(&mut menus, sort, |row, column| match column {
            0 => crate::theme::SortKey::Text(row.name.clone()),
            1 => crate::theme::SortKey::Text(row.end_action.clone()),
            2 => crate::theme::SortKey::Text(row.override_link.clone()),
            3 => crate::theme::SortKey::Text(row.aspect.to_owned()),
            _ => crate::theme::SortKey::Text(row.description.clone()),
        });
        let selected = menus.iter().position(|row| match self.selection {
            Selection::Menu(menu) => self.session.project.menu_root(menu) == row.id,
            _ => false,
        });
        // Encore と同じく、上にメニューの表、下に選んだメニューのボタンの表（境目はドラッグで動かせる）。
        let half = ui.available_height() * 0.5;
        let _ = egui::Panel::top("menus_split")
            .resizable(true)
            .default_size(half)
            .min_size(60.0)
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                egui::ScrollArea::both()
                    .id_salt("menus_table")
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.menus_table(ui, &menus, selected));
            });
        let _ = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                egui::ScrollArea::both()
                    .id_salt("menu_buttons_table")
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.menu_buttons_table(ui));
            });
    }

    fn menus_table(&mut self, ui: &mut Ui, menus: &[MenuRow], selected: Option<usize>) {
        let fill = crate::theme::TableFill::new(ui, "menus_grid");
        let mut sort = self.table_sort("menus_grid");
        // Encore と同じ 名前 / エンドアクション / 上書き / 縦横比 / 説明。
        let table = crate::theme::table_selecting("menus_grid", selected)
            .num_columns(6)
            .show(ui, |ui| {
                for (column, label) in [
                    tl!("Name"),
                    tl!("End Action"),
                    tl!("Override"),
                    tl!("Aspect ratio"),
                    tl!("Description"),
                ]
                .into_iter()
                .enumerate()
                {
                    let _ = crate::theme::sort_header(ui, &mut sort, column, label);
                }
                fill.cell(ui);
                ui.end_row();

                for row in menus {
                    let id = &row.id;
                    let first = ui.label(&row.name).rect;
                    ui.label(row.end_action.as_str());
                    ui.label(row.override_link.as_str());
                    ui.label(row.aspect);
                    let last = ui.label(row.description.as_str()).rect;
                    let row_id = egui::Id::unique("menus_row").with(id.0);
                    let response =
                        crate::theme::table_row(ui, row_id, first, fill.row_end(first, last));
                    // 右クリックでもその行を選ぶ（Encore と同じ）。
                    if response.clicked() || response.secondary_clicked() {
                        self.select(Selection::Menu(*id));
                    }
                    let _ = response
                        .context_menu(|ui| self.object_menu_items(ui, Selection::Menu(*id)));
                    let _ = self.pick_whip_target(ui, &response, NodeId::Menu(*id));
                    ui.end_row();
                }
            });
        fill.store(ui, table.response.rect);
        let _ = self.table_sorts.insert("menus_grid", sort);
    }

    fn menu_buttons_table(&mut self, ui: &mut Ui) {
        let Some(id) = self.viewer_menu() else {
            return;
        };
        let Some(menu) = self.session.project.menu(id).cloned() else {
            return;
        };

        let fill = crate::theme::TableFill::new(ui, "menu_buttons_grid");
        // 行の中身を先に作り、見出しで並べ替える（Encore と同じ。初めは番号順）。
        struct ButtonRow {
            index: usize,
            name: String,
            kind: &'static str,
            link: String,
            override_link: String,
            label: String,
        }
        let mut rows: Vec<ButtonRow> = menu
            .buttons
            .iter()
            .enumerate()
            .map(|(index, button)| ButtonRow {
                index,
                name: if button.name.is_empty() {
                    tlf!("Button {n}", n = index.saturating_add(1))
                } else {
                    button.name.clone()
                },
                kind: crate::properties::button_kind_label(button.kind),
                link: link_label(&self.session.project, &button.link),
                override_link: button.override_link.as_ref().map_or_else(
                    || tl!("Not set").to_owned(),
                    |link| link_label(&self.session.project, link),
                ),
                label: button.label.clone(),
            })
            .collect();
        let mut sort = self
            .table_sorts
            .get("menu_buttons_grid")
            .copied()
            .unwrap_or(crate::theme::TableSort {
                column: 1,
                ascending: true,
            });
        crate::theme::sort_rows(&mut rows, sort, |row, column| match column {
            0 => crate::theme::SortKey::Text(row.name.clone()),
            3 => crate::theme::SortKey::Text(row.kind.to_owned()),
            4 => crate::theme::SortKey::Text(menu.name.clone()),
            5 => crate::theme::SortKey::Text(row.link.clone()),
            6 => crate::theme::SortKey::Text(row.override_link.clone()),
            7 => crate::theme::SortKey::Text(row.label.clone()),
            // Small integers are exact.
            _ => crate::theme::SortKey::Number(row.index as f64),
        });
        let selected_row = rows
            .iter()
            .position(|row| Some(row.index) == self.primary_button());
        // Encore の 名前 / 番号 / … / メニュー / リンク / 上書き / … / ボタンテキスト のうち DiscCraft にある列。
        let page = self.session.project.menu_page_number(id);
        let menu_name = crate::menu_editor::menu_title(
            &self.session.project,
            self.session.project.menu_root(id),
        );
        let table = crate::theme::table_selecting("menu_buttons_grid", selected_row)
            .num_columns(9)
            .show(ui, |ui| {
                for (column, label) in [
                    tl!("Name"),
                    tl!("Number"),
                    tl!("Page Number"),
                    tl!("Button Kind"),
                    tl!("Menu"),
                    tl!("Link"),
                    tl!("Override"),
                    tl!("Button text"),
                ]
                .into_iter()
                .enumerate()
                {
                    let _ = crate::theme::sort_header(ui, &mut sort, column, label);
                }
                fill.cell(ui);
                ui.end_row();

                for row in &rows {
                    let index = row.index;
                    let selected =
                        self.selection == Selection::Menu(id) && self.button_selected(index);
                    // 名前の欄は選択の表示だけ（いくつも選んだときも分かるように）。押すのは行全体。
                    let first = ui.selectable_label(selected, row.name.as_str()).rect;
                    ui.label(index.saturating_add(1).to_string());
                    ui.label(page.to_string());
                    ui.label(row.kind);
                    ui.label(menu_name.as_str());
                    ui.label(row.link.as_str());
                    ui.label(row.override_link.as_str());
                    let last = ui.label(row.label.as_str()).rect;
                    let row_id = egui::Id::unique("menu_buttons_row").with(index);
                    if crate::theme::table_row(ui, row_id, first, fill.row_end(first, last))
                        .clicked()
                    {
                        let modifiers = ui.input(|i| i.modifiers);
                        if modifiers.shift || modifiers.command {
                            self.toggle_button(id, index);
                        } else {
                            self.select_only_button(id, index);
                        }
                    }
                    ui.end_row();
                }
            });
        fill.store(ui, table.response.rect);
        let _ = self.table_sorts.insert("menu_buttons_grid", sort);
    }

    /// タイムラインのタブ: タイムラインの表（Encore と同じ 名前 / エンドアクション / 上書き / デュレーション /
    /// エンコード後のサイズ / リモートメニュー の順。最後にチャプターの数）。
    pub(crate) fn timelines_tab(&mut self, ui: &mut Ui) {
        let rows: Vec<TimelineRow> = self
            .session
            .project
            .timelines
            .iter()
            .map(|(id, timeline)| {
                let project = &self.session.project;
                TimelineRow {
                    id: *id,
                    name: timeline.name.clone(),
                    video: timeline.video,
                    chapters: timeline.chapters.len(),
                    end_action: link_label(project, &timeline.end_action),
                    override_link: timeline.override_link.as_ref().map_or_else(
                        || tl!("Not set").to_owned(),
                        |link| link_label(project, link),
                    ),
                    remote: link_label(project, &timeline.menu_remote),
                    seconds: None,
                }
            })
            .collect();
        let mut rows = rows;
        for row in &mut rows {
            row.seconds = self.asset_seconds(row.video);
        }
        let sort = self.table_sort("timelines_grid");
        let usage = self.usage.clone();
        crate::theme::sort_rows(&mut rows, sort, |row, column| match column {
            0 => crate::theme::SortKey::Text(row.name.clone()),
            1 => crate::theme::SortKey::Text(row.end_action.clone()),
            2 => crate::theme::SortKey::Text(row.override_link.clone()),
            3 => row
                .seconds
                .map_or(crate::theme::SortKey::None, crate::theme::SortKey::Number),
            4 => usage
                .as_ref()
                .and_then(|usage| usage.timelines.get(&row.id.0))
                .map_or(crate::theme::SortKey::None, |bytes| {
                    crate::theme::SortKey::Number(*bytes as f64)
                }),
            5 => crate::theme::SortKey::Text(row.remote.clone()),
            _ => crate::theme::SortKey::Number(row.chapters as f64),
        });

        let selected = rows
            .iter()
            .position(|row| self.selection == Selection::Timeline(row.id));
        let fill = crate::theme::TableFill::new(ui, "timelines_grid");
        let mut sort = self.table_sort("timelines_grid");
        let table = crate::theme::table_selecting("timelines_grid", selected)
            .num_columns(8)
            .show(ui, |ui| {
                for (column, label) in [
                    tl!("Name"),
                    tl!("End Action"),
                    tl!("Override"),
                    tl!("Duration"),
                    tl!("Encoded Size"),
                    tl!("Remote Menu"),
                    tl!("Chapters"),
                ]
                .into_iter()
                .enumerate()
                {
                    let _ = crate::theme::sort_header(ui, &mut sort, column, label);
                }
                fill.cell(ui);
                ui.end_row();

                for row in &rows {
                    let selection = Selection::Timeline(row.id);
                    let first = ui.label(&row.name).rect;
                    ui.label(row.end_action.as_str());
                    ui.label(row.override_link.as_str());
                    match row.seconds {
                        Some(seconds) => {
                            let fps = self.asset_probe(row.video).fps;
                            ui.label(self.duration_text_at(seconds, fps))
                        }
                        None => ui.label(NO_VALUE),
                    };
                    let size = self
                        .usage
                        .as_ref()
                        .and_then(|usage| usage.timelines.get(&row.id.0))
                        .map_or_else(|| NO_VALUE.to_owned(), |bytes| format_bytes(*bytes));
                    ui.label(size)
                        .on_hover_text(tl!("Estimated size on the disc after the build"));
                    ui.label(row.remote.as_str());
                    let last = ui.label(row.chapters.to_string()).rect;
                    let row_id = egui::Id::unique("timelines_row").with(row.id.0);
                    let response =
                        crate::theme::table_row(ui, row_id, first, fill.row_end(first, last));
                    // 右クリックでもその行を選ぶ（Encore と同じ）。
                    if response.clicked() || response.secondary_clicked() {
                        self.select(selection);
                    }
                    let _ = response.context_menu(|ui| self.object_menu_items(ui, selection));
                    let _ = self.pick_whip_target(ui, &response, NodeId::Timeline(row.id));
                    ui.end_row();
                }
            });
        fill.store(ui, table.response.rect);
        let _ = self.table_sorts.insert("timelines_grid", sort);
    }

    /// ビルドのタブ: 出力、容量、チェック。
    pub(crate) fn build_tab(&mut self, ui: &mut Ui) {
        self.build_view(ui);
    }
}

/// `asset.thumbnail` の結果をテクスチャにする。
fn decode_thumbnail(
    ctx: &egui::Context,
    id: u64,
    value: &serde_json::Value,
) -> Option<egui::TextureHandle> {
    decode_rgb_texture(ctx, &format!("asset_thumbnail_{id}"), value)
}

/// `{"width", "height", "rgb": [u8]}`（asset.thumbnail / asset.frame の結果）をテクスチャにする。
pub(crate) fn decode_rgb_texture(
    ctx: &egui::Context,
    name: &str,
    value: &serde_json::Value,
) -> Option<egui::TextureHandle> {
    let width = usize::try_from(value.get("width")?.as_u64()?).ok()?;
    let height = usize::try_from(value.get("height")?.as_u64()?).ok()?;
    let rgb: Vec<u8> = value
        .get("rgb")?
        .as_array()?
        .iter()
        .map(|byte| byte.as_u64().and_then(|byte| u8::try_from(byte).ok()))
        .collect::<Option<Vec<u8>>>()?;
    if width == 0 || height == 0 || rgb.len() != width.checked_mul(height)?.checked_mul(3)? {
        return None;
    }

    let image = egui::ColorImage::from_rgb([width, height], &rgb);
    Some(ctx.load_texture(name, image, egui::TextureOptions::LINEAR))
}

/// `rect` に収まるように縦横比を保ってテクスチャを描く。
pub(crate) fn draw_fitted(
    painter: &egui::Painter,
    rect: egui::Rect,
    texture: &egui::TextureHandle,
) {
    let size = texture.size_vec2();
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    let scale = (rect.width() / size.x).min(rect.height() / size.y);
    let fitted = egui::Rect::from_center_size(rect.center(), size * scale);
    painter.image(
        texture.id(),
        fitted,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

/// プロジェクトの表の並べ替えの鍵（列は `ProjectColumn::ALL` の番号）。
fn project_sort_key(row: &ProjectRow, column: usize, kind: DiscKind) -> crate::theme::SortKey {
    use crate::theme::SortKey;
    let transcode = row.transcode.map(|state| state.by_kind(kind));
    let text = |text: String| {
        if text.is_empty() {
            SortKey::None
        } else {
            SortKey::Text(text)
        }
    };
    match ProjectColumn::ALL.get(column).copied() {
        Some(ProjectColumn::Type) => SortKey::Text(row.kind.to_owned()),
        Some(ProjectColumn::Duration) => row.seconds.map_or(SortKey::None, SortKey::Number),
        Some(ProjectColumn::Dimensions) => row.dimensions.map_or(SortKey::None, |(w, h)| {
            SortKey::Number(f64::from(w) * f64::from(h))
        }),
        Some(ProjectColumn::DvdStatus) => {
            transcode.map_or(SortKey::None, |(dvd, _)| SortKey::Text(format!("{dvd:?}")))
        }
        Some(ProjectColumn::DvdSettings) => transcode.map_or(SortKey::None, |(dvd, _)| {
            SortKey::Text(dvd.setting_label().to_owned())
        }),
        Some(ProjectColumn::BlurayStatus) => transcode.map_or(SortKey::None, |(_, bluray)| {
            SortKey::Text(format!("{bluray:?}"))
        }),
        Some(ProjectColumn::BluraySettings) => transcode.map_or(SortKey::None, |(_, bluray)| {
            SortKey::Text(bluray.setting_label().to_owned())
        }),
        Some(ProjectColumn::Size) => row.size.map_or(SortKey::None, |size| {
            // Sizes fit in f64 well enough for ordering.
            SortKey::Number(size as f64)
        }),
        Some(ProjectColumn::Category) => text(row.category.to_owned()),
        Some(ProjectColumn::Description) => text(row.description.clone()),
        // Seconds since 1970: exact in f64 for any file date.
        Some(ProjectColumn::Modified) => row
            .modified
            .map_or(SortKey::None, |secs| SortKey::Number(secs as f64)),
        Some(ProjectColumn::Path) => text(row.path.clone()),
        Some(ProjectColumn::Name) | None => SortKey::Text(row.name.clone()),
    }
}

/// ファイルの最終変更日時を、このコンピューターの時刻で（Encore の「最終変更日」）。
pub(crate) fn modified_text(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%Y/%m/%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// 説明の 1 行目（表のセルに出す分）。
fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().to_owned()
}

/// メニューの表の 1 行。
struct MenuRow {
    id: MenuId,
    name: String,
    end_action: String,
    /// 上書き（なければ「設定なし」）。
    override_link: String,
    /// 説明の 1 行目。
    description: String,
    /// 4:3 か 16:9。
    aspect: &'static str,
}

/// フォルダーの深さごとの字下げ（ピクセル）。
fn indent(depth: usize) -> f32 {
    // 深さは小さい整数。
    (depth.min(16) as f32) * 14.0
}

/// プロジェクトの表の行の、名前の前の小さな絵の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowIcon {
    Video,
    Audio,
    Image,
    Subtitle,
    Other,
    Timeline,
    Menu,
    Playlist,
    ChapterPlaylist,
    Slideshow,
    Folder,
}

impl RowIcon {
    fn of_asset(kind: AssetKind) -> Self {
        match kind {
            AssetKind::Video => Self::Video,
            AssetKind::Audio => Self::Audio,
            AssetKind::Image => Self::Image,
            AssetKind::Subtitle => Self::Subtitle,
            AssetKind::Other => Self::Other,
        }
    }

    /// 絵の色（種類ごとに少しずつ違う色）。
    fn color(self) -> egui::Color32 {
        match self {
            Self::Video | Self::Timeline => egui::Color32::from_rgb(120, 170, 225),
            Self::Audio => egui::Color32::from_rgb(130, 200, 140),
            Self::Image | Self::Slideshow => egui::Color32::from_rgb(220, 190, 110),
            Self::Subtitle => egui::Color32::from_rgb(190, 150, 220),
            Self::Menu => egui::Color32::from_rgb(230, 140, 100),
            Self::Playlist | Self::ChapterPlaylist => egui::Color32::from_rgb(200, 200, 200),
            Self::Folder => egui::Color32::from_rgb(225, 190, 120),
            Self::Other => egui::Color32::from_gray(150),
        }
    }
}

/// 名前の前の 14 x 12 の絵（DiscCraft の簡単な図形。種類ごとに形と色を変える）。
fn row_glyph(ui: &mut Ui, icon: RowIcon) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 12.0), egui::Sense::hover());
    let painter = ui.painter();
    let color = icon.color();
    let stroke = egui::Stroke::new(1.0, color);
    let r = rect.shrink2(egui::vec2(0.5, 1.0));
    match icon {
        // フィルムの帯: 枠と上下の穴。
        RowIcon::Video => {
            painter.rect_stroke(r, 1.0, stroke, egui::StrokeKind::Inside);
            for i in 0..3_u8 {
                let x = r.left() + 2.5 + f32::from(i) * 4.0;
                painter.vline(x, r.top()..=r.top() + 2.0, stroke);
                painter.vline(x, r.bottom() - 2.0..=r.bottom(), stroke);
            }
        }
        // 波形。
        RowIcon::Audio => {
            let heights = [3.0_f32, 7.0, 4.0, 9.0, 5.0, 2.0];
            for (i, h) in heights.iter().enumerate() {
                let x = r.left() + 1.5 + 2.0 * f32::from(u8::try_from(i).unwrap_or(0));
                let c = r.center().y;
                painter.vline(x, (c - h / 2.0)..=(c + h / 2.0), stroke);
            }
        }
        // 枠の中の山。
        RowIcon::Image => {
            painter.rect_stroke(r, 1.0, stroke, egui::StrokeKind::Inside);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(r.left() + 2.0, r.bottom() - 1.5),
                    egui::pos2(r.left() + 6.0, r.top() + 4.0),
                    egui::pos2(r.right() - 2.0, r.bottom() - 1.5),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        // 吹き出しの中の 2 行。
        RowIcon::Subtitle => {
            painter.rect_stroke(r, 2.0, stroke, egui::StrokeKind::Inside);
            painter.hline(
                (r.left() + 2.5)..=(r.right() - 2.5),
                r.center().y - 1.5,
                stroke,
            );
            painter.hline(
                (r.left() + 2.5)..=(r.right() - 5.0),
                r.center().y + 1.5,
                stroke,
            );
        }
        // 帯と区切り（タイムライン）。
        RowIcon::Timeline => {
            let band = egui::Rect::from_min_max(
                egui::pos2(r.left(), r.center().y - 2.5),
                egui::pos2(r.right(), r.center().y + 2.5),
            );
            painter.rect_filled(band, 1.0, color);
            painter.vline(r.left() + 5.0, r.top()..=r.bottom(), stroke);
        }
        // 枠の中の 3 つのボタン（メニュー）。
        RowIcon::Menu => {
            painter.rect_stroke(r, 1.0, stroke, egui::StrokeKind::Inside);
            for i in 0..3_u8 {
                let y = r.top() + 2.5 + f32::from(i) * 3.0;
                painter.hline((r.left() + 4.0)..=(r.right() - 4.0), y, stroke);
            }
        }
        // 3 行の一覧と、チャプターの印（チャプター再生リスト）。
        RowIcon::ChapterPlaylist => {
            for i in 0..3_u8 {
                let y = r.top() + 2.0 + f32::from(i) * 3.5;
                painter.hline((r.left() + 3.0)..=(r.left() + 8.0), y, stroke);
                painter.circle_filled(egui::pos2(r.left() + 1.0, y), 1.0, color);
            }
            painter.vline(r.right() - 2.0, r.top()..=r.bottom(), stroke);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(r.right() - 4.5, r.top()),
                    egui::pos2(r.right() + 0.5, r.top()),
                    egui::pos2(r.right() - 2.0, r.top() + 3.5),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        // 3 行の一覧と再生の三角（再生リスト）。
        RowIcon::Playlist => {
            for i in 0..3_u8 {
                let y = r.top() + 2.0 + f32::from(i) * 3.5;
                painter.hline(r.left()..=(r.left() + 7.0), y, stroke);
            }
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(r.right() - 4.5, r.top() + 2.0),
                    egui::pos2(r.right(), r.center().y),
                    egui::pos2(r.right() - 4.5, r.bottom() - 2.0),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        // 重なった 2 枚（スライドショー）。
        RowIcon::Slideshow => {
            let back = egui::Rect::from_min_max(r.min, r.max - egui::vec2(3.0, 3.0));
            let front = egui::Rect::from_min_max(r.min + egui::vec2(3.0, 3.0), r.max);
            painter.rect_stroke(back, 1.0, stroke, egui::StrokeKind::Inside);
            painter.rect_filled(front, 1.0, crate::theme::PANEL);
            painter.rect_stroke(front, 1.0, stroke, egui::StrokeKind::Inside);
        }
        RowIcon::Other => {
            painter.rect_stroke(r.shrink(1.5), 1.0, stroke, egui::StrokeKind::Inside);
        }
        // フォルダー: タブ付きの箱。
        RowIcon::Folder => {
            let body = egui::Rect::from_min_max(r.min + egui::vec2(0.0, 3.0), r.max);
            painter.rect_filled(body, 1.0, color.gamma_multiply(0.35));
            painter.rect_stroke(body, 1.0, stroke, egui::StrokeKind::Inside);
            let tab = egui::Rect::from_min_max(r.min, egui::pos2(r.min.x + 6.0, r.min.y + 3.0));
            painter.rect_filled(tab, 0.5, color);
        }
    }
}

pub(crate) fn asset_kind_label(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Video => tl!("Video file"),
        AssetKind::Audio => tl!("Audio file"),
        AssetKind::Image => tl!("Image file"),
        AssetKind::Subtitle => tl!("Subtitle file"),
        AssetKind::Other => tl!("Other file"),
    }
}
