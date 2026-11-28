//! メニューバーとキーボードショートカットの実行（[docs/ui-parity.md](../../../docs/ui-parity.md) §3.1）。
//!
//! メニューの並び（ファイル / 編集 / オブジェクト / メニュー / タイムライン / 表示 / ウィンドウ / ヘルプ）とキーの割り当ては
//! Encore の既定に合わせる。キーとメニューは同じ [`Action`] を通り、[`DiscCraftApp::run_action`] で実行する。

use crate::DiscCraftApp;
use crate::actions::{Action, pasted_text, pressed_actions};
use crate::app::Selection;
use crate::i18n::{Language, tl, tlf};
use crate::menu_editor::HighlightPreview;
use crate::menu_model::{self, Commands, MenuCommand, MenuSink, ModelSink, layout_label};
use crate::track_dialog::TrackKind;
use crate::workspace::Panel;
use craft_menu::{Gate, ItemRole, LayoutOptions, MenuBarUi, MenuRole, Platforms, Target};

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
    /// メニューバーを描き、このフレームで押されたショートカットと選ばれた項目を実行する。メニューは craft-menu で
    /// 描く（M16.4。ほかの Crafting Apps と同じ見た目と操作、Alt か F10 でキーボードから）。
    pub(crate) fn menu_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        // Ctrl+V の文字列は、貼り付けのイベントが消費される前に取っておく。
        let pasted = pasted_text(&ctx);
        // While a menu is open or the keyboard is on the bar (Alt, F10), the keys drive the bar.
        let bar_has_keys = craft_menu::bar::is_open(&ctx) || craft_menu::bar::is_focused(&ctx);
        let mut pressed = if bar_has_keys {
            Vec::new()
        } else {
            pressed_actions(&ctx)
        };
        if pressed.contains(&Action::Paste) {
            self.pasted_text = pasted;
        }
        // M9.55: the shortcuts pressed in a panel's own OS window in the last frame.
        pressed.append(&mut self.queued_actions);

        // The rows chosen in the menus the last frame showed come back from the UI runtime as fired commands (M9.54);
        // each names a row of the menus as that frame laid them out.
        let fired = std::mem::take(&mut self.fired_menu);
        let mut chosen: Vec<MenuCommand> = fired
            .iter()
            .filter_map(|command| self.menu_commands.get(command).cloned())
            .collect();
        let mut commands = Commands::default();
        let model = self.menu_model(&mut commands);
        let options = LayoutOptions::new("DiscCraft").label(&layout_label);
        let gate = Gate::default()
            .modal(ctx.memory(|memory| memory.top_modal_layer().is_some()))
            .text_editing(ctx.text_edit_focused());
        if let Some(native) = self.native_menu.as_mut() {
            // macOS: the system's menu bar, kept up to date with what the menus show.
            native.set_gate(gate);
            let key = craft_ui::contract::Tracked::new(model.clone());
            let _ = native.sync_with(&key, || craft_menu::mac_layout(&model, &options));
            chosen.extend(
                native
                    .take_clicks(&ctx)
                    .iter()
                    .filter_map(|command| commands.get(command).cloned()),
            );
        } else {
            let laid_out = craft_menu::layout(&model, Target::current(), &options).bar;
            if *self.menu_bar_model != laid_out {
                self.menu_bar_model.edit(|bar| *bar = laid_out);
            }
            // Room around the menus' names, as before (the names were packed together).
            ui.spacing_mut().button_padding = egui::vec2(9.0, 3.0);
            let _ = MenuBarUi::new(&self.menu_bar_model)
                .keyboard(!gate.modal)
                .show(ui);
        }
        self.menu_commands = commands;

        let mut run: Vec<MenuCommand> = pressed.into_iter().map(MenuCommand::Action).collect();
        run.extend(chosen);
        for command in run {
            self.run_menu_command(ui, command);
        }
        if let Some(asset) = self.subtitle_export.take() {
            self.export_subtitles(asset);
        }
    }

    /// Puts the menus in macOS's menu bar (craft-menu's native menus, as the other Crafting Apps have them). Call it
    /// once, on the main thread, from eframe's creator closure. Elsewhere, or when AppKit refuses, nothing happens and
    /// the menus stay in the window.
    pub fn install_native_menu(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "macos")]
        {
            let mut commands = Commands::default();
            let model = self.menu_model(&mut commands);
            let options = LayoutOptions::new("DiscCraft").label(&layout_label);
            self.native_menu =
                craft_menu::mac::install(ctx, &craft_menu::mac_layout(&model, &options).bar);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = ctx;
    }

    /// The key equivalents the macOS menu bar took, given back to egui as the key presses they were (eframe's
    /// `raw_input_hook`).
    pub fn native_menu_input(&mut self, ctx: &egui::Context) {
        if let Some(native) = self.native_menu.as_mut() {
            native.poll(ctx);
        }
    }

    /// The menu bar's model, before the layout: the menus in Encore's order (File, Edit, Object, Menu, Timeline, View,
    /// Window, Help), with the roles craft-menu's layouts place them by.
    pub(crate) fn menu_model(&self, commands: &mut Commands) -> craft_menu::MenuBar {
        let menus = vec![
            self.top_menu(commands, tl!("File"), MenuRole::File, Self::file_menu),
            self.top_menu(commands, tl!("Edit"), MenuRole::Edit, Self::edit_menu),
            self.top_menu(commands, tl!("Object"), MenuRole::Other, Self::object_items),
            self.top_menu(commands, tl!("Menu"), MenuRole::Other, Self::menu_menu),
            self.top_menu(
                commands,
                tl!("Timeline"),
                MenuRole::Other,
                Self::timeline_menu,
            ),
            self.top_menu(commands, tl!("View"), MenuRole::View, Self::view_menu),
            self.top_menu(commands, tl!("Window"), MenuRole::Window, Self::window_menu),
            self.top_menu(commands, tl!("Help"), MenuRole::Help, Self::help_menu),
        ];
        menu_model::bar(menus)
    }

    /// One top-level menu: `rows` writes its rows.
    fn top_menu(
        &self,
        commands: &mut Commands,
        title: &str,
        role: MenuRole,
        rows: fn(&Self, &mut dyn MenuSink),
    ) -> craft_menu::Menu {
        let mut sink = ModelSink::new(commands);
        rows(self, &mut sink);
        sink.menu(title, role)
    }

    /// Runs what a menu row or a shortcut chose.
    pub(crate) fn run_menu_command(&mut self, ui: &Ui, command: MenuCommand) {
        match command {
            MenuCommand::Action(action) => self.run_action(ui, action),
            MenuCommand::OpenRecent(path) => {
                self.confirm_or_run(ui.ctx(), crate::confirm::PendingAction::OpenRecent(path));
            }
            MenuCommand::Workspace(id) => self.set_workspace(id),
            MenuCommand::NewWorkspace => self.new_workspace = Some(String::new()),
            MenuCommand::DeleteWorkspace(name) => self.delete_workspace(&name),
            MenuCommand::ResetWorkspace => self.reset_workspace(),
            MenuCommand::Language(language) => self.set_language(language),
            MenuCommand::History { undo, steps } => {
                if steps > 0 {
                    let command = if undo { "edit.undo" } else { "edit.redo" };
                    let _ = self.run_command(command, serde_json::json!({ "steps": steps }));
                }
            }
            MenuCommand::Render(what) => self.start_render(what),
            MenuCommand::CreateTimeline(asset) => self.create_timeline(asset),
            MenuCommand::ExportSubtitles(asset) => self.subtitle_export = Some(asset),
        }
    }

    fn file_menu(&self, m: &mut dyn MenuSink) {
        m.submenu(tl!("New"), true, &mut |m| {
            let asset = matches!(self.selection, Selection::Asset(_));
            m.action(Action::NewProject, true);
            m.action(Action::NewMenu, true);
            m.action(Action::NewTimeline, asset);
            m.action(Action::NewSlideshow, true);
            m.action(Action::NewPlaylist, true);
            let timeline = matches!(self.selection, Selection::Timeline(_));
            m.action(Action::NewChapterPlaylist, timeline);
        });
        m.action(Action::OpenProject, true);
        m.separator();
        m.action(Action::CloseProject, true);
        // Encore: 閉じる（使っているビューアに出ているものを閉じる）。
        m.action(
            Action::CloseViewer,
            matches!(
                self.selection,
                Selection::Menu(_)
                    | Selection::Timeline(_)
                    | Selection::Slideshow(_)
                    | Selection::ChapterPlaylist(_)
            ),
        );
        m.action(Action::Save, true);
        m.action(Action::SaveAs, true);
        m.separator();
        m.submenu(tl!("Import"), true, &mut |m| {
            m.action(Action::ImportAsset, true);
            m.action(Action::ImportMenu, true);
            m.action(Action::ImportPopupMenu, true);
            // メニューページはビューアに開いているメニューのページになる。
            m.action(Action::ImportMenuPage, self.viewer_menu().is_some());
            m.action(Action::ImportTimeline, true);
            m.action(Action::ImportBluray, true);
            m.action(Action::ImportSlideshow, true);
        });
        // Encore: アセットを置き換え… / アセットを検索…（選んだアセットに。検索はファイルが見つからないときだけ）。
        let chosen_asset = match self.selection {
            Selection::Asset(id) => self.session.project.assets.get(&id),
            _ => None,
        };
        m.action(Action::ReplaceAsset, chosen_asset.is_some());
        m.action(
            Action::LocateAsset,
            chosen_asset.is_some_and(|asset| asset.offline),
        );
        m.separator();
        // Encore: トランスコード ▸ トランスコード設定… / 今すぐトランスコード / オリジナルに戻す（選んだアセットに）。
        m.submenu(tl!("Transcode"), true, &mut |m| {
            use crate::project_panel::TranscodeState;
            let idle = !self.build_job.is_running() && !self.transcode_job.is_running();
            let state = match self.selection {
                Selection::Asset(id) => self.transcode_state(id),
                _ => None,
            };
            let any = self
                .session
                .project
                .assets
                .keys()
                .any(|id| self.transcode_state(*id) == Some(TranscodeState::Untranscoded));
            let settable =
                matches!(self.selection, Selection::Asset(id) if self.has_transcode_settings(id));
            m.action(Action::TranscodeSettings, settable);
            m.action(
                Action::TranscodeNow,
                idle && state == Some(TranscodeState::Untranscoded),
            );
            m.action(Action::TranscodeAll, idle && any);
            m.action(
                Action::RevertToOriginal,
                idle && state == Some(TranscodeState::Transcoded),
            );
            m.separator();
            m.action(Action::EditQualityPresets, true);
        });
        // Encore: トランスコードされたファイルを検索 ▸ DVD… / Blu-ray…（選んだビデオのアセットに）。
        let video = matches!(self.selection, Selection::Asset(id) if self.is_video_asset(id));
        m.submenu(tl!("Locate Transcoded File"), video, &mut |m| {
            m.action_as(tl!("DVD…"), Action::LocateTranscodedDvd, true);
            m.action_as(tl!("Blu-ray…"), Action::LocateTranscodedBluray, true);
        });
        m.separator();
        m.action(Action::ProjectSettings, true);
        // Encore: フッテージを変換…（選んだビデオのアセットに）。
        let interpretable =
            matches!(self.selection, Selection::Asset(id) if self.can_interpret_footage(id));
        m.action(Action::InterpretFootage, interpretable);
        m.separator();
        // Encore: DVD-ROM フォルダーを選択… / ディスクのユーザー操作を設定…
        m.action(Action::ChooseRomFolder, true);
        m.action(Action::DiscUserOps, true);
        m.separator();
        m.action(Action::SetFirstPlay, self.selection_node().is_some());
        m.action(Action::Preview, true);
        // Encore: レンダリング ▸ プロジェクト / モーションメニュー / スライドショー（ビルドより先にキャッシュに作る）。
        m.submenu(tl!("Render"), true, &mut |m| {
            let idle = !self.build_job.is_running() && !self.transcode_job.is_running();
            let any = self.session.project.assets.keys().any(|id| {
                self.transcode_state(*id)
                    == Some(crate::project_panel::TranscodeState::Untranscoded)
            });
            m.action_as(tl!("Project"), Action::TranscodeAll, idle && any);
            // The selected motion menu (an HD Blu-ray's too: M6.387), the selected slideshow.
            let motion = match self.selection {
                Selection::Menu(id) => self
                    .session
                    .project
                    .menu(id)
                    .filter(|menu| menu.is_motion())
                    .map(|menu| menu.id),
                _ => None,
            };
            m.command(
                tl!("Motion Menu").to_owned(),
                MenuCommand::Render(disccraft_build::Render::MotionMenus(motion)),
                idle && motion.is_some(),
                None,
            );
            let slideshow = match self.selection {
                Selection::Slideshow(id) => Some(id),
                _ => None,
            };
            m.command(
                tl!("Slideshow").to_owned(),
                MenuCommand::Render(disccraft_build::Render::Slideshows(slideshow)),
                idle && slideshow.is_some(),
                None,
            );
        });
        m.action(Action::CheckProject, true);
        m.submenu(tl!("Build"), true, &mut |m| {
            m.action(Action::BuildFolder, true);
            m.action(Action::BuildImage, true);
        });
        // M6.506 (DiscCraft's own): the disc's label, for PhotoCraft to print on a printable disc.
        m.action(Action::CreateDiscLabel, true);
        // M6.509: the label linked to the project.
        let labelled = self.session.project.settings.disc_label.is_some();
        m.action(Action::OpenDiscLabel, labelled);
        self.recent_items(m);
        m.separator();
        m.action(Action::Exit, true);
    }

    /// 最近使ったプロジェクト（Encore と同じく終了の上に番号付きで並べる）。選ぶと開く。
    fn recent_items(&self, m: &mut dyn MenuSink) {
        if self.recent.is_empty() {
            return;
        }
        m.separator();
        for (index, path) in self.recent.iter().enumerate().take(4) {
            // Encore と同じく途中を省いた形（1 C:\Users\...\名前.ncor。M9.37: 長いパスでメニューが広く高くなり、
            // 低い画面で下の項目が外に出ていた）。
            let text = format!("{} {}", index.saturating_add(1), short_path(path));
            m.command(text, MenuCommand::OpenRecent(path.clone()), true, None);
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
    fn edit_menu(&self, m: &mut dyn MenuSink) {
        // 取り消す / やり直す操作の名前を出す（「元に戻す: ボタンを追加」「やり直せません」）。
        let undo = undo_text(self.session.undo_label(), self.session.undo_detail(), true);
        m.action_as(&undo, Action::Undo, self.session.can_undo());
        let redo = undo_text(self.session.redo_label(), self.session.redo_detail(), false);
        m.action_as(&redo, Action::Redo, self.session.can_redo());
        self.history_menu(m);
        m.separator();
        self.clipboard_items(m);
        // Encore: ソースからマーカーを更新（すべてのマーカーを置換）。
        m.action(
            Action::UpdateMarkersFromSource,
            self.viewer_timeline().is_some(),
        );
        // Encore: リップル削除（タイムラインビューアの再生ヘッドの下のクリップを消して詰める）。
        m.action(
            Action::RippleDelete,
            self.viewer_timeline()
                .and_then(|id| self.session.project.timeline(id))
                .is_some_and(|timeline| !timeline.more_video.is_empty()),
        );
        m.separator();
        m.action(Action::Rename, self.selection != Selection::None);
        m.separator();
        m.action(Action::Clear, self.can_clear());
        m.action(
            Action::EditOriginal,
            matches!(self.selection, Selection::Asset(_)),
        );
        m.action(Action::EditMenuInPhotoCraft, self.viewer_menu().is_some());
        let menu = self.edit_menu_target().is_some();
        m.action(Action::SelectAll, menu);
        m.action(Action::DeselectAll, !self.selected_buttons.is_empty());
        m.separator();
        m.action(Action::Preferences, true);
        // The interface language: in the app menu on macOS (as the other Crafting Apps have it); on Windows and Linux
        // it is in Preferences ▸ Interface.
        m.submenu_with(
            tl!("Language"),
            Some(ItemRole::Language),
            Platforms::MAC,
            &mut |m| {
                for language in Language::ALL {
                    m.command(
                        language.name().to_owned(),
                        MenuCommand::Language(language),
                        true,
                        Some(self.language == language),
                    );
                }
            },
        );
    }

    /// 切り取り / コピー / 貼り付け / 複製（編集メニューとキャンバスの右クリックで共通）。
    pub(crate) fn clipboard_items(&self, m: &mut dyn MenuSink) {
        let menu = self.edit_menu_target().is_some();
        let selected = menu && !self.selected_buttons.is_empty();
        // With no menu buttons chosen, Copy / Paste / Duplicate act on the item chosen in the Project panel.
        let item = !selected && self.copyable_item(self.selection).is_some();
        let paste_buttons = menu && !self.button_clipboard.is_empty();
        let paste_item = self.copyable_item(self.project_clipboard).is_some();
        // A graphic layer chosen in the menu is cut or copied for Paste as Subpicture (Encore).
        let graphic = self
            .edit_menu_target()
            .is_some_and(|menu| self.selected_menu_graphic(menu).is_some());
        m.action(Action::Cut, selected || graphic);
        m.action(Action::Copy, selected || item || graphic);
        m.action(Action::Paste, paste_buttons || paste_item);
        m.action(
            Action::PasteAsSubpicture,
            menu && self.graphic_clipboard.is_some() && self.selected_buttons.len() == 1,
        );
        m.action(Action::Duplicate, selected || item);
    }

    /// オブジェクト ▸ アレンジ / 整列 / 均等配置（メニューバーとキャンバスの右クリックで共通）。
    pub(crate) fn object_items(&self, m: &mut dyn MenuSink) {
        // Encore の並び: ボタンに変換 / オブジェクトに変換、それからアレンジ / 整列 / 均等配置。
        let target = self.edit_menu_target();
        let object_selected = target.is_some_and(|menu| {
            self.selected_menu_text(menu).is_some() || self.selected_menu_graphic(menu).is_some()
        });
        m.action(Action::ConvertToButton, object_selected);
        // Encore: 置換レイヤーに変換（Ctrl+J）は選んだグラフィックに。置換レイヤーはオブジェクトに変換 で戻す。
        let replacement = target
            .and_then(|menu| {
                let index = self.selected_menu_graphic(menu)?;
                self.session.project.menu(menu)?.graphics.get(index)
            })
            .map(|graphic| graphic.replacement);
        m.action(
            Action::ConvertToReplacementLayer,
            replacement == Some(false),
        );
        m.action(
            Action::ConvertToObject,
            target.is_some() && (!self.selected_buttons.is_empty() || replacement == Some(true)),
        );
        m.action(
            Action::CreateSubpicture,
            target.is_some() && !self.selected_buttons.is_empty(),
        );
        m.separator();
        m.action(Action::SpecifyLink, self.link_dialog_target().is_some());
        m.separator();
        m.action(Action::DropShadow, self.shadow_dialog_target().is_some());
        m.separator();
        // Encore: 水平方向に反転 / 垂直方向に反転 / 回転 ▸ がアレンジの上に並ぶ。
        let orient = self.shadow_dialog_target().is_some();
        m.action(Action::FlipHorizontal, orient);
        m.action(Action::FlipVertical, orient);
        m.submenu(tl!("Rotate"), true, &mut |m| {
            for action in [
                Action::Rotate180,
                Action::RotateClockwise,
                Action::RotateCounterClockwise,
            ] {
                m.action(action, orient);
            }
        });
        m.submenu(tl!("Arrange"), true, &mut |m| {
            let enabled = self.edit_menu_target().is_some() && !self.selected_buttons.is_empty();
            for action in [
                Action::BringToFront,
                Action::BringForward,
                Action::SendBackward,
                Action::SendToBack,
            ] {
                m.action(action, enabled);
            }
        });
        m.submenu(tl!("Align"), true, &mut |m| {
            let enabled = self.can_arrange(self.align_to_safe_area);
            for action in [
                Action::AlignLeft,
                Action::AlignHorizontalCenter,
                Action::AlignRight,
            ] {
                m.action(action, enabled);
            }
            m.separator();
            for action in [
                Action::AlignTop,
                Action::AlignVerticalCenter,
                Action::AlignBottom,
            ] {
                m.action(action, enabled);
            }
            m.separator();
            m.check(Action::AlignToSafeArea, self.align_to_safe_area);
        });
        m.submenu(tl!("Distribute"), true, &mut |m| {
            let enabled = self.can_arrange(self.distribute_to_safe_area);
            m.action(Action::DistributeVertically, enabled);
            m.action(Action::DistributeHorizontally, enabled);
            m.separator();
            m.check(Action::DistributeToSafeArea, self.distribute_to_safe_area);
        });
    }

    fn menu_menu(&self, m: &mut dyn MenuSink) {
        m.action_as(tl!("New Menu"), Action::NewMenu, true);
        // ビューアのメニューにチャプターボタンがあれば Encore と同じくそれにリンクを入れ、なければ選択中のタイムラインから
        // メニューを作る（DiscCraft 独自）。
        let timeline = self.selected_timeline().is_some();
        m.action(
            Action::CreateChapterIndex,
            timeline || self.viewer_menu_has_chapter_buttons(),
        );
        m.action(
            Action::CreateMultiPageChapterIndex,
            timeline || self.viewer_menu_has_chapter_buttons(),
        );
        // Encore: メニューを置き換え…（ビューアのメニューを PSD のデザインに。リンクは番号で残す）。
        m.action(Action::ReplaceMenu, self.viewer_menu().is_some());
        m.action(Action::AddMenuPage, self.viewer_menu().is_some());
        m.separator();
        let menu = matches!(self.selection, Selection::Menu(_));
        m.action(Action::AddButton, menu);
        m.separator();
        // Encore の並び: カラーセット / ユーザー操作、ファイル / テンプレート / After Effects、終了時の動作。
        let open = self.viewer_menu().is_some();
        m.action(Action::EditMenuColorSet, open);
        m.action(Action::SetMenuUserOps, open);
        m.separator();
        m.action(Action::SaveMenuAsFile, open);
        m.action(Action::SaveMenuAsTemplate, open);
        // Encore: After Effects コンポジションを作成…
        m.action(Action::CreateEffectCraftComposition, open);
        m.separator();
        m.action(Action::SetMenuEnd, open);
    }

    /// Encore と同じく、作成 / トラック / チャプターポイントの順。
    fn timeline_menu(&self, m: &mut dyn MenuSink) {
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
            m.action_as(tl!("New Timeline"), Action::NewTimeline, true);
        } else {
            m.submenu(tl!("New Timeline"), true, &mut |m| {
                let videos: Vec<(disccraft_project::AssetId, String)> = self
                    .session
                    .project
                    .assets
                    .values()
                    .filter(|asset| asset.kind == disccraft_project::AssetKind::Video)
                    .map(|asset| (asset.id, asset.name.clone()))
                    .collect();
                if videos.is_empty() {
                    m.note(tl!("Import a video first"));
                }
                for (id, name) in videos {
                    m.command(name, MenuCommand::CreateTimeline(id), true, None);
                }
            });
        }
        m.action_as(tl!("New Slideshow"), Action::NewSlideshow, true);
        // Encore の「新規チャプター再生リスト」: 選んでいるタイムラインがマスターになる。
        let selected_timeline = self.selected_timeline().is_some();
        m.action_as(
            tl!("New Chapter Playlist"),
            Action::NewChapterPlaylist,
            selected_timeline,
        );
        m.separator();
        let timeline = self.selected_timeline().is_some();
        m.action(Action::AddAudioTrack, timeline);
        m.action(Action::AddSubtitleTrack, timeline);
        let audio = self.track_count(TrackKind::Audio) > 0;
        let subtitles = self.track_count(TrackKind::Subtitle) > 0;
        m.action(Action::RemoveAudioTrack, audio);
        m.action(Action::RemoveSubtitleTrack, subtitles);
        m.separator();
        // 字幕の読み込み ▸（Encore と同じ 3 つ）: テキストスクリプト（SRT、WebVTT も読む）、FAB 画像スクリプト、
        // 画像スクリプト。書き出しは SRT かテキストスクリプト。
        m.submenu(tl!("Import Subtitles"), timeline, &mut |m| {
            m.action_as(tl!("Text Script…"), Action::ImportSubtitles, true);
            m.action(Action::ImportFabImageScript, true);
            m.action(Action::ImportImageScript, true);
            // M6.422（DiscCraft だけ）: タイムラインの動画（mkv）の中の字幕を字幕トラックに。
            m.separator();
            m.action(Action::ImportSourceSubtitles, true);
        });
        self.export_subtitles_menu(m);
        m.separator();
        m.action(Action::AddChapterPoint, timeline);
        m.action(Action::AddChapterPointsAtInterval, timeline);
        m.action(Action::SetPosterFrame, timeline);
        // FilmCraft のプロジェクトのマーカーをチャプターに（Encore は Premiere Pro のチャプターマーカーを読む）。
        m.action(Action::ImportFilmCraftMarkers, timeline);
        m.action(Action::ImportSourceChapters, timeline);
        m.action(Action::NewTimelineFromFilmCraft, true);
        m.separator();
        // Encore と同じく最後に再生。再生中にもう一度選ぶと止まる。
        m.action(Action::EditTimelineColorSet, timeline);
        m.action(
            Action::MoveTrackToView,
            self.viewer_timeline()
                .and_then(|id| self.session.project.timeline(id))
                .is_some_and(|timeline| !timeline.audio.is_empty()),
        );
        m.action(Action::SetTimelineUserOps, timeline);
        m.action(Action::SaveFrameAsFile, timeline);
        m.action(Action::Play, self.viewer_timeline().is_some());
        m.action(Action::PlayFaster, self.viewer_timeline().is_some());
        m.action(Action::PlaySlower, self.viewer_timeline().is_some());
        m.action(Action::StopPlayback, self.is_playing());
    }

    /// タイムライン ▸ 字幕を書き出し ▸ 字幕トラックの一覧。選ぶとメニューを閉じてから保存先を聞く。
    fn export_subtitles_menu(&self, m: &mut dyn MenuSink) {
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
        m.submenu(tl!("Export Subtitles"), !tracks.is_empty(), &mut |m| {
            if tracks.is_empty() {
                m.note(tl!("(none)"));
            }
            for (label, asset) in &tracks {
                m.command(
                    label.clone(),
                    MenuCommand::ExportSubtitles(*asset),
                    true,
                    None,
                );
            }
        });
    }

    fn view_menu(&self, m: &mut dyn MenuSink) {
        m.action(Action::ZoomIn, true);
        m.action(Action::ZoomOut, true);
        m.action(Action::FitInWindow, true);
        m.separator();
        let states = [
            (Action::ShowNormal, HighlightPreview::Normal),
            (Action::ShowSelected, HighlightPreview::Selected),
            (Action::ShowActivated, HighlightPreview::Activated),
        ];
        for (action, state) in states {
            m.check(action, self.highlight_preview == Some(state));
        }
        m.separator();
        m.check(Action::ShowSafeAreas, self.show_safe_areas);
        m.separator();
        // Encore: ガイドを表示 / ガイドにスナップ / ガイドをロック / ガイドを消去 / 新規ガイド…
        let menu_shown = matches!(self.selection, Selection::Menu(_));
        m.check(Action::ShowGuides, self.show_guides);
        m.check(Action::SnapToGuides, self.snap_to_guides);
        m.check(Action::LockGuides, self.lock_guides);
        m.action(Action::ClearGuides, self.has_guides());
        m.action(Action::NewGuide, menu_shown);
        m.separator();
        m.check(Action::ShowRouting, self.show_routing);
        m.separator();
        m.check(Action::ShowChapterNames, self.show_chapter_names);
    }

    fn window_menu(&self, m: &mut dyn MenuSink) {
        // Encore と同じく、ワークスペースを選び、名前を付けて保存し、消し、リセットする。
        m.submenu(tl!("Workspace"), true, &mut |m| {
            for id in self.workspace_ids() {
                let selected = self.workspace == id;
                m.command(id.label(), MenuCommand::Workspace(id), true, Some(selected));
            }
            m.separator();
            m.command(
                tl!("New Workspace…").to_owned(),
                MenuCommand::NewWorkspace,
                true,
                None,
            );
            m.submenu(
                tl!("Delete Workspace"),
                !self.user_workspaces.is_empty(),
                &mut |m| {
                    if self.user_workspaces.is_empty() {
                        m.note(tl!("(none)"));
                    }
                    for name in &self.user_workspaces {
                        m.command(
                            name.clone(),
                            MenuCommand::DeleteWorkspace(name.clone()),
                            true,
                            None,
                        );
                    }
                },
            );
            let reset = tlf!("Reset “{name}”", name = self.workspace.label());
            m.command(reset, MenuCommand::ResetWorkspace, true, None);
        });
        m.separator();
        for action in WINDOW_ACTIONS {
            m.check(action, self.window_shown(action));
        }
    }

    /// ウィンドウメニューの項目が今表示されているか（パネルはタブグループの前面に出ているか）。
    /// Encore's ウィンドウ ▸ ツール (Ctrl+F4, M6.339): shows or hides the toolbar under the menu bar, and keeps the
    /// choice for the next start.
    fn toggle_tools(&mut self) {
        self.show_tools = !self.show_tools;
        if let Err(error) = crate::workspace::save_hide_tools(!self.show_tools) {
            self.status = error;
        }
    }

    fn window_shown(&self, action: Action) -> bool {
        match (action, window_panel(action)) {
            (Action::WindowCheck, _) => self.check_window.open,
            (Action::WindowTools, _) => self.show_tools,
            (_, Some(panel)) => self.panel_front(panel),
            _ => false,
        }
    }

    /// タイムラインビューアが前面に出ているか。
    fn timeline_viewer_shown(&self) -> bool {
        self.panel_front(Panel::TimelineViewer)
    }

    /// 消去できるもの（最後に押したプロジェクト / メニュー / タイムラインのパネルで選んでいる項目、メニューの選択中の
    /// ボタン、タイムラインの選択中のチャプターポイント）があるか。
    fn can_clear(&self) -> bool {
        let in_list = match self.active_panel {
            Some(Panel::Project) => self.selection != Selection::None,
            Some(Panel::Menus) => matches!(self.selection, Selection::Menu(_)),
            Some(Panel::Timelines) => matches!(self.selection, Selection::Timeline(_)),
            _ => false,
        };
        if in_list {
            return true;
        }
        match self.selection {
            Selection::Menu(_) => {
                self.panel_front(Panel::MenuViewer) && !self.selected_buttons.is_empty()
            }
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
            | Action::ImportMenu
            | Action::ImportPopupMenu
            | Action::ImportMenuPage
            | Action::ImportTimeline
            | Action::ImportBluray
            | Action::ImportSlideshow
            | Action::Exit => self.run_file_action(ui, action),
            Action::NewMenu
            | Action::NewTimeline
            | Action::NewSlideshow
            | Action::NewPlaylist
            | Action::NewChapterPlaylist
            | Action::AddButton => self.run_create_action(action),
            Action::CreateChapterIndex => self.create_chapter_index(false),
            Action::CreateDiscLabel => self.open_disc_label(),
            Action::OpenDiscLabel => self.open_disc_label_file(),
            Action::CreateMultiPageChapterIndex => self.create_chapter_index(true),
            Action::CheckProject | Action::Preview | Action::BuildFolder | Action::BuildImage => {
                self.run_build_action(action);
            }
            Action::AddChapterPointsAtInterval => self.open_chapter_interval(),
            Action::Play => self.toggle_playback(ui.ctx()),
            Action::PlayFaster => self.play_faster(ui.ctx()),
            Action::PlaySlower => self.play_slower(ui.ctx()),
            Action::StopPlayback => self.stop_playback(),
            Action::AddAudioTrack => self.open_track_dialog(TrackKind::Audio),
            Action::AddSubtitleTrack => self.open_track_dialog(TrackKind::Subtitle),
            Action::RemoveAudioTrack => self.remove_last_track(TrackKind::Audio),
            Action::RemoveSubtitleTrack => self.remove_last_track(TrackKind::Subtitle),
            Action::ImportSubtitles => self.import_subtitles(),
            Action::ImportFabImageScript => self.import_image_script(true),
            Action::ImportImageScript => self.import_image_script(false),
            Action::ImportFilmCraftMarkers => self.import_filmcraft_markers(),
            Action::ImportSourceSubtitles => {
                if let Some(timeline) = self.viewer_timeline() {
                    let result = self.run_command(
                        "timeline.importSourceSubtitles",
                        serde_json::json!({ "timeline": timeline.0 }),
                    );
                    if let Some(added) = result
                        .as_ref()
                        .and_then(|value| value.get("added"))
                        .and_then(serde_json::Value::as_u64)
                    {
                        self.status = if added == 0 {
                            tl!("The video has no subtitles to take").to_string()
                        } else {
                            tlf!("Added {n} subtitle tracks from the video", n = added)
                        };
                    }
                }
            }
            Action::ImportSourceChapters => {
                if let Some(timeline) = self.viewer_timeline() {
                    let result = self.run_command(
                        "timeline.importSourceChapters",
                        serde_json::json!({ "timeline": timeline.0 }),
                    );
                    if let Some(added) = result
                        .as_ref()
                        .and_then(|value| value.get("added"))
                        .and_then(serde_json::Value::as_u64)
                    {
                        self.status = if added == 0 {
                            tl!("The source has no chapters to add").to_string()
                        } else {
                            tlf!("Added {n} chapter points from the source", n = added)
                        };
                    }
                }
            }
            Action::NewTimelineFromFilmCraft => self.new_timeline_from_filmcraft(),
            Action::About => self.about_open = true,
            Action::JoinDiscord => self.open_link(ui.ctx(), crate::help::DISCORD),
            Action::ArtCraftWebsite => self.open_link(ui.ctx(), crate::help::WEBSITE),
            Action::DiscCraftOnGitHub => self.open_link(ui.ctx(), crate::help::GITHUB),
            Action::ReportIssue => self.open_link(ui.ctx(), crate::help::ISSUES),
            Action::RevealLogFiles => self.reveal_log_files(),
            Action::SystemReport => self.system_report_open = true,
            Action::CheckForUpdates => self.check_for_updates(ui.ctx()),
            Action::Help => {
                let url = crate::help::manual_url(self.language);
                self.open_link(ui.ctx(), url);
            }
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::Rename => self.start_rename(),
            Action::Preferences => self.prefs.open = true,
            Action::Clear => self.clear_selection(),
            Action::Cut => {
                if self.copy_selected_graphic() {
                    let _ = self.remove_selected_graphic();
                } else {
                    self.cut_buttons(ui.ctx());
                }
            }
            Action::PasteAsSubpicture => self.paste_as_subpicture(),
            Action::Copy => {
                // A graphic layer chosen in the menu (for Paste as Subpicture); else menu buttons when some
                // are chosen; else the item chosen in the Project panel. What was copied last is what Paste makes.
                let graphic = self.copy_selected_graphic();
                if graphic {
                    // Kept for Paste as Subpicture.
                } else if self.copy_buttons(ui.ctx()) {
                    self.project_clipboard = Selection::None;
                } else if self.copyable_item(self.selection).is_some() {
                    self.copy_item(ui.ctx(), self.selection);
                }
            }
            Action::Paste => {
                // Buttons copied here or in another DiscCraft (the system clipboard's text) go into the menu.
                let from_system = self
                    .pasted_text
                    .as_deref()
                    .is_some_and(|text| crate::button_edit::parse_clipboard(text).is_some());
                let item = self.copyable_item(self.project_clipboard).is_some();
                let buttons = self.edit_menu_target().is_some()
                    && (!self.button_clipboard.is_empty() || (from_system && !item));
                if buttons {
                    self.paste_buttons();
                } else {
                    self.duplicate_item(self.project_clipboard);
                }
            }
            Action::Duplicate => {
                let buttons =
                    self.edit_menu_target().is_some() && !self.selected_buttons.is_empty();
                if buttons {
                    self.duplicate_buttons();
                } else {
                    self.duplicate_item(self.selection);
                }
            }
            Action::SelectAll => self.select_all_buttons(),
            Action::DeselectAll => self.deselect_all_buttons(),
            Action::EditMenuInPhotoCraft => {
                if let Some(menu) = self.viewer_menu() {
                    self.edit_menu_in_photocraft(menu);
                }
            }
            Action::DiscUserOps => self.open_disc_user_ops(),
            Action::ReplaceAsset | Action::LocateAsset => {
                if let Selection::Asset(asset) = self.selection {
                    self.relink_asset(asset);
                }
            }
            Action::InterpretFootage => {
                if let Selection::Asset(asset) = self.selection
                    && self.can_interpret_footage(asset)
                {
                    self.open_interpret_footage(asset);
                }
            }
            Action::ChooseRomFolder => self.pick_rom_content(),
            Action::UpdateMarkersFromSource => {
                if let Some(timeline) = self.viewer_timeline()
                    && let Some(added) = self
                        .run_command(
                            "timeline.importSourceChapters",
                            serde_json::json!({ "timeline": timeline.0, "replace": true }),
                        )
                        .as_ref()
                        .and_then(|value| value.get("added"))
                        .and_then(serde_json::Value::as_u64)
                {
                    self.status = tlf!(
                        "Replaced the chapter points with {n} from the source",
                        n = added
                    );
                }
            }
            Action::CloseViewer => self.close_viewer(),
            Action::EditQualityPresets => self.open_quality_presets(),
            Action::EditTimelineColorSet => {
                self.timeline_color_set_dialog = self.viewer_timeline();
            }
            Action::RippleDelete => self.ripple_delete_at_playhead(ui.ctx()),
            Action::SetPosterFrame => self.set_poster_frame_at_playhead(ui.ctx()),
            Action::SetTimelineUserOps => self.open_timeline_user_ops(),
            Action::MoveTrackToView => self.move_track_to_view(ui.ctx()),
            Action::SaveFrameAsFile => self.save_frame_as_file(ui.ctx()),
            Action::EditOriginal => {
                if let Selection::Asset(asset) = self.selection {
                    self.edit_original(asset);
                }
            }
            Action::TranscodeSettings => {
                if let Selection::Asset(asset) = self.selection {
                    self.open_transcode_settings(asset);
                }
            }
            Action::TranscodeNow => {
                if let Selection::Asset(asset) = self.selection {
                    self.start_transcode_now(Some(asset));
                }
            }
            Action::TranscodeAll => self.start_transcode_now(None),
            Action::LocateTranscodedDvd | Action::LocateTranscodedBluray => {
                if let Selection::Asset(asset) = self.selection {
                    let kind = if action == Action::LocateTranscodedDvd {
                        disccraft_project::DiscKind::Dvd
                    } else {
                        disccraft_project::DiscKind::BluRay
                    };
                    self.locate_transcoded(asset, kind);
                }
            }
            Action::RevertToOriginal => {
                if let Selection::Asset(asset) = self.selection {
                    self.revert_to_original(asset);
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
            Action::ReplaceMenu => self.replace_menu_from_file(),
            Action::AddMenuPage => self.add_menu_page(),
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
            | Action::ConvertToReplacementLayer
            | Action::CreateSubpicture
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
            | Action::WindowProgress
            | Action::WindowCharacter
            | Action::WindowStyles
            | Action::WindowMetadata
            | Action::WindowLibrary
            | Action::WindowMenuViewer
            | Action::WindowSlideshowViewer
            | Action::WindowTimelineViewer => self.toggle_window(action),
            Action::WindowTools => self.toggle_tools(),
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
            Action::ImportMenu => self.import_menu_file(crate::menu_commands::MenuImport::Menu),
            Action::ImportPopupMenu => {
                self.import_menu_file(crate::menu_commands::MenuImport::Popup);
            }
            Action::ImportMenuPage => {
                if let Some(menu) = self.viewer_menu() {
                    self.import_menu_file(crate::menu_commands::MenuImport::Page(menu));
                }
            }
            Action::ImportTimeline => self.import_timeline_files(),
            Action::ImportBluray => self.import_bluray(),
            Action::ImportSlideshow => self.import_slideshow_files(),
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
            // Encore の ファイル ▸ プレビュー: ビルドせずに、プロジェクトプレビューの窓で。
            Action::Preview => self.open_project_preview(serde_json::json!({})),
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
        self.show_panel(Panel::Properties);
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
            Action::ConvertToObject => {
                if !self.set_selected_replacement(false) {
                    self.convert_selected_buttons_to_objects();
                }
            }
            Action::ConvertToReplacementLayer => {
                let _ = self.set_selected_replacement(true);
            }
            Action::CreateSubpicture => {
                if let Some(menu) = self.edit_menu_target() {
                    let made = self
                        .run_command(
                            "button.createSubpicture",
                            serde_json::json!({ "menu": menu.0, "buttons": self.selected_buttons }),
                        )
                        .and_then(|made| made.get("buttons")?.as_u64());
                    if let Some(made) = made {
                        self.status = tlf!("Subpictures created: {n}", n = made);
                    }
                }
            }
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

    /// Edit ▸ Clear (Delete): removes what is selected in the panel last pressed, as in Encore — the item in the
    /// Project panel, the menu (or its selected buttons) in the Menus panel, the timeline in the Timelines panel;
    /// in the viewers the buttons, texts and graphics of the menu, or the timeline's selection.
    fn clear_selection(&mut self) {
        let item = matches!(
            self.selection,
            Selection::Asset(_)
                | Selection::Timeline(_)
                | Selection::Menu(_)
                | Selection::Slideshow(_)
                | Selection::Playlist(_)
                | Selection::ChapterPlaylist(_)
        );
        match self.active_panel {
            Some(Panel::Project) if item => {
                self.remove_object(self.selection);
                return;
            }
            Some(Panel::Menus) if matches!(self.selection, Selection::Menu(_)) => {
                if self.selected_buttons.is_empty() {
                    self.remove_object(self.selection);
                } else {
                    self.remove_selected_buttons();
                }
                return;
            }
            Some(Panel::Timelines) if matches!(self.selection, Selection::Timeline(_)) => {
                self.remove_object(self.selection);
                return;
            }
            _ => {}
        }
        match self.selection {
            Selection::Menu(_) if self.panel_front(Panel::MenuViewer) => {
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
            self.show_panel(Panel::TimelineViewer);
        } else if !self.timeline_viewer_shown() {
            // ズームは今のところタイムラインだけが持つ。
            return;
        }

        self.view_request = Some(request);
    }

    fn run_overlay_action(&mut self, action: Action) {
        match action {
            Action::ShowNormal => self.toggle_highlight_preview(HighlightPreview::Normal),
            Action::ShowSelected => self.toggle_highlight_preview(HighlightPreview::Selected),
            Action::ShowActivated => self.toggle_highlight_preview(HighlightPreview::Activated),
            Action::ShowSafeAreas => self.show_safe_areas = !self.show_safe_areas,
            Action::ShowRouting => self.show_routing = !self.show_routing,
            Action::ShowGuides => self.show_guides = !self.show_guides,
            Action::SnapToGuides => self.snap_to_guides = !self.snap_to_guides,
            Action::LockGuides => self.lock_guides = !self.lock_guides,
            Action::ShowAllThumbnails => self.toggle_flow_thumbnails(),
            Action::ShowChapterNames => self.show_chapter_names = !self.show_chapter_names,
            _ => {}
        }
    }

    /// ビルドのタブを前面に出す。
    fn show_build_tab(&mut self) {
        self.show_panel(Panel::Build);
    }

    /// パネルの表示を切り替える: 前面に出ているパネルなら閉じ、そうでなければ前面に出す（閉じていれば開く）。
    fn toggle_window(&mut self, action: Action) {
        match (action, window_panel(action)) {
            (Action::WindowCheck, _) => self.check_window.open = !self.check_window.open,
            (_, Some(panel)) => self.toggle_panel(panel),
            _ => {}
        }
    }
}

/// ウィンドウメニューの項目のパネル。
fn window_panel(action: Action) -> Option<Panel> {
    Some(match action {
        Action::WindowBuild => Panel::Build,
        Action::WindowDisc => Panel::Disc,
        Action::WindowFlowchart => Panel::Flowchart,
        Action::WindowMenus => Panel::Menus,
        Action::WindowMonitor => Panel::Monitor,
        Action::WindowProject => Panel::Project,
        Action::WindowProperties => Panel::Properties,
        Action::WindowTimelines => Panel::Timelines,
        Action::WindowLayers => Panel::Layers,
        Action::WindowProgress => Panel::Progress,
        Action::WindowCharacter => Panel::Character,
        Action::WindowStyles => Panel::Styles,
        Action::WindowMetadata => Panel::Metadata,
        Action::WindowLibrary => Panel::Library,
        Action::WindowMenuViewer => Panel::MenuViewer,
        Action::WindowSlideshowViewer => Panel::SlideshowViewer,
        Action::WindowTimelineViewer => Panel::TimelineViewer,
        _ => return None,
    })
}

/// ウィンドウメニューに並べるパネル（Encore と同じ順。ディスクは DiscCraft だけのパネルで、英語名の順の位置に置く）。
const WINDOW_ACTIONS: [Action; 19] = [
    Action::WindowBuild,
    Action::WindowCharacter,
    Action::WindowCheck,
    Action::WindowDisc,
    Action::WindowFlowchart,
    Action::WindowLayers,
    Action::WindowLibrary,
    Action::WindowMenus,
    Action::WindowMetadata,
    Action::WindowMonitor,
    Action::WindowProgress,
    Action::WindowProject,
    Action::WindowProperties,
    Action::WindowTimelines,
    Action::WindowTools,
    Action::WindowMenuViewer,
    Action::WindowSlideshowViewer,
    Action::WindowStyles,
    Action::WindowTimelineViewer,
];

impl DiscCraftApp {
    /// Edit ▸ History ▸: the steps that can be redone (the farthest first), then the ones that can be undone (the newest
    /// first). Choosing "Undo X" takes X back too, with every step above it; choosing "Redo X" redoes up to and
    /// including X. The steps' names are the commands' English labels, translated (M16.3).
    fn history_menu(&self, m: &mut dyn MenuSink) {
        let redo = self.session.redo_labels();
        let undo = self.session.undo_labels();
        let (redo_details, undo_details) =
            (self.session.redo_details(), self.session.undo_details());
        let named = |label: &str, detail: Option<&Option<String>>| {
            step_name(label, detail.and_then(Option::as_deref))
        };
        m.submenu(
            tl!("History"),
            !redo.is_empty() || !undo.is_empty(),
            &mut |m| {
                if redo.is_empty() && undo.is_empty() {
                    m.note(tl!("(none)"));
                }
                for (index, label) in redo.iter().enumerate() {
                    m.command(
                        tlf!(
                            "Redo {action}",
                            action = named(label, redo_details.get(index))
                        ),
                        MenuCommand::History {
                            undo: false,
                            steps: redo.len().saturating_sub(index),
                        },
                        true,
                        None,
                    );
                }
                for (index, label) in undo.iter().rev().enumerate() {
                    let detail = undo
                        .len()
                        .checked_sub(index.saturating_add(1))
                        .and_then(|at| undo_details.get(at));
                    m.command(
                        tlf!("Undo {action}", action = named(label, detail)),
                        MenuCommand::History {
                            undo: true,
                            steps: index.saturating_add(1),
                        },
                        true,
                        None,
                    );
                }
            },
        );
    }
}

/// A step's name: the command's English label, translated, and what the step did when it was named (M6.530:
/// "Set Link → Making: Chapter 1").
fn step_name(label: &str, detail: Option<&str>) -> String {
    let label = crate::i18n::t(label);
    match detail {
        Some(detail) => format!("{label} → {detail}"),
        None => label.to_owned(),
    }
}

/// Edit ▸ Undo / Redo with the name of the step ("Undo Add Button"); the command's English label is translated.
fn undo_text(label: Option<&str>, detail: Option<&str>, undo: bool) -> String {
    match (label, undo) {
        (Some(label), true) => tlf!("Undo {action}", action = step_name(label, detail)),
        (Some(label), false) => tlf!("Redo {action}", action = step_name(label, detail)),
        (None, true) => tl!("Can't Undo").to_owned(),
        (None, false) => tl!("Can't Redo").to_owned(),
    }
}

/// A recent project's path as Encore's File menu shows it: its first two parts, "...", and the file name
/// (`C:\Users\...\DCSUBTEST.ncor`); a path of four parts or fewer as it is.
pub(crate) fn short_path(path: &std::path::Path) -> String {
    let parts: Vec<String> = path
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.len() <= 4 {
        return path.display().to_string();
    }
    let separator = std::path::MAIN_SEPARATOR;
    let clean = |part: &str| part.trim_end_matches(['\\', '/']).to_owned();
    let first = parts.first().map_or_else(String::new, |part| clean(part));
    let second = parts.get(1).map_or_else(String::new, |part| clean(part));
    let name = parts.last().map_or("", String::as_str);
    // A root ("\") has nothing left once cleaned: the drive and the root are one part to show.
    if second.is_empty() {
        let third = parts.get(2).map_or_else(String::new, |part| clean(part));
        return format!("{first}{separator}{third}{separator}...{separator}{name}");
    }
    format!("{first}{separator}{second}{separator}...{separator}{name}")
}

#[cfg(test)]
mod tests {
    use super::short_path;
    use std::path::Path;

    /// Encore's File menu shows a recent project as `C:\Users\...\DCSUBTEST.ncor`; a short path as it is.
    /// The labels of a laid-out menu's rows (a separator as "-").
    fn labels(menu: &craft_menu::Menu) -> Vec<String> {
        menu.children
            .iter()
            .map(|node| match node {
                craft_menu::Node::Item(item) => item.label.clone(),
                craft_menu::Node::Submenu { label, .. } => format!("{label} ▸"),
                craft_menu::Node::Separator => "-".to_owned(),
                craft_menu::Node::Standard { label, .. } => label.clone(),
                _ => "?".to_owned(),
            })
            .collect()
    }

    /// M16.4: the menus are craft-menu's, laid out as the other Crafting Apps' are. On Windows About is last in
    /// Help, Preferences last in Edit and Exit last in File, and the language is chosen in Preferences; on macOS
    /// they and the language go to the app menu. Every row runs a command the bar can give back.
    #[test]
    fn the_menus_are_laid_out_as_the_other_crafting_apps() {
        use crate::menu_model::Commands;
        use craft_menu::{LayoutOptions, Target};
        let before = crate::i18n::current();
        let app = crate::DiscCraftApp::new(&egui::Context::default());
        crate::i18n::set_current(crate::i18n::Language::En);
        let mut commands = Commands::default();
        let model = app.menu_model(&mut commands);
        let options = LayoutOptions::new("DiscCraft");

        let windows = craft_menu::layout(&model, Target::Windows, &options).bar;
        let titles: Vec<&str> = windows
            .menus
            .iter()
            .map(|menu| menu.title.as_str())
            .collect();
        assert_eq!(
            titles,
            [
                "File", "Edit", "Object", "Menu", "Timeline", "View", "Window", "Help"
            ]
        );
        let last = |title: &str| {
            windows
                .menus
                .iter()
                .find(|menu| menu.title == title)
                .and_then(|menu| labels(menu).last().cloned())
        };
        assert_eq!(last("File").as_deref(), Some("Exit"));
        assert_eq!(last("Edit").as_deref(), Some("Preferences…"));
        assert_eq!(last("Help").as_deref(), Some("About DiscCraft"));
        assert!(
            windows
                .menus
                .iter()
                .all(|menu| !labels(menu).contains(&"Language ▸".to_owned())),
            "the language is in Preferences on Windows"
        );
        for item in windows.items() {
            if item.command.id == "disccraft.menu" {
                assert!(commands.get(&item.command).is_some(), "{}", item.label);
            }
        }

        let mac = craft_menu::layout(&model, Target::Mac, &options).bar;
        let app_menu = mac.menus.first().map(labels).unwrap_or_default();
        assert_eq!(
            mac.menus.first().map(|menu| menu.title.as_str()),
            Some("DiscCraft")
        );
        assert!(
            app_menu.contains(&"About DiscCraft".to_owned()),
            "{app_menu:?}"
        );
        assert!(app_menu.contains(&"Language ▸".to_owned()), "{app_menu:?}");
        crate::i18n::set_current(before);
    }

    #[test]
    fn a_recent_project_is_shown_as_encore_shows_it() {
        if cfg!(windows) {
            assert_eq!(
                short_path(Path::new(r"C:\Users\me\Documents\Discs\Movie.dcproj")),
                r"C:\Users\...\Movie.dcproj"
            );
            assert_eq!(
                short_path(Path::new(r"C:\Discs\Movie.dcproj")),
                r"C:\Discs\Movie.dcproj"
            );
        } else {
            assert_eq!(
                short_path(Path::new("/home/me/Discs/Movies/Movie.dcproj")),
                "/home/.../Movie.dcproj"
            );
            assert_eq!(
                short_path(Path::new("/home/Movie.dcproj")),
                "/home/Movie.dcproj"
            );
        }
    }
}
