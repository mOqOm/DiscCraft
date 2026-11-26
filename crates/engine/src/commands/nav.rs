use crate::params::{opt_u64, req_link, req_u64};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{FLOWCHART_LIMIT, FlowPoint, Link};
use serde_json::Value;

use super::{always_enabled, empty_object};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "nav.setFirstPlay",
        label: "Set First Play",
        menu: &["Navigation"],
        shortcut: None,
        params: r#"{"target": Link}"#,
        enabled: always_enabled,
        run: run_set_first_play,
        journal: true,
    },
    CommandSpec {
        id: "nav.setFirstPlayOverride",
        label: "Set Disc Override",
        menu: &["Disc"],
        shortcut: None,
        params: r#"{"target": Link | null}"#,
        enabled: always_enabled,
        run: run_set_first_play_override,
        journal: true,
    },
    CommandSpec {
        id: "nav.setTitleRemote",
        label: "Set Title Button",
        menu: &["Navigation"],
        shortcut: None,
        params: r#"{"target": Link}"#,
        enabled: always_enabled,
        run: run_set_title_remote,
        journal: true,
    },
    CommandSpec {
        id: "flowchart.moveNode",
        label: "Move Flowchart Node",
        menu: &["Flowchart"],
        shortcut: None,
        params: r#"{"node": item id | 0 (the disc), "x": 0..100000, "y": 0..100000}"#,
        enabled: always_enabled,
        run: run_move_node,
        journal: true,
    },
    CommandSpec {
        id: "flowchart.link",
        label: "Set Link",
        menu: &["Flowchart"],
        shortcut: None,
        params: r#"{"command": a link command's id, "params": its params, "node": the item it links to}"#,
        enabled: always_enabled,
        run: run_link_node,
        journal: true,
    },
    CommandSpec {
        id: "flowchart.arrange",
        label: "Auto Layout",
        menu: &["Flowchart"],
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

/// A link made on the flowchart to an item that was put there from the tray: the link command (`command` with
/// `params`), and the item takes its place in the tree beside the link, as Encore lines up the node of the newest
/// link, instead of staying where it was put. One step to undo.
fn run_link_node(session: &mut Session, params: &Value) -> CommandResult {
    let id = params
        .get("command")
        .and_then(Value::as_str)
        .ok_or("parameter \"command\" must be a command id")?;
    let node = req_u64(params, "node")?;
    let spec = super::find_command(id)
        .filter(|spec| spec.journal && !spec.id.starts_with("flowchart."))
        .ok_or_else(|| format!("{id} is not a link command"))?;
    (spec.enabled)(session).map_err(|reason| format!("{id}: {reason}"))?;
    let empty = Value::Object(serde_json::Map::new());
    let result = (spec.run)(session, params.get("params").unwrap_or(&empty))?;
    let _ = session.project.flowchart.remove(&node);
    Ok(result)
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

    /// As in Encore, the first timeline made becomes the disc's first play and the first menu its title button;
    /// later ones, and links already set, change nothing.
    #[test]
    fn the_first_timeline_and_menu_become_the_discs_links() -> Result<(), Box<dyn Error>> {
        use disccraft_project::Link;
        let mut session = Session::new();
        let asset = session
            .project
            .add_asset(std::path::Path::new("movie.mp4"))?
            .0;
        assert_eq!(session.project.settings.first_play, Link::NotSet);
        let first = session.execute("timeline.create", &json!({ "asset": asset }))?;
        let first = first
            .get("timeline")
            .and_then(|v| v.as_u64())
            .ok_or("timeline")?;
        let _ = session.execute("timeline.create", &json!({ "asset": asset }))?;
        assert!(matches!(
            session.project.settings.first_play,
            Link::Timeline { id, chapter: 0 } if id.0 == first
        ));
        assert_eq!(session.project.settings.title_remote, Link::NotSet);
        let menu = session.execute("menu.create", &json!({}))?;
        let menu = menu.get("menu").and_then(|v| v.as_u64()).ok_or("menu")?;
        let _ = session.execute("menu.create", &json!({}))?;
        assert!(matches!(
            session.project.settings.title_remote,
            Link::Menu { id, button: None } if id.0 == menu
        ));
        // Undoing the creation undoes the link with it.
        for _ in 0..4 {
            session.execute("edit.undo", &json!({}))?;
        }
        assert_eq!(session.project.settings.first_play, Link::NotSet);
        assert_eq!(session.project.settings.title_remote, Link::NotSet);
        Ok(())
    }

    /// A link made on the flowchart to an item put there from the tray: the item goes into the tree (its place is
    /// forgotten), as Encore lines up the node of the newest link; one undo takes back both. Only link commands.
    #[test]
    fn a_link_to_a_placed_item_lines_it_up_in_one_step() -> Result<(), Box<dyn Error>> {
        use disccraft_project::Link;
        let mut session = Session::new();
        let menu = session.project.add_menu(None)?;
        let other = session.project.add_menu(None)?;
        let _ = session
            .project
            .add_button(menu, "Go".to_owned(), None, Link::Stop)?;
        session.execute(
            "flowchart.moveNode",
            &json!({"node": other.0, "x": 300, "y": 40}),
        )?;
        let link = json!({
            "command": "button.setLink",
            "params": {"menu": menu.0, "button": 0, "target": {"menu": other.0}},
            "node": other.0,
        });
        session.execute("flowchart.link", &link)?;
        let button = |session: &Session| {
            session
                .project
                .menu(menu)
                .and_then(|menu| menu.buttons.first())
                .map(|button| button.link.clone())
        };
        assert!(matches!(button(&session), Some(Link::Menu { id, .. }) if id == other));
        assert!(session.project.flowchart.is_empty());
        session.execute("edit.undo", &json!({}))?;
        assert_eq!(button(&session), Some(Link::Stop));
        assert_eq!(session.project.flowchart.len(), 1);
        for bad in ["flowchart.arrange", "edit.undo", "no.such"] {
            let wrapped = json!({"command": bad, "params": {}, "node": other.0});
            assert!(
                session.execute("flowchart.link", &wrapped).is_err(),
                "{bad}"
            );
        }
        assert_eq!(session.project.flowchart.len(), 1);
        Ok(())
    }

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
