//! M6.323: video that goes on a DVD as it is says the title's shape. A 4:3 MPEG-2 file on a 16:9 title (the
//! timeline's choice, or what Interpret Footage makes a new timeline) goes on with its sequence headers saying
//! 16:9, the file itself untouched; on a 4:3 title it goes on as it is.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_project::{Link, Project};
use std::path::{Path, PathBuf};
use std::process::Command;

fn tool(env: &str, name: &str) -> Option<PathBuf> {
    let path = std::env::var_os(env).map_or_else(|| PathBuf::from(name), PathBuf::from);
    Command::new(&path)
        .arg("-version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| path)
}

/// The display aspect ratio of the first video stream of `file`, as ffprobe reads it.
fn display_aspect(ffprobe: &Path, file: &Path) -> String {
    let output = Command::new(ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries"])
        .args(["stream=display_aspect_ratio", "-of", "csv=p=0"])
        .arg(file)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_end_matches(',')
        .to_owned()
}

#[test]
fn passed_through_video_says_the_titles_shape() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-aspect-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Two seconds of DVD-compliant 4:3 MPEG-2.
    let video = dir.join("square.m2v");
    let made = Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=720x480:rate=30000/1001")
        .args([
            "-t",
            "2",
            "-c:v",
            "mpeg2video",
            "-b:v",
            "6M",
            "-maxrate",
            "9M",
        ])
        .args([
            "-bufsize", "1835k", "-g", "15", "-pix_fmt", "yuv420p", "-aspect", "4:3",
        ])
        .args(["-f", "mpeg2video"])
        .arg(&video)
        .status();
    assert!(made.is_ok_and(|status| status.success()));
    let before = std::fs::read(&video).unwrap();
    assert_eq!(display_aspect(&ffprobe, &video), "4:3");

    let mut project = Project::default();
    let asset = project.add_asset(&video).unwrap();
    let timeline = project
        .add_timeline(asset, Some("Main".to_string()))
        .unwrap();
    project.settings.first_play = Link::Timeline {
        id: timeline,
        chapter: 0,
    };
    let built = |project: &Project, name: &str| {
        let out = dir.join(name);
        let _ = build(
            project,
            &BuildOptions {
                output: OutputKind::Folder,
                path: out.clone(),
                cache: Some(dir.join("cache")),
                audio_language: None,
            },
        )
        .unwrap();
        out.join("VIDEO_TS/VTS_01_1.VOB")
    };
    let set_wide = |project: &mut Project, wide: bool| {
        if let Some(found) = project.timelines.get_mut(&timeline) {
            found.widescreen = Some(wide);
        }
    };

    set_wide(&mut project, false);
    assert_eq!(display_aspect(&ffprobe, &built(&project, "narrow")), "4:3");
    set_wide(&mut project, true);
    let wide = built(&project, "wide");
    assert_eq!(display_aspect(&ffprobe, &wide), "16:9");
    // Only the shape changed: it decodes as before, and the file itself is as it was.
    let decoded = Command::new(&ffmpeg)
        .args(["-v", "error", "-xerror", "-i"])
        .arg(&wide)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decoded.status.success() && decoded.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    assert_eq!(std::fs::read(&video).unwrap(), before);
    let _ = std::fs::remove_dir_all(&dir);
}
