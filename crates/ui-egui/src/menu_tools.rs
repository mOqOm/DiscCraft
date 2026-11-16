//! The menu viewer's tools beyond selecting and typing (Encore's toolbar): the direct selection tool, which picks a
//! part inside a button (its text or its video thumbnail) and moves it inside the button, the move tool, which
//! moves the selection from a drag anywhere on the menu, and the rotate tool, which turns a text or graphic by any
//! angle about its centre.

use crate::app::{DiscCraftApp, Selection};
use crate::menu_editor::{CanvasDrag, DragKind, to_canvas, to_frame_point};
use disccraft_project::{Button, Menu, Rect};
use serde_json::json;

/// A part inside a button that the direct selection tool picks (Encore's layers of a button set).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ButtonPart {
    /// The button's text (Encore's text layer).
    Label,
    /// The button's video thumbnail (Encore's `(%)` layer).
    Thumbnail,
}

/// A button part being dragged with the direct selection tool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PartDrag {
    pub(crate) menu: disccraft_project::MenuId,
    pub(crate) button: usize,
    pub(crate) part: ButtonPart,
    /// Where the pointer was when the drag started (canvas).
    pub(crate) origin: egui::Pos2,
    /// The part's offset (the text's or the thumbnail's) when the drag started.
    pub(crate) start_offset: [i16; 2],
    /// The part's offset now.
    pub(crate) offset: [i16; 2],
    /// The part's frame rectangle now (where it is drawn).
    pub(crate) now: Rect,
}

/// `button` with `part` moved by `offset` from its place.
fn with_part_offset(button: &Button, part: ButtonPart, offset: [i16; 2]) -> Button {
    let mut moved = button.clone();
    match part {
        ButtonPart::Label => moved.label_offset = offset,
        ButtonPart::Thumbnail => moved.thumbnail_offset = offset,
    }
    moved
}

/// The offset of `part` of `button`.
fn part_offset(button: &Button, part: ButtonPart) -> [i16; 2] {
    match part {
        ButtonPart::Label => button.label_offset,
        ButtonPart::Thumbnail => button.thumbnail_offset,
    }
}

/// Whether `rect` lies inside a frame of `frame` size.
fn inside_frame(rect: Rect, frame: (u16, u16)) -> bool {
    u32::from(rect.x) + u32::from(rect.w) <= u32::from(frame.0)
        && u32::from(rect.y) + u32::from(rect.h) <= u32::from(frame.1)
}

/// A text or graphic being turned with the rotate tool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RotateDrag {
    pub(crate) menu: disccraft_project::MenuId,
    pub(crate) target: MenuObject,
    /// The object's box (frame pixels), which turns about its centre.
    pub(crate) rect: Rect,
    /// The angle when the drag started (tenths of a degree).
    pub(crate) start: i16,
    /// The pointer's direction from the centre when the drag started (radians, clockwise on screen).
    pub(crate) origin: f32,
    /// The angle now (tenths of a degree).
    pub(crate) now: i16,
}

/// The rotate tool's angle in tenths of a degree: `start` turned by how far the pointer went round the centre
/// (from direction `origin` to `pointer`, radians clockwise on screen); with `snap` (Shift), to the nearest 15°.
pub(crate) fn dragged_angle(start: i16, origin: f32, pointer: f32, snap: bool) -> i16 {
    let turned = (pointer - origin).to_degrees() * 10.0;
    // Finite for finite directions; `as` saturates.
    let mut angle = i32::from(start).saturating_add(turned.round() as i32);
    if snap {
        angle = (angle as f32 / 150.0).round() as i32 * 150;
    }
    disccraft_project::normalized_angle(angle)
}

/// The centre of `rect` (frame pixels).
fn centre(rect: Rect) -> (f32, f32) {
    (
        f32::from(rect.x) + f32::from(rect.w) * 0.5,
        f32::from(rect.y) + f32::from(rect.h) * 0.5,
    )
}

/// The corners of `rect` turned by `angle` (tenths of a degree) about its centre, clockwise from the top left.
pub(crate) fn turned_corners(rect: Rect, angle: i16) -> [(f32, f32); 4] {
    let (cx, cy) = centre(rect);
    let (hw, hh) = (f32::from(rect.w) * 0.5, f32::from(rect.h) * 0.5);
    let (sin, cos) = (f32::from(angle) / 10.0).to_radians().sin_cos();
    [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)]
        .map(|(x, y)| (cx + x * cos - y * sin, cy + x * sin + y * cos))
}

/// The frame rectangle of `part` of `button` as it is drawn (moved, and turned with the content), when the button
/// has it.
pub(crate) fn part_rect(menu: &Menu, button: &Button, part: ButtonPart) -> Option<Rect> {
    match part {
        ButtonPart::Label => disccraft_menu::label_bounds(menu, button),
        ButtonPart::Thumbnail => button.thumbnail_placed().map(|area| {
            disccraft_project::rotated_bounds_about(
                area,
                button.content_centre(),
                button.orientation.angle,
            )
        }),
    }
}

/// The part of `button` at frame point (`x`, `y`): its text before its video thumbnail.
pub(crate) fn part_at(menu: &Menu, button: &Button, x: i32, y: i32) -> Option<ButtonPart> {
    let contains = |rect: Rect| {
        x >= i32::from(rect.x)
            && y >= i32::from(rect.y)
            && x < i32::from(rect.x) + i32::from(rect.w)
            && y < i32::from(rect.y) + i32::from(rect.h)
    };
    [ButtonPart::Label, ButtonPart::Thumbnail]
        .into_iter()
        .find(|part| part_rect(menu, button, *part).is_some_and(contains))
}

/// A part's offset moved by the drag (`dx`, `dy` in frame pixels): before the content's free turn, so the drag is
/// turned back into the content's own directions.
pub(crate) fn dragged_offset(start: [i16; 2], (dx, dy): (i32, i32), angle: i16) -> [i16; 2] {
    let (sin, cos) = (f32::from(angle) / 10.0).to_radians().sin_cos();
    // `as` is exact for frame distances.
    let (fx, fy) = (dx as f32, dy as f32);
    let (ux, uy) = (fx * cos + fy * sin, -fx * sin + fy * cos);
    let shift = |from: i16, by: f32| {
        // Finite; `as` saturates.
        (f32::from(from) + by)
            .round()
            .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
    };
    [shift(start[0], ux), shift(start[1], uy)]
}

/// An object of a menu the tools act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuObject {
    Button(usize),
    Text(usize),
    Graphic(usize),
}

/// The move tool's press: the object under the pointer becomes the selection when it is not selected yet; `None`
/// keeps the selection (also a press on nothing, which with the selection tool would deselect).
pub(crate) fn move_tool_pick(
    under: Option<MenuObject>,
    selected: &[MenuObject],
) -> Option<MenuObject> {
    under.filter(|object| !selected.contains(object))
}

/// The outline and handles of a part picked with the direct selection tool (Encore's blue).
const PART_COLOR: egui::Color32 = egui::Color32::from_rgb(0x4a, 0x90, 0xff);

impl DiscCraftApp {
    /// What is selected on `menu`: the buttons, or the text, or the graphic.
    pub(crate) fn selected_menu_objects(&self, menu: &Menu) -> Vec<MenuObject> {
        if self.selection != Selection::Menu(menu.id) {
            return Vec::new();
        }
        if let Some(text) = self.selected_menu_text(menu.id) {
            return vec![MenuObject::Text(text)];
        }
        if let Some(graphic) = self.selected_menu_graphic(menu.id) {
            return vec![MenuObject::Graphic(graphic)];
        }
        self.selected_buttons
            .iter()
            .copied()
            .filter(|index| *index < menu.buttons.len())
            .map(MenuObject::Button)
            .collect()
    }

    /// The object under `point`, in the order the selection tool finds them: buttons, then texts, then graphics.
    pub(crate) fn object_under(
        &self,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
        point: egui::Pos2,
    ) -> Option<MenuObject> {
        if let Some((index, _)) = self.hit_button(menu, frame, canvas, point) {
            return Some(MenuObject::Button(index));
        }
        if let Some(index) = self.hit_text(menu, frame, canvas, point) {
            return Some(MenuObject::Text(index));
        }
        self.hit_graphic(menu, frame, canvas, point)
            .map(|(index, _)| MenuObject::Graphic(index))
    }

    /// The move tool (Encore's Move tool): a press on an object that is not selected selects it; then a drag from
    /// wherever it started moves the selection (buttons together, a text or a graphic). A click on nothing keeps
    /// the selection.
    pub(crate) fn press_move_tool(
        &mut self,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
        point: egui::Pos2,
        released: bool,
    ) {
        let under = self.object_under(menu, frame, canvas, point);
        match move_tool_pick(under, &self.selected_menu_objects(menu)) {
            Some(MenuObject::Button(index)) => self.select_only_button(menu.id, index),
            Some(MenuObject::Text(index)) => self.select_menu_text(menu.id, index),
            Some(MenuObject::Graphic(index)) => self.select_menu_graphic(menu.id, index),
            None => {}
        }
        // A click (pressed and released at once) only selects.
        if released {
            return;
        }
        let selected = self.selected_menu_objects(menu);
        match selected.first() {
            Some(MenuObject::Text(index)) => self.press_text(menu, *index, point, false),
            Some(MenuObject::Graphic(index)) => {
                self.press_graphic(menu, *index, DragKind::Move, point, false);
            }
            Some(MenuObject::Button(_)) => self.start_buttons_move(menu, &selected, point),
            None => {}
        }
    }

    /// The direct selection tool (Encore's Direct Select tool): a press on a button selects the button and the part
    /// under the pointer (its text, else its video thumbnail), and a drag moves only that part, inside the button; a
    /// press on the rest of a button moves the button. Texts and graphics are picked and moved as with the selection
    /// tool; a press on nothing deselects.
    pub(crate) fn press_direct_selection(
        &mut self,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
        point: egui::Pos2,
        released: bool,
    ) {
        match self.object_under(menu, frame, canvas, point) {
            Some(MenuObject::Button(index)) => {
                self.select_only_button(menu.id, index);
                let Some(button) = menu.buttons.get(index) else {
                    return;
                };
                let (x, y) = to_frame_point(point, frame, canvas);
                let part = part_at(menu, button, x, y);
                self.selected_part = part;
                if released {
                    return;
                }
                match part.and_then(|part| part_rect(menu, button, part).map(|rect| (part, rect))) {
                    Some((part, rect)) => {
                        let offset = part_offset(button, part);
                        self.part_drag = Some(PartDrag {
                            menu: menu.id,
                            button: index,
                            part,
                            origin: point,
                            start_offset: offset,
                            offset,
                            now: rect,
                        });
                    }
                    None => self.start_buttons_move(menu, &[MenuObject::Button(index)], point),
                }
            }
            Some(MenuObject::Text(index)) => self.press_text(menu, index, point, released),
            Some(MenuObject::Graphic(index)) => {
                self.press_graphic(menu, index, DragKind::Move, point, released);
            }
            None => self.select(Selection::Menu(menu.id)),
        }
    }

    /// Follows a part drag: the text's or the thumbnail's offset, anywhere inside the frame (out of the button too:
    /// the button grows to take it in, as in Encore); on release sets it (one undo step). A place on another button
    /// is refused (buttons never overlap) and the part goes back.
    pub(crate) fn continue_part_drag(
        &mut self,
        ui: &egui::Ui,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
    ) {
        let Some(mut drag) = self.part_drag else {
            return;
        };
        let Some(button) = menu
            .buttons
            .get(drag.button)
            .filter(|_| drag.menu == menu.id)
        else {
            self.part_drag = None;
            return;
        };
        if let Some(point) = ui.input(|i| i.pointer.interact_pos()) {
            let (x0, y0) = to_frame_point(drag.origin, frame, canvas);
            let (x1, y1) = to_frame_point(point, frame, canvas);
            let (dx, dy) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
            let offset = dragged_offset(drag.start_offset, (dx, dy), button.orientation.angle);
            let moved = with_part_offset(button, drag.part, offset);
            // Only where the part stays inside the frame.
            if let Some(rect) =
                part_rect(menu, &moved, drag.part).filter(|rect| inside_frame(*rect, frame))
            {
                drag.offset = offset;
                drag.now = rect;
            }
        }
        if !ui.input(|i| i.pointer.any_released()) {
            self.part_drag = Some(drag);
            return;
        }
        self.part_drag = None;
        if drag.offset == part_offset(button, drag.part) {
            return;
        }
        let command = match drag.part {
            ButtonPart::Label => "button.setLabelOffset",
            ButtonPart::Thumbnail => "button.setThumbnailOffset",
        };
        let _ = self.run_command(
            command,
            json!({"menu": menu.id.0, "button": drag.button, "x": drag.offset[0], "y": drag.offset[1]}),
        );
        self.explain_overlap();
    }

    /// Outlines the part picked with the direct selection tool (Encore's blue handles on a layer inside a button
    /// set), where it is being dragged to while it is dragged.
    pub(crate) fn draw_selected_part(
        &self,
        ui: &egui::Ui,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
    ) {
        let Some(part) = self.selected_part else {
            return;
        };
        let [MenuObject::Button(index)] = self.selected_menu_objects(menu)[..] else {
            return;
        };
        let rect = match self
            .part_drag
            .filter(|drag| drag.menu == menu.id && drag.button == index)
        {
            Some(drag) => Some(drag.now),
            None => menu
                .buttons
                .get(index)
                .and_then(|button| part_rect(menu, button, part)),
        };
        let Some(rect) = rect else {
            return;
        };
        let area = to_canvas(rect, frame, canvas);
        let painter = ui.painter();
        let _ = painter.rect_stroke(
            area,
            0.0,
            egui::Stroke::new(1.0, PART_COLOR),
            egui::StrokeKind::Outside,
        );
        // Handles at the corners and the middles of the sides.
        for x in [area.left(), area.center().x, area.right()] {
            for y in [area.top(), area.center().y, area.bottom()] {
                if x == area.center().x && y == area.center().y {
                    continue;
                }
                painter.rect_filled(
                    egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(5.0, 5.0)),
                    0.0,
                    PART_COLOR,
                );
            }
        }
    }

    /// The rotate tool (Encore's Rotate tool): a press on an object that is not selected selects it; then a drag
    /// turns the selected text, graphic or button about its centre by how far the pointer goes round it (Shift: 15°
    /// steps). A button's content turns about its content box, and the button covers the turned content.
    pub(crate) fn press_rotate_tool(
        &mut self,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
        point: egui::Pos2,
        released: bool,
    ) {
        let under = self.object_under(menu, frame, canvas, point);
        match move_tool_pick(under, &self.selected_menu_objects(menu)) {
            Some(MenuObject::Button(index)) => self.select_only_button(menu.id, index),
            Some(MenuObject::Text(index)) => self.select_menu_text(menu.id, index),
            Some(MenuObject::Graphic(index)) => self.select_menu_graphic(menu.id, index),
            None => {}
        }
        if released {
            return;
        }
        let target = match self.selected_menu_objects(menu).first().copied() {
            Some(MenuObject::Text(index)) => menu.texts.get(index).map(|text| {
                (
                    MenuObject::Text(index),
                    disccraft_menu::text_rect(menu, text),
                    text.orientation.angle,
                )
            }),
            Some(MenuObject::Graphic(index)) => menu.graphics.get(index).map(|graphic| {
                (
                    MenuObject::Graphic(index),
                    graphic.rect,
                    graphic.orientation.angle,
                )
            }),
            Some(MenuObject::Button(index)) => menu.buttons.get(index).map(|button| {
                (
                    MenuObject::Button(index),
                    button.content_box(),
                    button.orientation.angle,
                )
            }),
            None => None,
        };
        let Some((target, rect, angle)) = target else {
            return;
        };
        let (cx, cy) = centre(rect);
        let (x, y) = to_frame_point(point, frame, canvas);
        // `as` is exact for frame coordinates.
        let origin = (y as f32 - cy).atan2(x as f32 - cx);
        self.rotate_drag = Some(RotateDrag {
            menu: menu.id,
            target,
            rect,
            start: angle,
            origin,
            now: angle,
        });
    }

    /// Follows a rotate drag; on release sets the angle (one undo step).
    pub(crate) fn continue_rotate_drag(
        &mut self,
        ui: &egui::Ui,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
    ) {
        let Some(mut drag) = self.rotate_drag else {
            return;
        };
        if drag.menu != menu.id {
            self.rotate_drag = None;
            return;
        }
        if let Some(point) = ui.input(|i| i.pointer.interact_pos()) {
            let (cx, cy) = centre(drag.rect);
            let (x, y) = to_frame_point(point, frame, canvas);
            let pointer = (y as f32 - cy).atan2(x as f32 - cx);
            let snap = ui.input(|i| i.modifiers.shift);
            drag.now = dragged_angle(drag.start, drag.origin, pointer, snap);
        }
        if !ui.input(|i| i.pointer.any_released()) {
            self.rotate_drag = Some(drag);
            return;
        }
        self.rotate_drag = None;
        if drag.now == drag.start {
            return;
        }
        let degrees = f64::from(drag.now) / 10.0;
        let params = match drag.target {
            MenuObject::Text(index) => {
                json!({"menu": menu.id.0, "texts": [index], "degrees": degrees})
            }
            MenuObject::Graphic(index) => {
                json!({"menu": menu.id.0, "graphics": [index], "degrees": degrees})
            }
            MenuObject::Button(index) => {
                json!({"menu": menu.id.0, "buttons": [index], "degrees": degrees})
            }
        };
        let _ = self.run_command("menu.setAngle", params);
        self.explain_overlap();
    }

    /// While turning: the object's box at the new angle and the angle beside it.
    pub(crate) fn draw_rotate_preview(
        &self,
        ui: &egui::Ui,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
    ) {
        let Some(drag) = self.rotate_drag.filter(|drag| drag.menu == menu.id) else {
            return;
        };
        let scale = crate::menu_editor::canvas_scale(frame, canvas);
        let points: Vec<egui::Pos2> = turned_corners(drag.rect, drag.now)
            .iter()
            .map(|(x, y)| canvas.min + egui::vec2(x * scale.x, y * scale.y))
            .collect();
        let painter = ui.painter();
        painter.add(egui::Shape::closed_line(
            points.clone(),
            egui::Stroke::new(1.5, PART_COLOR),
        ));
        let (cx, cy) = centre(drag.rect);
        let at = canvas.min + egui::vec2(cx * scale.x, cy * scale.y);
        painter.circle_filled(at, 2.5, PART_COLOR);
        let top = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        painter.text(
            egui::pos2(at.x, top - 6.0),
            egui::Align2::CENTER_BOTTOM,
            format!("{:.1}°", f32::from(drag.now) / 10.0),
            egui::FontId::proportional(13.0),
            egui::Color32::WHITE,
        );
    }

    /// Starts moving the selected buttons together from `point` (the last selected one leads, for snapping).
    fn start_buttons_move(&mut self, menu: &Menu, selected: &[MenuObject], point: egui::Pos2) {
        let group: Vec<(usize, Rect)> = selected
            .iter()
            .filter_map(|object| match object {
                MenuObject::Button(index) => {
                    menu.buttons.get(*index).map(|button| (*index, button.rect))
                }
                _ => None,
            })
            .collect();
        let Some(&(lead, start)) = group.last() else {
            return;
        };
        self.drag_rects = group.clone();
        self.canvas_drag = Some(CanvasDrag {
            menu: menu.id,
            button: lead,
            kind: DragKind::Move,
            start,
            origin: point,
            group,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{MenuObject, dragged_angle, dragged_offset, move_tool_pick, turned_corners};
    use disccraft_project::Rect;

    /// The rotate tool turns by how far the pointer goes round the centre, wraps, and snaps to 15° with Shift.
    #[test]
    fn the_rotate_tool_turns_by_the_pointer() {
        let quarter = std::f32::consts::FRAC_PI_2;
        assert_eq!(dragged_angle(0, 0.0, quarter, false), 900);
        assert_eq!(dragged_angle(300, quarter, 0.0, false), -600);
        assert_eq!(dragged_angle(1700, 0.0, quarter, false), -1000);
        assert_eq!(dragged_angle(0, 0.0, 0.3, true), 150);
        let rect = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 10,
        };
        // A quarter turn clockwise takes the top-left corner to the top right of the turned box.
        let [top_left, ..] = turned_corners(rect, 900);
        assert!(
            (top_left.0 - 15.0).abs() < 1e-4 && (top_left.1 + 5.0).abs() < 1e-4,
            "{top_left:?}"
        );
    }

    /// A part's drag moves its offset by the pointer's travel, turned back into the content's directions when the
    /// content is turned.
    #[test]
    fn a_part_moves_by_the_drag_in_the_contents_directions() {
        assert_eq!(dragged_offset([10, -5], (30, 20), 0), [40, 15]);
        // Content turned a quarter clockwise: dragging right moves the part up in its own directions.
        assert_eq!(dragged_offset([0, 0], (30, 0), 900), [0, -30]);
    }

    #[test]
    fn the_move_tool_keeps_or_takes_the_selection() {
        let selected = [MenuObject::Button(1), MenuObject::Button(3)];
        assert_eq!(move_tool_pick(Some(MenuObject::Button(3)), &selected), None);
        assert_eq!(move_tool_pick(None, &selected), None);
        assert_eq!(
            move_tool_pick(Some(MenuObject::Text(0)), &selected),
            Some(MenuObject::Text(0))
        );
        assert_eq!(
            move_tool_pick(Some(MenuObject::Button(2)), &[]),
            Some(MenuObject::Button(2))
        );
    }
}
