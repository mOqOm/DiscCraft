//! The Styles panel (Encore: Window ▸ Styles, Ctrl+F2): predefined looks put on the selected object with Apply,
//! kept in sets, with New and Delete.
//!
//! Encore's styles are Photoshop layer styles for images, text and shapes, shown by the panel's Image / Text /
//! Shape buttons (one on at a time). A text style here is the look of text: font, size, colour, character
//! formatting and drop shadow; it goes on a selected text object, or on the text of the selected buttons (whose
//! size, colour and font stay the menu's). An image or shape style (M6.332) is layer effects and a drop shadow; it
//! goes on the selected image or shape. Both through `menu.applyStyle`. The General set is built in (DiscCraft's
//! own); the User set is yours (`styles.json` in the library's folder).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::app::{DiscCraftApp, Selection};
use crate::i18n::{tl, tlf};

use egui::Ui;

/// The most styles the user's set holds.
const MAX_USER_STYLES: usize = 200;

/// One style: its name and the fields `menu.applyStyle` takes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Style {
    pub(crate) name: String,
    pub(crate) style: Value,
}

/// The user's styles, kept in `styles.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct UserStyles {
    #[serde(default)]
    pub(crate) styles: Vec<Style>,
}

fn styles_path() -> Option<std::path::PathBuf> {
    crate::workspace::library_dir().map(|dir| dir.join("styles.json"))
}

impl UserStyles {
    /// Loads the user's styles, or none when there is no file or it cannot be read.
    pub(crate) fn load() -> Self {
        styles_path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .map(|mut found| {
                found.styles.truncate(MAX_USER_STYLES);
                found
            })
            .unwrap_or_default()
    }

    fn save(&self) -> Result<(), String> {
        let path = styles_path().ok_or_else(|| "no library folder".to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        std::fs::write(&path, text).map_err(|error| error.to_string())
    }
}

/// Which styles the panel shows (Encore's Image / Text / Shape buttons, one on at a time): image and shape styles
/// are the same layer effects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum StyleFilter {
    Image,
    #[default]
    Text,
    Shape,
}

impl StyleFilter {
    /// Whether `style` is one this filter shows.
    fn shows(self, style: &Style) -> bool {
        is_graphic_style(style) != (self == StyleFilter::Text)
    }
}

/// An image or shape style: it holds layer effects.
fn is_graphic_style(style: &Style) -> bool {
    style.style.get("effects").is_some()
}

/// The state of the panel.
#[derive(Clone, Debug, Default)]
pub(crate) struct StylesState {
    /// The user's set is shown (else the General one).
    pub(crate) user_set: bool,
    /// Which kind of styles is shown.
    pub(crate) filter: StyleFilter,
    /// The style selected in the list.
    pub(crate) selected: Option<usize>,
    /// The name being typed for a new style, while it is asked for.
    pub(crate) naming: Option<String>,
    /// The user's styles.
    pub(crate) user: UserStyles,
}

/// The built-in General set: names (English; shown translated) and looks.
pub(crate) fn general_styles() -> Vec<Style> {
    let shadow = json!({"color": "#000000", "opacity": 75, "angle": 120, "distance": 5, "size": 5, "spread": 0});
    [
        (
            "Plain",
            json!({"bold": false, "italic": false, "underline": false, "strikethrough": false, "caps": "normal", "tracking": 0, "horizontalScale": 100, "verticalScale": 100, "shadow": null}),
        ),
        ("Bold White", json!({"color": "#FFFFFF", "bold": true})),
        (
            "Title with Shadow",
            json!({"color": "#FFE680", "bold": true, "tracking": 50, "shadow": shadow}),
        ),
        ("Drop Shadow", json!({"shadow": shadow})),
        ("Small Caps", json!({"caps": "small", "tracking": 100})),
        (
            "Wide Capitals",
            json!({"caps": "all", "tracking": 200, "horizontalScale": 110}),
        ),
        (
            "Red Emphasis",
            json!({"color": "#E03030", "bold": true, "italic": true}),
        ),
        (
            "Quiet Grey",
            json!({"color": "#B0B0B0", "bold": false, "italic": false, "shadow": null}),
        ),
        // Image and shape styles (M6.332): layer effects, DiscCraft's own.
        ("No Effects", json!({"effects": null, "shadow": null})),
        (
            "White Edge",
            json!({"effects": {"stroke": {"size": 3, "color": [255, 255, 255], "opacity": 100}}}),
        ),
        (
            "Dark Edge",
            json!({"effects": {"stroke": {"size": 2, "color": [0, 0, 0], "opacity": 100}}}),
        ),
        (
            "Soft Glow",
            json!({"effects": {"outerGlow": {"size": 12, "spread": 0, "color": [255, 255, 200], "opacity": 80}}}),
        ),
        (
            "Raised",
            json!({"effects": {"bevel": {"size": 6, "angle": 120, "highlight": 75, "shadow": 75}}}),
        ),
        (
            "Raised with Shadow",
            json!({"effects": {"bevel": {"size": 6, "angle": 120, "highlight": 75, "shadow": 75}}, "shadow": shadow}),
        ),
        (
            "Gold",
            json!({"effects": {"overlay": {"color": [212, 175, 55], "opacity": 70}, "bevel": {"size": 4, "angle": 120, "highlight": 60, "shadow": 60}}}),
        ),
        (
            "Inner Light",
            json!({"effects": {"innerGlow": {"size": 10, "spread": 0, "color": [255, 255, 255], "opacity": 70}}}),
        ),
    ]
    .into_iter()
    .map(|(name, style)| Style {
        name: name.to_owned(),
        style,
    })
    .collect()
}

/// What a style goes on.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Text(u64, usize),
    Buttons(u64, Vec<usize>),
    /// An image or a shape (M6.332).
    Graphic(u64, usize),
}

impl DiscCraftApp {
    /// The text object, or the buttons, selected in the menu shown in the Menu Viewer.
    fn style_target(&self) -> Option<Target> {
        let menu = self.viewer_menu()?;
        if let Some(index) = self.selected_menu_text(menu) {
            return Some(Target::Text(menu.0, index));
        }
        if let Some(index) = self.selected_menu_graphic(menu) {
            return Some(Target::Graphic(menu.0, index));
        }
        (self.selection == Selection::Menu(menu) && !self.selected_buttons.is_empty())
            .then(|| Target::Buttons(menu.0, self.selected_buttons.clone()))
    }

    fn apply_style(&mut self, style: &Style) {
        // A text style goes on text and buttons, an image or shape style on an image or a shape.
        let target = self
            .style_target()
            .filter(|target| matches!(target, Target::Graphic(..)) == is_graphic_style(style));
        match target {
            Some(Target::Graphic(menu, index)) => {
                let _ = self.run_command(
                    "menu.applyStyle",
                    json!({"menu": menu, "graphic": index, "style": style.style}),
                );
            }
            Some(Target::Text(menu, index)) => {
                let _ = self.run_command(
                    "menu.applyStyle",
                    json!({"menu": menu, "text": index, "style": style.style}),
                );
            }
            Some(Target::Buttons(menu, buttons)) => {
                for button in buttons {
                    let _ = self.run_command(
                        "menu.applyStyle",
                        json!({"menu": menu, "button": button, "style": style.style}),
                    );
                }
            }
            None if is_graphic_style(style) => {
                self.status =
                    tl!("Select an image or a shape in the Menu Viewer to apply this style")
                        .to_owned();
            }
            None => {
                self.status =
                    tl!("Select a text or buttons in the Menu Viewer to apply a style").to_owned();
            }
        }
    }

    /// Keeps the look of the selected text (or of the one selected button) as a new style of the user's set.
    fn new_style(&mut self, name: &str) {
        let query = match self.style_target() {
            Some(Target::Text(menu, index)) => json!({"menu": menu, "text": index}),
            Some(Target::Graphic(menu, index)) => json!({"menu": menu, "graphic": index}),
            Some(Target::Buttons(menu, buttons)) if buttons.len() == 1 => {
                json!({"menu": menu, "button": buttons.first().copied().unwrap_or(0)})
            }
            _ => {
                self.status =
                    tl!("Select one text, button, image or shape to make a style of its look")
                        .to_owned();
                return;
            }
        };
        let Some(style) = self
            .run_query("menu.styleOf", query)
            .and_then(|found| found.get("style").cloned())
        else {
            return;
        };
        if self.styles.user.styles.len() >= MAX_USER_STYLES {
            self.status = tlf!("The set holds at most {n} styles", n = MAX_USER_STYLES);
            return;
        }
        let name = name.trim();
        let name = if name.is_empty() {
            tlf!(
                "Style {n}",
                n = self.styles.user.styles.len().saturating_add(1)
            )
        } else {
            name.chars().take(80).collect()
        };
        let graphic = style.get("effects").is_some();
        self.styles.user.styles.push(Style { name, style });
        self.styles.user_set = true;
        // Shown with its kind.
        if graphic == (self.styles.filter == StyleFilter::Text) {
            self.styles.filter = if graphic {
                StyleFilter::Shape
            } else {
                StyleFilter::Text
            };
        }
        self.styles.selected = Some(self.styles.user.styles.len().saturating_sub(1));
        if let Err(error) = self.styles.user.save() {
            self.status = error;
        }
    }

    /// The Styles panel.
    pub(crate) fn styles_panel(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(tl!("Set:"));
            let label = |user: bool| if user { tl!("User") } else { tl!("General") };
            let before = self.styles.user_set;
            egui::ComboBox::from_id_salt("styles_set")
                .selected_text(label(self.styles.user_set))
                .show_ui(ui, |ui| {
                    for user in [false, true] {
                        let _ = ui.selectable_value(&mut self.styles.user_set, user, label(user));
                    }
                });
            if before != self.styles.user_set {
                self.styles.selected = None;
            }
        });
        // Encore's Image / Text / Shape buttons: one on at a time.
        ui.horizontal(|ui| {
            for (filter, label, hint) in [
                (
                    StyleFilter::Image,
                    tl!("Image"),
                    tl!("Toggle image style display"),
                ),
                (
                    StyleFilter::Text,
                    tl!("Text"),
                    tl!("Toggle text style display"),
                ),
                (
                    StyleFilter::Shape,
                    tl!("Shape"),
                    tl!("Toggle shape style display"),
                ),
            ] {
                if ui
                    .selectable_label(self.styles.filter == filter, label)
                    .on_hover_text(hint)
                    .clicked()
                    && self.styles.filter != filter
                {
                    self.styles.filter = filter;
                    self.styles.selected = None;
                }
            }
        });
        let all = if self.styles.user_set {
            self.styles.user.styles.clone()
        } else {
            general_styles()
        };
        // The styles shown, with their place in the set (what `selected` names).
        let filter = self.styles.filter;
        let shown: Vec<(usize, Style)> = all
            .iter()
            .cloned()
            .enumerate()
            .filter(|(_, style)| filter.shows(style))
            .collect();
        let styles = all;
        let has_target = self.style_target().is_some();
        let mut apply = None;
        egui::ScrollArea::vertical()
            .max_height((ui.available_height() - 64.0).max(60.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if shown.is_empty() {
                    ui.weak(tl!(
                        "No styles yet: select a text, button, image or shape and choose New"
                    ));
                }
                for (index, style) in &shown {
                    let index = *index;
                    let name = if self.styles.user_set {
                        style.name.clone()
                    } else {
                        crate::i18n::t(&style.name).to_owned()
                    };
                    let row = ui.selectable_label(self.styles.selected == Some(index), name);
                    if row.clicked() {
                        self.styles.selected = Some(index);
                    }
                    if row.double_clicked() {
                        apply = Some(index);
                    }
                }
            });
        ui.separator();
        let chosen = self.styles.selected.filter(|index| *index < styles.len());
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    chosen.is_some() && has_target,
                    egui::Button::new(tl!("Apply")),
                )
                .on_hover_text(tl!(
                    "Puts the style on the selected text, the text of the selected buttons, or the selected image or shape"
                ))
                .clicked()
            {
                apply = chosen;
            }
            if ui
                .add_enabled(has_target, egui::Button::new(tl!("New")))
                .on_hover_text(tl!(
                    "Keeps the look of the selected text, button, image or shape as a style of the User set"
                ))
                .clicked()
            {
                self.styles.naming = Some(String::new());
            }
            if ui
                .add_enabled(
                    self.styles.user_set && chosen.is_some(),
                    egui::Button::new(tl!("Delete")),
                )
                .clicked()
                && let Some(index) = chosen
            {
                let _ = self.styles.user.styles.remove(index);
                self.styles.selected = None;
                if let Err(error) = self.styles.user.save() {
                    self.status = error;
                }
            }
        });
        // New: the style's name.
        if let Some(mut name) = self.styles.naming.take() {
            let mut done = None;
            ui.horizontal(|ui| {
                ui.label(tl!("Name:"));
                let field = ui.add(egui::TextEdit::singleline(&mut name).desired_width(140.0));
                if field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
                if ui.button(tl!("OK")).clicked() {
                    done = Some(true);
                }
                if ui.button(tl!("Cancel")).clicked() {
                    done = Some(false);
                }
            });
            match done {
                Some(true) => self.new_style(&name),
                Some(false) => {}
                None => self.styles.naming = Some(name),
            }
        }
        if let Some(style) = apply.and_then(|index| styles.get(index)) {
            self.apply_style(style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::general_styles;

    #[test]
    fn the_general_styles_are_what_the_engine_takes() -> Result<(), Box<dyn std::error::Error>> {
        let mut session = disccraft_engine::Session::new();
        let menu = session
            .execute("menu.create", &serde_json::Value::Null)?
            .get("menu")
            .and_then(serde_json::Value::as_u64)
            .ok_or("menu")?;
        session.execute(
            "menu.addText",
            &serde_json::json!({"menu": menu, "text": "Title", "x": 40, "y": 40}),
        )?;
        session.execute(
            "menu.addButton",
            &serde_json::json!({"menu": menu, "label": "Play"}),
        )?;
        session.execute(
            "menu.addShape",
            &serde_json::json!({"menu": menu, "shape": "ellipse", "x": 300, "y": 200, "w": 100, "h": 100}),
        )?;
        let styles = general_styles();
        assert!(styles.len() >= 6);
        // M6.332: image and shape styles too, each going on the shape.
        assert!(
            styles
                .iter()
                .filter(|style| super::is_graphic_style(style))
                .count()
                >= 6
        );
        for style in &styles {
            let targets: &[&str] = if super::is_graphic_style(style) {
                &["graphic"]
            } else {
                &["text", "button"]
            };
            for target in targets {
                session
                    .execute(
                        "menu.applyStyle",
                        &serde_json::json!({"menu": menu, *target: 0, "style": style.style}),
                    )
                    .map_err(|error| format!("{}: {error}", style.name))?;
            }
            assert!(
                crate::i18n::Language::Ja.has(&style.name),
                "no Japanese name for {}",
                style.name
            );
        }
        Ok(())
    }
}
