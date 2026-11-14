//! DiscCraft headless CLI (L6).
//!
//! Provides non-interactive access to the DiscCraft editing engine:
//! command discovery, single command execution, scripted execution, and
//! project inspection.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod inspect;

use clap::{Parser, Subcommand};
use disccraft_engine::{Session, commands};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const MAX_SCRIPT_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Parser)]
#[command(name = "disccraft-cli", version, about = "DiscCraft headless CLI")]
struct Cli {
    #[command(subcommand)]
    command: Sub,
}

#[derive(Subcommand)]
enum Sub {
    Commands {
        #[arg(long)]
        json: bool,
    },
    Exec {
        project: PathBuf,
        command: String,
        params: Option<String>,
    },
    Run {
        project: PathBuf,
        script: PathBuf,
    },
    Inspect {
        project: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Run the MCP server (JSON-RPC over stdin / stdout) for AI agents
    Mcp,
    Build {
        project: PathBuf,
        /// Output folder (VIDEO_TS is written inside it)
        #[arg(long, required_unless_present = "iso")]
        out: Option<PathBuf>,
        /// Write a DVD ISO image instead of a folder
        #[arg(long)]
        iso: Option<PathBuf>,
        /// Keep transcoded media in this folder (shared between builds) instead of next to the output
        #[arg(long)]
        cache: Option<PathBuf>,
        /// Language (ISO 639-1, e.g. ja) of audio without its own when the project names none; else the
        /// system's language
        #[arg(long)]
        audio_language: Option<String>,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            use std::io::Write;
            // Like the output, a closed error stream is not a reason to panic.
            let _ = writeln!(std::io::stderr(), "error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Sub::Commands { json } => list_commands(json),
        Sub::Exec {
            project,
            command,
            params,
        } => exec_command(&project, &command, params.as_deref()),
        Sub::Run { project, script } => run_script(&project, &script),
        Sub::Inspect { project, json } => inspect_project(&project, json),
        Sub::Mcp => disccraft_automation::McpServer::new()
            .serve(std::io::stdin().lock(), std::io::stdout().lock())
            .map_err(|e| format!("mcp: {e}")),
        Sub::Build {
            project,
            out,
            iso,
            cache,
            audio_language,
        } => match (iso, out) {
            (Some(iso), _) => build_project(
                &project,
                &iso,
                "iso",
                (cache.as_deref(), audio_language.as_deref()),
            ),
            (None, Some(out)) => build_project(
                &project,
                &out,
                "folder",
                (cache.as_deref(), audio_language.as_deref()),
            ),
            (None, None) => Err("--out or --iso is required".to_string()),
        },
    }
}

fn list_commands(json: bool) -> Result<(), String> {
    let specs = commands();

    if json {
        let mut items = Vec::with_capacity(specs.len());

        for spec in specs {
            let mut map = Map::new();
            map.insert("id".to_string(), Value::String(spec.id.to_string()));
            map.insert("label".to_string(), Value::String(spec.label.to_string()));
            map.insert(
                "menu".to_string(),
                Value::Array(
                    spec.menu
                        .iter()
                        .map(|item| Value::String(item.to_string()))
                        .collect(),
                ),
            );

            let shortcut = match spec.shortcut {
                Some(shortcut) => Value::String(shortcut.to_string()),
                None => Value::Null,
            };
            map.insert("shortcut".to_string(), shortcut);
            map.insert("params".to_string(), Value::String(spec.params.to_string()));
            map.insert("journal".to_string(), Value::Bool(spec.journal));

            items.push(Value::Object(map));
        }

        print_json_pretty(&Value::Array(items))?;
    } else {
        let id_width = specs
            .iter()
            .fold(0usize, |acc, spec| acc.max(spec.id.chars().count()));
        let label_width = specs
            .iter()
            .fold(0usize, |acc, spec| acc.max(spec.label.chars().count()));

        for spec in specs {
            say(&format!(
                "{id:<id_width$}  {label:<label_width$}  {params}",
                id = spec.id,
                label = spec.label,
                params = spec.params,
                id_width = id_width,
                label_width = label_width
            ))?;
        }
    }

    Ok(())
}

fn exec_command(project: &Path, command: &str, params: Option<&str>) -> Result<(), String> {
    let params = parse_params(params)?;
    let mut session = open_session(project)?;

    let result = session.execute(command, &params)?;
    print_json_pretty(&result)?;
    save_if_dirty(&mut session, project)?;

    Ok(())
}

fn run_script(project: &Path, script: &Path) -> Result<(), String> {
    let metadata = fs::metadata(script).map_err(|error| format!("cannot read script: {error}"))?;
    if metadata.len() > MAX_SCRIPT_BYTES {
        return Err("script larger than 16 MiB".to_string());
    }

    let content =
        fs::read_to_string(script).map_err(|error| format!("cannot read script: {error}"))?;
    let mut session = open_session(project)?;

    for (index, raw) in content.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }

        let value = serde_json::from_str::<Value>(trimmed)
            .map_err(|error| format!("line {line}: invalid JSON: {error}"))?;

        let Some(command) = value.get("command").and_then(Value::as_str) else {
            return Err(format!("line {line}: missing command"));
        };

        let params = match value.get("params") {
            Some(params) => params.clone(),
            None => Value::Null,
        };

        let result = session
            .execute(command, &params)
            .map_err(|error| format!("line {line}: {error}"))?;

        let text = serde_json::to_string(&result)
            .map_err(|error| format!("line {line}: serialization failed: {error}"))?;
        say(&text)?;
    }

    save_if_dirty(&mut session, project)?;
    Ok(())
}

fn inspect_project(project: &Path, json: bool) -> Result<(), String> {
    let mut session = Session::open(project).map_err(|error| error.to_string())?;

    if json {
        let result = session.execute("project.inspect", &Value::Null)?;
        print_json_pretty(&result)?;
    } else {
        let summary = inspect::human_summary(&session.project)?;
        say(&summary)?;
    }

    Ok(())
}

fn build_project(
    project: &Path,
    out: &Path,
    output: &str,
    (cache, audio_language): (Option<&Path>, Option<&str>),
) -> Result<(), String> {
    if !project.exists() {
        return Err(format!(
            "project file does not exist: {}",
            project.display()
        ));
    }

    let mut session = Session::open(project)?;
    let mut params = Map::new();
    params.insert("output".to_string(), Value::String(output.to_string()));
    params.insert(
        "path".to_string(),
        Value::String(out.to_string_lossy().into_owned()),
    );
    if let Some(cache) = cache {
        params.insert(
            "cache".to_string(),
            Value::String(cache.to_string_lossy().into_owned()),
        );
    }
    if let Some(language) = audio_language {
        params.insert(
            "audioLanguage".to_string(),
            Value::String(language.to_string()),
        );
    }

    let result = session.execute("build.start", &Value::Object(params))?;
    print_build_summary(&result)
}

fn print_build_summary(result: &Value) -> Result<(), String> {
    let output = result
        .get("output")
        .and_then(Value::as_str)
        .ok_or_else(|| "build result missing output".to_string())?;
    let titles = result
        .get("titles")
        .and_then(Value::as_u64)
        .ok_or_else(|| "build result missing titles".to_string())?;
    let chapters = result
        .get("chapters")
        .and_then(Value::as_u64)
        .ok_or_else(|| "build result missing chapters".to_string())?;
    let total_bytes = result
        .get("totalBytes")
        .and_then(Value::as_u64)
        .ok_or_else(|| "build result missing totalBytes".to_string())?;
    let seconds = result
        .get("seconds")
        .and_then(Value::as_f64)
        .ok_or_else(|| "build result missing seconds".to_string())?;

    let mut megabytes_whole = total_bytes / 1_000_000;
    let remainder = total_bytes % 1_000_000;
    let mut megabytes_tenth = (remainder + 50_000) / 100_000;

    if megabytes_tenth == 10 {
        megabytes_whole += 1;
        megabytes_tenth = 0;
    }

    say(&format!("書き出した: {output}"))?;
    say(&format!(
        "タイトル {titles} / チャプター {chapters} / {megabytes_whole}.{megabytes_tenth} MB / {seconds:.1} 秒"
    ))
}

fn open_session(project: &Path) -> Result<Session, String> {
    if project.exists() {
        Session::open(project).map_err(|error| error.to_string())
    } else {
        let mut session = Session::new();
        session.path = Some(project.to_path_buf());
        Ok(session)
    }
}

fn save_if_dirty(session: &mut Session, project: &Path) -> Result<(), String> {
    if session.dirty {
        session
            .save(Some(project))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn parse_params(params: Option<&str>) -> Result<Value, String> {
    match params {
        Some(text) => {
            serde_json::from_str(text).map_err(|error| format!("invalid params JSON: {error}"))
        }
        None => Ok(Value::Object(Map::new())),
    }
}

fn print_json_pretty(value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    say(&text)
}

/// Writes one line to standard output. When the reader has gone (`disccraft-cli commands | head`), the rest of the
/// output is dropped and the command carries on (a script still runs and saves); `println!` would panic there.
fn say(text: &str) -> Result<(), String> {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    match writeln!(out, "{text}").and_then(|()| out.flush()) {
        Err(error) if error.kind() != std::io::ErrorKind::BrokenPipe => {
            Err(format!("cannot write the output: {error}"))
        }
        _ => Ok(()),
    }
}
