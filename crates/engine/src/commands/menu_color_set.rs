//! The whole colour set of a menu at once (Encore's Menu ▸ Edit Menu Color Set… dialog).

use crate::params::req_u64;
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{ColorSet, MenuId};
use serde_json::Value;

use super::{always_enabled, empty_object};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "menucolorset.save",
        label: "New Menu Color Set",
        menu: &["Menu"],
        shortcut: None,
        params: r#"{"name": string, "colorSet": a colour set (as menu.replaceColorSet takes)}"#,
        enabled: always_enabled,
        run: run_save,
        journal: true,
    },
    CommandSpec {
        id: "menucolorset.delete",
        label: "Delete Menu Color Set",
        menu: &["Menu"],
        shortcut: None,
        params: r#"{"name": string}"#,
        enabled: always_enabled,
        run: run_delete,
        journal: true,
    },
    CommandSpec {
        id: "menucolorset.export",
        label: "Export Menu Color Set",
        menu: &["Menu"],
        shortcut: None,
        params: r#"{"path": string, "colorSet": a colour set}"#,
        enabled: always_enabled,
        run: run_export,
        journal: false,
    },
    CommandSpec {
        id: "menucolorset.read",
        label: "Read Menu Color Set",
        menu: &["Menu"],
        shortcut: None,
        params: r#"{"path": string} -> {"colorSet": a colour set}"#,
        enabled: always_enabled,
        run: run_read,
        journal: false,
    },
    CommandSpec {
        id: "menu.replaceColorSet",
        label: "Replace Menu Color Set",
        menu: &["Menu"],
        shortcut: None,
        params: r##"{"menu": id, "colorSet": {"groups": [{"selected": [r, g, b], "activated": [r, g, b], "selectedOpacity"?: 0..100, "activatedOpacity"?: 0..100, "sameActivated"?: bool, "more"?: [{"selected": [r, g, b], "selectedOpacity"?: 0..100, "activated": [r, g, b], "activatedOpacity"?: 0..100} x 2]} x 3], "normal"?: {"color": [r, g, b], "opacity": 0..100}, "normalMore"?: [{"color": [r, g, b], "opacity": 0..100} x 2]} | null}"##,
        enabled: always_enabled,
        run: run_replace_color_set,
        journal: true,
    },
];

/// What a menu colour set file says it is.
const MENU_COLOR_SET_FORMAT: &str = "disccraft-menu-color-set";
/// The largest menu colour set file read.
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// A `"colorSet"` parameter: a valid colour set.
fn color_set_param(params: &Value) -> Result<ColorSet, String> {
    let value = params
        .get("colorSet")
        .ok_or_else(|| "parameter \"colorSet\" is required".to_string())?;
    let set: ColorSet = serde_json::from_value(value.clone())
        .map_err(|error| format!("\"colorSet\" is not a colour set: {error}"))?;
    if !set.is_valid() {
        return Err("every opacity must be 0 to 100".to_string());
    }
    Ok(set)
}

/// Encore's New in the Menu Color Set dialog: keeps the set under a name in the project.
fn run_save(session: &mut Session, params: &Value) -> CommandResult {
    let name = crate::params::req_str(params, "name")?;
    let set = color_set_param(params)?;
    session
        .project
        .save_menu_color_set(&name, set)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

/// Encore's Delete in the Menu Color Set dialog.
fn run_delete(session: &mut Session, params: &Value) -> CommandResult {
    let name = crate::params::req_str(params, "name")?;
    session
        .project
        .remove_menu_color_set(&name)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

/// Encore's Export in the Menu Color Set dialog: the set as a file another menu or project reads.
fn run_export(_session: &mut Session, params: &Value) -> CommandResult {
    let path = crate::params::req_str(params, "path")?;
    let set = color_set_param(params)?;
    let file = serde_json::json!({
        "format": MENU_COLOR_SET_FORMAT,
        "version": 1,
        "colorSet": set,
    });
    let text = serde_json::to_string_pretty(&file).map_err(|error| error.to_string())?;
    std::fs::write(&path, text).map_err(|error| format!("{path}: {error}"))?;
    Ok(serde_json::json!({ "path": path }))
}

/// Encore's Import in the Menu Color Set dialog: the set a file holds (the dialog shows it; OK gives it to the
/// menu).
fn run_read(_session: &mut Session, params: &Value) -> CommandResult {
    let path = crate::params::req_str(params, "path")?;
    let size = std::fs::metadata(&path)
        .map_err(|error| format!("{path}: {error}"))?
        .len();
    if size > MAX_FILE_BYTES {
        return Err(format!("{path}: not a menu colour set file (too large)"));
    }
    let text = std::fs::read_to_string(&path).map_err(|error| format!("{path}: {error}"))?;
    let file: Value =
        serde_json::from_str(&text).map_err(|_| format!("{path}: not a menu colour set file"))?;
    if file.get("format").and_then(Value::as_str) != Some(MENU_COLOR_SET_FORMAT) {
        return Err(format!("{path}: not a menu colour set file"));
    }
    let set = color_set_param(&file).map_err(|error| format!("{path}: {error}"))?;
    Ok(serde_json::json!({ "colorSet": set }))
}

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

    /// M6.289: a menu colour set is kept in the project under a name and deleted; written to a file and read
    /// back the same; a file of another kind and a bad set are refused.
    #[test]
    fn named_sets_and_files() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let mut group = HighlightGroup::new([200, 10, 10], [10, 200, 10]);
        group.selected_opacity = 60;
        let set = ColorSet {
            groups: [group; 3],
            normal: NormalColor {
                color: [9, 9, 9],
                opacity: 0,
            },
            normal_more: None,
        };
        session.execute(
            "menucolorset.save",
            &json!({ "name": " Warm ", "colorSet": set }),
        )?;
        assert_eq!(session.project.menu_color_sets.get("Warm"), Some(&set));
        let dir =
            std::env::temp_dir().join(format!("disccraft-menu-colors-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let file = dir.join("warm.dcmenucolors");
        session.execute(
            "menucolorset.export",
            &json!({ "path": file, "colorSet": set }),
        )?;
        let read = session.execute("menucolorset.read", &json!({ "path": file }))?;
        let back: ColorSet =
            serde_json::from_value(read.get("colorSet").cloned().ok_or("no set")?)?;
        assert_eq!(back, set);
        let other = dir.join("other.json");
        std::fs::write(&other, r#"{"format": "something-else", "colorSet": {}}"#)?;
        assert!(
            session
                .execute("menucolorset.read", &json!({ "path": other }))
                .is_err()
        );
        let mut bad = set;
        bad.normal.opacity = 101;
        assert!(
            session
                .execute(
                    "menucolorset.save",
                    &json!({ "name": "Bad", "colorSet": bad })
                )
                .is_err()
        );
        assert!(
            session
                .execute(
                    "menucolorset.save",
                    &json!({ "name": " ", "colorSet": set })
                )
                .is_err()
        );
        session.execute("menucolorset.delete", &json!({ "name": "Warm" }))?;
        assert!(session.project.menu_color_sets.is_empty());
        assert!(
            session
                .execute("menucolorset.delete", &json!({ "name": "Warm" }))
                .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

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
