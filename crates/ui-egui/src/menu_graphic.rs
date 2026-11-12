//! Graphic layers (images and shapes) on the menu viewer: selecting, moving and resizing them, their property
//! sheet, and placing them from the Library panel.

use crate::app::{DiscCraftApp, Selection};
use crate::i18n::{tl, tlf};
use crate::menu_editor::{DragKind, HANDLE, clamp_rect, to_canvas, to_frame_point};
use disccraft_project::{
    AssetId, AssetKind, GraphicKind, Link, Menu, MenuGraphic, MenuId, Rect, ShapeKind, TimelineId,
};
use egui::Ui;
use serde_json::json;

/// A graphic being moved or resized on the menu viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GraphicDrag {
    pub(crate) menu: MenuId,
    pub(crate) index: usize,
    pub(crate) kind: DragKind,
    /// The rectangle when the drag started (frame pixels).
    pub(crate) start: Rect,
    /// Where the pointer was when the drag started (canvas).
    pub(crate) origin: egui::Pos2,
    /// The rectangle now (frame pixels).
    pub(crate) now: Rect,
}

/// The rectangle a drag has reached: moved by the pointer's travel, or resized from its bottom-right corner.
pub(crate) fn dragged_graphic_rect(
    drag: &GraphicDrag,
    point: egui::Pos2,
    frame: (u16, u16),
    canvas: egui::Rect,
) -> Rect {
    let (x0, y0) = to_frame_point(drag.origin, frame, canvas);
    let (x1, y1) = to_frame_point(point, frame, canvas);
    let dx = x1.saturating_sub(x0);
    let dy = y1.saturating_sub(y0);
    let rect = match drag.kind {
        DragKind::Move => Rect {
            x: u16::try_from(i32::from(drag.start.x).saturating_add(dx).max(0)).unwrap_or(u16::MAX),
            y: u16::try_from(i32::from(drag.start.y).saturating_add(dy).max(0)).unwrap_or(u16::MAX),
            w: drag.start.w,
            h: drag.start.h,
        },
        DragKind::Resize => Rect {
            x: drag.start.x,
            y: drag.start.y,
            w: u16::try_from(i32::from(drag.start.w).saturating_add(dx).max(1)).unwrap_or(u16::MAX),
            h: u16::try_from(i32::from(drag.start.h).saturating_add(dy).max(1)).unwrap_or(u16::MAX),
        },
    };
    clamp_rect(rect, frame)
}

/// Encore's names for the shapes.
pub(crate) fn shape_label(shape: ShapeKind) -> &'static str {
    match shape {
        ShapeKind::Rectangle => tl!("Rectangle"),
        ShapeKind::RoundedRectangle => tl!("Rounded Rectangle"),
        ShapeKind::Ellipse => tl!("Ellipse"),
    }
}

impl DiscCraftApp {
    /// The graphic selected on `menu`, when there is one.
    pub(crate) fn selected_menu_graphic(&self, menu: MenuId) -> Option<usize> {
        if self.selection != Selection::Menu(menu) {
            return None;
        }
        let index = self.selected_graphic?;
        self.session
            .project
            .menu(menu)?
            .graphics
            .get(index)
            .map(|_| index)
    }

    /// Selects graphic `index` of `menu` (buttons and texts are deselected) and shows its sheet.
    pub(crate) fn select_menu_graphic(&mut self, menu: MenuId, index: usize) {
        self.select(Selection::Menu(menu));
        self.selected_graphic = Some(index);
    }

    /// Edit ▸ Clear with a graphic selected: removes it. Returns whether one was removed.
    pub(crate) fn remove_selected_graphic(&mut self) -> bool {
        let Selection::Menu(menu) = self.selection else {
            return false;
        };
        let Some(index) = self.selected_menu_graphic(menu) else {
            return false;
        };
        self.selected_graphic = None;
        let _ = self.run_command(
            "menu.removeGraphic",
            json!({ "menu": menu.0, "index": index }),
        );
        true
    }

    /// The frame rectangle of each graphic (the dragged one where it is being dragged to).
    fn graphic_rects(&self, menu: &Menu) -> Vec<Rect> {
        menu.graphics
            .iter()
            .enumerate()
            .map(|(index, graphic)| {
                self.graphic_drag
                    .filter(|drag| drag.menu == menu.id && drag.index == index)
                    .map_or(graphic.rect, |drag| drag.now)
            })
            .collect()
    }

    /// Outlines the selected graphic with a resize handle at its bottom-right corner.
    pub(crate) fn draw_menu_graphics(
        &self,
        ui: &Ui,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
    ) {
        let Some(selected) = self.selected_menu_graphic(menu.id) else {
            return;
        };
        let Some(rect) = self.graphic_rects(menu).get(selected).copied() else {
            return;
        };
        let outline = to_canvas(rect, frame, canvas);
        let color = egui::Color32::from_rgb(120, 200, 255);
        let painter = ui.painter();
        painter.rect_stroke(
            outline,
            0.0,
            egui::Stroke::new(1.0, color),
            egui::StrokeKind::Outside,
        );
        let handle = egui::Rect::from_min_size(
            outline.max - egui::vec2(HANDLE, HANDLE),
            egui::vec2(HANDLE, HANDLE),
        );
        painter.rect_filled(handle, 0.0, color);
    }

    /// The graphic under `point` (topmost first) and whether its resize handle was hit.
    pub(crate) fn hit_graphic(
        &self,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
        point: egui::Pos2,
    ) -> Option<(usize, DragKind)> {
        let selected = self.selected_menu_graphic(menu.id);
        self.graphic_rects(menu)
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, rect)| {
                let on_canvas = to_canvas(*rect, frame, canvas);
                if !on_canvas.contains(point) {
                    return None;
                }
                let handle = egui::Rect::from_min_size(
                    on_canvas.max - egui::vec2(HANDLE, HANDLE),
                    egui::vec2(HANDLE, HANDLE),
                );
                let kind = if selected == Some(index) && handle.contains(point) {
                    DragKind::Resize
                } else {
                    DragKind::Move
                };
                Some((index, kind))
            })
    }

    /// Presses on a graphic with the selection tool: selects it and starts a move or resize (unless it was a
    /// click).
    pub(crate) fn press_graphic(
        &mut self,
        menu: &Menu,
        index: usize,
        kind: DragKind,
        point: egui::Pos2,
        released: bool,
    ) {
        self.select_menu_graphic(menu.id, index);
        if released {
            return;
        }
        if let Some(graphic) = menu.graphics.get(index) {
            self.graphic_drag = Some(GraphicDrag {
                menu: menu.id,
                index,
                kind,
                start: graphic.rect,
                origin: point,
                now: graphic.rect,
            });
        }
    }

    /// Follows a graphic drag; on release sets the rectangle (one undo step).
    pub(crate) fn continue_graphic_drag(
        &mut self,
        ui: &Ui,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
    ) {
        let Some(mut drag) = self.graphic_drag else {
            return;
        };
        if drag.menu != menu.id {
            self.graphic_drag = None;
            return;
        }
        if let Some(point) = ui.input(|i| i.pointer.interact_pos()) {
            drag.now = dragged_graphic_rect(&drag, point, frame, canvas);
        }
        if !ui.input(|i| i.pointer.any_released()) {
            self.graphic_drag = Some(drag);
            return;
        }
        self.graphic_drag = None;
        if drag.now != drag.start {
            self.set_graphic_field(
                menu.id,
                drag.index,
                json!({ "x": drag.now.x, "y": drag.now.y, "w": drag.now.w, "h": drag.now.h }),
            );
        }
    }

    /// Runs `menu.setGraphic` with `fields` for graphic `index`.
    fn set_graphic_field(&mut self, menu: MenuId, index: usize, fields: serde_json::Value) {
        let mut params = json!({ "menu": menu.0, "index": index });
        if let (Some(map), Some(extra)) = (params.as_object_mut(), fields.as_object()) {
            for (key, value) in extra {
                let _ = map.insert(key.clone(), value.clone());
            }
        }
        let _ = self.run_command("menu.setGraphic", params);
    }

    /// Places a shape from the Library in the middle of the open menu and selects it.
    pub(crate) fn place_shape(&mut self, menu: MenuId, shape: ShapeKind) {
        let placed = self.run_command(
            "menu.addShape",
            json!({ "menu": menu.0, "shape": shape.as_str(), "color": "#FFFFFF" }),
        );
        self.select_placed_graphic(menu, placed);
    }

    /// Places an image asset on the open menu at its own size (shrunk to fit half the frame), centred.
    pub(crate) fn place_image(&mut self, menu: MenuId, asset: AssetId) {
        self.place_image_at(menu, asset, None);
    }

    /// A Project panel row dropped on the menu canvas (Encore: dragging assets and timelines into a menu): an
    /// image becomes a graphic at the drop point; a timeline, slideshow, playlist or chapter playlist becomes a
    /// button there, named after it and linked to it.
    pub(crate) fn accept_canvas_drop(
        &mut self,
        ui: &Ui,
        menu: &Menu,
        frame: (u16, u16),
        response: &egui::Response,
    ) {
        use crate::project_panel::DragRow;
        if response.dnd_hover_payload::<DragRow>().is_some() {
            let _ = ui.painter().rect_stroke(
                response.rect,
                0.0,
                ui.visuals().selection.stroke,
                egui::StrokeKind::Inside,
            );
        }
        let Some(payload) = response.dnd_release_payload::<DragRow>() else {
            return;
        };
        let point = ui
            .input(|i| i.pointer.interact_pos())
            .unwrap_or(response.rect.center());
        let (x, y) = to_frame_point(point, frame, response.rect);
        let x = u16::try_from(x.clamp(0, i32::from(frame.0.saturating_sub(1)))).unwrap_or(0);
        let y = u16::try_from(y.clamp(0, i32::from(frame.1.saturating_sub(1)))).unwrap_or(0);
        let id = payload.0;
        let project = &self.session.project;

        if let Some(asset) = project.assets.get(&AssetId(id)) {
            if asset.kind == AssetKind::Image {
                self.place_image_at(menu.id, AssetId(id), Some((x, y)));
            } else {
                self.status =
                    tl!("Drop an image, a timeline, a slideshow or a playlist on the menu")
                        .to_string();
            }
            return;
        }
        let dropped = if let Some(timeline) = project.timelines.get(&TimelineId(id)) {
            Some((
                timeline.name.clone(),
                Link::Timeline {
                    id: TimelineId(id),
                    chapter: 0,
                },
            ))
        } else if let Some(slideshow) = project.slideshows.get(&id) {
            Some((slideshow.name.clone(), Link::Slideshow { id }))
        } else if let Some(playlist) = project.playlists.get(&id) {
            Some((playlist.name.clone(), Link::Playlist { id }))
        } else {
            project
                .chapter_playlists
                .get(&id)
                .map(|list| (list.name.clone(), Link::ChapterPlaylist { id }))
        };
        let Some((label, link)) = dropped else {
            self.status =
                tl!("Drop an image, a timeline, a slideshow or a playlist on the menu").to_string();
            return;
        };
        // A default-sized button centred on the drop point (kept inside the frame).
        let size = disccraft_project::auto_button_rect(0, &project.settings);
        let rect = clamp_rect(
            Rect {
                x: x.saturating_sub(size.w / 2),
                y: y.saturating_sub(size.h / 2),
                w: size.w,
                h: size.h,
            },
            frame,
        );
        let mut params = json!({ "menu": menu.id.0, "label": label, "rect": { "x": rect.x, "y": rect.y, "w": rect.w, "h": rect.h } });
        if let (Some(map), Ok(target)) = (params.as_object_mut(), serde_json::to_value(&link)) {
            let _ = map.insert("target".to_owned(), target);
        }
        let added = self.run_command("menu.addButton", params);
        if let Some(index) = added
            .as_ref()
            .and_then(|value| value.get("button"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
        {
            self.select_only_button(menu.id, index);
        }
    }

    /// Places an image asset on the menu: at `top_left`, or centred when `None`; at its own size, shrunk to fit
    /// half the frame.
    pub(crate) fn place_image_at(
        &mut self,
        menu: MenuId,
        asset: AssetId,
        top_left: Option<(u16, u16)>,
    ) {
        let (frame_w, frame_h) = disccraft_project::frame_size(&self.session.project.settings);
        let (image_w, image_h) = self
            .asset_probe(asset)
            .dimensions
            .filter(|(w, h)| *w > 0 && *h > 0)
            .unwrap_or((u32::from(frame_w) / 3, u32::from(frame_h) / 3));
        // Fit within half the frame, keeping the image's shape.
        let max_w = f64::from(frame_w) / 2.0;
        let max_h = f64::from(frame_h) / 2.0;
        let scale = (max_w / f64::from(image_w))
            .min(max_h / f64::from(image_h))
            .min(1.0);
        let w = ((f64::from(image_w) * scale).round() as u16).max(1);
        let h = ((f64::from(image_h) * scale).round() as u16).max(1);
        let (x, y) = match top_left {
            Some((x, y)) => (
                x.min(frame_w.saturating_sub(w)),
                y.min(frame_h.saturating_sub(h)),
            ),
            None => (frame_w.saturating_sub(w) / 2, frame_h.saturating_sub(h) / 2),
        };
        let placed = self.run_command(
            "menu.addImage",
            json!({ "menu": menu.0, "asset": asset.0, "x": x, "y": y, "w": w, "h": h }),
        );
        self.select_placed_graphic(menu, placed);
    }

    fn select_placed_graphic(&mut self, menu: MenuId, placed: Option<serde_json::Value>) {
        if let Some(index) = placed
            .as_ref()
            .and_then(|value| value.get("index"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
        {
            self.select_menu_graphic(menu, index);
            self.tab = crate::workspace::CenterTab::Menu;
        }
    }

    /// The graphic's property sheet: what it is, its colour and opacity, its position and size, its place in the
    /// stacking order.
    pub(crate) fn graphic_sheet(&mut self, ui: &mut Ui, menu: MenuId, index: usize) {
        let Some(graphic) = self
            .session
            .project
            .menu(menu)
            .and_then(|m| m.graphics.get(index))
            .cloned()
        else {
            return;
        };
        let count = self
            .session
            .project
            .menu(menu)
            .map_or(0, |m| m.graphics.len());
        let project = self.session.project.clone();
        match graphic.kind {
            GraphicKind::Image { .. } => crate::properties::sheet_title(ui, tl!("Image")),
            GraphicKind::Shape { .. } => crate::properties::sheet_title(ui, tl!("Shape")),
        }

        match graphic.kind {
            GraphicKind::Image { asset } => {
                let name = project
                    .assets
                    .get(&asset)
                    .map_or_else(|| tl!("(missing)").to_string(), |asset| asset.name.clone());
                crate::properties::text_row(ui, tl!("Image:"), &name);
            }
            GraphicKind::Shape { shape, color } => {
                let mut chosen = None;
                ui.horizontal(|ui| {
                    crate::properties::prop_label(ui, tl!("Shape:"));
                    egui::ComboBox::from_id_salt(("graphic_shape", menu.0, index))
                        .selected_text(shape_label(shape))
                        .show_ui(ui, |ui| {
                            for kind in ShapeKind::ALL {
                                if ui
                                    .selectable_label(kind == shape, shape_label(kind))
                                    .clicked()
                                {
                                    chosen = Some(kind);
                                }
                            }
                        });
                });
                if let Some(kind) = chosen
                    && kind != shape
                {
                    self.set_graphic_field(menu, index, json!({ "shape": kind.as_str() }));
                }
                let mut rgb = crate::app::rgb_to_f32(color);
                let mut changed = false;
                ui.horizontal(|ui| {
                    crate::properties::prop_label(ui, tl!("Color:"));
                    changed = ui.color_edit_button_rgb(&mut rgb).changed();
                });
                if changed {
                    let [r, g, b] = crate::app::rgb_from_f32(rgb);
                    self.set_graphic_field(
                        menu,
                        index,
                        json!({ "color": format!("#{r:02X}{g:02X}{b:02X}") }),
                    );
                }
            }
        }

        let mut opacity = graphic.opacity;
        let mut opacity_changed = false;
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Opacity:"));
            opacity_changed = ui
                .add(egui::Slider::new(&mut opacity, 1..=100).suffix("%"))
                .drag_stopped();
        });
        if opacity_changed && opacity != graphic.opacity {
            self.set_graphic_field(menu, index, json!({ "opacity": opacity }));
        }
        ui.separator();

        let (frame_w, frame_h) = disccraft_project::frame_size(&project.settings);
        let mut rect = graphic.rect;
        let mut rect_changed = false;
        for (label, value, max) in [
            (tl!("X:"), &mut rect.x, frame_w.saturating_sub(1)),
            (tl!("Y:"), &mut rect.y, frame_h.saturating_sub(1)),
            (tl!("Width:"), &mut rect.w, frame_w),
            (tl!("Height:"), &mut rect.h, frame_h),
        ] {
            ui.horizontal(|ui| {
                crate::properties::prop_label(ui, label);
                let response = ui.add(egui::DragValue::new(value).range(1.min(max)..=max));
                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    rect_changed = true;
                }
            });
        }
        if rect_changed && rect != graphic.rect {
            let rect = clamp_rect(rect, (frame_w, frame_h));
            self.set_graphic_field(
                menu,
                index,
                json!({ "x": rect.x, "y": rect.y, "w": rect.w, "h": rect.h }),
            );
        }
        ui.separator();

        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Order:"));
            if ui
                .add_enabled(
                    index.saturating_add(1) < count,
                    egui::Button::new(tl!("Bring Forward")),
                )
                .clicked()
            {
                let _ = self.run_command(
                    "menu.moveGraphic",
                    json!({ "menu": menu.0, "from": index, "to": index.saturating_add(1) }),
                );
                self.selected_graphic = Some(index.saturating_add(1));
            }
            if ui
                .add_enabled(index > 0, egui::Button::new(tl!("Send Backward")))
                .clicked()
            {
                let _ = self.run_command(
                    "menu.moveGraphic",
                    json!({ "menu": menu.0, "from": index, "to": index.saturating_sub(1) }),
                );
                self.selected_graphic = Some(index.saturating_sub(1));
            }
        });
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, "");
            if ui.button(tl!("Delete")).clicked() {
                let _ = self.remove_selected_graphic();
            }
        });
    }

    /// The layer name of a graphic for the Layers panel ("Image: logo" / "Shape: Ellipse").
    pub(crate) fn graphic_layer_name(&self, graphic: &MenuGraphic) -> String {
        match graphic.kind {
            GraphicKind::Image { asset } => tlf!(
                "Image: {name}",
                name = self
                    .session
                    .project
                    .assets
                    .get(&asset)
                    .map_or_else(|| tl!("(missing)").to_string(), |asset| asset.name.clone())
            ),
            GraphicKind::Shape { shape, .. } => tlf!("Shape: {name}", name = shape_label(shape)),
        }
    }
}
