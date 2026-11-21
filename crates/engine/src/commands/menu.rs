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
        id: "menu.fillChapterIndexPages",
        label: "複数ページのチャプターインデックスを作成",
        menu: &["メニュー"],
        shortcut: None,
        params: r#"{"menu": id}"#,
        enabled: always_enabled,
        run: run_fill_chapter_index_pages,
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
        id: "button.setLabelOffset",
        label: "ボタンの文字の位置を設定",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "button": index, "x": int, "y": int} (元の場所からのずれ。ボタンの外に出すとボタンの範囲が広がる)"#,
        enabled: always_enabled,
        run: run_set_label_offset,
        journal: true,
    },
    CommandSpec {
        id: "button.setThumbnailOffset",
        label: "ボタンのビデオサムネールの位置を設定",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "button": index, "x": int, "y": int} (元の場所からのずれ。ボタンの外に出すとボタンの範囲が広がる)"#,
        enabled: always_enabled,
        run: run_set_thumbnail_offset,
        journal: true,
    },
    CommandSpec {
        id: "button.setThumbnailArea",
        label: "ボタンのビデオサムネールの範囲を設定",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "button": index, "area": {"x": int, "y": int, "w": int, "h": int}} (フレームの座標。ボタンの中)"#,
        enabled: always_enabled,
        run: run_set_thumbnail_area,
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
        id: "button.createSubpicture",
        label: "サブピクチャを作成",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "buttons": [index, ...]}"#,
        enabled: always_enabled,
        run: run_create_subpicture,
        journal: true,
    },
    CommandSpec {
        id: "button.pasteSubpicture",
        label: "サブピクチャとして貼り付け",
        menu: &["編集"],
        shortcut: Some("Shift+Ctrl+V"),
        params: r#"{"menu": id, "button": index, "graphic": a menu graphic (as the project file holds it)}"#,
        enabled: always_enabled,
        run: run_paste_subpicture,
        journal: true,
    },
    CommandSpec {
        id: "button.setSubpicture",
        label: "ボタンのサブピクチャを設定",
        menu: &["ボタン"],
        shortcut: None,
        params: r#"{"menu": id, "button": index, "path": "highlight.png" | null}"#,
        enabled: always_enabled,
        run: run_set_subpicture,
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
    // As in Encore, the first menu becomes the disc's title button.
    session.project.adopt_title_remote(id);
    // Encore's blank menu comes with guides on the action-safe (5%) and title-safe (10%) lines, hidden until
    // the guides are shown.
    let frame = disccraft_project::frame_size(&session.project.settings);
    if let Some(menu) = session.project.menu_mut(id) {
        menu.guides = disccraft_project::safe_area_guides(frame);
    }

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

/// Encore's Create Subpicture (M6.256): each button gets a subpicture (a `(=1)` layer) drawn in colour 1 of the
/// colour set, in place of its text or frame highlight. A button of a menu made from a PSD takes the shape of its
/// artwork (what its `(+)` set shows), found by its place or, when it was moved or resized since (M6.330), by its
/// name, and stretched over it as it is now; any other button (made in DiscCraft, or a PSD button without artwork)
/// takes the shape of its own text and video thumbnail (M6.330: as Encore makes a `(=1)` of a text button).
/// `{"buttons": n}`: how many got one. Refused when none of them has anything to make one of.
fn run_create_subpicture(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let wanted: Vec<usize> = params
        .get("buttons")
        .and_then(Value::as_array)
        .map(|buttons| {
            buttons
                .iter()
                .filter_map(|index| index.as_u64().and_then(|index| usize::try_from(index).ok()))
                .collect()
        })
        .ok_or_else(|| "parameter \"buttons\" must be a list of button numbers".to_string())?;
    let menu = session
        .project
        .menu(menu_id)
        .ok_or_else(|| format!("menu {} does not exist", menu_id.0))?;
    // The menu's PSD artwork, when it is made from one.
    let art = match menu.background {
        disccraft_project::MenuBackground::Image { asset } => session
            .project
            .assets
            .get(&asset)
            .map(|asset| asset.media_file().into_owned())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("psd"))
            })
            .map(|path| disccraft_photo::psd_button_artwork(&path))
            .transpose()
            .map_err(|error| error.to_string())?,
        _ => None,
    };
    let (frame_w, frame_h) = disccraft_project::frame_size(&session.project.settings);
    // A set's place on the frame, as the import puts its button.
    let scale = |value: u32, from: u32, to: u16| -> i64 {
        i64::try_from(u64::from(value) * u64::from(to) / u64::from(from.max(1))).unwrap_or(i64::MAX)
    };
    let near = |a: u16, b: i64| (i64::from(a) - b).abs() <= 2;
    let mut made = Vec::new();
    for index in wanted {
        let Some(button) = menu.buttons.get(index) else {
            return Err(format!("button {index} does not exist"));
        };
        let rect = button.rect;
        let artwork = art.as_ref().and_then(|art| {
            let placed = art.buttons.iter().find(|found| {
                let [x, y, w, h] = found.place;
                near(rect.x, scale(x, art.size.0, frame_w))
                    && near(rect.y, scale(y, art.size.1, frame_h))
                    && near(rect.w, scale(w, art.size.0, frame_w))
                    && near(rect.h, scale(h, art.size.1, frame_h))
            });
            // Moved or resized: its set by name, when only one has it.
            let named = || {
                let mut same = art
                    .buttons
                    .iter()
                    .filter(|found| !found.name.is_empty() && found.name == button.name);
                let first = same.next();
                first.filter(|_| same.next().is_none())
            };
            placed.or_else(named)?.artwork.as_ref()
        });
        let subpicture = match artwork {
            Some(artwork) => disccraft_project::Subpicture::from_values(
                u16::try_from(artwork.width).ok().unwrap_or(0),
                u16::try_from(artwork.height).ok().unwrap_or(0),
                &artwork.values,
            ),
            None => own_subpicture(menu, button),
        };
        if let Some(subpicture) = subpicture {
            made.push((index, subpicture));
        }
    }
    if made.is_empty() {
        return Err(
            "none of the buttons has anything to make a subpicture of (no artwork, text or video thumbnail)"
                .to_string(),
        );
    }
    let count = made.len();
    if let Some(menu) = session.project.menu_mut(menu_id) {
        for (index, subpicture) in made {
            replace_subpicture(menu, index, Some(subpicture), (frame_w, frame_h))?;
        }
    }
    Ok(object(&[("buttons", Value::from(count))]))
}

/// A button's own subpicture of its text and video thumbnail ([`disccraft_menu::own_subpicture`]), placed relative
/// to its content box.
fn own_subpicture(
    menu: &Menu,
    button: &disccraft_project::Button,
) -> Option<disccraft_project::Subpicture> {
    let (area, values) = disccraft_menu::own_subpicture(menu, button)?;
    let mut subpicture = disccraft_project::Subpicture::from_values(area.w, area.h, &values)?;
    let content = button.content_box();
    let relative =
        |start: u16, from: u16| i16::try_from(i32::from(start) - i32::from(from)).unwrap_or(0);
    subpicture.area = (area != content).then_some(disccraft_project::ContentBox {
        x: relative(area.x, content.x),
        y: relative(area.y, content.y),
        w: area.w,
        h: area.h,
    });
    Some(subpicture)
}

/// Gives button `index` `subpicture` in place of its own; when either reaches past the content box (a pasted layer,
/// M6.329), the button's rectangle is fitted to its content again.
fn replace_subpicture(
    menu: &mut Menu,
    index: usize,
    subpicture: Option<disccraft_project::Subpicture>,
    frame: (u16, u16),
) -> Result<(), String> {
    let Some(button) = menu.buttons.get_mut(index) else {
        return Err(format!("button {index} does not exist"));
    };
    let reaching =
        |sub: Option<&disccraft_project::Subpicture>| sub.is_some_and(|sub| sub.area.is_some());
    let refit = reaching(button.subpicture.as_ref()) || reaching(subpicture.as_ref());
    button.subpicture = subpicture;
    if refit {
        refit_button(menu, index, frame)?;
    }
    Ok(())
}

/// Encore's Edit ▸ Paste as Subpicture: the shape of a copied layer (an image or a shape) becomes colour 1 of the
/// button's own subpicture (a `(=1)` layer), over what the subpicture had; a button without one gets one, so it is
/// highlighted by that shape instead of its text or frame. Where the layer reaches past the button, the subpicture
/// reaches as far and the button widens to it (M6.329: Encore's button set takes the layer in), its text and
/// thumbnail staying where they are; a turned button widens in its own upright frame, turned with it (M6.360).
/// Refused when the layer does not reach the button.
fn run_paste_subpicture(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let index = req_usize(params, "button")?;
    let graphic: disccraft_project::MenuGraphic =
        serde_json::from_value(req_value(params, "graphic")?.clone())
            .map_err(|error| format!("parameter \"graphic\" is not a menu graphic: {error}"))?;
    let frame = disccraft_project::frame_size(&session.project.settings);
    let button = session
        .project
        .menu(menu_id)
        .ok_or_else(|| format!("menu {} does not exist", menu_id.0))?
        .buttons
        .get(index)
        .ok_or_else(|| format!("button {index} does not exist"))?
        .clone();
    let content = button.content_box();
    if content.w == 0 || content.h == 0 {
        return Err("the button has no size".to_string());
    }
    // Where the subpicture lies now (over the content box without one), and where the layer is.
    let current = button.subpicture_rect().unwrap_or(content);
    let turned = disccraft_project::normalized_angle(i32::from(button.orientation.angle)) != 0;
    let layer = clip_to_frame(
        disccraft_project::rotated_bounds(graphic.rect, graphic.orientation.angle),
        frame,
    );
    let pivot = button.content_centre();
    let angle = button.orientation.angle;
    // A turned button's content is drawn turned about the content box's centre: the layer is met where it lies
    // on screen.
    let on_screen = disccraft_project::rotated_bounds_about(content, pivot, angle);
    let layer = layer
        .filter(|layer| overlaps(*layer, on_screen))
        .ok_or_else(|| "the layer does not reach the button".to_string())?;
    let (area, covered) = if turned {
        turned_coverage(
            &session.project,
            &graphic,
            layer,
            current,
            (pivot, angle),
            frame,
        )?
    } else {
        let area = union(current, layer);
        let covered = disccraft_menu::graphic_coverage(&session.project, &graphic, area, frame)
            .ok_or_else(|| "the button has no size".to_string())?;
        (area, covered)
    };
    let (width, height) = (usize::from(area.w), usize::from(area.h));
    // What the subpicture had, stretched over where it lay, now inside the wider area.
    let old = button.subpicture.as_ref().and_then(|old| {
        Some((
            old.values()?,
            usize::from(old.width),
            usize::from(old.height),
        ))
    });
    let mut values = vec![0_u8; width * height];
    if let Some((pixels, old_w, old_h)) = old {
        let (cw, ch) = (usize::from(current.w).max(1), usize::from(current.h).max(1));
        let (dx, dy) = (
            usize::from(current.x.saturating_sub(area.x)),
            usize::from(current.y.saturating_sub(area.y)),
        );
        for y in 0..ch {
            let source_row = y * old_h / ch;
            for x in 0..cw {
                let value = pixels
                    .get(source_row * old_w + x * old_w / cw)
                    .copied()
                    .unwrap_or(0);
                if let Some(slot) = values.get_mut((y + dy) * width + x + dx) {
                    *slot = value;
                }
            }
        }
    }
    for (value, covers) in values.iter_mut().zip(&covered) {
        if *covers {
            *value = 1;
        }
    }
    let mut subpicture = disccraft_project::Subpicture::from_values(area.w, area.h, &values)
        .ok_or_else(|| "the subpicture would be too large".to_string())?;
    let relative =
        |start: u16, from: u16| i16::try_from(i32::from(start) - i32::from(from)).unwrap_or(0);
    subpicture.area = (area != content).then_some(disccraft_project::ContentBox {
        x: relative(area.x, content.x),
        y: relative(area.y, content.y),
        w: area.w,
        h: area.h,
    });
    let menu = session
        .project
        .menu_mut(menu_id)
        .ok_or_else(|| format!("menu {} does not exist", menu_id.0))?;
    if let Some(button) = menu.buttons.get_mut(index) {
        button.subpicture = Some(subpicture);
    }
    // The button's rectangle takes the subpicture in (its content box stays).
    refit_button(menu, index, frame)?;
    Ok(empty_object())
}

/// [`run_paste_subpicture`] for a button turned by a free angle about `pivot` (M6.360): its subpicture lies in the
/// button's own, upright frame and is drawn turned. The layer (`layer`: where it lies on screen) is turned back into
/// that frame; the subpicture's area (`current`, upright) widens to hold it, and each of its pixels takes the
/// layer's coverage at the screen pixel it is drawn on. The area does not reach left of or above the frame's origin.
fn turned_coverage(
    project: &disccraft_project::Project,
    graphic: &disccraft_project::MenuGraphic,
    layer: disccraft_project::Rect,
    current: disccraft_project::Rect,
    ((cx, cy), angle): ((f32, f32), i16),
    frame: (u16, u16),
) -> Result<(disccraft_project::Rect, Vec<bool>), String> {
    let on_screen = disccraft_menu::graphic_coverage(project, graphic, layer, frame)
        .ok_or_else(|| "the layer has no size".to_string())?;
    let (sin, cos) = (f32::from(angle) / 10.0).to_radians().sin_cos();
    // Upright → screen, and screen → upright (as the highlight draws a turned subpicture).
    let to_screen = |x: f32, y: f32| {
        let (px, py) = (x - cx, y - cy);
        (px * cos - py * sin + cx, px * sin + py * cos + cy)
    };
    let to_upright = |x: f32, y: f32| {
        let (px, py) = (x - cx, y - cy);
        (px * cos + py * sin + cx, -px * sin + py * cos + cy)
    };
    let (x0, y0) = (f32::from(layer.x), f32::from(layer.y));
    let (x1, y1) = (x0 + f32::from(layer.w), y0 + f32::from(layer.h));
    let corners = [
        to_upright(x0, y0),
        to_upright(x1, y0),
        to_upright(x0, y1),
        to_upright(x1, y1),
    ];
    let bound = |pick: fn(&(f32, f32)) -> f32, lowest: bool| {
        let values = corners.iter().map(pick);
        if lowest {
            values.fold(f32::INFINITY, f32::min).floor()
        } else {
            values.fold(f32::NEG_INFINITY, f32::max).ceil()
        }
    };
    let side = |value: f32| -> u16 {
        // Rounded and kept between 0 and the largest frame position; `as` saturates.
        value.clamp(0.0, f32::from(u16::MAX)) as u16
    };
    let left = side(bound(|c| c.0, true)).min(current.x);
    let top = side(bound(|c| c.1, true)).min(current.y);
    let right = side(bound(|c| c.0, false)).max(current.x.saturating_add(current.w));
    let bottom = side(bound(|c| c.1, false)).max(current.y.saturating_add(current.h));
    let area = disccraft_project::Rect {
        x: left,
        y: top,
        w: right.saturating_sub(left).max(1),
        h: bottom.saturating_sub(top).max(1),
    };
    let (width, height) = (usize::from(area.w), usize::from(area.h));
    let layer_w = usize::from(layer.w);
    let mut covered = vec![false; width * height];
    for row in 0..height {
        for col in 0..width {
            let (sx, sy) = to_screen(
                f32::from(area.x) + col as f32 + 0.5,
                f32::from(area.y) + row as f32 + 0.5,
            );
            let (dx, dy) = (sx.floor() - x0, sy.floor() - y0);
            if dx < 0.0 || dy < 0.0 || dx >= f32::from(layer.w) || dy >= f32::from(layer.h) {
                continue;
            }
            // Inside the layer's rectangle; `as` truncates the non-negative position.
            let at = (dy as usize) * layer_w + dx as usize;
            if on_screen.get(at).copied().unwrap_or(false)
                && let Some(slot) = covered.get_mut(row * width + col)
            {
                *slot = true;
            }
        }
    }
    Ok((area, covered))
}

/// The part of `rect` inside a frame of `frame` pixels; `None` when none is.
fn clip_to_frame(
    rect: disccraft_project::Rect,
    frame: (u16, u16),
) -> Option<disccraft_project::Rect> {
    let x0 = rect.x.min(frame.0);
    let y0 = rect.y.min(frame.1);
    let x1 = rect.x.saturating_add(rect.w).min(frame.0);
    let y1 = rect.y.saturating_add(rect.h).min(frame.1);
    (x1 > x0 && y1 > y0).then_some(disccraft_project::Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

/// Whether two rectangles share a pixel.
fn overlaps(a: disccraft_project::Rect, b: disccraft_project::Rect) -> bool {
    u32::from(a.x) < u32::from(b.x) + u32::from(b.w)
        && u32::from(b.x) < u32::from(a.x) + u32::from(a.w)
        && u32::from(a.y) < u32::from(b.y) + u32::from(b.h)
        && u32::from(b.y) < u32::from(a.y) + u32::from(a.h)
}

/// The smallest rectangle holding both.
fn union(a: disccraft_project::Rect, b: disccraft_project::Rect) -> disccraft_project::Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = a.x.saturating_add(a.w).max(b.x.saturating_add(b.w));
    let y1 = a.y.saturating_add(a.h).max(b.y.saturating_add(b.h));
    disccraft_project::Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

/// A button's own subpicture (its highlight, as a PSD's `(=1)`–`(=3)` layers give it) from a PNG stretched over the
/// button: transparent pixels none, the others colour 1 to 3 of the colour set by how light they are (null: none,
/// back to the text or frame highlight).
fn run_set_subpicture(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let button = req_usize(params, "button")?;
    let subpicture = match params.get("path") {
        None | Some(Value::Null) => None,
        Some(Value::String(path)) => {
            let (width, height, values) =
                disccraft_menu::subpicture_from_png(std::path::Path::new(path))
                    .map_err(|error| error.to_string())?;
            let size = |value: u32| u16::try_from(value).map_err(|_| format!("{path}: too large"));
            Some(
                disccraft_project::Subpicture::from_values(size(width)?, size(height)?, &values)
                    .ok_or_else(|| {
                        format!("{path}: no highlight in it (every pixel transparent)")
                    })?,
            )
        }
        Some(_) => return Err("parameter \"path\" must be a PNG path or null".to_string()),
    };
    let frame = disccraft_project::frame_size(&session.project.settings);
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    replace_subpicture(menu, button, subpicture, frame)?;
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
/// Encore's "Set as Blu-ray Pop-up Menu" for the menu and all its pages (Encore's pages are one menu): a pop-up
/// menu's pages pop up together over the title (M10.29).
fn run_set_popup(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let on = params
        .get("on")
        .and_then(Value::as_bool)
        .ok_or_else(|| "parameter \"on\" must be true or false".to_string())?;
    if session.project.menu(menu_id).is_none() {
        return Err(format!("menu {} does not exist", menu_id.0));
    }
    for page in session.project.menu_pages(menu_id) {
        if let Some(menu) = session.project.menu_mut(page) {
            menu.popup = on;
        }
    }
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

    target.set_rect(rect);
    Ok(empty_object())
}

/// Fits button `index`'s rectangle to its content (Encore's bounds of a button set): when the content is moved out
/// of its box or turned, the rectangle becomes the area the content covers and the box is kept relative to it;
/// otherwise the button is its box again.
pub(crate) fn refit_button(menu: &mut Menu, index: usize, frame: (u16, u16)) -> Result<(), String> {
    let button = menu
        .buttons
        .get(index)
        .ok_or_else(|| format!("button index {index} is out of range"))?
        .clone();
    let content = button.content_box();
    let extent = disccraft_menu::button_extent(menu, &button, frame);
    let turned = disccraft_project::normalized_angle(i32::from(button.orientation.angle)) != 0;
    let target = menu
        .buttons
        .get_mut(index)
        .ok_or_else(|| format!("button index {index} is out of range"))?;
    if !turned && extent == content {
        target.rect = content;
        target.body = None;
    } else {
        let relative =
            |start: u16, from: u16| i16::try_from(i32::from(start) - i32::from(from)).unwrap_or(0);
        target.body = Some(disccraft_project::ContentBox {
            x: relative(content.x, extent.x),
            y: relative(content.y, extent.y),
            w: content.w,
            h: content.h,
        });
        target.rect = extent;
    }
    Ok(())
}

/// Whether `rect` lies inside a frame of `frame` size.
fn inside_frame(rect: Rect, frame: (u16, u16)) -> bool {
    u32::from(rect.x) + u32::from(rect.w) <= u32::from(frame.0)
        && u32::from(rect.y) + u32::from(rect.h) <= u32::from(frame.1)
}

/// An integer parameter as i16.
fn coordinate(params: &Value, name: &str) -> Result<i16, String> {
    params
        .get(name)
        .and_then(Value::as_i64)
        .and_then(|value| i16::try_from(value).ok())
        .ok_or_else(|| format!("parameter \"{name}\" must be an integer"))
}

/// Moves a button's text from its place (Encore's Direct Select tool on the text layer). The text may leave the
/// button, whose rectangle then grows to take it in (it must stay inside the frame and clear of the other buttons).
fn run_set_label_offset(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let button = req_usize(params, "button")?;
    let offset = [coordinate(params, "x")?, coordinate(params, "y")?];
    let frame = disccraft_project::frame_size(&session.project.settings);
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let Some(target) = menu.buttons.get_mut(button) else {
        return Err(format!("button index {button} is out of range"));
    };
    target.label_offset = offset;
    let moved = target.clone();
    if disccraft_menu::label_bounds(menu, &moved).is_some_and(|bounds| !inside_frame(bounds, frame))
    {
        return Err("the button's text would leave the frame".to_string());
    }
    refit_button(menu, button, frame)?;
    Ok(empty_object())
}

/// Moves a button's video thumbnail from its place (the Direct Select tool on Encore's `(%)` layer); the button's
/// rectangle grows to take it in.
fn run_set_thumbnail_offset(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let button = req_usize(params, "button")?;
    let offset = [coordinate(params, "x")?, coordinate(params, "y")?];
    let frame = disccraft_project::frame_size(&session.project.settings);
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let Some(target) = menu.buttons.get_mut(button) else {
        return Err(format!("button index {button} is out of range"));
    };
    if target.thumbnail.is_none() {
        return Err("the button has no video thumbnail".to_string());
    }
    target.thumbnail_offset = offset;
    let placed = target.thumbnail_placed().map(|area| {
        disccraft_project::rotated_bounds_about(
            area,
            target.content_centre(),
            target.orientation.angle,
        )
    });
    let left_or_top = |value: i16, start: u16| i32::from(start) + i32::from(value) < 0;
    let thumbnail = target.thumbnail_rect();
    if thumbnail
        .is_some_and(|area| left_or_top(offset[0], area.x) || left_or_top(offset[1], area.y))
        || placed.is_some_and(|area| !inside_frame(area, frame))
    {
        return Err("the video thumbnail would leave the frame".to_string());
    }
    refit_button(menu, button, frame)?;
    Ok(empty_object())
}

/// Places a button's video thumbnail (Encore's `(%)` layer): a rectangle of the frame inside the button's content
/// box, kept as a part of the box so it moves and scales with it.
fn run_set_thumbnail_area(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let button = req_usize(params, "button")?;
    let area = req_rect(params, "area")?;
    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let Some(target) = menu.buttons.get_mut(button) else {
        return Err(format!("button index {button} is out of range"));
    };
    let rect = target.content_box();
    let inside = area.w >= 1
        && area.h >= 1
        && area.x >= rect.x
        && area.y >= rect.y
        && u32::from(area.x) + u32::from(area.w) <= u32::from(rect.x) + u32::from(rect.w)
        && u32::from(area.y) + u32::from(area.h) <= u32::from(rect.y) + u32::from(rect.h);
    if !inside {
        return Err("the video thumbnail must lie inside its button".to_string());
    }
    target.thumbnail = disccraft_project::ThumbnailArea::within(rect, area);
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

/// Encore's 複数ページのチャプターインデックスを作成 on a template menu: the menu and copies of it, page after page,
/// until every chapter has a button. `{"menus": [id, ...]}`, the menu itself first.
fn run_fill_chapter_index_pages(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let pages = session
        .project
        .fill_chapter_index_pages(menu_id)
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
    let menus: Vec<Value> = pages.iter().map(|page| Value::from(page.0)).collect();
    Ok(object(&[("menus", Value::Array(menus))]))
}

/// Make a menu follow a PSD edited outside (Encore's "Edit Menu in Photoshop"): the PSD becomes the background, the
/// `(+)` groups become the buttons (in order: the menu's buttons keep their links and get the new rectangles, new
/// groups add buttons, buttons without a group are removed), and the button texts and the text objects are cleared,
/// as they are in the picture now.
fn run_apply_psd(session: &mut Session, params: &Value) -> CommandResult {
    let menu_id = MenuId(req_u64(params, "menu")?);
    let path = req_str(params, "path")?;
    let Some(page) = session.project.menu(menu_id).map(|menu| menu.psd_page) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    let (pages, asset) = psd_buttons(session, &path)?;
    // A page of a PSD shown by page takes its own page's buttons (M10.27).
    let index = page.map_or(0, usize::from);
    let index = if index < pages.len() { index } else { 0 };
    let rects = pages.into_iter().nth(index).unwrap_or_default();

    let Some(menu) = session.project.menu_mut(menu_id) else {
        return Err(format!("menu {} does not exist", menu_id.0));
    };
    menu.background = MenuBackground::Image { asset };
    let groups = psd_groups(&rects);
    menu.subpicture_colors = groups.first;
    menu.second_subpicture_colors = groups.second;
    menu.texts.clear();
    for (index, found) in rects.iter().enumerate() {
        let group = groups.groups.get(index).copied().unwrap_or(1);
        match menu.buttons.get_mut(index) {
            Some(button) => {
                button.rect = found.rect;
                button.thumbnail = found.thumbnail;
                button.subpicture.clone_from(&found.subpicture);
                button.highlight_group = group;
                button.label.clear();
                if !found.name.is_empty() {
                    button.name.clone_from(&found.name);
                }
                // A chapter index button's prefix gives its kind; an ordinary `(+)` keeps the one set by hand.
                if found.kind != disccraft_project::ButtonKind::Normal {
                    button.kind = found.kind;
                }
            }
            None => menu.buttons.push(psd_button(found, group)),
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

/// A button made from a PSD's `(+)` group: its name, its place, video thumbnail and subpicture, its highlight group
/// in the automatic colour set, no text (the text is in the picture) and no link.
fn psd_button(found: &PsdMenuButton, highlight_group: u8) -> disccraft_project::Button {
    disccraft_project::Button {
        effects: Default::default(),
        name: found.name.clone(),
        label: String::new(),
        rect: found.rect,
        link: Link::NotSet,
        auto_activate: false,
        override_link: None,
        highlight_group,
        sync_name: true,
        name_from_link: false,
        routes: None,
        audio: None,
        subtitle: disccraft_project::SubtitleDefault::NoChange,
        kind: found.kind,
        text_subpicture: false,
        transition: None,
        label_format: Default::default(),
        label_font_style: None,
        shadow: None,
        orientation: Default::default(),
        hidden: false,
        locked: false,
        thumbnail: found.thumbnail,
        subpicture: found.subpicture.clone(),
        label_offset: [0, 0],
        thumbnail_offset: [0, 0],
        body: None,
    }
}

/// Encore's ファイル ▸ 読み込み ▸ メニュー… / ポップアップメニュー… / メニューページ…: a layered PSD becomes a new menu
/// named after the file (its picture the background, its `(+)` groups the buttons, the lowest group first as
/// Encore numbers them), a Blu-ray
/// pop-up menu with "popup", or a new page of the menu "pageOf". A PSD with `(#)` groups at its top level
/// (Encore's menu pages, M10.27) becomes a menu of pages: the top level the first page, each group the next (the
/// lowest first), every page over the layers they share. A top level without buttons is no page (Encore keeps it
/// as an empty first page, which a disc cannot use): the lowest group is the first page.
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
    let (pages, asset) = psd_buttons(session, &path)?;
    for rects in &pages {
        if rects.len() > disccraft_project::MAX_BUTTONS {
            return Err(format!(
                "a page of the PSD has {} buttons; a menu holds at most {}",
                rects.len(),
                disccraft_project::MAX_BUTTONS
            ));
        }
    }
    let paged = pages.len() > 1;
    let start = usize::from(paged && pages.first().is_some_and(Vec::is_empty));
    let first_buttons = pages.get(start).map_or(0, Vec::len);
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
    session.project.adopt_title_remote(menu_id);
    let mut page_ids = Vec::new();
    for (index, rects) in pages.iter().enumerate().skip(start) {
        let id = if index == start {
            menu_id
        } else {
            let page = session
                .project
                .add_menu_page(menu_id)
                .map_err(|error| error.to_string())?;
            page_ids.push(Value::from(page.0));
            page
        };
        let Some(menu) = session.project.menu_mut(id) else {
            return Err(format!("menu {} does not exist", id.0));
        };
        menu.background = MenuBackground::Image { asset };
        menu.psd_page = paged.then(|| u16::try_from(index).ok()).flatten();
        let groups = psd_groups(rects);
        menu.subpicture_colors = groups.first;
        menu.second_subpicture_colors = groups.second;
        if popup {
            menu.popup = true;
        }
        menu.buttons = rects
            .iter()
            .zip(groups.groups.iter().copied().chain(std::iter::repeat(1)))
            .map(|(found, group)| psd_button(found, group))
            .collect();
        menu.default_button = 0;
    }
    Ok(object(&[
        ("menu", Value::from(menu_id.0)),
        ("asset", Value::from(asset.0)),
        ("buttons", Value::from(first_buttons)),
        ("pages", Value::from(page_ids)),
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
    /// Its kind by the group name's prefix (`(+#)` / `(+>)` / `(+<)`).
    kind: disccraft_project::ButtonKind,
}

/// Encore's automatic colour set of a PSD menu (M6.336): the colours of highlight groups 1 and 2, and each button's
/// group.
struct PsdGroups {
    /// Group 1's colours: the first button's with subpicture layers.
    first: [Option<[u8; 3]>; 3],
    /// Group 2's colours: the next button's whose colours differ.
    second: [Option<[u8; 3]>; 3],
    /// Each button's highlight group (1 or 2).
    groups: Vec<u8>,
}

/// Groups a PSD menu's buttons by their subpicture layers' colours as Encore's automatic colour set does: the first
/// button's colours are group 1, the next different ones group 2; a button with a group's colours is in it, any
/// other (or one without layers) in group 1.
fn psd_groups(buttons: &[PsdMenuButton]) -> PsdGroups {
    let mut found: Vec<[Option<[u8; 3]>; 3]> = Vec::new();
    let groups = buttons
        .iter()
        .map(|button| {
            let colors = button.subpicture_colors;
            if colors.iter().all(Option::is_none) {
                return 1;
            }
            if let Some(index) = found.iter().position(|group| *group == colors) {
                return u8::try_from(index + 1).unwrap_or(1);
            }
            if found.len() < 2 {
                found.push(colors);
                return u8::try_from(found.len()).unwrap_or(1);
            }
            1
        })
        .collect();
    PsdGroups {
        first: found.first().copied().unwrap_or([None; 3]),
        second: found.get(1).copied().unwrap_or([None; 3]),
        groups,
    }
}

/// The buttons of a layered PSD menu (their names, places in the project's frame and video thumbnails) by page
/// (the top level's first, then each `(#)` group's, M10.27; one page without groups) and the PSD as an image
/// asset (the same path reuses it).
fn psd_buttons(
    session: &mut Session,
    path: &str,
) -> Result<(Vec<Vec<PsdMenuButton>>, AssetId), String> {
    let psd = disccraft_photo::load_psd_menu(std::path::Path::new(path))
        .map_err(|error| error.to_string())?;
    let (frame_w, frame_h) = disccraft_project::frame_size(&session.project.settings);
    let scale = |value: u32, from: u32, to: u16| -> u16 {
        let scaled = u64::from(value) * u64::from(to) / u64::from(from.max(1));
        u16::try_from(scaled).unwrap_or(u16::MAX)
    };
    let convert = |button: &disccraft_photo::PsdButton| {
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
            let (width, height) = (
                u16::try_from(sub.width).ok()?,
                u16::try_from(sub.height).ok()?,
            );
            let mut made = disccraft_project::Subpicture::from_values(width, height, &sub.values)?;
            // M6.335: the layers in their own colours, for rich-colour Blu-ray highlights.
            made.color_layers = sub.layers.each_ref().map(|layer| {
                layer
                    .as_deref()
                    .and_then(|rgba| disccraft_project::ColorLayer::from_rgba(width, height, rgba))
            });
            Some(made)
        });
        PsdMenuButton {
            name: button.name.clone(),
            kind: match button.kind {
                disccraft_photo::PsdButtonKind::Normal => disccraft_project::ButtonKind::Normal,
                disccraft_photo::PsdButtonKind::Chapter => disccraft_project::ButtonKind::Chapter,
                disccraft_photo::PsdButtonKind::Next => disccraft_project::ButtonKind::Next,
                disccraft_photo::PsdButtonKind::Previous => disccraft_project::ButtonKind::Previous,
            },
            rect,
            thumbnail,
            subpicture,
            subpicture_colors: button
                .subpicture
                .as_ref()
                .map_or([None; 3], |sub| sub.colors),
        }
    };
    let pages: Vec<Vec<PsdMenuButton>> = std::iter::once(&psd.buttons)
        .chain(psd.pages.iter().map(|(_, buttons)| buttons))
        .map(|buttons| buttons.iter().map(convert).collect())
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
    Ok((pages, asset))
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
            let (pages, asset) = psd_buttons(session, &path)?;
            let rects = pages.into_iter().next().unwrap_or_default();
            let groups = psd_groups(&rects);
            disccraft_project::MenuDesign {
                background: MenuBackground::Image { asset },
                colors: None,
                subpicture_colors: groups.first,
                second_subpicture_colors: groups.second,
                buttons: rects
                    .into_iter()
                    .zip(groups.groups.iter().copied().chain(std::iter::repeat(1)))
                    .map(|(found, group)| disccraft_project::DesignButton {
                        rect: found.rect,
                        name: Some(found.name),
                        // The button texts are part of the picture.
                        label: Some(String::new()),
                        thumbnail: found.thumbnail,
                        subpicture: found.subpicture,
                        highlight_group: Some(group),
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

    /// M6.284: Edit ▸ Paste as Subpicture — a copied layer's shape where it covers the button becomes colour 1 of
    /// the button's subpicture, added to what it had; a layer that misses the button is refused. M6.329: a layer
    /// that reaches past the button widens the subpicture and the button to it, the text staying where it was.
    #[test]
    fn a_layer_is_pasted_as_a_buttons_subpicture() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu = session.project.add_menu(None)?;
        let rect = disccraft_project::Rect {
            x: 100,
            y: 100,
            w: 200,
            h: 80,
        };
        let _ = session
            .project
            .add_button(menu, "Play".to_owned(), Some(rect), Link::NotSet)?;
        let shape = |x: u16, y: u16, w: u16, h: u16| {
            serde_json::json!({
                "rect": { "x": x, "y": y, "w": w, "h": h },
                "kind": { "shape": { "shape": "rectangle", "color": [0, 0, 255] } },
                "opacity": 20,
            })
        };
        // A 50 x 40 box over the button's left part, at 30 % into it.
        session.execute(
            "button.pasteSubpicture",
            &serde_json::json!({ "menu": menu.0, "button": 0, "graphic": shape(150, 110, 50, 40) }),
        )?;
        let values = |session: &Session| {
            session
                .project
                .menu(menu)
                .and_then(|menu| menu.buttons.first())
                .and_then(|button| button.subpicture.as_ref())
                .and_then(|subpicture| {
                    Some((subpicture.width, subpicture.height, subpicture.values()?))
                })
        };
        let (width, height, pixels) = values(&session).ok_or("no subpicture")?;
        assert_eq!((width, height), (200, 80));
        let at = |pixels: &[u8], x: usize, y: usize| pixels.get(y * 200 + x).copied();
        assert_eq!(at(&pixels, 50, 10), Some(1));
        assert_eq!(at(&pixels, 99, 49), Some(1));
        assert_eq!(at(&pixels, 100, 10), Some(0));
        assert_eq!(at(&pixels, 49, 10), Some(0));
        assert_eq!(pixels.iter().filter(|value| **value == 1).count(), 50 * 40);
        let button = |session: &Session| {
            session
                .project
                .menu(menu)
                .and_then(|menu| menu.buttons.first())
                .cloned()
        };
        assert_eq!(button(&session).ok_or("button")?.rect, rect);
        // A second layer adds to it, reaching 50 past the button's right and 70 past its bottom: the subpicture
        // covers both, the button widens to it and its content box (where the text is) stays.
        session.execute(
            "button.pasteSubpicture",
            &serde_json::json!({ "menu": menu.0, "button": 0, "graphic": shape(250, 150, 100, 100) }),
        )?;
        let (width, height, pixels) = values(&session).ok_or("no subpicture")?;
        assert_eq!((width, height), (250, 150));
        let wide = |pixels: &[u8], x: usize, y: usize| pixels.get(y * 250 + x).copied();
        // The first layer where it was, the second past the button's corner.
        assert_eq!(wide(&pixels, 60, 20), Some(1));
        assert_eq!(wide(&pixels, 100, 10), Some(0));
        assert_eq!(wide(&pixels, 249, 149), Some(1));
        assert_eq!(wide(&pixels, 149, 49), Some(0));
        let widened = button(&session).ok_or("button")?;
        assert_eq!(
            widened.rect,
            disccraft_project::Rect {
                x: 100,
                y: 100,
                w: 250,
                h: 150
            }
        );
        assert_eq!(widened.content_box(), rect);
        assert_eq!(
            widened.subpicture_rect(),
            Some(disccraft_project::Rect {
                x: 100,
                y: 100,
                w: 250,
                h: 150
            })
        );
        // Taking the subpicture away fits the button to its content again.
        session.execute(
            "button.setSubpicture",
            &serde_json::json!({ "menu": menu.0, "button": 0, "path": null }),
        )?;
        let fitted = button(&session).ok_or("button")?;
        assert_eq!((fitted.rect, fitted.body), (rect, None));
        session.execute(
            "button.pasteSubpicture",
            &serde_json::json!({ "menu": menu.0, "button": 0, "graphic": shape(250, 150, 100, 100) }),
        )?;
        assert!(
            session
                .execute(
                    "button.pasteSubpicture",
                    &serde_json::json!({ "menu": menu.0, "button": 0, "graphic": shape(0, 0, 50, 50) }),
                )
                .is_err()
        );
        assert!(
            session
                .execute(
                    "button.pasteSubpicture",
                    &serde_json::json!({ "menu": menu.0, "button": 0, "graphic": { "rect": 1 } }),
                )
                .is_err()
        );
        Ok(())
    }

    /// M6.360: on a button turned a quarter clockwise, a layer that reaches past it widens the subpicture in the
    /// button's upright frame: drawn turned, the subpicture covers the layer where it lies on screen.
    #[test]
    fn a_layer_is_pasted_on_a_turned_button() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu = session.project.add_menu(None)?;
        let rect = disccraft_project::Rect {
            x: 200,
            y: 200,
            w: 200,
            h: 80,
        };
        let _ = session
            .project
            .add_button(menu, String::new(), Some(rect), Link::NotSet)?;
        if let Some(button) = session
            .project
            .menu_mut(menu)
            .and_then(|menu| menu.buttons.first_mut())
        {
            button.orientation.angle = 900;
        }
        // On screen the button covers x 260..340, y 140..340; the layer x 330..390, y 300..360 reaches past it.
        session.execute(
            "button.pasteSubpicture",
            &serde_json::json!({ "menu": menu.0, "button": 0, "graphic": {
                "rect": { "x": 330, "y": 300, "w": 60, "h": 60 },
                "kind": { "shape": { "shape": "rectangle", "color": [0, 0, 255] } },
            } }),
        )?;
        let button = session
            .project
            .menu(menu)
            .and_then(|menu| menu.buttons.first())
            .cloned()
            .ok_or("button")?;
        // Upright, the layer is x 360..420, y 150..210: the area holds it and the content box.
        let area = button.subpicture_rect().ok_or("no subpicture")?;
        assert_eq!(
            area,
            disccraft_project::Rect {
                x: 200,
                y: 150,
                w: 220,
                h: 130
            }
        );
        assert_eq!(button.content_box(), rect);
        let subpicture = button.subpicture.as_ref().ok_or("no subpicture")?;
        let pixels = subpicture.values().ok_or("no values")?;
        assert_eq!((subpicture.width, subpicture.height), (220, 130));
        let at = |x: usize, y: usize| pixels.get((y - 150) * 220 + (x - 200)).copied();
        assert_eq!(at(410, 160), Some(1));
        assert_eq!(at(365, 205), Some(1));
        assert_eq!(at(355, 205), Some(0));
        assert_eq!(at(250, 250), Some(0));
        let count = pixels.iter().filter(|value| **value == 1).count();
        assert!((3500..=3700).contains(&count), "{count}");
        // The button's rectangle takes in the turned area: x 260..390, y 140..360 on screen.
        let near = |a: u16, b: u16| a.abs_diff(b) <= 1;
        assert!(
            near(button.rect.x, 260)
                && near(button.rect.y, 140)
                && near(button.rect.w, 130)
                && near(button.rect.h, 220),
            "{:?}",
            button.rect
        );
        // The highlight shows the layer where it lies on screen.
        let highlight = disccraft_menu::render_menu(
            &session.project,
            session.project.menu(menu).ok_or("menu")?,
        )?
        .highlight;
        let shown = |x: u16, y: u16| {
            let (dx, dy) = (x - highlight.x, y - highlight.y);
            highlight
                .pixels
                .get(usize::from(dy) * usize::from(highlight.width) + usize::from(dx))
                .copied()
        };
        assert_ne!(shown(385, 355), Some(0));
        assert_ne!(shown(335, 305), Some(0));
        assert_eq!(shown(300, 240), Some(0));
        Ok(())
    }

    /// M10.29: Set as Blu-ray Pop-up Menu on any page of a menu sets it for all its pages (Encore's pages are one
    /// menu), so they pop up together.
    #[test]
    fn the_popup_setting_goes_to_every_page() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu = session.project.add_menu(Some("Scenes".to_owned()))?;
        let page = session.project.add_menu_page(menu)?;
        session.execute(
            "menu.setPopup",
            &serde_json::json!({ "menu": page.0, "on": true }),
        )?;
        let popup = |id| session.project.menu(id).is_some_and(|menu| menu.popup);
        assert!(popup(menu) && popup(page));
        session.execute(
            "menu.setPopup",
            &serde_json::json!({ "menu": menu.0, "on": false }),
        )?;
        let popup = |id| session.project.menu(id).is_some_and(|menu| menu.popup);
        assert!(!popup(menu) && !popup(page));
        Ok(())
    }

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

    /// M10.27 (Encore's `(#)` page groups, GitHub issue #13): a PSD with page groups becomes a menu of pages. The top
    /// level's buttons are the first page's, each group's (the lowest first) its own page's, every page shows the
    /// PSD by its page; drawn, a page has the shared layers and its own, not the other pages'. A page added later
    /// shows only the shared layers.
    #[test]
    fn a_psds_page_groups_become_the_menus_pages() -> Result<(), Box<dyn Error>> {
        use disccraft_photo::PsdLayer;
        let dir =
            std::env::temp_dir().join(format!("disccraft-import-pages-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let (w, h) = (720u32, 480u32);
        let fill = |x0: u32, y0: u32, x1: u32, y1: u32, colour: [u8; 4]| {
            let mut rgba = vec![0u8; (w * h * 4) as usize];
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * w + x) * 4) as usize;
                    rgba[i..i + 4].copy_from_slice(&colour);
                }
            }
            rgba
        };
        let raster = |name: &str, rgba: Vec<u8>| PsdLayer::Raster {
            name: name.to_owned(),
            rgba,
            visible: true,
        };
        let button = |name: &str, x: u32, y: u32| PsdLayer::Group {
            name: format!("(+) {name}"),
            children: vec![raster(
                "text",
                fill(x, y, x + 200, y + 40, [255, 255, 255, 255]),
            )],
        };
        let path = dir.join("Paged.psd");
        disccraft_photo::write_layered_psd(
            &path,
            w,
            h,
            &[
                raster("背景", fill(0, 0, w, h, [0, 0, 120, 255])),
                button("Play", 100, 300),
                button("Next", 400, 300),
                PsdLayer::Group {
                    name: "(#) Episodes".to_owned(),
                    children: vec![
                        raster("art", fill(500, 40, 600, 100, [255, 0, 0, 255])),
                        button("Episode 1", 100, 380),
                        button("Back", 400, 380),
                        button("Episode 2", 100, 200),
                    ],
                },
            ],
        )?;
        let path_text = path.to_string_lossy().into_owned();
        let mut session = Session::new();
        let imported = session.execute("menu.import", &serde_json::json!({ "path": path_text }))?;
        let first = MenuId(value_u64(&imported, "menu")?);
        let pages = session.project.menu_pages(first);
        assert_eq!(pages.len(), 2);
        let second = *pages.get(1).ok_or("missing page")?;
        let names = |project: &disccraft_project::Project, id: MenuId| -> Vec<String> {
            project
                .menu(id)
                .map(|menu| {
                    menu.buttons
                        .iter()
                        .map(|button| button.name.clone())
                        .collect()
                })
                .unwrap_or_default()
        };
        assert_eq!(names(&session.project, first), ["Play", "Next"]);
        assert_eq!(
            names(&session.project, second),
            ["Episode 1", "Back", "Episode 2"]
        );
        let page_of = |project: &disccraft_project::Project, id: MenuId| {
            project.menu(id).and_then(|menu| menu.psd_page)
        };
        assert_eq!(
            (
                page_of(&session.project, first),
                page_of(&session.project, second)
            ),
            (Some(0), Some(1))
        );
        let second_menu = session.project.menu(second).ok_or("missing page")?;
        assert_eq!(
            second_menu.background,
            session
                .project
                .menu(first)
                .ok_or("missing menu")?
                .background
        );

        // Drawn: the shared background everywhere, each page's own layers only on it.
        let at = |project: &disccraft_project::Project,
                  menu: MenuId,
                  x: usize,
                  y: usize|
         -> Result<[u8; 3], Box<dyn Error>> {
            let menu = project.menu(menu).ok_or("missing menu")?;
            let drawn = disccraft_menu::render_menu(project, menu)?;
            let i = (y * usize::from(drawn.width) + x) * 3;
            Ok([drawn.rgb[i], drawn.rgb[i + 1], drawn.rgb[i + 2]])
        };
        let white = |colour: [u8; 3]| colour.iter().all(|value| *value > 200);
        assert!(
            white(at(&session.project, first, 150, 310)?),
            "Play on the first page"
        );
        assert!(
            !white(at(&session.project, first, 150, 390)?),
            "Episode 1 not on the first page"
        );
        assert!(
            white(at(&session.project, second, 150, 390)?),
            "Episode 1 on the second page"
        );
        assert!(
            !white(at(&session.project, second, 150, 310)?),
            "Play not on the second page"
        );
        assert!(
            at(&session.project, second, 550, 70)?[0] > 200,
            "the page's art"
        );
        assert!(
            at(&session.project, first, 550, 70)?[0] < 50,
            "no page art on the first page"
        );
        assert!(
            at(&session.project, first, 20, 20)?[2] > 100,
            "the shared background"
        );

        // The issue's form: no buttons at the top level, two page groups: they are the two pages.
        let bare = dir.join("Bare.psd");
        disccraft_photo::write_layered_psd(
            &bare,
            w,
            h,
            &[
                raster("背景", fill(0, 0, w, h, [0, 60, 0, 255])),
                PsdLayer::Group {
                    name: "(#) Main Page".to_owned(),
                    children: vec![button("Play", 100, 300), button("Episodes", 400, 300)],
                },
                PsdLayer::Group {
                    name: "(#) Episodes Page".to_owned(),
                    children: vec![button("Episode 1", 100, 380), button("Back", 400, 380)],
                },
            ],
        )?;
        let imported = session.execute(
            "menu.import",
            &serde_json::json!({ "path": bare.to_string_lossy() }),
        )?;
        let main = MenuId(value_u64(&imported, "menu")?);
        let bare_pages = session.project.menu_pages(main);
        assert_eq!(bare_pages.len(), 2);
        let episodes = *bare_pages.get(1).ok_or("missing page")?;
        assert_eq!(names(&session.project, main), ["Play", "Episodes"]);
        assert_eq!(names(&session.project, episodes), ["Episode 1", "Back"]);
        assert_eq!(
            (
                page_of(&session.project, main),
                page_of(&session.project, episodes)
            ),
            (Some(1), Some(2))
        );

        // A page added later: the shared layers only.
        let added = session.project.add_menu_page(first)?;
        if let Some(menu) = session.project.menu_mut(added) {
            menu.buttons.clear();
        }
        session.project.add_button(
            added,
            "Go".to_owned(),
            None,
            disccraft_project::Link::NotSet,
        )?;
        assert_eq!(
            page_of(&session.project, added),
            Some(disccraft_project::NO_PSD_PAGE)
        );
        assert!(
            !white(at(&session.project, added, 150, 310)?)
                && !white(at(&session.project, added, 150, 390)?)
        );
        assert!(at(&session.project, added, 550, 70)?[0] < 50);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// The button sets of a chapter index Encore writes come in with their kinds (Encore routes `(+>)` / `(+<)` as
    /// next / previous buttons, GitHub issue #13); applying the PSD again keeps a kind set by hand on a `(+)`.
    #[test]
    fn a_psds_chapter_index_buttons_keep_their_kinds() -> Result<(), Box<dyn Error>> {
        use disccraft_photo::PsdLayer;
        use disccraft_project::ButtonKind;
        let dir =
            std::env::temp_dir().join(format!("disccraft-import-kinds-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let (w, h) = (720u32, 480u32);
        let button = |name: &str, x: u32| {
            let mut rgba = vec![0u8; (w * h * 4) as usize];
            for y in 300..340 {
                for x in x..x + 100 {
                    let i = ((y * w + x) * 4) as usize;
                    rgba[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
            PsdLayer::Group {
                name: name.to_owned(),
                children: vec![PsdLayer::Raster {
                    name: "art".to_owned(),
                    rgba,
                    visible: true,
                }],
            }
        };
        let path = dir.join("Index.psd");
        disccraft_photo::write_layered_psd(
            &path,
            w,
            h,
            &[
                button("(+) Play", 20),
                button("(+#) Chapter 1", 140),
                button("(+>) next", 260),
                button("(+<) Previous", 380),
            ],
        )?;
        let path_text = path.to_string_lossy().into_owned();
        let mut session = Session::new();
        let imported = session.execute("menu.import", &serde_json::json!({ "path": path_text }))?;
        let menu = MenuId(value_u64(&imported, "menu")?);
        let kinds = |session: &Session| -> Vec<ButtonKind> {
            session
                .project
                .menu(menu)
                .map(|menu| menu.buttons.iter().map(|button| button.kind).collect())
                .unwrap_or_default()
        };
        assert_eq!(
            kinds(&session),
            [
                ButtonKind::Normal,
                ButtonKind::Chapter,
                ButtonKind::Next,
                ButtonKind::Previous
            ]
        );
        if let Some(play) = session
            .project
            .menu_mut(menu)
            .and_then(|menu| menu.buttons.first_mut())
        {
            play.kind = ButtonKind::Main;
        }
        session.execute(
            "menu.applyPsd",
            &serde_json::json!({ "menu": menu.0, "path": path_text }),
        )?;
        assert_eq!(kinds(&session).first(), Some(&ButtonKind::Main));
        assert_eq!(kinds(&session).get(2), Some(&ButtonKind::Next));
        let _ = std::fs::remove_dir_all(&dir);
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

    /// M6.193 (Encore): a button's text moved out of the button grows the button to take it in (its content keeps
    /// a box of its own) and moved back gives the plain button again; a part moved onto another button is allowed,
    /// as Encore allows it, and the project check reports the overlap (M6.194); a button turned freely covers its
    /// turned content; all undo.
    #[test]
    fn a_button_grows_to_its_moved_or_turned_content() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu = session.project.add_menu(None)?;
        let rect = disccraft_project::Rect {
            x: 100,
            y: 100,
            w: 200,
            h: 60,
        };
        let _ = session
            .project
            .add_button(menu, "Play".to_owned(), Some(rect), Link::NotSet)?;
        let other = disccraft_project::Rect {
            x: 100,
            y: 300,
            w: 200,
            h: 60,
        };
        let _ = session
            .project
            .add_button(menu, "Other".to_owned(), Some(other), Link::NotSet)?;
        let first = |session: &Session| {
            session
                .project
                .menu(menu)
                .and_then(|m| m.buttons.first())
                .cloned()
                .ok_or("no button")
        };
        // Out to the right: the rectangle reaches the text; the content box stays where the button was.
        session.execute(
            "button.setLabelOffset",
            &serde_json::json!({"menu": menu.0, "button": 0, "x": 220, "y": 0}),
        )?;
        let grown = first(&session)?;
        assert_eq!((grown.rect.x, grown.rect.y, grown.rect.h), (100, 100, 60));
        assert!(grown.rect.w > 300, "{:?}", grown.rect);
        assert_eq!(grown.content_box(), rect);
        // Back to its place: the plain button again.
        session.execute(
            "button.setLabelOffset",
            &serde_json::json!({"menu": menu.0, "button": 0, "x": 0, "y": 0}),
        )?;
        let back = first(&session)?;
        assert_eq!((back.rect, back.body), (rect, None));
        // Down onto the other button: allowed (Encore), and the check reports the overlap until it is undone.
        session.execute(
            "button.setLabelOffset",
            &serde_json::json!({"menu": menu.0, "button": 0, "x": 0, "y": 200}),
        )?;
        assert!(first(&session)?.rect.h > 200);
        assert!(
            session
                .project
                .check()
                .iter()
                .any(|problem| problem.code == "buttonsOverlap")
        );
        session.execute("edit.undo", &serde_json::json!({}))?;
        assert_eq!(first(&session)?.rect, rect);
        // Turned a quarter: on its side about (200, 130), 60 wide and 200 high.
        session.execute(
            "menu.setAngle",
            &serde_json::json!({"menu": menu.0, "buttons": [0], "degrees": 90}),
        )?;
        let turned = first(&session)?;
        assert_eq!(
            turned.rect,
            disccraft_project::Rect {
                x: 170,
                y: 30,
                w: 60,
                h: 200
            }
        );
        assert_eq!(turned.content_box(), rect);
        // Moving the turned button moves its content with it.
        session.execute(
            "button.setRect",
            &serde_json::json!({"menu": menu.0, "button": 0, "rect": {"x": 180, "y": 40, "w": 60, "h": 200}}),
        )?;
        assert_eq!(
            first(&session)?.content_box(),
            disccraft_project::Rect {
                x: 110,
                y: 110,
                ..rect
            }
        );
        session.execute("edit.undo", &serde_json::json!({}))?;
        session.execute("edit.undo", &serde_json::json!({}))?;
        assert_eq!((first(&session)?.rect, first(&session)?.body), (rect, None));
        Ok(())
    }

    /// M6.190: the Direct Select tool's commands move a button's text inside the button and place its video
    /// thumbnail inside it; anything leaving the button is refused, and both undo.
    #[test]
    fn a_buttons_text_and_thumbnail_move_inside_it() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let menu = session.project.add_menu(None)?;
        let rect = disccraft_project::Rect {
            x: 100,
            y: 100,
            w: 300,
            h: 100,
        };
        let _ = session
            .project
            .add_button(menu, "Go".to_owned(), Some(rect), Link::NotSet)?;
        let at = |session: &Session| {
            session
                .project
                .menu(menu)
                .and_then(|m| m.buttons.first())
                .map(|b| (b.label_offset, b.thumbnail))
        };
        session.execute(
            "button.setLabelOffset",
            &serde_json::json!({"menu": menu.0, "button": 0, "x": 60, "y": -5}),
        )?;
        assert_eq!(at(&session).map(|a| a.0), Some([60, -5]));
        assert!(
            session
                .execute(
                    "button.setLabelOffset",
                    &serde_json::json!({"menu": menu.0, "button": 0, "x": 900, "y": 0}),
                )
                .is_err()
        );
        session.execute(
            "button.setThumbnailArea",
            &serde_json::json!({"menu": menu.0, "button": 0, "area": {"x": 100, "y": 100, "w": 150, "h": 50}}),
        )?;
        assert_eq!(
            at(&session).and_then(|a| a.1),
            Some(disccraft_project::ThumbnailArea {
                x: 0,
                y: 0,
                w: 5_000,
                h: 5_000
            })
        );
        assert!(
            session
                .execute(
                    "button.setThumbnailArea",
                    &serde_json::json!({"menu": menu.0, "button": 0, "area": {"x": 90, "y": 100, "w": 50, "h": 50}}),
                )
                .is_err()
        );
        session.execute("edit.undo", &serde_json::json!({}))?;
        session.execute("edit.undo", &serde_json::json!({}))?;
        assert_eq!(at(&session), Some(([0, 0], None)));
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

    /// M6.256: Create Subpicture gives a PSD menu's button a highlight in the shape of its artwork; M6.330: a moved
    /// one by its name, and a button made in DiscCraft the shape of its text.
    #[test]
    fn create_subpicture_takes_the_shape_of_a_psd_buttons_artwork() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!(
            "disccraft-create-subpicture-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)?;
        let (w, h) = (720_usize, 480_usize);
        // A button's artwork: a bar over x 100..300, y 200..240 with a hole in its middle (x 180..220).
        let mut art = vec![0u8; w * h * 4];
        for y in 200..240 {
            for x in (100..180).chain(220..300) {
                let i = (y * w + x) * 4;
                if let Some(pixel) = art.get_mut(i..i + 4) {
                    pixel.copy_from_slice(&[240, 200, 40, 255]);
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
                        name: "bar".to_owned(),
                        rgba: art,
                        visible: true,
                    }],
                },
            ],
        )?;
        let mut session = Session::new();
        let imported = session.execute(
            "menu.import",
            &obj(&[("path", Value::String(path.to_string_lossy().into_owned()))]),
        )?;
        let menu_id = MenuId(value_u64(&imported, "menu")?);
        let button = |session: &Session| {
            session
                .project
                .menu(menu_id)
                .and_then(|menu| menu.buttons.first().cloned())
        };
        // No `(=1)` layer: the button has no subpicture of its own (a frame).
        assert!(button(&session).ok_or("button")?.subpicture.is_none());
        let made = session.execute(
            "button.createSubpicture",
            &serde_json::json!({"menu": menu_id.0, "buttons": [0]}),
        )?;
        assert_eq!(made.get("buttons"), Some(&Value::from(1)));
        let subpicture = button(&session)
            .and_then(|button| button.subpicture)
            .ok_or("no subpicture")?;
        assert_eq!((subpicture.width, subpicture.height), (200, 40));
        // Each row: 80 pixels of colour 1, the hole of 40, 80 of colour 1.
        let values = subpicture.values().ok_or("values")?;
        let row = values.get(..200).ok_or("row")?;
        assert!(row.iter().take(80).all(|value| *value == 1));
        assert!(row.iter().skip(80).take(40).all(|value| *value == 0));
        assert!(row.iter().skip(120).all(|value| *value == 1));
        // One step back: the frame again.
        session.execute("edit.undo", &Value::Null)?;
        assert!(button(&session).ok_or("button")?.subpicture.is_none());

        // M6.330: a button that was moved and resized finds its set by name: the same shape, stretched over it.
        session.execute(
            "button.setRect",
            &serde_json::json!({"menu": menu_id.0, "button": 0, "rect": {"x": 300, "y": 100, "w": 400, "h": 40}}),
        )?;
        session.execute(
            "button.createSubpicture",
            &serde_json::json!({"menu": menu_id.0, "buttons": [0]}),
        )?;
        let moved = button(&session).ok_or("button")?;
        assert_eq!(
            moved.subpicture_rect().map(|rect| (rect.w, rect.h)),
            Some((400, 40))
        );
        let values = moved
            .subpicture
            .as_ref()
            .and_then(disccraft_project::Subpicture::values)
            .ok_or("values")?;
        assert_eq!(values.len(), 200 * 40);
        assert_eq!(values.get(100), Some(&0));
        assert_eq!(values.get(10), Some(&1));

        // M6.330: a button made in DiscCraft takes the shape of its text (as Encore makes a `(=1)` of a text
        // button); one with neither text nor a thumbnail has nothing to make one of.
        let plain = value_u64(&session.execute("menu.create", &Value::Null)?, "menu")?;
        session.execute(
            "menu.addButton",
            &serde_json::json!({"menu": plain, "label": "Play", "rect": {"x": 100, "y": 100, "w": 200, "h": 60}}),
        )?;
        session.execute(
            "button.createSubpicture",
            &serde_json::json!({"menu": plain, "buttons": [0]}),
        )?;
        let text = session
            .project
            .menu(MenuId(plain))
            .and_then(|menu| menu.buttons.first().cloned())
            .ok_or("button")?;
        let sub = text.subpicture.as_ref().ok_or("no subpicture")?;
        let lit = sub
            .values()
            .ok_or("values")?
            .iter()
            .filter(|value| **value == 1)
            .count();
        // The letters, not the whole box: some of it lit, most of it not.
        let area = usize::from(sub.width) * usize::from(sub.height);
        assert!(lit > 100 && lit < area / 3, "{lit} of {area}");
        // The same shape as the text subpicture highlights.
        let menu = session.project.menu(MenuId(plain)).ok_or("menu")?.clone();
        let mut by_text = menu.clone();
        if let Some(button) = by_text.buttons.first_mut() {
            button.subpicture = None;
            button.text_subpicture = true;
        }
        let ours = disccraft_menu::render_menu(&session.project, &menu)?.highlight;
        let theirs = disccraft_menu::render_menu(&session.project, &by_text)?.highlight;
        let lit_pixels = |highlight: &disccraft_menu::HighlightLayer| {
            highlight.pixels.iter().filter(|value| **value == 1).count()
        };
        assert_eq!(lit_pixels(&ours), lit_pixels(&theirs));
        session.execute(
            "menu.addButton",
            &serde_json::json!({"menu": plain, "label": "", "rect": {"x": 100, "y": 300, "w": 200, "h": 60}}),
        )?;
        assert!(
            session
                .execute(
                    "button.createSubpicture",
                    &serde_json::json!({"menu": plain, "buttons": [1]}),
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

    /// M6.336: Encore's automatic colour set of a PSD menu: the first button's layer colours are group 1, the next
    /// different ones group 2, a button with a group's colours is in it and any other in group 1 (red, blue, blue,
    /// green: groups 1, 2, 2, 1).
    #[test]
    fn a_psds_buttons_get_highlight_groups_by_their_colours() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-psd-groups-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let (w, h) = (720_usize, 480_usize);
        let group = |index: usize, colour: [u8; 4]| {
            let mut rgba = vec![0u8; w * h * 4];
            for y in 200..220 {
                for x in 20 + index * 150..120 + index * 150 {
                    let i = (y * w + x) * 4;
                    if let Some(pixel) = rgba.get_mut(i..i + 4) {
                        pixel.copy_from_slice(&colour);
                    }
                }
            }
            disccraft_photo::PsdLayer::Group {
                name: format!("(+) B{index}"),
                children: vec![disccraft_photo::PsdLayer::Raster {
                    name: "(=1)".to_owned(),
                    rgba,
                    visible: false,
                }],
            }
        };
        let (red, blue, green) = ([230, 20, 20, 255], [20, 20, 230, 255], [20, 200, 20, 255]);
        let mut layers = vec![disccraft_photo::PsdLayer::Raster {
            name: "背景".to_owned(),
            rgba: [0, 0, 80, 255].repeat(w * h),
            visible: true,
        }];
        layers.extend(
            [red, blue, blue, green]
                .into_iter()
                .enumerate()
                .map(|(i, c)| group(i, c)),
        );
        let path = dir.join("groups.psd");
        disccraft_photo::write_layered_psd(&path, 720, 480, &layers)?;
        let mut session = Session::new();
        let imported = session.execute(
            "menu.import",
            &obj(&[("path", Value::String(path.to_string_lossy().into_owned()))]),
        )?;
        let _ = std::fs::remove_dir_all(&dir);
        let menu = session
            .project
            .menu(MenuId(value_u64(&imported, "menu")?))
            .ok_or("menu")?;
        let groups: Vec<u8> = menu
            .buttons
            .iter()
            .map(|button| button.highlight_group)
            .collect();
        assert_eq!(groups, vec![1, 2, 2, 1]);
        assert_eq!(menu.subpicture_colors, [Some([230, 20, 20]), None, None]);
        assert_eq!(
            menu.second_subpicture_colors,
            [Some([20, 20, 230]), None, None]
        );
        let set = menu.color_set_or_default();
        assert_eq!(set.groups[1].selected_color(1), ([20, 20, 230], 100));
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
