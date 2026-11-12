//! Graphic layers on menus (Encore's library images and shapes): add, change, reorder and remove.

use crate::params::{opt_str, opt_u64, req_str, req_u64, req_usize};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{
    AssetId, FULL_OPACITY, GraphicKind, Link, MAX_TEXT_SIZE, MIN_TEXT_SIZE, MenuGraphic, MenuId,
    MenuText, Rect, ShapeKind, TextFormat, frame_size,
};
use serde_json::Value;

use super::menu::parse_color;
use super::{always_enabled, empty_object, object};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "menu.addImage",
        label: "画像を配置",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "asset": id, "x"?: n, "y"?: n, "w"?: n, "h"?: n, "opacity"?: 1..100}"#,
        enabled: always_enabled,
        run: run_add_image,
        journal: true,
    },
    CommandSpec {
        id: "menu.addShape",
        label: "シェイプを配置",
        menu: &["メニュー"],
        shortcut: None,
        params: r##"{"menu": id, "shape": "rectangle" | "roundedRectangle" | "ellipse", "color"?: "#RRGGBB", "x"?: n, "y"?: n, "w"?: n, "h"?: n, "opacity"?: 1..100}"##,
        enabled: always_enabled,
        run: run_add_shape,
        journal: true,
    },
    CommandSpec {
        id: "menu.setGraphic",
        label: "グラフィックを変更",
        menu: &["メニュー"],
        shortcut: None,
        params: r##"{"menu": id, "index": n, "x"?: n, "y"?: n, "w"?: n, "h"?: n, "color"?: "#RRGGBB", "shape"?: string, "asset"?: id, "opacity"?: 1..100}"##,
        enabled: always_enabled,
        run: run_set_graphic,
        journal: true,
    },
    CommandSpec {
        id: "menu.moveGraphic",
        label: "グラフィックの重なりを変更",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "from": n, "to": n}"#,
        enabled: always_enabled,
        run: run_move_graphic,
        journal: true,
    },
    CommandSpec {
        id: "menu.convertToButton",
        label: "ボタンに変換",
        menu: &["メニュー"],
        shortcut: Some("Ctrl+B"),
        params: r#"{"menu": id, "text"?: n, "graphic"?: n}"#,
        enabled: always_enabled,
        run: run_convert_to_button,
        journal: true,
    },
    CommandSpec {
        id: "menu.convertToObject",
        label: "オブジェクトに変換",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "button": n}"#,
        enabled: always_enabled,
        run: run_convert_to_object,
        journal: true,
    },
    CommandSpec {
        id: "menu.removeGraphic",
        label: "グラフィックを削除",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "index": n}"#,
        enabled: always_enabled,
        run: run_remove_graphic,
        journal: true,
    },
];

/// A frame coordinate parameter.
fn coord(params: &Value, name: &str) -> Result<Option<u16>, String> {
    opt_u64(params, name)?
        .map(|value| u16::try_from(value).map_err(|_| format!("parameter \"{name}\" is too large")))
        .transpose()
}

/// The optional opacity parameter (1 to 100).
fn opacity(params: &Value) -> Result<Option<u8>, String> {
    opt_u64(params, "opacity")?
        .map(|value| {
            u8::try_from(value)
                .ok()
                .filter(|value| (1..=FULL_OPACITY).contains(value))
                .ok_or_else(|| "parameter \"opacity\" must be 1 to 100".to_string())
        })
        .transpose()
}

/// The rectangle of a new graphic: the given corner and size, or a box of a third of the frame in its middle.
fn new_rect(params: &Value, frame: (u16, u16)) -> Result<Rect, String> {
    let (width, height) = frame;
    let w = coord(params, "w")?.unwrap_or(width / 3);
    let h = coord(params, "h")?.unwrap_or(height / 3);
    let x = coord(params, "x")?.unwrap_or(width.saturating_sub(w) / 2);
    let y = coord(params, "y")?.unwrap_or(height.saturating_sub(h) / 2);
    Ok(Rect { x, y, w, h })
}

fn add_graphic(session: &mut Session, params: &Value, kind: GraphicKind) -> CommandResult {
    let menu = MenuId(req_u64(params, "menu")?);
    let frame = frame_size(&session.project.settings);
    let rect = new_rect(params, frame)?;
    let opacity = opacity(params)?.unwrap_or(FULL_OPACITY);
    let index = session
        .project
        .add_menu_graphic(
            menu,
            MenuGraphic {
                rect,
                kind,
                opacity,
                shadow: None,
                orientation: Default::default(),
            },
        )
        .map_err(|error| error.to_string())?;
    let index = u64::try_from(index).map_err(|_| "graphic index is too large".to_string())?;
    Ok(object(&[("index", Value::from(index))]))
}

fn run_add_image(session: &mut Session, params: &Value) -> CommandResult {
    let asset = AssetId(req_u64(params, "asset")?);
    add_graphic(session, params, GraphicKind::Image { asset })
}

fn shape_param(text: &str) -> Result<ShapeKind, String> {
    ShapeKind::parse(text).ok_or_else(|| {
        "parameter \"shape\" must be \"rectangle\", \"roundedRectangle\" or \"ellipse\"".to_string()
    })
}

fn run_add_shape(session: &mut Session, params: &Value) -> CommandResult {
    let shape = shape_param(&req_str(params, "shape")?)?;
    let color = match opt_str(params, "color")? {
        Some(color) => parse_color(&color)?,
        None => [255, 255, 255],
    };
    add_graphic(session, params, GraphicKind::Shape { shape, color })
}

fn run_set_graphic(session: &mut Session, params: &Value) -> CommandResult {
    let menu = MenuId(req_u64(params, "menu")?);
    let index = req_usize(params, "index")?;
    let x = coord(params, "x")?;
    let y = coord(params, "y")?;
    let w = coord(params, "w")?;
    let h = coord(params, "h")?;
    let color = opt_str(params, "color")?
        .map(|color| parse_color(&color))
        .transpose()?;
    let shape = opt_str(params, "shape")?
        .map(|shape| shape_param(&shape))
        .transpose()?;
    let asset = opt_u64(params, "asset")?.map(AssetId);
    let opacity = opacity(params)?;

    let current = session
        .project
        .menu(menu)
        .and_then(|menu| menu.graphics.get(index))
        .cloned()
        .ok_or_else(|| format!("graphic {index} does not exist"))?;

    if x.is_some() || y.is_some() || w.is_some() || h.is_some() {
        let rect = Rect {
            x: x.unwrap_or(current.rect.x),
            y: y.unwrap_or(current.rect.y),
            w: w.unwrap_or(current.rect.w),
            h: h.unwrap_or(current.rect.h),
        };
        session
            .project
            .set_menu_graphic_rect(menu, index, rect)
            .map_err(|error| error.to_string())?;
    }
    let kind = match (current.kind, asset, shape, color) {
        (_, Some(asset), _, _) => Some(GraphicKind::Image { asset }),
        (
            GraphicKind::Shape {
                shape: old,
                color: old_color,
            },
            None,
            shape,
            color,
        ) => Some(GraphicKind::Shape {
            shape: shape.unwrap_or(old),
            color: color.unwrap_or(old_color),
        }),
        (GraphicKind::Image { .. }, None, Some(shape), color) => Some(GraphicKind::Shape {
            shape,
            color: color.unwrap_or([255, 255, 255]),
        }),
        (GraphicKind::Image { .. }, None, None, Some(_)) => {
            return Err("an image has no colour; give a shape to turn it into one".to_string());
        }
        (GraphicKind::Image { .. }, None, None, None) => None,
    };
    if let Some(kind) = kind
        && kind != current.kind
    {
        session
            .project
            .set_menu_graphic_kind(menu, index, kind)
            .map_err(|error| error.to_string())?;
    }
    if let Some(opacity) = opacity {
        session
            .project
            .set_menu_graphic_opacity(menu, index, opacity)
            .map_err(|error| error.to_string())?;
    }
    Ok(empty_object())
}

fn run_move_graphic(session: &mut Session, params: &Value) -> CommandResult {
    let menu = MenuId(req_u64(params, "menu")?);
    let from = req_usize(params, "from")?;
    let to = req_usize(params, "to")?;
    session
        .project
        .move_menu_graphic(menu, from, to)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

/// Encore's オブジェクト ▸ ボタンに変換: a text becomes a button with the text as its label (the text object goes);
/// an image or shape becomes a button over it (the artwork stays as the button's picture).
fn run_convert_to_button(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let text = opt_u64(params, "text")?;
    let graphic = opt_u64(params, "graphic")?;
    let frame = frame_size(&session.project.settings);
    let (label, rect) = match (text, graphic) {
        (Some(index), None) => {
            let index =
                usize::try_from(index).map_err(|_| "text index is too large".to_string())?;
            let menu = session
                .project
                .menu(menu_id)
                .ok_or_else(|| format!("menu {} does not exist", menu_id.0))?;
            let text = menu
                .texts
                .get(index)
                .ok_or_else(|| format!("text {index} does not exist"))?;
            let rect = disccraft_menu::text_rect(menu, text);
            let label = text.text.lines().next().unwrap_or_default().to_owned();
            let menu = session
                .project
                .menu_mut(menu_id)
                .ok_or_else(|| format!("menu {} does not exist", menu_id.0))?;
            let _ = menu.texts.remove(index);
            (label, rect)
        }
        (None, Some(index)) => {
            let index =
                usize::try_from(index).map_err(|_| "graphic index is too large".to_string())?;
            let rect = session
                .project
                .menu(menu_id)
                .and_then(|menu| menu.graphics.get(index))
                .map(|graphic| graphic.rect)
                .ok_or_else(|| format!("graphic {index} does not exist"))?;
            (String::new(), rect)
        }
        _ => return Err("give exactly one of \"text\" or \"graphic\"".to_string()),
    };
    // Keep the button inside the frame.
    let rect = Rect {
        x: rect.x.min(frame.0.saturating_sub(1)),
        y: rect.y.min(frame.1.saturating_sub(1)),
        w: rect.w.max(1).min(
            frame
                .0
                .saturating_sub(rect.x.min(frame.0.saturating_sub(1))),
        ),
        h: rect.h.max(1).min(
            frame
                .1
                .saturating_sub(rect.y.min(frame.1.saturating_sub(1))),
        ),
    };
    let index = session
        .project
        .add_button(menu_id, label, Some(rect), Link::NotSet)
        .map_err(|error| error.to_string())?;
    let button = u64::try_from(index).map_err(|_| "button index is too large".to_string())?;
    Ok(object(&[("button", Value::from(button))]))
}

/// Encore's オブジェクト ▸ オブジェクトに変換: the button goes; its text stays as a text object of the same size.
fn run_convert_to_object(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let index = req_usize(params, "button")?;
    let button = session
        .project
        .menu(menu_id)
        .and_then(|menu| menu.buttons.get(index))
        .cloned()
        .ok_or_else(|| format!("button {index} does not exist"))?;
    session
        .project
        .remove_button(menu_id, index)
        .map_err(|error| error.to_string())?;
    if button.label.trim().is_empty() {
        return Ok(empty_object());
    }
    let menu = session
        .project
        .menu_mut(menu_id)
        .ok_or_else(|| format!("menu {} does not exist", menu_id.0))?;
    menu.texts.push(MenuText {
        text: button.label,
        x: button.rect.x,
        y: button.rect.y,
        size: button.rect.h.clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE),
        color: None,
        font: None,
        font_style: None,
        format: TextFormat::default(),
        shadow: None,
        orientation: Default::default(),
    });
    let text = u64::try_from(menu.texts.len().saturating_sub(1))
        .map_err(|_| "text index is too large".to_string())?;
    Ok(object(&[("text", Value::from(text))]))
}

fn run_remove_graphic(session: &mut Session, params: &Value) -> CommandResult {
    let menu = MenuId(req_u64(params, "menu")?);
    let index = req_usize(params, "index")?;
    session
        .project
        .remove_menu_graphic(menu, index)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use disccraft_project::{GraphicKind, ShapeKind};
    use serde_json::{Value, json};
    use std::error::Error;

    #[test]
    fn graphics_are_added_changed_reordered_and_undone() -> Result<(), Box<dyn Error>> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "disccraft-menu-graphic-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)?;
        let logo = dir.join("logo.png");
        std::fs::File::create(&logo)?;
        let mut session = Session::new();
        let asset = session.project.add_asset(&logo)?;
        let menu = session.project.add_menu(None)?;

        let added = session.execute(
            "menu.addShape",
            &json!({ "menu": menu.0, "shape": "ellipse", "color": "#ff8000", "x": 10, "y": 20, "w": 100, "h": 50 }),
        )?;
        assert_eq!(added.get("index").and_then(Value::as_u64), Some(0));
        let added = session.execute(
            "menu.addImage",
            &json!({ "menu": menu.0, "asset": asset.0, "opacity": 60 }),
        )?;
        assert_eq!(added.get("index").and_then(Value::as_u64), Some(1));
        // The default box is a third of the frame, centred.
        let image = session
            .project
            .menu(menu)
            .and_then(|m| m.graphics.get(1))
            .cloned()
            .ok_or("missing image")?;
        assert_eq!(
            (image.rect.x, image.rect.y, image.rect.w, image.rect.h),
            (240, 160, 240, 160)
        );
        assert_eq!(image.opacity, 60);

        assert!(
            session
                .execute("menu.addShape", &json!({ "menu": menu.0, "shape": "star" }))
                .is_err()
        );
        assert!(
            session
                .execute(
                    "menu.addShape",
                    &json!({ "menu": menu.0, "shape": "rectangle", "x": 700, "w": 100 })
                )
                .is_err()
        );

        session.execute(
            "menu.setGraphic",
            &json!({ "menu": menu.0, "index": 0, "shape": "roundedRectangle", "x": 0, "opacity": 30 }),
        )?;
        session.execute(
            "menu.moveGraphic",
            &json!({ "menu": menu.0, "from": 0, "to": 1 }),
        )?;
        let graphics = session
            .project
            .menu(menu)
            .map(|m| m.graphics.clone())
            .ok_or("missing menu")?;
        assert!(matches!(
            graphics.first().map(|g| g.kind),
            Some(GraphicKind::Image { .. })
        ));
        let shape = graphics.get(1).ok_or("missing shape")?;
        assert_eq!(
            shape.kind,
            GraphicKind::Shape {
                shape: ShapeKind::RoundedRectangle,
                color: [255, 128, 0]
            }
        );
        assert_eq!((shape.rect.x, shape.opacity), (0, 30));

        session.execute("menu.removeGraphic", &json!({ "menu": menu.0, "index": 1 }))?;
        assert_eq!(
            session.project.menu(menu).map(|m| m.graphics.len()),
            Some(1)
        );
        for _ in 0..5 {
            session.undo()?;
        }
        assert_eq!(
            session.project.menu(menu).map(|m| m.graphics.len()),
            Some(0)
        );
        Ok(())
    }

    /// A shape becomes a button over it; a text becomes a button with its words; a button becomes a text again.
    #[test]
    fn objects_and_buttons_convert_into_each_other() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu = session.project.add_menu(None)?;
        session.execute(
            "menu.addShape",
            &json!({ "menu": menu.0, "shape": "rectangle", "x": 100, "y": 50, "w": 300, "h": 60 }),
        )?;
        session.execute(
            "menu.addText",
            &json!({ "menu": menu.0, "text": "Bonus\nSecond line", "x": 20, "y": 400, "size": 40 }),
        )?;

        let shape_button = session.execute(
            "menu.convertToButton",
            &json!({ "menu": menu.0, "graphic": 0 }),
        )?;
        assert_eq!(shape_button.get("button").and_then(Value::as_u64), Some(0));
        let text_button = session.execute(
            "menu.convertToButton",
            &json!({ "menu": menu.0, "text": 0 }),
        )?;
        assert_eq!(text_button.get("button").and_then(Value::as_u64), Some(1));
        assert!(
            session
                .execute("menu.convertToButton", &json!({ "menu": menu.0 }))
                .is_err()
        );
        let m = session.project.menu(menu).cloned().ok_or("menu")?;
        assert_eq!(m.graphics.len(), 1);
        assert!(m.texts.is_empty());
        let first = m.buttons.first().ok_or("button")?;
        assert_eq!((first.rect.x, first.rect.w), (100, 300));
        assert!(first.label.is_empty());
        let second = m.buttons.get(1).ok_or("button")?;
        assert_eq!(second.label, "Bonus");
        assert_eq!((second.rect.x, second.rect.y), (20, 400));

        let text = session.execute(
            "menu.convertToObject",
            &json!({ "menu": menu.0, "button": 1 }),
        )?;
        assert_eq!(text.get("text").and_then(Value::as_u64), Some(0));
        let m = session.project.menu(menu).cloned().ok_or("menu")?;
        assert_eq!(m.buttons.len(), 1);
        assert_eq!(m.texts.first().map(|t| t.text.as_str()), Some("Bonus"));

        session.undo()?;
        session.undo()?;
        session.undo()?;
        let m = session.project.menu(menu).cloned().ok_or("menu")?;
        assert!(m.buttons.is_empty());
        assert_eq!(m.texts.len(), 1);
        Ok(())
    }
}
