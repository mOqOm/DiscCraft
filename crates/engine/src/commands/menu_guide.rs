//! Guides on menus (Encore's menu viewer guides): add, move, remove and clear. Guides only help placing buttons
//! in the viewer; nothing of them reaches the disc.

use crate::params::{req_str, req_u64, req_usize};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{Guide, GuideOrientation, MAX_MENU_GUIDES, MenuId, frame_size};
use serde_json::Value;

use super::{always_enabled, empty_object, object};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "menu.addGuide",
        label: "ガイドを追加",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "orientation": "horizontal" | "vertical", "position": n}"#,
        enabled: always_enabled,
        run: run_add_guide,
        journal: true,
    },
    CommandSpec {
        id: "menu.moveGuide",
        label: "ガイドを移動",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "index": n, "position": n}"#,
        enabled: always_enabled,
        run: run_move_guide,
        journal: true,
    },
    CommandSpec {
        id: "menu.removeGuide",
        label: "ガイドを削除",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "index": n}"#,
        enabled: always_enabled,
        run: run_remove_guide,
        journal: true,
    },
    CommandSpec {
        id: "menu.clearGuides",
        label: "ガイドを消去",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id}"#,
        enabled: always_enabled,
        run: run_clear_guides,
        journal: true,
    },
];

/// Parses `"horizontal"` / `"vertical"`.
fn parse_orientation(text: &str) -> Result<GuideOrientation, String> {
    match text {
        "horizontal" => Ok(GuideOrientation::Horizontal),
        "vertical" => Ok(GuideOrientation::Vertical),
        _ => Err("parameter \"orientation\" must be \"horizontal\" or \"vertical\"".to_string()),
    }
}

/// A guide position must lie on the frame: a horizontal guide within the height, a vertical one within the width.
fn checked_position(
    position: u64,
    orientation: GuideOrientation,
    frame: (u16, u16),
) -> Result<u16, String> {
    let limit = match orientation {
        GuideOrientation::Horizontal => frame.1,
        GuideOrientation::Vertical => frame.0,
    };
    match u16::try_from(position) {
        Ok(position) if position <= limit => Ok(position),
        _ => Err(format!("parameter \"position\" must be at most {limit}")),
    }
}

fn run_add_guide(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let frame = frame_size(&session.project.settings);
    let orientation = parse_orientation(&req_str(params, "orientation")?)?;
    let position = checked_position(req_u64(params, "position")?, orientation, frame)?;
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    if menu.guides.len() >= MAX_MENU_GUIDES {
        return Err(format!("a menu holds at most {MAX_MENU_GUIDES} guides"));
    }
    menu.guides.push(Guide {
        orientation,
        position,
    });
    let index = u64::try_from(menu.guides.len().saturating_sub(1))
        .map_err(|_| "guide index is too large".to_string())?;
    Ok(object(&[("index", Value::from(index))]))
}

fn run_move_guide(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let index = req_usize(params, "index")?;
    let position = req_u64(params, "position")?;
    let frame = frame_size(&session.project.settings);
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let Some(guide) = menu.guides.get_mut(index) else {
        return Err(format!("guide {index} does not exist"));
    };
    guide.position = checked_position(position, guide.orientation, frame)?;
    Ok(empty_object())
}

fn run_remove_guide(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let index = req_usize(params, "index")?;
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    if index >= menu.guides.len() {
        return Err(format!("guide {index} does not exist"));
    }
    let _ = menu.guides.remove(index);
    Ok(empty_object())
}

fn run_clear_guides(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.guides.clear();
    Ok(empty_object())
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use disccraft_project::{GuideOrientation, MenuId};
    use serde_json::json;
    use std::error::Error;

    fn session_with_menu() -> Result<(Session, u64), Box<dyn Error>> {
        let mut session = Session::new();
        let created = session.execute("menu.create", &json!({"name": "Main"}))?;
        let menu = created
            .get("menu")
            .and_then(serde_json::Value::as_u64)
            .ok_or("no menu id")?;
        Ok((session, menu))
    }

    #[test]
    fn guides_are_added_moved_removed_and_cleared() -> Result<(), Box<dyn Error>> {
        let (mut session, menu) = session_with_menu()?;
        let added = session.execute(
            "menu.addGuide",
            &json!({"menu": menu, "orientation": "horizontal", "position": 240}),
        )?;
        assert_eq!(
            added.get("index").and_then(serde_json::Value::as_u64),
            Some(0)
        );
        let _ = session.execute(
            "menu.addGuide",
            &json!({"menu": menu, "orientation": "vertical", "position": 360}),
        )?;
        let guides = session
            .project
            .menu(MenuId(menu))
            .map(|menu| menu.guides.clone())
            .ok_or("no menu")?;
        assert_eq!(guides.len(), 2);
        assert_eq!(guides[0].orientation, GuideOrientation::Horizontal);
        assert_eq!(guides[0].position, 240);
        assert_eq!(guides[1].orientation, GuideOrientation::Vertical);
        assert_eq!(guides[1].position, 360);

        let _ = session.execute(
            "menu.moveGuide",
            &json!({"menu": menu, "index": 0, "position": 100}),
        )?;
        assert_eq!(
            session
                .project
                .menu(MenuId(menu))
                .and_then(|menu| menu.guides.first())
                .map(|guide| guide.position),
            Some(100)
        );

        let _ = session.execute("menu.removeGuide", &json!({"menu": menu, "index": 0}))?;
        assert_eq!(
            session
                .project
                .menu(MenuId(menu))
                .map(|menu| menu.guides.len()),
            Some(1)
        );
        // Undo brings the removed guide back.
        let _ = session.execute("edit.undo", &json!({}))?;
        assert_eq!(
            session
                .project
                .menu(MenuId(menu))
                .map(|menu| menu.guides.len()),
            Some(2)
        );

        let _ = session.execute("menu.clearGuides", &json!({"menu": menu}))?;
        assert_eq!(
            session
                .project
                .menu(MenuId(menu))
                .map(|menu| menu.guides.is_empty()),
            Some(true)
        );
        Ok(())
    }

    #[test]
    fn bad_guides_are_refused() -> Result<(), Box<dyn Error>> {
        let (mut session, menu) = session_with_menu()?;
        for params in [
            json!({"menu": menu, "orientation": "diagonal", "position": 10}),
            // Beyond the 720x480 frame.
            json!({"menu": menu, "orientation": "horizontal", "position": 481}),
            json!({"menu": menu, "orientation": "vertical", "position": 721}),
            json!({"menu": menu + 1, "orientation": "vertical", "position": 1}),
        ] {
            assert!(
                session.execute("menu.addGuide", &params).is_err(),
                "{params}"
            );
        }
        assert!(
            session
                .execute(
                    "menu.moveGuide",
                    &json!({"menu": menu, "index": 0, "position": 1})
                )
                .is_err()
        );
        assert!(
            session
                .execute("menu.removeGuide", &json!({"menu": menu, "index": 0}))
                .is_err()
        );
        for _ in 0..disccraft_project::MAX_MENU_GUIDES {
            let _ = session.execute(
                "menu.addGuide",
                &json!({"menu": menu, "orientation": "horizontal", "position": 0}),
            )?;
        }
        assert!(
            session
                .execute(
                    "menu.addGuide",
                    &json!({"menu": menu, "orientation": "horizontal", "position": 0}),
                )
                .is_err()
        );
        Ok(())
    }
}
