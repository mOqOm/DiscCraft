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
        label: "Add Text",
        menu: &["Menu"],
        shortcut: None,
        params: r##"{"menu": id, "text": string, "x": n, "y": n, "size"?: 8..200, "color"?: "#RRGGBB", "vertical"?: bool (vertical text), "boxWidth"?: 16..4096 (paragraph text's width), "font"?: string | null, "fontStyle"?: string | null, and the character settings of menu.setText (bold, align, tracking, …)}"##,
        enabled: always_enabled,
        run: run_add_text,
        journal: true,
    },
    CommandSpec {
        id: "menu.placeText",
        label: "Place Text",
        menu: &["Menu"],
        shortcut: None,
        params: r##"{"menu": id, "text": MenuText (a text object as a project keeps it: a Library text item), "x"?: n, "y"?: n (its top left corner; else its own)}"##,
        enabled: always_enabled,
        run: run_place_text,
        journal: true,
    },
    CommandSpec {
        id: "menu.setText",
        label: "Change Text",
        menu: &["Menu"],
        shortcut: None,
        params: r##"{"menu": id, "index": n, "text"?: string, "x"?: n, "y"?: n, "size"?: 8..200, "color"?: "#RRGGBB" | null, "font"?: string | null, "fontStyle"?: string | null, "bold"?: bool, "italic"?: bool, "underline"?: bool, "strikethrough"?: bool, "caps"?: "normal" | "all" | "small", "align"?: "left" | "center" | "right" | "justify", "tracking"?: -1000..1000, "linePitch"?: 1..1000 | null (auto, 120 % of the size), "leading"?: -100..200, "kerning"?: true (metrics) | false | -1000..1000 (1/1000 em), "baseline"?: -100..100, "verticalScale"?: 1..1000, "horizontalScale"?: 1..1000, "script"?: "normal" | "superscript" | "subscript", "antialias"?: "none" | "crisp" | "strong" | "smooth" | "sharp", "tsume"?: 0..100, "mojikumi"?: "none" | "punctuationHalf" | "lineEndHalf" | "lineEndFull" | "punctuationFull", "kinsoku"?: "none" | "weak" | "strong", "kinsokuMethod"?: "pushInFirst" | "pushOutFirst" | "pushOutOnly", "hangingPunctuation"?: bool, "vertical"?: bool, "boxWidth"?: 16..4096 | null, "boxHeight"?: 16..4096 | null (a paragraph text's height: the lines past it are not drawn), "from"?: n, "to"?: n (M6.389: characters from..to of the text, counted in characters: size, color, font, fontStyle, bold, italic, underline, strikethrough, caps, tracking and baseline are set for them alone, null going back to the text's; the other fields for the whole text. Without them those fields are set for every character)}"##,
        enabled: always_enabled,
        run: run_set_text,
        journal: true,
    },
    CommandSpec {
        id: "button.setLabelFormat",
        label: "Set Button Text Format",
        menu: &["Button"],
        shortcut: None,
        params: r##"{"menu": id, "button": index, "fontStyle"?: string | null, "bold"?: bool, "italic"?: bool, "underline"?: bool, "strikethrough"?: bool, "caps"?: "normal" | "all" | "small", "align"?: "left" | "center" | "right" | "justify", "tracking"?: -1000..1000, "linePitch"?: 1..1000 | null (auto, 120 % of the size), "leading"?: -100..200, "kerning"?: true (metrics) | false | -1000..1000 (1/1000 em), "baseline"?: -100..100, "verticalScale"?: 1..1000, "horizontalScale"?: 1..1000, "script"?: "normal" | "superscript" | "subscript", "antialias"?: "none" | "crisp" | "strong" | "smooth" | "sharp", "tsume"?: 0..100, "mojikumi"?: "none" | "punctuationHalf" | "lineEndHalf" | "lineEndFull" | "punctuationFull", "kinsoku"?: "none" | "weak" | "strong", "kinsokuMethod"?: "pushInFirst" | "pushOutFirst" | "pushOutOnly", "hangingPunctuation"?: bool}"##,
        enabled: always_enabled,
        run: run_set_label_format,
        journal: true,
    },
    CommandSpec {
        id: "menu.setLabelFont",
        label: "Set Button Text Font",
        menu: &["Menu"],
        shortcut: None,
        params: r#"{"menu": id, "font": string | null}"#,
        enabled: always_enabled,
        run: run_set_label_font,
        journal: true,
    },
    CommandSpec {
        id: "menu.removeText",
        label: "Delete Text",
        menu: &["Menu"],
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

/// The width of a paragraph text's box, frame pixels.
fn checked_box(width: u64) -> Result<u16, String> {
    checked_box_side(width, "boxWidth")
}

/// A side of a paragraph text's box (parameter `name`), frame pixels.
fn checked_box_side(side: u64, name: &str) -> Result<u16, String> {
    use disccraft_project::{MAX_TEXT_BOX, MIN_TEXT_BOX};
    u16::try_from(side)
        .ok()
        .filter(|side| (MIN_TEXT_BOX..=MAX_TEXT_BOX).contains(side))
        .ok_or_else(|| format!("\"{name}\" must be {MIN_TEXT_BOX} to {MAX_TEXT_BOX}"))
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
    let vertical = opt_flag(params, "vertical")?.unwrap_or(false);
    let box_width = opt_u64(params, "boxWidth")?.map(checked_box).transpose()?;
    // The characters of the next text (Encore: the Character panel with nothing selected): its font, style and
    // format, as menu.setText takes them.
    let font = font_param(params)?.flatten();
    let font_style = name_param(params, "fontStyle")?.flatten();
    let mut format = TextFormat::default();
    FormatPatch::parse(params)?.apply(&mut format);
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    if menu.texts.len() >= MAX_MENU_TEXTS {
        return Err(format!("a menu holds at most {MAX_MENU_TEXTS} texts"));
    }
    menu.texts.push(MenuText {
        effects: Default::default(),
        box_width,
        box_height: None,
        text,
        x,
        y,
        size,
        color,
        font,
        font_style,
        format,
        shadow: None,
        orientation: Default::default(),
        hidden: false,
        locked: false,
        vertical,
        spans: Vec::new(),
    });
    let index = u64::try_from(menu.texts.len().saturating_sub(1))
        .map_err(|_| "text index is too large".to_string())?;
    Ok(object(&[("index", Value::from(index))]))
}

/// Places a whole text object (Encore: a Library text item placed in a menu): its text, look and box as kept,
/// at `x`, `y` when given, inside the frame; shown and unlocked.
fn run_place_text(session: &mut Session, params: &Value) -> CommandResult {
    use disccraft_project::MAX_FONT_NAME_CHARS;
    let menu_id = MenuId(req_u64(params, "menu")?);
    let (width, height) = frame_size(&session.project.settings);
    let value = params
        .get("text")
        .ok_or_else(|| "parameter \"text\" is missing".to_string())?;
    let mut text: MenuText = serde_json::from_value(value.clone())
        .map_err(|error| format!("parameter \"text\" must be a text object: {error}"))?;
    text.text = checked_text(std::mem::take(&mut text.text))?;
    text.size = checked_size(u64::from(text.size))?;
    text.x = match opt_u64(params, "x")? {
        Some(x) => checked_position(x, "x", width)?,
        None => text.x.min(width.saturating_sub(1)),
    };
    text.y = match opt_u64(params, "y")? {
        Some(y) => checked_position(y, "y", height)?,
        None => text.y.min(height.saturating_sub(1)),
    };
    text.box_width = text
        .box_width
        .map(|side| checked_box(u64::from(side)))
        .transpose()?;
    text.box_height = match text.box_width {
        Some(_) => text
            .box_height
            .map(|side| checked_box_side(u64::from(side), "boxHeight"))
            .transpose()?,
        None => None,
    };
    for name in [&mut text.font, &mut text.font_style] {
        if let Some(found) = name.as_mut() {
            *found = found.chars().take(MAX_FONT_NAME_CHARS).collect();
        }
    }
    text.hidden = false;
    text.locked = false;
    text.check_spans();
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    if menu.texts.len() >= MAX_MENU_TEXTS {
        return Err(format!("a menu holds at most {MAX_MENU_TEXTS} texts"));
    }
    menu.texts.push(text);
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
    let vertical = opt_flag(params, "vertical")?;
    // "boxWidth": null makes it point text again; leaving it out keeps the box.
    let box_width = match params.get("boxWidth") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(_) => Some(opt_u64(params, "boxWidth")?.map(checked_box).transpose()?),
    };
    // "boxHeight": null lets the box be as tall as its lines; leaving it out keeps the height.
    let box_height = match params.get("boxHeight") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(_) => Some(
            opt_u64(params, "boxHeight")?
                .map(|side| checked_box_side(side, "boxHeight"))
                .transpose()?,
        ),
    };
    let mut patch = patch;
    let chars = CharFields {
        size,
        color,
        font,
        font_style,
        format: patch.take_char_fields(),
    };
    // M6.389: characters from..to of the text get the character fields alone.
    let range = match (opt_u64(params, "from")?, opt_u64(params, "to")?) {
        (None, None) => None,
        (Some(from), Some(to)) => {
            if text.is_some() {
                return Err(
                    "\"text\" cannot change with a character range (\"from\", \"to\")".to_string(),
                );
            }
            let from = usize::try_from(from).map_err(|_| "\"from\" is too large".to_string())?;
            let to = usize::try_from(to).map_err(|_| "\"to\" is too large".to_string())?;
            Some(from..to)
        }
        _ => return Err("\"from\" and \"to\" go together".to_string()),
    };
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let Some(target) = menu.texts.get_mut(index) else {
        return Err(format!("text {index} does not exist"));
    };
    if let Some(range) = &range {
        let count = target.text.chars().count();
        if range.start >= range.end || range.end > count {
            return Err(format!(
                "the character range must be inside the text (0 to {count}, \"from\" before \"to\")"
            ));
        }
    }
    if let Some(text) = text {
        // The characters' own formatting follows the edit.
        target.retext(text);
    }
    if let Some(vertical) = vertical {
        target.vertical = vertical;
    }
    if let Some(box_width) = box_width {
        target.box_width = box_width;
    }
    if let Some(box_height) = box_height {
        target.box_height = box_height;
    }
    // Point text has no box, so no height either.
    if target.box_width.is_none() {
        target.box_height = None;
    }
    patch.apply(&mut target.format);
    if let Some(x) = x {
        target.x = x;
    }
    if let Some(y) = y {
        target.y = y;
    }
    match range {
        Some(range) => target.style_chars(range, &|style| chars.set_on(style)),
        None => {
            chars.apply(target);
            // Set for the whole text, a field is every character's: the spans' own goes.
            target.style_all_chars(&|style| chars.clear_from(style));
        }
    }
    Ok(empty_object())
}

/// The character fields of `menu.setText` (each `None` when left out): set for the whole text, or for a range of
/// its characters (M6.389).
struct CharFields {
    size: Option<u16>,
    /// `Some(None)`: the menu's text colour (for characters: the text's).
    color: Option<Option<[u8; 3]>>,
    /// `Some(None)`: the default font (for characters: the text's).
    font: Option<Option<String>>,
    font_style: Option<Option<String>>,
    format: FormatChars,
}

/// The character fields of [`FormatPatch`].
#[derive(Clone, Copy, Default)]
struct FormatChars {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strikethrough: Option<bool>,
    caps: Option<TextCaps>,
    tracking: Option<i16>,
    baseline: Option<i16>,
}

impl CharFields {
    /// Sets them for the whole text.
    fn apply(&self, target: &mut MenuText) {
        if let Some(size) = self.size {
            target.size = size;
        }
        if let Some(color) = self.color {
            target.color = color;
        }
        if let Some(font) = &self.font {
            target.font.clone_from(font);
        }
        if let Some(font_style) = &self.font_style {
            target.font_style.clone_from(font_style);
        }
        FormatPatch {
            bold: self.format.bold,
            italic: self.format.italic,
            underline: self.format.underline,
            strikethrough: self.format.strikethrough,
            caps: self.format.caps,
            tracking: self.format.tracking,
            baseline: self.format.baseline,
            ..FormatPatch::default()
        }
        .apply(&mut target.format);
    }

    /// Sets them for some characters (null going back to the text's).
    fn set_on(&self, style: &mut disccraft_project::SpanStyle) {
        if let Some(size) = self.size {
            style.size = Some(size);
        }
        if let Some(color) = self.color {
            style.color = color;
        }
        if let Some(font) = &self.font {
            style.font.clone_from(font);
        }
        if let Some(font_style) = &self.font_style {
            style.font_style.clone_from(font_style);
        }
        let FormatChars {
            bold,
            italic,
            underline,
            strikethrough,
            caps,
            tracking,
            baseline,
        } = self.format;
        style.bold = bold.or(style.bold);
        style.italic = italic.or(style.italic);
        style.underline = underline.or(style.underline);
        style.strikethrough = strikethrough.or(style.strikethrough);
        style.caps = caps.or(style.caps);
        style.tracking = tracking.or(style.tracking);
        style.baseline = baseline.or(style.baseline);
    }

    /// Takes the fields that are set out of some characters' own formatting.
    fn clear_from(&self, style: &mut disccraft_project::SpanStyle) {
        if self.size.is_some() {
            style.size = None;
        }
        if self.color.is_some() {
            style.color = None;
        }
        if self.font.is_some() {
            style.font = None;
        }
        if self.font_style.is_some() {
            style.font_style = None;
        }
        let format = self.format;
        if format.bold.is_some() {
            style.bold = None;
        }
        if format.italic.is_some() {
            style.italic = None;
        }
        if format.underline.is_some() {
            style.underline = None;
        }
        if format.strikethrough.is_some() {
            style.strikethrough = None;
        }
        if format.caps.is_some() {
            style.caps = None;
        }
        if format.tracking.is_some() {
            style.tracking = None;
        }
        if format.baseline.is_some() {
            style.baseline = None;
        }
    }
}

/// Encore's カーニング (M6.344): `true` the font's metric kerning; `false` none; a number (1/1000 em) the room
/// between every two characters changed by that much. (metric kerning, the number), each `None` when left out.
fn kerning_param(params: &Value) -> Result<(Option<bool>, Option<i16>), String> {
    match params.get("kerning") {
        None => Ok((None, None)),
        Some(Value::Bool(true)) => Ok((Some(true), None)),
        Some(Value::Bool(false)) => Ok((Some(false), Some(0))),
        Some(value) => value
            .as_i64()
            .and_then(|number| i16::try_from(number).ok())
            .filter(|number| (MIN_TRACKING..=MAX_TRACKING).contains(number))
            .map(|number| (Some(false), Some(number)))
            .ok_or_else(|| {
                format!(
                    "parameter \"kerning\" must be true (metrics), false or {MIN_TRACKING} to {MAX_TRACKING}"
                )
            }),
    }
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
    /// Encore's 行送り: `Some(None)` 自動.
    line_pitch: Option<Option<u16>>,
    leading: Option<i16>,
    kerning: Option<bool>,
    /// Encore's カーニング as a number (M6.344).
    manual_kerning: Option<i16>,
    baseline: Option<i16>,
    vertical_scale: Option<u16>,
    horizontal_scale: Option<u16>,
    script: Option<TextScript>,
    antialias: Option<TextAntialias>,
    tsume: Option<u8>,
    mojikumi: Option<disccraft_project::TextMojikumi>,
    kinsoku: Option<disccraft_project::TextKinsoku>,
    kinsoku_method: Option<disccraft_project::TextKinsokuMethod>,
    hanging_punctuation: Option<bool>,
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
                        "parameter \"align\" must be \"left\", \"center\", \"right\" or \"justify\""
                            .to_string()
                    })
                })
                .transpose()?,
            tracking: opt_i16(params, "tracking", MIN_TRACKING, MAX_TRACKING)?,
            line_pitch: match params.get("linePitch") {
                None => None,
                Some(Value::Null) => Some(None),
                Some(value) => Some(Some(
                    value
                        .as_u64()
                        .and_then(|value| u16::try_from(value).ok())
                        .filter(|value| {
                            (disccraft_project::MIN_LINE_PITCH..=disccraft_project::MAX_LINE_PITCH)
                                .contains(value)
                        })
                        .ok_or_else(|| {
                            format!(
                                "parameter \"linePitch\" must be null (auto) or {} to {}",
                                disccraft_project::MIN_LINE_PITCH,
                                disccraft_project::MAX_LINE_PITCH
                            )
                        })?,
                )),
            },
            leading: opt_i16(params, "leading", MIN_LEADING, MAX_LEADING)?,
            kerning: kerning_param(params)?.0,
            manual_kerning: kerning_param(params)?.1,
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
            tsume: match params.get("tsume") {
                None => None,
                Some(value) => Some(
                    value
                        .as_u64()
                        .and_then(|value| u8::try_from(value).ok())
                        .filter(|value| *value <= disccraft_project::MAX_TSUME)
                        .ok_or_else(|| "parameter \"tsume\" must be 0 to 100".to_string())?,
                ),
            },
            mojikumi: opt_str(params, "mojikumi")?
                .map(|text| {
                    disccraft_project::TextMojikumi::parse(&text).ok_or_else(|| {
                        "parameter \"mojikumi\" must be \"none\", \"punctuationHalf\", \"lineEndHalf\", \"lineEndFull\" or \"punctuationFull\""
                            .to_string()
                    })
                })
                .transpose()?,
            kinsoku: opt_str(params, "kinsoku")?
                .map(|text| {
                    disccraft_project::TextKinsoku::parse(&text).ok_or_else(|| {
                        "parameter \"kinsoku\" must be \"none\", \"weak\" or \"strong\"".to_string()
                    })
                })
                .transpose()?,
            kinsoku_method: opt_str(params, "kinsokuMethod")?
                .map(|text| {
                    disccraft_project::TextKinsokuMethod::parse(&text).ok_or_else(|| {
                        "parameter \"kinsokuMethod\" must be \"pushInFirst\", \"pushOutFirst\" or \"pushOutOnly\""
                            .to_string()
                    })
                })
                .transpose()?,
            hanging_punctuation: opt_flag(params, "hangingPunctuation")?,
        })
    }

    /// Takes the character fields out (M6.389: they can go to a range of characters).
    fn take_char_fields(&mut self) -> FormatChars {
        FormatChars {
            bold: self.bold.take(),
            italic: self.italic.take(),
            underline: self.underline.take(),
            strikethrough: self.strikethrough.take(),
            caps: self.caps.take(),
            tracking: self.tracking.take(),
            baseline: self.baseline.take(),
        }
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
            line_pitch,
            leading,
            kerning,
            manual_kerning,
            baseline,
            vertical_scale,
            horizontal_scale,
            script,
            antialias,
            tsume,
            mojikumi,
            kinsoku,
            kinsoku_method,
            hanging_punctuation,
        } = *self;
        if let Some(value) = line_pitch {
            format.line_pitch = value;
        }
        if let Some(value) = kinsoku {
            format.kinsoku = value;
        }
        if let Some(value) = kinsoku_method {
            format.kinsoku_method = value;
        }
        if let Some(value) = hanging_punctuation {
            format.hanging_punctuation = value;
        }
        if let Some(value) = tsume {
            format.tsume = value;
        }
        if let Some(value) = mojikumi {
            format.mojikumi = value;
        }
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
        if let Some(value) = manual_kerning {
            format.manual_kerning = value;
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

    /// M6.389: characters of a text get formatting of their own; set for the whole text, a field is every
    /// character's again; the spans follow the text as it is edited, and a wrong range is refused.
    #[test]
    fn characters_are_formatted_on_their_own() -> Result<(), Box<dyn Error>> {
        let (mut session, menu) = session_with_menu()?;
        session.execute(
            "menu.addText",
            &json!({"menu": menu, "text": "Hello world", "x": 40, "y": 30}),
        )?;
        let text = |session: &Session| {
            session
                .project
                .menus
                .values()
                .next()
                .and_then(|menu| menu.texts.first().cloned())
        };
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "from": 6, "to": 11, "size": 60, "color": "#FF0000", "bold": true, "align": "center"}),
        )?;
        let t = text(&session).ok_or("text")?;
        assert_eq!(t.size, disccraft_project::DEFAULT_TEXT_SIZE);
        assert_eq!(t.color, None);
        assert!(!t.format.bold);
        // A paragraph field goes to the whole text.
        assert_eq!(t.format.align, TextAlign::Center);
        assert_eq!(t.spans.len(), 1);
        let style = t.span_style_at(6);
        assert_eq!(
            (style.size, style.color, style.bold),
            (Some(60), Some([255, 0, 0]), Some(true))
        );
        // Null takes a field back to the text's.
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "from": 6, "to": 8, "color": null}),
        )?;
        let t = text(&session).ok_or("text")?;
        assert_eq!(t.span_style_at(6).color, None);
        assert_eq!(t.span_style_at(9).color, Some([255, 0, 0]));
        // Edited: the span moves with its characters.
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "text": "Oh, Hello world"}),
        )?;
        let t = text(&session).ok_or("text")?;
        assert_eq!(t.span_style_at(10).size, Some(60));
        assert_eq!(t.span_style_at(9).size, None);
        // The size for the whole text: every character's.
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "size": 40}),
        )?;
        let t = text(&session).ok_or("text")?;
        assert_eq!(t.size, 40);
        assert_eq!(t.span_style_at(10).size, None);
        assert_eq!(t.span_style_at(10).bold, Some(true));
        // Undone, the characters have their formatting back.
        session.execute("edit.undo", &json!({}))?;
        assert_eq!(
            text(&session).ok_or("text")?.span_style_at(10).size,
            Some(60)
        );
        for bad in [
            json!({"menu": menu, "index": 0, "from": 3, "to": 3, "bold": true}),
            json!({"menu": menu, "index": 0, "from": 3, "to": 99, "bold": true}),
            json!({"menu": menu, "index": 0, "from": 3, "bold": true}),
            json!({"menu": menu, "index": 0, "from": 0, "to": 2, "text": "x"}),
        ] {
            assert!(session.execute("menu.setText", &bad).is_err(), "{bad}");
        }
        Ok(())
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

    /// M6.266: a text gets a box to wrap in, loses it with null, and takes a kinsoku set; widths out of range
    /// and unknown sets are refused.
    #[test]
    fn a_text_gets_a_box_and_a_kinsoku_set() -> Result<(), Box<dyn Error>> {
        use disccraft_project::TextKinsoku;
        let (mut session, menu) = session_with_menu()?;
        session.execute(
            "menu.addText",
            &json!({"menu": menu, "text": "Welcome", "x": 40, "y": 30, "boxWidth": 300}),
        )?;
        let text = |session: &Session| {
            session
                .project
                .menus
                .values()
                .next()
                .and_then(|menu| menu.texts.first().cloned())
        };
        let added = text(&session).ok_or("no text")?;
        assert_eq!(added.box_width, Some(300));
        assert_eq!(added.format.kinsoku, TextKinsoku::Strong);
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "boxWidth": 120, "kinsoku": "weak"}),
        )?;
        let changed = text(&session).ok_or("no text")?;
        assert_eq!(changed.box_width, Some(120));
        assert_eq!(changed.format.kinsoku, TextKinsoku::Weak);
        // Leaving the box out keeps it; null takes it away.
        session.execute("menu.setText", &json!({"menu": menu, "index": 0, "x": 50}))?;
        assert_eq!(text(&session).ok_or("no text")?.box_width, Some(120));
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "boxWidth": null}),
        )?;
        assert_eq!(text(&session).ok_or("no text")?.box_width, None);
        session.execute("edit.undo", &json!({}))?;
        assert_eq!(text(&session).ok_or("no text")?.box_width, Some(120));
        for bad in [
            json!({"menu": menu, "index": 0, "boxWidth": 3}),
            json!({"menu": menu, "index": 0, "boxWidth": 70000}),
            json!({"menu": menu, "index": 0, "boxWidth": "wide"}),
            json!({"menu": menu, "index": 0, "kinsoku": "none at all"}),
        ] {
            assert!(session.execute("menu.setText", &bad).is_err(), "{bad}");
        }
        Ok(())
    }

    /// M6.298: a paragraph text's box gets a height and loses it with null; point text has none (taking the box
    /// away takes the height too); heights out of range are refused.
    #[test]
    fn a_text_box_gets_a_height() -> Result<(), Box<dyn Error>> {
        let (mut session, menu) = session_with_menu()?;
        session.execute(
            "menu.addText",
            &json!({"menu": menu, "text": "Welcome", "x": 40, "y": 30, "boxWidth": 300}),
        )?;
        let height = |session: &Session| {
            session
                .project
                .menus
                .values()
                .next()
                .and_then(|menu| menu.texts.first().map(|text| text.box_height))
        };
        assert_eq!(height(&session), Some(None));
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "boxHeight": 80}),
        )?;
        assert_eq!(height(&session), Some(Some(80)));
        session.execute("menu.setText", &json!({"menu": menu, "index": 0, "x": 50}))?;
        assert_eq!(height(&session), Some(Some(80)));
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "boxHeight": null}),
        )?;
        assert_eq!(height(&session), Some(None));
        session.execute("edit.undo", &json!({}))?;
        assert_eq!(height(&session), Some(Some(80)));
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "boxWidth": null}),
        )?;
        assert_eq!(height(&session), Some(None));
        for bad in [3, 70000] {
            assert!(
                session
                    .execute(
                        "menu.setText",
                        &json!({"menu": menu, "index": 0, "boxWidth": 200, "boxHeight": bad}),
                    )
                    .is_err()
            );
        }
        Ok(())
    }

    /// M6.304: a whole text object is placed (a Library text item): its look and box come with it, it lands where
    /// asked inside the frame, shown and unlocked; a text of nothing, or no text object at all, is refused.
    #[test]
    fn a_text_object_is_placed_whole() -> Result<(), Box<dyn Error>> {
        let (mut session, menu) = session_with_menu()?;
        session.execute(
            "menu.addText",
            &json!({"menu": menu, "text": "Chapters", "x": 40, "y": 30, "size": 36, "boxWidth": 200}),
        )?;
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "bold": true, "align": "center", "boxHeight": 90}),
        )?;
        let mut saved = session
            .project
            .menus
            .values()
            .next()
            .and_then(|found| found.texts.first().cloned())
            .ok_or("no text")?;
        saved.hidden = true;
        saved.locked = true;
        let placed = session.execute(
            "menu.placeText",
            &json!({"menu": menu, "text": saved, "x": 300, "y": 200}),
        )?;
        assert_eq!(
            placed.get("index").and_then(serde_json::Value::as_u64),
            Some(1)
        );
        let copy = session
            .project
            .menus
            .values()
            .next()
            .and_then(|found| found.texts.get(1).cloned())
            .ok_or("no copy")?;
        assert_eq!((copy.x, copy.y, copy.size), (300, 200, 36));
        assert_eq!((copy.box_width, copy.box_height), (Some(200), Some(90)));
        assert!(copy.format.bold && !copy.hidden && !copy.locked);
        assert_eq!(copy.text, "Chapters");
        // Undone: gone.
        session.execute("edit.undo", &json!({}))?;
        assert_eq!(
            session
                .project
                .menus
                .values()
                .next()
                .map(|found| found.texts.len()),
            Some(1)
        );
        let mut empty = saved.clone();
        empty.text = String::new();
        for bad in [
            json!({"menu": menu, "text": empty}),
            json!({"menu": menu, "text": 3}),
        ] {
            assert!(session.execute("menu.placeText", &bad).is_err(), "{bad}");
        }
        Ok(())
    }

    /// M6.311: a new text takes the characters it is given (the Character panel's for the next text): its font,
    /// style and format, in the one step that adds it.
    #[test]
    fn a_new_text_takes_its_characters() -> Result<(), Box<dyn Error>> {
        use disccraft_project::{TextAlign, TextKinsokuMethod};
        let (mut session, menu) = session_with_menu()?;
        session.execute(
            "menu.addText",
            &json!({"menu": menu, "text": "Chapters", "x": 40, "y": 30, "size": 48, "font": "Serif",
                "fontStyle": "Bold", "bold": true, "align": "center", "tracking": 50,
                "kinsokuMethod": "pushOutOnly"}),
        )?;
        let added = session
            .project
            .menus
            .values()
            .next()
            .and_then(|found| found.texts.first().cloned())
            .ok_or("no text")?;
        assert_eq!(added.size, 48);
        assert_eq!(
            (added.font.as_deref(), added.font_style.as_deref()),
            (Some("Serif"), Some("Bold"))
        );
        assert!(added.format.bold);
        assert_eq!(added.format.align, TextAlign::Center);
        assert_eq!(added.format.tracking, 50);
        assert_eq!(added.format.kinsoku_method, TextKinsokuMethod::PushOutOnly);
        // One step: undone, the menu has no text.
        session.execute("edit.undo", &json!({}))?;
        assert_eq!(
            session
                .project
                .menus
                .values()
                .next()
                .map(|found| found.texts.len()),
            Some(0)
        );
        assert!(
            session
                .execute(
                    "menu.addText",
                    &json!({"menu": menu, "text": "x", "x": 1, "y": 1, "align": "sideways"}),
                )
                .is_err()
        );
        Ok(())
    }

    /// M6.296: Encore's 禁則方法 and ぶら下がり: 追い込み優先 and no hanging at first, set by name, undone; a
    /// method of another name is refused.
    #[test]
    fn a_text_gets_a_kinsoku_method_and_hanging_punctuation() -> Result<(), Box<dyn Error>> {
        use disccraft_project::TextKinsokuMethod;
        let (mut session, menu) = session_with_menu()?;
        session.execute(
            "menu.addText",
            &json!({"menu": menu, "text": "ようこそ。", "x": 40, "y": 30, "boxWidth": 60}),
        )?;
        let format = |session: &Session| {
            session
                .project
                .menus
                .values()
                .next()
                .and_then(|menu| menu.texts.first().map(|text| text.format))
        };
        let added = format(&session).ok_or("no text")?;
        assert_eq!(added.kinsoku_method, TextKinsokuMethod::PushInFirst);
        assert!(!added.hanging_punctuation);
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "kinsokuMethod": "pushOutOnly", "hangingPunctuation": true}),
        )?;
        let changed = format(&session).ok_or("no text")?;
        assert_eq!(changed.kinsoku_method, TextKinsokuMethod::PushOutOnly);
        assert!(changed.hanging_punctuation);
        session.execute("edit.undo", &json!({}))?;
        assert_eq!(format(&session), Some(added));
        assert!(
            session
                .execute(
                    "menu.setText",
                    &json!({"menu": menu, "index": 0, "kinsokuMethod": "pushAround"}),
                )
                .is_err()
        );
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

    /// M6.344: Encore's カーニング: metrics, none, or a number between every two characters; the style read of the
    /// text gives it back the same way.
    #[test]
    fn kerning_is_metrics_or_a_number() -> Result<(), Box<dyn Error>> {
        let (mut session, menu) = session_with_menu()?;
        session.execute(
            "menu.addText",
            &json!({"menu": menu, "text": "AV", "x": 10, "y": 10}),
        )?;
        let kerning = |session: &Session| {
            session
                .project
                .menu(MenuId(menu))
                .and_then(|menu| menu.texts.first())
                .map(|text| (text.format.kerning, text.format.manual_kerning))
        };
        assert_eq!(kerning(&session), Some((true, 0)));
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "kerning": -50}),
        )?;
        assert_eq!(kerning(&session), Some((false, -50)));
        let style = session.execute("menu.styleOf", &json!({"menu": menu, "text": 0}))?;
        assert_eq!(style.pointer("/style/kerning"), Some(&json!(-50)));
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "kerning": false}),
        )?;
        assert_eq!(kerning(&session), Some((false, 0)));
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "kerning": true}),
        )?;
        assert_eq!(kerning(&session), Some((true, 0)));
        for bad in [json!(5000), json!("tight")] {
            assert!(
                session
                    .execute(
                        "menu.setText",
                        &json!({"menu": menu, "index": 0, "kerning": bad})
                    )
                    .is_err()
            );
        }
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

    /// M6.313: Encore's 行送り — a pitch in pixels, null for 自動; out of range is refused; left out keeps it.
    #[test]
    fn the_line_pitch_is_set_or_automatic() -> Result<(), Box<dyn Error>> {
        let (mut session, menu) = session_with_menu()?;
        session.execute(
            "menu.addText",
            &json!({"menu": menu, "text": "Title", "x": 10, "y": 10}),
        )?;
        let pitch = |session: &Session| {
            session
                .project
                .menu(MenuId(menu))
                .and_then(|menu| menu.texts.first())
                .map(|text| (text.format.line_pitch, text.format.leading))
        };
        assert_eq!(pitch(&session), Some((None, 0)));
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "linePitch": 60, "leading": 0}),
        )?;
        assert_eq!(pitch(&session), Some((Some(60), 0)));
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "bold": true}),
        )?;
        assert_eq!(pitch(&session), Some((Some(60), 0)));
        for bad in [json!(0), json!(1001), json!(-5), json!("tall")] {
            let params = json!({"menu": menu, "index": 0, "linePitch": bad});
            assert!(
                session.execute("menu.setText", &params).is_err(),
                "{params}"
            );
        }
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "linePitch": null}),
        )?;
        assert_eq!(pitch(&session), Some((None, 0)));
        session.undo()?;
        assert_eq!(pitch(&session), Some((Some(60), 0)));
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
