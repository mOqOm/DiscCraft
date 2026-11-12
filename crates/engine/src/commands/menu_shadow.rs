//! Drop shadows of menu objects (Encore's Object ▸ Drop Shadow…): a text, a button's text or a graphic.

use crate::params::{opt_u64, req_u64};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{DropShadow, MAX_SHADOW_DISTANCE, MAX_SHADOW_SIZE, MenuId};
use serde_json::Value;

use super::menu::parse_color;
use super::{always_enabled, empty_object};

pub(crate) const COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "menu.setShadow",
    label: "ドロップシャドウを設定",
    menu: &["オブジェクト"],
    shortcut: None,
    params: r##"{"menu": id, "text" | "button" | "graphic": index, "shadow": {"color"?: "#RRGGBB", "opacity"?: 0..100, "angle"?: 0..359, "distance"?: 0..100, "size"?: 0..100, "spread"?: 0..100} | null}"##,
    enabled: always_enabled,
    run: run_set_shadow,
    journal: true,
}];

/// Which object of the menu the shadow belongs to.
enum Target {
    Text(usize),
    Button(usize),
    Graphic(usize),
}

/// Exactly one of "text", "button" and "graphic".
fn target(params: &Value) -> Result<Target, String> {
    let mut found = Vec::new();
    for (name, make) in [
        ("text", Target::Text as fn(usize) -> Target),
        ("button", Target::Button as fn(usize) -> Target),
        ("graphic", Target::Graphic as fn(usize) -> Target),
    ] {
        if let Some(index) = opt_u64(params, name)? {
            let index = usize::try_from(index).map_err(|_| format!("\"{name}\" is too large"))?;
            found.push(make(index));
        }
    }
    if found.len() != 1 {
        return Err("give exactly one of \"text\", \"button\" and \"graphic\"".to_string());
    }
    found
        .pop()
        .ok_or_else(|| "give exactly one of \"text\", \"button\" and \"graphic\"".to_string())
}

/// A percentage, angle or pixel count parameter within `0..=max`.
fn opt_number(params: &Value, name: &str, max: u16) -> Result<Option<u16>, String> {
    match params.get(name) {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .filter(|n| *n <= max)
            .map(Some)
            .ok_or_else(|| format!("\"{name}\" must be 0 to {max}")),
    }
}

/// The "shadow" parameter: `None` to remove the shadow, or the fields given laid over `current` (or the default).
fn shadow_param(params: &Value, current: Option<DropShadow>) -> Result<Option<DropShadow>, String> {
    let Some(value) = params.get("shadow") else {
        return Err("parameter \"shadow\" is required (an object, or null)".to_string());
    };
    if value.is_null() {
        return Ok(None);
    }
    if !value.is_object() {
        return Err("\"shadow\" must be an object or null".to_string());
    }
    let mut shadow = current.unwrap_or_default();
    if let Some(color) = value.get("color") {
        let color = color
            .as_str()
            .ok_or_else(|| "\"color\" must be \"#RRGGBB\"".to_string())?;
        shadow.color = parse_color(color)?;
    }
    if let Some(opacity) = opt_number(value, "opacity", 100)? {
        shadow.opacity = u8::try_from(opacity).unwrap_or(100);
    }
    if let Some(angle) = opt_number(value, "angle", 359)? {
        shadow.angle = angle;
    }
    if let Some(distance) = opt_number(value, "distance", u16::from(MAX_SHADOW_DISTANCE))? {
        shadow.distance = u8::try_from(distance).unwrap_or(MAX_SHADOW_DISTANCE);
    }
    if let Some(size) = opt_number(value, "size", u16::from(MAX_SHADOW_SIZE))? {
        shadow.size = u8::try_from(size).unwrap_or(MAX_SHADOW_SIZE);
    }
    if let Some(spread) = opt_number(value, "spread", 100)? {
        shadow.spread = u8::try_from(spread).unwrap_or(100);
    }
    Ok(Some(shadow))
}

fn run_set_shadow(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let target = target(params)?;
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let slot: &mut Option<DropShadow> = match target {
        Target::Text(index) => {
            &mut menu
                .texts
                .get_mut(index)
                .ok_or_else(|| format!("text {index} does not exist"))?
                .shadow
        }
        Target::Button(index) => {
            &mut menu
                .buttons
                .get_mut(index)
                .ok_or_else(|| format!("button {index} does not exist"))?
                .shadow
        }
        Target::Graphic(index) => {
            &mut menu
                .graphics
                .get_mut(index)
                .ok_or_else(|| format!("graphic {index} does not exist"))?
                .shadow
        }
    };
    *slot = shadow_param(params, *slot)?;
    Ok(empty_object())
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use disccraft_project::{DropShadow, MenuId};
    use serde_json::json;

    fn session_with_menu() -> Result<(Session, u64), String> {
        let mut session = Session::new();
        let created = session.execute("menu.create", &json!({"name": "Main"}))?;
        let menu_id = created
            .get("menu")
            .and_then(serde_json::Value::as_u64)
            .ok_or("no menu id")?;
        session.execute(
            "menu.addText",
            &json!({"menu": menu_id, "text": "Title", "x": 10, "y": 10}),
        )?;
        session.execute("menu.addButton", &json!({"menu": menu_id, "label": "Play"}))?;
        Ok((session, menu_id))
    }

    #[test]
    fn sets_lays_over_and_clears_the_shadow_of_a_text() -> Result<(), String> {
        let (mut session, menu) = session_with_menu()?;
        session.execute(
            "menu.setShadow",
            &json!({"menu": menu, "text": 0, "shadow": {"opacity": 40, "color": "#ff0000"}}),
        )?;
        let text = |session: &Session| {
            session
                .project
                .menu(MenuId(menu))
                .and_then(|m| m.texts.first().cloned())
        };
        let shadow = text(&session).and_then(|t| t.shadow).ok_or("no shadow")?;
        assert_eq!(
            shadow,
            DropShadow {
                color: [255, 0, 0],
                opacity: 40,
                ..DropShadow::default()
            }
        );
        // Another call changes only the fields given.
        session.execute(
            "menu.setShadow",
            &json!({"menu": menu, "text": 0, "shadow": {"distance": 9}}),
        )?;
        let shadow = text(&session).and_then(|t| t.shadow).ok_or("no shadow")?;
        assert_eq!(
            (shadow.opacity, shadow.distance, shadow.color),
            (40, 9, [255, 0, 0])
        );
        session.execute(
            "menu.setShadow",
            &json!({"menu": menu, "text": 0, "shadow": null}),
        )?;
        assert!(text(&session).is_some_and(|t| t.shadow.is_none()));
        // Undo brings it back.
        session.execute("edit.undo", &json!({}))?;
        assert!(text(&session).is_some_and(|t| t.shadow.is_some()));
        Ok(())
    }

    #[test]
    fn a_button_gets_a_shadow_too() -> Result<(), String> {
        let (mut session, menu) = session_with_menu()?;
        session.execute(
            "menu.setShadow",
            &json!({"menu": menu, "button": 0, "shadow": {}}),
        )?;
        let button = session
            .project
            .menu(MenuId(menu))
            .and_then(|m| m.buttons.first().cloned())
            .ok_or("no button")?;
        assert_eq!(button.shadow, Some(DropShadow::default()));
        Ok(())
    }

    #[test]
    fn refuses_bad_targets_and_values() -> Result<(), String> {
        let (mut session, menu) = session_with_menu()?;
        for params in [
            json!({"menu": menu, "shadow": {}}),
            json!({"menu": menu, "text": 0, "button": 0, "shadow": {}}),
            json!({"menu": menu, "text": 5, "shadow": {}}),
            json!({"menu": menu, "graphic": 0, "shadow": {}}),
            json!({"menu": menu, "text": 0}),
            json!({"menu": menu, "text": 0, "shadow": {"opacity": 101}}),
            json!({"menu": menu, "text": 0, "shadow": {"angle": 360}}),
            json!({"menu": menu, "text": 0, "shadow": {"size": 101}}),
            json!({"menu": menu, "text": 0, "shadow": {"color": "red"}}),
            json!({"menu": menu, "text": 0, "shadow": 3}),
        ] {
            assert!(
                session.execute("menu.setShadow", &params).is_err(),
                "{params}"
            );
        }
        Ok(())
    }
}
