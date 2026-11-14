//! M6.185 check: a timeline whose Audio 1 is its video file's own sound (as a timeline made from a video with sound
//! now lays it out, like Encore) builds the same DVD title as one that plays the video's sound without a track:
//! MPEG-2 video and one AC-3 audio stream, read by ffprobe when ffmpeg is installed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_project::{AudioTrack, Link, Project};
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

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures")
}

/// Two seconds of a small H.264 picture with AAC sound in an MP4 (transcoded by the build).
fn video_with_sound(ffmpeg: &Path) -> PathBuf {
    let video = fixtures().join("sound_track.mp4");
    if video.is_file() {
        return video;
    }
    std::fs::create_dir_all(fixtures()).unwrap();
    let ok = Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=320x240:rate=30000/1001")
        .args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
        .args(["-t", "2", "-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .args(["-c:a", "aac", "-shortest"])
        .arg(&video)
        .status()
        .unwrap()
        .success();
    assert!(ok, "ffmpeg could not create the fixture");
    video
}

/// The audio and video streams of the title's VOB as "codec,type" lines (the navigation packets left out).
fn streams(ffprobe: &Path, vob: &Path) -> Vec<String> {
    let out = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name,codec_type",
        ])
        .args(["-of", "csv=p=0"])
        .arg(vob)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|line| line.trim().trim_end_matches(',').to_owned())
        .filter(|line| line.ends_with(",audio") || line.ends_with(",video"))
        .collect();
    lines.sort();
    lines
}

#[test]
fn the_videos_own_sound_as_audio_1_builds_like_the_implicit_soundtrack() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let video = video_with_sound(&ffmpeg);
    let dir = std::env::temp_dir().join(format!("disccraft-sound-track-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut built = Vec::new();
    for explicit in [false, true] {
        let mut project = Project::default();
        let v = project.add_asset(&video).unwrap();
        let t = project.add_timeline(v, Some("Main".to_string())).unwrap();
        if explicit {
            project.timeline_mut(t).unwrap().audio.push(AudioTrack {
                asset: v,
                language: String::new(),
            });
        }
        project.settings.first_play = Link::Timeline { id: t, chapter: 0 };
        let out = dir.join(if explicit { "explicit" } else { "implicit" });
        let report = build(
            &project,
            &BuildOptions {
                output: OutputKind::Folder,
                path: out.clone(),
                // One cache: the second build reuses the picture's transcode.
                cache: Some(dir.join("cache")),
                audio_language: Some("en".to_owned()),
            },
        )
        .unwrap();
        assert_eq!(report.titles, 1);
        built.push(streams(
            &ffprobe,
            &out.join("VIDEO_TS").join("VTS_01_1.VOB"),
        ));
    }
    let expected = vec!["ac3,audio".to_owned(), "mpeg2video,video".to_owned()];
    assert_eq!(built.first(), Some(&expected), "the implicit soundtrack");
    assert_eq!(
        built.get(1),
        Some(&expected),
        "the video's sound as Audio 1"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
