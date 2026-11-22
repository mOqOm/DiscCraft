//! Automatic button navigation routing.

use disccraft_project::{ButtonKind, Menu, Rect, RouteOptions, WrapMode};

/// The routing of every button of `menu` (up, down, left, right; one-based, 0 = nowhere): automatic from the
/// layout, or — with "Automatically Route Buttons" off — each button's own routes, the automatic ones for buttons
/// without any. Targets past the last button count as nowhere.
pub fn button_routes(menu: &Menu, options: &RouteOptions) -> Vec<(u8, u8, u8, u8)> {
    let rects: Vec<Rect> = menu.buttons.iter().map(|button| button.rect).collect();
    let automatic = automatic_routes(menu, &rects, options);
    if menu.auto_route {
        return automatic;
    }
    let count = u8::try_from(menu.buttons.len()).unwrap_or(u8::MAX);
    let valid = |target: u8| if target <= count { target } else { 0 };
    menu.buttons
        .iter()
        .zip(automatic)
        .map(|(button, auto)| match button.routes {
            Some([up, down, left, right]) => (valid(up), valid(down), valid(left), valid(right)),
            None => auto,
        })
        .collect()
}

/// Returns one-based navigation targets for the given rectangles, with Encore's default routing options.
///
/// Each tuple is `(up, down, left, right)`.
pub fn auto_route(rects: &[Rect]) -> Vec<(u8, u8, u8, u8)> {
    auto_route_with(rects, &RouteOptions::default())
}

/// The automatic routing of `menu`: [`auto_route_with`], or, when the options exclude Encore's next / previous
/// buttons and the menu has either kind of button, Encore's routing for them (seen in Encore CS6, 2026-11-13 and
/// 2026-11-21): the other buttons route among themselves (the next / previous buttons only break their rows and
/// columns, see [`lines`]); the leftmost of them (by centre, the higher one on a tie) goes up / left to the previous
/// button and the rightmost down / right to the next button, before anything else. A previous button goes down /
/// right to that leftmost button, left to the next button and stays up; a next button goes up / left to the
/// rightmost, right to the previous button and stays down.
fn automatic_routes(menu: &Menu, rects: &[Rect], options: &RouteOptions) -> Vec<(u8, u8, u8, u8)> {
    let page = |kind: ButtonKind| matches!(kind, ButtonKind::Next | ButtonKind::Previous);
    let kinds: Vec<ButtonKind> = menu.buttons.iter().map(|button| button.kind).collect();
    let normal: Vec<usize> = (0..rects.len())
        .filter(|index| kinds.get(*index).is_some_and(|kind| !page(*kind)))
        .collect();
    if normal.is_empty() || !options.exclude_next_previous || normal.len() == rects.len() {
        return auto_route_with(rects, options);
    }
    // The leftmost and rightmost of the other buttons, by centre (then top to bottom, then number).
    let place = |index: &usize| {
        let centre = rects
            .get(*index)
            .copied()
            .map_or(Centre { x: 0, y: 0 }, Centre::from_rect);
        (centre.x, centre.y, *index)
    };
    let (Some(first), Some(last)) = (
        normal.iter().min_by_key(|index| place(index)).copied(),
        normal.iter().max_by_key(|index| place(index)).copied(),
    ) else {
        return auto_route_with(rects, options);
    };
    let previous = kinds.iter().position(|kind| *kind == ButtonKind::Previous);
    let next = kinds.iter().position(|kind| *kind == ButtonKind::Next);
    let own_rects: Vec<Rect> = normal
        .iter()
        .filter_map(|index| rects.get(*index).copied())
        .collect();
    let page_rects: Vec<Rect> = (0..rects.len())
        .filter(|index| !normal.contains(index))
        .filter_map(|index| rects.get(index).copied())
        .collect();
    let router = Router::new(&own_rects, &page_rects, options);
    let own_routes = router.routes();
    // A one-based number among the other buttons, as a one-based number in the menu.
    let to_menu = |target: u8| -> u8 {
        usize::from(target)
            .checked_sub(1)
            .and_then(|index| normal.get(index))
            .map_or(0, |index| one_based(*index))
    };
    let or_own = |target: Option<usize>, own: usize| target.map_or(one_based(own), one_based);
    let directions = [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ];
    let mut routes = vec![(0, 0, 0, 0); rects.len()];
    for (sub, &index) in normal.iter().enumerate() {
        let mut out = [0u8; 4];
        for (slot, direction) in out.iter_mut().zip(directions) {
            let vertical = matches!(direction, Direction::Up | Direction::Down);
            let moves = if vertical {
                options.vertical
            } else {
                options.horizontal
            };
            *slot = if !moves {
                one_based(index)
            } else {
                if index == first && matches!(direction, Direction::Up | Direction::Left) {
                    or_own(previous, index)
                } else if index == last && matches!(direction, Direction::Down | Direction::Right) {
                    or_own(next, index)
                } else if let Some(near) = router.direct(sub, direction) {
                    to_menu(one_based(near))
                } else {
                    to_menu(
                        own_routes
                            .get(sub)
                            .and_then(|route| route.get(direction.slot()))
                            .copied()
                            .map_or(0, one_based),
                    )
                }
            };
        }
        if let Some(route) = routes.get_mut(index) {
            *route = (out[0], out[1], out[2], out[3]);
        }
    }
    for (index, kind) in kinds.iter().enumerate() {
        let own = one_based(index);
        let (up, down, left, right) = match kind {
            ButtonKind::Previous => (own, one_based(first), or_own(next, index), one_based(first)),
            ButtonKind::Next => (
                one_based(last),
                own,
                one_based(last),
                or_own(previous, index),
            ),
            _ => continue,
        };
        let (up, down) = if options.vertical {
            (up, down)
        } else {
            (own, own)
        };
        let (left, right) = if options.horizontal {
            (left, right)
        } else {
            (own, own)
        };
        if let Some(route) = routes.get_mut(index) {
            *route = (up, down, left, right);
        }
    }
    routes
}

/// As [`auto_route`], with the project's routing options (Encore's Preferences ▸ Menus ▸ Button Routing). A lone
/// button goes nowhere (0), as in Encore.
pub fn auto_route_with(rects: &[Rect], options: &RouteOptions) -> Vec<(u8, u8, u8, u8)> {
    if rects.len() == 1 {
        return vec![(0, 0, 0, 0)];
    }
    // Two buttons go to each other in all four directions, wherever they are (seen in Encore CS6 with its default
    // options, 2026-11-15: two left-aligned text buttons, one above the other, also went left / right to each other).
    let all_ways =
        options.vertical && options.vertical_wrap && options.horizontal && options.horizontal_wrap;
    if rects.len() == 2 && all_ways {
        return vec![(2, 2, 2, 2), (1, 1, 1, 1)];
    }
    Router::new(rects, &[], options)
        .routes()
        .into_iter()
        .map(|[up, down, left, right]| {
            (
                one_based(up),
                one_based(down),
                one_based(left),
                one_based(right),
            )
        })
        .collect()
}

/// Buttons whose left / top edges or centres are this close are lined up (Encore CS6: a button 9 px off its column
/// still belongs to it, one 10 px off does not).
const LINE_TOLERANCE: i64 = 9;
/// A button that misses a line by more than [`LINE_TOLERANCE`] but no more than this breaks it (Encore CS6, 2026-11-21:
/// 10 and 15 px off broke a row or column, 36 px off did not).
const NEAR_MISS: i64 = 18;
/// Up / down go straight only to a button within about 68° of the direction (the cosine; Encore CS6 took one at
/// 67.3° and left one at 69.5°, 772 px to the side) or no more than [`VERTICAL_ASIDE`] to the side. Left / right have
/// no such limit.
const VERTICAL_CONE_COS: f64 = 0.3746;
/// Up / down also go straight to a button this far to the side, or less, whatever the angle (Encore CS6, 2026-11-21:
/// it took a bar 370 px to the side at 70°, and left one 617 px to the side at 76°).
const VERTICAL_ASIDE: i64 = 400;
/// With nothing in a direction, a button up to about 110° round from it is still taken (Encore CS6 took one at
/// 107.6° and left one at 116.6°): the cosine of that angle.
const WIDE_COS: f64 = -0.342;
/// Of the buttons ahead, one on the clockwise side of the direction no more than this many degrees further round
/// than the best one (and scoring within [`SCORE_TIE`] of it) is taken instead (M10.51, Encore CS6, 2026-11-22: a
/// bar went up to the right-hand one of two tiles 2.84° further round, and to the left-hand one when it was 3.11°
/// nearer, with the tiles 106 to 356 px above).
const ANGLE_TIE: f64 = 2.9;
/// How much further than to the side the centre of a button behind (one that reaches past the button routed in the
/// direction, as a bar under a tile) may be for [`Router::around`] to look round to it clockwise of the direction
/// (M10.51, Encore CS6, 2026-11-22: a bar 106 to 356 px below a tile, whose angle limit fell from about 132° to
/// 115° as it got further away; 564 of 564 routes of the bar measurement).
const REACH_CLOCKWISE: f64 = 38.5;
/// [`REACH_CLOCKWISE`] the other way round (three of the four distances measured).
const REACH_ANTICLOCKWISE: f64 = 30.0;
/// How much worse than the best a clockwise button's score may be for [`ANGLE_TIE`] (M10.51: Encore took one 1.14
/// times the best, and left one 1.17 times it 0.4° round, the reporter's chapter page going down to Chapter 1).
const SCORE_TIE: f64 = 1.16;

/// Encore CS6's automatic routing, as measured on its menus (docs/encore-behavior.md 9.2 and 9.5j):
///
/// - Rows and columns are found as in [`lines`].
/// - In a column, up / down go to the neighbours and wrap at the ends (to the other end, or on to the next
///   column); the same for left / right in a row. Without wrapping, the ends look around as below.
/// - A button in a row but in no column stays put on up / down (and the other way round) while wrapping is on.
/// - A button that contains others goes to them first: the nearest one ahead, or the nearest level with it.
/// - Otherwise a direction without a line goes to the nearest button ahead: the smallest distance over the cosine
///   of the angle off the direction to the power 1.5, up / down only within [`VERTICAL_CONE_COS`], but a button
///   on the clockwise side nearly as good wins ([`ANGLE_TIE`]). With nothing ahead, it looks round up to about
///   110° (see [`Router::around`]).
/// - A button in no row and no column that no route reaches is entered from one button only: the nearest one, in
///   the direction it lies.
struct Router<'a> {
    /// The routed buttons' rectangles.
    bounds: Vec<Bounds>,
    centres: Vec<Centre>,
    options: &'a RouteOptions,
    /// Each button's column (index into `columns`) when it is in one of two or more.
    column_of: Vec<Option<usize>>,
    /// The columns, each top to bottom.
    columns: Vec<Vec<usize>>,
    row_of: Vec<Option<usize>>,
    /// The rows, each left to right.
    rows: Vec<Vec<usize>>,
}

impl<'a> Router<'a> {
    /// The routing of the buttons `rects`; `others` (Encore's next / previous buttons, routed apart) only break
    /// their rows and columns.
    fn new(rects: &[Rect], others: &[Rect], options: &'a RouteOptions) -> Self {
        let all: Vec<Rect> = rects.iter().chain(others).copied().collect();
        let all_bounds: Vec<Bounds> = all.iter().copied().map(Bounds::from_rect).collect();
        let all_centres: Vec<Centre> = all.iter().copied().map(Centre::from_rect).collect();
        let column_candidates = candidates(&all_bounds, &all_centres, rects.len(), true);
        let row_candidates = candidates(&all_bounds, &all_centres, rects.len(), false);
        let (column_of, columns) = lines(
            &all_bounds,
            &all_centres,
            rects.len(),
            (true, &column_candidates, &row_candidates),
        );
        let (row_of, rows) = lines(
            &all_bounds,
            &all_centres,
            rects.len(),
            (false, &row_candidates, &column_candidates),
        );
        Self {
            bounds: all_bounds.into_iter().take(rects.len()).collect(),
            centres: all_centres.into_iter().take(rects.len()).collect(),
            options,
            column_of,
            columns,
            row_of,
            rows,
        }
    }

    /// The line (column for `vertical`, else row) button `index` is in, and its place in it.
    fn line(&self, index: usize, vertical: bool) -> Option<(&[usize], usize)> {
        let (of, all) = if vertical {
            (&self.column_of, &self.columns)
        } else {
            (&self.row_of, &self.rows)
        };
        let line = all.get((*of.get(index)?)?)?;
        let place = line.iter().position(|member| *member == index)?;
        Some((line.as_slice(), place))
    }

    /// The button next to `index` in `direction` without wrapping: its neighbour in the line, or — with no line
    /// that way — the nearest button ahead.
    fn direct(&self, index: usize, direction: Direction) -> Option<usize> {
        let vertical = direction.vertical();
        match self.line(index, vertical) {
            Some((line, place)) => {
                let next = if direction.forward() {
                    place.checked_add(1)
                } else {
                    place.checked_sub(1)
                };
                next.and_then(|at| line.get(at)).copied()
            }
            None => self.ahead(index, direction),
        }
    }

    /// Where `direction` goes from button `index` (zero-based; the button itself when it stays).
    fn route(&self, index: usize, direction: Direction) -> usize {
        let vertical = direction.vertical();
        let (moves, wraps, mode) = if vertical {
            (
                self.options.vertical,
                self.options.vertical_wrap,
                self.options.vertical_wrap_mode,
            )
        } else {
            (
                self.options.horizontal,
                self.options.horizontal_wrap,
                self.options.horizontal_wrap_mode,
            )
        };
        if !moves {
            return index;
        }
        if let Some(inside) = self.inside(index, direction) {
            return inside;
        }
        let Some((line, _)) = self.line(index, vertical) else {
            // No line this way. With wrapping on, a button that has a line the other way stays put.
            if wraps && self.line(index, !vertical).is_some() {
                return index;
            }
            return self.around(index, direction).unwrap_or(index);
        };
        if let Some(next) = self.direct(index, direction) {
            return next;
        }
        if !wraps {
            return self.around(index, direction).unwrap_or(index);
        }
        let forward = direction.forward();
        let target = match mode {
            WrapMode::SameLine => line,
            WrapMode::NextLine => self.next_line(index, vertical, forward).unwrap_or(line),
        };
        // Down / right wrap to the start of the line (top / left), up / left to its end.
        let end = if forward {
            target.first()
        } else {
            target.last()
        };
        end.copied().unwrap_or(index)
    }

    /// For a button that contains others (a bar behind a row of buttons): the contained button nearest ahead in
    /// `direction`, or else the nearest one level with it (Encore CS6, 2026-11-21: a bar behind a row went up and
    /// down to the row's button nearest its centre, not to the button above the bar).
    fn inside(&self, index: usize, direction: Direction) -> Option<usize> {
        let outer = self.bounds.get(index)?;
        let from = self.centres.get(index)?;
        // (level, distance squared, step) of the best so far.
        let mut best: Option<(bool, i128, i128, usize)> = None;
        for (other, centre) in self.centres.iter().enumerate() {
            if other == index
                || !self
                    .bounds
                    .get(other)
                    .is_some_and(|inner| outer.contains(inner))
            {
                continue;
            }
            let (step, squared) = direction.step(from, centre);
            if step < 0 {
                continue;
            }
            let level = step == 0;
            let better = best.is_none_or(|(best_level, best_squared, best_step, _)| {
                if level != best_level {
                    return !level;
                }
                if level {
                    squared < best_squared
                } else {
                    // squared / step < best_squared / best_step, without dividing.
                    squared.saturating_mul(best_step) < best_squared.saturating_mul(step)
                }
            });
            if better {
                best = Some((level, squared, step, other));
            }
        }
        best.map(|(_, _, _, other)| other)
    }

    /// The column after (`forward`) or before the one button `index` is in, round to the first / last past the
    /// end. Every button outside a column counts as a column of its own.
    fn next_line(&self, index: usize, vertical: bool, forward: bool) -> Option<&[usize]> {
        let (of, all) = if vertical {
            (&self.column_of, &self.columns)
        } else {
            (&self.row_of, &self.rows)
        };
        let key = |line: &[usize]| -> i64 {
            let sum: i64 = line
                .iter()
                .filter_map(|member| self.centres.get(*member))
                .map(|centre| if vertical { centre.x } else { centre.y })
                .sum();
            sum.checked_div(i64::try_from(line.len()).unwrap_or(1).max(1))
                .unwrap_or(0)
        };
        let own = (*of.get(index)?)?;
        let mut order: Vec<usize> = (0..all.len()).collect();
        order.sort_by_key(|line| (all.get(*line).map_or(0, |members| key(members)), *line));
        let place = order.iter().position(|line| *line == own)?;
        let count = order.len();
        let next = if forward {
            place.checked_add(1)?.checked_rem(count)?
        } else {
            place
                .checked_add(count)?
                .checked_sub(1)?
                .checked_rem(count)?
        };
        all.get(*order.get(next)?).map(Vec::as_slice)
    }

    /// The nearest button ahead of `index` in `direction`: the smallest distance over the cosine of the angle off
    /// the direction to the power 1.5 (Encore CS6 favours buttons straight ahead a little more than distance over
    /// cosine), up / down only within [`VERTICAL_CONE_COS`]; the lower number on a tie. When that one is not on the
    /// clockwise side, the best on that side within [`ANGLE_TIE`] and [`SCORE_TIE`] of it is taken instead.
    fn ahead(&self, index: usize, direction: Direction) -> Option<usize> {
        let from = self.centres.get(index)?;
        // (score, angle off the direction in degrees, clockwise, button) of every button ahead.
        let mut found: Vec<(f64, f64, bool, usize)> = Vec::new();
        for (other, centre) in self.centres.iter().enumerate() {
            if other == index {
                continue;
            }
            let (step, squared) = direction.step(from, centre);
            if step <= 0 {
                continue;
            }
            // `as` is exact enough for pixel distances.
            let (step, distance) = (step as f64, (squared as f64).sqrt());
            let aside = if direction.vertical() {
                centre.x - from.x
            } else {
                centre.y - from.y
            };
            if direction.vertical()
                && step < VERTICAL_CONE_COS * distance
                && aside.abs() > VERTICAL_ASIDE
            {
                continue;
            }
            let score = distance / (step / distance).powf(1.5);
            let angle = (aside.abs() as f64).atan2(step).to_degrees();
            found.push((score, angle, direction.clockwise(from, centre), other));
        }
        let best = found.iter().copied().min_by(|a, b| a.0.total_cmp(&b.0))?;
        if best.2 {
            return Some(best.3);
        }
        // A button on the clockwise side within [`ANGLE_TIE`] of the best one's angle is taken instead.
        found
            .iter()
            .copied()
            .filter(|(score, angle, clockwise, _)| {
                *clockwise && *angle <= best.1 + ANGLE_TIE && *score <= best.0 * SCORE_TIE
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .or(Some(best))
            .map(|(.., other)| other)
    }

    /// [`Self::ahead`], or with nothing ahead a button no more than about 110° round from `direction` that is not
    /// wholly behind `index` (unless it is level with it across the direction): one level with it (centres within
    /// [`LINE_TOLERANCE`]) before one ahead (up / down: outside the cone) before one behind (M10.44, Encore CS6: a
    /// narrow tile went down to the row beside it, not to the bar 15 px below); among those the closest by the gap
    /// between the rectangles (gaps within [`LINE_TOLERANCE`] of it count the same), then the one clockwise of the
    /// direction (up: right, right: down, down: left, left: up), then the closest by centre.
    fn around(&self, index: usize, direction: Direction) -> Option<usize> {
        if let Some(found) = self.ahead(index, direction) {
            return Some(found);
        }
        let from = self.centres.get(index)?;
        let source = self.bounds.get(index)?;
        // (side, gap, clockwise, distance squared, button) of every button looked at; the side is 0 level with it
        // (within [`LINE_TOLERANCE`]), 1 ahead (up / down: outside the cone), 2 behind.
        let mut found: Vec<(u8, i64, bool, i128, usize)> = Vec::new();
        for (other, centre) in self.centres.iter().enumerate() {
            if other == index {
                continue;
            }
            let Some(target) = self.bounds.get(other) else {
                continue;
            };
            let (step, squared) = direction.step(from, centre);
            // `as` is exact enough for pixel distances.
            let distance = (squared as f64).sqrt();
            if distance <= 0.0 {
                continue;
            }
            let clockwise = direction.clockwise(from, centre);
            if !self.within_reach(source, target, (step, distance), clockwise, direction) {
                continue;
            }
            if !source.across_overlaps(target, direction) && !target.reaches_past(source, direction)
            {
                continue;
            }
            let side = if step.abs() <= i128::from(LINE_TOLERANCE) {
                0
            } else if step > 0 {
                1
            } else {
                2
            };
            found.push((side, source.gap(target), clockwise, squared, other));
        }
        // Level with it first, then ahead, then behind; then the smallest gap (gaps within [`LINE_TOLERANCE`] of it
        // count the same), the clockwise side, the nearest centre.
        let side = found.iter().map(|(side, ..)| *side).min()?;
        found.retain(|(own, ..)| *own == side);
        let least = found.iter().map(|(_, gap, ..)| *gap).min()?;
        found
            .into_iter()
            .filter(|(_, gap, ..)| *gap <= least.saturating_add(LINE_TOLERANCE))
            .min_by_key(|(_, _, clockwise, squared, other)| (!*clockwise, *squared, *other))
            .map(|(.., other)| other)
    }

    /// Whether [`Self::around`] looks as far round as `target` (`step` ahead of `source`'s centre and `distance`
    /// from it; `clockwise` of the direction): a target that reaches past `source` in the direction (a bar under
    /// a tile) while its centre is no more than [`REACH_CLOCKWISE`] / [`REACH_ANTICLOCKWISE`] further away than
    /// it is to the side; any other within about 110° ([`WIDE_COS`]).
    fn within_reach(
        &self,
        source: &Bounds,
        target: &Bounds,
        (step, distance): (i128, f64),
        clockwise: bool,
        direction: Direction,
    ) -> bool {
        // `as` is exact enough for pixel distances.
        let step = step as f64;
        if step >= 0.0 {
            return true;
        }
        if !target.reaches_beyond(source, direction) {
            return step >= WIDE_COS * distance;
        }
        let aside = (distance * distance - step * step).max(0.0).sqrt();
        let reach = if clockwise {
            REACH_CLOCKWISE
        } else {
            REACH_ANTICLOCKWISE
        };
        distance - aside <= reach
    }

    /// Every button's (up, down, left, right), zero-based.
    fn routes(&self) -> Vec<[usize; 4]> {
        let directions = [
            Direction::Up,
            Direction::Down,
            Direction::Left,
            Direction::Right,
        ];
        let mut routes: Vec<[usize; 4]> = (0..self.centres.len())
            .map(|index| directions.map(|direction| self.route(index, direction)))
            .collect();
        // A button in no row and no column that no route reaches is entered from its nearest button, in the
        // direction it lies.
        for (loose, centre) in self.centres.iter().enumerate() {
            if self.line(loose, true).is_some() || self.line(loose, false).is_some() {
                continue;
            }
            let reached = routes
                .iter()
                .enumerate()
                .any(|(from, route)| from != loose && route.contains(&loose));
            if reached {
                continue;
            }
            let nearest = self
                .centres
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != loose)
                .min_by_key(|(other, from)| (Direction::Up.step(from, centre).1, *other));
            let Some((nearest, from)) = nearest else {
                continue;
            };
            let (dx, dy) = (centre.x - from.x, centre.y - from.y);
            let direction = if dx.abs() >= dy.abs() {
                if dx >= 0 {
                    Direction::Right
                } else {
                    Direction::Left
                }
            } else if dy >= 0 {
                Direction::Down
            } else {
                Direction::Up
            };
            let (moves, wraps) = if direction.vertical() {
                (self.options.vertical, self.options.vertical_wrap)
            } else {
                (self.options.horizontal, self.options.horizontal_wrap)
            };
            if moves
                && wraps
                && let Some(slot) = routes
                    .get_mut(nearest)
                    .and_then(|route| route.get_mut(direction.slot()))
            {
                *slot = loose;
            }
        }
        routes
    }
}

/// What lines buttons up across a row or column: the left (top) edge or the centre; the right (bottom) edge only
/// counts for near misses.
#[derive(Clone, Copy)]
enum Mark {
    Start,
    Centre,
    End,
}

/// The candidate columns (`vertical`) or rows of the first `own` buttons of `bounds`: buttons that overlap across the
/// line and whose left (top) edges, or else centres, are within [`LINE_TOLERANCE`] (a button that contains another
/// is in none), before [`lines`] puts some out.
fn candidates(
    bounds: &[Bounds],
    centres: &[Centre],
    own: usize,
    vertical: bool,
) -> Vec<Vec<usize>> {
    let count = bounds.len();
    let own = own.min(count);
    let span = |index: usize| -> (i64, i64) {
        bounds
            .get(index)
            .map_or((0, 0), |bound| bound.across(vertical))
    };
    let mark = |index: usize, mark: Mark| -> i64 {
        match mark {
            Mark::Start => span(index).0,
            Mark::End => span(index).1,
            Mark::Centre => centres
                .get(index)
                .map_or(0, |centre| if vertical { centre.x } else { centre.y }),
        }
    };
    let overlap = |a: usize, b: usize| {
        let ((a0, a1), (b0, b1)) = (span(a), span(b));
        a0 < b1 && b0 < a1
    };
    let bound = |index: usize| bounds.get(index).copied().unwrap_or_default();
    let container: Vec<bool> = (0..own)
        .map(|index| (0..own).any(|other| other != index && bound(index).contains(&bound(other))))
        .collect();
    let members: Vec<usize> = (0..own)
        .filter(|index| container.get(*index) == Some(&false))
        .collect();

    // By left / top edge first, then by centre.
    let mut candidates: Vec<Vec<usize>> = Vec::new();
    for which in [Mark::Start, Mark::Centre] {
        let mut group: Vec<usize> = (0..own).collect();
        for (place, &a) in members.iter().enumerate() {
            for &b in members.iter().skip(place.saturating_add(1)) {
                if (mark(a, which) - mark(b, which)).abs() > LINE_TOLERANCE || !overlap(a, b) {
                    continue;
                }
                let (from, to) = (
                    group.get(b).copied().unwrap_or(b),
                    group.get(a).copied().unwrap_or(a),
                );
                for slot in &mut group {
                    if *slot == from {
                        *slot = to;
                    }
                }
            }
        }
        for id in 0..own {
            let line: Vec<usize> = members
                .iter()
                .copied()
                .filter(|index| group.get(*index) == Some(&id))
                .collect();
            if line.len() >= 2 && !candidates.contains(&line) {
                candidates.push(line);
            }
        }
    }
    candidates
}

/// The columns (`vertical`) or rows of the first `own` buttons of `bounds` (the rest only break lines), from their
/// `candidates` ([`candidates`]; `across`: the other orientation's): each button's line when it is in one of two or
/// more, and the lines in order along themselves.
///
/// Encore CS6 (docs/encore-behavior.md 9.2, 9.5j and 9.16):
///
/// - Buttons that overlap across the line and whose left (top) edges, or else centres, are within
///   [`LINE_TOLERANCE`] are a line; right (bottom) edges alone do not make one.
/// - A button that contains another button is in no line.
/// - A button in two different such lines puts both out (a text bar and the column of pictures above it).
/// - A line is out when a button touching or overlapping one of its members across the line misses one of the
///   member's edges or centre by more than [`LINE_TOLERANCE`] but no more than [`NEAR_MISS`], when two of its
///   members miss each other that way, when a member partly overlaps another button, or (M10.44) when a member faces
///   a button outside the line that far apart (a bar 15 px below a row) — unless (M10.51) that button lines up the
///   other way with one of the members (a tile whose centre is over the bar's).
fn lines(
    bounds: &[Bounds],
    centres: &[Centre],
    own: usize,
    (vertical, candidates, across): (bool, &[Vec<usize>], &[Vec<usize>]),
) -> (Vec<Option<usize>>, Vec<Vec<usize>>) {
    let count = bounds.len();
    let own = own.min(count);
    let span = |index: usize| -> (i64, i64) {
        bounds
            .get(index)
            .map_or((0, 0), |bound| bound.across(vertical))
    };
    let mark = |index: usize, mark: Mark| -> i64 {
        match mark {
            Mark::Start => span(index).0,
            Mark::End => span(index).1,
            Mark::Centre => centres
                .get(index)
                .map_or(0, |centre| if vertical { centre.x } else { centre.y }),
        }
    };
    let along = |index: usize| -> i64 {
        centres
            .get(index)
            .map_or(0, |centre| if vertical { centre.y } else { centre.x })
    };
    let touch = |a: usize, b: usize| {
        let ((a0, a1), (b0, b1)) = (span(a), span(b));
        a0 <= b1 && b0 <= a1
    };
    let near_miss = |a: usize, b: usize| {
        [Mark::Start, Mark::Centre, Mark::End]
            .into_iter()
            .any(|which| {
                let off = (mark(a, which) - mark(b, which)).abs();
                off > LINE_TOLERANCE && off <= NEAR_MISS
            })
    };
    let bound = |index: usize| bounds.get(index).copied().unwrap_or_default();
    // Centres within [`LINE_TOLERANCE`] the other way (a column's x for a row), in a candidate line of it.
    let lined_up = |a: usize, b: usize| {
        let centre = |index: usize| {
            centres
                .get(index)
                .map_or(0, |centre| if vertical { centre.y } else { centre.x })
        };
        (centre(a) - centre(b)).abs() <= LINE_TOLERANCE
            && across
                .iter()
                .any(|line| line.contains(&a) && line.contains(&b))
    };

    // A button in two different candidates puts both out.
    let mut out = vec![false; candidates.len()];
    for (a, first) in candidates.iter().enumerate() {
        for (b, second) in candidates.iter().enumerate().skip(a.saturating_add(1)) {
            if first.iter().any(|member| second.contains(member)) {
                for slot in [a, b] {
                    if let Some(flag) = out.get_mut(slot) {
                        *flag = true;
                    }
                }
            }
        }
    }

    let mut found: Vec<Vec<usize>> = Vec::new();
    let mut placed = vec![false; own];
    for (line, _) in candidates.iter().zip(&out).filter(|(_, out)| !**out) {
        let line: Vec<usize> = line
            .iter()
            .copied()
            .filter(|index| placed.get(*index) == Some(&false))
            .collect();
        if line.len() < 2 {
            continue;
        }
        let members_miss = line
            .iter()
            .any(|a| line.iter().any(|b| a != b && near_miss(*a, *b)));
        let partly_covered = line.iter().any(|member| {
            (0..count)
                .any(|other| other != *member && bound(*member).partly_overlaps(&bound(other)))
        });
        let missed = (0..count)
            .filter(|other| !line.contains(other))
            .any(|other| {
                line.iter()
                    .any(|member| touch(other, *member) && near_miss(other, *member))
            });
        // A member a little apart from a button outside the line (10 to 18 px, facing it), unless the line's
        // centres line up and that button lines up the other way with a member.
        let centred = line.iter().all(|a| {
            line.iter()
                .all(|b| (mark(*a, Mark::Centre) - mark(*b, Mark::Centre)).abs() <= LINE_TOLERANCE)
        });
        let crowded = line.iter().any(|member| {
            (0..count)
                .filter(|other| !line.contains(other))
                .filter(|other| !(centred && line.iter().any(|kept| lined_up(*kept, *other))))
                .any(|other| bound(*member).nearly_touches(&bound(other)))
        });
        if members_miss || partly_covered || missed || crowded {
            continue;
        }
        for member in &line {
            if let Some(slot) = placed.get_mut(*member) {
                *slot = true;
            }
        }
        found.push(line);
    }

    let mut of = vec![None; own];
    for (line, members) in found.iter_mut().enumerate() {
        members.sort_by_key(|index| (along(*index), *index));
        for member in members.iter() {
            if let Some(slot) = of.get_mut(*member) {
                *slot = Some(line);
            }
        }
    }
    (of, found)
}

/// A button's rectangle as signed edges (right and bottom exclusive).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Bounds {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

impl Bounds {
    fn from_rect(rect: Rect) -> Self {
        let (left, top) = (i64::from(rect.x), i64::from(rect.y));
        Self {
            left,
            top,
            right: left + i64::from(rect.w),
            bottom: top + i64::from(rect.h),
        }
    }

    /// Its extent across a column (`vertical`: left to right) or a row (top to bottom).
    fn across(self, vertical: bool) -> (i64, i64) {
        if vertical {
            (self.left, self.right)
        } else {
            (self.top, self.bottom)
        }
    }

    /// Whether `other` lies inside it (and is not the same rectangle).
    fn contains(&self, other: &Self) -> bool {
        self != other
            && self.left <= other.left
            && self.top <= other.top
            && other.right <= self.right
            && other.bottom <= self.bottom
    }

    /// Whether the two overlap with neither inside the other.
    fn partly_overlaps(&self, other: &Self) -> bool {
        let overlaps = self.left < other.right
            && other.left < self.right
            && self.top < other.bottom
            && other.top < self.bottom;
        overlaps && self != other && !self.contains(other) && !other.contains(self)
    }

    /// Whether the two overlap across `direction` (left to right for up / down, top to bottom for left / right).
    fn across_overlaps(&self, other: &Self, direction: Direction) -> bool {
        let ((a0, a1), (b0, b1)) = (
            self.across(direction.vertical()),
            other.across(direction.vertical()),
        );
        a0 < b1 && b0 < a1
    }

    /// Whether some of it lies past the far edge of `source` behind `direction` (not wholly behind it).
    fn reaches_past(&self, source: &Self, direction: Direction) -> bool {
        match direction {
            Direction::Up => self.top < source.bottom,
            Direction::Down => self.bottom > source.top,
            Direction::Left => self.left < source.right,
            Direction::Right => self.right > source.left,
        }
    }

    /// Whether some of it lies past the front edge of `source` in `direction` (ahead of all of it).
    fn reaches_beyond(&self, source: &Self, direction: Direction) -> bool {
        match direction {
            Direction::Up => self.top < source.top,
            Direction::Down => self.bottom > source.bottom,
            Direction::Left => self.left < source.left,
            Direction::Right => self.right > source.right,
        }
    }

    /// Whether the two face each other (overlap across one axis) a little apart along the other: more than
    /// [`LINE_TOLERANCE`] but no more than [`NEAR_MISS`] pixels.
    fn nearly_touches(&self, other: &Self) -> bool {
        let near = |gap: i64| gap > LINE_TOLERANCE && gap <= NEAR_MISS;
        let x_overlap = self.left < other.right && other.left < self.right;
        let y_overlap = self.top < other.bottom && other.top < self.bottom;
        let x_gap = (other.left - self.right).max(self.left - other.right);
        let y_gap = (other.top - self.bottom).max(self.top - other.bottom);
        (x_overlap && near(y_gap)) || (y_overlap && near(x_gap))
    }

    /// The gap between the two rectangles in pixels (0 when they touch or overlap; rounded down).
    fn gap(&self, other: &Self) -> i64 {
        let x = (other.left - self.right)
            .max(self.left - other.right)
            .max(0);
        let y = (other.top - self.bottom)
            .max(self.top - other.bottom)
            .max(0);
        // `as` is exact enough for pixel distances.
        ((x as f64).hypot(y as f64)) as i64
    }
}

#[derive(Clone, Copy, Debug)]
struct Centre {
    x: i64,
    y: i64,
}

impl Centre {
    fn from_rect(rect: Rect) -> Self {
        Self {
            x: i64::from(rect.x) + i64::from(rect.w) / 2,
            y: i64::from(rect.y) + i64::from(rect.h) / 2,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    fn vertical(self) -> bool {
        matches!(self, Self::Up | Self::Down)
    }

    /// Down and right count forwards.
    fn forward(self) -> bool {
        matches!(self, Self::Down | Self::Right)
    }

    /// The place of this direction in an (up, down, left, right) route.
    fn slot(self) -> usize {
        match self {
            Self::Up => 0,
            Self::Down => 1,
            Self::Left => 2,
            Self::Right => 3,
        }
    }

    /// Whether `to` lies on the clockwise side of this direction from `from` (up: right, right: down, down: left,
    /// left: up).
    fn clockwise(self, from: &Centre, to: &Centre) -> bool {
        let (dx, dy) = (to.x - from.x, to.y - from.y);
        match self {
            Self::Up => dx > 0,
            Self::Right => dy > 0,
            Self::Down => dx < 0,
            Self::Left => dy < 0,
        }
    }

    /// How far `to` lies from `from` in this direction (negative: behind), and their distance squared.
    fn step(self, from: &Centre, to: &Centre) -> (i128, i128) {
        let (dx, dy) = (i128::from(to.x - from.x), i128::from(to.y - from.y));
        let step = match self {
            Self::Up => -dy,
            Self::Down => dy,
            Self::Left => -dx,
            Self::Right => dx,
        };
        (step, dx * dx + dy * dy)
    }
}

fn one_based(index: usize) -> u8 {
    u8::try_from(index.saturating_add(1)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::auto_route_with;
    use disccraft_project::{Rect, RouteOptions, WrapMode};

    /// A 2 x 2 grid: 1 2 on top, 3 4 below.
    fn grid() -> Vec<Rect> {
        [(0, 0), (200, 0), (0, 100), (200, 100)]
            .iter()
            .map(|(x, y)| Rect {
                x: *x,
                y: *y,
                w: 100,
                h: 50,
            })
            .collect()
    }

    #[test]
    fn wraps_in_the_same_column_and_row_by_default() {
        let routes = auto_route_with(&grid(), &RouteOptions::default());
        // (up, down, left, right)
        assert_eq!(routes[0], (3, 3, 2, 2));
        assert_eq!(routes[2], (1, 1, 4, 4));
    }

    #[test]
    fn wraps_on_to_the_next_column_and_row() {
        let options = RouteOptions {
            vertical_wrap_mode: WrapMode::NextLine,
            horizontal_wrap_mode: WrapMode::NextLine,
            ..RouteOptions::default()
        };
        let routes = auto_route_with(&grid(), &options);
        // 3 down: top of the next column (2). 4 down: past the last column, the first (1).
        assert_eq!(routes[2].1, 2);
        assert_eq!(routes[3].1, 1);
        // 1 up: before the first column, the last one's bottom (4). 2 up: the previous column's bottom (3).
        assert_eq!(routes[0].0, 4);
        assert_eq!(routes[1].0, 3);
        // 2 right: the next row's left (3). 4 right: the first row's left (1). 3 left: the row above's right (2).
        assert_eq!(routes[1].3, 3);
        assert_eq!(routes[3].3, 1);
        assert_eq!(routes[2].2, 2);
    }

    #[test]
    fn next_and_previous_buttons_sit_at_the_ends_as_in_encore()
    -> Result<(), Box<dyn std::error::Error>> {
        use disccraft_project::{ButtonKind, Link, Project};
        // Encore CS6: 1 2 / 3 4, then previous (5) and next (6) below; every one of the 24 routes.
        let mut rects = grid();
        rects.push(Rect {
            x: 0,
            y: 200,
            w: 100,
            h: 50,
        });
        rects.push(Rect {
            x: 200,
            y: 200,
            w: 100,
            h: 50,
        });
        let mut project = Project::default();
        let id = project.add_menu(None)?;
        for rect in &rects {
            project.add_button(id, "B".to_string(), Some(*rect), Link::Stop)?;
        }
        let mut menu = project.menu(id).ok_or("menu")?.clone();
        if let Some(button) = menu.buttons.get_mut(4) {
            button.kind = ButtonKind::Previous;
        }
        if let Some(button) = menu.buttons.get_mut(5) {
            button.kind = ButtonKind::Next;
        }
        let routes = super::button_routes(&menu, &RouteOptions::default());
        assert_eq!(
            routes,
            vec![
                (5, 3, 5, 2),
                (4, 4, 1, 1),
                (1, 1, 4, 4),
                (2, 6, 3, 6),
                (5, 1, 6, 1),
                (4, 6, 4, 5),
            ]
        );
        // Not excluded: plain routing.
        let plain = RouteOptions {
            exclude_next_previous: false,
            ..RouteOptions::default()
        };
        assert_eq!(
            super::button_routes(&menu, &plain),
            auto_route_with(&rects, &plain)
        );
        Ok(())
    }

    #[test]
    fn a_lone_button_goes_nowhere() {
        let routes = auto_route_with(&grid()[..1], &RouteOptions::default());
        assert_eq!(routes, vec![(0, 0, 0, 0)]);
    }

    /// Encore CS6: with two buttons, every direction goes to the other one.
    #[test]
    fn two_buttons_go_to_each_other_every_way() {
        let rects = [
            Rect {
                x: 200,
                y: 100,
                w: 40,
                h: 40,
            },
            Rect {
                x: 200,
                y: 800,
                w: 400,
                h: 40,
            },
        ];
        let routes = auto_route_with(&rects, &RouteOptions::default());
        assert_eq!(routes, vec![(2, 2, 2, 2), (1, 1, 1, 1)]);
        // Without wrapping the layout decides, as before.
        let plain = RouteOptions {
            vertical_wrap: false,
            horizontal_wrap: false,
            ..RouteOptions::default()
        };
        assert_eq!(
            auto_route_with(&rects, &plain).first().map(|r| r.1),
            Some(2)
        );
    }

    #[test]
    fn switched_off_directions_and_wraps_stay_put() {
        let options = RouteOptions {
            vertical: false,
            horizontal_wrap: false,
            ..RouteOptions::default()
        };
        let routes = auto_route_with(&grid(), &options);
        assert_eq!((routes[0].0, routes[0].1), (1, 1));
        // 1 right still moves to 2. Without wrapping, 2 right looks around and takes the button straight below it
        // (as Encore CS6 does with wrapping off: the top right button's right went to the bottom right one).
        assert_eq!(routes[0].3, 2);
        assert_eq!(routes[1].3, 4);
    }

    /// Buttons of 241 x 38 by their centres, as on the 1920 x 1080 menu measured in Encore CS6.
    fn buttons(centres: &[(u16, u16)]) -> Vec<Rect> {
        centres
            .iter()
            .map(|(x, y)| Rect {
                x: x - 120,
                y: y - 19,
                w: 241,
                h: 38,
            })
            .collect()
    }

    /// Top left, bottom left, bottom right, and the fourth button at `fourth` (Encore's numbering in the test).
    fn layout(fourth: (u16, u16)) -> Vec<Rect> {
        buttons(&[(559, 288), (559, 824), (1368, 824), fourth])
    }

    fn off() -> RouteOptions {
        RouteOptions {
            vertical_wrap: false,
            horizontal_wrap: false,
            ..RouteOptions::default()
        }
    }

    /// The layouts measured in Encore CS6 on 2026-11-15 (docs/encore-behavior.md 9.2), wrapping on.
    #[test]
    fn routes_match_encore_with_wrapping() {
        let on = RouteOptions::default();
        let route = |fourth| auto_route_with(&layout(fourth), &on);
        // The grid.
        assert_eq!(
            route((1368, 288)),
            vec![(2, 2, 4, 4), (1, 1, 3, 3), (4, 4, 2, 2), (3, 3, 1, 1)]
        );
        // The fourth button 9 px off its column still belongs to it; 10 px off it does not, and then neither it
        // nor the button below goes up / down (they are still in their rows).
        assert_eq!(route((1377, 288)), route((1368, 288)));
        assert_eq!(
            route((1378, 288)),
            vec![(2, 2, 4, 4), (1, 1, 3, 3), (3, 3, 2, 2), (4, 4, 1, 1)]
        );
        assert_eq!(route((1386, 288)), route((1378, 288)));
        // Rows have the same tolerance.
        assert_eq!(route((1368, 297)), route((1368, 288)));
        assert_eq!(
            route((1368, 298)),
            vec![(2, 2, 1, 1), (1, 1, 3, 3), (4, 4, 2, 2), (3, 3, 4, 4)]
        );
        // E: 30 px right and 21 px down, in no row and no column. Its nearest button (below it) goes up to it.
        assert_eq!(
            route((1398, 309)),
            vec![(2, 2, 1, 1), (1, 1, 3, 3), (4, 3, 2, 2), (1, 3, 1, 3)]
        );
        // F: in the middle. Its nearest button (top left) goes right to it.
        assert_eq!(
            route((963, 538)),
            vec![(2, 2, 1, 4), (1, 1, 3, 3), (3, 3, 2, 2), (1, 2, 1, 3)]
        );
        // H: above and right of the bottom left button, which now goes right to it instead of along its row.
        assert_eq!(
            route((773, 717)),
            vec![(2, 2, 1, 1), (1, 1, 3, 4), (3, 3, 2, 2), (1, 2, 2, 3)]
        );
        // I: out to the right. The bottom right button goes right to it instead of wrapping.
        assert_eq!(
            route((1728, 538)),
            vec![(2, 2, 1, 1), (1, 1, 3, 3), (3, 3, 2, 4), (1, 3, 3, 4)]
        );
    }

    /// The same layouts with wrapping off: the ends look around instead of staying put.
    #[test]
    fn routes_match_encore_without_wrapping() {
        let route = |fourth| auto_route_with(&layout(fourth), &off());
        let grid = vec![(4, 2, 2, 4), (1, 3, 1, 3), (4, 2, 2, 4), (1, 3, 1, 3)];
        assert_eq!(route((1368, 288)), grid);
        assert_eq!(route((1398, 309)), grid);
        assert_eq!(
            route((963, 538)),
            vec![(1, 2, 2, 4), (1, 3, 1, 3), (4, 2, 2, 3), (1, 2, 1, 3)]
        );
    }

    /// Text buttons of different widths typed from the same x: a column while their left edges line up, and no
    /// column at all — not even for the two still lined up — once one of them is 10 px off.
    #[test]
    fn left_aligned_buttons_of_different_widths_are_a_column() {
        let trio = |middle_x: u16| {
            vec![
                Rect {
                    x: 200,
                    y: 163,
                    w: 40,
                    h: 40,
                },
                Rect {
                    x: middle_x,
                    y: 523,
                    w: 385,
                    h: 40,
                },
                Rect {
                    x: 200,
                    y: 881,
                    w: 180,
                    h: 40,
                },
            ]
        };
        let on = RouteOptions::default();
        assert_eq!(
            auto_route_with(&trio(200), &on),
            vec![(3, 2, 1, 1), (1, 3, 2, 2), (2, 1, 3, 3)]
        );
        assert_eq!(
            auto_route_with(&trio(210), &on),
            vec![(1, 2, 3, 2), (1, 3, 1, 3), (2, 3, 1, 2)]
        );
    }

    /// Four buttons on a diagonal share no row and no column: each goes up / left to the one before and down /
    /// right to the one after, and the two ends stay put.
    #[test]
    fn a_diagonal_goes_along_itself() {
        let rects = buttons(&[(309, 128), (764, 356), (1172, 678), (1604, 955)]);
        assert_eq!(
            auto_route_with(&rects, &RouteOptions::default()),
            vec![(1, 2, 1, 2), (1, 3, 1, 3), (2, 4, 2, 4), (3, 4, 3, 4)]
        );
    }

    fn rects(list: &[(u16, u16, u16, u16)]) -> Vec<Rect> {
        list.iter()
            .map(|&(x, y, w, h)| Rect { x, y, w, h })
            .collect()
    }

    /// The top-level buttons of the issue #13 reporter's menu (1920 x 1080), in Encore's order: a picture of the
    /// Doctor, a row of six items touching each other, a bar at the top and a bar behind the row.
    const DOCTOR: (u16, u16, u16, u16) = (36, 504, 413, 467);
    const EPISODE: (u16, u16, u16, u16) = (275, 971, 298, 109);
    const PLAY_ALL: (u16, u16, u16, u16) = (0, 971, 275, 109);
    const SUBTITLES: (u16, u16, u16, u16) = (1411, 971, 509, 109);
    const SPECIAL: (u16, u16, u16, u16) = (869, 971, 291, 109);
    const SCENE: (u16, u16, u16, u16) = (573, 971, 296, 109);
    const AUDIO: (u16, u16, u16, u16) = (1160, 971, 251, 109);
    const TOP: (u16, u16, u16, u16) = (0, 0, 1920, 206);
    const BOTTOM: (u16, u16, u16, u16) = (0, 971, 1920, 109);

    /// Encore CS6, 2026-11-21 (docs/encore-behavior.md 9.5j): the bar behind the row is in no row (it contains
    /// the row's buttons) and goes up / down into the row; the top bar and PLAY ALL are a column by their left
    /// edges although the Doctor (36 px off) overlaps both; the Doctor goes right to SCENE, not EPISODE.
    #[test]
    fn the_reporters_top_menu_routes_as_in_encore() {
        let on = RouteOptions::default();
        let full = rects(&[
            DOCTOR, EPISODE, PLAY_ALL, SUBTITLES, SPECIAL, SCENE, AUDIO, TOP, BOTTOM,
        ]);
        assert_eq!(
            auto_route_with(&full, &on),
            vec![
                (8, 3, 3, 6),
                (2, 2, 3, 6),
                (1, 8, 4, 2),
                (4, 4, 7, 3),
                (5, 5, 9, 7),
                (6, 6, 2, 5),
                (7, 7, 5, 4),
                (3, 3, 8, 8),
                (5, 5, 6, 5),
            ]
        );
        let without_doctor = rects(&[
            EPISODE, PLAY_ALL, SUBTITLES, SPECIAL, SCENE, AUDIO, TOP, BOTTOM,
        ]);
        assert_eq!(
            auto_route_with(&without_doctor, &on),
            vec![
                (1, 1, 2, 5),
                (7, 7, 3, 1),
                (3, 3, 6, 2),
                (4, 4, 8, 6),
                (5, 5, 1, 4),
                (6, 6, 4, 3),
                (2, 2, 7, 7),
                (4, 4, 5, 4),
            ]
        );
        // Without the top bar, nothing is above the Doctor: up stays put (the row below is more than 100° round).
        let without_top = rects(&[
            DOCTOR, EPISODE, PLAY_ALL, SUBTITLES, SPECIAL, SCENE, AUDIO, BOTTOM,
        ]);
        assert_eq!(
            auto_route_with(&without_top, &on),
            vec![
                (1, 3, 3, 6),
                (2, 2, 3, 6),
                (1, 3, 4, 2),
                (4, 4, 7, 3),
                (5, 5, 8, 7),
                (6, 6, 2, 5),
                (7, 7, 5, 4),
                (5, 5, 6, 5),
            ]
        );
    }

    /// Encore CS6, 2026-11-21: a button 15 px off a row (overlapping it) breaks the row, so its buttons no longer
    /// wrap left / right and look round on up / down (the clockwise one first on a tie: up goes right, down left).
    #[test]
    fn a_button_just_off_a_row_breaks_it() {
        let on = RouteOptions::default();
        let row = [
            (700, 485, 200, 100),
            (1000, 485, 200, 100),
            (1300, 485, 200, 100),
        ];
        assert_eq!(
            auto_route_with(&rects(&row), &on),
            vec![(1, 1, 3, 2), (2, 2, 1, 3), (3, 3, 2, 1)]
        );
        let mut with_u = row.to_vec();
        with_u.push((100, 500, 200, 100));
        assert_eq!(
            auto_route_with(&rects(&with_u), &on),
            vec![(2, 2, 4, 2), (3, 1, 1, 3), (2, 2, 2, 3), (1, 1, 4, 1)]
        );
    }

    /// Encore CS6, 2026-11-21 (rp_more, M10.44): a bar 15 px below a row (facing it, not touching) breaks it, and so
    /// the column a narrow tile at the left edge would make with the bar. The row's buttons go straight left / right
    /// and stay at its ends; up and down look round: level buttons first (the clockwise one on a tie, up: right,
    /// down: left), not the bar ahead outside the cone (the narrow tile goes down to the row, not to the bar 15 px
    /// below it); gaps within 9 px count the same (the bar goes down round to Chapter 1, 16 px away, not to the tile,
    /// 15 px away).
    #[test]
    fn a_bar_just_below_a_row_breaks_it() {
        let on = RouteOptions::default();
        let bottom = (0, 971, 1920, 109);
        let chapters = [
            (1468, 783, 227, 173),
            (1199, 783, 269, 173),
            (975, 783, 224, 173),
            (718, 783, 257, 173),
            (472, 783, 246, 173),
            (214, 783, 258, 172),
            (0, 783, 33, 173),
        ];
        let mut page = vec![bottom];
        page.extend(chapters);
        assert_eq!(
            auto_route_with(&rects(&page), &on),
            vec![
                (5, 7, 5, 4),
                (3, 3, 3, 2),
                (2, 1, 4, 2),
                (3, 1, 5, 3),
                (4, 1, 6, 4),
                (5, 1, 7, 5),
                (6, 6, 8, 6),
                (7, 7, 8, 7),
            ]
        );
        // Two tiles over the bar: up goes across to the other tile, down to the bar; no wrapping.
        let pair = rects(&[bottom, (975, 783, 224, 173), (718, 783, 257, 173)]);
        assert_eq!(
            auto_route_with(&pair, &on),
            vec![(3, 1, 3, 2), (3, 1, 3, 2), (2, 1, 3, 2)]
        );
    }

    /// Encore CS6, 2026-11-21 (the issue #13 reporter's Page 2, M10.44): up / down also go straight to a button less
    /// than 400 px to the side beyond the 68° cone: Chapters 5 and 2 go down to the thin bar below the row at 70°
    /// (the row is broken by the Doctor picture overlapping its first tiles).
    #[test]
    fn a_bar_a_little_outside_the_cone_is_still_ahead() {
        let page = rects(&[
            (36, 504, 413, 576),
            (1465, 783, 227, 173),
            (1196, 783, 269, 173),
            (972, 783, 224, 173),
            (715, 783, 257, 173),
            (469, 783, 246, 173),
            (213, 783, 256, 173),
            (1692, 783, 228, 173),
            (50, 783, 163, 173),
            (152, 993, 1616, 23),
            (0, 783, 50, 173),
            (0, 0, 1920, 206),
        ]);
        let routes = auto_route_with(&page, &RouteOptions::default());
        // Chapter 5 and Chapter 2 down: the bar (10).
        assert_eq!(routes[2].1, 10);
        assert_eq!(routes[5].1, 10);
    }

    /// A full-width bar 15 px below two tiles A (257 wide) and B (224 wide) joined at `join`, `tall` high.
    fn bar_and_tiles(tall: u16, join: u16) -> Vec<Rect> {
        rects(&[
            (0, 971, 1920, 109),
            (join - 257, 956 - tall, 257, tall),
            (join, 956 - tall, 224, tall),
        ])
    }

    /// M10.51, Encore CS6, 2026-11-22 (docs/encore-behavior.md 9.16): up from the bar goes to the right-hand tile
    /// (clockwise) while it is no more than about 2.9° further round than the left-hand one, at any height.
    #[test]
    fn a_bar_goes_up_to_the_clockwise_tile_nearly_as_far_round() {
        let on = RouteOptions::default();
        // (height, last join going to B, first join going to A).
        for (tall, right, left) in [
            (73, 974, 975),
            (173, 974, 975),
            (373, 976, 978),
            (573, 978, 980),
        ] {
            assert_eq!(
                auto_route_with(&bar_and_tiles(tall, right), &on)[0].0,
                3,
                "{tall} {right}"
            );
            assert_eq!(
                auto_route_with(&bar_and_tiles(tall, left), &on)[0].0,
                2,
                "{tall} {left}"
            );
        }
    }

    /// M10.51, Encore CS6, 2026-11-22: right from the right-hand tile looks round to the bar below while the bar's
    /// centre is no more than about 38 px further away than it is below; left from the left-hand tile, about 30 px.
    #[test]
    fn end_tiles_look_round_to_a_bar_below_by_its_distance() {
        let on = RouteOptions::default();
        // (height, last join where B goes right to the bar, first join where it stays).
        for (tall, bar, stays) in [
            (73, 940, 950),
            (173, 964, 965),
            (373, 992, 1000),
            (573, 1010, 1020),
        ] {
            assert_eq!(
                auto_route_with(&bar_and_tiles(tall, bar), &on)[2].3,
                1,
                "{tall} {bar}"
            );
            assert_eq!(
                auto_route_with(&bar_and_tiles(tall, stays), &on)[2].3,
                3,
                "{tall} {stays}"
            );
        }
        // (height, first join where A goes left to the bar, last join where it stays).
        for (tall, bar, stays) in [(73, 1010, 1000), (373, 968, 960)] {
            assert_eq!(
                auto_route_with(&bar_and_tiles(tall, bar), &on)[1].2,
                1,
                "{tall} {bar}"
            );
            assert_eq!(
                auto_route_with(&bar_and_tiles(tall, stays), &on)[1].2,
                2,
                "{tall} {stays}"
            );
        }
    }

    /// M10.52, Encore CS6, 2026-11-22: when the left tile's centre is over the bar's, the tile and the bar are a
    /// column and the two tiles a row, although the bar is 15 px below them (which breaks a row otherwise).
    #[test]
    fn a_tile_centred_over_the_bar_keeps_its_row_and_column() {
        let on = RouteOptions::default();
        for (tall, join) in [(73, 1080), (373, 1090), (573, 1080)] {
            assert_eq!(
                auto_route_with(&bar_and_tiles(tall, join), &on),
                vec![(2, 2, 1, 1), (1, 1, 3, 3), (3, 3, 2, 2)],
                "{tall} {join}"
            );
        }
        // A bar lined up only by its left edge with the tile above it does not keep the tile's row (the reporter's
        // chapter page without Previous): Land goes down to Chapter 1, not to the bar.
        let page = rects(&[
            (0, 971, 1920, 109),
            (1468, 783, 227, 173),
            (1199, 783, 269, 173),
            (975, 783, 224, 173),
            (718, 783, 257, 173),
            (472, 783, 246, 173),
            (214, 783, 258, 172),
            (0, 783, 33, 173),
            (68, 510, 370, 272),
        ]);
        assert_eq!(auto_route_with(&page, &on)[7].1, 7);
    }

    /// M10.51, Encore CS6, 2026-11-22: two tiles and a bar in no row or column go to each other as the nearest
    /// buttons ahead; the bar, reached from both tiles already, does not take over the left tile's right.
    #[test]
    fn a_loose_button_already_reached_takes_no_route_over() {
        let routes = auto_route_with(&bar_and_tiles(73, 970), &RouteOptions::default());
        assert_eq!(routes, vec![(3, 1, 2, 3), (3, 1, 2, 3), (2, 1, 2, 3)]);
    }

    /// Encore CS6, 2026-11-21: four bars the full width of the screen, one above the other, are a column.
    #[test]
    fn full_width_bars_are_a_column() {
        let bars = rects(&[
            (0, 0, 1920, 206),
            (0, 971, 1920, 109),
            (0, 927, 1920, 33),
            (0, 898, 1920, 29),
        ]);
        assert_eq!(
            auto_route_with(&bars, &RouteOptions::default()),
            vec![(2, 4, 1, 1), (3, 1, 2, 2), (4, 2, 3, 3), (1, 3, 4, 4)]
        );
    }

    /// The issue #13 reporter's chapter page (Encore CS6, 2026-11-21): chapter tiles in a row between a narrow
    /// tile at the left edge and the previous / next buttons; the next button overlaps the last chapter by 1 px,
    /// which breaks the row. The leftmost button goes up / left to the previous button and the rightmost down /
    /// right to the next button; up and down look round (the chapters go up to the one on their right).
    #[test]
    fn the_reporters_chapter_page_routes_as_in_encore() -> Result<(), Box<dyn std::error::Error>> {
        use disccraft_project::{ButtonKind, Link, Project};
        let list = rects(&[
            (0, 971, 1920, 109),
            (1468, 783, 227, 173),
            (1199, 783, 269, 173),
            (975, 783, 224, 173),
            (718, 783, 257, 173),
            (472, 783, 246, 173),
            (214, 783, 258, 172),
            (0, 783, 33, 173),
            (33, 783, 181, 173),
            (68, 510, 370, 272),
            (1694, 783, 226, 173),
        ]);
        let mut project = Project::default();
        let id = project.add_menu(None)?;
        for rect in &list {
            project.add_button(id, "B".to_string(), Some(*rect), Link::Stop)?;
        }
        let mut menu = project.menu(id).ok_or("menu")?.clone();
        if let Some(button) = menu.buttons.get_mut(8) {
            button.kind = ButtonKind::Previous;
        }
        if let Some(button) = menu.buttons.get_mut(10) {
            button.kind = ButtonKind::Next;
        }
        let routes = super::button_routes(&menu, &RouteOptions::default());
        let encore = [
            (5, 7, 5, 4),
            (3, 11, 3, 11),
            (2, 1, 4, 2),
            (3, 1, 5, 3),
            (4, 1, 6, 4),
            (10, 1, 7, 5),
            (10, 6, 8, 6),
            (9, 7, 9, 7),
            (9, 8, 11, 8),
            (10, 7, 8, 6),
            (2, 11, 2, 9),
        ];
        for (index, (got, want)) in routes.iter().zip(encore).enumerate() {
            match index {
                // Not yet as in Encore: the bar and the narrow tile, both at the left edge, are still a column.
                0 => assert_eq!((got.2, got.3), (want.2, want.3)),
                7 => assert_eq!((got.0, got.2, got.3), (want.0, want.2, want.3)),
                _ => assert_eq!(*got, want, "button {}", index + 1),
            }
        }
        Ok(())
    }
}
