//! The user's own library items (Encore's ライブラリ ▸ 項目を追加): menus, buttons and background colours saved from
//! the open menu, kept in `library.json` next to the workspace file and listed under the "User" set.

use disccraft_project::{ButtonSpec, MenuSpec};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The most items the user library holds.
pub(crate) const MAX_USER_ITEMS: usize = 200;

/// What a user item is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum UserItemKind {
    /// A whole menu: colours and buttons.
    Menu(MenuSpec),
    /// One button.
    Button(ButtonSpec),
    /// A background colour.
    Background([u8; 3]),
}

/// One saved item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UserItem {
    /// Display name (the menu's or button's name when saved).
    pub(crate) name: String,
    pub(crate) kind: UserItemKind,
}

/// The user library file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UserLibrary {
    #[serde(default)]
    pub(crate) items: Vec<UserItem>,
}

impl UserLibrary {
    /// Loads the library, or an empty one when there is no file or it cannot be read.
    pub(crate) fn load() -> Self {
        let Some(path) = library_path() else {
            return Self::default();
        };
        std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Saves the library; a failure is reported as a message for the status bar.
    pub(crate) fn save(&self) -> Result<(), String> {
        let path = library_path().ok_or_else(|| "no configuration folder".to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        std::fs::write(&path, text).map_err(|error| error.to_string())
    }

    /// Adds an item (the newest last). Fails when the library is full.
    pub(crate) fn add(&mut self, item: UserItem) -> Result<usize, String> {
        if self.items.len() >= MAX_USER_ITEMS {
            return Err(format!("the library holds at most {MAX_USER_ITEMS} items"));
        }
        self.items.push(item);
        Ok(self.items.len().saturating_sub(1))
    }
}

/// `library.json` in the configuration folder (beside `workspace.json`).
fn library_path() -> Option<PathBuf> {
    crate::workspace::config_dir().map(|dir| dir.join("disccraft").join("library.json"))
}

#[cfg(test)]
mod tests {
    use super::{UserItem, UserItemKind, UserLibrary};

    #[test]
    fn items_round_trip_through_json() -> Result<(), Box<dyn std::error::Error>> {
        let mut library = UserLibrary::default();
        library.add(UserItem {
            name: "Night".to_owned(),
            kind: UserItemKind::Background([1, 2, 3]),
        })?;
        library.add(UserItem {
            name: "Play".to_owned(),
            kind: UserItemKind::Button(disccraft_project::ButtonSpec {
                label: "Play".to_owned(),
                x: 10,
                y: 20,
                w: 300,
                h: 40,
            }),
        })?;
        let text = serde_json::to_string(&library)?;
        let back: UserLibrary = serde_json::from_str(&text)?;
        assert_eq!(back, library);
        Ok(())
    }
}
