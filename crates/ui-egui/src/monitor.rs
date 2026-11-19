//! モニターパネル（[docs/ui-parity.md](../../../docs/ui-parity.md) §3、M6.8）。
//!
//! ビルド済みの `VIDEO_TS` を `preview.*` コマンドで動かし、メニュー画面を
//! `disccraft-menu` の描画で映しながらリモコン操作を送る。映像そのものは
//! デコードせず、メニューの静止画と現在位置の表示で状態を示す。

use crate::app::{DiscCraftApp, Selection};
use crate::i18n::{tl, tlf};

use egui::{self, Ui};
use serde_json::{Value, json};

/// リモコンのキー（`preview.key` の `key` に合わせる）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RemoteKey {
    Up,
    Down,
    Left,
    Right,
    Enter,
    /// タイトル再生を終了させる（家庭用プレーヤーには無い、プレビュー専用の操作）。
    Finish,
    /// メニューボタン（再生中はタイムラインのメニューリモートへ）。
    Menu,
    /// タイトルボタン（タイトルメニューへ）。
    Title,
    /// Blu-ray のポップアップメニューのボタン（再生中のタイトルの上にポップアップを出す / 閉じる）。
    Popup,
}

/// モニターの拡大率（Encore のプレビューと同じ選択肢）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum MonitorZoom {
    /// パネルに合わせる。
    #[default]
    Fit,
    /// 4:3 の画面を 640 x 480 として、その何倍で映すか。
    Percent(u16),
}

impl MonitorZoom {
    /// 選択肢（Encore の並び）。
    pub(crate) const ALL: [MonitorZoom; 6] = [
        MonitorZoom::Percent(200),
        MonitorZoom::Percent(100),
        MonitorZoom::Percent(75),
        MonitorZoom::Percent(50),
        MonitorZoom::Percent(25),
        MonitorZoom::Fit,
    ];

    pub(crate) fn label(self) -> String {
        match self {
            MonitorZoom::Fit => tl!("Fit").to_owned(),
            MonitorZoom::Percent(percent) => format!("{percent}%"),
        }
    }

    /// ズームツールの 1 段（大きく / 小さく）。全体表示からは 100% / 50% へ。
    pub(crate) fn step(self, zoom_in: bool) -> MonitorZoom {
        const STEPS: [u16; 5] = [25, 50, 75, 100, 200];
        let next = match self {
            MonitorZoom::Fit if zoom_in => 100,
            MonitorZoom::Fit => 50,
            MonitorZoom::Percent(percent) if zoom_in => STEPS
                .iter()
                .copied()
                .find(|step| *step > percent)
                .unwrap_or(200),
            MonitorZoom::Percent(percent) => STEPS
                .iter()
                .rev()
                .copied()
                .find(|step| *step < percent)
                .unwrap_or(25),
        };
        MonitorZoom::Percent(next)
    }

    /// 映す大きさ。`Fit` は `area` に収まるいちばん大きな `aspect` の矩形。
    pub(crate) fn size(self, area: egui::Vec2, aspect: f32) -> egui::Vec2 {
        match self {
            MonitorZoom::Fit => {
                let width = area.x.min(area.y * aspect).max(1.0);
                egui::vec2(width, width / aspect)
            }
            MonitorZoom::Percent(percent) => {
                let scale = f32::from(percent) / 100.0;
                egui::vec2(480.0 * aspect * scale, 480.0 * scale)
            }
        }
    }
}

impl RemoteKey {
    pub(crate) fn param(self) -> &'static str {
        match self {
            RemoteKey::Up => "up",
            RemoteKey::Down => "down",
            RemoteKey::Left => "left",
            RemoteKey::Right => "right",
            RemoteKey::Enter => "enter",
            RemoteKey::Finish => "finish",
            RemoteKey::Menu => "menu",
            RemoteKey::Title => "title",
            RemoteKey::Popup => "popup",
        }
    }
}

/// プレビューの状態表示。
#[derive(Clone, Debug, Default)]
pub(crate) struct PreviewState {
    pub(crate) open: bool,
    pub(crate) label: String,
    pub(crate) menu: Option<PreviewMenu>,
    /// 再生中のタイトルとチャプター（1 から）。
    pub(crate) title: Option<(u8, u16)>,
    /// VM が選んでいるオーディオストリーム（1 から）。
    pub(crate) audio: Option<u8>,
    /// VM が表示にしている字幕ストリーム（1 から。消しているときは `None`）。
    pub(crate) subtitle: Option<u8>,
    /// Blu-ray のプロジェクトのプレビューか。
    pub(crate) bluray: bool,
    /// 再生中のタイトルの上に出ている Blu-ray のポップアップメニュー（PGCN とハイライトしているボタン）。
    pub(crate) popup: Option<PreviewMenu>,
    /// 再生中のタイトルにポップアップメニューがあるか。
    pub(crate) popup_available: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct PreviewMenu {
    /// VMGM の PGCN（1 始まり）。
    pub(crate) pgcn: u16,
    /// ハイライトされているボタン（1 始まり。0 は未選択）。
    pub(crate) button: u8,
}

/// Encore's monitor button セーフエリアの表示 / 非表示 in `rect`: flips `shown` when it is pressed and looks pressed
/// while the safe areas are shown.
pub(crate) fn safe_area_button(ui: &mut Ui, rect: egui::Rect, shown: &mut bool) {
    let response = ui
        .interact(
            rect,
            ui.scope_id().with("monitor_safe_areas"),
            egui::Sense::click(),
        )
        .on_hover_text(tl!("Show / Hide Safe Areas"));
    if response.clicked() {
        *shown = !*shown;
    }
    let visuals = ui.style().interact_selectable(&response, *shown);
    if *shown || response.hovered() {
        ui.painter().rect(
            rect,
            2.0,
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
    }
    crate::tool_icons::paint(
        ui.painter(),
        egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0)),
        crate::tool_icons::Glyph::SafeArea,
        visuals.fg_stroke.color,
    );
}
impl DiscCraftApp {
    /// モニターパネル。Encore のプレビューと同じく、上に映像（メニュー）、下に操作バーを置く。
    pub(crate) fn monitor_view(&mut self, ui: &mut Ui) {
        // 状態は開いたときとキー操作のときにだけ更新する（毎フレームのコマンド実行は
        // ステータス欄と dirty フラグを汚すので避ける）。
        // プロジェクトプレビューの窓が開いている間は、プレビューはその窓に出す（モニターはタイムライン）。
        let state = if self.project_preview.is_some() {
            PreviewState::default()
        } else {
            self.preview.clone()
        };
        // プレビューしていないときは、Encore と同じくタイムラインの再生ヘッドの位置の映像を映す。
        let timeline_frame = if state.open {
            None
        } else {
            self.timeline_frame(ui.ctx())
        };
        let timecode = timeline_frame
            .as_ref()
            .map(|(_, timecode)| timecode.clone());
        // Encore と同じく、モニターに今の字幕を重ねる（1 本目の字幕トラック）。
        let subtitle = match (state.open, self.viewer_timeline()) {
            (false, Some(id)) => {
                let seconds = crate::timeline_view::playhead_seconds(ui.ctx(), id);
                self.monitor_subtitle(id, seconds)
            }
            _ => None,
        };

        // プレビューでタイトルを再生しているときは、その映像（と VM が出している字幕）。
        let preview_picture = if state.open && state.menu.is_none() {
            self.preview_picture()
        } else {
            None
        };

        // タイムラインを映しているときは FilmCraft のプログラムモニターと同じ 3 段の操作部、プレビュー中は
        // リモコンの操作バー。
        let timeline = match (state.open, self.viewer_timeline()) {
            (false, Some(id)) => Some(id),
            _ => None,
        };
        self.drop_stale_subtitle_edit(timeline);
        egui::Panel::bottom("monitor_controls")
            .resizable(false)
            .show(ui, |ui| match timeline {
                Some(id) => self.timeline_controls(ui, id),
                None => self.monitor_bar(ui, &state, timecode.as_deref()),
            });

        // FilmCraft と同じく、映像のまわりはパネルの地、映像は黒い画面の中に。
        let surround = if timeline.is_some() {
            crate::theme::PANEL
        } else {
            egui::Color32::BLACK
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(surround))
            .show(ui, |ui| {
                if let Some((texture, _)) = &timeline_frame {
                    // タイムラインの画面（4:3 か 16:9。拡大率に合わせ、はみ出たところは切れる）に、ディスクと同じく
                    // 絵をレターボックス / ピラーボックスで収める（FilmCraft のモニターと同じく、まわりはパネルの地）。
                    // The screen: the frame's own aspect (the timeline's 16:9 or 4:3, or the HD frame).
                    let [frame_width, frame_height] = texture.size();
                    let screen_aspect = if frame_width == 0 || frame_height == 0 {
                        4.0 / 3.0
                    } else {
                        frame_width as f32 / frame_height as f32
                    };
                    let area = ui.max_rect().shrink(4.0);
                    let size = self.monitor_zoom.size(area.size(), screen_aspect);
                    let screen = egui::Rect::from_center_size(area.center(), size);
                    // The same picture goes to the video output (Preferences ▸ Audio/Video Output).
                    self.video_out_frame = Some((texture.clone(), screen_aspect, false));
                    ui.painter().rect_filled(screen, 0.0, egui::Color32::BLACK);
                    let picture = fit_picture(screen, screen_aspect);
                    ui.painter().image(
                        texture.id(),
                        picture,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    let shown = screen.intersect(area);
                    if let Some(note) = &self.monitor_note {
                        // Whether the disc takes the video as it is (H.264 a Blu-ray carries) or would have to
                        // convert it, which it cannot do without the picture.
                        let asset = self
                            .monitor_frame
                            .as_ref()
                            .map(|(asset, _, _)| disccraft_project::AssetId(*asset));
                        let fate = match asset.and_then(|asset| self.transcode_state(asset)) {
                            Some(crate::project_panel::TranscodeState::Unneeded) => {
                                tl!("This disc carries the video as it is, without converting it.")
                            }
                            Some(_) => {
                                tl!(
                                    "This disc would have to convert the video, which it cannot do."
                                )
                            }
                            None => "",
                        };
                        let text = format!(
                            "{}\n{}\n{fate}",
                            tlf!("The picture cannot be shown: {message}", message = note),
                            tl!("Play to hear the sound.")
                        );
                        let _ = ui.painter().with_clip_rect(shown).text(
                            shown.center(),
                            egui::Align2::CENTER_CENTER,
                            text,
                            egui::FontId::proportional(13.0),
                            egui::Color32::GRAY,
                        );
                    }
                    if self.subtitle_edit.is_some() {
                        // 字幕を追加したあと / ダブルクリックしたあとは、字幕の場所で入力する（Encore と同じ）。
                        self.subtitle_edit_box(ui, shown);
                    } else if let (Some(subtitle), Some(id)) = (&subtitle, timeline) {
                        let painter = ui.painter().clone();
                        let at =
                            self.draw_shown_subtitle(ui.ctx(), &painter, (shown, shown), subtitle);
                        // A picture subtitle has no text to change (as in Encore).
                        if subtitle.image.is_none() {
                            let response = ui
                                .interact(
                                    at,
                                    ui.scope_id().with("monitor_subtitle"),
                                    egui::Sense::click(),
                                )
                                .on_hover_text(tl!("Double-click to change the subtitle's text"));
                            if response.double_clicked() {
                                let seconds = crate::timeline_view::playhead_seconds(ui.ctx(), id);
                                self.edit_shown_subtitle(id, seconds);
                            }
                        }
                    }
                    if self.show_monitor_safe_areas {
                        crate::menu_editor::paint_safe_areas(ui.painter(), picture);
                    }
                    return;
                }
                if let Some(picture) = &preview_picture {
                    let area = ui.max_rect();
                    // On a 4:3 television (Preferences ▸ Preview) a 16:9 title is letterboxed: the screen is
                    // 4:3 and black, the picture as wide as it and nine sixteenths as high. Panned and scanned,
                    // the picture is as high as the screen and its middle fills it (the sides are cut).
                    let (screen_rect, frame) =
                        tv_screen(picture.wide, &self.preview_prefs, area, self.monitor_zoom);
                    if screen_rect != frame {
                        ui.painter()
                            .rect_filled(screen_rect, 0.0, egui::Color32::BLACK);
                    }
                    let clipped = ui.painter().with_clip_rect(screen_rect.intersect(area));
                    if picture.fitted {
                        // A slide: its own shape inside the black frame.
                        clipped.rect_filled(frame, 0.0, egui::Color32::BLACK);
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
                        let _ =
                            self.draw_shown_subtitle(ui.ctx(), &painter, (frame, area), subtitle);
                    }
                    if self.show_monitor_safe_areas {
                        crate::menu_editor::paint_safe_areas(ui.painter(), frame);
                    }
                    // Blu-ray のポップアップメニュー（ビルドした Blu-ray のプレビュー）。
                    if let Some(popup) = &state.popup {
                        self.draw_popup(ui, popup.pgcn, popup.button, frame);
                    }
                    return;
                }
                let Some(menu) = state.menu.as_ref().filter(|_| state.open) else {
                    ui.centered_and_justified(|ui| {
                        let text = if state.open {
                            state.label.clone()
                        } else {
                            tl!("Not previewing. Build first, then choose Open Build Output.")
                                .to_owned()
                        };
                        ui.colored_label(egui::Color32::GRAY, text);
                    });
                    return;
                };
                self.preview_menu_frame(ui, menu);
            });
    }

    /// 下の操作バー。Encore のモニターと同じく、左に拡大率、中央に移動ボタン（プレビュー中はリモコン）、
    /// 右端に今の時刻（オレンジ）。入りきらないときは横にスクロールする。
    fn monitor_bar(&mut self, ui: &mut Ui, state: &PreviewState, timecode: Option<&str>) {
        let total = ui.available_width();
        egui::ScrollArea::horizontal()
            .id_salt("monitor_bar_scroll")
            .show(ui, |ui| {
                ui.horizontal(|ui| self.monitor_bar_row(ui, state, timecode, total));
            });
    }

    fn monitor_bar_row(
        &mut self,
        ui: &mut Ui,
        state: &PreviewState,
        timecode: Option<&str>,
        total: f32,
    ) {
        // 中央と右の幅は前のフレームで測ったもの（変わったら描き直す）。
        let key = egui::Id::unique("monitor_bar_widths");
        let (center_width, right_width) = ui
            .data(|data| data.get_temp::<(f32, f32)>(key))
            .unwrap_or((0.0, 0.0));
        let start = ui.cursor().min.x;
        let gap = ui.spacing().item_spacing.x;

        // 左: 拡大率と、Encore と同じセーフエリアの表示 / 非表示。
        let mut zoom = self.monitor_zoom;
        zoom_combo(ui, "monitor_zoom", &mut zoom);
        self.monitor_zoom = zoom;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 22.0), egui::Sense::hover());
        safe_area_button(ui, rect, &mut self.show_monitor_safe_areas);

        // 中央: プレビュー中はリモコン、それ以外はタイムラインの移動ボタン。
        let center_at = start + (total - center_width) / 2.0;
        ui.add_space((center_at - ui.cursor().min.x).max(gap));
        let center_start = ui.cursor().min.x;
        // （タイムラインを映しているときは `timeline_controls` が描く。）
        ui.add_enabled_ui(state.open, |ui| self.remote_controls(ui));
        let measured_center = ui.cursor().min.x - center_start;

        // 右: ビルド結果を開く / プレビューを終了と、今の場所（プレビュー中）か時刻（オレンジ）。
        let right_at = start + total - right_width;
        ui.add_space((right_at - ui.cursor().min.x).max(gap));
        let right_start = ui.cursor().min.x;
        // ボタンの文字は折り返さない。
        let open =
            egui::Button::new(tl!("Open Build Output")).wrap_mode(egui::TextWrapMode::Extend);
        if ui.add(open).clicked() {
            self.open_preview();
        }
        let exit = egui::Button::new(tl!("Exit Preview")).wrap_mode(egui::TextWrapMode::Extend);
        if ui.add_enabled(state.open, exit).clicked() {
            self.close_preview();
        }
        ui.separator();
        match (state.open, timecode) {
            (true, _) => {
                let _ = ui.strong(state.label.clone()).on_hover_text(tl!("Now"));
            }
            // プレビューしていないときは、映しているタイムラインの再生ヘッドの時刻。
            (false, Some(timecode)) => {
                let _ = ui
                    .label(
                        egui::RichText::new(timecode)
                            .monospace()
                            .strong()
                            .color(ui.visuals().warn_fg_color),
                    )
                    .on_hover_text(tl!("Now"));
            }
            (false, None) => {
                ui.strong(tl!("Stopped"));
            }
        }
        let measured_right = ui.cursor().min.x - right_start;

        let measured = (measured_center, measured_right);
        if (measured.0 - center_width).abs() > 0.5 || (measured.1 - right_width).abs() > 0.5 {
            ui.data_mut(|data| data.insert_temp(key, measured));
            ui.ctx().request_repaint();
        }
    }

    /// タイムラインビューアのタイムラインの、再生ヘッドの位置のフレームと時刻。フレームが変わったときだけ
    /// `asset.frame` で取り直す（マウスのボタンを押している間は取り直さない）。
    fn timeline_frame(&mut self, ctx: &egui::Context) -> Option<(egui::TextureHandle, String)> {
        let id = self.viewer_timeline()?;
        let (num, den) = crate::timeline_view::frame_rate(self.session.project.settings.standard);
        let fps = f64::from(num) / f64::from(den.max(1));
        let seconds = crate::timeline_view::playhead_seconds(ctx, id);
        // `as` saturates; the frame number is only a cache key.
        let frame = (seconds * fps).round() as i64;
        let timecode = crate::timeline_view::timecode(seconds, num, den);
        // The clip under the playhead and the place in its file (Encore: several clips on the video track).
        let clips = self.timeline_clips(id);
        // A gap between clips shows black, as it plays on the disc.
        if crate::playback::in_gap(&clips, seconds) && !self.is_playing() {
            let black = ctx.load_texture(
                "monitor_gap_black",
                egui::ColorImage::filled([4, 3], egui::Color32::BLACK),
                egui::TextureOptions::NEAREST,
            );
            self.monitor_note = None;
            return Some((black, timecode));
        }
        let (clip, local) = crate::playback::clip_at(&clips, seconds)?;
        let video = clip.asset;

        let cached = self
            .monitor_frame
            .as_ref()
            .filter(|(asset, cached_frame, _)| *asset == video.0 && *cached_frame == frame)
            .map(|(_, _, texture)| texture.clone());
        if let Some(texture) = cached {
            return Some((texture, timecode));
        }

        // While it plays, the monitor shows the player's frames.
        if self.is_playing()
            && let Some((_, _, texture)) = &self.monitor_frame
        {
            return Some((texture.clone(), timecode));
        }

        // The frame as the disc shows it, at the monitor's screen size (`monitor_frame_size`): the same pictures
        // as playback, so stopping and playing show the same. It is decoded on the loader's thread (a long file
        // takes seconds to open, an HD frame a while to decode): the window is never held up, and scrubbing
        // shows the newest frame that is ready.
        let path = std::path::PathBuf::from(&self.session.project.assets.get(&video)?.path);
        let size = self.monitor_frame_size(id);
        // The frame of the file: the position in the clip, on the frame grid.
        let at = (local * fps).round().max(0.0) / fps;
        let key = (video.0, frame);
        if let Some((asked, picture)) = self.monitor_loader.take() {
            let texture = match picture.and_then(|picture| {
                rgb_texture(ctx, "monitor_frame", &picture).ok_or_else(String::new)
            }) {
                Ok(texture) => {
                    self.monitor_note = None;
                    texture
                }
                // A picture that cannot be decoded (a codec the decoder does not read): a black screen in the
                // monitor's shape with the reason on it. The time is shown and the sound still plays.
                Err(message) => {
                    self.monitor_note = Some(message);
                    let (width, height) = asked.size;
                    let rows = (48 * u64::from(height) / u64::from(width.max(1))).clamp(1, 192);
                    ctx.load_texture(
                        "monitor_no_picture",
                        egui::ColorImage::filled(
                            [48, usize::try_from(rows).unwrap_or(27)],
                            egui::Color32::BLACK,
                        ),
                        egui::TextureOptions::NEAREST,
                    )
                }
            };
            self.monitor_frame = Some((asked.key.0, asked.key.1, texture.clone()));
            if asked.key == key && asked.size == size {
                self.monitor_wanted = None;
                return Some((texture, timecode));
            }
        }
        if self.monitor_wanted != Some((key, size)) {
            self.monitor_loader.request(disccraft_engine::FrameRequest {
                path,
                seconds: at,
                size,
                key,
            });
            self.monitor_wanted = Some((key, size));
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
        // Until it comes, the monitor keeps what it shows.
        self.monitor_frame
            .as_ref()
            .map(|(_, _, texture)| (texture.clone(), timecode))
    }

    /// The size the monitor shows timeline `id` at, in square pixels: a Blu-ray HD disc's HD frame, else the SD
    /// frame at the timeline's aspect (16:9 or 4:3).
    pub(crate) fn monitor_frame_size(&self, id: disccraft_project::TimelineId) -> (u32, u32) {
        let settings = &self.session.project.settings;
        if let Some((width, height, _, _)) = crate::preview_playback::disc_hd(settings) {
            return (width, height);
        }
        let wide = self
            .session
            .project
            .timeline(id)
            .is_some_and(|timeline| timeline.wide(settings));
        crate::preview_playback::sd_size(wide, settings.standard)
    }

    /// プレビュー中のメニューを、プロジェクトの描画で映す。
    fn preview_menu_frame(&mut self, ui: &mut Ui, menu: &PreviewMenu) {
        let Some(menu_id) = self.menu_for_pgcn(menu.pgcn) else {
            ui.label(tl!("This PGC is not a menu (hidden PGC)."));
            return;
        };

        let Some(project_menu) = self.session.project.menu(menu_id).cloned() else {
            return;
        };

        let highlight = usize::from(menu.button.saturating_sub(1));

        let zoom = self.monitor_zoom;
        ui.centered_and_justified(|ui| {
            if let Some(canvas) = self.menu_canvas(ui, &project_menu, Some(highlight), zoom)
                && self.show_monitor_safe_areas
            {
                crate::menu_editor::paint_safe_areas(ui.painter(), canvas);
            }
        });
    }

    /// VMGM の PGCN からプロジェクトのメニューを引く。
    ///
    /// `disccraft-nav` はメニューを `MenuId` 昇順に PGCN 1..=n へ割り当てるため、
    /// 隠し PGC を除けばこの対応で戻る。
    pub(crate) fn menu_for_pgcn(&self, pgcn: u16) -> Option<disccraft_project::MenuId> {
        let index = usize::from(pgcn).checked_sub(1)?;
        self.session.project.menus.keys().nth(index).copied()
    }

    fn remote_controls(&mut self, ui: &mut Ui) {
        // 1 段に並べる: タイトル / メニュー、十字キーと決定、終了。
        // メニューボタンは再生中だけ押せる。タイトルボタンはメニューでも再生中でも押せる。
        let playing = self.preview.menu.is_none() && self.preview.open;
        if ui.button(tl!("Title")).clicked() {
            self.send_remote(RemoteKey::Title);
        }
        if ui
            .add_enabled(playing, egui::Button::new(tl!("Menu")))
            .clicked()
        {
            self.send_remote(RemoteKey::Menu);
        }
        // Blu-ray のリモコンのポップアップメニューのボタン。
        if self.preview.bluray
            && ui
                .add_enabled(
                    playing && (self.preview.popup_available || self.preview.popup.is_some()),
                    egui::Button::new(tl!("Pop-up Menu")),
                )
                .clicked()
        {
            self.send_remote(RemoteKey::Popup);
        }
        ui.separator();
        for (text, key) in [
            ("←", RemoteKey::Left),
            ("↑", RemoteKey::Up),
            ("↓", RemoteKey::Down),
            ("→", RemoteKey::Right),
        ] {
            if ui.button(text).clicked() {
                self.send_remote(key);
            }
        }
        if ui.button(tl!("Enter")).clicked() {
            self.send_remote(RemoteKey::Enter);
        }
        ui.separator();
        if ui.button(tl!("Finish")).clicked() {
            self.send_remote(RemoteKey::Finish);
        }

        // キーボードでも操作できるようにする。ただしテキスト入力中にフォーカスが
        // あるときは矢印キーを奪わない。
        let typing = ui.ctx().memory(|m| m.focused().is_some());

        if !typing {
            for key in RemoteKey::ALL {
                let pressed = match key {
                    RemoteKey::Up => ui.input(|i| i.key_pressed(egui::Key::ArrowUp)),
                    RemoteKey::Down => ui.input(|i| i.key_pressed(egui::Key::ArrowDown)),
                    RemoteKey::Left => ui.input(|i| i.key_pressed(egui::Key::ArrowLeft)),
                    RemoteKey::Right => ui.input(|i| i.key_pressed(egui::Key::ArrowRight)),
                    RemoteKey::Enter => ui.input(|i| i.key_pressed(egui::Key::Enter)),
                    RemoteKey::Finish | RemoteKey::Menu | RemoteKey::Title | RemoteKey::Popup => {
                        false
                    }
                };

                if pressed {
                    self.send_remote(key);
                }
            }
        }
    }

    pub(crate) fn open_preview(&mut self) {
        let Some(path) = self.preview_folder() else {
            self.status = tl!("Build first").to_string();
            return;
        };

        let value = self.with_player_settings(json!({ "path": path }));
        self.playback = None;
        if !self.preview_player_accepts() {
            return;
        }

        if let Some(state) = self.run_query("preview.open", value) {
            self.status = tlf!("Preview opened: {path}", path = path);
            self.preview = parse_preview_state(&state);
            self.note_parental_stop();
        }
    }

    /// Whether the player of Preferences ▸ Preview plays this disc at all: a DVD limited to regions the player's
    /// is not one of does not play (the status bar says so).
    pub(crate) fn preview_player_accepts(&mut self) -> bool {
        let region = self.preview_prefs.region;
        let settings = &self.session.project.settings;
        let refused = region != 0
            && settings.kind == disccraft_project::DiscKind::Dvd
            && !settings.regions.is_empty()
            && !settings.regions.contains(&region);
        if refused {
            self.status = tlf!(
                "The disc does not play in region {n} (Preferences ▸ Preview sets the player's region)",
                n = region
            );
        }
        !refused
    }

    /// `open` (the parameters of `preview.open`) with the player's parental level, when it has one set.
    pub(crate) fn with_player_settings(&self, mut open: serde_json::Value) -> serde_json::Value {
        let level = self.preview_prefs.parental_level;
        if (1..=8).contains(&level)
            && let Some(fields) = open.as_object_mut()
        {
            let _ = fields.insert("parentalLevel".to_owned(), json!(level));
        }
        open
    }

    /// Says why nothing plays when the disc's parental level stopped the preview's player.
    pub(crate) fn note_parental_stop(&mut self) {
        let player = self.preview_prefs.parental_level;
        let disc = self.session.project.settings.parental_level;
        if (1..=8).contains(&player)
            && disc > player
            && self.preview.title.is_none()
            && self.preview.menu.is_none()
        {
            self.status = tlf!(
                "The disc's parental level ({disc}) is above the player's ({player}): it does not play",
                disc = disc,
                player = player
            );
        }
    }

    /// Encore の「ここからプレビューする」: ビルドせずに、プロジェクトプレビューの窓をそのメニュー / タイトルから開く（メニューは VMGM の PGCN、
    /// タイトルはタイムラインの順、そのあとにスライドショー。ビルドと同じ数え方）。
    pub(crate) fn preview_from(&mut self, selection: Selection) {
        let project = &self.session.project;
        let mut params = serde_json::Map::new();
        match selection {
            Selection::Menu(id) => {
                let Some(index) = project.menus.keys().position(|key| *key == id) else {
                    return;
                };
                let _ = params.insert("menu".to_owned(), Value::from(index.saturating_add(1)));
            }
            Selection::Timeline(id) => {
                let Some(index) = project.timelines.keys().position(|key| *key == id) else {
                    return;
                };
                let chapter = self
                    .chapter_focus
                    .filter(|(timeline, _)| *timeline == id)
                    .map_or(1, |(_, chapter)| chapter.saturating_add(1));
                let _ = params.insert("title".to_owned(), Value::from(index.saturating_add(1)));
                let _ = params.insert("chapter".to_owned(), Value::from(chapter));
            }
            Selection::Slideshow(id) => {
                let Some(index) = project.slideshows.keys().position(|key| *key == id) else {
                    return;
                };
                let title = project
                    .timelines
                    .len()
                    .saturating_add(index)
                    .saturating_add(1);
                let _ = params.insert("title".to_owned(), Value::from(title));
            }
            // チャプター再生リストはタイムラインとスライドショーの後ろの題。
            Selection::ChapterPlaylist(id) => {
                let Some(index) = project.chapter_playlists.keys().position(|key| *key == id)
                else {
                    return;
                };
                let title = project
                    .timelines
                    .len()
                    .saturating_add(project.slideshows.len())
                    .saturating_add(index)
                    .saturating_add(1);
                let _ = params.insert("title".to_owned(), Value::from(title));
            }
            _ => {}
        }
        self.open_project_preview(Value::Object(params));
    }

    /// Encore の「ここからプレビューする」をメニューのタブのボタンで（M6.338）: そのメニューを、そのボタンを選んだ
    /// 状態から。
    pub(crate) fn preview_from_button(&mut self, menu: disccraft_project::MenuId, index: usize) {
        let Some(pgcn) = self
            .session
            .project
            .menus
            .keys()
            .position(|key| *key == menu)
        else {
            return;
        };
        self.open_project_preview(json!({
            "menu": pgcn.saturating_add(1),
            "button": index.saturating_add(1),
        }));
    }

    pub(crate) fn close_preview(&mut self) {
        if let Some(state) = self.run_query("preview.close", json!({})) {
            self.preview = parse_preview_state(&state);
        }
    }

    pub(crate) fn send_remote(&mut self, key: RemoteKey) {
        let before = self.preview.label.clone();
        let Some(state) = self.run_query("preview.key", json!({ "key": key.param() })) else {
            return;
        };

        let popup_before = self.preview.popup.is_some();
        self.preview = parse_preview_state(&state);
        // Blu-ray のポップアップを出した / 閉じたのは、何も起きなかったのではない。
        let popup_changed = popup_before != self.preview.popup.is_some();
        if key == RemoteKey::Menu && self.preview.label == before && !popup_changed {
            // プレーヤーと同じく何も起きない。理由はステータスに出す。
            self.status = tl!("This timeline has no menu remote").to_string();
        }
    }

    /// プレビューするフォルダ。ビルド結果がフォルダのときはその `VIDEO_TS`（Blu-ray なら `BDMV` を入れた
    /// フォルダー）、ISO のときはその隣に書いた `<名前>.files` の中のもの。
    fn preview_folder(&self) -> Option<String> {
        let output = self
            .last_build
            .as_ref()
            .map(|report| report.output.clone())?;

        let folder = if output.is_dir() {
            output
        } else {
            let stem = output.file_stem()?.to_string_lossy().into_owned();
            output.with_file_name(format!("{stem}.files"))
        };
        // A Blu-ray: the folder that holds BDMV (the preview reads index.bdmv in it).
        if folder.join("BDMV").join("index.bdmv").is_file() {
            return Some(folder.to_string_lossy().into_owned());
        }
        let candidate = if folder.join("VIDEO_TS").is_dir() {
            folder.join("VIDEO_TS")
        } else {
            folder
        };

        if candidate.is_dir() {
            return Some(candidate.to_string_lossy().into_owned());
        }

        None
    }
}

impl RemoteKey {
    /// キーボードで操作できるキー。
    pub(crate) const ALL: [RemoteKey; 5] = [
        RemoteKey::Up,
        RemoteKey::Down,
        RemoteKey::Left,
        RemoteKey::Right,
        RemoteKey::Enter,
    ];
}

/// 拡大率の選択（モニターとメニュービューアのツールバーの左端）。
pub(crate) fn zoom_combo(ui: &mut Ui, id: &str, zoom: &mut MonitorZoom) {
    let _ = egui::ComboBox::from_id_salt(id)
        .width(80.0)
        .selected_text(zoom.label())
        .show_ui(ui, |ui| {
            for choice in MonitorZoom::ALL {
                if ui
                    .selectable_label(*zoom == choice, choice.label())
                    .clicked()
                {
                    *zoom = choice;
                }
            }
        })
        .response
        .on_hover_text(tl!("Magnification"));
}

/// The television the previews stand for (Preferences ▸ Preview): the screen in `area` at `zoom`, and where the
/// picture lies on it. A 16:9 title on a 4:3 television is letterboxed (as wide as the screen) or panned and
/// scanned (as high as the screen, its sides past it); otherwise the picture is the screen.
pub(crate) fn tv_screen(
    wide: bool,
    prefs: &crate::workspace::PreviewPrefs,
    area: egui::Rect,
    zoom: MonitorZoom,
) -> (egui::Rect, egui::Rect) {
    let on_4x3 = wide && prefs.tv_4x3;
    let screen = if on_4x3 || !wide {
        4.0 / 3.0
    } else {
        16.0 / 9.0
    };
    let screen_rect = egui::Rect::from_center_size(area.center(), zoom.size(area.size(), screen));
    if !on_4x3 {
        return (screen_rect, screen_rect);
    }
    let picture = if prefs.pan_scan {
        egui::vec2(screen_rect.height() * 16.0 / 9.0, screen_rect.height())
    } else {
        egui::vec2(screen_rect.width(), screen_rect.width() * 9.0 / 16.0)
    };
    (
        screen_rect,
        egui::Rect::from_center_size(screen_rect.center(), picture),
    )
}

/// A colour of a timeline colour set as an egui colour (its opacity as alpha).
pub(crate) fn subtitle_color(color: disccraft_project::SubtitleColor) -> egui::Color32 {
    let [r, g, b] = color.rgb;
    let alpha = (u16::from(color.opacity.min(100)) * 255 + 50) / 100;
    egui::Color32::from_rgba_unmultiplied(r, g, b, u8::try_from(alpha).unwrap_or(255))
}

/// 字幕を映像の下の方に、カラーグループの色（文字とストローク）で重ねる。
pub(crate) fn draw_subtitle(
    painter: &egui::Painter,
    picture: egui::Rect,
    text: &str,
    (colors, look): (disccraft_project::SubtitleGroup, disccraft_project::CueLook),
) -> egui::Rect {
    let size = (picture.height() / 18.0).clamp(10.0, 40.0);
    let font = egui::FontId::proportional(size);
    // The subtitle area leaves a margin at the top and the bottom; the place is a percentage of the way down.
    let margin = picture.height() * 0.06;
    let lines = text.lines().count().max(1) as f32;
    let room = (picture.height() - margin * 2.0 - size * 1.2 * lines).max(0.0);
    let bottom =
        picture.min.y + margin + size * 1.2 * lines + room * f32::from(look.vertical()) / 100.0;
    // Left and right stand on the title-safe edges (a tenth of the width in), as on the disc.
    let (x, side) = match look.justify.unwrap_or_default() {
        disccraft_project::SubtitleJustify::Left => (
            picture.min.x + picture.width() * 0.10,
            egui::Align2::LEFT_BOTTOM,
        ),
        disccraft_project::SubtitleJustify::Center => {
            (picture.center().x, egui::Align2::CENTER_BOTTOM)
        }
        disccraft_project::SubtitleJustify::Right => (
            picture.max.x - picture.width() * 0.10,
            egui::Align2::RIGHT_BOTTOM,
        ),
    };
    let anchor = egui::pos2(x, bottom);
    let edge = f32::from(look.outline()) * 0.75;
    let offsets: &[(f32, f32)] = if edge > 0.0 {
        &[(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)]
    } else {
        &[]
    };
    for (dx, dy) in offsets.iter().map(|(dx, dy)| (dx * edge, dy * edge)) {
        painter.text(
            anchor + egui::vec2(dx, dy),
            side,
            text,
            font.clone(),
            subtitle_color(colors.stroke),
        );
    }
    painter.text(anchor, side, text, font, subtitle_color(colors.fill))
}

/// `preview.*` の戻り値を画面表示用にまとめる。
pub(crate) fn parse_preview_state(value: &Value) -> PreviewState {
    let mut state = parse_preview_location(value);
    let small = |value: Option<&Value>| {
        value
            .and_then(Value::as_u64)
            .and_then(|n| u8::try_from(n).ok())
    };
    state.audio = small(value.get("audio"));
    state.bluray = value.get("bluray").and_then(Value::as_bool) == Some(true);
    state.popup_available = value.get("popupAvailable").and_then(Value::as_bool) == Some(true);
    state.popup = value.get("popup").and_then(|popup| {
        let pgcn = popup
            .get("pgcn")
            .and_then(Value::as_u64)
            .and_then(|n| u16::try_from(n).ok())?;
        let button = small(popup.get("button"))?;
        Some(PreviewMenu { pgcn, button })
    });
    let subtitle = value.get("subtitle");
    state.subtitle = subtitle
        .filter(|subtitle| subtitle.get("on").and_then(Value::as_bool) == Some(true))
        .and_then(|subtitle| small(subtitle.get("track")));
    state.title = value
        .get("location")
        .and_then(|location| location.get("title"))
        .and_then(|title| {
            let number = small(title.get("title"))?;
            let chapter = title
                .get("chapter")
                .and_then(Value::as_u64)
                .and_then(|n| u16::try_from(n).ok())?;
            Some((number, chapter))
        });
    state
}

fn parse_preview_location(value: &Value) -> PreviewState {
    let open = value.get("open").and_then(Value::as_bool).unwrap_or(false);

    if !open {
        return PreviewState::default();
    }

    let Some(location) = value.get("location") else {
        return PreviewState {
            open: true,
            label: tl!("Stop").to_string(),
            menu: None,
            ..PreviewState::default()
        };
    };

    if let Some(text) = location.as_str() {
        // The VM says "stopped" when nothing plays.
        let label = if text == "stopped" {
            tl!("Stopped").to_string()
        } else {
            text.to_string()
        };
        return PreviewState {
            open: true,
            label,
            menu: None,
            ..PreviewState::default()
        };
    }

    if let Some(menu) = location.get("menu") {
        let pgcn = menu.get("pgcn").and_then(Value::as_u64).unwrap_or(0) as u16;
        let button = menu.get("button").and_then(Value::as_u64).unwrap_or(0) as u8;

        return PreviewState {
            open: true,
            label: tlf!(
                "Menu {pgcn} (button {button})",
                pgcn = pgcn,
                button = button
            ),
            menu: Some(PreviewMenu { pgcn, button }),
            ..PreviewState::default()
        };
    }

    if let Some(title) = location.get("title") {
        let number = title.get("title").and_then(Value::as_u64).unwrap_or(0);
        let chapter = title.get("chapter").and_then(Value::as_u64).unwrap_or(0);

        return PreviewState {
            open: true,
            label: tlf!(
                "Title {title} chapter {chapter}",
                title = number,
                chapter = chapter
            ),
            menu: None,
            ..PreviewState::default()
        };
    }

    PreviewState {
        open: true,
        label: tl!("Unknown").to_string(),
        menu: None,
        ..PreviewState::default()
    }
}

/// The largest rect of `aspect` centred in `screen` (letterbox or pillarbox).
pub(crate) fn fit_picture(screen: egui::Rect, aspect: f32) -> egui::Rect {
    if !(aspect.is_finite() && aspect > 0.0) || screen.height() <= 0.0 {
        return screen;
    }
    let size = if screen.width() / screen.height() > aspect {
        egui::vec2(screen.height() * aspect, screen.height())
    } else {
        egui::vec2(screen.width(), screen.width() / aspect)
    };
    egui::Rect::from_center_size(screen.center(), size)
}

/// The number (from 1) of the chapter that contains `now`: how many chapter points are at or before it (at least 1).
pub(crate) fn chapter_number(points: &[f64], now: f64) -> usize {
    points
        .iter()
        .filter(|point| **point <= now + 1e-6)
        .count()
        .max(1)
}

/// `now` より前の最後のチャプターポイント（なければ 0）。Encore と同じく、チャプターの途中からはそのチャプターの頭へ、
/// 頭にいるとき（頭から `frame` 秒 = 1 フレーム以内）はひとつ前のチャプターの頭へ戻る。
pub(crate) fn prev_point(points: &[f64], now: f64, frame: f64) -> f64 {
    let slack = frame.max(0.0) + 0.001;
    points
        .iter()
        .copied()
        .filter(|point| *point < now - slack)
        .fold(0.0, f64::max)
}

/// `now` より後の最初のチャプターポイント。
pub(crate) fn next_point(points: &[f64], now: f64) -> Option<f64> {
    points
        .iter()
        .copied()
        .filter(|point| *point > now + 0.001)
        .reduce(f64::min)
}

/// A texture of a frame the engine made (RGB24, row by row); `None` when its size and pixels do not agree.
pub(crate) fn rgb_texture(
    ctx: &egui::Context,
    name: &str,
    frame: &disccraft_engine::PlayerFrame,
) -> Option<egui::TextureHandle> {
    let width = usize::try_from(frame.width).ok()?;
    let height = usize::try_from(frame.height).ok()?;
    if width.checked_mul(height)?.checked_mul(3)? != frame.rgb.len() {
        return None;
    }
    let image = egui::ColorImage::from_rgb([width, height], &frame.rgb);
    Some(ctx.load_texture(name, image, egui::TextureOptions::LINEAR))
}

#[cfg(test)]
mod tests {
    use super::{PreviewState, chapter_number, parse_preview_state};
    use serde_json::json;

    #[test]
    fn pictures_fit_the_screen_as_on_the_disc() {
        use super::fit_picture;
        // A 4:3 picture on a 16:9 screen is pillarboxed; a 16:9 one on a 4:3 screen letterboxed.
        let wide = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(160.0, 90.0));
        let pillar = fit_picture(wide, 4.0 / 3.0);
        assert!((pillar.width() - 120.0).abs() < 1e-3 && (pillar.height() - 90.0).abs() < 1e-3);
        let narrow = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(120.0, 90.0));
        let letter = fit_picture(narrow, 16.0 / 9.0);
        assert!((letter.width() - 120.0).abs() < 1e-3 && (letter.height() - 67.5).abs() < 1e-3);
    }

    #[test]
    fn chapter_number_counts_points_at_or_before_now() {
        let points = [0.0, 10.0, 20.0];
        assert_eq!(chapter_number(&points, 0.0), 1);
        assert_eq!(chapter_number(&points, 9.99), 1);
        assert_eq!(chapter_number(&points, 10.0), 2);
        assert_eq!(chapter_number(&points, 25.0), 3);
        assert_eq!(chapter_number(&[], 5.0), 1);
        assert_eq!(chapter_number(&points, f64::NAN), 1);
    }

    #[test]
    fn chapter_navigation_steps_between_points() {
        let points = [0.0, 60.0, 120.0];
        let frame = 1.0 / 30.0;
        assert!((super::prev_point(&points, 90.0, frame) - 60.0).abs() < 1e-9);
        assert!((super::prev_point(&points, 60.0, frame) - 0.0).abs() < 1e-9);
        assert!((super::prev_point(&points, 0.0, frame) - 0.0).abs() < 1e-9);
        // One frame after a chapter's start still counts as its start (measured in Encore: from 10;00, with a
        // chapter at 9;29, the previous chapter is the one before); later it goes to that chapter's start.
        assert!((super::prev_point(&points, 60.0 + frame, frame) - 0.0).abs() < 1e-9);
        assert!((super::prev_point(&points, 60.0 + 2.5 * frame, frame) - 60.0).abs() < 1e-9);
        assert_eq!(super::next_point(&points, 60.0), Some(120.0));
        assert_eq!(super::next_point(&points, 120.0), None);
    }

    #[test]
    fn closed_state_is_empty() {
        let state = parse_preview_state(&json!({ "open": false }));
        assert!(!state.open);
        assert!(state.label.is_empty());
    }

    #[test]
    fn menu_location_is_parsed() {
        let state = parse_preview_state(&json!({
            "open": true,
            "location": { "menu": { "pgcn": 2, "button": 3 } }
        }));

        assert!(state.open);
        assert_eq!(state.label, "メニュー 2（ボタン 3）");

        let menu = state
            .menu
            .clone()
            .unwrap_or(super::PreviewMenu { pgcn: 0, button: 0 });
        assert_eq!((menu.pgcn, menu.button), (2, 3));
    }

    #[test]
    fn title_location_has_no_menu() {
        let state = parse_preview_state(&json!({
            "open": true,
            "location": { "title": { "title": 1, "chapter": 2 } }
        }));

        assert!(state.open);
        assert_eq!(state.label, "タイトル 1 チャプター 2");
        assert!(state.menu.is_none());
    }

    #[test]
    fn stopped_location_has_no_menu() {
        let state = parse_preview_state(&json!({ "open": true, "location": "stopped" }));

        assert!(state.open);
        // Shown in the display language, not the VM's word.
        assert_eq!(state.label, crate::i18n::tl!("Stopped"));
        assert!(state.menu.is_none());
    }

    #[test]
    fn a_title_location_carries_the_chosen_streams() {
        let state = parse_preview_state(&json!({
            "open": true,
            "location": { "title": { "title": 2, "chapter": 3 } },
            "audio": 2,
            "subtitle": { "on": true, "track": 1 }
        }));
        assert_eq!(state.title, Some((2, 3)));
        assert_eq!(state.audio, Some(2));
        assert_eq!(state.subtitle, Some(1));
        // A hidden subtitle is none; a menu is no title.
        let state = parse_preview_state(&json!({
            "open": true,
            "location": { "menu": { "pgcn": 1, "button": 1 } },
            "audio": null,
            "subtitle": { "on": false, "track": 1 }
        }));
        assert_eq!(state.title, None);
        assert_eq!(state.audio, None);
        assert_eq!(state.subtitle, None);
    }

    #[test]
    fn missing_location_defaults_to_stopped_label() {
        let PreviewState {
            open, label, menu, ..
        } = parse_preview_state(&json!({ "open": true }));

        assert!(open);
        assert_eq!(label, "停止");
        assert!(menu.is_none());
    }
}
