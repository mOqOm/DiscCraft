//! Encore's "hot text" timecodes (M6.483): the time in orange with a dotted line under it. Dragging it sideways moves
//! the time a frame for every point; a click lets the time be typed (Enter goes there, Esc or a click elsewhere
//! leaves it). Used by the Timeline viewer's timeline and source times and the Monitor's current time.

use crate::theme::Shade;
use egui::{Align2, Color32, FontId, Id, Pos2, Rect, Sense, Ui};

/// The time's colour (Encore: (198,145,0); darker on the light look's ground).
pub(crate) const HOT_TIME: Shade = Shade::new(
    Color32::from_rgb(198, 145, 0),
    Color32::from_rgb(170, 100, 0),
);

/// What happened to a hot timecode this frame.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum HotChange {
    /// Nothing.
    None,
    /// Dragged by this many frames (negative: back).
    Frames(i64),
    /// A timecode was typed and Enter pressed.
    Typed(String),
}

/// Draws `text` as a hot timecode placed at `at` by `align`, in `font`, and reports a drag or a typed time.
pub(crate) fn hot_timecode(
    ui: &mut Ui,
    id: Id,
    (at, align): (Pos2, Align2),
    text: &str,
    font: FontId,
) -> HotChange {
    let color = HOT_TIME.get();
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), color);
    let rect = align.anchor_size(at, galley.size());
    let editing_key = id.with("editing");
    // The typed text, and whether the field has just opened (its text is chosen then, as Encore's is, so typing
    // replaces it).
    let editing: Option<(String, bool)> = ui.data(|data| data.get_temp(editing_key));
    if let Some((mut typed, fresh)) = editing {
        // Typing: a field in the time's place, on a child that leaves the parent's layout as it is.
        let field_id = id.with("field");
        let field_rect = rect.expand2(egui::vec2(4.0, 2.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(field_rect));
        let mut output = egui::TextEdit::singleline(&mut typed)
            .id(field_id)
            .font(font)
            .desired_width(field_rect.width())
            .margin(egui::vec2(1.0, 0.0))
            .show(&mut child);
        let field = output.response.clone();
        if fresh {
            field.request_focus();
            output
                .state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::select_all_str(&typed)));
            output.state.store(ui.ctx(), field_id);
        }
        let (enter, escape) = ui.input(|input| {
            (
                input.key_pressed(egui::Key::Enter),
                input.key_pressed(egui::Key::Escape),
            )
        });
        if !fresh && (field.lost_focus() || escape || !field.has_focus()) {
            ui.data_mut(|data| data.remove::<(String, bool)>(editing_key));
            if enter && !escape {
                return HotChange::Typed(typed);
            }
            return HotChange::None;
        }
        ui.data_mut(|data| data.insert_temp(editing_key, (typed, false)));
        return HotChange::None;
    }
    let response = ui
        .interact(rect.expand(2.0), id, Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    let painter = ui.painter();
    painter.galley(rect.min, galley, color);
    dotted_underline(painter, rect, color);
    if response.clicked() {
        ui.data_mut(|data| data.insert_temp(editing_key, (text.to_owned(), true)));
        ui.ctx().request_repaint();
        return HotChange::None;
    }
    if response.dragged() {
        // A frame a point; the part of a point left over is kept for the next frame of the drag.
        let carry_key = id.with("carry");
        let carry: f32 = ui.data(|data| data.get_temp(carry_key)).unwrap_or(0.0);
        let total = carry + response.drag_delta().x;
        let frames = total.trunc();
        ui.data_mut(|data| data.insert_temp(carry_key, total - frames));
        // `as` saturates; a drag is a few hundred points.
        let frames = frames as i64;
        if frames != 0 {
            return HotChange::Frames(frames);
        }
    }
    if response.drag_stopped() {
        ui.data_mut(|data| data.remove::<f32>(id.with("carry")));
    }
    HotChange::None
}

/// Encore's dotted line under a hot text: a pixel on, a pixel off, just under the text. The dots are squares of one
/// pixel in a mesh of their own (lines and rectangles that small are rounded away when drawn).
fn dotted_underline(painter: &egui::Painter, rect: Rect, color: Color32) {
    let ppp = painter.pixels_per_point().max(0.25);
    let pixel = 1.0 / ppp;
    // On the pixel grid.
    let snap = |v: f32| (v * ppp).round() / ppp;
    let y = snap(rect.max.y + 1.0);
    let mut mesh = egui::Mesh::default();
    let mut x = snap(rect.min.x);
    let mut count = 0_u32;
    while x < rect.max.x && count < 400 {
        mesh.add_colored_rect(
            Rect::from_min_size(egui::pos2(x, y), egui::vec2(pixel, pixel)),
            color,
        );
        x += 2.0 * pixel;
        count += 1;
    }
    let _ = painter.add(egui::Shape::mesh(mesh));
}
