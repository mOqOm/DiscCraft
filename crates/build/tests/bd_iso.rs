//! M8.6 check: a Blu-ray project becomes a UDF 2.50 image that libbluray opens directly (its own UDF reader) and decodes
//! without errors. Skipped when ffmpeg is not available.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_project::{DiscKind, Link, Project, Tick};
use std::path::{Path, PathBuf};
use std::process::Command;

const TICKS_PER_SECOND: i64 = 254_016_000_000;
/// ffmpeg's `bluray:` input ignores playlists shorter than 180 seconds.
const SECONDS: u32 = 185;

fn tool(env: &str, name: &str) -> Option<PathBuf> {
    let path = std::env::var_os(env).map_or_else(|| PathBuf::from(name), PathBuf::from);
    Command::new(&path)
        .arg("-version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| path)
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures")
}

/// A long, DVD-compliant MPEG-2 elementary stream (passed through without transcoding).
fn long_video(ffmpeg: &Path) -> PathBuf {
    let video = fixtures().join("bd_iso.m2v");
    if !video.is_file() {
        std::fs::create_dir_all(fixtures()).unwrap();
        let seconds = SECONDS.to_string();
        let ok = Command::new(ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .args(["testsrc=size=720x480:rate=30000/1001", "-t", &seconds])
            .args(["-vf", "setdar=4/3", "-c:v", "mpeg2video", "-b:v", "800k"])
            .args([
                "-maxrate", "6M", "-bufsize", "1835k", "-g", "15", "-bf", "0",
            ])
            .args(["-pix_fmt", "yuv420p", "-f", "mpeg2video"])
            .arg(&video)
            .status()
            .unwrap()
            .success();
        assert!(ok, "ffmpeg could not create the fixture");
    }
    video
}

#[test]
fn blu_ray_iso_opens_in_libbluray() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let mut project = Project::default();
    project.settings.kind = DiscKind::BluRay;
    let v = project.add_asset(&long_video(&ffmpeg)).unwrap();
    let t = project.add_timeline(v, Some("Main".to_string())).unwrap();
    project
        .add_chapter(t, Tick(60 * TICKS_PER_SECOND), None)
        .unwrap();
    project.settings.first_play = Link::Timeline { id: t, chapter: 0 };

    let iso = fixtures().join("bd_iso.iso");
    let report = build(
        &project,
        &BuildOptions {
            output: OutputKind::Iso,
            path: iso.clone(),
            cache: None,
        },
    )
    .unwrap();
    assert_eq!(report.titles, 1);
    assert_eq!(report.total_bytes % 2048, 0);

    let input = format!("bluray:{}", iso.display()).replace('\\', "/");
    let probe = Command::new(&ffprobe)
        .args(["-v", "error", "-show_entries", "stream=codec_name"])
        .args(["-of", "compact", &input])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    assert!(
        probe.status.success(),
        "{}",
        String::from_utf8_lossy(&probe.stderr)
    );
    assert!(text.contains("codec_name=mpeg2video"), "{text}");

    let decode = Command::new(&ffmpeg)
        .args(["-v", "error", "-i", &input, "-map", "0", "-f", "null", "-"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&decode.stderr);
    assert!(decode.status.success(), "{stderr}");
    assert!(stderr.trim().is_empty(), "errors: {stderr}");
}
