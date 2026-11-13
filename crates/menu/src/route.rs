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
    let centres: Vec<Centre> = own_rects.iter().copied().map(Centre::from_rect).collect();
    let layout = Layout {
        rects: &own_rects,
        centres: &centres,
        options,
    };
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
                let near = choose_direction(&centres, sub, direction);
                if near != 0 {
                    to_menu(near)
                } else if index == first && matches!(direction, Direction::Up | Direction::Left) {
                    or_own(previous, index)
                } else if index == last && matches!(direction, Direction::Down | Direction::Right) {
                    or_own(next, index)
                } else {
                    to_menu(layout.choose(sub, direction))
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
    let centres: Vec<Centre> = rects.iter().copied().map(Centre::from_rect).collect();
    let layout = Layout {
        rects,
        centres: &centres,
        options,
    };

    rects
        .iter()
        .enumerate()
        .map(|(index, _)| {
            (
                layout.choose(index, Direction::Up),
                layout.choose(index, Direction::Down),
                layout.choose(index, Direction::Left),
                layout.choose(index, Direction::Right),
            )
        })
        .collect()
}

/// The buttons and options the routing is worked out from.
struct Layout<'a> {
    rects: &'a [Rect],
    centres: &'a [Centre],
    options: &'a RouteOptions,
}

impl Layout<'_> {
    /// The button `direction` goes to from button `index` (one-based; the button itself when it does not move).
    fn choose(&self, index: usize, direction: Direction) -> u8 {
        let vertical = matches!(direction, Direction::Up | Direction::Down);
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
            return one_based(index);
        }
        let target = choose_direction(self.centres, index, direction);
        if target != 0 {
            return target;
        }
        if !wraps {
            return one_based(index);
        }
        let forward = matches!(direction, Direction::Down | Direction::Right);
        let line = match mode {
            WrapMode::SameLine => self.line_of(index, vertical),
            WrapMode::NextLine => self.next_line(index, vertical, forward),
        };
        // Down / right wrap to the start of the line (top / left), up / left to its end.
        self.extreme(&line, vertical, forward)
            .map_or_else(|| one_based(index), one_based)
    }

    /// Whether buttons `a` and `b` share a column (`vertical`: their x ranges overlap) or a row.
    fn same_line(&self, a: usize, b: usize, vertical: bool) -> bool {
        let (Some(a), Some(b)) = (self.rects.get(a), self.rects.get(b)) else {
            return false;
        };
        let span = |rect: &Rect| {
            if vertical {
                (u32::from(rect.x), u32::from(rect.x) + u32::from(rect.w))
            } else {
                (u32::from(rect.y), u32::from(rect.y) + u32::from(rect.h))
            }
        };
        let (a0, a1) = span(a);
        let (b0, b1) = span(b);
        a0 < b1 && b0 < a1
    }

    /// The buttons in button `index`'s column (`vertical`) or row, itself included.
    fn line_of(&self, index: usize, vertical: bool) -> Vec<usize> {
        (0..self.rects.len())
            .filter(|other| *other == index || self.same_line(index, *other, vertical))
            .collect()
    }

    /// The column after (`forward`) or before button `index`'s, or the first / last one past the end; its own
    /// when there is no other.
    fn next_line(&self, index: usize, vertical: bool, forward: bool) -> Vec<usize> {
        let key = |i: usize| {
            self.centres
                .get(i)
                .map_or(0, |centre| if vertical { centre.x } else { centre.y })
        };
        let mine = key(index);
        let others: Vec<usize> = (0..self.rects.len())
            .filter(|other| *other != index && !self.same_line(index, *other, vertical))
            .collect();
        let after = |i: &usize| {
            if forward {
                key(*i) > mine
            } else {
                key(*i) < mine
            }
        };
        let pick = |candidates: Vec<usize>| {
            if forward {
                candidates.into_iter().min_by_key(|i| (key(*i), *i))
            } else {
                candidates
                    .into_iter()
                    .max_by_key(|i| (key(*i), std::cmp::Reverse(*i)))
            }
        };
        let beyond: Vec<usize> = others.iter().copied().filter(after).collect();
        let anchor = if beyond.is_empty() {
            pick(others)
        } else {
            pick(beyond)
        };
        anchor.map_or_else(
            || self.line_of(index, vertical),
            |anchor| self.line_of(anchor, vertical),
        )
    }

    /// The first (`first`: top / left-most) or last button of `line`.
    fn extreme(&self, line: &[usize], vertical: bool, first: bool) -> Option<usize> {
        let key = |i: usize| {
            self.centres
                .get(i)
                .map_or(0, |centre| if vertical { centre.y } else { centre.x })
        };
        if first {
            line.iter().copied().min_by_key(|i| (key(*i), *i))
        } else {
            line.iter()
                .copied()
                .max_by_key(|i| (key(*i), std::cmp::Reverse(*i)))
        }
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

fn choose_direction(centres: &[Centre], index: usize, direction: Direction) -> u8 {
    let Some(current) = centres.get(index) else {
        return 0;
    };

    let mut best: Option<(i64, usize)> = None;

    for (other_index, other) in centres.iter().enumerate() {
        if other_index == index {
            continue;
        }
        if !is_candidate(direction, current, other) {
            continue;
        }

        let score = score(direction, current, other);
        let better = match best {
            None => true,
            Some((best_score, best_index)) => {
                score < best_score || (score == best_score && other_index < best_index)
            }
        };

        if better {
            best = Some((score, other_index));
        }
    }

    best.map_or(0, |(_, other_index)| one_based(other_index))
}

fn is_candidate(direction: Direction, current: &Centre, other: &Centre) -> bool {
    match direction {
        Direction::Up => other.y < current.y,
        Direction::Down => other.y > current.y,
        Direction::Left => other.x < current.x,
        Direction::Right => other.x > current.x,
    }
}

fn score(direction: Direction, current: &Centre, other: &Centre) -> i64 {
    let (primary, secondary) = match direction {
        Direction::Up | Direction::Down => {
            ((current.y - other.y).abs(), (current.x - other.x).abs())
        }
        Direction::Left | Direction::Right => {
            ((current.x - other.x).abs(), (current.y - other.y).abs())
        }
    };

    primary.saturating_add(secondary.saturating_mul(2))
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

    #[test]
    fn switched_off_directions_and_wraps_stay_put() {
        let options = RouteOptions {
            vertical: false,
            horizontal_wrap: false,
            ..RouteOptions::default()
        };
        let routes = auto_route_with(&grid(), &options);
        assert_eq!((routes[0].0, routes[0].1), (1, 1));
        // 1 right still moves to 2; 2 right does not wrap.
        assert_eq!(routes[0].3, 2);
        assert_eq!(routes[1].3, 2);
    }
}
