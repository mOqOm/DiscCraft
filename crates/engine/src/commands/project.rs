use crate::params::{opt_str, opt_u64, req_str, req_u64};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{AssetId, MenuId, Severity, TimelineId};
use serde_json::Value;

use super::{always_enabled, empty_object, object};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "project.remove",
        label: "項目を削除",
        menu: &["プロジェクト"],
        shortcut: None,
        params: r#"{"kind": "asset"|"timeline"|"menu"|"slideshow"|"playlist", "id": id}"#,
        enabled: always_enabled,
        run: run_remove,
        journal: true,
    },
    CommandSpec {
        id: "project.rename",
        label: "名前を変更",
        menu: &["プロジェクト"],
        shortcut: None,
        params: r#"{"kind": "asset"|"timeline"|"menu"|"slideshow"|"playlist", "id": id, "name": string}"#,
        enabled: always_enabled,
        run: run_rename,
        journal: true,
    },
    CommandSpec {
        id: "project.setDescription",
        label: "説明を設定",
        menu: &["プロジェクト"],
        shortcut: None,
        params: r#"{"kind": "asset"|"timeline"|"menu"|"slideshow", "id": id, "description": string}"#,
        enabled: always_enabled,
        run: run_set_description,
        journal: true,
    },
    CommandSpec {
        id: "project.createFolder",
        label: "フォルダーを作成",
        menu: &["プロジェクト"],
        shortcut: None,
        params: r#"{"name"?: string, "parent"?: folder id}"#,
        enabled: always_enabled,
        run: run_create_folder,
        journal: true,
    },
    CommandSpec {
        id: "project.renameFolder",
        label: "フォルダーの名前を変更",
        menu: &["プロジェクト"],
        shortcut: None,
        params: r#"{"folder": id, "name": string}"#,
        enabled: always_enabled,
        run: run_rename_folder,
        journal: true,
    },
    CommandSpec {
        id: "project.removeFolder",
        label: "フォルダーを削除",
        menu: &["プロジェクト"],
        shortcut: None,
        params: r#"{"folder": id}"#,
        enabled: always_enabled,
        run: run_remove_folder,
        journal: true,
    },
    CommandSpec {
        id: "project.moveToFolder",
        label: "フォルダーへ移動",
        menu: &["プロジェクト"],
        shortcut: None,
        params: r#"{"id": item or folder id, "folder": folder id | null}"#,
        enabled: always_enabled,
        run: run_move_to_folder,
        journal: true,
    },
    CommandSpec {
        id: "project.check",
        label: "プロジェクトを検査",
        menu: &["プロジェクト"],
        shortcut: None,
        params: "{}",
        enabled: always_enabled,
        run: run_check,
        journal: false,
    },
    CommandSpec {
        id: "project.inspect",
        label: "プロジェクトを調べる",
        menu: &["プロジェクト"],
        shortcut: None,
        params: "{}",
        enabled: always_enabled,
        run: run_inspect,
        journal: false,
    },
    CommandSpec {
        id: "disc.usage",
        label: "容量の見積もり",
        menu: &["ディスク"],
        shortcut: None,
        params: "{}",
        enabled: always_enabled,
        run: run_usage,
        journal: false,
    },
];

/// Remove a whole object. Links that pointed at it become "not set"; an asset in use is not removed.
fn run_remove(session: &mut Session, params: &Value) -> CommandResult {
    let kind = opt_str(params, "kind")?.unwrap_or_default();
    let id = req_u64(params, "id")?;
    let project = &mut session.project;

    let result = match kind.as_str() {
        "asset" => project.remove_asset(AssetId(id)),
        "timeline" => project.remove_timeline(TimelineId(id)),
        "menu" => project.remove_menu(MenuId(id)),
        "slideshow" => project.remove_slideshow(id),
        "playlist" => project.remove_playlist(id),
        "chapterPlaylist" => project.remove_chapter_playlist(id),
        other => return Err(format!("parameter \"kind\" is not removable: {other}")),
    };
    result.map_err(|error| error.to_string())?;

    Ok(empty_object())
}

fn run_rename(session: &mut Session, params: &Value) -> CommandResult {
    let kind = opt_str(params, "kind")?.unwrap_or_default();
    let id = req_u64(params, "id")?;
    let name = opt_str(params, "name")?.unwrap_or_default();
    let name = name.trim();

    if name.is_empty() {
        return Err("parameter \"name\" must not be empty".to_string());
    }

    match kind.as_str() {
        "asset" => match session.project.assets.get_mut(&AssetId(id)) {
            Some(asset) => asset.name = name.to_string(),
            None => return Err(format!("asset {id} does not exist")),
        },
        "timeline" => match session.project.timeline_mut(TimelineId(id)) {
            Some(timeline) => timeline.name = name.to_string(),
            None => return Err(format!("timeline {id} does not exist")),
        },
        "menu" => match session.project.menu_mut(MenuId(id)) {
            Some(menu) => menu.name = name.to_string(),
            None => return Err(format!("menu {id} does not exist")),
        },
        "slideshow" => match session.project.slideshow_mut(id) {
            Some(slideshow) => slideshow.name = name.to_string(),
            None => return Err(format!("slideshow {id} does not exist")),
        },
        "playlist" => match session.project.playlist_mut(id) {
            Some(playlist) => playlist.name = name.to_string(),
            None => return Err(format!("playlist {id} does not exist")),
        },
        "chapterPlaylist" => match session.project.chapter_playlist_mut(id) {
            Some(playlist) => playlist.name = name.to_string(),
            None => return Err(format!("chapter playlist {id} does not exist")),
        },
        _ => {
            return Err(
                "parameter \"kind\" must be \"asset\", \"timeline\", \"menu\", \"slideshow\", \"playlist\" or \"chapterPlaylist\""
                    .to_string(),
            );
        }
    }

    Ok(empty_object())
}

/// The longest description of a timeline or menu, in characters.
const MAX_DESCRIPTION_CHARS: usize = 4096;

fn run_set_description(session: &mut Session, params: &Value) -> CommandResult {
    let kind = opt_str(params, "kind")?.unwrap_or_default();
    let id = req_u64(params, "id")?;
    let description = opt_str(params, "description")?
        .ok_or_else(|| "parameter \"description\" must be a string".to_string())?;
    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(format!(
            "the description is longer than {MAX_DESCRIPTION_CHARS} characters"
        ));
    }
    let slot = match kind.as_str() {
        "asset" => session
            .project
            .assets
            .get_mut(&AssetId(id))
            .map(|asset| &mut asset.description),
        "timeline" => session
            .project
            .timeline_mut(TimelineId(id))
            .map(|timeline| &mut timeline.description),
        "menu" => session
            .project
            .menu_mut(MenuId(id))
            .map(|menu| &mut menu.description),
        "slideshow" => session
            .project
            .slideshow_mut(id)
            .map(|slideshow| &mut slideshow.description),
        "chapterPlaylist" => session
            .project
            .chapter_playlist_mut(id)
            .map(|playlist| &mut playlist.description),
        _ => {
            return Err(
                "parameter \"kind\" must be \"asset\", \"timeline\", \"menu\", \"slideshow\" or \"chapterPlaylist\""
                    .to_string(),
            );
        }
    };
    let Some(slot) = slot else {
        return Err(format!("{kind} {id} does not exist"));
    };
    *slot = description;
    Ok(empty_object())
}

fn run_usage(session: &mut Session, _params: &Value) -> CommandResult {
    let e = disccraft_check::usage(&session.project)?;
    Ok(serde_json::json!({
        "capacityBytes": e.capacity_bytes,
        "estimatedBytes": e.estimated_bytes,
        "passthroughBytes": e.passthrough_bytes,
        "transcodeSeconds": e.transcode_seconds,
        "videoBitrate": e.video_bitrate,
        "fits": e.estimated_bytes <= e.capacity_bytes,
        "timelines": e
            .timelines
            .iter()
            .map(|(id, bytes)| serde_json::json!({ "timeline": id.0, "bytes": bytes }))
            .collect::<Vec<_>>(),
    }))
}

fn run_check(session: &mut Session, _params: &Value) -> CommandResult {
    // Whole-project checks: model invariants, reachability, navigation and disc capacity.
    let problems = disccraft_check::check_project(&session.project);

    let errors = u64::try_from(
        problems
            .iter()
            .filter(|problem| problem.severity == Severity::Error)
            .count(),
    )
    .map_err(|_| "too many problems".to_string())?;

    let warnings = u64::try_from(
        problems
            .iter()
            .filter(|problem| problem.severity == Severity::Warning)
            .count(),
    )
    .map_err(|_| "too many problems".to_string())?;

    let problems_value = serde_json::to_value(&problems).map_err(|error| error.to_string())?;

    Ok(object(&[
        ("errors", Value::from(errors)),
        ("warnings", Value::from(warnings)),
        ("problems", problems_value),
    ]))
}

fn run_inspect(session: &mut Session, _params: &Value) -> CommandResult {
    serde_json::to_value(&session.project).map_err(|error| error.to_string())
}

/// Encore's folders of the Project panel.
fn run_create_folder(session: &mut Session, params: &Value) -> CommandResult {
    let name = opt_str(params, "name")?;
    let parent = opt_u64(params, "parent")?;
    let id = session
        .project
        .add_folder(name, parent)
        .map_err(|error| error.to_string())?;
    Ok(object(&[("folder", Value::from(id))]))
}

fn run_rename_folder(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "folder")?;
    let name = req_str(params, "name")?;
    session
        .project
        .rename_folder(id, &name)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

fn run_remove_folder(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "folder")?;
    session
        .project
        .remove_folder(id)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

/// Moves an item or a folder into a folder (`"folder": null` moves it to the root).
fn run_move_to_folder(session: &mut Session, params: &Value) -> CommandResult {
    let item = req_u64(params, "id")?;
    let folder = match params.get("folder") {
        None | Some(Value::Null) => None,
        Some(_) => Some(req_u64(params, "folder")?),
    };
    session
        .project
        .move_to_folder(item, folder)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use disccraft_project::{AssetId, MenuId, TimelineId};
    use serde_json::{Map, Value, json};
    use std::error::Error;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn folders_are_created_filled_and_undone() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu = session
            .execute("menu.create", &json!({"name": "Main"}))?
            .get("menu")
            .and_then(Value::as_u64)
            .ok_or("no menu id")?;
        let folder = session
            .execute("project.createFolder", &json!({"name": "Menus"}))?
            .get("folder")
            .and_then(Value::as_u64)
            .ok_or("no folder id")?;
        session.execute(
            "project.moveToFolder",
            &json!({"id": menu, "folder": folder}),
        )?;
        assert_eq!(session.project.folder_of(menu), Some(folder));
        session.execute(
            "project.renameFolder",
            &json!({"folder": folder, "name": "All menus"}),
        )?;
        assert_eq!(
            session
                .project
                .folders
                .get(&folder)
                .map(|f| f.name.as_str()),
            Some("All menus")
        );
        assert!(
            session
                .execute("project.moveToFolder", &json!({"id": menu, "folder": 999}))
                .is_err()
        );
        session.execute("project.moveToFolder", &json!({"id": menu, "folder": null}))?;
        assert_eq!(session.project.folder_of(menu), None);
        session.undo()?;
        assert_eq!(session.project.folder_of(menu), Some(folder));
        session.execute("project.removeFolder", &json!({"folder": folder}))?;
        assert!(session.project.folders.is_empty());
        assert_eq!(session.project.folder_of(menu), None);
        Ok(())
    }

    #[test]
    fn descriptions_are_set_and_undone() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu = session
            .execute("menu.create", &json!({"name": "Main"}))?
            .get("menu")
            .and_then(Value::as_u64)
            .ok_or("menu id")?;
        session.execute(
            "project.setDescription",
            &json!({"kind": "menu", "id": menu, "description": "Top menu
with four buttons"}),
        )?;
        let description = |session: &Session| {
            session
                .project
                .menu(MenuId(menu))
                .map(|menu| menu.description.clone())
        };
        assert_eq!(
            description(&session).as_deref(),
            Some(
                "Top menu
with four buttons"
            )
        );
        for params in [
            json!({"kind": "asset", "id": menu, "description": "x"}),
            json!({"kind": "menu", "id": 999, "description": "x"}),
            json!({"kind": "menu", "id": menu}),
            json!({"kind": "menu", "id": menu, "description": "x".repeat(4097)}),
        ] {
            assert!(session.execute("project.setDescription", &params).is_err());
        }
        session.undo()?;
        assert_eq!(description(&session).as_deref(), Some(""));
        Ok(())
    }

    fn obj(entries: &[(&str, Value)]) -> Value {
        let mut map = Map::new();
        for (key, value) in entries {
            map.insert((*key).to_string(), (*value).clone());
        }
        Value::Object(map)
    }

    fn temp_dir(name: &str) -> Result<PathBuf, Box<dyn Error>> {
        let mut path = std::env::temp_dir();
        path.push(format!("disccraft-engine-{}-{name}", std::process::id()));
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    #[test]
    fn rename_every_object_kind() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("rename")?;
        let video = dir.join("movie.mp4");
        fs::write(&video, b"fake")?;

        let mut session = Session::new();
        session.execute("asset.import", &obj(&[("path", json!(video))]))?;
        session.execute("timeline.create", &obj(&[("asset", Value::from(1))]))?;
        session.execute("menu.create", &obj(&[]))?;
        session.execute("slideshow.create", &obj(&[]))?;
        session.execute("playlist.create", &obj(&[]))?;

        let cases = [
            ("asset", 1),
            ("timeline", 2),
            ("menu", 3),
            ("slideshow", 4),
            ("playlist", 5),
        ];

        for (kind, id) in cases {
            session.execute(
                "project.rename",
                &obj(&[
                    ("kind", json!(kind)),
                    ("id", Value::from(id)),
                    ("name", json!(" renamed ")),
                ]),
            )?;
        }

        assert_eq!(
            session
                .project
                .assets
                .get(&AssetId(1))
                .ok_or("missing asset")?
                .name,
            "renamed"
        );
        assert_eq!(
            session
                .project
                .timeline(TimelineId(2))
                .ok_or("missing timeline")?
                .name,
            "renamed"
        );
        assert_eq!(
            session.project.menu(MenuId(3)).ok_or("missing menu")?.name,
            "renamed"
        );
        assert_eq!(
            session
                .project
                .slideshow(4)
                .ok_or("missing slideshow")?
                .name,
            "renamed"
        );
        assert_eq!(
            session.project.playlist(5).ok_or("missing playlist")?.name,
            "renamed"
        );

        fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn rename_rejects_empty_name_and_unknown_kind() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        session.execute("menu.create", &obj(&[]))?;

        assert!(
            session
                .execute(
                    "project.rename",
                    &obj(&[
                        ("kind", json!("menu")),
                        ("id", Value::from(1)),
                        ("name", json!("  "))
                    ])
                )
                .is_err()
        );
        assert!(
            session
                .execute(
                    "project.rename",
                    &obj(&[
                        ("kind", json!("bin")),
                        ("id", Value::from(1)),
                        ("name", json!("x"))
                    ])
                )
                .is_err()
        );
        assert!(
            session
                .execute(
                    "project.rename",
                    &obj(&[
                        ("kind", json!("menu")),
                        ("id", Value::from(99)),
                        ("name", json!("x"))
                    ])
                )
                .is_err()
        );

        Ok(())
    }
}
