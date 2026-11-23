//! The user's own library items (Encore's ライブラリ ▸ 項目を追加): menus, buttons, background colours and text
//! objects saved from the open menu, kept in `library.json` next to the workspace file and listed under the "User"
//! set.

use crate::i18n::{tl, tlf};
use disccraft_project::{ButtonSpec, MenuGraphic, MenuSpec, MenuText};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The most items the user library holds (M6.403: a folder of an Encore library has hundreds of files).
pub(crate) const MAX_USER_ITEMS: usize = 5000;

/// The most sets of one's own (Encore's Library ▸ New Set; Import Folder… makes one per folder).
pub(crate) const MAX_USER_SETS: usize = 500;

/// The longest set name, in characters.
pub(crate) const MAX_SET_NAME: usize = 40;

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
    /// A text object (Encore's text items): its text, look and box.
    Text(MenuText),
    /// Image and shape layers placed together (Encore's layer sets), or one replacement layer (M6.331).
    Layers(LayerItem),
    /// A file of the library's own (M6.403, Encore's Library ▸ Add Item…).
    File(FileItem),
}

/// A file of the library's own (M6.403): a copy in the library's folder, and what it is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileItem {
    /// The copy, relative to the library's folder (`items/<name>`).
    pub(crate) file: String,
    /// What it is in the Library.
    pub(crate) kind: FileKind,
}

impl FileItem {
    /// Where the copy is.
    pub(crate) fn path(&self) -> Option<PathBuf> {
        crate::workspace::library_dir().map(|dir| dir.join(&self.file))
    }
}

/// What a file of the library is (Encore's kinds by the file's contents, docs/encore-behavior.md 9.18).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum FileKind {
    /// A menu: New Menu and Replace make menus of it (as File ▸ Import ▸ Menu… does).
    Menu,
    /// A button: placed as a picture with a button over it.
    Button,
    /// A picture: placed as an image layer.
    Image,
    /// A layer set: placed as an image layer of its layers.
    LayerSet,
    /// A replacement layer: placed as a replacement layer.
    Replacement,
}

impl From<disccraft_photo::LibraryFileKind> for FileKind {
    fn from(kind: disccraft_photo::LibraryFileKind) -> Self {
        match kind {
            disccraft_photo::LibraryFileKind::Menu => FileKind::Menu,
            disccraft_photo::LibraryFileKind::Button => FileKind::Button,
            disccraft_photo::LibraryFileKind::Image => FileKind::Image,
            disccraft_photo::LibraryFileKind::LayerSet => FileKind::LayerSet,
            disccraft_photo::LibraryFileKind::Replacement => FileKind::Replacement,
        }
    }
}

/// A user item of layers: its graphics, bottom first, where they were in the menu they came from; for each image
/// its file (`None` for a shape), so that another project finds the asset or imports it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LayerItem {
    pub(crate) graphics: Vec<MenuGraphic>,
    #[serde(default)]
    pub(crate) images: Vec<Option<String>>,
    /// One replacement layer (Encore's Library kind), else a layer set.
    #[serde(default)]
    pub(crate) replacement: bool,
}

/// One saved item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UserItem {
    /// Display name (the menu's or button's name when saved).
    pub(crate) name: String,
    pub(crate) kind: UserItemKind,
    /// The set of one's own it is in (one of [`UserLibrary::sets`]); empty: the "User" set.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) set: String,
}

/// The user library file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UserLibrary {
    #[serde(default)]
    pub(crate) items: Vec<UserItem>,
    /// The sets of one's own (Encore's Library ▸ New Set), in the order they were made, beside "General" and
    /// "User".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) sets: Vec<String>,
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

    /// `name` as a set's name: trimmed and at most [`MAX_SET_NAME`] characters; an error when it is empty, is the
    /// name of a built-in set, or is taken by another set (`except`: the set being renamed).
    pub(crate) fn set_name(&self, name: &str, except: Option<usize>) -> Result<String, String> {
        let name: String = name.trim().chars().take(MAX_SET_NAME).collect();
        if name.is_empty() {
            return Err(tl!("A set needs a name").to_owned());
        }
        let taken = |other: &str| other.to_lowercase() == name.to_lowercase();
        let builtin = ["General", "User"]
            .into_iter()
            .any(|builtin| taken(builtin) || taken(crate::i18n::t(builtin)));
        let used = self
            .sets
            .iter()
            .enumerate()
            .any(|(index, other)| Some(index) != except && taken(other));
        if builtin || used {
            return Err(crate::i18n::tlf!(
                "There is already a set named {name}",
                name = name
            ));
        }
        Ok(name)
    }

    /// Makes a set (Encore's New Set): its index.
    pub(crate) fn add_set(&mut self, name: &str) -> Result<usize, String> {
        if self.sets.len() >= MAX_USER_SETS {
            return Err(format!("the library holds at most {MAX_USER_SETS} sets"));
        }
        let name = self.set_name(name, None)?;
        self.sets.push(name);
        Ok(self.sets.len().saturating_sub(1))
    }

    /// Renames set `index`; its items go with it.
    pub(crate) fn rename_set(&mut self, index: usize, name: &str) -> Result<(), String> {
        let name = self.set_name(name, Some(index))?;
        let Some(old) = self.sets.get(index).cloned() else {
            return Err(format!("no set {index}"));
        };
        for item in self.items.iter_mut().filter(|item| item.set == old) {
            item.set.clone_from(&name);
        }
        if let Some(slot) = self.sets.get_mut(index) {
            *slot = name;
        }
        Ok(())
    }

    /// The set of one's own named `name` (letter case not counted), made when there is none (M6.403, Import
    /// Folder…): its name as kept. A built-in set's name, or a library with all its sets, gives the User set (empty).
    pub(crate) fn set_for(&mut self, name: &str) -> String {
        let wanted: String = name.trim().chars().take(MAX_SET_NAME).collect();
        if let Some(found) = self
            .sets
            .iter()
            .find(|set| set.to_lowercase() == wanted.to_lowercase())
        {
            return found.clone();
        }
        self.add_set(&wanted)
            .ok()
            .and_then(|index| self.sets.get(index).cloned())
            .unwrap_or_default()
    }

    /// Removes item `index`; a file of the library's own goes with it when no other item uses it (M6.403).
    pub(crate) fn remove_item(&mut self, index: usize) -> Option<UserItem> {
        if index >= self.items.len() {
            return None;
        }
        let item = self.items.remove(index);
        if let UserItemKind::File(file) = &item.kind {
            let used = self.items.iter().any(
                |other| matches!(&other.kind, UserItemKind::File(other) if other.file == file.file),
            );
            if !used && let Some(path) = file.path() {
                let _ = std::fs::remove_file(path);
            }
        }
        Some(item)
    }

    /// Removes set `index`: its items move to the "User" set (nothing is lost). How many moved.
    pub(crate) fn remove_set(&mut self, index: usize) -> usize {
        if index >= self.sets.len() {
            return 0;
        }
        let old = self.sets.remove(index);
        let mut moved = 0;
        for item in self.items.iter_mut().filter(|item| item.set == old) {
            item.set.clear();
            moved += 1;
        }
        moved
    }
}

/// `library.json` in the library's folder (Preferences ▸ Media; beside `workspace.json` unless another is
/// chosen).
fn library_path() -> Option<PathBuf> {
    crate::workspace::library_dir().map(|dir| dir.join("library.json"))
}

#[cfg(test)]
mod tests {
    use super::{UserItem, UserItemKind, UserLibrary};

    /// M6.305: sets of one's own are made, renamed (their items with them) and removed (their items go to the
    /// User set); a name is not empty nor taken, built-in names included.
    #[test]
    fn sets_are_made_renamed_and_removed() -> Result<(), String> {
        let mut library = UserLibrary::default();
        let concerts = library.add_set("  Concerts  ")?;
        assert_eq!(library.sets, ["Concerts"]);
        assert!(library.add_set("concerts").is_err());
        assert!(library.add_set("   ").is_err());
        assert!(library.add_set("User").is_err());
        library.add(UserItem {
            name: "Night".to_owned(),
            kind: UserItemKind::Background([1, 2, 3]),
            set: "Concerts".to_owned(),
        })?;
        library.add(UserItem {
            name: "Day".to_owned(),
            kind: UserItemKind::Background([4, 5, 6]),
            set: String::new(),
        })?;
        library.rename_set(concerts, "Live")?;
        assert_eq!(library.sets, ["Live"]);
        let sets: Vec<&str> = library.items.iter().map(|item| item.set.as_str()).collect();
        assert_eq!(sets, ["Live", ""]);
        // Keeping its own name is no clash.
        library.rename_set(concerts, "live")?;
        assert_eq!(library.remove_set(concerts), 1);
        assert!(library.sets.is_empty());
        assert!(library.items.iter().all(|item| item.set.is_empty()));
        Ok(())
    }

    #[test]
    fn items_round_trip_through_json() -> Result<(), Box<dyn std::error::Error>> {
        let mut library = UserLibrary::default();
        library.add(UserItem {
            name: "Night".to_owned(),
            kind: UserItemKind::Background([1, 2, 3]),
            set: String::new(),
        })?;
        library.add(UserItem {
            name: "Play".to_owned(),
            kind: UserItemKind::Button(disccraft_project::ButtonSpec {
                label: "Play".to_owned(),
                x: 10,
                y: 20,
                w: 300,
                h: 40,
                thumbnail: Some(disccraft_project::ThumbnailArea::WHOLE),
                kind: disccraft_project::ButtonKind::Chapter,
            }),
            set: String::new(),
        })?;
        // M6.304: a text item keeps the whole text object.
        library.add(UserItem {
            name: "Chapters".to_owned(),
            kind: UserItemKind::Text(disccraft_project::MenuText {
                effects: Default::default(),
                text: "Chapters".to_owned(),
                x: 40,
                y: 30,
                size: 36,
                color: Some([250, 240, 200]),
                font: None,
                font_style: None,
                format: disccraft_project::TextFormat {
                    bold: true,
                    ..Default::default()
                },
                shadow: None,
                orientation: Default::default(),
                vertical: false,
                box_width: Some(200),
                box_height: Some(90),
                hidden: false,
                locked: false,
                spans: Vec::new(),
            }),
            set: "Titles".to_owned(),
        })?;
        // M6.403: a file of the library's own keeps its copy and its kind.
        library.add(UserItem {
            name: "Citylights".to_owned(),
            kind: UserItemKind::File(super::FileItem {
                file: "items/Citylights.psd".to_owned(),
                kind: super::FileKind::Menu,
            }),
            set: "Menus".to_owned(),
        })?;
        let text = serde_json::to_string(&library)?;
        assert!(text.contains(r#""file":{"file":"items/Citylights.psd","kind":"menu"}"#));
        let back: UserLibrary = serde_json::from_str(&text)?;
        assert_eq!(back, library);
        Ok(())
    }

    /// M6.403: Import Folder… finds a set of one's own by its name (letter case not counted) or makes it; a built-in
    /// set's name gives the User set.
    #[test]
    fn an_imported_folder_finds_or_makes_its_set() {
        let mut library = UserLibrary::default();
        assert_eq!(library.set_for("Menus"), "Menus");
        assert_eq!(library.set_for(" menus "), "Menus");
        assert_eq!(library.set_for("User"), "");
        assert_eq!(library.sets, ["Menus"]);
    }
}
