//! Commands that edit several buttons of a menu at once (paste, remove, move, align, distribute). Each is one
//! undo step.

use crate::params::{opt_value, req_str, req_u64, req_value};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{Align, Arrange, Axis, Button, Link, MenuId, Rect, arranged_order};
use serde_json::Value;

use super::{always_enabled, empty_object, object};

/// Batch button commands.
pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "menu.pasteButtons",
        label: "ボタンを貼り付け",
        menu: &["編集"],
        shortcut: None,
        params: r#"{"menu": id, "buttons": [{"label": string, "rect": {"x": int, "y": int, "w": int, "h": int}, "target"?: Link, "autoActivate"?: bool}]}"#,
        enabled: always_enabled,
        run: run_paste,
        journal: true,
    },
    CommandSpec {
        id: "menu.removeButtons",
        label: "ボタンをまとめて削除",
        menu: &["編集"],
        shortcut: None,
        params: r#"{"menu": id, "buttons": [index]}"#,
        enabled: always_enabled,
        run: run_remove,
        journal: true,
    },
    CommandSpec {
        id: "button.setRects",
        label: "ボタンをまとめて移動",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "rects": [{"button": index, "rect": {"x": int, "y": int, "w": int, "h": int}}]}"#,
        enabled: always_enabled,
        run: run_set_rects,
        journal: true,
    },
    CommandSpec {
        id: "menu.reorderButtons",
        label: "ボタンの番号を並べ替え",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "order": [index]}"#,
        enabled: always_enabled,
        run: run_reorder,
        journal: true,
    },
    CommandSpec {
        id: "menu.arrangeButtons",
        label: "アレンジ",
        menu: &["オブジェクト"],
        shortcut: None,
        params: r#"{"menu": id, "buttons": [index], "arrange": "front" | "forward" | "backward" | "back"}"#,
        enabled: always_enabled,
        run: run_arrange,
        journal: true,
    },
    CommandSpec {
        id: "menu.alignButtons",
        label: "整列",
        menu: &["オブジェクト"],
        shortcut: None,
        params: r#"{"menu": id, "buttons": [index], "align": "left" | "center" | "right" | "top" | "middle" | "bottom", "safeArea"?: bool}"#,
        enabled: always_enabled,
        run: run_align,
        journal: true,
    },
    CommandSpec {
        id: "menu.distributeButtons",
        label: "均等配置",
        menu: &["オブジェクト"],
        shortcut: None,
        params: r#"{"menu": id, "buttons": [index], "axis": "horizontal" | "vertical", "safeArea"?: bool}"#,
        enabled: always_enabled,
        run: run_distribute,
        journal: true,
    },
];

fn run_paste(session: &mut Session, params: &Value) -> CommandResult {
    let menu = MenuId(req_u64(params, "menu")?);
    let Value::Array(items) = req_value(params, "buttons")? else {
        return Err("parameter \"buttons\" must be an array".to_string());
    };
    let buttons = items
        .iter()
        .map(pasted_button)
        .collect::<Result<Vec<Button>, String>>()?;
    let added = session
        .project
        .paste_buttons(menu, buttons)
        .map_err(|error| error.to_string())?;
    let added: Vec<Value> = added
        .into_iter()
        .map(|index| Value::from(u64::try_from(index).unwrap_or(u64::MAX)))
        .collect();
    Ok(object(&[("buttons", Value::Array(added))]))
}

/// One element of `menu.pasteButtons`.
fn pasted_button(item: &Value) -> Result<Button, String> {
    let label = req_str(item, "label")?;
    let rect = rect_of(req_value(item, "rect")?)?;
    let link = match opt_value(item, "target") {
        Some(value) => serde_json::from_value::<Link>(value.clone())
            .map_err(|error| format!("\"target\" must be a valid link: {error}"))?,
        None => Link::NotSet,
    };
    let auto_activate = item
        .get("autoActivate")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(Button {
        name: String::new(),
        label,
        rect,
        link,
        auto_activate,
        override_link: None,
        highlight_group: 1,
        sync_name: true,
        name_from_link: false,
        routes: None,
        audio: None,
        subtitle: disccraft_project::SubtitleDefault::NoChange,
        kind: Default::default(),
        shadow: None,
        orientation: Default::default(),
    })
}

fn run_remove(session: &mut Session, params: &Value) -> CommandResult {
    let menu = MenuId(req_u64(params, "menu")?);
    let buttons = index_array(params, "buttons")?;
    session
        .project
        .remove_buttons(menu, &buttons)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

fn run_set_rects(session: &mut Session, params: &Value) -> CommandResult {
    let menu = MenuId(req_u64(params, "menu")?);
    let Value::Array(items) = req_value(params, "rects")? else {
        return Err("parameter \"rects\" must be an array".to_string());
    };
    let rects = items
        .iter()
        .map(|item| {
            let index = usize::try_from(req_u64(item, "button")?)
                .map_err(|_| "button index is too large".to_string())?;
            Ok((index, rect_of(req_value(item, "rect")?)?))
        })
        .collect::<Result<Vec<(usize, Rect)>, String>>()?;
    session
        .project
        .set_button_rects(menu, &rects)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

fn run_reorder(session: &mut Session, params: &Value) -> CommandResult {
    let menu = MenuId(req_u64(params, "menu")?);
    let order = index_array(params, "order")?;
    session
        .project
        .reorder_buttons(menu, &order)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

fn run_arrange(session: &mut Session, params: &Value) -> CommandResult {
    let menu = MenuId(req_u64(params, "menu")?);
    let buttons = index_array(params, "buttons")?;
    let name = req_str(params, "arrange")?;
    let arrange = Arrange::parse(&name)
        .ok_or_else(|| "\"arrange\" must be front, forward, backward or back".to_string())?;
    let count = session
        .project
        .menu(menu)
        .ok_or_else(|| format!("menu {} does not exist", menu.0))?
        .buttons
        .len();
    if buttons.is_empty() || buttons.iter().any(|index| *index >= count) {
        return Err("button index out of range".to_string());
    }
    let order = arranged_order(count, &buttons, arrange);
    session
        .project
        .reorder_buttons(menu, &order)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

fn run_align(session: &mut Session, params: &Value) -> CommandResult {
    let menu = MenuId(req_u64(params, "menu")?);
    let buttons = index_array(params, "buttons")?;
    let name = req_str(params, "align")?;
    let align = Align::parse(&name).ok_or_else(|| {
        "\"align\" must be left, center, right, top, middle or bottom".to_string()
    })?;
    let safe_area = opt_bool(params, "safeArea")?;
    session
        .project
        .align_buttons(menu, &buttons, align, safe_area)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

fn run_distribute(session: &mut Session, params: &Value) -> CommandResult {
    let menu = MenuId(req_u64(params, "menu")?);
    let buttons = index_array(params, "buttons")?;
    let name = req_str(params, "axis")?;
    let axis =
        Axis::parse(&name).ok_or_else(|| "\"axis\" must be horizontal or vertical".to_string())?;
    let safe_area = opt_bool(params, "safeArea")?;
    session
        .project
        .distribute_buttons(menu, &buttons, axis, safe_area)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

fn rect_of(value: &Value) -> Result<Rect, String> {
    serde_json::from_value::<Rect>(value.clone())
        .map_err(|error| format!("\"rect\" must be a rect: {error}"))
}

/// A required array of button indexes.
fn index_array(params: &Value, name: &str) -> Result<Vec<usize>, String> {
    let error = || format!("parameter \"{name}\" must be an array of button indexes");
    let Value::Array(values) = req_value(params, name)? else {
        return Err(error());
    };
    values
        .iter()
        .map(|value| {
            value
                .as_u64()
                .and_then(|index| usize::try_from(index).ok())
                .ok_or_else(error)
        })
        .collect()
}

/// An optional boolean, false when absent.
fn opt_bool(params: &Value, name: &str) -> Result<bool, String> {
    match opt_value(params, name) {
        None => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(format!("parameter \"{name}\" must be true or false")),
    }
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use serde_json::{Value, json};
    use std::error::Error;

    fn rects(session: &Session, menu: u64) -> Vec<(u16, u16)> {
        session
            .project
            .menu(disccraft_project::MenuId(menu))
            .map(|menu| {
                menu.buttons
                    .iter()
                    .map(|button| (button.rect.x, button.rect.y))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn new_menu(session: &mut Session, buttons: usize) -> Result<u64, Box<dyn Error>> {
        let created = session.execute("menu.create", &json!({}))?;
        let menu = created
            .get("menu")
            .and_then(Value::as_u64)
            .ok_or("missing menu")?;
        for index in 0..buttons {
            session.execute(
                "menu.addButton",
                &json!({ "menu": menu, "label": format!("B{index}") }),
            )?;
        }
        Ok(menu)
    }

    #[test]
    fn paste_and_remove_are_one_undo_step_each() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu = new_menu(&mut session, 1)?;
        let pasted = session.execute(
            "menu.pasteButtons",
            &json!({ "menu": menu, "buttons": [
                { "label": "A", "rect": { "x": 10, "y": 20, "w": 100, "h": 40 }, "target": "stop" },
                { "label": "B", "rect": { "x": 10, "y": 80, "w": 100, "h": 40 } }
            ] }),
        )?;
        assert_eq!(pasted.get("buttons"), Some(&json!([1, 2])));
        assert_eq!(rects(&session, menu).len(), 3);

        session.execute(
            "menu.removeButtons",
            &json!({ "menu": menu, "buttons": [0, 2] }),
        )?;
        assert_eq!(rects(&session, menu), vec![(10, 20)]);

        session.undo()?;
        assert_eq!(rects(&session, menu).len(), 3);
        session.undo()?;
        assert_eq!(rects(&session, menu).len(), 1);

        let bad = session.execute(
            "menu.removeButtons",
            &json!({ "menu": menu, "buttons": ["x"] }),
        );
        assert!(bad.is_err());
        Ok(())
    }

    fn labels(session: &Session, menu: u64) -> Vec<String> {
        session
            .project
            .menu(disccraft_project::MenuId(menu))
            .map(|menu| {
                menu.buttons
                    .iter()
                    .map(|button| button.label.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn reorder_and_arrange_renumber_the_buttons() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu = new_menu(&mut session, 3)?;
        session.execute(
            "menu.reorderButtons",
            &json!({ "menu": menu, "order": [2, 0, 1] }),
        )?;
        assert_eq!(labels(&session, menu), vec!["B2", "B0", "B1"]);
        session.execute(
            "menu.arrangeButtons",
            &json!({ "menu": menu, "buttons": [0], "arrange": "front" }),
        )?;
        assert_eq!(labels(&session, menu), vec!["B0", "B1", "B2"]);
        session.undo()?;
        assert_eq!(labels(&session, menu), vec!["B2", "B0", "B1"]);
        let bad = session.execute(
            "menu.reorderButtons",
            &json!({ "menu": menu, "order": [0, 1] }),
        );
        assert!(bad.is_err());
        let bad = session.execute(
            "menu.arrangeButtons",
            &json!({ "menu": menu, "buttons": [7], "arrange": "back" }),
        );
        assert!(bad.is_err());
        Ok(())
    }

    #[test]
    fn move_align_and_distribute() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu = new_menu(&mut session, 3)?;
        session.execute(
            "button.setRects",
            &json!({ "menu": menu, "rects": [
                { "button": 0, "rect": { "x": 50, "y": 100, "w": 100, "h": 40 } },
                { "button": 2, "rect": { "x": 300, "y": 400, "w": 100, "h": 40 } }
            ] }),
        )?;
        assert_eq!(
            rects(&session, menu),
            vec![(50, 100), (160, 184), (300, 400)]
        );

        session.execute(
            "menu.alignButtons",
            &json!({ "menu": menu, "buttons": [0, 1, 2], "align": "left" }),
        )?;
        assert_eq!(rects(&session, menu), vec![(50, 100), (50, 184), (50, 400)]);

        session.execute(
            "menu.distributeButtons",
            &json!({ "menu": menu, "buttons": [0, 1, 2], "axis": "vertical" }),
        )?;
        // From 100 to 440; button 1 is 48 high (an added button), so the gaps are (340 - 128) / 2 = 106.
        assert_eq!(rects(&session, menu), vec![(50, 100), (50, 246), (50, 400)]);

        session.undo()?;
        session.undo()?;
        assert_eq!(
            rects(&session, menu),
            vec![(50, 100), (160, 184), (300, 400)]
        );

        let one = session.execute(
            "menu.alignButtons",
            &json!({ "menu": menu, "buttons": [1], "align": "top" }),
        );
        assert!(one.is_err());
        session.execute(
            "menu.alignButtons",
            &json!({ "menu": menu, "buttons": [1], "align": "top", "safeArea": true }),
        )?;
        assert_eq!(rects(&session, menu).get(1), Some(&(160, 24)));
        Ok(())
    }
}
