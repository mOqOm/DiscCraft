//! M6.422 check: the subtitle tracks inside an MKV (SubRip and ASS here) become a new timeline's subtitle tracks, each
//! taken out as a `.srt` beside the project, in the language the file names, with the file's times and text (ASS
//! override tags taken out). The file is made with ffmpeg; without it the test is skipped.

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

const JAPANESE: &str = "1\n00:00:00,500 --> 00:00:01,500\nこんにちは\n\n2\n00:00:02,000 --> 00:00:03,250\n二行目の\n字幕です\n\n";
const ENGLISH: &str = "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 480\n\n\
[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, \
Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, \
MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,\
1,2,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:00.75,0:00:01.75,Default,,0,0,0,,{\\i1}Hello{\\i0}, world\n\
Dialogue: 0,0:00:02.50,0:00:03.50,Default,,0,0,0,,Line one\\NLine two\n";

fn cues(session: &mut Session, asset: u64) -> Vec<(i64, i64, String)> {
    let value = session
        .execute("subtitle.cues", &json!({ "asset": asset }))
        .unwrap();
    value["cues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cue| {
            // `as` is exact enough for milliseconds.
            let ms = |key: &str| (cue[key].as_f64().unwrap() * 1000.0).round() as i64;
            let text = match &cue["text"] {
                Value::Array(lines) => lines
                    .iter()
                    .map(|line| line.as_str().unwrap().to_owned())
                    .collect::<Vec<_>>()
                    .join("|"),
                other => other.as_str().unwrap_or_default().replace('\n', "|"),
            };
            (ms("start"), ms("end"), text)
        })
        .collect()
}

#[test]
fn an_mkvs_subtitle_tracks_become_the_timelines() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-embedded-subs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ja.srt"), JAPANESE).unwrap();
    std::fs::write(dir.join("en.ass"), ENGLISH).unwrap();
    let video = dir.join("subtitled.mkv");
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
        .arg("-i")
        .arg(dir.join("ja.srt"))
        .arg("-i")
        .arg(dir.join("en.ass"))
        .args([
            "-map", "0:v", "-map", "1:a", "-map", "2:s", "-map", "3:s", "-t", "4",
        ])
        .args([
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-c:s", "copy",
        ])
        .args([
            "-metadata:s:s:0",
            "language=jpn",
            "-metadata:s:s:1",
            "language=eng",
        ])
        .arg(&video)
        .status()
        .unwrap();
    assert!(made.success());

    let mut session = Session::new();
    session.path = Some(dir.join("project.dcproj"));
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
    let tracks = session
        .project
        .timeline(disccraft_project::TimelineId(timeline))
        .unwrap()
        .subtitles
        .clone();
    let languages: Vec<&str> = tracks.iter().map(|track| track.language.as_str()).collect();
    assert_eq!(languages, ["ja", "en"]);
    // Each taken out beside the project.
    for track in &tracks {
        let asset = &session.project.assets[&track.asset];
        assert_eq!(asset.kind, disccraft_project::AssetKind::Subtitle);
        assert!(
            std::path::Path::new(&asset.path).starts_with(dir.join("project.subtitles")),
            "{}",
            asset.path
        );
    }
    assert_eq!(
        cues(&mut session, tracks[0].asset.0),
        [
            (500, 1500, "こんにちは".to_owned()),
            (2000, 3250, "二行目の|字幕です".to_owned()),
        ]
    );
    assert_eq!(
        cues(&mut session, tracks[1].asset.0),
        [
            (750, 1750, "Hello, world".to_owned()),
            (2500, 3500, "Line one|Line two".to_owned()),
        ]
    );
    // The probe lists them; the command adds them again beside the tracks there are.
    let probe = session
        .execute("asset.probe", &json!({"asset": asset}))
        .unwrap();
    assert_eq!(
        probe["subtitleStreams"],
        json!([
            {"format": "text", "language": "ja", "name": "", "forced": false},
            {"format": "text", "language": "en", "name": "", "forced": false},
        ]),
        "{probe}"
    );
    let imported = session
        .execute(
            "timeline.importSourceSubtitles",
            &json!({"timeline": timeline}),
        )
        .unwrap();
    assert_eq!(imported, json!({"added": 2, "skipped": 0}));
    let count = session
        .project
        .timeline(disccraft_project::TimelineId(timeline))
        .unwrap()
        .subtitles
        .len();
    assert_eq!(count, 4);
    let _ = std::fs::remove_dir_all(&dir);
}
