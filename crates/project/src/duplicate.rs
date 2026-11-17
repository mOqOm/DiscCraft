//! Copies of a project's items (Encore: Copy and Paste, or Duplicate, in the Project panel): a menu with its
//! pages, a timeline, a slideshow, a playlist or a chapter playlist gets a twin with a name of its own, in the same
//! folder. Links to other items are kept; a link the item had to itself points at the copy.

use crate::ids::{MenuId, TimelineId};
use crate::io::ProjectError;
use crate::link::Link;
use crate::model::Project;

/// The item a copy was made of, and the copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duplicate {
    /// A menu (its first page when it has several).
    Menu(MenuId),
    /// A timeline.
    Timeline(TimelineId),
    /// A slideshow.
    Slideshow(u64),
    /// A playlist.
    Playlist(u64),
    /// A chapter playlist.
    ChapterPlaylist(u64),
}

impl Duplicate {
    /// The copy's shared object id.
    #[must_use]
    pub fn id(self) -> u64 {
        match self {
            Self::Menu(id) => id.0,
            Self::Timeline(id) => id.0,
            Self::Slideshow(id) | Self::Playlist(id) | Self::ChapterPlaylist(id) => id,
        }
    }
}

/// `name` with a number after it that no name in `taken` has: "Main 2", then "Main 3". A name that already ends
/// in a number counts on from it.
fn copy_name(name: &str, taken: &[&str]) -> String {
    let (stem, from) = match name.rsplit_once(' ') {
        Some((stem, number)) if !stem.is_empty() => match number.parse::<u32>() {
            Ok(number) => (stem, number.saturating_add(1)),
            Err(_) => (name, 2),
        },
        _ => (name, 2),
    };
    (from..=u32::MAX)
        .map(|number| format!("{stem} {number}"))
        .find(|candidate| !taken.contains(&candidate.as_str()))
        .unwrap_or_else(|| format!("{stem} {from}"))
}

impl Project {
    /// Puts `copy` in the folder `original` is in.
    fn place_like(&mut self, original: u64, copy: u64) {
        if let Some(folder) = self.folder_of(original) {
            let _ = self.placements.insert(copy, folder);
        }
    }

    /// Copies the item `original` names. A menu is copied with all its pages (from any of them).
    ///
    /// # Errors
    ///
    /// When the item does not exist or no id is left.
    pub fn duplicate(&mut self, original: Duplicate) -> Result<Duplicate, ProjectError> {
        match original {
            Duplicate::Menu(id) => self.duplicate_menu(id).map(Duplicate::Menu),
            Duplicate::Timeline(id) => self.duplicate_timeline(id).map(Duplicate::Timeline),
            Duplicate::Slideshow(id) => self.duplicate_slideshow(id).map(Duplicate::Slideshow),
            Duplicate::Playlist(id) => self.duplicate_playlist(id).map(Duplicate::Playlist),
            Duplicate::ChapterPlaylist(id) => self
                .duplicate_chapter_playlist(id)
                .map(Duplicate::ChapterPlaylist),
        }
    }

    fn duplicate_menu(&mut self, id: MenuId) -> Result<MenuId, ProjectError> {
        if !self.menus.contains_key(&id) {
            return Err(ProjectError::Invalid(format!("menu {id} does not exist")));
        }
        let pages = self.menu_pages(id);
        // Each page's copy, the first page first.
        let mut twins = Vec::with_capacity(pages.len());
        for page in &pages {
            twins.push((*page, MenuId(self.alloc_id()?)));
        }
        let twin_of = |page: MenuId| {
            twins
                .iter()
                .find(|(from, _)| *from == page)
                .map(|(_, to)| *to)
        };
        let names: Vec<String> = self.menus.values().map(|menu| menu.name.clone()).collect();
        let taken: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut copies = Vec::with_capacity(pages.len());
        for (page, twin) in &twins {
            let Some(mut copy) = self.menus.get(page).cloned() else {
                continue;
            };
            copy.id = *twin;
            copy.name = copy_name(&copy.name, &taken);
            copy.page_of = copy.page_of.and_then(twin_of);
            // Links among the pages (and a menu's links to itself) go to the copies.
            let retarget = |link: &mut Link| {
                if let Link::Menu { id, .. } = link
                    && let Some(twin) = twin_of(*id)
                {
                    *id = twin;
                }
            };
            retarget(&mut copy.end_action);
            copy.override_link.iter_mut().for_each(retarget);
            for button in &mut copy.buttons {
                retarget(&mut button.link);
                button.override_link.iter_mut().for_each(retarget);
            }
            copies.push(copy);
        }
        for copy in copies {
            let (original, twin) = twins
                .iter()
                .find(|(_, to)| *to == copy.id)
                .copied()
                .unwrap_or((id, copy.id));
            let _ = self.menus.insert(twin, copy);
            self.place_like(original.0, twin.0);
        }
        twins
            .first()
            .map(|(_, twin)| *twin)
            .ok_or_else(|| ProjectError::Invalid(format!("menu {id} does not exist")))
    }

    fn duplicate_timeline(&mut self, id: TimelineId) -> Result<TimelineId, ProjectError> {
        let Some(mut copy) = self.timelines.get(&id).cloned() else {
            return Err(ProjectError::Invalid(format!(
                "timeline {id} does not exist"
            )));
        };
        let twin = TimelineId(self.alloc_id()?);
        let names: Vec<&str> = self.timelines.values().map(|t| t.name.as_str()).collect();
        copy.name = copy_name(&copy.name, &names);
        copy.id = twin;
        let retarget = |link: &mut Link| {
            if let Link::Timeline { id: target, .. } = link
                && *target == id
            {
                *target = twin;
            }
        };
        retarget(&mut copy.end_action);
        retarget(&mut copy.menu_remote);
        copy.override_link.iter_mut().for_each(retarget);
        let _ = self.timelines.insert(twin, copy);
        self.place_like(id.0, twin.0);
        Ok(twin)
    }

    fn duplicate_slideshow(&mut self, id: u64) -> Result<u64, ProjectError> {
        let Some(mut copy) = self.slideshows.get(&id).cloned() else {
            return Err(ProjectError::Invalid(format!(
                "slideshow {id} does not exist"
            )));
        };
        let twin = self.alloc_id()?;
        let names: Vec<&str> = self.slideshows.values().map(|s| s.name.as_str()).collect();
        copy.name = copy_name(&copy.name, &names);
        copy.id = twin;
        if matches!(copy.end_action, Link::Slideshow { id: target } if target == id) {
            copy.end_action = Link::Slideshow { id: twin };
        }
        let _ = self.slideshows.insert(twin, copy);
        self.place_like(id, twin);
        Ok(twin)
    }

    fn duplicate_playlist(&mut self, id: u64) -> Result<u64, ProjectError> {
        let Some(mut copy) = self.playlists.get(&id).cloned() else {
            return Err(ProjectError::Invalid(format!(
                "playlist {id} does not exist"
            )));
        };
        let twin = self.alloc_id()?;
        let names: Vec<&str> = self.playlists.values().map(|p| p.name.as_str()).collect();
        copy.name = copy_name(&copy.name, &names);
        copy.id = twin;
        if matches!(copy.end_action, Link::Playlist { id: target } if target == id) {
            copy.end_action = Link::Playlist { id: twin };
        }
        let _ = self.playlists.insert(twin, copy);
        self.place_like(id, twin);
        Ok(twin)
    }

    fn duplicate_chapter_playlist(&mut self, id: u64) -> Result<u64, ProjectError> {
        let Some(mut copy) = self.chapter_playlists.get(&id).cloned() else {
            return Err(ProjectError::Invalid(format!(
                "chapter playlist {id} does not exist"
            )));
        };
        let twin = self.alloc_id()?;
        let names: Vec<&str> = self
            .chapter_playlists
            .values()
            .map(|p| p.name.as_str())
            .collect();
        copy.name = copy_name(&copy.name, &names);
        copy.id = twin;
        let retarget = |link: &mut Link| {
            if matches!(link, Link::ChapterPlaylist { id: target } if *target == id) {
                *link = Link::ChapterPlaylist { id: twin };
            }
        };
        retarget(&mut copy.end_action);
        copy.override_link.iter_mut().for_each(retarget);
        let _ = self.chapter_playlists.insert(twin, copy);
        self.place_like(id, twin);
        Ok(twin)
    }
}

#[cfg(test)]
mod tests {
    use super::{Duplicate, copy_name};
    use crate::link::Link;
    use crate::model::Project;
    use std::error::Error;
    use std::path::Path;

    #[test]
    fn copies_count_on_from_the_name() {
        assert_eq!(copy_name("Main", &["Main"]), "Main 2");
        assert_eq!(copy_name("Main", &["Main", "Main 2"]), "Main 3");
        assert_eq!(copy_name("Main 2", &["Main", "Main 2"]), "Main 3");
        assert_eq!(copy_name("Scene 9", &[]), "Scene 10");
        assert_eq!(copy_name("7", &[]), "7 2");
        assert_eq!(copy_name("", &[]), " 2");
    }

    #[test]
    fn a_timeline_and_a_menu_are_copied_with_their_own_links() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(Path::new("a.m2v"))?;
        let timeline = project.add_timeline(asset, Some("Feature".to_owned()))?;
        let menu = project.add_menu(Some("Top".to_owned()))?;
        // The timeline loops; the menu's end action is itself.
        if let Some(found) = project.timeline_mut(timeline) {
            found.end_action = Link::Timeline {
                id: timeline,
                chapter: 0,
            };
            found.menu_remote = Link::Menu {
                id: menu,
                button: None,
            };
        }
        if let Some(found) = project.menu_mut(menu) {
            found.end_action = Link::Menu {
                id: menu,
                button: None,
            };
        }
        let folder = project.add_folder(Some("Extras".to_owned()), None)?;
        project.move_to_folder(timeline.0, Some(folder))?;

        let Duplicate::Timeline(twin) = project.duplicate(Duplicate::Timeline(timeline))? else {
            return Err("not a timeline".into());
        };
        assert_ne!(twin, timeline);
        let copy = project.timeline(twin).ok_or("no copy")?;
        assert_eq!(copy.name, "Feature 2");
        assert_eq!(copy.video, asset);
        // Its loop is its own; its link to the menu is kept; it sits in the same folder.
        assert_eq!(
            copy.end_action,
            Link::Timeline {
                id: twin,
                chapter: 0
            }
        );
        assert_eq!(
            copy.menu_remote,
            Link::Menu {
                id: menu,
                button: None
            }
        );
        assert_eq!(project.folder_of(twin.0), Some(folder));
        // The original is as it was.
        assert_eq!(
            project
                .timeline(timeline)
                .map(|found| found.end_action.clone()),
            Some(Link::Timeline {
                id: timeline,
                chapter: 0
            })
        );

        let Duplicate::Menu(menu_twin) = project.duplicate(Duplicate::Menu(menu))? else {
            return Err("not a menu".into());
        };
        let copy = project.menu(menu_twin).ok_or("no copy")?;
        assert_eq!(copy.name, "Top 2");
        assert_eq!(
            copy.end_action,
            Link::Menu {
                id: menu_twin,
                button: None
            }
        );
        // A second copy takes the next number.
        let again = project.duplicate(Duplicate::Menu(menu))?;
        assert_eq!(
            project
                .menu(crate::ids::MenuId(again.id()))
                .map(|m| m.name.clone()),
            Some("Top 3".to_owned())
        );
        // What is not there cannot be copied.
        assert!(project.duplicate(Duplicate::Slideshow(9999)).is_err());
        assert!(project.duplicate(Duplicate::Playlist(9999)).is_err());
        Ok(())
    }
}
