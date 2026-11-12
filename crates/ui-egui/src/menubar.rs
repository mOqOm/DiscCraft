//! メニューバーとキーボードショートカットの実行（[docs/ui-parity.md](../../../docs/ui-parity.md) §3.1）。
//!
//! メニューの並び（ファイル / 編集 / オブジェクト / メニュー / タイムライン / 表示 / ウィンドウ / ヘルプ）とキーの割り当ては
//! Encore の既定に合わせる。キーとメニューは同じ [`Action`] を通り、[`DiscCraftApp::run_action`] で実行する。

use crate::DiscCraftApp;
use crate::actions::{Action, menu_check, menu_item, pasted_text, pressed_actions};
use crate::app::Selection;
use crate::i18n::{Language, tl, tlf};
use crate::menu_editor::HighlightPreview;
use crate::track_dialog::TrackKind;
use crate::workspace::{BottomTab, CenterTab, LeftTab, LowerTab, RightTab, WorkspacePreset};

use crate::orient::OrientChange;
use disccraft_build::OutputKind;
use disccraft_project::{Align, Arrange, Axis};
use egui::Ui;

/// 状態を持つビュー（タイムラインのズームと再生ヘッド）への依頼。次にそのビューを描くときに取り出す。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewRequest {
    /// ズームイン。
    ZoomIn,
    /// ズームアウト。
    ZoomOut,
    /// 全体をウィンドウに合わせる。
    Fit,
    /// 再生ヘッドの位置にチャプターポイントを追加する。
    AddChapterPoint,
    /// 選択中のチャプターポイントを消去する。
    Clear,
}

impl DiscCraftApp {
    /// メニューバーを描き、このフレームで押されたショートカットとクリックされた項目を実行する。
    pub(crate) fn menu_bar(&mut self, ui: &mut Ui) {
        // Ctrl+V の文字列は、貼り付けのイベントが消費される前に取っておく。
        let pasted = pasted_text(ui.ctx());
        let pressed = pressed_actions(ui.ctx());
        if pressed.contains(&Action::Paste) {
            self.pasted_text = pasted;
        }
        let mut clicked = Vec::new();

        egui::containers::menu::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(tl!("File"), |ui| self.file_menu(ui, &mut clicked));
            ui.menu_button(tl!("Edit"), |ui| self.edit_menu(ui, &mut clicked));
            ui.menu_button(tl!("Object"), |ui| self.object_items(ui, &mut clicked));
            ui.menu_button(tl!("Menu"), |ui| self.menu_menu(ui, &mut clicked));
            ui.menu_button(tl!("Timeline"), |ui| {
                self.timeline_menu(ui, &mut clicked);
            });
            ui.menu_button(tl!("View"), |ui| self.view_menu(ui, &mut clicked));
            ui.menu_button(tl!("Window"), |ui| self.window_menu(ui, &mut clicked));
            ui.menu_button(tl!("Help"), |ui| {
                push(&mut clicked, ui, Action::About, true)
            });
        });

        for action in pressed.into_iter().chain(clicked) {
            self.run_action(ui, action);
        }
        // 編集 ▸ ヒストリー で選んだところまで、まとめて取り消す / やり直す。
        let jump = ui
            .ctx()
            .data_mut(|data| data.remove_temp::<(bool, usize)>(history_request_id()));
        if let Some((undo, steps)) = jump.filter(|(_, steps)| *steps > 0) {
            let command = if undo { "edit.undo" } else { "edit.redo" };
            let _ = self.run_command(command, serde_json::json!({ "steps": steps }));
        }
        if let Some(asset) = self.subtitle_export.take() {
            self.export_subtitles(asset);
        }

        let recent = ui.ctx().data_mut(|data| {
            data.remove_temp::<std::path::PathBuf>(egui::Id::unique("open_recent_request"))
        });
        if let Some(path) = recent {
            self.confirm_or_run(ui.ctx(), crate::confirm::PendingAction::OpenRecent(path));
        }
    }

    fn file_menu(&self, ui: &mut Ui, clicked: &mut Vec<Action>) {
        ui.menu_button(tl!("New"), |ui| {
            let asset = matches!(self.selection, Selection::Asset(_));
            push(clicked, ui, Action::NewProject, true);
            push(clicked, ui, Action::NewMenu, true);
            push(clicked, ui, Action::NewTimeline, asset);
            push(clicked, ui, Action::NewSlideshow, true);
            push(clicked, ui, Action::NewPlaylist, true);
            let timeline = matches!(self.selection, Selection::Timeline(_));
            push(clicked, ui, Action::NewChapterPlaylist, timeline);
        });
        push(clicked, ui, Action::OpenProject, true);
        ui.separator();
        push(clicked, ui, Action::CloseProject, true);
        push(clicked, ui, Action::Save, true);
        push(clicked, ui, Action::SaveAs, true);
        ui.separator();
        ui.menu_button(tl!("Import"), |ui| {
            push(clicked, ui, Action::ImportAsset, true);
        });
        ui.separator();
        push(clicked, ui, Action::ProjectSettings, true);
        ui.separator();
        // Encore: ディスクのユーザー操作を設定…
        push(clicked, ui, Action::DiscUserOps, true);
        ui.separator();
        push(
            clicked,
            ui,
            Action::SetFirstPlay,
            self.selection_node().is_some(),
        );
        push(clicked, ui, Action::Preview, true);
        push(clicked, ui, Action::CheckProject, true);
        ui.menu_button(tl!("Build"), |ui| {
            push(clicked, ui, Action::BuildFolder, true);
            push(clicked, ui, Action::BuildImage, true);
        });
        self.recent_items(ui);
        ui.separator();
        push(clicked, ui, Action::Exit, true);
    }

    /// 最近使ったプロジェクト（Encore と同じく終了の上に番号付きで並べる）。押すと開く。
    fn recent_items(&self, ui: &mut Ui) {
        if self.recent.is_empty() {
            return;
        }
        ui.separator();
        for (index, path) in self.recent.iter().enumerate().take(4) {
            let text = format!("{} {}", index.saturating_add(1), path.display());
            if ui.button(text).clicked() {
                ui.ctx().data_mut(|data| {
                    data.insert_temp(egui::Id::unique("open_recent_request"), path.clone());
                });
            }
        }
    }

    /// 選択中の項目のフローチャートのノード（ファーストプレイにできるもの）。
    fn selection_node(&self) -> Option<crate::flowchart::NodeId> {
        use crate::flowchart::NodeId;
        match self.selection {
            Selection::Timeline(id) => Some(NodeId::Timeline(id)),
            Selection::Menu(id) => Some(NodeId::Menu(id)),
            Selection::Slideshow(id) => Some(NodeId::Slideshow(id)),
            Selection::Playlist(id) => Some(NodeId::Playlist(id)),
            Selection::ChapterPlaylist(id) => Some(NodeId::ChapterPlaylist(id)),
            Selection::Asset(_) | Selection::None => None,
        }
    }

    /// Encore と同じく、取り消し / クリップボード / 名前の変更 / 消去と選択 / 環境設定 の順。
    fn edit_menu(&self, ui: &mut Ui, clicked: &mut Vec<Action>) {
        // Encore と同じく、取り消す / やり直す操作の名前を出す（「ボタンを追加の取り消し」「やり直しできません」）。
        let undo = undo_text(self.session.undo_label(), true);
        if labeled(ui, &undo, Action::Undo, self.session.can_undo()) {
            clicked.push(Action::Undo);
        }
        let redo = undo_text(self.session.redo_label(), false);
        if labeled(ui, &redo, Action::Redo, self.session.can_redo()) {
            clicked.push(Action::Redo);
        }
        self.history_menu(ui);
        ui.separator();
        self.clipboard_items(ui, clicked);
        ui.separator();
        push(
            clicked,
            ui,
            Action::Rename,
            self.selection != Selection::None,
        );
        ui.separator();
        push(clicked, ui, Action::Clear, self.can_clear());
        push(
            clicked,
            ui,
            Action::EditOriginal,
            matches!(self.selection, Selection::Asset(_)),
        );
        push(
            clicked,
            ui,
            Action::EditMenuInPhotoCraft,
            self.viewer_menu().is_some(),
        );
        let menu = self.edit_menu_target().is_some();
        push(clicked, ui, Action::SelectAll, menu);
        push(
            clicked,
            ui,
            Action::DeselectAll,
            !self.selected_buttons.is_empty(),
        );
        ui.separator();
        push(clicked, ui, Action::Preferences, true);
    }

    /// 切り取り / コピー / 貼り付け / 複製（編集メニューとキャンバスの右クリックで共通）。
    pub(crate) fn clipboard_items(&self, ui: &mut Ui, clicked: &mut Vec<Action>) {
        let menu = self.edit_menu_target().is_some();
        let selected = menu && !self.selected_buttons.is_empty();
        push(clicked, ui, Action::Cut, selected);
        push(clicked, ui, Action::Copy, selected);
        push(
            clicked,
            ui,
            Action::Paste,
            menu && !self.button_clipboard.is_empty(),
        );
        push(clicked, ui, Action::Duplicate, selected);
    }

    /// オブジェクト ▸ アレンジ / 整列 / 均等配置（メニューバーとキャンバスの右クリックで共通）。
    pub(crate) fn object_items(&self, ui: &mut Ui, clicked: &mut Vec<Action>) {
        // Encore の並び: ボタンに変換 / オブジェクトに変換、それからアレンジ / 整列 / 均等配置。
        let target = self.edit_menu_target();
        let object_selected = target.is_some_and(|menu| {
            self.selected_menu_text(menu).is_some() || self.selected_menu_graphic(menu).is_some()
        });
        push(clicked, ui, Action::ConvertToButton, object_selected);
        push(
            clicked,
            ui,
            Action::ConvertToObject,
            target.is_some() && !self.selected_buttons.is_empty(),
        );
        ui.separator();
        push(
            clicked,
            ui,
            Action::SpecifyLink,
            self.link_dialog_target().is_some(),
        );
        ui.separator();
        push(
            clicked,
            ui,
            Action::DropShadow,
            self.shadow_dialog_target().is_some(),
        );
        ui.separator();
        // Encore: 水平方向に反転 / 垂直方向に反転 / 回転 ▸ がアレンジの上に並ぶ。
        let orient = self.shadow_dialog_target().is_some();
        push(clicked, ui, Action::FlipHorizontal, orient);
        push(clicked, ui, Action::FlipVertical, orient);
        ui.menu_button(tl!("Rotate"), |ui| {
            for action in [
                Action::Rotate180,
                Action::RotateClockwise,
                Action::RotateCounterClockwise,
            ] {
                push(clicked, ui, action, orient);
            }
        });
        ui.menu_button(tl!("Arrange"), |ui| {
            let enabled = self.edit_menu_target().is_some() && !self.selected_buttons.is_empty();
            for action in [
                Action::BringToFront,
                Action::BringForward,
                Action::SendBackward,
                Action::SendToBack,
            ] {
                push(clicked, ui, action, enabled);
            }
        });
        ui.menu_button(tl!("Align"), |ui| {
            let enabled = self.can_arrange(self.align_to_safe_area);
            for action in [
                Action::AlignLeft,
                Action::AlignHorizontalCenter,
                Action::AlignRight,
            ] {
                push(clicked, ui, action, enabled);
            }
            ui.separator();
            for action in [
                Action::AlignTop,
                Action::AlignVerticalCenter,
                Action::AlignBottom,
            ] {
                push(clicked, ui, action, enabled);
            }
            ui.separator();
            if menu_check(ui, Action::AlignToSafeArea, self.align_to_safe_area) {
                clicked.push(Action::AlignToSafeArea);
            }
        });
        ui.menu_button(tl!("Distribute"), |ui| {
            let enabled = self.can_arrange(self.distribute_to_safe_area);
            push(clicked, ui, Action::DistributeVertically, enabled);
            push(clicked, ui, Action::DistributeHorizontally, enabled);
            ui.separator();
            if menu_check(
                ui,
                Action::DistributeToSafeArea,
                self.distribute_to_safe_area,
            ) {
                clicked.push(Action::DistributeToSafeArea);
            }
        });
    }

    fn menu_menu(&self, ui: &mut Ui, clicked: &mut Vec<Action>) {
        if labeled(ui, tl!("New Menu"), Action::NewMenu, true) {
            clicked.push(Action::NewMenu);
        }
        // DiscCraft はテンプレートのチャプターボタンを使わず、選択中のタイムラインからメニューを作る。
        let timeline = self.selected_timeline().is_some();
        push(clicked, ui, Action::CreateChapterIndex, timeline);
        push(clicked, ui, Action::CreateMultiPageChapterIndex, timeline);
        ui.separator();
        let menu = matches!(self.selection, Selection::Menu(_));
        push(clicked, ui, Action::AddButton, menu);
        ui.separator();
        // Encore の並び: カラーセット / ユーザー操作、ファイル / テンプレート / After Effects、エンドアクション。
        let open = self.viewer_menu().is_some();
        push(clicked, ui, Action::EditMenuColorSet, open);
        push(clicked, ui, Action::SetMenuUserOps, open);
        ui.separator();
        push(clicked, ui, Action::SaveMenuAsFile, open);
        push(clicked, ui, Action::SaveMenuAsTemplate, open);
        // Encore: After Effects コンポジションを作成…
        push(clicked, ui, Action::CreateEffectCraftComposition, open);
        ui.separator();
        push(clicked, ui, Action::SetMenuEnd, open);
    }

    /// Encore と同じく、作成 / トラック / チャプターポイントの順。
    fn timeline_menu(&mut self, ui: &mut Ui, clicked: &mut Vec<Action>) {
        // Encore の「新規タイムライン」はいつでも選べる。DiscCraft のタイムラインはビデオが要るので、ビデオを
        // 選んでいなければビデオの一覧から選ぶ。
        let video = match self.selection {
            Selection::Asset(id) => self
                .session
                .project
                .assets
                .get(&id)
                .filter(|asset| asset.kind == disccraft_project::AssetKind::Video)
                .map(|_| id),
            _ => None,
        };
        if video.is_some() {
            if labeled(ui, tl!("New Timeline"), Action::NewTimeline, true) {
                clicked.push(Action::NewTimeline);
            }
        } else {
            ui.menu_button(tl!("New Timeline"), |ui| {
                let videos: Vec<(disccraft_project::AssetId, String)> = self
                    .session
                    .project
                    .assets
                    .values()
                    .filter(|asset| asset.kind == disccraft_project::AssetKind::Video)
                    .map(|asset| (asset.id, asset.name.clone()))
                    .collect();
                if videos.is_empty() {
                    ui.label(tl!("Import a video first"));
                }
                for (id, name) in videos {
                    if ui.button(name).clicked() {
                        self.create_timeline(id);
                        ui.close();
                    }
                }
            });
        }
        if labeled(ui, tl!("New Slideshow"), Action::NewSlideshow, true) {
            clicked.push(Action::NewSlideshow);
        }
        // Encore の「新規チャプター再生リスト」: 選んでいるタイムラインがマスターになる。
        let selected_timeline = self.selected_timeline().is_some();
        if labeled(
            ui,
            tl!("New Chapter Playlist"),
            Action::NewChapterPlaylist,
            selected_timeline,
        ) {
            clicked.push(Action::NewChapterPlaylist);
        }
        ui.separator();
        let timeline = self.selected_timeline().is_some();
        push(clicked, ui, Action::AddAudioTrack, timeline);
        push(clicked, ui, Action::AddSubtitleTrack, timeline);
        let audio = self.track_count(TrackKind::Audio) > 0;
        let subtitles = self.track_count(TrackKind::Subtitle) > 0;
        push(clicked, ui, Action::RemoveAudioTrack, audio);
        push(clicked, ui, Action::RemoveSubtitleTrack, subtitles);
        ui.separator();
        // 字幕ファイル（SRT、WebVTT、タイムコード付きテキスト）の読み込みと、SRT での書き出し。
        push(clicked, ui, Action::ImportSubtitles, timeline);
        self.export_subtitles_menu(ui);
        ui.separator();
        push(clicked, ui, Action::AddChapterPoint, timeline);
        push(clicked, ui, Action::AddChapterPointsAtInterval, timeline);
        ui.separator();
        // Encore と同じく最後に再生。再生中にもう一度選ぶと止まる。
        push(clicked, ui, Action::Play, self.viewer_timeline().is_some());
    }

    /// タイムライン ▸ 字幕を書き出し ▸ 字幕トラックの一覧。選ぶとメニューを閉じてから保存先を聞く。
    fn export_subtitles_menu(&mut self, ui: &mut Ui) {
        let tracks: Vec<(String, disccraft_project::AssetId)> = self
            .selected_timeline()
            .and_then(|id| self.session.project.timeline(id))
            .map(|timeline| {
                timeline
                    .subtitles
                    .iter()
                    .enumerate()
                    .map(|(index, track)| {
                        let label = tlf!(
                            "Subtitle {n}: {language}",
                            n = index.saturating_add(1),
                            language = track.language
                        );
                        (label, track.asset)
                    })
                    .collect()
            })
            .unwrap_or_default();
        ui.add_enabled_ui(!tracks.is_empty(), |ui| {
            ui.menu_button(tl!("Export Subtitles"), |ui| {
                for (label, asset) in &tracks {
                    if ui.button(label).clicked() {
                        self.subtitle_export = Some(*asset);
                        ui.close();
                    }
                }
            });
        });
    }

    fn view_menu(&self, ui: &mut Ui, clicked: &mut Vec<Action>) {
        push(clicked, ui, Action::ZoomIn, true);
        push(clicked, ui, Action::ZoomOut, true);
        push(clicked, ui, Action::FitInWindow, true);
        ui.separator();
        let states = [
            (Action::ShowNormal, HighlightPreview::Normal),
            (Action::ShowSelected, HighlightPreview::Selected),
            (Action::ShowActivated, HighlightPreview::Activated),
        ];
        for (action, state) in states {
            if menu_check(ui, action, self.highlight_preview == state) {
                clicked.push(action);
            }
        }
        ui.separator();
        if menu_check(ui, Action::ShowSafeAreas, self.show_safe_areas) {
            clicked.push(Action::ShowSafeAreas);
        }
        ui.separator();
        // Encore: ガイドを表示 / ガイドにスナップ / ガイドをロック / ガイドを消去 / 新規ガイド…
        let menu_shown = matches!(self.selection, Selection::Menu(_));
        if menu_check(ui, Action::ShowGuides, self.show_guides) {
            clicked.push(Action::ShowGuides);
        }
        if menu_check(ui, Action::SnapToGuides, self.snap_to_guides) {
            clicked.push(Action::SnapToGuides);
        }
        if menu_check(ui, Action::LockGuides, self.lock_guides) {
            clicked.push(Action::LockGuides);
        }
        push(clicked, ui, Action::ClearGuides, self.has_guides());
        push(clicked, ui, Action::NewGuide, menu_shown);
        ui.separator();
        if menu_check(ui, Action::ShowRouting, self.show_routing) {
            clicked.push(Action::ShowRouting);
        }
        ui.separator();
        if menu_check(ui, Action::ShowChapterNames, self.show_chapter_names) {
            clicked.push(Action::ShowChapterNames);
        }
    }

    fn window_menu(&mut self, ui: &mut Ui, clicked: &mut Vec<Action>) {
        ui.menu_button(tl!("Workspace"), |ui| {
            for preset in WorkspacePreset::ALL {
                if ui
                    .selectable_label(self.workspace == preset, preset.label())
                    .clicked()
                {
                    self.set_workspace(preset);
                    ui.close();
                }
            }
        });
        ui.separator();
        for action in WINDOW_ACTIONS {
            if menu_check(ui, action, self.window_shown(action)) {
                clicked.push(action);
            }
        }
        ui.separator();
        ui.menu_button(tl!("Language"), |ui| {
            for language in Language::ALL {
                if ui
                    .selectable_label(self.language == language, language.name())
                    .clicked()
                {
                    self.set_language(language);
                    ui.close();
                }
            }
        });
    }

    /// ウィンドウメニューの項目が今表示されているか。
    fn window_shown(&self, action: Action) -> bool {
        let layout = &self.layout;
        match action {
            Action::WindowBuild => layout.left && layout.left_tab == LeftTab::Build,
            Action::WindowCheck => self.check_window.open,
            Action::WindowMenus => layout.left && layout.left_tab == LeftTab::Menus,
            Action::WindowProject => layout.left && layout.left_tab == LeftTab::Project,
            Action::WindowTimelines => layout.left && layout.left_tab == LeftTab::Timelines,
            Action::WindowDisc => layout.right && layout.right_tab == RightTab::Disc,
            Action::WindowProperties => layout.right && layout.right_tab == RightTab::Properties,
            Action::WindowLayers => layout.lower && layout.lower_tab == LowerTab::Layers,
            Action::WindowLibrary => layout.lower && layout.lower_tab == LowerTab::Library,
            Action::WindowSlideshowViewer => {
                layout.bottom && layout.bottom_tab == BottomTab::Slideshow
            }
            Action::WindowTimelineViewer => {
                layout.bottom && layout.bottom_tab == BottomTab::Timeline
            }
            Action::WindowFlowchart => self.tab == CenterTab::Flowchart,
            Action::WindowMenuViewer => self.tab == CenterTab::Menu,
            Action::WindowMonitor => self.tab == CenterTab::Monitor,
            _ => false,
        }
    }

    /// タイムラインビューアが前面に出ているか。
    fn timeline_viewer_shown(&self) -> bool {
        self.layout.bottom && self.layout.bottom_tab == BottomTab::Timeline
    }

    /// 消去できるもの（メニューの選択中のボタン、タイムラインの選択中のチャプターポイント）があるか。
    fn can_clear(&self) -> bool {
        match self.selection {
            Selection::Menu(_) => self.tab == CenterTab::Menu && !self.selected_buttons.is_empty(),
            Selection::Timeline(_) => self.timeline_viewer_shown(),
            _ => false,
        }
    }

    /// 1 つのコマンドを実行する（メニューのクリックとショートカットで共通）。
    pub(crate) fn run_action(&mut self, ui: &Ui, action: Action) {
        match action {
            Action::NewProject
            | Action::OpenProject
            | Action::CloseProject
            | Action::ProjectSettings
            | Action::SetFirstPlay
            | Action::Save
            | Action::SaveAs
            | Action::ImportAsset
            | Action::Exit => self.run_file_action(ui, action),
            Action::NewMenu
            | Action::NewTimeline
            | Action::NewSlideshow
            | Action::NewPlaylist
            | Action::NewChapterPlaylist
            | Action::AddButton => self.run_create_action(action),
            Action::CreateChapterIndex => self.create_chapter_index(false),
            Action::CreateMultiPageChapterIndex => self.create_chapter_index(true),
            Action::CheckProject | Action::Preview | Action::BuildFolder | Action::BuildImage => {
                self.run_build_action(action);
            }
            Action::AddChapterPointsAtInterval => self.open_chapter_interval(),
            Action::Play => self.toggle_playback(ui.ctx()),
            Action::AddAudioTrack => self.open_track_dialog(TrackKind::Audio),
            Action::AddSubtitleTrack => self.open_track_dialog(TrackKind::Subtitle),
            Action::RemoveAudioTrack => self.remove_last_track(TrackKind::Audio),
            Action::RemoveSubtitleTrack => self.remove_last_track(TrackKind::Subtitle),
            Action::ImportSubtitles => self.import_subtitles(),
            Action::About => self.about_open = true,
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::Rename => self.start_rename(),
            Action::Preferences => self.prefs.open = true,
            Action::Clear => self.clear_selection(),
            Action::Cut => self.cut_buttons(ui.ctx()),
            Action::Copy => {
                self.copy_buttons(ui.ctx());
            }
            Action::Paste => self.paste_buttons(),
            Action::Duplicate => self.duplicate_buttons(),
            Action::SelectAll => self.select_all_buttons(),
            Action::DeselectAll => self.deselect_all_buttons(),
            Action::EditMenuInPhotoCraft => {
                if let Some(menu) = self.viewer_menu() {
                    self.edit_menu_in_photocraft(menu);
                }
            }
            Action::DiscUserOps => self.open_disc_user_ops(),
            Action::EditOriginal => {
                if let Selection::Asset(asset) = self.selection {
                    self.edit_original(asset);
                }
            }
            Action::CreateEffectCraftComposition => {
                if let Some(menu) = self.viewer_menu() {
                    self.create_effectcraft_composition(menu);
                }
            }
            Action::EditMenuColorSet => self.open_color_set_dialog(),
            Action::SetMenuUserOps => self.open_menu_user_ops(),
            Action::SaveMenuAsFile => self.save_menu_as_file(),
            Action::SaveMenuAsTemplate => self.save_menu_as_template(),
            Action::SetMenuEnd => self.open_menu_end_action(),
            Action::AlignLeft
            | Action::AlignHorizontalCenter
            | Action::AlignRight
            | Action::AlignTop
            | Action::AlignVerticalCenter
            | Action::AlignBottom
            | Action::AlignToSafeArea
            | Action::BringToFront
            | Action::BringForward
            | Action::SendBackward
            | Action::SendToBack
            | Action::DistributeVertically
            | Action::DistributeHorizontally
            | Action::DistributeToSafeArea
            | Action::ConvertToButton
            | Action::ConvertToObject
            | Action::SpecifyLink
            | Action::DropShadow
            | Action::FlipHorizontal
            | Action::FlipVertical
            | Action::Rotate180
            | Action::RotateClockwise
            | Action::RotateCounterClockwise => self.run_object_action(action),
            Action::AddChapterPoint | Action::ZoomIn | Action::ZoomOut | Action::FitInWindow => {
                self.run_view_action(action)
            }
            Action::ShowNormal
            | Action::ShowSelected
            | Action::ShowActivated
            | Action::ShowSafeAreas
            | Action::ShowRouting
            | Action::ShowGuides
            | Action::SnapToGuides
            | Action::LockGuides
            | Action::ShowAllThumbnails
            | Action::ShowChapterNames => self.run_overlay_action(action),
            Action::ClearGuides => self.clear_guides(),
            Action::NewGuide => self.open_guide_dialog(),
            Action::WindowBuild
            | Action::WindowCheck
            | Action::WindowDisc
            | Action::WindowFlowchart
            | Action::WindowMenus
            | Action::WindowMonitor
            | Action::WindowProject
            | Action::WindowProperties
            | Action::WindowTimelines
            | Action::WindowLayers
            | Action::WindowLibrary
            | Action::WindowMenuViewer
            | Action::WindowSlideshowViewer
            | Action::WindowTimelineViewer => self.toggle_window(action),
        }
    }

    fn run_file_action(&mut self, ui: &Ui, action: Action) {
        match action {
            Action::NewProject => self.welcome.start_new_project(&self.recent),
            Action::OpenProject => {
                self.confirm_or_run(ui.ctx(), crate::confirm::PendingAction::OpenProject);
            }
            Action::CloseProject => {
                self.confirm_or_run(ui.ctx(), crate::confirm::PendingAction::CloseProject);
            }
            Action::ProjectSettings => self.open_project_settings(),
            Action::SetFirstPlay => {
                if let Some(link) = self.selection_node().and_then(crate::flowchart::link_to) {
                    self.set_first_play(&link);
                }
            }
            Action::Save => self.save_project(false),
            Action::SaveAs => self.save_project(true),
            Action::ImportAsset => self.import_assets(),
            Action::Exit => self.confirm_or_run(ui.ctx(), crate::confirm::PendingAction::Exit),
            _ => {}
        }
    }

    fn run_create_action(&mut self, action: Action) {
        match (action, self.selection) {
            (Action::NewMenu, _) => self.create_menu(),
            (Action::NewTimeline, Selection::Asset(asset)) => self.create_timeline(asset),
            (Action::NewTimeline, _) => {
                self.status = tl!("Select a video asset in the Project panel first").to_string();
            }
            (Action::NewSlideshow, _) => self.create_slideshow(),
            (Action::NewPlaylist, _) => self.create_playlist(),
            (Action::NewChapterPlaylist, Selection::Timeline(timeline)) => {
                self.create_chapter_playlist(timeline);
            }
            (Action::NewChapterPlaylist, _) => {
                self.status = tl!("Select a timeline in the Project panel first").to_string();
            }
            (Action::AddButton, Selection::Menu(menu)) => self.add_button(menu),
            _ => {}
        }
    }

    fn run_build_action(&mut self, action: Action) {
        match action {
            Action::CheckProject => {
                // Encore では浮動パネルが開く。
                self.check_window.open = true;
                self.check_project();
            }
            Action::Preview => {
                self.tab = CenterTab::Monitor;
                self.open_preview();
            }
            Action::BuildFolder => {
                self.show_build_tab();
                self.build_output = OutputKind::Folder;
            }
            Action::BuildImage => {
                self.show_build_tab();
                self.build_output = OutputKind::Iso;
            }
            _ => {}
        }
    }

    /// 「一定間隔でチャプターポイントを追加」ダイアログを、選択中のタイムラインに開く。
    fn open_chapter_interval(&mut self) {
        let Some(id) = self.selected_timeline() else {
            return;
        };
        let Some(video) = self
            .session
            .project
            .timeline(id)
            .map(|timeline| timeline.video)
        else {
            return;
        };
        let duration = self.asset_seconds(video).unwrap_or(0.0);
        self.chapter_interval = Some(crate::context_menus::ChapterInterval {
            timeline: id,
            minutes: 5.0,
            duration,
        });
    }

    /// 名前の変更: プロパティパネルを出して、名前の入力欄にフォーカスを移す。
    pub(crate) fn start_rename(&mut self) {
        if self.selection == Selection::None {
            return;
        }
        self.layout.right = true;
        self.layout.right_tab = RightTab::Properties;
        self.focus_rename = true;
    }

    /// オブジェクトメニューの項目。
    fn run_object_action(&mut self, action: Action) {
        match action {
            Action::AlignLeft => self.align_selected(Align::Left),
            Action::AlignHorizontalCenter => self.align_selected(Align::HorizontalCenter),
            Action::AlignRight => self.align_selected(Align::Right),
            Action::AlignTop => self.align_selected(Align::Top),
            Action::AlignVerticalCenter => self.align_selected(Align::VerticalCenter),
            Action::AlignBottom => self.align_selected(Align::Bottom),
            Action::AlignToSafeArea => self.align_to_safe_area = !self.align_to_safe_area,
            Action::BringToFront => self.arrange_selected(Arrange::BringToFront),
            Action::BringForward => self.arrange_selected(Arrange::BringForward),
            Action::SendBackward => self.arrange_selected(Arrange::SendBackward),
            Action::SendToBack => self.arrange_selected(Arrange::SendToBack),
            Action::DistributeVertically => self.distribute_selected(Axis::Vertical),
            Action::DistributeHorizontally => self.distribute_selected(Axis::Horizontal),
            Action::DistributeToSafeArea => {
                self.distribute_to_safe_area = !self.distribute_to_safe_area;
            }
            Action::ConvertToButton => self.convert_selected_to_button(),
            Action::ConvertToObject => self.convert_selected_buttons_to_objects(),
            Action::SpecifyLink => self.open_link_dialog(),
            Action::DropShadow => self.open_shadow_dialog(),
            Action::FlipHorizontal => self.orient_selected(OrientChange::FlipHorizontal),
            Action::FlipVertical => self.orient_selected(OrientChange::FlipVertical),
            Action::Rotate180 => self.orient_selected(OrientChange::Rotate180),
            Action::RotateClockwise => self.orient_selected(OrientChange::RotateClockwise),
            Action::RotateCounterClockwise => {
                self.orient_selected(OrientChange::RotateCounterClockwise);
            }
            _ => {}
        }
    }

    /// オブジェクト ▸ ボタンに変換: 選んでいるテキスト、または画像 / シェイプをボタンにして、そのボタンを選ぶ。
    fn convert_selected_to_button(&mut self) {
        let Some(menu) = self.edit_menu_target() else {
            return;
        };
        let params = if let Some(index) = self.selected_menu_text(menu) {
            serde_json::json!({ "menu": menu.0, "text": index })
        } else if let Some(index) = self.selected_menu_graphic(menu) {
            serde_json::json!({ "menu": menu.0, "graphic": index })
        } else {
            return;
        };
        if let Some(index) = self
            .run_command("menu.convertToButton", params)
            .and_then(|value| value.get("button").and_then(serde_json::Value::as_u64))
            .and_then(|index| usize::try_from(index).ok())
        {
            // The object is a button now (a text is gone, a graphic stays as the button's picture).
            self.selected_text = None;
            self.selected_graphic = None;
            self.select_only_button(menu, index);
        }
    }

    /// オブジェクト ▸ オブジェクトに変換: 選んでいるボタンをテキストに戻す（後ろの番号から）。
    fn convert_selected_buttons_to_objects(&mut self) {
        let Some(menu) = self.edit_menu_target() else {
            return;
        };
        let mut indexes = std::mem::take(&mut self.selected_buttons);
        indexes.sort_unstable();
        indexes.dedup();
        for index in indexes.into_iter().rev() {
            let _ = self.run_command(
                "menu.convertToObject",
                serde_json::json!({ "menu": menu.0, "button": index }),
            );
        }
    }

    fn clear_selection(&mut self) {
        match self.selection {
            Selection::Menu(_) if self.tab == CenterTab::Menu => {
                if !self.remove_selected_text() && !self.remove_selected_graphic() {
                    self.remove_selected_buttons();
                }
            }
            Selection::Timeline(_) if self.timeline_viewer_shown() => {
                self.view_request = Some(ViewRequest::Clear);
            }
            _ => {}
        }
    }

    fn run_view_action(&mut self, action: Action) {
        let request = match action {
            Action::ZoomIn => ViewRequest::ZoomIn,
            Action::ZoomOut => ViewRequest::ZoomOut,
            Action::FitInWindow => ViewRequest::Fit,
            _ => ViewRequest::AddChapterPoint,
        };

        if request == ViewRequest::AddChapterPoint {
            if !matches!(self.selection, Selection::Timeline(_)) {
                return;
            }
            self.layout.bottom = true;
            self.layout.bottom_tab = BottomTab::Timeline;
        } else if !self.timeline_viewer_shown() {
            // ズームは今のところタイムラインだけが持つ。
            return;
        }

        self.view_request = Some(request);
    }

    fn run_overlay_action(&mut self, action: Action) {
        match action {
            Action::ShowNormal => self.highlight_preview = HighlightPreview::Normal,
            Action::ShowSelected => self.highlight_preview = HighlightPreview::Selected,
            Action::ShowActivated => self.highlight_preview = HighlightPreview::Activated,
            Action::ShowSafeAreas => self.show_safe_areas = !self.show_safe_areas,
            Action::ShowRouting => self.show_routing = !self.show_routing,
            Action::ShowGuides => self.show_guides = !self.show_guides,
            Action::SnapToGuides => self.snap_to_guides = !self.snap_to_guides,
            Action::LockGuides => self.lock_guides = !self.lock_guides,
            Action::ShowAllThumbnails => self.flow_thumbnails = !self.flow_thumbnails,
            Action::ShowChapterNames => self.show_chapter_names = !self.show_chapter_names,
            _ => {}
        }
    }

    /// ビルドのタブを前面に出す。
    fn show_build_tab(&mut self) {
        self.layout.left = true;
        self.layout.left_tab = LeftTab::Build;
    }

    /// パネルの表示を切り替える。前面に出ているタブならグループを閉じ、そうでなければ前面に出す。
    /// 中央のタブはそのタブへ切り替える。
    fn toggle_window(&mut self, action: Action) {
        let layout = &mut self.layout;
        match action {
            Action::WindowBuild => toggle(&mut layout.left, &mut layout.left_tab, LeftTab::Build),
            Action::WindowCheck => self.check_window.open = !self.check_window.open,
            Action::WindowMenus => toggle(&mut layout.left, &mut layout.left_tab, LeftTab::Menus),
            Action::WindowProject => {
                toggle(&mut layout.left, &mut layout.left_tab, LeftTab::Project);
            }
            Action::WindowTimelines => {
                toggle(&mut layout.left, &mut layout.left_tab, LeftTab::Timelines);
            }
            Action::WindowDisc => toggle(&mut layout.right, &mut layout.right_tab, RightTab::Disc),
            Action::WindowProperties => {
                toggle(
                    &mut layout.right,
                    &mut layout.right_tab,
                    RightTab::Properties,
                );
            }
            Action::WindowLayers => {
                toggle(&mut layout.lower, &mut layout.lower_tab, LowerTab::Layers);
            }
            Action::WindowLibrary => {
                toggle(&mut layout.lower, &mut layout.lower_tab, LowerTab::Library);
            }
            Action::WindowSlideshowViewer => {
                toggle(
                    &mut layout.bottom,
                    &mut layout.bottom_tab,
                    BottomTab::Slideshow,
                );
            }
            Action::WindowTimelineViewer => {
                toggle(
                    &mut layout.bottom,
                    &mut layout.bottom_tab,
                    BottomTab::Timeline,
                );
            }
            Action::WindowFlowchart => self.tab = CenterTab::Flowchart,
            Action::WindowMenuViewer => self.tab = CenterTab::Menu,
            Action::WindowMonitor => self.tab = CenterTab::Monitor,
            _ => {}
        }
    }
}

/// グループの `tab` が前面に出ていれば閉じ、そうでなければグループを出して `tab` を前面にする。
fn toggle<T: Copy + PartialEq>(visible: &mut bool, current: &mut T, tab: T) {
    if *visible && *current == tab {
        *visible = false;
    } else {
        *visible = true;
        *current = tab;
    }
}

/// ウィンドウメニューに並べるパネル（Encore と同じ順。ディスクは DiscCraft だけのパネルで、英語名の順の位置に置く）。
const WINDOW_ACTIONS: [Action; 14] = [
    Action::WindowBuild,
    Action::WindowCheck,
    Action::WindowDisc,
    Action::WindowFlowchart,
    Action::WindowLayers,
    Action::WindowLibrary,
    Action::WindowMenus,
    Action::WindowMonitor,
    Action::WindowProject,
    Action::WindowProperties,
    Action::WindowTimelines,
    Action::WindowMenuViewer,
    Action::WindowSlideshowViewer,
    Action::WindowTimelineViewer,
];

/// `action` の項目を描き、クリックされたら `clicked` に積む。
fn push(clicked: &mut Vec<Action>, ui: &mut Ui, action: Action, enabled: bool) {
    if menu_item(ui, action, enabled) {
        clicked.push(action);
    }
}

/// 編集メニューの 取り消し / やり直し の表示名。日本語では Encore と同じく操作の名前を付ける（操作の名前は
/// コマンドの日本語の名前なので、英語の表示では付けない）。
/// Where 編集 ▸ ヒストリー leaves the chosen jump (undo?, steps) for after the menus are drawn.
fn history_request_id() -> egui::Id {
    egui::Id::unique("history_jump_request")
}

impl DiscCraftApp {
    /// 編集 ▸ ヒストリー ▸ (Encore): the steps that can be redone (the farthest first), then the ones that can be
    /// undone (the newest first). As in Encore, choosing a step goes to the state just after it: a redo step is
    /// redone, the undo steps above an undo step are taken back (the newest one changes nothing).
    fn history_menu(&self, ui: &mut Ui) {
        let redo = self.session.redo_labels();
        let undo = self.session.undo_labels();
        ui.add_enabled_ui(!redo.is_empty() || !undo.is_empty(), |ui| {
            ui.menu_button(tl!("History"), |ui| {
                egui::ScrollArea::vertical()
                    .max_height(480.0)
                    .show(ui, |ui| {
                        let mut chosen = None;
                        for (index, label) in redo.iter().enumerate() {
                            if ui.button(tlf!("Redo {action}", action = *label)).clicked() {
                                chosen = Some((false, redo.len().saturating_sub(index)));
                            }
                        }
                        for (index, label) in undo.iter().rev().enumerate() {
                            if ui.button(tlf!("Undo {action}", action = *label)).clicked() {
                                chosen = Some((true, index));
                            }
                        }
                        if let Some(jump) = chosen {
                            ui.ctx()
                                .data_mut(|data| data.insert_temp(history_request_id(), jump));
                            ui.close();
                        }
                    });
            });
        });
    }
}

fn undo_text(label: Option<&str>, undo: bool) -> String {
    let japanese = crate::i18n::current() == crate::i18n::Language::Ja;
    match (label, undo) {
        (Some(label), true) if japanese => tlf!("Undo {action}", action = label),
        (Some(label), false) if japanese => tlf!("Redo {action}", action = label),
        (None, true) => tl!("Can't Undo").to_owned(),
        (None, false) => tl!("Can't Redo").to_owned(),
        (Some(_), true) => tl!("Undo").to_owned(),
        (Some(_), false) => tl!("Redo").to_owned(),
    }
}

/// `action` のショートカットを添えて、別の表示名で項目を描く（例: メニューメニューの「新規メニュー」）。
fn labeled(ui: &mut Ui, label: &str, action: Action, enabled: bool) -> bool {
    let shortcut = action.shortcut_text(ui.ctx());
    ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(shortcut))
        .clicked()
}
