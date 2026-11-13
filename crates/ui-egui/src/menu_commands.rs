//! The rest of Encore's Menu menu for the menu being edited: Set Menu User Operations…, Save Menu as File…,
//! Save Menu as Template… and Set End Action….

use crate::app::DiscCraftApp;
use crate::flowchart::{NodeId, Port};
use crate::i18n::{tl, tlf};
use crate::user_library::{UserItem, UserItemKind};
use crate::user_ops::{UserOpsDialog, UserOpsTarget};
use disccraft_project::{Menu, MenuId};
use rfd::FileDialog;

impl DiscCraftApp {
    /// The menu being edited, or a status message when there is none.
    fn edited_menu(&mut self) -> Option<Menu> {
        let menu = self
            .viewer_menu()
            .and_then(|id| self.session.project.menu(id))
            .cloned();
        if menu.is_none() {
            self.status = tl!("Open a menu first").to_string();
        }
        menu
    }

    /// Menu ▸ Set Menu User Operations…: the User Operations dialog for the menu.
    pub(crate) fn open_menu_user_ops(&mut self) {
        let Some(menu) = self.edited_menu() else {
            return;
        };
        self.user_ops_dialog = Some(UserOpsDialog {
            target: UserOpsTarget::Menu(menu.id),
            prohibited: menu.prohibited_ops.clone(),
        });
    }

    /// Menu ▸ Set End Action…: the Specify Link dialog for the menu's end action.
    pub(crate) fn open_menu_end_action(&mut self) {
        let Some(menu) = self.edited_menu() else {
            return;
        };
        self.open_link_dialog_for(Port::EndAction(NodeId::Menu(menu.id)), &menu.end_action);
    }

    /// Menu ▸ Save Menu as File…: the menu as a layered PSD where the user chooses.
    pub(crate) fn save_menu_as_file(&mut self) {
        let Some(menu) = self.edited_menu() else {
            return;
        };
        let Some(path) = FileDialog::new()
            .add_filter("Photoshop (PSD)", &["psd"])
            .set_file_name(format!(
                "{}.psd",
                crate::photocraft::file_name_for(&menu.name)
            ))
            .save_file()
        else {
            return;
        };
        self.status = match self.export_menu_psd_to(&menu, &path) {
            Ok(()) => tlf!(
                "Saved the menu as {path}",
                path = path.display().to_string()
            ),
            Err(error) => error,
        };
    }

    /// File ▸ Import ▸ Menu… / Pop-up Menu… / Menu Page… (Encore's 読み込み ▸ メニュー… など): a layered PSD becomes
    /// a new menu named after the file (its `(+)` groups the buttons), a pop-up menu, or a new page of the open
    /// menu. The new menu opens in the viewer; like a new menu, the first one is the Title button and First Play.
    pub(crate) fn import_menu_file(&mut self, kind: MenuImport) {
        let title = match kind {
            MenuImport::Menu => tl!("Import as Menu"),
            MenuImport::Popup => tl!("Import as Pop-up Menu"),
            MenuImport::Page(_) => tl!("Import as Menu Page"),
        };
        let Some(path) = FileDialog::new()
            .set_title(title)
            .add_filter(tl!("Menu files"), &["psd"])
            .pick_file()
        else {
            return;
        };
        let path = path.to_string_lossy();
        let params = match kind {
            MenuImport::Menu => serde_json::json!({ "path": path }),
            MenuImport::Popup => serde_json::json!({ "path": path, "popup": true }),
            MenuImport::Page(menu) => serde_json::json!({ "path": path, "pageOf": menu.0 }),
        };
        let Some(id) = self
            .run_command("menu.import", params)
            .and_then(|value| value.get("menu").and_then(serde_json::Value::as_u64))
        else {
            return;
        };
        let link = disccraft_project::Link::Menu {
            id: MenuId(id),
            button: None,
        };
        if kind == MenuImport::Menu {
            if self.session.project.settings.title_remote == disccraft_project::Link::NotSet {
                self.set_title_remote(&link);
            }
            if self.session.project.settings.first_play == disccraft_project::Link::NotSet {
                self.set_first_play(&link);
            }
        }
        if !matches!(kind, MenuImport::Page(_)) {
            self.place_new_item(id);
        }
        self.select(crate::app::Selection::Menu(MenuId(id)));
        self.status = tl!("Imported the menu").to_owned();
    }

    /// Menu ▸ Replace Menu… (Encore's メニューを置き換え…): a layered PSD becomes the open menu's design. The
    /// menu's settings and its buttons' links stay, by button number.
    pub(crate) fn replace_menu_from_file(&mut self) {
        let Some(menu) = self.viewer_menu() else {
            return;
        };
        let Some(path) = FileDialog::new()
            .set_title(tl!("Import as Menu"))
            .add_filter(tl!("Menu files"), &["psd"])
            .pick_file()
        else {
            return;
        };
        let replaced = self.run_command(
            "menu.replace",
            serde_json::json!({ "menu": menu.0, "path": path.to_string_lossy() }),
        );
        if replaced.is_some() {
            self.status = tl!("Replaced the menu").to_owned();
        }
    }

    /// Menu ▸ Add New Page (Encore's 新規ページを追加): an empty page of the open menu, shown in the viewer.
    pub(crate) fn add_menu_page(&mut self) {
        let Some(menu) = self.viewer_menu() else {
            return;
        };
        let added = self.run_command("menu.addPage", serde_json::json!({ "menu": menu.0 }));
        if let Some(page) =
            added.and_then(|value| value.get("menu").and_then(serde_json::Value::as_u64))
        {
            self.select(crate::app::Selection::Menu(disccraft_project::MenuId(page)));
        }
    }

    /// Menu ▸ Save Menu as Template…: the menu into the library's User set (as Library ▸ New Item does).
    pub(crate) fn save_menu_as_template(&mut self) {
        let Some(menu) = self.edited_menu() else {
            return;
        };
        let Some(spec) = self.session.project.menu_spec(menu.id) else {
            return;
        };
        self.store_user_item(UserItem {
            name: menu.name.clone(),
            kind: UserItemKind::Menu(spec),
        });
    }
}

/// What 読み込み ▸ makes of a layered PSD.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuImport {
    /// A new menu (メニュー…).
    Menu,
    /// A new Blu-ray pop-up menu (ポップアップメニュー…).
    Popup,
    /// A new page of this menu (メニューページ…).
    Page(MenuId),
}
