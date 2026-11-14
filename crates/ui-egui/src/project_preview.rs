//! プロジェクトプレビュー（Encore の ファイル ▸ プレビュー）。ビルドせずに、プロジェクトのディスクを別の窓で
//! 試す: ナビゲーションはビルドが書くのと同じモデル（`disccraft_dvd::preview_model`）を VM で動かし、メニューは
//! プロジェクトから描いてハイライトを重ね、タイトルは素材から映像と音つきで再生する（[`crate::preview_playback`]）。
//!
//! 窓の下の操作バーは Encore と同じ並び: 拡大率、オーディオと字幕、今の項目（名前、ボタンかチャプター、
//! 静止画か再生の時刻）、タイトル / メニュー、十字キーと決定、前のチャプター・停止・一時停止・再生・次のチャプター・
//! チャプター番号、そして「ここで終了」（見ている項目を開いて終わる）と「終了して戻る」（開く前の選択に戻る）。

use crate::DiscCraftApp;
use crate::app::Selection;
use crate::i18n::{tl, tlf};
use crate::icons::{self, Icon};
use crate::monitor::{MonitorZoom, RemoteKey, parse_preview_state};
use crate::preview_playback::title_timeline;

use egui::{Context, Ui, ViewportBuilder, ViewportId};
use serde_json::{Value, json};

/// 開いているプロジェクトプレビュー。
#[derive(Clone)]
pub(crate) struct ProjectPreview {
    /// 開く前の選択（「終了して戻る」で戻る）。
    return_to: Selection,
    /// チャプター番号の欄。
    chapter_text: String,
    /// 映像の拡大率。
    zoom: MonitorZoom,
}

/// プレビューの終わり方。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Exit {
    /// ここで終了: 見ているメニュー / タイムライン（その時刻）を開いて終わる。
    Here,
    /// 終了して戻る: 開く前の選択に戻って終わる（窓の × も同じ）。
    Return,
}

/// 再生中のタイトルに重ねる Blu-ray のポップアップメニューの絵: メニューを黒の上と白の上に 1 回ずつ描き、
/// 2 枚の差から画素ごとの色と不透明度を出す（ポップアップの地は透けて、下のタイトルが見える）。メニューが
/// 変わるまで覚えておく。
pub(crate) struct PopupTexture {
    /// 描いたメニューの `picture_hash`。
    hash: u64,
    /// 透明度つきの絵。
    texture: egui::TextureHandle,
    /// メニューのフレームの大きさ。
    frame: (u16, u16),
}

/// 黒の上（`black`）と白の上（`white`）に描いた同じ絵から、透明度つきの RGBA（ストレート）を出す。不透明度は
/// 白と黒の差のいちばん小さいチャンネルから（重ねた絵の不透明度は全チャンネルで同じ）。
pub(crate) fn matte(black: &[u8], white: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(black.len() / 3 * 4);
    let (black, _) = black.as_chunks::<3>();
    let (white, _) = white.as_chunks::<3>();
    for (b, w) in black.iter().zip(white) {
        let spread = b
            .iter()
            .zip(w)
            .map(|(b, w)| w.saturating_sub(*b))
            .min()
            .unwrap_or(255);
        let alpha = 255_u8.saturating_sub(spread);
        if alpha == 0 {
            rgba.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        for channel in b {
            // Over black the colour is premultiplied by the opacity; undo it.
            let straight = (u16::from(*channel) * 255 / u16::from(alpha)).min(255);
            rgba.push(u8::try_from(straight).unwrap_or(255));
        }
        rgba.push(alpha);
    }
    rgba
}

/// 窓の大きさ（Encore のプロジェクトプレビューと同じくらい）。
const WINDOW_SIZE: [f32; 2] = [880.0, 600.0];

impl DiscCraftApp {
    /// ファイル ▸ プレビュー: ビルドせずに、プロジェクトのディスクをファーストプレイから（`start` があれば
    /// そのメニュー / タイトルから）別の窓でプレビューする。
    pub(crate) fn open_project_preview(&mut self, start: Value) {
        self.playback = None;
        self.preview_playback = None;
        self.preview_paused = false;
        self.preview_resume = None;
        let Some(state) = self.run_query("preview.open", start) else {
            return;
        };
        self.preview = parse_preview_state(&state);
        let return_to = self
            .project_preview
            .as_ref()
            .map_or(self.selection, |open| open.return_to);
        self.project_preview = Some(ProjectPreview {
            return_to,
            chapter_text: String::new(),
            zoom: MonitorZoom::Fit,
        });
        self.status = tl!("Project Preview").to_owned();
    }

    /// プロジェクトプレビューの窓（開いているときだけ）。
    pub(crate) fn project_preview_window(&mut self, ctx: &Context) {
        if self.project_preview.is_none() {
            return;
        }
        let title = if self.preview.bluray {
            tl!("Project Preview (Blu-ray)")
        } else {
            tl!("Project Preview")
        };
        let builder = ViewportBuilder::default()
            .with_title(title)
            .with_inner_size(WINDOW_SIZE)
            .with_min_inner_size([640.0, 420.0]);
        let exit = ctx.show_viewport_immediate(
            ViewportId::from_hash_of("disccraft_project_preview"),
            builder,
            |ui, _class| {
                let mut exit = None;
                if ui.ctx().input(|input| input.viewport().close_requested()) {
                    exit = Some(Exit::Return);
                }
                egui::Panel::bottom("project_preview_bar")
                    .frame(crate::theme::bar_frame())
                    .show_separator_line(false)
                    .show(ui, |ui| {
                        if let Some(chosen) = self.preview_bar(ui) {
                            exit = Some(chosen);
                        }
                    });
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(egui::Color32::BLACK))
                    .show(ui, |ui| self.preview_screen(ui));
                self.preview_keyboard(ui);
                exit
            },
        );
        if let Some(exit) = exit {
            self.exit_project_preview(ctx, exit);
        }
    }

    /// 映像: メニュー（ハイライトつき）、再生中のタイトル、それ以外は今の場所の説明。
    fn preview_screen(&mut self, ui: &mut Ui) {
        let zoom = self
            .project_preview
            .as_ref()
            .map_or(MonitorZoom::Fit, |open| open.zoom);
        let state = self.preview.clone();
        if self.preview_stopped() {
            ui.centered_and_justified(|ui| {
                ui.colored_label(
                    egui::Color32::GRAY,
                    tl!("Stopped. Press Play to start again."),
                );
            });
            return;
        }
        if let Some(menu) = &state.menu {
            let Some(project_menu) = self
                .menu_for_pgcn(menu.pgcn)
                .and_then(|id| self.session.project.menu(id).cloned())
            else {
                ui.centered_and_justified(|ui| {
                    ui.colored_label(egui::Color32::GRAY, state.label.clone());
                });
                return;
            };
            let highlight = usize::from(menu.button.saturating_sub(1));
            ui.centered_and_justified(|ui| {
                self.menu_canvas(ui, &project_menu, Some(highlight), zoom);
            });
            return;
        }
        if let Some(picture) = self.preview_picture() {
            let area = ui.max_rect();
            let aspect = if picture.wide { 16.0 / 9.0 } else { 4.0 / 3.0 };
            let frame = egui::Rect::from_center_size(area.center(), zoom.size(area.size(), aspect));
            ui.painter().image(
                picture.texture.id(),
                frame,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            if let Some(text) = &picture.subtitle {
                crate::monitor::draw_subtitle(ui.painter(), frame.intersect(area), text);
            }
            // Blu-ray のポップアップメニュー: タイトルは再生を続け、その上にメニューとハイライトを重ねる。
            if let Some(popup) = &state.popup {
                self.draw_popup(ui, popup.pgcn, popup.button, frame);
            }
            return;
        }
        ui.centered_and_justified(|ui| {
            ui.colored_label(egui::Color32::GRAY, state.label.clone());
        });
    }

    /// 再生中のタイトルの画面 `screen` に、Blu-ray のポップアップメニュー（PGCN `pgcn`）を重ね、`button` を
    /// ハイライトする。
    pub(crate) fn draw_popup(&mut self, ui: &mut Ui, pgcn: u16, button: u8, screen: egui::Rect) {
        let Some(menu) = self
            .menu_for_pgcn(pgcn)
            .and_then(|id| self.session.project.menu(id).cloned())
        else {
            return;
        };
        let hash = self.picture_hash(&menu);
        if self
            .popup_texture
            .as_ref()
            .is_none_or(|cached| cached.hash != hash)
        {
            self.popup_texture = self.render_popup(ui.ctx(), &menu, hash);
        }
        // ハイライトの形はメニューの絵の段の覚え（`menu_texture`）から。
        if self
            .menu_texture
            .as_ref()
            .is_none_or(|cached| cached.hash != hash)
        {
            self.render_menu_texture(ui.ctx(), &menu, hash);
        }
        let Some(cached) = &self.popup_texture else {
            return;
        };
        let frame = cached.frame;
        ui.painter().image(
            cached.texture.id(),
            screen,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        self.draw_buttons(
            ui,
            &menu,
            frame,
            screen,
            Some(usize::from(button.saturating_sub(1))),
        );
    }

    /// ポップアップメニューを透明度つきの絵にする（[`matte`]）。描けなければ `None`（ステータスに理由）。
    fn render_popup(
        &mut self,
        ctx: &Context,
        menu: &disccraft_project::Menu,
        hash: u64,
    ) -> Option<PopupTexture> {
        let project = &self.session.project;
        let plain = |background: Option<(&[u8], usize, usize)>| {
            disccraft_menu::render_menu_with(
                project,
                menu,
                disccraft_menu::MenuPictures {
                    background,
                    thumbnails: &[],
                },
            )
        };
        let rendered = (|| {
            let sized = plain(None)?;
            let (width, height) = (sized.width, sized.height);
            let pixels = usize::from(width) * usize::from(height) * 3;
            let black = vec![0_u8; pixels];
            let white = vec![255_u8; pixels];
            let size = (usize::from(width), usize::from(height));
            let over_black = plain(Some((&black, size.0, size.1)))?;
            let over_white = plain(Some((&white, size.0, size.1)))?;
            Ok::<_, disccraft_menu::MenuError>((
                width,
                height,
                matte(&over_black.rgb, &over_white.rgb),
            ))
        })();
        match rendered {
            Ok((width, height, rgba))
                if rgba.len() == usize::from(width) * usize::from(height) * 4 =>
            {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [usize::from(width), usize::from(height)],
                    &rgba,
                );
                Some(PopupTexture {
                    hash,
                    texture: ctx.load_texture("preview_popup", image, egui::TextureOptions::LINEAR),
                    frame: (width, height),
                })
            }
            Ok(_) => None,
            Err(error) => {
                self.status = error.to_string();
                None
            }
        }
    }

    /// 下の操作バー（Encore と同じ並び）。押された終わり方を返す。
    fn preview_bar(&mut self, ui: &mut Ui) -> Option<Exit> {
        let mut exit = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            // 拡大率。
            if let Some(open) = self.project_preview.as_mut() {
                crate::monitor::zoom_combo(ui, "project_preview_zoom", &mut open.zoom);
            }
            ui.separator();
            self.preview_streams(ui);
            ui.separator();
            self.preview_info(ui);
            ui.separator();
            self.preview_remote(ui);
            ui.separator();
            self.preview_transport(ui);
            ui.separator();
            ui.vertical(|ui| {
                if ui
                    .add_sized([110.0, 22.0], egui::Button::new(tl!("Exit Here")))
                    .on_hover_text(tl!("Close the preview and open what it shows"))
                    .clicked()
                {
                    exit = Some(Exit::Here);
                }
                if ui
                    .add_sized([110.0, 22.0], egui::Button::new(tl!("Exit and Return")))
                    .on_hover_text(tl!("Close the preview and go back to where you were"))
                    .clicked()
                {
                    exit = Some(Exit::Return);
                }
            });
        });
        exit
    }

    /// オーディオと字幕の選択（タイトルの再生中だけ。数はそのタイムラインのトラックの数）。
    fn preview_streams(&mut self, ui: &mut Ui) {
        let (audio_count, subtitle_count) = self.preview_stream_counts();
        let audio = self.preview.audio.unwrap_or(1);
        let subtitle = self.preview.subtitle;
        let mut chosen_audio = None;
        let mut chosen_subtitle = None;
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.add_enabled_ui(audio_count > 0, |ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                    icons::paint(ui.painter(), rect, Icon::Speaker, ui.visuals().text_color());
                    egui::ComboBox::from_id_salt("project_preview_audio")
                        .width(48.0)
                        .selected_text(audio.to_string())
                        .show_ui(ui, |ui| {
                            for track in 1..=audio_count.max(1) {
                                if ui
                                    .selectable_label(audio == track, track.to_string())
                                    .clicked()
                                    && audio != track
                                {
                                    chosen_audio = Some(track);
                                }
                            }
                        });
                })
                .response
                .on_hover_text(tl!("Audio Track"));
            });
            ui.horizontal(|ui| {
                ui.add_enabled_ui(subtitle_count > 0, |ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                    icons::paint(
                        ui.painter(),
                        rect,
                        Icon::Captions,
                        ui.visuals().text_color(),
                    );
                    let shown = subtitle.map_or_else(|| tl!("Off").to_owned(), |n| n.to_string());
                    egui::ComboBox::from_id_salt("project_preview_subtitle")
                        .width(48.0)
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(subtitle.is_none(), tl!("Off"))
                                .clicked()
                                && subtitle.is_some()
                            {
                                chosen_subtitle = Some(None);
                            }
                            for track in 1..=subtitle_count {
                                if ui
                                    .selectable_label(subtitle == Some(track), track.to_string())
                                    .clicked()
                                    && subtitle != Some(track)
                                {
                                    chosen_subtitle = Some(Some(track));
                                }
                            }
                        });
                })
                .response
                .on_hover_text(tl!("Subtitle Track"));
            });
        });
        if chosen_audio.is_some() || chosen_subtitle.is_some() {
            let mut params = serde_json::Map::new();
            if let Some(track) = chosen_audio {
                let _ = params.insert("audio".to_owned(), json!(track));
            }
            if let Some(track) = chosen_subtitle {
                let value = track.map_or_else(|| json!("off"), |n| json!(n));
                let _ = params.insert("subtitle".to_owned(), value);
            }
            if let Some(state) = self.run_query("preview.streams", Value::Object(params)) {
                self.preview = parse_preview_state(&state);
            }
        }
    }

    /// 再生中のタイトルのオーディオと字幕のトラックの数（タイムラインのとき。メニューやほかは 0）。
    fn preview_stream_counts(&self) -> (u8, u8) {
        let Some((title, _)) = self.preview.title.filter(|_| self.preview.open) else {
            return (0, 0);
        };
        let project = &self.session.project;
        let Some(timeline) = title_timeline(project, title).and_then(|id| project.timeline(id))
        else {
            return (0, 0);
        };
        // トラックがなければ、映像の素材の音の 1 本。
        let audio = u8::try_from(timeline.audio.len().max(1))
            .unwrap_or(8)
            .min(8);
        let subtitles = u8::try_from(timeline.subtitles.len()).unwrap_or(32).min(32);
        (audio, subtitles)
    }

    /// 今の項目: 名前、ボタン（メニュー）かチャプター（タイトル）、静止画 / モーション か再生の時刻。
    fn preview_info(&mut self, ui: &mut Ui) {
        let (name, number, detail) = self.preview_info_lines();
        egui::Frame::new()
            .fill(egui::Color32::from_gray(96))
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(8, 3))
            .show(ui, |ui| {
                ui.set_width(170.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.add(egui::Label::new(name).truncate());
                    ui.label(number);
                    ui.label(detail);
                });
            });
    }

    fn preview_info_lines(&self) -> (String, String, String) {
        let state = &self.preview;
        if self.preview_stopped() {
            return (tl!("Stopped").to_owned(), String::new(), String::new());
        }
        let project = &self.session.project;
        if let Some(menu) = &state.menu {
            let found = self
                .menu_for_pgcn(menu.pgcn)
                .and_then(|id| project.menu(id));
            let name = found.map_or_else(|| state.label.clone(), |menu| menu.name.clone());
            let kind = if found.is_some_and(disccraft_project::Menu::is_motion) {
                tl!("Motion")
            } else {
                tl!("Still")
            };
            return (
                name,
                tlf!("Button {n}", n = format!("{:02}", menu.button)),
                kind.to_owned(),
            );
        }
        if let Some((title, chapter)) = state.title {
            let name = title_name(project, title);
            let time = match self.preview_position() {
                Some(seconds) if self.preview_paused => {
                    tlf!("Paused {time}", time = self.duration_text(seconds))
                }
                Some(seconds) => self.duration_text(seconds),
                None => String::new(),
            };
            return (name, tlf!("Chapter {n}", n = format!("{chapter:02}")), time);
        }
        (state.label.clone(), String::new(), String::new())
    }

    /// タイトル / メニューのボタンと、十字キーと決定。
    fn preview_remote(&mut self, ui: &mut Ui) {
        let open = !self.preview_stopped();
        let playing = open && self.preview.menu.is_none();
        let mut key = None;
        ui.vertical(|ui| {
            if ui
                .add_enabled(
                    open,
                    egui::Button::new(tl!("Title")).min_size(egui::vec2(52.0, 20.0)),
                )
                .clicked()
            {
                key = Some(RemoteKey::Title);
            }
            if ui
                .add_enabled(
                    playing,
                    egui::Button::new(tl!("Menu")).min_size(egui::vec2(52.0, 20.0)),
                )
                .clicked()
            {
                key = Some(RemoteKey::Menu);
            }
            // Blu-ray のリモコンのポップアップメニューのボタン（タイトルにポップアップがあるときだけ）。
            if self.preview.bluray
                && ui
                    .add_enabled(
                        playing && (self.preview.popup_available || self.preview.popup.is_some()),
                        egui::Button::new(tl!("Pop-up Menu")).min_size(egui::vec2(52.0, 20.0)),
                    )
                    .clicked()
            {
                key = Some(RemoteKey::Popup);
            }
        });
        // 十字キー: 上、左・決定・右、下。
        ui.add_enabled_ui(open, |ui| {
            egui::Grid::new("project_preview_dpad")
                .spacing([2.0, 2.0])
                .show(ui, |ui| {
                    let mut pad = |ui: &mut Ui, text: &str, pressed: RemoteKey| {
                        if ui
                            .add(egui::Button::new(text).min_size(egui::vec2(22.0, 18.0)))
                            .clicked()
                        {
                            key = Some(pressed);
                        }
                    };
                    ui.label("");
                    pad(ui, "▲", RemoteKey::Up);
                    ui.label("");
                    ui.end_row();
                    pad(ui, "◀", RemoteKey::Left);
                    pad(ui, "●", RemoteKey::Enter);
                    pad(ui, "▶", RemoteKey::Right);
                    ui.end_row();
                    ui.label("");
                    pad(ui, "▼", RemoteKey::Down);
                    ui.label("");
                    ui.end_row();
                });
        });
        if let Some(key) = key {
            self.preview_paused = false;
            self.send_remote(key);
        }
    }

    /// 前のチャプター、停止、一時停止、再生、次のチャプター、エンドアクション、チャプター番号。
    fn preview_transport(&mut self, ui: &mut Ui) {
        let stopped = self.preview_stopped();
        let open = !stopped;
        let in_title = open && self.preview.title.is_some();
        let mut action: Option<&str> = None;
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                if transport_button(ui, Icon::PrevEdit, in_title, tl!("Previous Chapter")) {
                    action = Some("previous");
                }
                // 停止: 四角を描く（Craft のアイコンには停止がない）。
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(26.0, 22.0), egui::Sense::click());
                let response = response.on_hover_text(tl!("Stop"));
                let color = if open {
                    ui.visuals().text_color()
                } else {
                    ui.visuals().weak_text_color()
                };
                ui.painter().rect_filled(
                    egui::Rect::from_center_size(rect.center(), egui::vec2(9.0, 9.0)),
                    1.0,
                    color,
                );
                if open && response.clicked() {
                    action = Some("stop");
                }
                if transport_button(
                    ui,
                    Icon::Pause,
                    in_title && !self.preview_paused,
                    tl!("Pause"),
                ) {
                    action = Some("pause");
                }
                let can_play = !open || (in_title && self.preview_paused);
                if transport_button(ui, Icon::Play, can_play, tl!("Play")) {
                    action = Some("play");
                }
                if transport_button(ui, Icon::NextEdit, in_title, tl!("Next Chapter")) {
                    action = Some("next");
                }
            });
            ui.horizontal(|ui| {
                if transport_button(ui, Icon::GoToOut, in_title, tl!("Run the End Action")) {
                    action = Some("finish");
                }
                let Some(preview) = self.project_preview.as_mut() else {
                    return;
                };
                let field = ui.add_enabled(
                    in_title,
                    egui::TextEdit::singleline(&mut preview.chapter_text)
                        .desired_width(32.0)
                        .hint_text(
                            self.preview
                                .title
                                .map(|(_, chapter)| chapter.to_string())
                                .unwrap_or_default(),
                        ),
                );
                let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (transport_button(ui, Icon::ChevronRight, in_title, tl!("Go to Chapter"))
                    || entered)
                    && !preview.chapter_text.trim().is_empty()
                {
                    action = Some("chapter");
                }
            });
        });
        match action {
            Some("previous" | "next") => {
                self.preview_paused = false;
                let key = action.unwrap_or("next");
                if let Some(state) = self.run_query("preview.key", json!({ "key": key })) {
                    self.preview = parse_preview_state(&state);
                }
            }
            Some("chapter") => {
                let text = self
                    .project_preview
                    .as_mut()
                    .map(|preview| std::mem::take(&mut preview.chapter_text))
                    .unwrap_or_default();
                match text.trim().parse::<u16>() {
                    Ok(chapter) => {
                        self.preview_paused = false;
                        if let Some(state) =
                            self.run_query("preview.chapter", json!({ "chapter": chapter }))
                        {
                            self.preview = parse_preview_state(&state);
                        }
                    }
                    Err(_) => self.status = tl!("Enter a chapter number").to_owned(),
                }
            }
            Some("stop") => {
                self.preview_paused = false;
                self.close_preview();
            }
            Some("pause") => self.preview_paused = true,
            Some("play") if stopped => {
                // 止めたあとの再生: ファーストプレイからもう一度。
                self.open_project_preview(json!({}));
            }
            Some("play") => {
                self.preview_paused = false;
                self.preview_resume = self.preview_position();
                self.preview_playback = None;
            }
            Some("finish") => {
                self.preview_paused = false;
                self.send_remote(RemoteKey::Finish);
            }
            _ => {}
        }
    }

    /// プレビューが止まっているか: 閉じた（停止ボタン）か、ディスクが止まった（エンドアクションが停止など）。
    fn preview_stopped(&self) -> bool {
        !self.preview.open || (self.preview.menu.is_none() && self.preview.title.is_none())
    }

    /// プレビューの窓でのキー: 矢印と Enter はリモコン、Space は一時停止 / 再生（文字の入力中は奪わない）。
    fn preview_keyboard(&mut self, ui: &mut Ui) {
        if !self.preview.open || ui.ctx().memory(|memory| memory.focused().is_some()) {
            return;
        }
        let pressed = |key| ui.input(|input| input.key_pressed(key));
        let remote = [
            (egui::Key::ArrowUp, RemoteKey::Up),
            (egui::Key::ArrowDown, RemoteKey::Down),
            (egui::Key::ArrowLeft, RemoteKey::Left),
            (egui::Key::ArrowRight, RemoteKey::Right),
            (egui::Key::Enter, RemoteKey::Enter),
        ]
        .into_iter()
        .find(|(key, _)| pressed(*key))
        .map(|(_, remote)| remote);
        let space = pressed(egui::Key::Space);
        if let Some(key) = remote {
            self.preview_paused = false;
            self.send_remote(key);
        } else if space && self.preview.title.is_some() {
            if self.preview_paused {
                self.preview_paused = false;
                self.preview_resume = self.preview_position();
                self.preview_playback = None;
            } else {
                self.preview_paused = true;
            }
        }
    }

    /// プレビューを閉じる。ここで終了なら見ている項目を開き（タイトルはその時刻）、戻るなら開く前の選択へ。
    fn exit_project_preview(&mut self, ctx: &Context, exit: Exit) {
        let Some(open) = self.project_preview.take() else {
            return;
        };
        let here = match exit {
            Exit::Here => self.preview_here(),
            Exit::Return => None,
        };
        self.preview_paused = false;
        self.preview_resume = None;
        self.close_preview();
        match here {
            Some((selection, seconds)) => {
                self.select(selection);
                if let (Selection::Timeline(id), Some(seconds)) = (selection, seconds) {
                    crate::timeline_view::set_playhead_seconds(ctx, id, seconds);
                }
            }
            None => self.select(open.return_to),
        }
    }

    /// 見ている項目: メニュー、タイムライン（今の時刻）。
    fn preview_here(&self) -> Option<(Selection, Option<f64>)> {
        if let Some(menu) = &self.preview.menu {
            return self
                .menu_for_pgcn(menu.pgcn)
                .map(|id| (Selection::Menu(id), None));
        }
        let (title, _) = self.preview.title?;
        let project = &self.session.project;
        if let Some(id) = title_timeline(project, title) {
            return Some((Selection::Timeline(id), self.preview_position()));
        }
        // スライドショー、チャプター再生リスト（題番号はタイムラインの後ろ）。
        let index = usize::from(title)
            .checked_sub(1)?
            .checked_sub(project.timelines.len())?;
        let slideshows = crate::preview_playback::slideshow_ids(project);
        if let Some(id) = slideshows.get(index) {
            return Some((Selection::Slideshow(*id), None));
        }
        let index = index.checked_sub(slideshows.len())?;
        project
            .chapter_playlists
            .keys()
            .nth(index)
            .map(|id| (Selection::ChapterPlaylist(*id), None))
    }
}

/// 操作バーの小さなアイコンのボタン。押されたか。
fn transport_button(ui: &mut Ui, icon: Icon, enabled: bool, tip: &str) -> bool {
    let response = ui
        .add_enabled_ui(enabled, |ui| crate::icons::icon_button(ui, icon, false))
        .inner
        .on_hover_text(tip);
    enabled && response.clicked()
}

/// 題番号のタイトルの名前: タイムライン、スライドショー、チャプター再生リストの順（ビルドと同じ数え方）。
fn title_name(project: &disccraft_project::Project, title: u8) -> String {
    if let Some(timeline) = title_timeline(project, title).and_then(|id| project.timeline(id)) {
        return timeline.name.clone();
    }
    let Some(index) = usize::from(title)
        .checked_sub(1)
        .and_then(|index| index.checked_sub(project.timelines.len()))
    else {
        return tlf!("Title {n}", n = title);
    };
    let slideshows = crate::preview_playback::slideshow_ids(project);
    if let Some(slideshow) = slideshows.get(index).and_then(|id| project.slideshow(*id)) {
        return slideshow.name.clone();
    }
    index
        .checked_sub(slideshows.len())
        .and_then(|index| project.chapter_playlists.values().nth(index))
        .map_or_else(|| tlf!("Title {n}", n = title), |list| list.name.clone())
}

#[cfg(test)]
mod popup_tests {
    use super::matte;

    /// Opaque pixels keep their colour; the see-through background becomes transparent; a half-opaque grey keeps
    /// its colour at half the opacity.
    #[test]
    fn the_matte_finds_colour_and_opacity() {
        // Opaque red, transparent, and grey 200 at 50 % (100 over black, 100 + 127 over white).
        let black = [255, 0, 0, 0, 0, 0, 100, 100, 100];
        let white = [255, 0, 0, 255, 255, 255, 227, 227, 227];
        let rgba = matte(&black, &white);
        assert_eq!(rgba.get(0..4), Some(&[255, 0, 0, 255][..]));
        assert_eq!(rgba.get(4..8), Some(&[0, 0, 0, 0][..]));
        assert_eq!(rgba.get(11), Some(&128));
        assert!(rgba.get(8).is_some_and(|grey| grey.abs_diff(199) <= 1));
        // Odd lengths do not panic.
        assert_eq!(matte(&[1, 2], &[3, 4]), Vec::<u8>::new());
    }
}
