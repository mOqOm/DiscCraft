use crate::params::{opt_str, req_link, req_str, req_u64, req_usize, time_param};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{
    AssetId, AssetKind, AudioTrack, ClipTrim, DiscKind, Link, SubtitleColor, SubtitleColorSet,
    SubtitleGroup, SubtitleTrack, Tick, TimelineId, format_time,
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
        id: "timeline.appendVideo",
        label: "ビデオクリップを追加",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "asset": id}"#,
        enabled: always_enabled,
        run: run_append_video,
        journal: true,
    },
    CommandSpec {
        id: "timeline.removeVideo",
        label: "ビデオクリップを削除",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "clip": index (1 = the second clip), "ripple"?: bool (false: その場所を空ける)}"#,
        enabled: always_enabled,
        run: run_remove_video,
        journal: true,
    },
    CommandSpec {
        id: "timeline.trimVideo",
        label: "ビデオクリップをトリム",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "clip": index, "edge": "in" | "out", "timecode"?: "HH:MM:SS;FF", "time"?: ticks}"#,
        enabled: always_enabled,
        run: run_trim_video,
        journal: true,
    },
    CommandSpec {
        id: "timeline.moveVideo",
        label: "ビデオクリップを移動",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "clip": index, "timecode"?: "HH:MM:SS;FF", "time"?: ticks}"#,
        enabled: always_enabled,
        run: run_move_video,
        journal: true,
    },
    CommandSpec {
        id: "timeline.clips",
        label: "ビデオクリップの一覧",
        menu: &[],
        shortcut: None,
        params: r#"{"timeline": id}"#,
        enabled: always_enabled,
        run: run_clips,
        journal: false,
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
        id: "timeline.setParentalLevel",
        label: "視聴制限レベルを設定",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "level": null (ディスクのレベル) | 0 (なし) | 1-8}"#,
        enabled: always_enabled,
        run: run_set_parental_level,
        journal: true,
    },
    CommandSpec {
        id: "timeline.setParentalAlternative",
        label: "視聴制限で再生しないときの行き先を設定",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "target": Link (未設定: 停止)}"#,
        enabled: always_enabled,
        run: run_set_parental_alternative,
        journal: true,
    },
    CommandSpec {
        id: "timeline.setParentalChapter",
        label: "チャプターの視聴制限の代わりを設定",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "chapter": 0 から (チャプターの番号 - 1), "level": 2-8 (このレベルより低いプレーヤーは代わりを再生) | null (代わりを外す), "asset"?: id (代わりのビデオ。level があるときは必須)}"#,
        enabled: always_enabled,
        run: run_set_parental_chapter,
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
        id: "timeline.importSourceChapters",
        label: "ソースのチャプターをチャプターポイントに",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "replace"?: bool (今のチャプターポイントを置き換える)} → {"added", "skipped"} (mkv / mp4 に入っているチャプター)"#,
        enabled: always_enabled,
        run: run_import_source_chapters,
        journal: true,
    },
    CommandSpec {
        id: "timeline.importFilmCraftMarkers",
        label: "FilmCraft のマーカーをチャプターに",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "path": string (.fcproj), "sequence"?: string, "all"?: bool}"#,
        enabled: always_enabled,
        run: run_import_filmcraft_markers,
        journal: true,
    },
    CommandSpec {
        id: "filmcraft.sequences",
        label: "FilmCraft のプロジェクトのシーケンス",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"path": string (.fcproj)}"#,
        enabled: always_enabled,
        run: run_filmcraft_sequences,
        journal: false,
    },
    CommandSpec {
        id: "colorset.save",
        label: "字幕のカラーセットに名前を付けて保存",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"name": string, "timeline"?: id, "slideshow"?: id}"#,
        enabled: always_enabled,
        run: run_save_color_set,
        journal: true,
    },
    CommandSpec {
        id: "colorset.apply",
        label: "字幕のカラーセットを適用",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"name": string, "timeline"?: id, "slideshow"?: id}"#,
        enabled: always_enabled,
        run: run_apply_color_set,
        journal: true,
    },
    CommandSpec {
        id: "colorset.delete",
        label: "字幕のカラーセットを削除",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"name": string}"#,
        enabled: always_enabled,
        run: run_delete_color_set,
        journal: true,
    },
    CommandSpec {
        id: "colorset.export",
        label: "字幕のカラーセットを書き出し",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline"?: id, "slideshow"?: id, "path": string}"#,
        enabled: always_enabled,
        run: run_export_color_set,
        journal: false,
    },
    CommandSpec {
        id: "colorset.import",
        label: "字幕のカラーセットを読み込み",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline"?: id, "slideshow"?: id, "path": string}"#,
        enabled: always_enabled,
        run: run_import_color_set,
        journal: true,
    },
    CommandSpec {
        id: "timeline.setSubtitleColors",
        label: "タイムラインカラーセットを設定",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "groups": [{"fill": [r, g, b], "fillOpacity"?: 0-100, "stroke": [r, g, b], "strokeOpacity"?: 0-100, "antialias"?: [r, g, b], "antialiasOpacity"?: 0-100} x 3] | null}"#,
        enabled: always_enabled,
        run: run_set_subtitle_colors,
        journal: true,
    },
    CommandSpec {
        id: "timeline.setSubtitleGroup",
        label: "字幕トラックのカラーグループを設定",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "track": index, "group": 1-3}"#,
        enabled: always_enabled,
        run: run_set_subtitle_group,
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
        id: "timeline.trimAudio",
        label: "オーディオクリップをトリム",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "track": index, "clip"?: number (0 = 最初のクリップ), "edge": "in" | "out", "timecode"?: "HH:MM:SS;FF", "time"?: ticks}"#,
        enabled: always_enabled,
        run: run_trim_audio,
        journal: true,
    },
    CommandSpec {
        id: "timeline.moveAudio",
        label: "オーディオクリップを移動",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "track": index, "clip"?: number (0 = 最初のクリップ), "timecode"?: "HH:MM:SS;FF", "time"?: ticks}"#,
        enabled: always_enabled,
        run: run_move_audio,
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
        id: "timeline.addAudioClip",
        label: "オーディオクリップを追加",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "track": index, "asset": id, "timecode"?: "HH:MM:SS;FF", "time"?: ticks (省略: 最後のクリップの後ろ)}"#,
        enabled: always_enabled,
        run: run_add_audio_clip,
        journal: true,
    },
    CommandSpec {
        id: "timeline.removeAudioClip",
        label: "オーディオクリップを消去",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"timeline": id, "track": index, "clip": number (0 = 最初のクリップ)}"#,
        enabled: always_enabled,
        run: run_remove_audio_clip,
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
        // As in Encore, the first timeline becomes the disc's first play.
        session.project.adopt_first_play(timeline);
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
    // The chapters its file carries become its chapter points (Encore: the chapter markers embedded in a file).
    let _ = source_chapters(session, id);
    // As in Encore, the first timeline becomes the disc's first play.
    session.project.adopt_first_play(id);

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
        .and_then(|asset| disccraft_transcode::probe_video(&asset.media_file()).ok());
    if let (Some(probe), Some(video), Some(timeline)) =
        (probe, video, session.project.timelines.get_mut(&timeline))
    {
        timeline.widescreen = Some(probe.wide);
        if probe.has_audio && timeline.audio.is_empty() {
            timeline.audio.push(disccraft_project::AudioTrack {
                more: Vec::new(),
                asset: video,
                language: String::new(),
                start: 0,
                trim_start: 0,
                trim_end: None,
            });
        }
    }
}

fn run_add_chapter(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let time = time_param(params, &session.project.settings)?;
    let time = gop_chapter_time(session, timeline, time, None)?;
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

/// Where each video clip of `timeline` plays: its asset, start and length. An MPEG-2 elementary stream is as long
/// as its pictures; other files as long as they say.
fn clip_spans(
    session: &mut Session,
    timeline: TimelineId,
) -> Result<Vec<(AssetId, Tick, Tick)>, String> {
    let found = session
        .project
        .timeline(timeline)
        .ok_or_else(|| format!("timeline {} does not exist", timeline.0))?;
    let videos = found.videos();
    let trims: Vec<ClipTrim> = (0..videos.len())
        .map(|clip| found.video_trim(clip))
        .collect();
    let gaps: Vec<u32> = (0..videos.len())
        .map(|clip| found.video_gap(clip))
        .collect();
    let rate = session.project.settings.frame_rate();
    let mut spans = Vec::new();
    let mut frames = 0_i64;
    for ((video, trim), gap) in videos.into_iter().zip(trims).zip(gaps) {
        // The empty stretch in front of the clip.
        frames = frames.saturating_add(i64::from(gap));
        let path = session
            .project
            .assets
            .get(&video)
            .map(|asset| asset.media_file().into_owned())
            .ok_or_else(|| format!("asset {} does not exist", video.0))?;
        let length = match session.gops.starts(&path) {
            Some(gops) => gops.pictures,
            None => {
                let probe = disccraft_transcode::probe_video(&path)
                    .map_err(|error| format!("{}: {error}", path.display()))?;
                // `as` saturates; a clip's length is far inside the range.
                (probe.seconds * rate.as_f64()).round() as i64
            }
        };
        // The trimmed part of the clip.
        let end = trim.end.map_or(length, |end| i64::from(end).min(length));
        let length = end.saturating_sub(i64::from(trim.start));
        if length <= 0 {
            return Err(format!("{} has no length", path.display()));
        }
        let start = rate.tick_of(frames);
        frames = frames.saturating_add(length);
        spans.push((
            video,
            start,
            Tick(rate.tick_of(frames).0.saturating_sub(start.0)),
        ));
    }
    Ok(spans)
}

/// Encore: a video dropped on the video track is butted on to the end of the clips, wherever it is dropped, and a
/// chapter appears at the join.
fn run_append_video(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let asset = AssetId(req_u64(params, "asset")?);
    let spans = clip_spans(session, timeline)?;
    let join = spans.last().map_or(Tick::ZERO, |(_, start, length)| {
        Tick(start.0.saturating_add(length.0))
    });
    let chapter = session
        .project
        .append_timeline_video(timeline, asset, join)
        .map_err(|error| error.to_string())?;
    let sound = place_own_sound(session, timeline, asset, join)?;
    Ok(object(&[
        ("clip", Value::from(spans.len())),
        ("chapter", Value::from(chapter)),
        ("audioTrack", sound.map_or(Value::Null, Value::from)),
        ("time", Value::from(join.0)),
        (
            "timecode",
            Value::String(format_time(join, &session.project.settings)),
        ),
    ]))
}

/// Encore lays a clip's sound beside its picture (M6.361): the sound of video `asset`, just appended at `join`, goes
/// on the timeline at the clip's place. It joins the track of the clips' own sounds (one whose clips are all the
/// timeline's video files; the sound of the clip before is cut at the join when it runs on). A timeline without
/// audio tracks gets a track of all its clips' own sounds, each cut as its clip is. Nothing is placed for a video
/// without sound, nor when the tracks hold other sound. Returns the track it went on.
fn place_own_sound(
    session: &mut Session,
    timeline: TimelineId,
    asset: AssetId,
    join: Tick,
) -> Result<Option<usize>, String> {
    let has_sound = |session: &Session, asset: AssetId| {
        session.project.assets.get(&asset).is_some_and(|found| {
            found.kind == AssetKind::Video
                && disccraft_transcode::probe_video(&found.media_file())
                    .is_ok_and(|probe| probe.has_audio)
        })
    };
    if !has_sound(session, asset) {
        return Ok(None);
    }
    let rate = session.project.settings.frame_rate();
    let frame = |tick: Tick| u32::try_from(rate.frame_at(tick).max(0)).unwrap_or(u32::MAX);
    let at = frame(join);
    let spans = clip_spans(session, timeline)?;
    let Some(found) = session.project.timeline(timeline) else {
        return Ok(None);
    };
    let videos = found.videos();
    let earlier = videos.get(..videos.len().saturating_sub(1)).unwrap_or(&[]);
    if found.audio.is_empty() {
        // The clips' own sounds, as the preview plays a timeline without a track.
        let mut clips = Vec::new();
        for (index, (clip_asset, start, _)) in spans.iter().enumerate() {
            if has_sound(session, *clip_asset) {
                let trim = found.video_trim(index);
                clips.push(disccraft_project::AudioClip {
                    asset: *clip_asset,
                    start: frame(*start),
                    trim_start: trim.start,
                    trim_end: trim.end,
                });
            }
        }
        let Some(first) = clips.first().copied() else {
            return Ok(None);
        };
        let track = AudioTrack {
            more: clips.get(1..).unwrap_or(&[]).to_vec(),
            asset: first.asset,
            language: String::new(),
            start: first.start,
            trim_start: first.trim_start,
            trim_end: first.trim_end,
        };
        if let Some(found) = session.project.timeline_mut(timeline) {
            found.audio.push(track);
        }
        return Ok(Some(0));
    }
    let Some(index) = found.audio.iter().position(|track| {
        track
            .clips()
            .iter()
            .all(|(_, clip)| earlier.contains(&clip.asset))
    }) else {
        return Ok(None);
    };
    let Some(track) = found.audio.get(index).cloned() else {
        return Ok(None);
    };
    if track.more.len().saturating_add(1) >= disccraft_project::MAX_AUDIO_CLIPS {
        return Ok(None);
    }
    let mut track = track;
    // The sound before the join ends there (a clip trimmed at its out point leaves its file's sound running on).
    if let Some((number, last)) = track.clips().last().copied() {
        let (start, end) = audio_clip_span(session, &last);
        if end.is_none_or(|end| end > i64::from(at)) {
            if start >= i64::from(at) {
                return Ok(None);
            }
            let kept = u32::try_from(i64::from(at) - start).unwrap_or(u32::MAX);
            let cut = disccraft_project::AudioClip {
                trim_end: Some(last.trim_start.saturating_add(kept)),
                ..last
            };
            let _ = track.set_clip(number, cut);
        }
    }
    track.more.push(disccraft_project::AudioClip {
        asset,
        start: at,
        trim_start: 0,
        trim_end: None,
    });
    if let Some(found) = session.project.timeline_mut(timeline)
        && let Some(slot) = found.audio.get_mut(index)
    {
        *slot = track;
    }
    Ok(Some(index))
}

/// Where a video clip is on its timeline: its file, its first frame on the timeline, the frames of its file's head it
/// leaves out, and how many frames it plays.
#[derive(Clone, Copy)]
struct ClipPlace {
    asset: AssetId,
    start: i64,
    trim_start: u32,
    frames: i64,
}

/// The places of timeline `timeline`'s video clips (M6.379).
fn clip_places(session: &mut Session, timeline: TimelineId) -> Result<Vec<ClipPlace>, String> {
    let spans = clip_spans(session, timeline)?;
    let rate = session.project.settings.frame_rate();
    let Some(found) = session.project.timeline(timeline) else {
        return Err(format!("timeline {} does not exist", timeline.0));
    };
    Ok(spans
        .iter()
        .enumerate()
        .map(|(index, (asset, start, length))| ClipPlace {
            asset: *asset,
            start: rate.frame_at(*start),
            trim_start: found.video_trim(index).start,
            frames: rate.frame_at(*length),
        })
        .collect())
}

/// Encore links a clip's picture and its sound (M6.379): after an edit of the video clips, the audio clips linked
/// to them (on any track, a clip of the same file that starts where the video clip started and leaves out as much
/// of the file's head) follow. `before` are the clips' places before the edit, `mapping` what each became (`None`:
/// removed, and its sound with it). A linked clip takes its video clip's new place, in point and length; a track
/// left with no clip goes. Other sound stays where it is.
fn follow_video_edit(
    session: &mut Session,
    timeline: TimelineId,
    before: &[ClipPlace],
    mapping: &[Option<usize>],
) -> Result<(), String> {
    let after = clip_places(session, timeline)?;
    let Some(found) = session.project.timeline_mut(timeline) else {
        return Ok(());
    };
    let mut emptied = Vec::new();
    for (index, track) in found.audio.iter_mut().enumerate() {
        let mut changed = false;
        let mut clips = Vec::new();
        for (_, clip) in track.clips() {
            let linked = before.iter().position(|place| {
                place.asset == clip.asset
                    && place.start == i64::from(clip.start)
                    && place.trim_start == clip.trim_start
            });
            match linked.map(|at| mapping.get(at).copied().flatten()) {
                // Not linked: as it is.
                None => clips.push(clip),
                // Its video clip went.
                Some(None) => changed = true,
                Some(Some(now)) => {
                    let Some(place) = after.get(now) else {
                        clips.push(clip);
                        continue;
                    };
                    let (Ok(start), Ok(frames)) =
                        (u32::try_from(place.start), u32::try_from(place.frames))
                    else {
                        clips.push(clip);
                        continue;
                    };
                    let moved = disccraft_project::AudioClip {
                        asset: clip.asset,
                        start,
                        trim_start: place.trim_start,
                        trim_end: Some(place.trim_start.saturating_add(frames)),
                    };
                    changed |= moved != clip;
                    clips.push(moved);
                }
            }
        }
        if !changed {
            continue;
        }
        clips.sort_by_key(|clip| clip.start);
        match clips.split_first() {
            Some((first, rest)) => {
                track.more = rest.to_vec();
                let _ = track.set_clip(0, *first);
            }
            None => emptied.push(index),
        }
    }
    for index in emptied.into_iter().rev() {
        let _ = found.audio.remove(index);
    }
    Ok(())
}

/// Takes a clip after the first off the timeline; its chapters go with it and the rest moves up.
fn run_remove_video(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let clip = req_usize(params, "clip")?;
    let before = clip_places(session, timeline)?;
    let spans = clip_spans(session, timeline)?;
    let (_, start, length) = spans
        .get(clip)
        .copied()
        .ok_or_else(|| "there is no such video clip to remove".to_string())?;
    // The empty stretch in front of the clip goes with it.
    let gap = session
        .project
        .timeline(timeline)
        .map_or(0, |found| found.video_gap(clip));
    let gap = session
        .project
        .settings
        .frame_rate()
        .tick_of(i64::from(gap));
    let (start, length) = (
        Tick(start.0.saturating_sub(gap.0)),
        Tick(length.0.saturating_add(gap.0)),
    );
    // "ripple": false leaves the clip's place empty (Encore's Clear); the clips after it stay where they are.
    let ripple = match params.get("ripple") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(ripple)) => *ripple,
        Some(_) => return Err("parameter \"ripple\" must be a boolean".to_string()),
    };
    if ripple {
        session
            .project
            .remove_timeline_video(timeline, clip, (start, length))
    } else {
        let rate = session.project.settings.frame_rate();
        let frames = u32::try_from(rate.frame_at(rate.snap_nearest(length)))
            .map_err(|_| "the clip is too long".to_string())?;
        session
            .project
            .clear_timeline_video(timeline, clip, (start, length), frames)
    }
    .map_err(|error| error.to_string())?;
    // Its linked sound goes with it; the others follow their clips (M6.379).
    let mapping: Vec<Option<usize>> = (0..before.len())
        .map(|index| match index.cmp(&clip) {
            std::cmp::Ordering::Less => Some(index),
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some(index - 1),
        })
        .collect();
    follow_video_edit(session, timeline, &before, &mapping)?;
    Ok(empty_object())
}

/// Moves a video clip along the track to start at the time given (Encore: a clip dragged along the video track; it
/// moves by frames and leaves a gap, which plays black). The clip cannot go in front of the clip before it or past
/// the one after it, which stays where it is; the last clip can go as far as wanted. The chapters inside the clip
/// move with it.
///
/// Encore removes a clip that another is dropped on; here the move stops at the neighbour instead.
fn run_move_video(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let clip = req_usize(params, "clip")?;
    let before = clip_places(session, timeline)?;
    let time = time_param(params, &session.project.settings)?;
    let spans = clip_spans(session, timeline)?;
    let (_, start, length) = spans
        .get(clip)
        .copied()
        .ok_or_else(|| "there is no such video clip".to_string())?;
    let rate = session.project.settings.frame_rate();
    let found = session
        .project
        .timeline(timeline)
        .ok_or_else(|| format!("timeline {} does not exist", timeline.0))?;
    let old_gap = i64::from(found.video_gap(clip));
    let next = clip.saturating_add(1);
    let next_gap = (next < spans.len()).then(|| i64::from(found.video_gap(next)));
    // Frames: where the clip is, where it may start at the earliest and at the latest.
    let old_start = rate.frame_at(start);
    let earliest = old_start.saturating_sub(old_gap);
    let latest = next_gap.map_or(i64::MAX, |gap| old_start.saturating_add(gap));
    let wanted = rate.frame_at(rate.snap_nearest(Tick(time.0.max(0))));
    let new_start = wanted.clamp(earliest, latest);
    let moved = new_start.saturating_sub(old_start);
    let narrow =
        |frames: i64| u32::try_from(frames).map_err(|_| "the clip is too far along".to_string());
    let gap = narrow(old_gap.saturating_add(moved))?;
    let next_gap = next_gap
        .map(|next| narrow(next.saturating_sub(moved)))
        .transpose()?;
    let by = Tick(rate.tick_of(new_start).0.saturating_sub(start.0));
    session
        .project
        .move_timeline_video(timeline, clip, (gap, next_gap), (start, length), by)
        .map_err(|error| error.to_string())?;
    // Its linked sound moves with it (M6.379).
    let mapping: Vec<Option<usize>> = (0..before.len()).map(Some).collect();
    follow_video_edit(session, timeline, &before, &mapping)?;
    let at = rate.tick_of(new_start);
    Ok(object(&[
        ("start", Value::from(at.0)),
        (
            "timecode",
            Value::String(format_time(at, &session.project.settings)),
        ),
        ("gap", Value::from(gap)),
    ]))
}

/// Trims a video clip's in or out point to the time given on the timeline, as Encore CS6 does: the point goes to
/// the nearest GOP start of the clip's file (the out point also to its end), and at least one GOP stays. The clips
/// stay butted: what follows the trimmed part moves up, the chapters in it go, and the chapter at the clip's head
/// stays. Video that is used as it is is cut where it can be: an MPEG-2 elementary stream at its GOPs, and
/// (M10.21) H.264 that goes on an HD Blu-ray as it is with its in point on an IDR picture and its out point on a
/// key picture. Other video of an HD Blu-ray is encoded clip by clip, so it is trimmed to the frame asked for; on
/// a DVD (and an SD Blu-ray) video that is transcoded cannot be trimmed.
///
/// Encore leaves a gap where a clip's head was and, on a head trim, also shortens the clip's end; neither is
/// copied (a gap cannot be built, and the end did not ask to move).
fn run_trim_video(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let clip = req_usize(params, "clip")?;
    let before = clip_places(session, timeline)?;
    let out_edge = match req_str(params, "edge")?.as_str() {
        "in" => false,
        "out" => true,
        other => {
            return Err(format!(
                "parameter \"edge\" must be \"in\" or \"out\", not \"{other}\""
            ));
        }
    };
    let time = time_param(params, &session.project.settings)?;
    let spans = clip_spans(session, timeline)?;
    let (asset, start, length) = spans
        .get(clip)
        .copied()
        .ok_or_else(|| "there is no such video clip".to_string())?;
    let path = session
        .project
        .assets
        .get(&asset)
        .map(|asset| asset.media_file().into_owned())
        .ok_or_else(|| format!("asset {} does not exist", asset.0))?;
    let rate = session.project.settings.frame_rate();
    // Where the clip can be cut, how long its file is, and (H.264) which of the points a clip can start on.
    let mut any_frame = false;
    let (starts, pictures, h264) = match session.gops.starts(&path) {
        Some(gops) => (gops.starts.clone(), gops.pictures, None),
        None => {
            if disccraft_dvd::BdVideoFormat::of(&session.project).is_none() {
                return Err(
                    "only video that needs no transcode can be trimmed on this disc: an MPEG-2 video \
                     stream"
                        .to_string(),
                );
            }
            let cut = disccraft_dvd::bd_cut_points(&session.project, &path, &cut_index_dir())
                .map_err(|error| error.to_string())?;
            let probe = disccraft_transcode::probe_video(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            // `as` saturates; a clip's length is far inside the range.
            let pictures = (probe.seconds * rate.as_f64()).round() as i64;
            // Video that is encoded for the disc is cut on any frame.
            any_frame = cut.is_none();
            (
                cut.as_ref()
                    .map(disccraft_dvd::CutPoints::frames)
                    .unwrap_or_default(),
                pictures,
                cut,
            )
        }
    };
    struct Points {
        starts: Vec<i64>,
        pictures: i64,
    }
    let gops = Points { starts, pictures };
    let old = session
        .project
        .timeline(timeline)
        .map(|found| found.video_trim(clip))
        .unwrap_or_default();
    let (old_start, old_end) = (
        i64::from(old.start),
        old.end
            .map_or(gops.pictures, |end| i64::from(end).min(gops.pictures)),
    );
    // The frame of the file under `time`.
    let asked =
        old_start.saturating_add(rate.frame_at(rate.snap_nearest(time)) - rate.frame_at(start));
    // GOP starts, and for the out point the end of the file too.
    let mut points = gops.starts.clone();
    if out_edge {
        points.push(gops.pictures);
    }
    if any_frame {
        // The frame asked for, with at least one frame left of the clip.
        points = vec![if out_edge {
            asked.max(old_start.saturating_add(1)).min(gops.pictures)
        } else {
            asked.min(old_end.saturating_sub(1)).max(0)
        }];
    }
    let nearest = |allowed: &dyn Fn(i64) -> bool| {
        let mut near: Vec<i64> = points
            .iter()
            .copied()
            .filter(|point| allowed(*point) && *point <= gops.pictures)
            .collect();
        near.sort_by_key(|point| (point.saturating_sub(asked).abs(), *point));
        match &h264 {
            // An H.264 clip starts on an IDR picture: the nearest of the key pictures that is one (a few are
            // read from the file to tell).
            Some(cut) if !out_edge => near
                .into_iter()
                .take(MAX_IDR_TRIES)
                .find(|point| cut.can_start(*point)),
            _ => near.first().copied(),
        }
    };
    let (new_start, new_end) = if out_edge {
        let end = nearest(&|point| point > old_start)
            .ok_or_else(|| "the clip has no GOP to keep".to_string())?;
        (old_start, end)
    } else {
        let start = nearest(&|point| point < old_end)
            .ok_or_else(|| "the clip has no GOP to keep".to_string())?;
        (start, old_end)
    };
    let narrow = |frame: i64| u32::try_from(frame).map_err(|_| "the clip is too long".to_string());
    let trim = ClipTrim {
        start: narrow(new_start)?,
        end: (new_end < gops.pictures)
            .then(|| narrow(new_end))
            .transpose()?,
    };
    // What leaves the timeline (a trim that lengthens the clip leaves nothing; the chapters after it move back
    // by that much instead).
    let end_tick = Tick(start.0.saturating_add(length.0));
    let cut = if out_edge {
        let kept = rate.tick_of(new_end.saturating_sub(old_start)).0;
        let new_end_tick = Tick(start.0.saturating_add(kept));
        (
            new_end_tick,
            Tick(end_tick.0.saturating_sub(new_end_tick.0)),
            false,
        )
    } else {
        (
            start,
            rate.tick_of(new_start.saturating_sub(old_start)),
            true,
        )
    };
    if cut.1.0 < 0 {
        // The clip grows (a trim taken back): what follows makes room.
        let by = Tick(cut.1.0.saturating_neg());
        let from = if out_edge { end_tick } else { start };
        session
            .project
            .shift_timeline_chapters(timeline, from, by, !out_edge)
            .map_err(|error| error.to_string())?;
    }
    let cut = (cut.1.0 > 0).then_some(((cut.0, cut.1), cut.2));
    session
        .project
        .trim_timeline_video(timeline, clip, trim, cut)
        .map_err(|error| error.to_string())?;
    // Its linked sound is cut as it is, and the sound of the clips after it moves up with them (M6.379).
    let mapping: Vec<Option<usize>> = (0..before.len()).map(Some).collect();
    follow_video_edit(session, timeline, &before, &mapping)?;
    Ok(object(&[
        ("start", Value::from(trim.start)),
        ("end", trim.end.map_or(Value::Null, Value::from)),
    ]))
}

/// The most key pictures near an in point that are read to find an IDR picture.
const MAX_IDR_TRIES: usize = 64;

/// Where the small indexes of files asked for their cut points are kept (a build keeps its own in its cache).
fn cut_index_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("disccraft-cut-index")
}

/// The clips of the video track, in order: `{"clips": [{"asset", "start", "duration", "sourceStart"}],
/// "duration"}` (ticks; `sourceStart` is where in its file the clip starts playing).
fn run_clips(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let spans = clip_spans(session, timeline)?;
    let total = spans
        .last()
        .map_or(0, |(_, start, length)| start.0.saturating_add(length.0));
    let rate = session.project.settings.frame_rate();
    let found = session.project.timeline(timeline);
    let clips: Vec<Value> = spans
        .iter()
        .enumerate()
        .map(|(clip, (asset, start, length))| {
            // Where in its file the clip starts playing (its trimmed in point).
            let source = found
                .map(|found| rate.tick_of(i64::from(found.video_trim(clip).start)).0)
                .unwrap_or(0);
            object(&[
                ("asset", Value::from(asset.0)),
                ("start", Value::from(start.0)),
                ("duration", Value::from(length.0)),
                ("sourceStart", Value::from(source)),
            ])
        })
        .collect();
    Ok(object(&[
        ("clips", Value::Array(clips)),
        ("duration", Value::from(total)),
    ]))
}

/// Where a chapter asked for at `time` goes, as in Encore CS6: when the timeline's video is an MPEG-2 elementary
/// stream (used as it is, so its GOPs stay), the first frame of the last GOP at or before `time`, or of the next
/// GOP when another chapter (not `moving`) is already there. Other videos are transcoded with a GOP starting at
/// every chapter, so their chapters stay where they are put.
fn gop_chapter_time(
    session: &mut Session,
    timeline: TimelineId,
    time: Tick,
    moving: Option<usize>,
) -> Result<Tick, String> {
    let Some(found) = session.project.timeline(timeline) else {
        return Ok(time);
    };
    // The GOP starts of every clip, each after the frames of the clips before it.
    let mut starts = Vec::new();
    let mut offset = 0_i64;
    for (clip, video) in found.videos().into_iter().enumerate() {
        let Some(path) = session
            .project
            .assets
            .get(&video)
            .map(|asset| asset.media_file().into_owned())
        else {
            return Ok(time);
        };
        let Some(gops) = session.gops.starts(&path) else {
            return Ok(time);
        };
        // The empty stretch in front of the clip has no GOP to put a chapter on.
        offset = offset.saturating_add(i64::from(found.video_gap(clip)));
        // Only the GOPs inside the clip's trim, counted from its in point.
        let trim = found.video_trim(clip);
        let first = i64::from(trim.start);
        let end = trim
            .end
            .map_or(gops.pictures, |end| i64::from(end).min(gops.pictures));
        starts.extend(
            gops.starts
                .iter()
                .filter(|start| **start >= first && **start < end)
                .map(|start| start.saturating_sub(first).saturating_add(offset)),
        );
        offset = offset.saturating_add(end.saturating_sub(first).max(0));
    }
    let rate = session.project.settings.frame_rate();
    let taken = |frame: i64| {
        let at = rate.tick_of(frame);
        found
            .chapters
            .iter()
            .enumerate()
            .any(|(index, chapter)| Some(index) != moving && chapter.time == at)
    };
    let frame = rate.frame_at(rate.snap_nearest(time));
    crate::gop::chapter_frame(&starts, frame, taken)
        .map(|frame| rate.tick_of(frame))
        .ok_or_else(|| "there is already a chapter at this GOP".to_string())
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
    let time = gop_chapter_time(session, timeline, time, Some(chapter))?;

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

/// The parental level a title asks (M6.363): `null` the disc's, 0 none, 1 to 8.
pub(crate) fn parental_level_param(params: &Value) -> Result<Option<u8>, String> {
    match params.get("level") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|level| u8::try_from(level).ok())
            .filter(|level| *level <= 8)
            .map(Some)
            .ok_or_else(|| "parameter \"level\" must be null or 0 to 8".to_string()),
    }
}

/// The title's own parental level (M6.363).
fn run_set_parental_level(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let level = parental_level_param(params)?;
    let Some(found) = session.project.timeline_mut(timeline) else {
        return Err(format!("timeline {} does not exist", timeline.0));
    };
    found.parental_level = level;
    Ok(empty_object())
}

/// What plays in the title's place for a player below its parental level (M6.363).
fn run_set_parental_alternative(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let target = req_link(params, "target")?;
    if target
        == (Link::Timeline {
            id: timeline,
            chapter: 0,
        })
    {
        return Err("a title cannot play itself in its own place".to_string());
    }
    let Some(found) = session.project.timeline_mut(timeline) else {
        return Err(format!("timeline {} does not exist", timeline.0));
    };
    found.parental_alternative = target;
    Ok(empty_object())
}

/// M13.17: another video for a chapter, for players below a parental level (a DVD parental block); `level` null
/// takes it away.
fn run_set_parental_chapter(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let chapter = usize::try_from(req_u64(params, "chapter")?)
        .map_err(|_| "parameter \"chapter\" is too large".to_string())?;
    let level = match params.get("level") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_u64()
                .and_then(|level| u8::try_from(level).ok())
                .filter(|level| (2..=8).contains(level))
                .ok_or_else(|| "parameter \"level\" must be null or 2 to 8".to_string())?,
        ),
    };
    let alternative = match level {
        Some(_) => {
            let asset = AssetId(req_u64(params, "asset")?);
            match session.project.assets.get(&asset) {
                Some(found) if found.kind == disccraft_project::AssetKind::Video => Some(asset),
                Some(_) => return Err(format!("asset {} is not a video", asset.0)),
                None => return Err(format!("asset {} does not exist", asset.0)),
            }
        }
        None => None,
    };
    let Some(found) = session.project.timeline_mut(timeline) else {
        return Err(format!("timeline {} does not exist", timeline.0));
    };
    if chapter >= found.chapters.len() {
        return Err(format!("chapter {} does not exist", chapter + 1));
    }
    found
        .parental_chapters
        .retain(|parental| parental.chapter != chapter);
    if let (Some(level), Some(alternative)) = (level, alternative) {
        if found.parental_chapters.len() >= disccraft_project::MAX_PARENTAL_CHAPTERS {
            return Err(format!(
                "a timeline has at most {} chapters with a parental alternative",
                disccraft_project::MAX_PARENTAL_CHAPTERS
            ));
        }
        found
            .parental_chapters
            .push(disccraft_project::ParentalChapter {
                chapter,
                level,
                alternative,
            });
        found
            .parental_chapters
            .sort_by_key(|parental| parental.chapter);
    }
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

/// One colour of a colour set from its parameters: `[r, g, b]` and an opacity in percent.
fn subtitle_color(group: &Value, color: &str, opacity: &str) -> Result<SubtitleColor, String> {
    let channels = group
        .get(color)
        .and_then(Value::as_array)
        .filter(|channels| channels.len() == 3)
        .ok_or_else(|| format!("\"{color}\" must be [r, g, b]"))?;
    let mut rgb = [0_u8; 3];
    for (slot, channel) in rgb.iter_mut().zip(channels) {
        *slot = channel
            .as_u64()
            .and_then(|value| u8::try_from(value).ok())
            .ok_or_else(|| format!("\"{color}\" must be [r, g, b] of 0 to 255"))?;
    }
    let opacity = match group.get(opacity) {
        None | Some(Value::Null) => 100,
        Some(value) => value
            .as_u64()
            .and_then(|value| u8::try_from(value).ok())
            .filter(|value| *value <= 100)
            .ok_or_else(|| format!("\"{opacity}\" must be 0 to 100"))?,
    };
    Ok(SubtitleColor { rgb, opacity })
}

/// The name a colour set file says it is.
const COLOR_SET_FORMAT: &str = "disccraft.subtitleColorSet";
/// The largest colour set file read.
const MAX_COLOR_SET_BYTES: u64 = 64 * 1024;

/// The colour set of the timeline or slideshow the parameters name (`"timeline"` or `"slideshow"`).
fn owner_colors(session: &Session, params: &Value) -> Result<SubtitleColorSet, String> {
    if let Some(id) = params.get("timeline").and_then(Value::as_u64) {
        return session
            .project
            .timeline(TimelineId(id))
            .map(|timeline| timeline.subtitle_colors.unwrap_or_default())
            .ok_or_else(|| format!("timeline {id} does not exist"));
    }
    let id = req_u64(params, "slideshow")
        .map_err(|_| "give \"timeline\" or \"slideshow\"".to_string())?;
    session
        .project
        .slideshow(id)
        .map(|slideshow| slideshow.subtitle_colors.unwrap_or_default())
        .ok_or_else(|| format!("slideshow {id} does not exist"))
}

/// Writes a timeline's or a slideshow's subtitle colour set to a file (Encore: the export button of the Timeline
/// Color Set dialog), to be read into another timeline, slideshow or project.
fn run_export_color_set(session: &mut Session, params: &Value) -> CommandResult {
    let path = req_str(params, "path")?;
    let set = owner_colors(session, params)?;
    let groups: Vec<Value> = set
        .groups
        .iter()
        .map(|group| {
            let mut entry = vec![
                ("fill", serde_json::json!(group.fill.rgb)),
                ("fillOpacity", Value::from(group.fill.opacity)),
                ("stroke", serde_json::json!(group.stroke.rgb)),
                ("strokeOpacity", Value::from(group.stroke.opacity)),
            ];
            if let Some(color) = group.antialias {
                entry.push(("antialias", serde_json::json!(color.rgb)));
                entry.push(("antialiasOpacity", Value::from(color.opacity)));
            }
            object(&entry)
        })
        .collect();
    let file = object(&[
        ("format", Value::String(COLOR_SET_FORMAT.to_owned())),
        ("version", Value::from(1)),
        ("groups", Value::Array(groups)),
    ]);
    let text = serde_json::to_string_pretty(&file).map_err(|error| error.to_string())?;
    std::fs::write(&path, text).map_err(|error| format!("{path}: {error}"))?;
    Ok(object(&[("path", Value::String(path))]))
}

/// Reads a colour set file into a timeline or a slideshow (Encore: the import button of the Timeline Color Set
/// dialog).
fn run_import_color_set(session: &mut Session, params: &Value) -> CommandResult {
    let path = req_str(params, "path")?;
    let size = std::fs::metadata(&path)
        .map_err(|error| format!("{path}: {error}"))?
        .len();
    if size > MAX_COLOR_SET_BYTES {
        return Err(format!("{path}: not a colour set file (too large)"));
    }
    let text = std::fs::read_to_string(&path).map_err(|error| format!("{path}: {error}"))?;
    let file: Value =
        serde_json::from_str(&text).map_err(|_| format!("{path}: not a colour set file"))?;
    if file.get("format").and_then(Value::as_str) != Some(COLOR_SET_FORMAT) {
        return Err(format!("{path}: not a colour set file"));
    }
    let colors = color_set_param(&file)?.ok_or_else(|| format!("{path}: no colour groups"))?;
    if let Some(id) = params.get("timeline").and_then(Value::as_u64) {
        session
            .project
            .set_timeline_subtitle_colors(TimelineId(id), Some(colors))
            .map_err(|error| error.to_string())?;
    } else {
        let id = req_u64(params, "slideshow")
            .map_err(|_| "give \"timeline\" or \"slideshow\"".to_string())?;
        session
            .project
            .set_slideshow_subtitle_colors(id, Some(colors))
            .map_err(|error| error.to_string())?;
    }
    Ok(empty_object())
}

/// Gives the timeline or slideshow the parameters name a colour set.
fn set_owner_colors(
    session: &mut Session,
    params: &Value,
    colors: Option<SubtitleColorSet>,
) -> Result<(), String> {
    if let Some(id) = params.get("timeline").and_then(Value::as_u64) {
        return session
            .project
            .set_timeline_subtitle_colors(TimelineId(id), colors)
            .map_err(|error| error.to_string());
    }
    let id = req_u64(params, "slideshow")
        .map_err(|_| "give \"timeline\" or \"slideshow\"".to_string())?;
    session
        .project
        .set_slideshow_subtitle_colors(id, colors)
        .map_err(|error| error.to_string())
}

/// Keeps a timeline's or a slideshow's colours in the project under a name (Encore: New in the Timeline Color
/// Set dialog).
fn run_save_color_set(session: &mut Session, params: &Value) -> CommandResult {
    let name = req_str(params, "name")?;
    let set = owner_colors(session, params)?;
    session
        .project
        .save_subtitle_color_set(&name, set)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

/// Gives a timeline or a slideshow the colours of a named set of the project.
fn run_apply_color_set(session: &mut Session, params: &Value) -> CommandResult {
    let name = req_str(params, "name")?;
    let set = session
        .project
        .subtitle_color_sets
        .get(name.trim())
        .copied()
        .ok_or_else(|| format!("there is no colour set \"{name}\""))?;
    set_owner_colors(session, params, Some(set))?;
    Ok(empty_object())
}

/// Takes a named colour set out of the project (Encore: Delete in the Timeline Color Set dialog).
fn run_delete_color_set(session: &mut Session, params: &Value) -> CommandResult {
    let name = req_str(params, "name")?;
    session
        .project
        .remove_subtitle_color_set(&name)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

/// The `"groups"` of a colour set command: three groups of a fill and a stroke, or `null` for the default set.
pub(crate) fn color_set_param(params: &Value) -> Result<Option<SubtitleColorSet>, String> {
    let colors = match params.get("groups") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let groups = value
                .as_array()
                .filter(|groups| groups.len() == 3)
                .ok_or_else(|| "\"groups\" must be three groups or null".to_string())?;
            let mut set = SubtitleColorSet::default();
            for (slot, group) in set.groups.iter_mut().zip(groups) {
                *slot = SubtitleGroup {
                    fill: subtitle_color(group, "fill", "fillOpacity")?,
                    stroke: subtitle_color(group, "stroke", "strokeOpacity")?,
                    // Left out: made from the fill and the stroke.
                    antialias: match group.get("antialias") {
                        None | Some(Value::Null) => None,
                        Some(_) => Some(subtitle_color(group, "antialias", "antialiasOpacity")?),
                    },
                };
            }
            Some(set)
        }
    };
    Ok(colors)
}

/// The timeline colour set (Encore's Edit Timeline Color Set): three groups of a fill and a stroke; `null` goes
/// back to the default set.
fn run_set_subtitle_colors(session: &mut Session, params: &Value) -> CommandResult {
    let id = TimelineId(req_u64(params, "timeline")?);
    let colors = color_set_param(params)?;
    session
        .project
        .set_timeline_subtitle_colors(id, colors)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

/// The colour group (1 to 3) a subtitle track's subtitles use.
fn run_set_subtitle_group(session: &mut Session, params: &Value) -> CommandResult {
    let id = TimelineId(req_u64(params, "timeline")?);
    let track = req_usize(params, "track")?;
    let group = req_u64(params, "group")?
        .checked_sub(1)
        .and_then(|group| u8::try_from(group).ok())
        .ok_or_else(|| "\"group\" must be 1 to 3".to_string())?;
    session
        .project
        .set_subtitle_track_group(id, track, group)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

/// The sequences of a FilmCraft project and how many markers each has (for choosing one to take chapters from).
fn run_filmcraft_sequences(_session: &mut Session, params: &Value) -> CommandResult {
    let path = req_str(params, "path")?;
    let sequences = disccraft_film::read_sequences(std::path::Path::new(&path))
        .map_err(|error| error.to_string())?;
    let list: Vec<Value> = sequences
        .iter()
        .map(|sequence| {
            let chapters = sequence
                .markers
                .iter()
                .filter(|marker| marker.chapter)
                .count();
            object(&[
                ("id", Value::from(sequence.id)),
                ("name", Value::String(sequence.name.clone())),
                ("duration", Value::from(sequence.duration.0)),
                ("markers", Value::from(sequence.markers.len())),
                ("chapterMarkers", Value::from(chapters)),
            ])
        })
        .collect();
    Ok(object(&[("sequences", Value::Array(list))]))
}

/// The most chapters a title holds.
const MAX_CHAPTERS: usize = 99;

/// Chapter points from a FilmCraft sequence's markers (Encore makes chapters of Premiere Pro's chapter markers):
/// the chapter markers, or every marker with `"all": true`, each snapped to a GOP start like a chapter added by
/// hand and named after the marker. A marker at the very start names the first chapter. Markers past the end of
/// the timeline, on a taken place, or beyond 99 chapters are left out and counted.
/// Adds the chapters the timeline's video files carry (Matroska chapters, an MP4's chapter track) as chapter
/// points, each where it falls in its clip; a chapter at a clip's very start names the chapter that is there.
/// Returns how many were added and how many were left out (outside the clip's used part, on a point that is
/// there already, past the 99).
fn source_chapters(session: &mut Session, timeline: TimelineId) -> Result<(u64, u64), String> {
    const TICKS_PER_SECOND: f64 = 254_016_000_000.0;
    let spans = clip_spans(session, timeline)?;
    let rate = session.project.settings.frame_rate();
    let (mut added, mut skipped) = (0_u64, 0_u64);
    for (clip, (asset, start, length)) in spans.into_iter().enumerate() {
        let Some((path, trim)) = session.project.timeline(timeline).and_then(|found| {
            let asset = session.project.assets.get(&asset)?;
            Some((asset.media_file().into_owned(), found.video_trim(clip)))
        }) else {
            continue;
        };
        // Where the clip's used part starts in its file.
        let source_in = rate.tick_of(i64::from(trim.start));
        for (seconds, title) in disccraft_transcode::file_chapters(&path) {
            // `as` saturates; a chapter's time is far inside the range.
            let in_file = Tick((seconds * TICKS_PER_SECOND) as i64);
            let offset = in_file.0.saturating_sub(source_in.0);
            let name = (!title.is_empty()).then_some(title);
            if offset < 0 || offset >= length.0 {
                skipped = skipped.saturating_add(1);
                continue;
            }
            let at = Tick(start.0.saturating_add(offset));
            if at <= Tick::ZERO {
                // The first chapter is always at the start: the file's chapter gives it its name.
                if let Some(name) = name {
                    let _ = session.project.rename_chapter(timeline, 0, &name);
                }
                continue;
            }
            let count = session
                .project
                .timeline(timeline)
                .map_or(0, |found| found.chapters.len());
            if count >= MAX_CHAPTERS {
                skipped = skipped.saturating_add(1);
                continue;
            }
            let time = gop_chapter_time(session, timeline, at, None)?;
            match session.project.add_chapter(timeline, time, name) {
                Ok(_) => added = added.saturating_add(1),
                Err(_) => skipped = skipped.saturating_add(1),
            }
        }
    }
    Ok((added, skipped))
}

fn run_import_source_chapters(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    // "replace": the chapter points there are go first (Encore's Update Markers from Source (Replace All
    // Markers)); the first chapter, at the start, always stays.
    let replace = matches!(params.get("replace"), Some(Value::Bool(true)));
    if replace {
        let count = session
            .project
            .timeline(timeline)
            .ok_or_else(|| format!("timeline {} does not exist", timeline.0))?
            .chapters
            .len();
        for at in (1..count).rev() {
            session
                .project
                .remove_chapter(timeline, at)
                .map_err(|error| error.to_string())?;
        }
    }
    let (added, skipped) = source_chapters(session, timeline)?;
    if replace && added == 0 {
        // Nothing to put in their place: the command fails, and the chapter points stay as they were.
        return Err(
            "the source has no chapter markers to replace the chapter points with".to_string(),
        );
    }
    Ok(object(&[
        ("added", Value::from(added)),
        ("skipped", Value::from(skipped)),
    ]))
}

fn run_import_filmcraft_markers(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let path = req_str(params, "path")?;
    let wanted = opt_str(params, "sequence")?;
    let all = params.get("all").and_then(Value::as_bool).unwrap_or(false);

    let (timeline_name, video_name) = {
        let found = session
            .project
            .timeline(timeline)
            .ok_or_else(|| format!("timeline {} does not exist", timeline.0))?;
        let video = session
            .project
            .assets
            .get(&found.video)
            .map(|asset| asset.name.clone())
            .unwrap_or_default();
        (found.name.clone(), video)
    };
    let sequences = disccraft_film::read_sequences(std::path::Path::new(&path))
        .map_err(|error| error.to_string())?;
    let sequence = disccraft_film::choose_sequence(
        &sequences,
        wanted.as_deref(),
        &[timeline_name.as_str(), video_name.as_str()],
    )
    .ok_or_else(|| {
        let names: Vec<&str> = sequences.iter().map(|s| s.name.as_str()).collect();
        match &wanted {
            Some(wanted) => format!("the project has no sequence \"{wanted}\""),
            None if names.is_empty() => "the project has no sequences".to_string(),
            None => format!("choose a sequence: {}", names.join(", ")),
        }
    })?;

    let end = clip_spans(session, timeline)?
        .last()
        .map_or(Tick::ZERO, |(_, start, length)| {
            Tick(start.0.saturating_add(length.0))
        });
    let (mut added, mut skipped) = (0_u64, 0_u64);
    for marker in sequence
        .markers
        .iter()
        .filter(|marker| all || marker.chapter)
    {
        if marker.time <= Tick::ZERO {
            // The first chapter is always at the start: the marker gives it its name.
            if marker.time == Tick::ZERO && !marker.name.is_empty() {
                session
                    .project
                    .rename_chapter(timeline, 0, &marker.name)
                    .map_err(|error| error.to_string())?;
            }
            continue;
        }
        let count = session
            .project
            .timeline(timeline)
            .map_or(0, |found| found.chapters.len());
        if marker.time >= end || count >= MAX_CHAPTERS {
            skipped = skipped.saturating_add(1);
            continue;
        }
        let time = gop_chapter_time(session, timeline, marker.time, None)?;
        let name = (!marker.name.is_empty()).then(|| marker.name.clone());
        match session.project.add_chapter(timeline, time, name) {
            Ok(_) => added = added.saturating_add(1),
            Err(_) => skipped = skipped.saturating_add(1),
        }
    }
    Ok(object(&[
        ("sequence", Value::String(sequence.name.clone())),
        ("added", Value::from(added)),
        ("skipped", Value::from(skipped)),
    ]))
}

fn run_add_subtitle_track(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let asset = AssetId(req_u64(params, "asset")?);
    let language = opt_str(params, "language")?.unwrap_or_default();
    match session.project.assets.get(&asset) {
        Some(a) if a.kind == AssetKind::Subtitle => {}
        Some(_) => {
            return Err(format!(
                "asset {} is not a subtitle asset (.srt, .vtt, .txt, .idx or .sup)",
                asset.0
            ));
        }
        None => return Err(format!("asset {} does not exist", asset.0)),
    }
    let Some(timeline) = session.project.timeline_mut(timeline) else {
        return Err(format!("timeline {} does not exist", timeline.0));
    };
    if timeline.subtitles.len() >= 32 {
        return Err("DVD timelines support at most 32 subtitle tracks".to_string());
    }
    timeline.subtitles.push(SubtitleTrack {
        asset,
        language,
        group: 0,
    });
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
        more: Vec::new(),
        asset: asset_id,
        language,
        start: 0,
        trim_start: 0,
        trim_end: None,
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
/// The length of an audio clip's file in frames of the disc's standard rate, when it can be told.
fn audio_file_frames(session: &Session, asset: AssetId) -> Option<i64> {
    let rate = session.project.settings.frame_rate();
    session
        .project
        .assets
        .get(&asset)
        .and_then(|asset| disccraft_transcode::probe_audio_seconds(&asset.media_file()).ok())
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        // `as` saturates; a clip's length is far inside the range.
        .map(|seconds| ((seconds * rate.as_f64()).round() as i64).max(1))
}

/// Where a clip sounds on the timeline, frames: its start and its end (`None`: its file's length is not known,
/// so it is taken to go on to the end of the timeline).
fn audio_clip_span(session: &Session, clip: &disccraft_project::AudioClip) -> (i64, Option<i64>) {
    let start = i64::from(clip.start);
    let file_end = clip
        .trim_end
        .map(i64::from)
        .or_else(|| audio_file_frames(session, clip.asset));
    let length = file_end.map(|end| end.saturating_sub(i64::from(clip.trim_start)).max(1));
    (start, length.map(|length| start.saturating_add(length)))
}

/// The room clip `number` of `track` has on its track, frames: where the clip before it ends (0 without one)
/// and where the clip after it starts (`None` without one).
fn audio_clip_room(
    session: &Session,
    track: &disccraft_project::AudioTrack,
    number: usize,
) -> (i64, Option<i64>) {
    let clips = track.clips();
    let at = clips.iter().position(|(other, _)| *other == number);
    let before = at
        .and_then(|at| at.checked_sub(1))
        .and_then(|at| clips.get(at))
        .map(|(_, clip)| {
            let (start, end) = audio_clip_span(session, clip);
            // A clip of unknown length: at least its first frame is its own.
            end.unwrap_or_else(|| start.saturating_add(1))
        });
    let after = at
        .and_then(|at| clips.get(at.saturating_add(1)))
        .map(|(_, clip)| i64::from(clip.start));
    (before.unwrap_or(0), after)
}

/// The optional "clip" parameter: which clip of the track (0, the first, when left out).
fn clip_param(params: &Value) -> Result<usize, String> {
    match params.get("clip") {
        None | Some(Value::Null) => Ok(0),
        Some(_) => req_usize(params, "clip"),
    }
}

/// Adds a clip to an audio track (Encore: a sound file dropped on a track that has a clip): at the time given,
/// or right after the track's last clip. A clip does not go over another: one that would is refused.
fn run_add_audio_clip(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let track = req_usize(params, "track")?;
    let asset = AssetId(req_u64(params, "asset")?);
    let kind = session
        .project
        .assets
        .get(&asset)
        .map(|asset| asset.kind)
        .ok_or_else(|| format!("asset {} does not exist", asset.0))?;
    if !matches!(kind, AssetKind::Audio | AssetKind::Video) {
        return Err(format!("asset {} has no sound", asset.0));
    }
    let rate = session.project.settings.frame_rate();
    let found = session
        .project
        .timeline(timeline)
        .ok_or_else(|| format!("timeline {} does not exist", timeline.0))?;
    let old = found
        .audio
        .get(track)
        .cloned()
        .ok_or_else(|| format!("audio track {track} does not exist"))?;
    if old.more.len().saturating_add(1) >= disccraft_project::MAX_AUDIO_CLIPS {
        return Err(format!(
            "an audio track holds at most {} clips",
            disccraft_project::MAX_AUDIO_CLIPS
        ));
    }
    let spans: Vec<(i64, Option<i64>)> = old
        .clips()
        .iter()
        .map(|(_, clip)| audio_clip_span(session, clip))
        .collect();
    let asked = if params.get("time").is_some() || params.get("timecode").is_some() {
        let time = time_param(params, &session.project.settings)?;
        Some(rate.frame_at(rate.snap_nearest(Tick(time.0.max(0)))))
    } else {
        None
    };
    let start = match asked {
        Some(start) => start,
        // Right after the last clip.
        None => spans
            .iter()
            .map(|(start, end)| end.unwrap_or_else(|| start.saturating_add(1)))
            .max()
            .unwrap_or(0),
    };
    let length = audio_file_frames(session, asset);
    let end = length.map(|length| start.saturating_add(length));
    let overlaps = spans.iter().any(|(other_start, other_end)| {
        let ends_before = end.is_some_and(|end| end <= *other_start);
        let starts_after = other_end.is_some_and(|other_end| start >= other_end);
        !(ends_before || starts_after)
    });
    if overlaps {
        return Err(
            "the clip would lie over another clip of the track; put it where there is room"
                .to_string(),
        );
    }
    let clip = disccraft_project::AudioClip {
        asset,
        start: u32::try_from(start).map_err(|_| "the clip is too far along".to_string())?,
        trim_start: 0,
        trim_end: None,
    };
    let Some(slot) = session
        .project
        .timeline_mut(timeline)
        .and_then(|found| found.audio.get_mut(track))
    else {
        return Err(format!("audio track {track} does not exist"));
    };
    slot.more.push(clip);
    Ok(object(&[
        ("clip", Value::from(slot.more.len())),
        ("start", Value::from(clip.start)),
    ]))
}

/// Takes a clip off an audio track (Encore: Clear on an audio clip). The track's only clip stays: the track
/// itself is removed with `timeline.removeAudioTrack`.
fn run_remove_audio_clip(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let track = req_usize(params, "track")?;
    let clip = req_usize(params, "clip")?;
    let Some(slot) = session
        .project
        .timeline_mut(timeline)
        .ok_or_else(|| format!("timeline {} does not exist", timeline.0))?
        .audio
        .get_mut(track)
    else {
        return Err(format!("audio track {track} does not exist"));
    };
    if slot.clip(clip).is_none() {
        return Err(format!("audio clip {clip} does not exist"));
    }
    if !slot.remove_clip(clip) {
        return Err("an audio track's only clip cannot be removed; remove the track".to_string());
    }
    Ok(empty_object())
}

/// Moves an audio clip along its track (Encore: dragging an audio clip): its sound starts at the frame nearest
/// the time given, and the track is silent before it. On a track of several clips the clip stays between its
/// neighbours: it stops where the one before it ends and where the one after it starts.
fn run_move_audio(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let track = req_usize(params, "track")?;
    let number = clip_param(params)?;
    let time = time_param(params, &session.project.settings)?;
    let rate = session.project.settings.frame_rate();
    let frame = rate.frame_at(rate.snap_nearest(Tick(time.0.max(0))));
    let found = session
        .project
        .timeline(timeline)
        .ok_or_else(|| format!("timeline {} does not exist", timeline.0))?;
    let old_track = found
        .audio
        .get(track)
        .cloned()
        .ok_or_else(|| format!("audio track {track} does not exist"))?;
    let mut clip = old_track
        .clip(number)
        .ok_or_else(|| format!("audio clip {number} does not exist"))?;
    let (earliest, next_start) = audio_clip_room(session, &old_track, number);
    let (old_start, old_end) = audio_clip_span(session, &clip);
    let length = old_end.map(|end| end.saturating_sub(old_start));
    // No later than where it still ends in front of the next clip (when its length is known).
    let latest = match (next_start, length) {
        (Some(next), Some(length)) => next.saturating_sub(length).max(earliest),
        (Some(next), None) => next.saturating_sub(1).max(earliest),
        (None, _) => i64::MAX,
    };
    let start = frame.max(0).clamp(earliest, latest);
    let start = u32::try_from(start).map_err(|_| "the clip is too far along".to_string())?;
    clip.start = start;
    let at = rate.tick_of(i64::from(start));
    let Some(slot) = session
        .project
        .timeline_mut(timeline)
        .and_then(|found| found.audio.get_mut(track))
    else {
        return Err(format!("audio track {track} does not exist"));
    };
    let _ = slot.set_clip(number, clip);
    Ok(object(&[
        ("start", Value::from(at.0)),
        (
            "timecode",
            Value::String(format_time(at, &session.project.settings)),
        ),
        ("frame", Value::from(start)),
    ]))
}

/// Trims an audio clip's in or out point to the time given on the timeline (M6.254). The sound that stays keeps
/// its place: trimming the in point moves the clip's start along with it. At least one frame stays; an out point
/// at or past the file's end plays to the end. On a track of several clips an edge does not pass the clip next
/// to it. `{"start", "trimStart", "trimEnd"}` in frames (`trimEnd` null: to the end of the file).
fn run_trim_audio(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let track = req_usize(params, "track")?;
    let number = clip_param(params)?;
    let out_edge = match req_str(params, "edge")?.as_str() {
        "in" => false,
        "out" => true,
        other => {
            return Err(format!(
                "parameter \"edge\" must be \"in\" or \"out\", not \"{other}\""
            ));
        }
    };
    let time = time_param(params, &session.project.settings)?;
    let rate = session.project.settings.frame_rate();
    let asked = rate.frame_at(rate.snap_nearest(time));
    let found = session
        .project
        .timeline(timeline)
        .ok_or_else(|| format!("timeline {} does not exist", timeline.0))?;
    let old_track = found
        .audio
        .get(track)
        .cloned()
        .ok_or_else(|| format!("audio track {track} does not exist"))?;
    let old = old_track
        .clip(number)
        .ok_or_else(|| format!("audio clip {number} does not exist"))?;
    let (earliest, next_start) = audio_clip_room(session, &old_track, number);
    // The file's length in frames, when it can be told.
    let length = audio_file_frames(session, old.asset);
    let (start, trim_start) = (i64::from(old.start), i64::from(old.trim_start));
    let old_end = old
        .trim_end
        .map(i64::from)
        .or(length)
        .map(|end| length.map_or(end, |length| end.min(length)));
    let narrow = |frame: i64| u32::try_from(frame).map_err(|_| "the clip is too long".to_string());
    let mut new = old;
    if out_edge {
        // No further than where the next clip starts.
        let asked = next_start.map_or(asked, |next| asked.min(next));
        // The frame of the file under the time; at least one frame past the in point.
        let end = trim_start
            .saturating_add(asked.saturating_sub(start))
            .max(trim_start.saturating_add(1));
        new.trim_end = match length {
            Some(length) if end >= length => {
                // To the end of the file, unless that would run into the next clip.
                match next_start {
                    Some(next)
                        if start.saturating_add(length.saturating_sub(trim_start)) > next =>
                    {
                        Some(narrow(
                            trim_start
                                .saturating_add(next.saturating_sub(start))
                                .max(trim_start.saturating_add(1)),
                        )?)
                    }
                    _ => None,
                }
            }
            _ => Some(narrow(end)?),
        };
    } else {
        // No further back than where the clip before it ends.
        let asked = asked.max(earliest);
        // The in point moves by as much as the clip's head does; no further back than the file's head or
        // the timeline's, and one frame before the out point at most.
        let mut by = asked.saturating_sub(start).max(-trim_start).max(-start);
        if let Some(end) = old_end {
            by = by.min(end.saturating_sub(1).saturating_sub(trim_start));
        }
        new.trim_start = narrow(trim_start.saturating_add(by))?;
        new.start = narrow(start.saturating_add(by))?;
    }
    let Some(slot) = session
        .project
        .timeline_mut(timeline)
        .and_then(|found| found.audio.get_mut(track))
    else {
        return Err(format!("audio track {track} does not exist"));
    };
    let _ = slot.set_clip(number, new);
    Ok(object(&[
        ("start", Value::from(new.start)),
        ("trimStart", Value::from(new.trim_start)),
        ("trimEnd", new.trim_end.map_or(Value::Null, Value::from)),
    ]))
}

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

    /// M13.17: a chapter gets a parental alternative (a video, for players below a level from 2 to 8), another one
    /// replaces it, `level` null takes it away; a sound, a chapter that does not exist and a level out of range are
    /// refused.
    #[test]
    fn a_chapter_gets_a_parental_alternative() -> Result<(), Box<dyn Error>> {
        let dir =
            std::env::temp_dir().join(format!("disccraft-engine-parental-{}", std::process::id()));
        fs::create_dir_all(&dir)?;
        let movie = dir.join("movie.m2v");
        let bars = dir.join("bars.m2v");
        let sound = dir.join("sound.wav");
        for path in [&movie, &bars, &sound] {
            fs::write(path, b"")?;
        }
        let mut session = Session::new();
        let video = session.project.add_asset(&movie)?;
        let alternative = session.project.add_asset(&bars)?;
        let audio = session.project.add_asset(&sound)?;
        let timeline = session.project.add_timeline(video, None)?;
        let set = |session: &mut Session, params: Value| {
            session.execute("timeline.setParentalChapter", &params)
        };
        set(
            &mut session,
            serde_json::json!({ "timeline": timeline.0, "chapter": 0, "level": 5, "asset": alternative.0 }),
        )?;
        set(
            &mut session,
            serde_json::json!({ "timeline": timeline.0, "chapter": 0, "level": 3, "asset": alternative.0 }),
        )?;
        let chapters = |session: &Session| {
            session
                .project
                .timeline(timeline)
                .map(|found| found.parental_chapters.clone())
                .unwrap_or_default()
        };
        assert_eq!(
            chapters(&session),
            [disccraft_project::ParentalChapter {
                chapter: 0,
                level: 3,
                alternative,
            }]
        );
        for refused in [
            serde_json::json!({ "timeline": timeline.0, "chapter": 0, "level": 5, "asset": audio.0 }),
            serde_json::json!({ "timeline": timeline.0, "chapter": 1, "level": 5, "asset": alternative.0 }),
            serde_json::json!({ "timeline": timeline.0, "chapter": 0, "level": 1, "asset": alternative.0 }),
            serde_json::json!({ "timeline": timeline.0, "chapter": 0, "level": 9, "asset": alternative.0 }),
            serde_json::json!({ "timeline": timeline.0, "chapter": 0, "level": 5 }),
        ] {
            assert!(set(&mut session, refused.clone()).is_err(), "{refused}");
        }
        set(
            &mut session,
            serde_json::json!({ "timeline": timeline.0, "chapter": 0, "level": null }),
        )?;
        assert!(chapters(&session).is_empty());
        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }

    /// As in Encore CS6, a chapter on an MPEG-2 elementary stream goes to the start of the GOP before it, or to
    /// the next GOP when a chapter is already there; moving a chapter snaps the same way.
    #[test]
    fn chapters_snap_to_gop_starts() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-engine-gop-{}", std::process::id()));
        fs::create_dir_all(&dir)?;
        let video = dir.join("clip.m2v");
        // A sequence header, then ten GOPs of 13 pictures (start codes only).
        let mut bytes = vec![0_u8, 0, 1, 0xB3, 0x2D, 0x01, 0xE0];
        for _ in 0..10 {
            bytes.extend_from_slice(&[0, 0, 1, 0xB8, 0, 8]);
            for _ in 0..13 {
                bytes.extend_from_slice(&[0, 0, 1, 0x00, 0, 0x0F]);
            }
        }
        fs::write(&video, bytes)?;

        let mut session = Session::new();
        let asset = session.project.add_asset(&video)?;
        let timeline = session.project.add_timeline(asset, None)?;
        let add = |session: &mut Session, timecode: &str| {
            session.execute(
                "timeline.addChapter",
                &serde_json::json!({ "timeline": timeline.0, "timecode": timecode }),
            )
        };
        let timecode = |value: &Value| {
            value
                .get("timecode")
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        // Frame 30 is in the GOP that starts on frame 26; a second chapter there goes to the next GOP (39).
        assert_eq!(
            timecode(&add(&mut session, "00;00;01;00")?).as_deref(),
            Some("00;00;00;26")
        );
        assert_eq!(
            timecode(&add(&mut session, "00;00;01;00")?).as_deref(),
            Some("00;00;01;09")
        );
        // Frame 5 is in the first GOP, where chapter 1 sits: the next GOP (13).
        assert_eq!(
            timecode(&add(&mut session, "00;00;00;05")?).as_deref(),
            Some("00;00;00;13")
        );
        // Moving the chapter on frame 39 to frame 70 snaps to the GOP on frame 65.
        session.execute(
            "timeline.moveChapter",
            &serde_json::json!({ "timeline": timeline.0, "chapter": 3, "timecode": "00;00;02;10" }),
        )?;
        let times: Vec<i64> = session
            .project
            .timeline(timeline)
            .ok_or("timeline")?
            .chapters
            .iter()
            .map(|chapter| session.project.settings.frame_rate().frame_at(chapter.time))
            .collect();
        assert_eq!(times, vec![0, 13, 26, 65]);

        // A second clip is butted on to the end (130 frames in) with a chapter at the join, and chapters on it
        // snap to its GOPs.
        let appended = session.execute(
            "timeline.appendVideo",
            &serde_json::json!({ "timeline": timeline.0, "asset": asset.0 }),
        )?;
        assert_eq!(timecode(&appended).as_deref(), Some("00;00;04;10"));
        assert_eq!(appended.get("clip").and_then(Value::as_u64), Some(1));
        // Frame 150 is in the second clip's GOP that starts on frame 143.
        assert_eq!(
            timecode(&add(&mut session, "00;00;05;00")?).as_deref(),
            Some("00;00;04;23")
        );
        let clips = session.execute(
            "timeline.clips",
            &serde_json::json!({ "timeline": timeline.0 }),
        )?;
        assert_eq!(
            clips.pointer("/clips/1/start").and_then(Value::as_i64),
            Some(rate_tick(130))
        );
        assert_eq!(
            clips.get("duration").and_then(Value::as_i64),
            Some(rate_tick(260))
        );
        // Removing the clip takes its chapters with it; the first clip cannot go.
        session.execute(
            "timeline.removeVideo",
            &serde_json::json!({ "timeline": timeline.0, "clip": 1 }),
        )?;
        let found = session.project.timeline(timeline).ok_or("timeline")?;
        assert!(found.more_video.is_empty());
        assert_eq!(found.chapters.len(), 4);
        // A timeline's only clip stays.
        assert!(
            session
                .execute(
                    "timeline.removeVideo",
                    &serde_json::json!({ "timeline": timeline.0, "clip": 0 }),
                )
                .is_err()
        );
        // One step back: the clip and its chapters return.
        session.undo()?;
        // With two clips the first can go too: the second takes its place, and its chapters move to the start.
        session.execute(
            "timeline.removeVideo",
            &serde_json::json!({ "timeline": timeline.0, "clip": 0 }),
        )?;
        let found = session.project.timeline(timeline).ok_or("timeline")?;
        assert!(found.more_video.is_empty());
        let frames: Vec<i64> = found
            .chapters
            .iter()
            .map(|chapter| session.project.settings.frame_rate().frame_at(chapter.time))
            .collect();
        // The join's chapter lands on the start (one chapter there), the one at frame 143 on frame 13.
        assert_eq!(frames, vec![0, 13]);
        session.undo()?;
        let found = session.project.timeline(timeline).ok_or("timeline")?;
        assert_eq!(found.more_video.len(), 1);
        assert_eq!(found.chapters.len(), 6);
        // M6.267: Clear ("ripple": false) leaves the clip's place empty: the second clip stays where it plays,
        // with the first one's 130 frames of empty track in front of it, and so do its chapters.
        session.execute(
            "timeline.removeVideo",
            &serde_json::json!({ "timeline": timeline.0, "clip": 0, "ripple": false }),
        )?;
        let found = session.project.timeline(timeline).ok_or("timeline")?;
        assert!(found.more_video.is_empty());
        assert_eq!(found.video_gap(0), 130);
        let frames: Vec<i64> = found
            .chapters
            .iter()
            .map(|chapter| session.project.settings.frame_rate().frame_at(chapter.time))
            .collect();
        assert_eq!(frames, vec![0, 130, 143]);
        let clips = session.execute(
            "timeline.clips",
            &serde_json::json!({ "timeline": timeline.0 }),
        )?;
        assert_eq!(
            clips.get("duration").and_then(Value::as_i64),
            Some(rate_tick(260))
        );
        session.undo()?;
        let found = session.project.timeline(timeline).ok_or("timeline")?;
        assert_eq!((found.more_video.len(), found.chapters.len()), (1, 6));
        // Clearing the last clip is a plain removal: nothing follows it.
        session.execute(
            "timeline.removeVideo",
            &serde_json::json!({ "timeline": timeline.0, "clip": 1, "ripple": false }),
        )?;
        let found = session.project.timeline(timeline).ok_or("timeline")?;
        assert!(found.more_video.is_empty() && !found.has_video_gaps());
        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }

    /// Trimming a clip, as measured in Encore CS6 on a GOP-13 clip: the out point goes to the nearest GOP start,
    /// the in point too; the clips stay butted and the chapters follow.
    #[test]
    fn clips_are_trimmed_to_gop_starts() -> Result<(), Box<dyn Error>> {
        let dir =
            std::env::temp_dir().join(format!("disccraft-engine-trim-{}", std::process::id()));
        fs::create_dir_all(&dir)?;
        let video = dir.join("clip.m2v");
        let mut bytes = vec![0_u8, 0, 1, 0xB3, 0x2D, 0x01, 0xE0];
        for _ in 0..10 {
            bytes.extend_from_slice(&[0, 0, 1, 0xB8, 0, 8]);
            for _ in 0..13 {
                bytes.extend_from_slice(&[0, 0, 1, 0x00, 0, 0x0F]);
            }
        }
        fs::write(&video, bytes)?;
        let mut session = Session::new();
        let asset = session.project.add_asset(&video)?;
        let timeline = session.project.add_timeline(asset, None)?;
        session.execute(
            "timeline.appendVideo",
            &serde_json::json!({ "timeline": timeline.0, "asset": asset.0 }),
        )?;
        let frames = |session: &Session| -> Vec<i64> {
            session
                .project
                .timeline(timeline)
                .map(|found| {
                    found
                        .chapters
                        .iter()
                        .map(|chapter| session.project.settings.frame_rate().frame_at(chapter.time))
                        .collect()
                })
                .unwrap_or_default()
        };
        let trim = |session: &mut Session, clip: usize, edge: &str, frame: i64| {
            session.execute(
                "timeline.trimVideo",
                &serde_json::json!({ "timeline": timeline.0, "clip": clip, "edge": edge, "time": rate_tick(frame) }),
            )
        };
        assert_eq!(frames(&session), vec![0, 130]);

        // The first clip's out point at frame 95: the nearest GOP start is 91. The join and its chapter move up.
        let trimmed = trim(&mut session, 0, "out", 95)?;
        assert_eq!(trimmed.get("end").and_then(Value::as_u64), Some(91));
        assert_eq!(frames(&session), vec![0, 91]);
        // The second clip's in point 30 frames in (timeline frame 121): the nearest GOP start is 26. The chapter at
        // the clip's head stays where it is.
        let trimmed = trim(&mut session, 1, "in", 121)?;
        assert_eq!(trimmed.get("start").and_then(Value::as_u64), Some(26));
        assert_eq!(frames(&session), vec![0, 91]);
        let clips = session.execute(
            "timeline.clips",
            &serde_json::json!({ "timeline": timeline.0 }),
        )?;
        assert_eq!(
            clips.get("duration").and_then(Value::as_i64),
            Some(rate_tick(91 + 104))
        );
        // At least one GOP stays: an out point dragged to the clip's head stops one GOP in.
        let trimmed = trim(&mut session, 0, "out", 0)?;
        assert_eq!(trimmed.get("end").and_then(Value::as_u64), Some(13));
        assert_eq!(frames(&session), vec![0, 13]);
        // Taking the trim back lengthens the clip and what follows makes room.
        let trimmed = trim(&mut session, 0, "out", 500)?;
        assert_eq!(trimmed.get("end"), Some(&Value::Null));
        assert_eq!(frames(&session), vec![0, 130]);
        assert!(trim(&mut session, 5, "out", 10).is_err());
        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }

    /// Ticks of an NTSC frame number.
    fn rate_tick(frame: i64) -> i64 {
        disccraft_project::DiscSettings::default()
            .frame_rate()
            .tick_of(frame)
            .0
    }

    /// M10.21: H.264 that goes on an HD Blu-ray as it is is trimmed at its key pictures (the fixture, made by the
    /// Blu-ray tests with ffmpeg, has an IDR picture every 30 frames of its 90); on a DVD, where it is
    /// transcoded, it cannot be trimmed.
    #[test]
    fn h264_kept_as_it_is_is_trimmed_at_its_key_pictures() -> Result<(), Box<dyn Error>> {
        let video =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures/h264_1080i.mkv");
        if !video.is_file() {
            println!("skipped: no target/fixtures/h264_1080i.mkv");
            return Ok(());
        }
        let mut session = Session::new();
        let asset = session.project.add_asset(&video)?;
        let timeline = session.project.add_timeline(asset, None)?;
        let trim = |session: &mut Session, edge: &str, frame: i64| {
            session.execute(
                "timeline.trimVideo",
                &serde_json::json!({ "timeline": timeline.0, "clip": 0, "edge": edge, "time": rate_tick(frame) }),
            )
        };
        // A DVD project: the file would be transcoded.
        assert!(trim(&mut session, "in", 40).is_err());
        // An HD Blu-ray of another format: the clip is encoded for it, and cut on the frame asked for.
        session.project.settings.kind = disccraft_project::DiscKind::BluRay;
        session.project.settings.bd_video = disccraft_project::BdVideo::Hd720p59;
        let trimmed = trim(&mut session, "in", 40)?;
        assert_eq!(trimmed.get("start").and_then(Value::as_u64), Some(40));
        let trimmed = trim(&mut session, "out", 5)?;
        assert_eq!(trimmed.get("end").and_then(Value::as_u64), Some(45));
        let trimmed = trim(&mut session, "out", 500)?;
        assert_eq!(trimmed.get("end"), Some(&Value::Null));
        let trimmed = trim(&mut session, "in", -500)?;
        assert_eq!(trimmed.get("start").and_then(Value::as_u64), Some(0));
        session.project.settings.kind = disccraft_project::DiscKind::BluRay;
        session.project.settings.bd_video = disccraft_project::BdVideo::Hd1080i59;
        // The in point asked for at frame 40 goes to the key picture on frame 30; the out point at frame 70
        // to the one on frame 60 (counted in the file: the clip now starts 30 frames in).
        let trimmed = trim(&mut session, "in", 40)?;
        assert_eq!(trimmed.get("start").and_then(Value::as_u64), Some(30));
        assert_eq!(trimmed.get("end"), Some(&Value::Null));
        let trimmed = trim(&mut session, "out", 40)?;
        assert_eq!(trimmed.get("start").and_then(Value::as_u64), Some(30));
        assert_eq!(trimmed.get("end").and_then(Value::as_u64), Some(60));
        // The out point taken back past the last key picture: the end of the file.
        let trimmed = trim(&mut session, "out", 58)?;
        assert_eq!(trimmed.get("end"), Some(&Value::Null));
        // The in point taken back to the head.
        let trimmed = trim(&mut session, "in", -30)?;
        assert_eq!(trimmed.get("start").and_then(Value::as_u64), Some(0));
        Ok(())
    }

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

    /// M6.379: a clip's sound follows its picture (Encore links them): trimming a clip cuts its sound and moves
    /// the sound of the clips after it up, moving a clip moves its sound, and removing a clip takes its sound away.
    #[test]
    fn a_clips_sound_follows_its_edits() -> Result<(), Box<dyn Error>> {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures/h264_aac.mp4");
        if !fixture.is_file() {
            println!("skipped: no target/fixtures/h264_aac.mp4");
            return Ok(());
        }
        let mut session = Session::new();
        // An HD Blu-ray: its encoded clips are trimmed to the frame.
        session.project.settings.kind = disccraft_project::DiscKind::BluRay;
        session.project.settings.bd_video = disccraft_project::BdVideo::Hd1080p23;
        let video = value_u64(
            &session.execute("asset.import", &obj(&[("path", string_value(&fixture))]))?,
            "asset",
        )?;
        let timeline = value_u64(
            &session.execute("timeline.create", &obj(&[("asset", Value::from(video))]))?,
            "timeline",
        )?;
        let appended = session.execute(
            "timeline.appendVideo",
            &obj(&[
                ("timeline", Value::from(timeline)),
                ("asset", Value::from(video)),
            ]),
        )?;
        assert_eq!(appended.get("audioTrack"), Some(&Value::from(0)));
        let sound = |session: &Session| -> Vec<(u32, u32, Option<u32>)> {
            session
                .project
                .timeline(disccraft_project::TimelineId(timeline))
                .and_then(|found| found.audio.first())
                .map(|track| {
                    track
                        .clips()
                        .iter()
                        .map(|(_, clip)| (clip.start, clip.trim_start, clip.trim_end))
                        .collect()
                })
                .unwrap_or_default()
        };
        // The first clip's out point to frame 30: its sound ends there, the second's starts there.
        session.execute(
            "timeline.trimVideo",
            &obj(&[
                ("timeline", Value::from(timeline)),
                ("clip", Value::from(0)),
                ("edge", Value::from("out")),
                ("time", Value::from(rate_tick(30))),
            ]),
        )?;
        let laid = sound(&session);
        assert_eq!(laid.first(), Some(&(0, 0, Some(30))), "{laid:?}");
        assert_eq!(laid.get(1).map(|clip| clip.0), Some(30), "{laid:?}");
        // The second clip's in point to frame 10 of its file: its sound leaves out as much.
        session.execute(
            "timeline.trimVideo",
            &obj(&[
                ("timeline", Value::from(timeline)),
                ("clip", Value::from(1)),
                ("edge", Value::from("in")),
                ("time", Value::from(rate_tick(40))),
            ]),
        )?;
        let laid = sound(&session);
        assert_eq!(
            laid.get(1).map(|clip| (clip.0, clip.1)),
            Some((30, 10)),
            "{laid:?}"
        );
        // Moved 15 frames on, with a gap before it: its sound goes along.
        session.execute(
            "timeline.moveVideo",
            &obj(&[
                ("timeline", Value::from(timeline)),
                ("clip", Value::from(1)),
                ("time", Value::from(rate_tick(45))),
            ]),
        )?;
        let laid = sound(&session);
        assert_eq!(laid.get(1).map(|clip| clip.0), Some(45), "{laid:?}");
        // Removed: its sound goes, the first clip's stays.
        session.execute(
            "timeline.removeVideo",
            &obj(&[
                ("timeline", Value::from(timeline)),
                ("clip", Value::from(1)),
            ]),
        )?;
        assert_eq!(sound(&session), [(0, 0, Some(30))]);
        Ok(())
    }

    /// M6.361: an appended clip's sound goes on the timeline at the clip's place, as Encore lays it beside the
    /// picture: on the track of the clips' sounds (the one before cut at the join), or a new track of all the
    /// clips' sounds when there was none; a clip without sound adds none.
    #[test]
    fn an_appended_clips_sound_goes_beside_it() -> Result<(), Box<dyn Error>> {
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
        let (with_sound, silent) = (fixtures.join("h264_aac.mp4"), fixtures.join("ntsc.m2v"));
        if !with_sound.is_file() || !silent.is_file() {
            println!("skipped: no target/fixtures/h264_aac.mp4 or ntsc.m2v");
            return Ok(());
        }
        let mut session = Session::new();
        let import = |session: &mut Session, path: &PathBuf| -> Result<u64, Box<dyn Error>> {
            value_u64(
                &session.execute("asset.import", &obj(&[("path", string_value(path))]))?,
                "asset",
            )
        };
        let (sound, quiet) = (
            import(&mut session, &with_sound)?,
            import(&mut session, &silent)?,
        );
        let create = |session: &mut Session, asset: u64| -> Result<u64, Box<dyn Error>> {
            value_u64(
                &session.execute("timeline.create", &obj(&[("asset", Value::from(asset))]))?,
                "timeline",
            )
        };
        let append = |session: &mut Session, timeline: u64, asset: u64| {
            session.execute(
                "timeline.appendVideo",
                &obj(&[
                    ("timeline", Value::from(timeline)),
                    ("asset", Value::from(asset)),
                ]),
            )
        };
        let rate = session.project.settings.frame_rate();
        let join_frame = |appended: &Value| {
            appended
                .get("time")
                .and_then(Value::as_i64)
                .map(|ticks| u32::try_from(rate.frame_at(super::Tick(ticks))).unwrap_or(0))
        };
        let clips = |session: &Session, timeline: u64, track: usize| {
            session
                .project
                .timeline(disccraft_project::TimelineId(timeline))
                .and_then(|found| found.audio.get(track))
                .map(|track| {
                    track
                        .clips()
                        .into_iter()
                        .map(|(_, clip)| clip)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };

        // Audio 1 is the first clip's sound; the second clip's joins it at the join, its first clip trimmed at its
        // out point is cut there.
        let movie = create(&mut session, sound)?;
        if let Some(found) = session
            .project
            .timeline_mut(disccraft_project::TimelineId(movie))
        {
            found.video_trims = vec![disccraft_project::ClipTrim {
                start: 0,
                end: Some(30),
            }];
        }
        let appended = append(&mut session, movie, sound)?;
        assert_eq!(appended.get("audioTrack"), Some(&Value::from(0)));
        let at = join_frame(&appended).ok_or("no join")?;
        assert_eq!(at, 30);
        let laid = clips(&session, movie, 0);
        assert_eq!(laid.len(), 2);
        assert_eq!(
            laid.first().map(|clip| (clip.start, clip.trim_end)),
            Some((0, Some(30)))
        );
        assert_eq!(
            laid.get(1)
                .map(|clip| (clip.asset.0, clip.start, clip.trim_start, clip.trim_end)),
            Some((sound, 30, 0, None))
        );
        // A clip without sound adds none.
        let appended = append(&mut session, movie, quiet)?;
        assert_eq!(appended.get("audioTrack"), Some(&Value::Null));
        assert_eq!(clips(&session, movie, 0).len(), 2);

        // A timeline of a silent video has no track: the appended clip's sound makes one, from the join.
        let silent_movie = create(&mut session, quiet)?;
        let appended = append(&mut session, silent_movie, sound)?;
        assert_eq!(appended.get("audioTrack"), Some(&Value::from(0)));
        let at = join_frame(&appended).ok_or("no join")?;
        assert!(at > 0);
        let laid = clips(&session, silent_movie, 0);
        assert_eq!(
            laid.iter()
                .map(|clip| (clip.asset.0, clip.start))
                .collect::<Vec<_>>(),
            [(sound, at)]
        );
        // A track of another file's sound is left alone.
        let other = create(&mut session, quiet)?;
        session.execute(
            "timeline.addAudioTrack",
            &obj(&[
                ("timeline", Value::from(other)),
                ("asset", Value::from(sound)),
            ]),
        )?;
        let appended = append(&mut session, other, sound)?;
        assert_eq!(appended.get("audioTrack"), Some(&Value::Null));
        assert_eq!(clips(&session, other, 0).len(), 1);
        Ok(())
    }

    /// A clip moves along the track by frames and leaves a gap (Encore); it stops at the clip before it and at the
    /// clip after it, which stays where it is; its chapters go with it; removing it takes its gap along.
    #[test]
    fn a_clip_moves_and_leaves_a_gap() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("move-clip")?;
        let video = dir.join("clip.m2v");
        // A sequence header, then ten GOPs of 13 pictures (start codes only): 130 frames.
        let mut bytes = vec![0_u8, 0, 1, 0xB3, 0x2D, 0x01, 0xE0];
        for _ in 0..10 {
            bytes.extend_from_slice(&[0, 0, 1, 0xB8, 0, 8]);
            for _ in 0..13 {
                bytes.extend_from_slice(&[0, 0, 1, 0x00, 0, 0x0F]);
            }
        }
        fs::write(&video, bytes)?;
        let mut session = Session::new();
        let asset = session.project.add_asset(&video)?;
        let timeline = session.project.add_timeline(asset, None)?;
        for _ in 0..2 {
            session.execute(
                "timeline.appendVideo",
                &serde_json::json!({ "timeline": timeline.0, "asset": asset.0 }),
            )?;
        }
        let starts = |session: &mut Session| -> Result<Vec<i64>, String> {
            let clips = session.execute(
                "timeline.clips",
                &serde_json::json!({ "timeline": timeline.0 }),
            )?;
            Ok(clips
                .get("clips")
                .and_then(Value::as_array)
                .map(|clips| {
                    clips
                        .iter()
                        .filter_map(|clip| clip.get("start").and_then(Value::as_i64))
                        .collect()
                })
                .unwrap_or_default())
        };
        let chapters = |session: &Session| -> Vec<i64> {
            session
                .project
                .timeline(timeline)
                .map(|found| found.chapters.iter().map(|c| c.time.0).collect())
                .unwrap_or_default()
        };
        let frames = |ticks: Vec<i64>| -> Vec<i64> {
            ticks
                .into_iter()
                .map(|t| (t + rate_tick(1) / 2) / rate_tick(1))
                .collect()
        };
        assert_eq!(frames(starts(&mut session)?), [0, 130, 260]);
        let mv = |session: &mut Session, clip: usize, frame: i64| {
            session.execute(
                "timeline.moveVideo",
                &serde_json::json!({ "timeline": timeline.0, "clip": clip, "time": rate_tick(frame) }),
            )
        };
        // The last clip goes as far as asked: a gap of 40 frames.
        mv(&mut session, 2, 300)?;
        assert_eq!(frames(starts(&mut session)?), [0, 130, 300]);
        assert_eq!(frames(chapters(&session)), [0, 130, 300]);
        // The middle clip moves into the gap; the last clip stays. Past the last clip it stops at it.
        mv(&mut session, 1, 150)?;
        assert_eq!(frames(starts(&mut session)?), [0, 150, 300]);
        mv(&mut session, 1, 400)?;
        assert_eq!(frames(starts(&mut session)?), [0, 170, 300]);
        assert_eq!(frames(chapters(&session)), [0, 170, 300]);
        // It cannot go in front of the clip before it.
        mv(&mut session, 1, 10)?;
        assert_eq!(frames(starts(&mut session)?), [0, 130, 300]);
        // A chapter is not put into a gap's black: it goes to a GOP of a clip.
        let added = session.execute(
            "timeline.addChapter",
            &serde_json::json!({ "timeline": timeline.0, "time": rate_tick(280) }),
        )?;
        let at = added.get("time").and_then(Value::as_i64).ok_or("no time")?;
        assert!(frames(vec![at]) != [280], "a chapter in the gap");
        // Removing the last clip takes its gap along.
        session.execute(
            "timeline.removeVideo",
            &serde_json::json!({ "timeline": timeline.0, "clip": 2 }),
        )?;
        assert_eq!(frames(starts(&mut session)?), [0, 130]);
        assert!(
            session
                .project
                .timeline(timeline)
                .is_some_and(|t| !t.has_video_gaps())
        );
        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }

    /// A colour set goes to a file and into a slideshow (Encore: the export and import buttons of the colour set
    /// dialog); what is not a colour set file is refused.
    #[test]
    fn a_colour_set_is_exported_and_imported() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("colour-set")?;
        let mut session = Session::new();
        let video = session.project.add_asset(&dir.join("no-such.m2v"))?;
        let timeline = session.project.add_timeline(video, None)?;
        let show = session.project.add_slideshow(None)?;
        session.execute(
            "timeline.setSubtitleColors",
            &serde_json::json!({ "timeline": timeline.0, "groups": [
                {"fill": [1, 2, 3], "fillOpacity": 40, "stroke": [4, 5, 6]},
                {"fill": [7, 8, 9], "stroke": [0, 0, 0], "strokeOpacity": 0},
                {"fill": [10, 11, 12], "stroke": [13, 14, 15]},
            ]}),
        )?;
        let file = dir.join("set.dccolors");
        session.execute(
            "colorset.export",
            &serde_json::json!({ "timeline": timeline.0, "path": file }),
        )?;
        session.execute(
            "colorset.import",
            &serde_json::json!({ "slideshow": show, "path": file }),
        )?;
        let wanted = session
            .project
            .timeline(timeline)
            .and_then(|found| found.subtitle_colors)
            .ok_or("no colours")?;
        assert_eq!(
            wanted.groups.first().map(|g| (g.fill.rgb, g.fill.opacity)),
            Some(([1, 2, 3], 40))
        );
        assert_eq!(
            session
                .project
                .slideshow(show)
                .and_then(|s| s.subtitle_colors),
            Some(wanted)
        );
        // A named set of the project: saved from the timeline, applied to another timeline, deleted.
        let second = session.project.add_timeline(video, None)?;
        session.execute(
            "colorset.save",
            &serde_json::json!({ "name": " Warm ", "timeline": timeline.0 }),
        )?;
        assert!(session.project.subtitle_color_sets.contains_key("Warm"));
        session.execute(
            "colorset.apply",
            &serde_json::json!({ "name": "Warm", "timeline": second.0 }),
        )?;
        assert_eq!(
            session
                .project
                .timeline(second)
                .and_then(|found| found.subtitle_colors),
            Some(wanted)
        );
        assert!(
            session
                .execute(
                    "colorset.apply",
                    &serde_json::json!({ "name": "Cold", "timeline": second.0 })
                )
                .is_err()
        );
        assert!(
            session
                .execute(
                    "colorset.save",
                    &serde_json::json!({ "name": "  ", "timeline": second.0 })
                )
                .is_err()
        );
        session.execute("colorset.delete", &serde_json::json!({ "name": "Warm" }))?;
        assert!(session.project.subtitle_color_sets.is_empty());
        assert_eq!(
            session
                .project
                .timeline(second)
                .and_then(|found| found.subtitle_colors),
            Some(wanted)
        );
        // Other files are refused.
        let other = dir.join("other.json");
        fs::write(&other, "{\"groups\": []}")?;
        assert!(
            session
                .execute(
                    "colorset.import",
                    &serde_json::json!({ "timeline": timeline.0, "path": other })
                )
                .is_err()
        );
        fs::write(&other, "not json")?;
        assert!(
            session
                .execute(
                    "colorset.import",
                    &serde_json::json!({ "timeline": timeline.0, "path": other })
                )
                .is_err()
        );
        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }

    /// FilmCraft's chapter markers become chapter points (as Encore takes Premiere Pro's): snapped to GOP starts,
    /// named after the markers; a marker at the start names chapter 1; comment markers only with "all"; markers
    /// past the end are left out.
    #[test]
    fn filmcraft_chapter_markers_become_chapters() -> Result<(), Box<dyn Error>> {
        use filmcraft_project::{MarkerKind, Project, SequenceSettings};
        let dir = temp_dir("filmcraft-markers")?;
        let video = dir.join("clip.m2v");
        // A sequence header, then ten GOPs of 13 pictures (start codes only): 130 frames.
        let mut bytes = vec![0_u8, 0, 1, 0xB3, 0x2D, 0x01, 0xE0];
        for _ in 0..10 {
            bytes.extend_from_slice(&[0, 0, 1, 0xB8, 0, 8]);
            for _ in 0..13 {
                bytes.extend_from_slice(&[0, 0, 1, 0x00, 0, 0x0F]);
            }
        }
        fs::write(&video, bytes)?;

        let second = filmcraft_time::TICKS_PER_SECOND;
        let mut film = Project::new("Film");
        let _ = film.new_sequence("Other", SequenceSettings::default(), 1, 1, None);
        let cut = film.new_sequence("Clip", SequenceSettings::default(), 1, 1, None);
        let label = serde_json::to_value(filmcraft_project::Label::Violet)?;
        let mut markers = Vec::new();
        for (id, ticks, name, kind) in [
            (901_u64, 0_i64, "Opening", MarkerKind::Chapter),
            (902, second, "Scene 2", MarkerKind::Chapter),
            (903, second * 2, "A note", MarkerKind::Comment),
            (904, second * 60, "Too late", MarkerKind::Chapter),
        ] {
            markers.push(serde_json::from_value(serde_json::json!({
                "id": id, "start": ticks, "duration": 0, "name": name, "comment": "",
                "kind": kind, "color": label,
            }))?);
        }
        film.sequence_mut(cut).ok_or("no sequence")?.markers = markers;
        let project_path = dir.join("film.fcproj");
        fs::write(&project_path, filmcraft_format::encode(&film, false))?;

        let mut session = Session::new();
        let asset = session.project.add_asset(&video)?;
        let timeline = session.project.add_timeline(asset, None)?;
        let listed = session.execute(
            "filmcraft.sequences",
            &serde_json::json!({ "path": project_path }),
        )?;
        let names: Vec<&str> = listed
            .get("sequences")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|s| s.get("name").and_then(Value::as_str))
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(names, ["Other", "Clip"]);

        // The sequence called like the video ("clip") is taken without being named.
        let result = session.execute(
            "timeline.importFilmCraftMarkers",
            &serde_json::json!({ "timeline": timeline.0, "path": project_path }),
        )?;
        assert_eq!(result.get("sequence").and_then(Value::as_str), Some("Clip"));
        assert_eq!(value_u64(&result, "added")?, 1);
        assert_eq!(value_u64(&result, "skipped")?, 1);
        let chapters = |session: &Session| -> Vec<(i64, String)> {
            session
                .project
                .timeline(timeline)
                .map(|found| {
                    found
                        .chapters
                        .iter()
                        .map(|c| (c.time.0, c.name.clone()))
                        .collect()
                })
                .unwrap_or_default()
        };
        // One second is frame 30, in the GOP that starts on frame 26.
        assert_eq!(
            chapters(&session),
            [
                (0, "Opening".to_owned()),
                (rate_tick(26), "Scene 2".to_owned())
            ]
        );

        // Every marker: the comment at two seconds (frame 60, GOP 52) comes too; the chapter that is there stays.
        let result = session.execute(
            "timeline.importFilmCraftMarkers",
            &serde_json::json!({ "timeline": timeline.0, "path": project_path, "sequence": "Clip", "all": true }),
        )?;
        assert_eq!(value_u64(&result, "added")?, 2);
        let times: Vec<i64> = chapters(&session).iter().map(|(time, _)| *time).collect();
        assert_eq!(times, [0, rate_tick(26), rate_tick(39), rate_tick(52)]);

        // An unknown sequence, and a file that is no project, are errors.
        assert!(
            session
                .execute(
                    "timeline.importFilmCraftMarkers",
                    &serde_json::json!({ "timeline": timeline.0, "path": project_path, "sequence": "Nope" }),
                )
                .is_err()
        );
        assert!(
            session
                .execute(
                    "timeline.importFilmCraftMarkers",
                    &serde_json::json!({ "timeline": timeline.0, "path": video }),
                )
                .is_err()
        );
        let _ = fs::remove_dir_all(&dir);
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
    fn an_audio_clip_moves_along_its_track() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("move_audio")?;
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
            ]),
        )?;
        let start = |session: &Session| {
            session
                .project
                .timeline(disccraft_project::TimelineId(timeline_id))
                .and_then(|timeline| timeline.audio.first().map(|track| track.start))
        };
        assert_eq!(start(&session), Some(0));
        // Ten seconds in (NTSC drop-frame: frame 300), given as a timecode.
        let moved = session.execute(
            "timeline.moveAudio",
            &serde_json::json!({"timeline": timeline_id, "track": 0, "timecode": "00;00;10;00"}),
        )?;
        assert_eq!(start(&session), Some(300));
        assert_eq!(moved.get("frame"), Some(&Value::from(300)));
        // Before the start is the start; a track that is not there is an error and changes nothing.
        session.execute(
            "timeline.moveAudio",
            &serde_json::json!({"timeline": timeline_id, "track": 0, "time": -5}),
        )?;
        assert_eq!(start(&session), Some(0));
        assert!(
            session
                .execute(
                    "timeline.moveAudio",
                    &serde_json::json!({"timeline": timeline_id, "track": 3, "time": 0}),
                )
                .is_err()
        );
        // One step back: where it was moved to.
        session.execute("edit.undo", &Value::Null)?;
        assert_eq!(start(&session), Some(300));
        // The file keeps the start, and leaves it out when it is 0.
        let text = serde_json::to_string(&session.project)?;
        assert!(text.contains("\"start\":300"), "{text}");
        session.execute("edit.undo", &Value::Null)?;
        assert!(!serde_json::to_string(&session.project)?.contains("\"start\""));
        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }

    /// M6.269: an audio track takes several clips. A clip is added behind the last one or where there is room,
    /// moves and trims between its neighbours, and goes; the track's only clip stays.
    #[test]
    fn an_audio_track_takes_several_clips() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("audio_clips")?;
        let video = dir.join("movie.mp4");
        let audio = dir.join("audio.wav");
        fs::write(&video, b"fake")?;
        // Ten seconds of 16-bit mono silence at 48 kHz: 300 frames of the NTSC rate.
        let samples = 48_000_u32 * 10;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + samples * 2).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&48_000_u32.to_le_bytes());
        wav.extend_from_slice(&96_000_u32.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(samples * 2).to_le_bytes());
        wav.resize(wav.len() + (samples * 2) as usize, 0);
        fs::write(&audio, wav)?;

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
            ]),
        )?;
        // Each clip as (start, trim start, trim end), the first clip first.
        let clips = |session: &Session| -> Vec<(u32, u32, Option<u32>)> {
            session
                .project
                .timeline(disccraft_project::TimelineId(timeline_id))
                .and_then(|timeline| timeline.audio.first())
                .map(|track| {
                    (0..=track.more.len())
                        .filter_map(|number| track.clip(number))
                        .map(|clip| (clip.start, clip.trim_start, clip.trim_end))
                        .collect()
                })
                .unwrap_or_default()
        };
        let run = |session: &mut Session, command: &str, fields: serde_json::Value| {
            let mut params = serde_json::json!({"timeline": timeline_id, "track": 0});
            if let (Some(params), Some(fields)) = (params.as_object_mut(), fields.as_object()) {
                params.extend(fields.clone());
            }
            session.execute(command, &params)
        };

        // Without a time the clip goes behind the last one: the file is 300 frames long.
        let added = run(
            &mut session,
            "timeline.addAudioClip",
            serde_json::json!({"asset": audio_id}),
        )?;
        assert_eq!(added.get("clip").and_then(Value::as_u64), Some(1));
        assert_eq!(clips(&session), [(0, 0, None), (300, 0, None)]);
        // Over another clip it is refused.
        assert!(
            run(
                &mut session,
                "timeline.addAudioClip",
                serde_json::json!({"asset": audio_id, "time": rate_tick(100)})
            )
            .is_err()
        );
        // It does not move in front of where the first clip ends.
        run(
            &mut session,
            "timeline.moveAudio",
            serde_json::json!({"clip": 1, "time": rate_tick(50)}),
        )?;
        assert_eq!(clips(&session), [(0, 0, None), (300, 0, None)]);
        // The first clip trimmed to 100 frames makes room: the second moves up to frame 150.
        run(
            &mut session,
            "timeline.trimAudio",
            serde_json::json!({"clip": 0, "edge": "out", "time": rate_tick(100)}),
        )?;
        run(
            &mut session,
            "timeline.moveAudio",
            serde_json::json!({"clip": 1, "time": rate_tick(150)}),
        )?;
        assert_eq!(clips(&session), [(0, 0, Some(100)), (150, 0, None)]);
        // The first clip moves no further than where it still ends in front of the second.
        run(
            &mut session,
            "timeline.moveAudio",
            serde_json::json!({"clip": 0, "time": rate_tick(500)}),
        )?;
        assert_eq!(clips(&session), [(50, 0, Some(100)), (150, 0, None)]);
        // Its out point does not pass the second clip's start, nor the second's in point the first's end.
        run(
            &mut session,
            "timeline.trimAudio",
            serde_json::json!({"clip": 0, "edge": "out", "time": rate_tick(400)}),
        )?;
        run(
            &mut session,
            "timeline.trimAudio",
            serde_json::json!({"clip": 1, "edge": "in", "time": rate_tick(100)}),
        )?;
        assert_eq!(clips(&session), [(50, 0, Some(100)), (150, 0, None)]);
        // The first clip goes: the second is the track's clip now, and stays as the only one.
        run(
            &mut session,
            "timeline.removeAudioClip",
            serde_json::json!({"clip": 0}),
        )?;
        assert_eq!(clips(&session), [(150, 0, None)]);
        assert!(
            run(
                &mut session,
                "timeline.removeAudioClip",
                serde_json::json!({"clip": 0})
            )
            .is_err()
        );
        assert!(
            run(
                &mut session,
                "timeline.removeAudioClip",
                serde_json::json!({"clip": 3})
            )
            .is_err()
        );
        session.undo()?;
        assert_eq!(clips(&session), [(50, 0, Some(100)), (150, 0, None)]);
        // The file is in use by both clips: it cannot be removed while one is left.
        assert!(
            session
                .execute("asset.remove", &obj(&[("asset", Value::from(audio_id))]))
                .is_err()
        );
        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }

    /// M6.254: an audio clip's in and out points are trimmed on the timeline; the sound that stays keeps its
    /// place.
    #[test]
    fn an_audio_clip_is_trimmed_at_its_edges() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("trim_audio")?;
        let video = dir.join("movie.mp4");
        let audio = dir.join("audio.wav");
        fs::write(&video, b"fake")?;
        // Ten seconds of 16-bit mono silence at 48 kHz: 300 frames of the NTSC rate (299.7).
        let samples = 48_000_u32 * 10;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + samples * 2).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&48_000_u32.to_le_bytes());
        wav.extend_from_slice(&96_000_u32.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(samples * 2).to_le_bytes());
        wav.resize(wav.len() + (samples * 2) as usize, 0);
        fs::write(&audio, wav)?;

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
            ]),
        )?;
        let track = |session: &Session| -> Option<(u32, u32, Option<u32>)> {
            session
                .project
                .timeline(disccraft_project::TimelineId(timeline_id))
                .and_then(|timeline| timeline.audio.first())
                .map(|track| (track.start, track.trim_start, track.trim_end))
        };
        let trim = |session: &mut Session, edge: &str, frame: i64| {
            session.execute(
                "timeline.trimAudio",
                &serde_json::json!({"timeline": timeline_id, "track": 0, "edge": edge, "time": rate_tick(frame)}),
            )
        };
        assert_eq!(track(&session), Some((0, 0, None)));
        // The out point at frame 200 of the timeline: the file's frame 200.
        let trimmed = trim(&mut session, "out", 200)?;
        assert_eq!(trimmed.get("trimEnd"), Some(&Value::from(200)));
        assert_eq!(track(&session), Some((0, 0, Some(200))));
        // The in point at frame 60: the clip starts there, with the file's frame 60.
        trim(&mut session, "in", 60)?;
        assert_eq!(track(&session), Some((60, 60, Some(200))));
        // Moved 100 frames along: the same part of the file.
        session.execute(
            "timeline.moveAudio",
            &serde_json::json!({"timeline": timeline_id, "track": 0, "time": rate_tick(160)}),
        )?;
        assert_eq!(track(&session), Some((160, 60, Some(200))));
        // The in point taken back by 30 frames, then by more than there is of the file: its head.
        trim(&mut session, "in", 130)?;
        assert_eq!(track(&session), Some((130, 30, Some(200))));
        trim(&mut session, "in", 0)?;
        assert_eq!(track(&session), Some((100, 0, Some(200))));
        // The in point cannot pass the out point: one frame stays.
        trim(&mut session, "in", 5000)?;
        assert_eq!(track(&session), Some((299, 199, Some(200))));
        trim(&mut session, "in", 100)?;
        // The out point cannot pass the in point either, and past the file's end it is the end.
        trim(&mut session, "out", 0)?;
        assert_eq!(track(&session), Some((100, 0, Some(1))));
        trim(&mut session, "out", 5000)?;
        assert_eq!(track(&session), Some((100, 0, None)));
        // Undone step by step; the file leaves the trims out when there are none.
        session.execute("edit.undo", &Value::Null)?;
        assert_eq!(track(&session), Some((100, 0, Some(1))));
        let text = serde_json::to_string(&session.project)?;
        assert!(text.contains("\"trimEnd\":1"), "{text}");
        session.execute("edit.undo", &Value::Null)?;
        assert_eq!(track(&session), Some((100, 0, Some(200))));
        assert!(
            session
                .execute(
                    "timeline.trimAudio",
                    &serde_json::json!({"timeline": timeline_id, "track": 4, "edge": "in", "time": 0}),
                )
                .is_err()
        );
        let _ = fs::remove_dir_all(&dir);
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
