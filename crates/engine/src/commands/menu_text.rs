//! Text objects on menus (Encore's text tool): add, change and remove.

use crate::params::{opt_str, opt_u64, req_str, req_u64, req_usize};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{
    DEFAULT_TEXT_SIZE, MAX_BASELINE, MAX_FONT_NAME_CHARS, MAX_LEADING, MAX_MENU_TEXTS,
    MAX_TEXT_CHARS, MAX_TEXT_SCALE, MAX_TEXT_SIZE, MAX_TRACKING, MIN_BASELINE, MIN_LEADING,
    MIN_TEXT_SCALE, MIN_TEXT_SIZE, MIN_TRACKING, MenuId, MenuText, TextAlign, TextAntialias,
    TextCaps, TextFormat, TextScript, frame_size,
};
use serde_json::Value;

use super::menu::parse_color;
use super::{always_enabled, empty_object, object};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "menu.addText",
        label: "テキストを追加",
        menu: &["メニュー"],
        shortcut: None,
        params: r##"{"menu": id, "text": string, "x": n, "y": n, "size"?: 8..200, "color"?: "#RRGGBB"}"##,
        enabled: always_enabled,
        run: run_add_text,
        journal: true,
    },
    CommandSpec {
        id: "menu.setText",
        label: "テキストを変更",
        menu: &["メニュー"],
        shortcut: None,
        params: r##"{"menu": id, "index": n, "text"?: string, "x"?: n, "y"?: n, "size"?: 8..200, "color"?: "#RRGGBB" | null, "font"?: string | null, "fontStyle"?: string | null, "bold"?: bool, "italic"?: bool, "underline"?: bool, "strikethrough"?: bool, "caps"?: "normal" | "all" | "small", "align"?: "left" | "center" | "right", "tracking"?: -1000..1000, "leading"?: -100..200, "kerning"?: bool, "baseline"?: -100..100, "verticalScale"?: 1..1000, "horizontalScale"?: 1..1000, "script"?: "normal" | "superscript" | "subscript", "antialias"?: "none" | "crisp" | "strong" | "smooth" | "sharp"}"##,
        enabled: always_enabled,
        run: run_set_text,
        journal: true,
    },
    CommandSpec {
        id: "button.setLabelFormat",
        label: "ボタンの文字の書式を設定",
        menu: &["ボタン"],
        shortcut: None,
        params: r##"{"menu": id, "button": index, "fontStyle"?: string | null, "bold"?: bool, "italic"?: bool, "underline"?: bool, "strikethrough"?: bool, "caps"?: "normal" | "all" | "small", "align"?: "left" | "center" | "right", "tracking"?: -1000..1000, "leading"?: -100..200, "kerning"?: bool, "baseline"?: -100..100, "verticalScale"?: 1..1000, "horizontalScale"?: 1..1000, "script"?: "normal" | "superscript" | "subscript", "antialias"?: "none" | "crisp" | "strong" | "smooth" | "sharp"}"##,
        enabled: always_enabled,
        run: run_set_label_format,
        journal: true,
    },
    CommandSpec {
        id: "menu.setLabelFont",
        label: "ボタンの文字のフォントを設定",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "font": string | null}"#,
        enabled: always_enabled,
        run: run_set_label_font,
        journal: true,
    },
    CommandSpec {
        id: "menu.removeText",
        label: "テキストを削除",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "index": n}"#,
        enabled: always_enabled,
        run: run_remove_text,
        journal: true,
    },
];

/// Checks the text itself: not blank, and not too long.
fn checked_text(text: String) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("the text is empty".to_string());
    }
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(format!(
            "the text is longer than {MAX_TEXT_CHARS} characters"
        ));
    }
    Ok(text)
}

/// Checks a position against the menu frame (`limit` is the frame width or height).
fn checked_position(value: u64, name: &str, limit: u16) -> Result<u16, String> {
    u16::try_from(value)
        .ok()
        .filter(|value| *value < limit)
        .ok_or_else(|| {
            format!(
                "\"{name}\" must be inside the menu (0 to {})",
                limit.saturating_sub(1)
            )
        })
}

/// The "font" parameter: absent (`None`), null or "" (`Some(None)`, the default font), or a family name.
fn font_param(params: &Value) -> Result<Option<Option<String>>, String> {
    name_param(params, "font")
}

/// A font name parameter (`key` is "font" or "fontStyle"): absent (`None`), null or "" (`Some(None)`), or a name.
fn name_param(params: &Value, key: &str) -> Result<Option<Option<String>>, String> {
    match params.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(name)) => {
            let name = name.trim();
            if name.chars().count() > MAX_FONT_NAME_CHARS {
                return Err(format!(
                    "the font name is longer than {MAX_FONT_NAME_CHARS} characters"
                ));
            }
            Ok(Some((!name.is_empty()).then(|| name.to_owned())))
        }
        Some(_) => Err(format!("\"{key}\" must be a font name or null")),
    }
}

fn checked_size(value: u64) -> Result<u16, String> {
    u16::try_from(value)
        .ok()
        .filter(|size| (MIN_TEXT_SIZE..=MAX_TEXT_SIZE).contains(size))
        .ok_or_else(|| format!("\"size\" must be {MIN_TEXT_SIZE} to {MAX_TEXT_SIZE}"))
}

fn run_add_text(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let (width, height) = frame_size(&session.project.settings);
    let text = checked_text(req_str(params, "text")?)?;
    let x = checked_position(req_u64(params, "x")?, "x", width)?;
    let y = checked_position(req_u64(params, "y")?, "y", height)?;
    let size = match opt_u64(params, "size")? {
        Some(size) => checked_size(size)?,
        None => DEFAULT_TEXT_SIZE,
    };
    let color = opt_str(params, "color")?
        .map(|color| parse_color(&color))
        .transpose()?;
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    if menu.texts.len() >= MAX_MENU_TEXTS {
        return Err(format!("a menu holds at most {MAX_MENU_TEXTS} texts"));
    }
    menu.texts.push(MenuText {
        text,
        x,
        y,
        size,
        color,
        font: None,
        font_style: None,
        format: TextFormat::default(),
        shadow: None,
        orientation: Default::default(),
    });
    let index = u64::try_from(menu.texts.len().saturating_sub(1))
        .map_err(|_| "text index is too large".to_string())?;
    Ok(object(&[("index", Value::from(index))]))
}

fn run_set_text(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let index = req_usize(params, "index")?;
    let (width, height) = frame_size(&session.project.settings);
    let text = opt_str(params, "text")?.map(checked_text).transpose()?;
    let x = opt_u64(params, "x")?
        .map(|x| checked_position(x, "x", width))
        .transpose()?;
    let y = opt_u64(params, "y")?
        .map(|y| checked_position(y, "y", height))
        .transpose()?;
    let size = opt_u64(params, "size")?.map(checked_size).transpose()?;
    // "color": null goes back to the menu's text colour; leaving it out keeps the colour.
    let color = match params.get("color") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(color)) => Some(Some(parse_color(color)?)),
        Some(_) => return Err("\"color\" must be \"#RRGGBB\" or null".to_string()),
    };
    let font = font_param(params)?;
    let font_style = name_param(params, "fontStyle")?;
    let patch = FormatPatch::parse(params)?;
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let Some(target) = menu.texts.get_mut(index) else {
        return Err(format!("text {index} does not exist"));
    };
    if let Some(text) = text {
        target.text = text;
    }
    patch.apply(&mut target.format);
    if let Some(x) = x {
        target.x = x;
    }
    if let Some(y) = y {
        target.y = y;
    }
    if let Some(size) = size {
        target.size = size;
    }
    if let Some(color) = color {
        target.color = color;
    }
    if let Some(font) = font {
        target.font = font;
    }
    if let Some(font_style) = font_style {
        target.font_style = font_style;
    }
    Ok(empty_object())
}

/// The Character panel fields of `menu.setText` / `button.setLabelFormat` (each optional: left out keeps it).
#[derive(Clone, Copy, Default)]
struct FormatPatch {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strikethrough: Option<bool>,
    caps: Option<TextCaps>,
    align: Option<TextAlign>,
    tracking: Option<i16>,
    leading: Option<i16>,
    kerning: Option<bool>,
    baseline: Option<i16>,
    vertical_scale: Option<u16>,
    horizontal_scale: Option<u16>,
    script: Option<TextScript>,
    antialias: Option<TextAntialias>,
}

impl FormatPatch {
    fn parse(params: &Value) -> Result<Self, String> {
        Ok(Self {
            bold: opt_flag(params, "bold")?,
            italic: opt_flag(params, "italic")?,
            underline: opt_flag(params, "underline")?,
            strikethrough: opt_flag(params, "strikethrough")?,
            caps: opt_str(params, "caps")?
                .map(|text| {
                    TextCaps::parse(&text).ok_or_else(|| {
                        "parameter \"caps\" must be \"normal\", \"all\" or \"small\"".to_string()
                    })
                })
                .transpose()?,
            align: opt_str(params, "align")?
                .map(|text| {
                    TextAlign::parse(&text).ok_or_else(|| {
                        "parameter \"align\" must be \"left\", \"center\" or \"right\"".to_string()
                    })
                })
                .transpose()?,
            tracking: opt_i16(params, "tracking", MIN_TRACKING, MAX_TRACKING)?,
            leading: opt_i16(params, "leading", MIN_LEADING, MAX_LEADING)?,
            kerning: opt_flag(params, "kerning")?,
            baseline: opt_i16(params, "baseline", MIN_BASELINE, MAX_BASELINE)?,
            vertical_scale: opt_scale(params, "verticalScale")?,
            horizontal_scale: opt_scale(params, "horizontalScale")?,
            script: opt_str(params, "script")?
                .map(|text| {
                    TextScript::parse(&text).ok_or_else(|| {
                        "parameter \"script\" must be \"normal\", \"superscript\" or \"subscript\""
                            .to_string()
                    })
                })
                .transpose()?,
            antialias: opt_str(params, "antialias")?
                .map(|text| {
                    TextAntialias::parse(&text).ok_or_else(|| {
                        "parameter \"antialias\" must be \"none\", \"crisp\", \"strong\", \"smooth\" or \"sharp\""
                            .to_string()
                    })
                })
                .transpose()?,
        })
    }

    fn apply(&self, format: &mut TextFormat) {
        let Self {
            bold,
            italic,
            underline,
            strikethrough,
            caps,
            align,
            tracking,
            leading,
            kerning,
            baseline,
            vertical_scale,
            horizontal_scale,
            script,
            antialias,
        } = *self;
        if let Some(value) = bold {
            format.bold = value;
        }
        if let Some(value) = italic {
            format.italic = value;
        }
        if let Some(value) = underline {
            format.underline = value;
        }
        if let Some(value) = strikethrough {
            format.strikethrough = value;
        }
        if let Some(value) = caps {
            format.caps = value;
        }
        if let Some(value) = align {
            format.align = value;
        }
        if let Some(value) = tracking {
            format.tracking = value;
        }
        if let Some(value) = leading {
            format.leading = value;
        }
        if let Some(value) = kerning {
            format.kerning = value;
        }
        if let Some(value) = baseline {
            format.baseline = value;
        }
        if let Some(value) = vertical_scale {
            format.vertical_scale = value;
        }
        if let Some(value) = horizontal_scale {
            format.horizontal_scale = value;
        }
        if let Some(value) = script {
            format.script = value;
        }
        if let Some(value) = antialias {
            format.antialias = value;
        }
    }
}

/// Encore's Character panel on a button's text: the same fields as `menu.setText`, and "fontStyle".
fn run_set_label_format(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let button = req_usize(params, "button")?;
    let patch = FormatPatch::parse(params)?;
    let font_style = name_param(params, "fontStyle")?;
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let Some(target) = menu.buttons.get_mut(button) else {
        return Err(format!("button {button} does not exist"));
    };
    patch.apply(&mut target.label_format);
    if let Some(font_style) = font_style {
        target.label_font_style = font_style;
    }
    Ok(empty_object())
}

/// An optional text scale in per cent.
fn opt_scale(params: &Value, name: &str) -> Result<Option<u16>, String> {
    let min = i16::try_from(MIN_TEXT_SCALE).unwrap_or(1);
    let max = i16::try_from(MAX_TEXT_SCALE).unwrap_or(i16::MAX);
    Ok(opt_i16(params, name, min, max)?.and_then(|n| u16::try_from(n).ok()))
}

/// An optional boolean parameter.
fn opt_flag(params: &Value, name: &str) -> Result<Option<bool>, String> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(flag)) => Ok(Some(*flag)),
        Some(_) => Err(format!("parameter \"{name}\" must be a boolean")),
    }
}

/// An optional small integer parameter within `min..=max`.
fn opt_i16(params: &Value, name: &str, min: i16, max: i16) -> Result<Option<i16>, String> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let number = value
                .as_i64()
                .and_then(|n| i16::try_from(n).ok())
                .filter(|n| (min..=max).contains(n))
                .ok_or_else(|| format!("parameter \"{name}\" must be between {min} and {max}"))?;
            Ok(Some(number))
        }
    }
}

fn run_set_label_font(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let Some(font) = font_param(params)? else {
        return Err("parameter \"font\" is required (a family name, or null)".to_string());
    };
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.label_font = font;
    Ok(empty_object())
}

fn run_remove_text(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let index = req_usize(params, "index")?;
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    if index >= menu.texts.len() {
        return Err(format!("text {index} does not exist"));
    }
    let _ = menu.texts.remove(index);
    Ok(empty_object())
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use disccraft_project::{MenuId, TextAlign, TextCaps, TextFormat};
    use serde_json::json;
    use std::error::Error;

    fn session_with_menu() -> Result<(Session, u64), Box<dyn Error>> {
        let mut session = Session::new();
        let created = session.execute("menu.create", &json!({"name": "Main"}))?;
        let menu = created
            .get("menu")
            .and_then(serde_json::Value::as_u64)
            .ok_or("no menu id")?;
        Ok((session, menu))
    }

    #[test]
    fn a_button_text_is_formatted() -> Result<(), Box<dyn Error>> {
        let (mut session, menu) = session_with_menu()?;
        session.execute("menu.addButton", &json!({"menu": menu, "label": "Play"}))?;
        session.execute(
            "button.setLabelFormat",
            &json!({"menu": menu, "button": 0, "bold": true, "verticalScale": 120, "fontStyle": "Bold", "antialias": "none"}),
        )?;
        let button = session
            .project
            .menus
            .values()
            .next()
            .and_then(|menu| menu.buttons.first().cloned())
            .ok_or("button")?;
        assert!(button.label_format.bold);
        assert_eq!(button.label_format.vertical_scale, 120);
        assert_eq!(button.label_font_style.as_deref(), Some("Bold"));
        assert_eq!(
            button.label_format.antialias,
            disccraft_project::TextAntialias::None
        );
        assert!(
            session
                .execute(
                    "button.setLabelFormat",
                    &json!({"menu": menu, "button": 0, "tracking": 5000}),
                )
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn the_font_style_is_set_and_cleared() -> Result<(), Box<dyn Error>> {
        let (mut session, menu) = session_with_menu()?;
        session.execute(
            "menu.addText",
            &json!({"menu": menu, "text": "Title", "x": 40, "y": 30}),
        )?;
        let style = |session: &Session| {
            session
                .project
                .menus
                .values()
                .next()
                .and_then(|menu| menu.texts.first())
                .and_then(|text| text.font_style.clone())
        };
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "fontStyle": " Bold "}),
        )?;
        assert_eq!(style(&session).as_deref(), Some("Bold"));
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "fontStyle": null}),
        )?;
        assert_eq!(style(&session), None);
        for params in [
            json!({"menu": menu, "index": 0, "fontStyle": 3}),
            json!({"menu": menu, "index": 0, "fontStyle": "x".repeat(200)}),
        ] {
            assert!(session.execute("menu.setText", &params).is_err());
        }
        Ok(())
    }

    #[test]
    fn scales_and_scripts_are_set_and_checked() -> Result<(), Box<dyn Error>> {
        let (mut session, menu) = session_with_menu()?;
        session.execute(
            "menu.addText",
            &json!({"menu": menu, "text": "H2O", "x": 40, "y": 30}),
        )?;
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "verticalScale": 150, "horizontalScale": 50, "script": "subscript", "antialias": "none"}),
        )?;
        let format = session
            .project
            .menus
            .values()
            .next()
            .and_then(|menu| menu.texts.first())
            .map(|text| text.format)
            .ok_or("no text")?;
        assert_eq!(
            (
                format.vertical_scale,
                format.horizontal_scale,
                format.script
            ),
            (150, 50, disccraft_project::TextScript::Subscript)
        );
        assert_eq!(format.antialias, disccraft_project::TextAntialias::None);
        for params in [
            json!({"menu": menu, "index": 0, "verticalScale": 0}),
            json!({"menu": menu, "index": 0, "horizontalScale": 1001}),
            json!({"menu": menu, "index": 0, "script": "sup"}),
            json!({"menu": menu, "index": 0, "antialias": "blurry"}),
        ] {
            assert!(
                session.execute("menu.setText", &params).is_err(),
                "{params}"
            );
        }
        Ok(())
    }

    #[test]
    fn texts_are_added_changed_and_removed() -> Result<(), Box<dyn Error>> {
        let (mut session, menu) = session_with_menu()?;
        let added = session.execute(
            "menu.addText",
            &json!({"menu": menu, "text": "Welcome", "x": 40, "y": 30}),
        )?;
        assert_eq!(
            added.get("index").and_then(serde_json::Value::as_u64),
            Some(0)
        );
        let texts = |session: &Session| {
            session
                .project
                .menus
                .values()
                .next()
                .map(|menu| menu.texts.clone())
                .unwrap_or_default()
        };
        let first = texts(&session);
        assert_eq!(first.len(), 1);
        assert_eq!(first.first().map(|text| text.size), Some(32));

        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "text": "Hello\nWorld", "x": 100, "size": 48, "color": "#FF0000"}),
        )?;
        let changed = texts(&session);
        let text = changed.first().ok_or("no text")?;
        assert_eq!(
            (text.text.as_str(), text.x, text.y, text.size, text.color),
            ("Hello\nWorld", 100, 30, 48, Some([255, 0, 0]))
        );
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "color": null}),
        )?;
        assert_eq!(texts(&session).first().and_then(|text| text.color), None);

        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "font": "  Meiryo  "}),
        )?;
        assert_eq!(
            texts(&session).first().and_then(|text| text.font.clone()),
            Some("Meiryo".to_owned())
        );
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "font": ""}),
        )?;
        assert_eq!(
            texts(&session).first().and_then(|text| text.font.clone()),
            None
        );
        assert!(
            session
                .execute(
                    "menu.setText",
                    &json!({"menu": menu, "index": 0, "font": 3})
                )
                .is_err()
        );
        let label_font = |session: &Session| {
            session
                .project
                .menus
                .values()
                .next()
                .and_then(|menu| menu.label_font.clone())
        };
        session.execute(
            "menu.setLabelFont",
            &json!({"menu": menu, "font": "Yu Gothic"}),
        )?;
        assert_eq!(label_font(&session), Some("Yu Gothic".to_owned()));
        session.execute("menu.setLabelFont", &json!({"menu": menu, "font": null}))?;
        assert_eq!(label_font(&session), None);
        assert!(
            session
                .execute("menu.setLabelFont", &json!({"menu": menu}))
                .is_err()
        );

        session.execute("menu.removeText", &json!({"menu": menu, "index": 0}))?;
        assert!(texts(&session).is_empty());
        session.undo()?;
        assert_eq!(texts(&session).len(), 1);
        Ok(())
    }

    #[test]
    fn text_formatting_is_set_and_undone() -> Result<(), Box<dyn Error>> {
        let (mut session, menu) = session_with_menu()?;
        session.execute(
            "menu.addText",
            &json!({"menu": menu, "text": "Title", "x": 10, "y": 10}),
        )?;
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "bold": true, "align": "center", "tracking": 50, "leading": 4, "caps": "all"}),
        )?;
        let format = |session: &Session| {
            session
                .project
                .menu(MenuId(menu))
                .and_then(|menu| menu.texts.first())
                .map(|text| text.format)
        };
        assert_eq!(
            format(&session),
            Some(TextFormat {
                bold: true,
                align: TextAlign::Center,
                tracking: 50,
                leading: 4,
                caps: TextCaps::All,
                ..TextFormat::default()
            })
        );
        for params in [
            json!({"menu": menu, "index": 0, "align": "up"}),
            json!({"menu": menu, "index": 0, "tracking": 5000}),
            json!({"menu": menu, "index": 0, "bold": "yes"}),
        ] {
            assert!(
                session.execute("menu.setText", &params).is_err(),
                "{params}"
            );
        }
        session.undo()?;
        assert_eq!(format(&session), Some(TextFormat::default()));
        Ok(())
    }

    #[test]
    fn bad_texts_are_refused() -> Result<(), Box<dyn Error>> {
        let (mut session, menu) = session_with_menu()?;
        for params in [
            json!({"menu": menu, "text": "  ", "x": 0, "y": 0}),
            json!({"menu": menu, "text": "A", "x": 720, "y": 0}),
            json!({"menu": menu, "text": "A", "x": 0, "y": 480}),
            json!({"menu": menu, "text": "A", "x": 0, "y": 0, "size": 7}),
            json!({"menu": menu, "text": "A", "x": 0, "y": 0, "size": 201}),
            json!({"menu": menu, "text": "A", "x": 0, "y": 0, "color": "red"}),
            json!({"menu": menu, "text": "x".repeat(1001), "x": 0, "y": 0}),
            json!({"menu": 999, "text": "A", "x": 0, "y": 0}),
        ] {
            assert!(
                session.execute("menu.addText", &params).is_err(),
                "{params}"
            );
        }
        assert!(
            session
                .execute("menu.setText", &json!({"menu": menu, "index": 0, "x": 1}))
                .is_err()
        );
        assert!(
            session
                .execute("menu.removeText", &json!({"menu": menu, "index": 0}))
                .is_err()
        );

        for _ in 0..64 {
            session.execute(
                "menu.addText",
                &json!({"menu": menu, "text": "A", "x": 0, "y": 0}),
            )?;
        }
        assert!(
            session
                .execute(
                    "menu.addText",
                    &json!({"menu": menu, "text": "A", "x": 0, "y": 0})
                )
                .is_err()
        );
        Ok(())
    }
}
