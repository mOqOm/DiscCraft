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
        id: "menu.setRichHighlights",
        label: "リッチカラーハイライトの変更",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "on": bool}"#,
        enabled: always_enabled,
        run: run_set_rich_highlights,
        journal: true,
    },
    CommandSpec {
        id: "menu.addPage",
        label: "新規ページを追加",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id}"#,
        enabled: always_enabled,
        run: run_add_page,
        journal: true,
    },
    CommandSpec {
        id: "menu.replace",
        label: "メニューを置き換え",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "path"?: "menu.psd", "preset"?: string, "spec"?: MenuSpec}"#,
        enabled: always_enabled,
        run: run_replace,
        journal: true,
    },
    CommandSpec {
        id: "menu.import",
        label: "メニューとして読み込み",
        menu: &["ファイル", "読み込み"],
        shortcut: None,
        params: r#"{"path": "menu.psd", "popup"?: bool, "pageOf"?: id}"#,
        enabled: always_enabled,
        run: run_import,
        journal: true,
    },
    CommandSpec {
        id: "menu.fillChapterIndex",
        label: "チャプターインデックスを作成",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id}"#,
        enabled: always_enabled,
        run: run_fill_chapter_index,
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
        id: "button.setTextSubpicture",
        label: "テキストサブピクチャを作成を設定",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "button": index, "on": bool}"#,
        enabled: always_enabled,
        run: run_set_text_subpicture,
        journal: true,
    },
    CommandSpec {
        id: "button.setTransition",
        label: "ボタンのトランジションを設定",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "button": index, "asset": video asset id | null}"#,
        enabled: always_enabled,
        run: run_set_transition,
        journal: true,
    },
    CommandSpec {
        id: "menu.setPopup",
        label: "Blu-ray ポップアップメニューとして設定",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "on": bool}"#,
        enabled: always_enabled,
        run: run_set_popup,
        journal: true,
    },
    CommandSpec {
        id: "menu.setPopupBackground",
        label: "ポップアップの背景を設定",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "timeline": id | null, "timecode"?: "00;00;10;00"}"#,
        enabled: always_enabled,
        run: run_set_popup_background,
        journal: true,
    },
    CommandSpec {
        id: "menu.setLoops",
        label: "ループ回数を設定",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "loops": "once" | "forever" | 2..9999 | null}"#,
        enabled: always_enabled,
        run: run_set_loops,
        journal: true,
    },
    CommandSpec {
        id: "menu.setAnimateButtons",
        label: "モーションボタンの変更",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "on": bool}"#,
        enabled: always_enabled,
        run: run_set_animate_buttons,
        journal: true,
    },
    CommandSpec {
        id: "menu.setLoopPoint",
        label: "ループポイントを設定",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id, "timecode": "00;00;02;00" | null}"#,
        enabled: always_enabled,
        run: run_set_loop_point,
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
        // The highlight colour is group 1's selected colour (the automatic set's too, instead of a PSD layer's).
        if let Some(set) = menu.color_set.as_mut() {
            set.groups[0].selected = color;
        }
        menu.subpicture_colors[0] = None;
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

/// Encore's "Create Text Subpicture": whether the highlight is the button's text or a frame.
fn run_set_text_subpicture(session: &mut Session, params: &Value) -> CommandResult {
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
        return Err(format!("button {button} does not exist"));
    };
    target.text_subpicture = on;
    Ok(empty_object())
}

/// Encore's button Transition: a video played after the button is activated, before its link (null: none).
fn run_set_transition(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let button = req_usize(params, "button")?;
    let asset = match params.get("asset") {
        None | Some(Value::Null) => None,
        Some(value) => Some(disccraft_project::AssetId(value.as_u64().ok_or_else(
            || "parameter \"asset\" must be a video asset id or null".to_string(),
        )?)),
    };
    if let Some(asset) = asset {
        let kind = session.project.assets.get(&asset).map(|asset| asset.kind);
        if kind != Some(disccraft_project::AssetKind::Video) {
            return Err(format!("asset {} is not a video", asset.0));
        }
    }
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let Some(target) = menu.buttons.get_mut(button) else {
        return Err(format!("button {button} does not exist"));
    };
    target.transition = asset;
    Ok(empty_object())
}

/// Encore's "Set as Blu-ray Pop-up Menu".
fn run_set_popup(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let on = params
        .get("on")
        .and_then(Value::as_bool)
        .ok_or_else(|| "parameter \"on\" must be true or false".to_string())?;
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.popup = on;
    Ok(empty_object())
}

/// Encore's pop-up 背景タイムライン / 背景フレーム: the timeline and time the editor shows a pop-up menu over
/// (null: none). Leaving the timecode out keeps the time (or starts at 0).
fn run_set_popup_background(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let timeline = match params.get("timeline") {
        None | Some(Value::Null) => None,
        Some(value) => Some(disccraft_project::TimelineId(value.as_u64().ok_or_else(
            || "parameter \"timeline\" must be a timeline id or null".to_string(),
        )?)),
    };
    if let Some(timeline) = timeline
        && !session.project.timelines.contains_key(&timeline)
    {
        return Err(format!("timeline {} does not exist", timeline.0));
    }
    let time = opt_str(params, "timecode")?
        .map(|text| {
            disccraft_project::parse_time(&text, &session.project.settings)
                .map_err(|error| error.to_string())
        })
        .transpose()?;
    if time.is_some_and(|time| time.0 < 0) {
        return Err("\"timecode\" must not be negative".to_string());
    }
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.popup_background = timeline.map(|timeline| disccraft_project::PopupBackground {
        timeline,
        time: time
            .or_else(|| menu.popup_background.map(|background| background.time))
            .unwrap_or_default(),
    });
    Ok(empty_object())
}

/// Encore's ループ回数 of a motion menu: once (なし), a number of plays, forever (無限), or null for the earlier rule
/// (once with an end action, forever without).
fn run_set_loops(session: &mut Session, params: &Value) -> CommandResult {
    use disccraft_project::{MAX_MENU_LOOPS, MenuLoops};
    let menu_id = MenuId(req_u64(params, "menu")?);
    let loops = match params.get("loops") {
        None => return Err("parameter \"loops\" is required".to_string()),
        Some(Value::Null) => None,
        Some(Value::String(text)) if text == "once" => Some(MenuLoops::Once),
        Some(Value::String(text)) if text == "forever" => Some(MenuLoops::Forever),
        Some(value) => {
            let times = value
                .as_u64()
                .and_then(|n| u16::try_from(n).ok())
                .filter(|n| (1..=MAX_MENU_LOOPS).contains(n))
                .ok_or_else(|| {
                    format!(
                        "\"loops\" must be \"once\", \"forever\", 1 to {MAX_MENU_LOOPS} or null"
                    )
                })?;
            Some(if times == 1 {
                MenuLoops::Once
            } else {
                MenuLoops::Times(times)
            })
        }
    };
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.loops = loops;
    Ok(empty_object())
}

/// Encore's ループポイント of a motion menu: where the later plays start (null: from the start).
fn run_set_loop_point(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let point = match params.get("timecode") {
        None => return Err("parameter \"timecode\" is required (a timecode, or null)".to_string()),
        Some(Value::Null) => None,
        Some(Value::String(text)) if text.trim().is_empty() => None,
        Some(Value::String(text)) => Some(
            disccraft_project::parse_time(text.trim(), &session.project.settings)
                .map_err(|error| error.to_string())?,
        ),
        Some(_) => return Err("\"timecode\" must be a timecode or null".to_string()),
    };
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    if let Some(point) = point {
        let Some(seconds) = menu.motion_seconds() else {
            return Err(
                "only a motion menu (a video background, or motion buttons) has a loop point"
                    .to_string(),
            );
        };
        if point.0 <= 0 || point.seconds() >= f64::from(seconds) {
            return Err(format!(
                "the loop point must be inside the motion (0 to {seconds} s)"
            ));
        }
    }
    menu.loop_point = point;
    Ok(empty_object())
}

/// Encore's モーションボタン: the menu's video thumbnails play (all of them), and the menu becomes a motion menu
/// (15 seconds over a still background unless it has a duration).
fn run_set_animate_buttons(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let on = match params.get("on") {
        Some(Value::Bool(on)) => *on,
        _ => return Err("parameter \"on\" must be true or false".to_string()),
    };
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.animate_buttons = on;
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
/// Encore's ハイライトをリッチカラーで書き出し (Blu-ray highlights as 256-colour images).
fn run_set_rich_highlights(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let on = match params.get("on") {
        Some(Value::Bool(on)) => *on,
        _ => return Err("parameter \"on\" must be true or false".to_string()),
    };
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.rich_highlights = on;
    Ok(empty_object())
}

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

/// Encore's チャプターインデックスを作成 on a template menu: its chapter buttons link to the chapters that follow the
/// first linked one.
fn run_fill_chapter_index(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let linked = session
        .project
        .fill_chapter_index(menu_id)
        .map_err(|error| match error {
            disccraft_project::FillChapterIndexError::NoMenu => {
                format!("menu {} does not exist", menu_id.0)
            }
            disccraft_project::FillChapterIndexError::NoChapterButtons => {
                "the menu has no chapter buttons".to_string()
            }
            disccraft_project::FillChapterIndexError::NoLinkedChapterButton => {
                "link at least one chapter button to a timeline first".to_string()
            }
        })?;
    Ok(object(&[("buttons", Value::from(linked))]))
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
    let (rects, asset) = psd_buttons(session, &path)?;

    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.background = MenuBackground::Image { asset };
    menu.subpicture_colors = subpicture_colors(&rects);
    menu.texts.clear();
    for (index, found) in rects.iter().enumerate() {
        match menu.buttons.get_mut(index) {
            Some(button) => {
                button.rect = found.rect;
                button.thumbnail = found.thumbnail;
                button.subpicture.clone_from(&found.subpicture);
                button.label.clear();
                if !found.name.is_empty() {
                    button.name.clone_from(&found.name);
                }
            }
            None => menu.buttons.push(psd_button(found)),
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

/// A button made from a PSD's `(+)` group: its name, its place, video thumbnail and subpicture, no text (the text
/// is in the picture) and no link.
fn psd_button(found: &PsdMenuButton) -> disccraft_project::Button {
    disccraft_project::Button {
        name: found.name.clone(),
        label: String::new(),
        rect: found.rect,
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
        text_subpicture: false,
        transition: None,
        label_format: Default::default(),
        label_font_style: None,
        shadow: None,
        orientation: Default::default(),
        thumbnail: found.thumbnail,
        subpicture: found.subpicture.clone(),
    }
}

/// Encore's ファイル ▸ 読み込み ▸ メニュー… / ポップアップメニュー… / メニューページ…: a layered PSD becomes a new menu
/// named after the file (its picture the background, its `(+)` groups the buttons, the lowest group first as
/// Encore numbers them), a Blu-ray
/// pop-up menu with "popup", or a new page of the menu "pageOf".
fn run_import(session: &mut Session, params: &Value) -> CommandResult {
    let path = req_str(params, "path")?;
    let popup = match opt_value(params, "popup") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err("parameter \"popup\" must be true or false".to_string()),
    };
    let page_of = opt_u64(params, "pageOf")?.map(MenuId);
    if let Some(root) = page_of
        && session.project.menu(root).is_none()
    {
        return Err(format!("menu {} does not exist", root.0));
    }
    let (rects, asset) = psd_buttons(session, &path)?;
    if rects.len() > disccraft_project::MAX_BUTTONS {
        return Err(format!(
            "the PSD has {} buttons; a menu holds at most {}",
            rects.len(),
            disccraft_project::MAX_BUTTONS
        ));
    }
    let menu_id = match page_of {
        Some(root) => session
            .project
            .add_menu_page(root)
            .map_err(|error| error.to_string())?,
        None => {
            let name = std::path::Path::new(&path)
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned());
            session
                .project
                .add_menu(name)
                .map_err(|error| error.to_string())?
        }
    };
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.background = MenuBackground::Image { asset };
    menu.subpicture_colors = subpicture_colors(&rects);
    if popup {
        menu.popup = true;
    }
    menu.buttons = rects.iter().map(psd_button).collect();
    menu.default_button = 0;
    Ok(object(&[
        ("menu", Value::from(menu_id.0)),
        ("asset", Value::from(asset.0)),
        ("buttons", Value::from(rects.len())),
    ]))
}

/// One button of a PSD menu, in the project's frame.
struct PsdMenuButton {
    /// The name of its `(+)` group.
    name: String,
    /// Where it is.
    rect: Rect,
    /// Its `(%)` layer, as a part of the button.
    thumbnail: Option<disccraft_project::ThumbnailArea>,
    /// Its `(=1)` / `(=2)` / `(=3)` layers.
    subpicture: Option<disccraft_project::Subpicture>,
    /// The colours of those layers.
    subpicture_colors: [Option<[u8; 3]>; 3],
}

/// The colours of a PSD menu's subpicture layers: each the first button's that has the layer (for Encore's
/// automatic colour set).
fn subpicture_colors(buttons: &[PsdMenuButton]) -> [Option<[u8; 3]>; 3] {
    let mut colors = [None; 3];
    for (index, color) in colors.iter_mut().enumerate() {
        *color = buttons
            .iter()
            .find_map(|button| button.subpicture_colors.get(index).copied().flatten());
    }
    colors
}

/// The buttons of a layered PSD menu (their names, places in the project's frame and video thumbnails) and the
/// PSD as an image asset (the same path reuses it).
fn psd_buttons(session: &mut Session, path: &str) -> Result<(Vec<PsdMenuButton>, AssetId), String> {
    let psd = disccraft_photo::load_psd_menu(std::path::Path::new(path))
        .map_err(|error| error.to_string())?;
    let (frame_w, frame_h) = disccraft_project::frame_size(&session.project.settings);
    let scale = |value: u32, from: u32, to: u16| -> u16 {
        let scaled = u64::from(value) * u64::from(to) / u64::from(from.max(1));
        u16::try_from(scaled).unwrap_or(u16::MAX)
    };
    let rects: Vec<PsdMenuButton> = psd
        .buttons
        .iter()
        .map(|button| {
            let x = scale(button.x, psd.width, frame_w).min(frame_w.saturating_sub(8));
            let y = scale(button.y, psd.height, frame_h).min(frame_h.saturating_sub(8));
            let w = scale(button.width, psd.width, frame_w)
                .max(8)
                .min(frame_w.saturating_sub(x));
            let h = scale(button.height, psd.height, frame_h)
                .max(8)
                .min(frame_h.saturating_sub(y));
            let rect = Rect { x, y, w, h };
            let thumbnail = button.thumbnail.and_then(|[tx, ty, tw, th]| {
                let area = Rect {
                    x: scale(tx, psd.width, frame_w),
                    y: scale(ty, psd.height, frame_h),
                    w: scale(tw, psd.width, frame_w).max(1),
                    h: scale(th, psd.height, frame_h).max(1),
                };
                disccraft_project::ThumbnailArea::within(rect, area)
            });
            let subpicture = button.subpicture.as_ref().and_then(|sub| {
                disccraft_project::Subpicture::from_values(
                    u16::try_from(sub.width).ok()?,
                    u16::try_from(sub.height).ok()?,
                    &sub.values,
                )
            });
            PsdMenuButton {
                name: button.name.clone(),
                rect,
                thumbnail,
                subpicture,
                subpicture_colors: button
                    .subpicture
                    .as_ref()
                    .map_or([None; 3], |sub| sub.colors),
            }
        })
        .collect();

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
    Ok((rects, asset))
}

/// Encore's 新規ページを追加: an empty page after the menu's last page, with the first page's look and settings.
fn run_add_page(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    if session.project.menu(menu_id).is_none() {
        return Err(format!("menu {} does not exist", menu_id.0));
    }
    let page = session
        .project
        .add_menu_page(menu_id)
        .map_err(|error| error.to_string())?;
    let number = session.project.menu_page_number(page);
    Ok(object(&[
        ("menu", Value::from(page.0)),
        (
            "page",
            Value::from(u64::try_from(number).unwrap_or(u64::MAX)),
        ),
    ]))
}

/// Encore's メニューを置き換え: a layered PSD ("path"), a library menu ("preset") or a user's library menu ("spec")
/// becomes the menu's design; the menu's settings and its buttons' links (by number) stay.
fn run_replace(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    if session.project.menu(menu_id).is_none() {
        return Err(format!("menu {} does not exist", menu_id.0));
    }
    let design = match (
        opt_str(params, "path")?,
        opt_str(params, "preset")?,
        params.get("spec"),
    ) {
        (Some(path), None, None) => {
            let (rects, asset) = psd_buttons(session, &path)?;
            disccraft_project::MenuDesign {
                background: MenuBackground::Image { asset },
                colors: None,
                subpicture_colors: subpicture_colors(&rects),
                buttons: rects
                    .into_iter()
                    .map(|found| disccraft_project::DesignButton {
                        rect: found.rect,
                        name: Some(found.name),
                        // The button texts are part of the picture.
                        label: Some(String::new()),
                        thumbnail: found.thumbnail,
                        subpicture: found.subpicture,
                    })
                    .collect(),
            }
        }
        (None, Some(preset), None) => {
            let item = disccraft_project::library_item(&preset)
                .filter(|item| item.kind == disccraft_project::LibraryKind::Menu)
                .ok_or_else(|| format!("\"{preset}\" is not a library menu"))?;
            session
                .project
                .menu_design_from_spec(&disccraft_project::MenuSpec::from(item))
        }
        (None, None, Some(spec)) => {
            let spec: disccraft_project::MenuSpec = serde_json::from_value(spec.clone())
                .map_err(|error| format!("parameter \"spec\" is not a menu spec: {error}"))?;
            if spec.buttons.len() > 36 {
                return Err("a menu spec holds at most 36 buttons".to_string());
            }
            session.project.menu_design_from_spec(&spec)
        }
        _ => return Err("give exactly one of \"path\", \"preset\" or \"spec\"".to_string()),
    };
    session
        .project
        .replace_menu(menu_id, design)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
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

    #[test]
    fn a_popup_background_is_set_and_goes_with_its_timeline() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let asset = session
            .project
            .add_asset(std::path::Path::new("movie.m2v"))?;
        let timeline = session.project.add_timeline(asset, None)?.0;
        let menu = session
            .execute("menu.create", &serde_json::json!({"name": "Pop"}))?
            .get("menu")
            .and_then(Value::as_u64)
            .ok_or("menu")?;
        session.execute(
            "menu.setPopupBackground",
            &serde_json::json!({"menu": menu, "timeline": timeline, "timecode": "00;00;02;00"}),
        )?;
        let background = session
            .project
            .menu(MenuId(menu))
            .and_then(|menu| menu.popup_background)
            .ok_or("background")?;
        assert_eq!(background.timeline.0, timeline);
        assert!((background.time.seconds() - 2.0).abs() < 0.1);
        assert!(
            session
                .execute(
                    "menu.setPopupBackground",
                    &serde_json::json!({"menu": menu, "timeline": 999}),
                )
                .is_err()
        );
        session.execute(
            "project.remove",
            &serde_json::json!({"kind": "timeline", "id": timeline}),
        )?;
        assert!(
            session
                .project
                .menu(MenuId(menu))
                .is_some_and(|menu| menu.popup_background.is_none())
        );
        Ok(())
    }

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
    fn a_motion_menu_gets_a_loop_point_inside_its_motion() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu_id = value_u64(&session.execute("menu.create", &obj(&[]))?, "menu")?;
        let set = |timecode: Value| obj(&[("menu", Value::from(menu_id)), ("timecode", timecode)]);
        // A still menu has no loop point.
        assert!(
            session
                .execute(
                    "menu.setLoopPoint",
                    &set(Value::String("00;00;02;00".to_owned()))
                )
                .is_err()
        );
        if let Some(menu) = session.project.menu_mut(MenuId(menu_id)) {
            menu.background = disccraft_project::MenuBackground::Video {
                asset: disccraft_project::AssetId(1),
                seconds: 6,
            };
        }
        session.execute(
            "menu.setLoopPoint",
            &set(Value::String("00;00;02;00".to_owned())),
        )?;
        let point = |session: &Session| {
            session
                .project
                .menu(MenuId(menu_id))
                .and_then(|menu| menu.effective_loop_point())
                .map(|point| (point.seconds() * 10.0).round() / 10.0)
        };
        assert_eq!(point(&session), Some(2.0));
        // Past the end of the motion.
        assert!(
            session
                .execute(
                    "menu.setLoopPoint",
                    &set(Value::String("00;00;06;00".to_owned()))
                )
                .is_err()
        );
        session.execute("menu.setLoopPoint", &set(Value::Null))?;
        assert_eq!(point(&session), None);
        session.undo()?;
        assert_eq!(point(&session), Some(2.0));
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
    fn a_library_menu_replaces_the_design_and_keeps_the_links() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu_id = value_u64(&session.execute("menu.create", &obj(&[]))?, "menu")?;
        session.execute(
            "menu.addButton",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("label", Value::String("Play".to_string())),
                ("target", Value::String("stop".to_string())),
            ]),
        )?;
        session.execute(
            "menu.addText",
            &serde_json::json!({"menu": menu_id, "text": "Title", "x": 10, "y": 10}),
        )?;
        let replace = |preset: &str| {
            obj(&[
                ("menu", Value::from(menu_id)),
                ("preset", Value::String(preset.to_owned())),
            ])
        };
        session.execute("menu.replace", &replace("menu.bottom-bar"))?;
        let preset = disccraft_project::library_item("menu.bottom-bar").ok_or("no preset")?;
        let menu = session
            .project
            .menu(MenuId(menu_id))
            .ok_or("missing menu")?;
        assert_eq!(menu.buttons.len(), preset.buttons.len().max(1));
        let first = menu.buttons.first().ok_or("no button")?;
        assert_eq!(first.link, Link::Stop);
        assert_eq!(first.label, "Play");
        assert!(menu.texts.is_empty());
        assert!(
            session
                .execute("menu.replace", &replace("button.wide"))
                .is_err()
        );

        session.undo()?;
        let menu = session
            .project
            .menu(MenuId(menu_id))
            .ok_or("missing menu")?;
        assert_eq!(menu.buttons.len(), 1);
        assert_eq!(menu.texts.len(), 1);
        Ok(())
    }

    #[test]
    fn a_psd_is_imported_as_a_menu_a_pop_up_menu_or_a_page() -> Result<(), Box<dyn Error>> {
        let dir =
            std::env::temp_dir().join(format!("disccraft-import-menu-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let (w, h) = (720u32, 480u32);
        let fill = |x0: u32, y0: u32, x1: u32, y1: u32| {
            let mut rgba = vec![0u8; (w * h * 4) as usize];
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * w + x) * 4) as usize;
                    rgba[i..i + 4].copy_from_slice(&[200, 200, 200, 255]);
                }
            }
            rgba
        };
        let group = |name: &str, y: u32| disccraft_photo::PsdLayer::Group {
            name: format!("(+){name}"),
            children: vec![disccraft_photo::PsdLayer::Raster {
                name: "text".to_owned(),
                rgba: fill(100, y, 300, y + 40),
                visible: true,
            }],
        };
        let path = dir.join("Scenes.psd");
        disccraft_photo::write_layered_psd(
            &path,
            w,
            h,
            &[
                disccraft_photo::PsdLayer::Raster {
                    name: "背景".to_owned(),
                    rgba: fill(0, 0, w, h),
                    visible: true,
                },
                // Encore numbers the groups from the bottom of the stack, wherever they are on the screen.
                group("First", 200),
                group("Second", 100),
            ],
        )?;
        let path_text = path.to_string_lossy().into_owned();

        let mut session = Session::new();
        let imported = session.execute("menu.import", &serde_json::json!({ "path": path_text }))?;
        let menu_id = MenuId(value_u64(&imported, "menu")?);
        let menu = session.project.menu(menu_id).ok_or("missing menu")?;
        assert_eq!(menu.name, "Scenes");
        assert!(matches!(
            menu.background,
            disccraft_project::MenuBackground::Image { .. }
        ));
        let names: Vec<&str> = menu
            .buttons
            .iter()
            .map(|button| button.name.as_str())
            .collect();
        assert_eq!(names, ["First", "Second"]);
        assert!(menu.buttons.iter().all(|button| button.label.is_empty()));
        assert!(!menu.popup);

        let popup = session.execute(
            "menu.import",
            &serde_json::json!({ "path": path_text, "popup": true }),
        )?;
        let popup = MenuId(value_u64(&popup, "menu")?);
        assert!(session.project.menu(popup).ok_or("missing pop-up")?.popup);

        let page = session.execute(
            "menu.import",
            &serde_json::json!({ "path": path_text, "pageOf": menu_id.0 }),
        )?;
        let page = MenuId(value_u64(&page, "menu")?);
        assert_eq!(session.project.menu_pages(menu_id), [menu_id, page]);
        assert_eq!(
            session
                .project
                .menu(page)
                .ok_or("missing page")?
                .buttons
                .len(),
            2
        );
        // One PSD asset for all three.
        assert_eq!(session.project.assets.len(), 1);

        assert!(
            session
                .execute(
                    "menu.import",
                    &serde_json::json!({ "path": path_text, "pageOf": 999 })
                )
                .is_err()
        );
        session.undo()?;
        assert_eq!(session.project.menus.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn a_thumbnail_layer_becomes_the_buttons_video_thumbnail() -> Result<(), Box<dyn Error>> {
        let dir =
            std::env::temp_dir().join(format!("disccraft-import-thumb-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let (w, h) = (720u32, 480u32);
        let fill = |x0: u32, y0: u32, x1: u32, y1: u32| {
            let mut rgba = vec![0u8; (w * h * 4) as usize];
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * w + x) * 4) as usize;
                    rgba[i..i + 4].copy_from_slice(&[128, 128, 128, 255]);
                }
            }
            rgba
        };
        let path = dir.join("thumbs.psd");
        disccraft_photo::write_layered_psd(
            &path,
            w,
            h,
            &[
                disccraft_photo::PsdLayer::Raster {
                    name: "背景".to_owned(),
                    rgba: fill(0, 0, w, h),
                    visible: true,
                },
                disccraft_photo::PsdLayer::Group {
                    name: "(+)Scene".to_owned(),
                    children: vec![
                        disccraft_photo::PsdLayer::Raster {
                            name: "frame".to_owned(),
                            rgba: fill(100, 100, 300, 260),
                            visible: true,
                        },
                        disccraft_photo::PsdLayer::Raster {
                            name: "(%)video".to_owned(),
                            rgba: fill(100, 100, 300, 220),
                            visible: true,
                        },
                    ],
                },
            ],
        )?;
        let mut session = Session::new();
        let imported = session.execute(
            "menu.import",
            &serde_json::json!({ "path": path.to_string_lossy() }),
        )?;
        let menu_id = MenuId(value_u64(&imported, "menu")?);
        let button = session
            .project
            .menu(menu_id)
            .and_then(|menu| menu.buttons.first())
            .ok_or("no button")?;
        // NTSC keeps the PSD's 720 x 480 places.
        assert_eq!(
            button.thumbnail_rect(),
            Some(disccraft_project::Rect {
                x: 100,
                y: 100,
                w: 200,
                h: 120
            })
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn rich_highlights_are_a_menu_setting_that_can_be_undone() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let created = session.execute("menu.create", &serde_json::json!({}))?;
        let menu_id = created["menu"].as_u64().ok_or("menu id")?;
        session.execute(
            "menu.setRichHighlights",
            &serde_json::json!({ "menu": menu_id, "on": true }),
        )?;
        assert!(
            session
                .project
                .menu(MenuId(menu_id))
                .ok_or("menu")?
                .rich_highlights
        );
        assert_eq!(session.undo_label(), Some("リッチカラーハイライトの変更"));
        session.undo()?;
        assert!(
            !session
                .project
                .menu(MenuId(menu_id))
                .ok_or("menu")?
                .rich_highlights
        );
        assert!(
            session
                .execute(
                    "menu.setRichHighlights",
                    &serde_json::json!({ "menu": menu_id })
                )
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn motion_buttons_make_a_fifteen_second_motion_menu() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu_id = value_u64(&session.execute("menu.create", &obj(&[]))?, "menu")?;
        session.execute(
            "menu.addButton",
            &obj(&[
                ("menu", Value::from(menu_id)),
                ("label", Value::String("Scene".to_string())),
            ]),
        )?;
        if let Some(button) = session
            .project
            .menu_mut(MenuId(menu_id))
            .and_then(|menu| menu.buttons.first_mut())
        {
            button.thumbnail = Some(disccraft_project::ThumbnailArea::WHOLE);
        }
        let loop_point = |session: &mut Session| {
            session.execute(
                "menu.setLoopPoint",
                &serde_json::json!({ "menu": menu_id, "timecode": "00;00;05;00" }),
            )
        };
        // A still menu has no loop point.
        assert!(loop_point(&mut session).is_err());

        session.execute(
            "menu.setAnimateButtons",
            &serde_json::json!({ "menu": menu_id, "on": true }),
        )?;
        let menu = session.project.menu(MenuId(menu_id)).ok_or("menu")?;
        assert!(menu.plays_buttons());
        assert_eq!(menu.motion_seconds(), Some(15));
        assert_eq!(menu.duration_seconds, None);
        loop_point(&mut session)?;
        let menu = session.project.menu(MenuId(menu_id)).ok_or("menu")?;
        assert!(menu.effective_loop_point().is_some());
        // Past the 15 seconds: no.
        assert!(
            session
                .execute(
                    "menu.setLoopPoint",
                    &serde_json::json!({ "menu": menu_id, "timecode": "00;00;20;00" }),
                )
                .is_err()
        );

        session.undo()?;
        session.undo()?;
        let menu = session.project.menu(MenuId(menu_id)).ok_or("menu")?;
        assert!(!menu.animate_buttons);
        assert_eq!(menu.motion_seconds(), None);
        assert!(
            session
                .execute(
                    "menu.setAnimateButtons",
                    &serde_json::json!({ "menu": menu_id, "on": "yes" }),
                )
                .is_err()
        );
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
    fn a_psds_subpictures_and_their_colours_come_in() -> Result<(), Box<dyn Error>> {
        let dir =
            std::env::temp_dir().join(format!("disccraft-psd-colours-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let (w, h) = (720_usize, 480_usize);
        // A layer of `colour` over x 100..300, y `top`..`top + 20`.
        let strip = |top: usize, colour: [u8; 4]| {
            let mut rgba = vec![0u8; w * h * 4];
            for y in top..top + 20 {
                for x in 100..300 {
                    let i = (y * w + x) * 4;
                    if let Some(pixel) = rgba.get_mut(i..i + 4) {
                        pixel.copy_from_slice(&colour);
                    }
                }
            }
            rgba
        };
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
                    children: vec![
                        disccraft_photo::PsdLayer::Raster {
                            name: "(=1)".to_owned(),
                            rgba: strip(200, [255, 0, 0, 255]),
                            visible: false,
                        },
                        disccraft_photo::PsdLayer::Raster {
                            name: "(=2)".to_owned(),
                            rgba: strip(220, [0, 255, 0, 255]),
                            visible: false,
                        },
                    ],
                },
            ],
        )?;
        let path = path.to_string_lossy().into_owned();
        let mut session = Session::new();
        let imported = session.execute(
            "menu.import",
            &obj(&[("path", Value::String(path.clone()))]),
        )?;
        let menu_id = MenuId(value_u64(&imported, "menu")?);
        let menu = session.project.menu(menu_id).ok_or("menu")?;
        assert_eq!(
            menu.subpicture_colors,
            [Some([255, 0, 0]), Some([0, 255, 0]), None]
        );
        let values = menu
            .buttons
            .first()
            .and_then(|button| button.subpicture.as_ref())
            .and_then(disccraft_project::Subpicture::values)
            .ok_or("subpicture")?;
        assert_eq!((values.first(), values.last()), (Some(&1), Some(&2)));
        // Encore's automatic set: group 1 in the layers' colours.
        let group = menu.color_set_or_default().groups[0];
        assert_eq!(group.selected_color(1), ([255, 0, 0], 100));
        assert_eq!(group.activated_color(2), ([0, 255, 0], 100));

        // メニューを置き換え with the PSD brings them too.
        let other = value_u64(&session.execute("menu.create", &obj(&[]))?, "menu")?;
        session.execute(
            "menu.addButton",
            &obj(&[
                ("menu", Value::from(other)),
                ("label", Value::String("A".to_owned())),
            ]),
        )?;
        session.execute(
            "menu.replace",
            &obj(&[("menu", Value::from(other)), ("path", Value::String(path))]),
        )?;
        let replaced = session.project.menu(MenuId(other)).ok_or("menu")?;
        assert_eq!(replaced.subpicture_colors[0], Some([255, 0, 0]));
        assert!(
            replaced
                .buttons
                .first()
                .is_some_and(|button| button.subpicture.is_some())
        );
        // Setting the highlight colour by hand: the automatic set follows it.
        session.execute(
            "menu.setColors",
            &obj(&[
                ("menu", Value::from(other)),
                ("highlight", Value::String("#0000FF".to_owned())),
            ]),
        )?;
        let replaced = session.project.menu(MenuId(other)).ok_or("menu")?;
        assert_eq!(
            replaced.color_set_or_default().groups[0].selected,
            [0, 0, 255]
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
