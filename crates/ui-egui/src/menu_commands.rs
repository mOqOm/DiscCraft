//! The rest of Encore's Menu menu for the menu being edited: Set Menu User Operations…, Save Menu as File…,
//! Save Menu as Template… and Set End Action….

use crate::app::DiscCraftApp;
use crate::flowchart::{NodeId, Port};
use crate::i18n::{tl, tlf};
use crate::user_library::{UserItem, UserItemKind};
use crate::user_ops::{UserOpsDialog, UserOpsTarget};
use disccraft_project::Menu;
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
