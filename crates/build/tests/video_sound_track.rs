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
                more: Vec::new(),
                asset: v,
                language: String::new(),
                start: 0,
                trim_start: 0,
                trim_end: None,
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

/// M6.378: a title of two clips with sound and no audio tracks (as made before M6.361, or with its tracks taken
/// away) plays the clips' own sound on the disc too, as in the preview: one AC-3 stream, as long as the two clips.
#[test]
fn a_title_of_two_clips_without_tracks_has_their_sound() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let video = video_with_sound(&ffmpeg);
    let mut project = Project::default();
    let asset = project.add_asset(&video).unwrap();
    let timeline = project
        .add_timeline(asset, Some("Two".to_string()))
        .unwrap();
    // The second clip right after the first (two seconds of 29.97 is about 60 frames).
    let frame = 8_475_667_200_i64;
    let _ = project
        .append_timeline_video(timeline, asset, disccraft_project::Tick(60 * frame))
        .unwrap();
    project.settings.first_play = Link::Timeline {
        id: timeline,
        chapter: 0,
    };
    // The build sees a track of the two clips' sounds, the second at the join.
    let built = disccraft_dvd::with_clip_sounds(&project);
    let track = built
        .timeline(timeline)
        .and_then(|found| found.audio.first())
        .unwrap();
    let starts: Vec<u32> = track.clips().iter().map(|(_, clip)| clip.start).collect();
    // The first clip's length as the timeline counts it (its seconds at 29.97).
    let length =
        (disccraft_transcode::probe_video(&video).unwrap().seconds * 30_000.0 / 1001.0).round();
    assert_eq!(starts, [0, length as u32]);

    let dir = std::env::temp_dir().join(format!("disccraft-two-clips-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let options = BuildOptions {
        output: OutputKind::Folder,
        path: dir.join("disc"),
        cache: Some(dir.join("cache")),
        audio_language: None,
    };
    build(&project, &options).unwrap();
    let vob = dir.join("disc/VIDEO_TS/VTS_01_1.VOB");
    assert_eq!(streams(&ffprobe, &vob), ["ac3,audio", "mpeg2video,video"]);
    let duration = Command::new(&ffprobe)
        .args(["-v", "error", "-select_streams", "a:0", "-show_entries"])
        .args(["format=duration", "-of", "csv=p=0"])
        .arg(&vob)
        .output()
        .unwrap();
    let seconds: f64 = String::from_utf8_lossy(&duration.stdout)
        .trim()
        .parse()
        .unwrap_or(0.0);
    assert!((3.8..4.4).contains(&seconds), "{seconds}");
    let _ = std::fs::remove_dir_all(&dir);
}
