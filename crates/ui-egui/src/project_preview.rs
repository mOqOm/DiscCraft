//! プロジェクトプレビュー（Encore の ファイル ▸ プレビュー）。ビルドせずに、プロジェクトのディスクを別の窓で
//! 試す: ナビゲーションはビルドが書くのと同じモデル（`disccraft_dvd::preview_model`）を VM で動かし、メニューは
//! プロジェクトから描いてハイライトを重ね、タイトルは素材から映像と音つきで再生する（[`crate::preview_playback`]）。
//!
//! 窓の下の操作バーは Encore と同じ並び: 拡大率、オーディオと字幕、今の項目（名前、ボタンかチャプター、
//! 静止画か再生の時刻）、タイトル / メニュー、十字キーと決定、前のチャプター・停止・一時停止・再生・次のチャプター・
//! チャプター番号、そして「ここで終了」（見ている項目を開いて終わる）と「終了して戻る」（開く前の選択に戻る）。
//!
//! M6.514: 見た目も Encore のプレビューに近づけた: 初めは 100 %（実寸で真ん中）、拡大率は画面の絵の下、今の項目は
//! アイコン付きの 3 行の明るめの箱、タイトル / メニューは絵のボタン、十字キーは丸い 1 つの部品。絵は Craft のアイコンと
//! DiscCraft の描いた形で、Encore の絵柄は写さない（AGENTS.md ルール 2）。

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
const WINDOW_SIZE: [f32; 2] = [1040.0, 680.0];

impl DiscCraftApp {
    /// ファイル ▸ プレビュー: ビルドせずに、プロジェクトのディスクをファーストプレイから（`start` があれば
    /// そのメニュー / タイトルから）別の窓でプレビューする。
    pub(crate) fn open_project_preview(&mut self, start: Value) {
        self.playback = None;
        self.preview_playback = None;
        self.preview_paused = false;
        self.preview_resume = None;
        self.preview_slides.clear();
        if !self.preview_player_accepts() {
            return;
        }
        let start = self.with_player_settings(start);
        let Some(state) = self.run_query("preview.open", start) else {
            return;
        };
        self.preview = parse_preview_state(&state);
        self.note_parental_stop();
        let return_to = self
            .project_preview
            .as_ref()
            .map_or(self.selection, |open| open.return_to);
        self.project_preview = Some(ProjectPreview {
            return_to,
            chapter_text: String::new(),
            // M6.514: at its own size, as Encore's preview starts (100 %), not stretched to the window.
            zoom: MonitorZoom::Percent(100),
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
        // (The bar under the picture scrolls sideways when the window is made narrower than its buttons.)
        let exit = ctx.show_viewport_immediate(
            ViewportId::from_hash_of("disccraft_project_preview"),
            builder,
            |ui, class| {
                // M9.54: the remote's keys (arrows, Enter) stay the preview's. (Shown inside the main window, the
                // main window's keys are its.)
                if class != egui::ViewportClass::EmbeddedWindow {
                    crate::framework::host_window(ui);
                    crate::framework::give_back_escape(ui.ctx());
                }
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
                self.preview_motion = None;
                ui.centered_and_justified(|ui| {
                    ui.colored_label(egui::Color32::GRAY, state.label.clone());
                });
                return;
            };
            let highlight = usize::from(menu.button.saturating_sub(1));
            // A rendered motion menu plays (M6.334), as in Encore's preview; otherwise the still picture.
            let moving = self.motion_menu_picture(ui.ctx(), menu.pgcn, &project_menu);
            ui.centered_and_justified(|ui| {
                let _ = self.menu_canvas_over(
                    ui,
                    &project_menu,
                    (Some(highlight), zoom),
                    moving.as_ref(),
                );
            });
            return;
        }
        self.preview_motion = None;
        if let Some(picture) = self.preview_picture() {
            let area = ui.max_rect();
            // The television of Preferences ▸ Preview: a 16:9 title on a 4:3 one is letterboxed or panned and
            // scanned.
            let (screen, frame) =
                crate::monitor::tv_screen(picture.wide, &self.preview_prefs, area, zoom);
            ui.painter().rect_filled(screen, 0.0, egui::Color32::BLACK);
            let clipped = ui.painter().with_clip_rect(screen.intersect(area));
            if picture.fitted {
                // A slide: on black, in its own shape, moved by its pan and zoom (as on the disc).
                crate::preview_playback::draw_slides(
                    &clipped,
                    frame,
                    (&picture.texture, picture.view),
                    picture.blend.as_ref(),
                );
            } else {
                clipped.image(
                    picture.texture.id(),
                    frame,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            if let Some(subtitle) = &picture.subtitle {
                let painter = ui.painter().clone();
                let _ = self.draw_shown_subtitle(ui.ctx(), &painter, (frame, area), subtitle);
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

    /// The picture of motion menu `menu` (on the preview's PGC `pgcn`) playing, when it has been rendered into the
    /// build's cache (File ▸ Render ▸ Motion Menu, or a build): Encore's preview plays a motion menu, its motion
    /// buttons too, once it is rendered. It loops from its loop point. `None` (the still picture) for a menu that is not
    /// moving or not rendered.
    fn motion_menu_picture(
        &mut self,
        ctx: &Context,
        pgcn: u16,
        menu: &disccraft_project::Menu,
    ) -> Option<egui::TextureHandle> {
        if !menu.is_motion() {
            self.preview_motion = None;
            return None;
        }
        let fresh = self
            .preview_motion
            .as_ref()
            .is_none_or(|play| play.pgcn != pgcn || play.menu != menu.id);
        if fresh {
            let path = self.cache_options().and_then(|options| {
                disccraft_build::rendered_motion_menu(&self.session.project, &options, menu.id)
            });
            let frame = disccraft_project::frame_size(&self.session.project.settings);
            let height = 480_u32;
            // Square pixels in the menu's shape, as the still is shown.
            let width = (f64::from(height)
                * f64::from(crate::menu_editor::menu_aspect(menu, frame)))
            .round() as u32;
            let fps = self.preview_fps();
            let player = path.clone().map(|path| {
                disccraft_engine::Player::start_sized(path, None, 0.0, fps, Some((width, height)))
            });
            self.preview_motion = Some(MotionPlay {
                pgcn,
                menu: menu.id,
                path,
                player,
                texture: None,
                loop_from: menu
                    .effective_loop_point()
                    .map_or(0.0, |point| point.seconds()),
                fps,
                size: (width, height),
            });
        }
        let play = self.preview_motion.as_mut()?;
        let player = play.player.as_ref()?;
        if let Some(frame) = player.take_frame() {
            let (width, height) = (frame.width as usize, frame.height as usize);
            if frame.rgb.len() == width * height * 3 {
                let image = egui::ColorImage::from_rgb([width, height], &frame.rgb);
                match play.texture.as_mut() {
                    Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                    None => {
                        play.texture = Some(ctx.load_texture(
                            "preview_motion_menu",
                            image,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                }
            }
        }
        match player.status() {
            disccraft_engine::PlayerStatus::Playing => {}
            // One play done: again from the loop point.
            disccraft_engine::PlayerStatus::Ended => {
                play.player = play.path.clone().map(|path| {
                    disccraft_engine::Player::start_sized(
                        path,
                        None,
                        play.loop_from,
                        play.fps,
                        Some(play.size),
                    )
                });
            }
            disccraft_engine::PlayerStatus::Failed(error) => {
                self.status = error;
                play.player = None;
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(15));
        play.texture.clone()
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
        if self.menu_texture_due(hash) {
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
        // A Blu-ray's bar has one button more (Pop-up Menu) and was wider than the window: its last buttons
        // (Exit Here, Exit and Return) were cut off. The window is wider now, and a narrower one scrolls the bar.
        let scroll = egui::ScrollArea::horizontal()
            .id_salt("project_preview_bar")
            .auto_shrink([false, true]);
        let _ = scroll.show(ui, |ui| {
            exit = self.preview_bar_buttons(ui);
        });
        exit
    }

    /// The buttons of [`Self::preview_bar`], left to right.
    fn preview_bar_buttons(&mut self, ui: &mut Ui) -> Option<Exit> {
        let mut exit = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            // 拡大率（M6.514: Encore と同じく、画面の絵の下に拡大率）。
            if let Some(open) = self.project_preview.as_mut() {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(80.0, 18.0), egui::Sense::hover());
                    icons::paint(
                        ui.painter(),
                        egui::Rect::from_center_size(rect.center(), egui::vec2(18.0, 18.0)),
                        Icon::Monitor,
                        ui.visuals().text_color(),
                    );
                    crate::monitor::zoom_combo(ui, "project_preview_zoom", &mut open.zoom);
                });
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
        let timeline = match crate::preview_playback::title_kind(project, title) {
            Some(crate::preview_playback::TitleKind::Timeline(id)) => project.timeline(id),
            // A chapter playlist plays its master timeline's streams.
            Some(crate::preview_playback::TitleKind::ChapterPlaylist(id)) => project
                .chapter_playlists
                .get(&id)
                .and_then(|list| project.timeline(list.timeline)),
            // A slideshow: its sound (silence when it has none), and one subtitle stream when a slide makes a
            // subtitle (Create Subtitle), as the build gives it.
            Some(crate::preview_playback::TitleKind::Slideshow(id)) => {
                let subtitles = project
                    .slideshow(id)
                    .is_some_and(|show| show.slides.iter().any(|slide| slide.subtitle));
                return (1, u8::from(subtitles));
            }
            None => None,
        };
        let Some(timeline) = timeline else {
            return (0, 0);
        };
        // トラックがなければ、映像の素材の音の 1 本。
        let audio = u8::try_from(timeline.audio.len().max(1))
            .unwrap_or(8)
            .min(8);
        let subtitles = u8::try_from(timeline.subtitles.len()).unwrap_or(32).min(32);
        (audio, subtitles)
    }

    /// 今の項目: 名前、ボタン（メニュー）かチャプター（タイトル）、静止画 / モーション か再生の時刻。M6.514: Encore と
    /// 同じく 3 行それぞれに小さなアイコン（Craft のアイコン）を付け、角の丸い明るめの箱に。
    fn preview_info(&mut self, ui: &mut Ui) {
        let rows = self.preview_info_lines();
        egui::Frame::new()
            .fill(INFO_BOX.get())
            .stroke(egui::Stroke::new(1.0, INFO_EDGE.get()))
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.set_width(190.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    let color = crate::theme::TEXT_STRONG.get();
                    for (icon, text) in rows {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            let (rect, _) = ui
                                .allocate_exact_size(egui::vec2(13.0, 13.0), egui::Sense::hover());
                            if let Some(icon) = icon {
                                icons::paint(ui.painter(), rect, icon, color);
                            }
                            ui.add(
                                egui::Label::new(egui::RichText::new(text).color(color)).truncate(),
                            );
                        });
                    }
                });
            });
    }

    /// The item box's three rows: the menu or title (its icon and name), the button or chapter, and still /
    /// motion or the time played.
    fn preview_info_lines(&self) -> [(Option<Icon>, String); 3] {
        let state = &self.preview;
        if self.preview_stopped() {
            return [
                (Some(Icon::Square), tl!("Stopped").to_owned()),
                (None, String::new()),
                (None, String::new()),
            ];
        }
        let project = &self.session.project;
        if let Some(menu) = &state.menu {
            let found = self
                .menu_for_pgcn(menu.pgcn)
                .and_then(|id| project.menu(id));
            let name = found.map_or_else(|| state.label.clone(), |menu| menu.name.clone());
            let motion = found.is_some_and(disccraft_project::Menu::is_motion);
            let kind = if motion { tl!("Motion") } else { tl!("Still") };
            return [
                (Some(Icon::ListView), name),
                (
                    Some(Icon::Selection),
                    tlf!("Button {n}", n = format!("{:02}", menu.button)),
                ),
                (
                    Some(if motion { Icon::Film } else { Icon::Image }),
                    kind.to_owned(),
                ),
            ];
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
            return [
                (Some(Icon::Film), name),
                (
                    Some(Icon::Marker),
                    tlf!("Chapter {n}", n = format!("{chapter:02}")),
                ),
                (Some(Icon::Clock), time),
            ];
        }
        [
            (None, state.label.clone()),
            (None, String::new()),
            (None, String::new()),
        ]
    }

    /// タイトル / メニューのボタンと、十字キーと決定。M6.514: Encore と同じく、タイトルとメニューは絵のボタン
    /// （Craft のアイコン）を縦に並べ、十字キーは丸い 1 つの部品に。
    fn preview_remote(&mut self, ui: &mut Ui) {
        let open = !self.preview_stopped();
        let playing = open && self.preview.menu.is_none();
        let mut key = None;
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if transport_button(ui, Icon::Type, open, tl!("Title")) {
                key = Some(RemoteKey::Title);
            }
            if transport_button(ui, Icon::ListView, playing, tl!("Menu")) {
                key = Some(RemoteKey::Menu);
            }
            // Blu-ray のリモコンのポップアップメニューのボタン（タイトルにポップアップがあるときだけ）。
            if self.preview.bluray
                && transport_button(
                    ui,
                    Icon::Grid,
                    playing && (self.preview.popup_available || self.preview.popup.is_some()),
                    tl!("Pop-up Menu"),
                )
            {
                key = Some(RemoteKey::Popup);
            }
        });
        if let Some(pressed) = arrow_pad(ui, open) {
            key = Some(pressed);
        }
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

    /// Closes the Project Preview as its window's close box does (the manual's screenshot script, M15.1).
    pub(crate) fn close_project_preview_window(&mut self, ctx: &Context) {
        self.exit_project_preview(ctx, Exit::Return);
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

/// The item box's ground and edge (M6.514).
const INFO_BOX: crate::theme::Shade = crate::theme::Shade::gray(108, 206);
const INFO_EDGE: crate::theme::Shade = crate::theme::Shade::gray(136, 172);
/// The arrow pad: its ring, its edge, the Enter button in the middle (and under the pointer).
const PAD_RING: crate::theme::Shade = crate::theme::Shade::gray(82, 214);
const PAD_EDGE: crate::theme::Shade = crate::theme::Shade::gray(34, 150);
const PAD_ENTER: crate::theme::Shade = crate::theme::Shade::gray(112, 236);
const PAD_LIT: crate::theme::Shade = crate::theme::Shade::gray(140, 250);

/// Which part of the arrow pad centred on `center` (outer radius `outer`, Enter's radius `inner`) is at `offset`
/// from its centre: Enter in the middle, else the arrow of the quarter it is in; `None` outside the pad.
fn pad_part(offset: egui::Vec2, outer: f32, inner: f32) -> Option<RemoteKey> {
    let distance = offset.length();
    if distance > outer {
        return None;
    }
    if distance <= inner {
        return Some(RemoteKey::Enter);
    }
    Some(if offset.x.abs() > offset.y.abs() {
        if offset.x > 0.0 {
            RemoteKey::Right
        } else {
            RemoteKey::Left
        }
    } else if offset.y > 0.0 {
        RemoteKey::Down
    } else {
        RemoteKey::Up
    })
}

/// The remote's arrows and Enter as one round pad (M6.514, as the remote in Encore's preview; DiscCraft's own
/// drawing): a ring with an arrow at each quarter and a round Enter button in the middle. The key pressed.
fn arrow_pad(ui: &mut Ui, enabled: bool) -> Option<RemoteKey> {
    const SIZE: f32 = 62.0;
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(SIZE, SIZE), sense);
    let center = rect.center();
    let outer = SIZE / 2.0 - 1.0;
    let inner = outer * 0.4;
    let under = response
        .hover_pos()
        .filter(|_| enabled)
        .and_then(|pos| pad_part(pos - center, outer, inner));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let faded = |color: egui::Color32| {
            if enabled {
                color
            } else {
                color.gamma_multiply(0.45)
            }
        };
        painter.circle(
            center,
            outer,
            faded(PAD_RING.get()),
            egui::Stroke::new(1.0, PAD_EDGE.get()),
        );
        let arrow = faded(crate::theme::TEXT.get());
        let lit = crate::theme::TEXT_STRONG.get();
        for (key, direction) in [
            (RemoteKey::Up, egui::vec2(0.0, -1.0)),
            (RemoteKey::Right, egui::vec2(1.0, 0.0)),
            (RemoteKey::Down, egui::vec2(0.0, 1.0)),
            (RemoteKey::Left, egui::vec2(-1.0, 0.0)),
        ] {
            let at = center + direction * ((inner + outer) / 2.0 + 1.0);
            let across = egui::vec2(-direction.y, direction.x);
            let points = vec![
                at + direction * 4.0,
                at - direction * 3.0 + across * 4.5,
                at - direction * 3.0 - across * 4.5,
            ];
            let color = if under == Some(key) { lit } else { arrow };
            painter.add(egui::Shape::convex_polygon(
                points,
                color,
                egui::Stroke::NONE,
            ));
        }
        let enter = if under == Some(RemoteKey::Enter) {
            PAD_LIT.get()
        } else {
            faded(PAD_ENTER.get())
        };
        painter.circle(
            center,
            inner - 2.0,
            enter,
            egui::Stroke::new(1.0, PAD_EDGE.get()),
        );
    }
    let response = response.on_hover_text(match under {
        Some(RemoteKey::Enter) => tl!("Enter"),
        Some(RemoteKey::Up) => tl!("Up"),
        Some(RemoteKey::Down) => tl!("Down"),
        Some(RemoteKey::Left) => tl!("Left"),
        Some(RemoteKey::Right) => tl!("Right"),
        _ => tl!("Arrows and Enter of the remote"),
    });
    if !response.clicked() {
        return None;
    }
    response
        .interact_pointer_pos()
        .and_then(|pos| pad_part(pos - center, outer, inner))
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

/// A rendered motion menu playing in the project preview (M6.334).
pub(crate) struct MotionPlay {
    /// The preview's PGC it plays on, and its menu.
    pgcn: u16,
    menu: disccraft_project::MenuId,
    /// The rendered video (`None`: not rendered, the still is shown).
    path: Option<std::path::PathBuf>,
    player: Option<disccraft_engine::Player>,
    texture: Option<egui::TextureHandle>,
    /// Where each play after the first starts (the menu's loop point), seconds.
    loop_from: f64,
    fps: f64,
    size: (u32, u32),
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

#[cfg(test)]
mod pad_tests {
    use super::pad_part;
    use crate::monitor::RemoteKey;
    use egui::vec2;

    /// M6.514: the round pad's middle is Enter, each quarter its arrow, and outside it nothing.
    #[test]
    fn the_pad_is_split_into_enter_and_four_arrows() {
        let (outer, inner) = (30.0, 12.0);
        assert_eq!(
            pad_part(vec2(0.0, 0.0), outer, inner),
            Some(RemoteKey::Enter)
        );
        assert_eq!(
            pad_part(vec2(5.0, -6.0), outer, inner),
            Some(RemoteKey::Enter)
        );
        assert_eq!(
            pad_part(vec2(2.0, -20.0), outer, inner),
            Some(RemoteKey::Up)
        );
        assert_eq!(
            pad_part(vec2(-3.0, 22.0), outer, inner),
            Some(RemoteKey::Down)
        );
        assert_eq!(
            pad_part(vec2(-25.0, 4.0), outer, inner),
            Some(RemoteKey::Left)
        );
        assert_eq!(
            pad_part(vec2(18.0, 9.0), outer, inner),
            Some(RemoteKey::Right)
        );
        assert_eq!(pad_part(vec2(25.0, 25.0), outer, inner), None);
    }
}
