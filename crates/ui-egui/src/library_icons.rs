//! The Library panel's pictures (M6.485, as Encore's library shows them): one for each of the eight kinds, on the
//! kind toggles above the list and before each item's name, and one for each action under the list. DiscCraft's own
//! drawings in code (no Adobe artwork, AGENTS.md rule 2); the text items, Add Item and Delete use the Craft apps'
//! shared icons.

use crate::icons::{self, Icon};
use crate::library_panel::LibraryGroup;
use crate::theme;
use egui::epaint::PathShape;
use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, pos2, vec2};

/// A kind toggle's size (Encore: 26 x 16 pixels; a little taller here, to be easy to hit).
const TOGGLE_SIZE: Vec2 = vec2(26.0, 18.0);
/// The gap between the kind toggles (Encore: 4 pixels).
pub(crate) const TOGGLE_GAP: f32 = 4.0;
/// The width of the row of the eight kind toggles.
pub(crate) const TOGGLES_WIDTH: f32 = 8.0 * TOGGLE_SIZE.x + 7.0 * TOGGLE_GAP;
/// An action button's size.
const TOOL_SIZE: Vec2 = vec2(24.0, 20.0);
/// The height of the row of action buttons under the list.
pub(crate) const TOOLS_HEIGHT: f32 = TOOL_SIZE.y;
/// The square a picture is drawn in.
const PICTURE: f32 = 16.0;

/// The actions under the list (Encore's order).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tool {
    Place,
    Replace,
    SetBackground,
    NewMenu,
    AddItem,
    Delete,
}

/// Draws on a 16 x 16 grid in a rectangle, its lines a whole number of device pixels wide and on the pixel grid (so
/// that small pictures stay sharp).
struct Pen<'a> {
    painter: &'a Painter,
    origin: Pos2,
    unit: f32,
    ppp: f32,
    color: Color32,
}

impl<'a> Pen<'a> {
    fn new(painter: &'a Painter, rect: Rect, color: Color32) -> Self {
        let unit = rect.width().min(rect.height()) / 16.0;
        Self {
            painter,
            origin: rect.center() - vec2(8.0 * unit, 8.0 * unit),
            unit,
            ppp: painter.pixels_per_point().max(0.25),
            color,
        }
    }

    /// The lines' width in device pixels: one up to 149 %, two from 150 %, and so on.
    fn width_px(&self) -> f32 {
        self.ppp.round().max(1.0)
    }

    fn stroke(&self) -> Stroke {
        Stroke::new(self.width_px() / self.ppp, self.color)
    }

    /// A grid point, unsnapped.
    fn at(&self, (x, y): (f32, f32)) -> Pos2 {
        pos2(self.origin.x + x * self.unit, self.origin.y + y * self.unit)
    }

    /// Where a line through a grid point goes: its middle on a pixel's centre (an odd width) or on an edge (an even
    /// one), so that it covers whole pixels.
    fn on_line(&self, point: (f32, f32)) -> Pos2 {
        let odd = self.width_px() % 2.0 > 0.5;
        let ppp = self.ppp;
        let snap = |v: f32| {
            if odd {
                ((v * ppp).floor() + 0.5) / ppp
            } else {
                (v * ppp).round() / ppp
            }
        };
        let p = self.at(point);
        pos2(snap(p.x), snap(p.y))
    }

    /// A grid point on the pixels' edges (where a filled area begins or ends).
    fn on_edge(&self, point: (f32, f32)) -> Pos2 {
        let ppp = self.ppp;
        let p = self.at(point);
        pos2((p.x * ppp).round() / ppp, (p.y * ppp).round() / ppp)
    }

    fn line(&self, points: &[(f32, f32)]) {
        let points = points.iter().map(|&p| self.on_line(p)).collect();
        self.painter.add(PathShape::line(points, self.stroke()));
    }

    fn closed(&self, points: &[(f32, f32)]) {
        let points = points.iter().map(|&p| self.on_line(p)).collect();
        self.painter
            .add(PathShape::closed_line(points, self.stroke()));
    }

    fn frame(&self, (x0, y0): (f32, f32), (x1, y1): (f32, f32)) {
        self.closed(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)]);
    }

    fn fill_rect(&self, min: (f32, f32), max: (f32, f32), color: Color32) {
        self.painter.rect_filled(
            Rect::from_min_max(self.on_edge(min), self.on_edge(max)),
            0.0,
            color,
        );
    }

    fn fill(&self, points: &[(f32, f32)]) {
        let points = points.iter().map(|&p| self.at(p)).collect();
        self.painter
            .add(PathShape::convex_polygon(points, self.color, Stroke::NONE));
    }

    fn circle(&self, centre: (f32, f32), radius: f32) {
        self.painter
            .circle_stroke(self.at(centre), radius * self.unit, self.stroke());
    }

    fn dot(&self, centre: (f32, f32), radius: f32) {
        self.painter
            .circle_filled(self.at(centre), radius * self.unit, self.color);
    }

    /// A bar with round ends.
    fn pill(&self, min: (f32, f32), max: (f32, f32)) {
        let rect = Rect::from_min_max(self.on_line(min), self.on_line(max));
        self.painter.rect_stroke(
            rect,
            rect.height() / 2.0,
            self.stroke(),
            egui::StrokeKind::Middle,
        );
    }

    /// The menu picture in a box from `min` to `max`: a screen with three buttons in a column (as the Project
    /// panel's menus).
    fn menu(&self, (x0, y0): (f32, f32), (x1, y1): (f32, f32)) {
        self.frame((x0, y0), (x1, y1));
        let step = (y1 - y0) / 4.0;
        let inset = (x1 - x0) * 0.27;
        for row in 1..=3_u8 {
            let y = y0 + step * f32::from(row);
            self.line(&[(x0 + inset, y), (x1 - inset, y)]);
        }
    }

    /// The background picture in a box: the whole screen filled.
    fn background(&self, min: (f32, f32), max: (f32, f32)) {
        self.fill_rect(min, max, self.color.gamma_multiply(0.45));
        self.frame(min, max);
    }
}

/// Paints the picture of `group` centred in `rect` (16 x 16).
pub(crate) fn paint_kind(painter: &Painter, rect: Rect, group: LibraryGroup, color: Color32) {
    let pen = Pen::new(painter, rect, color);
    match group {
        LibraryGroup::Menus => pen.menu((1.5, 3.0), (14.5, 13.0)),
        // A button: a bar with round ends and its label.
        LibraryGroup::Buttons => {
            pen.pill((2.0, 5.0), (14.0, 11.0));
            pen.line(&[(5.5, 8.0), (10.5, 8.0)]);
        }
        // A picture: a frame with two hills and the sun.
        LibraryGroup::Images => {
            pen.frame((1.5, 3.0), (14.5, 13.0));
            pen.fill(&[(3.0, 12.0), (7.0, 6.5), (11.0, 12.0)]);
            pen.fill(&[(8.5, 12.0), (10.5, 9.0), (13.0, 12.0)]);
            pen.dot((11.5, 5.8), 1.2);
        }
        LibraryGroup::Backgrounds => pen.background((1.5, 3.0), (14.5, 13.0)),
        // Layers: three sheets one on another.
        LibraryGroup::LayerSets => {
            pen.closed(&[(8.0, 2.5), (14.0, 5.5), (8.0, 8.5), (2.0, 5.5)]);
            pen.line(&[(2.0, 8.0), (8.0, 11.0), (14.0, 8.0)]);
            pen.line(&[(2.0, 10.5), (8.0, 13.5), (14.0, 10.5)]);
        }
        LibraryGroup::Texts => icons::paint(painter, rect, Icon::Type, color),
        // Shapes: a circle, and a square over its lower right.
        LibraryGroup::Shapes => {
            pen.circle((6.0, 6.5), 4.0);
            pen.fill_rect((7.5, 8.0), (14.0, 14.0), color);
        }
        // A replacement layer: a frame for a picture still to come, crossed (as the Library's preview draws one).
        LibraryGroup::Replacements => {
            pen.frame((1.5, 3.0), (14.5, 13.0));
            pen.line(&[(1.5, 3.0), (14.5, 13.0)]);
            pen.line(&[(14.5, 3.0), (1.5, 13.0)]);
        }
    }
}

/// Paints the picture of `tool` centred in `rect` (16 x 16).
pub(crate) fn paint_tool(painter: &Painter, rect: Rect, tool: Tool, color: Color32) {
    let pen = Pen::new(painter, rect, color);
    match tool {
        // An arrow going into the menu (a frame).
        Tool::Place => {
            pen.frame((5.5, 6.5), (14.5, 14.0));
            pen.line(&[(1.5, 2.0), (10.0, 10.5)]);
            pen.line(&[(5.0, 10.5), (10.0, 10.5), (10.0, 5.5)]);
        }
        // Two arrows, one each way: one menu's design for the other's.
        Tool::Replace => {
            pen.line(&[(2.0, 5.5), (13.5, 5.5)]);
            pen.line(&[(10.5, 2.5), (13.5, 5.5), (10.5, 8.5)]);
            pen.line(&[(2.5, 10.5), (14.0, 10.5)]);
            pen.line(&[(5.5, 7.5), (2.5, 10.5), (5.5, 13.5)]);
        }
        // The background picture with a tick: use it.
        Tool::SetBackground => {
            pen.background((0.5, 3.0), (10.5, 12.5));
            pen.line(&[(11.0, 10.0), (12.5, 12.0), (15.5, 7.5)]);
        }
        // The menu picture with a plus: a new one.
        Tool::NewMenu => {
            pen.menu((0.5, 3.0), (10.5, 12.5));
            pen.line(&[(13.5, 5.5), (13.5, 10.5)]);
            pen.line(&[(11.0, 8.0), (16.0, 8.0)]);
        }
        Tool::AddItem => icons::paint(painter, rect, Icon::Import, color),
        Tool::Delete => icons::paint(painter, rect, Icon::Trash, color),
    }
}

/// A kind toggle: sunk into the panel while the kind is shown; under the pointer a raised box with the picture gold,
/// as the toolbar's tools (Encore's library).
pub(crate) fn kind_toggle(ui: &mut Ui, group: LibraryGroup, shown: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(TOGGLE_SIZE, Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        if shown {
            theme::sunken_box(ui.painter(), rect);
        } else if hovered {
            theme::hover_box(ui.painter(), rect);
        }
        let color = if hovered {
            theme::HOVER_ICON.get()
        } else {
            theme::TEXT.get()
        };
        paint_kind(
            ui.painter(),
            Rect::from_center_size(rect.center(), vec2(PICTURE, PICTURE)),
            group,
            color,
        );
    }
    response
}

/// An action button under the list: its picture; under the pointer a raised box with the picture gold; dim while
/// the action cannot be used (its tip still says what it is).
pub(crate) fn tool_button(ui: &mut Ui, tool: Tool, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(TOOL_SIZE, sense);
    if ui.is_rect_visible(rect) {
        let hovered = enabled && response.hovered();
        if hovered {
            theme::hover_box(ui.painter(), rect);
        }
        let color = if !enabled {
            theme::TEXT_WEAK.get().gamma_multiply(0.5)
        } else if hovered {
            theme::HOVER_ICON.get()
        } else {
            theme::TEXT.get()
        };
        paint_tool(
            ui.painter(),
            Rect::from_center_size(rect.center(), vec2(PICTURE, PICTURE)),
            tool,
            color,
        );
    }
    response
}

/// Paints the picture of `group` before a list row's name, centred at `centre`.
pub(crate) fn row_picture(painter: &Painter, centre: Pos2, group: LibraryGroup, color: Color32) {
    paint_kind(
        painter,
        Rect::from_center_size(centre, vec2(PICTURE, PICTURE)),
        group,
        color,
    );
}
