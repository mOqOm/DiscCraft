//! Text objects on menus (Encore's text tool) and the tools of the toolbar: the selection tool, the text tool
//! (click the menu to add a text) and the zoom tool (click to zoom in, Alt+click to zoom out).

use crate::app::{DiscCraftApp, Selection};
use crate::i18n::tl;
use crate::menu_editor::{to_canvas, to_frame_point};
use disccraft_project::{MAX_TEXT_SIZE, MIN_TEXT_SIZE, Menu, MenuId, MenuText, Rect};
use egui::Ui;
use serde_json::json;

/// The tool used on the menu viewer (the toolbar's tool buttons).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tool {
    /// Select and move buttons and texts.
    #[default]
    Selection,
    /// Click the menu to add a text there.
    Text,
    /// Click to zoom in, Alt+click to zoom out.
    Zoom,
}

impl Tool {
    /// The tools in toolbar order, with their keys.
    pub(crate) const ALL: [(Tool, egui::Key); 3] = [
        (Tool::Selection, egui::Key::V),
        (Tool::Text, egui::Key::T),
        (Tool::Zoom, egui::Key::Z),
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Tool::Selection => tl!("Selection Tool"),
            Tool::Text => tl!("Text Tool"),
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

    /// The text tool: adds "Text" at the clicked point, selects it and puts the cursor in its text field.
    pub(crate) fn add_menu_text(&mut self, menu: MenuId, x: u16, y: u16) {
        let added = self.run_command(
            "menu.addText",
            json!({"menu": menu.0, "text": tl!("Text"), "x": x, "y": y}),
        );
        if let Some(index) = added
            .and_then(|value| value.get("index").and_then(serde_json::Value::as_u64))
            .and_then(|index| usize::try_from(index).ok())
        {
            self.select_menu_text(menu, index);
            self.focus_rename = true;
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
                let mut rect = disccraft_menu::text_rect(menu, text);
                if let Some(drag) = self
                    .text_drag
                    .filter(|drag| drag.menu == menu.id && drag.index == index)
                {
                    rect.x = drag.now.0;
                    rect.y = drag.now.1;
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
    }

    /// The text under `point`, topmost first.
    pub(crate) fn hit_text(
        &self,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
        point: egui::Pos2,
    ) -> Option<usize> {
        self.text_rects(menu)
            .iter()
            .enumerate()
            .rev()
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
            drag.now = dragged_position(&drag, point, frame, canvas);
        }
        if !ui.input(|i| i.pointer.any_released()) {
            self.text_drag = Some(drag);
            return;
        }
        self.text_drag = None;
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
        let menu_color = self
            .session
            .project
            .menu(menu)
            .map_or([255, 255, 255], |menu| menu.text_color);

        let mut changed = None;
        ui.horizontal_top(|ui| {
            crate::properties::prop_label(ui, tl!("Text:"));
            let focus = std::mem::take(&mut self.focus_rename);
            let (response, value) =
                crate::properties::committed_text(ui, "menu_text_value", &text.text, true);
            if focus {
                response.request_focus();
            }
            changed = value.filter(|value| !value.trim().is_empty());
        });
        if let Some(value) = changed {
            self.set_text_field(menu, index, json!({ "text": value }));
        }

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
            self.set_text_field(menu, index, json!({ "size": size }));
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
            self.set_text_field(menu, index, json!({ "color": value }));
        }

        if let Some(font) = self.font_row(ui, "menu_text_font", text.font.as_deref()) {
            // A style the new family does not have goes back to its regular face.
            let keeps_style = text
                .font_style
                .as_ref()
                .is_none_or(|style| disccraft_menu::font_styles(font.as_deref()).contains(style));
            if keeps_style {
                self.set_text_field(menu, index, json!({ "font": font }));
            } else {
                self.set_text_field(menu, index, json!({ "font": font, "fontStyle": null }));
            }
        }
        if let Some(style) = font_style_row(
            ui,
            "menu_text_font_style",
            text.font.as_deref(),
            text.font_style.as_deref(),
        ) {
            self.set_text_field(menu, index, json!({ "fontStyle": style }));
        }

        // Encore の文字パネルのうち DiscCraft が描けるもの: 太字 / 斜体 / 下線 / 打ち消し線 / 大文字、行揃え、
        // トラッキング、行送り。
        let format = text.format;
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
        let mut leading = format.leading;
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
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Leading:"));
            let response = ui.add(
                egui::DragValue::new(&mut leading)
                    .range(disccraft_project::MIN_LEADING..=disccraft_project::MAX_LEADING)
                    .suffix(" px"),
            );
            if (response.drag_stopped() || (response.changed() && !response.dragged()))
                && leading != format.leading
            {
                change = Some(json!({ "leading": leading }));
            }
        });
        let mut kerning = format.kerning;
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("Kerning:"));
            if ui.checkbox(&mut kerning, tl!("Metrics")).changed() {
                change = Some(json!({ "kerning": kerning }));
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
        if let Some(fields) = change {
            self.set_text_field(menu, index, fields);
        }

        crate::properties::text_row(ui, tl!("Position:"), &format!("{}, {}", text.x, text.y));
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
            tool_glyph(ui.painter(), rect, tool, visuals.fg_stroke.color);
        }
        ui.separator();
        // プレビュー（ビルド結果をモニターで開く）。
        let (rect, response) = ui.allocate_exact_size(egui::vec2(26.0, 22.0), egui::Sense::click());
        let response = response.on_hover_text(crate::actions::Action::Preview.label());
        let visuals = ui.style().interact(&response);
        if response.hovered() {
            ui.painter().rect(
                rect,
                2.0,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        let c = rect.center();
        ui.painter()
            .circle_stroke(c, 7.0, egui::Stroke::new(1.2, visuals.fg_stroke.color));
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(c.x - 2.5, c.y - 4.0),
                egui::pos2(c.x + 4.0, c.y),
                egui::pos2(c.x - 2.5, c.y + 4.0),
            ],
            visuals.fg_stroke.color,
            egui::Stroke::NONE,
        ));
        if response.clicked() {
            self.run_action(ui, crate::actions::Action::Preview);
        }
    }
}

/// Our own simple pictures for the tools: an arrow, a "T" and a magnifier.
/// The font style list under the font (Encore's second font list: R, B, ...). Lists the family's styles when it
/// is opened; `Some(None)` chooses the regular face.
fn font_style_row(
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

fn tool_glyph(painter: &egui::Painter, rect: egui::Rect, tool: Tool, color: egui::Color32) {
    let c = rect.center();
    match tool {
        Tool::Selection => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(c.x - 4.0, c.y - 7.0),
                    egui::pos2(c.x + 5.0, c.y + 2.0),
                    egui::pos2(c.x + 0.5, c.y + 2.0),
                    egui::pos2(c.x - 4.0, c.y + 7.0),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        Tool::Text => {
            painter.text(
                c,
                egui::Align2::CENTER_CENTER,
                "T",
                egui::FontId::proportional(16.0),
                color,
            );
        }
        Tool::Zoom => {
            let lens = egui::pos2(c.x - 1.5, c.y - 1.5);
            painter.circle_stroke(lens, 5.0, egui::Stroke::new(1.5, color));
            painter.line_segment(
                [
                    egui::pos2(c.x + 2.0, c.y + 2.0),
                    egui::pos2(c.x + 6.5, c.y + 6.5),
                ],
                egui::Stroke::new(2.5, color),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TextDrag, dragged_position};
    use disccraft_project::MenuId;

    #[test]
    fn a_dragged_text_moves_with_the_pointer_and_stays_in_the_frame() {
        let canvas = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(720.0, 480.0));
        let drag = TextDrag {
            menu: MenuId(1),
            index: 0,
            start: (100, 50),
            origin: egui::pos2(110.0, 60.0),
            now: (100, 50),
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
