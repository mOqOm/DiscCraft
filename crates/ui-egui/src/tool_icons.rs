//! DiscCraft's own glyphs for the menu tools the Craft apps' shared icons have no picture for (Encore's Direct
//! Select, Move, Rotate and Vertical Type tools) and for the Monitor's safe-area and subtitle buttons. Drawn in code on the same 16×16 grid and with the same stroke
//! width as the shared icons ([`crate::icons`]), so they sit beside them on the toolbar; original drawings
//! following common conventions (a hollow arrow, an arrow with a four-way cross, a turning arrow, a T with a
//! downward arrow), no third-party or Adobe artwork.

use egui::epaint::{PathShape, PathStroke};
use egui::{Color32, Painter, Pos2, Rect, Stroke, pos2, vec2};

/// A tool glyph of DiscCraft's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Glyph {
    /// A hollow arrow: selects a part inside a button.
    DirectSelection,
    /// An arrow with a four-way cross: moves the selection from anywhere.
    Move,
    /// An arrow turning around a centre: turns the selection.
    Rotate,
    /// A T with a downward arrow: writes top to bottom.
    VerticalType,
    /// A frame inside a frame: the safe areas (the monitor's button).
    SafeArea,
    /// A T and an arrow to the left with a bar: the previous subtitle.
    SubtitlePrevious,
    /// A T and an arrow to the right with a bar: the next subtitle.
    SubtitleNext,
    /// An opening bracket and a T: trims the subtitle's in point.
    SubtitleTrimIn,
    /// A T and a closing bracket: trims the subtitle's out point.
    SubtitleTrimOut,
    /// A T and a plus: adds a subtitle.
    SubtitleAdd,
}

/// The selection tool's arrow, as the shared icon draws it (a filled polygon on the 16 grid).
const ARROW: [(f32, f32); 7] = [
    (4.0, 2.0),
    (4.0, 13.0),
    (6.8, 10.4),
    (8.8, 14.6),
    (10.4, 13.9),
    (8.5, 9.8),
    (12.2, 9.6),
];

/// Grid points to screen points in `rect` (the icon's square, 16 units a side).
struct Grid<'a> {
    painter: &'a Painter,
    rect: Rect,
    color: Color32,
}

impl Grid<'_> {
    fn scale(&self) -> f32 {
        self.rect.width().min(self.rect.height()) / 16.0
    }

    fn p(&self, x: f32, y: f32) -> Pos2 {
        let s = self.scale();
        let o = self.rect.center() - vec2(8.0 * s, 8.0 * s);
        pos2(o.x + x * s, o.y + y * s)
    }

    /// The shared icons' stroke: 1.25 grid units, at least one device pixel.
    fn stroke(&self) -> Stroke {
        let ppp = self.painter.pixels_per_point();
        let width = (1.25 * self.scale() * ppp - 0.5).ceil().max(1.0) / ppp;
        Stroke::new(width, self.color)
    }

    fn line(&self, points: &[(f32, f32)]) {
        let points: Vec<Pos2> = points.iter().map(|(x, y)| self.p(*x, *y)).collect();
        self.painter.add(PathShape::line(points, self.stroke()));
    }

    fn outline(&self, points: &[(f32, f32)]) {
        let points: Vec<Pos2> = points.iter().map(|(x, y)| self.p(*x, *y)).collect();
        self.painter
            .add(PathShape::closed_line(points, self.stroke()));
    }

    /// A T standing between `x0` and `x1` (grid units), from the top (y 3) down to y 13.
    fn t_mark(&self, x0: f32, x1: f32) {
        let mid = (x0 + x1) * 0.5;
        self.line(&[(x0, 4.6), (x0, 3.0), (x1, 3.0), (x1, 4.6)]);
        self.line(&[(mid, 3.0), (mid, 13.0)]);
        self.line(&[(mid - 1.6, 13.0), (mid + 1.6, 13.0)]);
    }

    fn fill(&self, points: &[(f32, f32)]) {
        let points: Vec<Pos2> = points.iter().map(|(x, y)| self.p(*x, *y)).collect();
        self.painter.add(PathShape::convex_polygon(
            points,
            self.color,
            PathStroke::NONE,
        ));
    }

    /// The arrow, scaled by `k` about its tip and moved by (`dx`, `dy`), filled as two convex parts.
    fn arrow(&self, k: f32, dx: f32, dy: f32) {
        let at = |i: usize| {
            ARROW.get(i).map_or((0.0, 0.0), |(x, y)| {
                (4.0 + (x - 4.0) * k + dx, 2.0 + (y - 2.0) * k + dy)
            })
        };
        // The head (tip, left side, the notch, the right barb) and the tail, both convex.
        self.fill(&[at(0), at(1), at(2), at(5), at(6)]);
        self.fill(&[at(2), at(3), at(4), at(5)]);
    }
}

/// Draws `glyph` in `rect` (a square; the glyph keeps the shared icons' 16-unit grid) in `color`.
pub(crate) fn paint(painter: &Painter, rect: Rect, glyph: Glyph, color: Color32) {
    let grid = Grid {
        painter,
        rect,
        color,
    };
    match glyph {
        Glyph::DirectSelection => grid.outline(&ARROW),
        Glyph::Move => {
            grid.arrow(0.72, -1.5, -0.5);
            // The four-way cross at the lower right, with small arrowheads.
            let (cx, cy, arm) = (11.0, 11.0, 3.6);
            grid.line(&[(cx - arm, cy), (cx + arm, cy)]);
            grid.line(&[(cx, cy - arm), (cx, cy + arm)]);
            let head = 1.3;
            grid.line(&[
                (cx - arm + head, cy - head),
                (cx - arm, cy),
                (cx - arm + head, cy + head),
            ]);
            grid.line(&[
                (cx + arm - head, cy - head),
                (cx + arm, cy),
                (cx + arm - head, cy + head),
            ]);
            grid.line(&[
                (cx - head, cy - arm + head),
                (cx, cy - arm),
                (cx + head, cy - arm + head),
            ]);
            grid.line(&[
                (cx - head, cy + arm - head),
                (cx, cy + arm),
                (cx + head, cy + arm - head),
            ]);
        }
        Glyph::Rotate => {
            // Three quarters and a bit of a circle, clockwise from the upper left, ending in an arrowhead.
            let (cx, cy, r) = (8.0, 8.5, 5.0);
            let steps = 24;
            let (from, to) = (-150.0_f32.to_radians(), 135.0_f32.to_radians());
            let points: Vec<(f32, f32)> = (0..=steps)
                .map(|i| {
                    // `as` is exact for the small step count.
                    let t = from + (to - from) * i as f32 / steps as f32;
                    (cx + r * t.cos(), cy + r * t.sin())
                })
                .collect();
            grid.line(&points);
            // The arrowhead at the end, pointing on round the circle (clockwise on screen).
            let (end_x, end_y) = (cx + r * to.cos(), cy + r * to.sin());
            let (tx, ty) = (-to.sin(), to.cos());
            let (nx, ny) = (to.cos(), to.sin());
            grid.fill(&[
                (end_x + tx * 2.2, end_y + ty * 2.2),
                (end_x + nx * 1.8, end_y + ny * 1.8),
                (end_x - nx * 1.8, end_y - ny * 1.8),
            ]);
        }
        Glyph::VerticalType => {
            // A T on the left and an arrow down on the right.
            grid.line(&[(1.5, 4.5), (1.5, 2.8), (9.5, 2.8), (9.5, 4.5)]);
            grid.line(&[(5.5, 2.8), (5.5, 13.5)]);
            grid.line(&[(3.8, 13.5), (7.2, 13.5)]);
            grid.line(&[(13.0, 2.5), (13.0, 12.0)]);
            grid.fill(&[(10.8, 10.6), (15.2, 10.6), (13.0, 14.0)]);
        }
        Glyph::SafeArea => {
            grid.outline(&[(1.5, 3.0), (14.5, 3.0), (14.5, 13.0), (1.5, 13.0)]);
            grid.outline(&[(4.5, 5.5), (11.5, 5.5), (11.5, 10.5), (4.5, 10.5)]);
        }
        Glyph::SubtitlePrevious => {
            grid.line(&[(1.2, 4.5), (1.2, 11.5)]);
            grid.fill(&[(6.0, 4.5), (6.0, 11.5), (2.2, 8.0)]);
            grid.t_mark(8.5, 15.0);
        }
        Glyph::SubtitleNext => {
            grid.t_mark(1.0, 7.5);
            grid.fill(&[(10.0, 4.5), (10.0, 11.5), (13.8, 8.0)]);
            grid.line(&[(14.8, 4.5), (14.8, 11.5)]);
        }
        Glyph::SubtitleTrimIn => {
            grid.line(&[(4.2, 2.0), (1.8, 2.0), (1.8, 14.0), (4.2, 14.0)]);
            grid.t_mark(7.0, 14.5);
        }
        Glyph::SubtitleTrimOut => {
            grid.t_mark(1.5, 9.0);
            grid.line(&[(11.8, 2.0), (14.2, 2.0), (14.2, 14.0), (11.8, 14.0)]);
        }
        Glyph::SubtitleAdd => {
            grid.t_mark(1.0, 8.5);
            grid.line(&[(12.5, 7.5), (12.5, 14.5)]);
            grid.line(&[(9.0, 11.0), (16.0, 11.0)]);
        }
    }
}
