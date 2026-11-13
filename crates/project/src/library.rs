//! DiscCraft's built-in library: original menu, button and background presets defined in code.
//!
//! Every preset here is DiscCraft's own (plain colors and rectangles). No image, name or layout is taken from
//! another product's template library.

use crate::ids::MenuId;
use crate::io::ProjectError;
use crate::link::Link;
use crate::menu::{MenuBackground, Rect, frame_size};
use crate::model::{DiscSettings, Project};
use crate::thumbnail::ThumbnailArea;
use serde::{Deserialize, Serialize};

/// A button of a menu spec, laid out in a 720x480 frame (an owned [`PresetButton`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonSpec {
    /// Visible label.
    pub label: String,
    /// Left edge.
    pub x: u16,
    /// Top edge.
    pub y: u16,
    /// Width.
    pub w: u16,
    /// Height.
    pub h: u16,
    /// The button's video thumbnail, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<ThumbnailArea>,
    /// What the button is for (a chapter index template's chapter buttons, its main / previous / next buttons).
    #[serde(default, skip_serializing_if = "is_normal")]
    pub kind: crate::menu::ButtonKind,
}

fn is_normal(kind: &crate::menu::ButtonKind) -> bool {
    *kind == crate::menu::ButtonKind::Normal
}

/// The kind of a preset button, by its label: "Chapter" and "Chapter N" are chapter buttons and "Main Menu" goes back
/// to the main menu, so the chapter presets work as chapter index templates (Encore's チャプターインデックスを作成).
fn preset_kind(label: &str) -> crate::menu::ButtonKind {
    if label == "Chapter" || label.starts_with("Chapter ") {
        crate::menu::ButtonKind::Chapter
    } else if label == "Main Menu" {
        crate::menu::ButtonKind::Main
    } else {
        crate::menu::ButtonKind::Normal
    }
}

impl ButtonSpec {
    pub(crate) fn preset(&self) -> PresetButton {
        PresetButton {
            label: "",
            x: self.x,
            y: self.y,
            w: self.w,
            h: self.h,
            thumbnail: self.thumbnail,
        }
    }
}

/// A whole menu as a library item: colours and buttons (an owned menu [`LibraryItem`], also what a user saves from
/// an open menu).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuSpec {
    /// Background colour.
    pub background: [u8; 3],
    /// Text colour.
    pub text: [u8; 3],
    /// Highlight colour.
    pub highlight: [u8; 3],
    /// Buttons in a 720x480 frame.
    pub buttons: Vec<ButtonSpec>,
}

impl From<&LibraryItem> for MenuSpec {
    fn from(item: &LibraryItem) -> Self {
        MenuSpec {
            background: item.background,
            text: item.text,
            highlight: item.highlight,
            buttons: item
                .buttons
                .iter()
                .map(|button| ButtonSpec {
                    label: button.label.to_owned(),
                    x: button.x,
                    y: button.y,
                    w: button.w,
                    h: button.h,
                    thumbnail: button.thumbnail,
                    kind: preset_kind(button.label),
                })
                .collect(),
        }
    }
}

/// What a library item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibraryKind {
    /// A complete menu preset.
    Menu,
    /// A single button preset.
    Button,
    /// A solid background color preset.
    Background,
}

/// A button of a preset, laid out in a 720x480 frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresetButton {
    /// Visible label.
    pub label: &'static str,
    /// Left edge.
    pub x: u16,
    /// Top edge.
    pub y: u16,
    /// Width.
    pub w: u16,
    /// Height.
    pub h: u16,
    /// The button's video thumbnail (the top three quarters of a thumbnail button), when it has one.
    pub thumbnail: Option<ThumbnailArea>,
}

/// The upper three quarters of a button: the video thumbnail of the thumbnail presets (the text goes below).
const UPPER_THUMBNAIL: Option<ThumbnailArea> = Some(ThumbnailArea {
    x: 0,
    y: 0,
    w: crate::thumbnail::THUMBNAIL_PARTS,
    h: crate::thumbnail::THUMBNAIL_PARTS / 4 * 3,
});

/// One library item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LibraryItem {
    /// Stable id, e.g. "menu.plain-dark".
    pub id: &'static str,
    /// English display name (the GUI translates it).
    pub name: &'static str,
    /// What kind of item this is.
    pub kind: LibraryKind,
    /// Menu background color, button preview fill, or background color.
    pub background: [u8; 3],
    /// Text color.
    pub text: [u8; 3],
    /// Highlight color.
    pub highlight: [u8; 3],
    /// Menu buttons, the single button of a button preset, or empty for a background preset.
    pub buttons: &'static [PresetButton],
}

/// The built-in library.
pub const LIBRARY: &[LibraryItem] = &[
    LibraryItem {
        id: "menu.plain-dark",
        name: "Plain Dark",
        kind: LibraryKind::Menu,
        background: [16, 20, 24],
        text: [232, 232, 232],
        highlight: [255, 200, 60],
        buttons: &[
            PresetButton {
                label: "Play Movie",
                x: 200,
                y: 150,
                w: 320,
                h: 44,
                thumbnail: None,
            },
            PresetButton {
                label: "Chapters",
                x: 200,
                y: 210,
                w: 320,
                h: 44,
                thumbnail: None,
            },
            PresetButton {
                label: "Extras",
                x: 200,
                y: 270,
                w: 320,
                h: 44,
                thumbnail: None,
            },
            PresetButton {
                label: "Setup",
                x: 200,
                y: 330,
                w: 320,
                h: 44,
                thumbnail: None,
            },
        ],
    },
    LibraryItem {
        id: "menu.plain-light",
        name: "Plain Light",
        kind: LibraryKind::Menu,
        background: [233, 228, 216],
        text: [32, 32, 32],
        highlight: [192, 57, 43],
        buttons: &[
            PresetButton {
                label: "Play Movie",
                x: 200,
                y: 170,
                w: 320,
                h: 44,
                thumbnail: None,
            },
            PresetButton {
                label: "Chapters",
                x: 200,
                y: 230,
                w: 320,
                h: 44,
                thumbnail: None,
            },
            PresetButton {
                label: "Extras",
                x: 200,
                y: 290,
                w: 320,
                h: 44,
                thumbnail: None,
            },
        ],
    },
    LibraryItem {
        id: "menu.bottom-bar",
        name: "Bottom Bar",
        kind: LibraryKind::Menu,
        background: [0, 0, 0],
        text: [240, 240, 240],
        highlight: [80, 180, 255],
        buttons: &[
            PresetButton {
                label: "Play",
                x: 60,
                y: 400,
                w: 180,
                h: 40,
                thumbnail: None,
            },
            PresetButton {
                label: "Chapters",
                x: 270,
                y: 400,
                w: 180,
                h: 40,
                thumbnail: None,
            },
            PresetButton {
                label: "Extras",
                x: 480,
                y: 400,
                w: 180,
                h: 40,
                thumbnail: None,
            },
        ],
    },
    LibraryItem {
        id: "menu.chapter-grid",
        name: "Chapter Grid",
        kind: LibraryKind::Menu,
        background: [20, 33, 61],
        text: [255, 255, 255],
        highlight: [252, 163, 17],
        buttons: &[
            PresetButton {
                label: "Chapter 1",
                x: 80,
                y: 110,
                w: 260,
                h: 60,
                thumbnail: None,
            },
            PresetButton {
                label: "Chapter 2",
                x: 380,
                y: 110,
                w: 260,
                h: 60,
                thumbnail: None,
            },
            PresetButton {
                label: "Chapter 3",
                x: 80,
                y: 200,
                w: 260,
                h: 60,
                thumbnail: None,
            },
            PresetButton {
                label: "Chapter 4",
                x: 380,
                y: 200,
                w: 260,
                h: 60,
                thumbnail: None,
            },
            PresetButton {
                label: "Chapter 5",
                x: 80,
                y: 290,
                w: 260,
                h: 60,
                thumbnail: None,
            },
            PresetButton {
                label: "Chapter 6",
                x: 380,
                y: 290,
                w: 260,
                h: 60,
                thumbnail: None,
            },
            PresetButton {
                label: "Main Menu",
                x: 260,
                y: 390,
                w: 200,
                h: 40,
                thumbnail: None,
            },
        ],
    },
    LibraryItem {
        id: "menu.thumbnail-grid",
        name: "Thumbnail Grid",
        kind: LibraryKind::Menu,
        background: [24, 24, 28],
        text: [235, 235, 235],
        highlight: [80, 170, 255],
        buttons: &[
            PresetButton {
                label: "Chapter 1",
                x: 90,
                y: 70,
                w: 240,
                h: 160,
                thumbnail: UPPER_THUMBNAIL,
            },
            PresetButton {
                label: "Chapter 2",
                x: 390,
                y: 70,
                w: 240,
                h: 160,
                thumbnail: UPPER_THUMBNAIL,
            },
            PresetButton {
                label: "Chapter 3",
                x: 90,
                y: 250,
                w: 240,
                h: 160,
                thumbnail: UPPER_THUMBNAIL,
            },
            PresetButton {
                label: "Chapter 4",
                x: 390,
                y: 250,
                w: 240,
                h: 160,
                thumbnail: UPPER_THUMBNAIL,
            },
        ],
    },
    LibraryItem {
        id: "menu.left-list",
        name: "Left List",
        kind: LibraryKind::Menu,
        background: [38, 38, 38],
        text: [220, 220, 220],
        highlight: [120, 200, 120],
        buttons: &[
            PresetButton {
                label: "Play Movie",
                x: 60,
                y: 120,
                w: 260,
                h: 40,
                thumbnail: None,
            },
            PresetButton {
                label: "Scene Selection",
                x: 60,
                y: 170,
                w: 260,
                h: 40,
                thumbnail: None,
            },
            PresetButton {
                label: "Audio",
                x: 60,
                y: 220,
                w: 260,
                h: 40,
                thumbnail: None,
            },
            PresetButton {
                label: "Subtitles",
                x: 60,
                y: 270,
                w: 260,
                h: 40,
                thumbnail: None,
            },
            PresetButton {
                label: "Extras",
                x: 60,
                y: 320,
                w: 260,
                h: 40,
                thumbnail: None,
            },
        ],
    },
    LibraryItem {
        id: "button.wide",
        name: "Wide Button",
        kind: LibraryKind::Button,
        background: [60, 60, 60],
        text: [255, 255, 255],
        highlight: [255, 208, 0],
        buttons: &[PresetButton {
            label: "Button",
            x: 0,
            y: 0,
            w: 320,
            h: 44,
            thumbnail: None,
        }],
    },
    LibraryItem {
        id: "button.small",
        name: "Small Button",
        kind: LibraryKind::Button,
        background: [60, 60, 60],
        text: [255, 255, 255],
        highlight: [255, 208, 0],
        buttons: &[PresetButton {
            label: "Button",
            x: 0,
            y: 0,
            w: 180,
            h: 36,
            thumbnail: None,
        }],
    },
    LibraryItem {
        id: "button.tile",
        name: "Tile",
        kind: LibraryKind::Button,
        background: [60, 60, 60],
        text: [255, 255, 255],
        highlight: [255, 208, 0],
        buttons: &[PresetButton {
            label: "Chapter",
            x: 0,
            y: 0,
            w: 160,
            h: 120,
            thumbnail: None,
        }],
    },
    LibraryItem {
        id: "button.thumbnail",
        name: "Video Thumbnail",
        kind: LibraryKind::Button,
        background: [60, 60, 60],
        text: [255, 255, 255],
        highlight: [255, 208, 0],
        buttons: &[PresetButton {
            label: "Chapter",
            x: 0,
            y: 0,
            w: 200,
            h: 150,
            thumbnail: UPPER_THUMBNAIL,
        }],
    },
    LibraryItem {
        id: "button.bar",
        name: "Full-Width Bar",
        kind: LibraryKind::Button,
        background: [60, 60, 60],
        text: [255, 255, 255],
        highlight: [255, 208, 0],
        buttons: &[PresetButton {
            label: "Button",
            x: 0,
            y: 0,
            w: 600,
            h: 40,
            thumbnail: None,
        }],
    },
    LibraryItem {
        id: "background.black",
        name: "Black",
        kind: LibraryKind::Background,
        background: [0, 0, 0],
        text: [255, 255, 255],
        highlight: [255, 208, 0],
        buttons: &[],
    },
    LibraryItem {
        id: "background.charcoal",
        name: "Charcoal",
        kind: LibraryKind::Background,
        background: [40, 40, 40],
        text: [255, 255, 255],
        highlight: [255, 208, 0],
        buttons: &[],
    },
    LibraryItem {
        id: "background.navy",
        name: "Navy",
        kind: LibraryKind::Background,
        background: [16, 32, 72],
        text: [255, 255, 255],
        highlight: [255, 208, 0],
        buttons: &[],
    },
    LibraryItem {
        id: "background.forest",
        name: "Forest",
        kind: LibraryKind::Background,
        background: [20, 60, 36],
        text: [255, 255, 255],
        highlight: [255, 208, 0],
        buttons: &[],
    },
    LibraryItem {
        id: "background.wine",
        name: "Wine",
        kind: LibraryKind::Background,
        background: [90, 20, 40],
        text: [255, 255, 255],
        highlight: [255, 208, 0],
        buttons: &[],
    },
    LibraryItem {
        id: "background.ivory",
        name: "Ivory",
        kind: LibraryKind::Background,
        background: [236, 232, 220],
        text: [255, 255, 255],
        highlight: [255, 208, 0],
        buttons: &[],
    },
];

/// The library item with `id`.
pub fn library_item(id: &str) -> Option<&'static LibraryItem> {
    LIBRARY.iter().find(|item| item.id == id)
}

/// `button`'s rectangle in the project's frame: x and w unchanged, y and h scaled by frame_height / 480
/// (u32 math, rounded down), then clamped so the rectangle stays inside the frame (w, h at least 1).
pub fn preset_rect(button: &PresetButton, settings: &DiscSettings) -> Rect {
    if crate::hd_frame::hd_frame(settings) {
        return crate::hd_frame::design_rect(
            settings,
            Rect {
                x: button.x,
                y: button.y,
                w: button.w,
                h: button.h,
            },
        );
    }
    let (frame_w, frame_h) = frame_size(settings);
    let frame_w = u32::from(frame_w);
    let frame_h = u32::from(frame_h);

    let x = u32::from(button.x);
    let y = u32::from(button.y).saturating_mul(frame_h) / 480;
    let w = u32::from(button.w);
    let h = u32::from(button.h).saturating_mul(frame_h) / 480;

    clamp_rect(x, y, w, h, frame_w, frame_h)
}

/// Builds an invalid-project error.
fn invalid(message: &str) -> ProjectError {
    ProjectError::Invalid(message.to_string())
}

/// Returns a menu preset by id.
fn preset_menu(id: &str) -> Result<&'static LibraryItem, ProjectError> {
    let item = library_item(id).ok_or_else(|| invalid("unknown library id"))?;
    if item.kind != LibraryKind::Menu {
        return Err(invalid("library item is not a menu"));
    }
    Ok(item)
}

/// Returns the single button of a button preset by id.
fn preset_button(id: &str) -> Result<&'static PresetButton, ProjectError> {
    let item = library_item(id).ok_or_else(|| invalid("unknown library id"))?;
    if item.kind != LibraryKind::Button {
        return Err(invalid("library item is not a button"));
    }
    item.buttons
        .first()
        .ok_or_else(|| invalid("button preset has no button"))
}

/// Returns a background preset by id.
fn preset_background(id: &str) -> Result<&'static LibraryItem, ProjectError> {
    let item = library_item(id).ok_or_else(|| invalid("unknown library id"))?;
    if item.kind != LibraryKind::Background {
        return Err(invalid("library item is not a background"));
    }
    Ok(item)
}

/// Clamps a rectangle into the frame, keeping width and height at least 1.
fn clamp_rect(x: u32, y: u32, w: u32, h: u32, frame_w: u32, frame_h: u32) -> Rect {
    let w = w.max(1);
    let h = h.max(1);

    let x = x.min(frame_w.saturating_sub(1));
    let y = y.min(frame_h.saturating_sub(1));

    let w = w.min(frame_w.saturating_sub(x).max(1));
    let h = h.min(frame_h.saturating_sub(y).max(1));

    Rect {
        x: u16::try_from(x).unwrap_or(0),
        y: u16::try_from(y).unwrap_or(0),
        w: u16::try_from(w).unwrap_or(1),
        h: u16::try_from(h).unwrap_or(1),
    }
}

impl Project {
    /// New menu from a menu preset. Name: `name` or the preset name. Background `MenuBackground::Color` of the
    /// preset, text and highlight colors, one button per preset button (label = `labels[i]` when present and
    /// non-empty, else the preset label; rect = `preset_rect`; link `Link::NotSet`), default button 0.
    /// Errors for an unknown id or a preset that is not a menu.
    pub fn add_menu_from_preset(
        &mut self,
        id: &str,
        name: Option<String>,
        labels: &[String],
    ) -> Result<MenuId, ProjectError> {
        let preset = preset_menu(id)?;

        let menu_name = match name {
            Some(value) => value,
            None => preset.name.to_string(),
        };
        self.add_menu_from_spec(menu_name, &MenuSpec::from(preset), labels)
    }

    /// New menu from a menu spec (a user's library item): like [`Self::add_menu_from_preset`], with the spec's
    /// colours and buttons.
    pub fn add_menu_from_spec(
        &mut self,
        name: String,
        spec: &MenuSpec,
        labels: &[String],
    ) -> Result<MenuId, ProjectError> {
        let menu_id = self.add_menu(Some(name))?;
        {
            let menu = self
                .menu_mut(menu_id)
                .ok_or_else(|| invalid("menu does not exist"))?;
            menu.background = MenuBackground::Color {
                color: spec.background,
            };
            menu.text_color = spec.text;
            menu.highlight_color = spec.highlight;
            menu.buttons.clear();
            menu.default_button = 0;
        }
        for (index, button) in spec.buttons.iter().enumerate() {
            let label = match labels.get(index) {
                Some(value) if !value.is_empty() => value.clone(),
                _ => button.label.clone(),
            };
            let rect = preset_rect(&button.preset(), &self.settings);
            let index = self.add_button(menu_id, label, Some(rect), Link::NotSet)?;
            self.set_button_thumbnail(menu_id, index, button.thumbnail);
            self.set_button_kind(menu_id, index, button.kind);
        }
        Ok(menu_id)
    }

    /// The open menu as a spec (what ライブラリ ▸ 項目を追加 saves): its colours (an image or video background
    /// counts as black) and its buttons, scaled back into a 720x480 frame.
    pub fn menu_spec(&self, menu: MenuId) -> Option<MenuSpec> {
        let menu = self.menu(menu)?;
        let background = match menu.background {
            MenuBackground::Color { color } => color,
            MenuBackground::Image { .. } | MenuBackground::Video { .. } => [0, 0, 0],
        };
        Some(MenuSpec {
            background,
            text: menu.text_color,
            highlight: menu.highlight_color,
            buttons: menu
                .buttons
                .iter()
                .map(|button| self.button_spec_of(button))
                .collect(),
        })
    }

    /// One button of `menu` as a spec (its label and rectangle in a 720x480 frame).
    pub fn button_spec(&self, menu: MenuId, index: usize) -> Option<ButtonSpec> {
        let button = self.menu(menu)?.buttons.get(index)?;
        Some(self.button_spec_of(button))
    }

    /// Gives a button its video thumbnail area (a spec's).
    fn set_button_thumbnail(
        &mut self,
        menu: MenuId,
        index: usize,
        thumbnail: Option<ThumbnailArea>,
    ) {
        if let Some(button) = self
            .menu_mut(menu)
            .and_then(|menu| menu.buttons.get_mut(index))
        {
            button.thumbnail = thumbnail;
        }
    }

    /// A button as a spec: its label, its rectangle in a 720x480 frame (the inverse of [`preset_rect`]) and its
    /// thumbnail.
    fn button_spec_of(&self, button: &crate::menu::Button) -> ButtonSpec {
        let (label, rect) = (button.label.as_str(), button.rect);
        let (_, frame_h) = frame_size(&self.settings);
        let scale = |value: u16| -> u16 {
            let scaled = u32::from(value).saturating_mul(480) / u32::from(frame_h.max(1));
            u16::try_from(scaled).unwrap_or(u16::MAX)
        };
        ButtonSpec {
            label: label.to_owned(),
            x: rect.x,
            y: scale(rect.y),
            w: rect.w,
            h: scale(rect.h).max(1),
            thumbnail: button.thumbnail,
            kind: button.kind,
        }
    }

    /// Adds a button preset to `menu`, centered horizontally ((frame_w - w) / 2), its top 16 px below the lowest
    /// bottom edge of the existing buttons (or at (frame_h - h) / 2 when the menu has none), clamped inside the
    /// frame. Label: `label` when Some and non-empty, else the preset label. Returns the new button index.
    /// Errors for unknown id, wrong kind, or a missing menu.
    pub fn place_preset_button(
        &mut self,
        menu: MenuId,
        id: &str,
        label: Option<String>,
    ) -> Result<usize, ProjectError> {
        let button = preset_button(id)?;
        let spec = ButtonSpec {
            label: button.label.to_owned(),
            x: button.x,
            y: button.y,
            w: button.w,
            h: button.h,
            thumbnail: button.thumbnail,
            kind: preset_kind(button.label),
        };
        self.place_button_spec(menu, &spec, label)
    }

    /// Adds a button spec (a user's library button) to `menu`, placed like [`Self::place_preset_button`].
    pub fn place_button_spec(
        &mut self,
        menu: MenuId,
        button: &ButtonSpec,
        label: Option<String>,
    ) -> Result<usize, ProjectError> {
        let base = preset_rect(&button.preset(), &self.settings);

        let (frame_w, frame_h) = frame_size(&self.settings);
        let frame_w = u32::from(frame_w);
        let frame_h = u32::from(frame_h);

        let w = u32::from(base.w);
        let h = u32::from(base.h);
        let x = frame_w.saturating_sub(w) / 2;
        // Below the last button, but never past the bottom of the frame (keep the full height).
        let y = self
            .next_button_y(menu, frame_h, h)?
            .min(frame_h.saturating_sub(h));
        let rect = clamp_rect(x, y, w, h, frame_w, frame_h);

        let label = match label {
            Some(value) if !value.is_empty() => value,
            _ => button.label.clone(),
        };

        let index = self.add_button(menu, label, Some(rect), Link::NotSet)?;
        self.set_button_thumbnail(menu, index, button.thumbnail);
        self.set_button_kind(menu, index, button.kind);
        Ok(index)
    }

    /// Gives a button just added its kind (a spec's).
    fn set_button_kind(&mut self, menu: MenuId, index: usize, kind: crate::menu::ButtonKind) {
        if let Some(button) = self
            .menu_mut(menu)
            .and_then(|menu| menu.buttons.get_mut(index))
        {
            button.kind = kind;
        }
    }

    /// Sets `menu`'s background to the color of a background preset. Errors for unknown id, wrong kind or missing menu.
    pub fn apply_background_preset(&mut self, menu: MenuId, id: &str) -> Result<(), ProjectError> {
        let preset = preset_background(id)?;

        let menu = self
            .menu_mut(menu)
            .ok_or_else(|| invalid("menu does not exist"))?;

        menu.background = MenuBackground::Color {
            color: preset.background,
        };

        Ok(())
    }

    /// Computes the next button top edge for a menu.
    fn next_button_y(&self, menu: MenuId, frame_h: u32, h: u32) -> Result<u32, ProjectError> {
        let menu = self
            .menu(menu)
            .ok_or_else(|| invalid("menu does not exist"))?;

        if menu.buttons.is_empty() {
            return Ok(frame_h.saturating_sub(h) / 2);
        }

        let mut max_bottom = 0u32;
        for button in &menu.buttons {
            let bottom = u32::from(button.rect.y).saturating_add(u32::from(button.rect.h));
            if bottom > max_bottom {
                max_bottom = bottom;
            }
        }

        Ok(max_bottom.saturating_add(16))
    }
}

#[cfg(test)]
mod tests {
    use super::{LIBRARY, LibraryKind};
    use crate::menu::{MAX_BUTTONS, MenuBackground};
    use crate::model::{Project, VideoStandard};
    use std::error::Error;

    fn test_err(message: &str) -> Box<dyn Error> {
        message.into()
    }

    #[test]
    fn library_presets_are_consistent() -> Result<(), Box<dyn Error>> {
        let mut seen: Vec<&'static str> = Vec::new();

        for item in LIBRARY {
            if seen.contains(&item.id) {
                return Err(test_err("duplicate library id"));
            }
            seen.push(item.id);

            match item.kind {
                LibraryKind::Menu => {
                    if item.buttons.len() > MAX_BUTTONS {
                        return Err(test_err("menu preset has too many buttons"));
                    }

                    for button in item.buttons {
                        let right = u32::from(button.x)
                            .checked_add(u32::from(button.w))
                            .ok_or_else(|| test_err("button width overflow"))?;
                        let bottom = u32::from(button.y)
                            .checked_add(u32::from(button.h))
                            .ok_or_else(|| test_err("button height overflow"))?;

                        if right > 720 || bottom > 480 {
                            return Err(test_err("button preset is outside 720x480"));
                        }
                    }
                }
                LibraryKind::Button => {
                    if item.buttons.len() != 1 {
                        return Err(test_err("button preset must have one button"));
                    }
                }
                LibraryKind::Background => {
                    if !item.buttons.is_empty() {
                        return Err(test_err("background preset must have no buttons"));
                    }
                }
            }
        }

        Ok(())
    }

    /// A menu round-trips through its spec: saving the open menu and making a new one from it gives the same
    /// colours, labels and rectangles (PAL buttons come back to their PAL size through the 480-line spec).
    #[test]
    fn menu_specs_round_trip() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        project.settings.standard = VideoStandard::Pal;
        let menu = project.add_menu_from_preset("menu.plain-dark", None, &[])?;
        let spec = project.menu_spec(menu).ok_or_else(|| test_err("spec"))?;
        assert_eq!(spec.background, [16, 20, 24]);
        assert_eq!(spec.buttons.len(), 4);
        assert_eq!(
            spec.buttons.first().map(|b| b.label.as_str()),
            Some("Play Movie")
        );

        let copy = project.add_menu_from_spec("Copy".to_owned(), &spec, &[])?;
        let original = project.menu(menu).ok_or_else(|| test_err("menu"))?.clone();
        let copied = project.menu(copy).ok_or_else(|| test_err("copy"))?;
        assert_eq!(copied.text_color, original.text_color);
        for (a, b) in original.buttons.iter().zip(&copied.buttons) {
            assert_eq!(a.label, b.label);
            // Within a line of rounding through the 480-line spec.
            assert!(a.rect.y.abs_diff(b.rect.y) <= 2 && a.rect.h.abs_diff(b.rect.h) <= 2);
            assert_eq!((a.rect.x, a.rect.w), (b.rect.x, b.rect.w));
        }

        let button = project
            .button_spec(menu, 1)
            .ok_or_else(|| test_err("button spec"))?;
        let placed = project.place_button_spec(copy, &button, None)?;
        assert_eq!(placed, 4);
        assert_eq!(
            project
                .menu(copy)
                .and_then(|m| m.buttons.get(4))
                .map(|b| b.label.as_str()),
            Some("Chapters")
        );
        assert!(project.button_spec(menu, 9).is_none());
        Ok(())
    }

    #[test]
    fn add_menu_from_preset_creates_buttons() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();

        let menu_id = project.add_menu_from_preset("menu.plain-dark", None, &[])?;
        {
            let menu = project
                .menu(menu_id)
                .ok_or_else(|| test_err("missing menu"))?;

            assert_eq!(menu.name, "Plain Dark");
            assert_eq!(menu.buttons.len(), 4);
            assert_eq!(
                menu.background,
                MenuBackground::Color {
                    color: [16, 20, 24]
                }
            );
            assert_eq!(menu.text_color, [232, 232, 232]);
            assert_eq!(menu.highlight_color, [255, 200, 60]);
            assert_eq!(
                menu.buttons
                    .first()
                    .ok_or_else(|| test_err("missing button"))?
                    .label,
                "Play Movie"
            );
            assert_eq!(
                menu.buttons
                    .get(1)
                    .ok_or_else(|| test_err("missing button"))?
                    .label,
                "Chapters"
            );
        }

        let labels = ["Start".to_owned()];
        let menu_id =
            project.add_menu_from_preset("menu.plain-dark", Some("Custom".to_owned()), &labels)?;
        {
            let menu = project
                .menu(menu_id)
                .ok_or_else(|| test_err("missing menu"))?;

            assert_eq!(menu.name, "Custom");
            assert_eq!(
                menu.buttons
                    .first()
                    .ok_or_else(|| test_err("missing button"))?
                    .label,
                "Start"
            );
            assert_eq!(
                menu.buttons
                    .get(1)
                    .ok_or_else(|| test_err("missing button"))?
                    .label,
                "Chapters"
            );
        }

        Ok(())
    }

    #[test]
    fn preset_rect_scales_for_pal() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        project.settings.standard = VideoStandard::Pal;

        let menu_id = project.add_menu_from_preset("menu.plain-dark", None, &[])?;
        let menu = project
            .menu(menu_id)
            .ok_or_else(|| test_err("missing menu"))?;
        let button = menu
            .buttons
            .first()
            .ok_or_else(|| test_err("missing button"))?;

        assert_eq!(button.rect.x, 200);
        assert_eq!(button.rect.y, 180);
        assert_eq!(button.rect.w, 320);
        assert_eq!(button.rect.h, 52);

        Ok(())
    }

    #[test]
    fn place_preset_button_centers_and_stacks() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;

        let index = project.place_preset_button(menu_id, "button.wide", None)?;
        assert_eq!(index, 0);
        {
            let menu = project
                .menu(menu_id)
                .ok_or_else(|| test_err("missing menu"))?;
            let button = menu
                .buttons
                .first()
                .ok_or_else(|| test_err("missing button"))?;

            assert_eq!(button.rect.x, 200);
            assert_eq!(button.rect.y, 218);
            assert_eq!(button.rect.w, 320);
            assert_eq!(button.rect.h, 44);
            assert_eq!(button.label, "Button");
        }

        let index =
            project.place_preset_button(menu_id, "button.wide", Some("Second".to_owned()))?;
        assert_eq!(index, 1);
        {
            let menu = project
                .menu(menu_id)
                .ok_or_else(|| test_err("missing menu"))?;
            let button = menu
                .buttons
                .get(1)
                .ok_or_else(|| test_err("missing button"))?;

            assert_eq!(button.rect.y, 278);
            assert_eq!(button.label, "Second");
        }

        Ok(())
    }

    #[test]
    fn apply_background_preset_sets_color_and_validates_kind() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;

        project.apply_background_preset(menu_id, "background.navy")?;
        {
            let menu = project
                .menu(menu_id)
                .ok_or_else(|| test_err("missing menu"))?;

            assert_eq!(
                menu.background,
                MenuBackground::Color {
                    color: [16, 32, 72]
                }
            );
        }

        assert!(
            project
                .apply_background_preset(menu_id, "menu.plain-dark")
                .is_err()
        );
        assert!(project.apply_background_preset(menu_id, "unknown").is_err());

        Ok(())
    }
}
