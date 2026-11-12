use crate::params::{opt_str, opt_value, req_str};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{Capacity, DiscKind, Project, VideoStandard};
use serde_json::Value;
use std::path::{Path, PathBuf};

use super::{always_enabled, empty_object, object};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "file.new",
        label: "新規プロジェクト",
        menu: &["ファイル"],
        shortcut: Some("Ctrl+N"),
        params: r#"{"kind"?: "dvd"|"bluRay", "standard"?: "ntsc"|"pal"}"#,
        enabled: always_enabled,
        run: run_new,
        journal: false,
    },
    CommandSpec {
        id: "file.open",
        label: "プロジェクトを開く",
        menu: &["ファイル"],
        shortcut: Some("Ctrl+O"),
        params: r#"{"path": string}"#,
        enabled: always_enabled,
        run: run_open,
        journal: false,
    },
    CommandSpec {
        id: "file.save",
        label: "保存",
        menu: &["ファイル"],
        shortcut: Some("Ctrl+S"),
        params: r#"{"path"?: string}"#,
        enabled: always_enabled,
        run: run_save,
        journal: false,
    },
    CommandSpec {
        id: "file.saveAs",
        label: "名前を付けて保存",
        menu: &["ファイル"],
        shortcut: Some("Ctrl+Shift+S"),
        params: r#"{"path": string}"#,
        enabled: always_enabled,
        run: run_save_as,
        journal: false,
    },
];

fn parse_kind(value: &Value) -> Result<DiscKind, String> {
    serde_json::from_value(value.clone())
        .map_err(|error| format!("parameter \"kind\" must be \"dvd\" or \"bluRay\": {error}"))
}

fn parse_standard(value: &Value) -> Result<VideoStandard, String> {
    serde_json::from_value(value.clone())
        .map_err(|error| format!("parameter \"standard\" must be \"ntsc\" or \"pal\": {error}"))
}

fn run_new(session: &mut Session, params: &Value) -> CommandResult {
    let mut project = Project::default();

    if let Some(value) = opt_value(params, "kind") {
        let kind = parse_kind(value)?;
        project.settings.kind = kind;
        project.settings.capacity = match kind {
            DiscKind::Dvd => Capacity::Dvd5,
            DiscKind::BluRay => Capacity::Bd25,
        };
    }

    if let Some(value) = opt_value(params, "standard") {
        project.settings.standard = parse_standard(value)?;
    }

    session.reset(project);
    session.path = None;
    session.dirty = true;
    Ok(empty_object())
}

fn run_open(session: &mut Session, params: &Value) -> CommandResult {
    let path = req_str(params, "path")?;
    let opened = Session::open(Path::new(&path)).map_err(|error| format!("file.open: {error}"))?;

    session.reset(opened.project);
    session.path = Some(PathBuf::from(&path));
    session.dirty = opened.dirty;

    Ok(object(&[("path", Value::String(path))]))
}

fn run_save(session: &mut Session, params: &Value) -> CommandResult {
    let given = opt_str(params, "path")?;
    let target = match given {
        Some(path) => PathBuf::from(path),
        None => session
            .path
            .clone()
            .ok_or_else(|| "file.save: no path known".to_string())?,
    };

    let saved = session
        .save(Some(&target))
        .map_err(|error| format!("file.save: {error}"))?;

    Ok(object(&[(
        "path",
        Value::String(saved.to_string_lossy().into_owned()),
    )]))
}

fn run_save_as(session: &mut Session, params: &Value) -> CommandResult {
    let path = req_str(params, "path")?;
    let saved = session
        .save(Some(Path::new(&path)))
        .map_err(|error| format!("file.saveAs: {error}"))?;

    Ok(object(&[(
        "path",
        Value::String(saved.to_string_lossy().into_owned()),
    )]))
}
