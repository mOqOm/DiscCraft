//! Removing whole objects (timelines, menus, slideshows, playlists, chapter playlists, assets) from a project.
//!
//! Removing an object also clears every link that points at it (first play, the Title button, button links,
//! end actions, menu remotes and overrides) back to [`Link::NotSet`], and drops a removed timeline from the
//! playlists that play it (its chapter playlists go with it). An asset can only be removed while nothing uses it.

use crate::ids::{AssetId, MenuId, TimelineId};
use crate::io::ProjectError;
use crate::link::Link;
use crate::lists::PlaylistItem;
use crate::menu::MenuBackground;
use crate::model::Project;

/// What a link points at, for clearing links to a removed object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Timeline(TimelineId),
    Menu(MenuId),
    Slideshow(u64),
    Playlist(u64),
    ChapterPlaylist(u64),
}

impl Target {
    /// Whether `link` points at this object.
    fn matches(self, link: &Link) -> bool {
        match (self, link) {
            (Target::Timeline(target), Link::Timeline { id, .. }) => *id == target,
            (Target::Menu(target), Link::Menu { id, .. }) => *id == target,
            (Target::Slideshow(target), Link::Slideshow { id }) => *id == target,
            (Target::Playlist(target), Link::Playlist { id }) => *id == target,
            (Target::ChapterPlaylist(target), Link::ChapterPlaylist { id }) => *id == target,
            _ => false,
        }
    }
}

impl Project {
    /// Remove a timeline, drop it from playlists, remove its chapter playlists and clear links to it.
    pub fn remove_timeline(&mut self, id: TimelineId) -> Result<(), ProjectError> {
        if self.timelines.remove(&id).is_none() {
            return Err(ProjectError::Invalid(format!(
                "timeline {id} does not exist"
            )));
        }
        for playlist in self.playlists.values_mut() {
            playlist
                .items
                .retain(|item| *item != PlaylistItem::Timeline(id));
        }
        for chapter_playlist in self.chapter_playlists_of(id) {
            let _ = self.remove_chapter_playlist(chapter_playlist);
        }
        self.clear_links(Target::Timeline(id));
        let _ = self.placements.remove(&id.0);
        Ok(())
    }

    /// Remove a chapter playlist and clear links to it.
    pub fn remove_chapter_playlist(&mut self, id: u64) -> Result<(), ProjectError> {
        if self.chapter_playlists.remove(&id).is_none() {
            return Err(ProjectError::Invalid(format!(
                "chapter playlist {id} does not exist"
            )));
        }
        for playlist in self.playlists.values_mut() {
            playlist
                .items
                .retain(|item| *item != PlaylistItem::ChapterPlaylist(id));
        }
        self.clear_links(Target::ChapterPlaylist(id));
        let _ = self.placements.remove(&id);
        Ok(())
    }

    /// Remove a menu and clear links to it.
    pub fn remove_menu(&mut self, id: MenuId) -> Result<(), ProjectError> {
        if self.menus.remove(&id).is_none() {
            return Err(ProjectError::Invalid(format!("menu {id} does not exist")));
        }
        self.clear_links(Target::Menu(id));
        let _ = self.placements.remove(&id.0);
        Ok(())
    }

    /// Remove a slideshow and clear links to it.
    pub fn remove_slideshow(&mut self, id: u64) -> Result<(), ProjectError> {
        if self.slideshows.remove(&id).is_none() {
            return Err(ProjectError::Invalid(format!(
                "slideshow {id} does not exist"
            )));
        }
        for playlist in self.playlists.values_mut() {
            playlist
                .items
                .retain(|item| *item != PlaylistItem::Slideshow(id));
        }
        self.clear_links(Target::Slideshow(id));
        let _ = self.placements.remove(&id);
        Ok(())
    }

    /// Remove a playlist and clear links to it.
    pub fn remove_playlist(&mut self, id: u64) -> Result<(), ProjectError> {
        if self.playlists.remove(&id).is_none() {
            return Err(ProjectError::Invalid(format!(
                "playlist {id} does not exist"
            )));
        }
        self.clear_links(Target::Playlist(id));
        let _ = self.placements.remove(&id);
        Ok(())
    }

    /// Remove an asset that nothing uses. Fails, naming the first user, while a timeline, menu or slideshow
    /// still refers to it.
    pub fn remove_asset(&mut self, id: AssetId) -> Result<(), ProjectError> {
        if !self.assets.contains_key(&id) {
            return Err(ProjectError::Invalid(format!("asset {id} does not exist")));
        }
        if let Some(user) = self.asset_user(id) {
            return Err(ProjectError::Invalid(format!(
                "asset {id} is used by {user}"
            )));
        }
        let _ = self.assets.remove(&id);
        let _ = self.placements.remove(&id.0);
        Ok(())
    }

    /// The name of the first object that uses asset `id`, if any.
    pub fn asset_user(&self, id: AssetId) -> Option<String> {
        for timeline in self.timelines.values() {
            let uses = timeline.video == id
                || timeline.audio.iter().any(|track| track.asset == id)
                || timeline.subtitles.iter().any(|track| track.asset == id);
            if uses {
                return Some(timeline.name.clone());
            }
        }
        for menu in self.menus.values() {
            let uses = menu.audio == Some(id)
                || match &menu.background {
                    MenuBackground::Image { asset } | MenuBackground::Video { asset, .. } => {
                        *asset == id
                    }
                    MenuBackground::Color { .. } => false,
                }
                || menu.graphics.iter().any(|graphic| {
                    matches!(graphic.kind, crate::graphic::GraphicKind::Image { asset } if asset == id)
                });
            if uses {
                return Some(menu.name.clone());
            }
        }
        for slideshow in self.slideshows.values() {
            if slideshow.audio == Some(id) || slideshow.slides.iter().any(|slide| slide.asset == id)
            {
                return Some(slideshow.name.clone());
            }
        }
        None
    }

    /// Set every link that points at `target` back to [`Link::NotSet`].
    fn clear_links(&mut self, target: Target) {
        self.visit_links(|link| {
            if target.matches(link) {
                *link = Link::NotSet;
            }
        });
    }

    /// Calls `visit` with every link in the project (first play, title button, buttons, end actions, remotes).
    pub(crate) fn visit_links(&mut self, mut visit: impl FnMut(&mut Link)) {
        visit(&mut self.settings.first_play);
        visit(&mut self.settings.title_remote);
        if let Some(link) = self.settings.first_play_override.as_mut() {
            visit(link);
        }
        for menu in self.menus.values_mut() {
            visit(&mut menu.end_action);
            if let Some(link) = menu.override_link.as_mut() {
                visit(link);
            }
            for button in &mut menu.buttons {
                visit(&mut button.link);
                if let Some(link) = button.override_link.as_mut() {
                    visit(link);
                }
            }
        }
        for timeline in self.timelines.values_mut() {
            visit(&mut timeline.end_action);
            visit(&mut timeline.menu_remote);
            if let Some(link) = timeline.override_link.as_mut() {
                visit(link);
            }
        }
        for slideshow in self.slideshows.values_mut() {
            visit(&mut slideshow.end_action);
        }
        for playlist in self.playlists.values_mut() {
            visit(&mut playlist.end_action);
        }
        for playlist in self.chapter_playlists.values_mut() {
            visit(&mut playlist.end_action);
            if let Some(link) = playlist.override_link.as_mut() {
                visit(link);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ids::TimelineId;
    use crate::link::Link;
    use crate::model::Project;
    use std::error::Error;
    use std::path::Path;

    fn project_with_timeline() -> Result<(Project, TimelineId, crate::ids::MenuId), Box<dyn Error>>
    {
        let mut project = Project::default();
        let asset = project.add_asset(Path::new("a.m2v"))?;
        let timeline = project.add_timeline(asset, None)?;
        let menu = project.add_menu(Some("top".to_owned()))?;
        Ok((project, timeline, menu))
    }

    #[test]
    fn removing_a_timeline_clears_links_and_playlist_items() -> Result<(), Box<dyn Error>> {
        let (mut project, timeline, menu) = project_with_timeline()?;
        project.settings.first_play = Link::Timeline {
            id: timeline,
            chapter: 0,
        };
        let playlist = project.add_playlist(None)?;
        let _ = project.add_playlist_item(playlist, timeline)?;
        let _ = project.add_button(
            menu,
            "play".to_owned(),
            None,
            Link::Timeline {
                id: timeline,
                chapter: 0,
            },
        )?;

        let chapter_playlist = project.add_chapter_playlist(timeline, None)?;
        project.settings.title_remote = Link::ChapterPlaylist {
            id: chapter_playlist,
        };

        project.remove_timeline(timeline)?;

        assert!(!project.timelines.contains_key(&timeline));
        assert_eq!(project.settings.first_play, Link::NotSet);
        assert!(project.playlists.values().all(|list| list.items.is_empty()));
        assert!(project.chapter_playlists.is_empty());
        assert_eq!(project.settings.title_remote, Link::NotSet);
        let link = project
            .menus
            .get(&menu)
            .and_then(|menu| menu.buttons.first())
            .map(|button| button.link.clone());
        assert_eq!(link, Some(Link::NotSet));
        Ok(())
    }

    #[test]
    fn removing_a_menu_clears_the_title_button() -> Result<(), Box<dyn Error>> {
        let (mut project, timeline, menu) = project_with_timeline()?;
        project.settings.title_remote = Link::Menu {
            id: menu,
            button: None,
        };
        if let Some(timeline) = project.timelines.get_mut(&timeline) {
            timeline.end_action = Link::Menu {
                id: menu,
                button: None,
            };
        }

        project.remove_menu(menu)?;

        assert_eq!(project.settings.title_remote, Link::NotSet);
        assert_eq!(
            project
                .timelines
                .get(&timeline)
                .map(|t| t.end_action.clone()),
            Some(Link::NotSet)
        );
        Ok(())
    }

    #[test]
    fn an_asset_in_use_cannot_be_removed() -> Result<(), Box<dyn Error>> {
        let (mut project, timeline, _) = project_with_timeline()?;
        let asset = project
            .timelines
            .get(&timeline)
            .map(|timeline| timeline.video)
            .ok_or("missing timeline")?;

        assert!(project.remove_asset(asset).is_err());
        project.remove_timeline(timeline)?;
        project.remove_asset(asset)?;
        assert!(project.assets.is_empty());
        Ok(())
    }

    #[test]
    fn removing_a_missing_object_is_an_error() {
        let mut project = Project::default();
        assert!(project.remove_timeline(TimelineId(9)).is_err());
        assert!(project.remove_slideshow(9).is_err());
        assert!(project.remove_playlist(9).is_err());
    }
}
