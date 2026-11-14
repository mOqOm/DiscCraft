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
        id: "timeline.import",
        label: "タイムラインとして読み込み",
        menu: &["ファイル", "読み込み"],
        shortcut: None,
        params: r#"{"paths": [string]}"#,
        enabled: always_enabled,
        run: run_import,
        journal: true,
    },
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

/// Encore's 読み込み ▸ タイムライン…: each video file becomes an asset and a timeline of its own, named after the
/// file, in the order of the file names (one undo step for all of them).
fn run_import(session: &mut Session, params: &Value) -> CommandResult {
    let paths = req_paths(params)?;
    let mut assets = Vec::with_capacity(paths.len());
    let mut timelines = Vec::with_capacity(paths.len());
    for path in &paths {
        let file = std::path::Path::new(path);
        if !file.is_file() {
            return Err(format!("timeline.import: file does not exist: {path}"));
        }
        let asset = session
            .project
            .add_asset(file)
            .map_err(|error| error.to_string())?;
        let kind = session.project.assets.get(&asset).map(|asset| asset.kind);
        if kind != Some(disccraft_project::AssetKind::Video) {
            return Err(format!("timeline.import: not a video file: {path}"));
        }
        let timeline = session
            .project
            .add_timeline(asset, None)
            .map_err(|error| error.to_string())?;
        take_source_streams(session, timeline);
        assets.push(Value::from(asset.0));
        timelines.push(Value::from(timeline.0));
    }
    Ok(object(&[
        ("assets", Value::Array(assets)),
        ("timelines", Value::Array(timelines)),
    ]))
}

/// The "paths" parameter: one or more file paths, in the order of their file names (as Encore takes the files of
/// one import).
pub(crate) fn req_paths(params: &Value) -> Result<Vec<String>, String> {
    let paths: Vec<String> = params
        .get("paths")
        .and_then(Value::as_array)
        .ok_or_else(|| "parameter \"paths\" must be a list of file paths".to_string())?
        .iter()
        .map(|path| {
            path.as_str()
                .map(str::to_owned)
                .ok_or_else(|| "parameter \"paths\" must be a list of file paths".to_string())
        })
        .collect::<Result<_, _>>()?;
    if paths.is_empty() {
        return Err("parameter \"paths\" is empty".to_string());
    }
    let mut paths = paths;
    paths.sort_by_key(|path| {
        std::path::Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    });
    Ok(paths)
}

fn run_create(session: &mut Session, params: &Value) -> CommandResult {
    let asset = AssetId(req_u64(params, "asset")?);
    let name = opt_str(params, "name")?;
    let id = session
        .project
        .add_timeline(asset, name)
        .map_err(|error| error.to_string())?;
    take_source_streams(session, id);

    Ok(object(&[("timeline", Value::from(id.0))]))
}

/// A new timeline takes from its video what Encore's timelines do: the aspect (16:9 for a wide picture, 4:3
/// otherwise), and the file's sound as its own track, Audio 1 (Encore lays a video's picture and sound out as
/// separate tracks, so more audio and subtitle tracks go beside them). Nothing changes when the file cannot be read
/// (an offline asset): the timeline follows the disc's aspect and plays the video's sound, if any.
fn take_source_streams(session: &mut Session, timeline: TimelineId) {
    let video = session
        .project
        .timelines
        .get(&timeline)
        .map(|timeline| timeline.video);
    let probe = video
        .and_then(|video| session.project.assets.get(&video))
        .filter(|asset| asset.kind == AssetKind::Video)
        .and_then(|asset| disccraft_transcode::probe_video(std::path::Path::new(&asset.path)).ok());
    if let (Some(probe), Some(video), Some(timeline)) =
        (probe, video, session.project.timelines.get_mut(&timeline))
    {
        timeline.widescreen = Some(probe.wide);
        if probe.has_audio && timeline.audio.is_empty() {
            timeline.audio.push(disccraft_project::AudioTrack {
                asset: video,
                language: String::new(),
            });
        }
    }
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

    let (asset_id, asset_kind, path) = match session.project.assets.get(&asset) {
        Some(asset) => (asset.id, asset.kind, asset.path.clone()),
        None => return Err(format!("asset {} does not exist", asset.0)),
    };

    // An audio file, or a video file's own sound (Encore lays a video's picture and sound out as separate tracks).
    match asset_kind {
        AssetKind::Audio => {}
        AssetKind::Video => {
            let has_audio = disccraft_transcode::probe_video(std::path::Path::new(&path))
                .is_ok_and(|probe| probe.has_audio);
            if !has_audio {
                return Err(format!("video asset {} has no sound", asset_id.0));
            }
        }
        _ => return Err(format!("asset {} is not an audio asset", asset_id.0)),
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

    /// A new timeline takes its video's aspect, as in Encore: 16:9 for a 16:9 source, 4:3 for a 4:3 one, whatever
    /// the disc's (needs the fixtures the ffmpeg tests make).
    #[test]
    fn a_timeline_takes_its_video_aspect() -> Result<(), Box<dyn Error>> {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
        let wide = fixtures.join("h264_aac.mp4");
        let narrow = fixtures.join("ntsc.m2v");
        if !wide.is_file() || !narrow.is_file() {
            println!("skipped: no fixtures");
            return Ok(());
        }
        let mut session = Session::default();
        let mut aspect = |path: &Path| -> Result<Option<bool>, Box<dyn Error>> {
            let imported = session.execute(
                "timeline.import",
                &serde_json::json!({"paths": [path.to_string_lossy()]}),
            )?;
            let id = imported["timelines"][0].as_u64().ok_or("no timeline")?;
            let asset = imported["assets"][0].as_u64().ok_or("no asset")?;
            let created =
                session.execute("timeline.create", &serde_json::json!({"asset": asset}))?;
            let second = created["timeline"].as_u64().ok_or("no timeline")?;
            let of = |id: u64| {
                session
                    .project
                    .timelines
                    .get(&disccraft_project::TimelineId(id))
                    .and_then(|timeline| timeline.widescreen)
            };
            assert_eq!(of(id), of(second));
            Ok(of(id))
        };
        assert_eq!(aspect(&wide)?, Some(true));
        assert_eq!(aspect(&narrow)?, Some(false));
        Ok(())
    }

    /// A timeline made from a video with sound lays the sound out as Audio 1, the video file itself (Encore
    /// separates a video's picture and sound into tracks). Uses the transcode tests' fixture when it is there.
    #[test]
    fn a_new_timeline_takes_its_videos_sound_as_audio_1() -> Result<(), Box<dyn Error>> {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures/h264_aac.mp4");
        if !fixture.is_file() {
            println!(
                "skipped: no target/fixtures/h264_aac.mp4 (made by the transcode tests with ffmpeg)"
            );
            return Ok(());
        }
        let mut session = Session::new();
        let video = value_u64(
            &session.execute("asset.import", &obj(&[("path", string_value(&fixture))]))?,
            "asset",
        )?;
        let timeline = value_u64(
            &session.execute("timeline.create", &obj(&[("asset", Value::from(video))]))?,
            "timeline",
        )?;
        let made = session
            .project
            .timeline(disccraft_project::TimelineId(timeline))
            .ok_or("no timeline")?;
        assert_eq!(made.audio.len(), 1);
        assert_eq!(made.audio.first().map(|track| track.asset.0), Some(video));
        assert_eq!(
            made.audio.first().map(|track| track.language.as_str()),
            Some("")
        );
        assert_eq!(made.widescreen, Some(true));
        let probe = session.execute("asset.probe", &obj(&[("asset", Value::from(video))]))?;
        assert_eq!(probe.get("audio"), Some(&Value::from(true)));
        // The same sound again, as a second track (another language, say).
        let added = session.execute(
            "timeline.addAudioTrack",
            &obj(&[
                ("timeline", Value::from(timeline)),
                ("asset", Value::from(video)),
                ("language", Value::from("en")),
            ]),
        )?;
        assert_eq!(added.get("track"), Some(&Value::from(1)));
        Ok(())
    }

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
    fn video_files_import_as_timelines_and_images_as_one_slideshow() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("import-as")?;
        let file = |name: &str| -> Result<Value, Box<dyn Error>> {
            let path = dir.join(name);
            fs::write(&path, b"fake")?;
            Ok(string_value(&path))
        };
        let mut session = Session::new();
        // Two videos: an asset and a timeline each, named after the file, in file name order; one undo step.
        let imported = session.execute(
            "timeline.import",
            &obj(&[(
                "paths",
                Value::Array(vec![file("clipB.mp4")?, file("clipA.mp4")?]),
            )]),
        )?;
        let timelines = imported
            .get("timelines")
            .and_then(Value::as_array)
            .ok_or("timelines")?;
        assert_eq!(timelines.len(), 2);
        let names: Vec<&str> = session
            .project
            .timelines
            .values()
            .map(|timeline| timeline.name.as_str())
            .collect();
        assert_eq!(names, vec!["clipA", "clipB"]);
        assert_eq!(session.project.assets.len(), 2);
        assert_eq!(session.undo_label(), Some("タイムラインとして読み込み"));
        session.undo()?;
        assert!(session.project.timelines.is_empty() && session.project.assets.is_empty());
        // An image is not a timeline: nothing is imported.
        assert!(
            session
                .execute(
                    "timeline.import",
                    &obj(&[(
                        "paths",
                        Value::Array(vec![file("clip.mp4")?, file("still.png")?])
                    )]),
                )
                .is_err()
        );
        assert!(session.project.assets.is_empty());

        // Three images: one slideshow, the slides in file name order, named after the first.
        let imported = session.execute(
            "slideshow.import",
            &obj(&[(
                "paths",
                Value::Array(vec![
                    file("pic2.png")?,
                    file("pic1.png")?,
                    file("pic3.png")?,
                ]),
            )]),
        )?;
        let id = value_u64(&imported, "slideshow")?;
        let slideshow = session.project.slideshow(id).ok_or("slideshow")?;
        assert_eq!(slideshow.name, "pic1");
        let slides: Vec<String> = slideshow
            .slides
            .iter()
            .filter_map(|slide| session.project.assets.get(&slide.asset))
            .map(|asset| asset.name.clone())
            .collect();
        assert_eq!(slides, vec!["pic1", "pic2", "pic3"]);
        assert_eq!(session.undo_label(), Some("スライドショーとして読み込み"));
        let _ = fs::remove_dir_all(&dir);
        Ok(())
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
        // The disc's aspect is every title's: the timeline follows it again.
        session.execute("disc.setAspect", &obj(&[("aspect", Value::from("4:3"))]))?;
        assert_eq!(wide(&session), Some((None, false)));
        session.execute("disc.setAspect", &obj(&[("aspect", Value::from("16:9"))]))?;
        assert_eq!(wide(&session), Some((None, true)));
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
        assert_eq!(wide(&session), Some((None, true)));
        // Undoing the disc's aspect gives the timeline its own back.
        session.undo()?;
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

    /// A video's sound can be an audio track (see `a_new_timeline_takes_its_videos_sound_as_audio_1`); a video
    /// with no sound it can read cannot.
    #[test]
    fn add_audio_track_rejects_a_video_without_sound() -> Result<(), Box<dyn Error>> {
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
        assert!(message.contains("has no sound"), "{message}");
        Ok(())
    }
}
