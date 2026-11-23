//! M6.418 check: the audio tracks of a timeline play the sound streams they name. A video with two sound streams
//! (440 Hz and 880 Hz) made into a DVD with a track for each has two AC-3 streams, each the pitch of its own stream.
//! Made and decoded by ffmpeg (the outside oracle) when it is installed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_project::{AudioTrack, Link, Project};
use std::path::{Path, PathBuf};
use std::process::Command;

fn ffmpeg() -> Option<PathBuf> {
    let path =
        std::env::var_os("DISCCRAFT_FFMPEG").map_or_else(|| PathBuf::from("ffmpeg"), PathBuf::from);
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

/// Two seconds of picture with two sound streams: 440 Hz, then 880 Hz.
fn two_streams(ffmpeg: &Path) -> PathBuf {
    let path = fixtures().join("two_sound_streams.mkv");
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
            "testsrc2=size=320x240:rate=30000/1001",
        ])
        .args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
        .args(["-f", "lavfi", "-i", "sine=frequency=880:sample_rate=48000"])
        .args(["-map", "0:v", "-map", "1:a", "-map", "2:a", "-t", "2"])
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"])
        .arg(&path)
        .status()
        .unwrap()
        .success();
    assert!(ok, "ffmpeg could not make the fixture");
    path
}

/// The pitch of sound stream `stream` of `vob`, from its zero crossings over a second.
fn pitch(ffmpeg: &Path, vob: &Path, stream: usize) -> f64 {
    let decoded = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(vob)
        .args(["-map", &format!("0:a:{stream}"), "-ss", "0.5", "-t", "1"])
        .args(["-f", "s16le", "-ac", "1", "-ar", "48000", "-"])
        .output()
        .unwrap();
    assert!(decoded.status.success());
    let samples: Vec<i16> = decoded
        .stdout
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&b| i16::from_le_bytes(b))
        .collect();
    let crossings = samples
        .windows(2)
        .filter(|pair| (pair[0] < 0) != (pair[1] < 0))
        .count();
    crossings as f64 / 2.0 / (samples.len() as f64 / 48_000.0)
}

#[test]
fn each_track_plays_its_own_sound_stream() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let video = two_streams(&ffmpeg);
    let mut project = Project::default();
    let asset = project.add_asset(&video).unwrap();
    let timeline = project
        .add_timeline(asset, Some("Main".to_string()))
        .unwrap();
    // The second stream first, then the first: each track plays the stream it names.
    for stream in [1, 0] {
        project
            .timeline_mut(timeline)
            .unwrap()
            .audio
            .push(AudioTrack {
                stream,
                ..AudioTrack::of(asset)
            });
    }
    project.settings.first_play = Link::Timeline {
        id: timeline,
        chapter: 0,
    };
    let dir = std::env::temp_dir().join(format!("disccraft-sound-streams-{}", std::process::id()));
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
    let (first, second) = (pitch(&ffmpeg, &vob, 0), pitch(&ffmpeg, &vob, 1));
    assert!(
        (first - 880.0).abs() < 5.0,
        "track 1 plays stream 2: {first} Hz"
    );
    assert!(
        (second - 440.0).abs() < 5.0,
        "track 2 plays stream 1: {second} Hz"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
