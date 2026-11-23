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
        id: "menu.setLayer",
        label: "レイヤーの表示 / ロックを設定",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "kind": "button" | "text" | "graphic", "index": n, "hidden"?: bool, "locked"?: bool}"#,
        enabled: always_enabled,
        run: run_set_layer,
        journal: true,
    },
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
        id: "menu.placePsdButton",
        label: "PSD のボタンを配置（ライブラリのボタン）",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "path": "button.psd", "x"?: n, "y"?: n (PSD の左上をそこに。省略で中央)} → {"graphic": index, "button": index}"#,
        enabled: always_enabled,
        run: run_place_psd_button,
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
        id: "menu.placeLayers",
        label: "レイヤーを配置（ライブラリのレイヤーセット / 置換レイヤー）",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "graphics": [a menu graphic (as the project file holds it), ...] (下から順), "x"?: n, "y"?: n (全体の左上をそこに。省略で元の場所)} → {"graphics": [index, ...]}"#,
        enabled: always_enabled,
        run: run_place_layers,
        journal: true,
    },
    CommandSpec {
        id: "menu.setEffects",
        label: "レイヤー効果を設定",
        menu: &["オブジェクト"],
        shortcut: None,
        params: r##"{"menu": id, "graphic" | "text" | "button": index (画像・シェイプ / テキスト / ボタンの文字), "effects": {"stroke"?: {"size", "color": [r, g, b], "opacity"}, "outerGlow"? / "innerGlow"?: {"size", "spread", "color", "opacity"}, "bevel"?: {"size", "angle", "highlight", "shadow"}, "overlay"?: {"color", "opacity"}} | null (なし)}"##,
        enabled: always_enabled,
        run: run_set_effects,
        journal: true,
    },
    CommandSpec {
        id: "menu.setGraphic",
        label: "グラフィックを変更",
        menu: &["メニュー"],
        shortcut: None,
        params: r##"{"menu": id, "index": n, "x"?: n, "y"?: n, "w"?: n, "h"?: n, "color"?: "#RRGGBB", "shape"?: string, "asset"?: id, "opacity"?: 1..100, "replacement"?: bool (Encore's replacement layer)}"##,
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
/// Encore's Layers panel eye and lock on a button set, a text or a graphic: hidden is not drawn (in the viewer nor on
/// the disc; a hidden button stays a button), locked cannot be picked in the menu viewer.
fn run_set_layer(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let kind = req_str(params, "kind")?;
    let index = req_usize(params, "index")?;
    let flag = |name: &str| match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(format!("parameter \"{name}\" must be a boolean")),
    };
    let (hidden, locked) = (flag("hidden")?, flag("locked")?);
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let (slot_hidden, slot_locked) = match kind.as_str() {
        "button" => menu
            .buttons
            .get_mut(index)
            .map(|layer| (&mut layer.hidden, &mut layer.locked)),
        "text" => menu
            .texts
            .get_mut(index)
            .map(|layer| (&mut layer.hidden, &mut layer.locked)),
        "graphic" => menu
            .graphics
            .get_mut(index)
            .map(|layer| (&mut layer.hidden, &mut layer.locked)),
        _ => return Err("\"kind\" must be \"button\", \"text\" or \"graphic\"".to_string()),
    }
    .ok_or_else(|| format!("{kind} {index} does not exist"))?;
    if let Some(hidden) = hidden {
        *slot_hidden = hidden;
    }
    if let Some(locked) = locked {
        *slot_locked = locked;
    }
    Ok(empty_object())
}

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
                hidden: false,
                locked: false,
                replacement: false,
                effects: Default::default(),
            },
        )
        .map_err(|error| error.to_string())?;
    let index = u64::try_from(index).map_err(|_| "graphic index is too large".to_string())?;
    Ok(object(&[("index", Value::from(index))]))
}

/// How a PSD of the Library goes onto the frame (M6.405): at its own size when it fits in it, else stretched into it
/// as a menu's PSD of another size is.
#[derive(Clone, Copy, Debug)]
struct CanvasFit {
    canvas: (u32, u32),
    frame: (u16, u16),
    fits: bool,
}

impl CanvasFit {
    fn new(canvas: (u32, u32), frame: (u16, u16)) -> Self {
        let canvas = (canvas.0.max(1), canvas.1.max(1));
        let fits = canvas.0 <= u32::from(frame.0) && canvas.1 <= u32::from(frame.1);
        Self {
            canvas,
            frame,
            fits,
        }
    }

    fn scale(&self, value: u32, canvas: u32, frame: u16) -> u16 {
        let scaled = if self.fits {
            u64::from(value)
        } else {
            u64::from(value) * u64::from(frame) / u64::from(canvas)
        };
        u16::try_from(scaled).unwrap_or(u16::MAX)
    }

    /// A length across the canvas, in the frame.
    fn x(&self, value: u32) -> u16 {
        self.scale(value, self.canvas.0, self.frame.0)
    }

    /// A length down the canvas, in the frame.
    fn y(&self, value: u32) -> u16 {
        self.scale(value, self.canvas.1, self.frame.1)
    }
}

/// A button of the Library placed in a menu (M6.405, Encore's Library ▸ Place of a button PSD; docs/encore-behavior.md
/// 9.18): the PSD as the button's picture (an image layer, its highlight layers left out) and a button over its set,
/// as a PSD menu's button is made: the set's place, its `(=1)` to `(=3)` layers the button's subpicture, its `(%)`
/// layer its video thumbnail. The PSD at its own size (one larger than the frame stretched into it, as a menu's PSD
/// is: the subpicture is then left to the button's frame), its top left corner at `x`, `y` (kept in the frame) or
/// centred.
fn run_place_psd_button(session: &mut Session, params: &Value) -> CommandResult {
    let menu = MenuId(req_u64(params, "menu")?);
    let path = req_str(params, "path")?;
    if session.project.menu(menu).is_none() {
        return Err(format!("menu {} does not exist", menu.0));
    }
    let psd = disccraft_photo::load_psd_menu(std::path::Path::new(&path))
        .map_err(|error| format!("{path}: {error}"))?;
    let button = psd
        .buttons
        .first()
        .ok_or_else(|| format!("{path} has no button set"))?;
    let frame = frame_size(&session.project.settings);
    let fit = CanvasFit::new((psd.width, psd.height), frame);
    let (w, h) = (
        fit.x(psd.width).clamp(1, frame.0.max(1)),
        fit.y(psd.height).clamp(1, frame.1.max(1)),
    );
    let place = |name: &str, extent: u16, frame: u16| -> Result<u16, String> {
        let most = frame.saturating_sub(extent);
        Ok(match opt_u64(params, name)? {
            Some(value) => u16::try_from(value).unwrap_or(u16::MAX).min(most),
            None => most / 2,
        })
    };
    let (x, y) = (place("x", w, frame.0)?, place("y", h, frame.1)?);
    let before = session.project.clone();
    let placed = place_psd_button(session, menu, (&path, button), Rect { x, y, w, h }, fit);
    if placed.is_err() {
        session.project = before;
    }
    let (graphic, index) = placed?;
    Ok(object(&[
        (
            "graphic",
            Value::from(u64::try_from(graphic).unwrap_or(u64::MAX)),
        ),
        (
            "button",
            Value::from(u64::try_from(index).unwrap_or(u64::MAX)),
        ),
    ]))
}

/// The picture (the PSD at `path` over `rect`) and the button of [`run_place_psd_button`]: their numbers.
fn place_psd_button(
    session: &mut Session,
    menu: MenuId,
    (path, button): (&str, &disccraft_photo::PsdButton),
    rect: Rect,
    fit: CanvasFit,
) -> Result<(usize, usize), String> {
    let (frame_w, frame_h) = fit.frame;
    let asset = match session
        .project
        .assets
        .values()
        .find(|asset| std::path::Path::new(&asset.path) == std::path::Path::new(path))
    {
        Some(asset) => asset.id,
        None => session
            .project
            .add_asset(std::path::Path::new(path))
            .map_err(|error| error.to_string())?,
    };
    let graphic = session
        .project
        .add_menu_graphic(
            menu,
            MenuGraphic {
                rect,
                kind: GraphicKind::Image { asset },
                opacity: FULL_OPACITY,
                shadow: None,
                orientation: Default::default(),
                hidden: false,
                locked: false,
                replacement: false,
                effects: Default::default(),
            },
        )
        .map_err(|error| error.to_string())?;
    // The set's place in the frame.
    let left = rect
        .x
        .saturating_add(fit.x(button.x))
        .min(frame_w.saturating_sub(1));
    let top = rect
        .y
        .saturating_add(fit.y(button.y))
        .min(frame_h.saturating_sub(1));
    let set = Rect {
        x: left,
        y: top,
        w: fit.x(button.width).max(1).min(frame_w.saturating_sub(left)),
        h: fit.y(button.height).max(1).min(frame_h.saturating_sub(top)),
    };
    let index = session
        .project
        .add_button(menu, String::new(), Some(set), Link::NotSet)
        .map_err(|error| error.to_string())?;
    // Its own highlight layers, at their own size only.
    let subpicture = button
        .subpicture
        .as_ref()
        .filter(|_| fit.fits)
        .and_then(|sub| {
            let (width, height) = (
                u16::try_from(sub.width).ok()?,
                u16::try_from(sub.height).ok()?,
            );
            let mut made = disccraft_project::Subpicture::from_values(width, height, &sub.values)?;
            made.color_layers = sub.layers.each_ref().map(|layer| {
                layer
                    .as_deref()
                    .and_then(|rgba| disccraft_project::ColorLayer::from_rgba(width, height, rgba))
            });
            Some(made)
        });
    let thumbnail = button.thumbnail.and_then(|[tx, ty, tw, th]| {
        let area = Rect {
            x: rect.x.saturating_add(fit.x(tx)),
            y: rect.y.saturating_add(fit.y(ty)),
            w: fit.x(tw).max(1),
            h: fit.y(th).max(1),
        };
        disccraft_project::ThumbnailArea::within(set, area)
    });
    if let Some(made) = session
        .project
        .menu_mut(menu)
        .and_then(|found| found.buttons.get_mut(index))
    {
        made.name.clone_from(&button.name);
        made.kind = match button.kind {
            disccraft_photo::PsdButtonKind::Normal => disccraft_project::ButtonKind::Normal,
            disccraft_photo::PsdButtonKind::Chapter => disccraft_project::ButtonKind::Chapter,
            disccraft_photo::PsdButtonKind::Next => disccraft_project::ButtonKind::Next,
            disccraft_photo::PsdButtonKind::Previous => disccraft_project::ButtonKind::Previous,
        };
        // The highlight is the set's own layers (or the button's frame), not a text's.
        made.text_subpicture = false;
        made.subpicture = subpicture;
        made.thumbnail = thumbnail;
    }
    Ok((graphic, index))
}

/// Encore's Library layer set or replacement layer placed in a menu (M6.331): its graphics on top of the menu's
/// others, in one step; with `x` and `y` the set moved so its top-left corner is there, kept inside the frame.
fn run_place_layers(session: &mut Session, params: &Value) -> CommandResult {
    let menu = MenuId(req_u64(params, "menu")?);
    let graphics: Vec<MenuGraphic> = serde_json::from_value(
        params
            .get("graphics")
            .cloned()
            .ok_or_else(|| "parameter \"graphics\" is missing".to_string())?,
    )
    .map_err(|error| format!("parameter \"graphics\" must be a list of menu graphics: {error}"))?;
    if graphics.is_empty() {
        return Err("parameter \"graphics\" holds no graphic".to_string());
    }
    let (width, height) = frame_size(&session.project.settings);
    // The set's bounds, and how far it moves.
    let left = graphics
        .iter()
        .map(|graphic| graphic.rect.x)
        .min()
        .unwrap_or(0);
    let top = graphics
        .iter()
        .map(|graphic| graphic.rect.y)
        .min()
        .unwrap_or(0);
    let right = graphics
        .iter()
        .map(|graphic| u32::from(graphic.rect.x) + u32::from(graphic.rect.w))
        .max()
        .unwrap_or(0);
    let bottom = graphics
        .iter()
        .map(|graphic| u32::from(graphic.rect.y) + u32::from(graphic.rect.h))
        .max()
        .unwrap_or(0);
    let (set_w, set_h) = (right - u32::from(left), bottom - u32::from(top));
    let at = |name: &str, own: u16, extent: u32, frame: u16| -> Result<i64, String> {
        let wanted = opt_u64(params, name)?.unwrap_or(u64::from(own));
        // Kept inside the frame (from its far edge when the set is as large).
        let most = u64::from(frame).saturating_sub(u64::from(extent));
        Ok(i64::try_from(wanted.min(most)).unwrap_or(0) - i64::from(own))
    };
    let dx = at("x", left, set_w, width)?;
    let dy = at("y", top, set_h, height)?;
    let shift =
        |start: u16, by: i64| u16::try_from((i64::from(start) + by).max(0)).unwrap_or(u16::MAX);
    let before = session.project.clone();
    let mut placed = Vec::new();
    for mut graphic in graphics {
        graphic.rect.x = shift(graphic.rect.x, dx);
        graphic.rect.y = shift(graphic.rect.y, dy);
        graphic.hidden = false;
        graphic.locked = false;
        match session.project.add_menu_graphic(menu, graphic) {
            Ok(index) => placed.push(Value::from(u64::try_from(index).unwrap_or(u64::MAX))),
            Err(error) => {
                // All or nothing.
                session.project = before;
                return Err(error.to_string());
            }
        }
    }
    Ok(object(&[("graphics", Value::Array(placed))]))
}

/// An image's or a shape's layer effects (Encore's layer styles, M6.332): each value brought inside its range.
fn run_set_effects(session: &mut Session, params: &Value) -> CommandResult {
    let menu = MenuId(req_u64(params, "menu")?);
    let effects = match params.get("effects") {
        None | Some(Value::Null) => disccraft_project::LayerEffects::default(),
        Some(value) => serde_json::from_value::<disccraft_project::LayerEffects>(value.clone())
            .map_err(|error| format!("parameter \"effects\" is not layer effects: {error}"))?,
    }
    .sane();
    let target = (
        opt_u64(params, "graphic")?,
        opt_u64(params, "text")?,
        opt_u64(params, "button")?,
    );
    let found = session
        .project
        .menu_mut(menu)
        .ok_or_else(|| format!("menu {} does not exist", menu.0))?;
    let index = |value: u64| usize::try_from(value).unwrap_or(usize::MAX);
    // An image's or a shape's (M6.332), a text object's or a button's text (M6.342).
    let slot = match target {
        (Some(graphic), None, None) => found
            .graphics
            .get_mut(index(graphic))
            .map(|graphic| &mut graphic.effects)
            .ok_or_else(|| format!("graphic {graphic} does not exist"))?,
        (None, Some(text), None) => found
            .texts
            .get_mut(index(text))
            .map(|text| &mut text.effects)
            .ok_or_else(|| format!("text {text} does not exist"))?,
        (None, None, Some(button)) => found
            .buttons
            .get_mut(index(button))
            .map(|button| &mut button.effects)
            .ok_or_else(|| format!("button {button} does not exist"))?,
        _ => return Err("give exactly one of \"graphic\", \"text\" or \"button\"".to_string()),
    };
    *slot = effects;
    Ok(empty_object())
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
    let replacement = match params.get("replacement") {
        None => None,
        Some(value) => Some(
            value
                .as_bool()
                .ok_or_else(|| "parameter \"replacement\" must be true or false".to_string())?,
        ),
    };

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
    if let Some(replacement) = replacement
        && let Some(graphic) = session
            .project
            .menu_mut(menu)
            .and_then(|menu| menu.graphics.get_mut(index))
    {
        graphic.replacement = replacement;
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
    // The text's layer effects go with it (M6.342).
    let mut effects = disccraft_project::LayerEffects::default();
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
            effects = text.effects;
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
    if let Some(added) = session
        .project
        .menu_mut(menu_id)
        .and_then(|menu| menu.buttons.get_mut(index))
    {
        added.effects = effects;
    }
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
        // The button's layer effects stay with its text (M6.342).
        effects: button.effects,
        box_width: None,
        box_height: None,
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
        hidden: false,
        locked: false,
        vertical: false,
        spans: Vec::new(),
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

    /// M6.405 (Encore's Library ▸ Place of a button, docs/encore-behavior.md 9.18): a button PSD goes in as its
    /// picture with a button over its set: the set's place, its `(=1)` layer the subpicture, its name; centred, or
    /// with its corner where asked; a PSD larger than the frame is stretched into it and leaves the highlight to the
    /// button's frame.
    #[test]
    fn a_library_button_psd_is_placed_with_its_highlight() -> Result<(), Box<dyn Error>> {
        use disccraft_photo::PsdLayer;
        let dir =
            std::env::temp_dir().join(format!("disccraft-place-button-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        // A 400x120 button: art at (20, 20, 360, 80), a highlight line under it.
        let write = |path: &std::path::Path, (w, h): (u32, u32)| -> Result<(), Box<dyn Error>> {
            let fill = |x0: u32, y0: u32, x1: u32, y1: u32| {
                let mut rgba = vec![0u8; (w * h * 4) as usize];
                for y in y0..y1 {
                    for x in x0..x1 {
                        let i = ((y * w + x) * 4) as usize;
                        if let Some(pixel) = rgba.get_mut(i..i + 4) {
                            pixel.copy_from_slice(&[200, 80, 120, 255]);
                        }
                    }
                }
                rgba
            };
            disccraft_photo::write_layered_psd(
                path,
                w,
                h,
                &[PsdLayer::Group {
                    name: "(+) Btn".to_owned(),
                    id: None,
                    children: vec![
                        PsdLayer::Raster {
                            name: "art".to_owned(),
                            rgba: fill(20, 20, 380, 100),
                            visible: true,
                        },
                        PsdLayer::Raster {
                            name: "(=1) hl".to_owned(),
                            rgba: fill(20, 94, 380, 100),
                            visible: false,
                        },
                    ],
                }],
            )?;
            Ok(())
        };
        let small = dir.join("button.psd");
        write(&small, (400, 120))?;
        let mut session = Session::new();
        let menu = session.project.add_menu(None)?;
        let placed = session.execute(
            "menu.placePsdButton",
            &json!({ "menu": menu.0, "path": small.to_string_lossy() }),
        )?;
        let found = session.project.menu(menu).ok_or("menu")?.clone();
        let graphic = found.graphics.last().ok_or("graphic")?;
        // Centred in the 720x480 frame.
        assert_eq!(
            (
                graphic.rect.x,
                graphic.rect.y,
                graphic.rect.w,
                graphic.rect.h
            ),
            (160, 180, 400, 120)
        );
        let index = placed
            .get("button")
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
            .ok_or("button")?;
        let button = found.buttons.get(index).ok_or("button")?;
        assert_eq!(
            (button.rect.x, button.rect.y, button.rect.w, button.rect.h),
            (180, 200, 360, 80)
        );
        assert_eq!(button.name, "Btn");
        assert!(!button.text_subpicture);
        let sub = button.subpicture.as_ref().ok_or("subpicture")?;
        assert_eq!((sub.width, sub.height), (360, 80));

        // With its corner where asked.
        let placed = session.execute(
            "menu.placePsdButton",
            &json!({ "menu": menu.0, "path": small.to_string_lossy(), "x": 10, "y": 20 }),
        )?;
        let index = placed
            .get("button")
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
            .ok_or("button")?;
        let button = session
            .project
            .menu(menu)
            .and_then(|menu| menu.buttons.get(index))
            .ok_or("button")?;
        assert_eq!((button.rect.x, button.rect.y), (30, 40));

        // Larger than the frame: stretched into it, no subpicture of its own.
        let large = dir.join("large.psd");
        write(&large, (1440, 960))?;
        let placed = session.execute(
            "menu.placePsdButton",
            &json!({ "menu": menu.0, "path": large.to_string_lossy() }),
        )?;
        let index = placed
            .get("button")
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
            .ok_or("button")?;
        let found = session.project.menu(menu).ok_or("menu")?;
        let graphic = found.graphics.last().ok_or("graphic")?;
        assert_eq!(
            (
                graphic.rect.x,
                graphic.rect.y,
                graphic.rect.w,
                graphic.rect.h
            ),
            (0, 0, 720, 480)
        );
        let button = found.buttons.get(index).ok_or("button")?;
        assert_eq!(
            (button.rect.x, button.rect.y, button.rect.w, button.rect.h),
            (10, 10, 180, 40)
        );
        assert!(button.subpicture.is_none());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// M6.331: a Library layer set is placed in one step, its layers on top in their order, moved with their
    /// corner where asked and kept inside the frame; a replacement layer stays one; a bad one places nothing.
    #[test]
    fn a_layer_set_is_placed_in_one_step() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu = session.project.add_menu(None)?;
        let settings = session.project.settings.clone();
        let band = disccraft_project::layer_preset("layers.band-top")
            .ok_or("preset")?
            .graphics(&settings);
        let placed = session.execute(
            "menu.placeLayers",
            &json!({ "menu": menu.0, "graphics": band, "x": 100, "y": 300 }),
        )?;
        assert_eq!(placed.get("graphics"), Some(&json!([0, 1])));
        let graphics = |session: &Session| {
            session
                .project
                .menu(menu)
                .map(|menu| menu.graphics.clone())
                .unwrap_or_default()
        };
        let rects: Vec<(u16, u16, u16, u16)> = graphics(&session)
            .iter()
            .map(|graphic| {
                (
                    graphic.rect.x,
                    graphic.rect.y,
                    graphic.rect.w,
                    graphic.rect.h,
                )
            })
            .collect();
        // The band is the whole width: it stays at x 0; its top moves to 300 (the line 70 below it).
        assert_eq!(rects, [(0, 300, 720, 70), (0, 370, 720, 3)]);
        // Undone in one step.
        session.execute("edit.undo", &Value::Null)?;
        assert!(graphics(&session).is_empty());

        let photo = disccraft_project::layer_preset("replace.photo-round")
            .ok_or("preset")?
            .graphics(&settings);
        session.execute(
            "menu.placeLayers",
            &json!({ "menu": menu.0, "graphics": photo }),
        )?;
        let placed = graphics(&session);
        assert_eq!(placed.len(), 1);
        assert!(placed.iter().all(|graphic| graphic.replacement));
        assert!(matches!(
            placed.first().map(|graphic| graphic.kind),
            Some(GraphicKind::Shape {
                shape: ShapeKind::Ellipse,
                ..
            })
        ));
        // An image of an asset the project does not have: nothing is placed.
        let missing = json!([{ "rect": { "x": 0, "y": 0, "w": 10, "h": 10 }, "kind": { "image": { "asset": 99 } } }]);
        assert!(
            session
                .execute(
                    "menu.placeLayers",
                    &json!({ "menu": menu.0, "graphics": missing })
                )
                .is_err()
        );
        assert_eq!(graphics(&session).len(), 1);
        assert!(
            session
                .execute(
                    "menu.placeLayers",
                    &json!({ "menu": menu.0, "graphics": [] })
                )
                .is_err()
        );
        Ok(())
    }

    /// M6.197: the Layers panel's eye and lock set a layer hidden or locked, each on its own; anything else is
    /// refused; it undoes.
    #[test]
    fn layers_are_hidden_and_locked() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu = session.project.add_menu(None)?;
        let _ = session.project.add_button(
            menu,
            "Play".to_owned(),
            None,
            disccraft_project::Link::NotSet,
        )?;
        session.execute(
            "menu.addText",
            &json!({"menu": menu.0, "text": "Hello", "x": 10, "y": 10}),
        )?;
        session.execute(
            "menu.setLayer",
            &json!({"menu": menu.0, "kind": "text", "index": 0, "hidden": true}),
        )?;
        session.execute(
            "menu.setLayer",
            &json!({"menu": menu.0, "kind": "button", "index": 0, "locked": true}),
        )?;
        let state = |session: &Session| {
            session.project.menu(menu).map(|m| {
                (
                    m.texts.first().map(|t| (t.hidden, t.locked)),
                    m.buttons.first().map(|b| (b.hidden, b.locked)),
                )
            })
        };
        assert_eq!(
            state(&session),
            Some((Some((true, false)), Some((false, true))))
        );
        for bad in [
            json!({"menu": menu.0, "kind": "layer", "index": 0, "hidden": true}),
            json!({"menu": menu.0, "kind": "graphic", "index": 0, "hidden": true}),
            json!({"menu": menu.0, "kind": "text", "index": 0, "hidden": "yes"}),
        ] {
            assert!(session.execute("menu.setLayer", &bad).is_err(), "{bad}");
        }
        session.execute("edit.undo", &json!({}))?;
        assert_eq!(
            state(&session),
            Some((Some((true, false)), Some((false, false))))
        );
        Ok(())
    }

    /// M6.283: Object ▸ Convert to Replacement Layer sets a graphic's `(!)` flag; a still that replaces the layer
    /// keeps it, with the layer's opacity; Convert to Object clears it; anything but a bool is refused.
    #[test]
    fn a_graphic_becomes_a_replacement_layer_and_back() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!(
            "disccraft-replacement-layer-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)?;
        let still = dir.join("still.png");
        std::fs::File::create(&still)?;
        let mut session = Session::new();
        let asset = session.project.add_asset(&still)?;
        let menu = session.project.add_menu(None)?;
        session.execute(
            "menu.addShape",
            &json!({ "menu": menu.0, "shape": "rectangle", "color": "#808080", "x": 100, "y": 50, "w": 200, "h": 200, "opacity": 70 }),
        )?;
        let graphic = |session: &Session| {
            session
                .project
                .menu(menu)
                .and_then(|menu| menu.graphics.first())
                .cloned()
        };
        session.execute(
            "menu.setGraphic",
            &json!({ "menu": menu.0, "index": 0, "replacement": true }),
        )?;
        assert!(graphic(&session).is_some_and(|graphic| graphic.replacement));
        // A still dropped on it: its picture and fitted place, the flag and the opacity kept.
        session.execute(
            "menu.setGraphic",
            &json!({ "menu": menu.0, "index": 0, "asset": asset.0, "x": 100, "y": 125, "w": 200, "h": 50 }),
        )?;
        let replaced = graphic(&session).ok_or("graphic")?;
        assert_eq!(replaced.kind, GraphicKind::Image { asset });
        assert!(replaced.replacement);
        assert_eq!(
            (replaced.rect.y, replaced.rect.h, replaced.opacity),
            (125, 50, 70)
        );
        session.execute(
            "menu.setGraphic",
            &json!({ "menu": menu.0, "index": 0, "replacement": false }),
        )?;
        assert!(graphic(&session).is_some_and(|graphic| !graphic.replacement));
        assert!(
            session
                .execute(
                    "menu.setGraphic",
                    &json!({ "menu": menu.0, "index": 0, "replacement": "yes" }),
                )
                .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

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
