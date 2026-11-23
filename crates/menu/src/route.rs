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
/// button and the rightmost down / right to the next button, before anything else (without that button, as any
/// other: M10.54, the reporter's chapter page with only a next button). A previous button goes down /
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
                let to_previous =
                    index == first && matches!(direction, Direction::Up | Direction::Left);
                let to_next =
                    index == last && matches!(direction, Direction::Down | Direction::Right);
                if let (true, Some(previous)) = (to_previous, previous) {
                    one_based(previous)
                } else if let (true, Some(next)) = (to_next, next) {
                    one_based(next)
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
/// How much two buttons may have in common and still count as not touching or overlapping for [`lines`] (M10.56,
/// Encore CS6, 2026-11-22: a 15 px strip touching the bottom of a button in a row, or 1 px into it, left the row
/// whole).
const SLIGHT_OVERLAP: i64 = 1;
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
/// With nothing in a direction, [`Router::around`] looks round to a button behind that does not reach past the
/// button (a tile above a bar, looked at from the bar going down) while the button's centre is no more than this
/// many degrees off the direction, clockwise of it (M10.58, Encore CS6, 2026-11-22, docs/encore-behavior.md 9.16 and
/// 9.19: a bar 106 px below the tiles' centres went down to the left tile at 127.4° and not at 129.5°).
const BEHIND_ANGLE_CLOCKWISE: f64 = 128.5;
/// [`BEHIND_ANGLE_CLOCKWISE`] the other way round (the right tile at 120.2°, not at 121.6°).
const BEHIND_ANGLE_ANTICLOCKWISE: f64 = 121.0;
/// How far behind the centre of the button routed (along the direction) the centre of a button looked round to as
/// [`BEHIND_ANGLE_CLOCKWISE`] may be, clockwise of the direction (M10.58: a thin bar went down to a tile 172 px above
/// its centre and to none 192 px above; a tall picture button does not reach the row under it).
const BEHIND_DEPTH_CLOCKWISE: f64 = 182.0;
/// [`BEHIND_DEPTH_CLOCKWISE`] the other way round (M10.58: a tile 106 px above the bar's centre was reached, tiles
/// 156 px above at 104° and 113° were not).
const BEHIND_DEPTH_ANTICLOCKWISE: f64 = 131.0;
/// Looking round behind, the buttons within this many degrees of the direction come first, the nearest of them, and
/// the others only when there are none (M10.58, Encore CS6, 2026-11-22: a bar went down to the nearer tile at 110.5°
/// rather than to the one at 102.5°, but to the one at 103.4° rather than to the nearer one at 111.9°, and to a tile at
/// 113.1° when nothing else was in reach).
const LOOK_ROUND_FIRST: f64 = 111.2;
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

/// Looking round ([`Router::around`]), a button level with the one routed counts this many times as far as one
/// ahead, or one behind within [`LOOK_ROUND_FIRST`] that overlaps it across the direction (M10.59, Encore CS6,
/// 2026-11-22, docs/encore-behavior.md 9.21: down from a tile went to a strip ahead 1.23 times as far as the tile
/// beside it; up from a tile beside a bar went to the tile beside it 1.55 times as near as the bar behind it).
const LEVEL_WEIGHT: f64 = 1.4;

/// Where a button looked round to ([`Router::around`]) is from the one routed, along the direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    /// Centres within [`LINE_TOLERANCE`] along the direction.
    Level,
    /// Further along the direction.
    Ahead,
    /// Back against the direction.
    Behind,
}

/// A button within reach of [`Router::around`].
#[derive(Clone, Copy, Debug)]
struct Seen {
    /// Where it is along the direction.
    side: Side,
    /// The gap between the rectangles.
    gap: i64,
    /// Whether it is clockwise of the direction.
    clockwise: bool,
    /// The squared distance between the centres.
    squared: i128,
    /// Whether it is wholly aside across the direction (no overlap across it, and not reaching past the button).
    aside: bool,
    /// Its centre's angle off the direction, in degrees (0 straight ahead, 180 straight behind).
    angle: f64,
    /// Whether it partly covers the button routed.
    covers: bool,
    /// Its index.
    other: usize,
}

/// Keeps of `group` those `wanted`, when there are any.
fn prefer(group: &mut Vec<Seen>, wanted: impl Fn(&Seen) -> bool) {
    if group.iter().any(&wanted) {
        group.retain(wanted);
    }
}

/// The angle in degrees of `to` off `direction` seen from `from` (0 straight ahead, 180 straight behind).
fn angle_off(from: &Centre, to: &Centre, direction: Direction) -> f64 {
    let (step, squared) = direction.step(from, to);
    // `as` is exact enough for pixel distances.
    let aside = ((squared - step * step).max(0) as f64).sqrt();
    aside.atan2(step as f64).to_degrees()
}

/// Encore CS6's automatic routing, as measured on its menus (docs/encore-behavior.md 9.2 and 9.5j):
///
/// - Rows and columns are found as in [`lines`].
/// - In a column, up / down go to the neighbours and wrap at the ends (to the other end, or on to the next
///   column); the same for left / right in a row. Without wrapping, the ends look around as below.
/// - A button in a row but in no column stays put on up / down (and the other way round) while wrapping is on.
/// - A button that contains others goes to them first: the nearest one ahead, or the nearest level with it.
/// - Otherwise a direction without a line goes to the nearest button ahead: the smallest distance over the cosine
///   of the angle off the direction to the power 1.5, up / down only within [`VERTICAL_CONE_COS`], but a button
///   on the clockwise side nearly as good wins ([`ANGLE_TIE`]). With nothing ahead, it looks round as far as
///   [`Router::within_reach`] allows (see [`Router::around`]).
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
        // M10.59: each way once on its own, then again knowing the other way's lines (a column of a bar and the
        // narrow tile at the left of a row stands when the row does).
        let (_, first_columns) = lines(
            &all_bounds,
            &all_centres,
            rects.len(),
            (true, &column_candidates, &row_candidates),
            &[],
        );
        let (_, first_rows) = lines(
            &all_bounds,
            &all_centres,
            rects.len(),
            (false, &row_candidates, &column_candidates),
            &[],
        );
        let (column_of, columns) = lines(
            &all_bounds,
            &all_centres,
            rects.len(),
            (true, &column_candidates, &row_candidates),
            &first_rows,
        );
        let (row_of, rows) = lines(
            &all_bounds,
            &all_centres,
            rects.len(),
            (false, &row_candidates, &column_candidates),
            &first_columns,
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
            // M10.60: a thin strip touching it, centred on it, as a button it holds.
            if let Some(touching) = self.touching(index, direction) {
                return touching;
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

    /// For a button in no line this way: a thinner, shorter button touching one of its edges from outside (or
    /// [`SLIGHT_OVERLAP`] off it), within its span along that edge and centred on it (within [`LINE_TOLERANCE`]),
    /// ahead of it or level with it (within [`LINE_TOLERANCE`]): the nearest, as a button it holds (M10.60, Encore
    /// CS6, 2026-11-22, docs/encore-behavior.md 9.22: left and right from a wide button went to the 15 px strip under
    /// it, 0 to 9 px off its centre, touching or 1 px apart, as on the reporter's page).
    fn touching(&self, index: usize, direction: Direction) -> Option<usize> {
        let outer = self.bounds.get(index)?;
        let from = self.centres.get(index)?;
        self.centres
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .filter(|(other, _)| {
                self.bounds
                    .get(*other)
                    .is_some_and(|inner| outer.holds_strip(inner))
            })
            .filter_map(|(other, centre)| {
                let (step, squared) = direction.step(from, centre);
                (step >= -i128::from(LINE_TOLERANCE)).then_some((squared, other))
            })
            .min()
            .map(|(_, other)| other)
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
    /// cosine), up / down only within [`VERTICAL_CONE_COS`], not one wholly beyond the first button the line
    /// straight ahead from the centre meets (M10.55); the lower number on a tie. When that one is not on the
    /// clockwise side, the best on that side within [`ANGLE_TIE`] and [`SCORE_TIE`] of it is taken instead.
    fn ahead(&self, index: usize, direction: Direction) -> Option<usize> {
        let from = self.centres.get(index)?;
        // M10.55: a button wholly beyond the first one the line straight ahead from the centre meets is hidden by
        // it (Encore CS6, the reporter's menu: up from a bar under a row of wide buttons goes to the row, not to the
        // bar at the top straight above it, which scores better).
        let line_hit = |b: &Bounds| match direction {
            Direction::Up | Direction::Down => b.left <= from.x && from.x < b.right,
            Direction::Left | Direction::Right => b.top <= from.y && from.y < b.bottom,
        };
        // (near edge distance, far edge distance) along the direction from the centre.
        let edges = |b: &Bounds| match direction {
            Direction::Up => (from.y - b.bottom, from.y - b.top),
            Direction::Down => (b.top - from.y, b.bottom - from.y),
            Direction::Left => (from.x - b.right, from.x - b.left),
            Direction::Right => (b.left - from.x, b.right - from.x),
        };
        let first_far = self
            .bounds
            .iter()
            .enumerate()
            .filter(|(other, b)| *other != index && line_hit(b) && edges(b).0 > 0)
            .map(|(_, b)| edges(b))
            .min_by_key(|(near, _)| *near)
            .map(|(_, far)| far);
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
            if let (Some(limit), Some(b)) = (first_far, self.bounds.get(other))
                && edges(b).0 >= limit
            {
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

    /// [`Self::ahead`], or with nothing ahead a button within reach ([`Self::within_reach`]), looked for on each side:
    /// level with it (centres within [`LINE_TOLERANCE`] along the direction), ahead (up / down: outside the cone),
    /// behind. On each side, the buttons that overlap it across the direction (or reach past it) before those wholly
    /// aside (M10.59); behind, those within [`LOOK_ROUND_FIRST`] of the direction first (M10.58), and those that do not
    /// partly cover it first (M10.59); then the closest by the gap between the rectangles (gaps within
    /// [`LINE_TOLERANCE`] of it count the same), the clockwise side (up: right, right: down, down: left, left: up), the
    /// closest by centre. Of the sides' best ones the nearest by centre, a level one counting [`LEVEL_WEIGHT`] times as
    /// far, and the one behind only within [`LOOK_ROUND_FIRST`] and overlapping across; else the one behind (M10.59).
    fn around(&self, index: usize, direction: Direction) -> Option<usize> {
        if let Some(found) = self.ahead(index, direction) {
            return Some(found);
        }
        let from = self.centres.get(index)?;
        let source = self.bounds.get(index)?;
        let mut found: Vec<Seen> = Vec::new();
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
            let side = if step.abs() <= i128::from(LINE_TOLERANCE) {
                Side::Level
            } else if step > 0 {
                Side::Ahead
            } else {
                Side::Behind
            };
            found.push(Seen {
                side,
                gap: source.gap(target),
                clockwise,
                squared,
                aside: !source.across_overlaps(target, direction)
                    && !target.reaches_past(source, direction),
                angle: angle_off(from, centre, direction),
                covers: target.partly_overlaps(source),
                other,
            });
        }
        let best_of = |side: Side| -> Option<Seen> {
            let mut group: Vec<Seen> = found
                .iter()
                .copied()
                .filter(|seen| seen.side == side)
                .collect();
            prefer(&mut group, |seen| !seen.aside);
            if side == Side::Behind {
                prefer(&mut group, |seen| seen.angle <= LOOK_ROUND_FIRST);
                prefer(&mut group, |seen| !seen.covers);
            }
            let least = group.iter().map(|seen| seen.gap).min()?;
            group
                .into_iter()
                .filter(|seen| seen.gap <= least.saturating_add(LINE_TOLERANCE))
                .min_by_key(|seen| (!seen.clockwise, seen.squared, seen.other))
        };
        let bests: Vec<Seen> = [Side::Level, Side::Ahead, Side::Behind]
            .into_iter()
            .filter_map(best_of)
            .collect();
        // `as` is exact enough for pixel distances.
        let weighed = |seen: &Seen| {
            let distance = (seen.squared as f64).sqrt();
            if seen.side == Side::Level {
                distance * LEVEL_WEIGHT
            } else {
                distance
            }
        };
        bests
            .iter()
            .filter(|seen| {
                seen.side != Side::Behind || (seen.angle <= LOOK_ROUND_FIRST && !seen.aside)
            })
            .min_by(|a, b| {
                weighed(a)
                    .total_cmp(&weighed(b))
                    .then(a.other.cmp(&b.other))
            })
            .or_else(|| bests.iter().find(|seen| seen.side == Side::Behind))
            .map(|seen| seen.other)
    }

    /// Whether [`Self::around`] looks as far round as `target` (`step` ahead of `source`'s centre and `distance`
    /// from it; `clockwise` of the direction): a target that reaches past `source` in the direction (a bar under
    /// a tile) while its centre is no more than [`REACH_CLOCKWISE`] / [`REACH_ANTICLOCKWISE`] further away than
    /// it is to the side; any other (a tile above a bar, looked at from the bar going down) while its centre is
    /// within [`BEHIND_ANGLE_CLOCKWISE`] / [`BEHIND_ANGLE_ANTICLOCKWISE`] of the direction and no more than
    /// [`BEHIND_DEPTH_CLOCKWISE`] / [`BEHIND_DEPTH_ANTICLOCKWISE`] behind `source`'s centre (M10.58).
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
        let aside = (distance * distance - step * step).max(0.0).sqrt();
        if target.reaches_beyond(source, direction) {
            let reach = if clockwise {
                REACH_CLOCKWISE
            } else {
                REACH_ANTICLOCKWISE
            };
            return distance - aside <= reach;
        }
        let (most_angle, most_depth) = if clockwise {
            (BEHIND_ANGLE_CLOCKWISE, BEHIND_DEPTH_CLOCKWISE)
        } else {
            (BEHIND_ANGLE_ANTICLOCKWISE, BEHIND_DEPTH_ANTICLOCKWISE)
        };
        aside.atan2(step).to_degrees() <= most_angle && -step <= most_depth
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

/// A candidate column or row: its buttons, and whether they line up by their left (top) edges (else by their
/// centres).
struct Candidate {
    members: Vec<usize>,
    by_edge: bool,
}

/// The candidate columns (`vertical`) or rows of the first `own` buttons of `bounds`: buttons that overlap across the
/// line and whose left (top) edges, or else centres, are within [`LINE_TOLERANCE`] (a button that contains another
/// is in none), before [`lines`] puts some out.
fn candidates(bounds: &[Bounds], centres: &[Centre], own: usize, vertical: bool) -> Vec<Candidate> {
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
    let mut candidates: Vec<Candidate> = Vec::new();
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
            if line.len() >= 2 && !candidates.iter().any(|known| known.members == line) {
                candidates.push(Candidate {
                    members: line,
                    by_edge: matches!(which, Mark::Start),
                });
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
/// - A button in a line by centres and in a line by left (top) edges puts the one by centres out (M10.61: a wide
///   button's column with a bar by their left edges stands, the strip centred under it is in no column; with
///   neither standing for other reasons, a text bar and the column of pictures above it are in none).
/// - A line is out when a button touching or overlapping one of its members across the line (by more than
///   [`SLIGHT_OVERLAP`], M10.56) misses one of the
///   member's edges or centre by more than [`LINE_TOLERANCE`] but no more than [`NEAR_MISS`], when two of its
///   members miss each other that way, when a member partly overlaps another button, or (M10.44) when a member faces
///   a button outside the line that far apart (a bar 15 px below a row) — unless (M10.51) that button lines up the
///   other way with one of the members (a tile whose centre is over the bar's) — or (M10.57) when a member's nearest
///   button across the line is a thinner strip off its centre, however far ([`faces_strip_aside`]).
/// - M10.59: neither of the last two puts a line out when that button makes a line of the other way with the line's
///   first button (its lowest number: the same rectangles route differently by the buttons' order), or is in a line
///   of the other way found in a first pass (`kept`) with one of its members.
fn lines(
    bounds: &[Bounds],
    centres: &[Centre],
    own: usize,
    (vertical, candidates, across): (bool, &[Candidate], &[Candidate]),
    kept: &[Vec<usize>],
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
    // Across the line, more than [`SLIGHT_OVERLAP`] in common (M10.56: a strip touching a button's bottom, or
    // 1 px into it, does not count).
    let touch = |a: usize, b: usize| {
        let ((a0, a1), (b0, b1)) = (span(a), span(b));
        a1.min(b1) - a0.max(b0) > SLIGHT_OVERLAP
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
                .any(|line| line.members.contains(&a) && line.members.contains(&b))
    };

    // M10.61: a button in a candidate by centres and in one by edges puts the one by centres out (Encore CS6,
    // 2026-11-22, docs/encore-behavior.md 9.22: a wide button's column by its left edge with the bar under it stood,
    // and the 15 px strip centred under the button, touching it, was in no column). Candidates of one kind share no
    // button.
    let out: Vec<bool> = candidates
        .iter()
        .map(|line| {
            !line.by_edge
                && candidates.iter().any(|other| {
                    other.by_edge
                        && line
                            .members
                            .iter()
                            .any(|member| other.members.contains(member))
                })
        })
        .collect();

    let mut found: Vec<Vec<usize>> = Vec::new();
    let mut placed = vec![false; own];
    for (line, _) in candidates.iter().zip(&out).filter(|(_, out)| !**out) {
        let line: Vec<usize> = line
            .members
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
        // A member partly covered by another button, by more than [`SLIGHT_OVERLAP`] each way.
        let partly_covered = line.iter().any(|member| {
            (0..count).any(|other| {
                let (m, o) = (bound(*member), bound(other));
                other != *member
                    && m.partly_overlaps(&o)
                    && m.right.min(o.right) - m.left.max(o.left) > SLIGHT_OVERLAP
                    && m.bottom.min(o.bottom) - m.top.max(o.top) > SLIGHT_OVERLAP
            })
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
        // M10.59: a button outside the line that makes a line of the other way with the line's first button (its
        // lowest number, as Encore numbers them), or that is in a line of the other way found before with one of its
        // members, does not put the line out.
        let first = line.iter().copied().min().unwrap_or(0);
        let excused = |other: usize| {
            across
                .iter()
                .any(|line| line.members.contains(&first) && line.members.contains(&other))
                || kept.iter().any(|found| {
                    found.contains(&other) && line.iter().any(|member| found.contains(member))
                })
        };
        let crowded = line.iter().any(|member| {
            (0..count)
                .filter(|other| !line.contains(other))
                .filter(|other| !(centred && line.iter().any(|kept| lined_up(*kept, *other))))
                .filter(|other| !excused(*other))
                .any(|other| bound(*member).nearly_touches(&bound(other)))
        });
        let faced = line.iter().any(|member| {
            faces_strip_aside(bounds, (&line, *member), vertical, &|other| {
                line.iter().any(|kept| lined_up(*kept, other)) || excused(other)
            })
        });
        if members_miss || partly_covered || missed || crowded || faced {
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

/// Whether `member` of `line` (a column for `vertical`, else a row) faces a strip that puts the line out (M10.57,
/// Encore CS6, 2026-11-22, docs/encore-behavior.md 9.19): the nearest button outside the line on either side across
/// it (overlapping the member along the line; touching or [`SLIGHT_OVERLAP`] into it counts), however far, when it
/// is thinner across the line than the member and its centre along the line is more than [`LINE_TOLERANCE`] off the
/// member's, unless it lines up the other way with a member (`lined_up`). A bar under a row of tiles puts the row
/// out at 4 to 94 px; a picture taller than the row touching it, and a strip centred under one member, do not.
fn faces_strip_aside(
    bounds: &[Bounds],
    (line, member): (&[usize], usize),
    vertical: bool,
    lined_up: &dyn Fn(usize) -> bool,
) -> bool {
    let Some(m) = bounds.get(member).copied() else {
        return false;
    };
    // Twice the centre along the line, and the extent across it.
    let centre_along = |b: &Bounds| {
        if vertical {
            b.top + b.bottom
        } else {
            b.left + b.right
        }
    };
    let overlaps_along = |o: &Bounds| {
        if vertical {
            o.top < m.bottom && m.top < o.bottom
        } else {
            o.left < m.right && m.left < o.right
        }
    };
    let thickness = |b: &Bounds| {
        let (start, end) = b.across(vertical);
        end - start
    };
    let (m0, m1) = m.across(vertical);
    [false, true].into_iter().any(|after| {
        // The gap to a button on that side across the line.
        let gap = |o: &Bounds| -> Option<i64> {
            let (o0, o1) = o.across(vertical);
            if after {
                (o0 >= m1 - SLIGHT_OVERLAP).then_some(o0 - m1)
            } else {
                (o1 <= m0 + SLIGHT_OVERLAP).then_some(m0 - o1)
            }
        };
        bounds
            .iter()
            .enumerate()
            .filter(|(other, o)| *other != member && !line.contains(other) && overlaps_along(o))
            .filter_map(|(other, o)| gap(o).map(|g| (g, other, o)))
            .min_by_key(|(g, other, _)| (*g, *other))
            .is_some_and(|(_, other, o)| {
                thickness(o) < thickness(&m)
                    && (centre_along(o) - centre_along(&m)).abs() > 2 * LINE_TOLERANCE
                    && !lined_up(other)
            })
    })
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

    /// Whether `strip` is a thinner, shorter button against one of its edges (touching, or [`SLIGHT_OVERLAP`] off
    /// it), within its span along that edge and centred on it within [`LINE_TOLERANCE`] (M10.60).
    fn holds_strip(&self, strip: &Self) -> bool {
        let near = |a: i64, b: i64| (a - b).abs() <= SLIGHT_OVERLAP;
        let centred =
            |a0: i64, a1: i64, b0: i64, b1: i64| (a0 + a1 - b0 - b1).abs() <= 2 * LINE_TOLERANCE;
        let (width, height) = (strip.right - strip.left, strip.bottom - strip.top);
        let (own_width, own_height) = (self.right - self.left, self.bottom - self.top);
        let under_or_over = strip.left >= self.left
            && strip.right <= self.right
            && width < own_width
            && height < own_height
            && centred(strip.left, strip.right, self.left, self.right)
            && (near(strip.top, self.bottom) || near(strip.bottom, self.top));
        let beside = strip.top >= self.top
            && strip.bottom <= self.bottom
            && height < own_height
            && width < own_width
            && centred(strip.top, strip.bottom, self.top, self.bottom)
            && (near(strip.left, self.right) || near(strip.right, self.left));
        under_or_over || beside
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

    /// M10.54, Encore CS6 (GitHub issue #13: the reporter's chapter page with a next button and a plain button named
    /// Previous): without a previous button the leftmost of the other buttons routes up and left as any other (Land
    /// goes up to Doctor Top), and stays only where nothing is that way.
    #[test]
    fn without_a_previous_button_the_leftmost_routes_as_any_other()
    -> Result<(), Box<dyn std::error::Error>> {
        use disccraft_project::{ButtonKind, Link, Project};
        let rects = rects(&[
            (76, 956, 341, 15),
            (1465, 783, 227, 173),
            (1196, 783, 269, 173),
            (972, 783, 224, 173),
            (715, 783, 257, 173),
            (469, 783, 246, 173),
            (211, 783, 258, 172),
            (1692, 783, 228, 173),
            (0, 783, 30, 173),
            (30, 783, 181, 173),
            (0, 971, 1920, 109),
            (63, 511, 370, 272),
            (0, 0, 1920, 206),
        ]);
        let mut project = Project::default();
        let id = project.add_menu(None)?;
        for rect in &rects {
            project.add_button(id, "B".to_string(), Some(*rect), Link::Stop)?;
        }
        let mut menu = project.menu(id).ok_or("menu")?.clone();
        if let Some(button) = menu.buttons.get_mut(7) {
            button.kind = ButtonKind::Next;
        }
        let routes = super::button_routes(&menu, &RouteOptions::default());
        // Land (9): up to Doctor Top (12), down to MAIN (1), left nowhere, right to Previous (10).
        assert_eq!(routes.get(8), Some(&(12, 1, 9, 10)));
        // The rightmost (Chapter 6) still goes down and right to the next button.
        assert_eq!(routes.get(1).map(|route| (route.1, route.3)), Some((8, 8)));
        Ok(())
    }

    /// M10.55, Encore CS6 (GitHub issue #13, the reporter's first page): up from the bottom bar goes to the row of
    /// wide buttons just above it (to the clockwise one of the two at its centre), not to the bar at the top straight
    /// above, which the row hides though it scores better.
    #[test]
    fn a_button_beyond_the_one_straight_ahead_is_hidden() {
        let page = rects(&[
            (76, 956, 341, 15),
            (47, 515, 400, 358),
            (0, 0, 1920, 206),
            (0, 971, 1920, 109),
            (964, 873, 482, 83),
            (1446, 873, 474, 83),
            (483, 873, 481, 83),
            (0, 873, 483, 83),
        ]);
        let routes = auto_route_with(&page, &RouteOptions::default());
        // Bottom (4): up to EPISODE 31 (5); EPISODE 31 still goes up to the top bar (3).
        assert_eq!(routes.get(3).map(|route| route.0), Some(5));
        assert_eq!(routes.get(4).map(|route| route.0), Some(3));
    }

    /// M10.56, Encore CS6, 2026-11-22 (docs/encore-behavior.md 9.17): a 15 px strip touching the bottom of the first
    /// of two wide buttons in a row, or 1 px into it, leaves the row whole (left and right wrap between the two) and
    /// makes a column with the button it lines up with.
    #[test]
    fn a_strip_just_touching_a_button_leaves_its_row_whole() {
        for strip_top in [956, 955] {
            let page = rects(&[
                (0, 873, 483, 83),
                (76, strip_top, 341, 15),
                (483, 873, 481, 83),
            ]);
            let routes = auto_route_with(&page, &RouteOptions::default());
            assert_eq!(routes.first(), Some(&(2, 2, 3, 3)), "{strip_top}");
            assert_eq!(
                routes.get(2).map(|r| (r.2, r.3)),
                Some((1, 1)),
                "{strip_top}"
            );
        }
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

    /// M10.53, Encore CS6, 2026-11-22 (docs/encore-behavior.md 9.16): with nothing below, down from a bar looks round
    /// to a tile above it, the tiles 106 px above the bar's centre: on the left (clockwise) up to about 128.5° off
    /// straight down, on the right up to about 121° (M10.58 reads it so; M10.53 had measured from the bar's top edge).
    /// On the reporter's chapter page the bar goes down to Chapter 1 (104°), not to Chapter 2 (113°) nearer it.
    #[test]
    fn a_bar_looks_round_to_the_tiles_above_as_far_as_their_angle() {
        let on = RouteOptions::default();
        // (join of the tiles, where down from the bar goes: 2 the left tile, 3 the right one, 1 nowhere).
        for (join, down) in [
            (890, 2),
            (950, 2),
            (960, 1),
            (1020, 1),
            (1030, 3),
            (1070, 3),
            (1080, 2),
            (1100, 3),
            (1120, 3),
        ] {
            assert_eq!(
                auto_route_with(&bar_and_tiles(73, join), &on)[0].1,
                down,
                "{join}"
            );
        }
        // The tiles 256 px above: too far round on either side.
        for join in [890, 1000, 1120] {
            assert_eq!(
                auto_route_with(&bar_and_tiles(173 + 200, join), &on)[0].1,
                1
            );
        }
        let page = rects(&[
            (0, 971, 1920, 109),
            (1468, 783, 227, 173),
            (1199, 783, 269, 173),
            (975, 783, 224, 173),
            (718, 783, 257, 173),
            (472, 783, 246, 173),
            (214, 783, 258, 172),
            (0, 783, 33, 173),
        ]);
        assert_eq!(auto_route_with(&page, &on)[0].1, 7);
    }

    /// M10.51, Encore CS6, 2026-11-22: two tiles and a bar in no row or column go to each other as the nearest
    /// buttons ahead; the bar, reached from both tiles already, does not take over the left tile's right.
    #[test]
    fn a_loose_button_already_reached_takes_no_route_over() {
        let routes = auto_route_with(&bar_and_tiles(73, 970), &RouteOptions::default());
        assert_eq!(routes, vec![(3, 1, 2, 3), (3, 1, 2, 3), (2, 1, 2, 3)]);
    }

    /// M10.58, Encore CS6, 2026-11-22 (docs/encore-behavior.md 9.19): down from a bar under two tiles goes to the
    /// nearer tile while it is within about 111° of straight down, else to the other one; to a tile further round
    /// (113°) only when nothing else is in reach; to none 192 px below the tiles' centres. Right from the bar does
    /// not look round to a tile 126° round.
    #[test]
    fn a_bar_looks_round_to_the_tiles_above_it_as_encore_does() {
        let on = RouteOptions::default();
        let (ch1, ch2, ch3) = (
            (214, 783, 258, 173),
            (472, 783, 246, 173),
            (718, 783, 257, 173),
        );
        // (bar, where down goes: 2 the left tile, 3 the right one, 1 the bar itself).
        for (bar, down) in [
            ((0, 971, 1920, 109), 2),
            ((0, 971, 1920, 90), 2),
            ((0, 971, 1920, 70), 3),
            ((0, 971, 1920, 23), 3),
            ((152, 993, 1616, 23), 3),
            ((152, 960, 1616, 23), 3),
            ((152, 1030, 1616, 23), 2),
            ((152, 1050, 1616, 23), 1),
        ] {
            let routes = auto_route_with(&rects(&[bar, ch1, ch2]), &on);
            assert_eq!(routes.first().map(|route| route.1), Some(down), "{bar:?}");
        }
        let routes = auto_route_with(&rects(&[(0, 971, 1920, 109), ch2, ch3]), &on);
        assert_eq!(routes.first().map(|route| (route.1, route.3)), Some((2, 1)));
    }

    /// M10.60, Encore CS6, 2026-11-22 (docs/encore-behavior.md 9.22): a wide button in no row or column goes left and
    /// right to the thin strip under it, centred on it within 9 px, touching or 1 px apart (the reporter's EPISODE 1
    /// and its strip, 5 px off); with the row standing (the wide button first, the bar lining up with it), left and
    /// right go along the row.
    #[test]
    fn a_wide_button_goes_aside_to_the_strip_under_it() {
        let on = RouteOptions::default();
        let (bar, ep1, ep2) = ((0, 971, 1920, 109), (0, 873, 483, 83), (483, 873, 481, 83));
        // The strip (EP2 first: the row is out), then EP1's (left, right): 4 the strip, 2 EP2.
        for (strip, want) in [
            ((70, 956, 343, 15), (4, 4)),
            ((76, 956, 341, 15), (4, 4)),
            ((80, 956, 341, 15), (4, 4)),
            ((76, 957, 341, 14), (4, 4)),
        ] {
            let routes = auto_route_with(&rects(&[bar, ep2, ep1, strip]), &on);
            assert_eq!(
                routes.get(2).map(|route| (route.2, route.3)),
                Some(want),
                "{strip:?}"
            );
        }
        let routes = auto_route_with(&rects(&[bar, ep1, ep2, (76, 956, 341, 15)]), &on);
        assert_eq!(routes.get(1).map(|route| (route.2, route.3)), Some((3, 3)));
    }

    /// M10.57, Encore CS6, 2026-11-22 (docs/encore-behavior.md 9.19): two touching tiles over a thin bar 37 px
    /// below them, off both their centres, are no row: up from the left tile goes to the right one, and the right
    /// tile goes down and right to the bar. (A picture taller than the row touching it keeps the row, as on the
    /// reporter's chapter page below.)
    #[test]
    fn a_thin_bar_off_centre_below_puts_a_row_out() {
        let page = rects(&[
            (152, 993, 1616, 23),
            (214, 783, 258, 173),
            (472, 783, 246, 173),
        ]);
        let routes = auto_route_with(&page, &RouteOptions::default());
        assert_eq!(routes.get(1..), Some(&[(3, 3, 2, 3), (2, 1, 2, 1)][..]));
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
        // M10.59: the bar and the narrow tile, both at the left edge, are no column (the row's first button is the
        // rightmost tile, which the bar does not line up with), as in Encore.
        for (index, (got, want)) in routes.iter().zip(encore).enumerate() {
            assert_eq!(*got, want, "button {}", index + 1);
        }
        Ok(())
    }

    /// M10.59, Encore CS6, 2026-11-22 (docs/encore-behavior.md 9.21): the same rectangles route differently by the
    /// buttons' order. A row of tiles 15 px over a bar, with a narrow tile at the left edge the bar's left edge lines
    /// up with: when the narrow tile is the row's first button, the row and the column of the bar and the narrow tile
    /// stand; when the rightmost tile is, neither does.
    #[test]
    fn the_order_of_the_buttons_decides_a_row_over_a_bar() {
        let on = RouteOptions::default();
        let (bar, land) = ((0, 971, 1920, 109), (0, 783, 33, 173));
        let tiles = [
            (214, 783, 258, 172),
            (472, 783, 246, 173),
            (718, 783, 257, 173),
            (975, 783, 224, 173),
            (1199, 783, 269, 173),
            (1468, 783, 227, 173),
        ];
        let land_first: Vec<_> = [bar, land].into_iter().chain(tiles).collect();
        let encore = [
            [1, 1, 0, 0],
            [0, 0, 7, 2],
            [2, 2, 1, 3],
            [3, 3, 2, 4],
            [4, 4, 3, 5],
            [5, 5, 4, 6],
            [6, 6, 5, 7],
            [7, 7, 6, 1],
        ];
        let one_based = |routes: &[[u8; 4]]| -> Vec<(u8, u8, u8, u8)> {
            routes
                .iter()
                .map(|[u, d, l, r]| (u + 1, d + 1, l + 1, r + 1))
                .collect()
        };
        assert_eq!(
            auto_route_with(&rects(&land_first), &on),
            one_based(&encore)
        );
        let tile_first: Vec<_> = [bar]
            .into_iter()
            .chain(tiles.into_iter().rev())
            .chain([land])
            .collect();
        let encore = [
            [4, 6, 4, 3],
            [2, 2, 2, 1],
            [1, 0, 3, 1],
            [2, 0, 4, 2],
            [3, 0, 5, 3],
            [4, 0, 6, 4],
            [5, 5, 7, 5],
            [6, 6, 7, 6],
        ];
        assert_eq!(
            auto_route_with(&rects(&tile_first), &on),
            one_based(&encore)
        );
    }

    /// M10.61, Encore CS6, 2026-11-22 (docs/encore-behavior.md 9.22, S6 and S2): a wide button whose left edge lines up
    /// with the bar under it, and the 15 px strip centred under it, touching it. With the wide button first, its row
    /// stands, and so does its column with the bar (by their left edges): the strip's column by centres goes, and down
    /// from the button goes to the strip it holds. With the button beside it first, neither line stands.
    #[test]
    fn a_column_by_edges_stands_against_a_strip_centred_under_its_button() {
        let on = RouteOptions::default();
        let (bar, wide, beside, strip) = (
            (0, 971, 1920, 109),
            (0, 873, 483, 83),
            (483, 873, 481, 83),
            (76, 956, 341, 15),
        );
        let one_based = |routes: &[[u8; 4]]| -> Vec<(u8, u8, u8, u8)> {
            routes
                .iter()
                .map(|[u, d, l, r]| (u + 1, d + 1, l + 1, r + 1))
                .collect()
        };
        // Bar, wide, beside, strip: the bar and the wide button go up and down to each other.
        let encore = [[1, 1, 0, 0], [0, 3, 2, 2], [2, 2, 1, 1], [1, 0, 1, 2]];
        assert_eq!(
            auto_route_with(&rects(&[bar, wide, beside, strip]), &on),
            one_based(&encore)
        );
        // Bar, beside, wide, strip: no row and no column.
        let encore = [[1, 3, 1, 0], [2, 0, 2, 0], [1, 3, 3, 3], [2, 0, 2, 1]];
        assert_eq!(
            auto_route_with(&rects(&[bar, beside, wide, strip]), &on),
            one_based(&encore)
        );
    }

    /// M10.59, Encore CS6, 2026-11-22 (docs/encore-behavior.md 9.21): looking round, a strip ahead is taken before the
    /// tile beside it 1.23 times as near; down from a strip under a tile goes back to that tile (120° round) before a
    /// tile wholly aside (96° round), and to the tile wholly aside when the one over it is out of reach; up from a tile
    /// goes to a strip behind it that it overlaps rather than to the tile beside it.
    #[test]
    fn looking_round_weighs_level_ahead_and_behind_as_encore_does() {
        let on = RouteOptions::default();
        let (e1, e2) = ((0, 873, 483, 83), (483, 873, 481, 83));
        // (E1, E2, strip): down from E2, down from the strip, up from E1 (1 E1, 2 E2, 3 the strip).
        for (tiles, strip, want) in [
            (
                ((200, 873, 283, 83), e2),
                (86, 956, 341, 15),
                (Some(3), Some(1), None),
            ),
            ((e1, e2), (86, 1057, 341, 15), (Some(3), None, None)),
            ((e1, e2), (86, 956, 341, 15), (Some(3), Some(2), None)),
            ((e1, e2), (300, 956, 341, 15), (None, None, Some(3))),
        ] {
            let routes = auto_route_with(&rects(&[tiles.0, tiles.1, strip]), &on);
            let got = (
                routes.get(1).map(|route| route.1),
                routes.get(2).map(|route| route.1),
                routes.first().map(|route| route.0),
            );
            for (got, want) in [(got.0, want.0), (got.1, want.1), (got.2, want.2)] {
                if want.is_some() {
                    assert_eq!(got, want, "{strip:?}");
                }
            }
        }
    }
}
