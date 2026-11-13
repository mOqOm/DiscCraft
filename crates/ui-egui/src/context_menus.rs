//! 右クリックメニュー（Encore と同じ項目の並び。[docs/ui-parity.md](../../../docs/ui-parity.md) §3.5）。
//!
//! プロジェクトの表の行とフローチャートのノードには「開く / 名前の変更… / ファーストプレイに設定」、
//! タイムラインには「ここにチャプターポイントを追加 / チャプターポイントを削除 /
//! 一定間隔でチャプターポイントを追加…」を出す。

use crate::DiscCraftApp;
use crate::app::Selection;
use crate::flowchart::{NodeId, link_to};
use crate::i18n::{tl, tlf};

use disccraft_project::TimelineId;
use egui::{Context, Id, Modal, Ui};

/// 「一定間隔でチャプターポイントを追加」ダイアログの状態。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ChapterInterval {
    /// 対象のタイムライン。
    pub(crate) timeline: TimelineId,
    /// 間隔（分）。
    pub(crate) minutes: f64,
    /// タイムラインの長さ（秒）。
    pub(crate) duration: f64,
}

impl DiscCraftApp {
    /// 項目（アセット、タイムライン、メニュー、スライドショー、再生リスト）の右クリックメニューの中身。
    pub(crate) fn object_menu_items(&mut self, ui: &mut Ui, selection: Selection) {
        if ui.button(tl!("Open")).clicked() {
            self.select(selection);
            ui.close();
        }
        // アセットは作ったアプリで開ける（Encore の「オリジナルを編集」）と、ファイルの場所を開ける
        // （「エクスプローラーで表示」）。
        if let Selection::Asset(id) = selection {
            if crate::actions::menu_item(ui, crate::actions::Action::EditOriginal, true) {
                self.select(selection);
                self.edit_original(id);
                ui.close();
            }
            let path = self
                .session
                .project
                .assets
                .get(&id)
                .map(|asset| asset.path.clone());
            if ui
                .add_enabled(path.is_some(), egui::Button::new(tl!("Show in Explorer")))
                .clicked()
            {
                if let Some(path) = path {
                    self.reveal_in_file_manager(std::path::Path::new(&path));
                }
                ui.close();
            }
            // Encore: アセットを置き換え… / アセットを検索…（検索はオフラインのときだけ）。
            ui.separator();
            if ui.button(tl!("Replace Asset…")).clicked() {
                self.relink_asset(id);
                ui.close();
            }
            let offline = self
                .session
                .project
                .assets
                .get(&id)
                .is_some_and(|asset| asset.offline);
            if ui
                .add_enabled(offline, egui::Button::new(tl!("Locate Asset…")))
                .clicked()
            {
                self.relink_asset(id);
                ui.close();
            }
        }
        if ui.button(tl!("Rename…")).clicked() {
            self.select(selection);
            self.start_rename();
            ui.close();
        }
        if let Selection::Menu(id) = selection {
            ui.separator();
            if crate::actions::menu_item(ui, crate::actions::Action::EditMenuInPhotoCraft, true) {
                self.edit_menu_in_photocraft(id);
                ui.close();
            }
        }

        ui.separator();
        let asset = match selection {
            Selection::Asset(id) => Some(id),
            _ => None,
        };
        let timeline = match selection {
            Selection::Timeline(id) => Some(id),
            _ => None,
        };
        ui.menu_button(tl!("New"), |ui| self.new_items(ui, asset, timeline));
        self.move_to_folder_items(ui, selection);

        // Encore と同じく、ファーストプレイの項目は「ファーストプレイを解除」になる。
        ui.separator();
        self.first_play_items(ui, selection);

        ui.separator();
        if ui.button(tl!("Clear")).clicked() {
            self.remove_object(selection);
            ui.close();
        }
        // エンドアクションのある項目には「リンクの指定…」（Ctrl+L のダイアログ）。
        if matches!(
            selection,
            Selection::Menu(_)
                | Selection::Timeline(_)
                | Selection::Slideshow(_)
                | Selection::Playlist(_)
                | Selection::ChapterPlaylist(_)
        ) {
            ui.separator();
            if crate::actions::menu_item(ui, crate::actions::Action::SpecifyLink, true) {
                self.select(selection);
                self.open_link_dialog();
                ui.close();
            }
        }
        // Encore と同じく最後に「ここからプレビューする」（ビルド結果をその項目から）。
        if matches!(
            selection,
            Selection::Menu(_)
                | Selection::Timeline(_)
                | Selection::Slideshow(_)
                | Selection::ChapterPlaylist(_)
        ) {
            ui.separator();
            if ui.button(tl!("Preview from Here")).clicked() {
                self.preview_from(selection);
                ui.close();
            }
        }
    }

    /// The right-click menu of the flowchart's background (Encore: すべてのサムネールを表示 / 新規 ▸ / 読み込み ▸ /
    /// プレビュー).
    pub(crate) fn flow_background_menu(&mut self, ui: &mut Ui) {
        if crate::actions::menu_check(
            ui,
            crate::actions::Action::ShowAllThumbnails,
            self.flow_thumbnails,
        ) {
            self.flow_thumbnails = !self.flow_thumbnails;
            ui.close();
        }
        ui.separator();
        ui.menu_button(tl!("New"), |ui| self.new_items(ui, None, None));
        ui.menu_button(tl!("Import"), |ui| self.import_items(ui));
        ui.separator();
        if crate::actions::menu_item(ui, crate::actions::Action::Preview, true) {
            self.run_action(ui, crate::actions::Action::Preview);
            ui.close();
        }
    }

    /// The right-click menu of a flowchart node (Encore: 開く / ボタンを表示 / メニューを Photoshop で編集 /
    /// リンクの指定… / ここからプレビューする). The disc node only gets リンクの指定 (the first play).
    pub(crate) fn flow_node_menu(&mut self, ui: &mut Ui, node: NodeId) {
        let selection = node_selection(node);
        if selection != Selection::None {
            if ui.button(tl!("Open")).clicked() {
                self.select(selection);
                ui.close();
            }
            if ui.button(tl!("Rename…")).clicked() {
                self.select(selection);
                self.start_rename();
                ui.close();
            }
            if let Selection::Menu(id) = selection {
                // Encore の「ボタンを表示」: 外すとノードのボタンのポートを畳む。
                let mut shown = !self.flow_hidden_buttons.contains(&id);
                if ui.checkbox(&mut shown, tl!("Show Buttons")).changed() {
                    if shown {
                        self.flow_hidden_buttons.retain(|menu| *menu != id);
                    } else {
                        self.flow_hidden_buttons.push(id);
                    }
                    ui.close();
                }
                ui.separator();
                if crate::actions::menu_item(ui, crate::actions::Action::EditMenuInPhotoCraft, true)
                {
                    self.edit_menu_in_photocraft(id);
                    ui.close();
                }
            }
            ui.separator();
        }
        // リンクの指定: ディスクはファーストプレイ、ほかはエンドアクション。
        let project = &self.session.project;
        let (current, targets) = match node {
            NodeId::FirstPlay => (
                project.settings.first_play.clone(),
                crate::menu_editor::link_targets(project, false),
            ),
            NodeId::Menu(id) => (
                project
                    .menu(id)
                    .map(|menu| menu.end_action.clone())
                    .unwrap_or(disccraft_project::Link::NotSet),
                crate::menu_editor::link_targets(project, true),
            ),
            NodeId::Timeline(id) => (
                project
                    .timeline(id)
                    .map(|timeline| timeline.end_action.clone())
                    .unwrap_or(disccraft_project::Link::NotSet),
                crate::menu_editor::link_targets(project, false),
            ),
            NodeId::Slideshow(id) => (
                project
                    .slideshow(id)
                    .map(|slideshow| slideshow.end_action.clone())
                    .unwrap_or(disccraft_project::Link::NotSet),
                crate::menu_editor::link_targets(project, false),
            ),
            NodeId::Playlist(id) => (
                project
                    .playlist(id)
                    .map(|playlist| playlist.end_action.clone())
                    .unwrap_or(disccraft_project::Link::NotSet),
                crate::menu_editor::link_targets(project, false),
            ),
            NodeId::ChapterPlaylist(id) => (
                project
                    .chapter_playlist(id)
                    .map(|playlist| playlist.end_action.clone())
                    .unwrap_or(disccraft_project::Link::NotSet),
                crate::menu_editor::link_targets(project, false),
            ),
        };
        let mut chosen = None;
        let _ = ui.menu_button(tl!("Specify Link"), |ui| {
            chosen = self.link_menu(ui, &current, &targets);
            if chosen.is_some() {
                ui.close();
            }
        });
        if let Some(link) = chosen {
            match node {
                NodeId::FirstPlay => self.set_first_play(&link),
                NodeId::Menu(id) => self.set_menu_end_action(id, &link),
                NodeId::Timeline(id) => self.set_end_action(id, &link),
                NodeId::Slideshow(id) => self.set_slideshow_end_action(id, &link),
                NodeId::Playlist(id) => self.set_playlist_end_action(id, &link),
                NodeId::ChapterPlaylist(id) => self.set_chapter_playlist_end_action(id, &link),
            }
        }
        if selection == Selection::None {
            return;
        }
        ui.separator();
        self.first_play_items(ui, selection);
        ui.separator();
        if ui.button(tl!("Clear")).clicked() {
            self.remove_object(selection);
            ui.close();
        }
        ui.separator();
        if ui.button(tl!("Preview from Here")).clicked() {
            self.preview_from(selection);
            ui.close();
        }
    }

    /// 「フォルダーへ移動 ▸」: プロジェクトのルートと各フォルダー（今いる所には印）。
    fn move_to_folder_items(&mut self, ui: &mut Ui, selection: Selection) {
        let Some(item) = crate::project_panel::selection_item_id(selection) else {
            return;
        };
        let current = self.session.project.folder_of(item);
        let folders: Vec<(u64, String)> = self
            .session
            .project
            .folders
            .values()
            .map(|folder| {
                (
                    folder.id,
                    crate::project_panel::folder_path(&self.session.project, folder.id),
                )
            })
            .collect();
        let _ = ui.menu_button(tl!("Move to Folder"), |ui| {
            let mut target: Option<Option<u64>> = None;
            if ui
                .selectable_label(current.is_none(), tl!("Project root"))
                .clicked()
            {
                target = Some(None);
            }
            for (id, name) in &folders {
                if ui.selectable_label(current == Some(*id), name).clicked() {
                    target = Some(Some(*id));
                }
            }
            if let Some(folder) = target {
                let _ = self.run_command(
                    "project.moveToFolder",
                    serde_json::json!({ "id": item, "folder": folder }),
                );
                ui.close();
            }
        });
    }

    /// 「ファーストプレイに設定」（ファーストプレイの項目では「ファーストプレイを解除」。Encore と同じ）。
    fn first_play_items(&mut self, ui: &mut Ui, selection: Selection) {
        let link = selection_node(selection).and_then(link_to);
        let is_first_play = link
            .as_ref()
            .is_some_and(|link| same_target(link, &self.session.project.settings.first_play));
        if is_first_play {
            if ui.button(tl!("Clear First Play")).clicked() {
                self.set_first_play(&disccraft_project::Link::NotSet);
                ui.close();
            }
        } else if ui
            .add_enabled(link.is_some(), egui::Button::new(tl!("Set as First Play")))
            .clicked()
        {
            if let Some(link) = link {
                self.set_first_play(&link);
            }
            ui.close();
        }
    }

    /// 読み込み ▸ の項目（Encore のプロジェクトパネル・フローチャート: アセット… / メニュー… / ポップアップメニュー…）。
    fn import_items(&mut self, ui: &mut Ui) {
        if ui.button(tl!("Asset…")).clicked() {
            self.import_assets();
            ui.close();
        }
        if ui.button(tl!("Menu…")).clicked() {
            ui.close();
            self.import_menu_file(crate::menu_commands::MenuImport::Menu);
        }
        if ui.button(tl!("Pop-up Menu…")).clicked() {
            ui.close();
            self.import_menu_file(crate::menu_commands::MenuImport::Popup);
        }
        if ui.button(tl!("Timeline…")).clicked() {
            ui.close();
            self.import_timeline_files();
        }
        if ui.button(tl!("Slideshow…")).clicked() {
            ui.close();
            self.import_slideshow_files();
        }
    }

    /// The right-click menu of an empty part of the Project panel (Encore: 読み込み ▸ / 新規 ▸).
    pub(crate) fn project_background_menu(&mut self, ui: &mut Ui) {
        ui.menu_button(tl!("Import"), |ui| self.import_items(ui));
        ui.separator();
        ui.menu_button(tl!("New"), |ui| self.new_items(ui, None, None));
    }

    /// The items of 新規 ▸ (the Project panel's right-click menus and its New button): Menu, Timeline, Slideshow,
    /// Playlist, Chapter Playlist, Folder. A timeline needs a video: the chosen video asset, or one picked from the
    /// list. A chapter playlist needs a timeline: the right-clicked one, or one picked from the list.
    pub(crate) fn new_items(
        &mut self,
        ui: &mut Ui,
        asset: Option<disccraft_project::AssetId>,
        timeline: Option<TimelineId>,
    ) {
        // Encore の並び: フォルダーが先頭。
        if ui.button(tl!("Folder")).clicked() {
            self.create_folder(self.new_item_folder);
            ui.close();
        }
        if ui.button(tl!("Menu")).clicked() {
            self.create_menu();
            ui.close();
        }
        let videos: Vec<(disccraft_project::AssetId, String)> = self
            .session
            .project
            .assets
            .values()
            .filter(|asset| asset.kind == disccraft_project::AssetKind::Video)
            .map(|asset| (asset.id, asset.name.clone()))
            .collect();
        let chosen_video = asset.filter(|id| videos.iter().any(|(video, _)| video == id));
        match chosen_video {
            Some(video) => {
                if ui.button(tl!("Timeline")).clicked() {
                    self.create_timeline(video);
                    ui.close();
                }
            }
            None if videos.is_empty() => {
                let _ = ui
                    .add_enabled(false, egui::Button::new(tl!("Timeline")))
                    .on_disabled_hover_text(tl!("Import a video first"));
            }
            None => {
                ui.menu_button(tl!("Timeline"), |ui| {
                    ui.label(tl!("Video for the timeline:"));
                    for (video, name) in &videos {
                        if ui.button(name).clicked() {
                            self.create_timeline(*video);
                            ui.close();
                        }
                    }
                });
            }
        }
        if ui.button(tl!("Slideshow")).clicked() {
            self.create_slideshow();
            ui.close();
        }
        if ui.button(tl!("Playlist")).clicked() {
            self.create_playlist();
            ui.close();
        }
        self.new_chapter_playlist_item(ui, timeline);
    }

    /// 新規 ▸ チャプター再生リスト: マスターになるタイムライン（右クリックしたもの、または一覧から）。
    pub(crate) fn new_chapter_playlist_item(&mut self, ui: &mut Ui, timeline: Option<TimelineId>) {
        let timelines: Vec<(TimelineId, String)> = self
            .session
            .project
            .timelines
            .values()
            .map(|timeline| (timeline.id, timeline.name.clone()))
            .collect();
        match timeline.filter(|id| timelines.iter().any(|(timeline, _)| timeline == id)) {
            Some(timeline) => {
                if ui.button(tl!("Chapter Playlist")).clicked() {
                    self.create_chapter_playlist(timeline);
                    ui.close();
                }
            }
            None if timelines.is_empty() => {
                let _ = ui
                    .add_enabled(false, egui::Button::new(tl!("Chapter Playlist")))
                    .on_disabled_hover_text(tl!("Create a timeline first"));
            }
            None => {
                ui.menu_button(tl!("Chapter Playlist"), |ui| {
                    ui.label(tl!("Master timeline:"));
                    for (timeline, name) in &timelines {
                        if ui.button(name).clicked() {
                            self.create_chapter_playlist(*timeline);
                            ui.close();
                        }
                    }
                });
            }
        }
    }

    /// Show a file in the system's file manager (Explorer selects it; elsewhere its folder opens).
    pub(crate) fn reveal_in_file_manager(&mut self, path: &std::path::Path) {
        #[cfg(windows)]
        let result = std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn();
        #[cfg(target_os = "macos")]
        let result = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
        #[cfg(not(any(windows, target_os = "macos")))]
        let result = std::process::Command::new("xdg-open")
            .arg(path.parent().unwrap_or(path))
            .spawn();
        if let Err(error) = result {
            self.status = error.to_string();
        }
    }

    /// 「一定間隔でチャプターポイントを追加」ダイアログ（開いているときだけ描く）。
    pub(crate) fn chapter_interval_dialog(&mut self, ctx: &Context) {
        let Some(mut state) = self.chapter_interval else {
            return;
        };

        let mut done = None;
        let response = Modal::new(Id::unique("chapter_interval")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.heading(tl!("Add Chapter Points at Interval"));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(tl!("Interval:"));
                ui.add(
                    egui::DragValue::new(&mut state.minutes)
                        .range(0.5..=120.0)
                        .speed(0.5)
                        .suffix(tl!(" min")),
                );
            });
            let count = interval_times(state.duration, state.minutes).len();
            ui.small(tl!(
                "Chapter points are added at every interval from the start."
            ));
            ui.label(tlf!("{count} chapter points will be added.", count = count));
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
                self.chapter_interval = None;
                self.add_chapters_at_interval(state);
            }
            Some(false) => self.chapter_interval = None,
            None => self.chapter_interval = Some(state),
        }
    }

    /// 間隔ごとにチャプターポイントを足す（既にある時刻は飛ばす）。
    fn add_chapters_at_interval(&mut self, state: ChapterInterval) {
        let existing: Vec<i64> = self
            .session
            .project
            .timeline(state.timeline)
            .map(|timeline| {
                timeline
                    .chapters
                    .iter()
                    .map(|chapter| chapter.time.0)
                    .collect()
            })
            .unwrap_or_default();

        for seconds in interval_times(state.duration, state.minutes) {
            let ticks = crate::timeline_view::seconds_to_ticks(seconds);
            if existing.contains(&ticks) {
                continue;
            }
            let _ = self.run_command(
                "timeline.addChapter",
                serde_json::json!({ "timeline": state.timeline.0, "time": ticks }),
            );
        }
    }
}

/// 選択をフローチャートのノードにする（ファーストプレイにできるものだけ）。
pub(crate) fn selection_node(selection: Selection) -> Option<NodeId> {
    match selection {
        Selection::Timeline(id) => Some(NodeId::Timeline(id)),
        Selection::Menu(id) => Some(NodeId::Menu(id)),
        Selection::Slideshow(id) => Some(NodeId::Slideshow(id)),
        Selection::Playlist(id) => Some(NodeId::Playlist(id)),
        Selection::ChapterPlaylist(id) => Some(NodeId::ChapterPlaylist(id)),
        Selection::Asset(_) | Selection::None => None,
    }
}

/// フローチャートのノードを選択にする。
pub(crate) fn node_selection(node: NodeId) -> Selection {
    match node {
        NodeId::FirstPlay => Selection::None,
        NodeId::Menu(id) => Selection::Menu(id),
        NodeId::Timeline(id) => Selection::Timeline(id),
        NodeId::Slideshow(id) => Selection::Slideshow(id),
        NodeId::Playlist(id) => Selection::Playlist(id),
        NodeId::ChapterPlaylist(id) => Selection::ChapterPlaylist(id),
    }
}

/// 長さ `duration` 秒のタイムラインに `minutes` 分ごとに打つ時刻（0 秒は含めない。最大 99 個）。
pub(crate) fn interval_times(duration: f64, minutes: f64) -> Vec<f64> {
    let step = minutes * 60.0;
    if !duration.is_finite() || !step.is_finite() || step <= 0.0 {
        return Vec::new();
    }

    let mut times = Vec::new();
    let mut time = step;
    while time < duration && times.len() < 99 {
        times.push(time);
        time += step;
    }
    times
}

/// Whether two links go to the same place (a menu link with or without a button counts as the menu).
fn same_target(a: &disccraft_project::Link, b: &disccraft_project::Link) -> bool {
    use disccraft_project::Link;
    match (a, b) {
        (Link::Menu { id: x, .. }, Link::Menu { id: y, .. }) => x == y,
        (Link::Timeline { id: x, .. }, Link::Timeline { id: y, .. }) => x == y,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::interval_times;

    #[test]
    fn interval_times_skip_the_start_and_end() {
        assert_eq!(interval_times(600.0, 5.0), vec![300.0]);
        assert_eq!(interval_times(601.0, 5.0), vec![300.0, 600.0]);
        assert!(interval_times(60.0, 5.0).is_empty());
        assert!(interval_times(f64::NAN, 5.0).is_empty());
        assert!(interval_times(600.0, 0.0).is_empty());
    }

    #[test]
    fn interval_times_are_bounded() {
        assert_eq!(interval_times(1.0e9, 0.5).len(), 99);
    }
}
