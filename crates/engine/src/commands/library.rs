//! Library commands: DiscCraft's built-in menu, button and background presets.

use crate::params::{opt_str, req_str, req_u64};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{
    ButtonSpec, ChapterIndexLabels, LIBRARY, LibraryKind, MenuId, MenuSpec, TimelineId,
};
use serde_json::Value;

use super::{always_enabled, object};

/// Library command specifications.
pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "library.list",
        label: "ライブラリの項目",
        menu: &["ライブラリ"],
        shortcut: None,
        params: "{}",
        enabled: always_enabled,
        run: run_list,
        journal: false,
    },
    CommandSpec {
        id: "library.newMenu",
        label: "ライブラリから新規メニュー",
        menu: &["ライブラリ"],
        shortcut: None,
        params: r#"{"preset"?: string, "spec"?: MenuSpec, "name"?: string, "labels"?: [string]}"#,
        enabled: always_enabled,
        run: run_new_menu,
        journal: true,
    },
    CommandSpec {
        id: "library.place",
        label: "ライブラリのボタンを配置",
        menu: &["ライブラリ"],
        shortcut: None,
        params: r#"{"menu": id, "preset"?: string, "button"?: ButtonSpec, "label"?: string}"#,
        enabled: always_enabled,
        run: run_place,
        journal: true,
    },
    CommandSpec {
        id: "library.chapterIndex",
        label: "チャプターインデックスを作成",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"timeline": id, "name": string, "perPage"?: count, "labels"?: [string], "previous"?: string, "next"?: string}"#,
        enabled: always_enabled,
        run: run_chapter_index,
        journal: true,
    },
    CommandSpec {
        id: "library.setBackground",
        label: "ライブラリの背景を設定",
        menu: &["ライブラリ"],
        shortcut: None,
        params: r#"{"menu": id, "preset": string}"#,
        enabled: always_enabled,
        run: run_set_background,
        journal: true,
    },
];

fn hex(color: [u8; 3]) -> String {
    let [r, g, b] = color;
    format!("#{r:02X}{g:02X}{b:02X}")
}

fn kind_name(kind: LibraryKind) -> &'static str {
    match kind {
        LibraryKind::Menu => "menu",
        LibraryKind::Button => "button",
        LibraryKind::Background => "background",
    }
}

fn run_list(_session: &mut Session, _params: &Value) -> CommandResult {
    let items = LIBRARY
        .iter()
        .map(|item| {
            let buttons = item
                .buttons
                .iter()
                .map(|button| {
                    object(&[
                        ("label", Value::from(button.label)),
                        ("x", Value::from(button.x)),
                        ("y", Value::from(button.y)),
                        ("w", Value::from(button.w)),
                        ("h", Value::from(button.h)),
                    ])
                })
                .collect();
            object(&[
                ("id", Value::from(item.id)),
                ("name", Value::from(item.name)),
                ("kind", Value::from(kind_name(item.kind))),
                ("background", Value::from(hex(item.background))),
                ("text", Value::from(hex(item.text))),
                ("highlight", Value::from(hex(item.highlight))),
                ("buttons", Value::Array(buttons)),
            ])
        })
        .collect();
    Ok(object(&[("items", Value::Array(items))]))
}

/// An optional array of strings (`"labels"`); missing or null is empty.
fn string_array(params: &Value, name: &str) -> Result<Vec<String>, String> {
    let error = || format!("parameter \"{name}\" must be an array of strings");
    match params.get(name) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| value.as_str().map(str::to_owned).ok_or_else(error))
            .collect(),
        Some(_) => Err(error()),
    }
}

/// A new menu from a built-in preset ("preset") or from a spec ("spec": a user's library item).
fn run_new_menu(session: &mut Session, params: &Value) -> CommandResult {
    let name = opt_str(params, "name")?.filter(|name| !name.trim().is_empty());
    let labels = string_array(params, "labels")?;
    let menu = match (opt_str(params, "preset")?, params.get("spec")) {
        (Some(preset), None) => session
            .project
            .add_menu_from_preset(&preset, name, &labels)
            .map_err(|error| error.to_string())?,
        (None, Some(spec)) => {
            let spec: MenuSpec = serde_json::from_value(spec.clone())
                .map_err(|error| format!("parameter \"spec\" is not a menu spec: {error}"))?;
            if spec.buttons.len() > 36 {
                return Err("a menu spec holds at most 36 buttons".to_string());
            }
            session
                .project
                .add_menu_from_spec(name.unwrap_or_else(|| "Menu".to_owned()), &spec, &labels)
                .map_err(|error| error.to_string())?
        }
        _ => return Err("give exactly one of \"preset\" or \"spec\"".to_string()),
    };
    Ok(object(&[("menu", Value::from(menu.0))]))
}

/// A button from a built-in preset ("preset") or a spec ("button": a user's library item).
fn run_place(session: &mut Session, params: &Value) -> CommandResult {
    let menu = MenuId(req_u64(params, "menu")?);
    let label = opt_str(params, "label")?;
    let index = match (opt_str(params, "preset")?, params.get("button")) {
        (Some(preset), None) => session
            .project
            .place_preset_button(menu, &preset, label)
            .map_err(|error| error.to_string())?,
        (None, Some(spec)) => {
            let spec: ButtonSpec = serde_json::from_value(spec.clone())
                .map_err(|error| format!("parameter \"button\" is not a button spec: {error}"))?;
            session
                .project
                .place_button_spec(menu, &spec, label)
                .map_err(|error| error.to_string())?
        }
        _ => return Err("give exactly one of \"preset\" or \"button\"".to_string()),
    };
    let index = u64::try_from(index).map_err(|_| "button index is too large".to_string())?;
    Ok(object(&[("button", Value::from(index))]))
}

fn run_chapter_index(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let name = req_str(params, "name")?;
    let per_page = match params.get("perPage") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let count = value
                .as_u64()
                .ok_or_else(|| "parameter \"perPage\" must be a positive integer".to_string())?;
            Some(
                usize::try_from(count)
                    .map_err(|_| "parameter \"perPage\" is too large".to_string())?,
            )
        }
    };
    let labels = ChapterIndexLabels {
        chapters: string_array(params, "labels")?,
        previous: opt_str(params, "previous")?.unwrap_or_default(),
        next: opt_str(params, "next")?.unwrap_or_default(),
    };
    let menus = session
        .project
        .create_chapter_index(timeline, &name, per_page, &labels)
        .map_err(|error| error.to_string())?;
    let menus = menus.into_iter().map(|menu| Value::from(menu.0)).collect();
    Ok(object(&[("menus", Value::Array(menus))]))
}

fn run_set_background(session: &mut Session, params: &Value) -> CommandResult {
    let menu = MenuId(req_u64(params, "menu")?);
    let preset = req_str(params, "preset")?;
    session
        .project
        .apply_background_preset(menu, &preset)
        .map_err(|error| error.to_string())?;
    Ok(object(&[]))
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use serde_json::{Value, json};
    use std::error::Error;

    #[test]
    fn new_menu_place_and_background_are_one_undo_step_each() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let listed = session.execute("library.list", &json!({}))?;
        let count = listed
            .get("items")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        assert!(count > 0);

        let created = session.execute(
            "library.newMenu",
            &json!({ "preset": "menu.plain-dark", "labels": ["Start"] }),
        )?;
        let menu = created
            .get("menu")
            .and_then(Value::as_u64)
            .ok_or("missing menu")?;
        let placed = session.execute(
            "library.place",
            &json!({ "menu": menu, "preset": "button.small" }),
        )?;
        assert_eq!(placed.get("button").and_then(Value::as_u64), Some(4));
        let _ = session.execute(
            "library.setBackground",
            &json!({ "menu": menu, "preset": "background.navy" }),
        )?;

        let first = session
            .project
            .menus
            .values()
            .next()
            .and_then(|menu| menu.buttons.first())
            .map(|button| button.label.clone());
        assert_eq!(first.as_deref(), Some("Start"));

        session.undo()?;
        session.undo()?;
        session.undo()?;
        assert!(session.project.menus.is_empty());
        Ok(())
    }

    #[test]
    fn chapter_index_pages_are_one_undo_step() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!(
            "disccraft-engine-{}-chapter-index",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)?;
        let video = dir.join("movie.mp4");
        std::fs::write(&video, b"fake")?;

        let mut session = Session::new();
        let import = session.execute(
            "asset.import",
            &json!({ "path": video.to_string_lossy().to_string() }),
        )?;
        let asset = import.get("asset").and_then(Value::as_u64).ok_or("asset")?;
        let created = session.execute("timeline.create", &json!({ "asset": asset }))?;
        let timeline = created
            .get("timeline")
            .and_then(Value::as_u64)
            .ok_or("timeline")?;
        for seconds in 1..=4 {
            session.execute(
                "timeline.addChapter",
                &json!({ "timeline": timeline, "timecode": format!("00:00:{seconds:02};00") }),
            )?;
        }

        let index = session.execute(
            "library.chapterIndex",
            &json!({
                "timeline": timeline,
                "name": "Scenes",
                "perPage": 2,
                "labels": ["Opening"],
                "previous": "Back",
                "next": "More",
            }),
        )?;
        let pages = index
            .get("menus")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        assert_eq!(pages, 3);
        assert_eq!(session.project.menus.len(), 3);

        session.undo()?;
        assert!(session.project.menus.is_empty());

        assert!(
            session
                .execute(
                    "library.chapterIndex",
                    &json!({ "timeline": timeline, "name": "x", "perPage": 0 })
                )
                .is_err()
        );
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn a_menu_preset_cannot_be_placed_as_a_button() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let created = session.execute("library.newMenu", &json!({ "preset": "menu.left-list" }))?;
        let menu = created
            .get("menu")
            .and_then(Value::as_u64)
            .ok_or("missing menu")?;
        assert!(
            session
                .execute(
                    "library.place",
                    &json!({ "menu": menu, "preset": "menu.plain-dark" })
                )
                .is_err()
        );
        assert!(
            session
                .execute("library.newMenu", &json!({ "preset": "no-such-preset" }))
                .is_err()
        );
        Ok(())
    }
}
