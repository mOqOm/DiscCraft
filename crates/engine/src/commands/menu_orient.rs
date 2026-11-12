//! Flipping and turning menu objects (Encore's Object ▸ Flip Horizontal / Flip Vertical / Rotate ▸): texts,
//! buttons and graphics. A quarter turn keeps the object's centre and swaps its width and height.

use crate::params::{opt_str, req_u64};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{MenuId, Orientation, Rect, frame_size};
use serde_json::Value;

use super::{always_enabled, empty_object};

pub(crate) const COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "menu.orient",
    label: "反転 / 回転",
    menu: &["オブジェクト"],
    shortcut: None,
    params: r#"{"menu": id, "texts"?: [index], "buttons"?: [index], "graphics"?: [index], "flip"?: "horizontal" | "vertical", "rotate"?: 90 | 180 | 270}"#,
    enabled: always_enabled,
    run: run_orient,
    journal: true,
}];

/// What happens to every object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    FlipHorizontal,
    FlipVertical,
    /// Quarter turns clockwise (1 to 3).
    Rotate(u8),
}

impl Change {
    fn apply(self, orientation: Orientation) -> Orientation {
        match self {
            Change::FlipHorizontal => orientation.flipped_horizontally(),
            Change::FlipVertical => orientation.flipped_vertically(),
            Change::Rotate(turns) => orientation.rotated(turns),
        }
    }

    fn swaps_sides(self) -> bool {
        matches!(self, Change::Rotate(1 | 3))
    }
}

/// Exactly one of "flip" and "rotate".
fn change(params: &Value) -> Result<Change, String> {
    let flip = opt_str(params, "flip")?;
    let rotate = params.get("rotate").filter(|value| !value.is_null());
    match (flip.as_deref(), rotate) {
        (Some("horizontal"), None) => Ok(Change::FlipHorizontal),
        (Some("vertical"), None) => Ok(Change::FlipVertical),
        (Some(_), None) => Err("\"flip\" must be \"horizontal\" or \"vertical\"".to_string()),
        (None, Some(degrees)) => match degrees.as_u64() {
            Some(90) => Ok(Change::Rotate(1)),
            Some(180) => Ok(Change::Rotate(2)),
            Some(270) => Ok(Change::Rotate(3)),
            _ => Err("\"rotate\" must be 90, 180 or 270 (degrees clockwise)".to_string()),
        },
        _ => Err("give exactly one of \"flip\" and \"rotate\"".to_string()),
    }
}

/// An optional list of object indexes (sorted, without repeats).
fn indexes(params: &Value, name: &str) -> Result<Vec<usize>, String> {
    let Some(value) = params.get(name).filter(|value| !value.is_null()) else {
        return Ok(Vec::new());
    };
    let items = value
        .as_array()
        .ok_or_else(|| format!("\"{name}\" must be a list of indexes"))?;
    let mut list = items
        .iter()
        .map(|item| {
            item.as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| format!("\"{name}\" must be a list of indexes"))
        })
        .collect::<Result<Vec<usize>, String>>()?;
    list.sort_unstable();
    list.dedup();
    Ok(list)
}

/// `rect` given the size `w` x `h`, keeping its centre, moved back inside the frame where it would stick out.
fn recentred(rect: Rect, w: u16, h: u16, frame: (u16, u16)) -> Rect {
    let place = |start: u16, old: u16, new: u16, limit: u16| -> u16 {
        let doubled_centre = u32::from(start)
            .saturating_mul(2)
            .saturating_add(u32::from(old));
        let start = doubled_centre.saturating_sub(u32::from(new)) / 2;
        let start = start.min(u32::from(limit.saturating_sub(new)));
        u16::try_from(start).unwrap_or(0)
    };
    Rect {
        x: place(rect.x, rect.w, w, frame.0),
        y: place(rect.y, rect.h, h, frame.1),
        w,
        h,
    }
}

/// The rectangle of a button or graphic after the change: turned a quarter, its sides swap around the centre.
/// Fails when it would no longer fit in the frame.
fn turned_rect(rect: Rect, change: Change, frame: (u16, u16), what: &str) -> Result<Rect, String> {
    if !change.swaps_sides() {
        return Ok(rect);
    }
    if rect.h > frame.0 || rect.w > frame.1 {
        return Err(format!("{what} would not fit in the frame once turned"));
    }
    Ok(recentred(rect, rect.h, rect.w, frame))
}

fn run_orient(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let change = change(params)?;
    let texts = indexes(params, "texts")?;
    let buttons = indexes(params, "buttons")?;
    let graphics = indexes(params, "graphics")?;
    if texts.is_empty() && buttons.is_empty() && graphics.is_empty() {
        return Err(
            "give the \"texts\", \"buttons\" or \"graphics\" to flip or rotate".to_string(),
        );
    }
    let frame = frame_size(&session.project.settings);
    let menu = session
        .project
        .menu(menu_id)
        .ok_or_else(|| format!("menu {} does not exist", menu_id.0))?;

    // Work every change out first, so a refusal leaves the menu as it was.
    let mut text_plan = Vec::with_capacity(texts.len());
    for index in texts {
        let text = menu
            .texts
            .get(index)
            .ok_or_else(|| format!("text {index} does not exist"))?;
        let before = disccraft_menu::text_rect(menu, text);
        let mut turned = text.clone();
        turned.orientation = change.apply(text.orientation);
        let after = disccraft_menu::text_rect(menu, &turned);
        let rect = recentred(before, after.w, after.h, frame);
        // A text's corner must stay inside the frame (a text larger than the frame starts at its corner).
        let x = rect.x.min(frame.0.saturating_sub(1));
        let y = rect.y.min(frame.1.saturating_sub(1));
        text_plan.push((index, turned.orientation, x, y));
    }
    let mut button_plan = Vec::with_capacity(buttons.len());
    for index in buttons {
        let button = menu
            .buttons
            .get(index)
            .ok_or_else(|| format!("button {index} does not exist"))?;
        let what = format!("button {}", index.saturating_add(1));
        let rect = turned_rect(button.rect, change, frame, &what)?;
        button_plan.push((index, change.apply(button.orientation), rect));
    }
    let mut graphic_plan = Vec::with_capacity(graphics.len());
    for index in graphics {
        let graphic = menu
            .graphics
            .get(index)
            .ok_or_else(|| format!("graphic {index} does not exist"))?;
        let rect = turned_rect(graphic.rect, change, frame, "the graphic")?;
        graphic_plan.push((index, change.apply(graphic.orientation), rect));
    }

    let menu = session
        .project
        .menu_mut(menu_id)
        .ok_or_else(|| format!("menu {} does not exist", menu_id.0))?;
    for (index, orientation, x, y) in text_plan {
        if let Some(text) = menu.texts.get_mut(index) {
            text.orientation = orientation;
            text.x = x;
            text.y = y;
        }
    }
    for (index, orientation, rect) in button_plan {
        if let Some(button) = menu.buttons.get_mut(index) {
            button.orientation = orientation;
            button.rect = rect;
        }
    }
    for (index, orientation, rect) in graphic_plan {
        if let Some(graphic) = menu.graphics.get_mut(index) {
            graphic.orientation = orientation;
            graphic.rect = rect;
        }
    }
    Ok(empty_object())
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use disccraft_project::{MenuId, Orientation, Rect};
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

    fn graphic(session: &Session, menu: u64) -> Option<(Rect, Orientation)> {
        session
            .project
            .menu(MenuId(menu))
            .and_then(|menu| menu.graphics.first())
            .map(|graphic| (graphic.rect, graphic.orientation))
    }

    #[test]
    fn a_quarter_turn_keeps_the_centre_and_swaps_the_sides() -> Result<(), Box<dyn Error>> {
        let (mut session, menu) = session_with_menu()?;
        session.execute(
            "menu.addShape",
            &json!({"menu": menu, "shape": "rectangle", "x": 100, "y": 100, "w": 100, "h": 50}),
        )?;
        session.execute(
            "menu.orient",
            &json!({"menu": menu, "graphics": [0], "rotate": 90}),
        )?;
        let (rect, orientation) = graphic(&session, menu).ok_or("no graphic")?;
        assert_eq!(
            rect,
            Rect {
                x: 125,
                y: 75,
                w: 50,
                h: 100
            }
        );
        assert_eq!(
            orientation,
            Orientation {
                flip: false,
                turns: 1
            }
        );
        session.execute(
            "menu.orient",
            &json!({"menu": menu, "graphics": [0], "rotate": 270}),
        )?;
        let (rect, orientation) = graphic(&session, menu).ok_or("no graphic")?;
        assert_eq!(
            rect,
            Rect {
                x: 100,
                y: 100,
                w: 100,
                h: 50
            }
        );
        assert!(orientation.is_identity());
        Ok(())
    }

    #[test]
    fn flips_add_up_like_mirrors() -> Result<(), Box<dyn Error>> {
        let (mut session, menu) = session_with_menu()?;
        session.execute(
            "menu.addShape",
            &json!({"menu": menu, "shape": "ellipse", "x": 10, "y": 10, "w": 40, "h": 20}),
        )?;
        session.execute(
            "menu.orient",
            &json!({"menu": menu, "graphics": [0], "flip": "horizontal"}),
        )?;
        assert_eq!(
            graphic(&session, menu).map(|(_, o)| o),
            Some(Orientation {
                flip: true,
                turns: 0
            })
        );
        // Mirroring both ways is half a turn; the rectangle does not move.
        session.execute(
            "menu.orient",
            &json!({"menu": menu, "graphics": [0], "flip": "vertical"}),
        )?;
        let (rect, orientation) = graphic(&session, menu).ok_or("no graphic")?;
        assert_eq!(
            orientation,
            Orientation {
                flip: false,
                turns: 2
            }
        );
        assert_eq!((rect.x, rect.y, rect.w, rect.h), (10, 10, 40, 20));
        session.execute("edit.undo", &json!({}))?;
        assert_eq!(
            graphic(&session, menu).map(|(_, o)| o),
            Some(Orientation {
                flip: true,
                turns: 0
            })
        );
        Ok(())
    }

    #[test]
    fn a_turn_that_does_not_fit_changes_nothing() -> Result<(), Box<dyn Error>> {
        let (mut session, menu) = session_with_menu()?;
        session.execute(
            "menu.addShape",
            &json!({"menu": menu, "shape": "rectangle", "x": 10, "y": 10, "w": 40, "h": 20}),
        )?;
        session.execute(
            "menu.addButton",
            &json!({"menu": menu, "label": "Wide", "rect": {"x": 20, "y": 300, "w": 600, "h": 60}}),
        )?;
        assert!(
            session
                .execute(
                    "menu.orient",
                    &json!({"menu": menu, "graphics": [0], "buttons": [0], "rotate": 90}),
                )
                .is_err()
        );
        assert_eq!(
            graphic(&session, menu).map(|(_, o)| o),
            Some(Orientation::default())
        );
        // Half a turn always fits.
        session.execute(
            "menu.orient",
            &json!({"menu": menu, "buttons": [0], "rotate": 180}),
        )?;
        let button = session
            .project
            .menu(MenuId(menu))
            .and_then(|menu| menu.buttons.first().cloned())
            .ok_or("no button")?;
        assert_eq!(button.orientation.turns, 2);
        assert_eq!(button.rect.w, 600);
        Ok(())
    }

    #[test]
    fn a_text_turns_about_its_centre() -> Result<(), Box<dyn Error>> {
        let (mut session, menu) = session_with_menu()?;
        session.execute(
            "menu.addText",
            &json!({"menu": menu, "text": "Welcome", "x": 200, "y": 200, "size": 40}),
        )?;
        let text = |session: &Session| {
            session
                .project
                .menu(MenuId(menu))
                .and_then(|menu| menu.texts.first().cloned())
        };
        session.execute(
            "menu.orient",
            &json!({"menu": menu, "texts": [0], "rotate": 90}),
        )?;
        let turned = text(&session).ok_or("no text")?;
        assert_eq!(turned.orientation.turns, 1);
        if !filmcraft_has_fonts() {
            return Ok(());
        }
        // A wide word stands up: it is now taller than it is wide, still centred where it was.
        let menu_ref = session.project.menu(MenuId(menu)).ok_or("menu")?;
        let rect = disccraft_menu::text_rect(menu_ref, &turned);
        assert!(rect.h > rect.w, "{rect:?}");
        Ok(())
    }

    /// Whether this computer has a font to measure text with.
    fn filmcraft_has_fonts() -> bool {
        !disccraft_menu::font_families().is_empty()
    }

    #[test]
    fn refuses_bad_requests() -> Result<(), Box<dyn Error>> {
        let (mut session, menu) = session_with_menu()?;
        session.execute(
            "menu.addShape",
            &json!({"menu": menu, "shape": "rectangle", "x": 10, "y": 10, "w": 40, "h": 20}),
        )?;
        for params in [
            json!({"menu": menu, "rotate": 90}),
            json!({"menu": menu, "graphics": [0]}),
            json!({"menu": menu, "graphics": [0], "rotate": 45}),
            json!({"menu": menu, "graphics": [0], "flip": "diagonal"}),
            json!({"menu": menu, "graphics": [0], "flip": "horizontal", "rotate": 90}),
            json!({"menu": menu, "graphics": [3], "rotate": 90}),
            json!({"menu": menu, "graphics": "0", "rotate": 90}),
            json!({"menu": menu, "texts": [0], "rotate": 90}),
            json!({"menu": 999, "graphics": [0], "rotate": 90}),
        ] {
            assert!(session.execute("menu.orient", &params).is_err(), "{params}");
        }
        Ok(())
    }
}
