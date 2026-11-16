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
/// buttons and the menu has both kinds of button, Encore's routing for them (seen in Encore CS6, 2026-11-13):
/// the other buttons route among themselves; the lowest-numbered one wraps up / left to the previous button and
/// the highest-numbered one wraps down / right to the next button. A previous button goes down / right to the
/// lowest, left to the next button and stays up; a next button goes up / left to the highest, right to the
/// previous button and stays down.
fn automatic_routes(menu: &Menu, rects: &[Rect], options: &RouteOptions) -> Vec<(u8, u8, u8, u8)> {
    let page = |kind: ButtonKind| matches!(kind, ButtonKind::Next | ButtonKind::Previous);
    let kinds: Vec<ButtonKind> = menu.buttons.iter().map(|button| button.kind).collect();
    let normal: Vec<usize> = (0..rects.len())
        .filter(|index| kinds.get(*index).is_some_and(|kind| !page(*kind)))
        .collect();
    let (Some(&first), Some(&last)) = (normal.first(), normal.last()) else {
        return auto_route_with(rects, options);
    };
    if !options.exclude_next_previous || normal.len() == rects.len() {
        return auto_route_with(rects, options);
    }
    let previous = kinds.iter().position(|kind| *kind == ButtonKind::Previous);
    let next = kinds.iter().position(|kind| *kind == ButtonKind::Next);
    let own_rects: Vec<Rect> = normal
        .iter()
        .filter_map(|index| rects.get(*index).copied())
        .collect();
    let router = Router::new(&own_rects, options);
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
                if let Some(near) = router.direct(sub, direction) {
                    to_menu(one_based(near))
                } else if index == first && matches!(direction, Direction::Up | Direction::Left) {
                    or_own(previous, index)
                } else if index == last && matches!(direction, Direction::Down | Direction::Right) {
                    or_own(next, index)
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
    Router::new(rects, options)
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

/// Buttons whose centres, left / top edges or right / bottom edges are this close are lined up (Encore CS6: a
/// button 9 px off its column still belongs to it, one 10 px off does not).
const LINE_TOLERANCE: i64 = 9;
/// With nothing strictly in a direction, a button up to about 110° round from it is still taken (Encore CS6 took
/// one at 107.6° and left one at 116.6°): the cosine of that angle.
const WIDE_COS: f64 = -0.342;

/// Encore CS6's automatic routing, as measured on its menus (docs/encore-behavior.md 9.2):
///
/// - Buttons that overlap along x form a column when they are all lined up (by centre or by an edge, within
///   [`LINE_TOLERANCE`]); rows likewise along y. One button out of line breaks the column for all of them.
/// - In a column, up / down go to the neighbours and wrap at the ends (to the other end, or on to the next
///   column); the same for left / right in a row. Without wrapping, the ends look around as below.
/// - A button in a row but in no column stays put on up / down (and the other way round) while wrapping is on.
/// - A button in no row and no column goes to the nearest button in each direction; so does every direction that
///   has no line when wrapping is off. "Nearest" favours buttons straight ahead (distance over the cosine of the
///   angle off the direction); with nothing ahead, a button a little past the side still counts.
/// - A button in no row and no column is entered from one button only: the nearest one, in the direction it lies.
struct Router<'a> {
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
    fn new(rects: &[Rect], options: &'a RouteOptions) -> Self {
        let centres: Vec<Centre> = rects.iter().copied().map(Centre::from_rect).collect();
        let (column_of, columns) = lines(rects, &centres, true);
        let (row_of, rows) = lines(rects, &centres, false);
        Self {
            centres,
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
    /// that way — the nearest button strictly in that direction.
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
        let Some((line, place)) = self.line(index, vertical) else {
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
        let _ = place;
        end.copied().unwrap_or(index)
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

    /// The nearest button strictly in `direction` from `index`: the smallest distance over the cosine of the
    /// angle off the direction (distance squared over the step in the direction), the lower number on a tie.
    fn ahead(&self, index: usize, direction: Direction) -> Option<usize> {
        let from = self.centres.get(index)?;
        let mut best: Option<(i128, i128, usize)> = None;
        for (other, centre) in self.centres.iter().enumerate() {
            if other == index {
                continue;
            }
            let (step, squared) = direction.step(from, centre);
            if step <= 0 {
                continue;
            }
            let better = best.is_none_or(|(best_squared, best_step, _)| {
                // squared / step < best_squared / best_step, without dividing.
                squared.saturating_mul(best_step) < best_squared.saturating_mul(step)
            });
            if better {
                best = Some((squared, step, other));
            }
        }
        best.map(|(_, _, other)| other)
    }

    /// [`Self::ahead`], or with nothing ahead the closest button no more than about 110° round from `direction`.
    fn around(&self, index: usize, direction: Direction) -> Option<usize> {
        if let Some(found) = self.ahead(index, direction) {
            return Some(found);
        }
        let from = self.centres.get(index)?;
        let mut best: Option<(i128, usize)> = None;
        for (other, centre) in self.centres.iter().enumerate() {
            if other == index {
                continue;
            }
            let (step, squared) = direction.step(from, centre);
            // `as` is exact enough for pixel distances.
            let distance = (squared as f64).sqrt();
            if distance <= 0.0 || (step as f64) < WIDE_COS * distance {
                continue;
            }
            if best.is_none_or(|(best_squared, _)| squared < best_squared) {
                best = Some((squared, other));
            }
        }
        best.map(|(_, other)| other)
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
        // A button in no row and no column is entered from its nearest button, in the direction it lies.
        for (loose, centre) in self.centres.iter().enumerate() {
            if self.line(loose, true).is_some() || self.line(loose, false).is_some() {
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

/// The columns (`vertical`) or rows of the buttons: each button's line when it is in one of two or more buttons,
/// and the lines in order along themselves.
///
/// Buttons whose centres line up are a line, whatever else is around them. Buttons lined up only by an edge
/// (text buttons typed from the same x) are a line when every button they overlap across the line's direction
/// is lined up with them too: one button out of line there breaks it for all of them.
fn lines(
    rects: &[Rect],
    centres: &[Centre],
    vertical: bool,
) -> (Vec<Option<usize>>, Vec<Vec<usize>>) {
    let span = |rect: &Rect| -> (i64, i64) {
        if vertical {
            (i64::from(rect.x), i64::from(rect.x) + i64::from(rect.w))
        } else {
            (i64::from(rect.y), i64::from(rect.y) + i64::from(rect.h))
        }
    };
    let across = |index: usize| -> i64 {
        centres
            .get(index)
            .map_or(0, |centre| if vertical { centre.x } else { centre.y })
    };
    let along = |index: usize| -> i64 {
        centres
            .get(index)
            .map_or(0, |centre| if vertical { centre.y } else { centre.x })
    };
    let count = rects.len();
    // Buttons joined by `joined`, as one group id per button.
    let groups = |joined: &dyn Fn(usize, usize) -> bool| -> Vec<usize> {
        let mut group: Vec<usize> = (0..count).collect();
        for a in 0..count {
            for b in a.saturating_add(1)..count {
                if !joined(a, b) {
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
        group
    };
    let members_of = |group: &[usize], id: usize| -> Vec<usize> {
        (0..count)
            .filter(|index| group.get(*index) == Some(&id))
            .collect()
    };
    let overlapping = groups(&|a, b| match (rects.get(a), rects.get(b)) {
        (Some(first), Some(second)) => {
            let ((a0, a1), (b0, b1)) = (span(first), span(second));
            a0 < b1 && b0 < a1
        }
        _ => false,
    });
    let centred = groups(&|a, b| (across(a) - across(b)).abs() <= LINE_TOLERANCE);

    let mut found: Vec<Vec<usize>> = Vec::new();
    let mut placed = vec![false; count];
    // Whole overlapping groups lined up by an edge.
    for id in 0..count {
        let members = members_of(&overlapping, id);
        if members.len() < 2 {
            continue;
        }
        let spread = |edge: &dyn Fn(&Rect) -> i64| -> Option<i64> {
            let values: Vec<i64> = members
                .iter()
                .filter_map(|index| rects.get(*index))
                .map(edge)
                .collect();
            Some(values.iter().max()? - values.iter().min()?)
        };
        let by_edge = [spread(&|rect| span(rect).0), spread(&|rect| span(rect).1)]
            .into_iter()
            .flatten()
            .any(|spread| spread <= LINE_TOLERANCE);
        if by_edge {
            for member in &members {
                if let Some(slot) = placed.get_mut(*member) {
                    *slot = true;
                }
            }
            found.push(members);
        }
    }
    // Buttons lined up by their centres.
    for id in 0..count {
        let members: Vec<usize> = members_of(&centred, id)
            .into_iter()
            .filter(|index| placed.get(*index) == Some(&false))
            .collect();
        if members.len() >= 2 {
            found.push(members);
        }
    }

    let mut of = vec![None; count];
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
}
