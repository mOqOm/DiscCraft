//! Button transitions (Encore's button Transition tab): a video played after a button is activated, before its
//! link. A build expands every such button into a hidden title of the video whose end action is the button's link.

use crate::ids::{AssetId, TimelineId};
use crate::io::ProjectError;
use crate::link::Link;
use crate::model::{AssetKind, Project};

/// Whether `link` plays a title (where an override applies after it ends).
fn plays_title(link: &Link) -> bool {
    matches!(
        link,
        Link::Timeline { .. }
            | Link::Slideshow { .. }
            | Link::Playlist { .. }
            | Link::ChapterPlaylist { .. }
    )
}

impl Project {
    /// Whether any button has a transition.
    pub fn has_button_transitions(&self) -> bool {
        self.menus.values().any(|menu| {
            menu.buttons
                .iter()
                .any(|button| button.transition.is_some())
        })
    }

    /// A copy of the project for building: every button with a transition (a video asset) and a link goes to a
    /// hidden title of that video instead, whose end action is the button's link and which carries the button's
    /// override. Buttons with the same transition, link and override share one title. A transition that is not a
    /// video, or on a button without a link, is left out.
    pub fn with_transitions_expanded(&self) -> Result<Project, ProjectError> {
        let mut out = self.clone();
        if !self.has_button_transitions() {
            return Ok(out);
        }
        let mut made: Vec<((AssetId, Link, Option<Link>), TimelineId)> = Vec::new();
        let menus: Vec<_> = out.menus.keys().copied().collect();
        for menu in menus {
            let count = out.menu(menu).map_or(0, |menu| menu.buttons.len());
            for index in 0..count {
                let Some(button) = out
                    .menu(menu)
                    .and_then(|menu| menu.buttons.get(index))
                    .cloned()
                else {
                    continue;
                };
                let Some(asset) = button.transition else {
                    continue;
                };
                let video = out
                    .assets
                    .get(&asset)
                    .is_some_and(|asset| asset.kind == AssetKind::Video);
                if !video || button.link == Link::NotSet {
                    continue;
                }
                let override_link = button
                    .override_link
                    .clone()
                    .filter(|_| plays_title(&button.link));
                let key = (asset, button.link.clone(), override_link.clone());
                let timeline = match made.iter().find(|(made_key, _)| *made_key == key) {
                    Some((_, id)) => *id,
                    None => {
                        let name = if button.name.trim().is_empty() {
                            "Transition".to_string()
                        } else {
                            format!("Transition: {}", button.name)
                        };
                        let id = out.add_timeline(asset, Some(name))?;
                        if let Some(timeline) = out.timelines.get_mut(&id) {
                            timeline.end_action = button.link.clone();
                            timeline.override_link = override_link.clone();
                        }
                        made.push((key, id));
                        id
                    }
                };
                if let Some(target) = out
                    .menu_mut(menu)
                    .and_then(|menu| menu.buttons.get_mut(index))
                {
                    target.link = Link::Timeline {
                        id: timeline,
                        chapter: 0,
                    };
                    target.override_link = None;
                    target.transition = None;
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use crate::link::Link;
    use crate::model::Project;
    use std::error::Error;
    use std::path::Path;

    #[test]
    fn a_transition_becomes_a_title_that_ends_at_the_link() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let movie = project.add_asset(Path::new("movie.m2v"))?;
        let wipe = project.add_asset(Path::new("wipe.m2v"))?;
        let feature = project.add_timeline(movie, Some("Feature".to_string()))?;
        let menu = project.add_menu(None)?;
        let link = Link::Timeline {
            id: feature,
            chapter: 0,
        };
        project.add_button(menu, "Play".to_string(), None, link.clone())?;
        project.add_button(menu, "Again".to_string(), None, link.clone())?;
        project.add_button(menu, "Plain".to_string(), None, link.clone())?;
        let back = project.add_menu(None)?;
        if let Some(menu) = project.menu_mut(menu) {
            for button in menu.buttons.iter_mut().take(2) {
                button.transition = Some(wipe);
                button.override_link = Some(Link::Menu {
                    id: back,
                    button: None,
                });
            }
        }
        assert!(project.has_button_transitions());
        let built = project.with_transitions_expanded()?;
        // One shared hidden title for the two buttons; the third keeps its link.
        assert_eq!(built.timelines.len(), 2);
        let buttons = &built.menu(menu).ok_or("menu")?.buttons;
        let first = buttons.first().ok_or("button")?;
        let Link::Timeline { id: hidden, .. } = first.link else {
            return Err("the button does not go to the transition".into());
        };
        assert_ne!(hidden, feature);
        assert_eq!(
            buttons.get(1).map(|b| b.link.clone()),
            Some(first.link.clone())
        );
        assert_eq!(buttons.get(2).map(|b| b.link.clone()), Some(link.clone()));
        assert!(first.override_link.is_none() && first.transition.is_none());
        let title = built.timelines.get(&hidden).ok_or("hidden title")?;
        assert_eq!(title.video, wipe);
        assert_eq!(title.end_action, link);
        assert_eq!(
            title.override_link,
            Some(Link::Menu {
                id: back,
                button: None
            })
        );
        // The project itself is untouched.
        assert_eq!(project.timelines.len(), 1);
        Ok(())
    }

    #[test]
    fn a_transition_without_a_link_or_video_is_left_out() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let picture = project.add_asset(Path::new("still.png"))?;
        let wipe = project.add_asset(Path::new("wipe.m2v"))?;
        let menu = project.add_menu(None)?;
        project.add_button(menu, "Nothing".to_string(), None, Link::NotSet)?;
        project.add_button(menu, "Picture".to_string(), None, Link::Stop)?;
        if let Some(menu) = project.menu_mut(menu) {
            if let Some(button) = menu.buttons.get_mut(0) {
                button.transition = Some(wipe);
            }
            if let Some(button) = menu.buttons.get_mut(1) {
                button.transition = Some(picture);
            }
        }
        let built = project.with_transitions_expanded()?;
        assert!(built.timelines.is_empty());
        Ok(())
    }
}
