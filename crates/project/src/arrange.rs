//! Editing several menu buttons at once: aligning and distributing them (Object ▸ Align / Distribute), and the
//! batch edits behind cut, copy, paste, duplicate and moving a multiple selection.
//!
//! Each operation changes the menu in one call, so the engine records it as one undo step.

use crate::check_menu::rects_overlap;
use crate::ids::MenuId;
use crate::io::ProjectError;
use crate::link::Link;
use crate::menu::{Button, MAX_BUTTONS, Rect, frame_size};
use crate::model::Project;

/// Most indexes one batch edit accepts (a menu holds at most [`MAX_BUTTONS`]; more means a bad request).
const MAX_BATCH: usize = 256;
/// Grid step of the search for a free place to paste (disc pixels).
const FREE_STEP: i32 = 8;

/// The edge or centre line that buttons line up on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// Left edges.
    Left,
    /// Horizontal centres (a vertical line through the middle).
    HorizontalCenter,
    /// Right edges.
    Right,
    /// Top edges.
    Top,
    /// Vertical centres (a horizontal line through the middle).
    VerticalCenter,
    /// Bottom edges.
    Bottom,
}

impl Align {
    /// Parses the command name (`left`, `center`, `right`, `top`, `middle`, `bottom`).
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "left" => Some(Self::Left),
            "center" => Some(Self::HorizontalCenter),
            "right" => Some(Self::Right),
            "top" => Some(Self::Top),
            "middle" => Some(Self::VerticalCenter),
            "bottom" => Some(Self::Bottom),
            _ => None,
        }
    }

    /// The command name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::HorizontalCenter => "center",
            Self::Right => "right",
            Self::Top => "top",
            Self::VerticalCenter => "middle",
            Self::Bottom => "bottom",
        }
    }
}

/// Object ▸ Arrange. In DiscCraft the stacking order of buttons is their number, so arranging renumbers them:
/// the front is the highest number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrange {
    /// To the front (the last numbers).
    BringToFront,
    /// One step forward (past the next button that is not selected).
    BringForward,
    /// One step backward.
    SendBackward,
    /// To the back (the first numbers).
    SendToBack,
}

impl Arrange {
    /// Parses the command name (`front`, `forward`, `backward`, `back`).
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "front" => Some(Self::BringToFront),
            "forward" => Some(Self::BringForward),
            "backward" => Some(Self::SendBackward),
            "back" => Some(Self::SendToBack),
            _ => None,
        }
    }

    /// The command name.
    pub fn name(self) -> &'static str {
        match self {
            Self::BringToFront => "front",
            Self::BringForward => "forward",
            Self::SendBackward => "backward",
            Self::SendToBack => "back",
        }
    }
}

/// The new order of `count` buttons after arranging `selected` (an entry `i` of the result is the old index of the
/// button that gets number `i + 1`). Selected buttons keep their order among themselves.
pub fn arranged_order(count: usize, selected: &[usize], arrange: Arrange) -> Vec<usize> {
    let picked = |index: &usize| selected.contains(index);
    let mut order: Vec<usize> = (0..count).collect();
    match arrange {
        Arrange::BringToFront => {
            order = (0..count)
                .filter(|index| !picked(index))
                .chain((0..count).filter(picked))
                .collect();
        }
        Arrange::SendToBack => {
            order = (0..count)
                .filter(picked)
                .chain((0..count).filter(|index| !picked(index)))
                .collect();
        }
        Arrange::BringForward => {
            // From the front down, swap each selected button with the unselected one right above it.
            for position in (0..count.saturating_sub(1)).rev() {
                let (Some(here), Some(above)) = (order.get(position), order.get(position + 1))
                else {
                    continue;
                };
                if picked(here) && !picked(above) {
                    order.swap(position, position + 1);
                }
            }
        }
        Arrange::SendBackward => {
            for position in 1..count {
                let (Some(here), Some(below)) = (order.get(position), order.get(position - 1))
                else {
                    continue;
                };
                if picked(here) && !picked(below) {
                    order.swap(position, position - 1);
                }
            }
        }
    }
    order
}

/// The new order after moving the button at `from` to number `to + 1` (the button sheet's Number).
pub fn moved_order(count: usize, from: usize, to: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..count).filter(|index| *index != from).collect();
    if from < count {
        order.insert(to.min(order.len()), from);
    }
    order
}

/// The direction buttons are spread along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Left to right.
    Horizontal,
    /// Top to bottom.
    Vertical,
}

impl Axis {
    /// Parses the command name (`horizontal`, `vertical`).
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "horizontal" => Some(Self::Horizontal),
            "vertical" => Some(Self::Vertical),
            _ => None,
        }
    }

    /// The command name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }
}

/// The smallest rectangle holding all of `rects`, or `None` when there are none.
pub fn bounds(rects: &[Rect]) -> Option<Rect> {
    let left = rects.iter().map(|rect| i64::from(rect.x)).min()?;
    let top = rects.iter().map(|rect| i64::from(rect.y)).min()?;
    let right = rects.iter().map(right_edge).max()?;
    let bottom = rects.iter().map(bottom_edge).max()?;
    Some(Rect {
        x: to_u16(left),
        y: to_u16(top),
        w: to_u16(right - left),
        h: to_u16(bottom - top),
    })
}

/// The safe area buttons are aligned to when "relative to safe area" is on: 5% in from each edge of the frame,
/// the inner guide the menu viewer draws.
pub fn safe_area(frame: (u16, u16)) -> Rect {
    let inset_x = frame.0 / 20;
    let inset_y = frame.1 / 20;
    Rect {
        x: inset_x,
        y: inset_y,
        w: frame.0.saturating_sub(inset_x.saturating_mul(2)),
        h: frame.1.saturating_sub(inset_y.saturating_mul(2)),
    }
}

/// Lines `rects` up on an edge or centre line of `area`, or of their own bounds when `area` is `None`.
/// Sizes do not change; the result is in the same order as `rects`.
pub fn align_rects(rects: &[Rect], align: Align, area: Option<Rect>) -> Vec<Rect> {
    let Some(reference) = area.or_else(|| bounds(rects)) else {
        return Vec::new();
    };
    let left = i64::from(reference.x);
    let top = i64::from(reference.y);
    rects
        .iter()
        .map(|rect| {
            let mut out = *rect;
            let (w, h) = (i64::from(rect.w), i64::from(rect.h));
            match align {
                Align::Left => out.x = to_u16(left),
                Align::HorizontalCenter => {
                    out.x = to_u16((2 * left + i64::from(reference.w) - w).div_euclid(2));
                }
                Align::Right => out.x = to_u16(right_edge(&reference) - w),
                Align::Top => out.y = to_u16(top),
                Align::VerticalCenter => {
                    out.y = to_u16((2 * top + i64::from(reference.h) - h).div_euclid(2));
                }
                Align::Bottom => out.y = to_u16(bottom_edge(&reference) - h),
            }
            out
        })
        .collect()
}

/// Spreads `rects` along `axis` with equal gaps between neighbours, keeping their order along the axis. Without
/// `area` the outermost two stay where they are; with `area` the first and last touch its edges, and a single
/// rectangle is centred in it. The result is in the same order as `rects`.
pub fn distribute_rects(rects: &[Rect], axis: Axis, area: Option<Rect>) -> Vec<Rect> {
    let mut out = rects.to_vec();
    let Some(reference) = area.or_else(|| bounds(rects)) else {
        return out;
    };
    let low = start_on(&reference, axis);
    let high = low + size_on(&reference, axis);

    let mut order: Vec<(i64, usize)> = rects
        .iter()
        .enumerate()
        .map(|(index, rect)| (start_on(rect, axis), index))
        .collect();
    order.sort_unstable();

    if let ([(_, index)], Some(_)) = (order.as_slice(), area) {
        if let Some(rect) = out.get_mut(*index) {
            let position = low + (high - low - size_on(rect, axis)).div_euclid(2);
            move_on(rect, axis, position);
        }
        return out;
    }
    let Some(gaps) = i64::try_from(order.len())
        .ok()
        .and_then(|count| count.checked_sub(1))
    else {
        return out;
    };
    if gaps == 0 {
        return out;
    }

    let total: i64 = rects.iter().map(|rect| size_on(rect, axis)).sum();
    let free = high - low - total;
    let mut before = 0_i64;
    for (step, (_, index)) in (0_i64..).zip(order.iter()) {
        let Some(rect) = out.get_mut(*index) else {
            continue;
        };
        move_on(rect, axis, low + before + (free * step).div_euclid(gaps));
        before += size_on(rect, axis);
    }
    out
}

/// Where `rect` starts along `axis`.
fn start_on(rect: &Rect, axis: Axis) -> i64 {
    match axis {
        Axis::Horizontal => i64::from(rect.x),
        Axis::Vertical => i64::from(rect.y),
    }
}

/// How long `rect` is along `axis`.
fn size_on(rect: &Rect, axis: Axis) -> i64 {
    match axis {
        Axis::Horizontal => i64::from(rect.w),
        Axis::Vertical => i64::from(rect.h),
    }
}

/// Moves `rect` along `axis` so that it starts at `position`.
fn move_on(rect: &mut Rect, axis: Axis, position: i64) {
    match axis {
        Axis::Horizontal => rect.x = to_u16(position),
        Axis::Vertical => rect.y = to_u16(position),
    }
}

/// The offset nearest to `(0, 0)` that puts all of `group` inside the frame without overlapping `existing`, for
/// paste and duplicate (DVD buttons may not overlap). Ties go down, then right. `None` when there is no room.
pub fn free_offset(group: &[Rect], existing: &[Rect], frame: (u16, u16)) -> Option<(i32, i32)> {
    let outline = bounds(group)?;
    let range = |start: u16, size: u16, frame: u16| {
        let low = -i32::from(start);
        let high = i32::from(frame) - i32::from(start) - i32::from(size);
        (low <= high).then_some((low, high))
    };
    let (min_dx, max_dx) = range(outline.x, outline.w, frame.0)?;
    let (min_dy, max_dy) = range(outline.y, outline.h, frame.1)?;
    let steps = |low: i32, high: i32| {
        let mut values: Vec<i32> = (low.div_euclid(FREE_STEP)..=high.div_euclid(FREE_STEP))
            .map(|step| step * FREE_STEP)
            .filter(|value| (low..=high).contains(value))
            .collect();
        values.extend(
            [0, low, high]
                .into_iter()
                .filter(|value| (low..=high).contains(value)),
        );
        values.sort_unstable();
        values.dedup();
        values
    };
    let dxs = steps(min_dx, max_dx);
    let dys = steps(min_dy, max_dy);

    let mut offsets: Vec<(i32, i32)> = dys
        .iter()
        .flat_map(|dy| dxs.iter().map(move |dx| (*dx, *dy)))
        .collect();
    offsets.sort_by_key(|(dx, dy)| {
        let distance = i64::from(*dx) * i64::from(*dx) + i64::from(*dy) * i64::from(*dy);
        (distance, *dy < 0, *dx < 0)
    });
    offsets.into_iter().find(|(dx, dy)| {
        group.iter().all(|rect| {
            let placed = Rect {
                x: to_u16(i64::from(rect.x) + i64::from(*dx)),
                y: to_u16(i64::from(rect.y) + i64::from(*dy)),
                ..*rect
            };
            !existing.iter().any(|other| rects_overlap(&placed, other))
        })
    })
}

/// Keeps `rect` inside the frame (sizes larger than the frame are left alone and pinned to the top left).
fn keep_in_frame(rect: Rect, frame: (u16, u16)) -> Rect {
    Rect {
        x: rect.x.min(frame.0.saturating_sub(rect.w)),
        y: rect.y.min(frame.1.saturating_sub(rect.h)),
        ..rect
    }
}

fn right_edge(rect: &Rect) -> i64 {
    i64::from(rect.x) + i64::from(rect.w)
}

fn bottom_edge(rect: &Rect) -> i64 {
    i64::from(rect.y) + i64::from(rect.h)
}

fn to_u16(value: i64) -> u16 {
    u16::try_from(value.clamp(0, i64::from(u16::MAX))).unwrap_or(0)
}

fn invalid(message: &str) -> ProjectError {
    ProjectError::Invalid(message.to_string())
}

/// Sorted, de-duplicated indexes, all below `len`.
fn checked_indexes(indexes: &[usize], len: usize) -> Result<Vec<usize>, ProjectError> {
    if indexes.len() > MAX_BATCH {
        return Err(invalid("too many buttons in one request"));
    }
    let mut sorted = indexes.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted.iter().any(|index| *index >= len) {
        return Err(invalid("button index out of range"));
    }
    Ok(sorted)
}

impl Project {
    /// Adds copies of `buttons` to the end of a menu (paste and duplicate) and returns their indexes. Each copy is
    /// renamed after its new number. A link whose target does not exist in this project (a copy from another
    /// project) is cleared.
    pub fn paste_buttons(
        &mut self,
        menu: MenuId,
        buttons: Vec<Button>,
    ) -> Result<Vec<usize>, ProjectError> {
        let start = self
            .menu(menu)
            .ok_or_else(|| invalid("menu does not exist"))?
            .buttons
            .len();
        if buttons.is_empty() {
            return Err(invalid("nothing to paste"));
        }
        if start.saturating_add(buttons.len()) > MAX_BUTTONS {
            return Err(invalid("menu has too many buttons"));
        }

        let frame = frame_size(&self.settings);
        let buttons: Vec<Button> = buttons
            .into_iter()
            .enumerate()
            .map(|(offset, button)| {
                let link = if self.link_resolves(&button.link) {
                    button.link
                } else {
                    Link::NotSet
                };
                Button {
                    name: format!("Button {}", start.saturating_add(offset).saturating_add(1)),
                    rect: keep_in_frame(button.rect, frame),
                    link,
                    ..button
                }
            })
            .collect();
        let count = buttons.len();
        let target = self
            .menu_mut(menu)
            .ok_or_else(|| invalid("menu does not exist"))?;
        target.buttons.extend(buttons);
        Ok((start..start.saturating_add(count)).collect())
    }

    /// Removes several buttons at once. The default button keeps pointing at the same button, or goes back to the
    /// first when it was removed.
    pub fn remove_buttons(&mut self, menu: MenuId, indexes: &[usize]) -> Result<(), ProjectError> {
        let target = self
            .menu_mut(menu)
            .ok_or_else(|| invalid("menu does not exist"))?;
        let sorted = checked_indexes(indexes, target.buttons.len())?;
        if sorted.is_empty() {
            return Err(invalid("no buttons to remove"));
        }

        let default = usize::try_from(target.default_button).unwrap_or(usize::MAX);
        for index in sorted.iter().rev() {
            target.buttons.remove(*index);
        }
        // Links that pick a button of this menu follow it to its new number; a removed one picks no button.
        self.visit_links(|link| {
            if let Link::Menu { id, button } = link
                && *id == menu
                && let Some(number) = button
                && let Ok(old) = usize::try_from(*number)
            {
                *button = if sorted.contains(&old) {
                    None
                } else {
                    u32::try_from(old - sorted.iter().filter(|index| **index < old).count()).ok()
                };
            }
        });
        let target = self
            .menu_mut(menu)
            .ok_or_else(|| invalid("menu does not exist"))?;
        let default = if sorted.contains(&default) {
            0
        } else {
            default.saturating_sub(sorted.iter().filter(|index| **index < default).count())
        };
        target.default_button = if default < target.buttons.len() {
            u32::try_from(default).unwrap_or(0)
        } else {
            0
        };
        Ok(())
    }

    /// Renumbers the buttons of a menu: `order[i]` is the old index of the button that gets number `i + 1`
    /// (Object ▸ Arrange and the button sheet's Number). The default button and every link that picks a button of
    /// this menu follow their button.
    pub fn reorder_buttons(&mut self, menu: MenuId, order: &[usize]) -> Result<(), ProjectError> {
        let target = self
            .menu_mut(menu)
            .ok_or_else(|| invalid("menu does not exist"))?;
        let count = target.buttons.len();
        let mut seen = vec![false; count];
        for old in order {
            match seen.get_mut(*old) {
                Some(slot) if !*slot => *slot = true,
                _ => return Err(invalid("the order must name every button once")),
            }
        }
        if order.len() != count {
            return Err(invalid("the order must name every button once"));
        }

        let buttons: Vec<Button> = order
            .iter()
            .filter_map(|old| target.buttons.get(*old).cloned())
            .collect();
        target.buttons = buttons;
        let new_index = |old: usize| order.iter().position(|index| *index == old);
        let default = usize::try_from(target.default_button).unwrap_or(usize::MAX);
        target.default_button = new_index(default)
            .and_then(|index| u32::try_from(index).ok())
            .unwrap_or(0);

        self.visit_links(|link| {
            if let Link::Menu { id, button } = link
                && *id == menu
                && let Some(number) = button
                && let Ok(old) = usize::try_from(*number)
            {
                *button = new_index(old).and_then(|index| u32::try_from(index).ok());
            }
        });
        Ok(())
    }

    /// Moves or resizes several buttons at once (dragging or nudging a multiple selection).
    pub fn set_button_rects(
        &mut self,
        menu: MenuId,
        rects: &[(usize, Rect)],
    ) -> Result<(), ProjectError> {
        let target = self
            .menu_mut(menu)
            .ok_or_else(|| invalid("menu does not exist"))?;
        let indexes: Vec<usize> = rects.iter().map(|(index, _)| *index).collect();
        checked_indexes(&indexes, target.buttons.len())?;
        for (index, rect) in rects {
            if let Some(button) = target.buttons.get_mut(*index) {
                button.rect = *rect;
            }
        }
        Ok(())
    }

    /// Object ▸ Align: lines the buttons up on the edge or centre line of their bounds, or of the safe area.
    /// Needs two buttons, or one when aligning to the safe area.
    pub fn align_buttons(
        &mut self,
        menu: MenuId,
        indexes: &[usize],
        align: Align,
        to_safe_area: bool,
    ) -> Result<(), ProjectError> {
        self.arrange(menu, indexes, to_safe_area, |rects, area| {
            align_rects(rects, align, area)
        })
    }

    /// Object ▸ Distribute: equal gaps between the buttons along `axis`, within their bounds or the safe area.
    /// Needs two buttons, or one when distributing in the safe area.
    pub fn distribute_buttons(
        &mut self,
        menu: MenuId,
        indexes: &[usize],
        axis: Axis,
        to_safe_area: bool,
    ) -> Result<(), ProjectError> {
        self.arrange(menu, indexes, to_safe_area, |rects, area| {
            distribute_rects(rects, axis, area)
        })
    }

    fn arrange(
        &mut self,
        menu: MenuId,
        indexes: &[usize],
        to_safe_area: bool,
        layout: impl Fn(&[Rect], Option<Rect>) -> Vec<Rect>,
    ) -> Result<(), ProjectError> {
        let frame = frame_size(&self.settings);
        let target = self
            .menu_mut(menu)
            .ok_or_else(|| invalid("menu does not exist"))?;
        let sorted = checked_indexes(indexes, target.buttons.len())?;
        let needed = if to_safe_area { 1 } else { 2 };
        if sorted.len() < needed {
            return Err(invalid("select more buttons"));
        }

        let rects: Vec<Rect> = sorted
            .iter()
            .filter_map(|index| target.buttons.get(*index).map(|button| button.rect))
            .collect();
        let area = to_safe_area.then(|| safe_area(frame));
        for (index, rect) in sorted.iter().zip(layout(&rects, area)) {
            if let Some(button) = target.buttons.get_mut(*index) {
                button.rect = keep_in_frame(rect, frame);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Align, Arrange, Axis, align_rects, arranged_order, bounds, distribute_rects, free_offset,
        moved_order, safe_area,
    };
    use crate::link::Link;
    use crate::menu::{Button, Rect};
    use crate::model::Project;
    use crate::{MenuId, TimelineId};
    use std::error::Error;

    fn rect(x: u16, y: u16, w: u16, h: u16) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn bounds_hold_every_rect() {
        let rects = [rect(10, 20, 30, 40), rect(100, 5, 10, 10)];
        assert_eq!(bounds(&rects), Some(rect(10, 5, 100, 55)));
        assert_eq!(bounds(&[]), None);
    }

    #[test]
    fn align_uses_the_selection_bounds() {
        let rects = [rect(10, 20, 100, 40), rect(50, 100, 200, 40)];
        let left = align_rects(&rects, Align::Left, None);
        assert_eq!(left, vec![rect(10, 20, 100, 40), rect(10, 100, 200, 40)]);

        let right = align_rects(&rects, Align::Right, None);
        assert_eq!(right, vec![rect(150, 20, 100, 40), rect(50, 100, 200, 40)]);

        // The bounds run from 10 to 250, so the centre line is at 130.
        let center = align_rects(&rects, Align::HorizontalCenter, None);
        assert_eq!(center, vec![rect(80, 20, 100, 40), rect(30, 100, 200, 40)]);

        let bottom = align_rects(&rects, Align::Bottom, None);
        assert_eq!(bottom, vec![rect(10, 100, 100, 40), rect(50, 100, 200, 40)]);

        let middle = align_rects(&rects, Align::VerticalCenter, None);
        assert_eq!(middle, vec![rect(10, 60, 100, 40), rect(50, 60, 200, 40)]);
    }

    #[test]
    fn align_to_the_safe_area() {
        let area = safe_area((720, 480));
        assert_eq!(area, rect(36, 24, 648, 432));
        let one = align_rects(&[rect(300, 300, 100, 40)], Align::Top, Some(area));
        assert_eq!(one, vec![rect(300, 24, 100, 40)]);
        let one = align_rects(&[rect(300, 300, 100, 40)], Align::Right, Some(area));
        assert_eq!(one, vec![rect(584, 300, 100, 40)]);
    }

    #[test]
    fn distribute_makes_equal_gaps() {
        // Out of order on purpose: the order along the axis is what counts.
        let rects = [
            rect(0, 300, 100, 40),
            rect(0, 0, 100, 40),
            rect(0, 50, 100, 60),
        ];
        let spread = distribute_rects(&rects, Axis::Vertical, None);
        // Bounds 0..340, buttons take 140, so two gaps of 100.
        assert_eq!(
            spread,
            vec![
                rect(0, 300, 100, 40),
                rect(0, 0, 100, 40),
                rect(0, 140, 100, 60)
            ]
        );
    }

    #[test]
    fn distribute_in_the_safe_area() {
        let area = rect(36, 24, 648, 432);
        let rects = [rect(100, 0, 100, 40), rect(300, 0, 100, 40)];
        let spread = distribute_rects(&rects, Axis::Horizontal, Some(area));
        assert_eq!(spread, vec![rect(36, 0, 100, 40), rect(584, 0, 100, 40)]);

        let single = distribute_rects(&[rect(0, 0, 100, 40)], Axis::Horizontal, Some(area));
        assert_eq!(single, vec![rect(310, 0, 100, 40)]);
    }

    #[test]
    fn overlapping_buttons_never_go_below_zero() {
        let rects = [
            rect(0, 0, 300, 40),
            rect(10, 0, 300, 40),
            rect(20, 0, 300, 40),
        ];
        let spread = distribute_rects(&rects, Axis::Horizontal, None);
        assert!(spread.iter().all(|rect| rect.x <= 20));
    }

    #[test]
    fn free_offset_finds_the_nearest_room() {
        let stack = [
            rect(160, 120, 400, 48),
            rect(160, 184, 400, 48),
            rect(160, 248, 400, 48),
            rect(160, 312, 400, 48),
        ];
        // Copying the middle two: right below the stack is as near as right above it, and down wins.
        let group = [rect(160, 184, 400, 48), rect(160, 248, 400, 48)];
        assert_eq!(free_offset(&group, &stack, (720, 480)), Some((0, 176)));
        // A free place is used as it is.
        assert_eq!(
            free_offset(&[rect(10, 10, 50, 20)], &stack, (720, 480)),
            Some((0, 0))
        );
        // No room for something the size of the frame.
        assert_eq!(
            free_offset(&[rect(0, 0, 720, 480)], &stack, (720, 480)),
            None
        );
    }

    #[test]
    fn arranging_keeps_the_selection_in_order() {
        assert_eq!(
            arranged_order(5, &[1, 3], Arrange::BringToFront),
            vec![0, 2, 4, 1, 3]
        );
        assert_eq!(
            arranged_order(5, &[1, 3], Arrange::SendToBack),
            vec![1, 3, 0, 2, 4]
        );
        assert_eq!(
            arranged_order(5, &[1, 3], Arrange::BringForward),
            vec![0, 2, 1, 4, 3]
        );
        assert_eq!(
            arranged_order(5, &[1, 3], Arrange::SendBackward),
            vec![1, 0, 3, 2, 4]
        );
        // Already at the front: nothing moves.
        assert_eq!(
            arranged_order(3, &[2], Arrange::BringForward),
            vec![0, 1, 2]
        );
        assert_eq!(moved_order(4, 3, 0), vec![3, 0, 1, 2]);
        assert_eq!(moved_order(4, 0, 2), vec![1, 2, 0, 3]);
    }

    #[test]
    fn reordering_moves_the_default_button_and_links() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = menu_with(&mut project, 3)?;
        project.menu_mut(menu).ok_or("missing menu")?.default_button = 2;
        project.settings.first_play = Link::Menu {
            id: menu,
            button: Some(2),
        };
        project.reorder_buttons(menu, &[2, 0, 1])?;
        let edited = project.menu(menu).ok_or("missing menu")?;
        let labels: Vec<&str> = edited
            .buttons
            .iter()
            .map(|button| button.label.as_str())
            .collect();
        assert_eq!(labels, vec!["B2", "B0", "B1"]);
        assert_eq!(edited.default_button, 0);
        assert_eq!(
            project.settings.first_play,
            Link::Menu {
                id: menu,
                button: Some(0)
            }
        );
        assert!(project.reorder_buttons(menu, &[0, 0, 1]).is_err());
        assert!(project.reorder_buttons(menu, &[0, 1]).is_err());

        // Removing the linked button drops the button from the link.
        project.remove_buttons(menu, &[0])?;
        assert_eq!(
            project.settings.first_play,
            Link::Menu {
                id: menu,
                button: None
            }
        );
        Ok(())
    }

    fn menu_with(project: &mut Project, count: usize) -> Result<MenuId, Box<dyn Error>> {
        let menu = project.add_menu(None)?;
        for index in 0..count {
            project.add_button(menu, format!("B{index}"), None, Link::Stop)?;
        }
        Ok(menu)
    }

    #[test]
    fn paste_renames_and_clears_missing_links() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = menu_with(&mut project, 1)?;
        let copy = Button {
            auto_activate: false,
            override_link: None,
            highlight_group: 1,
            sync_name: true,
            name_from_link: false,
            routes: None,
            audio: None,
            subtitle: crate::model::SubtitleDefault::NoChange,
            kind: Default::default(),
            text_subpicture: false,
            transition: None,
            label_format: Default::default(),
            label_font_style: None,
            shadow: None,
            orientation: Default::default(),
            thumbnail: None,
            subpicture: None,
            name: "Button 1".to_string(),
            label: "Copy".to_string(),
            rect: rect(700, 470, 100, 40),
            link: Link::Timeline {
                id: TimelineId(99),
                chapter: 0,
            },
        };
        let added = project.paste_buttons(menu, vec![copy])?;
        assert_eq!(added, vec![1]);
        let pasted = project
            .menu(menu)
            .and_then(|menu| menu.buttons.get(1))
            .ok_or("missing button")?;
        assert_eq!(pasted.name, "Button 2");
        assert_eq!(pasted.link, Link::NotSet);
        // Kept inside the 720x480 frame.
        assert_eq!(pasted.rect, rect(620, 440, 100, 40));
        assert!(project.paste_buttons(menu, Vec::new()).is_err());
        Ok(())
    }

    #[test]
    fn remove_buttons_keeps_the_default_button() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = menu_with(&mut project, 5)?;
        project.menu_mut(menu).ok_or("missing menu")?.default_button = 3;
        project.remove_buttons(menu, &[0, 1, 1])?;
        let edited = project.menu(menu).ok_or("missing menu")?;
        let labels: Vec<&str> = edited
            .buttons
            .iter()
            .map(|button| button.label.as_str())
            .collect();
        assert_eq!(labels, vec!["B2", "B3", "B4"]);
        assert_eq!(edited.default_button, 1);

        project.remove_buttons(menu, &[1])?;
        assert_eq!(project.menu(menu).ok_or("missing menu")?.default_button, 0);
        assert!(project.remove_buttons(menu, &[5]).is_err());
        Ok(())
    }

    #[test]
    fn align_and_distribute_need_enough_buttons() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = menu_with(&mut project, 3)?;
        assert!(
            project
                .align_buttons(menu, &[0], Align::Left, false)
                .is_err()
        );
        project.align_buttons(menu, &[0], Align::Left, true)?;
        project.align_buttons(menu, &[0, 1, 2], Align::Right, false)?;
        project.distribute_buttons(menu, &[0, 1, 2], Axis::Vertical, true)?;
        let edited = project.menu(menu).ok_or("missing menu")?;
        let first = edited.buttons.first().ok_or("missing button")?.rect;
        let last = edited.buttons.get(2).ok_or("missing button")?.rect;
        assert_eq!(first.y, 24);
        assert_eq!(last.y + last.h, 456);
        assert!(project.set_button_rects(menu, &[(3, first)]).is_err());
        Ok(())
    }
}
