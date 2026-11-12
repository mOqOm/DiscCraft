//! Reachability checks for navigation links.

use crate::warning;
use disccraft_project::{Link, MenuId, Problem, Project, TimelineId};
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
    pending: Vec<Link>,
}

impl ReachState {
    /// Creates an empty state.
    fn new() -> Self {
        Self {
            timelines: BTreeSet::new(),
            menus: BTreeSet::new(),
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
            _ => {}
        }
    }

    /// Queues a link if it can lead to a reachable object.
    fn push_pending(&mut self, link: &Link) {
        match link {
            Link::Timeline { .. } | Link::Menu { .. } => self.pending.push(link.clone()),
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
