//! モニターパネル（[docs/ui-parity.md](../../../docs/ui-parity.md) §3、M6.8）。
//!
//! ビルド済みの `VIDEO_TS` を `preview.*` コマンドで動かし、メニュー画面を
//! `disccraft-menu` の描画で映しながらリモコン操作を送る。映像そのものは
//! デコードせず、メニューの静止画と現在位置の表示で状態を示す。

use crate::app::{DiscCraftApp, Selection};
use crate::i18n::{tl, tlf};
use crate::workspace::CenterTab;

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
        }
    }
}

/// プレビューの状態表示。
#[derive(Clone, Debug, Default)]
pub(crate) struct PreviewState {
    pub(crate) open: bool,
    pub(crate) label: String,
    pub(crate) menu: Option<PreviewMenu>,
}

#[derive(Clone, Debug)]
pub(crate) struct PreviewMenu {
    /// VMGM の PGCN（1 始まり）。
    pub(crate) pgcn: u16,
    /// ハイライトされているボタン（1 始まり。0 は未選択）。
    pub(crate) button: u8,
}

impl DiscCraftApp {
    /// モニターパネル。Encore のプレビューと同じく、上に映像（メニュー）、下に操作バーを置く。
    pub(crate) fn monitor_view(&mut self, ui: &mut Ui) {
        // 状態は開いたときとキー操作のときにだけ更新する（毎フレームのコマンド実行は
        // ステータス欄と dirty フラグを汚すので避ける）。
        let state = self.preview.clone();
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

        egui::Panel::bottom("monitor_controls")
            .resizable(false)
            .show(ui, |ui| self.monitor_bar(ui, &state, timecode.as_deref()));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(egui::Color32::BLACK))
            .show(ui, |ui| {
                if let Some((texture, _)) = &timeline_frame {
                    // 4:3 の画面として映す（拡大率に合わせる。はみ出たところは切れる）。
                    let area = ui.max_rect();
                    let size = self.monitor_zoom.size(area.size(), 4.0 / 3.0);
                    let picture = egui::Rect::from_center_size(area.center(), size);
                    ui.painter().image(
                        texture.id(),
                        picture,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    if let Some(text) = &subtitle {
                        draw_subtitle(ui.painter(), picture.intersect(area), text);
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

        // 左: 拡大率。
        let mut zoom = self.monitor_zoom;
        zoom_combo(ui, "monitor_zoom", &mut zoom);
        self.monitor_zoom = zoom;

        // 中央: プレビュー中はリモコン、それ以外はタイムラインの移動ボタン。
        let center_at = start + (total - center_width) / 2.0;
        ui.add_space((center_at - ui.cursor().min.x).max(gap));
        let center_start = ui.cursor().min.x;
        match (state.open, self.viewer_timeline()) {
            (false, Some(id)) => self.timeline_transport(ui, id),
            _ => {
                ui.add_enabled_ui(state.open, |ui| self.remote_controls(ui));
            }
        }
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

    /// タイムラインの移動ボタン（Encore のモニターの並び）: 1 フレーム戻す / 再生 / 1 フレーム進める |
    /// 前のチャプター / 今のチャプターの番号 / 次のチャプター / チャプターを追加（再生ヘッドの位置に）。
    fn timeline_transport(&mut self, ui: &mut Ui, id: disccraft_project::TimelineId) {
        let Some(timeline) = self.session.project.timeline(id).cloned() else {
            return;
        };
        let ctx = ui.ctx().clone();
        let (num, den) = crate::timeline_view::frame_rate(self.session.project.settings.standard);
        let step = f64::from(den.max(1)) / f64::from(num.max(1));
        let now = crate::timeline_view::playhead_seconds(&ctx, id);
        let points: Vec<f64> = timeline
            .chapters
            .iter()
            .map(|chapter| crate::timeline_view::ticks_to_seconds(chapter.time.0))
            .collect();
        let end = self.asset_seconds(timeline.video).unwrap_or(now);

        let mut target = None;
        let mut toggle = false;
        let playing = self.is_playing();
        ui.horizontal(|ui| {
            if ui.button("◀|").on_hover_text(tl!("Previous Frame")).clicked() {
                target = Some((now - step).max(0.0));
            }
            let (play, tip) = if playing {
                ("■", tl!("Stop"))
            } else {
                ("▶", tl!("Play"))
            };
            if ui
                .button(play)
                .on_hover_text(format!("{tip} (Space)"))
                .clicked()
            {
                toggle = true;
            }
            if ui.button("|▶").on_hover_text(tl!("Next Frame")).clicked() {
                target = Some((now + step).min(end));
            }
            ui.separator();
            if ui.button("|◀◀").on_hover_text(tl!("Previous Chapter")).clicked() {
                target = Some(prev_point(&points, now));
            }
            let _ = ui
                .label(format!("{:02}", chapter_number(&points, now)))
                .on_hover_text(tl!("Chapter"));
            if ui.button("▶▶|").on_hover_text(tl!("Next Chapter")).clicked() {
                target = Some(next_point(&points, now).unwrap_or(end));
            }
            if ui
                .button("+")
                .on_hover_text(tl!("Add Chapter at Playhead"))
                .clicked()
            {
                let _ = self.run_command(
                    "timeline.addChapter",
                    json!({ "timeline": id.0, "time": crate::timeline_view::seconds_to_ticks(now) }),
                );
            }
        });

        if let Some(seconds) = target {
            // 動かしたら再生は止める。
            self.playback = None;
            crate::timeline_view::set_playhead_seconds(&ctx, id, seconds);
        }
        if toggle {
            self.toggle_playback(&ctx);
        }
    }

    /// タイムラインビューアのタイムラインの、再生ヘッドの位置のフレームと時刻。フレームが変わったときだけ
    /// `asset.frame` で取り直す（マウスのボタンを押している間は取り直さない）。
    fn timeline_frame(&mut self, ctx: &egui::Context) -> Option<(egui::TextureHandle, String)> {
        let id = self.viewer_timeline()?;
        let video = self.session.project.timeline(id)?.video;
        let (num, den) = crate::timeline_view::frame_rate(self.session.project.settings.standard);
        let fps = f64::from(num) / f64::from(den.max(1));
        let seconds = crate::timeline_view::playhead_seconds(ctx, id);
        // `as` saturates; the frame number is only a cache key.
        let frame = (seconds * fps).round() as i64;
        let timecode = crate::timeline_view::timecode(seconds, num, den);

        let cached = self
            .monitor_frame
            .as_ref()
            .filter(|(asset, cached_frame, _)| *asset == video.0 && *cached_frame == frame)
            .map(|(_, _, texture)| texture.clone());
        if let Some(texture) = cached {
            return Some((texture, timecode));
        }

        let dragging = ctx.input(|input| input.pointer.any_down());
        if dragging && let Some((_, _, texture)) = &self.monitor_frame {
            return Some((texture.clone(), timecode));
        }

        let value = self.run_query(
            "asset.frame",
            json!({ "asset": video.0, "seconds": f64::from(u32::try_from(frame.max(0)).unwrap_or(u32::MAX)) / fps }),
        )?;
        let texture = crate::project_panel::decode_rgb_texture(ctx, "monitor_frame", &value)?;
        self.monitor_frame = Some((video.0, frame, texture.clone()));
        Some((texture, timecode))
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
            self.menu_canvas(ui, &project_menu, Some(highlight), zoom);
        });
    }

    /// VMGM の PGCN からプロジェクトのメニューを引く。
    ///
    /// `disccraft-nav` はメニューを `MenuId` 昇順に PGCN 1..=n へ割り当てるため、
    /// 隠し PGC を除けばこの対応で戻る。
    fn menu_for_pgcn(&self, pgcn: u16) -> Option<disccraft_project::MenuId> {
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
                    RemoteKey::Finish | RemoteKey::Menu | RemoteKey::Title => false,
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

        let value = json!({ "path": path });

        if let Some(state) = self.run_query("preview.open", value) {
            self.status = tlf!("Preview opened: {path}", path = path);
            self.preview = parse_preview_state(&state);
        }
    }

    /// Encore の「ここからプレビューする」: ビルド結果をそのメニュー / タイトルから開く（メニューは VMGM の PGCN、
    /// タイトルはタイムラインの順、そのあとにスライドショー。ビルドと同じ数え方）。
    pub(crate) fn preview_from(&mut self, selection: Selection) {
        let Some(path) = self.preview_folder() else {
            self.status = tl!("Build first").to_string();
            return;
        };
        let project = &self.session.project;
        let mut params = serde_json::Map::new();
        let _ = params.insert("path".to_owned(), Value::from(path.clone()));
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
        self.tab = CenterTab::Monitor;
        if let Some(state) = self.run_query("preview.open", Value::Object(params)) {
            self.status = tlf!("Preview opened: {path}", path = path);
            self.preview = parse_preview_state(&state);
        }
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

        self.preview = parse_preview_state(&state);
        if key == RemoteKey::Menu && self.preview.label == before {
            // プレーヤーと同じく何も起きない。理由はステータスに出す。
            self.status = tl!("This timeline has no menu remote").to_string();
        }
    }

    /// プレビューするフォルダ。ビルド結果がフォルダのときはその `VIDEO_TS`、
    /// ISO のときは同じ場所にある `VIDEO_TS` を探す。
    fn preview_folder(&self) -> Option<String> {
        let output = self
            .last_build
            .as_ref()
            .map(|report| report.output.clone())?;

        let candidate = if output.is_dir() {
            let direct = output.join("VIDEO_TS");
            if direct.is_dir() { direct } else { output }
        } else {
            output.parent()?.join("VIDEO_TS")
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

/// 字幕を映像の下の方に、黒い縁取りの白い文字で重ねる。
fn draw_subtitle(painter: &egui::Painter, picture: egui::Rect, text: &str) {
    let size = (picture.height() / 18.0).clamp(10.0, 40.0);
    let font = egui::FontId::proportional(size);
    let anchor = egui::pos2(picture.center().x, picture.max.y - picture.height() * 0.06);
    for (dx, dy) in [(-1.5, 0.0), (1.5, 0.0), (0.0, -1.5), (0.0, 1.5)] {
        painter.text(
            anchor + egui::vec2(dx, dy),
            egui::Align2::CENTER_BOTTOM,
            text,
            font.clone(),
            egui::Color32::BLACK,
        );
    }
    painter.text(
        anchor,
        egui::Align2::CENTER_BOTTOM,
        text,
        font,
        egui::Color32::WHITE,
    );
}

/// `preview.*` の戻り値を画面表示用にまとめる。
pub(crate) fn parse_preview_state(value: &Value) -> PreviewState {
    let open = value.get("open").and_then(Value::as_bool).unwrap_or(false);

    if !open {
        return PreviewState::default();
    }

    let Some(location) = value.get("location") else {
        return PreviewState {
            open: true,
            label: tl!("Stop").to_string(),
            menu: None,
        };
    };

    if let Some(text) = location.as_str() {
        return PreviewState {
            open: true,
            label: text.to_string(),
            menu: None,
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
        };
    }

    PreviewState {
        open: true,
        label: tl!("Unknown").to_string(),
        menu: None,
    }
}

/// The number (from 1) of the chapter that contains `now`: how many chapter points are at or before it (at least 1).
fn chapter_number(points: &[f64], now: f64) -> usize {
    points
        .iter()
        .filter(|point| **point <= now + 1e-6)
        .count()
        .max(1)
}

/// `now` より前の最後のチャプターポイント（なければ 0）。同じ位置にいるときはひとつ前へ戻る。
pub(crate) fn prev_point(points: &[f64], now: f64) -> f64 {
    points
        .iter()
        .copied()
        .filter(|point| *point < now - 0.001)
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

#[cfg(test)]
mod tests {
    use super::{PreviewState, chapter_number, parse_preview_state};
    use serde_json::json;

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
        assert!((super::prev_point(&points, 90.0) - 60.0).abs() < 1e-9);
        assert!((super::prev_point(&points, 60.0) - 0.0).abs() < 1e-9);
        assert!((super::prev_point(&points, 0.0) - 0.0).abs() < 1e-9);
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
        assert_eq!(state.label, "stopped");
        assert!(state.menu.is_none());
    }

    #[test]
    fn missing_location_defaults_to_stopped_label() {
        let PreviewState { open, label, menu } = parse_preview_state(&json!({ "open": true }));

        assert!(open);
        assert_eq!(label, "停止");
        assert!(menu.is_none());
    }
}
