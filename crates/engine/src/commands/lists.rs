//! Slideshow and playlist commands.

use crate::params::{
    opt_str, opt_u64, opt_value, req_link, req_str, req_u64, req_usize, time_param,
};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{
    AssetId, PlaylistItem, SlideScale, SlideTransition, Tick, TimelineId, TransitionKind,
};
use serde::Serialize;
use serde_json::Value;

use super::{always_enabled, empty_object, object};

/// Maximum playlist items accepted by create.
const MAX_PLAYLIST_ITEMS: usize = 99;

/// Slideshow and playlist commands.
pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "slideshow.import",
        label: "スライドショーとして読み込み",
        menu: &["ファイル", "読み込み"],
        shortcut: None,
        params: r#"{"paths": [string]}"#,
        enabled: always_enabled,
        run: run_import_slideshow,
        journal: true,
    },
    CommandSpec {
        id: "slideshow.create",
        label: "スライドショーを作成",
        menu: &["スライドショー"],
        shortcut: None,
        params: r#"{"name"?: string}"#,
        enabled: always_enabled,
        run: run_create_slideshow,
        journal: true,
    },
    CommandSpec {
        id: "slideshow.addSlide",
        label: "スライドを追加",
        menu: &["スライドショー"],
        shortcut: None,
        params: r#"{"slideshow": id, "asset": id, "seconds"?: number}"#,
        enabled: always_enabled,
        run: run_add_slide,
        journal: true,
    },
    CommandSpec {
        id: "slideshow.setSlideDuration",
        label: "スライドのデュレーションを設定",
        menu: &["スライドショー"],
        shortcut: None,
        params: r#"{"slideshow": id, "slide": index, "seconds"?: number, "timecode"?: string, "time"?: ticks}"#,
        enabled: always_enabled,
        run: run_set_slide_duration,
        journal: true,
    },
    CommandSpec {
        id: "slideshow.setSlideScale",
        label: "スライドのスケールを設定",
        menu: &["スライドショー"],
        shortcut: None,
        params: r#"{"slideshow": id, "slide": index, "scale": "scaleAndMatte" | "scaleAndCrop" | "noScale"}"#,
        enabled: always_enabled,
        run: run_set_slide_scale,
        journal: true,
    },
    CommandSpec {
        id: "slideshow.setSlideTransition",
        label: "スライドのトランジションを設定",
        menu: &["スライドショー"],
        shortcut: None,
        params: r#"{"slideshow": id, "slide": index, "kind": "none" | "crossDissolve" | "dipToBlack" | "wipe" | null, "seconds"?: number, "timecode"?: string, "time"?: ticks}"#,
        enabled: always_enabled,
        run: run_set_slide_transition,
        journal: true,
    },
    CommandSpec {
        id: "slideshow.setTransition",
        label: "スライドショーのトランジションを設定",
        menu: &["スライドショー"],
        shortcut: None,
        params: r#"{"slideshow": id, "kind"?: "none" | "crossDissolve" | "dipToBlack" | "wipe", "seconds"?: number, "timecode"?: string, "time"?: ticks}"#,
        enabled: always_enabled,
        run: run_set_transition,
        journal: true,
    },
    CommandSpec {
        id: "slideshow.setManualAdvance",
        label: "スライドショーの手動で進めるを設定",
        menu: &["スライドショー"],
        shortcut: None,
        params: r#"{"slideshow": id, "manual": bool}"#,
        enabled: always_enabled,
        run: run_set_manual_advance,
        journal: true,
    },
    CommandSpec {
        id: "slideshow.setDefaultDuration",
        label: "スライドショーのスライドのデュレーションを設定",
        menu: &["スライドショー"],
        shortcut: None,
        params: r#"{"slideshow": id, "seconds"?: number, "timecode"?: string, "time"?: ticks}"#,
        enabled: always_enabled,
        run: run_set_default_duration,
        journal: true,
    },
    CommandSpec {
        id: "slideshow.removeSlide",
        label: "スライドを削除",
        menu: &["スライドショー"],
        shortcut: None,
        params: r#"{"slideshow": id, "slide": index}"#,
        enabled: always_enabled,
        run: run_remove_slide,
        journal: true,
    },
    CommandSpec {
        id: "slideshow.setAudio",
        label: "音声を設定",
        menu: &["スライドショー"],
        shortcut: None,
        params: r#"{"slideshow": id, "asset"?: id | null}"#,
        enabled: always_enabled,
        run: run_set_audio,
        journal: true,
    },
    CommandSpec {
        id: "slideshow.addAudioClip",
        label: "スライドショーにオーディオを追加",
        menu: &["スライドショー"],
        shortcut: None,
        params: r#"{"slideshow": id, "asset": id}"#,
        enabled: always_enabled,
        run: run_add_audio_clip,
        journal: true,
    },
    CommandSpec {
        id: "slideshow.removeAudioClip",
        label: "スライドショーのオーディオを削除",
        menu: &["スライドショー"],
        shortcut: None,
        params: r#"{"slideshow": id, "index": n}"#,
        enabled: always_enabled,
        run: run_remove_audio_clip,
        journal: true,
    },
    CommandSpec {
        id: "slideshow.setAudioOptions",
        label: "スライドショーのオーディオの設定",
        menu: &["スライドショー"],
        shortcut: None,
        params: r#"{"slideshow": id, "loop"?: bool, "fit"?: bool}"#,
        enabled: always_enabled,
        run: run_set_audio_options,
        journal: true,
    },
    CommandSpec {
        id: "slideshow.setEndAction",
        label: "終了アクションを設定",
        menu: &["スライドショー"],
        shortcut: None,
        params: r#"{"slideshow": id, "target": Link}"#,
        enabled: always_enabled,
        run: run_set_slideshow_end_action,
        journal: true,
    },
    CommandSpec {
        id: "slideshow.list",
        label: "スライドショー一覧",
        menu: &["スライドショー"],
        shortcut: None,
        params: "{}",
        enabled: always_enabled,
        run: run_list_slideshows,
        journal: false,
    },
    CommandSpec {
        id: "playlist.create",
        label: "プレイリストを作成",
        menu: &["プレイリスト"],
        shortcut: None,
        params: r#"{"name"?: string, "items"?: [id]}"#,
        enabled: always_enabled,
        run: run_create_playlist,
        journal: true,
    },
    CommandSpec {
        id: "playlist.addItem",
        label: "項目を追加",
        menu: &["プレイリスト"],
        shortcut: None,
        params: r#"{"playlist": id, "timeline"?: id, "slideshow"?: id, "chapterPlaylist"?: id, "target"?: Link, "at"?: index}"#,
        enabled: always_enabled,
        run: run_add_playlist_item,
        journal: true,
    },
    CommandSpec {
        id: "playlist.moveItem",
        label: "項目を移動",
        menu: &["プレイリスト"],
        shortcut: None,
        params: r#"{"playlist": id, "from": index, "to": index}"#,
        enabled: always_enabled,
        run: run_move_playlist_item,
        journal: true,
    },
    CommandSpec {
        id: "playlist.removeItem",
        label: "項目を削除",
        menu: &["プレイリスト"],
        shortcut: None,
        params: r#"{"playlist": id, "item": index}"#,
        enabled: always_enabled,
        run: run_remove_playlist_item,
        journal: true,
    },
    CommandSpec {
        id: "playlist.setEndAction",
        label: "終了アクションを設定",
        menu: &["プレイリスト"],
        shortcut: None,
        params: r#"{"playlist": id, "target": Link}"#,
        enabled: always_enabled,
        run: run_set_playlist_end_action,
        journal: true,
    },
    CommandSpec {
        id: "playlist.list",
        label: "プレイリスト一覧",
        menu: &["プレイリスト"],
        shortcut: None,
        params: "{}",
        enabled: always_enabled,
        run: run_list_playlists,
        journal: false,
    },
];

/// Encore's 読み込み ▸ スライドショー…: the images become assets and the slides of one new slideshow, in the order
/// of their file names; the slideshow is named after the first one.
fn run_import_slideshow(session: &mut Session, params: &Value) -> CommandResult {
    let paths = super::timeline::req_paths(params)?;
    let mut assets = Vec::with_capacity(paths.len());
    for path in &paths {
        let file = std::path::Path::new(path);
        if !file.is_file() {
            return Err(format!("slideshow.import: file does not exist: {path}"));
        }
        let asset = session
            .project
            .add_asset(file)
            .map_err(|error| error.to_string())?;
        let kind = session.project.assets.get(&asset).map(|asset| asset.kind);
        if kind != Some(disccraft_project::AssetKind::Image) {
            return Err(format!("slideshow.import: not an image: {path}"));
        }
        assets.push(asset);
    }
    let name = assets
        .first()
        .and_then(|asset| session.project.assets.get(asset))
        .map(|asset| asset.name.clone());
    let id = session
        .project
        .add_slideshow(name)
        .map_err(|error| error.to_string())?;
    let duration = session
        .project
        .slideshow(id)
        .map(|slideshow| slideshow.slide_duration)
        .ok_or_else(|| format!("slideshow {id} does not exist"))?;
    for asset in &assets {
        let _ = session
            .project
            .add_slide(id, *asset, duration)
            .map_err(|error| error.to_string())?;
    }
    Ok(object(&[
        ("slideshow", Value::from(id)),
        (
            "assets",
            Value::Array(assets.iter().map(|asset| Value::from(asset.0)).collect()),
        ),
    ]))
}

fn run_create_slideshow(session: &mut Session, params: &Value) -> CommandResult {
    let name = opt_str(params, "name")?;
    let id = session
        .project
        .add_slideshow(name)
        .map_err(|error| error.to_string())?;

    Ok(object(&[("slideshow", Value::from(id))]))
}

fn run_add_slide(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "slideshow")?;
    let asset = req_u64(params, "asset")?;
    // Without "seconds", the slideshow's own slide duration.
    let duration = match opt_value(params, "seconds") {
        Some(value) => seconds_to_tick(value)?,
        None => session
            .project
            .slideshow(id)
            .map(|slideshow| slideshow.slide_duration)
            .ok_or_else(|| format!("slideshow {id} does not exist"))?,
    };

    let index = session
        .project
        .add_slide(id, AssetId(asset), duration)
        .map_err(|error| error.to_string())?;

    let slide = u64::try_from(index).map_err(|_| "slide index is too large".to_string())?;
    Ok(object(&[("slide", Value::from(slide))]))
}

/// A duration from "seconds" (number), "timecode" (string) or "time" (ticks).
fn duration_param(session: &Session, params: &Value) -> Result<Tick, String> {
    match opt_value(params, "seconds") {
        Some(value) => seconds_to_tick(value),
        None => time_param(params, &session.project.settings),
    }
}

fn run_set_slide_duration(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "slideshow")?;
    let slide = req_usize(params, "slide")?;
    let duration = duration_param(session, params)?;
    session
        .project
        .set_slide_duration(id, slide, duration)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

fn run_set_default_duration(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "slideshow")?;
    let duration = duration_param(session, params)?;
    session
        .project
        .set_slideshow_slide_duration(id, duration)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

fn run_set_slide_scale(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "slideshow")?;
    let slide = req_usize(params, "slide")?;
    let scale = SlideScale::parse(&req_str(params, "scale")?).ok_or_else(|| {
        "parameter \"scale\" must be \"scaleAndMatte\", \"scaleAndCrop\" or \"noScale\"".to_string()
    })?;
    session
        .project
        .set_slide_scale(id, slide, scale)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

/// One slide's own transition. `"kind": null` (or no kind) makes the slide follow the show again; without a
/// duration the slide keeps its own, or takes the show's.
fn run_set_slide_transition(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "slideshow")?;
    let slide = req_usize(params, "slide")?;
    let (show_duration, current) = session
        .project
        .slideshow(id)
        .map(|show| {
            (
                show.transition_duration,
                show.slides.get(slide).and_then(|slide| slide.transition),
            )
        })
        .ok_or_else(|| format!("slideshow {id} does not exist"))?;
    let transition = match opt_str(params, "kind")? {
        None => None,
        Some(text) => {
            let kind = TransitionKind::parse(&text).ok_or_else(|| {
                "parameter \"kind\" must be \"none\", \"crossDissolve\", \"dipToBlack\" or \"wipe\""
                    .to_string()
            })?;
            let given = ["seconds", "timecode", "time"]
                .iter()
                .any(|name| params.get(*name).is_some_and(|value| !value.is_null()));
            let duration = if given {
                duration_param(session, params)?
            } else {
                current.map_or(show_duration, |transition| transition.duration)
            };
            Some(SlideTransition { kind, duration })
        }
    };
    session
        .project
        .set_slide_transition(id, slide, transition)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

/// The transition and its length; a missing "kind" keeps the kind, a missing duration keeps the length.
fn run_set_transition(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "slideshow")?;
    let current = session
        .project
        .slideshow(id)
        .map(|slideshow| (slideshow.transition, slideshow.transition_duration))
        .ok_or_else(|| format!("slideshow {id} does not exist"))?;
    let kind = match opt_str(params, "kind")? {
        Some(text) => TransitionKind::parse(&text).ok_or_else(|| {
            "parameter \"kind\" must be \"none\", \"crossDissolve\", \"dipToBlack\" or \"wipe\""
                .to_string()
        })?,
        None => current.0,
    };
    let given = ["seconds", "timecode", "time"]
        .iter()
        .any(|name| params.get(*name).is_some_and(|value| !value.is_null()));
    let duration = if given {
        duration_param(session, params)?
    } else {
        current.1
    };
    session
        .project
        .set_transition(id, kind, duration)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

fn run_set_manual_advance(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "slideshow")?;
    let manual = params
        .get("manual")
        .and_then(Value::as_bool)
        .ok_or_else(|| "parameter \"manual\" must be a boolean".to_string())?;
    session
        .project
        .set_manual_advance(id, manual)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

fn run_remove_slide(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "slideshow")?;
    let slide = req_usize(params, "slide")?;

    session
        .project
        .remove_slide(id, slide)
        .map_err(|error| error.to_string())?;

    Ok(empty_object())
}

fn run_set_audio(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "slideshow")?;
    let asset = opt_u64(params, "asset")?;

    let Some(slideshow) = session.project.slideshow_mut(id) else {
        return Err(format!("slideshow {id} does not exist"));
    };

    slideshow.audio = asset.map(AssetId);
    // One asset replaces the whole list.
    slideshow.audio_clips.clear();
    Ok(empty_object())
}

fn run_add_audio_clip(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "slideshow")?;
    let asset = req_u64(params, "asset")?;
    let index = session
        .project
        .add_slideshow_audio(id, AssetId(asset))
        .map_err(|error| error.to_string())?;
    let index = u64::try_from(index).map_err(|_| "audio clip index is too large".to_string())?;
    Ok(object(&[("index", Value::from(index))]))
}

fn run_remove_audio_clip(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "slideshow")?;
    let index = req_usize(params, "index")?;
    session
        .project
        .remove_slideshow_audio(id, index)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

/// Encore's "Loop Audio" and "Fit Slideshow to Audio Duration".
fn run_set_audio_options(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "slideshow")?;
    let flag = |name: &str| -> Result<Option<bool>, String> {
        match params.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(flag)) => Ok(Some(*flag)),
            Some(_) => Err(format!("parameter \"{name}\" must be a boolean")),
        }
    };
    let looped = flag("loop")?;
    let fit = flag("fit")?;
    let Some(slideshow) = session.project.slideshow_mut(id) else {
        return Err(format!("slideshow {id} does not exist"));
    };
    if let Some(looped) = looped {
        slideshow.loop_audio = looped;
    }
    if let Some(fit) = fit {
        slideshow.fit_to_audio = fit;
    }
    Ok(empty_object())
}

fn run_set_slideshow_end_action(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "slideshow")?;
    let target = req_link(params, "target")?;

    let Some(slideshow) = session.project.slideshow_mut(id) else {
        return Err(format!("slideshow {id} does not exist"));
    };

    slideshow.end_action = target;
    Ok(empty_object())
}

fn run_list_slideshows(session: &mut Session, _params: &Value) -> CommandResult {
    let mut list = Vec::new();

    for slideshow in session.project.slideshows.values() {
        list.push(to_json_value(slideshow)?);
    }

    Ok(Value::Array(list))
}

fn run_create_playlist(session: &mut Session, params: &Value) -> CommandResult {
    let name = opt_str(params, "name")?;
    let items = parse_timeline_ids(params)?;

    if items.len() > MAX_PLAYLIST_ITEMS {
        return Err("playlist has too many items".to_string());
    }

    for item in &items {
        if !session.project.timelines.contains_key(&TimelineId(*item)) {
            return Err(format!("timeline {item} does not exist"));
        }
    }

    let id = session
        .project
        .add_playlist(name)
        .map_err(|error| error.to_string())?;

    for item in items {
        session
            .project
            .add_playlist_item(id, TimelineId(item))
            .map_err(|error| error.to_string())?;
    }

    Ok(object(&[("playlist", Value::from(id))]))
}

/// The item of `playlist.addItem`: one of "timeline" / "slideshow" / "chapterPlaylist", or "target" (a link
/// that plays a title, as the pick whip and the link menu give it).
fn playlist_item_param(params: &Value) -> Result<PlaylistItem, String> {
    if let Some(timeline) = opt_u64(params, "timeline")? {
        return Ok(PlaylistItem::Timeline(TimelineId(timeline)));
    }
    if let Some(slideshow) = opt_u64(params, "slideshow")? {
        return Ok(PlaylistItem::Slideshow(slideshow));
    }
    if let Some(list) = opt_u64(params, "chapterPlaylist")? {
        return Ok(PlaylistItem::ChapterPlaylist(list));
    }
    if opt_value(params, "target").is_some() {
        let link = req_link(params, "target")?;
        return PlaylistItem::from_link(&link).ok_or_else(|| {
            "parameter \"target\" must play a timeline, slideshow or chapter playlist".to_string()
        });
    }
    Err(
        "one of \"timeline\", \"slideshow\", \"chapterPlaylist\" or \"target\" is required"
            .to_string(),
    )
}

fn run_add_playlist_item(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "playlist")?;
    let item = playlist_item_param(params)?;
    let at = opt_u64(params, "at")?
        .map(|at| usize::try_from(at).map_err(|_| "parameter \"at\" is too large".to_string()))
        .transpose()?;

    let index = session
        .project
        .add_playlist_entry(id, item, at)
        .map_err(|error| error.to_string())?;

    let item = u64::try_from(index).map_err(|_| "item index is too large".to_string())?;
    Ok(object(&[("item", Value::from(item))]))
}

fn run_move_playlist_item(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "playlist")?;
    let from = req_usize(params, "from")?;
    let to = req_usize(params, "to")?;
    session
        .project
        .move_playlist_item(id, from, to)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

fn run_remove_playlist_item(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "playlist")?;
    let item = req_usize(params, "item")?;

    session
        .project
        .remove_playlist_item(id, item)
        .map_err(|error| error.to_string())?;

    Ok(empty_object())
}

fn run_set_playlist_end_action(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "playlist")?;
    let target = req_link(params, "target")?;

    let Some(playlist) = session.project.playlist_mut(id) else {
        return Err(format!("playlist {id} does not exist"));
    };

    playlist.end_action = target;
    Ok(empty_object())
}

fn run_list_playlists(session: &mut Session, _params: &Value) -> CommandResult {
    let mut list = Vec::new();

    for playlist in session.project.playlists.values() {
        list.push(to_json_value(playlist)?);
    }

    Ok(Value::Array(list))
}

/// Seconds (JSON number) → ticks, through whole milliseconds (254_016_000 ticks per millisecond).
fn seconds_to_tick(value: &Value) -> Result<Tick, String> {
    let seconds = finite_seconds(value)?;
    if !(0.0..=86_400.0).contains(&seconds) {
        return Err("parameter \"seconds\" must be between 0 and 86400".to_string());
    }
    // In range, so the cast is exact for whole milliseconds.
    let millis = (seconds * 1000.0).round() as i64;
    millis
        .checked_mul(254_016_000)
        .map(Tick)
        .ok_or_else(|| "parameter \"seconds\" is too large".to_string())
}

fn finite_seconds(value: &Value) -> Result<f64, String> {
    let seconds = value
        .as_f64()
        .ok_or_else(|| "parameter \"seconds\" must be a number".to_string())?;

    if !seconds.is_finite() {
        return Err("parameter \"seconds\" must be a finite number".to_string());
    }

    if seconds < 0.0 {
        return Err("parameter \"seconds\" must not be negative".to_string());
    }

    Ok(seconds)
}

fn parse_timeline_ids(params: &Value) -> Result<Vec<u64>, String> {
    match opt_value(params, "items") {
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| value_u64(value, "items"))
            .collect(),
        Some(_) => Err("parameter \"items\" must be an array".to_string()),
        None => Ok(Vec::new()),
    }
}

fn value_u64(value: &Value, name: &str) -> Result<u64, String> {
    value
        .as_u64()
        .ok_or_else(|| format!("parameter \"{name}\" must contain non-negative integers"))
}

fn to_json_value<T: Serialize>(value: &T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use disccraft_project::{
        PlaylistItem, SlideScale, SlideTransition, Tick, TransitionKind, has_errors,
    };
    use serde_json::{Map, Value};
    use std::error::Error;
    use std::fs::{self, File};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

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

    fn create_temp_dir() -> Result<PathBuf, Box<dyn Error>> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path =
            std::env::temp_dir().join(format!("disccraft-m7-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    fn run_slideshow_and_playlist(dir: &Path) -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();

        let slide1 = dir.join("slide1.png");
        let slide2 = dir.join("slide2.png");
        File::create(&slide1)?;
        File::create(&slide2)?;

        let asset1 = session.project.add_asset(&slide1)?;
        let asset2 = session.project.add_asset(&slide2)?;

        let video1 = dir.join("movie1.mp4");
        let video2 = dir.join("movie2.mp4");
        File::create(&video1)?;
        File::create(&video2)?;

        let video_asset1 = session.project.add_asset(&video1)?;
        let timeline1 = session.project.add_timeline(video_asset1, None)?;
        let video_asset2 = session.project.add_asset(&video2)?;
        let timeline2 = session.project.add_timeline(video_asset2, None)?;

        let created = session.execute("slideshow.create", &obj(&[]))?;
        let slideshow = value_u64(&created, "slideshow")?;

        session.execute(
            "slideshow.addSlide",
            &obj(&[
                ("slideshow", Value::from(slideshow)),
                ("asset", Value::from(asset1.0)),
                ("seconds", Value::from(2)),
            ]),
        )?;

        session.execute(
            "slideshow.addSlide",
            &obj(&[
                ("slideshow", Value::from(slideshow)),
                ("asset", Value::from(asset2.0)),
            ]),
        )?;

        let playlist_created = session.execute(
            "playlist.create",
            &obj(&[(
                "items",
                Value::Array(vec![Value::from(timeline1.0), Value::from(timeline2.0)]),
            )]),
        )?;

        let playlist = value_u64(&playlist_created, "playlist")?;

        // A slideshow and a chapter link join the timelines; the slideshow is moved to the front.
        session.execute(
            "playlist.addItem",
            &obj(&[
                ("playlist", Value::from(playlist)),
                ("slideshow", Value::from(slideshow)),
            ]),
        )?;
        session.execute(
            "playlist.addItem",
            &obj(&[
                ("playlist", Value::from(playlist)),
                (
                    "target",
                    serde_json::json!({ "timeline": timeline1.0, "chapter": 0 }),
                ),
                ("at", Value::from(0)),
            ]),
        )?;
        session.execute(
            "playlist.moveItem",
            &obj(&[
                ("playlist", Value::from(playlist)),
                ("from", Value::from(3)),
                ("to", Value::from(0)),
            ]),
        )?;
        let items = session
            .project
            .playlist(playlist)
            .map(|list| list.items.clone())
            .ok_or("missing playlist")?;
        assert_eq!(
            items,
            vec![
                PlaylistItem::Slideshow(slideshow),
                PlaylistItem::Timeline(timeline1),
                PlaylistItem::Timeline(timeline1),
                PlaylistItem::Timeline(timeline2),
            ]
        );
        assert!(
            session
                .execute(
                    "playlist.addItem",
                    &obj(&[
                        ("playlist", Value::from(playlist)),
                        ("target", Value::from("stop")),
                    ]),
                )
                .is_err()
        );

        let problems = session.project.check();
        assert!(!has_errors(&problems));

        Ok(())
    }

    #[test]
    fn audio_clips_are_listed_and_undone() -> Result<(), Box<dyn Error>> {
        let dir = create_temp_dir()?;
        let first = dir.join("a.wav");
        let second = dir.join("b.wav");
        File::create(&first)?;
        File::create(&second)?;
        let mut session = Session::new();
        let a = session.project.add_asset(&first)?;
        let b = session.project.add_asset(&second)?;
        let slideshow = session.project.add_slideshow(None)?;
        // An older single audio becomes the first clip when a second one is added.
        session.execute(
            "slideshow.setAudio",
            &obj(&[
                ("slideshow", Value::from(slideshow)),
                ("asset", Value::from(a.0)),
            ]),
        )?;
        session.execute(
            "slideshow.addAudioClip",
            &obj(&[
                ("slideshow", Value::from(slideshow)),
                ("asset", Value::from(b.0)),
            ]),
        )?;
        let clips = |session: &Session| {
            session
                .project
                .slideshow(slideshow)
                .map(|show| show.audio_list())
                .unwrap_or_default()
        };
        assert_eq!(clips(&session), vec![a, b]);
        session.execute(
            "slideshow.setAudioOptions",
            &obj(&[
                ("slideshow", Value::from(slideshow)),
                ("loop", Value::Bool(true)),
                ("fit", Value::Bool(true)),
            ]),
        )?;
        let show = session.project.slideshow(slideshow).ok_or("missing")?;
        assert!(show.loop_audio && show.fit_to_audio);
        session.execute(
            "slideshow.removeAudioClip",
            &obj(&[
                ("slideshow", Value::from(slideshow)),
                ("index", Value::from(0)),
            ]),
        )?;
        assert_eq!(clips(&session), vec![b]);
        assert_eq!(
            session
                .project
                .slideshow(slideshow)
                .and_then(|show| show.audio),
            Some(b)
        );
        session.undo()?;
        assert_eq!(clips(&session), vec![a, b]);
        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn slide_scales_are_set_and_undone() -> Result<(), Box<dyn Error>> {
        let dir = create_temp_dir()?;
        let image = dir.join("slide.png");
        File::create(&image)?;
        let mut session = Session::new();
        let asset = session.project.add_asset(&image)?;
        let slideshow = session.project.add_slideshow(None)?;
        session
            .project
            .add_slide(slideshow, asset, Tick(254_016_000_000))?;
        let current = |session: &Session| {
            session
                .project
                .slideshow(slideshow)
                .and_then(|show| show.slides.first())
                .map(|slide| slide.scale)
        };
        session.execute(
            "slideshow.setSlideScale",
            &obj(&[
                ("slideshow", Value::from(slideshow)),
                ("slide", Value::from(0)),
                ("scale", Value::from("scaleAndCrop")),
            ]),
        )?;
        assert_eq!(current(&session), Some(SlideScale::ScaleAndCrop));
        session.undo()?;
        assert_eq!(current(&session), Some(SlideScale::ScaleAndMatte));

        // A slide's own transition: the show's length unless given; null follows the show again.
        let own = |session: &Session| {
            session
                .project
                .slideshow(slideshow)
                .and_then(|show| show.slides.first())
                .and_then(|slide| slide.transition)
        };
        session.execute(
            "slideshow.setSlideTransition",
            &obj(&[
                ("slideshow", Value::from(slideshow)),
                ("slide", Value::from(0)),
                ("kind", Value::from("wipe")),
            ]),
        )?;
        let show_length = session
            .project
            .slideshow(slideshow)
            .map(|show| show.transition_duration)
            .ok_or("slideshow")?;
        assert_eq!(
            own(&session),
            Some(SlideTransition {
                kind: TransitionKind::Wipe,
                duration: show_length,
            })
        );
        session.execute(
            "slideshow.setSlideTransition",
            &obj(&[
                ("slideshow", Value::from(slideshow)),
                ("slide", Value::from(0)),
                ("kind", Value::from("wipe")),
                ("seconds", Value::from(1)),
            ]),
        )?;
        assert_eq!(
            own(&session).map(|t| t.duration),
            Some(Tick(254_016_000_000))
        );
        session.execute(
            "slideshow.setSlideTransition",
            &obj(&[
                ("slideshow", Value::from(slideshow)),
                ("slide", Value::from(0)),
                ("kind", Value::Null),
            ]),
        )?;
        assert_eq!(own(&session), None);
        session.undo()?;
        assert!(own(&session).is_some());
        assert!(
            session
                .execute(
                    "slideshow.setSlideScale",
                    &obj(&[
                        ("slideshow", Value::from(slideshow)),
                        ("slide", Value::from(0)),
                        ("scale", Value::from("stretch")),
                    ]),
                )
                .is_err()
        );
        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn transitions_are_set_and_undone() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let created = session.execute("slideshow.create", &obj(&[]))?;
        let slideshow = value_u64(&created, "slideshow")?;
        let current = |session: &Session| {
            session
                .project
                .slideshow(slideshow)
                .map(|show| (show.transition, show.transition_duration))
        };
        session.execute(
            "slideshow.setTransition",
            &obj(&[
                ("slideshow", Value::from(slideshow)),
                ("kind", Value::from("crossDissolve")),
                ("seconds", Value::from(1)),
            ]),
        )?;
        assert_eq!(
            current(&session),
            Some((TransitionKind::CrossDissolve, Tick(254_016_000_000)))
        );
        // Only the kind: the length stays.
        session.execute(
            "slideshow.setTransition",
            &obj(&[
                ("slideshow", Value::from(slideshow)),
                ("kind", Value::from("wipe")),
            ]),
        )?;
        assert_eq!(
            current(&session),
            Some((TransitionKind::Wipe, Tick(254_016_000_000)))
        );
        session.undo()?;
        assert_eq!(
            current(&session),
            Some((TransitionKind::CrossDissolve, Tick(254_016_000_000)))
        );
        session.execute(
            "slideshow.setManualAdvance",
            &obj(&[
                ("slideshow", Value::from(slideshow)),
                ("manual", Value::Bool(true)),
            ]),
        )?;
        assert_eq!(
            session
                .project
                .slideshow(slideshow)
                .map(|show| show.manual_advance),
            Some(true)
        );
        session.undo()?;
        assert_eq!(
            session
                .project
                .slideshow(slideshow)
                .map(|show| show.manual_advance),
            Some(false)
        );
        for params in [
            obj(&[
                ("slideshow", Value::from(slideshow)),
                ("kind", Value::from("spin")),
            ]),
            obj(&[
                ("slideshow", Value::from(slideshow)),
                ("seconds", Value::from(60)),
            ]),
        ] {
            assert!(session.execute("slideshow.setTransition", &params).is_err());
        }
        Ok(())
    }

    #[test]
    fn slideshow_and_playlist_commands() -> Result<(), Box<dyn Error>> {
        let dir = create_temp_dir()?;
        let result = run_slideshow_and_playlist(&dir);
        let _ = fs::remove_dir_all(&dir);
        result?;
        Ok(())
    }

    #[test]
    fn unknown_asset_rejected() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let created = session.execute("slideshow.create", &obj(&[]))?;
        let slideshow = value_u64(&created, "slideshow")?;

        let result = session.execute(
            "slideshow.addSlide",
            &obj(&[
                ("slideshow", Value::from(slideshow)),
                ("asset", Value::from(999_999)),
                ("seconds", Value::from(5)),
            ]),
        );

        assert!(result.is_err());
        Ok(())
    }
}
