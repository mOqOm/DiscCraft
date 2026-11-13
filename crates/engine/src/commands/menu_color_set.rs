//! The whole colour set of a menu at once (Encore's Menu ▸ Edit Menu Color Set… dialog).

use crate::params::req_u64;
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{ColorSet, MenuId};
use serde_json::Value;

use super::{always_enabled, empty_object};

pub(crate) const COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "menu.replaceColorSet",
    label: "メニューカラーセットを設定",
    menu: &["メニュー"],
    shortcut: None,
    params: r##"{"menu": id, "colorSet": {"groups": [{"selected": [r, g, b], "activated": [r, g, b], "selectedOpacity"?: 0..100, "activatedOpacity"?: 0..100, "sameActivated"?: bool, "more"?: [{"selected": [r, g, b], "selectedOpacity"?: 0..100, "activated": [r, g, b], "activatedOpacity"?: 0..100} x 2]} x 3], "normal"?: {"color": [r, g, b], "opacity": 0..100}, "normalMore"?: [{"color": [r, g, b], "opacity": 0..100} x 2]} | null}"##,
    enabled: always_enabled,
    run: run_replace_color_set,
    journal: true,
}];

fn run_replace_color_set(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let Some(value) = params.get("colorSet") else {
        return Err(
            "parameter \"colorSet\" is required (a colour set, or null for automatic)".to_string(),
        );
    };
    let set = if value.is_null() {
        None
    } else {
        let set: ColorSet = serde_json::from_value(value.clone())
            .map_err(|error| format!("\"colorSet\" is not a colour set: {error}"))?;
        if !set.is_valid() {
            return Err("every opacity must be 0 to 100".to_string());
        }
        Some(set)
    };
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    // Group 1's selected colour is also the menu's highlight colour (the automatic set is made of it).
    if let Some(first) = set.as_ref().and_then(|set| set.groups.first()) {
        menu.highlight_color = first.selected;
    }
    menu.color_set = set;
    Ok(empty_object())
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use disccraft_project::{ColorSet, HighlightGroup, MenuId, NormalColor};
    use serde_json::json;
    use std::error::Error;

    #[test]
    fn the_whole_set_is_replaced_in_one_step() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let created = session.execute("menu.create", &json!({"name": "Main"}))?;
        let menu = created
            .get("menu")
            .and_then(serde_json::Value::as_u64)
            .ok_or("no menu id")?;
        let mut group = HighlightGroup::new([10, 20, 30], [40, 50, 60]);
        group.selected_opacity = 47;
        group.same_activated = true;
        let set = ColorSet {
            groups: [group; 3],
            normal: NormalColor {
                color: [1, 2, 3],
                opacity: 20,
            },
            normal_more: Some(
                [NormalColor {
                    color: [4, 5, 6],
                    opacity: 30,
                }; 2],
            ),
        };
        session.execute(
            "menu.replaceColorSet",
            &json!({"menu": menu, "colorSet": serde_json::to_value(set)?}),
        )?;
        let stored = session.project.menu(MenuId(menu)).ok_or("menu")?;
        assert_eq!(stored.color_set, Some(set));
        assert_eq!(stored.highlight_color, [10, 20, 30]);
        // Back to automatic.
        session.execute(
            "menu.replaceColorSet",
            &json!({"menu": menu, "colorSet": null}),
        )?;
        assert_eq!(
            session.project.menu(MenuId(menu)).ok_or("menu")?.color_set,
            None
        );
        session.execute("edit.undo", &json!({}))?;
        assert_eq!(
            session.project.menu(MenuId(menu)).ok_or("menu")?.color_set,
            Some(set)
        );
        // Bad sets change nothing.
        let mut bad = serde_json::to_value(set)?;
        bad["normal"]["opacity"] = json!(101);
        let mut bad_more = serde_json::to_value(set)?;
        bad_more["normalMore"][1]["opacity"] = json!(101);
        for params in [
            json!({"menu": menu}),
            json!({"menu": menu, "colorSet": {"groups": []}}),
            json!({"menu": menu, "colorSet": bad}),
            json!({"menu": menu, "colorSet": bad_more}),
            json!({"menu": 999, "colorSet": null}),
        ] {
            assert!(
                session.execute("menu.replaceColorSet", &params).is_err(),
                "{params}"
            );
        }
        Ok(())
    }
}
