//! The Craft apps' Timeline pieces (ROADMAP M9.8), ported from FilmCraft (https://github.com/storytold/filmcraft at
//! rev `19d46c0748cf5c0ef5b0b2a421b85e8ae6d99db6`, MIT OR Apache-2.0; see NOTICE):
//! - the ruler's strides and the marker flag (`crates/ui-egui/src/panels/timeline/ruler.rs`, `src/widgets.rs`),
//! - the view's zoom and scroll, eased, kept inside the timeline, following the playhead
//!   (`panels/timeline/layout.rs`, `panels/timeline.rs` `animate_view`),
//! - the zoom scroll bar with its two round handles (`panels/timeline/scrollbars.rs`, `widgets.rs` `ZoomThumbPart`),
//! - the wheel (`panels/timeline/interact.rs` `wheel_input`, `wheel_scroll`),
//! - the playhead's head on the tick row (`panels/timeline.rs`),
//! - the colours of FilmCraft's Medium theme (`src/theme.rs`), the one nearest DiscCraft's greys.
//!
//! What differs: FilmCraft's Timeline reaches ten minutes past its sequence (an edit grows); a disc title is its
//! clip, so here it reaches a little past the clip. The app state FilmCraft's functions read is passed in.

use egui::{Color32, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

/// The Timeline's colours: FilmCraft's Medium theme (Premiere's brightness slider at its middle).
pub(crate) mod style {
    use egui::Color32;

    /// Background of the tracks, the ruler and the headers.
    pub(crate) const TL_BG: Color32 = Color32::from_rgb(0x32, 0x32, 0x32);
    /// The ruler's ticks.
    pub(crate) const RULER_TICK: Color32 = Color32::from_rgb(0x8d, 0x8d, 0x8d);
    /// The ruler's timecodes.
    pub(crate) const RULER_TEXT: Color32 = Color32::from_rgb(0xb0, 0xb0, 0xb0);
    /// The playhead.
    pub(crate) const PLAYHEAD: Color32 = Color32::from_rgb(0x58, 0x95, 0xec);
    /// The big current-time timecode.
    pub(crate) const TIMECODE: Color32 = Color32::from_rgb(0x40, 0x96, 0xf3);
    /// A selected clip's border.
    pub(crate) const SELECTED_BORDER: Color32 = Color32::from_rgb(0xeb, 0xeb, 0xeb);
    /// The scroll bar's track and its thumb.
    pub(crate) const SEPARATOR: Color32 = Color32::from_rgb(0x30, 0x30, 0x30);
    pub(crate) const SCROLL_THUMB: Color32 = Color32::from_gray(0x4b);
    pub(crate) const SCROLL_THUMB_HOVER: Color32 = Color32::from_gray(0x6a);
    /// Text and icons.
    pub(crate) const TEXT: Color32 = Color32::from_rgb(0xd1, 0xd1, 0xd1);
    pub(crate) const TEXT_DIM: Color32 = Color32::from_rgb(0xb0, 0xb0, 0xb0);
    pub(crate) const ICON_ACTIVE: Color32 = Color32::from_rgb(0xd1, 0xd1, 0xd1);
    /// A hovered toolbar button.
    pub(crate) const HOVER: Color32 = Color32::from_rgb(64, 64, 64);
    /// FilmCraft's default marker colour (its label colour "Green").
    pub(crate) const MARKER: Color32 = Color32::from_rgb(0x3d, 0xa8, 0x4b);
    /// A track header's badge (Premiere's track targeting blue).
    pub(crate) const TRACK_TARGET: Color32 = Color32::from_rgb(0x26, 0x5b, 0xc1);
    /// Clip colours: Premiere's default labels for video (Iris) and audio (Caribbean) media.
    pub(crate) const CLIP_VIDEO: Color32 = Color32::from_rgb(0x1d, 0x4a, 0x64);
    pub(crate) const CLIP_AUDIO: Color32 = Color32::from_rgb(0x35, 0x54, 0x18);
    /// A clip's name.
    pub(crate) const CLIP_NAME: Color32 = Color32::from_rgb(0xd9, 0xd9, 0xd9);
    /// A caption (subtitle) block, and its track's lane.
    pub(crate) const CAPTION: Color32 = Color32::from_rgb(0x6b, 0x4f, 0x8f);
    pub(crate) const CAPTION_LANE: Color32 = Color32::from_rgb(0x23, 0x20, 0x2a);
}

/// Height of a clip's name band (FilmCraft's `draw_clip`).
pub(crate) const NAME_BAND: f32 = 16.0;

/// `c` lightened toward white by `f` (FilmCraft's `lighten`).
fn lighten(c: Color32, f: f32) -> Color32 {
    // `as` saturates; the value is 0..=255.
    let l = |v: u8| (f32::from(v) + (255.0 - f32::from(v)) * f).round() as u8;
    Color32::from_rgb(l(c.r()), l(c.g()), l(c.b()))
}

/// The waveform's colour on a clip: half the clip's colour (FilmCraft's `draw_clip`).
pub(crate) fn darker(c: Color32) -> Color32 {
    Color32::from_rgb(c.r() / 2, c.g() / 2, c.b() / 2)
}

/// The height of a clip's waveform for a clip body `body_height` tall: the lower part of a tall clip,
/// under the name strip; the whole clip on a compact track (under 50 points), where a share of it
/// would be too small to read and the name is drawn over the waveform.
pub(crate) fn waveform_zone_height(body_height: f32) -> f32 {
    if body_height < 50.0 {
        (body_height - 2.0).max(8.0)
    } else {
        (body_height * 0.42).max(8.0)
    }
}

/// Display gain for a waveform: normalisation of the source to full scale so quiet recordings stay readable, but
/// by at most +12 dB, so a near-silent track does not look like a loud one.
pub(crate) fn waveform_display_gain(source_peak: f32) -> f32 {
    const MAX_BOOST: f32 = 3.981_072; // +12 dB
    (1.0 / source_peak.max(1e-9)).min(MAX_BOOST)
}

/// Draws a waveform at the foot of a clip `body` (FilmCraft's `draw_waveform`): per point column, the loudest peak
/// of the time it covers (`seconds_at(x)` is the time at `x`), on the logarithmic scale of Premiere's dynamic
/// waveforms (−48 dB empty, 0 dB full).
pub(crate) fn draw_waveform(
    painter: &egui::Painter,
    body: Rect,
    peaks: &crate::waveforms::Peaks,
    seconds_at: impl Fn(f32) -> f64,
    col: Color32,
) {
    use disccraft_engine::waveform::{PEAK_RATE, SAMPLES_PER_PEAK};
    let zone_h = waveform_zone_height(body.height());
    let area = Rect::from_min_max(
        pos2(body.min.x + 1.0, body.max.y - zone_h - 1.0),
        pos2(body.max.x - 1.0, body.max.y - 1.0),
    );
    let clip = painter.clip_rect().intersect(area);
    if clip.width() <= 0.0 {
        return;
    }
    let gain = waveform_display_gain(peaks.max);
    let per_second = f64::from(PEAK_RATE) / SAMPLES_PER_PEAK as f64;
    let index = |seconds: f64| -> usize {
        // `as` saturates; a negative time is the first peak.
        (seconds.max(0.0) * per_second) as usize
    };
    let mut mesh = egui::Mesh::default();
    // `as` saturates; the columns are on screen.
    for x in (clip.min.x.floor() as i32)..(clip.max.x.ceil() as i32) {
        let x = x as f32;
        let s0 = index(seconds_at(x));
        let s1 = index(seconds_at(x + 1.0)).max(s0 + 1);
        let m = peaks
            .peaks
            .iter()
            .skip(s0)
            .take(s1 - s0)
            .fold(0f32, |m, (a, b)| m.max(a.abs()).max(b.abs()));
        let db = 20.0 * (m * gain).max(1e-5).log10();
        let h = ((db + 48.0) / 48.0).clamp(0.0, 1.0) * area.height();
        if h > 0.3 {
            mesh.add_colored_rect(
                Rect::from_min_max(pos2(x, area.max.y - h), pos2(x + 1.0, area.max.y)),
                col,
            );
        }
    }
    painter.add(mesh);
}

/// A clip's body as FilmCraft draws it: the label colour with a 1 pt lighter top edge.
pub(crate) fn clip_body(painter: &egui::Painter, body: Rect, fill: Color32) {
    painter.rect_filled(body, 0.0, fill);
    painter.line_segment(
        [
            pos2(body.min.x, body.min.y + 0.5),
            pos2(body.max.x, body.min.y + 0.5),
        ],
        Stroke::new(1.0, lighten(fill, 0.15)),
    );
}

/// A clip's name in its band, the corner marks where it reaches its media's first (`head`) and last (`tail`)
/// frame, and the 1 pt black separator at its left edge (FilmCraft's `draw_clip`).
pub(crate) fn clip_name_and_edges(
    painter: &egui::Painter,
    body: Rect,
    name: &str,
    head: bool,
    tail: bool,
) {
    let w = body.width();
    let clip = painter.with_clip_rect(body.shrink2(vec2(1.0, 0.0)).intersect(painter.clip_rect()));
    if w > 24.0 && !name.is_empty() {
        clip.text(
            pos2(body.min.x + 6.0, body.min.y + 8.5),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(11.5),
            style::CLIP_NAME,
        );
    }
    let tri = 5.0;
    if head && w > 12.0 {
        clip.add(egui::Shape::convex_polygon(
            vec![
                body.min,
                body.min + vec2(tri, 0.0),
                body.min + vec2(0.0, tri),
            ],
            Color32::from_white_alpha(220),
            Stroke::NONE,
        ));
    }
    if tail && w > 12.0 {
        let tr = pos2(body.max.x, body.min.y);
        clip.add(egui::Shape::convex_polygon(
            vec![tr, tr + vec2(0.0, tri), tr - vec2(tri, 0.0)],
            Color32::from_white_alpha(220),
            Stroke::NONE,
        ));
    }
    painter.line_segment(
        [
            pos2(body.min.x + 0.5, body.min.y),
            pos2(body.min.x + 0.5, body.max.y),
        ],
        Stroke::new(1.0, Color32::BLACK),
    );
}

/// The Ruler's two strides, in frames: `(tick, label)`.
///
/// `label` is the first ladder step wide enough to keep timestamps apart (110 pt); `tick` is the
/// finest divisor of `label` that still leaves ticks 12 pt apart. Deriving the tick stride as a
/// divisor puts every label on a tick and keeps the ticks between labels evenly spaced.
pub(crate) fn ruler_steps(frame_px: f64, base: i64) -> (i64, i64) {
    // `FrameRate` is plain serde data, so a step past `base` can overflow; saturate instead.
    let big = |k: i64| base.saturating_mul(k);
    let steps = [
        1,
        2,
        5,
        10,
        base / 2,
        base,
        big(2),
        big(5),
        big(10),
        big(15),
        big(30),
        big(60),
        big(120),
        big(300),
        big(600),
        big(1800),
        big(3600),
    ];
    let label_step = (*steps
        .iter()
        .find(|s| **s as f64 * frame_px >= 110.0)
        .unwrap_or(&big(3600)))
    .max(1);
    // Labels are at least 110 pt apart, so a divisor no larger than 9 keeps ticks >= 12 pt.
    let minor = (1..=9)
        .rev()
        .filter(|k| label_step.rem_euclid(*k) == 0)
        .map(|k| label_step / k)
        .find(|m| *m as f64 * frame_px >= 12.0)
        .unwrap_or(label_step)
        .max(1);
    (minor, label_step)
}

#[derive(Clone, Copy)]
pub(crate) enum MarkerPart {
    Whole,
}

/// Point markers use the whole flag.
pub(crate) fn marker_glyph(
    painter: &egui::Painter,
    rect: Rect,
    tip_height: f32,
    part: MarkerPart,
    color: Color32,
) {
    let x = rect.center().x;
    let y = rect.top();
    let shoulder = rect.bottom() - tip_height;
    let points = match part {
        MarkerPart::Whole => vec![
            pos2(rect.left(), y),
            pos2(rect.right(), y),
            pos2(rect.right(), shoulder),
            pos2(x, rect.bottom()),
            pos2(rect.left(), shoulder),
        ],
    };
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

/// The playhead's short head on the tick row (as in Effect Controls, so it leaves the markers and timecodes clear)
/// and its line down through the tracks, at `px`.
pub(crate) fn draw_playhead(painter: &egui::Painter, px: f32, ruler: Rect, bottom: f32) {
    let (top, tip) = (ruler.max.y - 13.0, ruler.max.y);
    let head = [
        pos2(px - 5.0, top),
        pos2(px + 5.0, top),
        pos2(px + 5.0, tip - 5.0),
        pos2(px, tip),
        pos2(px - 5.0, tip - 5.0),
    ];
    painter.add(egui::Shape::convex_polygon(
        head.to_vec(),
        style::PLAYHEAD,
        Stroke::NONE,
    ));
    // a path, not a line segment: egui rounds line segments to whole pixels, so while playing
    // the line would move in uneven pixel steps and part from its head
    painter.add(egui::Shape::line(
        vec![pos2(px, tip), pos2(px, bottom)],
        Stroke::new(1.0, style::PLAYHEAD),
    ));
}

/// How far the Timeline reaches, in seconds: a little past its clip (FilmCraft's reaches ten minutes past an edit,
/// which can grow; a title's clip does not), enough that fitting the clip (94 % of the width) is inside it.
pub(crate) fn timeline_extent(clip_seconds: f64) -> f64 {
    let clip = if clip_seconds.is_finite() {
        clip_seconds.max(0.0)
    } else {
        0.0
    };
    clip + (clip * 0.1).max(2.0)
}

/// The least `pps` (points a second) the Timeline zooms out to: its whole extent fits a view
/// `width` points wide, and no farther. A zero or invalid width gives a tiny positive floor, never zero.
pub(crate) fn min_pps(clip_seconds: f64, width: f32) -> f64 {
    (f64::from(width.max(0.0)) / timeline_extent(clip_seconds)).clamp(1e-6, 24_000.0)
}

/// The latest time the left edge of a view `width` points wide can show at `pps` points a second.
pub(crate) fn max_scroll(clip_seconds: f64, width: f32, pps: f64) -> f64 {
    if pps <= 0.0 || !pps.is_finite() {
        return 0.0;
    }
    (timeline_extent(clip_seconds) - f64::from(width) / pps).max(0.0)
}

/// Where the left edge goes so the playhead at `ph` seconds shows in a view `width` points wide
/// (the page-scroll margin from the left), or `None` when it is in view already.
pub(crate) fn scroll_to_reveal(ph: f64, scroll: f64, pps: f64, width: f64) -> Option<f64> {
    if !(ph.is_finite() && scroll.is_finite() && pps > 0.0 && pps.is_finite()) {
        return None;
    }
    let px = (ph - scroll) * pps;
    (px < 0.0 || px > width).then(|| (ph - width * 0.05 / pps).max(0.0))
}

/// A Timeline view's zoom and scroll (FilmCraft's `TimelineView`): what is shown, and where it is easing to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct View {
    /// Points a second, shown and target.
    pub(crate) pps: f64,
    pub(crate) target_pps: f64,
    /// The time at the left edge (seconds), shown and target.
    pub(crate) scroll: f64,
    pub(crate) target_scroll: f64,
    /// Fit the clip to the width on the next frame (a new view, View ▸ Fit).
    pub(crate) fit_pending: bool,
    /// A zoom about a time keeps it under the same x: (time, x).
    pub(crate) zoom_anchor: Option<(f64, f32)>,
    /// The playhead the last frame saw (a jump while stopped brings it into view).
    pub(crate) seen_playhead: Option<f64>,
    /// The canvas's left edge and width the last frame (View ▸ Zoom In / Out zooms about the playhead there).
    pub(crate) left: f32,
    pub(crate) width: f32,
}

impl Default for View {
    fn default() -> Self {
        Self {
            pps: 50.0,
            target_pps: 50.0,
            scroll: 0.0,
            target_scroll: 0.0,
            fit_pending: true,
            zoom_anchor: None,
            seen_playhead: None,
            left: 0.0,
            width: 0.0,
        }
    }
}

impl View {
    /// Eases zoom and scroll toward their targets, fits a new view, follows the playhead while playing, brings it
    /// into view after a jump while stopped, and keeps the view inside the Timeline's extent (FilmCraft's
    /// `animate_view`). `content` is the tracks' rect; returns whether it is still moving (repaint).
    pub(crate) fn animate(
        &mut self,
        dt: f64,
        content: Rect,
        clip_seconds: f64,
        playhead: f64,
        playing: bool,
    ) -> bool {
        let width = content.width();
        self.left = content.min.x;
        self.width = width;
        let dur = clip_seconds.max(1.0);
        if self.fit_pending && width > 50.0 {
            self.target_pps =
                (f64::from(width) * 0.94 / dur).clamp(min_pps(clip_seconds, width), 24_000.0);
            self.target_scroll = 0.0;
            self.fit_pending = false;
        }
        // however the zoom was set (wheel, keys, the bar), zooming out stops at the extent
        let floor = min_pps(clip_seconds, width);
        self.target_pps = self.target_pps.max(floor);
        self.pps = self.pps.max(floor);
        let k = 1.0 - (-dt.min(0.05) * 20.0).exp();
        let zooming = (self.pps - self.target_pps).abs() / self.target_pps > 0.001;
        self.pps += (self.target_pps - self.pps) * k;
        if let Some((anchor_t, anchor_x)) = self.zoom_anchor.filter(|_| zooming) {
            self.scroll = (anchor_t - f64::from(anchor_x - content.min.x) / self.pps).max(0.0);
            self.target_scroll =
                (anchor_t - f64::from(anchor_x - content.min.x) / self.target_pps).max(0.0);
        } else {
            self.scroll += (self.target_scroll - self.scroll) * k;
            self.zoom_anchor = None;
        }
        let moving = (self.pps - self.target_pps).abs() > 1e-6
            || (self.scroll - self.target_scroll).abs() > 1e-6;
        if (self.pps - self.target_pps).abs() / self.target_pps < 0.0005 {
            self.pps = self.target_pps;
        }
        if (self.scroll - self.target_scroll).abs() * self.pps < 0.05 {
            self.scroll = self.target_scroll;
        }
        // follow the playhead while playing (page scrolling)
        let w = f64::from(width);
        if playing {
            let px = (playhead - self.scroll) * self.pps;
            if px > w * 0.95 || px < 0.0 {
                self.target_scroll = (playhead - w * 0.05 / self.pps).max(0.0);
                self.scroll = self.target_scroll;
            }
        }
        // a jump while stopped brings the playhead into view
        let moved = self.seen_playhead.is_some_and(|seen| seen != playhead);
        self.seen_playhead = Some(playhead);
        if moved
            && !playing
            && let Some(s) = scroll_to_reveal(playhead, self.target_scroll, self.target_pps, w)
        {
            self.target_scroll = s;
            self.scroll = s;
        }
        // the view stays inside the Timeline's extent
        if !self.scroll.is_finite() || !self.target_scroll.is_finite() {
            (self.scroll, self.target_scroll) = (0.0, 0.0);
        }
        self.scroll = self
            .scroll
            .clamp(0.0, max_scroll(clip_seconds, width, self.pps));
        self.target_scroll = self
            .target_scroll
            .clamp(0.0, max_scroll(clip_seconds, width, self.target_pps));
        moving
    }

    /// Zooms by `factor` about the time under `anchor_x` (a wheel or View ▸ Zoom In / Out about the playhead).
    pub(crate) fn zoom_about(
        &mut self,
        factor: f64,
        anchor_t: f64,
        anchor_x: f32,
        width: f32,
        clip_seconds: f64,
    ) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        self.target_pps = (self.target_pps * factor).clamp(min_pps(clip_seconds, width), 24_000.0);
        self.zoom_anchor = Some((anchor_t, anchor_x));
    }
}

/// What the wheel and the trackpad did this frame.
pub(crate) struct WheelInput {
    /// Scroll in points (wheel lines and pages converted), positive = content moves right / down.
    pub(crate) delta: egui::Vec2,
    /// Pinch factor (1 = none).
    pub(crate) pinch: f32,
    pub(crate) command: bool,
    pub(crate) alt: bool,
    pub(crate) shift: bool,
}

/// The wheel events of this frame, read from the events themselves: egui turns Cmd + wheel into a
/// zoom and Shift + wheel into a sideways scroll before anyone asks, and the Timeline has its own
/// meaning for both. The modifiers are those of the events.
pub(crate) fn wheel_input(ctx: &egui::Context) -> WheelInput {
    ctx.input(|i| {
        let mut w = WheelInput {
            delta: egui::Vec2::ZERO,
            pinch: 1.0,
            command: false,
            alt: false,
            shift: false,
        };
        for e in &i.events {
            match e {
                egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } => {
                    let points = match unit {
                        egui::MouseWheelUnit::Point => 1.0,
                        egui::MouseWheelUnit::Line => 40.0,
                        egui::MouseWheelUnit::Page => 400.0,
                    };
                    if delta.x.is_finite() && delta.y.is_finite() {
                        w.delta += *delta * points;
                    }
                    w.command |= modifiers.command || modifiers.mac_cmd || modifiers.ctrl;
                    w.alt |= modifiers.alt;
                    w.shift |= modifiers.shift;
                }
                egui::Event::Zoom(z) if z.is_finite() && *z > 0.0 => w.pinch *= *z,
                _ => {}
            }
        }
        w
    })
}

/// Wheel input over the Timeline (FilmCraft's, as in Premiere Pro): Option / Alt + wheel or a pinch zooms about
/// the pointer, Cmd / Ctrl + wheel, Shift + wheel and a sideways gesture scroll the Timeline sideways; the plain
/// wheel is left to the tracks (up and down). Returns whether the wheel was the Timeline's.
pub(crate) fn wheel_scroll(
    ui: &Ui,
    content: Rect,
    view: &mut View,
    clip_seconds: f64,
    time_at: impl Fn(f32) -> f64,
) -> bool {
    let ctx = ui.ctx().clone();
    let Some(p) = ctx.pointer_hover_pos().filter(|p| content.contains(*p)) else {
        return false;
    };
    let w = wheel_input(&ctx);
    if (w.pinch - 1.0).abs() > 1e-4 || (w.alt && w.delta.y != 0.0) {
        let f = if (w.pinch - 1.0).abs() > 1e-4 {
            f64::from(w.pinch)
        } else {
            (1.0_f64 + f64::from(w.delta.y) * 0.01).clamp(0.5, 2.0)
        };
        view.zoom_about(f, time_at(p.x), p.x, content.width(), clip_seconds);
        return true;
    }
    if w.delta == egui::Vec2::ZERO {
        return false;
    }
    let sideways = if w.delta.x.abs() > w.delta.y.abs() {
        Some(w.delta.x)
    } else if w.command || w.shift {
        Some(w.delta.y)
    } else {
        None
    };
    match sideways {
        Some(d) => {
            let limit = max_scroll(clip_seconds, content.width(), view.pps);
            view.target_scroll = (view.target_scroll - f64::from(d) / view.pps).clamp(0.0, limit);
            view.scroll = view.target_scroll;
            true
        }
        None => false,
    }
}

/// The narrowest the zoom scroll bar's thumb gets: room for its two handles and a part between
/// them to grab.
const MIN_THUMB_W: f32 = 34.0;

/// Where the zoom scroll bar's thumb is: (left edge, width, seconds of scroll per point of thumb
/// travel). The thumb is as wide as the share of the Timeline in view (never narrower than
/// [`MIN_THUMB_W`]) and its travel covers the whole scroll range, so it reaches both ends of the
/// bar and moves with the pointer when dragged.
pub(crate) fn zoom_thumb(bar: Rect, clip_seconds: f64, scroll: f64, pps: f64) -> (f32, f32, f64) {
    let vis = f64::from(bar.width()) / pps.max(1e-9);
    let total = timeline_extent(clip_seconds).max(vis);
    let w = ((vis / total) as f32 * bar.width()).clamp(MIN_THUMB_W.min(bar.width()), bar.width());
    let travel = (bar.width() - w).max(0.0);
    let range = (total - vis).max(0.0);
    let at = if range > 0.0 {
        (scroll / range).clamp(0.0, 1.0) as f32 * travel
    } else {
        0.0
    };
    (
        bar.min.x + at,
        w,
        if travel > 0.5 {
            range / f64::from(travel)
        } else {
            0.0
        },
    )
}

/// What a pointer is over on a zoom scroll bar: the circular handle at either end of the thumb zooms, the middle
/// scrolls, and a press beside the thumb brings it there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ZoomThumbPart {
    Beside,
    Left,
    Middle,
    Right,
}

impl ZoomThumbPart {
    pub(crate) fn at(thumb: Rect, pos: egui::Pos2) -> Self {
        if !thumb.expand2(vec2(0.0, 4.0)).contains(pos) {
            Self::Beside
        } else if (pos.x - thumb.min.x).abs() < 9.0 {
            Self::Left
        } else if (pos.x - thumb.max.x).abs() < 9.0 {
            Self::Right
        } else {
            Self::Middle
        }
    }
}

/// A drag of the zoom scroll bar: where it was grabbed, the scroll and zoom then, and what was grabbed
/// (0 the middle, 1 the left handle, 2 the right one).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BarDrag {
    grab: f32,
    start: f64,
    start_pps: f64,
    mode: u8,
}

/// The zoom scroll bar: the thumb is the part of the Timeline in view; dragging its middle scrolls, dragging a round
/// handle zooms keeping the other end of the view, a press beside it brings it there, Esc puts a drag back.
pub(crate) fn zoom_scrollbar(
    ui: &mut Ui,
    bar: Rect,
    view: &mut View,
    clip_seconds: f64,
    id: egui::Id,
) {
    let painter = ui.painter().clone();
    painter.rect_filled(bar, bar.height() / 2.0, style::SEPARATOR);
    let vis = f64::from(bar.width()) / view.pps;
    let total = timeline_extent(clip_seconds).max(vis);
    let (left, w, per_point) = zoom_thumb(bar, clip_seconds, view.scroll, view.pps);
    let thumb = Rect::from_min_max(pos2(left, bar.min.y), pos2(left + w, bar.max.y));
    let hover = ui.rect_contains_pointer(thumb);
    painter.rect_filled(
        thumb,
        thumb.height() / 2.0,
        if hover {
            style::SCROLL_THUMB_HOVER
        } else {
            style::SCROLL_THUMB
        },
    );
    for x in [
        thumb.min.x + thumb.height() / 2.0,
        thumb.max.x - thumb.height() / 2.0,
    ] {
        painter.circle_filled(pos2(x, thumb.center().y), 4.5, style::TL_BG);
        painter.circle_stroke(
            pos2(x, thumb.center().y),
            4.5,
            Stroke::new(1.5, style::ICON_ACTIVE),
        );
    }
    let resp = ui.interact(bar, id, Sense::click_and_drag());
    let drag_key = id.with("drag");
    let mut drag = ui
        .data(|d| d.get_temp::<Option<BarDrag>>(drag_key))
        .flatten();
    let range = (total - vis).max(0.0);
    // a press beside the thumb brings the thumb's middle under the pointer (and a drag goes on from there)
    let jump = |view: &mut View, x: f32| {
        if per_point > 0.0 {
            view.target_scroll = (f64::from(x - bar.min.x - w / 2.0) * per_point).clamp(0.0, range);
            view.scroll = view.target_scroll;
        }
    };
    if resp.clicked()
        && let Some(pos) = resp
            .interact_pointer_pos()
            .filter(|pos| !thumb.contains(*pos))
    {
        jump(view, pos.x);
    }
    // Where the button went down on the bar, remembered here: by the time a drag is known (the pointer has moved
    // past egui's threshold) the pointer may have left the handle it was pressed on.
    let press_key = id.with("press");
    if resp.is_pointer_button_down_on()
        && ui.input(|i| i.pointer.primary_pressed())
        && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
    {
        ui.data_mut(|d| d.insert_temp(press_key, pos));
    }
    if resp.drag_started_by(egui::PointerButton::Primary)
        && let Some(pos) = ui
            .data(|d| d.get_temp::<egui::Pos2>(press_key))
            .or_else(|| ui.input(|i| i.pointer.press_origin()))
            .or(resp.interact_pointer_pos())
    {
        let part = ZoomThumbPart::at(thumb, pos);
        if part == ZoomThumbPart::Beside {
            jump(view, pos.x);
        }
        let mode = match part {
            ZoomThumbPart::Left => 1,
            ZoomThumbPart::Right => 2,
            ZoomThumbPart::Beside | ZoomThumbPart::Middle => 0,
        };
        drag = Some(BarDrag {
            grab: pos.x,
            start: view.target_scroll,
            start_pps: view.target_pps,
            mode,
        });
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape))
        && let Some(d) = drag.take()
    {
        view.target_scroll = d.start;
        view.scroll = d.start;
        view.target_pps = d.start_pps;
        view.pps = d.start_pps;
    }
    if let Some(d) = drag
        && resp.dragged()
    {
        match d.mode {
            // the thumb's middle: scroll, the thumb staying under the pointer
            0 => {
                if let Some(pos) = resp.interact_pointer_pos() {
                    view.target_scroll =
                        (d.start + f64::from(pos.x - d.grab) * per_point).clamp(0.0, range);
                    view.scroll = view.target_scroll;
                }
            }
            // a handle: zoom, the other end of the view staying where it is
            _ => {
                let dx = f64::from(resp.drag_delta().x) / f64::from(bar.width().max(1.0)) * total;
                let floor = min_pps(clip_seconds, bar.width());
                if d.mode == 1 {
                    let end = view.scroll + vis;
                    let ns = (view.scroll + dx).clamp(0.0, (end - 0.05).max(0.0));
                    view.pps =
                        (f64::from(bar.width()) / (end - ns).max(0.05)).clamp(floor, 24_000.0);
                    view.scroll = ns;
                    view.target_scroll = ns;
                } else {
                    let ne = (view.scroll + vis + dx).max(view.scroll + 0.05);
                    view.pps = (f64::from(bar.width()) / (ne - view.scroll)).clamp(floor, 24_000.0);
                }
                view.target_pps = view.pps;
            }
        }
    }
    if resp.drag_stopped() {
        drag = None;
    }
    let handle_grabbed = matches!(drag, Some(BarDrag { mode: 1 | 2, .. }));
    ui.data_mut(|d| d.insert_temp(drag_key, drag));
    zoom_thumb_cursor(ui, &resp, thumb, handle_grabbed);
}

/// The horizontal resize cursor of a zoom scroll bar: over either handle of its `thumb`, and for as
/// long as a drag of a handle goes on (`handle_grabbed`), which the pointer can take off the bar.
/// `resp` is the bar's response.
pub(crate) fn zoom_thumb_cursor(ui: &Ui, resp: &Response, thumb: Rect, handle_grabbed: bool) {
    let on_handle = if resp.dragged() {
        handle_grabbed
    } else {
        resp.hover_pos().is_some_and(|pos| {
            matches!(
                ZoomThumbPart::at(thumb, pos),
                ZoomThumbPart::Left | ZoomThumbPart::Right
            )
        })
    };
    if on_handle {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        View, ZoomThumbPart, max_scroll, min_pps, ruler_steps, scroll_to_reveal, timeline_extent,
        zoom_thumb,
    };
    use egui::{Rect, pos2};

    #[test]
    fn ruler_labels_stay_apart_and_ticks_divide_them() {
        // 29.97 (base 30) at 2 points a frame: a label every second (60 pt is too close, 2 s = 120 pt).
        let (tick, label) = ruler_steps(2.0, 30);
        assert_eq!(label, 60);
        assert_eq!(label % tick, 0);
        assert!(tick as f64 * 2.0 >= 12.0);
        // Zoomed far out: minutes apart, never below one frame.
        let (tick, label) = ruler_steps(0.001, 30);
        assert!(label >= 30 * 60);
        assert!(tick >= 1);
        // Zoomed far in: every frame labelled.
        assert_eq!(ruler_steps(200.0, 25), (1, 1));
    }

    #[test]
    fn the_view_reaches_a_little_past_the_clip() {
        assert!((timeline_extent(20.0) - 22.0).abs() < 1e-9);
        assert!((timeline_extent(3600.0) - 3960.0).abs() < 1e-9);
        assert_eq!(timeline_extent(f64::NAN), 2.0);
        // Zoomed out all the way, the extent fills the width and cannot scroll.
        let pps = min_pps(20.0, 880.0);
        assert!((pps - 40.0).abs() < 1e-9);
        assert_eq!(max_scroll(20.0, 880.0, pps), 0.0);
        assert!(max_scroll(20.0, 880.0, 80.0) > 0.0);
        assert_eq!(max_scroll(20.0, 880.0, 0.0), 0.0);
    }

    #[test]
    fn a_jump_out_of_view_scrolls_to_it() {
        assert_eq!(scroll_to_reveal(5.0, 0.0, 10.0, 100.0), None);
        let s = scroll_to_reveal(50.0, 0.0, 10.0, 100.0).unwrap_or(-1.0);
        assert!((s - 49.5).abs() < 1e-9);
        assert_eq!(scroll_to_reveal(f64::INFINITY, 0.0, 10.0, 100.0), None);
    }

    #[test]
    fn the_thumb_shows_the_part_in_view() {
        let bar = Rect::from_min_max(pos2(0.0, 0.0), pos2(210.0, 10.0));
        // The whole extent (22 s for a 20 s clip) in view: the thumb fills the bar.
        let (left, w, _) = zoom_thumb(bar, 20.0, 0.0, 9.0);
        assert_eq!((left, w), (0.0, 210.0));
        // 7 s of the 22 in view, scrolled to the end: the thumb is that share and at the right.
        let (left, w, per) = zoom_thumb(bar, 20.0, 15.0, 30.0);
        assert!((w - 210.0 * 7.0 / 22.0).abs() < 1e-3);
        assert!((left + w - 210.0).abs() < 1e-3);
        assert!(per > 0.0);
        let thumb = Rect::from_min_max(pos2(left, 0.0), pos2(left + w, 10.0));
        assert_eq!(
            ZoomThumbPart::at(thumb, pos2(left + 2.0, 5.0)),
            ZoomThumbPart::Left
        );
        assert_eq!(
            ZoomThumbPart::at(thumb, pos2(left + w / 2.0, 5.0)),
            ZoomThumbPart::Middle
        );
        assert_eq!(
            ZoomThumbPart::at(thumb, pos2(10.0, 5.0)),
            ZoomThumbPart::Beside
        );
    }

    /// Drives the zoom scroll bar with pointer events: press at `from`, move to `to` in steps, release.
    fn drag_bar(view: &mut View, from: egui::Pos2, to: egui::Pos2) {
        let ctx = egui::Context::default();
        let bar = Rect::from_min_max(pos2(100.0, 100.0), pos2(1100.0, 110.0));
        let screen = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(1200.0, 800.0));
        let frame = |events: Vec<egui::Event>, view: &mut View| {
            let input = egui::RawInput {
                events,
                screen_rect: Some(screen),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                // As the timeline's bar row does: the bar's place is taken first, then the bar drawn there.
                let _ = ui.allocate_rect(bar, egui::Sense::hover());
                super::zoom_scrollbar(ui, bar, view, 20.0, egui::Id::unique("bar"));
            });
            // No renderer here: the font atlas upload is dropped on purpose.
            output.textures_delta.clear();
        };
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![egui::Event::PointerMoved(from)], view);
        frame(vec![button(from, true)], view);
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            frame(vec![egui::Event::PointerMoved(from.lerp(to, t))], view);
        }
        frame(vec![button(to, false)], view);
        frame(vec![], view);
    }

    #[test]
    fn dragging_a_handle_zooms_and_the_middle_scrolls() {
        // 1000 points at 50 pps: 20 s of the 22 s extent in view; the thumb is 0..909, its right handle at 904.
        let fresh = || View {
            pps: 50.0,
            target_pps: 50.0,
            fit_pending: false,
            ..View::default()
        };
        let mut view = fresh();
        drag_bar(
            &mut view,
            pos2(100.0 + 904.0, 105.0),
            pos2(100.0 + 504.0, 105.0),
        );
        assert!(view.pps > 80.0, "zoomed in to {}", view.pps);
        assert_eq!(view.scroll, 0.0);
        // The middle of a zoomed-in thumb scrolls.
        let mut view = View {
            pps: 200.0,
            target_pps: 200.0,
            fit_pending: false,
            ..View::default()
        };
        drag_bar(
            &mut view,
            pos2(100.0 + 100.0, 105.0),
            pos2(100.0 + 400.0, 105.0),
        );
        assert!(view.scroll > 1.0, "scrolled to {}", view.scroll);
        assert_eq!(view.pps, 200.0);
    }

    #[test]
    fn a_new_view_fits_the_clip_and_stays_inside() {
        let content = Rect::from_min_max(pos2(200.0, 0.0), pos2(1200.0, 100.0));
        let mut view = View::default();
        let _ = view.animate(1.0, content, 20.0, 0.0, false);
        assert!(!view.fit_pending);
        // 94 % of 1000 points for 20 s.
        assert!((view.target_pps - 47.0).abs() < 1e-6);
        // Playing past the right edge pages the view along.
        view.pps = 100.0;
        view.target_pps = 100.0;
        let _ = view.animate(1.0, content, 60.0, 15.0, true);
        assert!(view.scroll > 0.0);
        // Scrolled past the end, the view comes back inside.
        view.scroll = 1e9;
        view.target_scroll = 1e9;
        let _ = view.animate(0.016, content, 60.0, 15.0, false);
        assert!(view.scroll <= max_scroll(60.0, 1000.0, view.pps) + 1e-9);
    }
}
