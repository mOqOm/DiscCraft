//! Build commands.

use crate::params::{opt_str, req_str};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_build::{BuildOptions, OutputKind, TranscodeStatus};
use serde_json::Value;
use std::path::PathBuf;

use super::{always_enabled, object};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "build.start",
        label: "ビルド",
        menu: &["ファイル"],
        shortcut: None,
        params: r#"{"output": "folder" | "iso" | "ddp" (DVD のみ), "path": string, "cache"?: string, "audioLanguage"?: "ja"}"#,
        enabled: always_enabled,
        run: run_build,
        journal: false,
    },
    CommandSpec {
        id: "build.transcode",
        label: "今すぐトランスコード",
        menu: &["ファイル", "トランスコード"],
        shortcut: None,
        params: r#"{"output": "folder" | "iso" | "ddp" (DVD のみ), "path": string, "cache"?: string, "asset"?: id (そのアセットを使うタイムラインだけ。なければプロジェクト全体)}"#,
        enabled: always_enabled,
        run: run_transcode,
        journal: false,
    },
    CommandSpec {
        id: "build.revert",
        label: "オリジナルに戻す",
        menu: &["ファイル", "トランスコード"],
        shortcut: None,
        params: r#"{"output": "folder" | "iso" | "ddp" (DVD のみ), "path": string, "cache"?: string, "asset": id} → {"removed": 消したファイルの数}"#,
        enabled: always_enabled,
        run: run_revert,
        journal: false,
    },
    CommandSpec {
        id: "asset.transcodeStatus",
        label: "アセットのトランスコードステータス",
        menu: &["アセット"],
        shortcut: None,
        params: r#"{"output"?: "folder" | "iso" | "ddp", "path"?: string}"#,
        enabled: always_enabled,
        run: run_transcode_status,
        journal: false,
    },
];

/// Every asset's transcode status. With the build output (`output` and `path`), transcodes already in that
/// build's cache are reported as `"transcoded"`.
fn run_transcode_status(session: &mut Session, params: &Value) -> CommandResult {
    // Only an existing cache counts: checking it means probing every timeline for the bitrate budget.
    let cache = match opt_str(params, "path")?.filter(|path| !path.trim().is_empty()) {
        Some(path) => {
            let output = parse_output(&opt_str(params, "output")?.unwrap_or_default())
                .unwrap_or(OutputKind::Folder);
            Some(disccraft_build::cache_dir(&BuildOptions {
                output,
                path: PathBuf::from(path),
                cache: None,
                audio_language: None,
            }))
            .filter(|dir| dir.is_dir())
        }
        None => None,
    };
    let assets = disccraft_build::transcode_status(&session.project, cache.as_deref())
        .into_iter()
        .map(|(asset, status)| {
            object(&[
                ("asset", Value::from(asset.0)),
                ("status", Value::from(status_name(status))),
                // A video that fits the disc as it is (whether or not a preset of its own transcodes it).
                (
                    "fits",
                    Value::from(disccraft_build::video_fits_as_is(&session.project, asset)),
                ),
                // And on each kind of disc (for the column of the other kind).
                (
                    "fitsDvd",
                    Value::from(disccraft_build::video_fits_disc(
                        &session.project,
                        asset,
                        disccraft_project::DiscKind::Dvd,
                    )),
                ),
                (
                    "fitsBluray",
                    Value::from(disccraft_build::video_fits_disc(
                        &session.project,
                        asset,
                        disccraft_project::DiscKind::BluRay,
                    )),
                ),
            ])
        })
        .collect();
    Ok(object(&[("assets", Value::Array(assets))]))
}

/// The system's language as ISO 639-1 (`ja` from `ja_JP.UTF-8`), from the usual environment variables, as the GUI
/// starts its display language; `None` when none says (Windows sets none of them).
fn system_language() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|variable| {
            let value = std::env::var(variable).ok()?;
            let primary = value
                .split(['-', '_', '.', '@'])
                .next()?
                .to_ascii_lowercase();
            (primary.len() == 2 && primary.bytes().all(|b| b.is_ascii_lowercase()))
                .then_some(primary)
        })
}

fn status_name(status: TranscodeStatus) -> &'static str {
    match status {
        TranscodeStatus::NotApplicable => "n/a",
        TranscodeStatus::Unneeded => "unneeded",
        TranscodeStatus::Untranscoded => "untranscoded",
        TranscodeStatus::Transcoded => "transcoded",
    }
}

/// The build a transcode command speaks of: where it writes, and so where its cache is.
fn transcode_options(params: &Value) -> Result<BuildOptions, String> {
    let output = parse_output(&req_str(params, "output")?)?;
    Ok(BuildOptions {
        output,
        path: PathBuf::from(req_str(params, "path")?),
        cache: opt_str(params, "cache")?
            .filter(|cache| !cache.trim().is_empty())
            .map(PathBuf::from),
        audio_language: opt_str(params, "audioLanguage")?
            .filter(|code| !code.trim().is_empty())
            .or_else(system_language),
    })
}

/// Encore's Transcode Now: makes the build's transcodes (of one asset's timelines, or all) without writing a disc.
fn run_transcode(session: &mut Session, params: &Value) -> CommandResult {
    let options = transcode_options(params)?;
    let asset = crate::params::opt_u64(params, "asset")?.map(disccraft_project::AssetId);
    let started = std::time::Instant::now();
    disccraft_build::transcode_now(&session.project, &options, asset, &mut |_, _| {})?;
    Ok(object(&[(
        "seconds",
        Value::from(started.elapsed().as_secs_f64()),
    )]))
}

/// Encore's Revert to Original: takes an asset's transcodes out of the build's cache.
fn run_revert(session: &mut Session, params: &Value) -> CommandResult {
    let options = transcode_options(params)?;
    let asset = disccraft_project::AssetId(crate::params::req_u64(params, "asset")?);
    let removed = disccraft_build::revert_to_original(&session.project, &options, asset)?;
    Ok(object(&[("removed", Value::from(removed as u64))]))
}

fn run_build(session: &mut Session, params: &Value) -> CommandResult {
    let output_text = req_str(params, "output")?;
    let output = parse_output(&output_text)?;
    let path = PathBuf::from(req_str(params, "path")?);
    let cache = crate::params::opt_str(params, "cache")?
        .filter(|cache| !cache.trim().is_empty())
        .map(PathBuf::from);
    // The language of audio without its own, when the project names none: the one given, else the system's.
    let audio_language = crate::params::opt_str(params, "audioLanguage")?
        .filter(|code| !code.trim().is_empty())
        .or_else(system_language);
    let options = BuildOptions {
        output,
        path,
        cache,
        audio_language,
    };

    let report = disccraft_build::build(&session.project, &options)?;
    let titles =
        u64::try_from(report.titles).map_err(|_| "title count is too large".to_string())?;
    let chapters =
        u64::try_from(report.chapters).map_err(|_| "chapter count is too large".to_string())?;

    Ok(object(&[
        (
            "output",
            Value::String(report.output.to_string_lossy().into_owned()),
        ),
        ("titles", Value::from(titles)),
        ("chapters", Value::from(chapters)),
        ("totalBytes", Value::from(report.total_bytes)),
        ("seconds", Value::from(report.seconds)),
        (
            "layerBreak",
            report
                .layer_break
                .as_ref()
                .map_or(Value::Null, |found| Value::from(found.sector)),
        ),
        (
            "layerBreakDetail",
            report
                .layer_break
                .as_ref()
                .map_or(Value::Null, layer_break_json),
        ),
    ]))
}

/// The layer break as read back from the image: the sectors, the mode, where it is and whether it is where asked.
fn layer_break_json(found: &disccraft_build::LayerBreak) -> Value {
    let place = found.place.as_ref().map_or(Value::Null, |place| {
        object(&[
            ("timeline", Value::from(place.timeline)),
            ("title", Value::from(place.title)),
            ("vts", Value::from(place.vts)),
            ("cell", Value::from(place.cell)),
            ("time90k", Value::from(place.time_90k)),
            (
                "milliseconds",
                Value::from(disccraft_build::millis(place.time_90k)),
            ),
        ])
    });
    object(&[
        ("sector", Value::from(found.sector)),
        ("totalSectors", Value::from(found.total_sectors)),
        ("layer0Sectors", Value::from(found.sector)),
        ("layer1Sectors", Value::from(found.layer1_sectors())),
        ("manual", Value::from(found.manual)),
        ("place", place),
        (
            "requested90k",
            found.requested_90k.map_or(Value::Null, Value::from),
        ),
        (
            "asRequested",
            found.as_requested().map_or(Value::Null, Value::from),
        ),
    ])
}

fn parse_output(text: &str) -> Result<OutputKind, String> {
    match text {
        "folder" | "dvdFolder" => Ok(OutputKind::Folder),
        "iso" => Ok(OutputKind::Iso),
        "ddp" => Ok(OutputKind::Ddp),
        _ => Err(format!("unknown build output: {text}")),
    }
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use serde_json::{Map, Value};
    use std::error::Error;

    fn object(entries: &[(&str, Value)]) -> Value {
        let mut map = Map::new();
        for (key, value) in entries {
            map.insert((*key).to_string(), (*value).clone());
        }
        Value::Object(map)
    }

    #[test]
    fn build_start_on_empty_project_fails() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let path = std::env::temp_dir().join(format!(
            "disccraft-engine-build-empty-{}",
            std::process::id()
        ));

        let params = object(&[
            ("output", Value::String("folder".to_string())),
            ("path", Value::String(path.to_string_lossy().into_owned())),
        ]);

        match session.execute("build.start", &params) {
            Ok(_) => Err("expected build failure".to_string().into()),
            Err(message) => {
                if message.is_empty() {
                    Err("expected non-empty build error".to_string().into())
                } else {
                    Ok(())
                }
            }
        }
    }
    #[test]
    fn transcode_status_lists_every_asset() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let _ = session
            .project
            .add_asset(std::path::Path::new("movie.mp4"))?;
        let _ = session
            .project
            .add_asset(std::path::Path::new("cover.png"))?;

        let result = session.execute("asset.transcodeStatus", &object(&[]))?;
        let statuses: Vec<&str> = result
            .get("assets")
            .and_then(Value::as_array)
            .ok_or("missing assets")?
            .iter()
            .filter_map(|entry| entry.get("status").and_then(Value::as_str))
            .collect();
        assert_eq!(statuses, ["untranscoded", "n/a"]);
        Ok(())
    }
}
