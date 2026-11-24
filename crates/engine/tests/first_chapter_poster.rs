//! M6.439 check: a timeline made of a video that starts black gets Encore's first-chapter poster frame, at the first
//! half second that shows something; one that does not start black gets none (the files are made with ffmpeg;
//! without it the test is skipped).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_engine::Session;
use disccraft_project::{FrameRate, TimelineId};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;

fn ffmpeg() -> Option<PathBuf> {
    let path =
        std::env::var_os("DISCCRAFT_FFMPEG").map_or_else(|| PathBuf::from("ffmpeg"), PathBuf::from);
    Command::new(&path)
        .arg("-version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|_| path)
}

/// An NTSC MPEG-2 file of `filter` (an ffmpeg filter graph making the picture), three seconds long.
fn make(ffmpeg: &Path, filter: &str, out: &Path) {
    let made = Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", filter])
        .args([
            "-t",
            "3",
            "-r",
            "30000/1001",
            "-c:v",
            "mpeg2video",
            "-q:v",
            "4",
        ])
        .arg(out)
        .status()
        .unwrap();
    assert!(made.success(), "{filter}");
}

/// The first chapter's poster of a timeline made of `video`, as a frame number at 29.97.
fn poster_frame(video: &Path) -> Option<i64> {
    let mut session = Session::new();
    let asset = session
        .execute("asset.import", &json!({"path": video.to_string_lossy()}))
        .unwrap()["asset"]
        .as_u64()
        .unwrap();
    let timeline = session
        .execute("timeline.create", &json!({"asset": asset}))
        .unwrap()["timeline"]
        .as_u64()
        .unwrap();
    let poster = session
        .project
        .timeline(TimelineId(timeline))
        .unwrap()
        .chapters
        .first()
        .unwrap()
        .poster;
    poster.map(|tick| FrameRate::FPS_29_97.frame_at(tick))
}

#[test]
fn a_timeline_that_starts_black_gets_encores_poster() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-poster-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // Black for 1.2 s, then a white title on black: the poster is at 1.5 s (frame 45, Encore's 1;15).
    let fade = dir.join("fade.mpg");
    make(
        &ffmpeg,
        "color=c=black:s=720x480:r=30000/1001,drawbox=x=200:y=200:w=320:h=60:c=white:t=fill:enable='gte(t,1.2)'",
        &fade,
    );
    assert_eq!(poster_frame(&fade), Some(45));

    // Grey from the start: no poster (Encore: 00;00;00;00).
    let grey = dir.join("grey.mpg");
    make(
        &ffmpeg,
        "color=c=gray:s=720x480:r=30000/1001,drawbox=x=200:y=200:w=320:h=60:c=white:t=fill",
        &grey,
    );
    assert_eq!(poster_frame(&grey), None);

    // Black all through: none either.
    let black = dir.join("black.mpg");
    make(&ffmpeg, "color=c=black:s=720x480:r=30000/1001", &black);
    assert_eq!(poster_frame(&black), None);

    let _ = std::fs::remove_dir_all(&dir);
}
