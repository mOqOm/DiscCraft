use crate::actions::Action;
use crate::app::{DiscCraftApp, Selection, rgb_to_f32};
use crate::button_edit::moved;
use crate::i18n::{tl, tlf};

use disccraft_project::{
    Link, Menu, MenuBackground, MenuId, Project, Rect, TimelineId, format_time,
};

use egui::{self, Context, Ui};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub(crate) struct MenuTexture {
    pub(crate) hash: u64,
    pub(crate) handle: egui::TextureHandle,
    /// The highlight subpicture as white masks the size of the frame, one per colour (1 to 3), tinted with a
    /// state's colours to preview it: a frame, the text when the button makes a text subpicture, or the
    /// subpicture's layers.
    pub(crate) highlights: [Option<egui::TextureHandle>; 3],
    /// A rich-colour menu's PSD buttons: each one's normal, selected and activated pictures in its subpicture
    /// layers' own colours, over the button's rectangle (M6.335); `None` for a button without colour layers.
    pub(crate) colour_states: Vec<Option<[Option<egui::TextureHandle>; 3]>>,
    pub(crate) width: u16,
    pub(crate) height: u16,
    /// Drawn while a video of the menu (its background) was still being opened, so without it: drawn again once
    /// the files are open (M6.490: a motion menu stayed black in the Menu Viewer, where Encore shows the video's
    /// first frame).
    pub(crate) waiting: bool,
}

/// ハイライトのプレビュー状態（Encore の「通常 / 選択 / 決定」に当たる）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HighlightPreview {
    /// 通常。
    Normal,
    /// 選択（ハイライト）。
    Selected,
    /// 決定。
    Activated,
}

/// The subpicture state shown after `state`'s button is pressed (Encore's 標準 / 選択時 / アクティベート時, M6.489):
/// that state, one at a time; none when it was the one shown.
pub(crate) fn toggled_preview(
    shown: Option<HighlightPreview>,
    state: HighlightPreview,
) -> Option<HighlightPreview> {
    if shown == Some(state) {
        None
    } else {
        Some(state)
    }
}

impl HighlightPreview {
    /// 全状態。
    pub(crate) const ALL: [HighlightPreview; 3] = [
        HighlightPreview::Normal,
        HighlightPreview::Selected,
        HighlightPreview::Activated,
    ];

    /// ツールバーの絵（M6.487）。
    fn glyph(self) -> crate::tool_icons::Glyph {
        match self {
            HighlightPreview::Normal => crate::tool_icons::Glyph::SubpictureNormal,
            HighlightPreview::Selected => crate::tool_icons::Glyph::SubpictureSelected,
            HighlightPreview::Activated => crate::tool_icons::Glyph::SubpictureActivated,
        }
    }

    /// ツールバーのヒント（Encore と同じ文言）。
    pub(crate) fn hint(self) -> &'static str {
        match self {
            HighlightPreview::Normal => tl!("Show normal subpicture"),
            HighlightPreview::Selected => tl!("Show selected subpicture highlight"),
            HighlightPreview::Activated => tl!("Show activated subpicture highlight"),
        }
    }
}

/// ドラッグ中のボタン。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CanvasDrag {
    pub(crate) menu: MenuId,
    /// 押したボタン。
    pub(crate) button: usize,
    pub(crate) kind: DragKind,
    /// ドラッグ開始時の押したボタンの矩形（ディスク座標）。
    pub(crate) start: Rect,
    /// ドラッグ開始時のポインター位置（キャンバス座標）。
    pub(crate) origin: egui::Pos2,
    /// いっしょに動かすボタン（番号と開始時の矩形）。移動は選んでいるボタン全部、リサイズは押したボタンだけ。
    pub(crate) group: Vec<(usize, Rect)>,
}

/// キャンバスの何もないところからのドラッグ（範囲で選ぶ）。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Marquee {
    pub(crate) menu: MenuId,
    /// 押した位置（キャンバス座標）。
    pub(crate) origin: egui::Pos2,
    /// Shift / Ctrl を押していたときの、元の選択（範囲のボタンを加える）。
    pub(crate) base: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DragKind {
    Move,
    Resize,
}

/// ボタンの右下リサイズハンドル（キャンバス上の辺の長さ）。
pub(crate) const HANDLE: f32 = 10.0;
/// チェックが要求するボタンの最小辺。
pub(crate) const MIN_SIDE: u16 = 8;

/// The safe areas over a picture drawn in `rect` (the menu viewer's and the monitor's): action safe (90 %) and
/// title safe (80 %) of the picture, as Encore outlines them.
pub(crate) fn paint_safe_areas(painter: &egui::Painter, rect: egui::Rect) {
    for (fraction, color) in [
        (0.10_f32, egui::Color32::from_white_alpha(90)),
        (0.05_f32, egui::Color32::from_white_alpha(45)),
    ] {
        let inset_x = (rect.width() * fraction * 0.5)
            .min(rect.width() * 0.4)
            .max(0.0);
        let inset_y = (rect.height() * fraction * 0.5)
            .min(rect.height() * 0.4)
            .max(0.0);
        let _ = painter.rect_stroke(
            rect.shrink2(egui::vec2(inset_x, inset_y)),
            0.0,
            egui::Stroke::new(1.0, color),
            egui::StrokeKind::Inside,
        );
    }
}

/// キャンバス上の矩形をディスク座標へ変換する。
///
/// DVD のピクセルは正方形ではないため、x と y で拡大率が異なる。
pub(crate) fn to_canvas(rect: Rect, frame: (u16, u16), canvas: egui::Rect) -> egui::Rect {
    let scale = canvas_scale(frame, canvas);
    let min = canvas.min + egui::vec2(f32::from(rect.x) * scale.x, f32::from(rect.y) * scale.y);
    egui::Rect::from_min_size(
        min,
        egui::vec2(f32::from(rect.w) * scale.x, f32::from(rect.h) * scale.y),
    )
}

pub(crate) fn canvas_scale(frame: (u16, u16), canvas: egui::Rect) -> egui::Vec2 {
    let width = f32::from(frame.0.max(1));
    let height = f32::from(frame.1.max(1));

    egui::vec2(canvas.width() / width, canvas.height() / height)
}

/// キャンバス上の点をディスク座標へ変換し、枠内に収める。
pub(crate) fn to_frame_point(
    point: egui::Pos2,
    frame: (u16, u16),
    canvas: egui::Rect,
) -> (i32, i32) {
    let scale = canvas_scale(frame, canvas);
    let raw_x = (point.x - canvas.min.x) / scale.x.max(f32::MIN);
    let raw_y = (point.y - canvas.min.y) / scale.y.max(f32::MIN);

    (round_clamped(raw_x), round_clamped(raw_y))
}

/// 透明を表す市松模様のますの大きさ（ピクセル。Encore のメニュービューアーと同じ）。
pub(crate) const CHECK_CELL: usize = 16;

/// 透明を表す市松模様（白と薄い灰色の `cell` ピクセルのます）の RGB。
fn transparency_checks(width: usize, height: usize, cell: usize) -> Vec<u8> {
    let cell = cell.max(1);
    (0..height)
        .flat_map(|y| (0..width).map(move |x| (x / cell + y / cell) % 2))
        .flat_map(|odd| [if odd == 1 { 204 } else { 255 }; 3])
        .collect()
}

/// ディスク座標は u16 なので、0 から 65535 の間に収める。
fn round_clamped(value: f32) -> i32 {
    if !value.is_finite() {
        return 0;
    }

    value.round().clamp(0.0, 65_535.0) as i32
}

/// ディスク座標の矩形を枠内にclampする（最小辺も保証）。
pub(crate) fn clamp_rect(rect: Rect, frame: (u16, u16)) -> Rect {
    let w = rect.w.max(MIN_SIDE).min(frame.0.max(MIN_SIDE));
    let h = rect.h.max(MIN_SIDE).min(frame.1.max(MIN_SIDE));
    let frame_w = i32::from(frame.0);
    let frame_h = i32::from(frame.1);

    let x = round_clamped(f32::from(rect.x)).clamp(0, (frame_w - i32::from(w)).max(0));
    let y = round_clamped(f32::from(rect.y)).clamp(0, (frame_h - i32::from(h)).max(0));

    Rect {
        x: u16::try_from(x).unwrap_or(0),
        y: u16::try_from(y).unwrap_or(0),
        w,
        h,
    }
}

/// 押した位置から今の位置までの移動量（ディスク座標）。
pub(crate) fn drag_delta(
    drag: &CanvasDrag,
    pointer: egui::Pos2,
    frame: (u16, u16),
    canvas: egui::Rect,
) -> (i32, i32) {
    let start = to_frame_point(drag.origin, frame, canvas);
    let now = to_frame_point(pointer, frame, canvas);
    (now.0.saturating_sub(start.0), now.1.saturating_sub(start.1))
}

/// ドラッグ結果の矩形を計算する。
pub(crate) fn dragged_rect(
    drag: &CanvasDrag,
    pointer: egui::Pos2,
    frame: (u16, u16),
    canvas: egui::Rect,
) -> Rect {
    let start_point = to_frame_point(drag.origin, frame, canvas);
    let now = to_frame_point(pointer, frame, canvas);
    let dx = now.0.saturating_sub(start_point.0);
    let dy = now.1.saturating_sub(start_point.1);

    let x = i32::from(drag.start.x).saturating_add(dx);
    let y = i32::from(drag.start.y).saturating_add(dy);

    let rect = match drag.kind {
        DragKind::Move => Rect {
            x: u16::try_from(x.max(0)).unwrap_or(u16::MAX),
            y: u16::try_from(y.max(0)).unwrap_or(u16::MAX),
            w: drag.start.w,
            h: drag.start.h,
        },
        DragKind::Resize => Rect {
            x: drag.start.x,
            y: drag.start.y,
            w: u16::try_from(
                i32::from(drag.start.w)
                    .saturating_add(dx)
                    .max(i32::from(MIN_SIDE)),
            )
            .unwrap_or(u16::MAX),
            h: u16::try_from(
                i32::from(drag.start.h)
                    .saturating_add(dy)
                    .max(i32::from(MIN_SIDE)),
            )
            .unwrap_or(u16::MAX),
        },
    };

    clamp_rect(rect, frame)
}

/// The outline of a button that overlaps another one (red, as in Encore's Menu Viewer).
const OVERLAP_COLOR: egui::Color32 = egui::Color32::from_rgb(0xff, 0x30, 0x30);

impl DiscCraftApp {
    pub(crate) fn menu_editor(&mut self, ui: &mut Ui, id: MenuId) {
        let Some(menu) = self.session.project.menu(id).cloned() else {
            ui.label(tl!("Menu not found"));
            return;
        };

        // Encore と同じく、ビューアはキャンバスだけ。ボタンのテキストやリンク、背景はプロパティパネルで設定する。
        self.menu_preview(ui, &menu);
    }

    fn menu_preview(&mut self, ui: &mut Ui, menu: &Menu) {
        // Encore のメニュービューアと同じく、操作は下のツールバー。狭いビューアでは 2 段以上に折り返す（右の
        // ページの矢印が隠れない。M9.37）。
        let _ = egui::Panel::bottom("menu_viewer_bar")
            .resizable(false)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| self.canvas_controls(ui, menu.id));
            });
        let _ = egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::both()
                .id_salt("menu_viewer_canvas")
                // ドラッグはボタンを動かすのに使う（キャンバスを動かすのはスクロールバー）。
                .scroll_source(egui::containers::scroll_area::ScrollSource {
                    scroll_bar: true,
                    drag: egui::containers::scroll_area::DragScroll::Never,
                    mouse_wheel: true,
                })
                .show(ui, |ui| {
                    // M6.525: a pick whip held near the edge of a zoomed menu scrolls it.
                    crate::pickwhip::scroll_near_edges(ui);
                    self.menu_canvas_area(ui, menu);
                });
        });
    }

    /// メニュービューアのキャンバス（拡大率に合わせた大きさ。全体表示は収まる大きさ）。
    fn menu_canvas_area(&mut self, ui: &mut Ui, menu: &Menu) {
        // ドロップシャドウのダイアログのプレビュー中は、下書きを重ねた絵を出す。
        let previewed = self
            .shadow_preview(menu)
            .or_else(|| self.effects_preview(menu))
            .or_else(|| self.color_set_preview(menu));
        let menu = previewed.as_ref().unwrap_or(menu);
        let hash = self.picture_hash(menu);
        if self.menu_texture_due(hash) {
            self.render_menu_texture(ui.ctx(), menu, hash);
        }

        let Some(cached) = &self.menu_texture else {
            return;
        };

        if cached.width == 0 || cached.height == 0 {
            return;
        }

        let area = egui::vec2(
            ui.available_width(),
            (ui.available_height() - 4.0).max(120.0),
        );
        let frame = (cached.width, cached.height);
        // Encore の「テレビ画面に合わせてメニューピクセルを校正」を外すと、ピクセルをそのまま（720x480 は 3:2）で。
        let aspect = if self.correct_pixel_aspect {
            menu_aspect(menu, frame)
        } else {
            f32::from(frame.0.max(1)) / f32::from(frame.1.max(1))
        };
        let size = self.menu_zoom.size(area, aspect);

        let response =
            ui.add(egui::Image::new((cached.handle.id(), size)).sense(egui::Sense::click()));
        let canvas = response.rect;

        self.draw_safe_areas(ui, canvas);
        // Encore: while the button routes are shown, dropping a project item on the menu makes nothing.
        if !self.show_routing {
            self.accept_canvas_drop(ui, menu, frame, &response);
        }
        self.draw_menu_graphics(ui, menu, frame, canvas);
        self.draw_menu_texts(ui, menu, frame, canvas);
        self.draw_buttons(ui, menu, frame, canvas, None);
        self.draw_selected_part(ui, menu, frame, canvas);
        self.draw_rotate_preview(ui, menu, frame, canvas);
        self.draw_guides(ui, menu, frame, canvas);
        if self.show_routing {
            self.draw_route_crosses(ui, menu, frame, canvas);
            self.draw_route_handles(ui, menu, frame, canvas);
        }
        // 手前にポップアップ（右クリックメニューなど）があるときは、キャンバスの押下として扱わない。
        let under_pointer = response.contains_pointer();
        if under_pointer {
            let pointer = ui.input(|i| i.pointer.interact_pos());
            let over_guide = match self.guide_drag {
                Some(drag) => menu.guides.get(drag.index).map(|guide| guide.orientation),
                None => pointer
                    .and_then(|point| self.hit_guide(menu, frame, canvas, point))
                    .map(|(_, guide)| guide.orientation),
            };
            let cursor = match self.tool {
                crate::menu_text::Tool::Selection => {
                    over_guide.map(|orientation| match orientation {
                        disccraft_project::GuideOrientation::Horizontal => {
                            egui::CursorIcon::ResizeVertical
                        }
                        disccraft_project::GuideOrientation::Vertical => {
                            egui::CursorIcon::ResizeHorizontal
                        }
                    })
                }
                crate::menu_text::Tool::Move => Some(egui::CursorIcon::Move),
                crate::menu_text::Tool::DirectSelection => None,
                crate::menu_text::Tool::Rotate => Some(egui::CursorIcon::Crosshair),
                crate::menu_text::Tool::Text => Some(egui::CursorIcon::Text),
                crate::menu_text::Tool::VerticalText => Some(egui::CursorIcon::VerticalText),
                crate::menu_text::Tool::Zoom if ui.input(|i| i.modifiers.alt) => {
                    Some(egui::CursorIcon::ZoomOut)
                }
                crate::menu_text::Tool::Zoom => Some(egui::CursorIcon::ZoomIn),
            };
            if let Some(cursor) = cursor {
                ui.ctx().set_cursor_icon(cursor);
            }
        }
        self.handle_canvas_drag(ui, menu, frame, canvas, under_pointer);
        if self.canvas_drag.is_none() && self.marquee.is_none() {
            self.nudge_buttons(ui.ctx(), menu, frame);
        }
        self.canvas_context_menu(ui, menu, frame, &response);
    }

    /// キャンバスの右クリック: その位置のボタンを選び（選んでいるボタンの上なら選択はそのまま）、
    /// 切り取り / コピー / 貼り付け / 複製 / 消去、名前の変更…、整列 / 均等配置。
    fn canvas_context_menu(
        &mut self,
        ui: &Ui,
        menu: &Menu,
        frame: (u16, u16),
        response: &egui::Response,
    ) {
        if response.secondary_clicked()
            && let Some(point) = response.interact_pointer_pos()
        {
            match self.hit_button(menu, frame, response.rect, point) {
                Some((index, _))
                    if self.selection == Selection::Menu(menu.id)
                        && self.button_selected(index) => {}
                Some((index, _)) => self.select_only_button(menu.id, index),
                None => self.select(Selection::Menu(menu.id)),
            }
        }
        let mut chosen = Vec::new();
        let _ = response.context_menu(|ui| {
            use crate::menu_model::MenuSink;
            let mut sink = crate::menu_model::EguiSink::new(ui);
            self.clipboard_items(&mut sink);
            let has_object = !self.selected_buttons.is_empty()
                || self.selected_menu_text(menu.id).is_some()
                || self.selected_menu_graphic(menu.id).is_some();
            sink.action(Action::Clear, has_object);
            sink.separator();
            sink.action(Action::Rename, self.primary_button().is_some());
            sink.separator();
            self.object_items(&mut sink);
            if !sink.chosen.is_empty() {
                sink.ui.close();
            }
            chosen = sink.chosen;
        });
        for command in chosen {
            self.run_menu_command(ui, command);
        }
    }

    /// メニューの静止画だけをキャンバスに描く（モニター用。ドラッグは受け付けない）。
    ///
    /// `highlight` はリモコンでハイライトされているボタン（0 始まり）。
    /// Returns where the menu's picture is drawn.
    pub(crate) fn menu_canvas(
        &mut self,
        ui: &mut Ui,
        menu: &Menu,
        highlight: Option<usize>,
        zoom: crate::monitor::MonitorZoom,
    ) -> Option<egui::Rect> {
        self.menu_canvas_over(ui, menu, (highlight, zoom), None)
    }

    /// [`Self::menu_canvas`] with `moving` (a picture of the menu playing: its rendered motion menu, M6.334) painted
    /// in place of its still picture, the buttons and highlight over it as over the still.
    pub(crate) fn menu_canvas_over(
        &mut self,
        ui: &mut Ui,
        menu: &Menu,
        (highlight, zoom): (Option<usize>, crate::monitor::MonitorZoom),
        moving: Option<&egui::TextureHandle>,
    ) -> Option<egui::Rect> {
        let previewed = self
            .shadow_preview(menu)
            .or_else(|| self.effects_preview(menu))
            .or_else(|| self.color_set_preview(menu));
        let menu = previewed.as_ref().unwrap_or(menu);
        let hash = self.picture_hash(menu);
        if self.menu_texture_due(hash) {
            self.render_menu_texture(ui.ctx(), menu, hash);
        }

        let Some(cached) = &self.menu_texture else {
            return None;
        };

        if cached.width == 0 || cached.height == 0 {
            return None;
        }

        // 拡大率に合わせた 4:3 の大きさ（全体表示はパネルに収める）。
        let frame = (cached.width, cached.height);
        let size = zoom.size(ui.available_size(), menu_aspect(menu, frame));

        // The picture is painted on exactly the rectangle the buttons and the highlight are laid over: an
        // image widget in a justified layout (the project preview's) reports the whole width it was given,
        // wider than the picture it paints, and the highlight was then drawn stretched over the picture.
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        ui.painter().image(
            moving.unwrap_or(&cached.handle).id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        self.draw_buttons(ui, menu, frame, rect, highlight);
        Some(rect)
    }

    /// 下のツールバー（Encore のメニュービューアの並びと絵のボタン。M6.487）: 拡大率 | 校正 | セーフエリア・
    /// ボタンルート・ガイド・新規ガイド | 標準・選択時・アクティベート時 | ページ送り。押している切り替えは
    /// くぼんだ箱、マウスが乗ると金色。名前はヒントに（Encore と同じ文言）。ボタンの追加は Encore と同じく
    /// メニュー ▸ ボタンを追加。
    fn canvas_controls(&mut self, ui: &mut Ui, menu: MenuId) {
        use crate::tool_icons::{Glyph, bar_button, bar_divider};
        ui.spacing_mut().item_spacing.x = 4.0;
        let mut zoom = self.menu_zoom;
        crate::monitor::zoom_combo(ui, "menu_viewer_zoom", &mut zoom);
        self.menu_zoom = zoom;
        bar_divider(ui);
        if bar_button(
            ui,
            Glyph::Calibrate,
            (self.correct_pixel_aspect, true),
            tl!("Correct menu pixels to the TV screen"),
        ) {
            self.correct_pixel_aspect = !self.correct_pixel_aspect;
        }
        bar_divider(ui);
        if bar_button(
            ui,
            Glyph::SafeArea,
            (self.show_safe_areas, true),
            tl!("Show / hide safe areas"),
        ) {
            self.show_safe_areas = !self.show_safe_areas;
        }
        if bar_button(
            ui,
            Glyph::Routing,
            (self.show_routing, true),
            tl!("Set button routing"),
        ) {
            self.show_routing = !self.show_routing;
        }
        if bar_button(
            ui,
            Glyph::Guides,
            (self.show_guides, true),
            tl!("Show guides"),
        ) {
            self.show_guides = !self.show_guides;
        }
        if bar_button(ui, Glyph::NewGuide, (false, true), tl!("New guide")) {
            self.open_guide_dialog();
        }
        bar_divider(ui);
        for state in HighlightPreview::ALL {
            if bar_button(
                ui,
                state.glyph(),
                (self.highlight_preview == Some(state), true),
                state.hint(),
            ) {
                self.toggle_highlight_preview(state);
            }
        }
        bar_divider(ui);
        self.page_buttons(ui, menu);
    }

    /// Encore の 標準 / 選択時 / アクティベート時 のボタン（とメニューの 表示 ▸ …サブピクチャを表示）を押したとき
    /// （[`toggled_preview`]）。
    pub(crate) fn toggle_highlight_preview(&mut self, state: HighlightPreview) {
        self.highlight_preview = toggled_preview(self.highlight_preview, state);
    }

    /// Encore のビューアの下の ページ送り: 前のページを表示 / 現在のページ番号（0 から）/ 次のページを表示。
    fn page_buttons(&mut self, ui: &mut Ui, menu: MenuId) {
        let pages = self.session.project.menu_pages(menu);
        let number = self.session.project.menu_page_number(menu);
        let previous = number
            .checked_sub(1)
            .and_then(|index| pages.get(index).copied());
        let next = pages.get(number.saturating_add(1)).copied();
        let mut go = None;
        // The Craft apps' shared chevrons, in the bar's look (M6.487).
        let page_button = |ui: &mut Ui, enabled: bool, icon: crate::icons::Icon, tip: &str| {
            crate::tool_icons::bar_button_with(
                ui,
                |painter, rect, color| crate::icons::paint(painter, rect, icon, color),
                (false, enabled),
                tip,
            )
        };
        if page_button(
            ui,
            previous.is_some(),
            crate::icons::Icon::ChevronLeft,
            tl!("Show Previous Page"),
        ) {
            go = previous;
        }
        // Encore: a mark, the number in its tip.
        crate::tool_icons::bar_mark(
            ui,
            crate::tool_icons::Glyph::MenuPage,
            &tlf!("Current page number: {n}", n = number),
        );
        if page_button(
            ui,
            next.is_some(),
            crate::icons::Icon::ChevronRight,
            tl!("Show Next Page"),
        ) {
            go = next;
        }
        if let Some(page) = go {
            self.select(crate::app::Selection::Menu(page));
        }
    }

    /// タイトルセーフ（5%）とアクションセーフ（10%）の枠。
    fn draw_safe_areas(&self, ui: &Ui, canvas: egui::Rect) {
        if self.show_safe_areas {
            paint_safe_areas(ui.painter(), canvas);
        }
    }

    pub(crate) fn draw_buttons(
        &self,
        ui: &Ui,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
        highlight: Option<usize>,
    ) {
        // リサイズのハンドルは、ボタンを 1 つだけ選んでいるときに出す。
        let single = self.selected_buttons.len() == 1;
        // Encore outlines overlapping buttons in red while the menu is edited (not in the monitor).
        let overlapping = match highlight {
            Some(_) => Vec::new(),
            None => disccraft_project::overlapping_buttons(menu),
        };
        for (index, button) in menu.buttons.iter().enumerate() {
            let rect = self.drag_rect_of(menu.id, index).unwrap_or(button.rect);

            let canvas_rect = to_canvas(rect, frame, canvas);
            let selected = highlight.is_none() && self.button_selected(index);
            let highlighted = highlight == Some(index);
            // ボタンのハイライトグループの色（選択時 / アクティベート時）。
            let group = menu.highlight_group(button.highlight_group);
            let [r, g, b] = group.selected;
            let selected_alpha = disccraft_project::opacity_to_alpha(group.selected_opacity);
            let ([ar, ag, ab], activated_opacity) = group.activated_look();
            let activated_alpha = disccraft_project::opacity_to_alpha(activated_opacity);
            // Encore の標準グループ: 選ばれていないボタンの枠の色（不透明度 0 なら見えない）。
            let set = menu.color_set_or_default();
            let normal = set.normal;

            // モニター（highlight を渡す）はリモコンのハイライトだけを描く。
            // 編集画面は選択中と既定ボタンをハイライトとして描く。
            let active = match highlight {
                Some(_) => highlighted,
                None => selected || self.is_default(menu, index),
            };

            // The disc's own highlight (the frame, the text of a text subpicture, or a subpicture's layers) in the
            // state's colours 1 to 3.
            // A preview (the monitor, the project preview) shows the remote's highlight on its button in its
            // selected look, whatever the editor shows. The editor shows the state of Encore's 標準 / 選択時 /
            // アクティベート時 on every button, and no subpicture while none is on (M6.489).
            let (look, lit) = match highlight {
                Some(_) => (Some(HighlightPreview::Selected), highlighted),
                None => (self.highlight_preview, true),
            };
            let tint = |n: u8| {
                let (color, opacity) = match look {
                    Some(HighlightPreview::Selected) if lit => group.selected_color(n),
                    Some(HighlightPreview::Activated) if lit => group.activated_color(n),
                    Some(_) => {
                        let normal = set.normal_color(n);
                        (normal.color, normal.opacity)
                    }
                    None => return None,
                };
                let [tr, tg, tb] = color;
                (opacity > 0).then(|| {
                    egui::Color32::from_rgba_unmultiplied(
                        tr,
                        tg,
                        tb,
                        disccraft_project::opacity_to_alpha(opacity),
                    )
                })
            };
            if let Some(cached) = self
                .menu_texture
                .as_ref()
                .filter(|cached| (cached.width, cached.height) == frame)
            {
                let (fw, fh) = (f32::from(frame.0).max(1.0), f32::from(frame.1).max(1.0));
                let uv = egui::Rect::from_min_max(
                    egui::pos2(f32::from(rect.x) / fw, f32::from(rect.y) / fh),
                    egui::pos2(
                        f32::from(rect.x.saturating_add(rect.w)) / fw,
                        f32::from(rect.y.saturating_add(rect.h)) / fh,
                    ),
                );
                // A rich-colour menu's PSD button shows its layers in their own colours in the editor, as Encore's
                // Menu Viewer does (a preview keeps the colour set's, as Encore's preview does).
                let own = cached
                    .colour_states
                    .get(index)
                    .and_then(Option::as_ref)
                    .filter(|_| highlight.is_none());
                if let Some(states) = own {
                    let state = match look {
                        Some(HighlightPreview::Selected) if lit => Some(1),
                        Some(HighlightPreview::Activated) if lit => Some(2),
                        Some(_) => Some(0),
                        None => None,
                    };
                    if let Some(Some(texture)) = state.and_then(|state| states.get(state)) {
                        let whole =
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                        ui.painter()
                            .image(texture.id(), canvas_rect, whole, egui::Color32::WHITE);
                    }
                } else {
                    for (n, mask) in (1_u8..).zip(&cached.highlights) {
                        if let (Some(tint), Some(mask)) = (tint(n), mask) {
                            ui.painter().image(mask.id(), canvas_rect, uv, tint);
                        }
                    }
                }
            }

            let color = match look {
                Some(HighlightPreview::Selected) if lit => {
                    egui::Color32::from_rgba_unmultiplied(r, g, b, selected_alpha)
                }
                Some(HighlightPreview::Activated) if lit => {
                    egui::Color32::from_rgba_unmultiplied(ar, ag, ab, activated_alpha)
                }
                Some(_) if normal.opacity > 0 => {
                    let [nr, ng, nb] = normal.color;
                    egui::Color32::from_rgba_unmultiplied(
                        nr,
                        ng,
                        nb,
                        disccraft_project::opacity_to_alpha(normal.opacity),
                    )
                }
                _ => egui::Color32::from_white_alpha(if active { 160 } else { 70 }),
            };

            let overlaps = overlapping.get(index).copied().unwrap_or(false);
            if overlaps {
                // M6.481: white with red dashes over it, so the frame shows by its stripes even where the red
                // cannot be told from the button's colour.
                let frame_rect = canvas_rect.shrink(1.0);
                let _ = ui.painter().rect_stroke(
                    frame_rect,
                    0.0,
                    egui::Stroke::new(2.0, egui::Color32::WHITE),
                    egui::StrokeKind::Middle,
                );
                let corners = [
                    frame_rect.left_top(),
                    frame_rect.right_top(),
                    frame_rect.right_bottom(),
                    frame_rect.left_bottom(),
                    frame_rect.left_top(),
                ];
                let _ = ui.painter().add(egui::Shape::dashed_line(
                    &corners,
                    egui::Stroke::new(2.0, OVERLAP_COLOR),
                    5.0,
                    4.0,
                ));
            } else {
                let stroke = egui::Stroke::new(if active { 2.0 } else { 1.0 }, color);
                let _ =
                    ui.painter()
                        .rect_stroke(canvas_rect, 0.0, stroke, egui::StrokeKind::Inside);
            }

            // 絵のないビデオサムネール（リンクがない、ビデオ以外へのリンク）は、編集画面だけ点線で場所を示す。
            if highlight.is_none()
                && let Some(area) = button.thumbnail.map(|area| area.rect_in(rect))
                && self
                    .session
                    .project
                    .thumbnail_source(&button.link)
                    .is_none()
            {
                let area = to_canvas(area, frame, canvas).shrink(1.0);
                let corners = [
                    area.left_top(),
                    area.right_top(),
                    area.right_bottom(),
                    area.left_bottom(),
                    area.left_top(),
                ];
                let _ = ui.painter().add(egui::Shape::dashed_line(
                    &corners,
                    egui::Stroke::new(1.0, egui::Color32::from_white_alpha(110)),
                    4.0,
                    3.0,
                ));
            }

            if selected && single {
                let handle = egui::Rect::from_min_size(
                    canvas_rect.max - egui::vec2(HANDLE, HANDLE),
                    egui::vec2(HANDLE, HANDLE),
                );

                ui.painter()
                    .rect_filled(handle, 0.0, egui::Color32::from_rgb(r, g, b));
            }
        }
    }

    fn is_default(&self, menu: &Menu, index: usize) -> bool {
        u32::try_from(index)
            .ok()
            .is_some_and(|value| menu.default_button == value)
    }

    /// ドラッグ中なら、そのボタンの今の矩形。
    fn drag_rect_of(&self, menu: MenuId, index: usize) -> Option<Rect> {
        self.canvas_drag.as_ref().filter(|drag| drag.menu == menu)?;
        self.drag_rects
            .iter()
            .find(|(dragged, _)| *dragged == index)
            .map(|(_, rect)| *rect)
    }

    /// キャンバスのドラッグ: ボタンの移動（選んでいるボタンをまとめて）とリサイズ、何もないところからの範囲選択。
    /// Shift / Ctrl を押しながらのクリックは選択に加える / 外す。
    fn handle_canvas_drag(
        &mut self,
        ui: &mut Ui,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
        under_pointer: bool,
    ) {
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let released = ui.input(|i| i.pointer.any_released());

        if self.text_drag.is_some() {
            self.continue_text_drag(ui, menu, frame, canvas);
            return;
        }
        if self.graphic_drag.is_some() {
            self.continue_graphic_drag(ui, menu, frame, canvas);
            return;
        }
        if self.part_drag.is_some() {
            self.continue_part_drag(ui, menu, frame, canvas);
            return;
        }
        if self.rotate_drag.is_some() {
            self.continue_rotate_drag(ui, menu, frame, canvas);
            return;
        }
        if self.route_drag.is_some() {
            self.continue_route_drag(ui, menu, frame, canvas);
            return;
        }
        if let Some(drag) = self.guide_drag {
            self.continue_guide_drag(menu, drag, pointer, released, frame, canvas);
            return;
        }
        if let Some(drag) = self.canvas_drag.clone() {
            self.continue_drag(menu, &drag, pointer, released, frame, canvas);
            return;
        }
        if let Some(marquee) = self.marquee.clone() {
            self.continue_marquee(ui, menu, &marquee, frame, canvas);
            return;
        }

        let Some(point) = pointer else {
            return;
        };
        // テキストツールは、離したときにその位置へテキストを置く（押したときに置くと、離したクリックで
        // テキストの入力欄のフォーカスが外れる）。
        let vertical = self.tool == crate::menu_text::Tool::VerticalText;
        if self.tool == crate::menu_text::Tool::Text || vertical {
            // Where the button went down: a release somewhere else is a drag (a text box).
            let origin_key = egui::Id::unique(("text_tool_origin", menu.id.0));
            if ui.input(|i| i.pointer.primary_pressed()) && under_pointer && canvas.contains(point)
            {
                ui.data_mut(|data| data.insert_temp(origin_key, point));
            }
            if ui.input(|i| i.pointer.primary_released()) && under_pointer && canvas.contains(point)
            {
                let (x, y) = to_frame_point(point, frame, canvas);
                // Dragged (Encore: drawing a text box): a paragraph text as wide as the drag (a vertical one
                // as tall), with its corner where the box's is.
                let origin = ui
                    .data_mut(|data| data.remove_temp::<egui::Pos2>(origin_key))
                    .filter(|origin| canvas.contains(*origin))
                    .map(|origin| to_frame_point(origin, frame, canvas));
                let (x, y, box_width) = match origin {
                    Some((ox, oy)) => {
                        let along = if vertical {
                            (y - oy).abs()
                        } else {
                            (x - ox).abs()
                        };
                        let side = u16::try_from(along)
                            .ok()
                            .filter(|side| *side >= disccraft_project::MIN_TEXT_BOX)
                            .map(|side| side.min(disccraft_project::MAX_TEXT_BOX));
                        match side {
                            // A vertical text's first column is at the right of its box.
                            Some(side) => (x.min(ox), y.min(oy), Some(side)),
                            None => (x, y, None),
                        }
                    }
                    None => (x, y, None),
                };
                let x =
                    u16::try_from(x.clamp(0, i32::from(frame.0.saturating_sub(1)))).unwrap_or(0);
                let y =
                    u16::try_from(y.clamp(0, i32::from(frame.1.saturating_sub(1)))).unwrap_or(0);
                self.add_menu_text(menu.id, x, y, vertical, box_width);
            }
            return;
        }
        if !ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary)) {
            return;
        }
        if !under_pointer || !canvas.contains(point) {
            return;
        }
        let modifiers = ui.input(|i| i.modifiers);
        let additive = modifiers.shift || modifiers.command;

        // ズームツールは拡大（Alt で縮小）する。
        if self.tool == crate::menu_text::Tool::Zoom {
            self.zoom_menu_viewer(!modifiers.alt);
            return;
        }
        // ダイレクト選択ツールは、ボタンの中の部品（文字 / ビデオサムネール）を選んで動かす。
        if self.tool == crate::menu_text::Tool::DirectSelection {
            self.press_direct_selection(menu, frame, canvas, point, released);
            return;
        }
        // 回転ツールは、選んでいるテキスト / 画像を中心のまわりに回す。
        if self.tool == crate::menu_text::Tool::Rotate {
            self.press_rotate_tool(menu, frame, canvas, point, released);
            return;
        }
        // 移動ツールは、どこからドラッグしても選んでいるものを動かす。
        if self.tool == crate::menu_text::Tool::Move {
            self.press_move_tool(menu, frame, canvas, point, released);
            return;
        }

        // ルートの取っ手（ルートを手で決めるときだけ出る）はいちばん手前。
        if let Some((button, direction)) = self.hit_route_handle(menu, frame, canvas, point) {
            self.press_route_handle(menu, button, direction, point, released);
            return;
        }
        // ガイドはボタンより手前でつかめる（ロック中は素通り）。
        if let Some((index, guide)) = self.hit_guide(menu, frame, canvas, point) {
            if !released {
                self.guide_drag = Some(crate::guides::GuideDrag {
                    menu: menu.id,
                    index,
                    position: guide.position,
                });
            }
            return;
        }

        // 選んでいる段落テキストの枠のハンドル: ドラッグで枠の幅（縦書きは高さ）、横書きの下の辺のハンドルは枠の高さを変える。
        if !additive && let Some(handle) = self.hit_text_box_handle(menu, frame, canvas, point) {
            self.press_text_box_handle(menu, handle, point, released);
            return;
        }

        // ボタンはテキストより手前、テキストは画像 / シェイプより手前。ボタンがなければテキスト、次に画像 /
        // シェイプを選んで動かす。
        if self.hit_button(menu, frame, canvas, point).is_none() && !additive {
            if let Some(text) = self.hit_text(menu, frame, canvas, point) {
                self.press_text(menu, text, point, released);
                return;
            }
            if let Some((index, kind)) = self.hit_graphic(menu, frame, canvas, point) {
                self.press_graphic(menu, index, kind, point, released);
                return;
            }
        }

        let Some((index, kind)) = self.hit_button(menu, frame, canvas, point) else {
            // 何もないところを押すとボタンの選択を外し、プロパティをメニューのシートに戻す。
            // そのままドラッグすると範囲で選ぶ（Shift / Ctrl を押していれば今の選択に加える）。
            let base = if additive {
                self.selected_buttons.clone()
            } else {
                self.select(Selection::Menu(menu.id));
                Vec::new()
            };
            if !released {
                self.marquee = Some(Marquee {
                    menu: menu.id,
                    origin: point,
                    base,
                });
            }
            return;
        };
        if additive {
            self.toggle_button(menu.id, index);
            return;
        }

        let group_move = kind == DragKind::Move
            && self.selection == Selection::Menu(menu.id)
            && self.button_selected(index)
            && self.selected_buttons.len() > 1;
        if group_move {
            // 選んでいるボタンをまとめて動かす。押したボタンを主にする。
            let mut selection: Vec<usize> = self
                .selected_buttons
                .iter()
                .copied()
                .filter(|selected| *selected != index)
                .collect();
            selection.push(index);
            self.set_button_selection(menu.id, selection);
        } else {
            self.select_only_button(menu.id, index);
        }
        // 押してすぐ離した（同じフレームで離した。タッチパッドのタップなど）ならクリックで、ドラッグを始めない。
        // 始めてしまうと、離したことに気づかないまま次のクリックの位置までボタンが動く。
        if released {
            return;
        }
        let Some(button) = menu.buttons.get(index) else {
            return;
        };
        let group: Vec<(usize, Rect)> = match kind {
            DragKind::Move => self
                .selected_buttons
                .iter()
                .filter_map(|selected| {
                    menu.buttons
                        .get(*selected)
                        .map(|button| (*selected, button.rect))
                })
                .collect(),
            DragKind::Resize => vec![(index, button.rect)],
        };
        self.drag_rects = group.clone();
        self.canvas_drag = Some(CanvasDrag {
            menu: menu.id,
            button: index,
            kind,
            start: button.rect,
            origin: point,
            group,
        });
    }

    /// ドラッグ中: 矩形を追いかけ、離したら動いたボタンをまとめて確定する（1 回の取り消しになる）。
    fn continue_drag(
        &mut self,
        menu: &Menu,
        drag: &CanvasDrag,
        pointer: Option<egui::Pos2>,
        released: bool,
        frame: (u16, u16),
        canvas: egui::Rect,
    ) {
        if drag.menu != menu.id {
            self.canvas_drag = None;
            self.drag_rects.clear();
            return;
        }
        if released {
            let changed: Vec<(usize, Rect)> = self
                .drag_rects
                .iter()
                .copied()
                .filter(|(index, rect)| {
                    menu.buttons
                        .get(*index)
                        .is_some_and(|button| button.rect != *rect)
                })
                .collect();
            self.canvas_drag = None;
            self.drag_rects.clear();
            self.commit_button_rects(menu.id, &changed);
            return;
        }
        if let Some(point) = pointer {
            self.drag_rects = match drag.kind {
                DragKind::Resize => {
                    let rect = dragged_rect(drag, point, frame, canvas);
                    let rect = self.snap_resize(menu, rect, frame, canvas);
                    vec![(drag.button, clamp_rect(rect, frame))]
                }
                DragKind::Move => {
                    let delta = drag_delta(drag, point, frame, canvas);
                    let (dx, dy) = self.snap_move(menu, drag.start, delta, frame, canvas);
                    moved(&drag.group, dx, dy, frame)
                }
            };
        }
    }

    /// 範囲選択中: 枠を描き、離したら枠にかかるボタンを選ぶ。
    fn continue_marquee(
        &mut self,
        ui: &Ui,
        menu: &Menu,
        marquee: &Marquee,
        frame: (u16, u16),
        canvas: egui::Rect,
    ) {
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let released = ui.input(|i| i.pointer.any_released());
        if marquee.menu != menu.id {
            self.marquee = None;
            return;
        }
        let Some(point) = pointer else {
            return;
        };
        let area = egui::Rect::from_two_pos(marquee.origin, point);
        if !released {
            let painter = ui.painter();
            painter.rect_filled(area, 0.0, egui::Color32::from_white_alpha(24));
            let _ = painter.rect_stroke(
                area,
                0.0,
                egui::Stroke::new(1.0, egui::Color32::from_white_alpha(200)),
                egui::StrokeKind::Inside,
            );
            return;
        }
        self.marquee = None;
        let mut hits = marquee.base.clone();
        for (index, button) in menu.buttons.iter().enumerate() {
            // Hidden and locked buttons (the Layers panel) are not picked.
            if button.hidden || button.locked {
                continue;
            }
            if area.intersects(to_canvas(button.rect, frame, canvas)) && !hits.contains(&index) {
                hits.push(index);
            }
        }
        if hits.is_empty() {
            self.selected_buttons.clear();
        } else {
            self.set_button_selection(menu.id, hits);
        }
    }

    pub(crate) fn hit_button(
        &self,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
        point: egui::Pos2,
    ) -> Option<(usize, DragKind)> {
        for (index, button) in menu.buttons.iter().enumerate() {
            // A hidden or locked button (the Layers panel) is not picked in the viewer.
            if button.hidden || button.locked {
                continue;
            }
            let rect = to_canvas(button.rect, frame, canvas);

            let handle = egui::Rect::from_min_size(
                rect.max - egui::vec2(HANDLE, HANDLE),
                egui::vec2(HANDLE * 2.0, HANDLE * 2.0),
            );

            if handle.contains(point) {
                return Some((index, DragKind::Resize));
            }

            if rect.contains(point) {
                return Some((index, DragKind::Move));
            }
        }

        None
    }

    /// Whether the menu's picture is to be drawn (again): it is not there, the menu changed (`hash`), or it was
    /// drawn while a video of it was still being opened and the files are open now.
    pub(crate) fn menu_texture_due(&self, hash: u64) -> bool {
        self.menu_texture
            .as_ref()
            .is_none_or(|cached| cached.hash != hash || (cached.waiting && !self.media_pending))
    }

    pub(crate) fn render_menu_texture(&mut self, ctx: &Context, menu: &Menu, hash: u64) {
        // Whether a video of the menu is still being opened (its frame is not had yet): the picture is drawn again
        // when it is open. The app looks again every 250 ms while a file is opening.
        let pending_before = std::mem::replace(&mut self.media_pending, false);
        let rendered = self.render_menu_picture(menu);
        let waiting = self.media_pending;
        self.media_pending = waiting || pending_before;
        if waiting {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        match rendered {
            Ok(rendered) => {
                let width = usize::from(rendered.width);
                let height = usize::from(rendered.height);

                if rendered.rgb.len() != width * height * 3 {
                    self.status = tl!("The menu image has an invalid size").to_string();
                    self.menu_texture = None;
                    return;
                }

                let image = egui::ColorImage::from_rgb([width, height], &rendered.rgb);
                let handle = ctx.load_texture("menu", image, egui::TextureOptions::LINEAR);
                let highlights = [1_u8, 2, 3].map(|n| {
                    highlight_mask(&rendered.highlight, width, height, n).map(|mask| {
                        ctx.load_texture(
                            format!("menu_highlight_{n}"),
                            mask,
                            egui::TextureOptions::NEAREST,
                        )
                    })
                });

                self.menu_texture = Some(MenuTexture {
                    hash,
                    handle,
                    highlights,
                    colour_states: colour_state_textures(ctx, menu),
                    width: rendered.width,
                    height: rendered.height,
                    waiting,
                });
            }
            Err(error) => {
                self.status = error.to_string();
                self.menu_texture = None;
            }
        }
    }

    /// メニューの絵。背景がビデオなら、Encore と同じくビデオの最初のフレームの上に描く（取り出せなければ黒）。
    /// Blu-ray のポップアップは、背景タイムラインがあればそのタイムラインの背景フレームの上に、なければ透明（市松）の上に描く。
    /// ビデオサムネールのボタンには、リンク先のチャプターのポスターフレームを入れる。
    pub(crate) fn render_menu_picture(
        &mut self,
        menu: &Menu,
    ) -> Result<disccraft_menu::RenderedMenu, disccraft_menu::MenuError> {
        self.render_menu_picture_checked(menu, CHECK_CELL)
    }

    /// [`Self::render_menu_picture`] with the checkerboard's cells `cell` pixels wide (a thumbnail's larger, so they
    /// are still seen once it is made small, as in Encore's flowchart).
    pub(crate) fn render_menu_picture_checked(
        &mut self,
        menu: &Menu,
        cell: usize,
    ) -> Result<disccraft_menu::RenderedMenu, disccraft_menu::MenuError> {
        let frames: Vec<(usize, std::sync::Arc<Vec<u8>>, usize, usize)> = self
            .session
            .project
            .menu_thumbnails(menu)
            .into_iter()
            .filter_map(|thumbnail| {
                let (rgb, width, height) = if thumbnail.source.still {
                    // A slideshow's first slide (Encore).
                    let (rgb, width, height) =
                        disccraft_menu::image_rgb(&self.session.project, thumbnail.source.asset)?;
                    (std::sync::Arc::new(rgb), width, height)
                } else {
                    // A still thumbnail without a poster frame: the chapter's first bright frame (Encore).
                    self.video_frame_from(
                        thumbnail.source.asset,
                        thumbnail.source.start,
                        Some(!thumbnail.source.poster && !menu.plays_buttons()),
                    )?
                };
                Some((thumbnail.button, rgb, width, height))
            })
            .collect();
        let thumbnails: Vec<disccraft_menu::ThumbnailPicture<'_>> = frames
            .iter()
            .map(
                |(button, rgb, width, height)| disccraft_menu::ThumbnailPicture {
                    button: *button,
                    rgb: rgb.as_slice(),
                    width: *width,
                    height: *height,
                },
            )
            .collect();
        let popup_picture = if menu.popup
            && let Some(background) = menu.popup_background
            && let Some(video) = self
                .session
                .project
                .timelines
                .get(&background.timeline)
                .map(|timeline| timeline.video)
        {
            self.video_frame_at(video, background.time)
        } else {
            None
        };
        // Blu-ray のポップアップの背景はディスクに出ない（下にタイトルが流れる）。背景タイムラインがなければ、Encore と
        // 同じく透明（市松）の上に描く（M6.407）。
        let popup_picture = popup_picture.or_else(|| {
            (menu.popup
                && self.session.project.settings.kind == disccraft_project::DiscKind::BluRay)
                .then(|| {
                    let (width, height) =
                        disccraft_project::frame_size(&self.session.project.settings);
                    let (width, height) = (usize::from(width), usize::from(height));
                    (
                        std::sync::Arc::new(transparency_checks(width, height, cell)),
                        width,
                        height,
                    )
                })
        });
        let wide = menu_aspect(
            menu,
            disccraft_project::frame_size(&self.session.project.settings),
        ) > 1.5;
        let video_picture = match &menu.background {
            MenuBackground::Video { asset, .. } => self.first_video_frame(*asset, wide),
            _ => None,
        };
        for (rgb, width, height) in popup_picture.iter().chain(video_picture.iter()) {
            if let Ok(rendered) = disccraft_menu::render_menu_with(
                &self.session.project,
                menu,
                disccraft_menu::MenuPictures {
                    background: Some((rgb.as_slice(), *width, *height)),
                    thumbnails: &thumbnails,
                },
            ) {
                return Ok(rendered);
            }
        }
        // 新しいメニュー（ボタンがまだない）も、背景とテキストを出す。
        disccraft_menu::render_menu_with(
            &self.session.project,
            menu,
            disccraft_menu::MenuPictures {
                background: None,
                thumbnails: &thumbnails,
            },
        )
    }

    /// メニューの絵を描き直すかどうかを決める値: メニューと、ビデオサムネールの絵の出どころ（リンク先の
    /// ポスターフレームを動かしても描き直す）。
    pub(crate) fn picture_hash(&self, menu: &Menu) -> u64 {
        let mut hasher = DefaultHasher::new();
        menu_hash(menu).hash(&mut hasher);
        for thumbnail in self.session.project.menu_thumbnails(menu) {
            (
                thumbnail.button,
                thumbnail.source.asset.0,
                thumbnail.source.start.0,
            )
                .hash(&mut hasher);
        }
        hasher.finish()
    }

    /// [`Self::first_video_frame`] for other modules.
    pub(crate) fn first_video_frame_pub(
        &mut self,
        asset: disccraft_project::AssetId,
        wide: bool,
    ) -> Option<(std::sync::Arc<Vec<u8>>, usize, usize)> {
        self.first_video_frame(asset, wide)
    }

    /// ビデオのアセットの `time` のフレーム（`asset.frame`。アセットと時刻ごとに 1 回だけ取り出して覚える）。
    fn video_frame_at(
        &mut self,
        asset: disccraft_project::AssetId,
        time: disccraft_project::Tick,
    ) -> Option<(std::sync::Arc<Vec<u8>>, usize, usize)> {
        self.video_frame_from(asset, time, None)
    }

    /// The frame at `time` (`asset.frame`, remembered). A video thumbnail (`thumbnail` is `Some(bright)`) takes the
    /// picture without letterbox bars and, when `bright`, the first bright frame from there.
    fn video_frame_from(
        &mut self,
        asset: disccraft_project::AssetId,
        time: disccraft_project::Tick,
        thumbnail: Option<bool>,
    ) -> Option<(std::sync::Arc<Vec<u8>>, usize, usize)> {
        let key = (asset.0, time.0, thumbnail);
        let (thumbnail, bright) = (thumbnail.is_some(), thumbnail == Some(true));
        if let Some(cached) = self.timed_frames.get(&key) {
            return cached.clone();
        }
        // Not waited for: asked again once the file is open.
        let value = self.run_query_quiet(
            "asset.frame",
            serde_json::json!({
                "asset": asset.0,
                "seconds": time.seconds().max(0.0),
                "bright": bright,
                "bars": !thumbnail,
                "wait": false,
            }),
        );
        if crate::project_panel::is_pending(value.as_ref()) {
            self.media_pending = true;
            return None;
        }
        let frame = value.and_then(|value| {
            let width = usize::try_from(value.get("width")?.as_u64()?).ok()?;
            let height = usize::try_from(value.get("height")?.as_u64()?).ok()?;
            let rgb: Vec<u8> = value
                .get("rgb")?
                .as_array()?
                .iter()
                .map(|byte| byte.as_u64().and_then(|b| u8::try_from(b).ok()))
                .collect::<Option<Vec<u8>>>()?;
            Some((std::sync::Arc::new(rgb), width, height))
        });
        let _ = self.timed_frames.insert(key, frame.clone());
        frame
    }

    /// ビデオのアセットの最初のフレーム（`asset.frame`。アセットごとに 1 回だけ取り出して覚える）。`wide`: 16:9 のメニューの
    /// 背景として、16:9 の画面に合わせて帯を付ける（M6.406: いつも 4:3 の画面の帯だった）。
    fn first_video_frame(
        &mut self,
        asset: disccraft_project::AssetId,
        wide: bool,
    ) -> Option<(std::sync::Arc<Vec<u8>>, usize, usize)> {
        if let Some(cached) = self.first_frames.get(&(asset.0, wide)) {
            return cached.clone();
        }
        let params = if wide {
            serde_json::json!({ "asset": asset.0, "seconds": 0.0, "width": 960, "height": 540, "wait": false })
        } else {
            serde_json::json!({ "asset": asset.0, "seconds": 0.0, "wait": false })
        };
        let value = self.run_query_quiet("asset.frame", params);
        if crate::project_panel::is_pending(value.as_ref()) {
            self.media_pending = true;
            return None;
        }
        let frame = value.and_then(|value| {
            let width = usize::try_from(value.get("width")?.as_u64()?).ok()?;
            let height = usize::try_from(value.get("height")?.as_u64()?).ok()?;
            let rgb: Vec<u8> = value
                .get("rgb")?
                .as_array()?
                .iter()
                .map(|byte| byte.as_u64().and_then(|b| u8::try_from(b).ok()))
                .collect::<Option<Vec<u8>>>()?;
            Some((std::sync::Arc::new(rgb), width, height))
        });
        let _ = self.first_frames.insert((asset.0, wide), frame.clone());
        frame
    }

    /// ボタンのテキストの入力欄を、選んだボタンが変わったときだけ合わせる。
    pub(crate) fn sync_button_label(&mut self, menu_id: MenuId, index: usize, label: &str) {
        if self.button_label_menu == Some(menu_id) && self.button_label_button == Some(index) {
            return;
        }

        self.button_label_menu = Some(menu_id);
        self.button_label_button = Some(index);
        self.button_label = label.to_string();
    }

    /// 背景とハイライトの色の入力欄を、メニューが変わったときだけ合わせる。
    pub(crate) fn sync_menu_colors(&mut self, menu: &Menu) {
        if self.color_menu == Some(menu.id) {
            return;
        }

        self.color_menu = Some(menu.id);

        let background = match &menu.background {
            MenuBackground::Color { color } => *color,
            MenuBackground::Image { .. } | MenuBackground::Video { .. } => [0, 0, 0],
        };

        self.bg_color = rgb_to_f32(background);
    }
}

/// The shape the menu is shown at: 16:9 for a widescreen menu or an HD frame (square pixels), else 4:3.
pub(crate) fn menu_aspect(menu: &Menu, frame: (u16, u16)) -> f32 {
    if frame.0 > 720 {
        f32::from(frame.0) / f32::from(frame.1.max(1))
    } else if menu.widescreen {
        16.0 / 9.0
    } else {
        4.0 / 3.0
    }
}

/// A rich-colour menu's PSD buttons' pictures in their subpicture layers' own colours (M6.335), as Encore's Menu
/// Viewer shows them: for each button, its normal, selected and activated pictures over its rectangle.
fn colour_state_textures(
    ctx: &Context,
    menu: &Menu,
) -> Vec<Option<[Option<egui::TextureHandle>; 3]>> {
    if !menu.rich_highlights {
        return Vec::new();
    }
    menu.buttons
        .iter()
        .enumerate()
        .map(|(index, button)| {
            let states = disccraft_menu::button_color_states(button)?;
            let size = [usize::from(button.rect.w), usize::from(button.rect.h)];
            let mut state = 0_u8;
            Some(states.map(|picture| {
                state = state.saturating_add(1);
                let rgba = picture?;
                // egui wants exactly the bytes of the size.
                (size[0].checked_mul(size[1])?.checked_mul(4)? == rgba.len()).then(|| {
                    ctx.load_texture(
                        format!("menu_colour_{index}_{state}"),
                        egui::ColorImage::from_rgba_unmultiplied(size, &rgba),
                        egui::TextureOptions::NEAREST,
                    )
                })
            }))
        })
        .collect()
}

/// Colour `n` (1 to 3) of the highlight layer as a white mask the size of the frame (`None` for colours 2 and 3
/// when the layer has none).
fn highlight_mask(
    layer: &disccraft_menu::HighlightLayer,
    width: usize,
    height: usize,
    n: u8,
) -> Option<egui::ColorImage> {
    if n > 1 && !layer.pixels.contains(&n) {
        return None;
    }
    let mut pixels = vec![egui::Color32::TRANSPARENT; width.checked_mul(height)?];
    let layer_w = usize::from(layer.width);
    for (index, value) in layer.pixels.iter().enumerate() {
        if *value != n || layer_w == 0 {
            continue;
        }
        let x = usize::from(layer.x).checked_add(index % layer_w)?;
        let y = usize::from(layer.y).checked_add(index / layer_w)?;
        if x < width
            && y < height
            && let Some(pixel) = y
                .checked_mul(width)
                .and_then(|row| row.checked_add(x))
                .and_then(|i| pixels.get_mut(i))
        {
            *pixel = egui::Color32::WHITE;
        }
    }
    Some(egui::ColorImage {
        size: [width, height],
        pixels,
        source_size: egui::vec2(width as f32, height as f32),
    })
}

pub(crate) fn menu_hash(menu: &Menu) -> u64 {
    let mut hasher = DefaultHasher::new();

    // ガイドは絵に入らないので、動かしても描き直さない。
    let plain;
    let menu = if menu.guides.is_empty() {
        menu
    } else {
        plain = Menu {
            guides: Vec::new(),
            ..menu.clone()
        };
        &plain
    };
    if let Ok(text) = serde_json::to_string(menu) {
        text.hash(&mut hasher);
    } else {
        menu.name.hash(&mut hasher);
        menu.buttons.len().hash(&mut hasher);
    }

    hasher.finish()
}

/// Returns selectable link targets.
pub(crate) fn link_targets(project: &Project, include_chapters: bool) -> Vec<(String, Link)> {
    let mut targets = vec![
        (tl!("Not set").to_string(), Link::NotSet),
        (tl!("Stop").to_string(), Link::Stop),
        (tl!("Last menu").to_string(), Link::LastMenu),
    ];

    for (id, timeline) in project.timelines.iter() {
        if include_chapters {
            if timeline.chapters.is_empty() {
                targets.push((
                    tlf!("{name}: chapter 1", name = timeline.name),
                    Link::Timeline {
                        id: *id,
                        chapter: 0,
                    },
                ));
            } else {
                for (index, chapter) in timeline.chapters.iter().enumerate() {
                    let chapter_label = crate::timeline_view::chapter_label(&chapter.name, index);

                    let time = format_time(chapter.time, &project.settings);
                    let chapter_index = u32::try_from(index).unwrap_or(0);

                    targets.push((
                        format!("{}: {} ({})", timeline.name, chapter_label, time),
                        Link::Timeline {
                            id: *id,
                            chapter: chapter_index,
                        },
                    ));
                }
            }
        } else {
            targets.push((
                tlf!("{name}: chapter 1", name = timeline.name),
                Link::Timeline {
                    id: *id,
                    chapter: 0,
                },
            ));
        }
    }

    for id in project.menus.keys() {
        targets.push((
            tlf!("Menu: {name}", name = menu_title(project, *id)),
            Link::Menu {
                id: *id,
                button: None,
            },
        ));
    }

    for (id, slideshow) in project.slideshows.iter() {
        targets.push((
            tlf!("Slideshow: {name}", name = slideshow.name),
            Link::Slideshow { id: *id },
        ));
    }

    for (id, playlist) in project.playlists.iter() {
        targets.push((
            tlf!("Playlist: {name}", name = playlist.name),
            Link::Playlist { id: *id },
        ));
    }

    for (id, playlist) in project.chapter_playlists.iter() {
        targets.push((
            tlf!("Chapter Playlist: {name}", name = playlist.name),
            Link::ChapterPlaylist { id: *id },
        ));
    }

    targets
}

/// Returns a human-readable label for a link.
pub(crate) fn link_label(project: &Project, link: &Link) -> String {
    match link {
        Link::NotSet => tl!("Not set").to_string(),
        Link::Stop => tl!("Stop").to_string(),
        Link::LastMenu => tl!("Last menu").to_string(),
        Link::Timeline { id, chapter } => timeline_link_label(project, *id, *chapter),
        Link::Menu { id, button } => menu_link_label(project, *id, *button),
        Link::Playlist { id } => project.playlists.get(id).map_or_else(
            || tlf!("Playlist {id}", id = id),
            |playlist| tlf!("Playlist: {name}", name = playlist.name),
        ),
        Link::ChapterPlaylist { id } => project.chapter_playlists.get(id).map_or_else(
            || tlf!("Chapter playlist {id}", id = id),
            |playlist| tlf!("Chapter Playlist: {name}", name = playlist.name),
        ),
        Link::Slideshow { id } => project.slideshows.get(id).map_or_else(
            || tlf!("Slideshow {id}", id = id),
            |slideshow| tlf!("Slideshow: {name}", name = slideshow.name),
        ),
    }
}

fn timeline_link_label(project: &Project, id: TimelineId, chapter: u32) -> String {
    let timeline = project.timeline(id);
    let name = timeline
        .map(|timeline| timeline.name.clone())
        .unwrap_or_else(|| tl!("Unknown").to_string());

    let chapter_label = timeline
        .and_then(|timeline| {
            usize::try_from(chapter)
                .ok()
                .and_then(|index| timeline.chapters.get(index))
        })
        .map(|found| {
            let index = usize::try_from(chapter).unwrap_or(usize::MAX);
            crate::timeline_view::chapter_label(&found.name, index)
        })
        .unwrap_or_else(|| tlf!("Chapter {n}", n = chapter.saturating_add(1)));

    // Encore と同じ「名前:チャプター」。
    format!("{}:{}", name, chapter_label)
}

/// A menu's name as the lists show it: a later page is its first page's name and the page number.
pub(crate) fn menu_title(project: &Project, id: MenuId) -> String {
    let root = project.menu_root(id);
    let name = project
        .menu(root)
        .map(|menu| menu.name.clone())
        .unwrap_or_else(|| tl!("Unknown").to_string());
    if root == id {
        name
    } else {
        tlf!(
            "{name}: page {n}",
            name = name,
            n = project.menu_page_number(id)
        )
    }
}

fn menu_link_label(project: &Project, id: MenuId, button: Option<u32>) -> String {
    let name = menu_title(project, id);

    // Encore と同じ「メニュー名:初期設定」/「メニュー名:ボタン」。
    match button {
        Some(index) => {
            let button = project
                .menu(id)
                .and_then(|menu| {
                    usize::try_from(index)
                        .ok()
                        .and_then(|i| menu.buttons.get(i))
                })
                .map(|button| button.label.trim().to_owned())
                .filter(|label| !label.is_empty())
                .unwrap_or_else(|| tlf!("Button {n}", n = index.saturating_add(1)));
            format!("{name}:{button}")
        }
        None => format!("{name}:{}", tl!("Default")),
    }
}

#[cfg(test)]
mod canvas_tests {
    use super::{
        CanvasDrag, DragKind, HighlightPreview, clamp_rect, drag_delta, dragged_rect, to_canvas,
        to_frame_point, toggled_preview,
    };
    use disccraft_project::{MenuId, Rect};
    use egui::{Rect as ERect, pos2, vec2};

    const NTSC: (u16, u16) = (720, 480);

    /// M6.490: a menu's picture drawn while its background video was still being opened (black where the video's
    /// first frame goes) is drawn again once the files are open, not every frame before that; a changed menu is
    /// always drawn again.
    #[test]
    fn a_menu_picture_waiting_for_its_video_is_drawn_again_once_open() {
        let ctx = egui::Context::default();
        let mut app = crate::DiscCraftApp::new(&ctx);
        assert!(app.menu_texture_due(7));
        app.menu_texture = Some(super::MenuTexture {
            hash: 7,
            handle: ctx.load_texture(
                "waiting",
                egui::ColorImage::filled([1, 1], egui::Color32::BLACK),
                egui::TextureOptions::default(),
            ),
            highlights: [None, None, None],
            colour_states: Vec::new(),
            width: 1,
            height: 1,
            waiting: true,
        });
        app.media_pending = true;
        assert!(!app.menu_texture_due(7), "still opening");
        app.media_pending = false;
        assert!(app.menu_texture_due(7), "open now");
        if let Some(texture) = app.menu_texture.as_mut() {
            texture.waiting = false;
        }
        assert!(!app.menu_texture_due(7));
        assert!(app.menu_texture_due(8));
    }

    /// Encore's 標準 / 選択時 / アクティベート時 (M6.489, measured): one at a time, none at first; pressing another
    /// moves to it, pressing the one on turns them all off.
    #[test]
    fn the_subpicture_buttons_go_one_at_a_time_and_off_again() {
        use HighlightPreview::{Activated, Normal, Selected};
        assert_eq!(toggled_preview(None, Normal), Some(Normal));
        assert_eq!(toggled_preview(Some(Normal), Selected), Some(Selected));
        assert_eq!(toggled_preview(Some(Selected), Selected), None);
        assert_eq!(toggled_preview(Some(Selected), Activated), Some(Activated));
        assert_eq!(toggled_preview(Some(Activated), Activated), None);
    }

    fn canvas() -> ERect {
        ERect::from_min_size(pos2(100.0, 50.0), vec2(360.0, 240.0))
    }

    #[test]
    fn canvas_maps_frame_to_display() {
        let rect = Rect {
            x: 360,
            y: 240,
            w: 360,
            h: 240,
        };

        let drawn = to_canvas(rect, NTSC, canvas());
        assert_eq!(drawn.min, pos2(280.0, 170.0));
        assert_eq!(drawn.max, pos2(460.0, 290.0));
    }

    #[test]
    fn frame_point_round_trips() {
        let drawn = to_canvas(
            Rect {
                x: 120,
                y: 80,
                w: 10,
                h: 10,
            },
            NTSC,
            canvas(),
        );

        let point = to_frame_point(drawn.min, NTSC, canvas());
        assert_eq!(point, (120, 80));
    }

    #[test]
    fn clamp_rect_keeps_the_button_in_frame() {
        let clamped = clamp_rect(
            Rect {
                x: 700,
                y: 470,
                w: 400,
                h: 400,
            },
            NTSC,
        );

        assert!(u32::from(clamped.x) + u32::from(clamped.w) <= 720);
        assert!(u32::from(clamped.y) + u32::from(clamped.h) <= 480);
        assert!(clamped.w >= 8 && clamped.h >= 8);
    }

    #[test]
    fn drag_moves_by_the_pointer_delta() {
        let start = Rect {
            x: 100,
            y: 100,
            w: 200,
            h: 40,
        };

        let drag = CanvasDrag {
            menu: MenuId(1),
            button: 0,
            kind: DragKind::Move,
            start,
            origin: pos2(150.0, 100.0),
            group: Vec::new(),
        };

        // 100 画面ピクセル = 200 ディスクピクセル（x）、100 = 200（y は 240/480 なので 200）。
        let moved = dragged_rect(&drag, pos2(250.0, 150.0), NTSC, canvas());
        assert_eq!(moved.x, 300);
        assert_eq!(moved.y, 200);
        assert_eq!((moved.w, moved.h), (200, 40));
    }

    #[test]
    fn a_group_moves_by_the_pointer_delta() {
        let first = Rect {
            x: 100,
            y: 100,
            w: 200,
            h: 40,
        };
        let second = Rect { y: 200, ..first };
        let drag = CanvasDrag {
            menu: MenuId(1),
            button: 0,
            kind: DragKind::Move,
            start: first,
            origin: pos2(150.0, 100.0),
            group: vec![(0, first), (3, second)],
        };

        let (dx, dy) = drag_delta(&drag, pos2(160.0, 105.0), NTSC, canvas());
        assert_eq!((dx, dy), (20, 10));
        let moved = crate::button_edit::moved(&drag.group, dx, dy, NTSC);
        let positions: Vec<(usize, u16, u16)> = moved
            .iter()
            .map(|(index, rect)| (*index, rect.x, rect.y))
            .collect();
        assert_eq!(positions, vec![(0, 120, 110), (3, 120, 210)]);
    }

    #[test]
    fn drag_resizes_the_button() {
        let start = Rect {
            x: 100,
            y: 100,
            w: 200,
            h: 40,
        };

        let drag = CanvasDrag {
            menu: MenuId(1),
            button: 2,
            kind: DragKind::Resize,
            start,
            origin: pos2(200.0, 120.0),
            group: Vec::new(),
        };

        let resized = dragged_rect(&drag, pos2(250.0, 140.0), NTSC, canvas());
        assert_eq!((resized.x, resized.y), (100, 100));
        assert!(resized.w > start.w);
        assert!(resized.h > start.h);
    }

    #[test]
    fn drag_never_leaves_the_frame() {
        let drag = CanvasDrag {
            menu: MenuId(1),
            button: 0,
            kind: DragKind::Move,
            start: Rect {
                x: 700,
                y: 460,
                w: 20,
                h: 20,
            },
            origin: pos2(100.0, 50.0),
            group: Vec::new(),
        };

        let moved = dragged_rect(&drag, pos2(900.0, 600.0), NTSC, canvas());
        assert_eq!(u32::from(moved.x) + u32::from(moved.w), 720);
        assert_eq!(u32::from(moved.y) + u32::from(moved.h), 480);
    }
}
