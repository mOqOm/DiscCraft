//! Text objects on menus (Encore's text tool) and the tools of the toolbar, in Encore's order: the selection tool,
//! the direct selection tool (a part inside a button), the rotate tool (any angle), the vertical text tool, the move
//! tool (drag anywhere to move the selection), the text tool (click the menu to add a text) and the zoom
//! tool (click to zoom in, Alt+click to zoom out).

use crate::app::{DiscCraftApp, Selection};
use crate::i18n::tl;
use crate::menu_editor::{to_canvas, to_frame_point};
use disccraft_project::{
    DEFAULT_TEXT_SIZE, MAX_TEXT_SIZE, MIN_TEXT_SIZE, Menu, MenuId, MenuText, Rect,
};
use egui::Ui;
use serde_json::json;

/// The numbers Encore's カーニング list offers (1/1000 em).
const KERNING_CHOICES: [i16; 14] = [-100, -75, -50, -25, -10, -5, 0, 5, 10, 25, 50, 75, 100, 200];

/// The tool used on the menu viewer (the toolbar's tool buttons).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tool {
    /// Select and move buttons and texts.
    #[default]
    Selection,
    /// Pick a part inside a button (its text or its video thumbnail) and move it inside the button (Encore's Direct
    /// Select tool).
    DirectSelection,
    /// Drag anywhere on the menu to move what is selected; pressing an object that is not selected selects it
    /// first. A click on nothing keeps the selection (Encore's Move tool).
    Move,
    /// Drag round the selected text or graphic to turn it by any angle about its centre (Encore's Rotate tool).
    Rotate,
    /// Click the menu to add a text there.
    Text,
    /// Click the menu to add a text written top to bottom there (Encore's Vertical Text tool).
    VerticalText,
    /// Click to zoom in, Alt+click to zoom out.
    Zoom,
}

impl Tool {
    /// The tools in toolbar order, with their keys.
    pub(crate) const ALL: [(Tool, egui::Key); 7] = [
        (Tool::Selection, egui::Key::V),
        (Tool::DirectSelection, egui::Key::A),
        (Tool::Move, egui::Key::M),
        (Tool::Rotate, egui::Key::R),
        (Tool::Text, egui::Key::T),
        (Tool::VerticalText, egui::Key::Y),
        (Tool::Zoom, egui::Key::Z),
    ];

    /// Draws the tool's icon: the Craft apps' shared one (FilmCraft's Selection, Type and Zoom tools) or, for the
    /// tools they have none for, DiscCraft's own ([`crate::tool_icons`]).
    pub(crate) fn paint_icon(
        self,
        painter: &egui::Painter,
        rect: egui::Rect,
        color: egui::Color32,
    ) {
        use crate::icons::Icon;
        use crate::tool_icons::Glyph;
        let shared = |icon| crate::icons::paint(painter, rect, icon, color);
        let own = |glyph| crate::tool_icons::paint(painter, rect, glyph, color);
        match self {
            Tool::Selection => shared(Icon::Selection),
            Tool::DirectSelection => own(Glyph::DirectSelection),
            Tool::Move => own(Glyph::Move),
            Tool::Rotate => own(Glyph::Rotate),
            Tool::Text => shared(Icon::Type),
            Tool::VerticalText => own(Glyph::VerticalType),
            Tool::Zoom => shared(Icon::Zoom),
        }
    }

    /// The tool's name (Encore's tooltip without the key).
    pub(crate) fn label(self) -> &'static str {
        match self {
            Tool::Selection => tl!("Selection Tool"),
            Tool::DirectSelection => tl!("Direct Select Tool"),
            Tool::Move => tl!("Move Tool"),
            Tool::Rotate => tl!("Rotate Tool"),
            Tool::Text => tl!("Text Tool"),
            Tool::VerticalText => tl!("Vertical Text Tool"),
            Tool::Zoom => tl!("Zoom Tool"),
        }
    }
}

/// A text being dragged on the menu viewer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextDrag {
    pub(crate) menu: MenuId,
    pub(crate) index: usize,
    /// Where the text was when the drag started (frame pixels).
    pub(crate) start: (u16, u16),
    /// Where the pointer was when the drag started (canvas).
    pub(crate) origin: egui::Pos2,
    /// Where the text is now (frame pixels).
    pub(crate) now: (u16, u16),
    /// A paragraph text's box being resized by its handle instead (Encore: dragging a text box's handle): its
    /// width when the drag started (the height for vertical text), and now.
    pub(crate) resize: Option<(u16, u16)>,
    /// The box's height (a vertical box's width: how far its columns reach) is being resized instead (the handle
    /// at the middle of a horizontal box's bottom edge, of a vertical box's right edge).
    pub(crate) height: bool,
}

/// The handle of a paragraph text's box height on the canvas: the middle of its bottom edge, or for vertical text
/// (whose "height" is how wide its columns reach) of its right edge.
pub(crate) fn height_handle(outline: egui::Rect, vertical: bool) -> egui::Rect {
    let at = if vertical {
        outline.right_center()
    } else {
        outline.center_bottom()
    };
    egui::Rect::from_center_size(at, egui::vec2(9.0, 9.0))
}

/// A paragraph text's box width (its height for vertical text) after its handle moved from `origin` to `point`:
/// grown or shrunk by the pointer's travel across (down, for vertical text), within the box's limits.
pub(crate) fn resized_box(
    start: u16,
    (origin, point): (egui::Pos2, egui::Pos2),
    vertical: bool,
    frame: (u16, u16),
    canvas: egui::Rect,
) -> u16 {
    let (x0, y0) = to_frame_point(origin, frame, canvas);
    let (x1, y1) = to_frame_point(point, frame, canvas);
    let delta = if vertical {
        y1.saturating_sub(y0)
    } else {
        x1.saturating_sub(x0)
    };
    let width = i32::from(start).saturating_add(delta).clamp(
        i32::from(disccraft_project::MIN_TEXT_BOX),
        i32::from(disccraft_project::MAX_TEXT_BOX),
    );
    u16::try_from(width).unwrap_or(start)
}

/// The handle of a paragraph text's box on the canvas (`outline`: the selected text's outline): the middle of its
/// right edge, or of its bottom edge for vertical text.
pub(crate) fn box_handle(outline: egui::Rect, vertical: bool) -> egui::Rect {
    let at = if vertical {
        outline.center_bottom()
    } else {
        outline.right_center()
    };
    egui::Rect::from_center_size(at, egui::vec2(9.0, 9.0))
}

/// Moves `start` by the pointer's travel from `origin` to `point`, kept inside the frame.
pub(crate) fn dragged_position(
    drag: &TextDrag,
    point: egui::Pos2,
    frame: (u16, u16),
    canvas: egui::Rect,
) -> (u16, u16) {
    let (x0, y0) = to_frame_point(drag.origin, frame, canvas);
    let (x1, y1) = to_frame_point(point, frame, canvas);
    let clamp = |start: u16, delta: i32, limit: u16| {
        let value = i32::from(start).saturating_add(delta);
        u16::try_from(value.clamp(0, i32::from(limit.saturating_sub(1)))).unwrap_or(0)
    };
    (
        clamp(drag.start.0, x1.saturating_sub(x0), frame.0),
        clamp(drag.start.1, y1.saturating_sub(y0), frame.1),
    )
}

impl DiscCraftApp {
    /// The menu's text `index`, when it exists.
    fn menu_text(&self, menu: MenuId, index: usize) -> Option<MenuText> {
        self.session.project.menu(menu)?.texts.get(index).cloned()
    }

    /// The text selected on `menu`, when there is one.
    pub(crate) fn selected_menu_text(&self, menu: MenuId) -> Option<usize> {
        if self.selection != Selection::Menu(menu) {
            return None;
        }
        let index = self.selected_text?;
        self.menu_text(menu, index).map(|_| index)
    }

    /// Selects text `index` of `menu` (the buttons are deselected) and shows its sheet.
    pub(crate) fn select_menu_text(&mut self, menu: MenuId, index: usize) {
        self.select(Selection::Menu(menu));
        self.selected_text = Some(index);
    }

    /// The text tools: add "Text" at the clicked point (the vertical one writes top to bottom, with the click at
    /// the top of the column's middle, as Encore places it), select it and put the cursor in its text field.
    ///
    /// `box_width`: the tool was dragged (Encore: a text box), so the text wraps at that width (a vertical
    /// text's columns at that height) and `x`, `y` is the box's corner.
    pub(crate) fn add_menu_text(
        &mut self,
        menu: MenuId,
        x: u16,
        y: u16,
        vertical: bool,
        box_width: Option<u16>,
    ) {
        // Half a column (the default size × 1.2) to the left of the click.
        let half_column = u16::try_from(u32::from(DEFAULT_TEXT_SIZE) * 3 / 5).unwrap_or(0);
        let x = if vertical && box_width.is_none() {
            x.saturating_sub(half_column)
        } else {
            x
        };
        let mut params =
            json!({"menu": menu.0, "text": tl!("Text"), "x": x, "y": y, "vertical": vertical});
        if let Some(fields) = params.as_object_mut() {
            if let Some(width) = box_width {
                let _ = fields.insert("boxWidth".to_owned(), json!(width));
            }
            // The characters set for the next text (the Character panel with nothing selected).
            for (key, value) in &self.next_text {
                let _ = fields.insert(key.clone(), value.clone());
            }
        }
        let added = self.run_command("menu.addText", params);
        if let Some(index) = added
            .and_then(|value| value.get("index").and_then(serde_json::Value::as_u64))
            .and_then(|index| usize::try_from(index).ok())
        {
            self.select_menu_text(menu, index);
            self.focus_rename = true;
        }
    }

    /// The format of the next text typed: the defaults with what was set for it.
    pub(crate) fn next_text_format(&self) -> disccraft_project::TextFormat {
        let mut value =
            serde_json::to_value(disccraft_project::TextFormat::default()).unwrap_or_default();
        if let Some(fields) = value.as_object_mut() {
            for (key, set) in &self.next_text {
                if fields.contains_key(key) {
                    let _ = fields.insert(key.clone(), set.clone());
                }
            }
        }
        serde_json::from_value(value).unwrap_or_default()
    }

    /// The Character panel with nothing selected (Encore: its values are the next text's): size, colour, font,
    /// style and format of the next text typed, and Reset.
    pub(crate) fn next_text_rows(&mut self, ui: &mut Ui) {
        ui.weak(tl!(
            "Nothing is selected: these are the characters of the next text typed"
        ));
        ui.add_space(4.0);
        let text = |key: &str| {
            self.next_text
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        // Read before the rows that change them.
        let font = text("font");
        let font_style = text("fontStyle");
        let color = text("color").and_then(|hex| crate::app::hex_to_rgb(&hex));
        let mut changes: Vec<(String, serde_json::Value)> = Vec::new();
        let mut size = self
            .next_text
            .get("size")
            .and_then(serde_json::Value::as_u64)
            .and_then(|size| u16::try_from(size).ok())
            .unwrap_or(DEFAULT_TEXT_SIZE);
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Size:"));
            let response = ui.add(
                egui::DragValue::new(&mut size)
                    .range(MIN_TEXT_SIZE..=MAX_TEXT_SIZE)
                    .suffix(" px"),
            );
            if response.drag_stopped() || (response.changed() && !response.dragged()) {
                changes.push(("size".to_owned(), json!(size)));
            }
        });
        let mut own = color.is_some();
        let mut rgb = crate::app::rgb_to_f32(color.unwrap_or([255, 255, 255]));
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Color:"));
            let toggled = ui.checkbox(&mut own, tl!("Own color")).changed();
            let picked = own && ui.color_edit_button_rgb(&mut rgb).changed();
            if toggled || picked {
                let value = own.then(|| {
                    let [r, g, b] = crate::app::rgb_from_f32(rgb);
                    format!("#{r:02X}{g:02X}{b:02X}")
                });
                changes.push(("color".to_owned(), json!(value)));
            }
        });
        if let Some(chosen) = self.font_row(ui, "next_text_font", font.as_deref()) {
            changes.push(("font".to_owned(), json!(chosen)));
            changes.push(("fontStyle".to_owned(), serde_json::Value::Null));
        }
        if let Some(style) = font_style_row(
            ui,
            "next_text_font_style",
            font.as_deref(),
            font_style.as_deref(),
        ) {
            changes.push(("fontStyle".to_owned(), json!(style)));
        }
        let format = self.next_text_format();
        if let Some(serde_json::Value::Object(fields)) = self.format_rows(ui, format) {
            changes.extend(fields);
        }
        for (key, value) in changes {
            if value.is_null() {
                let _ = self.next_text.remove(&key);
            } else {
                let _ = self.next_text.insert(key, value);
            }
        }
        ui.add_space(4.0);
        if ui
            .add_enabled(!self.next_text.is_empty(), egui::Button::new(tl!("Reset")))
            .on_hover_text(tl!("The next text typed gets the default characters"))
            .clicked()
        {
            self.next_text.clear();
        }
    }

    /// Edit ▸ Clear with a text selected: removes it. Returns whether a text was removed.
    pub(crate) fn remove_selected_text(&mut self) -> bool {
        let Selection::Menu(menu) = self.selection else {
            return false;
        };
        let Some(index) = self.selected_menu_text(menu) else {
            return false;
        };
        self.selected_text = None;
        let _ = self.run_command("menu.removeText", json!({"menu": menu.0, "index": index}));
        true
    }

    /// The frame rectangle of each text of `menu` (the dragged text where it is being dragged to).
    fn text_rects(&self, menu: &Menu) -> Vec<Rect> {
        menu.texts
            .iter()
            .enumerate()
            .map(|(index, text)| {
                // As drawn (turned by its free angle), moved by the drag; a box being resized at its new size.
                let drag = self
                    .text_drag
                    .filter(|drag| drag.menu == menu.id && drag.index == index);
                let mut rect = match drag.and_then(|drag| Some((drag.resize?, drag.height))) {
                    Some(((_, now), height)) => {
                        let mut resized = text.clone();
                        if height {
                            resized.box_height = Some(now);
                        } else {
                            resized.box_width = Some(now);
                        }
                        disccraft_menu::text_bounds(menu, &resized)
                    }
                    None => disccraft_menu::text_bounds(menu, text),
                };
                if let Some(drag) = drag {
                    let shift = |at: u16, now: u16, start: u16| {
                        let moved = i32::from(at) + i32::from(now) - i32::from(start);
                        u16::try_from(moved.max(0)).unwrap_or(at)
                    };
                    rect.x = shift(rect.x, drag.now.0, drag.start.0);
                    rect.y = shift(rect.y, drag.now.1, drag.start.1);
                }
                rect
            })
            .collect()
    }

    /// Outlines the selected text on the canvas (and the dragged text where it will land).
    pub(crate) fn draw_menu_texts(
        &self,
        ui: &Ui,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
    ) {
        let Some(selected) = self.selected_menu_text(menu.id) else {
            return;
        };
        let Some(rect) = self.text_rects(menu).get(selected).copied() else {
            return;
        };
        let outline = to_canvas(rect, frame, canvas).expand(2.0);
        let painter = ui.painter();
        painter.rect_stroke(
            outline,
            0.0,
            egui::Stroke::new(1.0, egui::Color32::from_rgb(120, 200, 255)),
            egui::StrokeKind::Outside,
        );
        // Small squares at the corners, like a selected object.
        for corner in [
            outline.left_top(),
            outline.right_top(),
            outline.left_bottom(),
            outline.right_bottom(),
        ] {
            painter.rect_filled(
                egui::Rect::from_center_size(corner, egui::vec2(5.0, 5.0)),
                0.0,
                egui::Color32::from_rgb(120, 200, 255),
            );
        }
        // A paragraph text's box: a handle to drag its width (its height for vertical text).
        if let Some(text) = menu
            .texts
            .get(selected)
            .filter(|text| text.box_width.is_some())
        {
            // And one to drag its height (lines past it are not drawn), a vertical box's width (columns past it).
            let handles = [
                box_handle(outline, text.vertical),
                height_handle(outline, text.vertical),
            ];
            for handle in handles {
                painter.rect_filled(handle, 0.0, egui::Color32::WHITE);
                painter.rect_stroke(
                    handle,
                    0.0,
                    egui::Stroke::new(1.0, egui::Color32::from_rgb(120, 200, 255)),
                    egui::StrokeKind::Inside,
                );
            }
            // Encore's overset mark: a red plus at the box's corner when lines are left out.
            if disccraft_menu::text_overset(menu, text) {
                let mark = egui::Rect::from_center_size(
                    outline.right_bottom() - egui::vec2(6.0, 6.0),
                    egui::vec2(9.0, 9.0),
                );
                let red = egui::Color32::from_rgb(230, 60, 50);
                painter.rect_filled(mark, 0.0, egui::Color32::WHITE);
                painter.rect_stroke(
                    mark,
                    0.0,
                    egui::Stroke::new(1.0, red),
                    egui::StrokeKind::Inside,
                );
                let stroke = egui::Stroke::new(1.5, red);
                painter.line_segment(
                    [
                        mark.center() - egui::vec2(3.0, 0.0),
                        mark.center() + egui::vec2(3.0, 0.0),
                    ],
                    stroke,
                );
                painter.line_segment(
                    [
                        mark.center() - egui::vec2(0.0, 3.0),
                        mark.center() + egui::vec2(0.0, 3.0),
                    ],
                    stroke,
                );
            }
        }
    }

    /// The selected paragraph text whose box handle is under `point` (it is pressed to resize the box), and
    /// whether it is the handle of the box's height.
    pub(crate) fn hit_text_box_handle(
        &self,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
        point: egui::Pos2,
    ) -> Option<(usize, bool)> {
        let selected = self.selected_menu_text(menu.id)?;
        let text = menu
            .texts
            .get(selected)
            .filter(|text| text.box_width.is_some() && !text.hidden && !text.locked)?;
        let rect = self.text_rects(menu).get(selected).copied()?;
        let outline = to_canvas(rect, frame, canvas).expand(2.0);
        if box_handle(outline, text.vertical)
            .expand(2.0)
            .contains(point)
        {
            return Some((selected, false));
        }
        height_handle(outline, text.vertical)
            .expand(2.0)
            .contains(point)
            .then_some((selected, true))
    }

    /// Presses on a paragraph text's box handle (`height`: the handle of the box's height): starts resizing the
    /// box (unless it was a click). A box without a height starts from the height its lines take.
    pub(crate) fn press_text_box_handle(
        &mut self,
        menu: &Menu,
        (index, height): (usize, bool),
        point: egui::Pos2,
        released: bool,
    ) {
        if released {
            return;
        }
        let Some(text) = menu.texts.get(index) else {
            return;
        };
        let side = if height {
            // Without one, from what its lines take (a vertical text's columns: their width).
            Some(text.box_height.unwrap_or_else(|| {
                let rect = disccraft_menu::text_rect(menu, text);
                let taken = if text.vertical { rect.w } else { rect.h };
                taken.clamp(
                    disccraft_project::MIN_TEXT_BOX,
                    disccraft_project::MAX_TEXT_BOX,
                )
            }))
        } else {
            text.box_width
        };
        if let Some(side) = side {
            self.text_drag = Some(TextDrag {
                menu: menu.id,
                index,
                start: (text.x, text.y),
                origin: point,
                now: (text.x, text.y),
                resize: Some((side, side)),
                height,
            });
        }
    }

    /// The text under `point`, topmost first.
    pub(crate) fn hit_text(
        &self,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
        point: egui::Pos2,
    ) -> Option<usize> {
        // A hidden or locked text (the Layers panel) is not picked.
        self.text_rects(menu)
            .iter()
            .enumerate()
            .rev()
            .filter(|(index, _)| {
                menu.texts
                    .get(*index)
                    .is_some_and(|text| !text.hidden && !text.locked)
            })
            .find(|(_, rect)| to_canvas(**rect, frame, canvas).expand(2.0).contains(point))
            .map(|(index, _)| index)
    }

    /// Presses on a text with the selection tool: selects it and starts dragging it (unless it was a click).
    pub(crate) fn press_text(
        &mut self,
        menu: &Menu,
        index: usize,
        point: egui::Pos2,
        released: bool,
    ) {
        self.select_menu_text(menu.id, index);
        if released {
            return;
        }
        if let Some(text) = menu.texts.get(index) {
            self.text_drag = Some(TextDrag {
                menu: menu.id,
                index,
                start: (text.x, text.y),
                origin: point,
                now: (text.x, text.y),
                resize: None,
                height: false,
            });
        }
    }

    /// Follows a text drag; on release moves the text (one undo step).
    pub(crate) fn continue_text_drag(
        &mut self,
        ui: &Ui,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
    ) {
        let Some(mut drag) = self.text_drag else {
            return;
        };
        if drag.menu != menu.id {
            self.text_drag = None;
            return;
        }
        if let Some(point) = ui.input(|i| i.pointer.interact_pos()) {
            match drag.resize {
                Some((start, _)) => {
                    // A box's height grows downwards, as a vertical box's column length does; a vertical box's
                    // width (its "height") rightwards, as a horizontal box's width.
                    let vertical = menu.texts.get(drag.index).is_some_and(|text| text.vertical);
                    let down = drag.height != vertical;
                    let side = resized_box(start, (drag.origin, point), down, frame, canvas);
                    drag.resize = Some((start, side));
                }
                None => drag.now = dragged_position(&drag, point, frame, canvas),
            }
        }
        if !ui.input(|i| i.pointer.any_released()) {
            self.text_drag = Some(drag);
            return;
        }
        self.text_drag = None;
        if let Some((start, side)) = drag.resize {
            let had_height = menu
                .texts
                .get(drag.index)
                .is_some_and(|text| text.box_height.is_some());
            if drag.height && (side != start || !had_height) {
                self.set_text_field(menu.id, drag.index, json!({ "boxHeight": side }));
            } else if !drag.height && side != start {
                self.set_text_field(menu.id, drag.index, json!({ "boxWidth": side }));
            }
            return;
        }
        if drag.now != drag.start {
            let _ = self.run_command(
                "menu.setText",
                json!({"menu": menu.id.0, "index": drag.index, "x": drag.now.0, "y": drag.now.1}),
            );
        }
    }

    /// The zoom tool: one step in (or out with Alt) on the menu viewer.
    pub(crate) fn zoom_menu_viewer(&mut self, zoom_in: bool) {
        self.menu_zoom = self.menu_zoom.step(zoom_in);
    }

    /// The text's property sheet: the text, its size, its colour and its position.
    pub(crate) fn text_sheet(&mut self, ui: &mut Ui, menu: MenuId, index: usize) {
        crate::properties::sheet_title(ui, tl!("Text"));
        let Some(text) = self.menu_text(menu, index) else {
            return;
        };
        let mut changed = None;
        ui.horizontal_top(|ui| {
            crate::properties::prop_label(ui, tl!("Text:"));
            let focus = std::mem::take(&mut self.focus_rename);
            let (response, value) = crate::properties::committed_text(
                ui,
                &text_field_salt(menu, index),
                &text.text,
                true,
            );
            if focus {
                response.request_focus();
            }
            changed = value.filter(|value| !value.trim().is_empty());
        });
        if let Some(value) = changed {
            self.set_text_field(menu, index, json!({ "text": value }));
        }

        self.text_character_rows(ui, menu, index);
        crate::properties::text_row(ui, tl!("Position:"), &format!("{}, {}", text.x, text.y));
        // M6.342: its layer effects, as an image's.
        self.effects_row(ui, menu, crate::effects_dialog::EffectsTarget::Text(index));
    }

    /// The character rows of text `index` of `menu` (Encore's Character panel): size, direction, colour, font,
    /// font style and formatting. In the text's property sheet and in the Character panel.
    pub(crate) fn text_character_rows(&mut self, ui: &mut Ui, menu: MenuId, index: usize) {
        let Some(stored) = self.menu_text(menu, index) else {
            return;
        };
        // M6.389: with characters selected in the text's field, the character rows show and set theirs (Encore:
        // characters selected with the text tool); else the whole text's.
        let chars = selected_chars(ui.ctx(), menu, index, &stored);
        let text = with_chars(&stored, chars.as_ref());
        if let Some(range) = &chars {
            let mut whole = false;
            ui.horizontal(|ui| {
                crate::properties::prop_label(ui, tl!("Characters:"));
                let _ = ui.label(
                    tl!("{start} to {end} (selected in the text)")
                        .replace("{start}", &(range.start + 1).to_string())
                        .replace("{end}", &range.end.to_string()),
                );
                whole = ui
                    .small_button(tl!("Whole text"))
                    .on_hover_text(tl!(
                        "Set the size, colour, font and style of the whole text instead"
                    ))
                    .clicked();
            });
            if whole {
                clear_selected_chars(ui.ctx(), menu, index);
            }
        }
        let menu_color = self
            .session
            .project
            .menu(menu)
            .map_or([255, 255, 255], |menu| menu.text_color);

        let mut size = text.size;
        let mut size_changed = false;
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Size:"));
            let response = ui.add(
                egui::DragValue::new(&mut size)
                    .range(MIN_TEXT_SIZE..=MAX_TEXT_SIZE)
                    .suffix(" px"),
            );
            size_changed = (response.drag_stopped() || (response.changed() && !response.dragged()))
                && size != text.size;
        });
        if size_changed {
            self.set_char_field(menu, index, json!({ "size": size }), chars.as_ref());
        }

        // Encore's text orientation: written across (horizontal text tool) or top to bottom (vertical one).
        let mut vertical = text.vertical;
        let mut orientation_changed = false;
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Direction:"));
            orientation_changed |= ui
                .radio_value(&mut vertical, false, tl!("Horizontal"))
                .changed();
            orientation_changed |= ui
                .radio_value(&mut vertical, true, tl!("Vertical"))
                .changed();
        });
        if orientation_changed && vertical != text.vertical {
            self.set_text_field(menu, index, json!({ "vertical": vertical }));
        }

        // Paragraph text (Encore: a box drawn with the text tool): the width the lines wrap at.
        let mut boxed = text.box_width.is_some();
        let mut box_width = text.box_width.unwrap_or(320);
        let mut box_changed = false;
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Wrap:"));
            box_changed |= ui
                .checkbox(&mut boxed, tl!("At a width"))
                .on_hover_text(tl!(
                    "Paragraph text: the lines wrap at this width (a vertical text's columns at this height). Dragging with a text tool draws such a box"
                ))
                .changed();
            let response = ui.add_enabled(
                boxed,
                egui::DragValue::new(&mut box_width)
                    .range(disccraft_project::MIN_TEXT_BOX..=disccraft_project::MAX_TEXT_BOX)
                    .suffix(" px"),
            );
            box_changed |= (response.drag_stopped() || (response.changed() && !response.dragged()))
                && Some(box_width) != text.box_width;
        });
        if box_changed {
            let value = if boxed {
                json!(box_width)
            } else {
                serde_json::Value::Null
            };
            if boxed != text.box_width.is_some() || boxed {
                self.set_text_field(menu, index, json!({ "boxWidth": value }));
            }
        }
        // The box's height (Encore: an area text's height): the lines past it are not drawn.
        let mut limited = text.box_height.is_some();
        let mut box_height = text.box_height.unwrap_or(120);
        let mut height_changed = false;
        let overset = self
            .session
            .project
            .menu(menu)
            .is_some_and(|found| disccraft_menu::text_overset(found, &text));
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, "");
            ui.add_enabled_ui(boxed, |ui| {
                height_changed |= ui
                    .checkbox(&mut limited, tl!("Box height"))
                    .on_hover_text(tl!(
                        "The lines past this height are not drawn (a vertical text: the columns past this width)"
                    ))
                    .changed();
                let response = ui.add_enabled(
                    limited,
                    egui::DragValue::new(&mut box_height)
                        .range(disccraft_project::MIN_TEXT_BOX..=disccraft_project::MAX_TEXT_BOX)
                        .suffix(" px"),
                );
                height_changed |= (response.drag_stopped()
                    || (response.changed() && !response.dragged()))
                    && Some(box_height) != text.box_height;
            });
        });
        if overset {
            ui.horizontal(|ui| {
                crate::properties::prop_label(ui, "");
                ui.colored_label(
                    egui::Color32::from_rgb(230, 80, 60),
                    tl!("Some of the text does not fit in the box"),
                );
            });
        }
        if height_changed {
            let value = if limited {
                json!(box_height)
            } else {
                serde_json::Value::Null
            };
            self.set_text_field(menu, index, json!({ "boxHeight": value }));
        }

        let mut own = text.color.is_some();
        let mut color = text.color.unwrap_or(menu_color);
        let mut color_changed = None;
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Color:"));
            if ui.checkbox(&mut own, tl!("Own color")).changed() {
                color_changed = Some(own.then_some(color));
            }
            if own {
                let mut rgb = crate::app::rgb_to_f32(color);
                if ui.color_edit_button_rgb(&mut rgb).changed() {
                    color = crate::app::rgb_from_f32(rgb);
                    color_changed = Some(Some(color));
                }
            }
        });
        if let Some(color) = color_changed {
            let value = color.map(|[r, g, b]| format!("#{r:02X}{g:02X}{b:02X}"));
            self.set_char_field(menu, index, json!({ "color": value }), chars.as_ref());
        }

        if let Some(font) = self.font_row(ui, "menu_text_font", text.font.as_deref()) {
            // A style the new family does not have goes back to its regular face.
            let keeps_style = text
                .font_style
                .as_ref()
                .is_none_or(|style| disccraft_menu::font_styles(font.as_deref()).contains(style));
            let fields = if keeps_style {
                json!({ "font": font })
            } else {
                json!({ "font": font, "fontStyle": null })
            };
            self.set_char_field(menu, index, fields, chars.as_ref());
        }
        if let Some(style) = font_style_row(
            ui,
            "menu_text_font_style",
            text.font.as_deref(),
            text.font_style.as_deref(),
        ) {
            self.set_char_field(menu, index, json!({ "fontStyle": style }), chars.as_ref());
        }

        let change = self.format_rows(ui, text.format);
        if let Some(fields) = change {
            self.set_char_field(menu, index, fields, chars.as_ref());
        }
    }

    /// [`Self::set_text_field`] for character fields: of the characters `chars` alone when some are selected
    /// (M6.389; the engine sets the paragraph fields among them for the whole text).
    fn set_char_field(
        &mut self,
        menu: MenuId,
        index: usize,
        mut fields: serde_json::Value,
        chars: Option<&std::ops::Range<usize>>,
    ) {
        if let (Some(range), Some(object)) = (chars, fields.as_object_mut()) {
            let _ = object.insert("from".to_owned(), json!(range.start));
            let _ = object.insert("to".to_owned(), json!(range.end));
        }
        self.set_text_field(menu, index, fields);
    }

    /// Encore の文字パネルのうち DiscCraft が描けるもの（テキストとボタンの文字で共通）: スタイル、行揃えと
    /// アンチエイリアス、トラッキング、行送り、カーニング、垂直比率 / 水平比率、ベースライン。変えた項目を
    /// コマンドの引数（`menu.setText` / `button.setLabelFormat`）で返す。
    pub(crate) fn format_rows(
        &mut self,
        ui: &mut Ui,
        format: disccraft_project::TextFormat,
    ) -> Option<serde_json::Value> {
        // Encore の文字パネルのうち DiscCraft が描けるもの: 太字 / 斜体 / 下線 / 打ち消し線 / 大文字、行揃え、
        // トラッキング、行送り。
        let mut change: Option<serde_json::Value> = None;
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Style:"));
            let toggles: [(&str, bool, &str, &str); 4] = [
                ("B", format.bold, "bold", tl!("Faux bold")),
                ("I", format.italic, "italic", tl!("Faux italic")),
                ("U", format.underline, "underline", tl!("Underline")),
                (
                    "S",
                    format.strikethrough,
                    "strikethrough",
                    tl!("Strikethrough"),
                ),
            ];
            for (glyph, on, key, hint) in toggles {
                let label = match key {
                    "bold" => egui::RichText::new(glyph).strong(),
                    "italic" => egui::RichText::new(glyph).italics(),
                    "underline" => egui::RichText::new(glyph).underline(),
                    _ => egui::RichText::new(glyph).strikethrough(),
                };
                if ui.selectable_label(on, label).on_hover_text(hint).clicked() {
                    change = Some(json!({ key: !on }));
                }
            }
            ui.separator();
            use disccraft_project::TextCaps;
            for (glyph, caps, hint) in [
                ("TT", TextCaps::All, tl!("All caps")),
                ("Tt", TextCaps::Small, tl!("Small caps")),
            ] {
                let on = format.caps == caps;
                if ui.selectable_label(on, glyph).on_hover_text(hint).clicked() {
                    let next = if on { TextCaps::Normal } else { caps };
                    change = Some(json!({ "caps": next.as_str() }));
                }
            }
            // Encore の T¹ / T₁: 上付き文字 / 下付き文字（もう一度押すと戻る）。
            use disccraft_project::TextScript;
            for (glyph, script, hint) in [
                ("T²", TextScript::Superscript, tl!("Superscript")),
                ("T₂", TextScript::Subscript, tl!("Subscript")),
            ] {
                let on = format.script == script;
                if ui.selectable_label(on, glyph).on_hover_text(hint).clicked() {
                    let next = if on { TextScript::Normal } else { script };
                    change = Some(json!({ "script": next.as_str() }));
                }
            }
        });
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Alignment:"));
            use disccraft_project::TextAlign;
            for (glyph, align, hint) in [
                ("⇤", TextAlign::Left, tl!("Align left")),
                ("↔", TextAlign::Center, tl!("Align center")),
                ("⇥", TextAlign::Right, tl!("Align right")),
                ("≡", TextAlign::Justify, tl!("Justify last left")),
            ] {
                if ui
                    .selectable_label(format.align == align, glyph)
                    .on_hover_text(hint)
                    .clicked()
                    && format.align != align
                {
                    change = Some(json!({ "align": align.as_str() }));
                }
            }
            // Encore の行揃えの右の「aa」: アンチエイリアス（なし / 鮮明 / 強く / 滑らかに / シャープ）。
            ui.separator();
            let _ = ui.label("aa").on_hover_text(tl!("Anti-aliasing"));
            use disccraft_project::TextAntialias;
            let name = |mode: TextAntialias| match mode {
                TextAntialias::None => tl!("None"),
                TextAntialias::Crisp => tl!("Crisp"),
                TextAntialias::Strong => tl!("Strong"),
                TextAntialias::Smooth => tl!("Smooth"),
                TextAntialias::Sharp => tl!("Sharp"),
            };
            egui::ComboBox::from_id_salt("menu_text_antialias")
                .width(90.0)
                .selected_text(name(format.antialias))
                .show_ui(ui, |ui| {
                    for mode in TextAntialias::ALL {
                        if ui
                            .selectable_label(format.antialias == mode, name(mode))
                            .clicked()
                            && format.antialias != mode
                        {
                            change = Some(json!({ "antialias": mode.as_str() }));
                        }
                    }
                });
        });
        let mut tracking = format.tracking;
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Tracking:"));
            let response = ui.add(
                egui::DragValue::new(&mut tracking)
                    .range(disccraft_project::MIN_TRACKING..=disccraft_project::MAX_TRACKING)
                    .speed(5),
            );
            if (response.drag_stopped() || (response.changed() && !response.dragged()))
                && tracking != format.tracking
            {
                change = Some(json!({ "tracking": tracking }));
            }
        });
        // Encore の 行送り: 自動（サイズの 120 %）か、行のベースラインの間隔（px）。▾ は Encore の一覧（6〜72）。
        // 前の版の「行送りに足すピクセル」は、行送りを選ぶと 0 に戻す。
        let mut pitch = format.line_pitch.unwrap_or(0);
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Leading:"));
            let auto = tl!("Automatic");
            let response = ui
                .add(
                    egui::DragValue::new(&mut pitch)
                        .range(0..=disccraft_project::MAX_LINE_PITCH)
                        .custom_formatter(move |value, _| {
                            if value < 0.5 {
                                auto.to_owned()
                            } else {
                                format!("{value:.0} px")
                            }
                        })
                        .custom_parser(move |text| {
                            let text = text.trim().trim_end_matches("px").trim();
                            if text.is_empty() || text == auto || text.eq_ignore_ascii_case("auto")
                            {
                                Some(0.0)
                            } else {
                                text.parse::<f64>().ok()
                            }
                        }),
                )
                .on_hover_text(tl!(
                    "How far apart the baselines of the lines are; Automatic: 120 % of the size"
                ));
            let picked = |pitch: Option<u16>| json!({ "linePitch": pitch, "leading": 0 });
            if (response.drag_stopped() || (response.changed() && !response.dragged()))
                && pitch != format.line_pitch.unwrap_or(0)
            {
                change = Some(picked((pitch > 0).then_some(pitch)));
            }
            let _ = ui.menu_button("▾", |ui| {
                if ui
                    .selectable_label(format.line_pitch.is_none(), auto)
                    .clicked()
                {
                    change = Some(picked(None));
                    ui.close();
                }
                for px in disccraft_project::LINE_PITCHES {
                    if ui
                        .selectable_label(format.line_pitch == Some(px), format!("{px} px"))
                        .clicked()
                    {
                        change = Some(picked(Some(px)));
                        ui.close();
                    }
                }
            });
            if format.leading != 0 {
                let _ = ui
                    .weak(format!("{:+} px", format.leading))
                    .on_hover_text(tl!(
                        "Extra leading from an older project: choosing a leading takes it away"
                    ));
            }
        });
        // Encore の 詰め（日本語の文字のまわりの空きを詰める割合）と 文字組み（約物の幅）。
        let mut tsume = format.tsume;
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Tsume:"));
            let response = ui
                .add(
                    egui::DragValue::new(&mut tsume)
                        .range(0..=disccraft_project::MAX_TSUME)
                        .suffix(" %"),
                )
                .on_hover_text(tl!(
                    "Takes this much of the blank around Japanese characters out"
                ));
            if (response.drag_stopped() || (response.changed() && !response.dragged()))
                && tsume != format.tsume
            {
                change = Some(json!({ "tsume": tsume }));
            }
        });
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Mojikumi:"));
            use disccraft_project::TextMojikumi;
            let name = |mode: TextMojikumi| match mode {
                TextMojikumi::None => tl!("None"),
                TextMojikumi::PunctuationHalf => tl!("Half-width punctuation"),
                TextMojikumi::LineEndHalf => tl!("Half-width punctuation at line ends"),
                TextMojikumi::LineEndFull => tl!("Full-width punctuation at line ends"),
                TextMojikumi::PunctuationFull => tl!("Full-width punctuation"),
            };
            egui::ComboBox::from_id_salt("menu_text_mojikumi")
                .width(170.0)
                .selected_text(name(format.mojikumi))
                .show_ui(ui, |ui| {
                    for mode in TextMojikumi::ALL {
                        if ui
                            .selectable_label(format.mojikumi == mode, name(mode))
                            .clicked()
                            && format.mojikumi != mode
                        {
                            change = Some(json!({ "mojikumi": mode.as_str() }));
                        }
                    }
                });
        });
        // Encore の 禁則処理（段落テキストの行頭・行末に置かない文字）。
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Kinsoku:"));
            use disccraft_project::TextKinsoku;
            let name = |mode: TextKinsoku| match mode {
                TextKinsoku::None => tl!("None"),
                TextKinsoku::Strong => tl!("Strong kinsoku"),
                TextKinsoku::Weak => tl!("Weak kinsoku"),
            };
            egui::ComboBox::from_id_salt("menu_text_kinsoku")
                .width(170.0)
                .selected_text(name(format.kinsoku))
                .show_ui(ui, |ui| {
                    for mode in TextKinsoku::ALL {
                        if ui
                            .selectable_label(format.kinsoku == mode, name(mode))
                            .on_hover_text(tl!(
                                "Where the lines of a text that wraps at a width may not break"
                            ))
                            .clicked()
                            && format.kinsoku != mode
                        {
                            change = Some(json!({ "kinsoku": mode.as_str() }));
                        }
                    }
                });
        });
        // Encore の 禁則方法（追い込み優先 / 追い出し優先 / 追い出しのみ）と ぶら下がり。
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Kinsoku method:"));
            use disccraft_project::TextKinsokuMethod;
            let name = |mode: TextKinsokuMethod| match mode {
                TextKinsokuMethod::PushInFirst => tl!("Push In First"),
                TextKinsokuMethod::PushOutFirst => tl!("Push Out First"),
                TextKinsokuMethod::PushOutOnly => tl!("Push Out Only"),
            };
            ui.add_enabled_ui(format.kinsoku != disccraft_project::TextKinsoku::None, |ui| {
                egui::ComboBox::from_id_salt("menu_text_kinsoku_method")
                    .width(170.0)
                    .selected_text(name(format.kinsoku_method))
                    .show_ui(ui, |ui| {
                        for mode in TextKinsokuMethod::ALL {
                            if ui
                                .selectable_label(format.kinsoku_method == mode, name(mode))
                                .clicked()
                                && format.kinsoku_method != mode
                            {
                                change = Some(json!({ "kinsokuMethod": mode.as_str() }));
                            }
                        }
                    })
                    .response
                    .on_hover_text(tl!(
                        "How a character that may not start a line is kept off it: taken into the line above, or the character before it taken down"
                    ));
            });
        });
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, "");
            let mut hanging = format.hanging_punctuation;
            if ui
                .checkbox(&mut hanging, tl!("Hanging punctuation"))
                .on_hover_text(tl!(
                    "A full-width comma or full stop that does not fit hangs past the end of the line"
                ))
                .changed()
            {
                change = Some(json!({ "hangingPunctuation": hanging }));
            }
        });
        // Encore's カーニング (M6.344): メトリクス, or a number between every two characters (1/1000 em).
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Kerning:"));
            let shown = if format.kerning {
                tl!("Metrics").to_owned()
            } else {
                format.manual_kerning.to_string()
            };
            egui::ComboBox::from_id_salt("text_kerning")
                .width(90.0)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(format.kerning, tl!("Metrics"))
                        .clicked()
                    {
                        change = Some(json!({ "kerning": true }));
                    }
                    for value in KERNING_CHOICES {
                        let chosen = !format.kerning && format.manual_kerning == value;
                        if ui.selectable_label(chosen, value.to_string()).clicked() {
                            change = Some(json!({ "kerning": value }));
                        }
                    }
                });
            if !format.kerning {
                let mut value = format.manual_kerning;
                let dragged = ui.add(
                    egui::DragValue::new(&mut value)
                        .range(disccraft_project::MIN_TRACKING..=disccraft_project::MAX_TRACKING),
                );
                if (dragged.drag_stopped() || dragged.lost_focus())
                    && value != format.manual_kerning
                {
                    change = Some(json!({ "kerning": value }));
                }
            }
        });
        // Encore の 垂直比率 / 水平比率。
        for (label, key, current) in [
            (
                tl!("Vertical scale:"),
                "verticalScale",
                format.vertical_scale,
            ),
            (
                tl!("Horizontal scale:"),
                "horizontalScale",
                format.horizontal_scale,
            ),
        ] {
            let mut value = current;
            ui.horizontal(|ui| {
                crate::properties::prop_label(ui, label);
                let response = ui.add(
                    egui::DragValue::new(&mut value)
                        .range(
                            disccraft_project::MIN_TEXT_SCALE..=disccraft_project::MAX_TEXT_SCALE,
                        )
                        .suffix(" %"),
                );
                if (response.drag_stopped() || (response.changed() && !response.dragged()))
                    && value != current
                {
                    change = Some(json!({ key: value }));
                }
            });
        }
        let mut baseline = format.baseline;
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Baseline:"));
            let response = ui.add(
                egui::DragValue::new(&mut baseline)
                    .range(disccraft_project::MIN_BASELINE..=disccraft_project::MAX_BASELINE)
                    .suffix(" px"),
            );
            if (response.drag_stopped() || (response.changed() && !response.dragged()))
                && baseline != format.baseline
            {
                change = Some(json!({ "baseline": baseline }));
            }
        });
        change
    }

    /// The font families on this computer (read once).
    fn font_families(&mut self) -> std::sync::Arc<Vec<String>> {
        self.font_families
            .get_or_insert_with(|| std::sync::Arc::new(disccraft_menu::font_families()))
            .clone()
    }

    /// A "Font:" row: the default font or a family on this computer. Returns the choice (`None` in the
    /// inner option is the default font) when it changes.
    pub(crate) fn font_row(
        &mut self,
        ui: &mut Ui,
        salt: &str,
        current: Option<&str>,
    ) -> Option<Option<String>> {
        let families = self.font_families();
        let mut chosen = None;
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Font:"));
            let shown = current.map_or_else(|| tl!("Default font").to_owned(), str::to_owned);
            egui::ComboBox::from_id_salt(salt)
                .width(200.0)
                .height(320.0)
                // 検索の欄を押しても閉じない。選んだら閉じる。
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    // フォントは数百あるので、上の欄で名前を絞り込む。
                    let filter_id = egui::Id::unique("font_filter").with(salt);
                    let mut filter = ui
                        .data(|data| data.get_temp::<String>(filter_id))
                        .unwrap_or_default();
                    let search = ui.add(
                        egui::TextEdit::singleline(&mut filter)
                            .hint_text(tl!("Search fonts"))
                            .desired_width(180.0),
                    );
                    if search.changed() {
                        ui.data_mut(|data| data.insert_temp(filter_id, filter.clone()));
                    }
                    let needle = filter.trim().to_lowercase();
                    if ui
                        .selectable_label(current.is_none(), tl!("Default font"))
                        .clicked()
                    {
                        chosen = Some(None);
                        ui.close();
                    }
                    for family in families.iter().filter(|family| {
                        needle.is_empty() || family.to_lowercase().contains(&needle)
                    }) {
                        if ui
                            .selectable_label(current == Some(family.as_str()), family)
                            .clicked()
                        {
                            chosen = Some(Some(family.clone()));
                            ui.close();
                        }
                    }
                });
            if current.is_some_and(|font| !families.iter().any(|family| family == font)) {
                let _ = ui
                    .small(tl!("(not on this computer)"))
                    .on_hover_text(tl!("The default font is used instead"));
            }
        });
        chosen.filter(|choice: &Option<String>| choice.as_deref() != current)
    }

    fn set_text_field(&mut self, menu: MenuId, index: usize, fields: serde_json::Value) {
        let mut params = json!({"menu": menu.0, "index": index});
        if let (Some(params), Some(fields)) = (params.as_object_mut(), fields.as_object()) {
            for (key, value) in fields {
                let _ = params.insert(key.clone(), value.clone());
            }
        }
        let _ = self.run_command("menu.setText", params);
    }

    /// The tool buttons at the left of the toolbar, and their keys (V, T, Z) when no text field has the keyboard.
    /// Encore のツールバーの「メニューを Photoshop で編集」に当たるボタン。ビューアにメニューがあるときだけ押せる。
    /// 絵は PhotoCraft のアプリのアイコン（assets/icons/photocraft-48.png。押せないときは薄く）。
    fn edit_in_photocraft_button(&mut self, ui: &mut Ui) {
        let enabled = self.viewer_menu().is_some();
        let sense = if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(26.0, 22.0), sense);
        let response = response.on_hover_text(crate::actions::Action::EditMenuInPhotoCraft.label());
        let visuals = ui.style().interact(&response);
        if enabled && response.hovered() {
            ui.painter().rect(
                rect,
                2.0,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        if self.photocraft_icon.is_none() {
            self.photocraft_icon = photocraft_icon(ui.ctx());
        }
        if let Some(icon) = &self.photocraft_icon {
            let tint = if enabled {
                egui::Color32::WHITE
            } else {
                egui::Color32::from_white_alpha(90)
            };
            ui.painter().image(
                icon.id(),
                egui::Rect::from_center_size(rect.center(), egui::vec2(18.0, 18.0)),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                tint,
            );
        }
        if enabled && response.clicked() {
            self.run_action(ui, crate::actions::Action::EditMenuInPhotoCraft);
        }
    }

    pub(crate) fn tool_buttons(&mut self, ui: &mut Ui) {
        let typing = ui.ctx().egui_wants_keyboard_input();
        for (tool, key) in Tool::ALL {
            if !typing && ui.input(|i| i.modifiers.is_none() && i.key_pressed(key)) {
                self.tool = tool;
            }
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(26.0, 22.0), egui::Sense::click());
            let response = response.on_hover_text(format!("{} ({})", tool.label(), key.name()));
            if response.clicked() {
                self.tool = tool;
            }
            let selected = self.tool == tool;
            let visuals = ui.style().interact_selectable(&response, selected);
            if selected || response.hovered() {
                ui.painter().rect(
                    rect,
                    2.0,
                    visuals.weak_bg_fill,
                    visuals.bg_stroke,
                    egui::StrokeKind::Inside,
                );
            }
            let side = rect.height() - 6.0;
            tool.paint_icon(
                ui.painter(),
                egui::Rect::from_center_size(rect.center(), egui::vec2(side, side)),
                visuals.fg_stroke.color,
            );
        }
        ui.separator();
        self.edit_in_photocraft_button(ui);
        // プレビュー（ビルド結果をモニターで開く）: Craft の共通の再生のアイコン。
        let response = crate::icons::icon_button(ui, crate::icons::Icon::Play, false)
            .on_hover_text(crate::actions::Action::Preview.label());
        if response.clicked() {
            self.run_action(ui, crate::actions::Action::Preview);
        }
    }
}

/// The font style list under the font (Encore's second font list: R, B, ...). Lists the family's styles when it
/// is opened; `Some(None)` chooses the regular face.
/// The salt of the field text `index` of `menu` is typed in (one per text, so that a selection in it stays with
/// its text).
fn text_field_salt(menu: MenuId, index: usize) -> String {
    format!("menu_text_value_{}_{index}", menu.0)
}

/// The characters selected in the field of text `index` of `menu` (M6.389), when some are and they are inside
/// `text`: counted in characters.
fn selected_chars(
    ctx: &egui::Context,
    menu: MenuId,
    index: usize,
    text: &MenuText,
) -> Option<std::ops::Range<usize>> {
    let id = egui::Id::unique(text_field_salt(menu, index).as_str());
    let range = egui::TextEdit::load_state(ctx, id)?.cursor.char_range()?;
    let (a, b): (usize, usize) = (range.primary.index.into(), range.secondary.index.into());
    let (start, end) = (a.min(b), a.max(b));
    (start < end && end <= text.text.chars().count()).then_some(start..end)
}

/// Lets go of the characters selected in the field of text `index` of `menu`: the character rows set the whole
/// text again.
fn clear_selected_chars(ctx: &egui::Context, menu: MenuId, index: usize) {
    let id = egui::Id::unique(text_field_salt(menu, index).as_str());
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        state.cursor.set_char_range(None);
        state.store(ctx, id);
    }
}

/// `text` as its characters `chars` (when some are selected) are set: its character fields replaced by those the
/// first of them has of its own (M6.389).
fn with_chars(text: &MenuText, chars: Option<&std::ops::Range<usize>>) -> MenuText {
    let Some(range) = chars else {
        return text.clone();
    };
    let own = text.span_style_at(range.start);
    let mut shown = text.clone();
    if let Some(size) = own.size {
        shown.size = size;
    }
    if let Some(color) = own.color {
        shown.color = Some(color);
    }
    if own.font.is_some() {
        shown.font.clone_from(&own.font);
    }
    if own.font_style.is_some() {
        shown.font_style.clone_from(&own.font_style);
    }
    let format = &mut shown.format;
    format.bold = own.bold.unwrap_or(format.bold);
    format.italic = own.italic.unwrap_or(format.italic);
    format.underline = own.underline.unwrap_or(format.underline);
    format.strikethrough = own.strikethrough.unwrap_or(format.strikethrough);
    format.caps = own.caps.unwrap_or(format.caps);
    format.tracking = own.tracking.unwrap_or(format.tracking);
    format.baseline = own.baseline.unwrap_or(format.baseline);
    shown
}

pub(crate) fn font_style_row(
    ui: &mut Ui,
    salt: &str,
    family: Option<&str>,
    current: Option<&str>,
) -> Option<Option<String>> {
    const REGULAR: &str = "Regular";
    let mut chosen = None;
    ui.horizontal(|ui| {
        crate::properties::prop_label(ui, tl!("Font style:"));
        egui::ComboBox::from_id_salt(salt)
            .width(200.0)
            .selected_text(current.unwrap_or(REGULAR))
            .show_ui(ui, |ui| {
                let styles = disccraft_menu::font_styles(family);
                for style in &styles {
                    let on = current.map_or(style == REGULAR, |current| current == style);
                    if ui.selectable_label(on, style).clicked() {
                        chosen = Some((style != REGULAR).then(|| style.clone()));
                    }
                }
            });
    });
    chosen.filter(|choice: &Option<String>| choice.as_deref() != current)
}

/// PhotoCraft's app icon (48 px, MIT OR Apache-2.0; see ATTRIBUTION.md) as a texture; `None` if it cannot be read.
fn photocraft_icon(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let (width, height, rgba) = decode_rgba_png(PHOTOCRAFT_ICON)?;
    let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba);
    Some(ctx.load_texture("photocraft_icon", image, egui::TextureOptions::LINEAR))
}

/// The PhotoCraft app icon, built in.
const PHOTOCRAFT_ICON: &[u8] = include_bytes!("../../../assets/icons/photocraft-48.png");

/// An 8-bit RGBA (or RGB) PNG as (width, height, RGBA); `None` for anything else or a picture over 256 px.
fn decode_rgba_png(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().ok()?;
    let (color, depth) = reader.output_color_type();
    if depth != png::BitDepth::Eight {
        return None;
    }
    let info = reader.info();
    let (width, height) = (
        usize::try_from(info.width).ok()?,
        usize::try_from(info.height).ok()?,
    );
    if width == 0 || height == 0 || width > 256 || height > 256 {
        return None;
    }
    let mut data = vec![0_u8; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut data).ok()?;
    let data = data.get(..frame.buffer_size())?;
    let rgba = match color {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        _ => return None,
    };
    (rgba.len() == width * height * 4).then_some((width, height, rgba))
}

#[cfg(test)]
mod tests {
    /// M6.389: the characters selected in a text's field are the ones the character rows set, shown with their own
    /// formatting; a selection of another text's field, an empty one or one past the text is not, and letting go
    /// of it leaves the whole text.
    #[test]
    fn the_characters_selected_in_the_field_are_the_ones_set() {
        use egui::text::{CCursor, CCursorRange};
        let ctx = egui::Context::default();
        let menu = disccraft_project::MenuId(1);
        let mut text = hello();
        text.style_chars(6..11, &|style| style.size = Some(60));
        let select = |index: usize, from: usize, to: usize| {
            let id = egui::Id::unique(super::text_field_salt(menu, index).as_str());
            let mut state = egui::TextEdit::load_state(&ctx, id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(
                CCursor::new(from),
                CCursor::new(to),
            )));
            state.store(&ctx, id);
        };
        assert_eq!(super::selected_chars(&ctx, menu, 0, &text), None);
        // Selected backwards: the same characters.
        select(0, 9, 6);
        assert_eq!(super::selected_chars(&ctx, menu, 0, &text), Some(6..9));
        assert_eq!(super::selected_chars(&ctx, menu, 1, &text), None);
        let shown = super::with_chars(&text, Some(&(6..9)));
        assert_eq!(shown.size, 60);
        assert_eq!(super::with_chars(&text, None).size, text.size);
        select(0, 4, 4);
        assert_eq!(super::selected_chars(&ctx, menu, 0, &text), None);
        select(0, 4, 40);
        assert_eq!(super::selected_chars(&ctx, menu, 0, &text), None);
        select(0, 0, 5);
        super::clear_selected_chars(&ctx, menu, 0);
        assert_eq!(super::selected_chars(&ctx, menu, 0, &text), None);
    }

    /// A text "Hello world" of 32 px.
    fn hello() -> disccraft_project::MenuText {
        disccraft_project::MenuText {
            effects: Default::default(),
            box_width: None,
            box_height: None,
            text: "Hello world".to_owned(),
            x: 0,
            y: 0,
            size: 32,
            color: None,
            font: None,
            font_style: None,
            format: Default::default(),
            shadow: None,
            orientation: Default::default(),
            hidden: false,
            locked: false,
            vertical: false,
            spans: Vec::new(),
        }
    }

    /// The built-in PhotoCraft icon reads as a 48 px RGBA picture; other bytes do not.
    #[test]
    fn the_photocraft_icon_is_built_in() {
        let decoded = super::decode_rgba_png(super::PHOTOCRAFT_ICON);
        assert_eq!(
            decoded.as_ref().map(|(w, h, rgba)| (*w, *h, rgba.len())),
            Some((48, 48, 48 * 48 * 4))
        );
        assert!(super::decode_rgba_png(b"not a png").is_none());
    }

    use super::{TextDrag, box_handle, dragged_position, height_handle, resized_box};
    use disccraft_project::MenuId;

    /// A paragraph text's box handle: at the middle of the right edge (the bottom for vertical text); dragging it
    /// widens or narrows the box by the pointer's travel, within the box's limits.
    #[test]
    fn a_text_box_is_resized_by_its_handle() {
        let canvas = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(720.0, 480.0));
        let outline = egui::Rect::from_min_max(egui::pos2(100.0, 50.0), egui::pos2(300.0, 90.0));
        assert_eq!(box_handle(outline, false).center(), egui::pos2(300.0, 70.0));
        assert_eq!(box_handle(outline, true).center(), egui::pos2(200.0, 90.0));
        // M6.303: a horizontal box's height has its handle at the middle of the bottom edge; M6.317: a vertical
        // box's width (how far its columns reach) at the middle of the right edge.
        assert_eq!(
            height_handle(outline, false).center(),
            egui::pos2(200.0, 90.0)
        );
        assert_eq!(
            height_handle(outline, true).center(),
            egui::pos2(300.0, 70.0)
        );
        let from = egui::pos2(300.0, 70.0);
        let frame = (720, 480);
        assert_eq!(
            resized_box(200, (from, egui::pos2(360.0, 90.0)), false, frame, canvas),
            260
        );
        assert_eq!(
            resized_box(200, (from, egui::pos2(250.0, 70.0)), false, frame, canvas),
            150
        );
        // Down for vertical text.
        assert_eq!(
            resized_box(200, (from, egui::pos2(300.0, 100.0)), true, frame, canvas),
            230
        );
        // Never narrower than the smallest box.
        assert_eq!(
            resized_box(200, (from, egui::pos2(-400.0, 70.0)), false, frame, canvas),
            disccraft_project::MIN_TEXT_BOX
        );
    }

    #[test]
    fn a_dragged_text_moves_with_the_pointer_and_stays_in_the_frame() {
        let canvas = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(720.0, 480.0));
        let drag = TextDrag {
            menu: MenuId(1),
            index: 0,
            start: (100, 50),
            origin: egui::pos2(110.0, 60.0),
            now: (100, 50),
            resize: None,
            height: false,
        };
        assert_eq!(
            dragged_position(&drag, egui::pos2(130.0, 50.0), (720, 480), canvas),
            (120, 40)
        );
        assert_eq!(
            dragged_position(&drag, egui::pos2(-500.0, -500.0), (720, 480), canvas),
            (0, 0)
        );
        assert_eq!(
            dragged_position(&drag, egui::pos2(5000.0, 5000.0), (720, 480), canvas),
            (719, 479)
        );
    }
}
