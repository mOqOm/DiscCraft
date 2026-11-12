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
mod layers;

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
        Some("ci") => run_ci(&root),
        Some(_) | None => Err(usage()),
    };

    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn usage() -> String {
    "usage: cargo xtask <fmt|clippy|test|layers|assets|ci>".to_string()
}

fn repo_root() -> Result<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    match manifest_dir.parent() {
        Some(root) => Ok(root.to_path_buf()),
        None => Err("cannot find repository root".to_string()),
    }
}

fn cargo_cmd(root: &Path, args: &[&str]) -> Result<()> {
    let cargo = match std::env::var("CARGO") {
        Ok(cargo) => cargo,
        Err(_) => "cargo".to_string(),
    };
    let command = format!("cargo {}", args.join(" "));

    let status = Command::new(cargo)
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

fn run_test(root: &Path) -> Result<()> {
    cargo_cmd(root, &["test", "--workspace"])
}

fn run_ci(root: &Path) -> Result<()> {
    println!("==> cargo fmt --all -- --check");
    cargo_cmd(root, &["fmt", "--all", "--", "--check"])?;

    println!("==> cargo clippy --workspace --all-targets -- -D warnings");
    run_clippy(root)?;

    println!("==> cargo test --workspace");
    run_test(root)?;

    println!("==> layers");
    layers::run(root)?;

    println!("==> assets");
    assets::run(root)?;

    Ok(())
}
