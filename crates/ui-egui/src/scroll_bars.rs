//! Encore's scroll bars (M6.453, measured on its Flowchart panel): 18 thick in a dark edge, an arrow at each end
//! that steps (held, it repeats), a press on the track pages, the thumb drags. The thumb shows only while there is
//! something to scroll, and both arrows are dim while there is not.

use egui::{Color32, CornerRadius, Id, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};

/// Thickness of a bar with its edges (Encore: 18).
pub(crate) const BAR: f32 = 18.0;
/// How far a press on an arrow scrolls.
const STEP: f32 = 20.0;
/// The shortest thumb.
const MIN_THUMB: f32 = 16.0;
/// How long a held arrow or track waits before it repeats, and then between repeats (seconds).
const REPEAT_DELAY: f64 = 0.4;
const REPEAT_EVERY: f64 = 0.05;

/// The bar's outer edge, the line inside it and its track (Encore: 26, 50 and 55).
const EDGE: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(26), Color32::from_gray(170));
const INNER: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(50), Color32::from_gray(225));
const TRACK: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(55), Color32::from_gray(232));
/// The thumb: its dark edge, a light line inside it and its face (Encore: 6, 78 and 64).
const THUMB_EDGE: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(6), Color32::from_gray(140));
const THUMB_LIGHT: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(78), Color32::from_gray(252));
const THUMB_FACE: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(64), Color32::from_gray(215));
/// An arrow while there is something to scroll, and while there is not (Encore: 172 and 102).
const ARROW: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(172), Color32::from_gray(80));
const ARROW_DIM: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(102), Color32::from_gray(170));

/// Which way a bar scrolls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    Horizontal,
    Vertical,
}

/// What a press on a bar hit.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    /// The arrow at the start (up or left).
    Back,
    /// The arrow at the end (down or right).
    On,
    /// The track before the thumb.
    PageBack,
    /// The track after the thumb.
    PageOn,
    /// The thumb, grabbed this far from its start.
    Thumb(f32),
}

/// A press held on a bar: what it hit, when, and when it last repeated.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Held {
    part: Part,
    since: f64,
    last: f64,
}

/// What a bar scrolls: how far it is scrolled, how much shows, and how long the whole is (points).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Span {
    pub(crate) offset: f32,
    pub(crate) view: f32,
    pub(crate) content: f32,
}

impl Span {
    /// How far it can be scrolled (0 when everything shows, less than a point beyond it included, or for sizes that
    /// are not numbers).
    fn range(self) -> f32 {
        let range = self.content - self.view;
        if range.is_finite() && range >= 1.0 {
            range
        } else {
            0.0
        }
    }

    /// `offset` kept in the range.
    fn kept(self, offset: f32) -> f32 {
        if offset.is_finite() {
            offset.clamp(0.0, self.range())
        } else {
            0.0
        }
    }
}

/// The thumb on a track `track` long: its start along the track and its length; `None` while there is nothing to
/// scroll.
pub(crate) fn thumb(track: f32, span: Span) -> Option<(f32, f32)> {
    let range = span.range();
    let usable = |value: f32| value.is_finite() && value > 0.0;
    if range <= 0.0 || !usable(track) || !usable(span.content) {
        return None;
    }
    let length = (track * span.view / span.content).clamp(MIN_THUMB.min(track), track);
    let at = (span.kept(span.offset) / range) * (track - length);
    Some((at, length))
}

/// The offset that puts the thumb (`length` long) at `at` along a track `track` long.
pub(crate) fn offset_at(at: f32, track: f32, length: f32, span: Span) -> f32 {
    let room = track - length;
    if room.is_nan() || room <= 0.0 || !at.is_finite() {
        return span.kept(span.offset);
    }
    (at / room).clamp(0.0, 1.0) * span.range()
}

/// The part of the bar along its length at `along` points from its start: the arrows are squares at the ends.
fn part_at(along: f32, length: f32, track_thumb: Option<(f32, f32)>) -> Option<Part> {
    if along < BAR {
        return Some(Part::Back);
    }
    if along >= length - BAR {
        return Some(Part::On);
    }
    let (at, thumb) = track_thumb?;
    let on_track = along - BAR;
    Some(if on_track < at {
        Part::PageBack
    } else if on_track >= at + thumb {
        Part::PageOn
    } else {
        Part::Thumb(on_track - at)
    })
}

/// Where `part` scrolls to from `offset` (an arrow a step, the track a page).
fn stepped(part: Part, span: Span) -> f32 {
    let page = (span.view - STEP).max(STEP);
    let to = match part {
        Part::Back => span.offset - STEP,
        Part::On => span.offset + STEP,
        Part::PageBack => span.offset - page,
        Part::PageOn => span.offset + page,
        Part::Thumb(_) => span.offset,
    };
    span.kept(to)
}

/// Draw a bar in `rect` and handle presses and drags on it; returns the new offset when it scrolled.
pub(crate) fn scroll_bar(ui: &mut Ui, rect: Rect, axis: Axis, id: Id, span: Span) -> Option<f32> {
    let along = |point: Pos2| match axis {
        Axis::Horizontal => point.x - rect.min.x,
        Axis::Vertical => point.y - rect.min.y,
    };
    let length = match axis {
        Axis::Horizontal => rect.width(),
        Axis::Vertical => rect.height(),
    };
    let track = (length - 2.0 * BAR).max(0.0);
    let found = thumb(track, span);
    draw_bar(ui, rect, axis, found, span.range() > 0.0);

    let response = ui.interact(rect, id, Sense::click_and_drag());
    let held_id = id.with("held");
    let now = ui.input(|input| input.time);
    let mut held: Option<Held> = ui.data(|data| data.get_temp(held_id));
    let mut to = None;
    if response.is_pointer_button_down_on() {
        if held.is_none()
            && let Some(pointer) = ui.input(|input| input.pointer.press_origin())
            && let Some(part) = part_at(along(pointer), length, found)
        {
            held = Some(Held {
                part,
                since: now,
                last: now,
            });
            to = Some(stepped(part, span));
        }
    } else {
        held = None;
    }
    if let Some(state) = held.as_mut() {
        match state.part {
            Part::Thumb(grab) => {
                if let (Some(pointer), Some((_, thumb))) = (response.interact_pointer_pos(), found)
                {
                    to = Some(offset_at(along(pointer) - BAR - grab, track, thumb, span));
                }
            }
            part => {
                if now - state.since > REPEAT_DELAY && now - state.last >= REPEAT_EVERY {
                    state.last = now;
                    // The track pages only until the thumb reaches the pointer.
                    let reached = matches!(part, Part::PageBack | Part::PageOn)
                        && response.interact_pointer_pos().is_some_and(|pointer| {
                            !matches!(part_at(along(pointer), length, found), Some(now) if now == part)
                        });
                    if !reached {
                        to = Some(stepped(part, span));
                    }
                }
                ui.ctx().request_repaint();
            }
        }
    }
    ui.data_mut(|data| match held {
        Some(state) => {
            let _ = data.insert_temp(held_id, state);
        }
        None => data.remove::<Held>(held_id),
    });
    to.filter(|to| (to - span.offset).abs() > f32::EPSILON)
}

/// The bar: its edges and track, the arrows, and the thumb when there is one.
fn draw_bar(ui: &Ui, rect: Rect, axis: Axis, found: Option<(f32, f32)>, active: bool) {
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, EDGE.get());
    painter.rect_filled(rect.shrink(1.0), CornerRadius::ZERO, INNER.get());
    painter.rect_filled(rect.shrink(2.0), CornerRadius::ZERO, TRACK.get());
    let color = if active { ARROW.get() } else { ARROW_DIM.get() };
    let (back, on) = match axis {
        Axis::Horizontal => (
            Rect::from_min_size(rect.min, vec2(BAR, rect.height())),
            Rect::from_min_size(pos2(rect.max.x - BAR, rect.min.y), vec2(BAR, rect.height())),
        ),
        Axis::Vertical => (
            Rect::from_min_size(rect.min, vec2(rect.width(), BAR)),
            Rect::from_min_size(pos2(rect.min.x, rect.max.y - BAR), vec2(rect.width(), BAR)),
        ),
    };
    // Encore: 10 wide and 6 high up and down, 12 high and 7 wide left and right.
    let (back_dir, on_dir, size) = match axis {
        Axis::Horizontal => (vec2(-1.0, 0.0), vec2(1.0, 0.0), vec2(7.0, 12.0)),
        Axis::Vertical => (vec2(0.0, -1.0), vec2(0.0, 1.0), vec2(10.0, 6.0)),
    };
    painter.add(arrow(back.center(), back_dir, size, color));
    painter.add(arrow(on.center(), on_dir, size, color));
    if let Some((at, length)) = found {
        let thumb = match axis {
            Axis::Horizontal => Rect::from_min_size(
                pos2(rect.min.x + BAR + at, rect.min.y),
                vec2(length, rect.height()),
            ),
            Axis::Vertical => Rect::from_min_size(
                pos2(rect.min.x, rect.min.y + BAR + at),
                vec2(rect.width(), length),
            ),
        };
        painter.rect_filled(thumb, CornerRadius::ZERO, THUMB_EDGE.get());
        painter.rect_filled(thumb.shrink(1.0), CornerRadius::ZERO, THUMB_LIGHT.get());
        painter.rect_filled(thumb.shrink(2.0), CornerRadius::ZERO, THUMB_FACE.get());
    }
}

/// A filled triangle centred on `center` pointing along `dir` (a unit vector on an axis), `size` across and along
/// as it lies (width by height).
pub(crate) fn arrow(center: Pos2, dir: Vec2, size: Vec2, color: Color32) -> Shape {
    let half = size / 2.0;
    let points = if dir.x.abs() > 0.0 {
        let tip = center + vec2(dir.x * half.x, 0.0);
        let back = center.x - dir.x * half.x;
        vec![
            tip,
            pos2(back, center.y - half.y),
            pos2(back, center.y + half.y),
        ]
    } else {
        let tip = center + vec2(0.0, dir.y * half.y);
        let back = center.y - dir.y * half.y;
        vec![
            tip,
            pos2(center.x - half.x, back),
            pos2(center.x + half.x, back),
        ]
    };
    Shape::convex_polygon(points, color, Stroke::NONE)
}

#[cfg(test)]
mod tests {
    use super::{BAR, Part, Span, offset_at, part_at, stepped, thumb};

    fn span(offset: f32, view: f32, content: f32) -> Span {
        Span {
            offset,
            view,
            content,
        }
    }

    #[test]
    fn the_thumb_shows_what_is_in_view() {
        // Half of it in view, scrolled to the end: half the track, at its end.
        assert_eq!(thumb(100.0, span(500.0, 500.0, 1000.0)), Some((50.0, 50.0)));
        assert_eq!(thumb(100.0, span(0.0, 500.0, 1000.0)), Some((0.0, 50.0)));
        // A little in view: the shortest thumb.
        assert_eq!(thumb(100.0, span(0.0, 10.0, 100_000.0)), Some((0.0, 16.0)));
        // Nothing to scroll, or sizes that are not numbers: no thumb (Encore).
        assert_eq!(thumb(100.0, span(0.0, 500.0, 400.0)), None);
        assert_eq!(thumb(100.0, span(0.0, 500.0, 500.6)), None);
        assert_eq!(thumb(100.0, span(0.0, f32::NAN, 400.0)), None);
        assert_eq!(thumb(0.0, span(0.0, 10.0, 400.0)), None);
        // An offset past the end keeps the thumb on the track.
        assert_eq!(thumb(100.0, span(9e9, 500.0, 1000.0)), Some((50.0, 50.0)));
    }

    #[test]
    fn dragging_the_thumb_scrolls_in_proportion() {
        let whole = span(0.0, 500.0, 1000.0);
        assert_eq!(offset_at(25.0, 100.0, 50.0, whole), 250.0);
        assert_eq!(offset_at(-10.0, 100.0, 50.0, whole), 0.0);
        assert_eq!(offset_at(80.0, 100.0, 50.0, whole), 500.0);
        assert_eq!(offset_at(f32::NAN, 100.0, 50.0, whole), 0.0);
        assert_eq!(offset_at(10.0, 50.0, 50.0, whole), 0.0);
    }

    #[test]
    fn a_press_finds_the_arrows_track_and_thumb() {
        let thumb_at = Some((20.0, 30.0));
        let length = 2.0 * BAR + 100.0;
        assert_eq!(part_at(5.0, length, thumb_at), Some(Part::Back));
        assert_eq!(part_at(length - 1.0, length, thumb_at), Some(Part::On));
        assert_eq!(part_at(BAR + 10.0, length, thumb_at), Some(Part::PageBack));
        assert_eq!(
            part_at(BAR + 25.0, length, thumb_at),
            Some(Part::Thumb(5.0))
        );
        assert_eq!(part_at(BAR + 60.0, length, thumb_at), Some(Part::PageOn));
        // Without a thumb only the arrows answer.
        assert_eq!(part_at(BAR + 10.0, length, None), None);
    }

    #[test]
    fn arrows_step_and_the_track_pages_within_the_range() {
        let at = span(100.0, 300.0, 1000.0);
        assert_eq!(stepped(Part::Back, at), 80.0);
        assert_eq!(stepped(Part::On, at), 120.0);
        assert_eq!(stepped(Part::PageOn, at), 380.0);
        assert_eq!(stepped(Part::PageBack, at), 0.0);
        assert_eq!(stepped(Part::On, span(700.0, 300.0, 1000.0)), 700.0);
        assert_eq!(stepped(Part::On, span(0.0, 300.0, 200.0)), 0.0);
    }
}
