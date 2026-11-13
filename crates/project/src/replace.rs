//! Encore's メニューを置き換え: another design (a library menu or a layered PSD) for a menu that keeps the menu's
//! settings. The background, the text objects, the pictures and shapes, and the buttons' places and looks come
//! from the new design; the menu's own settings and its buttons' links (by button number) stay. When the menu has
//! more buttons than the design, the extra buttons stay as they are, in the design's plain button style.

use crate::ids::MenuId;
use crate::io::ProjectError;
use crate::library::{MenuSpec, preset_rect};
use crate::link::Link;
use crate::menu::{MenuBackground, Orientation, Rect, TextFormat};
use crate::model::Project;

/// The look of a menu that replaces another one's.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuDesign {
    /// The new background.
    pub background: MenuBackground,
    /// Text and highlight colours, when the design has its own (a library menu).
    pub colors: Option<([u8; 3], [u8; 3])>,
    /// The design's buttons, in number order.
    pub buttons: Vec<DesignButton>,
    /// The colours of the design's subpicture layers (a PSD's), for the automatic colour set.
    pub subpicture_colors: [Option<[u8; 3]>; 3],
}

/// One button of a [`MenuDesign`].
#[derive(Clone, Debug, PartialEq)]
pub struct DesignButton {
    /// Where the button is, in the project's frame.
    pub rect: Rect,
    /// The button's name in the design (a PSD's button layer), when it has one.
    pub name: Option<String>,
    /// The button text: `None` keeps the menu's (a library menu's text is a placeholder), `Some("")` when the
    /// text is part of the picture (a PSD).
    pub label: Option<String>,
    /// The design's video thumbnail for the button (a PSD's `(%)` layer, a library thumbnail button).
    pub thumbnail: Option<crate::thumbnail::ThumbnailArea>,
    /// The design's subpicture for the button (a PSD's `(=1)` / `(=2)` / `(=3)` layers).
    pub subpicture: Option<crate::subpicture::Subpicture>,
}

impl Project {
    /// A library menu (a preset or a user's item) as a design: its colours and its buttons in this project's frame.
    /// The buttons keep the menu's texts.
    pub fn menu_design_from_spec(&self, spec: &MenuSpec) -> MenuDesign {
        MenuDesign {
            background: MenuBackground::Color {
                color: spec.background,
            },
            colors: Some((spec.text, spec.highlight)),
            buttons: spec
                .buttons
                .iter()
                .map(|button| DesignButton {
                    rect: preset_rect(&button.preset(), &self.settings),
                    name: None,
                    label: None,
                    thumbnail: button.thumbnail,
                    subpicture: None,
                })
                .collect(),
            subpicture_colors: [None; 3],
        }
    }

    /// Replaces `menu`'s design (Encore's メニューを置き換え). Both the menu and the design need at least one button,
    /// as in Encore.
    pub fn replace_menu(&mut self, menu: MenuId, design: MenuDesign) -> Result<(), ProjectError> {
        let current = self
            .menu(menu)
            .ok_or_else(|| invalid("menu does not exist"))?;
        if current.buttons.is_empty() {
            return Err(invalid("the menu has no buttons to replace"));
        }
        if design.buttons.is_empty() {
            return Err(invalid("the replacing menu has no buttons"));
        }
        let kept = current.buttons.len();

        let target = self
            .menu_mut(menu)
            .ok_or_else(|| invalid("menu does not exist"))?;
        target.background = design.background;
        if let Some((text, highlight)) = design.colors {
            target.text_color = text;
            target.highlight_color = highlight;
        }
        target.subpicture_colors = design.subpicture_colors;
        // The old design's objects go with it.
        target.texts.clear();
        target.graphics.clear();
        for button in &mut target.buttons {
            // The new design's plain button style; routes follow the new places.
            button.label_format = TextFormat::default();
            button.label_font_style = None;
            button.shadow = None;
            button.orientation = Orientation::default();
            button.routes = None;
        }
        for button in &mut target.buttons {
            button.thumbnail = None;
            button.subpicture = None;
        }
        for (button, new) in target.buttons.iter_mut().zip(&design.buttons) {
            button.rect = new.rect;
            button.thumbnail = new.thumbnail;
            button.subpicture.clone_from(&new.subpicture);
            if let Some(label) = &new.label {
                button.label.clone_from(label);
            }
            if let Some(name) = new.name.as_ref().filter(|name| !name.is_empty()) {
                button.name.clone_from(name);
            }
        }
        // More buttons in the design: they come without links.
        for new in design.buttons.iter().skip(kept) {
            let index = self.add_button(
                menu,
                new.label.clone().unwrap_or_default(),
                Some(new.rect),
                Link::NotSet,
            )?;
            if let Some(button) = self
                .menu_mut(menu)
                .and_then(|menu| menu.buttons.get_mut(index))
            {
                if let Some(name) = new.name.as_ref().filter(|name| !name.is_empty()) {
                    button.name.clone_from(name);
                }
                button.thumbnail = new.thumbnail;
                button.subpicture.clone_from(&new.subpicture);
            }
        }
        Ok(())
    }
}

/// Builds an invalid-project error.
fn invalid(message: &str) -> ProjectError {
    ProjectError::Invalid(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::{DesignButton, MenuDesign};
    use crate::{Link, MenuBackground, MenuId, Project, Rect, TimelineId};

    fn rect(x: u16) -> Rect {
        Rect {
            x,
            y: 100,
            w: 120,
            h: 40,
        }
    }

    fn menu_with_links(project: &mut Project, links: usize) -> Result<MenuId, crate::ProjectError> {
        let menu = project.add_menu(Some("Main".to_owned()))?;
        for index in 0..links {
            let _ = project.add_button(
                menu,
                format!("Button {index}"),
                Some(rect(10)),
                Link::Timeline {
                    id: TimelineId(u64::try_from(index).unwrap_or(0) + 1),
                    chapter: 0,
                },
            )?;
        }
        Ok(menu)
    }

    fn design(buttons: usize) -> MenuDesign {
        MenuDesign {
            background: MenuBackground::Color { color: [0, 0, 80] },
            colors: Some(([255, 255, 255], [255, 0, 0])),
            subpicture_colors: [None; 3],
            buttons: (0..buttons)
                .map(|index| DesignButton {
                    thumbnail: None,
                    subpicture: None,
                    rect: rect(200 + u16::try_from(index).unwrap_or(0) * 10),
                    name: None,
                    label: None,
                })
                .collect(),
        }
    }

    #[test]
    fn links_stay_with_the_button_numbers() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let menu = menu_with_links(&mut project, 2)?;
        project.replace_menu(menu, design(3))?;
        let replaced = project.menu(menu).ok_or("missing menu")?;
        assert_eq!(
            replaced.background,
            MenuBackground::Color { color: [0, 0, 80] }
        );
        assert_eq!(replaced.buttons.len(), 3);
        let first = replaced.buttons.first().ok_or("no button")?;
        assert_eq!(first.rect.x, 200);
        assert_eq!(first.label, "Button 0");
        assert!(matches!(
            first.link,
            Link::Timeline {
                id: TimelineId(1),
                ..
            }
        ));
        let added = replaced.buttons.get(2).ok_or("no button")?;
        assert_eq!(added.link, Link::NotSet);
        assert_eq!(added.rect.x, 220);
        Ok(())
    }

    #[test]
    fn extra_buttons_stay_and_empty_menus_are_refused() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let menu = menu_with_links(&mut project, 3)?;
        project.replace_menu(menu, design(1))?;
        let replaced = project.menu(menu).ok_or("missing menu")?;
        assert_eq!(replaced.buttons.len(), 3);
        let last = replaced.buttons.get(2).ok_or("no button")?;
        assert_eq!(last.rect.x, 10);
        assert!(matches!(
            last.link,
            Link::Timeline {
                id: TimelineId(3),
                ..
            }
        ));

        assert!(project.replace_menu(menu, design(0)).is_err());
        let empty = project.add_menu(None)?;
        assert!(project.replace_menu(empty, design(1)).is_err());
        Ok(())
    }
}
