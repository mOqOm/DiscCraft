use crate::params::{opt_u64, req_link, req_u64};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{FLOWCHART_LIMIT, FlowPoint, Link};
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
    CommandSpec {
        id: "flowchart.moveNode",
        label: "フローチャートのノードを移動",
        menu: &["フローチャート"],
        shortcut: None,
        params: r#"{"node": item id | 0 (the disc), "x": 0..100000, "y": 0..100000}"#,
        enabled: always_enabled,
        run: run_move_node,
        journal: true,
    },
    CommandSpec {
        id: "flowchart.arrange",
        label: "自動レイアウト",
        menu: &["フローチャート"],
        shortcut: None,
        params: r#"{"node"?: item id | 0 (the disc); without it, every node}"#,
        enabled: always_enabled,
        run: run_arrange,
        journal: true,
    },
];

/// Put a flowchart node where it was dragged (its top-left, in points from the flowchart's top-left).
fn run_move_node(session: &mut Session, params: &Value) -> CommandResult {
    let node = req_u64(params, "node")?;
    if !session.project.flowchart_node_exists(node) {
        return Err(format!("no flowchart node {node}"));
    }
    let coordinate = |name: &str| -> Result<i32, String> {
        params
            .get(name)
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
            .filter(|value| (0..=FLOWCHART_LIMIT).contains(value))
            .ok_or_else(|| {
                format!("parameter \"{name}\" must be an integer from 0 to {FLOWCHART_LIMIT}")
            })
    };
    let point = FlowPoint {
        x: coordinate("x")?,
        y: coordinate("y")?,
    };
    // 消えた項目の位置は残しておく意味がないので、ついでに落とす。
    let project = &mut session.project;
    let stale: Vec<u64> = project
        .flowchart
        .keys()
        .copied()
        .filter(|id| !project.flowchart_node_exists(*id))
        .collect();
    for id in stale {
        project.flowchart.remove(&id);
    }
    project.flowchart.insert(node, point);
    Ok(empty_object())
}

/// Encore's 自動レイアウト: forget where a node was moved (`node`), or every moved node (すべて自動レイアウト), so
/// the flowchart lays it out automatically again.
fn run_arrange(session: &mut Session, params: &Value) -> CommandResult {
    let flowchart = &mut session.project.flowchart;
    match opt_u64(params, "node")? {
        Some(node) => {
            if flowchart.remove(&node).is_none() {
                return Err(format!(
                    "flowchart node {node} is already laid out automatically"
                ));
            }
        }
        None if flowchart.is_empty() => {
            return Err("the flowchart is already laid out automatically".to_string());
        }
        None => flowchart.clear(),
    }
    Ok(empty_object())
}

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

#[cfg(test)]
mod tests {
    use crate::Session;
    use disccraft_project::{FLOWCHART_DISC, FlowPoint};
    use serde_json::json;
    use std::error::Error;

    #[test]
    fn flowchart_nodes_move_check_their_input_and_undo() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu = session.project.add_menu(None)?;
        session.execute(
            "flowchart.moveNode",
            &json!({"node": menu.0, "x": 300, "y": 40}),
        )?;
        session.execute(
            "flowchart.moveNode",
            &json!({"node": FLOWCHART_DISC, "x": 0, "y": 0}),
        )?;
        assert_eq!(
            session.project.flowchart.get(&menu.0),
            Some(&FlowPoint { x: 300, y: 40 })
        );
        for bad in [
            json!({"node": 999, "x": 1, "y": 1}),
            json!({"node": menu.0, "x": -1, "y": 1}),
            json!({"node": menu.0, "x": 1, "y": 100_001}),
            json!({"node": menu.0, "x": 1.5, "y": 1}),
            json!({"node": menu.0, "x": 1}),
            json!({"node": menu.0, "x": i64::MAX, "y": 1}),
        ] {
            assert!(
                session.execute("flowchart.moveNode", &bad).is_err(),
                "{bad}"
            );
        }
        session.execute("edit.undo", &json!({}))?;
        assert_eq!(session.project.flowchart.len(), 1);

        // One node back to the automatic layout, then all.
        session.execute(
            "flowchart.moveNode",
            &json!({"node": FLOWCHART_DISC, "x": 5, "y": 6}),
        )?;
        assert_eq!(session.project.flowchart.len(), 2);
        session.execute("flowchart.arrange", &json!({"node": FLOWCHART_DISC}))?;
        assert!(
            session
                .execute("flowchart.arrange", &json!({"node": FLOWCHART_DISC}))
                .is_err()
        );
        assert_eq!(session.project.flowchart.len(), 1);
        session.execute("flowchart.arrange", &json!({}))?;
        assert!(session.project.flowchart.is_empty());
        assert!(session.execute("flowchart.arrange", &json!({})).is_err());
        session.execute("edit.undo", &json!({}))?;
        assert_eq!(session.project.flowchart.len(), 1);
        Ok(())
    }
}
