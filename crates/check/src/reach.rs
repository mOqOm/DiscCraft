//! Reachability checks for navigation links.

use crate::warning;
use disccraft_project::{Link, MenuId, PlaylistItem, Problem, Project, TimelineId};
use std::collections::BTreeSet;

/// Appends warnings for timelines and menus that cannot be reached.
pub(crate) fn check(project: &Project, problems: &mut Vec<Problem>) {
    let mut state = ReachState::new();
    state.visit(project, &effective_first_play(project));
    state.visit(project, &project.settings.title_remote);
    state.drain(project);
    add_warnings(project, &state, problems);
}

/// Tracks reachable objects and pending links.
struct ReachState {
    timelines: BTreeSet<TimelineId>,
    menus: BTreeSet<MenuId>,
    /// M6.557: the playlists, chapter playlists and slideshows visited (ids), so each is followed once.
    lists: BTreeSet<u64>,
    pending: Vec<Link>,
}

impl ReachState {
    /// Creates an empty state.
    fn new() -> Self {
        Self {
            timelines: BTreeSet::new(),
            menus: BTreeSet::new(),
            lists: BTreeSet::new(),
            pending: Vec::new(),
        }
    }

    /// Visits a link and queues newly reachable edges.
    fn visit(&mut self, project: &Project, link: &Link) {
        match link {
            Link::Timeline { id, .. } => {
                if self.timelines.insert(*id) {
                    self.push_timeline_edges(project, *id);
                }
            }
            Link::Menu { id, .. } if self.menus.insert(*id) => {
                self.push_menu_edges(project, *id);
            }
            // M6.557: a playlist plays its items (a timeline in it is reached), then its end action; a chapter
            // playlist and a slideshow lead on by their end actions (and a chapter playlist's override).
            Link::Playlist { id } if self.lists.insert(*id) => {
                let Some(playlist) = project.playlists.get(id) else {
                    return;
                };
                for item in &playlist.items {
                    self.pending.push(match item {
                        PlaylistItem::Timeline(id) => Link::Timeline {
                            id: *id,
                            chapter: 0,
                        },
                        PlaylistItem::Slideshow(id) => Link::Slideshow { id: *id },
                        PlaylistItem::ChapterPlaylist(id) => Link::ChapterPlaylist { id: *id },
                    });
                }
                self.push_pending(&playlist.end_action);
            }
            Link::ChapterPlaylist { id } if self.lists.insert(*id) => {
                if let Some(list) = project.chapter_playlists.get(id) {
                    self.push_pending(&list.end_action);
                    if let Some(link) = &list.override_link {
                        self.push_pending(link);
                    }
                }
            }
            Link::Slideshow { id } if self.lists.insert(*id) => {
                if let Some(show) = project.slideshows.get(id) {
                    self.push_pending(&show.end_action);
                }
            }
            _ => {}
        }
    }

    /// Queues a link if it can lead to a reachable object.
    fn push_pending(&mut self, link: &Link) {
        match link {
            Link::Timeline { .. }
            | Link::Menu { .. }
            | Link::Playlist { .. }
            | Link::ChapterPlaylist { .. }
            | Link::Slideshow { .. } => self.pending.push(link.clone()),
            _ => {}
        }
    }

    /// Queues edges from a timeline.
    fn push_timeline_edges(&mut self, project: &Project, id: TimelineId) {
        let Some(timeline) = project.timelines.get(&id) else {
            return;
        };

        self.push_pending(&timeline.end_action);
        if let Some(link) = &timeline.override_link {
            self.push_pending(link);
        }
        self.push_pending(&timeline.menu_remote);
    }

    /// Queues edges from a menu.
    fn push_menu_edges(&mut self, project: &Project, id: MenuId) {
        let Some(menu) = project.menus.get(&id) else {
            return;
        };

        self.push_pending(&menu.end_action);
        for button in &menu.buttons {
            self.push_pending(&button.link);
        }
    }

    /// Processes queued links until none remain.
    fn drain(&mut self, project: &Project) {
        while let Some(link) = self.pending.pop() {
            self.visit(project, &link);
        }
    }
}

/// Returns the link used as the reachability root.
fn effective_first_play(project: &Project) -> Link {
    if project.settings.first_play != Link::NotSet {
        return project.settings.first_play.clone();
    }

    if let Some(id) = project.menus.keys().next().copied() {
        return Link::Menu { id, button: None };
    }

    if let Some(id) = project.timelines.keys().next().copied() {
        return Link::Timeline { id, chapter: 0 };
    }

    Link::NotSet
}

/// Appends warnings for unreached objects.
fn add_warnings(project: &Project, state: &ReachState, problems: &mut Vec<Problem>) {
    for timeline in project.timelines.values() {
        if !state.timelines.contains(&timeline.id) {
            problems.push(warning(
                "unreachableTimeline",
                format!("timeline {} is unreachable", timeline.id.0),
            ));
        }
    }

    for menu in project.menus.values() {
        if !state.menus.contains(&menu.id) {
            problems.push(warning(
                "unreachableMenu",
                format!("menu {} is unreachable", menu.id.0),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use disccraft_project::{Link, Problem, Project, Severity};
    use std::error::Error;
    use std::path::Path;

    fn has_warning_code(problems: &[Problem], code: &str) -> bool {
        problems
            .iter()
            .any(|problem| problem.code == code && problem.severity == Severity::Warning)
    }

    fn has_warning_message(problems: &[Problem], code: &str, needle: &str) -> bool {
        problems
            .iter()
            .any(|problem| problem.code == code && problem.message.contains(needle))
    }

    #[test]
    fn unreachable_timeline_when_menu_links_to_one() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(Path::new("movie.mp4"))?;
        let first = project.add_timeline(asset, Some("First".to_string()))?;
        let second = project.add_timeline(asset, Some("Second".to_string()))?;
        let menu = project.add_menu(Some("Main".to_string()))?;

        project.settings.first_play = Link::Menu {
            id: menu,
            button: None,
        };
        project.add_button(
            menu,
            "Play".to_string(),
            None,
            Link::Timeline {
                id: first,
                chapter: 0,
            },
        )?;

        let mut problems = Vec::new();
        check(&project, &mut problems);

        assert!(has_warning_message(
            &problems,
            "unreachableTimeline",
            &format!("timeline {}", second.0)
        ));
        assert!(!has_warning_code(&problems, "unreachableMenu"));
        Ok(())
    }

    #[test]
    fn unreachable_menu_when_no_links_to_it() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(Path::new("movie.mp4"))?;
        let timeline = project.add_timeline(asset, None)?;
        let menu = project.add_menu(Some("Extra".to_string()))?;

        project.settings.first_play = Link::Timeline {
            id: timeline,
            chapter: 0,
        };

        let mut problems = Vec::new();
        check(&project, &mut problems);

        assert!(has_warning_message(
            &problems,
            "unreachableMenu",
            &format!("menu {}", menu.0)
        ));
        assert!(!has_warning_code(&problems, "unreachableTimeline"));
        Ok(())
    }

    #[test]
    fn title_remote_makes_menu_reachable() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(Path::new("movie.mp4"))?;
        let timeline = project.add_timeline(asset, None)?;
        let menu = project.add_menu(Some("Title".to_string()))?;

        project.settings.first_play = Link::Timeline {
            id: timeline,
            chapter: 0,
        };
        project.settings.title_remote = Link::Menu {
            id: menu,
            button: None,
        };

        let mut problems = Vec::new();
        check(&project, &mut problems);

        assert!(!has_warning_code(&problems, "unreachableMenu"));
        assert!(!has_warning_code(&problems, "unreachableTimeline"));
        Ok(())
    }

    /// M6.557: a playlist reaches the timelines in it and its end action; a chapter playlist and a slideshow reach
    /// what their end actions (and a chapter playlist's override) go to.
    #[test]
    fn reachability_follows_playlists() -> Result<(), Box<dyn Error>> {
        use disccraft_project::PlaylistItem;
        let mut project = Project::default();
        let asset = project.add_asset(Path::new("movie.mp4"))?;
        let first = project.add_timeline(asset, Some("First".to_string()))?;
        let second = project.add_timeline(asset, Some("Second".to_string()))?;
        let after = project.add_timeline(asset, Some("After".to_string()))?;
        let menu = project.add_menu(Some("Menu".to_string()))?;
        let show = project.add_slideshow(None)?;
        let chapters = project.add_chapter_playlist(first, None)?;
        let list = project.add_playlist(None)?;
        for item in [
            PlaylistItem::Timeline(first),
            PlaylistItem::Slideshow(show),
            PlaylistItem::ChapterPlaylist(chapters),
            PlaylistItem::Timeline(second),
        ] {
            project.add_playlist_entry(list, item, None)?;
        }
        project.settings.first_play = Link::Playlist { id: list };
        // Only the slideshow's end action leads to the menu, only the chapter playlist's to the last timeline.
        project
            .slideshows
            .get_mut(&show)
            .ok_or("missing slideshow")?
            .end_action = Link::Menu {
            id: menu,
            button: None,
        };
        project
            .chapter_playlists
            .get_mut(&chapters)
            .ok_or("missing chapter playlist")?
            .end_action = Link::Timeline {
            id: after,
            chapter: 0,
        };

        let mut problems = Vec::new();
        check(&project, &mut problems);

        assert!(
            !has_warning_code(&problems, "unreachableTimeline"),
            "{problems:?}"
        );
        assert!(
            !has_warning_code(&problems, "unreachableMenu"),
            "{problems:?}"
        );
        Ok(())
    }

    #[test]
    fn reachability_follows_end_actions() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(Path::new("movie.mp4"))?;
        let first = project.add_timeline(asset, Some("First".to_string()))?;
        let second = project.add_timeline(asset, Some("Second".to_string()))?;

        project.settings.first_play = Link::Timeline {
            id: first,
            chapter: 0,
        };
        project
            .timeline_mut(first)
            .ok_or("missing timeline")?
            .end_action = Link::Timeline {
            id: second,
            chapter: 0,
        };

        let mut problems = Vec::new();
        check(&project, &mut problems);

        assert!(!has_warning_code(&problems, "unreachableTimeline"));
        Ok(())
    }

    #[test]
    fn not_set_first_play_uses_first_menu() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(Path::new("movie.mp4"))?;
        let timeline = project.add_timeline(asset, None)?;
        let _menu = project.add_menu(Some("Main".to_string()))?;

        let mut problems = Vec::new();
        check(&project, &mut problems);

        assert!(!has_warning_code(&problems, "unreachableMenu"));
        assert!(has_warning_message(
            &problems,
            "unreachableTimeline",
            &format!("timeline {}", timeline.0)
        ));
        Ok(())
    }
}
