//! `cargo xtask sweep [--dry-run]` (M6.354): deletes the build products in `target/debug` that the current builds no
//! longer use. Cargo never does: every dependency update or profile change leaves the old ones behind, and
//! `target/debug` grew to hundreds of gigabytes (558 GB on 2026-11-18, 369 GB on 2026-11-19).
//!
//! In use is what cargo reports for the builds this repository makes: the tests (`cargo test --workspace
//! --all-targets --no-run`), clippy as `cargo xtask ci` runs it, the applications (`cargo build --workspace`, and
//! each as `cargo run -p` builds it) and xtask itself.
//! A file in `deps` / `examples` is kept when its name (up to the first dot, with or without a `lib` in front) is one
//! of theirs, a directory in `build` when one of their files or build script outputs is in it. Incremental caches
//! are kept for three days. Nothing written after the sweep started is touched.
//!
//! `cargo xtask ci` sweeps by itself when `target/debug` is larger than `DISCCRAFT_SWEEP_GB` (default 100).

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path};
use std::time::{Duration, SystemTime};

use crate::Result;
use crate::cargo_json;

/// Incremental caches unused this long are deleted.
const INCREMENTAL_DAYS: u64 = 3;

pub fn run(root: &Path, dry_run: bool) -> Result<()> {
    let started = SystemTime::now();
    let debug = root.join("target").join("debug");
    let mut builds = Vec::new();
    for args in [
        &["test", "--workspace", "--all-targets", "--no-run"][..],
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ][..],
        // Without xtask, which is running (a workspace build unifies features differently and would relink it), and
        // xtask as `cargo xtask` builds it.
        &["build", "--workspace", "--exclude", "xtask"][..],
        // Each application alone, as `cargo run -p disccraft` / `-p disccraft-cli` builds it (the features of its
        // dependencies differ from a workspace build's).
        &["build", "--package", "disccraft"][..],
        &["build", "--package", "disccraft-cli"][..],
        &["build", "--package", "xtask"][..],
    ] {
        println!("==> what cargo {} uses", args.join(" "));
        builds.push(cargo_json::build(root, args)?);
    }

    // The names in use: `foo-0123abcd` (and `libfoo-0123abcd`), and the `build` directories.
    let mut stems = HashSet::new();
    let mut build_dirs = HashSet::new();
    for build in &builds {
        let paths = build
            .artifacts
            .iter()
            .flat_map(|artifact| artifact.filenames.iter().chain(artifact.executable.iter()))
            .chain(build.out_dirs.iter());
        for path in paths {
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                let stem = name.split('.').next().unwrap_or(name);
                stems.insert(stem.to_string());
                stems.insert(stem.strip_prefix("lib").unwrap_or(stem).to_string());
            }
            if let Ok(inside) = path.strip_prefix(debug.join("build"))
                && let Some(Component::Normal(dir)) = inside.components().next()
            {
                build_dirs.insert(dir.to_string_lossy().into_owned());
            }
        }
    }
    if stems.is_empty() {
        return Err("cargo reported no build products: nothing is swept".to_string());
    }

    let mut swept = Swept::default();
    for dir in ["deps", "examples"] {
        let Ok(entries) = fs::read_dir(debug.join(dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let stem = name.split('.').next().unwrap_or(&name);
            let used =
                stems.contains(stem) || stems.contains(stem.strip_prefix("lib").unwrap_or(stem));
            if !used && older_than(&entry.path(), started) {
                swept.remove(&entry.path(), dry_run);
            }
        }
    }
    if let Ok(entries) = fs::read_dir(debug.join("build")) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !build_dirs.contains(&name) && older_than(&entry.path(), started) {
                swept.remove(&entry.path(), dry_run);
            }
        }
    }
    let unused = started
        .checked_sub(Duration::from_secs(INCREMENTAL_DAYS * 24 * 60 * 60))
        .unwrap_or(started);
    if let Ok(entries) = fs::read_dir(debug.join("incremental")) {
        for entry in entries.flatten() {
            if older_than(&entry.path(), unused) {
                swept.remove(&entry.path(), dry_run);
            }
        }
    }
    println!(
        "sweep: {} {} ({:.1} GB){}",
        if dry_run { "would delete" } else { "deleted" },
        swept.count,
        swept.bytes as f64 / 1e9,
        if swept.failed > 0 {
            format!(", {} could not be deleted (in use?)", swept.failed)
        } else {
            String::new()
        }
    );
    Ok(())
}

/// The size of `target/debug`'s build products (`deps`, `examples`, `build`, `incremental`), bytes.
pub fn debug_size(root: &Path) -> u64 {
    let debug = root.join("target").join("debug");
    ["deps", "examples", "build", "incremental"]
        .iter()
        .map(|dir| tree_size(&debug.join(dir)))
        .sum()
}

#[derive(Default)]
struct Swept {
    count: usize,
    bytes: u64,
    failed: usize,
}

impl Swept {
    fn remove(&mut self, path: &Path, dry_run: bool) {
        let bytes = tree_size(path);
        if dry_run {
            println!("  {:8.1} MB  {}", bytes as f64 / 1e6, path.display());
        }
        let removed = dry_run
            || if path.is_dir() {
                fs::remove_dir_all(path).is_ok()
            } else {
                fs::remove_file(path).is_ok()
            };
        if removed {
            self.count += 1;
            self.bytes += bytes;
        } else {
            self.failed += 1;
        }
    }
}

/// Whether `path` was last written before `time`.
fn older_than(path: &Path, time: SystemTime) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| modified < time)
}

fn tree_size(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if !metadata.is_dir() {
        return metadata.len();
    }
    fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| tree_size(&entry.path()))
                .sum()
        })
        .unwrap_or(0)
}
