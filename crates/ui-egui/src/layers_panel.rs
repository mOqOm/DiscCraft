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
use disccraft_menu::PsdTreeNode;
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
        // M10.47: a menu made from a PSD lists the PSD's own layers, as Encore does.
        if let Some((key, tree)) = self.psd_tree_of(root) {
            self.psd_layers_panel(ui, (id, root), &key, &tree);
            return;
        }
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

/// A row of a PSD menu's layer tree (M10.47): where its layer is (the indexes from the top, level by level) and how
/// deep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PsdRow {
    pub(crate) path: Vec<usize>,
    pub(crate) depth: usize,
}

/// The rows of a PSD's layer tree, the top layer first; a group's layers follow it when `open` says it is open.
pub(crate) fn psd_rows(
    tree: &[PsdTreeNode],
    open: &dyn Fn(&[usize], &PsdTreeNode) -> bool,
) -> Vec<PsdRow> {
    fn walk(
        nodes: &[PsdTreeNode],
        (prefix, depth): (&mut Vec<usize>, usize),
        open: &dyn Fn(&[usize], &PsdTreeNode) -> bool,
        rows: &mut Vec<PsdRow>,
    ) {
        for (index, node) in nodes.iter().enumerate() {
            prefix.push(index);
            rows.push(PsdRow {
                path: prefix.clone(),
                depth,
            });
            if node.group && open(prefix, node) {
                walk(
                    &node.children,
                    (prefix, depth.saturating_add(1)),
                    open,
                    rows,
                );
            }
            let _ = prefix.pop();
        }
    }
    let mut rows = Vec::new();
    walk(tree, (&mut Vec::new(), 0), open, &mut rows);
    rows
}

/// The layer at `path` of a PSD's layer tree.
pub(crate) fn psd_node<'a>(tree: &'a [PsdTreeNode], path: &[usize]) -> Option<&'a PsdTreeNode> {
    let (first, rest) = path.split_first()?;
    let node = tree.get(*first)?;
    if rest.is_empty() {
        Some(node)
    } else {
        psd_node(&node.children, rest)
    }
}

/// The button set the layer at `path` belongs to (its page and number): its own, or the nearest set it is inside.
pub(crate) fn psd_owning_button(tree: &[PsdTreeNode], path: &[usize]) -> Option<(usize, usize)> {
    (1..=path.len())
        .rev()
        .find_map(|length| psd_node(tree, path.get(..length)?)?.button)
}

/// What a click in a PSD menu's layer tree asks for (M10.47).
#[derive(Clone, Debug, PartialEq, Eq)]
enum PsdClick {
    /// A group's opener.
    Toggle(Vec<usize>),
    /// A page's `(#)` group: show the page.
    ShowPage(usize),
    /// A button set, or a layer inside one: select the button (adding to the selection).
    Button((usize, usize), bool),
    /// A button set's eye.
    ButtonEye((usize, usize)),
    /// Another layer: the page's menu.
    Menu,
}

impl DiscCraftApp {
    /// The layer tree of the PSD menu `root` is made from (its background, or the layers over its motion video), read
    /// again when the file changes; `None` for a menu not made from a PSD.
    fn psd_tree_of(&mut self, root: MenuId) -> Option<(String, std::sync::Arc<Vec<PsdTreeNode>>)> {
        let menu = self.session.project.menu(root)?;
        let asset = menu.background.document()?;
        let path = self.session.project.assets.get(&asset)?.path.clone();
        if !path.to_ascii_lowercase().ends_with(".psd") {
            return None;
        }
        let modified = std::fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok();
        if let Some((stamp, tree)) = self.psd_trees.get(&path)
            && *stamp == modified
        {
            return Some((path, std::sync::Arc::clone(tree)));
        }
        let tree =
            std::sync::Arc::new(disccraft_menu::psd_layer_tree(std::path::Path::new(&path)).ok()?);
        let _ = self
            .psd_trees
            .insert(path.clone(), (modified, std::sync::Arc::clone(&tree)));
        Some((path, tree))
    }

    /// The Layers panel of a menu made from a PSD (M10.47): what DiscCraft added to the page in the viewer (its texts,
    /// images and the buttons the PSD has no set for), then the PSD's layer tree as Encore lists it: each layer's
    /// name and eye, the groups opening to their layers (the shown page's `(#)` group open at first), the button sets
    /// marked. A click on a `(#)` group shows its page; on a set (or a layer in one), selects its button.
    fn psd_layers_panel(
        &mut self,
        ui: &mut Ui,
        (viewed, root): (MenuId, MenuId),
        key: &str,
        tree: &[PsdTreeNode],
    ) {
        let project = &self.session.project;
        // Each page's menu by its number in the PSD (page 0: the top level's buttons).
        let pages: std::collections::HashMap<usize, MenuId> = project
            .menu_pages(root)
            .into_iter()
            .filter_map(|id| {
                let page = project.menu(id)?.psd_page;
                match page {
                    None => Some((0, id)),
                    Some(disccraft_project::NO_PSD_PAGE) => None,
                    Some(page) => Some((usize::from(page), id)),
                }
            })
            .collect();
        let viewed_page = project
            .menu(viewed)
            .and_then(|menu| menu.psd_page)
            .map_or(0, usize::from);
        let Some(menu) = project.menu(viewed).cloned() else {
            return;
        };
        // DiscCraft's own layers of the page, above the PSD's.
        let psd_buttons = count_page_buttons(tree, viewed_page);
        let mut own: Vec<LayerRow> = (psd_buttons..menu.buttons.len())
            .rev()
            .map(LayerRow::Button)
            .collect();
        own.extend((0..menu.texts.len()).rev().map(LayerRow::Text));
        own.extend((0..menu.graphics.len()).rev().map(LayerRow::Graphic));
        let mut own_click = None;
        for row in own {
            if let Some(click) = self.layer_row(ui, &menu, row, 0.0) {
                own_click = Some(click);
            }
        }
        // A group the user opened or closed stays so, whichever page is shown (showing a page by a click on a button
        // of its group must not close the group); the others: the shown page's `(#)` group open.
        let chosen = self.psd_groups_open.clone();
        let open = |path: &[usize], node: &PsdTreeNode| {
            chosen
                .get(&(key.to_owned(), path.to_vec()))
                .copied()
                .unwrap_or(node.page == Some(viewed_page))
        };
        let mut clicked = None;
        for row in psd_rows(tree, &open) {
            let Some(node) = psd_node(tree, &row.path) else {
                continue;
            };
            let button = node.button;
            let state = button.and_then(|(page, index)| {
                let menu = self.session.project.menu(*pages.get(&page)?)?;
                let hidden = menu.buttons.get(index)?.hidden;
                let selected = self.selection == Selection::Menu(menu.id)
                    && self.button_selected(index)
                    && self.selected_part.is_none();
                Some((hidden, selected))
            });
            let shown = node.page == Some(viewed_page);
            if let Some(click) =
                self.psd_row(ui, (node, &row), open(&row.path, node), (state, shown))
            {
                clicked = Some(match click {
                    PsdClick::Menu => psd_owning_button(tree, &row.path)
                        .map_or(PsdClick::Menu, |button| PsdClick::Button(button, false)),
                    other => other,
                });
            }
        }
        if let Some(click) = own_click {
            match click {
                LayerClick::Select(row, add) => self.select_layer(viewed, row, add),
                LayerClick::ToggleHidden(row) => self.toggle_layer(&menu, row, true),
                LayerClick::ToggleLocked(row) => self.toggle_layer(&menu, row, false),
                LayerClick::ToggleOpen(_) | LayerClick::ShowPage | LayerClick::TogglePage => {}
            }
            return;
        }
        let Some(click) = clicked else {
            return;
        };
        match click {
            PsdClick::Toggle(path) => {
                let was_open = psd_node(tree, &path).is_some_and(|node| open(&path, node));
                let _ = self
                    .psd_groups_open
                    .insert((key.to_owned(), path), !was_open);
            }
            PsdClick::ShowPage(page) => {
                if let Some(id) = pages.get(&page) {
                    self.select(Selection::Menu(*id));
                    self.show_panel(Panel::MenuViewer);
                }
            }
            PsdClick::Button((page, index), add) => {
                if let Some(id) = pages.get(&page) {
                    self.select_layer(*id, LayerRow::Button(index), add);
                }
            }
            PsdClick::ButtonEye((page, index)) => {
                if let Some(found) = pages
                    .get(&page)
                    .and_then(|id| self.session.project.menu(*id))
                    .cloned()
                {
                    self.toggle_layer(&found, LayerRow::Button(index), true);
                }
            }
            PsdClick::Menu => {
                self.select(Selection::Menu(viewed));
                self.show_panel(Panel::MenuViewer);
            }
        }
    }

    /// Draws one row of a PSD's layer tree: the eye (the layer's own; a button set's also hidden when its button is),
    /// the button mark, the opener of a group, an icon and the name, indented by its depth; reports a click on it.
    /// `state`: a button set's button (hidden, selected); `shown`: a `(#)` group whose page is in the viewer.
    fn psd_row(
        &self,
        ui: &mut Ui,
        (node, row): (&PsdTreeNode, &PsdRow),
        open: bool,
        (state, shown): (Option<(bool, bool)>, bool),
    ) -> Option<PsdClick> {
        let width = ui.available_width().max(COLUMN * 6.0);
        let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
        let visuals = ui.visuals().clone();
        let painter = ui.painter();
        let selected = state.is_some_and(|(_, selected)| selected);
        if selected {
            painter.rect_filled(rect, 0.0, visuals.selection.bg_fill.gamma_multiply(0.6));
        } else if shown {
            painter.rect_filled(rect, 0.0, visuals.selection.bg_fill.gamma_multiply(0.35));
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
        let (eye, lock, mark) = (cell(0.0), cell(1.0), cell(2.0));
        for divider in [eye, lock, mark] {
            painter.rect_stroke(
                divider.shrink(2.0),
                2.0,
                visuals.widgets.noninteractive.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        let glyph = |at: Rect, icon: Icon, tint: Color32| {
            crate::icons::paint(
                painter,
                Rect::from_center_size(at.center(), vec2(13.0, 13.0)),
                icon,
                tint,
            );
        };
        let hidden = !node.visible || state.is_some_and(|(hidden, _)| hidden);
        if !hidden {
            // A button set's eye is the button's (it can be flipped); another layer's is the PSD's.
            glyph(eye, Icon::Eye, if state.is_some() { color } else { faint });
        }
        if node.button.is_some() {
            let mark = Rect::from_center_size(mark.center(), vec2(11.0, 8.0));
            painter.rect_filled(mark, 2.5, faint);
        }
        let indent = INDENT * row.depth as f32;
        let opener = Rect::from_min_size(
            pos2(rect.min.x + COLUMN * 3.0 + 2.0 + indent, rect.min.y),
            vec2(14.0, ROW),
        );
        if node.group {
            painter.text(
                opener.center(),
                egui::Align2::CENTER_CENTER,
                if open { "▼" } else { "▶" },
                egui::FontId::proportional(9.0),
                color,
            );
        }
        let icon = if node.group {
            Icon::Folder
        } else if node.name.trim_start().starts_with("(%)") {
            Icon::Film
        } else if node.name.trim_start().starts_with("(=") {
            Icon::Square
        } else {
            Icon::Image
        };
        let icon_rect = Rect::from_min_size(pos2(opener.max.x + 2.0, rect.min.y), vec2(18.0, ROW));
        glyph(icon_rect, icon, if node.visible { color } else { faint });
        painter.text(
            pos2(icon_rect.max.x + 4.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            node.name.clone(),
            egui::FontId::proportional(12.0),
            if node.visible { color } else { faint },
        );
        if !response.clicked() {
            return None;
        }
        let at = response.interact_pointer_pos()?;
        if node.group && opener.expand2(vec2(2.0, 0.0)).contains(at) {
            return Some(PsdClick::Toggle(row.path.clone()));
        }
        if let Some(button) = node.button {
            if eye.contains(at) {
                return Some(PsdClick::ButtonEye(button));
            }
            let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
            return Some(PsdClick::Button(button, add));
        }
        if let Some(page) = node.page {
            return Some(PsdClick::ShowPage(page));
        }
        Some(PsdClick::Menu)
    }
}

/// How many button sets page `page` of a PSD's layer tree has.
fn count_page_buttons(tree: &[PsdTreeNode], page: usize) -> usize {
    tree.iter()
        .map(|node| {
            usize::from(node.button.is_some_and(|(found, _)| found == page))
                + count_page_buttons(&node.children, page)
        })
        .sum()
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
    use super::{count_page_buttons, psd_node, psd_owning_button, psd_rows};
    use disccraft_menu::PsdTreeNode;

    fn node(name: &str, children: Vec<PsdTreeNode>) -> PsdTreeNode {
        PsdTreeNode {
            name: name.to_owned(),
            visible: true,
            group: !children.is_empty(),
            page: None,
            button: None,
            children,
        }
    }

    /// M10.47: a PSD menu's tree lists its layers from the top, a group's layers after it only while it is open;
    /// a layer inside a button set belongs to that set's button; each page counts its own sets.
    #[test]
    fn a_psd_tree_lists_its_open_groups_and_finds_each_layers_button() {
        let mut play = node(
            "(+) Play",
            vec![node("(=1)select", vec![]), node("Play", vec![])],
        );
        play.button = Some((0, 0));
        let mut back = node("(+) Back", vec![node("back", vec![])]);
        back.button = Some((1, 0));
        let mut page = node("(#) Page 1", vec![back]);
        page.page = Some(1);
        let tree = vec![node("Logo", vec![]), play, page, node("Background", vec![])];
        let names = |open: &dyn Fn(&[usize], &PsdTreeNode) -> bool| -> Vec<String> {
            psd_rows(&tree, open)
                .iter()
                .filter_map(|row| {
                    psd_node(&tree, &row.path)
                        .map(|node| format!("{}{}", "-".repeat(row.depth), node.name))
                })
                .collect()
        };
        assert_eq!(
            names(&|_, _| false),
            ["Logo", "(+) Play", "(#) Page 1", "Background"]
        );
        assert_eq!(
            names(&|_, node| node.page.is_some() || node.button.is_some()),
            [
                "Logo",
                "(+) Play",
                "-(=1)select",
                "-Play",
                "(#) Page 1",
                "-(+) Back",
                "--back",
                "Background"
            ]
        );
        assert_eq!(psd_owning_button(&tree, &[1, 1]), Some((0, 0)));
        assert_eq!(psd_owning_button(&tree, &[2, 0, 0]), Some((1, 0)));
        assert_eq!(psd_owning_button(&tree, &[0]), None);
        assert_eq!(
            (count_page_buttons(&tree, 0), count_page_buttons(&tree, 1)),
            (1, 1)
        );
    }
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
