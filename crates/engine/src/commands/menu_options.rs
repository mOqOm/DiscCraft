//! The project's menu options (Encore's Preferences ▸ Menus): how automatic routing moves and wraps, and how new
//! buttons are named.

use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::WrapMode;
use serde_json::Value;

use super::{always_enabled, empty_object};

pub(crate) const COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "disc.setMenuOptions",
    label: "メニューの環境設定を変更",
    menu: &["編集", "環境設定"],
    shortcut: None,
    params: r#"{"vertical"?: bool, "verticalWrap"?: bool, "verticalWrapMode"?: "same" | "next", "horizontal"?: bool, "horizontalWrap"?: bool, "horizontalWrapMode"?: "same" | "next", "excludeNextPrevious"?: bool, "syncButtonNames"?: bool, "nameButtonsFromLinks"?: bool}"#,
    enabled: always_enabled,
    run: run_set_menu_options,
    journal: true,
}];

/// An optional boolean parameter.
fn flag(params: &Value, name: &str) -> Result<Option<bool>, String> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(format!("parameter \"{name}\" must be true or false")),
    }
}

/// An optional wrap mode parameter.
fn mode(params: &Value, name: &str) -> Result<Option<WrapMode>, String> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => WrapMode::parse(text)
            .map(Some)
            .ok_or_else(|| format!("parameter \"{name}\" must be \"same\" or \"next\"")),
        Some(_) => Err(format!("parameter \"{name}\" must be \"same\" or \"next\"")),
    }
}

fn run_set_menu_options(session: &mut Session, params: &Value) -> CommandResult {
    let vertical = flag(params, "vertical")?;
    let vertical_wrap = flag(params, "verticalWrap")?;
    let vertical_mode = mode(params, "verticalWrapMode")?;
    let horizontal = flag(params, "horizontal")?;
    let horizontal_wrap = flag(params, "horizontalWrap")?;
    let horizontal_mode = mode(params, "horizontalWrapMode")?;
    let exclude = flag(params, "excludeNextPrevious")?;
    let sync_names = flag(params, "syncButtonNames")?;
    let from_links = flag(params, "nameButtonsFromLinks")?;
    let settings = &mut session.project.settings;
    let routing = &mut settings.routing;
    if let Some(value) = vertical {
        routing.vertical = value;
    }
    if let Some(value) = vertical_wrap {
        routing.vertical_wrap = value;
    }
    if let Some(value) = vertical_mode {
        routing.vertical_wrap_mode = value;
    }
    if let Some(value) = horizontal {
        routing.horizontal = value;
    }
    if let Some(value) = horizontal_wrap {
        routing.horizontal_wrap = value;
    }
    if let Some(value) = horizontal_mode {
        routing.horizontal_wrap_mode = value;
    }
    if let Some(value) = exclude {
        routing.exclude_next_previous = value;
    }
    if let Some(value) = sync_names {
        settings.button_defaults.sync_name = value;
    }
    if let Some(value) = from_links {
        settings.button_defaults.name_from_link = value;
    }
    Ok(empty_object())
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use disccraft_project::{MenuId, RouteOptions, WrapMode};
    use serde_json::json;
    use std::error::Error;

    #[test]
    fn options_are_set_and_new_buttons_follow_them() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        session.execute(
            "disc.setMenuOptions",
            &json!({"verticalWrapMode": "next", "horizontal": false, "syncButtonNames": false, "nameButtonsFromLinks": true}),
        )?;
        let settings = &session.project.settings;
        assert_eq!(
            settings.routing,
            RouteOptions {
                vertical_wrap_mode: WrapMode::NextLine,
                horizontal: false,
                ..RouteOptions::default()
            }
        );
        let created = session.execute("menu.create", &json!({"name": "Main"}))?;
        let menu = created
            .get("menu")
            .and_then(serde_json::Value::as_u64)
            .ok_or("no menu id")?;
        session.execute(
            "menu.addButton",
            &json!({"menu": menu, "label": "Play", "target": {"menu": menu}}),
        )?;
        let button = session
            .project
            .menu(MenuId(menu))
            .and_then(|menu| menu.buttons.first().cloned())
            .ok_or("no button")?;
        assert!(!button.sync_name && button.name_from_link);
        // The name comes from the link: the menu's own name.
        assert_eq!(button.name, "Main");
        session.execute("edit.undo", &json!({}))?;
        session.execute("edit.undo", &json!({}))?;
        session.execute("edit.undo", &json!({}))?;
        assert_eq!(session.project.settings.routing, RouteOptions::default());
        for params in [
            json!({"vertical": "yes"}),
            json!({"verticalWrapMode": "diagonal"}),
        ] {
            assert!(session.execute("disc.setMenuOptions", &params).is_err());
        }
        Ok(())
    }
}
