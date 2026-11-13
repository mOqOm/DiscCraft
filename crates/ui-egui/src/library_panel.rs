//! ライブラリパネル（Encore のライブラリに当たる）。
//!
//! DiscCraft 独自のプリセット（メニュー、ボタン、背景。どれもコードで作った色と四角だけ）と、メニューに置ける
//! 画像（プロジェクトの画像アセット）とシェイプ（長方形 / 角丸長方形 / 楕円）を一覧にする。
//! 上に「セット」とプレビュー、種類ごとの表示の切り替え、一覧、下に「配置」「新規メニュー」「背景を設定」。
//! 一覧の項目をダブルクリックすると、その種類のいちばんよく使う操作をする。

use crate::DiscCraftApp;
use crate::app::Selection;
use crate::i18n::{tl, tlf};
use crate::user_library::{UserItem, UserItemKind};
use disccraft_project::{
    AssetId, AssetKind, LIBRARY, LibraryItem, LibraryKind, Link, MenuId, ShapeKind, library_item,
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
}

/// ライブラリのセット（Encore の「セット :」）: 一般（組み込み）と ユーザー（項目を追加 で足したもの）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LibrarySet {
    #[default]
    General,
    User,
}

/// 種類の表示の切り替え（Encore の並び: メニュー / ボタン / 画像 / 背景 / シェイプ）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LibraryGroup {
    Menus,
    Buttons,
    Images,
    Backgrounds,
    Shapes,
}

impl LibraryGroup {
    const ALL: [LibraryGroup; 5] = [
        LibraryGroup::Menus,
        LibraryGroup::Buttons,
        LibraryGroup::Images,
        LibraryGroup::Backgrounds,
        LibraryGroup::Shapes,
    ];

    fn index(self) -> usize {
        match self {
            LibraryGroup::Menus => 0,
            LibraryGroup::Buttons => 1,
            LibraryGroup::Images => 2,
            LibraryGroup::Backgrounds => 3,
            LibraryGroup::Shapes => 4,
        }
    }

    fn of_kind(kind: LibraryKind) -> Self {
        match kind {
            LibraryKind::Menu => LibraryGroup::Menus,
            LibraryKind::Button => LibraryGroup::Buttons,
            LibraryKind::Background => LibraryGroup::Backgrounds,
        }
    }

    fn label(self) -> &'static str {
        match self {
            LibraryGroup::Menus => tl!("Menus"),
            LibraryGroup::Buttons => tl!("Buttons"),
            LibraryGroup::Images => tl!("Images"),
            LibraryGroup::Backgrounds => tl!("Backgrounds"),
            LibraryGroup::Shapes => tl!("Shapes"),
        }
    }

    fn hint(self) -> &'static str {
        match self {
            LibraryGroup::Menus => tl!("Toggle menu display"),
            LibraryGroup::Buttons => tl!("Toggle button display"),
            LibraryGroup::Images => tl!("Toggle image display"),
            LibraryGroup::Backgrounds => tl!("Toggle background display"),
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
    pub(crate) show: [bool; 5],
    /// 表示しているセット。
    pub(crate) set: LibrarySet,
}

impl Default for LibraryState {
    fn default() -> Self {
        Self {
            selected: None,
            show: [true; 5],
            set: LibrarySet::General,
        }
    }
}

/// プレビューの高さ。
const PREVIEW_HEIGHT: f32 = 120.0;

/// 一覧の 1 行。
#[derive(Clone, Debug)]
struct LibraryRow {
    pick: LibraryPick,
    name: String,
    kind: &'static str,
}

impl DiscCraftApp {
    /// ライブラリのタブ。
    pub(crate) fn library_tab(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(tl!("Set:"));
            let sets = [
                (LibrarySet::General, tl!("General")),
                (LibrarySet::User, tl!("User")),
            ];
            let current = sets
                .iter()
                .find(|(set, _)| *set == self.library.set)
                .map_or("", |(_, name)| *name);
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

        ui.horizontal(|ui| {
            for group in LibraryGroup::ALL {
                if let Some(show) = self.library.show.get_mut(group.index()) {
                    let _ = ui
                        .toggle_value(show, group.label())
                        .on_hover_text(group.hint());
                }
            }
        });
        ui.separator();

        self.library_list(ui);
        ui.separator();
        self.library_tools(ui, selected);
    }

    fn library_shows(&self, group: LibraryGroup) -> bool {
        self.library
            .show
            .get(group.index())
            .copied()
            .unwrap_or(true)
    }

    /// 一覧の行: 一般のセットはプリセット、画像アセット、シェイプ。ユーザーのセットは足した項目（表示している種類だけ）。
    fn library_rows(&self) -> Vec<LibraryRow> {
        let mut rows = Vec::new();
        if self.library.set == LibrarySet::User {
            for (index, item) in self.user_library.items.iter().enumerate() {
                let (group, kind) = match item.kind {
                    UserItemKind::Menu(_) => (LibraryGroup::Menus, kind_label(LibraryKind::Menu)),
                    UserItemKind::Button(_) => {
                        (LibraryGroup::Buttons, kind_label(LibraryKind::Button))
                    }
                    UserItemKind::Background(_) => (
                        LibraryGroup::Backgrounds,
                        kind_label(LibraryKind::Background),
                    ),
                };
                if self.library_shows(group) {
                    rows.push(LibraryRow {
                        pick: LibraryPick::User(index),
                        name: item.name.clone(),
                        kind,
                    });
                }
            }
            return rows;
        }
        for item in LIBRARY {
            if self.library_shows(LibraryGroup::of_kind(item.kind)) {
                rows.push(LibraryRow {
                    pick: LibraryPick::Preset(item.id),
                    name: item_name(item.id).to_owned(),
                    kind: kind_label(item.kind),
                });
            }
        }
        if self.library_shows(LibraryGroup::Images) {
            for asset in self.session.project.assets.values() {
                if asset.kind == AssetKind::Image {
                    rows.push(LibraryRow {
                        pick: LibraryPick::Image(asset.id),
                        name: asset.name.clone(),
                        kind: tl!("Image"),
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
                });
            }
        }
        rows
    }

    /// 項目の一覧（名前と種類）。
    fn library_list(&mut self, ui: &mut Ui) {
        let rows = self.library_rows();
        let selected = rows
            .iter()
            .position(|row| self.library.selected == Some(row.pick));
        let mut clicked = None;
        let mut double_clicked = None;
        let mut menu_action = None;
        crate::theme::list("library_grid", selected)
            .num_columns(2)
            .show(ui, |ui| {
                for row in &rows {
                    let response =
                        ui.selectable_label(self.library.selected == Some(row.pick), &row.name);
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
                    ui.label(row.kind);
                    ui.end_row();
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
            LibraryPick::Image(_) | LibraryPick::Shape(_) => Some(LibraryKind::Button),
            LibraryPick::User(index) => {
                self.user_library
                    .items
                    .get(index)
                    .map(|item| match item.kind {
                        UserItemKind::Menu(_) => LibraryKind::Menu,
                        UserItemKind::Button(_) => LibraryKind::Button,
                        UserItemKind::Background(_) => LibraryKind::Background,
                    })
            }
        }
    }

    /// 下のボタン（Encore と同じ並び: 配置 / 置き換え / 背景を設定 / 新規メニュー / 項目を追加 / 削除）。
    fn library_tools(&mut self, ui: &mut Ui, selected: Option<LibraryPick>) {
        let menu = self.viewer_menu();
        let kind = self.pick_kind(selected);
        let placeable = kind == Some(LibraryKind::Button);
        let is_menu = kind == Some(LibraryKind::Menu);
        let is_background = kind == Some(LibraryKind::Background);
        let is_user = matches!(selected, Some(LibraryPick::User(_)));
        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            let place = ui
                .add_enabled(placeable && menu.is_some(), egui::Button::new(tl!("Place")))
                .on_hover_text(tl!("Place the selected item in the open menu"));
            let replace = ui
                .add_enabled(is_menu && menu.is_some(), egui::Button::new(tl!("Replace")))
                .on_hover_text(tl!(
                    "Replace the open menu with the selected menu, keeping its links"
                ));
            let background = ui
                .add_enabled(
                    is_background && menu.is_some(),
                    egui::Button::new(tl!("Set Background")),
                )
                .on_hover_text(tl!("Use the selected background in the open menu"));
            let new_menu = ui
                .add_enabled(is_menu, egui::Button::new(tl!("New Menu")))
                .on_hover_text(tl!("Create a new menu from the selected menu"));
            if place.clicked() {
                action = Some(LibraryAction::Place);
            }
            if replace.clicked() {
                action = Some(LibraryAction::Replace);
            }
            if background.clicked() {
                action = Some(LibraryAction::SetBackground);
            }
            if new_menu.clicked() {
                action = Some(LibraryAction::NewMenu);
            }
            let add = ui
                .add_enabled(menu.is_some(), egui::Button::new(tl!("Add Item")))
                .on_hover_text(tl!(
                    "Save the open menu (or its selected button) to the User set of the library"
                ));
            let delete = ui
                .add_enabled(is_user, egui::Button::new(tl!("Delete")))
                .on_hover_text(tl!("Remove the selected item from the library"));
            if add.clicked() {
                action = Some(LibraryAction::AddItem);
            }
            if delete.clicked() {
                action = Some(LibraryAction::Remove);
            }
        });
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

    /// 項目を追加: 開いているメニュー（ボタンを選んでいればそのボタン）をユーザーのセットに保存する。
    fn add_library_item(&mut self) {
        let Some(menu) = self.viewer_menu() else {
            self.status = tl!("Open a menu first").to_owned();
            return;
        };
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
                }
            }
        };
        self.store_user_item(item);
    }

    /// Adds `item` to the User set, saves the library and shows the item.
    pub(crate) fn store_user_item(&mut self, item: UserItem) {
        let name = item.name.clone();
        match self.user_library.add(item) {
            Ok(index) => {
                if let Err(error) = self.user_library.save() {
                    self.status = tlf!("Could not save the library: {error}", error = error);
                    return;
                }
                self.library.set = LibrarySet::User;
                self.library.selected = Some(LibraryPick::User(index));
                self.status = tlf!("Added {name} to the library", name = name);
            }
            Err(error) => self.status = error,
        }
    }

    /// 削除: ユーザーの項目を外す。
    fn remove_user_item(&mut self, index: usize) {
        if index >= self.user_library.items.len() {
            return;
        }
        let _ = self.user_library.items.remove(index);
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
            (LibraryPick::Image(_) | LibraryPick::Shape(_) | LibraryPick::User(_), _)
            | (LibraryPick::Preset(_), LibraryAction::AddItem | LibraryAction::Remove) => {}
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
        match pick {
            Some(LibraryPick::Image(asset)) => {
                let texture = self.asset_thumbnail(ui.ctx(), asset);
                let (outer, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), PREVIEW_HEIGHT),
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
                    egui::vec2(ui.available_width(), PREVIEW_HEIGHT),
                    egui::Sense::hover(),
                );
                let painter = ui.painter_at(outer);
                painter.rect_filled(outer, 2.0, ui.visuals().extreme_bg_color);
                let rect = egui::Rect::from_center_size(
                    outer.center(),
                    egui::vec2(PREVIEW_HEIGHT * 1.2, PREVIEW_HEIGHT * 0.6),
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
    let width = ui.available_width().min(PREVIEW_HEIGHT * 1.5);
    let (outer, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), PREVIEW_HEIGHT),
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
        if !multi_page
            && self.viewer_menu_has_chapter_buttons()
            && let Some(menu) = self.viewer_menu()
        {
            // Encore: a chapter button has to link to a timeline first.
            let linked = self.session.project.menu(menu).is_some_and(|found| {
                found.buttons.iter().any(|button| {
                    button.kind == disccraft_project::ButtonKind::Chapter
                        && matches!(button.link, disccraft_project::Link::Timeline { .. })
                })
            });
            if linked {
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
    use super::{item_name, label_text};
    use disccraft_project::LIBRARY;

    #[test]
    fn every_library_item_has_a_display_name() {
        for item in LIBRARY {
            assert!(!item_name(item.id).is_empty(), "{}", item.id);
        }
    }

    #[test]
    fn numbered_chapter_labels_keep_their_number() {
        assert!(label_text("Chapter 3").contains('3'));
        assert_eq!(label_text("Something else"), "Something else");
    }
}
