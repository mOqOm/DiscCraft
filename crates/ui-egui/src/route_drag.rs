//! The button routes in the menu viewer, drawn as Encore draws them: a cross on every button with its number in
//! the middle and, on each arrow, the number of the button that arrow key moves to (0: nowhere). The selected
//! buttons' crosses are green. With "Automatically Route Buttons" off, an arrow of the cross is dragged onto the
//! button it should go to.

use crate::app::{DiscCraftApp, Selection};
use crate::menu_editor::to_canvas;
use disccraft_project::{Menu, MenuId};
use egui::{Pos2, Ui};
use serde_json::json;

/// A route being dragged from a button's edge handle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RouteDrag {
    pub(crate) menu: MenuId,
    /// The button the route leaves.
    pub(crate) button: usize,
    /// 0 up, 1 down, 2 left, 3 right.
    pub(crate) direction: usize,
    /// Where the pointer is now (canvas).
    pub(crate) now: Pos2,
}

/// The parameter name of a direction.
pub(crate) fn direction_name(direction: usize) -> &'static str {
    match direction {
        0 => "up",
        1 => "down",
        2 => "left",
        _ => "right",
    }
}

/// The side of one cell of a route cross (screen points; the cross keeps its size when the viewer zooms).
const CELL: f32 = 15.0;

/// The arrow of `direction` (0 up, 1 down, 2 left, 3 right) of the route cross on a button drawn at `rect`.
pub(crate) fn route_handle(rect: egui::Rect, direction: usize) -> egui::Rect {
    let offset = match direction {
        0 => egui::vec2(0.0, -CELL),
        1 => egui::vec2(0.0, CELL),
        2 => egui::vec2(-CELL, 0.0),
        _ => egui::vec2(CELL, 0.0),
    };
    egui::Rect::from_center_size(rect.center() + offset, egui::vec2(CELL, CELL))
}

/// The outline of an arrow cell: a square with its outer side drawn to a point.
fn arrow_shape(cell: egui::Rect, direction: usize) -> Vec<Pos2> {
    let tip = CELL * 0.35;
    match direction {
        0 => vec![
            cell.left_bottom(),
            cell.left_top() + egui::vec2(0.0, tip),
            cell.center_top(),
            cell.right_top() + egui::vec2(0.0, tip),
            cell.right_bottom(),
        ],
        1 => vec![
            cell.left_top(),
            cell.right_top(),
            cell.right_bottom() - egui::vec2(0.0, tip),
            cell.center_bottom(),
            cell.left_bottom() - egui::vec2(0.0, tip),
        ],
        2 => vec![
            cell.right_top(),
            cell.right_bottom(),
            cell.left_bottom() + egui::vec2(tip, 0.0),
            cell.left_center(),
            cell.left_top() + egui::vec2(tip, 0.0),
        ],
        _ => vec![
            cell.left_top(),
            cell.right_top() - egui::vec2(tip, 0.0),
            cell.right_center(),
            cell.right_bottom() - egui::vec2(tip, 0.0),
            cell.left_bottom(),
        ],
    }
}

impl DiscCraftApp {
    /// Whether the viewer shows route handles for `menu`: routes are shown and not routed automatically.
    pub(crate) fn route_handles_shown(&self, menu: &Menu) -> bool {
        self.show_routing && !menu.auto_route
    }

    /// Draws the route cross of every button (Encore's Show Button Routing).
    pub(crate) fn draw_route_crosses(
        &self,
        ui: &Ui,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
    ) {
        let routes = disccraft_menu::button_routes(menu, &self.session.project.settings.routing);
        let painter = ui.painter();
        let white = egui::Color32::WHITE;
        let font = egui::FontId::proportional(10.0);
        let bold = egui::FontId::proportional(11.0);
        for (index, (button, route)) in menu.buttons.iter().zip(routes).enumerate() {
            let rect = to_canvas(button.rect, frame, canvas);
            let fill = if self.selection == Selection::Menu(menu.id) && self.button_selected(index)
            {
                egui::Color32::from_rgb(0, 150, 60)
            } else {
                egui::Color32::BLACK
            };
            let stroke = egui::Stroke::new(1.0, white);
            let targets = [route.0, route.1, route.2, route.3];
            for (direction, target) in targets.iter().enumerate() {
                let cell = route_handle(rect, direction);
                painter.add(egui::Shape::convex_polygon(
                    arrow_shape(cell, direction),
                    fill,
                    stroke,
                ));
                let _ = painter.text(
                    cell.center(),
                    egui::Align2::CENTER_CENTER,
                    target.to_string(),
                    font.clone(),
                    white,
                );
            }
            let middle = egui::Rect::from_center_size(rect.center(), egui::vec2(CELL, CELL));
            painter.rect_filled(middle, 0.0, fill);
            painter.rect_stroke(middle, 0.0, stroke, egui::StrokeKind::Inside);
            let _ = painter.text(
                middle.center(),
                egui::Align2::CENTER_CENTER,
                index.saturating_add(1).to_string(),
                bold.clone(),
                white,
            );
        }
    }

    /// Draws the route being dragged (when routes are set by hand).
    pub(crate) fn draw_route_handles(
        &self,
        ui: &Ui,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
    ) {
        if !self.route_handles_shown(menu) {
            return;
        }
        let painter = ui.painter();
        let color = egui::Color32::from_rgb(120, 200, 255);
        if let Some(drag) = self.route_drag.filter(|drag| drag.menu == menu.id)
            && let Some(button) = menu.buttons.get(drag.button)
        {
            let from = route_handle(to_canvas(button.rect, frame, canvas), drag.direction).center();
            painter.line_segment([from, drag.now], egui::Stroke::new(2.0, color));
        }
    }

    /// The route handle under `point`: (button, direction).
    pub(crate) fn hit_route_handle(
        &self,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
        point: Pos2,
    ) -> Option<(usize, usize)> {
        if !self.route_handles_shown(menu) {
            return None;
        }
        menu.buttons.iter().enumerate().find_map(|(index, button)| {
            let rect = to_canvas(button.rect, frame, canvas);
            (0..4)
                .find(|direction| route_handle(rect, *direction).expand(2.0).contains(point))
                .map(|direction| (index, direction))
        })
    }

    /// Starts dragging a route from a handle (a click alone does nothing).
    pub(crate) fn press_route_handle(
        &mut self,
        menu: &Menu,
        button: usize,
        direction: usize,
        point: Pos2,
        released: bool,
    ) {
        self.select_only_button(menu.id, button);
        if released {
            return;
        }
        self.route_drag = Some(RouteDrag {
            menu: menu.id,
            button,
            direction,
            now: point,
        });
    }

    /// Follows a route drag; on release, the route goes to the button under the pointer (or nowhere when
    /// released on empty canvas).
    pub(crate) fn continue_route_drag(
        &mut self,
        ui: &Ui,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
    ) {
        let Some(mut drag) = self.route_drag else {
            return;
        };
        if drag.menu != menu.id {
            self.route_drag = None;
            return;
        }
        if let Some(point) = ui.input(|i| i.pointer.interact_pos()) {
            drag.now = point;
        }
        if !ui.input(|i| i.pointer.any_released()) {
            self.route_drag = Some(drag);
            return;
        }
        self.route_drag = None;
        let target = menu
            .buttons
            .iter()
            .enumerate()
            .find(|(index, button)| {
                *index != drag.button && to_canvas(button.rect, frame, canvas).contains(drag.now)
            })
            .map(|(index, _)| index);
        let number = target.map_or(0, |index| index.saturating_add(1));
        let _ = self.run_command(
            "button.setRoutes",
            json!({ "menu": menu.id.0, "button": drag.button, direction_name(drag.direction): number }),
        );
        if self.selection != Selection::Menu(menu.id) {
            self.select_only_button(menu.id, drag.button);
        }
    }
}
