//! モニターの下の操作部（2 段）。
//!
//! 1. Encore の操作バー（M6.476。Encore CS6 で並びと動きを確かめ、描き方は DiscCraft のもの）: 左に今の時刻（青、
//!    等幅）と拡大率とセーフエリア、中ほどに 字幕の 5 つのボタン | 1 フレーム戻す・再生 / 停止・1 フレーム進める
//!    | 前のチャプター・今のチャプターの番号・次のチャプター・チャプターを追加、右に再生の速さ（M6.427）と
//!    「ビルド結果を開く」とタイムラインの長さ。狭いときは速さ、ビルド結果を開く、字幕のボタンの順に隠す。
//! 2. 小さなタイムライン（スクラブバー。FilmCraft（MIT OR Apache-2.0）の `panels/monitor.rs` の `mini_timeline`
//!    の描き方を移したもの。M9.12）: 目盛り、チャプターポイントの旗、青い再生ヘッド。押す・ドラッグで動かす。

use crate::DiscCraftApp;
use crate::craft_timeline::{self, style};
use crate::i18n::{tl, tlf};
use crate::icons::{self, Icon};
use crate::timeline_view::{frame_rate, playhead_seconds, set_playhead_seconds, timecode};

use disccraft_project::TimelineId;
use egui::{Align2, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

/// 1 段目（操作バー）の高さ。
const ROW1: f32 = 26.0;
/// スクラブバーの高さ。
const SCRUB: f32 = 22.0;
/// 操作部全体の高さ。
pub(crate) const CONTROLS_HEIGHT: f32 = 2.0 + ROW1 + 4.0 + SCRUB + 6.0;
/// 左右の余白（FilmCraft と同じ 14 pt）。
const SIDE: f32 = 14.0;
/// 再生の速さのメニューの幅。
const SPEED_WIDTH: f32 = 64.0;
/// 「ビルド結果を開く」の幅。
const OPEN_WIDTH: f32 = 120.0;
/// 右の項目の前の空き。
const EXTRA_GAP: f32 = 8.0;
/// 操作バーのボタンの間隔と、マウスが乗ったときの箱の大きさ（Encore: 25 おきに 23 × 16）。
const BAR_PITCH: f32 = 25.0;
const BAR_BOX: egui::Vec2 = vec2(23.0, 16.0);
/// ボタンのアイコンの大きさ（描いた絵が Encore のアイコンと同じくらいの 10 前後になる大きさ）。
const BAR_ICON_SIZE: f32 = 18.0;
/// ボタンのまとまりの間の区切り線の場所の幅と、線の高さ（Encore: 18）。
const SEPARATOR: f32 = 10.0;
const SEPARATOR_HEIGHT: f32 = 18.0;
/// 字幕の 5 つのボタンの幅（`subtitle_buttons` が 26 おきに置く）。
const SUBTITLES_WIDTH: f32 = 5.0 * 26.0;
/// 1 フレーム戻す・再生・1 フレーム進める と、前のチャプター・番号・次のチャプター・チャプターを追加 の幅。
const MIDDLE_WIDTH: f32 = 3.0 * BAR_PITCH + SEPARATOR + 4.0 * BAR_PITCH;

/// 操作バーのボタンの操作。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Transport {
    StepBack,
    PlayStop,
    StepForward,
    PreviousChapter,
    NextChapter,
    AddChapter,
}

/// 操作バーの中ほどのボタンの置き場所と、入る項目。
#[derive(Clone, Copy, Debug, PartialEq)]
struct BarLayout {
    /// 中ほどのボタン（字幕のボタンを出すときはその左端から）の左端。
    start: f32,
    /// 字幕の 5 つのボタンを出すか。
    subtitles: bool,
    /// 再生の速さを出すか。
    speed: bool,
    /// 「ビルド結果を開く」を出すか。
    open: bool,
}

/// 操作バーの中ほどのボタンを Encore と同じくバーの中央 `center` に置き、`left`（セーフエリアの右）から `right`
/// （タイムラインの長さの左）の間に入る DiscCraft の項目を決める。入らないときは速さ、ビルド結果を開く、字幕の
/// ボタンの順に隠し、それでも入らなければ左に寄せる。
fn bar_layout(left: f32, right: f32, center: f32) -> BarLayout {
    for (subtitles, speed, open) in [
        (true, true, true),
        (true, false, true),
        (true, false, false),
        (false, false, false),
    ] {
        let middle = MIDDLE_WIDTH
            + if subtitles {
                SUBTITLES_WIDTH + SEPARATOR
            } else {
                0.0
            };
        let extras = if speed { SPEED_WIDTH + EXTRA_GAP } else { 0.0 }
            + if open { OPEN_WIDTH + EXTRA_GAP } else { 0.0 };
        let room = right - extras;
        if left + middle <= room {
            return BarLayout {
                start: (center - middle / 2.0).clamp(left, room - middle),
                subtitles,
                speed,
                open,
            };
        }
    }
    BarLayout {
        start: left,
        subtitles: false,
        speed: false,
        open: false,
    }
}

impl DiscCraftApp {
    /// タイムラインを映しているモニターの操作部（2 段）。
    pub(crate) fn timeline_controls(&mut self, ui: &mut Ui, id: TimelineId) {
        let Some(timeline) = self.session.project.timeline(id).cloned() else {
            return;
        };
        let ctx = ui.ctx().clone();
        let (num, den) = frame_rate(self.session.project.settings.standard);
        let fps = f64::from(num) / f64::from(den.max(1));
        let step = 1.0 / fps.max(1.0);
        let now = playhead_seconds(&ctx, id);
        let end = Some(self.timeline_seconds(id))
            .filter(|seconds| *seconds > 0.0)
            .unwrap_or(now)
            .max(step);
        let points: Vec<f64> = timeline
            .chapters
            .iter()
            .map(|chapter| crate::timeline_view::ticks_to_seconds(chapter.time.0))
            .collect();
        let chapter = crate::monitor::chapter_number(&points, now);

        let width = ui.available_width();
        let (area, _) = ui.allocate_exact_size(vec2(width, CONTROLS_HEIGHT), Sense::hover());

        // 1 段目: Encore の操作バー。左に拡大率とセーフエリア、右端に今の時刻（M6.483: Encore と同じオレンジの
        // ホットテキスト。横にドラッグで 1 点 1 フレーム、押すと打ち込める）。
        let row1 = Rect::from_min_size(
            pos2(area.min.x + SIDE, area.min.y + 2.0),
            vec2((width - 2.0 * SIDE).max(1.0), ROW1),
        );
        let painter = ui.painter().clone();
        let zoom_rect = Rect::from_min_size(pos2(row1.min.x, row1.min.y + 1.0), vec2(96.0, 24.0));
        let mut zoom = self.monitor_zoom;
        ui.scope_builder(egui::UiBuilder::new().max_rect(zoom_rect), |ui| {
            crate::monitor::zoom_combo(ui, "monitor_zoom", &mut zoom);
        });
        self.monitor_zoom = zoom;
        // Encore と同じく、拡大率の右にセーフエリアの表示 / 非表示。
        let safe = Rect::from_min_size(
            pos2(zoom_rect.max.x + 6.0, zoom_rect.min.y),
            vec2(24.0, 24.0),
        );
        crate::monitor::safe_area_button(ui, safe, &mut self.show_monitor_safe_areas);
        let now_text = timecode(now, num, den);
        let time_font = FontId::proportional(12.5);
        let time_width = painter
            .layout_no_wrap(now_text.clone(), time_font.clone(), egui::Color32::WHITE)
            .size()
            .x;
        let mut moved_to: Option<f64> = None;
        match crate::hot_text::hot_timecode(
            ui,
            ui.scope_id().with("monitor_time"),
            (row1.right_center(), Align2::RIGHT_CENTER),
            &now_text,
            time_font,
        ) {
            crate::hot_text::HotChange::Frames(frames) => {
                // `as` is exact for a drag's frames.
                moved_to = Some(now + frames as f64 * step);
            }
            crate::hot_text::HotChange::Typed(text) => {
                match disccraft_project::parse_time(&text, &self.session.project.settings) {
                    Ok(time) => moved_to = Some(crate::timeline_view::ticks_to_seconds(time.0)),
                    Err(error) => self.status = error.to_string(),
                }
            }
            crate::hot_text::HotChange::None => {}
        }

        // 中ほど: 字幕の 5 つのボタン | 1 フレーム戻す・再生・1 フレーム進める | 前のチャプター・番号・
        // 次のチャプター・チャプターを追加（Encore の並び）。
        let right = row1.max.x - time_width - 16.0;
        let layout = bar_layout(safe.max.x + 10.0, right, row1.center().x);
        let middle_y = row1.center().y;
        let lines = egui::Rangef::new(
            middle_y - SEPARATOR_HEIGHT / 2.0,
            middle_y + SEPARATOR_HEIGHT / 2.0,
        );
        let slot = |x: f32| Rect::from_center_size(pos2(x + BAR_PITCH / 2.0, middle_y), BAR_BOX);
        let mut x = layout.start;
        if layout.subtitles {
            self.subtitle_buttons(ui, pos2(x, safe.min.y), id, now, (num, den));
            x += SUBTITLES_WIDTH;
            crate::theme::bar_separator(&painter, x + SEPARATOR / 2.0 - 1.0, lines);
            x += SEPARATOR;
        }
        let playing = self.is_playing();
        let mut pressed = None;
        for (icon, action, tip) in [
            (
                Icon::StepBack,
                Transport::StepBack,
                tl!("Previous Frame (Left Arrow)"),
            ),
            (
                if playing { Icon::Pause } else { Icon::Play },
                Transport::PlayStop,
                tl!("Play / Stop (Space)"),
            ),
            (
                Icon::StepFwd,
                Transport::StepForward,
                tl!("Next Frame (Right Arrow)"),
            ),
        ] {
            if bar_button(ui, slot(x), icon, true, tip) {
                pressed = Some(action);
            }
            x += BAR_PITCH;
        }
        crate::theme::bar_separator(&painter, x + SEPARATOR / 2.0 - 1.0, lines);
        x += SEPARATOR;
        if bar_button(ui, slot(x), Icon::PrevEdit, true, tl!("Previous Chapter")) {
            pressed = Some(Transport::PreviousChapter);
        }
        x += BAR_PITCH;
        // 今のチャプターの番号（Encore: 2 桁。ツールチップは「チャプター 01」）。
        let number = format!("{chapter:02}");
        let number_rect = Rect::from_min_size(pos2(x, row1.min.y), vec2(BAR_PITCH, ROW1));
        painter.text(
            number_rect.center(),
            Align2::CENTER_CENTER,
            &number,
            FontId::proportional(12.0),
            crate::theme::BAR_ICON.get(),
        );
        let _ = ui
            .interact(
                number_rect,
                ui.scope_id().with("monitor_chapter_number"),
                Sense::hover(),
            )
            .on_hover_text(tlf!("Chapter {n}", n = number));
        x += BAR_PITCH;
        // Encore: 最後のチャプターでは灰色。
        let next = crate::monitor::next_point(&points, now);
        if bar_button(
            ui,
            slot(x),
            Icon::NextEdit,
            next.is_some(),
            tl!("Next Chapter"),
        ) {
            pressed = Some(Transport::NextChapter);
        }
        x += BAR_PITCH;
        // Encore: タイムラインの頭（チャプター 1 がある）では灰色。
        if bar_button(
            ui,
            slot(x),
            Icon::Marker,
            now >= step / 2.0,
            tl!("Add Chapter at Playhead"),
        ) {
            pressed = Some(Transport::AddChapter);
        }

        // 右: タイムラインの長さの左に「ビルド結果を開く」、その左に再生の速さ。
        let mut extra_right = right;
        if layout.open {
            let open_rect = Rect::from_min_size(
                pos2(extra_right - OPEN_WIDTH, middle_y - 11.0),
                vec2(OPEN_WIDTH, 22.0),
            );
            let open = ui.put(
                open_rect,
                egui::Button::new(tl!("Open Build Output")).wrap_mode(egui::TextWrapMode::Truncate),
            );
            if open.clicked() {
                self.open_preview();
            }
            extra_right = open_rect.min.x - EXTRA_GAP;
        }
        if layout.speed {
            let speed_rect = Rect::from_min_size(
                pos2(extra_right - SPEED_WIDTH, middle_y - 11.0),
                vec2(SPEED_WIDTH, 22.0),
            );
            let mut speed = self.playback_speed;
            ui.scope_builder(egui::UiBuilder::new().max_rect(speed_rect), |ui| {
                egui::ComboBox::from_id_salt("monitor_speed")
                    .width(SPEED_WIDTH)
                    .selected_text(crate::playback::speed_label(speed))
                    .show_ui(ui, |ui| {
                        for step in crate::playback::PLAYBACK_SPEEDS {
                            let _ = ui.selectable_value(
                                &mut speed,
                                step,
                                crate::playback::speed_label(step),
                            );
                        }
                    })
                    .response
                    .on_hover_text(tl!(
                        "Playback speed (J slower, L faster, K stop). The sound keeps its pitch up to 2×; faster playback is silent."
                    ));
            });
            if (speed - self.playback_speed).abs() > 1e-9 {
                self.set_playback_speed(&ctx, speed);
            }
        }

        // 2 段目: スクラブバー。
        let bar = Rect::from_min_size(
            pos2(row1.min.x, row1.max.y + 4.0),
            vec2(row1.width(), SCRUB),
        );
        if let Some(seconds) = scrub_bar(ui, bar, (now, end, fps), &points) {
            self.playback = None;
            set_playhead_seconds(&ctx, id, seconds);
        }

        let target = match pressed {
            Some(Transport::StepBack) => Some(step_frames(now, -1, fps, end)),
            Some(Transport::PlayStop) => {
                self.toggle_playback(&ctx);
                None
            }
            Some(Transport::StepForward) => Some(step_frames(now, 1, fps, end)),
            Some(Transport::PreviousChapter) => {
                Some(crate::monitor::prev_point(&points, now, step))
            }
            Some(Transport::NextChapter) => next,
            Some(Transport::AddChapter) => {
                let _ = self.run_command(
                    "timeline.addChapter",
                    json!({ "timeline": id.0, "time": crate::timeline_view::seconds_to_ticks(now) }),
                );
                None
            }
            None => None,
        };
        if let Some(seconds) = target.or(moved_to.map(|seconds| seconds.clamp(0.0, end))) {
            // 動かしたら再生は止める。
            self.playback = None;
            set_playhead_seconds(&ctx, id, seconds);
        }
    }

    /// ← / → で、タイムラインビューアのタイムラインを 1 フレームずつ戻す / 進める（押し続けると続けて送る。
    /// 再生中なら止める）。Home / End で頭と終わりへ（Encore と同じ。M6.476 でモニターの 先頭へ / 末尾へ の
    /// ボタンの代わりに）。文字の入力中と、モニターでビルドしたディスクをプレビューしている間（矢印は
    /// リモコン）は何もしない。メニュービューアで選んだボタンを矢印で動かしたときは、そちらが先にキーを使う。
    pub(crate) fn step_frames_by_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() || (self.project_preview.is_none() && self.preview.open)
        {
            return;
        }
        let Some(id) = self.viewer_timeline() else {
            return;
        };
        let (frames, jump) = ctx.input_mut(|input| {
            let mut frames = 0_i32;
            while input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
                frames = frames.saturating_sub(1);
            }
            while input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
                frames = frames.saturating_add(1);
            }
            let jump = if input.consume_key(egui::Modifiers::NONE, egui::Key::Home) {
                Some(Jump::Start)
            } else if input.consume_key(egui::Modifiers::NONE, egui::Key::End) {
                Some(Jump::End)
            } else {
                None
            };
            (frames, jump)
        });
        if frames == 0 && jump.is_none() {
            return;
        }
        let Some(video) = self
            .session
            .project
            .timeline(id)
            .map(|timeline| timeline.video)
        else {
            return;
        };
        let (num, den) = frame_rate(self.session.project.settings.standard);
        let fps = f64::from(num) / f64::from(den.max(1));
        let now = playhead_seconds(ctx, id);
        let end = self.asset_seconds(video).unwrap_or(now);
        let to = match jump {
            Some(Jump::Start) => 0.0,
            Some(Jump::End) => Some(self.timeline_seconds(id))
                .filter(|seconds| *seconds > 0.0)
                .unwrap_or(end),
            None => step_frames(now, frames, fps, end),
        };
        self.playback = None;
        set_playhead_seconds(ctx, id, to);
    }
}

/// Home / End で動かす先。
#[derive(Clone, Copy)]
enum Jump {
    Start,
    End,
}

/// `now` から `frames` フレーム（負なら戻る）動かした時刻。フレームの頭にそろえ、0 から `end` の間に収める。
pub(crate) fn step_frames(now: f64, frames: i32, fps: f64, end: f64) -> f64 {
    let end = if end.is_finite() { end.max(0.0) } else { 0.0 };
    let now = if now.is_finite() { now } else { 0.0 };
    if !(fps.is_finite() && fps > 0.0) {
        return now.clamp(0.0, end);
    }
    let frame = (now * fps).round() + f64::from(frames);
    (frame / fps).clamp(0.0, end)
}

/// Encore の操作バーのボタン（M6.476）: アイコン（押せないときは暗く）と、マウスが乗ったときの箱と金色のアイコン。
/// 押されたか。
fn bar_button(ui: &mut Ui, rect: Rect, icon: Icon, enabled: bool, tip: &str) -> bool {
    let response = ui
        .interact(
            rect,
            ui.scope_id().with(("monitor_transport", tip)),
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        )
        .on_hover_text(tip);
    let hovered = enabled && response.hovered();
    if hovered {
        crate::theme::hover_box(ui.painter(), rect);
    }
    let color = if hovered {
        crate::theme::HOVER_ICON.get()
    } else if enabled {
        crate::theme::BAR_ICON.get()
    } else {
        ui.visuals().weak_text_color().gamma_multiply(0.6)
    };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(BAR_ICON_SIZE, BAR_ICON_SIZE)),
        icon,
        color,
    );
    enabled && response.clicked()
}

/// FilmCraft の小さなタイムライン: 目盛り（FilmCraft の定規と同じ刻み）、チャプターポイントの旗、青い再生ヘッド。
/// 押すかドラッグした位置の秒（フレームにそろえる）を返す。`(now, end, fps)` は再生ヘッド、長さ、フレームレート。
fn scrub_bar(
    ui: &mut Ui,
    bar: Rect,
    (now, end, fps): (f64, f64, f64),
    points: &[f64],
) -> Option<f64> {
    let response = ui.interact(
        bar,
        ui.scope_id().with("monitor_scrub"),
        Sense::click_and_drag(),
    );
    let painter = ui.painter();
    let x_of = |seconds: f64| bar.min.x + ((seconds / end).clamp(0.0, 1.0) as f32) * bar.width();

    // 目盛り: 小さいものは 3.5 pt、大きいものは 8 pt（大きいものは 110 pt 以上、小さいものは 12 pt 以上離す）。
    let frames = (end * fps).max(1.0);
    let frame_px = f64::from(bar.width()) / frames;
    // `as` saturates; the base is a small frame count.
    let base = fps.round().max(1.0) as i64;
    let (minor, major) = craft_timeline::ruler_steps(frame_px, base);
    let mut frame = 0i64;
    for _ in 0..512 {
        // Frame numbers on screen are small; exact in f64.
        let x = bar.min.x + (frame as f64 * frame_px) as f32;
        if x > bar.max.x + 0.5 {
            break;
        }
        let height = if frame % major == 0 { 8.0 } else { 3.5 };
        painter.line_segment(
            [pos2(x, bar.max.y - height), pos2(x, bar.max.y)],
            Stroke::new(1.0, style::RULER_TICK),
        );
        frame = frame.saturating_add(minor);
    }

    // チャプターポイント: 緑の旗（FilmCraft のマーカー）。
    for point in points {
        let x = x_of(*point);
        craft_timeline::marker_glyph(
            painter,
            Rect::from_min_max(pos2(x - 4.0, bar.min.y), pos2(x + 4.0, bar.min.y + 10.0)),
            3.0,
            craft_timeline::MarkerPart::Whole,
            style::MARKER.get(),
        );
    }

    // 再生ヘッド: 線の上に青い三角。
    let x = x_of(now);
    let head = bar.max.y - 11.0;
    painter.add(egui::Shape::convex_polygon(
        vec![
            pos2(x - 5.5, head),
            pos2(x + 5.5, head),
            pos2(x + 5.5, head + 5.0),
            pos2(x, head + 9.0),
            pos2(x - 5.5, head + 5.0),
        ],
        style::PLAYHEAD_HEAD_BOTTOM,
        Stroke::NONE,
    ));
    painter.line_segment(
        [pos2(x, head + 8.0), pos2(x, bar.max.y)],
        Stroke::new(1.0, style::PLAYHEAD_LINE),
    );

    if !(response.clicked() || response.dragged()) {
        return None;
    }
    let pointer = response.interact_pointer_pos()?;
    let fraction = f64::from(((pointer.x - bar.min.x) / bar.width().max(1.0)).clamp(0.0, 1.0));
    Some(snap_to_frame(fraction * end, fps))
}

/// `seconds` をいちばん近いフレームの頭にそろえる。
pub(crate) fn snap_to_frame(seconds: f64, fps: f64) -> f64 {
    if fps <= 0.0 || !seconds.is_finite() {
        return seconds.max(0.0);
    }
    ((seconds * fps).round() / fps).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::{
        BarLayout, EXTRA_GAP, MIDDLE_WIDTH, OPEN_WIDTH, SEPARATOR, SPEED_WIDTH, SUBTITLES_WIDTH,
        bar_layout, snap_to_frame, step_frames,
    };

    #[test]
    fn arrow_keys_step_one_frame_within_the_timeline() {
        let fps = 30000.0 / 1001.0;
        let next = step_frames(1.0, 1, fps, 6.0);
        assert_eq!((next * fps).round(), (1.0_f64 * fps).round() + 1.0);
        let back = step_frames(next, -1, fps, 6.0);
        assert_eq!((back * fps).round(), (1.0_f64 * fps).round());
        // 頭より前、終わりより後には行かない。
        assert_eq!(step_frames(0.0, -1, fps, 6.0), 0.0);
        assert_eq!(step_frames(6.0, 1, fps, 6.0), 6.0);
        // 押し続けて 1 フレームに何回か来ても、その分だけ進む。
        let three = step_frames(0.0, 3, 25.0, 6.0);
        assert!((three - 0.12).abs() < 1e-9);
        // 壊れた値でも落ちない。
        assert_eq!(step_frames(f64::NAN, 1, 25.0, 6.0), 0.04);
        assert_eq!(step_frames(2.0, 1, 0.0, 6.0), 2.0);
        assert_eq!(step_frames(2.0, 1, 25.0, f64::NAN), 0.0);
    }

    #[test]
    fn a_scrub_lands_on_a_frame() {
        let fps = 30_000.0 / 1001.0;
        let snapped = snap_to_frame(1.02, fps);
        // Frame 31 of 29.97 fps.
        assert!((snapped - 31.0 / fps).abs() < 1e-9, "{snapped}");
        assert_eq!(snap_to_frame(-1.0, fps), 0.0);
        assert_eq!(snap_to_frame(f64::NAN, fps), 0.0);
        assert_eq!(snap_to_frame(2.0, 0.0), 2.0);
    }

    #[test]
    fn the_bar_centres_encores_buttons_and_drops_extras_when_narrow() {
        let full = SUBTITLES_WIDTH + SEPARATOR + MIDDLE_WIDTH;
        let extras = SPEED_WIDTH + OPEN_WIDTH + 2.0 * EXTRA_GAP;
        // Wide: everything, the middle in the bar's centre (Encore).
        let wide = bar_layout(270.0, 1000.0, 540.0);
        assert_eq!(
            wide,
            BarLayout {
                start: 540.0 - full / 2.0,
                subtitles: true,
                speed: true,
                open: true,
            }
        );
        // Room only just for all of it: pushed right up to the left end.
        let tight = bar_layout(100.0, 100.0 + full + extras, 900.0);
        assert!(tight.subtitles && tight.speed && tight.open);
        assert_eq!(tight.start, 100.0);
        // A little narrower: the speed menu goes first, then Open Build Output, then the subtitle buttons.
        let no_speed = bar_layout(100.0, 100.0 + full + extras - 1.0, 400.0);
        assert!(no_speed.subtitles && !no_speed.speed && no_speed.open);
        let no_open = bar_layout(100.0, 100.0 + full + OPEN_WIDTH, 400.0);
        assert!(no_open.subtitles && !no_open.speed && !no_open.open);
        let bare = bar_layout(100.0, 100.0 + full - 1.0, 200.0);
        assert_eq!(
            bare,
            BarLayout {
                start: 200.0 - MIDDLE_WIDTH / 2.0,
                subtitles: false,
                speed: false,
                open: false,
            }
        );
        // No room at all: at the left, nothing extra.
        let none = bar_layout(100.0, 120.0, 110.0);
        assert_eq!(none.start, 100.0);
        assert!(!none.subtitles && !none.speed && !none.open);
    }
}
