//! Workspace automation tasks for DiscCraft.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::path::{Path, PathBuf};
use std::process::Command;

mod assets;
mod cargo_json;
mod icons;
mod layers;
mod manual;
mod sweep;
mod test_run;

type Result<T> = std::result::Result<T, String>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = match repo_root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    };

    let command = args.first().map(String::as_str);
    let result = match command {
        Some("fmt") => run_fmt(&root),
        Some("clippy") => run_clippy(&root),
        Some("test") => run_test(&root),
        Some("layers") => layers::run(&root),
        Some("assets") => assets::run(&root),
        Some("icons") => icons::run(&root),
        Some("manual") => manual::run(&root, args.get(1..).unwrap_or_default()),
        Some("ci") => run_ci(&root),
        Some("sweep") => sweep::run(&root, args.get(1).is_some_and(|arg| arg == "--dry-run")),
        Some(_) | None => Err(usage()),
    };

    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn usage() -> String {
    "usage: cargo xtask <fmt|clippy|test|layers|assets|icons|manual [--lang ja] [--pdf] [--release <tag>]|ci|sweep [--dry-run]>"
        .to_string()
}

fn repo_root() -> Result<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    match manifest_dir.parent() {
        Some(root) => Ok(root.to_path_buf()),
        None => Err("cannot find repository root".to_string()),
    }
}

/// Takes out of `command`'s environment what `cargo run` set for xtask itself (its package's `CARGO_PKG_*`,
/// `CARGO_MANIFEST_DIR`, ...). A build script that reruns when one of them changes (ring's) otherwise made every
/// switch between `cargo xtask` and a cargo run from the shell rebuild 22 crates, the application among them
/// (M6.354).
pub(crate) fn without_xtask_env(command: &mut Command) -> &mut Command {
    for (key, _) in std::env::vars_os() {
        let key = key.to_string_lossy();
        if key.starts_with("CARGO_PKG_")
            || matches!(
                key.as_ref(),
                "CARGO_MANIFEST_DIR"
                    | "CARGO_MANIFEST_PATH"
                    | "CARGO_PRIMARY_PACKAGE"
                    | "CARGO_BIN_NAME"
                    | "CARGO_CRATE_NAME"
                    | "RUST_RECURSION_COUNT"
            )
        {
            command.env_remove(key.as_ref());
        }
    }
    command
}

fn cargo_cmd(root: &Path, args: &[&str]) -> Result<()> {
    let cargo = match std::env::var("CARGO") {
        Ok(cargo) => cargo,
        Err(_) => "cargo".to_string(),
    };
    let command = format!("cargo {}", args.join(" "));

    let status = without_xtask_env(&mut Command::new(cargo))
        .args(args)
        .current_dir(root)
        .status()
        .map_err(|error| format!("failed to run {command}: {error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("{command} failed"))
    }
}

fn run_fmt(root: &Path) -> Result<()> {
    cargo_cmd(root, &["fmt", "--all"])
}

fn run_clippy(root: &Path) -> Result<()> {
    cargo_cmd(
        root,
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )
}

/// Every test target (libraries, binaries, integration tests, examples), without doc-tests: no crate has any, and
/// collecting none still took a rustdoc pass of each of the 26 libraries, about 40 s of every run (M6.353). The test
/// binaries run several at a time (M6.354, [`test_run`]).
fn run_test(root: &Path) -> Result<()> {
    test_run::run(root)
}

fn run_ci(root: &Path) -> Result<()> {
    println!("==> cargo fmt --all -- --check");
    cargo_cmd(root, &["fmt", "--all", "--", "--check"])?;

    println!("==> cargo clippy --workspace --all-targets -- -D warnings");
    run_clippy(root)?;

    println!("==> tests (cargo test --workspace --all-targets, several binaries at a time)");
    run_test(root)?;

    println!("==> layers");
    layers::run(root)?;

    println!("==> assets");
    assets::run(root)?;

    // M15.1: the user manual's links between chapters and its pictures.
    println!("==> manual");
    manual::check_all(root)?;

    // Old build products are deleted once they take too much room (M6.354).
    let limit_gb = std::env::var("DISCCRAFT_SWEEP_GB")
        .ok()
        .and_then(|gb| gb.parse::<u64>().ok())
        .unwrap_or(100);
    let size = sweep::debug_size(root);
    if size > limit_gb.saturating_mul(1_000_000_000) {
        println!(
            "==> target/debug is {:.0} GB (over {limit_gb} GB): sweep",
            size as f64 / 1e9
        );
        sweep::run(root, false)?;
    }

    Ok(())
}
