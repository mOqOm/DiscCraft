//! M6.246 check: the chapters a Matroska or MP4 file carries become the chapter points of a timeline made of it
//! (the files are made with ffmpeg; without it the test is skipped).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_engine::Session;
use serde_json::{Value, json};
use std::path::PathBuf;
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

#[test]
fn a_files_chapters_become_chapter_points() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
    let source = fixtures.join("h264_aac.mp4");
    if !source.is_file() {
        eprintln!("h264_aac.mp4 fixture not found; skipping");
        return;
    }
    let dir =
        std::env::temp_dir().join(format!("disccraft-source-chapters-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let metadata = dir.join("chapters.txt");
    std::fs::write(
        &metadata,
        ";FFMETADATA1\n\
         [CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=2000\ntitle=Opening\n\
         [CHAPTER]\nTIMEBASE=1/1000\nSTART=2000\nEND=4000\ntitle=Middle\n\
         [CHAPTER]\nTIMEBASE=1/1000\nSTART=4000\nEND=6000\ntitle=Ending\n",
    )
    .unwrap();
    for container in ["mkv", "mp4"] {
        let video = dir.join(format!("chapters.{container}"));
        let made = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-i"])
            .arg(&source)
            .arg("-i")
            .arg(&metadata)
            .args([
                "-map",
                "0",
                "-map_metadata",
                "1",
                "-map_chapters",
                "1",
                "-c",
                "copy",
            ])
            .arg(&video)
            .status()
            .unwrap();
        assert!(made.success(), "{container}");

        let chapters = disccraft_transcode::file_chapters(&video);
        let titles: Vec<&str> = chapters.iter().map(|(_, title)| title.as_str()).collect();
        assert_eq!(titles, ["Opening", "Middle", "Ending"], "{container}");
        assert!(
            (chapters[1].0 - 2.0).abs() < 0.01,
            "{container}: {chapters:?}"
        );

        // A timeline made of the file has them: the first names the chapter at the start.
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
        let found = session
            .project
            .timeline(disccraft_project::TimelineId(timeline))
            .unwrap();
        let names: Vec<&str> = found
            .chapters
            .iter()
            .map(|chapter| chapter.name.as_str())
            .collect();
        assert_eq!(names, ["Opening", "Middle", "Ending"], "{container}");
        let seconds: Vec<f64> = found
            .chapters
            .iter()
            .map(|chapter| chapter.time.0 as f64 / 254_016_000_000.0)
            .collect();
        assert!(seconds[0].abs() < 1e-9, "{container}");
        assert!((seconds[1] - 2.0).abs() < 0.05, "{container}: {seconds:?}");
        assert!((seconds[2] - 4.0).abs() < 0.05, "{container}: {seconds:?}");
        // Asked again, nothing is added twice.
        let again = session
            .execute(
                "timeline.importSourceChapters",
                &json!({"timeline": timeline}),
            )
            .unwrap();
        assert_eq!(
            again.get("added"),
            Some(&Value::from(0)),
            "{container}: {again}"
        );
        // M6.274: with "replace" the chapter points there are go first: one put in by hand at 1 s is gone,
        // and the source's three are back.
        session
            .execute(
                "timeline.addChapter",
                &json!({"timeline": timeline, "time": 254_016_000_000_i64}),
            )
            .unwrap();
        let replaced = session
            .execute(
                "timeline.importSourceChapters",
                &json!({"timeline": timeline, "replace": true}),
            )
            .unwrap();
        assert_eq!(replaced.get("added"), Some(&Value::from(2)), "{container}");
        let found = session.project.timelines.values().next().unwrap();
        let names: Vec<&str> = found
            .chapters
            .iter()
            .map(|chapter| chapter.name.as_str())
            .collect();
        assert_eq!(names.len(), 3, "{container}: {names:?}");
        assert_eq!(names[1..], ["Middle", "Ending"], "{container}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
