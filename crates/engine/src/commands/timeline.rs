use crate::params::{opt_str, req_link, req_str, req_u64, req_usize, time_param};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{
    AssetId, AssetKind, AudioTrack, DiscKind, Link, SubtitleTrack, TimelineId, format_time,
};
use serde_json::{Map, Value};

use super::{always_enabled, empty_object, object};

const DVD_MAX_AUDIO_TRACKS: usize = 8;

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "timeline.create",
        label: "タイムラインを作成",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"asset": id, "name"?: string}"#,
        enabled: always_enabled,
        run: run_create,
        journal: true,
    },
    CommandSpec {
        id: "timeline.addChapter",
        label: "チャプターを追加",
        menu: &["タイムライン"],
        shortcut: Some("C"),
        params: r#"{"timeline": id, "timecode"?: "HH:MM:SS;FF", "time"?: ticks, "name"?: string}"#,
        enabled: always_enabled,
        run: run_add_chapter,
        journal: true,
    },
    CommandSpec {
        id: "timeline.removeChapter",
        label: "チャプターを削除",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "chapter": index}"#,
        enabled: always_enabled,
        run: run_remove_chapter,
        journal: true,
    },
    CommandSpec {
        id: "timeline.moveChapter",
        label: "チャプターを移動",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "chapter": index, "timecode"?: "HH:MM:SS;FF", "time"?: ticks}"#,
        enabled: always_enabled,
        run: run_move_chapter,
        journal: true,
    },
    CommandSpec {
        id: "timeline.renameChapter",
        label: "チャプターの名前を変更",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "chapter": index, "name": string}"#,
        enabled: always_enabled,
        run: run_rename_chapter,
        journal: true,
    },
    CommandSpec {
        id: "timeline.setPosterFrame",
        label: "ポスターフレームを設定",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "chapter": index, "time"?: ticks | null, "timecode"?: string}"#,
        enabled: always_enabled,
        run: run_set_poster_frame,
        journal: true,
    },
    CommandSpec {
        id: "timeline.setEndAction",
        label: "終了アクションを設定",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "target": Link}"#,
        enabled: always_enabled,
        run: run_set_end_action,
        journal: true,
    },
    CommandSpec {
        id: "timeline.setMenuRemote",
        label: "リモートメニューを設定",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "target": Link}"#,
        enabled: always_enabled,
        run: run_set_menu_remote,
        journal: true,
    },
    CommandSpec {
        id: "timeline.list",
        label: "タイムライン一覧",
        menu: &["タイムライン"],
        shortcut: None,
        params: "{}",
        enabled: always_enabled,
        run: run_list,
        journal: false,
    },
    CommandSpec {
        id: "timeline.addAudioTrack",
        label: "オーディオトラックを追加",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "asset": id, "language"?: string}"#,
        enabled: always_enabled,
        run: run_add_audio_track,
        journal: true,
    },
    CommandSpec {
        id: "timeline.addSubtitleTrack",
        label: "字幕トラックを追加",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "asset": id (.srt), "language"?: string}"#,
        enabled: always_enabled,
        run: run_add_subtitle_track,
        journal: true,
    },
    CommandSpec {
        id: "timeline.removeSubtitleTrack",
        label: "字幕トラックを削除",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "track": index}"#,
        enabled: always_enabled,
        run: run_remove_subtitle_track,
        journal: true,
    },
    CommandSpec {
        id: "timeline.setAspect",
        label: "タイムラインの縦横比を設定",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "aspect": "4:3" | "16:9" | null (the disc's)}"#,
        enabled: always_enabled,
        run: run_set_aspect,
        journal: true,
    },
    CommandSpec {
        id: "timeline.setOverride",
        label: "上書きを設定",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "target": Link | null}"#,
        enabled: always_enabled,
        run: run_set_override,
        journal: true,
    },
    CommandSpec {
        id: "timeline.setUserOps",
        label: "ユーザー操作を設定",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "prohibited": ["fastForward", ...]}"#,
        enabled: always_enabled,
        run: run_set_user_ops,
        journal: true,
    },
    CommandSpec {
        id: "timeline.setTrackLanguage",
        label: "トラックの言語を設定",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "kind": "audio"|"subtitle", "track": index, "language": "ja" (ISO 639-1, or "")}"#,
        enabled: always_enabled,
        run: run_set_track_language,
        journal: true,
    },
    CommandSpec {
        id: "timeline.removeAudioTrack",
        label: "オーディオトラックを削除",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "track": index}"#,
        enabled: always_enabled,
        run: run_remove_audio_track,
        journal: true,
    },
];

fn run_create(session: &mut Session, params: &Value) -> CommandResult {
    let asset = AssetId(req_u64(params, "asset")?);
    let name = opt_str(params, "name")?;
    let id = session
        .project
        .add_timeline(asset, name)
        .map_err(|error| error.to_string())?;

    Ok(object(&[("timeline", Value::from(id.0))]))
}

fn run_add_chapter(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let time = time_param(params, &session.project.settings)?;
    let name = opt_str(params, "name")?;

    let index = session
        .project
        .add_chapter(timeline, time, name)
        .map_err(|error| error.to_string())?;

    let chapter = u64::try_from(index).map_err(|_| "chapter index is too large".to_string())?;
    let timecode = format_time(time, &session.project.settings);

    Ok(object(&[
        ("chapter", Value::from(chapter)),
        ("time", Value::from(time.0)),
        ("timecode", Value::String(timecode)),
    ]))
}

fn run_remove_chapter(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let chapter = req_usize(params, "chapter")?;

    session
        .project
        .remove_chapter(timeline, chapter)
        .map_err(|error| error.to_string())?;

    Ok(empty_object())
}

fn run_move_chapter(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let chapter = req_usize(params, "chapter")?;
    let time = time_param(params, &session.project.settings)?;

    let index = session
        .project
        .move_chapter(timeline, chapter, time)
        .map_err(|error| error.to_string())?;

    let new_index = u64::try_from(index).map_err(|_| "chapter index is too large".to_string())?;
    Ok(object(&[("chapter", Value::from(new_index))]))
}

fn run_rename_chapter(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let chapter = req_usize(params, "chapter")?;
    let name = req_str(params, "name")?;

    session
        .project
        .rename_chapter(timeline, chapter, &name)
        .map_err(|error| error.to_string())?;

    Ok(empty_object())
}

/// Set a chapter's poster frame; without "time" / "timecode" (or with `"time": null`) it goes back to the
/// chapter's own frame.
fn run_set_poster_frame(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let chapter = req_usize(params, "chapter")?;
    let given = ["time", "timecode"]
        .iter()
        .any(|name| params.get(*name).is_some_and(|value| !value.is_null()));
    let poster = if given {
        Some(time_param(params, &session.project.settings)?)
    } else {
        None
    };

    session
        .project
        .set_poster_frame(timeline, chapter, poster)
        .map_err(|error| error.to_string())?;

    Ok(empty_object())
}

fn run_set_end_action(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let target = req_link(params, "target")?;

    let Some(timeline) = session.project.timeline_mut(timeline) else {
        return Err(format!("timeline {} does not exist", timeline.0));
    };

    timeline.end_action = target;
    Ok(empty_object())
}

/// Where the remote's MENU key goes while the timeline plays (DVD: the title set's root menu).
fn run_set_menu_remote(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let target = req_link(params, "target")?;

    let Some(timeline) = session.project.timeline_mut(timeline) else {
        return Err(format!("timeline {} does not exist", timeline.0));
    };

    timeline.menu_remote = target;
    Ok(empty_object())
}

fn run_list(session: &mut Session, _params: &Value) -> CommandResult {
    let settings = &session.project.settings;
    let mut timelines = Vec::new();

    for timeline in session.project.timelines.values() {
        let mut chapters = Vec::new();

        for chapter in &timeline.chapters {
            let mut chapter_map = Map::new();
            chapter_map.insert("time".to_string(), Value::from(chapter.time.0));
            chapter_map.insert(
                "timecode".to_string(),
                Value::String(format_time(chapter.time, settings)),
            );
            chapter_map.insert("name".to_string(), Value::String(chapter.name.clone()));
            chapters.push(Value::Object(chapter_map));
        }

        let mut map = Map::new();
        map.insert("id".to_string(), Value::from(timeline.id.0));
        map.insert("name".to_string(), Value::String(timeline.name.clone()));
        map.insert("video".to_string(), Value::from(timeline.video.0));
        map.insert("chapters".to_string(), Value::Array(chapters));
        timelines.push(Value::Object(map));
    }

    Ok(Value::Array(timelines))
}

fn run_add_subtitle_track(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let asset = AssetId(req_u64(params, "asset")?);
    let language = opt_str(params, "language")?.unwrap_or_default();
    match session.project.assets.get(&asset) {
        Some(a) if a.kind == AssetKind::Subtitle => {}
        Some(_) => return Err(format!("asset {} is not a subtitle (.srt) asset", asset.0)),
        None => return Err(format!("asset {} does not exist", asset.0)),
    }
    let Some(timeline) = session.project.timeline_mut(timeline) else {
        return Err(format!("timeline {} does not exist", timeline.0));
    };
    if timeline.subtitles.len() >= 32 {
        return Err("DVD timelines support at most 32 subtitle tracks".to_string());
    }
    timeline.subtitles.push(SubtitleTrack { asset, language });
    let index = u64::try_from(timeline.subtitles.len().saturating_sub(1))
        .map_err(|_| "subtitle track index is too large".to_string())?;
    Ok(object(&[("track", Value::from(index))]))
}

fn run_remove_subtitle_track(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let track = req_usize(params, "track")?;
    let Some(timeline) = session.project.timeline_mut(timeline) else {
        return Err(format!("timeline {} does not exist", timeline.0));
    };
    if track >= timeline.subtitles.len() {
        return Err(format!("subtitle track {track} does not exist"));
    }
    timeline.subtitles.remove(track);
    Ok(object(&[]))
}

fn run_add_audio_track(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let asset = AssetId(req_u64(params, "asset")?);
    let language = opt_str(params, "language")?.unwrap_or_default();

    let (asset_id, asset_kind) = match session.project.assets.get(&asset) {
        Some(asset) => (asset.id, asset.kind),
        None => return Err(format!("asset {} does not exist", asset.0)),
    };

    if asset_kind != AssetKind::Audio {
        return Err(format!("asset {} is not an audio asset", asset_id.0));
    }

    let is_dvd = session.project.settings.kind == DiscKind::Dvd;
    let Some(timeline) = session.project.timeline_mut(timeline) else {
        return Err(format!("timeline {} does not exist", timeline.0));
    };

    if is_dvd && timeline.audio.len() >= DVD_MAX_AUDIO_TRACKS {
        return Err("DVD timelines support at most 8 audio tracks".to_string());
    }

    timeline.audio.push(AudioTrack {
        asset: asset_id,
        language,
    });

    let index = u64::try_from(timeline.audio.len().saturating_sub(1))
        .map_err(|_| "audio track index is too large".to_string())?;
    Ok(object(&[("track", Value::from(index))]))
}

/// The timeline's own aspect ratio (`null`: follow the disc).
fn run_set_aspect(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let aspect = match params.get("aspect") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if text == "4:3" => Some(false),
        Some(Value::String(text)) if text == "16:9" => Some(true),
        Some(_) => return Err("parameter \"aspect\" must be \"4:3\", \"16:9\" or null".to_string()),
    };
    let Some(timeline) = session.project.timeline_mut(timeline) else {
        return Err(format!("timeline {} does not exist", timeline.0));
    };
    timeline.widescreen = aspect;
    Ok(empty_object())
}

/// Set where to go when the title that the end action leads to has finished (Encore's Override). `null` or a
/// not-set link removes it.
fn run_set_override(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let target = match params.get("target") {
        None | Some(Value::Null) => None,
        Some(_) => Some(req_link(params, "target")?).filter(|link| !matches!(link, Link::NotSet)),
    };
    if let Some(link) = &target
        && !session.project.link_resolves(link)
    {
        return Err("the override target does not exist".to_string());
    }
    let Some(timeline) = session.project.timeline_mut(timeline) else {
        return Err(format!("timeline {} does not exist", timeline.0));
    };
    timeline.override_link = target;
    Ok(empty_object())
}

/// Set the remote operations a timeline prohibits (an empty list allows everything).
fn run_set_user_ops(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let ops = super::user_ops_param(params)?;
    let Some(timeline) = session.project.timeline_mut(timeline) else {
        return Err(format!("timeline {} does not exist", timeline.0));
    };
    timeline.prohibited_ops = ops;
    Ok(empty_object())
}

/// Change the language of an audio or subtitle track (Encore's language list in the track header).
fn run_set_track_language(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let kind = req_str(params, "kind")?;
    let track = req_usize(params, "track")?;
    let language = req_str(params, "language")?.trim().to_ascii_lowercase();
    // DVD writes ISO 639-1 codes (two letters) into the IFO; empty means "not specified".
    if !(language.is_empty()
        || (language.len() == 2 && language.chars().all(|c| c.is_ascii_lowercase())))
    {
        return Err(format!(
            "language {language:?} is not a two-letter ISO 639-1 code"
        ));
    }
    let Some(timeline) = session.project.timeline_mut(timeline) else {
        return Err(format!("timeline {} does not exist", timeline.0));
    };
    let slot = match kind.as_str() {
        "audio" => timeline
            .audio
            .get_mut(track)
            .map(|track| &mut track.language),
        "subtitle" => timeline
            .subtitles
            .get_mut(track)
            .map(|track| &mut track.language),
        other => return Err(format!("unknown track kind {other:?} (audio or subtitle)")),
    };
    let Some(slot) = slot else {
        return Err(format!("{kind} track {track} does not exist"));
    };
    *slot = language;
    Ok(empty_object())
}

fn run_remove_audio_track(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let track = req_usize(params, "track")?;

    let Some(timeline) = session.project.timeline_mut(timeline) else {
        return Err(format!("timeline {} does not exist", timeline.0));
    };

    if track >= timeline.audio.len() {
        return Err(format!("audio track index {track} is out of range"));
    }

    timeline.audio.remove(track);
    Ok(empty_object())
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use serde_json::{Map, Value};
    use std::error::Error;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn temp_dir(name: &str) -> Result<PathBuf, Box<dyn Error>> {
        let mut path = std::env::temp_dir();
        path.push(format!("disccraft-engine-{}-{name}", std::process::id()));
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    fn string_value(path: &Path) -> Value {
        Value::String(path.to_string_lossy().into_owned())
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
    fn a_timeline_keeps_its_own_aspect() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("aspect")?;
        let video = dir.join("movie.mp4");
        fs::write(&video, b"fake")?;
        let mut session = Session::new();
        let video_id = value_u64(
            &session.execute("asset.import", &obj(&[("path", string_value(&video))]))?,
            "asset",
        )?;
        let timeline_id = value_u64(
            &session.execute("timeline.create", &obj(&[("asset", Value::from(video_id))]))?,
            "timeline",
        )?;
        let wide = |session: &Session| {
            session.project.timelines.values().next().map(|timeline| {
                (
                    timeline.widescreen,
                    timeline.wide(&session.project.settings),
                )
            })
        };
        assert_eq!(wide(&session), Some((None, false)));
        session.execute(
            "timeline.setAspect",
            &obj(&[
                ("timeline", Value::from(timeline_id)),
                ("aspect", Value::from("16:9")),
            ]),
        )?;
        assert_eq!(wide(&session), Some((Some(true), true)));
        session.execute("disc.setAspect", &obj(&[("aspect", Value::from("16:9"))]))?;
        session.execute(
            "timeline.setAspect",
            &obj(&[
                ("timeline", Value::from(timeline_id)),
                ("aspect", Value::from("4:3")),
            ]),
        )?;
        assert_eq!(wide(&session), Some((Some(false), false)));
        assert!(
            session
                .execute(
                    "timeline.setAspect",
                    &obj(&[
                        ("timeline", Value::from(timeline_id)),
                        ("aspect", Value::from("2.35"))
                    ]),
                )
                .is_err()
        );
        session.undo()?;
        assert_eq!(wide(&session), Some((Some(true), true)));
        Ok(())
    }

    #[test]
    fn overrides_are_set_checked_and_undone() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("override")?;
        let video = dir.join("movie.mp4");
        fs::write(&video, b"fake")?;
        let mut session = Session::new();
        let video_id = value_u64(
            &session.execute("asset.import", &obj(&[("path", string_value(&video))]))?,
            "asset",
        )?;
        let timeline_id = value_u64(
            &session.execute("timeline.create", &obj(&[("asset", Value::from(video_id))]))?,
            "timeline",
        )?;
        let menu = value_u64(&session.execute("menu.create", &obj(&[]))?, "menu")?;
        let set =
            |target: Value| obj(&[("timeline", Value::from(timeline_id)), ("target", target)]);
        let current = |session: &Session| {
            session
                .project
                .timelines
                .values()
                .next()
                .and_then(|timeline| timeline.override_link.clone())
        };
        session.execute(
            "timeline.setOverride",
            &set(obj(&[("menu", Value::from(menu))])),
        )?;
        assert!(matches!(
            current(&session),
            Some(disccraft_project::Link::Menu { .. })
        ));
        // A menu that does not exist is refused.
        assert!(
            session
                .execute(
                    "timeline.setOverride",
                    &set(obj(&[("menu", Value::from(999))]))
                )
                .is_err()
        );
        session.execute("timeline.setOverride", &set(Value::Null))?;
        assert_eq!(current(&session), None);
        session.undo()?;
        assert!(current(&session).is_some());
        Ok(())
    }

    #[test]
    fn user_ops_are_set_and_undone() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("user_ops")?;
        let video = dir.join("movie.mp4");
        fs::write(&video, b"fake")?;
        let mut session = Session::new();
        let video_id = value_u64(
            &session.execute("asset.import", &obj(&[("path", string_value(&video))]))?,
            "asset",
        )?;
        let timeline_id = value_u64(
            &session.execute("timeline.create", &obj(&[("asset", Value::from(video_id))]))?,
            "timeline",
        )?;
        let set = |names: &[&str]| {
            obj(&[
                ("timeline", Value::from(timeline_id)),
                (
                    "prohibited",
                    Value::Array(names.iter().map(|name| Value::from(*name)).collect()),
                ),
            ])
        };
        session.execute(
            "timeline.setUserOps",
            &set(&["rewind", "fastForward", "rewind"]),
        )?;
        let ops = |session: &Session| {
            session
                .project
                .timelines
                .values()
                .next()
                .map(|timeline| timeline.prohibited_ops.clone())
                .unwrap_or_default()
        };
        assert_eq!(
            ops(&session),
            vec![
                disccraft_project::UserOp::FastForward,
                disccraft_project::UserOp::Rewind
            ]
        );
        assert!(
            session
                .execute("timeline.setUserOps", &set(&["warp"]))
                .is_err()
        );
        assert!(
            session
                .execute(
                    "timeline.setUserOps",
                    &obj(&[("timeline", Value::from(timeline_id))])
                )
                .is_err()
        );
        session.undo()?;
        assert!(ops(&session).is_empty());
        Ok(())
    }

    #[test]
    fn set_track_language_and_undo() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("set_track_language")?;
        let video = dir.join("movie.mp4");
        let audio = dir.join("audio.wav");
        fs::write(&video, b"fake")?;
        fs::write(&audio, b"fake")?;

        let mut session = Session::new();
        let video_id = value_u64(
            &session.execute("asset.import", &obj(&[("path", string_value(&video))]))?,
            "asset",
        )?;
        let audio_id = value_u64(
            &session.execute("asset.import", &obj(&[("path", string_value(&audio))]))?,
            "asset",
        )?;
        let timeline_id = value_u64(
            &session.execute("timeline.create", &obj(&[("asset", Value::from(video_id))]))?,
            "timeline",
        )?;
        session.execute(
            "timeline.addAudioTrack",
            &obj(&[
                ("timeline", Value::from(timeline_id)),
                ("asset", Value::from(audio_id)),
                ("language", Value::String("ja".to_string())),
            ]),
        )?;

        let set = |language: &str, kind: &str, track: u64| {
            obj(&[
                ("timeline", Value::from(timeline_id)),
                ("kind", Value::String(kind.to_string())),
                ("track", Value::from(track)),
                ("language", Value::String(language.to_string())),
            ])
        };
        session.execute("timeline.setTrackLanguage", &set("EN", "audio", 0))?;
        let language = |session: &Session| {
            session
                .project
                .timelines
                .values()
                .next()
                .and_then(|timeline| timeline.audio.first())
                .map(|track| track.language.clone())
        };
        assert_eq!(language(&session).as_deref(), Some("en"));

        // Bad codes, kinds and tracks are refused and leave the track alone.
        assert!(
            session
                .execute("timeline.setTrackLanguage", &set("eng", "audio", 0))
                .is_err()
        );
        assert!(
            session
                .execute("timeline.setTrackLanguage", &set("j1", "audio", 0))
                .is_err()
        );
        assert!(
            session
                .execute("timeline.setTrackLanguage", &set("fr", "video", 0))
                .is_err()
        );
        assert!(
            session
                .execute("timeline.setTrackLanguage", &set("fr", "audio", 3))
                .is_err()
        );
        assert!(
            session
                .execute("timeline.setTrackLanguage", &set("fr", "subtitle", 0))
                .is_err()
        );
        assert_eq!(language(&session).as_deref(), Some("en"));

        session.undo()?;
        assert_eq!(language(&session).as_deref(), Some("ja"));
        Ok(())
    }

    #[test]
    fn add_audio_track_and_undo() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("add_audio_track_undo")?;
        let video = dir.join("movie.mp4");
        let audio = dir.join("audio.wav");
        fs::write(&video, b"fake")?;
        fs::write(&audio, b"fake")?;

        let mut session = Session::new();

        let video_import =
            session.execute("asset.import", &obj(&[("path", string_value(&video))]))?;
        let video_id = value_u64(&video_import, "asset")?;

        let audio_import =
            session.execute("asset.import", &obj(&[("path", string_value(&audio))]))?;
        let audio_id = value_u64(&audio_import, "asset")?;

        let create =
            session.execute("timeline.create", &obj(&[("asset", Value::from(video_id))]))?;
        let timeline_id = value_u64(&create, "timeline")?;

        let add = session.execute(
            "timeline.addAudioTrack",
            &obj(&[
                ("timeline", Value::from(timeline_id)),
                ("asset", Value::from(audio_id)),
                ("language", Value::String("ja".to_string())),
            ]),
        )?;
        assert_eq!(value_u64(&add, "track")?, 0);

        let timeline = session
            .project
            .timelines
            .values()
            .next()
            .ok_or("missing timeline")?;
        assert_eq!(timeline.audio.len(), 1);
        assert_eq!(
            timeline.audio.first().map(|track| track.language.as_str()),
            Some("ja")
        );

        session.undo()?;

        let timeline = session
            .project
            .timelines
            .values()
            .next()
            .ok_or("missing timeline")?;
        assert!(timeline.audio.is_empty());

        fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn poster_frames_are_set_cleared_and_undone() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("poster_frame")?;
        let video = dir.join("movie.mp4");
        fs::write(&video, b"fake")?;

        let mut session = Session::new();
        let import = session.execute("asset.import", &obj(&[("path", string_value(&video))]))?;
        let create = session.execute(
            "timeline.create",
            &obj(&[("asset", Value::from(value_u64(&import, "asset")?))]),
        )?;
        let timeline_id = value_u64(&create, "timeline")?;
        let timeline = Value::from(timeline_id);
        let poster_of = |session: &Session| {
            session
                .project
                .timeline(disccraft_project::TimelineId(timeline_id))
                .and_then(|timeline| timeline.chapters.first())
                .and_then(|chapter| chapter.poster)
        };

        session.execute(
            "timeline.setPosterFrame",
            &obj(&[
                ("timeline", timeline.clone()),
                ("chapter", Value::from(0)),
                ("timecode", Value::from("00;00;02;00")),
            ]),
        )?;
        assert!(poster_of(&session).is_some_and(|poster| poster > disccraft_project::Tick::ZERO));

        session.execute(
            "timeline.setPosterFrame",
            &obj(&[
                ("timeline", timeline.clone()),
                ("chapter", Value::from(0)),
                ("time", Value::Null),
            ]),
        )?;
        assert_eq!(poster_of(&session), None);
        session.execute("edit.undo", &obj(&[]))?;
        assert!(poster_of(&session).is_some());

        for params in [
            obj(&[
                ("timeline", timeline.clone()),
                ("chapter", Value::from(5)),
                ("time", Value::from(10)),
            ]),
            obj(&[
                ("timeline", timeline.clone()),
                ("chapter", Value::from(0)),
                ("time", Value::from(-1)),
            ]),
        ] {
            assert!(session.execute("timeline.setPosterFrame", &params).is_err());
        }
        Ok(())
    }

    #[test]
    fn rename_and_retime_a_chapter_with_undo() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("rename_chapter")?;
        let video = dir.join("movie.mp4");
        fs::write(&video, b"fake")?;

        let mut session = Session::new();
        let import = session.execute("asset.import", &obj(&[("path", string_value(&video))]))?;
        let create = session.execute(
            "timeline.create",
            &obj(&[("asset", Value::from(value_u64(&import, "asset")?))]),
        )?;
        let timeline = Value::from(value_u64(&create, "timeline")?);
        session.execute(
            "timeline.addChapter",
            &obj(&[
                ("timeline", timeline.clone()),
                ("timecode", Value::from("00;00;07;24")),
            ]),
        )?;

        session.execute(
            "timeline.renameChapter",
            &obj(&[
                ("timeline", timeline.clone()),
                ("chapter", Value::from(1)),
                ("name", Value::from("Opening")),
            ]),
        )?;
        let chapter_name = |session: &Session| {
            session
                .project
                .timelines
                .values()
                .next()
                .and_then(|timeline| timeline.chapters.get(1))
                .map(|chapter| chapter.name.clone())
        };
        assert_eq!(chapter_name(&session).as_deref(), Some("Opening"));

        // The In point typed in the Properties panel uses the same timecode form as the display.
        let moved = session.execute(
            "timeline.moveChapter",
            &obj(&[
                ("timeline", timeline.clone()),
                ("chapter", Value::from(1)),
                ("timecode", Value::from("00;00;02;00")),
            ]),
        )?;
        assert_eq!(value_u64(&moved, "chapter")?, 1);

        session.undo()?;
        session.undo()?;
        assert_eq!(chapter_name(&session).as_deref(), Some("Chapter 2"));

        let missing = session.execute(
            "timeline.renameChapter",
            &obj(&[
                ("timeline", timeline),
                ("chapter", Value::from(7)),
                ("name", Value::from("x")),
            ]),
        );
        assert!(missing.is_err());

        fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn add_audio_track_rejects_video_asset() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("add_audio_track_video")?;
        let video = dir.join("movie.mp4");
        fs::write(&video, b"fake")?;

        let mut session = Session::new();

        let import = session.execute("asset.import", &obj(&[("path", string_value(&video))]))?;
        let video_id = value_u64(&import, "asset")?;

        let create =
            session.execute("timeline.create", &obj(&[("asset", Value::from(video_id))]))?;
        let timeline_id = value_u64(&create, "timeline")?;

        let result = session.execute(
            "timeline.addAudioTrack",
            &obj(&[
                ("timeline", Value::from(timeline_id)),
                ("asset", Value::from(video_id)),
            ]),
        );

        fs::remove_dir_all(dir)?;
        let message = result.err().ok_or("expected video asset rejection")?;
        assert!(message.contains("not an audio asset"), "{message}");
        Ok(())
    }
}
