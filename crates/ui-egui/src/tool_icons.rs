//! DiscCraft's own glyphs for the menu tools the Craft apps' shared icons have no picture for (Encore's Direct
//! Select, Move, Rotate and Vertical Type tools), for the Monitor's safe-area and subtitle buttons and for the Menu
//! Viewer's bar (M6.487: calibrate, routing, guides, the three subpicture states, the page). Drawn in code on the same 16×16 grid and with the same stroke
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
    /// A screen with a two-headed arrow across it: the menu's pixels stretched to the TV screen.
    Calibrate,
    /// A small square with an arrowhead on each side: the buttons' routes (up, down, left, right).
    Routing,
    /// A frame crossed by a vertical and a horizontal guide.
    Guides,
    /// The guides and a plus: a new guide.
    NewGuide,
    /// A button's outline: its normal subpicture.
    SubpictureNormal,
    /// A button half filled: its selected highlight.
    SubpictureSelected,
    /// A button filled, with rays from its corner: its activated highlight.
    SubpictureActivated,
    /// A screen with three buttons in a column: a menu page (the page number's mark).
    MenuPage,
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

    /// The outline of a bar with round ends from (`x0`, `y0`) to (`x1`, `y1`) (a button), as points.
    fn stadium(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<(f32, f32)> {
        let r = (y1 - y0) * 0.5;
        let (cy, left, right) = (y0 + r, x0 + r, x1 - r);
        let steps = 8_u8;
        let mut points = Vec::with_capacity(2 * usize::from(steps) + 2);
        for i in 0..=steps {
            // From the top round the right end to the bottom.
            let t = (-90.0 + 180.0 * f32::from(i) / f32::from(steps)).to_radians();
            points.push((right + r * t.cos(), cy + r * t.sin()));
        }
        for i in 0..=steps {
            let t = (90.0 + 180.0 * f32::from(i) / f32::from(steps)).to_radians();
            points.push((left + r * t.cos(), cy + r * t.sin()));
        }
        points
    }

    fn fill_in(&self, points: &[(f32, f32)], color: Color32) {
        let points: Vec<Pos2> = points.iter().map(|(x, y)| self.p(*x, *y)).collect();
        self.painter
            .add(PathShape::convex_polygon(points, color, PathStroke::NONE));
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
        Glyph::Calibrate => {
            grid.outline(&[(1.5, 3.5), (14.5, 3.5), (14.5, 12.5), (1.5, 12.5)]);
            grid.line(&[(5.0, 8.0), (11.0, 8.0)]);
            grid.fill(&[(3.6, 8.0), (6.2, 6.0), (6.2, 10.0)]);
            grid.fill(&[(12.4, 8.0), (9.8, 6.0), (9.8, 10.0)]);
        }
        Glyph::Routing => {
            grid.outline(&[(6.2, 6.2), (9.8, 6.2), (9.8, 9.8), (6.2, 9.8)]);
            grid.fill(&[(8.0, 1.2), (5.6, 4.2), (10.4, 4.2)]);
            grid.fill(&[(8.0, 14.8), (5.6, 11.8), (10.4, 11.8)]);
            grid.fill(&[(1.2, 8.0), (4.2, 5.6), (4.2, 10.4)]);
            grid.fill(&[(14.8, 8.0), (11.8, 5.6), (11.8, 10.4)]);
        }
        Glyph::Guides => {
            grid.outline(&[(2.5, 3.0), (13.5, 3.0), (13.5, 13.0), (2.5, 13.0)]);
            grid.line(&[(6.0, 0.8), (6.0, 15.2)]);
            grid.line(&[(0.8, 9.5), (15.2, 9.5)]);
        }
        Glyph::NewGuide => {
            grid.outline(&[(1.5, 4.5), (9.0, 4.5), (9.0, 13.0), (1.5, 13.0)]);
            grid.line(&[(4.0, 2.5), (4.0, 15.0)]);
            grid.line(&[(0.0, 10.0), (9.0, 10.0)]);
            grid.line(&[(13.0, 2.5), (13.0, 8.5)]);
            grid.line(&[(10.0, 5.5), (16.0, 5.5)]);
        }
        Glyph::SubpictureNormal => grid.outline(&Grid::stadium(1.5, 5.0, 14.5, 11.0)),
        Glyph::SubpictureSelected => {
            let button = Grid::stadium(1.5, 5.0, 14.5, 11.0);
            grid.fill_in(&button, color.gamma_multiply(0.45));
            grid.outline(&button);
        }
        Glyph::SubpictureActivated => {
            let button = Grid::stadium(1.0, 6.5, 12.0, 12.0);
            grid.fill(&button);
            grid.outline(&button);
            grid.line(&[(12.6, 5.0), (14.6, 3.0)]);
            grid.line(&[(13.6, 7.6), (16.0, 7.6)]);
            grid.line(&[(10.4, 4.4), (10.4, 2.0)]);
        }
        Glyph::MenuPage => {
            grid.outline(&[(2.0, 3.0), (14.0, 3.0), (14.0, 13.0), (2.0, 13.0)]);
            for y in [5.8, 8.0, 10.2] {
                grid.line(&[(5.0, y), (11.0, y)]);
            }
        }
    }
}

/// A picture button on a viewer's bar (M6.487, Encore's Menu Viewer bar, as the Monitor's): `glyph` sunk in while
/// `on`, a raised box with the picture gold under the pointer, dim while it cannot be used (its tip still shows).
/// Gives whether it was clicked.
pub(crate) fn bar_button(ui: &mut egui::Ui, glyph: Glyph, state: (bool, bool), tip: &str) -> bool {
    bar_button_with(
        ui,
        |painter, rect, color| paint(painter, rect, glyph, color),
        state,
        tip,
    )
}

/// [`bar_button`] with a picture `draw` paints (in a 16-point square, in a colour): one of the Craft apps' shared
/// icons, say.
pub(crate) fn bar_button_with(
    ui: &mut egui::Ui,
    draw: impl FnOnce(&Painter, Rect, Color32),
    (on, enabled): (bool, bool),
    tip: &str,
) -> bool {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(BAR_BUTTON, sense);
    let response = response.on_hover_text(tip);
    if ui.is_rect_visible(rect) {
        let hovered = enabled && response.hovered();
        if on {
            crate::theme::sunken_box(ui.painter(), rect);
        } else if hovered {
            crate::theme::hover_box(ui.painter(), rect);
        }
        let color = if !enabled {
            crate::theme::BAR_ICON.get().gamma_multiply(0.4)
        } else if hovered {
            crate::theme::HOVER_ICON.get()
        } else {
            crate::theme::BAR_ICON.get()
        };
        draw(
            ui.painter(),
            Rect::from_center_size(rect.center(), vec2(16.0, 16.0)),
            color,
        );
    }
    enabled && response.clicked()
}

/// A mark on a viewer's bar that is not a button (Encore's page number): `glyph` in the bar's colour, `tip` under
/// the pointer.
pub(crate) fn bar_mark(ui: &mut egui::Ui, glyph: Glyph, tip: &str) {
    let (rect, response) = ui.allocate_exact_size(BAR_BUTTON, egui::Sense::hover());
    let _ = response.on_hover_text(tip);
    if ui.is_rect_visible(rect) {
        paint(
            ui.painter(),
            Rect::from_center_size(rect.center(), vec2(16.0, 16.0)),
            glyph,
            crate::theme::BAR_ICON.get(),
        );
    }
}

/// The size of a [`bar_button`] (Encore's Menu Viewer bar: about 20 pixels high, 28 apart).
pub(crate) const BAR_BUTTON: egui::Vec2 = vec2(24.0, 20.0);

/// The line between a bar's groups of buttons (Encore's: a dark line with a light one beside it).
pub(crate) fn bar_divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(6.0, BAR_BUTTON.y), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        crate::theme::bar_separator(
            ui.painter(),
            rect.center().x - 1.0,
            rect.y_range().shrink(2.0),
        );
    }
}
