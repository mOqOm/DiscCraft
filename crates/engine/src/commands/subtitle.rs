//! Subtitle files: the cues of a subtitle asset (for the timeline and the monitor) and SRT export.

use std::path::Path;

use crate::params::{req_str, req_u64};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_menu::subtitle_file::{read_subtitles, write_srt};
use disccraft_project::{AssetId, AssetKind};
use serde_json::{Value, json};

use super::always_enabled;

/// Ticks per second (`filmcraft-time`).
const TICKS_PER_SECOND: f64 = 254_016_000_000.0;

/// Subtitle commands.
pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "subtitle.cues",
        label: "字幕の一覧",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"asset": id}"#,
        enabled: always_enabled,
        run: run_cues,
        journal: false,
    },
    CommandSpec {
        id: "subtitle.exportSrt",
        label: "字幕を書き出し",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"asset": id, "path": string}"#,
        enabled: always_enabled,
        run: run_export,
        journal: false,
    },
];

/// The subtitle file of an asset.
fn subtitle_path(session: &Session, params: &Value) -> Result<String, String> {
    let id = req_u64(params, "asset")?;
    let asset = session
        .project
        .assets
        .get(&AssetId(id))
        .ok_or_else(|| format!("unknown asset: {id}"))?;
    if asset.kind != AssetKind::Subtitle {
        return Err("not a subtitle asset".to_string());
    }
    Ok(asset.path.clone())
}

/// `{"cues": [{"start": seconds, "end": seconds, "text": string}]}`, sorted by start.
fn run_cues(session: &mut Session, params: &Value) -> CommandResult {
    let path = subtitle_path(session, params)?;
    let cues = read_subtitles(Path::new(&path), &session.project.settings)
        .map_err(|error| error.to_string())?;
    let cues: Vec<Value> = cues
        .iter()
        .map(|cue| {
            json!({
                "start": cue.start.0 as f64 / TICKS_PER_SECOND,
                "end": cue.end.0 as f64 / TICKS_PER_SECOND,
                "text": cue.text,
            })
        })
        .collect();
    Ok(json!({ "cues": cues }))
}

/// Writes the asset's cues (any supported format) to `path` as SRT in UTF-8.
fn run_export(session: &mut Session, params: &Value) -> CommandResult {
    let source = subtitle_path(session, params)?;
    let target = req_str(params, "path")?;
    let cues = read_subtitles(Path::new(&source), &session.project.settings)
        .map_err(|error| error.to_string())?;
    std::fs::write(&target, write_srt(&cues)).map_err(|error| format!("{target}: {error}"))?;
    Ok(json!({ "path": target, "cues": cues.len() }))
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use serde_json::{Value, json};
    use std::error::Error;

    #[test]
    fn cues_and_export() -> Result<(), Box<dyn Error>> {
        let dir =
            std::env::temp_dir().join(format!("disccraft-engine-subs-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let vtt = dir.join("en.vtt");
        std::fs::write(&vtt, "WEBVTT\n\n00:01.000 --> 00:02.500\nHello\n")?;
        let mut session = Session::new();
        let imported = session.execute("asset.import", &json!({ "path": vtt }))?;
        let asset = imported
            .get("asset")
            .and_then(Value::as_u64)
            .ok_or("asset")?;

        let cues = session.execute("subtitle.cues", &json!({ "asset": asset }))?;
        let first = cues.get("cues").and_then(|cues| cues.get(0)).ok_or("cue")?;
        assert_eq!(first.get("text").and_then(Value::as_str), Some("Hello"));
        assert_eq!(first.get("end").and_then(Value::as_f64), Some(2.5));

        let srt = dir.join("en.srt");
        session.execute(
            "subtitle.exportSrt",
            &json!({ "asset": asset, "path": srt }),
        )?;
        assert!(std::fs::read_to_string(&srt)?.contains("00:00:01,000 --> 00:00:02,500"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
