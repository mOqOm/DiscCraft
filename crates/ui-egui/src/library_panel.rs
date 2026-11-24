//! ライブラリパネル（Encore のライブラリに当たる）。
//!
//! DiscCraft 独自のプリセット（メニュー、ボタン、背景。どれもコードで作った色と四角だけ）と、メニューに置ける
//! 画像（プロジェクトの画像アセット）とシェイプ（長方形 / 角丸長方形 / 楕円）を一覧にする。
//! 上に「セット」とプレビュー、種類ごとの表示の切り替え（絵のボタン）、枠で囲んだ一覧（行の頭に種類の絵）、
//! 下の右に操作の絵のボタン（配置 / 置換 / 背景を設定 / 新規メニュー / 項目を追加 / 項目を削除）。どれも Encore の
//! ライブラリの並びと形（M6.485）。一覧の項目をダブルクリックすると、その種類のいちばんよく使う操作をする。

use crate::DiscCraftApp;
use crate::app::Selection;
use crate::i18n::{tl, tlf};
use crate::user_library::{FileItem, FileKind, LayerItem, UserItem, UserItemKind};
use disccraft_project::{
    AssetId, AssetKind, GraphicKind, LAYER_PRESETS, LIBRARY, LibraryItem, LibraryKind, Link,
    MenuGraphic, MenuId, ShapeKind, layer_preset, library_item,
};
use egui::{Color32, Ui};
use serde_json::{Value, json};

/// 一覧で選べるもの: プリセット、プロジェクトの画像アセット、シェイプ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LibraryPick {
    /// `LIBRARY` の項目の ID。
    Preset(&'static str),
    /// プロジェクトの画像アセット。
    Image(AssetId),
    /// シェイプ。
    Shape(ShapeKind),
    /// ユーザーの項目（`user_library.items` の番号）。
    User(usize),
    /// 組み込みのレイヤーセット / 置換レイヤー（`LAYER_PRESETS` の ID。M6.331）。
    Layers(&'static str),
}

/// A Library item being dragged (to the menu viewer, where it is placed at the drop point).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LibraryDrag(pub(crate) LibraryPick);

/// ライブラリのセット（Encore の「セット :」）: 一般（組み込み）、ユーザー（項目を追加 で足したもの）、自分で作った
/// セット（Encore の 新規セット。`user_library.sets` の番号）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LibrarySet {
    #[default]
    General,
    User,
    Own(usize),
}

/// The New Set / Rename Set dialog: the set being renamed (`None`: a new one) and the name typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SetNameDraft {
    pub(crate) renaming: Option<usize>,
    pub(crate) name: String,
}

/// 種類の表示の切り替え（Encore の並び: メニュー / ボタン / 画像 / 背景 / レイヤーセット / テキストアイテム /
/// シェイプ / 置換レイヤー）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LibraryGroup {
    Menus,
    Buttons,
    Images,
    Backgrounds,
    LayerSets,
    Texts,
    Shapes,
    Replacements,
}

/// How many kinds the Library shows (Encore's eight).
const GROUPS: usize = 8;

impl LibraryGroup {
    const ALL: [LibraryGroup; GROUPS] = [
        LibraryGroup::Menus,
        LibraryGroup::Buttons,
        LibraryGroup::Images,
        LibraryGroup::Backgrounds,
        LibraryGroup::LayerSets,
        LibraryGroup::Texts,
        LibraryGroup::Shapes,
        LibraryGroup::Replacements,
    ];

    fn index(self) -> usize {
        match self {
            LibraryGroup::Menus => 0,
            LibraryGroup::Buttons => 1,
            LibraryGroup::Images => 2,
            LibraryGroup::Backgrounds => 3,
            LibraryGroup::LayerSets => 4,
            LibraryGroup::Texts => 5,
            LibraryGroup::Shapes => 6,
            LibraryGroup::Replacements => 7,
        }
    }

    fn of_kind(kind: LibraryKind) -> Self {
        match kind {
            LibraryKind::Menu => LibraryGroup::Menus,
            LibraryKind::Button => LibraryGroup::Buttons,
            LibraryKind::Background => LibraryGroup::Backgrounds,
        }
    }

    /// The kind toggle's tip (Encore's words).
    fn hint(self) -> &'static str {
        match self {
            LibraryGroup::Menus => tl!("Toggle menu display"),
            LibraryGroup::Buttons => tl!("Toggle button display"),
            LibraryGroup::Images => tl!("Toggle image display"),
            LibraryGroup::Backgrounds => tl!("Toggle background display"),
            LibraryGroup::LayerSets => tl!("Toggle layer set display"),
            LibraryGroup::Replacements => tl!("Toggle replacement layer display"),
            LibraryGroup::Texts => tl!("Toggle text item display"),
            LibraryGroup::Shapes => tl!("Toggle shape display"),
        }
    }
}

/// ライブラリパネルの状態。
#[derive(Clone, Debug)]
pub(crate) struct LibraryState {
    /// 選択中の項目。
    pub(crate) selected: Option<LibraryPick>,
    /// 表示する種類（[`LibraryGroup::ALL`] の順）。
    pub(crate) show: [bool; GROUPS],
    /// 表示しているセット。
    pub(crate) set: LibrarySet,
}

impl Default for LibraryState {
    fn default() -> Self {
        Self {
            selected: None,
            show: [true; GROUPS],
            set: LibrarySet::General,
        }
    }
}

/// プレビューの高さ。
const PREVIEW_HEIGHT: f32 = 120.0;

/// The preview's height in a panel of `ui`'s height: [`PREVIEW_HEIGHT`], or less in a short panel (a third of it,
/// at least 48), so that the list under it stays in sight (M9.37: on a small screen it took the whole panel).
fn preview_height(ui: &Ui) -> f32 {
    PREVIEW_HEIGHT.min((ui.available_height() / 3.0).max(48.0))
}

/// 一覧の 1 行。
#[derive(Clone, Debug)]
struct LibraryRow {
    pick: LibraryPick,
    name: String,
    /// 種類の名前（行の上にマウスを置くと出る）。
    kind: &'static str,
    /// 種類（行の頭の絵）。
    group: LibraryGroup,
}

/// The kinds shown after a click on the toggle of kind `index` (Encore's library, M6.485): a click shows that kind
/// alone; a Ctrl+click (⌘ on a Mac) shows or hides it and leaves the others as they are. A click on the only kind
/// shown shows every kind again (DiscCraft's way back to all of them in one click).
fn kinds_after_click(show: [bool; GROUPS], index: usize, add: bool) -> [bool; GROUPS] {
    let mut next = show;
    if add {
        if let Some(shown) = next.get_mut(index) {
            *shown = !*shown;
        }
        return next;
    }
    let alone: [bool; GROUPS] = std::array::from_fn(|kind| kind == index);
    if show == alone { [true; GROUPS] } else { alone }
}

impl DiscCraftApp {
    /// ライブラリのタブ。
    pub(crate) fn library_tab(&mut self, ui: &mut Ui) {
        self.poll_library_import(ui.ctx());
        ui.horizontal(|ui| {
            ui.label(tl!("Set:"));
            let mut sets = vec![
                (LibrarySet::General, tl!("General").to_owned()),
                (LibrarySet::User, tl!("User").to_owned()),
            ];
            sets.extend(
                self.user_library
                    .sets
                    .iter()
                    .enumerate()
                    .map(|(index, name)| (LibrarySet::Own(index), name.clone())),
            );
            let current = sets
                .iter()
                .find(|(set, _)| *set == self.library.set)
                .map_or(String::new(), |(_, name)| name.clone());
            let mut chosen = None;
            egui::ComboBox::from_id_salt("library_set")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (set, name) in sets {
                        if ui.selectable_label(set == self.library.set, name).clicked() {
                            chosen = Some(set);
                        }
                    }
                })
                .response
                .on_hover_text(tl!("Select a library set"));
            if let Some(set) = chosen
                && set != self.library.set
            {
                self.library.set = set;
                self.library.selected = None;
            }
        });

        let selected = self.library.selected;
        self.library_preview(ui, selected);

        // Encore: the eight kinds as picture toggles in a row in the middle (M6.485). A narrow panel wraps them, so
        // that none is cut off (M9.37).
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = crate::library_icons::TOGGLE_GAP;
            let spare = ui.available_width() - crate::library_icons::TOGGLES_WIDTH;
            if spare > 0.0 {
                ui.add_space(spare / 2.0);
            }
            let add = ui.input(|input| input.modifiers.command);
            for group in LibraryGroup::ALL {
                let response = crate::library_icons::kind_toggle(
                    ui,
                    group,
                    self.library_shows(group),
                )
                .on_hover_text(format!(
                    "{}\n{}",
                    group.hint(),
                    tl!("Click: this kind only (again: every kind). Ctrl+click: add or remove it.")
                ));
                if response.clicked() {
                    self.library.show = kinds_after_click(self.library.show, group.index(), add);
                }
            }
        });
        ui.add_space(2.0);

        // The list fills the panel down to the row of actions.
        let spacing = ui.spacing().item_spacing.y;
        let list_height = ui.available_height() - crate::library_icons::TOOLS_HEIGHT - spacing;
        self.library_list(ui, list_height);
        self.library_tools(ui, selected);
    }

    fn library_shows(&self, group: LibraryGroup) -> bool {
        self.library
            .show
            .get(group.index())
            .copied()
            .unwrap_or(true)
    }

    /// The name of the set of one's own that `set` is (empty for "User"); `None` for "General".
    pub(crate) fn own_set_name(&self, set: LibrarySet) -> Option<String> {
        match set {
            LibrarySet::General => None,
            LibrarySet::User => Some(String::new()),
            LibrarySet::Own(index) => Some(
                self.user_library
                    .sets
                    .get(index)
                    .cloned()
                    .unwrap_or_default(),
            ),
        }
    }

    /// 一覧の行: 一般のセットはプリセット、画像アセット、シェイプ。ユーザーと自分のセットは足した項目（表示している種類だけ）。
    fn library_rows(&self) -> Vec<LibraryRow> {
        let mut rows = Vec::new();
        if let Some(set) = self.own_set_name(self.library.set) {
            for (index, item) in self
                .user_library
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.set == set)
            {
                let (group, kind) = match item.kind {
                    UserItemKind::Menu(_) => (LibraryGroup::Menus, kind_label(LibraryKind::Menu)),
                    UserItemKind::Button(_) => {
                        (LibraryGroup::Buttons, kind_label(LibraryKind::Button))
                    }
                    UserItemKind::Background(_) => (
                        LibraryGroup::Backgrounds,
                        kind_label(LibraryKind::Background),
                    ),
                    UserItemKind::Text(_) => (LibraryGroup::Texts, tl!("Text")),
                    UserItemKind::Layers(LayerItem {
                        replacement: true, ..
                    }) => (LibraryGroup::Replacements, tl!("Replacement Layer")),
                    UserItemKind::Layers(_) => (LibraryGroup::LayerSets, tl!("Layer Set")),
                    UserItemKind::File(ref file) => match file.kind {
                        FileKind::Menu => (LibraryGroup::Menus, kind_label(LibraryKind::Menu)),
                        FileKind::Button => {
                            (LibraryGroup::Buttons, kind_label(LibraryKind::Button))
                        }
                        FileKind::Image => (LibraryGroup::Images, tl!("Image")),
                        FileKind::LayerSet => (LibraryGroup::LayerSets, tl!("Layer Set")),
                        FileKind::Replacement => {
                            (LibraryGroup::Replacements, tl!("Replacement Layer"))
                        }
                    },
                };
                if self.library_shows(group) {
                    rows.push(LibraryRow {
                        pick: LibraryPick::User(index),
                        name: item.name.clone(),
                        kind,
                        group,
                    });
                }
            }
            return rows;
        }
        for item in LIBRARY {
            let group = LibraryGroup::of_kind(item.kind);
            if self.library_shows(group) {
                rows.push(LibraryRow {
                    pick: LibraryPick::Preset(item.id),
                    name: item_name(item.id).to_owned(),
                    kind: kind_label(item.kind),
                    group,
                });
            }
        }
        if self.library_shows(LibraryGroup::Images) {
            for asset in self.session.project.assets.values() {
                if asset.kind == AssetKind::Image && !asset.menu_file {
                    rows.push(LibraryRow {
                        pick: LibraryPick::Image(asset.id),
                        name: asset.name.clone(),
                        kind: tl!("Image"),
                        group: LibraryGroup::Images,
                    });
                }
            }
        }
        if self.library_shows(LibraryGroup::Shapes) {
            for shape in ShapeKind::ALL {
                rows.push(LibraryRow {
                    pick: LibraryPick::Shape(shape),
                    name: crate::menu_graphic::shape_label(shape).to_owned(),
                    kind: tl!("Shape"),
                    group: LibraryGroup::Shapes,
                });
            }
        }
        // DiscCraft's own layer sets and replacement layers (M6.331).
        for preset in LAYER_PRESETS {
            let (group, kind) = if preset.replacement {
                (LibraryGroup::Replacements, tl!("Replacement Layer"))
            } else {
                (LibraryGroup::LayerSets, tl!("Layer Set"))
            };
            if self.library_shows(group) {
                rows.push(LibraryRow {
                    pick: LibraryPick::Layers(preset.id),
                    name: layer_preset_name(preset.id).to_owned(),
                    kind,
                    group,
                });
            }
        }
        rows
    }

    /// 項目の一覧。Encore（M6.485）: パネルの地に暗い線の箱、行の頭に種類の絵、行の間に線（種類の列はない。
    /// 種類の名前は行の上にマウスを置くと出る）。箱は `height` の高さで、行が多いと中だけがスクロールする。
    fn library_list(&mut self, ui: &mut Ui, height: f32) {
        let rows = self.library_rows();
        let (frame, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), height.max(48.0)),
            egui::Sense::hover(),
        );
        ui.painter().rect_stroke(
            frame,
            0.0,
            egui::Stroke::new(1.0, crate::theme::LIST_EDGE),
            egui::StrokeKind::Inside,
        );
        let mut clicked = None;
        let mut double_clicked = None;
        let mut menu_action = None;
        let mut inside = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(frame.shrink(1.0))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        egui::ScrollArea::vertical()
            .id_salt("library_rows")
            .auto_shrink([false, false])
            .show(&mut inside, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let row_height = ui.spacing().interact_size.y.max(18.0);
                let font = egui::TextStyle::Body.resolve(ui.style());
                for row in &rows {
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), row_height),
                        egui::Sense::click_and_drag(),
                    );
                    let chosen = self.library.selected == Some(row.pick);
                    if ui.is_rect_visible(rect) {
                        let painter = ui.painter();
                        if chosen {
                            painter.rect_filled(rect, 0.0, crate::theme::SELECTION_ROW);
                        }
                        painter.hline(
                            rect.x_range(),
                            rect.max.y - 0.5,
                            egui::Stroke::new(1.0, crate::theme::LIST_ROW_LINE),
                        );
                        let text = crate::theme::TEXT.get();
                        crate::library_icons::row_picture(
                            painter,
                            egui::pos2(rect.min.x + 14.0, rect.center().y),
                            row.group,
                            text,
                        );
                        painter.text(
                            egui::pos2(rect.min.x + 28.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            &row.name,
                            font.clone(),
                            text,
                        );
                    }
                    let response = response.on_hover_text(row.kind);
                    // Encore: an item dragged into the menu viewer is placed where it is dropped.
                    if response.drag_started() {
                        egui::DragAndDrop::set_payload(ui.ctx(), LibraryDrag(row.pick));
                    }
                    if response.clicked() {
                        clicked = Some(row.pick);
                    }
                    if response.double_clicked() {
                        double_clicked = Some(row.pick);
                    }
                    let _ = response.context_menu(|ui| {
                        if let Some(action) = self.library_menu_items(ui, row.pick) {
                            menu_action = Some((row.pick, action));
                        }
                    });
                }
            });
        if let Some(pick) = clicked {
            self.library.selected = Some(pick);
        }
        if let Some(pick) = double_clicked {
            self.library.selected = Some(pick);
            self.library_default_action(pick);
        }
        if let Some((pick, action)) = menu_action {
            self.run_library_action(pick, action);
        }
    }

    /// 選んでいる項目の種類（画像とシェイプはボタンと同じく「配置」）。
    fn pick_kind(&self, pick: Option<LibraryPick>) -> Option<LibraryKind> {
        match pick? {
            LibraryPick::Preset(id) => library_item(id).map(|item| item.kind),
            LibraryPick::Image(_) | LibraryPick::Shape(_) | LibraryPick::Layers(_) => {
                Some(LibraryKind::Button)
            }
            LibraryPick::User(index) => {
                self.user_library
                    .items
                    .get(index)
                    .map(|item| match item.kind {
                        UserItemKind::Menu(_)
                        | UserItemKind::File(FileItem {
                            kind: FileKind::Menu,
                            ..
                        }) => LibraryKind::Menu,
                        // A file's picture, layer set, replacement layer or button is placed.
                        UserItemKind::File(_) => LibraryKind::Button,
                        // A text item and layers are placed, as a button is.
                        UserItemKind::Button(_)
                        | UserItemKind::Text(_)
                        | UserItemKind::Layers(_) => LibraryKind::Button,
                        UserItemKind::Background(_) => LibraryKind::Background,
                    })
            }
        }
    }

    /// 下の右の絵のボタン（Encore と同じ並びと名前: 配置 / 置換 / 背景を設定 / 新規メニュー / 項目を追加 /
    /// 項目を削除。M6.485）。使えないものは淡く、ヒントの 1 行目が名前、2 行目が説明。
    fn library_tools(&mut self, ui: &mut Ui, selected: Option<LibraryPick>) {
        use crate::library_icons::Tool;
        let menu = self.viewer_menu();
        let kind = self.pick_kind(selected);
        let placeable = kind == Some(LibraryKind::Button);
        let is_menu = kind == Some(LibraryKind::Menu);
        let is_background = kind == Some(LibraryKind::Background);
        let is_user = matches!(selected, Some(LibraryPick::User(_)));
        let tools = [
            (
                Tool::Place,
                LibraryAction::Place,
                placeable && menu.is_some(),
                tl!("Place"),
                tl!("Place the selected item in the open menu"),
            ),
            (
                Tool::Replace,
                LibraryAction::Replace,
                is_menu && menu.is_some(),
                tl!("Replace"),
                tl!("Replace the open menu with the selected menu, keeping its links"),
            ),
            (
                Tool::SetBackground,
                LibraryAction::SetBackground,
                is_background && menu.is_some(),
                tl!("Set Background"),
                tl!("Use the selected background in the open menu"),
            ),
            (
                Tool::NewMenu,
                LibraryAction::NewMenu,
                is_menu,
                tl!("New Menu"),
                tl!("Create a new menu from the selected menu"),
            ),
            (
                Tool::AddItem,
                LibraryAction::AddItem,
                menu.is_some(),
                tl!("Add Item"),
                tl!(
                    "Save the open menu (or its selected button, text, image or shape) to the User set of the library"
                ),
            ),
            (
                Tool::Delete,
                LibraryAction::Remove,
                is_user,
                tl!("Delete Item"),
                tl!("Remove the selected item from the library"),
            ),
        ];
        let mut action = None;
        let _ = ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), crate::library_icons::TOOLS_HEIGHT),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                // Right to left: the last first.
                for (tool, tool_action, enabled, name, hint) in tools.into_iter().rev() {
                    let response = crate::library_icons::tool_button(ui, tool, enabled)
                        .on_hover_text(format!("{name}\n{hint}"));
                    if enabled && response.clicked() {
                        action = Some(tool_action);
                    }
                }
            },
        );
        match action {
            Some(LibraryAction::AddItem) => self.add_library_item(),
            Some(action) => {
                if let Some(pick) = selected {
                    self.run_library_action(pick, action);
                }
            }
            None => {}
        }
    }

    /// 項目を追加: 開いているメニュー（ボタンを選んでいればそのボタン、テキストを選んでいればそのテキスト）を
    /// ユーザーのセットに保存する。
    fn add_library_item(&mut self) {
        let Some(menu) = self.viewer_menu() else {
            self.status = tl!("Open a menu first").to_owned();
            return;
        };
        // A selected text object: a text item (Encore's text items), named by its first line.
        if let Some(text) = self.selected_menu_text(menu).and_then(|index| {
            self.session
                .project
                .menu(menu)
                .and_then(|found| found.texts.get(index).cloned())
        }) {
            let name: String = text
                .text
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or_default()
                .trim()
                .chars()
                .take(40)
                .collect();
            self.store_user_item(UserItem {
                name,
                kind: UserItemKind::Text(text),
                set: String::new(),
            });
            return;
        }
        // A selected image or shape: a layer set of it, or a replacement layer (M6.331).
        if let Some(graphic) = self.selected_menu_graphic(menu).and_then(|index| {
            self.session
                .project
                .menu(menu)
                .and_then(|found| found.graphics.get(index).cloned())
        }) {
            let name = graphic.kind_name(&self.session.project);
            let item = self.layer_item(vec![graphic]);
            self.store_user_item(UserItem {
                name,
                kind: UserItemKind::Layers(item),
                set: String::new(),
            });
            return;
        }
        let project = &self.session.project;
        let item = match self
            .primary_button()
            .filter(|_| self.selection == Selection::Menu(menu))
        {
            Some(index) => {
                let Some(spec) = project.button_spec(menu, index) else {
                    return;
                };
                let name = project
                    .menu(menu)
                    .and_then(|m| m.buttons.get(index))
                    .map(|button| {
                        if button.label.trim().is_empty() {
                            button.name.clone()
                        } else {
                            button.label.clone()
                        }
                    })
                    .unwrap_or_default();
                UserItem {
                    name,
                    kind: UserItemKind::Button(spec),
                    set: String::new(),
                }
            }
            None => {
                let Some(spec) = project.menu_spec(menu) else {
                    return;
                };
                let name = project
                    .menu(menu)
                    .map(|m| m.name.clone())
                    .unwrap_or_default();
                UserItem {
                    name,
                    kind: UserItemKind::Menu(spec),
                    set: String::new(),
                }
            }
        };
        self.store_user_item(item);
    }

    /// The open menu's image and shape layers saved as one layer set (Encore's layer set item, M6.331).
    fn add_menu_layers_item(&mut self) {
        let Some(menu) = self.viewer_menu() else {
            self.status = tl!("Open a menu first").to_owned();
            return;
        };
        let Some((name, graphics)) = self
            .session
            .project
            .menu(menu)
            .map(|found| (found.name.clone(), found.graphics.clone()))
        else {
            return;
        };
        if graphics.is_empty() {
            self.status = tl!("The menu has no image or shape layers").to_owned();
            return;
        }
        let item = self.layer_item(graphics);
        self.store_user_item(UserItem {
            name: tlf!("{name} Layers", name = name),
            kind: UserItemKind::Layers(item),
            set: String::new(),
        });
    }

    /// `graphics` as a Library item: each image's file kept, so it is found or imported where it is placed; one
    /// replacement layer is a replacement layer item.
    fn layer_item(&self, graphics: Vec<MenuGraphic>) -> LayerItem {
        let images = graphics
            .iter()
            .map(|graphic| match graphic.kind {
                GraphicKind::Image { asset } => self
                    .session
                    .project
                    .assets
                    .get(&asset)
                    .map(|found| found.path.clone()),
                GraphicKind::Shape { .. } => None,
            })
            .collect();
        let replacement = graphics.len() == 1 && graphics.iter().all(|graphic| graphic.replacement);
        LayerItem {
            graphics,
            images,
            replacement,
        }
    }

    /// Places Library layers in `menu` (their top left corner at `at`, frame pixels; else where they were) and
    /// selects the top one: an image's asset is the project's asset of that file, imported when there is none.
    pub(crate) fn place_layers(&mut self, menu: MenuId, item: &LayerItem, at: Option<(u16, u16)>) {
        let mut graphics = item.graphics.clone();
        for (graphic, image) in graphics.iter_mut().zip(&item.images) {
            let (GraphicKind::Image { asset }, Some(path)) = (&mut graphic.kind, image) else {
                continue;
            };
            let known = self
                .session
                .project
                .assets
                .values()
                .find(|found| found.kind == AssetKind::Image && &found.path == path)
                .map(|found| found.id);
            let id = match known {
                Some(id) => Some(id),
                None => self
                    .run_command("asset.import", json!({ "path": path }))
                    .and_then(|imported| imported.get("asset").and_then(Value::as_u64))
                    .map(AssetId),
            };
            match id {
                Some(id) => *asset = id,
                None => return,
            }
        }
        let mut params = json!({ "menu": menu.0, "graphics": graphics });
        if let (Some((x, y)), Some(map)) = (at, params.as_object_mut()) {
            map.insert("x".to_owned(), json!(x));
            map.insert("y".to_owned(), json!(y));
        }
        let placed = self.run_command("menu.placeLayers", params);
        let top = placed
            .as_ref()
            .and_then(|value| value.get("graphics"))
            .and_then(Value::as_array)
            .and_then(|indices| indices.last())
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok());
        if let Some(index) = top {
            self.select_menu_graphic(menu, index);
        }
    }

    /// A built-in layer set or replacement layer as a Library item of the open project's frame.
    fn preset_layers(&self, id: &str) -> Option<LayerItem> {
        let preset = layer_preset(id)?;
        let graphics = preset.graphics(&self.session.project.settings);
        Some(LayerItem {
            images: vec![None; graphics.len()],
            graphics,
            replacement: preset.replacement,
        })
    }

    /// Adds `item` to the set shown (a set of one's own, else the User set), saves the library and shows the item.
    pub(crate) fn store_user_item(&mut self, mut item: UserItem) {
        let name = item.name.clone();
        let set = match self.library.set {
            LibrarySet::Own(index) if index < self.user_library.sets.len() => {
                LibrarySet::Own(index)
            }
            _ => LibrarySet::User,
        };
        item.set = self.own_set_name(set).unwrap_or_default();
        match self.user_library.add(item) {
            Ok(index) => {
                if let Err(error) = self.user_library.save() {
                    self.status = tlf!("Could not save the library: {error}", error = error);
                    return;
                }
                self.library.set = set;
                self.library.selected = Some(LibraryPick::User(index));
                self.status = tlf!("Added {name} to the library", name = name);
            }
            Err(error) => self.status = error,
        }
    }

    /// 削除: ユーザーの項目を外す（ライブラリのファイルは、ほかの項目が使っていなければ一緒に消す。M6.403）。
    fn remove_user_item(&mut self, index: usize) {
        if self.user_library.remove_item(index).is_none() {
            return;
        }
        if let Err(error) = self.user_library.save() {
            self.status = tlf!("Could not save the library: {error}", error = error);
        }
        self.library.selected = None;
    }

    /// 右クリックメニュー（配置 / 新規メニューを作成 / 背景を設定、ユーザーの項目には 削除）。押した操作を返す。
    fn library_menu_items(&mut self, ui: &mut Ui, pick: LibraryPick) -> Option<LibraryAction> {
        let menu = self.viewer_menu();
        let kind = self.pick_kind(Some(pick));
        let mut action = None;
        if ui
            .add_enabled(
                kind == Some(LibraryKind::Button) && menu.is_some(),
                egui::Button::new(tl!("Place")),
            )
            .clicked()
        {
            action = Some(LibraryAction::Place);
        }
        if ui
            .add_enabled(
                kind == Some(LibraryKind::Menu) && menu.is_some(),
                egui::Button::new(tl!("Replace")),
            )
            .clicked()
        {
            action = Some(LibraryAction::Replace);
        }
        if ui
            .add_enabled(
                kind == Some(LibraryKind::Menu),
                egui::Button::new(tl!("Create New Menu")),
            )
            .clicked()
        {
            action = Some(LibraryAction::NewMenu);
        }
        if ui
            .add_enabled(
                kind == Some(LibraryKind::Background) && menu.is_some(),
                egui::Button::new(tl!("Set Background")),
            )
            .clicked()
        {
            action = Some(LibraryAction::SetBackground);
        }
        if matches!(pick, LibraryPick::User(_)) {
            ui.separator();
            if ui.button(tl!("Delete")).clicked() {
                action = Some(LibraryAction::Remove);
            }
        }
        if action.is_some() {
            ui.close();
        }
        action
    }

    /// ダブルクリックの操作: メニューは新規メニュー、ボタン / 画像 / シェイプは配置、背景は背景を設定。
    fn library_default_action(&mut self, pick: LibraryPick) {
        let action = match self.pick_kind(Some(pick)) {
            Some(LibraryKind::Menu) => LibraryAction::NewMenu,
            Some(LibraryKind::Button) => LibraryAction::Place,
            Some(LibraryKind::Background) => LibraryAction::SetBackground,
            None => return,
        };
        self.run_library_action(pick, action);
    }

    /// ユーザーの項目の操作: メニューはその仕様から新規メニュー、ボタンは配置、背景はその色。
    fn run_user_action(&mut self, index: usize, action: LibraryAction) {
        let Some(item) = self.user_library.items.get(index).cloned() else {
            return;
        };
        match (item.kind, action) {
            (_, LibraryAction::Remove) => self.remove_user_item(index),
            // M6.403: a file of the library's own.
            (UserItemKind::File(file), LibraryAction::NewMenu) if file.kind == FileKind::Menu => {
                self.library_file_menu(&file, None);
            }
            (UserItemKind::File(file), LibraryAction::Replace) if file.kind == FileKind::Menu => {
                let Some(menu) = self.viewer_menu() else {
                    self.status = tl!("Open a menu first").to_owned();
                    return;
                };
                self.library_file_menu(&file, Some(menu));
            }
            (UserItemKind::File(file), LibraryAction::Place) if file.kind != FileKind::Menu => {
                let Some(menu) = self.viewer_menu() else {
                    self.status = tl!("Open a menu first").to_owned();
                    return;
                };
                self.place_library_file(menu, &file, None);
            }
            (UserItemKind::Menu(spec), LibraryAction::NewMenu) => {
                let created = self.run_command(
                    "library.newMenu",
                    json!({ "spec": spec, "name": item.name }),
                );
                if let Some(id) =
                    created.and_then(|value| value.get("menu").and_then(Value::as_u64))
                {
                    if self.session.project.settings.title_remote == Link::NotSet {
                        self.set_title_remote(&Link::Menu {
                            id: MenuId(id),
                            button: None,
                        });
                    }
                    self.select(Selection::Menu(MenuId(id)));
                }
            }
            (UserItemKind::Menu(spec), LibraryAction::Replace) => {
                self.library_replace_menu(json!({ "spec": spec }));
            }
            (UserItemKind::Text(text), LibraryAction::Place) => {
                if let Some(menu) = self.viewer_menu() {
                    self.place_text_item(menu, &text, None);
                }
            }
            (UserItemKind::Layers(layers), LibraryAction::Place) => {
                if let Some(menu) = self.viewer_menu() {
                    self.place_layers(menu, &layers, None);
                }
            }
            (UserItemKind::Button(spec), LibraryAction::Place) => {
                let Some(menu) = self.viewer_menu() else {
                    self.status = tl!("Open a menu first").to_owned();
                    return;
                };
                let placed =
                    self.run_command("library.place", json!({ "menu": menu.0, "button": spec }));
                if let Some(index) = placed
                    .and_then(|value| value.get("button").and_then(Value::as_u64))
                    .and_then(|index| usize::try_from(index).ok())
                {
                    self.select_only_button(menu, index);
                }
            }
            (UserItemKind::Background(color), LibraryAction::SetBackground) => {
                let Some(menu) = self.viewer_menu() else {
                    self.status = tl!("Open a menu first").to_owned();
                    return;
                };
                let _ = self.run_command(
                    "menu.setBackground",
                    crate::gui_params::background_color(menu.0, color),
                );
            }
            _ => {}
        }
    }

    fn run_library_action(&mut self, pick: LibraryPick, action: LibraryAction) {
        if let LibraryPick::User(index) = pick {
            self.run_user_action(index, action);
            return;
        }
        match (pick, action) {
            (LibraryPick::Preset(id), LibraryAction::NewMenu) => {
                if let Some(item) = library_item(id) {
                    self.library_new_menu(item);
                }
            }
            (LibraryPick::Preset(id), LibraryAction::Replace) => {
                self.library_replace_menu(json!({ "preset": id }));
            }
            (LibraryPick::Preset(id), LibraryAction::Place) => {
                if let Some(item) = library_item(id) {
                    self.library_place(item);
                }
            }
            (LibraryPick::Preset(id), LibraryAction::SetBackground) => {
                if let Some(item) = library_item(id) {
                    self.library_set_background(item);
                }
            }
            (LibraryPick::Image(asset), LibraryAction::Place) => {
                let Some(menu) = self.viewer_menu() else {
                    self.status = tl!("Open a menu first").to_owned();
                    return;
                };
                self.place_image(menu, asset);
            }
            (LibraryPick::Shape(shape), LibraryAction::Place) => {
                let Some(menu) = self.viewer_menu() else {
                    self.status = tl!("Open a menu first").to_owned();
                    return;
                };
                self.place_shape(menu, shape);
            }
            (LibraryPick::Layers(id), LibraryAction::Place) => {
                let Some(menu) = self.viewer_menu() else {
                    self.status = tl!("Open a menu first").to_owned();
                    return;
                };
                if let Some(layers) = self.preset_layers(id) {
                    self.place_layers(menu, &layers, None);
                }
            }
            (
                LibraryPick::Image(_)
                | LibraryPick::Shape(_)
                | LibraryPick::User(_)
                | LibraryPick::Layers(_),
                _,
            )
            | (LibraryPick::Preset(_), LibraryAction::AddItem | LibraryAction::Remove) => {}
        }
    }

    /// A Library item dropped on the menu viewer at `at` (frame pixels): a button is placed centred there, an image
    /// or a shape with its top left corner there, a background becomes the menu's background (Encore: dragging a
    /// Library item into a menu). A menu is not dropped: New Menu and Replace Menu make menus.
    pub(crate) fn drop_library_item(&mut self, menu: MenuId, pick: LibraryPick, at: (u16, u16)) {
        self.library.selected = Some(pick);
        match (pick, self.pick_kind(Some(pick))) {
            (LibraryPick::Image(asset), _) => self.place_image_at(menu, asset, Some(at)),
            (LibraryPick::Shape(shape), _) => self.place_shape_at(menu, shape, Some(at)),
            (LibraryPick::Layers(id), _) => {
                if let Some(layers) = self.preset_layers(id) {
                    self.place_layers(menu, &layers, Some(at));
                }
            }
            (_, Some(LibraryKind::Button)) => {
                let (preset, spec, label) = match pick {
                    LibraryPick::Preset(id) => {
                        let label = library_item(id)
                            .and_then(|item| item.buttons.first())
                            .map(|button| label_text(button.label))
                            .unwrap_or_default();
                        (Some(id), None, label)
                    }
                    LibraryPick::User(index) => match self.user_library.items.get(index) {
                        Some(UserItem {
                            kind: UserItemKind::Button(spec),
                            ..
                        }) => (None, Some(spec.clone()), String::new()),
                        // A text item: its top left corner where it is dropped.
                        Some(UserItem {
                            kind: UserItemKind::Text(text),
                            ..
                        }) => {
                            let text = text.clone();
                            self.place_text_item(menu, &text, Some(at));
                            return;
                        }
                        // Layers: their top left corner where they are dropped.
                        Some(UserItem {
                            kind: UserItemKind::Layers(layers),
                            ..
                        }) => {
                            let layers = layers.clone();
                            self.place_layers(menu, &layers, Some(at));
                            return;
                        }
                        // A file of the library's own (M6.403): its top left corner where it is dropped.
                        Some(UserItem {
                            kind: UserItemKind::File(file),
                            ..
                        }) => {
                            let file = file.clone();
                            self.place_library_file(menu, &file, Some(at));
                            return;
                        }
                        _ => return,
                    },
                    _ => return,
                };
                let mut params = json!({ "menu": menu.0, "x": at.0, "y": at.1 });
                if let Some(map) = params.as_object_mut() {
                    match (preset, spec) {
                        (Some(id), _) => {
                            map.insert("preset".to_owned(), json!(id));
                            map.insert("label".to_owned(), json!(label));
                        }
                        (None, Some(spec)) => {
                            map.insert("button".to_owned(), json!(spec));
                        }
                        (None, None) => return,
                    }
                }
                let placed = self.run_command("library.place", params);
                if let Some(index) = placed
                    .and_then(|value| value.get("button").and_then(Value::as_u64))
                    .and_then(|index| usize::try_from(index).ok())
                {
                    self.select_only_button(menu, index);
                }
            }
            (_, Some(LibraryKind::Background)) => {
                self.run_library_action(pick, LibraryAction::SetBackground);
            }
            (_, Some(LibraryKind::Menu)) => {
                self.status =
                    tl!("A menu from the Library is made with New Menu or Replace Menu").to_owned();
            }
            (_, None) => {}
        }
    }

    /// Places a Library text item in `menu` (its top left corner at `at`, frame pixels; else where it was) and
    /// selects it.
    fn place_text_item(
        &mut self,
        menu: MenuId,
        text: &disccraft_project::MenuText,
        at: Option<(u16, u16)>,
    ) {
        let mut params = json!({ "menu": menu.0, "text": text });
        if let (Some((x, y)), Some(map)) = (at, params.as_object_mut()) {
            map.insert("x".to_owned(), json!(x));
            map.insert("y".to_owned(), json!(y));
        }
        let placed = self.run_command("menu.placeText", params);
        if let Some(index) = placed
            .and_then(|value| value.get("index").and_then(Value::as_u64))
            .and_then(|index| usize::try_from(index).ok())
        {
            self.select_menu_text(menu, index);
        }
    }

    /// メニューのプリセットから新しいメニューを作り、メニュービューアで開く。
    /// 最初のメニューは（メニュー ▸ 新規メニューと同じく）ディスクのタイトルボタンになる。
    fn library_new_menu(&mut self, item: &'static LibraryItem) {
        let labels: Vec<String> = item
            .buttons
            .iter()
            .map(|button| label_text(button.label))
            .collect();
        let created = self.run_command(
            "library.newMenu",
            json!({ "preset": item.id, "name": item_name(item.id), "labels": labels }),
        );
        let Some(id) = created.and_then(|value| value.get("menu").and_then(Value::as_u64)) else {
            return;
        };
        if self.session.project.settings.title_remote == Link::NotSet {
            self.set_title_remote(&Link::Menu {
                id: MenuId(id),
                button: None,
            });
        }
        self.select(Selection::Menu(MenuId(id)));
        self.status = tlf!("Created menu {name}", name = item_name(item.id));
    }

    /// 置き換え（Encore）: 選んだライブラリのメニューを、開いているメニューのデザインにする。リンクは番号で残す。
    /// `source` は `menu.replace` の "preset" か "spec"。
    fn library_replace_menu(&mut self, source: Value) {
        let Some(menu) = self.viewer_menu() else {
            self.status = tl!("Open a menu first").to_owned();
            return;
        };
        let mut params = source;
        if let Some(object) = params.as_object_mut() {
            let _ = object.insert("menu".to_owned(), json!(menu.0));
        }
        if self.run_command("menu.replace", params).is_some() {
            self.status = tl!("Replaced the menu").to_owned();
        }
    }

    /// ボタンのプリセットを開いているメニューに置く。
    fn library_place(&mut self, item: &'static LibraryItem) {
        let Some(menu) = self.viewer_menu() else {
            self.status = tl!("Open a menu first").to_owned();
            return;
        };
        let label = item
            .buttons
            .first()
            .map(|button| label_text(button.label))
            .unwrap_or_default();
        let placed = self.run_command(
            "library.place",
            json!({ "menu": menu.0, "preset": item.id, "label": label }),
        );
        if let Some(index) = placed
            .and_then(|value| value.get("button").and_then(Value::as_u64))
            .and_then(|index| usize::try_from(index).ok())
        {
            self.select_only_button(menu, index);
        }
    }

    /// 背景のプリセットを開いているメニューの背景にする。
    fn library_set_background(&mut self, item: &'static LibraryItem) {
        let Some(menu) = self.viewer_menu() else {
            self.status = tl!("Open a menu first").to_owned();
            return;
        };
        let _ = self.run_command(
            "library.setBackground",
            json!({ "menu": menu.0, "preset": item.id }),
        );
    }

    /// プレビュー: プリセットは絵（下の関数）、画像はサムネール、シェイプは形。
    fn library_preview(&mut self, ui: &mut Ui, pick: Option<LibraryPick>) {
        let preview = preview_height(ui);
        match pick {
            Some(LibraryPick::Image(asset)) => {
                let texture = self.asset_thumbnail(ui.ctx(), asset);
                let (outer, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), preview),
                    egui::Sense::hover(),
                );
                let painter = ui.painter_at(outer);
                painter.rect_filled(outer, 2.0, ui.visuals().extreme_bg_color);
                if let Some(texture) = texture {
                    let size = texture.size_vec2();
                    let scale = (outer.width() / size.x.max(1.0))
                        .min(outer.height() / size.y.max(1.0))
                        .min(1.0);
                    let rect = egui::Rect::from_center_size(outer.center(), size * scale);
                    painter.image(
                        texture.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
            }
            Some(LibraryPick::Shape(shape)) => {
                let (outer, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), preview),
                    egui::Sense::hover(),
                );
                let painter = ui.painter_at(outer);
                painter.rect_filled(outer, 2.0, ui.visuals().extreme_bg_color);
                let rect = egui::Rect::from_center_size(
                    outer.center(),
                    egui::vec2(preview * 1.2, preview * 0.6),
                );
                match shape {
                    ShapeKind::Rectangle => {
                        let _ = painter.rect_filled(rect, 0.0, Color32::WHITE);
                    }
                    ShapeKind::RoundedRectangle => {
                        let _ = painter.rect_filled(rect, rect.height() / 8.0, Color32::WHITE);
                    }
                    ShapeKind::Ellipse => {
                        let points: Vec<egui::Pos2> = (0..64)
                            .map(|i| {
                                let angle = (i as f32) / 64.0 * std::f32::consts::TAU;
                                rect.center()
                                    + egui::vec2(
                                        angle.cos() * rect.width() / 2.0,
                                        angle.sin() * rect.height() / 2.0,
                                    )
                            })
                            .collect();
                        let _ = painter.add(egui::Shape::convex_polygon(
                            points,
                            Color32::WHITE,
                            egui::Stroke::NONE,
                        ));
                    }
                }
            }
            Some(LibraryPick::Preset(id)) => {
                preset_preview(ui, library_item(id).map(PreviewItem::from).as_ref());
            }
            Some(LibraryPick::Layers(id)) => {
                let frame = disccraft_project::frame_size(&self.session.project.settings);
                if let Some(layers) = self.preset_layers(id) {
                    layers_preview(ui, &layers, frame);
                }
            }
            // A file of the library's own: its picture (M6.403).
            Some(LibraryPick::User(index))
                if let Some(UserItem {
                    kind: UserItemKind::File(file),
                    ..
                }) = self.user_library.items.get(index) =>
            {
                let file = file.clone();
                self.library_file_preview(ui, &file, preview);
            }
            Some(LibraryPick::User(index))
                if let Some(UserItem {
                    kind: UserItemKind::Layers(layers),
                    ..
                }) = self.user_library.items.get(index) =>
            {
                let frame = disccraft_project::frame_size(&self.session.project.settings);
                let layers = layers.clone();
                layers_preview(ui, &layers, frame);
            }
            Some(LibraryPick::User(index)) => {
                let item = self
                    .user_library
                    .items
                    .get(index)
                    .map(PreviewItem::from_user);
                preset_preview(ui, item.as_ref());
            }
            None => preset_preview(ui, None),
        }
    }
}

/// What the preview draws: a menu, a button or a background, from a built-in preset or a user item.
struct PreviewItem {
    kind: LibraryKind,
    background: [u8; 3],
    text: [u8; 3],
    highlight: [u8; 3],
    /// Buttons as (label, x, y, w, h) in a 720x480 frame.
    buttons: Vec<(String, u16, u16, u16, u16)>,
    /// Each button's video thumbnail.
    thumbnails: Vec<Option<disccraft_project::ThumbnailArea>>,
}

impl From<&'static LibraryItem> for PreviewItem {
    fn from(item: &'static LibraryItem) -> Self {
        PreviewItem {
            kind: item.kind,
            background: item.background,
            text: item.text,
            highlight: item.highlight,
            buttons: item
                .buttons
                .iter()
                .map(|b| (label_text(b.label), b.x, b.y, b.w, b.h))
                .collect(),
            thumbnails: item.buttons.iter().map(|b| b.thumbnail).collect(),
        }
    }
}

impl PreviewItem {
    fn from_user(item: &UserItem) -> Self {
        match &item.kind {
            UserItemKind::Menu(spec) => PreviewItem {
                kind: LibraryKind::Menu,
                background: spec.background,
                text: spec.text,
                highlight: spec.highlight,
                buttons: spec
                    .buttons
                    .iter()
                    .map(|b| (b.label.clone(), b.x, b.y, b.w, b.h))
                    .collect(),
                thumbnails: spec.buttons.iter().map(|b| b.thumbnail).collect(),
            },
            UserItemKind::Button(spec) => PreviewItem {
                kind: LibraryKind::Button,
                background: [60, 60, 60],
                text: [255, 255, 255],
                highlight: [255, 208, 0],
                buttons: vec![(spec.label.clone(), spec.x, spec.y, spec.w, spec.h)],
                thumbnails: vec![spec.thumbnail],
            },
            UserItemKind::Background(color) => PreviewItem {
                kind: LibraryKind::Background,
                background: *color,
                text: [255, 255, 255],
                highlight: [255, 208, 0],
                buttons: Vec::new(),
                thumbnails: Vec::new(),
            },
            // Layers are drawn by [`layers_preview`], files by `library_file_preview`; here an empty frame.
            UserItemKind::Layers(_) | UserItemKind::File(_) => PreviewItem {
                kind: LibraryKind::Button,
                background: [60, 60, 60],
                text: [255, 255, 255],
                highlight: [255, 208, 0],
                buttons: Vec::new(),
                thumbnails: Vec::new(),
            },
            // A text item: its first line in its colour, as a placed item is shown.
            UserItemKind::Text(text) => PreviewItem {
                kind: LibraryKind::Button,
                background: [60, 60, 60],
                text: text.color.unwrap_or([255, 255, 255]),
                highlight: [255, 208, 0],
                buttons: vec![(
                    text.text.lines().next().unwrap_or_default().to_owned(),
                    text.x,
                    text.y,
                    text.box_width.unwrap_or(320),
                    text.size.saturating_mul(2),
                )],
                thumbnails: vec![None],
            },
        }
    }
}

/// What the Library's set menu was asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SetAction {
    New,
    Rename(usize),
    Delete(usize),
}

impl DiscCraftApp {
    /// The Library's own items of its panel menu (and of its tab's right-click menu), in Encore's order: Add Item…
    /// (files; M6.403) with DiscCraft's Import Folder…, then New Set… / Delete Set / Rename Set…, then DiscCraft's
    /// Add the Menu's Layers as a Layer Set (M6.331).
    pub(crate) fn library_menu(&mut self, ui: &mut Ui) {
        let importing = self.library_import.is_some();
        let own = match self.library.set {
            LibrarySet::Own(index) => Some(index),
            _ => None,
        };
        let mut set_action = None;
        // Behind the check mark column, as the panel menu's other items (Encore lines them all up).
        let label = |text: &str| egui::Button::new(crate::actions::indented(text.to_owned()));
        if ui
            .add_enabled(!importing, label(tl!("Add Items from Files…")))
            .on_hover_text(tl!(
                "Copy files into the set shown: each is a menu, a button, a picture, a layer set or a replacement layer by its contents"
            ))
            .clicked()
        {
            ui.close();
            self.library_add_files();
        }
        if ui
            .add_enabled(!importing, label(tl!("Import Folder…")))
            .on_hover_text(tl!(
                "Make a set of the folder chosen and of each folder under it that has PSDs or pictures"
            ))
            .clicked()
        {
            ui.close();
            self.library_import_folder();
        }
        ui.separator();
        if ui.add(label(tl!("New Set…"))).clicked() {
            set_action = Some(SetAction::New);
            ui.close();
        }
        if ui
            .add_enabled(own.is_some(), label(tl!("Delete Set")))
            .on_hover_text(tl!("Its items move to the User set"))
            .clicked()
        {
            set_action = own.map(SetAction::Delete);
            ui.close();
        }
        if ui
            .add_enabled(own.is_some(), label(tl!("Rename Set…")))
            .clicked()
        {
            set_action = own.map(SetAction::Rename);
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(
                self.viewer_menu().is_some(),
                label(tl!("Add the Menu's Layers as a Layer Set")),
            )
            .clicked()
        {
            ui.close();
            self.add_menu_layers_item();
        }
        if let Some(action) = set_action {
            self.library_set_action(action);
        }
    }

    /// New Set… and Rename Set… open the name dialog; Delete Set moves the set's items to the User set.
    fn library_set_action(&mut self, action: SetAction) {
        match action {
            SetAction::New => {
                self.library_set_dialog = Some(SetNameDraft {
                    renaming: None,
                    name: String::new(),
                });
            }
            SetAction::Rename(index) => {
                self.library_set_dialog = Some(SetNameDraft {
                    renaming: Some(index),
                    name: self
                        .user_library
                        .sets
                        .get(index)
                        .cloned()
                        .unwrap_or_default(),
                });
            }
            SetAction::Delete(index) => {
                let moved = self.user_library.remove_set(index);
                self.library.set = LibrarySet::User;
                self.library.selected = None;
                match self.user_library.save() {
                    Ok(()) => {
                        self.status =
                            tlf!("Set deleted; {n} items moved to the User set", n = moved);
                    }
                    Err(error) => {
                        self.status = tlf!("Could not save the library: {error}", error = error);
                    }
                }
            }
        }
    }

    /// The New Set / Rename Set dialog while it is open (Encore's Library ▸ New Set…): a name, OK and Cancel.
    pub(crate) fn library_set_window(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.library_set_dialog.clone() else {
            return;
        };
        let mut done = None;
        let response = egui::Modal::new(egui::Id::unique("library_set_dialog")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.heading(if draft.renaming.is_some() {
                tl!("Rename Set")
            } else {
                tl!("New Set")
            });
            ui.horizontal(|ui| {
                ui.label(tl!("Name:"));
                let field = ui.add(
                    egui::TextEdit::singleline(&mut draft.name)
                        .char_limit(crate::user_library::MAX_SET_NAME)
                        .desired_width(220.0),
                );
                field.request_focus();
                if field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button(tl!("OK")).clicked() {
                    done = Some(true);
                }
                if ui.button(tl!("Cancel")).clicked() {
                    done = Some(false);
                }
            });
        });
        if response.should_close() {
            done = Some(false);
        }
        match done {
            Some(true) => {
                let result = match draft.renaming {
                    Some(index) => self
                        .user_library
                        .rename_set(index, &draft.name)
                        .map(|()| index),
                    None => self.user_library.add_set(&draft.name),
                };
                match result.and_then(|index| self.user_library.save().map(|()| index)) {
                    Ok(index) => {
                        self.library_set_dialog = None;
                        self.library.set = LibrarySet::Own(index);
                        self.library.selected = None;
                    }
                    // The dialog stays open with the reason.
                    Err(error) => {
                        self.status = error;
                        self.library_set_dialog = Some(draft);
                    }
                }
            }
            Some(false) => self.library_set_dialog = None,
            None => self.library_set_dialog = Some(draft),
        }
    }
}

/// ライブラリの操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LibraryAction {
    Place,
    /// 置き換え（開いているメニューを選んだメニューのデザインに）。
    Replace,
    NewMenu,
    SetBackground,
    /// 項目を追加（開いているメニューをユーザーのセットへ）。
    AddItem,
    /// ユーザーの項目を外す。
    Remove,
}

/// プリセットのプレビュー: メニューは背景とボタン（最初のボタンはハイライトの色の枠）、ボタンは画面の中央に 1 つ、
/// 背景は色。
fn preset_preview(ui: &mut Ui, item: Option<&PreviewItem>) {
    let preview = preview_height(ui);
    let width = ui.available_width().min(preview * 1.5);
    let (outer, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), preview),
        egui::Sense::hover(),
    );
    let frame =
        egui::Rect::from_center_size(outer.center(), egui::vec2(width, width * 480.0 / 720.0));
    let painter = ui.painter_at(outer);
    let Some(item) = item else {
        painter.rect_filled(frame, 2.0, ui.visuals().extreme_bg_color);
        return;
    };
    let scale = frame.width() / 720.0;
    let color = |[r, g, b]: [u8; 3]| Color32::from_rgb(r, g, b);
    let fill = match item.kind {
        LibraryKind::Menu | LibraryKind::Background => color(item.background),
        LibraryKind::Button => Color32::from_gray(24),
    };
    painter.rect_filled(frame, 2.0, fill);
    for (index, (label, x, y, w, h)) in item.buttons.iter().enumerate() {
        let size = egui::vec2(f32::from(*w), f32::from(*h)) * scale;
        let rect = match item.kind {
            LibraryKind::Button => egui::Rect::from_center_size(frame.center(), size),
            _ => egui::Rect::from_min_size(
                frame.min + egui::vec2(f32::from(*x), f32::from(*y)) * scale,
                size,
            ),
        };
        if item.kind == LibraryKind::Button {
            painter.rect_filled(rect, 2.0, color(item.background));
        }
        let stroke_color = if index == 0 {
            color(item.highlight)
        } else {
            color(item.text).gamma_multiply(0.4)
        };
        painter.rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(1.0, stroke_color),
            egui::StrokeKind::Inside,
        );
        // ビデオサムネールの場所（灰色）と、その下の文字。
        let mut text_center = rect.center();
        if let Some(area) = item.thumbnails.get(index).copied().flatten() {
            let part =
                |value: u16| f32::from(value) / f32::from(disccraft_project::THUMBNAIL_PARTS);
            let thumb = egui::Rect::from_min_size(
                rect.min + egui::vec2(rect.width() * part(area.x), rect.height() * part(area.y)),
                egui::vec2(rect.width() * part(area.w), rect.height() * part(area.h)),
            )
            .shrink(1.0);
            painter.rect_filled(thumb, 0.0, Color32::from_gray(110));
            if rect.bottom() - thumb.bottom() > 4.0 {
                text_center = egui::pos2(rect.center().x, (thumb.bottom() + rect.bottom()) / 2.0);
            }
        }
        painter.text(
            text_center,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional((rect.height() * 0.5).clamp(6.0, 12.0)),
            color(item.text),
        );
    }
}

/// The preview of Library layers (M6.331): each shape in its colour and opacity, each image a grey box, a
/// replacement layer with a cross, in a frame of `frame` pixels.
fn layers_preview(ui: &mut Ui, item: &LayerItem, frame: (u16, u16)) {
    let (frame_w, frame_h) = (f32::from(frame.0.max(1)), f32::from(frame.1.max(1)));
    let preview = preview_height(ui);
    let width = ui.available_width().min(preview * 1.5);
    let (outer, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), preview),
        egui::Sense::hover(),
    );
    let height = (width * frame_h / frame_w).min(preview);
    let width = height * frame_w / frame_h;
    let area = egui::Rect::from_center_size(outer.center(), egui::vec2(width, height));
    let painter = ui.painter_at(outer);
    painter.rect_filled(area, 2.0, Color32::from_gray(24));
    let scale = width / frame_w;
    for graphic in &item.graphics {
        let rect = egui::Rect::from_min_size(
            area.min + egui::vec2(f32::from(graphic.rect.x), f32::from(graphic.rect.y)) * scale,
            egui::vec2(f32::from(graphic.rect.w), f32::from(graphic.rect.h)) * scale,
        );
        let alpha = f32::from(graphic.opacity.min(100)) / 100.0;
        match graphic.kind {
            GraphicKind::Shape { shape, color } => {
                let fill = Color32::from_rgb(color[0], color[1], color[2]).gamma_multiply(alpha);
                match shape {
                    ShapeKind::Rectangle => {
                        let _ = painter.rect_filled(rect, 0.0, fill);
                    }
                    ShapeKind::RoundedRectangle => {
                        let _ =
                            painter.rect_filled(rect, rect.height().min(rect.width()) / 8.0, fill);
                    }
                    ShapeKind::Ellipse => {
                        let _ = painter.add(egui::Shape::ellipse_filled(
                            rect.center(),
                            rect.size() / 2.0,
                            fill,
                        ));
                    }
                }
            }
            GraphicKind::Image { .. } => {
                let _ =
                    painter.rect_filled(rect, 0.0, Color32::from_gray(110).gamma_multiply(alpha));
            }
        }
        if graphic.replacement {
            let stroke = egui::Stroke::new(1.0, Color32::from_gray(220));
            let _ = painter.line_segment([rect.left_top(), rect.right_bottom()], stroke);
            let _ = painter.line_segment([rect.right_top(), rect.left_bottom()], stroke);
        }
    }
}

/// The display name of a built-in layer set or replacement layer (in the display language).
pub(crate) fn layer_preset_name(id: &str) -> &'static str {
    match id {
        "layers.band-top" => tl!("Title Band"),
        "layers.band-bottom" => tl!("Bottom Band"),
        "layers.frame" => tl!("Thin Frame"),
        "layers.panel" => tl!("Side Panel"),
        "replace.photo-4x3" => tl!("Photo 4:3"),
        "replace.photo-16x9" => tl!("Photo 16:9"),
        "replace.photo-round" => tl!("Round Photo"),
        _ => layer_preset(id).map_or("", |preset| preset.name),
    }
}

/// 種類の表示名。
fn kind_label(kind: LibraryKind) -> &'static str {
    match kind {
        LibraryKind::Menu => tl!("Menu"),
        LibraryKind::Button => tl!("Button"),
        LibraryKind::Background => tl!("Background"),
    }
}

/// 項目の表示名（表示言語に合わせる）。
pub(crate) fn item_name(id: &str) -> &'static str {
    match id {
        "menu.plain-dark" => tl!("Plain Dark"),
        "menu.plain-light" => tl!("Plain Light"),
        "menu.bottom-bar" => tl!("Bottom Bar"),
        "menu.chapter-grid" => tl!("Chapter Grid"),
        "menu.thumbnail-grid" => tl!("Thumbnail Grid"),
        "menu.chapter-pages" => tl!("Chapter Pages"),
        "menu.scene-pages" => tl!("Scene Pages"),
        "menu.left-list" => tl!("Left List"),
        "button.wide" => tl!("Wide Button"),
        "button.small" => tl!("Small Button"),
        "button.tile" => tl!("Tile"),
        "button.thumbnail" => tl!("Video Thumbnail"),
        "button.bar" => tl!("Full-Width Bar"),
        "background.black" => tl!("Black"),
        "background.charcoal" => tl!("Charcoal"),
        "background.navy" => tl!("Navy"),
        "background.forest" => tl!("Forest"),
        "background.wine" => tl!("Wine"),
        "background.ivory" => tl!("Ivory"),
        _ => library_item(id).map_or("", |item| item.name),
    }
}

impl DiscCraftApp {
    /// Whether the menu in the viewer has chapter buttons (a template for Encore's チャプターインデックスを作成).
    pub(crate) fn viewer_menu_has_chapter_buttons(&self) -> bool {
        self.viewer_menu()
            .and_then(|id| self.session.project.menu(id))
            .is_some_and(|menu| {
                menu.buttons
                    .iter()
                    .any(|button| button.kind == disccraft_project::ButtonKind::Chapter)
            })
    }

    /// メニュー ▸ チャプターインデックスを作成（`multi_page` は 複数ページの…）。ビューアのメニューにチャプターボタンが
    /// あれば Encore と同じく、リンク済みのチャプターボタンに続けて残りのチャプターボタンにリンクを入れる。なければ
    /// 選択中のタイムラインのチャプターごとにボタンを並べたメニューを作り、最初のページを開く（DiscCraft 独自）。
    pub(crate) fn create_chapter_index(&mut self, multi_page: bool) {
        if self.viewer_menu_has_chapter_buttons()
            && let Some(menu) = self.viewer_menu()
        {
            // Encore: a chapter button has to link to a timeline first.
            let linked = self.session.project.menu(menu).is_some_and(|found| {
                found.buttons.iter().any(|button| {
                    button.kind == disccraft_project::ButtonKind::Chapter
                        && matches!(button.link, disccraft_project::Link::Timeline { .. })
                })
            });
            if linked && multi_page {
                // M6.255: the menu and copies of it, until every chapter has a button.
                let pages = self
                    .run_command(
                        "menu.fillChapterIndexPages",
                        serde_json::json!({ "menu": menu.0 }),
                    )
                    .and_then(|made| made.get("menus")?.as_array().map(Vec::len));
                if let Some(pages) = pages {
                    self.status = tlf!("Chapter index: {n} pages", n = pages);
                }
            } else if linked {
                let _ = self.run_command(
                    "menu.fillChapterIndex",
                    serde_json::json!({ "menu": menu.0 }),
                );
            } else {
                self.status = tl!(
                    "Could not create the chapter index. Link at least one chapter button of the current menu to a timeline."
                )
                .to_string();
            }
            return;
        }
        let Some(id) = self.selected_timeline() else {
            return;
        };
        let Some(timeline) = self.session.project.timeline(id) else {
            return;
        };
        let labels: Vec<String> = timeline
            .chapters
            .iter()
            .enumerate()
            .map(|(index, chapter)| crate::timeline_view::chapter_label(&chapter.name, index))
            .collect();
        let name = tlf!("{name} Chapters", name = timeline.name);
        let per_page = multi_page.then_some(disccraft_project::CHAPTERS_PER_PAGE);
        let created = self.run_command(
            "library.chapterIndex",
            serde_json::json!({
                "timeline": id.0,
                "name": name,
                "perPage": per_page,
                "labels": labels,
                "previous": tl!("Previous"),
                "next": tl!("Next"),
            }),
        );
        let first = created
            .as_ref()
            .and_then(|value| value.get("menus"))
            .and_then(serde_json::Value::as_array)
            .and_then(|menus| menus.first())
            .and_then(serde_json::Value::as_u64);
        if let Some(menu) = first {
            self.select(Selection::Menu(MenuId(menu)));
        }
    }
}

/// プリセットのボタンの文字（表示言語に合わせる）。
fn label_text(label: &str) -> String {
    let text = match label {
        "Play Movie" => tl!("Play Movie"),
        "Play" => tl!("Play"),
        "Chapters" => tl!("Chapters"),
        "Chapter" => tl!("Chapter"),
        "Extras" => tl!("Extras"),
        "Setup" => tl!("Setup"),
        "Main Menu" => tl!("Main Menu"),
        "Previous" => tl!("Previous"),
        "Next" => tl!("Next"),
        "Scene Selection" => tl!("Scene Selection"),
        "Audio" => tl!("Audio"),
        "Subtitles" => tl!("Subtitles"),
        "Button" => tl!("Button"),
        other => {
            return match other.strip_prefix("Chapter ") {
                Some(n) => tlf!("Chapter {n}", n = n),
                None => other.to_owned(),
            };
        }
    };
    text.to_owned()
}

#[cfg(test)]
mod tests {
    use super::{GROUPS, item_name, kinds_after_click, label_text};
    use disccraft_project::LIBRARY;

    /// Encore's kind toggles (M6.485): a click shows that kind alone, a Ctrl+click shows or hides it beside the
    /// others; a click on the only kind shown shows them all again.
    #[test]
    fn a_kind_click_shows_that_kind_alone_and_ctrl_adds_it() {
        let all = [true; GROUPS];
        let menus_only = [true, false, false, false, false, false, false, false];
        assert_eq!(kinds_after_click(all, 0, false), menus_only);
        // Again on the only one: every kind.
        assert_eq!(kinds_after_click(menus_only, 0, false), all);
        // Another kind: that one alone.
        assert_eq!(
            kinds_after_click(menus_only, 2, false),
            [false, false, true, false, false, false, false, false]
        );
        // Ctrl+click adds and removes, the others kept.
        let menus_and_images = [true, false, true, false, false, false, false, false];
        assert_eq!(kinds_after_click(menus_only, 2, true), menus_and_images);
        assert_eq!(
            kinds_after_click(menus_and_images, 0, true),
            [false, false, true, false, false, false, false, false]
        );
        let mut all_but_shapes = all;
        all_but_shapes[6] = false;
        assert_eq!(kinds_after_click(all, 6, true), all_but_shapes);
        // A kind out of range changes nothing with Ctrl, and hides every kind without it.
        assert_eq!(kinds_after_click(all, GROUPS, true), all);
        assert_eq!(kinds_after_click(all, GROUPS, false), [false; GROUPS]);
    }

    #[test]
    fn every_library_item_has_a_display_name() {
        for item in LIBRARY {
            assert!(!item_name(item.id).is_empty(), "{}", item.id);
        }
        // M6.331: and every layer set and replacement layer, each its own.
        for preset in disccraft_project::LAYER_PRESETS {
            assert!(
                !super::layer_preset_name(preset.id).is_empty()
                    && super::layer_preset_name(preset.id) != preset.id,
                "{}",
                preset.id
            );
        }
    }

    #[test]
    fn numbered_chapter_labels_keep_their_number() {
        assert!(label_text("Chapter 3").contains('3'));
        assert_eq!(label_text("Something else"), "Something else");
    }
}
