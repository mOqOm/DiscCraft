//! モニターの下の操作部（FilmCraft のプログラムモニターと同じ 3 段。ROADMAP M9.12）。
//!
//! FilmCraft（MIT OR Apache-2.0）の `panels/monitor.rs` の `controls_row`、`mini_timeline`、`transport` の
//! 配置と描き方を、DiscCraft のタイムライン（チャプターポイント、再生ヘッド、素材の再生）に移したもの:
//!
//! 1. 左に今の時刻（青、等幅）と拡大率、右に今のチャプターとタイムラインの長さ。
//! 2. 小さなタイムライン（スクラブバー）: 目盛り、チャプターポイントの旗、青い再生ヘッド。押す・ドラッグで動かす。
//! 3. 再生のボタン（中央）: チャプターを追加、先頭へ、前のチャプター、1 フレーム戻す、再生 / 停止、
//!    1 フレーム進める、次のチャプター、末尾へ、ポスターフレームを設定。右端に「ビルド結果を開く」。

use crate::DiscCraftApp;
use crate::craft_timeline::{self, style};
use crate::i18n::{tl, tlf};
use crate::icons::{self, Icon};
use crate::timeline_view::{frame_rate, playhead_seconds, set_playhead_seconds, timecode};

use disccraft_project::TimelineId;
use egui::{Align2, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

/// 1 段目（時刻と拡大率）の高さ。
const ROW1: f32 = 26.0;
/// スクラブバーの高さ。
const SCRUB: f32 = 22.0;
/// 再生のボタンの大きさ（FilmCraft と同じ 28 pt）。
const BUTTON: f32 = 28.0;
/// 操作部全体の高さ。
pub(crate) const CONTROLS_HEIGHT: f32 = 2.0 + ROW1 + 4.0 + SCRUB + 6.0 + BUTTON + 6.0;
/// 左右の余白（FilmCraft と同じ 14 pt）。
const SIDE: f32 = 14.0;

/// 再生のボタンの操作。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Transport {
    AddChapter,
    GoToStart,
    PreviousChapter,
    StepBack,
    PlayStop,
    StepForward,
    NextChapter,
    GoToEnd,
    PosterFrame,
}

impl DiscCraftApp {
    /// タイムラインを映しているモニターの操作部（3 段）。
    pub(crate) fn timeline_controls(&mut self, ui: &mut Ui, id: TimelineId) {
        let Some(timeline) = self.session.project.timeline(id).cloned() else {
            return;
        };
        let ctx = ui.ctx().clone();
        let (num, den) = frame_rate(self.session.project.settings.standard);
        let fps = f64::from(num) / f64::from(den.max(1));
        let step = 1.0 / fps.max(1.0);
        let now = playhead_seconds(&ctx, id);
        let end = self.asset_seconds(timeline.video).unwrap_or(now).max(step);
        let points: Vec<f64> = timeline
            .chapters
            .iter()
            .map(|chapter| crate::timeline_view::ticks_to_seconds(chapter.time.0))
            .collect();
        let chapter = crate::monitor::chapter_number(&points, now);

        let width = ui.available_width();
        let (area, _) = ui.allocate_exact_size(vec2(width, CONTROLS_HEIGHT), Sense::hover());

        // 1 段目: 今の時刻（青）と拡大率、右にチャプターと長さ。
        let row1 = Rect::from_min_size(
            pos2(area.min.x + SIDE, area.min.y + 2.0),
            vec2((width - 2.0 * SIDE).max(1.0), ROW1),
        );
        let painter = ui.painter().clone();
        painter.text(
            row1.left_center(),
            Align2::LEFT_CENTER,
            timecode(now, num, den),
            FontId::monospace(15.0),
            style::TIMECODE,
        );
        let zoom_rect =
            Rect::from_min_size(pos2(row1.min.x + 130.0, row1.min.y + 1.0), vec2(96.0, 24.0));
        let mut zoom = self.monitor_zoom;
        ui.scope_builder(egui::UiBuilder::new().max_rect(zoom_rect), |ui| {
            crate::monitor::zoom_combo(ui, "monitor_zoom", &mut zoom);
        });
        self.monitor_zoom = zoom;
        let duration = timecode(end, num, den);
        let right = painter.text(
            row1.right_center(),
            Align2::RIGHT_CENTER,
            duration,
            FontId::monospace(13.0),
            style::TEXT_DIM,
        );
        painter.text(
            pos2(right.left() - 16.0, row1.center().y),
            Align2::RIGHT_CENTER,
            tlf!("Chapter {n}", n = format!("{chapter:02}")),
            FontId::proportional(12.0),
            style::TEXT_DIM,
        );

        // 2 段目: スクラブバー。
        let bar = Rect::from_min_size(
            pos2(row1.min.x, row1.max.y + 4.0),
            vec2(row1.width(), SCRUB),
        );
        if let Some(seconds) = scrub_bar(ui, bar, (now, end, fps), &points) {
            self.playback = None;
            set_playhead_seconds(&ctx, id, seconds);
        }

        // 3 段目: 再生のボタン。
        let row3 = Rect::from_min_size(pos2(area.min.x, bar.max.y + 6.0), vec2(width, BUTTON));
        let playing = self.is_playing();
        let buttons = [
            (
                Icon::Marker,
                Transport::AddChapter,
                tl!("Add Chapter at Playhead"),
            ),
            (Icon::GoToIn, Transport::GoToStart, tl!("Go to Start")),
            (
                Icon::PrevEdit,
                Transport::PreviousChapter,
                tl!("Previous Chapter"),
            ),
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
            (Icon::NextEdit, Transport::NextChapter, tl!("Next Chapter")),
            (Icon::GoToOut, Transport::GoToEnd, tl!("Go to End")),
            (
                Icon::Camera,
                Transport::PosterFrame,
                tl!("Set Poster Frame"),
            ),
        ];
        let gap = 6.0;
        let total = buttons.len() as f32 * (BUTTON + gap) - gap;
        let mut x = row3.center().x - total / 2.0;
        let mut pressed = None;
        for (icon, action, tip) in buttons {
            let rect = Rect::from_min_size(pos2(x, row3.min.y), vec2(BUTTON, BUTTON));
            if transport_button(ui, rect, icon, action == Transport::PlayStop, tip) {
                pressed = Some(action);
            }
            x += BUTTON + gap;
        }
        // 右端: ビルドしたディスクをモニターでプレビューする。
        let open_rect = Rect::from_min_size(
            pos2(row3.max.x - SIDE - 120.0, row3.min.y + 3.0),
            vec2(120.0, 22.0),
        );
        if open_rect.min.x > x + 8.0 {
            let open = ui.put(
                open_rect,
                egui::Button::new(tl!("Open Build Output")).wrap_mode(egui::TextWrapMode::Truncate),
            );
            if open.clicked() {
                self.open_preview();
            }
        }

        let target = match pressed {
            Some(Transport::AddChapter) => {
                let _ = self.run_command(
                    "timeline.addChapter",
                    json!({ "timeline": id.0, "time": crate::timeline_view::seconds_to_ticks(now) }),
                );
                None
            }
            Some(Transport::GoToStart) => Some(0.0),
            Some(Transport::PreviousChapter) => Some(crate::monitor::prev_point(&points, now)),
            Some(Transport::StepBack) => Some(step_frames(now, -1, fps, end)),
            Some(Transport::PlayStop) => {
                self.toggle_playback(&ctx);
                None
            }
            Some(Transport::StepForward) => Some(step_frames(now, 1, fps, end)),
            Some(Transport::NextChapter) => {
                Some(crate::monitor::next_point(&points, now).unwrap_or(end))
            }
            Some(Transport::GoToEnd) => Some(end),
            Some(Transport::PosterFrame) => {
                // 今のチャプターのポスターフレーム（Encore の ポスターフレームを設定）。
                let _ = self.run_command(
                    "timeline.setPosterFrame",
                    json!({
                        "timeline": id.0,
                        "chapter": chapter.saturating_sub(1),
                        "time": crate::timeline_view::seconds_to_ticks(now),
                    }),
                );
                None
            }
            None => None,
        };
        if let Some(seconds) = target {
            // 動かしたら再生は止める。
            self.playback = None;
            set_playhead_seconds(&ctx, id, seconds);
        }
    }

    /// ← / → で、タイムラインビューアのタイムラインを 1 フレームずつ戻す / 進める（押し続けると続けて送る。
    /// 再生中なら止める）。文字の入力中と、モニターでビルドしたディスクをプレビューしている間（矢印は
    /// リモコン）は何もしない。メニュービューアで選んだボタンを矢印で動かしたときは、そちらが先にキーを使う。
    pub(crate) fn step_frames_by_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() || (self.project_preview.is_none() && self.preview.open)
        {
            return;
        }
        let Some(id) = self.viewer_timeline() else {
            return;
        };
        let frames = ctx.input_mut(|input| {
            let mut frames = 0_i32;
            while input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
                frames = frames.saturating_sub(1);
            }
            while input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
                frames = frames.saturating_add(1);
            }
            frames
        });
        if frames == 0 {
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
        self.playback = None;
        set_playhead_seconds(ctx, id, step_frames(now, frames, fps, end));
    }
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

/// FilmCraft の再生のボタン: 押せる範囲、乗ったときの地、アイコン（再生は少し大きく）。押されたか。
fn transport_button(ui: &mut Ui, rect: Rect, icon: Icon, large: bool, tip: &str) -> bool {
    let response = ui
        .interact(
            rect,
            ui.scope_id().with(("monitor_transport", tip)),
            Sense::click(),
        )
        .on_hover_text(tip);
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, style::HOVER);
    }
    let size = if large { 20.0 } else { 18.0 };
    let color = if response.hovered() {
        style::ICON_ACTIVE
    } else {
        style::TEXT_DIM
    };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(size, size)),
        icon,
        color,
    );
    response.clicked()
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
            style::MARKER,
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
        style::PLAYHEAD,
        Stroke::NONE,
    ));
    painter.line_segment(
        [pos2(x, head + 8.0), pos2(x, bar.max.y)],
        Stroke::new(1.0, style::PLAYHEAD),
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
    use super::{snap_to_frame, step_frames};

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
}
