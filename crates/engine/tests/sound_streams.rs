//! M6.418 check: a video file with several sound streams (an MKV with two languages) gives a timeline made of it an
//! audio track for each stream, a track's stream can be changed to another the file has and not to one it lacks,
//! and `asset.probe` counts them. The file is made with ffmpeg; without it the test is skipped.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_engine::Session;
use serde_json::json;
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
fn each_sound_stream_of_a_file_becomes_a_track() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-sound-streams-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let video = dir.join("two_languages.mkv");
    let made = Command::new(&ffmpeg)
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
        .args([
            "-metadata:s:a:0",
            "language=jpn",
            "-metadata:s:a:1",
            "language=eng",
        ])
        .arg(&video)
        .status()
        .unwrap();
    assert!(made.success());

    let mut session = Session::new();
    let asset = session
        .execute("asset.import", &json!({"path": video.to_string_lossy()}))
        .unwrap()["asset"]
        .as_u64()
        .unwrap();
    let probe = session
        .execute("asset.probe", &json!({"asset": asset}))
        .unwrap();
    assert_eq!(probe["audioStreams"], 2, "{probe}");
    // M6.421: the languages the file names for its streams.
    assert_eq!(probe["audioLanguages"], json!(["ja", "en"]), "{probe}");
    let timeline = session
        .execute("timeline.create", &json!({"asset": asset}))
        .unwrap()["timeline"]
        .as_u64()
        .unwrap();
    let tracks = &session
        .project
        .timeline(disccraft_project::TimelineId(timeline))
        .unwrap()
        .audio;
    let streams: Vec<u32> = tracks.iter().map(|track| track.stream).collect();
    assert_eq!(streams, [0, 1], "a track for each stream");
    let languages: Vec<&str> = tracks.iter().map(|track| track.language.as_str()).collect();
    assert_eq!(
        languages,
        ["ja", "en"],
        "each track in its stream's language"
    );

    // The first track can play the second stream; there is no third.
    session
        .execute(
            "timeline.setAudioStream",
            &json!({"timeline": timeline, "track": 0, "stream": 1}),
        )
        .unwrap();
    let error = session
        .execute(
            "timeline.setAudioStream",
            &json!({"timeline": timeline, "track": 0, "stream": 2}),
        )
        .unwrap_err();
    assert!(error.contains("no stream 2"), "{error}");
    let added = session
        .execute(
            "timeline.addAudioTrack",
            &json!({"timeline": timeline, "asset": asset, "stream": 1}),
        )
        .unwrap()["track"]
        .as_u64()
        .unwrap();
    let tracks = &session
        .project
        .timeline(disccraft_project::TimelineId(timeline))
        .unwrap()
        .audio;
    assert_eq!(tracks[0].stream, 1);
    assert_eq!(tracks[added as usize].stream, 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// M6.421: an MP4's sound tracks name their languages (`mdhd`) too; a file that names none (a Matroska file's default
/// English on every stream) gives the tracks none, so they take the disc's own.
#[test]
fn a_files_stream_languages_become_its_tracks_languages() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir =
        std::env::temp_dir().join(format!("disccraft-stream-languages-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let make = |name: &str, tags: &[&str]| {
        let path = dir.join(name);
        let made = Command::new(&ffmpeg)
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
            .args(["-map", "0:v", "-map", "1:a", "-map", "2:a", "-t", "1"])
            .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"])
            .args(tags)
            .arg(&path)
            .status()
            .unwrap();
        assert!(made.success());
        path
    };
    let languages_of = |path: &std::path::Path| -> Vec<String> {
        let mut session = Session::new();
        let asset = session
            .execute("asset.import", &json!({"path": path.to_string_lossy()}))
            .unwrap()["asset"]
            .as_u64()
            .unwrap();
        let timeline = session
            .execute("timeline.create", &json!({"asset": asset}))
            .unwrap()["timeline"]
            .as_u64()
            .unwrap();
        session
            .project
            .timeline(disccraft_project::TimelineId(timeline))
            .unwrap()
            .audio
            .iter()
            .map(|track| track.language.clone())
            .collect()
    };
    let mp4 = make(
        "two_languages.mp4",
        &[
            "-metadata:s:a:0",
            "language=fra",
            "-metadata:s:a:1",
            "language=tha",
        ],
    );
    assert_eq!(languages_of(&mp4), ["fr", "th"]);
    // M6.423: a transport stream's PMT names them too.
    let ts = make(
        "two_languages.ts",
        &[
            "-metadata:s:a:0",
            "language=spa",
            "-metadata:s:a:1",
            "language=kor",
        ],
    );
    assert_eq!(languages_of(&ts), ["es", "ko"]);
    let untagged = make("untagged.mkv", &[]);
    assert_eq!(
        languages_of(&untagged),
        ["", ""],
        "no language named: the disc's own"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
