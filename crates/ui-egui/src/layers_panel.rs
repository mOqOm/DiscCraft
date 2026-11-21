//! The Layers panel of the open menu, as Encore draws it: the menu's name, then one row per layer from the top
//! down — each button as a button set `(+) name` that opens to its own layers (its text, video thumbnail and
//! subpicture), the texts, the images and shapes, and the locked Background — with an eye (shown / hidden), a lock
//! and the button mark in columns at the left. A menu with pages shows them as Encore's `(#)` layer groups (GitHub
//! issue #13): the later pages' groups above the first page's layers, the last page on top, each opening to its
//! own layers; a click on one shows that page.

use crate::app::{DiscCraftApp, Selection};
use crate::i18n::{tl, tlf};
use crate::icons::Icon;
use crate::menu_tools::ButtonPart;
use crate::workspace::Panel;
use disccraft_project::{GraphicKind, Menu, MenuId, ShapeKind};
use egui::{Color32, Rect, Sense, Ui, pos2, vec2};
use serde_json::json;

/// A row's height.
const ROW: f32 = 20.0;
/// The width of the eye, lock and button-mark columns.
const COLUMN: f32 = 20.0;
/// How far a button set's own layers are indented.
const INDENT: f32 = 16.0;

/// A row of the Layers panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LayerRow {
    /// A button set.
    Button(usize),
    /// The text layer of a button set.
    ButtonText(usize),
    /// The video thumbnail `(%)` of a button set.
    ButtonThumbnail(usize),
    /// The subpicture `(=1)` of a button set.
    ButtonSubpicture(usize),
    /// A text object.
    Text(usize),
    /// An image or a shape.
    Graphic(usize),
    /// The menu's background.
    Background,
}

/// What a click in the Layers panel asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LayerClick {
    Select(LayerRow, bool),
    ToggleHidden(LayerRow),
    ToggleLocked(LayerRow),
    ToggleOpen(usize),
    /// A page's `(#)` group: show the page.
    ShowPage,
    /// A page's `(#)` group's opener.
    TogglePage,
}

/// A row of a menu's Layers panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PanelRow {
    /// A later page of the menu as Encore's `(#)` layer group (its number, counted from 0 as Encore does).
    Page { page: MenuId, number: usize },
    /// A layer of `menu`: the first page's at the top level, a later page's inside its group (`nested`).
    Layer {
        menu: MenuId,
        row: LayerRow,
        nested: bool,
    },
}

/// The rows of the menu `root` (its first page) and its pages: each later page's `(#)` group, the last on top, with
/// its own layers (no background) when `open_page` says it is open, then the first page's layers. `open_button`
/// says which button sets are open.
pub(crate) fn panel_rows(
    project: &disccraft_project::Project,
    root: MenuId,
    open_page: impl Fn(MenuId) -> bool,
    open_button: impl Fn(MenuId, usize) -> bool,
) -> Vec<PanelRow> {
    let mut rows = Vec::new();
    for (number, page) in project
        .menu_pages(root)
        .into_iter()
        .enumerate()
        .skip(1)
        .rev()
    {
        let Some(menu) = project.menu(page) else {
            continue;
        };
        rows.push(PanelRow::Page { page, number });
        if !open_page(page) {
            continue;
        }
        rows.extend(
            layer_rows(menu, |index| open_button(page, index))
                .into_iter()
                .filter(|row| *row != LayerRow::Background)
                .map(|row| PanelRow::Layer {
                    menu: page,
                    row,
                    nested: true,
                }),
        );
    }
    if let Some(first) = project.menu(root) {
        rows.extend(
            layer_rows(first, |index| open_button(root, index))
                .into_iter()
                .map(|row| PanelRow::Layer {
                    menu: root,
                    row,
                    nested: false,
                }),
        );
    }
    rows
}

/// The rows of `menu` from the top layer down: the buttons (the last on top) each followed by its own layers when it
/// is open, the texts, the images and shapes, and the background.
pub(crate) fn layer_rows(menu: &Menu, open: impl Fn(usize) -> bool) -> Vec<LayerRow> {
    let mut rows = Vec::new();
    for (index, button) in menu.buttons.iter().enumerate().rev() {
        rows.push(LayerRow::Button(index));
        if !open(index) {
            continue;
        }
        if !button.label.is_empty() {
            rows.push(LayerRow::ButtonText(index));
        }
        if button.thumbnail.is_some() {
            rows.push(LayerRow::ButtonThumbnail(index));
        }
        if button.subpicture.is_some() {
            rows.push(LayerRow::ButtonSubpicture(index));
        }
    }
    rows.extend((0..menu.texts.len()).rev().map(LayerRow::Text));
    rows.extend((0..menu.graphics.len()).rev().map(LayerRow::Graphic));
    rows.push(LayerRow::Background);
    rows
}

impl DiscCraftApp {
    /// The Layers panel of the menu in the menu viewer.
    pub(crate) fn layers_panel(&mut self, ui: &mut Ui) {
        let Some(id) = self.viewer_menu() else {
            ui.label(tl!("Open a menu to see its layers."));
            return;
        };
        let root = self.session.project.menu_root(id);
        let Some(first) = self.session.project.menu(root) else {
            return;
        };
        ui.label(first.name.clone());
        let collapsed = self.collapsed_layers.clone();
        let toggled = self.toggled_pages.clone();
        // The page in the viewer has its group open; a click on the opener flips that.
        let rows = panel_rows(
            &self.session.project,
            root,
            |page| (page == id) != toggled.contains(&page),
            |menu, index| !collapsed.contains(&(menu, index)),
        );
        let mut clicked = None;
        for row in rows {
            let click = match row {
                PanelRow::Page { page, number } => self
                    .page_row(
                        ui,
                        (page, number),
                        (page == id) != toggled.contains(&page),
                        page == id,
                    )
                    .map(|click| (page, click)),
                PanelRow::Layer { menu, row, nested } => {
                    let Some(found) = self.session.project.menu(menu).cloned() else {
                        continue;
                    };
                    let nest = if nested { INDENT } else { 0.0 };
                    self.layer_row(ui, &found, row, nest)
                        .map(|click| (menu, click))
                }
            };
            if click.is_some() {
                clicked = click;
            }
        }
        let Some((menu, click)) = clicked else {
            return;
        };
        let Some(found) = self.session.project.menu(menu).cloned() else {
            return;
        };
        match click {
            LayerClick::Select(row, add) => self.select_layer(menu, row, add),
            LayerClick::ToggleHidden(row) => self.toggle_layer(&found, row, true),
            LayerClick::ToggleLocked(row) => self.toggle_layer(&found, row, false),
            // Closed sets are kept; a closed one opens again.
            LayerClick::ToggleOpen(index) => {
                if !self.collapsed_layers.remove(&(menu, index)) {
                    let _ = self.collapsed_layers.insert((menu, index));
                }
            }
            LayerClick::ShowPage => {
                self.select(Selection::Menu(menu));
                self.show_panel(Panel::MenuViewer);
            }
            LayerClick::TogglePage => {
                if !self.toggled_pages.remove(&menu) {
                    let _ = self.toggled_pages.insert(menu);
                }
            }
        }
    }

    /// Draws a later page's `(#)` group row (`number` counted from 0, as Encore numbers pages): its opener, a folder
    /// and its name, marked when the viewer shows it; reports a click on it.
    fn page_row(
        &self,
        ui: &mut Ui,
        (_page, number): (MenuId, usize),
        open: bool,
        shown: bool,
    ) -> Option<LayerClick> {
        let width = ui.available_width().max(COLUMN * 6.0);
        let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
        let visuals = ui.visuals().clone();
        let painter = ui.painter();
        if shown {
            painter.rect_filled(rect, 0.0, visuals.selection.bg_fill.gamma_multiply(0.35));
        } else if response.hovered() {
            painter.rect_filled(rect, 0.0, visuals.widgets.hovered.weak_bg_fill);
        }
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            visuals.widgets.noninteractive.bg_stroke,
        );
        let color = visuals.text_color();
        for column in 0..3 {
            let cell = Rect::from_min_size(
                pos2(rect.min.x + column as f32 * COLUMN, rect.min.y),
                vec2(COLUMN, ROW),
            );
            painter.rect_stroke(
                cell.shrink(2.0),
                2.0,
                visuals.widgets.noninteractive.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        let opener = Rect::from_min_size(
            pos2(rect.min.x + COLUMN * 3.0 + 2.0, rect.min.y),
            vec2(14.0, ROW),
        );
        painter.text(
            opener.center(),
            egui::Align2::CENTER_CENTER,
            if open { "▼" } else { "▶" },
            egui::FontId::proportional(9.0),
            color,
        );
        let icon_rect = Rect::from_min_size(pos2(opener.max.x + 2.0, rect.min.y), vec2(18.0, ROW));
        crate::icons::paint(
            painter,
            Rect::from_center_size(icon_rect.center(), vec2(13.0, 13.0)),
            Icon::Folder,
            color,
        );
        painter.text(
            pos2(icon_rect.max.x + 4.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            format!("(#) {}", tlf!("Page {n}", n = number)),
            egui::FontId::proportional(12.0),
            color,
        );
        if !response.clicked() {
            return None;
        }
        let at = response.interact_pointer_pos()?;
        if opener.expand2(vec2(2.0, 0.0)).contains(at) {
            return Some(LayerClick::TogglePage);
        }
        Some(LayerClick::ShowPage)
    }

    /// Draws one row (indented by `nest` inside a page's group) and reports a click on it.
    fn layer_row(&self, ui: &mut Ui, menu: &Menu, row: LayerRow, nest: f32) -> Option<LayerClick> {
        let width = ui.available_width().max(COLUMN * 6.0);
        let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
        let visuals = ui.visuals().clone();
        let painter = ui.painter();
        let selected = self.layer_selected(menu, row);
        if selected {
            painter.rect_filled(rect, 0.0, visuals.selection.bg_fill.gamma_multiply(0.6));
        } else if response.hovered() {
            painter.rect_filled(rect, 0.0, visuals.widgets.hovered.weak_bg_fill);
        }
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            visuals.widgets.noninteractive.bg_stroke,
        );
        let color = visuals.text_color();
        let faint = color.gamma_multiply(0.45);
        let cell = |column: f32| {
            Rect::from_min_size(
                pos2(rect.min.x + column * COLUMN, rect.min.y),
                vec2(COLUMN, ROW),
            )
        };
        let glyph = |at: Rect, icon: Icon, tint: Color32| {
            crate::icons::paint(
                painter,
                Rect::from_center_size(at.center(), vec2(13.0, 13.0)),
                icon,
                tint,
            );
        };
        let (hidden, locked, own) = layer_state(menu, row);
        // The eye (shown / hidden), the lock, and the button mark.
        let (eye, lock, mark) = (cell(0.0), cell(1.0), cell(2.0));
        for divider in [eye, lock, mark] {
            painter.rect_stroke(
                divider.shrink(2.0),
                2.0,
                visuals.widgets.noninteractive.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        if !hidden {
            glyph(eye, Icon::Eye, if own { color } else { faint });
        }
        if locked {
            glyph(lock, Icon::Lock, if own { color } else { faint });
        }
        if matches!(row, LayerRow::Button(_)) {
            let mark = Rect::from_center_size(mark.center(), vec2(11.0, 8.0));
            painter.rect_filled(mark, 2.5, faint);
        }
        // The name, after the opener of a button set and the layer's icon.
        let indent = nest
            + match row {
                LayerRow::ButtonText(_)
                | LayerRow::ButtonThumbnail(_)
                | LayerRow::ButtonSubpicture(_) => INDENT,
                _ => 0.0,
            };
        let opener = Rect::from_min_size(
            pos2(rect.min.x + COLUMN * 3.0 + 2.0 + indent, rect.min.y),
            vec2(14.0, ROW),
        );
        if let LayerRow::Button(index) = row {
            let open = !self.collapsed_layers.contains(&(menu.id, index));
            painter.text(
                opener.center(),
                egui::Align2::CENTER_CENTER,
                if open { "▼" } else { "▶" },
                egui::FontId::proportional(9.0),
                color,
            );
        }
        let icon_rect = Rect::from_min_size(pos2(opener.max.x + 2.0, rect.min.y), vec2(18.0, ROW));
        glyph(icon_rect, layer_icon(menu, row), color);
        painter.text(
            pos2(icon_rect.max.x + 4.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            self.layer_name(menu, row),
            egui::FontId::proportional(12.0),
            color,
        );

        if !response.clicked() {
            return None;
        }
        let at = response.interact_pointer_pos()?;
        if eye.contains(at) {
            return own.then_some(LayerClick::ToggleHidden(row));
        }
        if lock.contains(at) {
            return own.then_some(LayerClick::ToggleLocked(row));
        }
        if let LayerRow::Button(index) = row
            && opener.expand2(vec2(2.0, 0.0)).contains(at)
        {
            return Some(LayerClick::ToggleOpen(index));
        }
        let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        Some(LayerClick::Select(row, add))
    }

    /// The name a row shows: `(+) name` for a button set, as Encore names them.
    fn layer_name(&self, menu: &Menu, row: LayerRow) -> String {
        let button_name = |index: usize| {
            menu.buttons.get(index).map_or_else(String::new, |button| {
                if button.label.is_empty() {
                    button.name.clone()
                } else {
                    button.label.clone()
                }
            })
        };
        match row {
            LayerRow::Button(index) => format!("(+) {}", button_name(index)),
            LayerRow::ButtonText(index) => menu
                .buttons
                .get(index)
                .map_or_else(String::new, |button| button.label.clone()),
            LayerRow::ButtonThumbnail(_) => format!("(%) {}", tl!("Video Thumbnail")),
            LayerRow::ButtonSubpicture(_) => format!("(=1) {}", tl!("Subpicture")),
            LayerRow::Text(index) => menu
                .texts
                .get(index)
                .map(|text| text.text.lines().next().unwrap_or_default().to_owned())
                .unwrap_or_default(),
            LayerRow::Graphic(index) => menu
                .graphics
                .get(index)
                .map(|graphic| self.graphic_layer_name(graphic))
                .unwrap_or_default(),
            // The layer's name, as Encore (and the PSD) call it, not a word of the interface.
            LayerRow::Background => "Background".to_owned(),
        }
    }

    /// Whether the row is what is selected (a button set, or one of its own layers with the direct selection).
    fn layer_selected(&self, menu: &Menu, row: LayerRow) -> bool {
        let this_menu = self.selection == Selection::Menu(menu.id);
        let button = |index: usize, part: Option<ButtonPart>| {
            this_menu && self.button_selected(index) && self.selected_part == part
        };
        match row {
            LayerRow::Button(index) => button(index, None),
            LayerRow::ButtonText(index) => button(index, Some(ButtonPart::Label)),
            LayerRow::ButtonThumbnail(index) => button(index, Some(ButtonPart::Thumbnail)),
            LayerRow::ButtonSubpicture(_) => false,
            LayerRow::Text(index) => self.selected_menu_text(menu.id) == Some(index),
            LayerRow::Graphic(index) => self.selected_menu_graphic(menu.id) == Some(index),
            // The locked background is never shown as picked (Encore).
            LayerRow::Background => false,
        }
    }

    /// Selects a row's layer, as a click on it in the menu viewer would (a button set's own layer as with the
    /// direct selection tool; the background gives the menu's sheet).
    fn select_layer(&mut self, menu: MenuId, row: LayerRow, add: bool) {
        match row {
            LayerRow::Button(index) if add => self.toggle_button(menu, index),
            LayerRow::Button(index) | LayerRow::ButtonSubpicture(index) => {
                self.select_only_button(menu, index);
            }
            LayerRow::ButtonText(index) => {
                self.select_only_button(menu, index);
                self.selected_part = Some(ButtonPart::Label);
            }
            LayerRow::ButtonThumbnail(index) => {
                self.select_only_button(menu, index);
                self.selected_part = Some(ButtonPart::Thumbnail);
            }
            LayerRow::Text(index) => self.select_menu_text(menu, index),
            LayerRow::Graphic(index) => self.select_menu_graphic(menu, index),
            LayerRow::Background => self.select(Selection::Menu(menu)),
        }
        self.show_panel(Panel::MenuViewer);
    }

    /// Flips a layer's eye (`hidden`) or lock.
    fn toggle_layer(&mut self, menu: &Menu, row: LayerRow, hidden: bool) {
        let (is_hidden, is_locked, _) = layer_state(menu, row);
        let (kind, index) = match row {
            LayerRow::Button(index) => ("button", index),
            LayerRow::Text(index) => ("text", index),
            LayerRow::Graphic(index) => ("graphic", index),
            _ => return,
        };
        let params = if hidden {
            json!({"menu": menu.id.0, "kind": kind, "index": index, "hidden": !is_hidden})
        } else {
            json!({"menu": menu.id.0, "kind": kind, "index": index, "locked": !is_locked})
        };
        if self.run_command("menu.setLayer", params).is_some() {
            self.status = if hidden {
                tlf!(
                    "Layer {state}",
                    state = if is_hidden {
                        tl!("shown")
                    } else {
                        tl!("hidden")
                    }
                )
            } else {
                tlf!(
                    "Layer {state}",
                    state = if is_locked {
                        tl!("unlocked")
                    } else {
                        tl!("locked")
                    }
                )
            };
        }
    }
}

/// A row's eye and lock (`hidden`, `locked`) and whether they are its own (a button set's own layers show the set's;
/// the background is always shown and locked).
fn layer_state(menu: &Menu, row: LayerRow) -> (bool, bool, bool) {
    let button = |index: usize| {
        menu.buttons
            .get(index)
            .map_or((false, false), |button| (button.hidden, button.locked))
    };
    match row {
        LayerRow::Button(index) => {
            let (hidden, locked) = button(index);
            (hidden, locked, true)
        }
        LayerRow::ButtonText(index)
        | LayerRow::ButtonThumbnail(index)
        | LayerRow::ButtonSubpicture(index) => {
            let (hidden, locked) = button(index);
            (hidden, locked, false)
        }
        LayerRow::Text(index) => menu.texts.get(index).map_or((false, false, true), |text| {
            (text.hidden, text.locked, true)
        }),
        LayerRow::Graphic(index) => menu
            .graphics
            .get(index)
            .map_or((false, false, true), |graphic| {
                (graphic.hidden, graphic.locked, true)
            }),
        LayerRow::Background => (false, true, false),
    }
}

/// The Craft icon of a row's layer.
fn layer_icon(menu: &Menu, row: LayerRow) -> Icon {
    match row {
        LayerRow::Button(_) => Icon::Folder,
        LayerRow::ButtonText(_) | LayerRow::Text(_) => Icon::Type,
        LayerRow::ButtonThumbnail(_) => Icon::Film,
        LayerRow::ButtonSubpicture(_) => Icon::Square,
        LayerRow::Graphic(index) => match menu.graphics.get(index).map(|graphic| graphic.kind) {
            Some(GraphicKind::Shape {
                shape: ShapeKind::Ellipse,
                ..
            }) => Icon::Ellipse,
            Some(GraphicKind::Shape { .. }) => Icon::Rectangle,
            _ => Icon::Image,
        },
        LayerRow::Background => Icon::Pen,
    }
}

#[cfg(test)]
mod tests {
    use super::{LayerRow, PanelRow, layer_rows, panel_rows};
    use disccraft_project::{Link, Project};

    /// A menu with pages lists them as Encore's `(#)` groups above the first page's layers, the last page on top
    /// (numbered from 0 as Encore does); an open group shows its page's own layers inside it, without a background.
    #[test]
    fn a_menus_pages_are_its_page_groups() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        project.menu_mut(menu).ok_or("menu")?.buttons.clear();
        let _ = project.add_button(menu, "Play".to_owned(), None, Link::NotSet)?;
        let one = project.add_menu_page(menu)?;
        let two = project.add_menu_page(menu)?;
        let _ = project.add_button(one, "Back".to_owned(), None, Link::NotSet)?;
        let closed = panel_rows(&project, menu, |_| false, |_, _| false);
        assert_eq!(
            closed,
            vec![
                PanelRow::Page {
                    page: two,
                    number: 2
                },
                PanelRow::Page {
                    page: one,
                    number: 1
                },
                PanelRow::Layer {
                    menu,
                    row: LayerRow::Button(0),
                    nested: false
                },
                PanelRow::Layer {
                    menu,
                    row: LayerRow::Background,
                    nested: false
                },
            ]
        );
        let open = panel_rows(&project, menu, |page| page == one, |_, _| false);
        assert_eq!(
            open.get(2),
            Some(&PanelRow::Layer {
                menu: one,
                row: LayerRow::Button(0),
                nested: true
            })
        );
        assert_eq!(open.len(), closed.len() + 1);
        // A menu without pages lists its own layers only.
        let single = project.add_menu(None)?;
        assert!(
            panel_rows(&project, single, |_| true, |_, _| true)
                .iter()
                .all(|row| matches!(row, PanelRow::Layer { nested: false, .. }))
        );
        Ok(())
    }

    /// The rows go from the top layer down: the last button first, each button set with its text when it is open,
    /// then the texts and the background.
    #[test]
    fn layers_list_from_the_top_down() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        let _ = project.add_button(menu, "One".to_owned(), None, Link::NotSet)?;
        let _ = project.add_button(menu, "Two".to_owned(), None, Link::NotSet)?;
        let mut menu = project.menu(menu).ok_or("menu")?.clone();
        menu.texts.push(disccraft_project::MenuText {
            effects: Default::default(),
            box_width: None,
            box_height: None,
            text: "Title".to_owned(),
            x: 0,
            y: 0,
            size: 30,
            color: None,
            font: None,
            font_style: None,
            format: Default::default(),
            shadow: None,
            orientation: Default::default(),
            vertical: false,
            hidden: false,
            locked: false,
            spans: Vec::new(),
        });
        assert_eq!(
            layer_rows(&menu, |index| index == 1),
            vec![
                LayerRow::Button(1),
                LayerRow::ButtonText(1),
                LayerRow::Button(0),
                LayerRow::Text(0),
                LayerRow::Background
            ]
        );
        Ok(())
    }
}
