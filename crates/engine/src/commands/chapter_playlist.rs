//! Chapter playlist commands (Encore's チャプター再生リスト).

use crate::params::{opt_str, opt_u64, req_link, req_str, req_u64, req_usize};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{Link, PlaybackOrder, TimelineId};
use serde_json::Value;

use super::{always_enabled, empty_object, object};

/// Chapter playlist commands.
pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "chapterplaylist.create",
        label: "チャプター再生リストを作成",
        menu: &["チャプター再生リスト"],
        shortcut: None,
        params: r#"{"timeline": id, "name"?: string, "chapters"?: [index]}"#,
        enabled: always_enabled,
        run: run_create,
        journal: true,
    },
    CommandSpec {
        id: "chapterplaylist.addChapter",
        label: "チャプターを追加",
        menu: &["チャプター再生リスト"],
        shortcut: None,
        params: r#"{"chapterPlaylist": id, "chapter": index, "at"?: index}"#,
        enabled: always_enabled,
        run: run_add_chapter,
        journal: true,
    },
    CommandSpec {
        id: "chapterplaylist.removeEntry",
        label: "エントリを削除",
        menu: &["チャプター再生リスト"],
        shortcut: None,
        params: r#"{"chapterPlaylist": id, "entry": index}"#,
        enabled: always_enabled,
        run: run_remove_entry,
        journal: true,
    },
    CommandSpec {
        id: "chapterplaylist.moveEntry",
        label: "エントリを移動",
        menu: &["チャプター再生リスト"],
        shortcut: None,
        params: r#"{"chapterPlaylist": id, "from": index, "to": index}"#,
        enabled: always_enabled,
        run: run_move_entry,
        journal: true,
    },
    CommandSpec {
        id: "chapterplaylist.setEndAction",
        label: "終了アクションを設定",
        menu: &["チャプター再生リスト"],
        shortcut: None,
        params: r#"{"chapterPlaylist": id, "target": Link}"#,
        enabled: always_enabled,
        run: run_set_end_action,
        journal: true,
    },
    CommandSpec {
        id: "chapterplaylist.setOverride",
        label: "上書きを設定",
        menu: &["チャプター再生リスト"],
        shortcut: None,
        params: r#"{"chapterPlaylist": id, "target"?: Link}"#,
        enabled: always_enabled,
        run: run_set_override,
        journal: true,
    },
    CommandSpec {
        id: "chapterplaylist.setOrder",
        label: "再生の種類を設定",
        menu: &["チャプター再生リスト"],
        shortcut: None,
        params: r#"{"chapterPlaylist": id, "order": "sequential" | "random" | "shuffle"}"#,
        enabled: always_enabled,
        run: run_set_order,
        journal: true,
    },
    CommandSpec {
        id: "chapterplaylist.setUserOps",
        label: "ユーザー操作を設定",
        menu: &["チャプター再生リスト"],
        shortcut: None,
        params: r#"{"chapterPlaylist": id, "prohibited": [op]}"#,
        enabled: always_enabled,
        run: run_set_user_ops,
        journal: true,
    },
    CommandSpec {
        id: "chapterplaylist.list",
        label: "チャプター再生リスト一覧",
        menu: &["チャプター再生リスト"],
        shortcut: None,
        params: "{}",
        enabled: always_enabled,
        run: run_list,
        journal: false,
    },
];

fn run_create(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = TimelineId(req_u64(params, "timeline")?);
    let name = opt_str(params, "name")?;
    let chapters = chapter_indexes(params)?;

    let id = session
        .project
        .add_chapter_playlist(timeline, name)
        .map_err(|error| error.to_string())?;
    for chapter in chapters {
        session
            .project
            .add_chapter_playlist_entry(id, chapter, None)
            .map_err(|error| error.to_string())?;
    }

    Ok(object(&[("chapterPlaylist", Value::from(id))]))
}

/// The optional "chapters" list: zero-based chapter indexes.
fn chapter_indexes(params: &Value) -> Result<Vec<u32>, String> {
    let Some(value) = params.get("chapters") else {
        return Ok(Vec::new());
    };
    let list = value
        .as_array()
        .ok_or_else(|| "parameter \"chapters\" must be a list of chapter indexes".to_string())?;
    list.iter()
        .map(|item| {
            item.as_u64()
                .and_then(|index| u32::try_from(index).ok())
                .ok_or_else(|| {
                    "parameter \"chapters\" must be a list of chapter indexes".to_string()
                })
        })
        .collect()
}

fn run_add_chapter(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "chapterPlaylist")?;
    let chapter = u32::try_from(req_u64(params, "chapter")?)
        .map_err(|_| "parameter \"chapter\" is too large".to_string())?;
    let at = opt_u64(params, "at")?
        .map(|at| usize::try_from(at).map_err(|_| "parameter \"at\" is too large".to_string()))
        .transpose()?;

    let entry = session
        .project
        .add_chapter_playlist_entry(id, chapter, at)
        .map_err(|error| error.to_string())?;
    let entry = u64::try_from(entry).map_err(|_| "entry index is too large".to_string())?;
    Ok(object(&[("entry", Value::from(entry))]))
}

fn run_remove_entry(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "chapterPlaylist")?;
    let entry = req_usize(params, "entry")?;
    session
        .project
        .remove_chapter_playlist_entry(id, entry)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

fn run_move_entry(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "chapterPlaylist")?;
    let from = req_usize(params, "from")?;
    let to = req_usize(params, "to")?;
    session
        .project
        .move_chapter_playlist_entry(id, from, to)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

fn run_set_end_action(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "chapterPlaylist")?;
    let target = req_link(params, "target")?;
    let Some(playlist) = session.project.chapter_playlist_mut(id) else {
        return Err(format!("chapter playlist {id} does not exist"));
    };
    playlist.end_action = target;
    Ok(empty_object())
}

fn run_set_override(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "chapterPlaylist")?;
    let target = match params.get("target") {
        None | Some(Value::Null) => None,
        Some(_) => Some(req_link(params, "target")?).filter(|link| !matches!(link, Link::NotSet)),
    };
    if let Some(link) = &target
        && !session.project.link_resolves(link)
    {
        return Err("the override target does not exist".to_string());
    }
    let Some(playlist) = session.project.chapter_playlist_mut(id) else {
        return Err(format!("chapter playlist {id} does not exist"));
    };
    playlist.override_link = target;
    Ok(empty_object())
}

fn run_set_order(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "chapterPlaylist")?;
    let order = req_str(params, "order")?;
    let order = PlaybackOrder::parse(&order).ok_or_else(|| {
        "parameter \"order\" must be \"sequential\", \"random\" or \"shuffle\"".to_string()
    })?;
    session
        .project
        .set_chapter_playlist_order(id, order)
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

fn run_set_user_ops(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "chapterPlaylist")?;
    let ops = super::user_ops_param(params)?;
    let Some(playlist) = session.project.chapter_playlist_mut(id) else {
        return Err(format!("chapter playlist {id} does not exist"));
    };
    playlist.prohibited_ops = ops;
    Ok(empty_object())
}

fn run_list(session: &mut Session, _params: &Value) -> CommandResult {
    let mut list = Vec::new();
    for playlist in session.project.chapter_playlists.values() {
        list.push(serde_json::to_value(playlist).map_err(|error| error.to_string())?);
    }
    Ok(Value::Array(list))
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use disccraft_project::{Link, PlaybackOrder};
    use serde_json::{Value, json};
    use std::error::Error;

    fn value_u64(value: &Value, key: &str) -> Result<u64, Box<dyn Error>> {
        value
            .get(key)
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("missing {key}").into())
    }

    fn session_with_chapters() -> Result<(Session, u64), Box<dyn Error>> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "disccraft-chapter-playlist-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)?;
        let video = dir.join("a.m2v");
        std::fs::File::create(&video)?;
        let mut session = Session::new();
        let asset = session.project.add_asset(&video)?;
        let timeline = session
            .project
            .add_timeline(asset, Some("Feature".to_owned()))?;
        let timeline = timeline.0;
        for timecode in ["00:00:10:00", "00:00:20:00"] {
            let _ = session.execute(
                "timeline.addChapter",
                &json!({ "timeline": timeline, "timecode": timecode }),
            )?;
        }
        Ok((session, timeline))
    }

    #[test]
    fn chapter_playlist_commands_round_trip() -> Result<(), Box<dyn Error>> {
        let (mut session, timeline) = session_with_chapters()?;
        let created = session.execute(
            "chapterplaylist.create",
            &json!({ "timeline": timeline, "chapters": [2, 0] }),
        )?;
        let id = value_u64(&created, "chapterPlaylist")?;
        assert_eq!(
            session
                .project
                .chapter_playlist(id)
                .map(|list| list.name.clone()),
            Some("Feature Chapter Playlist".to_owned())
        );

        let added = session.execute(
            "chapterplaylist.addChapter",
            &json!({ "chapterPlaylist": id, "chapter": 1, "at": 1 }),
        )?;
        assert_eq!(value_u64(&added, "entry")?, 1);
        assert!(
            session
                .execute(
                    "chapterplaylist.addChapter",
                    &json!({ "chapterPlaylist": id, "chapter": 7 })
                )
                .is_err()
        );
        session.execute(
            "chapterplaylist.moveEntry",
            &json!({ "chapterPlaylist": id, "from": 2, "to": 0 }),
        )?;
        session.execute(
            "chapterplaylist.removeEntry",
            &json!({ "chapterPlaylist": id, "entry": 1 }),
        )?;
        let chapters = session
            .project
            .chapter_playlist(id)
            .map(|list| list.chapters.clone());
        assert_eq!(chapters, Some(vec![0, 1]));

        session.execute(
            "chapterplaylist.setEndAction",
            &json!({ "chapterPlaylist": id, "target": { "timeline": timeline, "chapter": 0 } }),
        )?;
        session.execute(
            "chapterplaylist.setOverride",
            &json!({ "chapterPlaylist": id, "target": "stop" }),
        )?;
        session.execute(
            "chapterplaylist.setOrder",
            &json!({ "chapterPlaylist": id, "order": "shuffle" }),
        )?;
        session.execute(
            "chapterplaylist.setUserOps",
            &json!({ "chapterPlaylist": id, "prohibited": ["fastForward"] }),
        )?;
        let playlist = session
            .project
            .chapter_playlist(id)
            .cloned()
            .ok_or("missing chapter playlist")?;
        assert!(matches!(playlist.end_action, Link::Timeline { .. }));
        assert_eq!(playlist.override_link, Some(Link::Stop));
        assert_eq!(playlist.order, PlaybackOrder::Shuffle);
        assert_eq!(playlist.prohibited_ops.len(), 1);

        // Whole-object commands know the kind.
        session.execute(
            "project.rename",
            &json!({ "kind": "chapterPlaylist", "id": id, "name": "Highlights" }),
        )?;
        session.execute(
            "project.setDescription",
            &json!({ "kind": "chapterPlaylist", "id": id, "description": "Best bits" }),
        )?;
        let listed = session.execute("chapterplaylist.list", &json!({}))?;
        assert_eq!(listed.as_array().map(Vec::len), Some(1));
        assert_eq!(
            listed
                .get(0)
                .and_then(|item| item.get("name"))
                .and_then(Value::as_str),
            Some("Highlights")
        );

        // Undo walks everything back.
        for _ in 0..10 {
            session.undo()?;
        }
        assert!(session.project.chapter_playlists.is_empty());
        Ok(())
    }

    #[test]
    fn removing_the_timeline_removes_its_chapter_playlists() -> Result<(), Box<dyn Error>> {
        let (mut session, timeline) = session_with_chapters()?;
        let created = session.execute(
            "chapterplaylist.create",
            &json!({ "timeline": timeline, "chapters": [1] }),
        )?;
        let id = value_u64(&created, "chapterPlaylist")?;
        session.execute(
            "project.remove",
            &json!({ "kind": "timeline", "id": timeline }),
        )?;
        assert!(session.project.chapter_playlist(id).is_none());
        session.undo()?;
        assert!(session.project.chapter_playlist(id).is_some());
        session.execute(
            "project.remove",
            &json!({ "kind": "chapterPlaylist", "id": id }),
        )?;
        assert!(session.project.chapter_playlist(id).is_none());
        Ok(())
    }
}
