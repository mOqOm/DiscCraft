//! What DiscCraft's tests that compare with ffmpeg share (AGENTS.md rule 9, M9.67): where ffmpeg and ffprobe are, and
//! where the fixtures they make are kept. A dev-dependency only; nothing in the product uses it.
//!
//! A tool is looked for as the tests always have, `DISCCRAFT_FFMPEG` / `DISCCRAFT_FFPROBE` first; without one, through
//! craft-libs' oracle-test search ([`craft_media_testkit::tool`]: `CRAFT_MEDIA_FFMPEG` / `CRAFT_MEDIA_FFPROBE`, the
//! `PATH`, then the usual install folders). It counts only when it answers `-version`; without it, a test skips only
//! the part that compares.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// ffmpeg, when there is one that runs (found once for the test program).
pub fn ffmpeg() -> Option<PathBuf> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| find(std::env::var_os("DISCCRAFT_FFMPEG"), "ffmpeg"))
        .clone()
}

/// ffprobe, when there is one that runs (found once for the test program).
pub fn ffprobe() -> Option<PathBuf> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| find(std::env::var_os("DISCCRAFT_FFPROBE"), "ffprobe"))
        .clone()
}

/// The workspace's `target/fixtures`, where the tests keep the material they make with ffmpeg and look for the
/// material other tests made. Not created here.
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures")
}

/// A name beside the fixture `out` that no other test thread or process uses, with `out`'s extension (ffmpeg picks
/// its muxer from it): a fixture is made there and then moved to `out`, so a test never reads one another test is
/// still writing (craft-media-testkit's).
pub use craft_media_testkit::temp_path;

/// `explicit` (the DISCCRAFT_ variable) when it is set, else the tool craft-libs' search finds, else `name` for the
/// system to look up; `None` when that does not answer `-version`.
fn find(explicit: Option<OsString>, name: &str) -> Option<PathBuf> {
    let path = match explicit.filter(|path| !path.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => craft_media_testkit::tool(name).unwrap_or_else(|| PathBuf::from(name)),
    };
    answers(&path).then_some(path)
}

fn answers(program: &Path) -> bool {
    Command::new(program)
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn a_tool_that_does_not_run_is_none() {
        let missing = std::env::temp_dir()
            .join("disccraft-no-such-tool")
            .join("ffmpeg.exe");
        assert_eq!(find(Some(missing.into_os_string()), "ffmpeg"), None);
        assert_eq!(find(None, "disccraft-no-such-tool"), None);
    }

    #[test]
    fn the_variable_wins_over_the_search() {
        let Some(ffmpeg) = ffmpeg() else {
            eprintln!("ffmpeg not found; skipping");
            return;
        };
        // A name the search finds nothing for: the variable's tool is still used.
        assert_eq!(
            find(
                Some(ffmpeg.clone().into_os_string()),
                "disccraft-no-such-tool"
            ),
            Some(ffmpeg)
        );
        // An empty variable counts as not set.
        assert!(find(Some(OsString::new()), "ffmpeg").is_some());
    }

    #[test]
    fn the_fixtures_are_the_workspaces() {
        let fixtures = fixtures();
        assert!(fixtures.ends_with("target/fixtures"));
        let manifest = std::fs::read_to_string(fixtures.join("../../Cargo.toml")).unwrap();
        assert!(manifest.contains("[workspace]"));
    }
}
