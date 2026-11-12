//! Folders of the Project panel (Encore's folders): any item can sit in a folder and folders nest. Folders
//! only organise the panel; nothing of them reaches the disc.

use crate::io::ProjectError;
use crate::model::Project;
use serde::{Deserialize, Serialize};

/// The most folders in a project.
pub const MAX_FOLDERS: usize = 999;

/// A folder of the Project panel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    /// Shared object id.
    pub id: u64,
    /// Display name.
    pub name: String,
    /// The folder this one sits in; `None` at the root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
}

impl Project {
    /// Adds a folder (at the root, or inside `parent`).
    pub fn add_folder(
        &mut self,
        name: Option<String>,
        parent: Option<u64>,
    ) -> Result<u64, ProjectError> {
        if self.folders.len() >= MAX_FOLDERS {
            return Err(ProjectError::Invalid("too many folders".to_string()));
        }
        if let Some(parent) = parent
            && !self.folders.contains_key(&parent)
        {
            return Err(ProjectError::Invalid(
                "parent folder does not exist".to_string(),
            ));
        }
        let id = self.alloc_id()?;
        let name = name
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| format!("Folder {}", self.folders.len().saturating_add(1)));
        let _ = self.folders.insert(id, Folder { id, name, parent });
        Ok(id)
    }

    /// Renames a folder.
    pub fn rename_folder(&mut self, id: u64, name: &str) -> Result<(), ProjectError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ProjectError::Invalid(
                "folder name must not be empty".to_string(),
            ));
        }
        let folder = self
            .folders
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("folder does not exist".to_string()))?;
        folder.name = name.to_string();
        Ok(())
    }

    /// Removes a folder; what was inside moves to the folder's parent.
    pub fn remove_folder(&mut self, id: u64) -> Result<(), ProjectError> {
        let parent = self
            .folders
            .remove(&id)
            .ok_or_else(|| ProjectError::Invalid("folder does not exist".to_string()))?
            .parent;
        for folder in self.folders.values_mut() {
            if folder.parent == Some(id) {
                folder.parent = parent;
            }
        }
        let inside: Vec<u64> = self
            .placements
            .iter()
            .filter(|(_, folder)| **folder == id)
            .map(|(item, _)| *item)
            .collect();
        for item in inside {
            match parent {
                Some(parent) => {
                    let _ = self.placements.insert(item, parent);
                }
                None => {
                    let _ = self.placements.remove(&item);
                }
            }
        }
        Ok(())
    }

    /// Moves an item (an asset, timeline, menu, slideshow, playlist or folder) into `folder`, or to the root
    /// with `None`. A folder cannot move into itself or one of its own folders.
    pub fn move_to_folder(&mut self, item: u64, folder: Option<u64>) -> Result<(), ProjectError> {
        if let Some(folder) = folder
            && !self.folders.contains_key(&folder)
        {
            return Err(ProjectError::Invalid("folder does not exist".to_string()));
        }
        if self.folders.contains_key(&item) {
            if let Some(target) = folder
                && (target == item || self.folder_is_inside(target, item))
            {
                return Err(ProjectError::Invalid(
                    "a folder cannot move into itself".to_string(),
                ));
            }
            if let Some(moving) = self.folders.get_mut(&item) {
                moving.parent = folder;
            }
            return Ok(());
        }
        if !self.item_exists(item) {
            return Err(ProjectError::Invalid(format!("item {item} does not exist")));
        }
        match folder {
            Some(folder) => {
                let _ = self.placements.insert(item, folder);
            }
            None => {
                let _ = self.placements.remove(&item);
            }
        }
        Ok(())
    }

    /// The folder an item sits in (`None` at the root, or when the folder is gone).
    pub fn folder_of(&self, item: u64) -> Option<u64> {
        self.placements
            .get(&item)
            .copied()
            .filter(|folder| self.folders.contains_key(folder))
    }

    /// Whether an id names an asset, timeline, menu, slideshow or playlist.
    pub fn item_exists(&self, id: u64) -> bool {
        self.assets.contains_key(&crate::ids::AssetId(id))
            || self.timelines.contains_key(&crate::ids::TimelineId(id))
            || self.menus.contains_key(&crate::ids::MenuId(id))
            || self.slideshows.contains_key(&id)
            || self.playlists.contains_key(&id)
    }

    /// Whether `folder` is `ancestor` or sits (at any depth) inside it.
    fn folder_is_inside(&self, folder: u64, ancestor: u64) -> bool {
        let mut current = Some(folder);
        // Bounded by the folder count, so a damaged file cannot loop forever.
        for _ in 0..=self.folders.len() {
            match current {
                Some(id) if id == ancestor => return true,
                Some(id) => current = self.folders.get(&id).and_then(|folder| folder.parent),
                None => return false,
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use crate::Project;
    use std::error::Error;

    #[test]
    fn folders_nest_and_hold_items() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(Some("Main".to_string()))?;
        let root = project.add_folder(Some("Menus".to_string()), None)?;
        let inner = project.add_folder(None, Some(root))?;
        assert_eq!(
            project.folders.get(&inner).map(|f| f.name.as_str()),
            Some("Folder 2")
        );

        project.move_to_folder(menu.0, Some(inner))?;
        assert_eq!(project.folder_of(menu.0), Some(inner));
        // A folder cannot move into its own child.
        assert!(project.move_to_folder(root, Some(inner)).is_err());
        assert!(project.move_to_folder(999, Some(root)).is_err());

        // Removing the inner folder lifts the menu into the outer one; removing that one lifts it to the root.
        project.remove_folder(inner)?;
        assert_eq!(project.folder_of(menu.0), Some(root));
        project.remove_folder(root)?;
        assert_eq!(project.folder_of(menu.0), None);
        assert!(project.placements.is_empty());
        Ok(())
    }
}
