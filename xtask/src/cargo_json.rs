//! What a cargo command builds, from its JSON messages (`--message-format=json-render-diagnostics`): every unit's
//! files, a test unit's executable and package, and the build scripts' output directories. The diagnostics are
//! rendered to the terminal as usual; only the messages on standard output are read.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::Result;

/// One unit cargo built (or found fresh).
#[derive(Debug, Clone)]
pub struct Artifact {
    /// The files it wrote: libraries, executables, debug information.
    pub filenames: Vec<PathBuf>,
    /// The executable, for a binary or a test.
    pub executable: Option<PathBuf>,
    /// Built as a test (a test harness that runs `#[test]` functions).
    pub test: bool,
    /// The target's name (`bluray_folder`, `disccraft_bd`).
    pub target: String,
    /// The target's kind (`lib`, `bin`, `test`, `example`, `custom-build`).
    pub kind: String,
    /// The package's directory (where cargo runs its tests).
    pub package_dir: PathBuf,
}

/// The units and build script output directories of one cargo command.
#[derive(Debug, Default)]
pub struct Build {
    pub artifacts: Vec<Artifact>,
    pub out_dirs: Vec<PathBuf>,
}

/// Runs `cargo <args> --message-format=json-render-diagnostics` in `root` and reads what it built. An error when cargo
/// fails (the diagnostics are on the terminal).
pub fn build(root: &Path, args: &[&str]) -> Result<Build> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut all: Vec<&str> = args.to_vec();
    // The message format goes before a `--` that passes arguments on (clippy's `-D warnings`).
    let at = all.iter().position(|arg| *arg == "--").unwrap_or(all.len());
    all.insert(at, "--message-format=json-render-diagnostics");
    let command = format!("cargo {}", args.join(" "));
    let output = crate::without_xtask_env(&mut Command::new(cargo))
        .args(&all)
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("failed to run {command}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{command} failed"));
    }
    let mut build = Build::default();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match message.get("reason").and_then(|reason| reason.as_str()) {
            Some("compiler-artifact") => {
                if let Some(artifact) = artifact_of(&message) {
                    build.artifacts.push(artifact);
                }
            }
            Some("build-script-executed") => {
                if let Some(out_dir) = message.get("out_dir").and_then(|dir| dir.as_str()) {
                    build.out_dirs.push(PathBuf::from(out_dir));
                }
            }
            _ => {}
        }
    }
    Ok(build)
}

fn artifact_of(message: &serde_json::Value) -> Option<Artifact> {
    let filenames = message
        .get("filenames")?
        .as_array()?
        .iter()
        .filter_map(|name| name.as_str().map(PathBuf::from))
        .collect();
    let executable = message
        .get("executable")
        .and_then(|exe| exe.as_str())
        .map(PathBuf::from);
    let test = message
        .get("profile")
        .and_then(|profile| profile.get("test"))
        .and_then(|test| test.as_bool())
        .unwrap_or(false);
    let target = message.get("target")?;
    let name = target.get("name")?.as_str()?.to_string();
    let kind = target
        .get("kind")
        .and_then(|kinds| kinds.as_array())
        .and_then(|kinds| kinds.first())
        .and_then(|kind| kind.as_str())
        .unwrap_or("")
        .to_string();
    let package_dir = Path::new(message.get("manifest_path")?.as_str()?)
        .parent()?
        .to_path_buf();
    Some(Artifact {
        filenames,
        executable,
        test,
        target: name,
        kind,
        package_dir,
    })
}
