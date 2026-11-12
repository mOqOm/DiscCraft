use crate::params::{
    opt_str, opt_u64, opt_value, req_link, req_str, req_u64, req_usize, req_value,
};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{AssetId, Link, MAX_MENU_SECONDS, Menu, MenuBackground, MenuId, Rect};
use serde::Serialize;
use serde_json::{Map, Value};

use super::{always_enabled, empty_object, object};

const COLOR_ERROR: &str = "color must be \"#RRGGBB\"";

/// Menu and button commands.
pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "menu.setAspect",
        label: "メニューの縦横比を設定",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "aspect": "4:3" | "16:9"}"#,
        enabled: always_enabled,
        run: run_set_aspect,
        journal: true,
    },
    CommandSpec {
        id: "menu.applyPsd",
        label: "PSD をメニューに反映",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "path": "menu.psd"}"#,
        enabled: always_enabled,
        run: run_apply_psd,
        journal: true,
    },
    CommandSpec {
        id: "menu.applyMotion",
        label: "モーションをメニューに反映",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "path": "menu.mp4"}"#,
        enabled: always_enabled,
        run: run_apply_motion,
        journal: true,
    },
    CommandSpec {
        id: "menu.setUserOps",
        label: "メニューのユーザー操作を設定",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "prohibited": ["buttonSelect", ...]}"#,
        enabled: always_enabled,
        run: run_set_user_ops,
        journal: true,
    },
    CommandSpec {
        id: "menu.create",
        label: "メニューを作成",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"name"?: string}"#,
        enabled: always_enabled,
        run: run_create,
        journal: true,
    },
    CommandSpec {
        id: "menu.setBackground",
        label: "背景を設定",
        menu: &["メニュー"],
        shortcut: None,
        params: r##"{"menu": id, "color"?: "#RRGGBB", "asset"?: id}"##,
        enabled: always_enabled,
        run: run_set_background,
        journal: true,
    },
    CommandSpec {
        id: "menu.setColors",
        label: "色を設定",
        menu: &["メニュー"],
        shortcut: None,
        params: r##"{"menu": id, "text"?: "#RRGGBB", "highlight"?: "#RRGGBB"}"##,
        enabled: always_enabled,
        run: run_set_colors,
        journal: true,
    },
    CommandSpec {
        id: "menu.addButton",
        label: "ボタンを追加",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "label": string, "target"?: Link, "rect"?: {"x": int, "y": int, "w": int, "h": int}}"#,
        enabled: always_enabled,
        run: run_add_button,
        journal: true,
    },
    CommandSpec {
        id: "menu.removeButton",
        label: "ボタンを削除",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "button": index}"#,
        enabled: always_enabled,
        run: run_remove_button,
        journal: true,
    },
    CommandSpec {
        id: "button.setRect",
        label: "ボタンの位置と大きさを設定",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "button": index, "rect": {"x": int, "y": int, "w": int, "h": int}}"#,
        enabled: always_enabled,
        run: run_set_rect,
        journal: true,
    },
    CommandSpec {
        id: "menu.setDefaultButton",
        label: "既定ボタンを設定",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "button": index}"#,
        enabled: always_enabled,
        run: run_set_default_button,
        journal: true,
    },
    CommandSpec {
        id: "button.setLink",
        label: "リンクを設定",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "button": index, "target": Link}"#,
        enabled: always_enabled,
        run: run_set_link,
        journal: true,
    },
    CommandSpec {
        id: "menu.setOverride",
        label: "メニューの上書きを設定",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "target": Link | null}"#,
        enabled: always_enabled,
        run: run_set_menu_override,
        journal: true,
    },
    CommandSpec {
        id: "button.setStreams",
        label: "ボタンのオーディオ / 字幕を設定",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "button": index, "audio"?: 1..8 | null, "subtitle"?: 1..32 | "off" | null}"#,
        enabled: always_enabled,
        run: run_set_streams,
        journal: true,
    },
    CommandSpec {
        id: "button.setKind",
        label: "ボタンの種類を設定",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "button": index, "kind": "normal" | "chapter" | "next" | "previous" | "main"}"#,
        enabled: always_enabled,
        run: run_set_kind,
        journal: true,
    },
    CommandSpec {
        id: "button.setOverride",
        label: "ボタンの上書きを設定",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "button": index, "target": Link | null}"#,
        enabled: always_enabled,
        run: run_set_override,
        journal: true,
    },
    CommandSpec {
        id: "button.setLabel",
        label: "ラベルを設定",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "button": index, "label": string}"#,
        enabled: always_enabled,
        run: run_set_label,
        journal: true,
    },
    CommandSpec {
        id: "menu.setAudio",
        label: "メニューの音声を設定",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "asset": id | null}"#,
        enabled: always_enabled,
        run: run_set_audio,
        journal: true,
    },
    CommandSpec {
        id: "menu.setEndAction",
        label: "メニューのエンドアクションを設定",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "target": Link}"#,
        enabled: always_enabled,
        run: run_set_end_action,
        journal: true,
    },
    CommandSpec {
        id: "menu.setDuration",
        label: "メニューのデュレーションを設定",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "seconds": 1..254 | null}"#,
        enabled: always_enabled,
        run: run_set_duration,
        journal: true,
    },
    CommandSpec {
        id: "menu.setColorSet",
        label: "メニューのカラーセットを設定",
        menu: &["メニュー"],
        shortcut: None,
        params: r##"{"menu": id, "group": 1..3, "selected"?: "#RRGGBB", "activated"?: "#RRGGBB"}"##,
        enabled: always_enabled,
        run: run_set_color_set,
        journal: true,
    },
    CommandSpec {
        id: "menu.setButtonOffset",
        label: "ボタンの番号のオフセットを設定",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "offset": 0..35}"#,
        enabled: always_enabled,
        run: run_set_button_offset,
        journal: true,
    },
    CommandSpec {
        id: "menu.setAutoRoute",
        label: "ボタンのルートを自動設定",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "on": bool}"#,
        enabled: always_enabled,
        run: run_set_auto_route,
        journal: true,
    },
    CommandSpec {
        id: "button.setRoutes",
        label: "ボタンのルートを設定",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "button": index, "up"?: n, "down"?: n, "left"?: n, "right"?: n}"#,
        enabled: always_enabled,
        run: run_set_routes,
        journal: true,
    },
    CommandSpec {
        id: "button.setName",
        label: "ボタンの名前を設定",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "button": index, "name": string}"#,
        enabled: always_enabled,
        run: run_set_name,
        journal: true,
    },
    CommandSpec {
        id: "button.setNameOptions",
        label: "ボタンの名前の同期を設定",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "button": index, "syncName"?: bool, "nameFromLink"?: bool}"#,
        enabled: always_enabled,
        run: run_set_name_options,
        journal: true,
    },
    CommandSpec {
        id: "button.setHighlightGroup",
        label: "ボタンのハイライトグループを設定",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "button": index, "group": 1..3}"#,
        enabled: always_enabled,
        run: run_set_highlight_group,
        journal: true,
    },
    CommandSpec {
        id: "button.setAutoActivate",
        label: "自動アクティベートを設定",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "button": index, "on": bool}"#,
        enabled: always_enabled,
        run: run_set_auto_activate,
        journal: true,
    },
    CommandSpec {
        id: "menu.list",
        label: "メニュー一覧",
        menu: &["メニュー"],
        shortcut: None,
        params: "{}",
        enabled: always_enabled,
        run: run_list,
        journal: false,
    },
];

fn run_create(session: &mut Session, params: &Value) -> CommandResult {
    let name = opt_str(params, "name")?;
    let id = session
        .project
        .add_menu(name)
        .map_err(|error| error.to_string())?;

    Ok(object(&[("menu", Value::from(id.0))]))
}

fn run_set_background(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let color = opt_str(params, "color")?;
    let asset = opt_u64(params, "asset")?;
    // A video asset makes a motion menu: {"menu", "video": id, "seconds"?: 1..=30 (default 10)}.
    let background = match opt_u64(params, "video")? {
        Some(video) if color.is_none() && asset.is_none() => {
            let seconds = u32::try_from(opt_u64(params, "seconds")?.unwrap_or(10))
                .map_err(|_| "parameter \"seconds\" is too large".to_string())?;
            MenuBackground::Video {
                asset: AssetId(video),
                seconds,
            }
        }
        Some(_) => {
            return Err("\"video\" cannot be combined with \"color\" or \"asset\"".to_string());
        }
        None => select_background(color, asset)?,
    };

    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };

    menu.background = background;
    Ok(empty_object())
}

fn select_background(color: Option<String>, asset: Option<u64>) -> Result<MenuBackground, String> {
    match (color, asset) {
        (Some(color), None) => Ok(MenuBackground::Color {
            color: parse_color(&color)?,
        }),
        (None, Some(asset)) => Ok(MenuBackground::Image {
            asset: AssetId(asset),
        }),
        (Some(_), Some(_)) => {
            Err("parameters \"color\" and \"asset\" are mutually exclusive".to_string())
        }
        (None, None) => Err("missing parameter \"color\" or \"asset\"".to_string()),
    }
}

fn run_set_colors(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let text = opt_str(params, "text")?
        .map(|value| parse_color(&value))
        .transpose()?;
    let highlight = opt_str(params, "highlight")?
        .map(|value| parse_color(&value))
        .transpose()?;

    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };

    if let Some(color) = text {
        menu.text_color = color;
    }
    if let Some(color) = highlight {
        menu.highlight_color = color;
        // The highlight colour is group 1's selected colour.
        if let Some(set) = menu.color_set.as_mut() {
            set.groups[0].selected = color;
        }
    }

    Ok(empty_object())
}

/// One highlight group of the menu's colour set (Encore's Color Set): the selected and / or activated colour.
fn run_set_color_set(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let group = req_u64(params, "group")?;
    if !(1..=3).contains(&group) {
        return Err("parameter \"group\" must be 1, 2 or 3".to_string());
    }
    let selected = opt_str(params, "selected")?
        .map(|value| parse_color(&value))
        .transpose()?;
    let activated = opt_str(params, "activated")?
        .map(|value| parse_color(&value))
        .transpose()?;
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let mut set = menu.color_set_or_default();
    // `group` is 1..=3, so the index is in range.
    let index = usize::try_from(group.saturating_sub(1)).unwrap_or(0).min(2);
    if let Some(slot) = set.groups.get_mut(index) {
        if let Some(color) = selected {
            slot.selected = color;
        }
        if let Some(color) = activated {
            slot.activated = color;
        }
    }
    if index == 0
        && let Some(color) = selected
    {
        menu.highlight_color = color;
    }
    menu.color_set = Some(set);
    Ok(empty_object())
}

/// What a button is for (Encore's button 種類).
fn run_set_kind(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let button = req_usize(params, "button")?;
    let kind =
        disccraft_project::ButtonKind::parse(&req_str(params, "kind")?).ok_or_else(|| {
            "parameter \"kind\" must be \"normal\", \"chapter\", \"next\", \"previous\" or \"main\""
                .to_string()
        })?;
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let Some(target) = menu.buttons.get_mut(button) else {
        return Err(format!("button {button} does not exist"));
    };
    target.kind = kind;
    Ok(empty_object())
}

/// Which highlight group a button uses.
fn run_set_highlight_group(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let button = req_usize(params, "button")?;
    let group = req_u64(params, "group")?;
    let group = u8::try_from(group)
        .ok()
        .filter(|group| (1..=3).contains(group))
        .ok_or_else(|| "parameter \"group\" must be 1, 2 or 3".to_string())?;
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let Some(target) = menu.buttons.get_mut(button) else {
        return Err(format!("button index {button} is out of range"));
    };
    target.highlight_group = group;
    Ok(empty_object())
}

fn run_add_button(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let label = req_str(params, "label")?;
    let link = opt_link(params, "target")?;
    let rect = opt_rect(params, "rect")?;

    let index = session
        .project
        .add_button(menu_id, label, rect, link)
        .map_err(|error| error.to_string())?;

    let button = u64::try_from(index).map_err(|_| "button index is too large".to_string())?;
    Ok(object(&[("button", Value::from(button))]))
}

fn run_remove_button(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let button = req_usize(params, "button")?;

    session
        .project
        .remove_button(menu_id, button)
        .map_err(|error| error.to_string())?;

    Ok(empty_object())
}

fn run_set_rect(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let button = req_usize(params, "button")?;
    let rect = req_rect(params, "rect")?;

    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };

    let Some(target) = menu.buttons.get_mut(button) else {
        return Err(format!("button index {button} is out of range"));
    };

    target.rect = rect;
    Ok(empty_object())
}

fn run_set_default_button(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let button = req_u64(params, "button")?;
    let button = u32::try_from(button).map_err(|_| "button index is too large".to_string())?;

    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };

    let len = u32::try_from(menu.buttons.len()).unwrap_or(u32::MAX);
    if button >= len {
        return Err(format!("button index {button} is out of range"));
    }

    menu.default_button = button;
    Ok(empty_object())
}

fn run_set_link(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let button = req_usize(params, "button")?;
    let index = button;
    let target = req_link(params, "target")?;

    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };

    let Some(button) = menu.buttons.get_mut(button) else {
        return Err(format!("button index {button} is out of range"));
    };

    button.link = target;
    // Encore's "Set Name from Link".
    if button.name_from_link {
        let sync = button.sync_name;
        let link = button.link.clone();
        if let Some(name) = Menu::link_target_name(&session.project, &link)
            && let Some(button) = session
                .project
                .menu_mut(menu_id)
                .and_then(|menu| menu.buttons.get_mut(index))
        {
            button.name = name.clone();
            if sync {
                button.label = name;
            }
        }
    }
    Ok(empty_object())
}

/// A menu's override: where to go after the title its end action plays. `null` or no target clears it.
fn run_set_menu_override(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let target = match params.get("target") {
        None | Some(Value::Null) => None,
        Some(_) => Some(req_link(params, "target")?).filter(|link| !matches!(link, Link::NotSet)),
    };
    if let Some(link) = &target
        && !session.project.link_resolves(link)
    {
        return Err("the override target does not exist".to_string());
    }
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.override_link = target;
    Ok(empty_object())
}

fn run_set_override(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let button = req_usize(params, "button")?;
    let target = match params.get("target") {
        None | Some(Value::Null) => None,
        Some(_) => Some(req_link(params, "target")?).filter(|link| !matches!(link, Link::NotSet)),
    };
    if let Some(link) = &target
        && !session.project.link_resolves(link)
    {
        return Err("the override target does not exist".to_string());
    }
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let Some(button) = menu.buttons.get_mut(button) else {
        return Err(format!("button index {button} is out of range"));
    };
    button.override_link = target;
    Ok(empty_object())
}

/// The audio and subtitle a button selects before it jumps: each key that is present is set (`null` clears it).
fn run_set_streams(session: &mut Session, params: &Value) -> CommandResult {
    use disccraft_project::SubtitleDefault;
    let menu_id = MenuId(req_u64(params, "menu")?);
    let index = req_usize(params, "button")?;
    let audio = match params.get("audio") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(value) => Some(Some(
            value
                .as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .filter(|n| (1..=8).contains(n))
                .ok_or_else(|| "parameter \"audio\" must be 1 to 8 or null".to_string())?,
        )),
    };
    let subtitle = match params.get("subtitle") {
        None => None,
        Some(Value::Null) => Some(SubtitleDefault::NoChange),
        Some(Value::String(text)) if text == "off" => Some(SubtitleDefault::Off),
        Some(value) => Some(SubtitleDefault::Stream(
            value
                .as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .filter(|n| (1..=32).contains(n))
                .ok_or_else(|| {
                    "parameter \"subtitle\" must be 1 to 32, \"off\" or null".to_string()
                })?,
        )),
    };
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let Some(button) = menu.buttons.get_mut(index) else {
        return Err(format!("button index {index} is out of range"));
    };
    if let Some(audio) = audio {
        button.audio = audio;
    }
    if let Some(subtitle) = subtitle {
        button.subtitle = subtitle;
    }
    Ok(empty_object())
}

fn run_set_label(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let button = req_usize(params, "button")?;
    let label = req_str(params, "label")?;

    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };

    let Some(button) = menu.buttons.get_mut(button) else {
        return Err(format!("button index {button} is out of range"));
    };

    // Encore's "Sync Button Text and Name".
    if button.sync_name && !label.trim().is_empty() {
        button.name = label.clone();
    }
    button.label = label;
    Ok(empty_object())
}

/// Encore's menu "Offset": the first button's number on the remote's number keys, minus one. The offset plus
/// the button count must fit the 36 numbers of a DVD menu.
fn run_set_button_offset(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let offset = req_u64(params, "offset")?;
    let offset = u8::try_from(offset)
        .ok()
        .filter(|offset| *offset <= disccraft_project::MAX_BUTTON_OFFSET)
        .ok_or_else(|| {
            format!(
                "parameter \"offset\" must be between 0 and {}",
                disccraft_project::MAX_BUTTON_OFFSET
            )
        })?;
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    if usize::from(offset).saturating_add(menu.buttons.len()) > 36 {
        return Err("the offset plus the number of buttons must not exceed 36".to_string());
    }
    menu.button_offset = offset;
    Ok(empty_object())
}

/// Encore's "Automatically Route Buttons".
fn run_set_auto_route(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let on = params
        .get("on")
        .and_then(Value::as_bool)
        .ok_or_else(|| "parameter \"on\" must be true or false".to_string())?;
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.auto_route = on;
    Ok(empty_object())
}

/// A button's own routing (one-based button numbers, 0 = nowhere). A direction left out keeps its value; the
/// starting point is the button's routes, or the automatic ones.
fn run_set_routes(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let index = req_usize(params, "button")?;
    let Some(menu) = session.project.menu(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let count = menu.buttons.len();
    if index >= count {
        return Err(format!("button index {index} is out of range"));
    }
    let direction = |name: &str| -> Result<Option<u8>, String> {
        match params.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value
                .as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .filter(|n| usize::from(*n) <= count)
                .map(Some)
                .ok_or_else(|| format!("parameter \"{name}\" must be 0 or a button number")),
        }
    };
    let (up, down, left, right) = (
        direction("up")?,
        direction("down")?,
        direction("left")?,
        direction("right")?,
    );
    let current = disccraft_menu::button_routes(menu, &session.project.settings.routing)
        .get(index)
        .copied()
        .unwrap_or((0, 0, 0, 0));
    let routes = [
        up.unwrap_or(current.0),
        down.unwrap_or(current.1),
        left.unwrap_or(current.2),
        right.unwrap_or(current.3),
    ];
    let Some(button) = session
        .project
        .menu_mut(menu_id)
        .and_then(|menu| menu.buttons.get_mut(index))
    else {
        return Err(format!("button index {index} is out of range"));
    };
    button.routes = Some(routes);
    Ok(empty_object())
}

/// A button's name (the layer set name in Encore). With "Sync Button Text and Name" on, the text follows.
fn run_set_name(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let button = req_usize(params, "button")?;
    let name = req_str(params, "name")?;
    if name.trim().is_empty() {
        return Err("parameter \"name\" must not be empty".to_string());
    }
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let Some(target) = menu.buttons.get_mut(button) else {
        return Err(format!("button index {button} is out of range"));
    };
    if target.sync_name {
        target.label = name.clone();
    }
    target.name = name;
    Ok(empty_object())
}

/// Encore's "Sync Button Text and Name" / "Set Name from Link" switches; turning one on applies it at once.
fn run_set_name_options(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let index = req_usize(params, "button")?;
    let flag = |name: &str| -> Result<Option<bool>, String> {
        match params.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(flag)) => Ok(Some(*flag)),
            Some(_) => Err(format!("parameter \"{name}\" must be a boolean")),
        }
    };
    let sync = flag("syncName")?;
    let from_link = flag("nameFromLink")?;
    let link_name = {
        let Some(button) = session
            .project
            .menu(menu_id)
            .and_then(|menu| menu.buttons.get(index))
        else {
            return Err(format!("button index {index} is out of range"));
        };
        Menu::link_target_name(&session.project, &button.link)
    };
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let Some(button) = menu.buttons.get_mut(index) else {
        return Err(format!("button index {index} is out of range"));
    };
    if let Some(sync) = sync {
        button.sync_name = sync;
        if sync && !button.label.trim().is_empty() {
            button.name = button.label.clone();
        }
    }
    if let Some(from_link) = from_link {
        button.name_from_link = from_link;
        if from_link && let Some(name) = link_name {
            button.name = name.clone();
            if button.sync_name {
                button.label = name;
            }
        }
    }
    Ok(empty_object())
}

/// Whether highlighting the button already activates it.
fn run_set_auto_activate(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let button = req_usize(params, "button")?;
    let on = params
        .get("on")
        .and_then(Value::as_bool)
        .ok_or_else(|| "parameter \"on\" must be true or false".to_string())?;
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let Some(target) = menu.buttons.get_mut(button) else {
        return Err(format!("button index {button} is out of range"));
    };
    target.auto_activate = on;
    Ok(empty_object())
}

/// The audio played (and looped) with a menu; `null` removes it.
fn run_set_audio(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let audio = opt_u64(params, "asset")?.map(AssetId);
    if let Some(asset) = audio {
        let is_audio = session
            .project
            .assets
            .get(&asset)
            .is_some_and(|asset| asset.kind == disccraft_project::AssetKind::Audio);
        if !is_audio {
            return Err(format!("asset {} is not an audio asset", asset.0));
        }
    }
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.audio = audio;
    Ok(empty_object())
}

/// Where a menu goes when its duration is over (still) or it has played once (motion).
fn run_set_end_action(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let target = req_link(params, "target")?;
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.end_action = target;
    Ok(empty_object())
}

/// How long a still menu stays; `null` holds it until a button is pressed.
fn run_set_duration(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let seconds = match opt_u64(params, "seconds")? {
        None => None,
        Some(seconds) => {
            let seconds = u32::try_from(seconds)
                .ok()
                .filter(|seconds| (1..=MAX_MENU_SECONDS).contains(seconds))
                .ok_or_else(|| format!("\"seconds\" must be 1 to {MAX_MENU_SECONDS}"))?;
            Some(seconds)
        }
    };
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.duration_seconds = seconds;
    Ok(empty_object())
}

/// The menu's aspect ratio (a DVD shows every menu at the first menu's).
fn run_set_aspect(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let wide = match req_str(params, "aspect")?.as_str() {
        "4:3" => false,
        "16:9" => true,
        _ => return Err("parameter \"aspect\" must be \"4:3\" or \"16:9\"".to_string()),
    };
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.widescreen = wide;
    Ok(empty_object())
}

/// Make a menu follow a PSD edited outside (Encore's "Edit Menu in Photoshop"): the PSD becomes the background, the
/// `(+)` groups become the buttons (in order: the menu's buttons keep their links and get the new rectangles, new
/// groups add buttons, buttons without a group are removed), and the button texts and the text objects are cleared,
/// as they are in the picture now.
fn run_apply_psd(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let path = req_str(params, "path")?;
    if session.project.menu(menu_id).is_none() {
        return Err(format!("menu {} does not exist", menu_id.0));
    }
    let psd = disccraft_photo::load_psd_menu(std::path::Path::new(&path))
        .map_err(|error| error.to_string())?;
    let (frame_w, frame_h) = disccraft_project::frame_size(&session.project.settings);
    let rects: Vec<(String, Rect)> = psd
        .buttons
        .iter()
        .map(|button| {
            let scale = |value: u32, from: u32, to: u16| -> u16 {
                let scaled = u64::from(value) * u64::from(to) / u64::from(from.max(1));
                u16::try_from(scaled).unwrap_or(u16::MAX)
            };
            let x = scale(button.x, psd.width, frame_w).min(frame_w.saturating_sub(8));
            let y = scale(button.y, psd.height, frame_h).min(frame_h.saturating_sub(8));
            let w = scale(button.width, psd.width, frame_w)
                .max(8)
                .min(frame_w.saturating_sub(x));
            let h = scale(button.height, psd.height, frame_h)
                .max(8)
                .min(frame_h.saturating_sub(y));
            (button.name.clone(), Rect { x, y, w, h })
        })
        .collect();

    let asset = match session
        .project
        .assets
        .values()
        .find(|asset| std::path::Path::new(&asset.path) == std::path::Path::new(&path))
    {
        Some(asset) => asset.id,
        None => session
            .project
            .add_asset(std::path::Path::new(&path))
            .map_err(|error| error.to_string())?,
    };

    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.background = MenuBackground::Image { asset };
    menu.texts.clear();
    for (index, (name, rect)) in rects.iter().enumerate() {
        match menu.buttons.get_mut(index) {
            Some(button) => {
                button.rect = *rect;
                button.label.clear();
                if !name.is_empty() {
                    button.name.clone_from(name);
                }
            }
            None => menu.buttons.push(disccraft_project::Button {
                name: name.clone(),
                label: String::new(),
                rect: *rect,
                link: Link::NotSet,
                auto_activate: false,
                override_link: None,
                highlight_group: 1,
                sync_name: true,
                name_from_link: false,
                routes: None,
                audio: None,
                subtitle: disccraft_project::SubtitleDefault::NoChange,
                kind: Default::default(),
                shadow: None,
                orientation: Default::default(),
            }),
        }
    }
    let extra: Vec<usize> = (rects.len()..menu.buttons.len()).collect();
    if !extra.is_empty() {
        session
            .project
            .remove_buttons(menu_id, &extra)
            .map_err(|error| error.to_string())?;
    }
    Ok(object(&[
        ("asset", Value::from(asset.0)),
        ("buttons", Value::from(rects.len())),
    ]))
}

/// Make a movie rendered from the menu (EffectCraft) the menu's motion background: the movie becomes a video
/// asset (the same path reuses it) looped for its length (1 to 30 seconds), and the button texts and text objects
/// are cleared, as they are in the movie.
fn run_apply_motion(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let path = req_str(params, "path")?;
    if session.project.menu(menu_id).is_none() {
        return Err(format!("menu {} does not exist", menu_id.0));
    }
    let probe = disccraft_transcode::probe_video(std::path::Path::new(&path))
        .map_err(|error| format!("{path}: {error}"))?;
    // Whole seconds, at least 1 and at most the longest loop; `as` saturates.
    let seconds = (probe.seconds.round().max(1.0) as u32).clamp(1, 30);
    let asset = match session
        .project
        .assets
        .values()
        .find(|asset| std::path::Path::new(&asset.path) == std::path::Path::new(&path))
    {
        Some(asset) => asset.id,
        None => session
            .project
            .add_asset(std::path::Path::new(&path))
            .map_err(|error| error.to_string())?,
    };
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.background = MenuBackground::Video { asset, seconds };
    menu.texts.clear();
    for button in &mut menu.buttons {
        button.label.clear();
    }
    Ok(object(&[
        ("asset", Value::from(asset.0)),
        ("seconds", Value::from(seconds)),
    ]))
}

/// Set the remote operations a menu prohibits (an empty list allows everything).
fn run_set_user_ops(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let ops = super::user_ops_param(params)?;
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.prohibited_ops = ops;
    Ok(empty_object())
}

fn run_list(session: &mut Session, _params: &Value) -> CommandResult {
    let mut menus = Vec::new();

    for menu in session.project.menus.values() {
        let mut buttons = Vec::new();

        for button in &menu.buttons {
            let mut map = Map::new();
            map.insert("label".to_string(), Value::String(button.label.clone()));
            map.insert("rect".to_string(), to_json_value(&button.rect)?);
            map.insert("link".to_string(), to_json_value(&button.link)?);
            buttons.push(Value::Object(map));
        }

        let mut map = Map::new();
        map.insert("id".to_string(), Value::from(menu.id.0));
        map.insert("name".to_string(), Value::String(menu.name.clone()));
        map.insert("buttons".to_string(), Value::Array(buttons));
        map.insert(
            "defaultButton".to_string(),
            Value::from(u64::from(menu.default_button)),
        );
        menus.push(Value::Object(map));
    }

    Ok(Value::Array(menus))
}

fn to_json_value<T: Serialize>(value: &T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

fn opt_link(params: &Value, name: &str) -> Result<Link, String> {
    match opt_value(params, name) {
        Some(value) => serde_json::from_value::<Link>(value.clone())
            .map_err(|error| format!("parameter \"{name}\" must be a valid link: {error}")),
        None => Ok(Link::NotSet),
    }
}

fn opt_rect(params: &Value, name: &str) -> Result<Option<Rect>, String> {
    match opt_value(params, name) {
        Some(value) => serde_json::from_value::<Rect>(value.clone())
            .map(Some)
            .map_err(|error| format!("parameter \"{name}\" must be a rect: {error}")),
        None => Ok(None),
    }
}

fn req_rect(params: &Value, name: &str) -> Result<Rect, String> {
    let value = req_value(params, name)?;
    serde_json::from_value::<Rect>(value.clone())
        .map_err(|error| format!("parameter \"{name}\" must be a rect: {error}"))
}

pub(crate) fn parse_color(text: &str) -> Result<[u8; 3], String> {
    let mut chars = text.chars();
    if chars.next() != Some('#') {
        return Err(COLOR_ERROR.to_string());
    }

    let mut color = [0u8; 3];
    for channel in color.iter_mut() {
        let hi = chars
            .next()
            .and_then(hex_value)
            .ok_or_else(|| COLOR_ERROR.to_string())?;
        let lo = chars
            .next()
            .and_then(hex_value)
            .ok_or_else(|| COLOR_ERROR.to_string())?;
        *channel = hi.saturating_mul(16).saturating_add(lo);
    }

    if chars.next().is_some() {
        return Err(COLOR_ERROR.to_string());
    }

    Ok(color)
}

fn hex_value(ch: char) -> Option<u8> {
    let value = ch.to_digit(16)?;
    u8::try_from(value).ok()
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use disccraft_project::{Link, MenuId};
    use serde_json::{Map, Value};
    use std::error::Error;

    fn obj(entries: &[(&str, Value)]) -> Value {
        let mut map = Map::new();
        for (key, value) in entries {
            map.insert((*key).to_string(), (*value).clone());
        }
        Value::Object(map)
    }

    fn value_u64(value: &Value, key: &str) -> Result<u64, Box<dyn Error>> {
        value
            .get(key)
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("missing {key}").into())
    }

    #[test]
    fn a_rendered_movie_becomes_the_motion_background() -> Result<(), Box<dyn Error>> {
        let movie = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("fixtures")
            .join("h264_aac.mp4");
        if !movie.is_file() {
            println!("skipped: no fixture");
            return Ok(());
        }
        let mut session = Session::new();
        let menu_id = value_u64(&session.execute("menu.create", &obj(&[]))?, "menu")?;
        session.execute(
            "menu.addButton",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("label", Value::String("Play".to_string())),
            ]),
        )?;
        let path = movie.to_string_lossy().into_owned();
        let applied = session.execute(
            "menu.applyMotion",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("path", Value::String(path)),
            ]),
        )?;
        let menu = session
            .project
            .menu(MenuId(menu_id))
            .ok_or("missing menu")?;
        assert!(matches!(
            menu.background,
            disccraft_project::MenuBackground::Video { seconds: 6, .. }
        ));
        assert_eq!(value_u64(&applied, "seconds")?, 6);
        assert!(menu.buttons.iter().all(|button| button.label.is_empty()));
        assert!(
            session
                .execute(
                    "menu.applyMotion",
                    &obj(&[
                        ("menu", Value::from(menu_id)),
                        ("path", Value::String("missing.mp4".to_owned())),
                    ]),
                )
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn a_menu_has_its_own_aspect() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let first = value_u64(&session.execute("menu.create", &obj(&[]))?, "menu")?;
        session.execute(
            "menu.setAspect",
            &obj(&[
                ("menu", Value::from(first)),
                ("aspect", Value::from("16:9")),
            ]),
        )?;
        assert!(session.project.menus_wide());
        assert!(
            session
                .execute(
                    "menu.setAspect",
                    &obj(&[
                        ("menu", Value::from(first)),
                        ("aspect", Value::from("wide"))
                    ]),
                )
                .is_err()
        );
        // A second 4:3 menu is a warning, not an error.
        let second = value_u64(&session.execute("menu.create", &obj(&[]))?, "menu")?;
        assert!(
            !session
                .project
                .menu(MenuId(second))
                .ok_or("menu")?
                .widescreen
        );
        assert!(
            session
                .project
                .check()
                .iter()
                .any(|problem| problem.code == "menuAspectMix")
        );
        session.undo()?;
        session.undo()?;
        assert!(!session.project.menus_wide());
        Ok(())
    }

    #[test]
    fn menu_overrides_are_set_and_undone() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu_id = value_u64(&session.execute("menu.create", &obj(&[]))?, "menu")?;
        let set = |target: Value| obj(&[("menu", Value::from(menu_id)), ("target", target)]);
        session.execute("menu.setOverride", &set(Value::String("stop".to_owned())))?;
        let current = |session: &Session| {
            session
                .project
                .menu(MenuId(menu_id))
                .and_then(|menu| menu.override_link.clone())
        };
        assert_eq!(current(&session), Some(Link::Stop));
        assert!(
            session
                .execute("menu.setOverride", &set(obj(&[("menu", Value::from(999))])))
                .is_err()
        );
        session.execute("menu.setOverride", &set(Value::Null))?;
        assert_eq!(current(&session), None);
        session.undo()?;
        assert_eq!(current(&session), Some(Link::Stop));
        Ok(())
    }

    #[test]
    fn button_offsets_are_bounded() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu_id = value_u64(&session.execute("menu.create", &obj(&[]))?, "menu")?;
        session.execute(
            "menu.addButton",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("label", Value::String("A".to_string())),
            ]),
        )?;
        session.execute(
            "menu.setButtonOffset",
            &obj(&[("menu", Value::from(menu_id)), ("offset", Value::from(6))]),
        )?;
        assert_eq!(
            session
                .project
                .menu(MenuId(menu_id))
                .map(|menu| menu.button_offset),
            Some(6)
        );
        for offset in [36_u64, 35] {
            // 35 + one button = 36 fits; 36 does not.
            let result = session.execute(
                "menu.setButtonOffset",
                &obj(&[
                    ("menu", Value::from(menu_id)),
                    ("offset", Value::from(offset)),
                ]),
            );
            assert_eq!(result.is_ok(), offset == 35, "{offset}");
        }
        session.undo()?;
        assert_eq!(
            session
                .project
                .menu(MenuId(menu_id))
                .map(|menu| menu.button_offset),
            Some(6)
        );
        Ok(())
    }

    #[test]
    fn routes_are_automatic_until_set_by_hand() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu_id = value_u64(&session.execute("menu.create", &obj(&[]))?, "menu")?;
        for label in ["A", "B", "C"] {
            session.execute(
                "menu.addButton",
                &obj(&[
                    ("menu", Value::from(menu_id)),
                    ("label", Value::String(label.to_string())),
                ]),
            )?;
        }
        let routes = |session: &Session| {
            session
                .project
                .menu(MenuId(menu_id))
                .map(|menu| disccraft_menu::button_routes(menu, &session.project.settings.routing))
                .unwrap_or_default()
        };
        // Three buttons in a column: 1 → down 2, 2 → down 3.
        assert_eq!(routes(&session).first().map(|r| r.1), Some(2));
        session.execute(
            "button.setRoutes",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("button", Value::from(0)),
                ("down", Value::from(3)),
            ]),
        )?;
        // Still automatic while the menu routes by itself.
        assert_eq!(routes(&session).first().map(|r| r.1), Some(2));
        session.execute(
            "menu.setAutoRoute",
            &obj(&[("menu", Value::from(menu_id)), ("on", Value::Bool(false))]),
        )?;
        assert_eq!(routes(&session).first().map(|r| r.1), Some(3));
        // The other directions kept the automatic values; buttons without routes keep theirs.
        assert_eq!(routes(&session).get(1).map(|r| r.1), Some(3));
        assert!(
            session
                .execute(
                    "button.setRoutes",
                    &obj(&[
                        ("menu", Value::from(menu_id)),
                        ("button", Value::from(0)),
                        ("up", Value::from(9)),
                    ]),
                )
                .is_err()
        );
        session.undo()?;
        assert_eq!(routes(&session).first().map(|r| r.1), Some(2));
        Ok(())
    }

    #[test]
    fn button_names_follow_the_text_or_the_link() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu_id = value_u64(
            &session.execute("menu.create", &obj(&[("name", Value::from("Main"))]))?,
            "menu",
        )?;
        let extras = value_u64(
            &session.execute("menu.create", &obj(&[("name", Value::from("Extras"))]))?,
            "menu",
        )?;
        session.execute(
            "menu.addButton",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("label", Value::String("Play".to_string())),
            ]),
        )?;
        let button = |session: &Session| {
            session
                .project
                .menu(MenuId(menu_id))
                .and_then(|menu| menu.buttons.first().cloned())
        };
        // A new button is named after its text; the text keeps the name in step.
        assert_eq!(button(&session).map(|b| b.name), Some("Play".to_string()));
        session.execute(
            "button.setLabel",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("button", Value::from(0)),
                ("label", Value::from("Start")),
            ]),
        )?;
        assert_eq!(button(&session).map(|b| b.name), Some("Start".to_string()));
        // Without the sync the name stays.
        session.execute(
            "button.setNameOptions",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("button", Value::from(0)),
                ("syncName", Value::Bool(false)),
            ]),
        )?;
        session.execute(
            "button.setLabel",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("button", Value::from(0)),
                ("label", Value::from("Go")),
            ]),
        )?;
        assert_eq!(
            button(&session).map(|b| (b.name, b.label)),
            Some(("Start".to_string(), "Go".to_string()))
        );
        // The name from the link: the target's name.
        session.execute(
            "button.setNameOptions",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("button", Value::from(0)),
                ("nameFromLink", Value::Bool(true)),
            ]),
        )?;
        session.execute(
            "button.setLink",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("button", Value::from(0)),
                ("target", obj(&[("menu", Value::from(extras))])),
            ]),
        )?;
        assert_eq!(button(&session).map(|b| b.name), Some("Extras".to_string()));
        session.execute(
            "button.setName",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("button", Value::from(0)),
                ("name", Value::from("Bonus")),
            ]),
        )?;
        assert_eq!(button(&session).map(|b| b.name), Some("Bonus".to_string()));
        session.undo()?;
        assert_eq!(button(&session).map(|b| b.name), Some("Extras".to_string()));
        Ok(())
    }

    #[test]
    fn colour_sets_and_highlight_groups_are_set_and_undone() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu_id = value_u64(&session.execute("menu.create", &obj(&[]))?, "menu")?;
        session.execute(
            "menu.addButton",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("label", Value::String("Play".to_string())),
            ]),
        )?;
        session.execute(
            "menu.setColorSet",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("group", Value::from(2)),
                ("selected", Value::from("#FF0000")),
                ("activated", Value::from("#00FF00")),
            ]),
        )?;
        session.execute(
            "button.setHighlightGroup",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("button", Value::from(0)),
                ("group", Value::from(2)),
            ]),
        )?;
        let menu = session
            .project
            .menu(MenuId(menu_id))
            .ok_or("missing menu")?;
        assert_eq!(menu.highlight_group(2).selected, [255, 0, 0]);
        assert_eq!(menu.highlight_group(2).activated, [0, 255, 0]);
        // Group 1 keeps the highlight colour; group 3 copied it.
        assert_eq!(menu.highlight_group(1).selected, menu.highlight_color);
        assert_eq!(menu.highlight_group(3).selected, menu.highlight_color);
        assert_eq!(
            menu.buttons.first().map(|button| button.highlight_group),
            Some(2)
        );
        assert!(
            session
                .execute(
                    "button.setHighlightGroup",
                    &obj(&[
                        ("menu", Value::from(menu_id)),
                        ("button", Value::from(0)),
                        ("group", Value::from(4)),
                    ]),
                )
                .is_err()
        );
        session.undo()?;
        session.undo()?;
        let menu = session
            .project
            .menu(MenuId(menu_id))
            .ok_or("missing menu")?;
        assert!(menu.color_set.is_none());
        assert_eq!(
            menu.buttons.first().map(|button| button.highlight_group),
            Some(1)
        );
        Ok(())
    }

    #[test]
    fn button_overrides_are_set_and_undone() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu_id = value_u64(&session.execute("menu.create", &obj(&[]))?, "menu")?;
        session.execute(
            "menu.addButton",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("label", Value::String("Play".to_string())),
            ]),
        )?;
        let set = |target: Value| {
            obj(&[
                ("menu", Value::from(menu_id)),
                ("button", Value::from(0)),
                ("target", target),
            ])
        };
        session.execute("button.setOverride", &set(Value::String("stop".to_owned())))?;
        let current = |session: &Session| {
            session
                .project
                .menu(MenuId(menu_id))
                .and_then(|menu| menu.buttons.first())
                .and_then(|button| button.override_link.clone())
        };
        assert_eq!(current(&session), Some(Link::Stop));
        assert!(
            session
                .execute(
                    "button.setOverride",
                    &set(obj(&[("menu", Value::from(999))]))
                )
                .is_err()
        );
        session.execute("button.setOverride", &set(Value::Null))?;
        assert_eq!(current(&session), None);
        session.undo()?;
        assert_eq!(current(&session), Some(Link::Stop));
        Ok(())
    }

    #[test]
    fn an_edited_psd_becomes_the_menu() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-apply-psd-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let mut session = Session::new();
        let menu_id = value_u64(&session.execute("menu.create", &obj(&[]))?, "menu")?;
        for label in ["Play", "Extras"] {
            session.execute(
                "menu.addButton",
                &obj(&[
                    ("menu", Value::from(menu_id)),
                    ("label", Value::String(label.to_string())),
                ]),
            )?;
        }
        session.execute(
            "menu.addText",
            &serde_json::json!({"menu": menu_id, "text": "Title", "x": 10, "y": 10}),
        )?;

        // The edited PSD has one button, moved to x 100..300, y 200..260.
        let (w, h) = (720_usize, 480_usize);
        let mut label = vec![0u8; w * h * 4];
        for y in 200..260 {
            for x in 100..300 {
                let i = (y * w + x) * 4;
                if let Some(pixel) = label.get_mut(i..i + 4) {
                    pixel.copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
        let path = dir.join("menu.psd");
        disccraft_photo::write_layered_psd(
            &path,
            720,
            480,
            &[
                disccraft_photo::PsdLayer::Raster {
                    name: "背景".to_owned(),
                    rgba: [0, 0, 80, 255].repeat(w * h),
                    visible: true,
                },
                disccraft_photo::PsdLayer::Group {
                    name: "(+) Play".to_owned(),
                    children: vec![disccraft_photo::PsdLayer::Raster {
                        name: "Play".to_owned(),
                        rgba: label,
                        visible: true,
                    }],
                },
            ],
        )?;
        let path_text = path.to_string_lossy().into_owned();
        session.execute(
            "menu.applyPsd",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("path", Value::String(path_text.clone())),
            ]),
        )?;

        let menu = session
            .project
            .menu(MenuId(menu_id))
            .ok_or("missing menu")?;
        assert_eq!(menu.buttons.len(), 1);
        let button = menu.buttons.first().ok_or("no button")?;
        assert_eq!(
            (button.rect.x, button.rect.y, button.rect.w, button.rect.h),
            (100, 200, 200, 60)
        );
        assert!(button.label.is_empty());
        assert!(menu.texts.is_empty());
        let asset = match &menu.background {
            disccraft_project::MenuBackground::Image { asset } => *asset,
            other => return Err(format!("background {other:?}").into()),
        };
        assert_eq!(
            session
                .project
                .assets
                .get(&asset)
                .map(|asset| asset.path.clone()),
            Some(path_text.clone())
        );
        // Applying again reuses the asset.
        session.execute(
            "menu.applyPsd",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("path", Value::String(path_text)),
            ]),
        )?;
        assert_eq!(session.project.assets.len(), 1);

        // Applying the same PSD again changed nothing, so one undo goes back to before the edit.
        session.undo()?;
        let menu = session
            .project
            .menu(MenuId(menu_id))
            .ok_or("missing menu")?;
        assert_eq!(menu.buttons.len(), 2);
        assert_eq!(menu.texts.len(), 1);
        assert!(
            session
                .execute(
                    "menu.applyPsd",
                    &obj(&[
                        ("menu", Value::from(menu_id)),
                        ("path", Value::String("missing.psd".to_owned())),
                    ]),
                )
                .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn create_menu_and_auto_buttons() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let created = session.execute("menu.create", &obj(&[]))?;
        let menu_id = value_u64(&created, "menu")?;

        for label in ["Play", "Setup", "Quit"] {
            session.execute(
                "menu.addButton",
                &obj(&[
                    ("menu", Value::from(menu_id)),
                    ("label", Value::String(label.to_string())),
                ]),
            )?;
        }

        let menu = session
            .project
            .menu(MenuId(menu_id))
            .ok_or("missing menu")?;

        assert_eq!(menu.buttons.len(), 3);

        let y_values: Vec<u16> = menu.buttons.iter().map(|button| button.rect.y).collect();
        assert_eq!(y_values, vec![120, 184, 248]);

        let x_values: Vec<u16> = menu.buttons.iter().map(|button| button.rect.x).collect();
        assert_eq!(x_values, vec![160, 160, 160]);

        Ok(())
    }

    #[test]
    fn set_link_missing_timeline_rolls_back() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let created = session.execute("menu.create", &obj(&[]))?;
        let menu_id = value_u64(&created, "menu")?;

        session.execute(
            "menu.addButton",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("label", Value::String("Play".to_string())),
                ("target", Value::String("stop".to_string())),
            ]),
        )?;

        let result = session.execute(
            "button.setLink",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("button", Value::from(0)),
                (
                    "target",
                    obj(&[("timeline", Value::from(999)), ("chapter", Value::from(0))]),
                ),
            ]),
        );

        assert!(result.is_err());

        let menu = session
            .project
            .menu(MenuId(menu_id))
            .ok_or("missing menu")?;
        assert_eq!(
            menu.buttons.first().ok_or("missing button")?.link,
            Link::Stop
        );

        Ok(())
    }

    #[test]
    fn set_background_requires_one_parameter() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let created = session.execute("menu.create", &obj(&[]))?;
        let menu_id = value_u64(&created, "menu")?;

        let result = session.execute(
            "menu.setBackground",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("color", Value::String("#ff0000".to_string())),
                ("asset", Value::from(1)),
            ]),
        );

        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn remove_button_fixes_default_button() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let created = session.execute("menu.create", &obj(&[]))?;
        let menu_id = value_u64(&created, "menu")?;

        for label in ["A", "B", "C"] {
            session.execute(
                "menu.addButton",
                &obj(&[
                    ("menu", Value::from(menu_id)),
                    ("label", Value::String(label.to_string())),
                ]),
            )?;
        }

        session.execute(
            "menu.setDefaultButton",
            &obj(&[("menu", Value::from(menu_id)), ("button", Value::from(2))]),
        )?;

        session.execute(
            "menu.removeButton",
            &obj(&[("menu", Value::from(menu_id)), ("button", Value::from(2))]),
        )?;

        let menu = session
            .project
            .menu(MenuId(menu_id))
            .ok_or("missing menu")?;
        assert_eq!(menu.default_button, 0);

        Ok(())
    }

    #[test]
    fn set_button_rect() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let created = session.execute("menu.create", &obj(&[]))?;
        let menu_id = value_u64(&created, "menu")?;

        session.execute(
            "menu.addButton",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("label", Value::String("Play".to_string())),
            ]),
        )?;

        session.execute(
            "button.setRect",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("button", Value::from(0)),
                (
                    "rect",
                    obj(&[
                        ("x", Value::from(64)),
                        ("y", Value::from(200)),
                        ("w", Value::from(300)),
                        ("h", Value::from(60)),
                    ]),
                ),
            ]),
        )?;

        let menu = session
            .project
            .menu(MenuId(menu_id))
            .ok_or("missing menu")?;
        let rect = menu.buttons.first().ok_or("missing button")?.rect;

        assert_eq!((rect.x, rect.y, rect.w, rect.h), (64, 200, 300, 60));

        // 枠外は check が検出し、コマンドはロールバックされる。
        let out_of_frame = session.execute(
            "button.setRect",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("button", Value::from(0)),
                (
                    "rect",
                    obj(&[
                        ("x", Value::from(700)),
                        ("y", Value::from(0)),
                        ("w", Value::from(300)),
                        ("h", Value::from(60)),
                    ]),
                ),
            ]),
        );
        assert!(out_of_frame.is_err());

        let menu = session
            .project
            .menu(MenuId(menu_id))
            .ok_or("missing menu")?;
        assert_eq!(menu.buttons.first().ok_or("missing button")?.rect, rect);

        Ok(())
    }

    #[test]
    fn menu_list_output() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let created = session.execute("menu.create", &obj(&[]))?;
        let menu_id = value_u64(&created, "menu")?;

        session.execute(
            "menu.addButton",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("label", Value::String("Play".to_string())),
                ("target", Value::String("stop".to_string())),
            ]),
        )?;

        let list = session.execute("menu.list", &obj(&[]))?;
        let menus = list.as_array().ok_or("expected array")?;
        let menu = menus
            .first()
            .ok_or("missing menu")?
            .as_object()
            .ok_or("expected object")?;

        assert_eq!(menu.get("id").and_then(Value::as_u64), Some(menu_id));
        assert_eq!(menu.get("name").and_then(Value::as_str), Some("Menu 1"));
        assert_eq!(menu.get("defaultButton").and_then(Value::as_u64), Some(0));

        let buttons = menu
            .get("buttons")
            .and_then(Value::as_array)
            .ok_or("missing buttons")?;
        let button = buttons
            .first()
            .ok_or("missing button")?
            .as_object()
            .ok_or("expected object")?;

        assert_eq!(button.get("label").and_then(Value::as_str), Some("Play"));
        assert_eq!(button.get("link").and_then(Value::as_str), Some("stop"));

        let rect = button
            .get("rect")
            .and_then(Value::as_object)
            .ok_or("missing rect")?;
        assert_eq!(rect.get("x").and_then(Value::as_u64), Some(160));
        assert_eq!(rect.get("y").and_then(Value::as_u64), Some(120));
        assert_eq!(rect.get("w").and_then(Value::as_u64), Some(400));
        assert_eq!(rect.get("h").and_then(Value::as_u64), Some(48));

        Ok(())
    }
}
