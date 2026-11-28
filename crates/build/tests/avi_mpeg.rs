//! M6.417 check: MPEG-2 video in an AVI (with B pictures, as capture cards and converters wrote it) makes a DVD whose
//! pictures are the source's: its chunks are read as one elementary stream, its sound from the AVI. Made and compared
//! by ffmpeg (the outside oracle) when it is installed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_project::{Link, Project};
use disccraft_test_support::{ffmpeg, fixtures};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Three seconds of moving test picture as 4:3 MPEG-2 (I B B P …) in an AVI, with a PCM tone.
fn mpeg2_avi(ffmpeg: &Path) -> PathBuf {
    let path = fixtures().join("mpeg2_in.avi");
    if path.is_file() {
        return path;
    }
    std::fs::create_dir_all(fixtures()).unwrap();
    let ok = Command::new(ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=720x480:rate=30000/1001",
        ])
        .args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
        .args([
            "-t",
            "3",
            "-c:v",
            "mpeg2video",
            "-b:v",
            "6M",
            "-bf",
            "2",
            "-g",
            "15",
            "-aspect",
            "4:3",
        ])
        .args(["-c:a", "pcm_s16le"])
        .arg(&path)
        .status()
        .unwrap()
        .success();
    assert!(ok, "ffmpeg could not make the fixture");
    path
}

#[test]
fn mpeg2_in_an_avi_makes_a_dvd_of_its_pictures() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let video = mpeg2_avi(&ffmpeg);
    let mut project = Project::default();
    let asset = project.add_asset(&video).unwrap();
    let timeline = project
        .add_timeline(asset, Some("Main".to_string()))
        .unwrap();
    project.settings.first_play = Link::Timeline {
        id: timeline,
        chapter: 0,
    };
    let dir = std::env::temp_dir().join(format!("disccraft-avi-mpeg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = build(
        &project,
        &BuildOptions {
            output: OutputKind::Folder,
            path: dir.join("out"),
            cache: Some(dir.join("cache")),
            audio_language: Some("en".to_owned()),
        },
    )
    .unwrap();
    let vob = dir.join("out").join("VIDEO_TS").join("VTS_01_1.VOB");
    // The disc's pictures against the source's, frame by frame.
    let compared = Command::new(&ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(&vob)
        .arg("-i")
        .arg(&video)
        .args([
            "-filter_complex",
            "[0:v]setpts=PTS-STARTPTS,format=yuv420p[a];[1:v]setpts=PTS-STARTPTS,format=yuv420p[b];[a][b]psnr",
            "-f",
            "null",
            "-",
        ])
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&compared.stderr);
    let line = log
        .lines()
        .rfind(|line| line.contains("PSNR y:"))
        .unwrap_or_default();
    let low = line
        .split("min:")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0);
    assert!(
        low > 30.0,
        "the worst frame is {low} dB from the source's: {line}"
    );
    let streams = Command::new(&ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(&vob)
        .output()
        .unwrap();
    let streams = String::from_utf8_lossy(&streams.stderr);
    assert!(streams.contains("Audio: ac3"), "{streams}");
    let _ = std::fs::remove_dir_all(&dir);
}
