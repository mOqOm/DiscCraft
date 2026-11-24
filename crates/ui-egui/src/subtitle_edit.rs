//! Encore's five subtitle buttons in the Monitor, right of the safe-area button: go to the previous / next subtitle,
//! trim a subtitle's in / out point to the current time, and add a subtitle at the current time, which is then typed
//! on the monitor. A subtitle on the monitor can be double-clicked to change its text. The behaviour follows what
//! Encore CS6 does (docs/encore-subtitles.md):
//!
//! - previous / next go to the start of the nearest subtitle starting before / after the current time;
//! - the trims work only while the playhead is inside a subtitle, and leave it at least 13 frames long; the out
//!   point is the current frame (the subtitle ends before it);
//! - a subtitle is added only where the default two seconds fit before the next subtitle;
//! - while typing, Enter starts a new line; Esc or a click elsewhere ends typing and keeps the text; a
//!   double-clicked subtitle starts with all its text selected.
//!
//! The buttons act on the subtitle track shown in the monitor (the first track when none was picked) and are greyed
//! out when there is no subtitle track or nothing for them to do. Edits go through the engine's `subtitle.addCue` /
//! `subtitle.setCue` (undoable); the first edit copies the subtitle file's cues into the project and leaves the file
//! as it is.

use crate::DiscCraftApp;
use crate::i18n::tl;
use crate::timeline_view::{seconds_to_ticks, set_playhead_seconds, ticks_to_seconds};
use crate::tool_icons::Glyph;
use disccraft_project::{AssetId, TimelineId};
use egui::{Rect, Ui, pos2, vec2};
use serde_json::{Value, json};

/// Ticks per second (`filmcraft-time`).
const TICKS_PER_SECOND: i128 = 254_016_000_000;

/// One of the Monitor's subtitle buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SubtitleButton {
    Previous,
    Next,
    TrimIn,
    TrimOut,
    Add,
}

impl SubtitleButton {
    /// The buttons in Encore's order.
    pub(crate) const ALL: [Self; 5] = [
        Self::Previous,
        Self::Next,
        Self::TrimIn,
        Self::TrimOut,
        Self::Add,
    ];

    fn glyph(self) -> Glyph {
        match self {
            Self::Previous => Glyph::SubtitlePrevious,
            Self::Next => Glyph::SubtitleNext,
            Self::TrimIn => Glyph::SubtitleTrimIn,
            Self::TrimOut => Glyph::SubtitleTrimOut,
            Self::Add => Glyph::SubtitleAdd,
        }
    }

    fn tooltip(self) -> &'static str {
        match self {
            Self::Previous => tl!("Previous Subtitle"),
            Self::Next => tl!("Next Subtitle"),
            Self::TrimIn => tl!("Trim Subtitle In Point to Here"),
            Self::TrimOut => tl!("Trim Subtitle Out Point to Here"),
            Self::Add => tl!("Add Subtitle"),
        }
    }
}

/// What a button does now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SubtitleStep {
    /// Move the playhead to this time (ticks).
    Seek(i64),
    /// Move this subtitle's in point to the current time.
    TrimIn(usize),
    /// Move this subtitle's out point to the current time.
    TrimOut(usize),
    /// Add a subtitle from the current time to this time (ticks).
    Add(i64),
}

/// The shortest subtitle a trim button leaves, in frames at `num` / `den` frames a second. Encore CS6 greys the trims
/// out until more than 0.4 seconds stay: 13 frames in NTSC and 11 in PAL (dragging a subtitle's edge stops one frame
/// shorter, at 12 and 10).
pub(crate) fn min_subtitle_frames(num: u32, den: u32) -> i64 {
    // The nominal rate (30 for 29.97): `as` is exact for a frame rate.
    let nominal = (f64::from(num) / f64::from(den.max(1))).round() as i64;
    nominal.saturating_mul(2) / 5 + 1
}

/// What `button` does at `now` (ticks) on a track's subtitles (`(start, end)` in ticks, sorted by start), or `None`
/// when it has nothing to do, as Encore CS6 decides it (`frame` is one frame, `added` the length of an added
/// subtitle and `shortest` the shortest subtitle a trim leaves, in ticks):
///
/// - previous / next: the start of the last subtitle starting before now / the first starting after;
/// - trim in / out: only inside a subtitle, and only when at least `shortest` ([`min_subtitle_frames`]) is left of it (the out
///   point is the current frame, so the subtitle ends before it);
/// - add: where no subtitle is shown and `added` fits before the next one.
pub(crate) fn subtitle_step(
    button: SubtitleButton,
    cues: &[(i64, i64)],
    now: i64,
    (frame, added, shortest): (i64, i64, i64),
) -> Option<SubtitleStep> {
    let under = cues
        .iter()
        .enumerate()
        .find(|(_, (start, end))| *start <= now && now < *end)
        .map(|(index, &(start, end))| (index, start, end));
    match button {
        SubtitleButton::Previous => cues
            .iter()
            .map(|&(start, _)| start)
            .filter(|&start| start < now)
            .max()
            .map(SubtitleStep::Seek),
        SubtitleButton::Next => cues
            .iter()
            .map(|&(start, _)| start)
            .filter(|&start| start > now)
            .min()
            .map(SubtitleStep::Seek),
        SubtitleButton::TrimIn => under
            // Less than a frame after its start (a subtitle file's times are not on frames) is its start.
            .filter(|&(_, start, end)| {
                now.saturating_sub(start) >= frame && end.saturating_sub(now) >= shortest
            })
            .map(|(index, _, _)| SubtitleStep::TrimIn(index)),
        SubtitleButton::TrimOut => under
            .filter(|&(_, start, _)| now.saturating_sub(start) >= shortest)
            .map(|(index, _, _)| SubtitleStep::TrimOut(index)),
        SubtitleButton::Add => {
            let end = now.saturating_add(added);
            let fits = cues
                .iter()
                .map(|&(start, _)| start)
                .filter(|&start| start > now)
                .min()
                .is_none_or(|next| next >= end);
            (under.is_none() && fits).then_some(SubtitleStep::Add(end))
        }
    }
}

/// `seconds` on the frame grid (`num` / `den` frames a second), in ticks.
pub(crate) fn frame_ticks(seconds: f64, num: u32, den: u32) -> i64 {
    if !seconds.is_finite() || num == 0 || den == 0 {
        return 0;
    }
    // `as` saturates; a playhead is far inside the range.
    let frames = (seconds * f64::from(num) / f64::from(den)).round() as i128;
    let ticks = frames.saturating_mul(TICKS_PER_SECOND * i128::from(den)) / i128::from(num);
    i64::try_from(ticks).unwrap_or(0)
}

/// A subtitle being typed on the monitor.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SubtitleEdit {
    pub(crate) timeline: TimelineId,
    pub(crate) asset: AssetId,
    pub(crate) cue: usize,
    pub(crate) text: String,
    /// The text before typing (Esc goes back to it; an unchanged text is not sent).
    pub(crate) original: String,
    /// Give the text box the keyboard on its first frame.
    pub(crate) focus: bool,
    /// Start with all the text selected (a double-clicked subtitle, as in Encore).
    pub(crate) select_all: bool,
}

impl DiscCraftApp {
    /// The subtitle track the Monitor's subtitle buttons act on: the one shown in the monitor, else the first.
    pub(crate) fn subtitle_target(&self, timeline: TimelineId) -> Option<AssetId> {
        let tracks = &self.session.project.timeline(timeline)?.subtitles;
        let track = self.shown_subtitle(timeline).unwrap_or(0);
        tracks.get(track).map(|track| track.asset)
    }

    /// A subtitle asset's cues as `(start, end)` ticks.
    fn subtitle_spans(&mut self, asset: AssetId) -> Vec<(i64, i64)> {
        self.subtitle_cues(asset)
            .iter()
            .map(|cue| (seconds_to_ticks(cue.start), seconds_to_ticks(cue.end)))
            .collect()
    }

    /// The five subtitle buttons from `at` (left edge, 24-point squares 2 apart) for timeline `id` at `now`
    /// (seconds) on a `num` / `den` frame grid.
    pub(crate) fn subtitle_buttons(
        &mut self,
        ui: &mut Ui,
        at: egui::Pos2,
        id: TimelineId,
        now: f64,
        (num, den): (u32, u32),
    ) {
        let target = self.subtitle_target(id);
        let cues = target
            .map(|asset| self.subtitle_spans(asset))
            .unwrap_or_default();
        let now_ticks = frame_ticks(now, num, den);
        // One frame, and Encore's two seconds on the frame grid (60 frames in NTSC, 50 in PAL).
        let frame = frame_ticks(f64::from(den) / f64::from(num.max(1)), num, den).max(1);
        // `as` is exact for a second's count of frames.
        // The preference's length (Encore: Subtitle Duration), two seconds unless changed.
        let added_frames = (f64::from(self.timeline_prefs.subtitle_seconds.max(1)) * f64::from(num)
            / f64::from(den.max(1)))
        .round() as i64;
        let added = frame.saturating_mul(added_frames);
        let shortest = frame.saturating_mul(min_subtitle_frames(num, den));
        let mut pressed = None;
        for (index, button) in SubtitleButton::ALL.into_iter().enumerate() {
            // `as` is exact for five buttons.
            let rect =
                Rect::from_min_size(pos2(at.x + index as f32 * 26.0, at.y), vec2(24.0, 24.0));
            let step = target
                .and_then(|_| subtitle_step(button, &cues, now_ticks, (frame, added, shortest)));
            if subtitle_button(ui, rect, button, step.is_some())
                && let (Some(asset), Some(step)) = (target, step)
            {
                pressed = Some((asset, step));
            }
        }
        if let Some((asset, step)) = pressed {
            self.run_subtitle_step(ui.ctx(), id, asset, step, now_ticks);
        }
    }

    fn run_subtitle_step(
        &mut self,
        ctx: &egui::Context,
        id: TimelineId,
        asset: AssetId,
        step: SubtitleStep,
        now: i64,
    ) {
        match step {
            SubtitleStep::Seek(time) => {
                self.playback = None;
                set_playhead_seconds(ctx, id, ticks_to_seconds(time));
            }
            SubtitleStep::TrimIn(cue) => {
                let _ = self.run_command(
                    "subtitle.setCue",
                    json!({ "asset": asset.0, "cue": cue, "start": now }),
                );
            }
            SubtitleStep::TrimOut(cue) => {
                let _ = self.run_command(
                    "subtitle.setCue",
                    json!({ "asset": asset.0, "cue": cue, "end": now }),
                );
            }
            SubtitleStep::Add(end) => {
                self.playback = None;
                let added = self.run_command(
                    "subtitle.addCue",
                    json!({ "asset": asset.0, "start": now, "end": end }),
                );
                let cue = added
                    .as_ref()
                    .and_then(|value| value.get("cue"))
                    .and_then(Value::as_u64)
                    .and_then(|cue| usize::try_from(cue).ok());
                if let Some(cue) = cue {
                    // Show the new subtitle while it is typed.
                    set_playhead_seconds(ctx, id, ticks_to_seconds(now));
                    self.subtitle_edit = Some(SubtitleEdit {
                        timeline: id,
                        asset,
                        cue,
                        text: String::new(),
                        original: String::new(),
                        focus: true,
                        select_all: false,
                    });
                }
            }
        }
    }

    /// Starts typing the subtitle shown at `seconds` on timeline `id` (a double-click on it in the monitor).
    pub(crate) fn edit_shown_subtitle(&mut self, id: TimelineId, seconds: f64) {
        let Some(asset) = self.subtitle_target(id) else {
            return;
        };
        let cues = self.subtitle_cues(asset);
        let Some((cue, text)) = cues
            .iter()
            .enumerate()
            .find(|(_, cue)| cue.start <= seconds && seconds < cue.end)
            .map(|(index, cue)| (index, cue.text.clone()))
        else {
            return;
        };
        self.playback = None;
        self.subtitle_edit = Some(SubtitleEdit {
            timeline: id,
            asset,
            cue,
            original: text.clone(),
            text,
            focus: true,
            select_all: true,
        });
    }

    /// The text box for the subtitle being typed, where the subtitle shows on `picture`. As in Encore, Enter starts
    /// a new line, and Esc or a click elsewhere ends typing and keeps the text.
    pub(crate) fn subtitle_edit_box(&mut self, ui: &mut Ui, picture: Rect) {
        let Some(edit) = self.subtitle_edit.as_mut() else {
            return;
        };
        let size = (picture.height() / 18.0).clamp(10.0, 40.0);
        // `as` is exact for the few lines a subtitle holds.
        let lines = edit.text.split('\n').count().clamp(1, 4) as f32;
        let height = size * 1.35 * lines + 10.0;
        let width = (picture.width() * 0.8).max(120.0);
        let bottom = picture.max.y - picture.height() * 0.06;
        let rect = Rect::from_min_max(
            pos2(picture.center().x - width / 2.0, bottom - height),
            pos2(picture.center().x + width / 2.0, bottom),
        );
        let id = ui.scope_id().with("monitor_subtitle_edit");
        let response = ui.put(
            rect,
            egui::TextEdit::multiline(&mut edit.text)
                .id(id)
                .font(egui::FontId::proportional(size))
                .align(egui::Align2::CENTER_TOP)
                .desired_rows(1)
                .hint_text(tl!("Type the subtitle")),
        );
        if edit.focus {
            response.request_focus();
            edit.focus = false;
            if edit.select_all {
                edit.select_all = false;
                let end = edit.text.chars().count();
                let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(end),
                    )));
                state.store(ui.ctx(), id);
            }
            return;
        }
        if response.lost_focus() {
            self.finish_subtitle_edit();
        }
    }

    /// Keeps the typed text (when it changed) and stops typing.
    fn finish_subtitle_edit(&mut self) {
        let Some(edit) = self.subtitle_edit.take() else {
            return;
        };
        if edit.text != edit.original {
            let _ = self.run_command(
                "subtitle.setCue",
                json!({ "asset": edit.asset.0, "cue": edit.cue, "text": edit.text }),
            );
        }
    }

    /// Stops typing a subtitle of another timeline than the one in the monitor (its box is not shown there).
    pub(crate) fn drop_stale_subtitle_edit(&mut self, shown: Option<TimelineId>) {
        if self
            .subtitle_edit
            .as_ref()
            .is_some_and(|edit| Some(edit.timeline) != shown)
        {
            self.finish_subtitle_edit();
        }
    }
}

/// One subtitle button: its glyph (dimmed when it has nothing to do) and Encore's tooltip. True when clicked.
fn subtitle_button(ui: &mut Ui, rect: Rect, button: SubtitleButton, enabled: bool) -> bool {
    let response = ui
        .interact(
            rect,
            ui.scope_id()
                .with(("monitor_subtitle_button", button as u8)),
            if enabled {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            },
        )
        .on_hover_text(button.tooltip());
    // Encore's bar (M6.476): a raised box and a gold icon under the pointer.
    let hovered = enabled && response.hovered();
    if hovered {
        crate::theme::hover_box(ui.painter(), rect.shrink2(vec2(0.0, 3.0)));
    }
    let color = if hovered {
        crate::theme::HOVER_ICON.get()
    } else if enabled {
        crate::theme::BAR_ICON.get()
    } else {
        ui.visuals().weak_text_color().gamma_multiply(0.6)
    };
    crate::tool_icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(16.0, 16.0)),
        button.glyph(),
        color,
    );
    enabled && response.clicked()
}

#[cfg(test)]
mod tests {
    use super::{SubtitleButton, SubtitleStep, frame_ticks, min_subtitle_frames, subtitle_step};

    const S: i64 = 254_016_000_000;

    /// As Encore CS6 does it: previous / next go to subtitle starts; the trims work only inside a subtitle and leave
    /// 13 frames; adding needs a time with no subtitle and two seconds before the next one.
    #[test]
    fn each_button_knows_what_it_does_now() {
        use SubtitleButton::{Next, Previous, TrimIn, TrimOut};
        // NTSC frames: subtitles at frames 0..60 and 150..210, as measured in Encore.
        let f = 8_475_667_200_i64;
        let cues = [(0, 60 * f), (150 * f, 210 * f)];
        let at = |button, frame: i64| subtitle_step(button, &cues, frame * f, (f, 60 * f, 13 * f));

        assert_eq!(at(Previous, 105), Some(SubtitleStep::Seek(0)));
        assert_eq!(at(Previous, 150), Some(SubtitleStep::Seek(0)));
        assert_eq!(at(Previous, 0), None);
        assert_eq!(at(Next, 0), Some(SubtitleStep::Seek(150 * f)));
        assert_eq!(at(Next, 30), Some(SubtitleStep::Seek(150 * f)));
        assert_eq!(at(Next, 150), None);

        // Inside the first subtitle: the in point up to frame 47 (13 frames stay), the out point from frame 13.
        assert_eq!(at(TrimIn, 30), Some(SubtitleStep::TrimIn(0)));
        assert_eq!(at(TrimIn, 47), Some(SubtitleStep::TrimIn(0)));
        assert_eq!(at(TrimIn, 48), None);
        assert_eq!(at(TrimIn, 0), None);
        assert_eq!(at(TrimOut, 12), None);
        assert_eq!(at(TrimOut, 13), Some(SubtitleStep::TrimOut(0)));
        assert_eq!(at(TrimOut, 59), Some(SubtitleStep::TrimOut(0)));
        // Between subtitles and after the last nothing trims.
        for frame in [60, 105, 300] {
            assert_eq!(at(TrimIn, frame), None);
            assert_eq!(at(TrimOut, frame), None);
        }
    }

    /// Adding, as Encore CS6 does it: not on a subtitle, and two seconds must fit before the next one.
    #[test]
    fn a_subtitle_is_added_where_two_seconds_fit() {
        use SubtitleButton::{Add, Previous};
        let f = 8_475_667_200_i64;
        let cues = [(0, 60 * f), (150 * f, 210 * f)];
        let at = |button, frame: i64| subtitle_step(button, &cues, frame * f, (f, 60 * f, 13 * f));

        // Frame 90 fits before the subtitle at 150, 91 does not.
        assert_eq!(at(Add, 30), None);
        assert_eq!(at(Add, 150), None);
        assert_eq!(at(Add, 60), Some(SubtitleStep::Add(120 * f)));
        assert_eq!(at(Add, 90), Some(SubtitleStep::Add(150 * f)));
        assert_eq!(at(Add, 91), None);
        assert_eq!(at(Add, 105), None);
        assert_eq!(at(Add, 210), Some(SubtitleStep::Add(270 * f)));
        assert_eq!(
            subtitle_step(Add, &[], 0, (f, 60 * f, 13 * f)),
            Some(SubtitleStep::Add(60 * f))
        );
        assert_eq!(subtitle_step(Previous, &[], f, (f, 60 * f, 13 * f)), None);
    }

    /// The trims leave more than 0.4 seconds, as measured in Encore CS6: 13 frames in NTSC, 11 in PAL.
    #[test]
    fn the_shortest_subtitle_follows_the_frame_rate() {
        use SubtitleButton::{TrimIn, TrimOut};
        assert_eq!(min_subtitle_frames(30_000, 1001), 13);
        assert_eq!(min_subtitle_frames(25, 1), 11);
        assert_eq!(min_subtitle_frames(24_000, 1001), 10);
        assert_eq!(min_subtitle_frames(0, 0), 1);
        // PAL: a subtitle at frames 50..100 (2:00 to 4:00); the out point trims from 2:11, not at 2:10.
        let f = S / 25;
        let cues = [(50 * f, 100 * f)];
        let at = |button, frame: i64| {
            subtitle_step(
                button,
                &cues,
                frame * f,
                (f, 50 * f, min_subtitle_frames(25, 1) * f),
            )
        };
        assert_eq!(at(TrimOut, 60), None);
        assert_eq!(at(TrimOut, 61), Some(SubtitleStep::TrimOut(0)));
        assert_eq!(at(TrimIn, 89), Some(SubtitleStep::TrimIn(0)));
        assert_eq!(at(TrimIn, 90), None);
    }

    /// The current time goes to the frame grid exactly (NTSC and PAL).
    #[test]
    fn the_playhead_lands_on_a_frame() {
        assert_eq!(frame_ticks(1.0, 25, 1), S);
        assert_eq!(frame_ticks(1.02, 25, 1), S + S / 25);
        // NTSC frame 30 is 1.001 s.
        assert_eq!(frame_ticks(1.0, 30_000, 1001), 30 * 8_475_667_200);
        assert_eq!(frame_ticks(f64::NAN, 25, 1), 0);
        assert_eq!(frame_ticks(1.0, 0, 1), 0);
    }
}
