//! M6.429 check: a Blu-ray DiscCraft builds is imported back as a timeline (its clip, its chapters at their frames,
//! its sound in its language), and a playlist of two play items of parts of that clip becomes two trimmed clips with
//! the chapters placed by their play items. M6.491: in a Blu-ray project its subtitles come back as a subtitle
//! track, at their times on the timeline (a subtitle the cut leaves showing carried to the part's start and cleared
//! at its end), and ffmpeg (the outside oracle) decodes them. The video and sound are made with ffmpeg; without it
//! the test is skipped.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_engine::Session;
use disccraft_project::{AudioTrack, DiscKind, Link, Project, SubtitleTrack, Tick, TimelineId};
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

/// A Blu-ray (SD, MPEG-2) of 6 s of video with Japanese sound and subtitles (0.5 to 1.5 s, 2.5 to 3.5 s and 4.5 to
/// 5.5 s) and chapters at 2 s and 4 s, built into `out`.
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
    let srt = dir.join("ja.srt");
    std::fs::write(
        &srt,
        "1\n00:00:00,500 --> 00:00:01,500\nOne\n\n2\n00:00:02,500 --> 00:00:03,500\nTwo\n\n\
         3\n00:00:04,500 --> 00:00:05,500\nThree\n\n",
    )
    .unwrap();
    let subtitles = project.add_asset(&srt).unwrap();
    project
        .timeline_mut(timeline)
        .unwrap()
        .subtitles
        .push(SubtitleTrack {
            asset: subtitles,
            language: "ja".to_owned(),
            group: 0,
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

/// The display sets of the `.sup` `path`: each PCS's time in milliseconds, and whether it shows something.
fn display_sets(path: &Path) -> Vec<(i64, bool)> {
    let sup = std::fs::read(path).unwrap();
    let mut found = Vec::new();
    let mut at = 0;
    while at + 13 <= sup.len() {
        assert_eq!(&sup[at..at + 2], b"PG");
        let pts = i64::from(u32::from_be_bytes(sup[at + 2..at + 6].try_into().unwrap()));
        let length = usize::from(u16::from_be_bytes([sup[at + 11], sup[at + 12]]));
        if sup[at + 10] == 0x16 {
            found.push(((pts + 45) / 90, sup[at + 13 + 10] > 0));
        }
        at += 13 + length;
    }
    assert_eq!(at, sup.len());
    found
}

/// The display sets of the `.sup` that subtitle track `track` of `timeline` is, within a frame of the times given.
fn assert_sets(session: &Session, timeline: u64, expected: &[(i64, bool)]) {
    let found = session.project.timeline(TimelineId(timeline)).unwrap();
    let asset = &session.project.assets[&found.subtitles[0].asset];
    let sets = display_sets(Path::new(&asset.path));
    assert_eq!(sets.len(), expected.len(), "{sets:?}");
    for (set, wanted) in sets.iter().zip(expected) {
        assert!(
            (set.0 - wanted.0).abs() <= 34 && set.1 == wanted.1,
            "{sets:?} against {expected:?}"
        );
    }
}

/// ffmpeg decodes the subtitles of `sup` (drawing them to re-encode them as a DVD's) without a complaint.
fn assert_decodes(ffmpeg: &Path, sup: &Path) {
    let decode = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(sup)
        .args(["-map", "0:s", "-c:s", "dvdsub", "-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decode.status.success() && decode.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&decode.stderr)
    );
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

/// M6.491: in a Blu-ray project, the subtitles of the disc built (and of its playlist 99 of two parts) come in as
/// subtitle tracks in their language, at their times.
fn subtitles_come_in(ffmpeg: &Path, disc: &Path) {
    // M6.491: in a Blu-ray project, the subtitles come in as a subtitle track in their language, at their times.
    let mut session = Session::new();
    session.project.settings.kind = DiscKind::BluRay;
    // The subtitles taken out go beside the project (`<dir>/bluray.subtitles`), not into the shared temporary folder.
    session.path = Some(disc.with_file_name("bluray.dcproj"));
    let _ = std::fs::remove_dir_all(disc.with_file_name("bluray.subtitles"));
    let imported = session
        .execute(
            "timeline.importBluray",
            &json!({ "path": disc.to_string_lossy() }),
        )
        .unwrap();
    assert_eq!(imported["subtitleTracks"], 1, "{imported}");
    assert_eq!(imported["subtitlesLeftOut"], json!([]), "{imported}");
    let timeline = imported["timeline"].as_u64().unwrap();
    let found = session.project.timeline(TimelineId(timeline)).unwrap();
    assert_eq!(found.subtitles[0].language, "ja");
    assert_sets(
        &session,
        timeline,
        &[
            (500, true),
            (1_500, false),
            (2_500, true),
            (3_500, false),
            (4_500, true),
            (5_500, false),
        ],
    );
    let asset = &session.project.assets[&found.subtitles[0].asset];
    assert_decodes(ffmpeg, Path::new(&asset.path));
    // The window takes them out later, on a thread of its own: the import gives the job, and adding it after
    // taking it out lays the track. The parts played are 1 s to 3 s and 4 s to 6 s: the first subtitle shows at the
    // first part's start (carried there), the second still shows at its end (cleared there). The folder beside the
    // project is gone, as before a project's first subtitles: the job makes it.
    std::fs::remove_dir_all(disc.with_file_name("bluray.subtitles")).unwrap();
    let imported = session
        .execute(
            "timeline.importBluray",
            &json!({ "path": disc.to_string_lossy(), "playlist": 99, "subtitlesLater": true }),
        )
        .unwrap();
    assert_eq!(imported["subtitleTracks"], 0, "{imported}");
    let jobs = imported["subtitleJobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 1, "{imported}");
    let timeline = imported["timeline"].as_u64().unwrap();
    assert!(
        session
            .project
            .timeline(TimelineId(timeline))
            .unwrap()
            .subtitles
            .is_empty()
    );
    disccraft_engine::take_bluray_subtitle(&jobs[0], &|| true).unwrap();
    let added = session
        .execute("timeline.addBluraySubtitle", &json!({ "job": jobs[0] }))
        .unwrap();
    assert_eq!(added["track"], 0, "{added}");
    assert_sets(
        &session,
        timeline,
        &[
            (0, true),
            (500, false),
            (1_500, true),
            (2_000, false),
            (2_500, true),
            (3_500, false),
        ],
    );
    let found = session.project.timeline(TimelineId(timeline)).unwrap();
    let asset = &session.project.assets[&found.subtitles[0].asset];
    assert_decodes(ffmpeg, Path::new(&asset.path));
    // A job stopped as it starts writes nothing.
    assert!(disccraft_engine::take_bluray_subtitle(&jobs[0], &|| false).is_err());
    assert!(disccraft_engine::take_bluray_subtitle(&json!({ "pid": 1 }), &|| true).is_err());
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
    // A DVD takes no PGS (yet): the subtitles are left out.
    assert_eq!(imported["subtitleTracks"], 0, "{imported}");
    assert_eq!(
        imported["subtitlesLeftOut"],
        json!(["jpn PG"]),
        "{imported}"
    );
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

    subtitles_come_in(&ffmpeg, &disc);

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
