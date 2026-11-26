//! Text styles (Encore: the Styles panel applies a predefined look to the selected object). A style here is the
//! look of text: its font, size, colour, character formatting and drop shadow, as the fields of `menu.setText`
//! (and `"shadow"` as `menu.setShadow` takes it). `menu.applyStyle` puts a style on a text object or on a
//! button's text in one step; `menu.styleOf` reads the look of one back, to keep it as a style.

use serde_json::{Map, Value};

use disccraft_project::{DropShadow, MenuId, TextFormat};

use crate::params::{opt_u64, req_u64};
use crate::{CommandResult, CommandSpec, Session};

use super::{always_enabled, object};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "menu.applyStyle",
        label: "Apply Style",
        menu: &["Object"],
        shortcut: None,
        params: r##"{"menu": id, "text" | "button" | "graphic": index, "style": {"size"?, "color"?, "font"?, "fontStyle"?, "bold"?, … (the character settings of menu.setText), "shadow"?: {…} | null, "effects"?: {…} | null (an image's or a shape's layer effects)}}"##,
        enabled: always_enabled,
        run: run_apply_style,
        journal: true,
    },
    CommandSpec {
        id: "menu.styleOf",
        label: "Get Style",
        menu: &["Object"],
        shortcut: None,
        params: r#"{"menu": id, "text" | "button" | "graphic": index}"#,
        enabled: always_enabled,
        run: run_style_of,
        journal: false,
    },
];

/// The fields of a style a text object takes (`menu.setText`): everything of its look but the text and place.
const TEXT_FIELDS: [&str; 24] = [
    "size",
    "color",
    "font",
    "fontStyle",
    "bold",
    "italic",
    "underline",
    "strikethrough",
    "caps",
    "align",
    "tracking",
    "linePitch",
    "leading",
    "kerning",
    "baseline",
    "verticalScale",
    "horizontalScale",
    "script",
    "antialias",
    "tsume",
    "mojikumi",
    "kinsoku",
    "kinsokuMethod",
    "hangingPunctuation",
];
/// The fields a button's text takes (`button.setLabelFormat`): its size, colour and font are the menu's.
const BUTTON_FIELDS: [&str; 21] = [
    "fontStyle",
    "bold",
    "italic",
    "underline",
    "strikethrough",
    "caps",
    "align",
    "tracking",
    "linePitch",
    "leading",
    "kerning",
    "baseline",
    "verticalScale",
    "horizontalScale",
    "script",
    "antialias",
    "tsume",
    "mojikumi",
    "kinsoku",
    "kinsokuMethod",
    "hangingPunctuation",
];

/// The text object or the button a command names: `(is_button, index)`; `None` for an image or a shape
/// ([`graphic_target`]).
fn target(params: &Value) -> Result<(bool, usize), String> {
    let index = |name: &str| -> Result<Option<usize>, String> {
        opt_u64(params, name)?
            .map(|index| usize::try_from(index).map_err(|_| format!("\"{name}\" is too large")))
            .transpose()
    };
    match (index("text")?, index("button")?) {
        (Some(text), None) => Ok((false, text)),
        (None, Some(button)) => Ok((true, button)),
        _ => Err("give exactly one of \"text\", \"button\" and \"graphic\"".to_string()),
    }
}

/// The image or shape a command names, when it names one (and nothing else).
fn graphic_target(params: &Value) -> Result<Option<usize>, String> {
    let Some(graphic) = opt_u64(params, "graphic")? else {
        return Ok(None);
    };
    if params.get("text").is_some() || params.get("button").is_some() {
        return Err("give exactly one of \"text\", \"button\" and \"graphic\"".to_string());
    }
    usize::try_from(graphic)
        .map(Some)
        .map_err(|_| "\"graphic\" is too large".to_string())
}

/// A style on an image or a shape (M6.332): its layer effects and drop shadow, each when the style has it.
fn apply_graphic_style(
    session: &mut Session,
    menu: u64,
    graphic: usize,
    style: &Map<String, Value>,
) -> CommandResult {
    let exists = session
        .project
        .menu(MenuId(menu))
        .is_some_and(|found| graphic < found.graphics.len());
    if !exists {
        return Err(format!("graphic {graphic} does not exist"));
    }
    if let Some(effects) = style.get("effects") {
        let mut fields = Map::new();
        let _ = fields.insert("menu".to_owned(), Value::from(menu));
        let _ = fields.insert("graphic".to_owned(), Value::from(graphic));
        let _ = fields.insert("effects".to_owned(), effects.clone());
        let _ = run_inner(session, "menu.setEffects", &Value::Object(fields))?;
    }
    if let Some(shadow) = style.get("shadow") {
        let mut fields = Map::new();
        let _ = fields.insert("menu".to_owned(), Value::from(menu));
        let _ = fields.insert("graphic".to_owned(), Value::from(graphic));
        let _ = fields.insert("shadow".to_owned(), shadow.clone());
        let _ = run_inner(session, "menu.setShadow", &Value::Object(fields))?;
    }
    Ok(super::empty_object())
}

fn run_apply_style(session: &mut Session, params: &Value) -> CommandResult {
    let menu = req_u64(params, "menu")?;
    let style = params
        .get("style")
        .and_then(Value::as_object)
        .ok_or_else(|| "parameter \"style\" must be an object".to_string())?;
    if let Some(graphic) = graphic_target(params)? {
        return apply_graphic_style(session, menu, graphic, style);
    }
    let (is_button, index) = target(params)?;
    // The fields the target takes, as its own command reads them (so every value is checked as there).
    let mut fields = Map::new();
    let _ = fields.insert("menu".to_owned(), Value::from(menu));
    let _ = fields.insert(
        if is_button { "button" } else { "index" }.to_owned(),
        Value::from(index),
    );
    let allowed: &[&str] = if is_button {
        &BUTTON_FIELDS
    } else {
        &TEXT_FIELDS
    };
    for name in allowed {
        if let Some(value) = style.get(*name) {
            let _ = fields.insert((*name).to_owned(), value.clone());
        }
    }
    let command = if is_button {
        "button.setLabelFormat"
    } else {
        "menu.setText"
    };
    let _ = run_inner(session, command, &Value::Object(fields))?;
    if let Some(shadow) = style.get("shadow") {
        let mut fields = Map::new();
        let _ = fields.insert("menu".to_owned(), Value::from(menu));
        let _ = fields.insert(
            if is_button { "button" } else { "text" }.to_owned(),
            Value::from(index),
        );
        let _ = fields.insert("shadow".to_owned(), shadow.clone());
        let _ = run_inner(session, "menu.setShadow", &Value::Object(fields))?;
    }
    Ok(super::empty_object())
}

/// Runs another command's function inside this one (one undo step for the whole style).
fn run_inner(session: &mut Session, id: &str, params: &Value) -> CommandResult {
    let spec = super::find_command(id).ok_or_else(|| format!("unknown command: {id}"))?;
    (spec.run)(session, params)
}

fn format_fields(format: &TextFormat, fields: &mut Map<String, Value>) {
    let mut put = |name: &str, value: Value| {
        let _ = fields.insert(name.to_owned(), value);
    };
    put("bold", Value::from(format.bold));
    put("italic", Value::from(format.italic));
    put("underline", Value::from(format.underline));
    put("strikethrough", Value::from(format.strikethrough));
    put("caps", Value::from(format.caps.as_str()));
    put("align", Value::from(format.align.as_str()));
    put("tracking", Value::from(format.tracking));
    put(
        "linePitch",
        format.line_pitch.map_or(Value::Null, Value::from),
    );
    put("leading", Value::from(format.leading));
    // Encore's カーニング: metrics, or the number (M6.344).
    put(
        "kerning",
        if format.kerning {
            Value::from(true)
        } else {
            Value::from(format.manual_kerning)
        },
    );
    put("baseline", Value::from(format.baseline));
    put("verticalScale", Value::from(format.vertical_scale));
    put("horizontalScale", Value::from(format.horizontal_scale));
    put("script", Value::from(format.script.as_str()));
    put("antialias", Value::from(format.antialias.as_str()));
    put("tsume", Value::from(format.tsume));
    put("mojikumi", Value::from(format.mojikumi.as_str()));
    put("kinsoku", Value::from(format.kinsoku.as_str()));
    put("kinsokuMethod", Value::from(format.kinsoku_method.as_str()));
    put(
        "hangingPunctuation",
        Value::from(format.hanging_punctuation),
    );
}

fn shadow_value(shadow: Option<&DropShadow>) -> Value {
    match shadow {
        None => Value::Null,
        Some(shadow) => {
            let [r, g, b] = shadow.color;
            serde_json::json!({
                "color": format!("#{r:02X}{g:02X}{b:02X}"),
                "opacity": shadow.opacity,
                "angle": shadow.angle,
                "distance": shadow.distance,
                "size": shadow.size,
                "spread": shadow.spread,
            })
        }
    }
}

fn run_style_of(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let menu = session
        .project
        .menu(menu_id)
        .ok_or_else(|| format!("menu {} does not exist", menu_id.0))?;
    // An image's or a shape's: its layer effects and drop shadow (M6.332).
    if let Some(index) = graphic_target(params)? {
        let graphic = menu
            .graphics
            .get(index)
            .ok_or_else(|| format!("graphic {index} does not exist"))?;
        let effects = serde_json::to_value(graphic.effects).map_err(|error| error.to_string())?;
        let mut fields = Map::new();
        let _ = fields.insert("effects".to_owned(), effects);
        let _ = fields.insert("shadow".to_owned(), shadow_value(graphic.shadow.as_ref()));
        return Ok(object(&[("style", Value::Object(fields))]));
    }
    let (is_button, index) = target(params)?;
    let mut fields = Map::new();
    if is_button {
        let button = menu
            .buttons
            .get(index)
            .ok_or_else(|| format!("button {index} does not exist"))?;
        let _ = fields.insert(
            "fontStyle".to_owned(),
            button
                .label_font_style
                .clone()
                .map_or(Value::Null, Value::from),
        );
        format_fields(&button.label_format, &mut fields);
        let _ = fields.insert("shadow".to_owned(), shadow_value(button.shadow.as_ref()));
    } else {
        let text = menu
            .texts
            .get(index)
            .ok_or_else(|| format!("text {index} does not exist"))?;
        let _ = fields.insert("size".to_owned(), Value::from(text.size));
        let _ = fields.insert(
            "color".to_owned(),
            text.color.map_or(Value::Null, |[r, g, b]| {
                Value::from(format!("#{r:02X}{g:02X}{b:02X}"))
            }),
        );
        let _ = fields.insert(
            "font".to_owned(),
            text.font.clone().map_or(Value::Null, Value::from),
        );
        let _ = fields.insert(
            "fontStyle".to_owned(),
            text.font_style.clone().map_or(Value::Null, Value::from),
        );
        format_fields(&text.format, &mut fields);
        let _ = fields.insert("shadow".to_owned(), shadow_value(text.shadow.as_ref()));
    }
    Ok(object(&[("style", Value::Object(fields))]))
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use serde_json::{Value, json};

    /// M6.342: a text object and a button's text have layer effects too, and they go with it when the text becomes
    /// a button and the button a text again.
    #[test]
    fn texts_and_buttons_have_layer_effects() -> Result<(), Box<dyn std::error::Error>> {
        let mut session = Session::new();
        let menu = session
            .execute("menu.create", &Value::Null)?
            .get("menu")
            .and_then(Value::as_u64)
            .ok_or("menu")?;
        session.execute(
            "menu.addText",
            &json!({"menu": menu, "text": "Play", "x": 100, "y": 100, "size": 40}),
        )?;
        let glow =
            json!({"outerGlow": {"size": 8, "spread": 10, "color": [255, 255, 0], "opacity": 80}});
        session.execute(
            "menu.setEffects",
            &json!({"menu": menu, "text": 0, "effects": glow}),
        )?;
        let found = |session: &Session| {
            session
                .project
                .menu(disccraft_project::MenuId(menu))
                .cloned()
        };
        let set = found(&session)
            .and_then(|menu| menu.texts.first().map(|text| text.effects))
            .ok_or("text")?;
        assert_eq!(set.outer_glow.map(|glow| glow.size), Some(8));
        // Text to button: the effects go along.
        session.execute("menu.convertToButton", &json!({"menu": menu, "text": 0}))?;
        let button = found(&session)
            .and_then(|menu| menu.buttons.first().map(|button| button.effects))
            .ok_or("button")?;
        assert_eq!(button, set);
        // A button's own effects, and back to a text.
        session.execute(
            "menu.setEffects",
            &json!({"menu": menu, "button": 0, "effects": {"stroke": {"size": 2, "color": [0, 0, 0], "opacity": 100}}}),
        )?;
        session.execute("menu.convertToObject", &json!({"menu": menu, "button": 0}))?;
        let text = found(&session)
            .and_then(|menu| menu.texts.first().map(|text| text.effects))
            .ok_or("text")?;
        assert_eq!(text.stroke.map(|stroke| stroke.size), Some(2));
        for bad in [
            json!({"menu": menu, "text": 5, "effects": null}),
            json!({"menu": menu, "text": 0, "button": 0, "effects": null}),
            json!({"menu": menu, "effects": null}),
        ] {
            assert!(session.execute("menu.setEffects", &bad).is_err(), "{bad}");
        }
        Ok(())
    }

    /// M6.332: an image's or a shape's style is its layer effects and drop shadow: read of one, put on another in
    /// one step, taken away with null; effects out of range are brought in, ones that are not effects refused.
    #[test]
    fn a_shapes_style_is_its_layer_effects() -> Result<(), Box<dyn std::error::Error>> {
        let mut session = Session::new();
        let menu = session
            .execute("menu.create", &Value::Null)?
            .get("menu")
            .and_then(Value::as_u64)
            .ok_or("menu")?;
        for x in [40, 300] {
            session.execute(
                "menu.addShape",
                &json!({"menu": menu, "shape": "rectangle", "x": x, "y": 100, "w": 120, "h": 80}),
            )?;
        }
        session.execute(
            "menu.setEffects",
            &json!({"menu": menu, "graphic": 0, "effects": {
                "stroke": {"size": 80, "color": [255, 255, 255], "opacity": 100},
                "bevel": {"size": 6, "angle": 120, "highlight": 75, "shadow": 75}
            }}),
        )?;
        let effects = |session: &Session, index: usize| {
            session
                .project
                .menu(disccraft_project::MenuId(menu))
                .and_then(|found| found.graphics.get(index))
                .map(|graphic| graphic.effects)
        };
        let first = effects(&session, 0).ok_or("effects")?;
        // The stroke's size is brought inside its range.
        assert_eq!(
            first.stroke.map(|stroke| stroke.size),
            Some(disccraft_project::MAX_EFFECT_SIZE)
        );
        let style = session
            .execute("menu.styleOf", &json!({"menu": menu, "graphic": 0}))?
            .get("style")
            .cloned()
            .ok_or("style")?;
        assert!(style.get("effects").is_some() && style.get("shadow").is_some());
        session.execute(
            "menu.applyStyle",
            &json!({"menu": menu, "graphic": 1, "style": style}),
        )?;
        assert_eq!(effects(&session, 1), Some(first));
        // One step back: the second shape plain again.
        session.execute("edit.undo", &Value::Null)?;
        assert!(effects(&session, 1).ok_or("effects")?.is_empty());
        session.execute(
            "menu.setEffects",
            &json!({"menu": menu, "graphic": 0, "effects": null}),
        )?;
        assert!(effects(&session, 0).ok_or("effects")?.is_empty());
        for bad in [
            json!({"menu": menu, "graphic": 0, "effects": {"stroke": "thick"}}),
            json!({"menu": menu, "graphic": 9, "effects": null}),
        ] {
            assert!(session.execute("menu.setEffects", &bad).is_err(), "{bad}");
        }
        assert!(
            session
                .execute(
                    "menu.styleOf",
                    &json!({"menu": menu, "graphic": 0, "text": 0})
                )
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn a_style_is_read_of_a_text_and_put_on_another_in_one_step()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut session = Session::new();
        let menu = session
            .execute("menu.create", &Value::Null)?
            .get("menu")
            .and_then(Value::as_u64)
            .ok_or("menu")?;
        for text in ["Title", "Other"] {
            session.execute(
                "menu.addText",
                &json!({"menu": menu, "text": text, "x": 40, "y": 40}),
            )?;
        }
        session.execute("menu.addButton", &json!({"menu": menu, "label": "Play"}))?;
        // The first text gets a look of its own.
        session.execute(
            "menu.setText",
            &json!({"menu": menu, "index": 0, "size": 48, "color": "#FFE680", "bold": true, "tracking": 50, "caps": "small"}),
        )?;
        session.execute(
            "menu.setShadow",
            &json!({"menu": menu, "text": 0, "shadow": {"opacity": 60, "distance": 7}}),
        )?;
        let style = session
            .execute("menu.styleOf", &json!({"menu": menu, "text": 0}))?
            .get("style")
            .cloned()
            .ok_or("style")?;
        assert_eq!(style.get("size"), Some(&json!(48)));
        assert_eq!(style.get("color"), Some(&json!("#FFE680")));
        assert_eq!(style.pointer("/shadow/distance"), Some(&json!(7)));

        // Put on the second text: the same look, its own words and place.
        session.execute(
            "menu.applyStyle",
            &json!({"menu": menu, "text": 1, "style": style}),
        )?;
        let texts = |session: &Session| {
            session
                .project
                .menu(disccraft_project::MenuId(menu))
                .map(|menu| menu.texts.clone())
                .unwrap_or_default()
        };
        let (first, second) = (texts(&session)[0].clone(), texts(&session)[1].clone());
        assert_eq!(second.text, "Other");
        assert_eq!(
            (second.size, second.color, second.format, second.shadow),
            (first.size, first.color, first.format, first.shadow)
        );
        assert!(second.shadow.is_some());
        // One step back takes the whole style off again.
        session.execute("edit.undo", &Value::Null)?;
        let plain = texts(&session)[1].clone();
        assert!(plain.shadow.is_none() && !plain.format.bold && plain.color.is_none());

        // On a button: the formatting and the shadow; its size, colour and font stay the menu's.
        session.execute(
            "menu.applyStyle",
            &json!({"menu": menu, "button": 0, "style": style}),
        )?;
        let button = session
            .project
            .menu(disccraft_project::MenuId(menu))
            .and_then(|menu| menu.buttons.first().cloned())
            .ok_or("button")?;
        assert!(button.label_format.bold && button.label_format.tracking == 50);
        assert!(button.shadow.is_some());
        // A style may take the shadow off, and a bad value is refused as its own command refuses it.
        session.execute(
            "menu.applyStyle",
            &json!({"menu": menu, "button": 0, "style": {"bold": false, "shadow": null}}),
        )?;
        let button = session
            .project
            .menu(disccraft_project::MenuId(menu))
            .and_then(|menu| menu.buttons.first().cloned())
            .ok_or("button")?;
        assert!(!button.label_format.bold && button.shadow.is_none());
        for bad in [
            json!({"menu": menu, "text": 1, "style": {"size": 9999}}),
            json!({"menu": menu, "text": 1, "button": 0, "style": {}}),
            json!({"menu": menu, "text": 9, "style": {"bold": true}}),
            json!({"menu": menu, "text": 1}),
        ] {
            assert!(session.execute("menu.applyStyle", &bad).is_err(), "{bad}");
        }
        Ok(())
    }
}
