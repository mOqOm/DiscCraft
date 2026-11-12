//! プロパティパネル（Encore と同じ形のプロパティシート。[docs/ui-parity.md](../../../docs/ui-parity.md) §3.3）。
//!
//! 上に選択中のものの種類（ディスク / アセット / タイムライン / メニュー …）を見出しとして出し、その下に
//! 右寄せのラベルと値を 1 行ずつ並べる。リンク欄はピックウィップを左、選択欄を右に置く。グループの間は区切り線。
//! 何も選んでいないときはディスクのプロパティを出す。

use crate::app::{DiscCraftApp, Selection};
use crate::flowchart::{NodeId, Port};
use crate::i18n::{tl, tlf};
use crate::menu_editor::{link_label, link_targets};
use crate::timeline_view::{chapter_label, frame_rate, set_selected_chapter, ticks_to_seconds};

use disccraft_project::{
    AssetId, Capacity, DiscKind, Link, MenuId, TimelineId, VideoStandard, format_time,
};
use egui::{Align, Layout, Ui};
use serde_json::json;

/// ラベルの列の幅。
const LABEL_WIDTH: f32 = 130.0;
/// 選択欄の幅。
const FIELD_WIDTH: f32 = 130.0;

impl DiscCraftApp {
    /// プロパティのシート。
    pub(crate) fn property_sheet(&mut self, ui: &mut Ui) {
        match self.selection {
            Selection::None => self.disc_sheet(ui),
            Selection::Asset(id) => self.asset_sheet(ui, id),
            Selection::Timeline(id) => match self.focused_chapter(id) {
                Some(index) => self.chapter_sheet(ui, id, index),
                None => self.timeline_sheet(ui, id),
            },
            Selection::Menu(id) => match (self.selected_menu_text(id), self.focused_button(id)) {
                (Some(index), _) => self.text_sheet(ui, id, index),
                (None, Some(index)) => self.button_sheet(ui, id, index),
                (None, None) => match self.selected_menu_graphic(id) {
                    Some(index) => self.graphic_sheet(ui, id, index),
                    None => self.menu_sheet(ui, id),
                },
            },
            Selection::Slideshow(id) => match self.focused_slide(id) {
                Some(index) => self.slide_sheet(ui, id, index),
                None => self.slideshow_sheet(ui, id),
            },
            Selection::Playlist(id) => self.playlist_sheet(ui, id),
            Selection::ChapterPlaylist(id) => self.chapter_playlist_sheet(ui, id),
        }
    }

    /// ディスク（何も選んでいないとき、とディスクのタブ）。
    pub(crate) fn disc_sheet(&mut self, ui: &mut Ui) {
        sheet_title(ui, tl!("Disc"));

        // Encore と同じく、いちばん上に名前と説明。名前は ISO のボリュームラベルにもなる。
        let name = self.session.project.settings.name.clone();
        let mut changed_name = None;
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Name:"));
            let (response, text) = committed_text(ui, "disc_name", &name, false);
            let _ = response.on_hover_text(tl!(
                "Also the volume label of the ISO image (letters and digits only)"
            ));
            changed_name = text;
        });
        if let Some(name) = changed_name {
            let _ = self.run_command("disc.setName", serde_json::json!({ "name": name }));
        }
        let description = self.session.project.settings.description.clone();
        let mut changed_description = None;
        ui.horizontal_top(|ui| {
            prop_label(ui, tl!("Description:"));
            changed_description = committed_text(ui, "disc_description", &description, true).1;
        });
        if let Some(description) = changed_description {
            let _ = self.run_command(
                "disc.setDescription",
                serde_json::json!({ "description": description }),
            );
        }
        ui.separator();

        let first_play = self.session.project.settings.first_play.clone();
        let targets = link_targets(&self.session.project, true);
        if let Some(link) = self.link_row(
            ui,
            tl!("First Play:"),
            Port::FirstPlay,
            &first_play,
            &targets,
        ) {
            self.set_first_play(&link);
        }
        // 上書き（Encore のディスクの「上書き」）: ファーストプレイの先のタイトルが終わったあとの行き先。
        let can_override = matches!(
            first_play,
            Link::Timeline { .. } | Link::Slideshow { .. } | Link::Playlist { .. }
        );
        let current = self
            .session
            .project
            .settings
            .first_play_override
            .clone()
            .unwrap_or(Link::NotSet);
        let mut chosen = None;
        let _ = ui
            .add_enabled_ui(can_override, |ui| {
                chosen =
                    self.link_row(ui, tl!("Override:"), Port::DiscOverride, &current, &targets);
            })
            .response
            .on_disabled_hover_text(tl!(
                "Set the first play to a timeline, slideshow or playlist first"
            ));
        if let Some(link) = chosen {
            let _ =
                self.run_link_command_pub("nav.setFirstPlayOverride", serde_json::json!({}), &link);
        }
        ui.separator();

        let title = self.session.project.settings.title_remote.clone();
        let menus: Vec<(String, Link)> = link_targets(&self.session.project, false)
            .into_iter()
            .filter(|(_, link)| matches!(link, Link::NotSet | Link::Menu { .. }))
            .collect();
        if let Some(link) =
            self.link_row(ui, tl!("Title Button:"), Port::TitleButton, &title, &menus)
        {
            self.set_title_remote(&link);
        }
        ui.separator();
        self.disc_stream_rows(ui);
        ui.separator();
        // Encore と同じく、ディスクのユーザー操作（ファーストプレイと、すべてのタイトルとメニュー）。
        let disc_ops = self.session.project.settings.prohibited_ops.clone();
        self.user_ops_row(ui, crate::user_ops::UserOpsTarget::Disc, &disc_ops);
        ui.separator();

        self.disc_format_rows(ui);
    }

    /// Encore の「オーディオを設定」「字幕を設定」: ディスクが始まるときに選ぶトラック。
    fn disc_stream_rows(&mut self, ui: &mut Ui) {
        let settings = self.session.project.settings.clone();
        if let Some(track) = audio_choice_row(
            ui,
            tl!("Set Audio:"),
            "disc_audio_default",
            settings.audio_default,
        ) {
            let _ = self.run_command("disc.setAudio", serde_json::json!({ "track": track }));
        }
        if let Some(choice) = subtitle_choice_row(
            ui,
            tl!("Set Subtitle:"),
            "disc_subtitle_default",
            settings.subtitle_default,
        ) {
            let _ = self.run_command(
                "disc.setSubtitle",
                serde_json::json!({ "track": subtitle_value(choice) }),
            );
        }
    }

    /// オーサリングモード、テレビ方式、縦横比、容量。
    fn disc_format_rows(&mut self, ui: &mut Ui) {
        let settings = self.session.project.settings.clone();

        // Encore と同じく、オーサリングモードと縦横比はラジオボタン（Blu-ray が先）。
        let kinds = [(DiscKind::BluRay, "Blu-ray"), (DiscKind::Dvd, "DVD")];
        if let Some(kind) = radio_row(ui, tl!("Authoring mode:"), settings.kind, &kinds, true) {
            self.set_disc_kind(kind);
        }

        let standards = [(VideoStandard::Ntsc, "NTSC"), (VideoStandard::Pal, "PAL")];
        if let Some(standard) = choice_row(
            ui,
            tl!("TV standard:"),
            "sheet_standard",
            settings.standard,
            &standards,
        ) {
            self.set_disc_standard(standard);
        }

        let aspects = [(false, "4:3"), (true, "16:9")];
        if let Some(wide) = radio_row(
            ui,
            tl!("Aspect ratio:"),
            settings.widescreen,
            &aspects,
            true,
        ) {
            self.set_widescreen(wide);
        }

        let capacities: Vec<(Capacity, &str)> = Capacity::choices(settings.kind)
            .iter()
            .map(|capacity| (*capacity, capacity_name(*capacity)))
            .collect();
        if let Some(capacity) = choice_row(
            ui,
            tl!("Disc capacity:"),
            "sheet_capacity",
            settings.capacity,
            &capacities,
        ) {
            self.set_capacity(capacity);
        }
    }

    /// アセット。
    /// アセット（Encore と同じ並び: 名前、説明、トランスコード、場所と種類、長さや寸法）。
    fn asset_sheet(&mut self, ui: &mut Ui, id: AssetId) {
        sheet_title(ui, tl!("Asset"));
        self.name_row(ui);

        let Some(asset) = self.session.project.assets.get(&id).cloned() else {
            return;
        };
        self.description_row(ui, "asset", id.0, &asset.description);
        ui.separator();

        if let Some(state) = self.transcode_state(id) {
            let (dvd, bluray) = state.by_kind(self.session.project.settings.kind);
            text_row(ui, tl!("DVD Transcode Settings:"), dvd.setting_label());
            text_row(ui, tl!("DVD Transcode Status:"), dvd.status_label());
            text_row(
                ui,
                tl!("Blu-ray Transcode Settings:"),
                bluray.setting_label(),
            );
            text_row(ui, tl!("Blu-ray Transcode Status:"), bluray.status_label());
            ui.separator();
        }

        text_row(ui, tl!("Location:"), &asset.path);
        text_row(ui, tl!("Media category:"), media_category(asset.kind));
        text_row(
            ui,
            tl!("Format:"),
            crate::project_panel::asset_kind_label(asset.kind),
        );
        if let Some(secs) = self.asset_probe(id).modified {
            text_row(
                ui,
                tl!("Last modified:"),
                &crate::project_panel::modified_text(secs),
            );
        }
        if asset.offline {
            ui.horizontal(|ui| {
                prop_label(ui, "");
                ui.colored_label(egui::Color32::YELLOW, tl!("Offline"));
                if ui.button(tl!("Locate Asset…")).clicked() {
                    self.relink_asset(id);
                }
            });
        }
        ui.separator();

        let probe = self.asset_probe(id);
        if let Some(seconds) = probe.seconds {
            text_row(
                ui,
                tl!("Duration:"),
                &self.duration_text_at(seconds, probe.fps),
            );
        }
        if let Some(fps) = probe.fps {
            text_row(ui, tl!("Frame rate:"), &fps_text(fps));
        }
        if let Ok(metadata) = std::fs::metadata(&asset.path) {
            text_row(
                ui,
                tl!("Size:"),
                &crate::build_panel::format_bytes(metadata.len()),
            );
        }
        if let Some((width, height)) = probe.dimensions {
            text_row(ui, tl!("Dimensions:"), &format!("{width}x{height}"));
        }
    }

    /// タイムライン。
    fn timeline_sheet(&mut self, ui: &mut Ui, id: TimelineId) {
        sheet_title(ui, tl!("Timeline"));
        self.name_row(ui);

        let Some(timeline) = self.session.project.timeline(id).cloned() else {
            return;
        };
        self.description_row(ui, "timeline", id.0, &timeline.description);
        ui.separator();
        let targets = link_targets(&self.session.project, false);
        let port = Port::EndAction(NodeId::Timeline(id));
        if let Some(link) =
            self.link_row(ui, tl!("End Action:"), port, &timeline.end_action, &targets)
        {
            self.set_end_action(id, &link);
        }
        // 上書き: エンドアクションの先のタイトルが終わったあと、その終了アクションの代わりに行く先。
        // エンドアクションがタイトル（タイムライン・スライドショー・再生リスト）のときだけ使える。
        let can_override = matches!(
            timeline.end_action,
            Link::Timeline { .. } | Link::Slideshow { .. } | Link::Playlist { .. }
        );
        let current = timeline.override_link.clone().unwrap_or(Link::NotSet);
        let mut chosen = None;
        let _ = ui
            .add_enabled_ui(can_override, |ui| {
                chosen =
                    self.link_row(ui, tl!("Override:"), Port::Override(id), &current, &targets);
            })
            .response
            .on_disabled_hover_text(tl!(
                "Set the end action to a timeline, slideshow or playlist first"
            ));
        if let Some(link) = chosen {
            let _ = self.run_link_command_pub(
                "timeline.setOverride",
                serde_json::json!({ "timeline": id.0 }),
                &link,
            );
        }
        // 再生中にリモコンのメニューボタンを押したときの飛び先。未設定なら何も起きない（Blu-ray ではメニューだけ、
        // ポップアップメニューとして）。
        let remote_targets = link_targets(&self.session.project, true);
        if let Some(link) = self.link_row(
            ui,
            tl!("Remote Menu:"),
            Port::MenuRemote(id),
            &timeline.menu_remote,
            &remote_targets,
        ) {
            self.set_menu_remote(id, &link);
        }
        self.user_ops_row(
            ui,
            crate::user_ops::UserOpsTarget::Timeline(id),
            &timeline.prohibited_ops,
        );
        ui.separator();

        let video = self.asset_display_name(timeline.video);
        let seconds = self.asset_seconds(timeline.video);
        let source_fps = self.asset_probe(timeline.video).fps;
        let standard = self.session.project.settings.standard;
        let (width, height) = frame_size(standard);

        // Encore と同じく、エンコード後のサイズ（ディスクの使用量の見積もり。まだなければ --）。
        let size = self
            .usage
            .as_ref()
            .and_then(|usage| usage.timelines.get(&id.0))
            .map_or_else(
                || "--".to_owned(),
                |bytes| crate::build_panel::format_bytes(*bytes),
            );
        // Encore と同じく縦横比のラジオボタン。選ぶとこのタイムラインだけの設定になる。
        let wide = timeline.wide(&self.session.project.settings);
        if let Some(choice) = radio_row(
            ui,
            tl!("Aspect ratio:"),
            wide,
            &[(false, "4:3"), (true, "16:9")],
            true,
        ) {
            let aspect = if choice { "16:9" } else { "4:3" };
            let _ = self.run_command(
                "timeline.setAspect",
                serde_json::json!({ "timeline": id.0, "aspect": aspect }),
            );
        }
        text_row(ui, tl!("Encoded Size:"), &size);
        text_row(ui, tl!("Video:"), &video);
        if let Some(seconds) = seconds {
            text_row(
                ui,
                tl!("Duration:"),
                &self.duration_text_at(seconds, source_fps),
            );
        }
        text_row(ui, tl!("Frame rate:"), &frame_rate_text(standard));
        text_row(ui, tl!("Dimensions:"), &format!("{width}x{height}"));
        text_row(ui, tl!("Audio tracks:"), &timeline.audio.len().to_string());
        text_row(
            ui,
            tl!("Subtitle tracks:"),
            &timeline.subtitles.len().to_string(),
        );
        text_row(ui, tl!("Chapters:"), &timeline.chapters.len().to_string());
    }

    /// タイムラインビューアで選ばれている、`id` のチャプターポイントの番号。
    fn focused_chapter(&self, id: TimelineId) -> Option<usize> {
        let (timeline, index) = self.chapter_focus?;
        let count = self.session.project.timeline(id)?.chapters.len();
        (timeline == id && index < count).then_some(index)
    }

    /// チャプターポイント（Encore と同じく、名前、チャプター番号、イン）。
    fn chapter_sheet(&mut self, ui: &mut Ui, id: TimelineId, index: usize) {
        sheet_title(ui, tl!("Chapter"));
        let Some(chapter) = self
            .session
            .project
            .timeline(id)
            .and_then(|timeline| timeline.chapters.get(index))
            .cloned()
        else {
            return;
        };
        let name = chapter_label(&chapter.name, index);
        let in_point = format_time(chapter.time, &self.session.project.settings);
        let poster = chapter
            .poster
            .map(|poster| format_time(poster, &self.session.project.settings))
            .unwrap_or_default();

        // 入力中でないときは、取り消しなどで変わった値に入力欄を合わせる。
        let name_id = ui.make_persistent_id("chapter_name");
        let in_id = ui.make_persistent_id("chapter_in");
        let poster_id = ui.make_persistent_id("chapter_poster");
        let key = (id.0, index);
        let changed = self.chapter_key != Some(key);
        self.chapter_key = Some(key);
        if changed || !ui.memory(|memory| memory.has_focus(name_id)) {
            self.chapter_name_value.clone_from(&name);
        }
        if changed || !ui.memory(|memory| memory.has_focus(in_id)) {
            self.chapter_in_value.clone_from(&in_point);
        }
        if changed || !ui.memory(|memory| memory.has_focus(poster_id)) {
            self.chapter_poster_value.clone_from(&poster);
        }

        ui.horizontal(|ui| {
            prop_label(ui, tl!("Name:"));
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.chapter_name_value)
                    .id(name_id)
                    .desired_width(200.0),
            );
            if committed(ui, &response) && self.chapter_name_value.trim() != name.trim() {
                let typed = self.chapter_name_value.clone();
                let _ = self.run_command(
                    "timeline.renameChapter",
                    json!({"timeline": id.0, "chapter": index, "name": typed}),
                );
            }
        });
        ui.separator();

        text_row(
            ui,
            tl!("Chapter Number:"),
            &index.saturating_add(1).to_string(),
        );
        // 最初のチャプターポイントはタイムラインの先頭から動かせない。
        if index == 0 {
            text_row(ui, tl!("In:"), &in_point);
        } else {
            ui.horizontal(|ui| {
                prop_label(ui, tl!("In:"));
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.chapter_in_value)
                        .id(in_id)
                        .desired_width(FIELD_WIDTH),
                );
                if committed(ui, &response) && self.chapter_in_value.trim() != in_point {
                    let typed = self.chapter_in_value.trim().to_owned();
                    let moved = self.run_command(
                        "timeline.moveChapter",
                        json!({"timeline": id.0, "chapter": index, "timecode": typed}),
                    );
                    // 時刻の順に並べ直されるので、移った先の番号を選び直す。
                    let new_index = moved
                        .as_ref()
                        .and_then(|value| value.get("chapter"))
                        .and_then(serde_json::Value::as_u64)
                        .and_then(|value| usize::try_from(value).ok());
                    if let Some(new_index) = new_index {
                        self.chapter_focus = Some((id, new_index));
                        set_selected_chapter(ui.ctx(), id, Some(new_index));
                    }
                }
            });
        }
        // ポスターフレーム（Encore と同じタイムコードの欄。空にするとチャプターの頭に戻る）。
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Poster Frame:"));
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.chapter_poster_value)
                    .id(poster_id)
                    .desired_width(FIELD_WIDTH)
                    .hint_text(in_point.as_str()),
            );
            if committed(ui, &response) && self.chapter_poster_value.trim() != poster {
                let typed = self.chapter_poster_value.trim().to_owned();
                let params = if typed.is_empty() {
                    json!({"timeline": id.0, "chapter": index, "time": serde_json::Value::Null})
                } else {
                    json!({"timeline": id.0, "chapter": index, "timecode": typed})
                };
                let _ = self.run_command("timeline.setPosterFrame", params);
            }
        });
    }

    /// メニュービューアで選ばれている、`id` のメニューのボタンの番号。
    fn focused_button(&self, id: MenuId) -> Option<usize> {
        let index = self.primary_button()?;
        let count = self.session.project.menu(id)?.buttons.len();
        (index < count).then_some(index)
    }

    /// メニュー（Encore の「基本」タブにあたる項目のうち DiscCraft にあるもの）。
    fn menu_sheet(&mut self, ui: &mut Ui, id: MenuId) {
        sheet_title(ui, tl!("Menu"));
        self.name_row(ui);

        let Some(menu) = self.session.project.menu(id).cloned() else {
            return;
        };
        self.description_row(ui, "menu", id.0, &menu.description);
        ui.separator();
        // Encore の「基本」と同じく名前の次にエンドアクション。デュレーションは「モーション」の項目。
        let targets = link_targets(&self.session.project, true);
        let port = Port::EndAction(NodeId::Menu(id));
        if let Some(link) = self.link_row(ui, tl!("End Action:"), port, &menu.end_action, &targets)
        {
            self.set_menu_end_action(id, &link);
        }
        // 上書き: エンドアクションの先のタイトルが終わったあとの行き先（Encore のメニューの「上書き」）。
        let can_override = matches!(
            menu.end_action,
            Link::Timeline { .. } | Link::Slideshow { .. } | Link::Playlist { .. }
        );
        let current = menu.override_link.clone().unwrap_or(Link::NotSet);
        let mut chosen = None;
        let _ = ui
            .add_enabled_ui(can_override, |ui| {
                chosen = self.link_row(
                    ui,
                    tl!("Override:"),
                    Port::MenuOverride(id),
                    &current,
                    &targets,
                );
            })
            .response
            .on_disabled_hover_text(tl!(
                "Set the end action to a timeline, slideshow or playlist first"
            ));
        if let Some(link) = chosen {
            let _ = self.run_link_command_pub(
                "menu.setOverride",
                serde_json::json!({ "menu": id.0 }),
                &link,
            );
        }
        self.duration_row(ui, &menu);
        self.user_ops_row(
            ui,
            crate::user_ops::UserOpsTarget::Menu(id),
            &menu.prohibited_ops,
        );
        // ボタンの文字のフォント。
        if let Some(font) = self.font_row(ui, "menu_label_font", menu.label_font.as_deref()) {
            let _ = self.run_command(
                "menu.setLabelFont",
                serde_json::json!({ "menu": id.0, "font": font }),
            );
        }
        ui.separator();
        let buttons: Vec<(usize, String)> = menu
            .buttons
            .iter()
            .enumerate()
            .map(|(index, button)| (index, button_title(index, &button.label)))
            .collect();
        let current = usize::try_from(menu.default_button).unwrap_or(usize::MAX);
        let current_text = buttons
            .iter()
            .find(|(index, _)| *index == current)
            .map_or_else(|| tl!("None").to_owned(), |(_, text)| text.clone());
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Default button:"));
            let mut chosen = None;
            egui::ComboBox::from_id_salt("menu_default_button")
                .width(FIELD_WIDTH)
                .selected_text(current_text)
                .show_ui(ui, |ui| {
                    for (index, text) in &buttons {
                        if ui.selectable_label(*index == current, text).clicked() {
                            chosen = Some(*index);
                        }
                    }
                });
            if let Some(index) = chosen.filter(|index| *index != current) {
                self.set_default_button(id, index);
            }
        });
        // Encore の「オフセット」: リモコンの数字キーで数えるボタンの番号の始まり（チャプターの 2 ページ目を 7 から、など）。
        let mut offset = menu.button_offset;
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Offset:"));
            let response = ui.add(
                egui::DragValue::new(&mut offset).range(0..=disccraft_project::MAX_BUTTON_OFFSET),
            );
            if (response.drag_stopped() || (response.changed() && !response.dragged()))
                && offset != menu.button_offset
            {
                let _ = self.run_command(
                    "menu.setButtonOffset",
                    serde_json::json!({ "menu": id.0, "offset": offset }),
                );
            }
        });
        // Encore の「ボタンのルートを自動設定」: 外すとボタンのシートで上下左右の行き先を選べる。
        let mut auto_route = menu.auto_route;
        ui.horizontal(|ui| {
            prop_label(ui, "");
            if ui
                .checkbox(&mut auto_route, tl!("Automatically Route Buttons"))
                .changed()
            {
                let _ = self.run_command(
                    "menu.setAutoRoute",
                    serde_json::json!({ "menu": id.0, "on": auto_route }),
                );
            }
        });
        ui.separator();

        self.menu_look_rows(ui, &menu);
        ui.separator();

        text_row(ui, tl!("Buttons:"), &menu.buttons.len().to_string());
        // Encore と同じ 4:3 / 16:9 のラジオボタン。DVD のメニューはすべて最初のメニューの縦横比になる。
        if let Some(wide) = radio_row(
            ui,
            tl!("Aspect ratio:"),
            menu.widescreen,
            &[(false, "4:3"), (true, "16:9")],
            true,
        ) {
            let aspect = if wide { "16:9" } else { "4:3" };
            let _ = self.run_command(
                "menu.setAspect",
                serde_json::json!({ "menu": id.0, "aspect": aspect }),
            );
        }
    }

    /// The disc's aspect ratio (the titles; menus stay 4:3).
    fn set_widescreen(&mut self, wide: bool) {
        let aspect = if wide { "16:9" } else { "4:3" };
        let _ = self.run_command("disc.setAspect", serde_json::json!({ "aspect": aspect }));
    }

    /// デュレーション: 静止メニューは「静止」（ボタンが押されるまで）か秒数。過ぎるとエンドアクションへ。
    /// モーションメニューは動画の長さで、1 回流れたらエンドアクションへ（なければ繰り返す）。
    fn duration_row(&mut self, ui: &mut Ui, menu: &disccraft_project::Menu) {
        if let disccraft_project::MenuBackground::Video { seconds, .. } = menu.background {
            text_row(ui, tl!("Duration:"), &tlf!("{n} s (motion)", n = seconds));
            return;
        }
        let key = egui::Id::unique("menu_duration").with(menu.id.0);
        let mut still = menu.duration_seconds.is_none();
        // ドラッグ中の値は画面のメモリーに置き、離したときに 1 回だけ確定する（取り消しが 1 回で済むように）。
        let mut seconds = ui
            .ctx()
            .data(|data| data.get_temp::<u32>(key))
            .or(menu.duration_seconds)
            .unwrap_or(DEFAULT_MENU_SECONDS);
        let mut chosen = None;
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Duration:"));
            if ui.checkbox(&mut still, tl!("Still")).changed() {
                chosen = Some((!still).then_some(seconds));
            }
            if !still {
                let response = ui.add(
                    egui::DragValue::new(&mut seconds)
                        .range(1..=disccraft_project::MAX_MENU_SECONDS)
                        .suffix(tl!(" s")),
                );
                if response.dragged() {
                    ui.ctx().data_mut(|data| data.insert_temp(key, seconds));
                } else if response.drag_stopped() || response.changed() {
                    ui.ctx().data_mut(|data| data.remove::<u32>(key));
                    chosen = Some(Some(seconds));
                }
            }
        });
        if !still && menu.end_action == Link::NotSet {
            ui.weak(tl!(
                "Set an end action to leave the menu when the time is up."
            ));
        }
        if let Some(value) = chosen.filter(|value| *value != menu.duration_seconds) {
            self.set_menu_duration(menu.id, value);
        }
    }

    /// 背景の色と画像、ハイライトの色。
    fn menu_look_rows(&mut self, ui: &mut Ui, menu: &disccraft_project::Menu) {
        self.sync_menu_colors(menu);
        self.background_rows(ui, menu);
        self.menu_audio_row(ui, menu);

        // カラーセット（Encore）: ハイライトグループ 1〜3 の選択時 / アクティベート時の色。グループ 1 の選択時が
        // 従来のハイライトの色。
        let set = menu.color_set_or_default();
        let mut change: Option<(u8, Option<[u8; 3]>, Option<[u8; 3]>)> = None;
        for (index, group) in set.groups.iter().enumerate() {
            let number = u8::try_from(index.saturating_add(1)).unwrap_or(1);
            ui.horizontal(|ui| {
                prop_label(ui, &tlf!("Highlight {n}:", n = number));
                let mut selected = crate::app::rgb_to_f32(group.selected);
                if ui
                    .color_edit_button_rgb(&mut selected)
                    .on_hover_text(tl!("Selected"))
                    .changed()
                {
                    change = Some((number, Some(crate::app::rgb_from_f32(selected)), None));
                }
                ui.label(tl!("Activated:"));
                let mut activated = crate::app::rgb_to_f32(group.activated);
                if ui
                    .color_edit_button_rgb(&mut activated)
                    .on_hover_text(tl!("Activated"))
                    .changed()
                {
                    change = Some((number, None, Some(crate::app::rgb_from_f32(activated))));
                }
            });
        }
        if let Some((group, selected, activated)) = change {
            let mut params = serde_json::Map::new();
            let _ = params.insert("menu".to_owned(), serde_json::Value::from(menu.id.0));
            let _ = params.insert("group".to_owned(), serde_json::Value::from(group));
            if let Some([r, g, b]) = selected {
                let _ = params.insert(
                    "selected".to_owned(),
                    serde_json::Value::from(format!("#{r:02X}{g:02X}{b:02X}")),
                );
            }
            if let Some([r, g, b]) = activated {
                let _ = params.insert(
                    "activated".to_owned(),
                    serde_json::Value::from(format!("#{r:02X}{g:02X}{b:02X}")),
                );
            }
            let _ = self.run_command("menu.setColorSet", serde_json::Value::Object(params));
        }
    }

    /// ボタンの番号（Encore と同じく選んで変えられる。ほかのボタンは詰めて並べ直す）。
    fn number_row(&mut self, ui: &mut Ui, id: MenuId, index: usize) {
        let count = self
            .session
            .project
            .menu(id)
            .map_or(0, |menu| menu.buttons.len());
        let mut chosen = None;
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Number:"));
            egui::ComboBox::from_id_salt("button_number")
                .width(60.0)
                .selected_text(index.saturating_add(1).to_string())
                .show_ui(ui, |ui| {
                    for number in 0..count {
                        let text = number.saturating_add(1).to_string();
                        if ui.selectable_label(number == index, text).clicked() {
                            chosen = Some(number);
                        }
                    }
                });
        });
        if let Some(to) = chosen.filter(|to| *to != index) {
            self.renumber_button(id, index, to);
        }
    }

    /// 背景: 色 / 画像（SVG や PSD も）/ 動画（モーションメニュー。ループの秒数）を選び、その中身を選ぶ。
    fn background_rows(&mut self, ui: &mut Ui, menu: &disccraft_project::Menu) {
        use disccraft_project::MenuBackground;
        let kind = match menu.background {
            MenuBackground::Color { .. } => 0,
            MenuBackground::Image { .. } => 1,
            MenuBackground::Video { .. } => 2,
        };
        let kinds = [tl!("Color"), tl!("Image"), tl!("Video")];
        let images = self.image_assets();
        let videos = self.assets_of(disccraft_project::AssetKind::Video);
        let mut chosen_kind = None;
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Background:"));
            egui::ComboBox::from_id_salt("menu_background_kind")
                .width(90.0)
                .selected_text(kinds.get(kind).copied().unwrap_or_default())
                .show_ui(ui, |ui| {
                    for (index, label) in kinds.iter().enumerate() {
                        // 画像と動画は、そのアセットがあるときだけ選べる。
                        let available = match index {
                            1 => !images.is_empty(),
                            2 => !videos.is_empty(),
                            _ => true,
                        };
                        if ui
                            .add_enabled(available, egui::Button::selectable(index == kind, *label))
                            .clicked()
                        {
                            chosen_kind = Some(index);
                        }
                    }
                });
            if kind == 0 && ui.color_edit_button_rgb(&mut self.bg_color).changed() {
                self.set_background_color(menu.id);
            }
        });
        match chosen_kind.filter(|chosen| *chosen != kind) {
            Some(0) => self.set_background_color(menu.id),
            Some(1) => {
                if let Some((asset, _)) = images.first() {
                    self.set_background_image(menu.id, *asset);
                }
            }
            Some(2) => {
                if let Some((asset, _)) = videos.first() {
                    self.set_background_video(menu.id, *asset, DEFAULT_LOOP_SECONDS);
                }
            }
            _ => {}
        }

        match &menu.background {
            MenuBackground::Image { asset } => {
                if let Some(chosen) = asset_choice(
                    ui,
                    "menu_background_image",
                    tl!("Image:"),
                    &images,
                    Some(*asset),
                ) {
                    self.set_background_image(menu.id, chosen);
                }
            }
            MenuBackground::Video { asset, seconds } => {
                if let Some(chosen) = asset_choice(
                    ui,
                    "menu_background_video",
                    tl!("Video:"),
                    &videos,
                    Some(*asset),
                ) {
                    self.set_background_video(menu.id, chosen, *seconds);
                }
                self.loop_row(ui, menu.id, *asset, *seconds);
            }
            MenuBackground::Color { .. } => {}
        }
    }

    /// モーションメニューのループの長さ（1〜30 秒）。ドラッグを離したときに 1 回だけ確定する。
    fn loop_row(&mut self, ui: &mut Ui, menu: MenuId, asset: AssetId, seconds: u32) {
        let key = egui::Id::unique("menu_loop_seconds").with(menu.0);
        let mut value = ui
            .ctx()
            .data(|data| data.get_temp::<u32>(key))
            .unwrap_or(seconds);
        let mut chosen = None;
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Loop:"));
            let response = ui.add(
                egui::DragValue::new(&mut value)
                    .range(1..=MAX_LOOP_SECONDS)
                    .suffix(tl!(" s")),
            );
            if response.dragged() {
                ui.ctx().data_mut(|data| data.insert_temp(key, value));
            } else if response.drag_stopped() || response.changed() {
                ui.ctx().data_mut(|data| data.remove::<u32>(key));
                chosen = Some(value);
            }
        });
        if let Some(value) = chosen.filter(|value| *value != seconds) {
            self.set_background_video(menu, asset, value);
        }
    }

    /// オーディオ: メニューといっしょに流す音声（なし / 音声のアセット）。静止メニューは音声の長さで繰り返す。
    fn menu_audio_row(&mut self, ui: &mut Ui, menu: &disccraft_project::Menu) {
        let sounds = self.assets_of(disccraft_project::AssetKind::Audio);
        let current = menu
            .audio
            .and_then(|asset| sounds.iter().find(|(id, _)| *id == asset))
            .map_or_else(|| tl!("None").to_owned(), |(_, name)| name.clone());
        let mut chosen: Option<Option<AssetId>> = None;
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Audio:"));
            egui::ComboBox::from_id_salt("menu_audio")
                .width(FIELD_WIDTH)
                .selected_text(current)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(menu.audio.is_none(), tl!("None"))
                        .clicked()
                    {
                        chosen = Some(None);
                    }
                    for (asset, name) in &sounds {
                        if ui
                            .selectable_label(menu.audio == Some(*asset), name)
                            .clicked()
                        {
                            chosen = Some(Some(*asset));
                        }
                    }
                });
        });
        if let Some(audio) = chosen.filter(|audio| *audio != menu.audio) {
            self.run_command(
                "menu.setAudio",
                serde_json::json!({ "menu": menu.id.0, "asset": audio.map(|asset| asset.0) }),
            );
        }
    }

    /// ボタン（メニュービューアでボタンを選んだとき）。ボタンテキスト、番号、リンク。
    fn button_sheet(&mut self, ui: &mut Ui, id: MenuId, index: usize) {
        sheet_title(ui, tl!("Button"));
        let count = self.selected_buttons.len();
        if count > 1 {
            // いくつも選んでいるときは、最後に選んだボタンのシートを出す。
            ui.label(tlf!("{n} buttons selected", n = count));
        }
        let Some(button) = self
            .session
            .project
            .menu(id)
            .and_then(|menu| menu.buttons.get(index))
            .cloned()
        else {
            return;
        };

        // 名前（Encore の「名前」。ボタンテキストと同期しているときはテキストに従う）。
        let name_key = format!("button_name_{}_{}", id.0, index);
        let mut typed_name = None;
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Name:"));
            ui.add_enabled_ui(!button.sync_name && !button.name_from_link, |ui| {
                ui.set_max_width(208.0);
                typed_name = committed_text(ui, &name_key, &button.name, false).1;
            });
        });
        if let Some(name) = typed_name.filter(|name| !name.trim().is_empty()) {
            let _ = self.run_command(
                "button.setName",
                serde_json::json!({ "menu": id.0, "button": index, "name": name.trim() }),
            );
        }
        self.sync_button_label(id, index, &button.label);
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Button text:"));
            let response =
                ui.add(egui::TextEdit::singleline(&mut self.button_label).desired_width(200.0));
            // 編集 ▸ 名前の変更… とキャンバスの右クリックの「名前の変更…」はここに来る。
            if std::mem::take(&mut self.focus_rename) {
                response.request_focus();
            }
            if committed(ui, &response) && self.button_label != button.label {
                self.set_button_label(id, index);
            }
        });
        self.number_row(ui, id, index);
        // 種類（Encore のボタンの「種類」）: 標準 / チャプター / 次へ / 前へ / メイン。
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Kind:"));
            let mut chosen = None;
            egui::ComboBox::from_id_salt(("button_kind", id.0, index))
                .selected_text(button_kind_label(button.kind))
                .width(FIELD_WIDTH)
                .show_ui(ui, |ui| {
                    for kind in disccraft_project::ButtonKind::ALL {
                        if ui
                            .selectable_label(button.kind == kind, button_kind_label(kind))
                            .clicked()
                        {
                            chosen = Some(kind);
                        }
                    }
                });
            if let Some(kind) = chosen.filter(|kind| *kind != button.kind) {
                let _ = self.run_command(
                    "button.setKind",
                    serde_json::json!({ "menu": id.0, "button": index, "kind": kind.as_str() }),
                );
            }
        });
        // ハイライト（Encore のボタンの「ハイライト」）: カラーセットのグループ 1〜3。
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Highlight:"));
            let mut chosen = None;
            egui::ComboBox::from_id_salt(("button_highlight_group", id.0, index))
                .selected_text(tlf!("Group {n}", n = button.highlight_group))
                .width(FIELD_WIDTH)
                .show_ui(ui, |ui| {
                    for group in 1..=3_u8 {
                        if ui
                            .selectable_label(
                                button.highlight_group == group,
                                tlf!("Group {n}", n = group),
                            )
                            .clicked()
                        {
                            chosen = Some(group);
                        }
                    }
                });
            if let Some(group) = chosen.filter(|group| *group != button.highlight_group) {
                let _ = self.run_command(
                    "button.setHighlightGroup",
                    serde_json::json!({ "menu": id.0, "button": index, "group": group }),
                );
            }
        });
        self.route_rows(ui, id, index);
        ui.separator();

        let targets = link_targets(&self.session.project, true);
        let port = Port::Button { menu: id, index };
        if let Some(link) = self.link_row(ui, tl!("Link:"), port, &button.link, &targets) {
            self.set_button_link(id, index, &link);
        }
        // 上書き: このボタンで再生したタイトルが終わったあとの行き先（リンクがタイトルのときだけ）。
        let can_override = matches!(
            button.link,
            Link::Timeline { .. } | Link::Slideshow { .. } | Link::Playlist { .. }
        );
        let current = button.override_link.clone().unwrap_or(Link::NotSet);
        let mut chosen = None;
        let _ = ui
            .add_enabled_ui(can_override, |ui| {
                chosen = self.link_row(
                    ui,
                    tl!("Override:"),
                    Port::ButtonOverride { menu: id, index },
                    &current,
                    &targets,
                );
            })
            .response
            .on_disabled_hover_text(tl!(
                "Set the link to a timeline, slideshow or playlist first"
            ));
        if let Some(link) = chosen {
            let _ = self.run_link_command_pub(
                "button.setOverride",
                serde_json::json!({ "menu": id.0, "button": index }),
                &link,
            );
        }
        // Encore のリンクの指定の「オーディオ」「字幕」: このボタンで飛ぶときに選ぶトラック。
        if let Some(audio) = audio_choice_row(ui, tl!("Audio:"), "button_audio", button.audio) {
            let _ = self.run_command(
                "button.setStreams",
                serde_json::json!({ "menu": id.0, "button": index, "audio": audio }),
            );
        }
        if let Some(subtitle) =
            subtitle_choice_row(ui, tl!("Subtitle:"), "button_subtitle", button.subtitle)
        {
            let _ = self.run_command(
                "button.setStreams",
                serde_json::json!({ "menu": id.0, "button": index, "subtitle": subtitle_value(subtitle) }),
            );
        }
        // 選んだだけで決定になるボタン（次のページへ、など）。
        let mut auto = button.auto_activate;
        ui.horizontal(|ui| {
            prop_label(ui, "");
            if ui.checkbox(&mut auto, tl!("Auto Activate")).changed() {
                self.run_command(
                    "button.setAutoActivate",
                    crate::gui_params::auto_activate(id.0, index, auto),
                );
            }
        });
        // Encore の「ボタンテキストと名前を同期」「リンク先から名前を設定」。
        let mut sync = button.sync_name;
        let mut from_link = button.name_from_link;
        let mut options: Option<serde_json::Value> = None;
        ui.horizontal(|ui| {
            prop_label(ui, "");
            if ui
                .checkbox(&mut sync, tl!("Sync Button Text and Name"))
                .changed()
            {
                options = Some(serde_json::json!({ "syncName": sync }));
            }
        });
        ui.horizontal(|ui| {
            prop_label(ui, "");
            if ui
                .checkbox(&mut from_link, tl!("Set Name from Link"))
                .changed()
            {
                options = Some(serde_json::json!({ "nameFromLink": from_link }));
            }
        });
        if let Some(mut fields) = options
            && let Some(map) = fields.as_object_mut()
        {
            let _ = map.insert("menu".to_owned(), serde_json::Value::from(id.0));
            let _ = map.insert("button".to_owned(), serde_json::Value::from(index));
            let _ = self.run_command("button.setNameOptions", fields);
        }
    }

    /// ボタンのルート（上 / 下 / 左 / 右の行き先）。メニューが自動で決めているときは出さない。
    fn route_rows(&mut self, ui: &mut Ui, id: MenuId, index: usize) {
        let Some(menu) = self.session.project.menu(id) else {
            return;
        };
        if menu.auto_route {
            return;
        }
        let count = menu.buttons.len();
        let names: Vec<String> = menu
            .buttons
            .iter()
            .enumerate()
            .map(|(i, button)| {
                let text = if button.label.trim().is_empty() {
                    button.name.clone()
                } else {
                    button.label.clone()
                };
                format!("{}: {text}", i.saturating_add(1))
            })
            .collect();
        let current = disccraft_menu::button_routes(menu, &self.session.project.settings.routing)
            .get(index)
            .copied()
            .unwrap_or((0, 0, 0, 0));
        let directions = [
            ("up", tl!("Up:"), current.0),
            ("down", tl!("Down:"), current.1),
            ("left", tl!("Left:"), current.2),
            ("right", tl!("Right:"), current.3),
        ];
        let mut change: Option<(&str, u8)> = None;
        for (key, label, value) in directions {
            ui.horizontal(|ui| {
                prop_label(ui, label);
                let text = usize::from(value)
                    .checked_sub(1)
                    .and_then(|i| names.get(i).cloned())
                    .unwrap_or_else(|| tl!("None").to_owned());
                egui::ComboBox::from_id_salt(("button_route", id.0, index, key))
                    .selected_text(text)
                    .width(FIELD_WIDTH)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(value == 0, tl!("None")).clicked() {
                            change = Some((key, 0));
                        }
                        for (i, name) in names.iter().enumerate().take(count) {
                            let number = u8::try_from(i.saturating_add(1)).unwrap_or(u8::MAX);
                            if ui.selectable_label(value == number, name).clicked() {
                                change = Some((key, number));
                            }
                        }
                    });
            });
        }
        if let Some((key, number)) = change {
            let _ = self.run_command(
                "button.setRoutes",
                serde_json::json!({ "menu": id.0, "button": index, key: number }),
            );
        }
    }

    /// スライドショー。
    /// スライドショー（Encore の「基本」: 名前、説明、エンドアクション、スライドのデュレーション、全体のデュレーション）。
    fn slideshow_sheet(&mut self, ui: &mut Ui, id: u64) {
        sheet_title(ui, tl!("Slideshow"));
        self.name_row(ui);

        let Some(slideshow) = self.session.project.slideshow(id).cloned() else {
            return;
        };
        self.description_row(ui, "slideshow", id, &slideshow.description);
        ui.separator();
        let targets = link_targets(&self.session.project, false);
        let port = Port::EndAction(NodeId::Slideshow(id));
        if let Some(link) = self.link_row(
            ui,
            tl!("End Action:"),
            port,
            &slideshow.end_action,
            &targets,
        ) {
            self.set_slideshow_end_action(id, &link);
        }
        ui.separator();
        ui.horizontal(|ui| {
            prop_label(ui, "");
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                self.slide_duration_row(ui, id, "sheet");
            });
        });
        ui.horizontal(|ui| {
            prop_label(ui, "");
            self.manual_advance_row(ui, id);
        });
        ui.separator();
        // Encore の「トランジション」タブ: トランジションとデュレーション。
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Transition:"));
            self.transition_combo(ui, id, "sheet");
        });
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Transition duration:"));
            ui.scope(|ui| {
                ui.set_max_width(FIELD_WIDTH + 8.0);
                self.transition_duration_row(ui, id, "sheet");
            });
        });
        ui.separator();

        let ticks = slideshow
            .slides
            .iter()
            .fold(0_i64, |sum, slide| sum.saturating_add(slide.duration.0));
        text_row(ui, tl!("Slides:"), &slideshow.slides.len().to_string());
        text_row(
            ui,
            tl!("Total duration:"),
            &self.duration_text(ticks_to_seconds(ticks)),
        );
    }

    /// スライドのトランジションの行: 種類（スライドショーの設定と一致 / 各種類）と、独自のときの長さ。
    fn slide_transition_rows(
        &mut self,
        ui: &mut Ui,
        id: u64,
        index: usize,
        slideshow: &disccraft_project::Slideshow,
        slide: &disccraft_project::Slide,
    ) {
        use disccraft_project::TransitionKind;
        let own = slide.transition;
        let mut chosen: Option<Option<TransitionKind>> = None;
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Transition:"));
            let current_text = match own {
                Some(transition) => crate::slideshow_view::transition_label(transition.kind),
                None => tl!("Match slideshow setting"),
            };
            egui::ComboBox::from_id_salt(("slide_transition", id, index))
                .selected_text(current_text)
                .width(200.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(own.is_none(), tl!("Match slideshow setting"))
                        .clicked()
                    {
                        chosen = Some(None);
                    }
                    for kind in TransitionKind::ALL {
                        let selected = own.is_some_and(|transition| transition.kind == kind);
                        if ui
                            .selectable_label(
                                selected,
                                crate::slideshow_view::transition_label(kind),
                            )
                            .clicked()
                        {
                            chosen = Some(Some(kind));
                        }
                    }
                });
        });
        match chosen {
            Some(None) if own.is_some() => {
                let _ = self.run_command(
                    "slideshow.setSlideTransition",
                    json!({"slideshow": id, "slide": index, "kind": serde_json::Value::Null}),
                );
            }
            Some(Some(kind)) if own.map(|transition| transition.kind) != Some(kind) => {
                let _ = self.run_command(
                    "slideshow.setSlideTransition",
                    json!({"slideshow": id, "slide": index, "kind": kind.as_str()}),
                );
            }
            _ => {}
        }
        // 長さ: 独自のトランジションのときだけ変えられる（一致のときはスライドショーの長さを薄く）。
        let duration = format_time(
            own.map_or(slideshow.transition_duration, |transition| {
                transition.duration
            }),
            &self.session.project.settings,
        );
        let mut typed = None;
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Transition duration:"));
            ui.add_enabled_ui(own.is_some(), |ui| {
                ui.set_max_width(FIELD_WIDTH + 8.0);
                typed = committed_text(
                    ui,
                    &format!("slide_transition_duration_{id}_{index}"),
                    &duration,
                    false,
                )
                .1;
            });
        });
        if let (Some(text), Some(transition)) = (typed, own) {
            let _ = self.run_command(
                "slideshow.setSlideTransition",
                json!({"slideshow": id, "slide": index, "kind": transition.kind.as_str(), "timecode": text.trim()}),
            );
        }
    }

    /// スライドショービューアで選んだ、`id` のスライドショーのスライドの番号。
    fn focused_slide(&self, id: u64) -> Option<usize> {
        let (show, index) = self.slide_focus?;
        let count = self.session.project.slideshow(id)?.slides.len();
        (show == id && index < count).then_some(index)
    }

    /// スライド（Encore の「基本」のうち DiscCraft にあるもの: 名前、場所、形式、デュレーション、スライドショーの
    /// 設定と一致、インポイント、アウトポイント）。
    fn slide_sheet(&mut self, ui: &mut Ui, id: u64, index: usize) {
        sheet_title(ui, tl!("Slide"));
        let Some(slideshow) = self.session.project.slideshow(id).cloned() else {
            return;
        };
        let Some(slide) = slideshow.slides.get(index).cloned() else {
            return;
        };
        let settings = self.session.project.settings.clone();
        let asset = self.session.project.assets.get(&slide.asset).cloned();
        let name = asset
            .as_ref()
            .map_or_else(|| tl!("Unknown").to_owned(), |asset| asset.name.clone());
        text_row(ui, tl!("Name:"), &name);
        ui.separator();
        if let Some(asset) = &asset {
            text_row(ui, tl!("Location:"), &asset.path);
            text_row(
                ui,
                tl!("Format:"),
                crate::project_panel::asset_kind_label(asset.kind),
            );
            ui.separator();
        }

        // デュレーション（タイムコードで入力）と、スライドショーの設定と一致のチェック。
        let duration = format_time(slide.duration, &settings);
        let mut typed = None;
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Duration:"));
            let (_, committed) = committed_text(
                ui,
                &format!("slide_duration_{id}_{index}"),
                &duration,
                false,
            );
            typed = committed;
        });
        if let Some(text) = typed {
            let _ = self.run_command(
                "slideshow.setSlideDuration",
                json!({"slideshow": id, "slide": index, "timecode": text.trim()}),
            );
        }
        let matches = slide.duration == slideshow.slide_duration;
        let mut wanted = matches;
        ui.horizontal(|ui| {
            prop_label(ui, "");
            let _ = ui.checkbox(&mut wanted, tl!("Match slideshow setting"));
        });
        if wanted && !matches {
            let _ = self.run_command(
                "slideshow.setSlideDuration",
                json!({"slideshow": id, "slide": index, "time": slideshow.slide_duration.0}),
            );
        }
        ui.separator();
        // スケール（Encore と同じ 3 択）。
        let mut chosen_scale = None;
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Scale:"));
            egui::ComboBox::from_id_salt(("slide_scale", id, index))
                .selected_text(slide_scale_label(slide.scale))
                .width(200.0)
                .show_ui(ui, |ui| {
                    for scale in disccraft_project::SlideScale::ALL {
                        if ui
                            .selectable_label(slide.scale == scale, slide_scale_label(scale))
                            .clicked()
                        {
                            chosen_scale = Some(scale);
                        }
                    }
                });
        });
        if let Some(scale) = chosen_scale.filter(|scale| *scale != slide.scale) {
            let _ = self.run_command(
                "slideshow.setSlideScale",
                json!({"slideshow": id, "slide": index, "scale": scale.as_str()}),
            );
        }
        ui.separator();
        // トランジション（Encore のスライドの「トランジション」タブ）: スライドショーの設定か、このスライドだけの
        // もの。1 枚目には切り替わりがない。
        if index > 0 {
            self.slide_transition_rows(ui, id, index, &slideshow, &slide);
            ui.separator();
        }
        let in_point = slideshow
            .slides
            .iter()
            .take(index)
            .fold(0_i64, |sum, slide| sum.saturating_add(slide.duration.0));
        let out_point = in_point.saturating_add(slide.duration.0);
        text_row(
            ui,
            tl!("In point:"),
            &format_time(disccraft_project::Tick(in_point), &settings),
        );
        text_row(
            ui,
            tl!("Out point:"),
            &format_time(disccraft_project::Tick(out_point), &settings),
        );
    }

    /// 再生リスト（Encore のシート: 名前 / タイムライン の一覧（ピックウィップ、▶ の リンクの指定、上へ / 下へ、
    /// 削除）/ エンドアクション。項目はタイムライン・スライドショー・チャプター再生リスト）。
    fn playlist_sheet(&mut self, ui: &mut Ui, id: u64) {
        sheet_title(ui, tl!("Playlist"));
        self.name_row(ui);
        ui.separator();

        let Some(playlist) = self.session.project.playlist(id).cloned() else {
            return;
        };
        self.playlist_items_rows(ui, id, &playlist);
        ui.separator();

        let targets = link_targets(&self.session.project, false);
        let port = Port::EndAction(NodeId::Playlist(id));
        let mut chosen = None;
        let _ = ui
            .add_enabled_ui(!playlist.items.is_empty(), |ui| {
                chosen =
                    self.link_row(ui, tl!("End Action:"), port, &playlist.end_action, &targets);
            })
            .response
            .on_disabled_hover_text(tl!("Add an item to the playlist first"));
        if let Some(link) = chosen {
            self.set_playlist_end_action(id, &link);
        }
    }

    /// 「タイムライン:」の一覧と、その右の ピックウィップ / ▶ / 上へ / 下へ / 削除。
    fn playlist_items_rows(
        &mut self,
        ui: &mut Ui,
        id: u64,
        playlist: &disccraft_project::Playlist,
    ) {
        use disccraft_project::PlaylistItem;
        let pick = self
            .playlist_item_pick
            .filter(|(list, index)| *list == id && *index < playlist.items.len())
            .map(|(_, index)| index);
        let mut picked = None;
        let mut removed = None;
        let mut added: Option<PlaylistItem> = None;
        let mut moved = None;
        ui.horizontal_top(|ui| {
            prop_label(ui, tl!("Timelines:"));
            let frame = egui::Frame::group(ui.style()).inner_margin(4.0);
            frame.show(ui, |ui| {
                ui.set_width((ui.available_width() - 64.0).max(160.0));
                ui.set_min_height(120.0);
                egui::ScrollArea::vertical()
                    .id_salt(("playlist_items", id))
                    .max_height(160.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                        if playlist.items.is_empty() {
                            ui.weak(tl!("Drag timelines, slideshows or chapter playlists here, or use ▶."));
                        }
                        for (index, item) in playlist.items.iter().enumerate() {
                            let name = item
                                .name_in(&self.session.project)
                                .unwrap_or_else(|| tl!("(missing)").to_string());
                            let text = format!("{} {}", index.saturating_add(1), name);
                            let response = ui.selectable_label(pick == Some(index), text);
                            if response.clicked() {
                                picked = Some(index);
                            }
                            if response.double_clicked() {
                                removed = Some(index);
                            }
                        }
                        });
                    });
            });
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                self.pick_whip(ui, Port::PlaylistItems(id));
                let _ = ui
                    .menu_button("▶", |ui| {
                        ui.menu_button(tl!("Specify Link"), |ui| {
                            for (name, item) in playlist_candidates(&self.session.project) {
                                if ui.button(name).clicked() {
                                    added = Some(item);
                                    ui.close();
                                }
                            }
                        });
                        if ui
                            .add_enabled(pick.is_some(), egui::Button::new(tl!("Delete")))
                            .clicked()
                        {
                            removed = pick;
                            ui.close();
                        }
                    })
                    .response
                    .on_hover_text(tl!("Add an item or remove the selected one"));
                let can_up = pick.is_some_and(|index| index > 0);
                let can_down =
                    pick.is_some_and(|index| index.saturating_add(1) < playlist.items.len());
                if ui
                    .add_enabled(can_up, egui::Button::new("▲"))
                    .on_hover_text(tl!("Up"))
                    .clicked()
                    && let Some(index) = pick
                {
                    moved = Some((index, index.saturating_sub(1)));
                }
                if ui
                    .add_enabled(can_down, egui::Button::new("▼"))
                    .on_hover_text(tl!("Down"))
                    .clicked()
                    && let Some(index) = pick
                {
                    moved = Some((index, index.saturating_add(1)));
                }
                if ui
                    .add_enabled(pick.is_some(), egui::Button::new(tl!("Remove")))
                    .on_hover_text(tl!("Remove the selected entry (or double-click it)"))
                    .clicked()
                {
                    removed = pick;
                }
            });
        });
        if let Some(index) = picked {
            self.playlist_item_pick = Some((id, index));
        }
        if let Some(item) = added {
            self.add_playlist_entry(id, item);
        }
        if let Some((from, to)) = moved {
            self.move_playlist_item(id, from, to);
        }
        if let Some(index) = removed {
            self.remove_playlist_item(id, index);
        }
    }

    /// チャプター再生リスト（Encore のシート: 名前 / 説明 / エンドアクション / 上書き / カラーセット / ユーザー操作 /
    /// 縦横比 / エンコード後のサイズ / デュレーション / オーディオトラック / 字幕トラック。カラーセットはない）。
    fn chapter_playlist_sheet(&mut self, ui: &mut Ui, id: u64) {
        sheet_title(ui, tl!("Chapter Playlist"));
        self.name_row(ui);

        let Some(playlist) = self.session.project.chapter_playlist(id).cloned() else {
            return;
        };
        self.description_row(ui, "chapterPlaylist", id, &playlist.description);
        ui.separator();
        let targets = link_targets(&self.session.project, false);
        let port = Port::EndAction(NodeId::ChapterPlaylist(id));
        if let Some(link) =
            self.link_row(ui, tl!("End Action:"), port, &playlist.end_action, &targets)
        {
            self.set_chapter_playlist_end_action(id, &link);
        }
        let can_override = matches!(
            playlist.end_action,
            Link::Timeline { .. }
                | Link::Slideshow { .. }
                | Link::Playlist { .. }
                | Link::ChapterPlaylist { .. }
        );
        let current = playlist.override_link.clone().unwrap_or(Link::NotSet);
        let mut chosen = None;
        let _ = ui
            .add_enabled_ui(can_override, |ui| {
                chosen = self.link_row(
                    ui,
                    tl!("Override:"),
                    Port::ChapterPlaylistOverride(id),
                    &current,
                    &targets,
                );
            })
            .response
            .on_disabled_hover_text(tl!(
                "Set the end action to a timeline, slideshow or playlist first"
            ));
        if let Some(link) = chosen {
            let _ = self.run_link_command_pub(
                "chapterplaylist.setOverride",
                serde_json::json!({ "chapterPlaylist": id }),
                &link,
            );
        }
        ui.separator();
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Playback:"));
            self.playback_order_combo(ui, id, playlist.order, "sheet");
        });
        self.user_ops_row(
            ui,
            crate::user_ops::UserOpsTarget::ChapterPlaylist(id),
            &playlist.prohibited_ops,
        );
        ui.separator();

        let master = self.session.project.timeline(playlist.timeline).cloned();
        let seconds = self.chapter_playlist_seconds(id);
        let aspect = master.as_ref().map_or("--", |timeline| {
            if timeline.wide(&self.session.project.settings) {
                "16:9"
            } else {
                "4:3"
            }
        });
        text_row(ui, tl!("Aspect ratio:"), aspect);
        text_row(
            ui,
            tl!("Master timeline:"),
            &master.as_ref().map_or_else(
                || tl!("(missing)").to_string(),
                |timeline| timeline.name.clone(),
            ),
        );
        text_row(ui, tl!("Entries:"), &playlist.chapters.len().to_string());
        if let Some(seconds) = seconds {
            text_row(ui, tl!("Duration:"), &self.duration_text(seconds));
        }
        if let Some(timeline) = &master {
            text_row(
                ui,
                tl!("Audio tracks:"),
                &timeline.audio.len().max(1).to_string(),
            );
            text_row(
                ui,
                tl!("Subtitle tracks:"),
                &timeline.subtitles.len().to_string(),
            );
        }
    }

    /// 説明の行（Encore のプロパティの「説明」。フォーカスが外れたときに確定）。
    fn description_row(&mut self, ui: &mut Ui, kind: &str, id: u64, current: &str) {
        let mut changed = None;
        ui.horizontal_top(|ui| {
            prop_label(ui, tl!("Description:"));
            changed = committed_text(ui, &format!("{kind}_description"), current, true).1;
        });
        if let Some(description) = changed {
            let _ = self.run_command(
                "project.setDescription",
                serde_json::json!({ "kind": kind, "id": id, "description": description }),
            );
        }
    }

    /// 名前の行（Enter で確定。名前の変更 Alt+Ctrl+R でここにフォーカスが来る）。
    fn name_row(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            prop_label(ui, tl!("Name:"));
            self.rename_field(ui);
        });
    }

    /// リンクの行: ラベル、ピックウィップ、選択欄。選ばれたリンクを返す。
    fn link_row(
        &mut self,
        ui: &mut Ui,
        label: &str,
        port: Port,
        current: &Link,
        targets: &[(String, Link)],
    ) -> Option<Link> {
        let mut selected = None;
        let current_label = link_label(&self.session.project, current);
        ui.horizontal(|ui| {
            prop_label(ui, label);
            self.pick_whip(ui, port);
            egui::ComboBox::from_id_salt(("link_row", format!("{port:?}")))
                .width(FIELD_WIDTH)
                .selected_text(current_label)
                .show_ui(ui, |ui| {
                    for (name, link) in targets {
                        if ui.selectable_label(current == link, name).clicked() {
                            selected = Some(link.clone());
                        }
                    }
                });
            // Encore のリンクの欄の ▶: コピー / 貼り付け / 削除、項目ごとのサブメニュー（メニューのボタン、
            // タイムラインのチャプター）、前のメニューに戻る / 停止。
            let _ = ui
                .menu_button("▶", |ui| {
                    if let Some(link) = self.link_menu(ui, current, targets) {
                        selected = Some(link);
                        ui.close();
                    }
                    ui.separator();
                    // Encore の ▶ の最後の「リンクの指定…」: この欄のためのダイアログ。
                    if ui.button(tl!("Specify Link…")).clicked() {
                        self.open_link_dialog_for(port, current);
                        ui.close();
                    }
                })
                .response
                .on_hover_text(tl!("Link"));
        });
        selected
    }

    /// The ▶ menu of a link field. Returns the chosen link.
    pub(crate) fn link_menu(
        &mut self,
        ui: &mut Ui,
        current: &Link,
        targets: &[(String, Link)],
    ) -> Option<Link> {
        let mut chosen = None;
        let allowed = |link: &Link| targets.iter().any(|(_, target)| target == link);
        if ui
            .add_enabled(*current != Link::NotSet, egui::Button::new(tl!("Copy")))
            .clicked()
        {
            self.link_clipboard = Some(current.clone());
            ui.close();
        }
        let paste = self.link_clipboard.clone().filter(|link| allowed(link));
        if ui
            .add_enabled(paste.is_some(), egui::Button::new(tl!("Paste")))
            .clicked()
        {
            chosen = paste;
        }
        if ui
            .add_enabled(
                *current != Link::NotSet && allowed(&Link::NotSet),
                egui::Button::new(tl!("Delete")),
            )
            .clicked()
        {
            chosen = Some(Link::NotSet);
        }
        ui.separator();

        let project = self.session.project.clone();
        // メニュー: 初期設定のボタンと、ボタンごと。
        for (id, menu) in &project.menus {
            let whole = Link::Menu {
                id: *id,
                button: None,
            };
            if !allowed(&whole) {
                continue;
            }
            ui.menu_button(menu.name.as_str(), |ui| {
                if ui.button(tl!("Default")).clicked() {
                    chosen = Some(whole.clone());
                }
                for (index, button) in menu.buttons.iter().enumerate() {
                    let Ok(number) = u32::try_from(index) else {
                        continue;
                    };
                    let text = if button.label.trim().is_empty() {
                        tlf!("Button {n}", n = index.saturating_add(1))
                    } else {
                        button.label.clone()
                    };
                    if ui.button(text).clicked() {
                        chosen = Some(Link::Menu {
                            id: *id,
                            button: Some(number),
                        });
                    }
                }
            });
        }
        // タイムライン: チャプターごと（チャプターを選べない欄は 1 つ目だけ）。
        for (id, timeline) in &project.timelines {
            let chapters: Vec<(String, Link)> = targets
                .iter()
                .filter(
                    |(_, link)| matches!(link, Link::Timeline { id: target, .. } if target == id),
                )
                .map(|(_, link)| (link_sub_label(&project, link), link.clone()))
                .collect();
            if chapters.is_empty() {
                continue;
            }
            ui.menu_button(timeline.name.as_str(), |ui| {
                for (text, link) in chapters {
                    if ui.button(text).clicked() {
                        chosen = Some(link);
                    }
                }
            });
        }
        // スライドショーと再生リストはそのまま選ぶ。
        for (text, link) in targets {
            if matches!(link, Link::Slideshow { .. } | Link::Playlist { .. })
                && ui.button(text.as_str()).clicked()
            {
                chosen = Some(link.clone());
            }
        }
        ui.separator();
        for link in [Link::LastMenu, Link::Stop] {
            if ui
                .add_enabled(
                    allowed(&link),
                    egui::Button::new(link_label(&project, &link)),
                )
                .clicked()
            {
                chosen = Some(link);
            }
        }
        chosen
    }

    fn asset_display_name(&self, id: AssetId) -> String {
        self.session
            .project
            .assets
            .get(&id)
            .map(|asset| asset.name.clone())
            .unwrap_or_else(|| tl!("Unknown").to_string())
    }
}

/// 入力欄。入力中の文字は欄が持ち、フォーカスが外れたとき（1 行の欄は Enter でも）に、変わっていれば
/// その文字を返す。フォーカスがないときは `current` を映す。
pub(crate) fn committed_text(
    ui: &mut Ui,
    salt: &str,
    current: &str,
    multiline: bool,
) -> (egui::Response, Option<String>) {
    let id = egui::Id::unique(salt);
    let buffer_id = id.with("buffer");
    let mut buffer = ui
        .data(|data| data.get_temp::<String>(buffer_id))
        .unwrap_or_else(|| current.to_owned());
    let edit = if multiline {
        egui::TextEdit::multiline(&mut buffer).desired_rows(6)
    } else {
        egui::TextEdit::singleline(&mut buffer)
    };
    // Encore と同じく、パネルの右端まで広げる。
    let width = (ui.available_width() - 8.0).max(FIELD_WIDTH);
    let response = ui.add(edit.id(id).desired_width(width));
    let committed = (response.lost_focus() && buffer != current).then(|| buffer.clone());
    if response.has_focus() {
        ui.data_mut(|data| data.insert_temp(buffer_id, buffer));
    } else {
        ui.data_mut(|data| data.remove::<String>(buffer_id));
    }
    (response, committed)
}

/// The part of a link's label after the item's name: a chapter ("チャプター 1") or a menu's button.
fn link_sub_label(project: &disccraft_project::Project, link: &Link) -> String {
    let label = link_label(project, link);
    match label.split_once(':') {
        Some((_, rest)) => rest.trim().to_owned(),
        None => label,
    }
}

/// スライドのスケールの名前（Encore の一覧の名前）。
fn slide_scale_label(scale: disccraft_project::SlideScale) -> &'static str {
    match scale {
        disccraft_project::SlideScale::ScaleAndMatte => tl!("Scale and Apply Matte"),
        disccraft_project::SlideScale::ScaleAndCrop => tl!("Scale and Crop"),
        disccraft_project::SlideScale::NoScale => tl!("Do Not Scale"),
    }
}

/// Encore の「メディアのカテゴリ」: 素材の中身の種類。
pub(crate) fn media_category(kind: disccraft_project::AssetKind) -> &'static str {
    use disccraft_project::AssetKind;
    match kind {
        AssetKind::Video => tl!("Video and audio"),
        AssetKind::Audio => tl!("Audio"),
        AssetKind::Image => tl!("Still image"),
        AssetKind::Subtitle => tl!("Subtitles"),
        AssetKind::Other => tl!("Other"),
    }
}

/// シートの見出し（選択中のものの種類）。
/// What a playlist can hold, with Encore's names: every timeline, slideshow and chapter playlist.
fn playlist_candidates(
    project: &disccraft_project::Project,
) -> Vec<(String, disccraft_project::PlaylistItem)> {
    use disccraft_project::PlaylistItem;
    let mut out = Vec::new();
    for (id, timeline) in &project.timelines {
        out.push((
            tlf!("Timeline: {name}", name = timeline.name),
            PlaylistItem::Timeline(*id),
        ));
    }
    for (id, slideshow) in &project.slideshows {
        out.push((
            tlf!("Slideshow: {name}", name = slideshow.name),
            PlaylistItem::Slideshow(*id),
        ));
    }
    for (id, playlist) in &project.chapter_playlists {
        out.push((
            tlf!("Chapter Playlist: {name}", name = playlist.name),
            PlaylistItem::ChapterPlaylist(*id),
        ));
    }
    out
}

pub(crate) fn sheet_title(ui: &mut Ui, title: &str) {
    let height = ui.spacing().interact_size.y + 4.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rect, 2.0, crate::theme::HEADER);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        title,
        egui::FontId::proportional(14.0),
        crate::theme::TEXT_STRONG,
    );
    ui.add_space(4.0);
}

/// A button kind as Encore shows it.
pub(crate) fn button_kind_label(kind: disccraft_project::ButtonKind) -> &'static str {
    use disccraft_project::ButtonKind;
    match kind {
        ButtonKind::Normal => tl!("Normal"),
        ButtonKind::Chapter => tl!("Chapter"),
        ButtonKind::Next => tl!("Next"),
        ButtonKind::Previous => tl!("Previous"),
        ButtonKind::Main => tl!("Main"),
    }
}

/// 右寄せのラベル（幅をそろえる）。
pub(crate) fn prop_label(ui: &mut Ui, text: &str) {
    // 長いラベル（DVD トランスコードステータス: など）は左に切れないよう、その幅まで広げる。
    let text_width = egui::WidgetText::from(text)
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        )
        .size()
        .x;
    // 同じシートのいちばん長いラベルに幅をそろえる（Encore と同じく右寄せの列になる）。幅は前のフレームで
    // 測ったもの。
    let wanted = text_width + ui.spacing().item_spacing.x;
    let key = egui::Id::unique("prop_label_column");
    let pass = ui.ctx().cumulative_pass_nr();
    let (seen_pass, column, growing) = ui
        .data(|data| data.get_temp::<(u64, f32, f32)>(key))
        .unwrap_or((pass, LABEL_WIDTH, LABEL_WIDTH));
    let (column, growing) = if seen_pass == pass {
        (column, growing.max(wanted))
    } else {
        (growing, wanted.max(LABEL_WIDTH))
    };
    ui.data_mut(|data| data.insert_temp(key, (pass, column, growing)));
    if wanted > column {
        ui.ctx().request_repaint();
    }
    let size = egui::vec2(column.max(wanted), ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(size, Layout::right_to_left(Align::Center), |ui| {
        ui.label(text);
    });
}

/// ディスク容量の表示名（BDXL は層の数も）。
fn capacity_name(capacity: Capacity) -> &'static str {
    match capacity {
        Capacity::Dvd5 => "DVD5 (4.7 GB)",
        Capacity::Dvd9 => "DVD9 (8.5 GB)",
        Capacity::Bd25 => "BD25 (25 GB)",
        Capacity::Bd50 => "BD50 (50 GB)",
        Capacity::Bd100 => tl!("BD100 (100 GB, BDXL 3 layers)"),
        Capacity::Bd128 => tl!("BD128 (128 GB, BDXL 4 layers)"),
        Capacity::Custom { .. } => tl!("Custom"),
    }
}

/// 動画の背景にしたときのループの初めの長さ（秒）。
const DEFAULT_LOOP_SECONDS: u32 = 10;
/// モーションメニューのループの最長（秒。検査と同じ）。
const MAX_LOOP_SECONDS: u32 = 30;

/// アセットを 1 つ選ぶ行。選び直したらそのアセットを返す。
fn asset_choice(
    ui: &mut Ui,
    id: &str,
    label: &str,
    assets: &[(AssetId, String)],
    current: Option<AssetId>,
) -> Option<AssetId> {
    let text = current
        .and_then(|asset| assets.iter().find(|(id, _)| *id == asset))
        .map_or_else(|| tl!("None").to_owned(), |(_, name)| name.clone());
    let mut chosen = None;
    ui.horizontal(|ui| {
        prop_label(ui, label);
        egui::ComboBox::from_id_salt(id)
            .width(FIELD_WIDTH)
            .selected_text(text)
            .show_ui(ui, |ui| {
                for (asset, name) in assets {
                    if ui.selectable_label(current == Some(*asset), name).clicked() {
                        chosen = Some(*asset);
                    }
                }
            });
    });
    chosen.filter(|asset| Some(*asset) != current)
}

/// デュレーションを静止から秒数にしたときの初めの値（秒）。
const DEFAULT_MENU_SECONDS: u32 = 30;

/// ボタンの一覧での表示（「1: 本編を再生」。テキストがなければ「ボタン 1」）。
fn button_title(index: usize, label: &str) -> String {
    let number = index.saturating_add(1);
    if label.trim().is_empty() {
        tlf!("Button {n}", n = number)
    } else {
        format!("{number}: {label}")
    }
}

/// 1 行の入力欄で Enter が押されて確定したか。
fn committed(ui: &Ui, response: &egui::Response) -> bool {
    response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))
}

/// 読み取り専用の行。
pub(crate) fn text_row(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        prop_label(ui, label);
        // 値が長いとき（場所のパス、トランスコードステータス）はパネルの幅で折り返す。
        ui.add(egui::Label::new(value).wrap());
    });
}

/// 選択欄の行。選ばれた値を返す。
/// A row of radio buttons (Encore's 4:3 / 16:9 and authoring mode). Returns the choice when it changes.
fn radio_row<T: Copy + PartialEq>(
    ui: &mut Ui,
    label: &str,
    current: T,
    choices: &[(T, &str)],
    enabled: bool,
) -> Option<T> {
    let mut selected = None;
    ui.horizontal(|ui| {
        prop_label(ui, label);
        ui.add_enabled_ui(enabled, |ui| {
            for (value, text) in choices {
                if ui.radio(*value == current, *text).clicked() && *value != current {
                    selected = Some(*value);
                }
            }
        })
        .response
        .on_disabled_hover_text(tl!("Not available here."));
    });
    selected
}

/// An audio track combo: 変更なし と 1 … 8。
pub(crate) fn audio_choice_row(
    ui: &mut Ui,
    label: &str,
    id: &str,
    current: Option<u8>,
) -> Option<Option<u8>> {
    let numbers: Vec<String> = (1..=8).map(|n| n.to_string()).collect();
    let mut audio: Vec<(Option<u8>, &str)> = vec![(None, tl!("No Change"))];
    for (index, text) in numbers.iter().enumerate() {
        if let Ok(n) = u8::try_from(index + 1) {
            audio.push((Some(n), text.as_str()));
        }
    }
    choice_row(ui, label, id, current, &audio)
}

/// A subtitle combo: 変更なし / オフ と 1 … 32。
pub(crate) fn subtitle_choice_row(
    ui: &mut Ui,
    label: &str,
    id: &str,
    current: disccraft_project::SubtitleDefault,
) -> Option<disccraft_project::SubtitleDefault> {
    use disccraft_project::SubtitleDefault;
    let numbers: Vec<String> = (1..=32).map(|n| n.to_string()).collect();
    let mut subtitles: Vec<(SubtitleDefault, &str)> = vec![
        (SubtitleDefault::NoChange, tl!("No Change")),
        (SubtitleDefault::Off, tl!("Off")),
    ];
    for (index, text) in numbers.iter().enumerate() {
        if let Ok(n) = u8::try_from(index + 1) {
            subtitles.push((SubtitleDefault::Stream(n), text.as_str()));
        }
    }
    choice_row(ui, label, id, current, &subtitles)
}

/// The command value of a subtitle choice (`null` / `"off"` / the track number).
pub(crate) fn subtitle_value(choice: disccraft_project::SubtitleDefault) -> serde_json::Value {
    use disccraft_project::SubtitleDefault;
    match choice {
        SubtitleDefault::NoChange => serde_json::Value::Null,
        SubtitleDefault::Off => serde_json::Value::from("off"),
        SubtitleDefault::Stream(n) => serde_json::Value::from(n),
    }
}

fn choice_row<T: Copy + PartialEq>(
    ui: &mut Ui,
    label: &str,
    id: &str,
    current: T,
    choices: &[(T, &str)],
) -> Option<T> {
    let mut selected = None;
    let current_text = choices
        .iter()
        .find(|(value, _)| *value == current)
        .map_or("", |(_, text)| *text);
    ui.horizontal(|ui| {
        prop_label(ui, label);
        egui::ComboBox::from_id_salt(id)
            .width(FIELD_WIDTH)
            .selected_text(current_text)
            .show_ui(ui, |ui| {
                for (value, text) in choices {
                    if ui.selectable_label(*value == current, *text).clicked() {
                        selected = Some(*value);
                    }
                }
            });
    });
    selected
}

/// テレビ方式の画面サイズ（今のトランスコードの出力）。
pub(crate) fn frame_size(standard: VideoStandard) -> (u32, u32) {
    match standard {
        VideoStandard::Ntsc => (720, 480),
        VideoStandard::Pal => (720, 576),
    }
}

/// フレームレートの表示（29.97 fps など）。
fn frame_rate_text(standard: VideoStandard) -> String {
    let (num, den) = frame_rate(standard);
    fps_text(f64::from(num) / f64::from(den.max(1)))
}

/// fps の値の表示（24 fps、29.97 fps など）。
pub(crate) fn fps_text(fps: f64) -> String {
    if (fps - fps.round()).abs() < 0.001 {
        format!("{fps:.0} fps")
    } else {
        format!("{fps:.2} fps")
    }
}

#[cfg(test)]
mod tests {
    use super::{frame_rate_text, frame_size};
    use disccraft_project::VideoStandard;

    #[test]
    fn frame_rates_read_like_the_standard() {
        assert_eq!(frame_rate_text(VideoStandard::Ntsc), "29.97 fps");
        assert_eq!(frame_rate_text(VideoStandard::Pal), "25 fps");
        assert_eq!(frame_size(VideoStandard::Pal), (720, 576));
    }
}
