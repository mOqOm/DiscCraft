use crate::params::req_link;
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::Link;
use serde_json::Value;

use super::{always_enabled, empty_object};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "nav.setFirstPlay",
        label: "最初の再生を設定",
        menu: &["ナビゲーション"],
        shortcut: None,
        params: r#"{"target": Link}"#,
        enabled: always_enabled,
        run: run_set_first_play,
        journal: true,
    },
    CommandSpec {
        id: "nav.setFirstPlayOverride",
        label: "ディスクの上書きを設定",
        menu: &["ディスク"],
        shortcut: None,
        params: r#"{"target": Link | null}"#,
        enabled: always_enabled,
        run: run_set_first_play_override,
        journal: true,
    },
    CommandSpec {
        id: "nav.setTitleRemote",
        label: "タイトルリモートを設定",
        menu: &["ナビゲーション"],
        shortcut: None,
        params: r#"{"target": Link}"#,
        enabled: always_enabled,
        run: run_set_title_remote,
        journal: true,
    },
];

fn run_set_first_play(session: &mut Session, params: &Value) -> CommandResult {
    let target = req_link(params, "target")?;
    session.project.settings.first_play = target;
    Ok(empty_object())
}

/// The disc's override: where to go after the title the first play plays. `null` or no target clears it.
fn run_set_first_play_override(session: &mut Session, params: &Value) -> CommandResult {
    let target = match params.get("target") {
        None | Some(Value::Null) => None,
        Some(_) => Some(req_link(params, "target")?).filter(|link| !matches!(link, Link::NotSet)),
    };
    if let Some(link) = &target
        && !session.project.link_resolves(link)
    {
        return Err("the override target does not exist".to_string());
    }
    session.project.settings.first_play_override = target;
    Ok(empty_object())
}

fn run_set_title_remote(session: &mut Session, params: &Value) -> CommandResult {
    let target = req_link(params, "target")?;
    session.project.settings.title_remote = target;
    Ok(empty_object())
}
