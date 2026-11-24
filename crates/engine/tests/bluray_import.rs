//! M6.429 check: a Blu-ray DiscCraft builds is imported back as a timeline (its clip, its chapters at their frames,
//! its sound in its language), and a playlist of two play items of parts of that clip becomes two trimmed clips with
//! the chapters placed by their play items. The video and sound are made with ffmpeg; without it the test is
//! skipped.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_engine::Session;
use disccraft_project::{AudioTrack, DiscKind, Link, Project, Tick, TimelineId};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Ticks of one NTSC frame.
const FRAME: i64 = 8_475_667_200;

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

fn run(ffmpeg: &Path, args: &[&str], out: &Path) {
    let ok = Command::new(ffmpeg)
        .args(["-v", "error", "-y"])
        .args(args)
        .arg(out)
        .status()
        .unwrap()
        .success();
    assert!(ok, "ffmpeg could not make {}", out.display());
}

/// A Blu-ray (SD, MPEG-2) of 6 s of video with Japanese sound and chapters at 2 s and 4 s, built into `out`.
fn built_bluray(ffmpeg: &Path, dir: &Path) -> PathBuf {
    let video = dir.join("video.m2v");
    run(
        ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=720x480:rate=30000/1001",
            "-t",
            "6",
            "-c:v",
            "mpeg2video",
            "-b:v",
            "4M",
            "-maxrate",
            "8M",
            "-bufsize",
            "1835k",
            "-g",
            "15",
            "-bf",
            "2",
            "-pix_fmt",
            "yuv420p",
            "-aspect",
            "16:9",
            "-f",
            "mpeg2video",
        ],
        &video,
    );
    let sound = dir.join("sound.wav");
    run(
        ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            "6",
            "-ac",
            "2",
            "-c:a",
            "pcm_s16le",
        ],
        &sound,
    );
    let mut project = Project::default();
    project.settings.kind = DiscKind::BluRay;
    let asset = project.add_asset(&video).unwrap();
    let timeline = project
        .add_timeline(asset, Some("Main".to_owned()))
        .unwrap();
    let audio = project.add_asset(&sound).unwrap();
    project
        .timeline_mut(timeline)
        .unwrap()
        .audio
        .push(AudioTrack {
            language: "ja".to_owned(),
            ..AudioTrack::of(audio)
        });
    for frames in [60, 120] {
        project
            .add_chapter(timeline, Tick(frames * FRAME), None)
            .unwrap();
    }
    project.settings.first_play = Link::Timeline {
        id: timeline,
        chapter: 0,
    };
    let out = dir.join("disc");
    build(
        &project,
        &BuildOptions {
            output: OutputKind::Folder,
            path: out.clone(),
            cache: Some(dir.join("cache")),
            audio_language: Some("ja".to_owned()),
        },
    )
    .unwrap();
    out
}

/// The chapters of `timeline` in frames.
fn chapter_frames(session: &Session, timeline: u64) -> Vec<i64> {
    session
        .project
        .timeline(TimelineId(timeline))
        .unwrap()
        .chapters
        .iter()
        .map(|chapter| chapter.time.0 / FRAME)
        .collect()
}

#[test]
fn a_blu_ray_playlist_becomes_a_timeline() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-bluray-import-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let disc = built_bluray(&ffmpeg, &dir);

    let mut session = Session::new();
    let listed = session
        .execute(
            "bluray.playlists",
            &json!({ "path": disc.to_string_lossy() }),
        )
        .unwrap();
    let main = listed["main"].as_u64().unwrap();
    let playlist = listed["playlists"]
        .as_array()
        .unwrap()
        .iter()
        .find(|playlist| playlist["playlist"] == main)
        .unwrap()
        .clone();
    assert_eq!(playlist["chapters"], 3, "{listed}");
    assert_eq!(playlist["audio"][0]["language"], "jpn", "{listed}");
    assert_eq!(playlist["audio"][0]["readable"], true, "{listed}");

    // The main playlist: one clip, the chapters at the frames they were built at, the sound in Japanese.
    let imported = session
        .execute(
            "timeline.importBluray",
            &json!({ "path": disc.to_string_lossy() }),
        )
        .unwrap();
    assert_eq!(imported["clips"], 1, "{imported}");
    assert_eq!(imported["chapters"], 3, "{imported}");
    assert_eq!(imported["audioTracks"], 1, "{imported}");
    let timeline = imported["timeline"].as_u64().unwrap();
    assert_eq!(chapter_frames(&session, timeline), [0, 60, 120]);
    let found = session.project.timeline(TimelineId(timeline)).unwrap();
    assert_eq!(found.audio.len(), 1);
    assert_eq!(found.audio[0].language, "ja");
    let clips = session
        .execute("timeline.clips", &json!({ "timeline": timeline }))
        .unwrap();
    let frames = clips["duration"].as_i64().unwrap() / FRAME;
    assert!((178..=181).contains(&frames), "{clips}");

    // A playlist of two parts of the clip: 1 s to 3 s and 4 s to 6 s, with chapters at the first part's start,
    // 1 s into it, and 1 s into the second part.
    let bdmv = disc.join("BDMV");
    let main_file = bdmv.join("PLAYLIST").join(format!("{main:05}.mpls"));
    let mut parts = disccraft_bdmv::mpls::read_mpls(&std::fs::read(&main_file).unwrap()).unwrap();
    let whole = parts.items[0].clone();
    let second = 45_000_u32;
    let part = |from: u32, to: u32| disccraft_bdmv::mpls::PlayItem {
        in_time: whole.in_time + from * second,
        out_time: whole.in_time + to * second,
        ..whole.clone()
    };
    parts.items = vec![part(1, 3), part(4, 6)];
    parts.marks = vec![
        disccraft_bdmv::mpls::PlayListMark {
            play_item: 0,
            time: whole.in_time + second,
        },
        disccraft_bdmv::mpls::PlayListMark {
            play_item: 0,
            time: whole.in_time + 2 * second,
        },
        disccraft_bdmv::mpls::PlayListMark {
            play_item: 1,
            time: whole.in_time + 5 * second,
        },
    ];
    std::fs::write(
        bdmv.join("PLAYLIST").join("00099.mpls"),
        disccraft_bdmv::mpls::write_mpls(&parts).unwrap(),
    )
    .unwrap();
    let imported = session
        .execute(
            "timeline.importBluray",
            &json!({ "path": disc.to_string_lossy(), "playlist": 99 }),
        )
        .unwrap();
    assert_eq!(imported["clips"], 2, "{imported}");
    // One file, one asset.
    assert_eq!(
        imported["assets"].as_array().unwrap().len(),
        1,
        "{imported}"
    );
    let timeline = imported["timeline"].as_u64().unwrap();
    let found = session.project.timeline(TimelineId(timeline)).unwrap();
    let trims: Vec<(u32, Option<u32>)> = found
        .video_trims
        .iter()
        .map(|trim| (trim.start, trim.end))
        .collect();
    assert_eq!(trims, [(30, Some(90)), (120, Some(180))]);
    assert_eq!(chapter_frames(&session, timeline), [0, 30, 90]);
    // The sound follows the clips: the second part at 2 s, trimmed as its picture.
    let track = &found.audio[0];
    assert_eq!(
        (track.start, track.trim_start, track.trim_end),
        (0, 30, Some(90))
    );
    assert_eq!(
        track
            .more
            .iter()
            .map(|clip| (clip.start, clip.trim_start, clip.trim_end))
            .collect::<Vec<_>>(),
        [(60, 120, Some(180))]
    );
    let clips = session
        .execute("timeline.clips", &json!({ "timeline": timeline }))
        .unwrap();
    assert_eq!(clips["duration"].as_i64().unwrap() / FRAME, 120, "{clips}");

    // Not a Blu-ray, and an image that is not mounted: errors, not panics.
    let error = session
        .execute(
            "timeline.importBluray",
            &json!({ "path": dir.join("cache").to_string_lossy() }),
        )
        .unwrap_err();
    assert!(error.contains("no Blu-ray"), "{error}");
    let error = session
        .execute(
            "bluray.playlists",
            &json!({ "path": dir.join("disc.iso").to_string_lossy() }),
        )
        .unwrap_err();
    assert!(error.contains("mounted"), "{error}");
    let _ = std::fs::remove_dir_all(&dir);
}
