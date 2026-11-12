use crate::{CommandResult, CommandSpec, Session};
use serde_json::Value;

use super::empty_object;
use crate::UNDO_LIMIT;

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "edit.undo",
        label: "元に戻す",
        menu: &["編集"],
        shortcut: Some("Ctrl+Z"),
        params: r#"{"steps"?: 1..100}"#,
        enabled: enabled_undo,
        run: run_undo,
        journal: false,
    },
    CommandSpec {
        id: "edit.redo",
        label: "やり直し",
        menu: &["編集"],
        shortcut: Some("Ctrl+Shift+Z"),
        params: r#"{"steps"?: 1..100}"#,
        enabled: enabled_redo,
        run: run_redo,
        journal: false,
    },
];

fn enabled_undo(session: &Session) -> Result<(), String> {
    if session.can_undo() {
        Ok(())
    } else {
        Err("nothing to undo".to_string())
    }
}

fn enabled_redo(session: &Session) -> Result<(), String> {
    if session.can_redo() {
        Ok(())
    } else {
        Err("nothing to redo".to_string())
    }
}

/// How many steps to go ("steps", 1 by default; Encore's Edit ▸ History jumps several at once).
fn steps(params: &Value) -> Result<usize, String> {
    match params.get("steps") {
        None | Some(Value::Null) => Ok(1),
        Some(value) => value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| (1..=UNDO_LIMIT).contains(n))
            .ok_or_else(|| format!("\"steps\" must be 1 to {UNDO_LIMIT}")),
    }
}

fn run_undo(session: &mut Session, params: &Value) -> CommandResult {
    let steps = steps(params)?;
    if steps > session.undo_depth() {
        return Err(format!("only {} steps can be undone", session.undo_depth()));
    }
    for _ in 0..steps {
        session.undo()?;
    }
    Ok(empty_object())
}

fn run_redo(session: &mut Session, params: &Value) -> CommandResult {
    let steps = steps(params)?;
    if steps > session.redo_depth() {
        return Err(format!("only {} steps can be redone", session.redo_depth()));
    }
    for _ in 0..steps {
        session.redo()?;
    }
    Ok(empty_object())
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use serde_json::json;
    use std::error::Error;

    #[test]
    fn several_steps_are_undone_and_redone_at_once() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        for name in ["A", "B", "C"] {
            session.execute("menu.create", &json!({ "name": name }))?;
        }
        assert_eq!(session.undo_depth(), 3);
        session.execute("edit.undo", &json!({"steps": 2}))?;
        assert_eq!(session.project.menus.len(), 1);
        assert_eq!((session.undo_depth(), session.redo_depth()), (1, 2));
        assert_eq!(session.redo_labels().len(), 2);
        session.execute("edit.redo", &json!({"steps": 2}))?;
        assert_eq!(session.project.menus.len(), 3);
        for params in [
            json!({"steps": 0}),
            json!({"steps": 4}),
            json!({"steps": "two"}),
        ] {
            assert!(session.execute("edit.undo", &params).is_err(), "{params}");
        }
        // A refused jump changes nothing.
        assert_eq!(session.project.menus.len(), 3);
        Ok(())
    }
}
