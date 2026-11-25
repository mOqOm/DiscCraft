//! M6.409 check: a clip that needs a transcode on a DVD can be trimmed to any frame and can have a gap in front of
//! it. The build transcodes the clip's whole file, with closed GOPs starting on the trim's frames, and cuts the trim
//! out of it as it cuts an MPEG-2 clip that is used as it is. Read back from the built IFO, and decoded by ffmpeg,
//! when ffmpeg (which makes the source video) is installed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_project::{ClipTrim, Link, Project, Tick, TimelineId};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Ticks of one NTSC frame (1001/30000 s).
const FRAME_TICKS: i64 = 8_475_667_200;
/// 90 kHz ticks of one NTSC frame.
const FRAME_90K: u64 = 3_003;

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

/// Four seconds (120 frames at 29.97) of H.264 with AAC sound: transcoded by a DVD build.
fn source(ffmpeg: &Path) -> PathBuf {
    let video = fixtures().join("transcoded_clips.mp4");
    if video.is_file() {
        return video;
    }
    std::fs::create_dir_all(fixtures()).unwrap();
    let ok = Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=320x240:rate=30000/1001")
        .args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
        .args(["-t", "4", "-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .args(["-c:a", "aac", "-shortest"])
        .arg(&video)
        .status()
        .unwrap()
        .success();
    assert!(ok, "ffmpeg could not create the fixture");
    video
}

/// A DVD project of one timeline of `video`, played first.
fn project_of(video: &Path) -> (Project, TimelineId) {
    let mut project = Project::default();
    let asset = project.add_asset(video).unwrap();
    let timeline = project
        .add_timeline(asset, Some("Main".to_string()))
        .unwrap();
    project.settings.first_play = Link::Timeline {
        id: timeline,
        chapter: 0,
    };
    (project, timeline)
}

/// Builds `project` into a folder under the temporary directory named after `name`; the folder's VIDEO_TS.
fn build_folder(project: &Project, name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("disccraft-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = build(
        project,
        &BuildOptions {
            output: OutputKind::Folder,
            path: dir.join("out"),
            cache: Some(dir.join("cache")),
            audio_language: Some("en".to_owned()),
        },
    )
    .unwrap();
    dir.join("out").join("VIDEO_TS")
}

/// The title's cells' lengths in frames, read from the built VTS IFO.
fn cells_in_frames(video_ts: &Path) -> Vec<u64> {
    let vtsi =
        disccraft_ifo::read_vtsi(&std::fs::read(video_ts.join("VTS_01_0.IFO")).unwrap()).unwrap();
    let pgc = vtsi.pgcs.first().unwrap();
    pgc.cells
        .iter()
        .map(|cell| {
            let ticks = cell.playback_time.to_90k().unwrap();
            (ticks + FRAME_90K / 2) / FRAME_90K
        })
        .collect()
}

/// ffmpeg decodes the title set's video without an error.
fn decodes(ffmpeg: &Path, video_ts: &Path) {
    let decoded = Command::new(ffmpeg)
        .args(["-v", "error", "-xerror", "-i"])
        .arg(video_ts.join("VTS_01_1.VOB"))
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decoded.status.success() && decoded.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
}

/// A gap in front of a clip that needs a transcode plays black, then the clip. Before M6.409 the clip was made as a
/// timeline of its own that kept the gaps, so it was joined again and again until the stack ran out (the app closed).
#[test]
fn a_gap_in_front_of_a_clip_that_needs_a_transcode_plays_black() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let video = source(&ffmpeg);
    let (mut project, timeline) = project_of(&video);
    project.timelines.get_mut(&timeline).unwrap().video_gaps = vec![15];
    let video_ts = build_folder(&project, "transcoded-gap");
    let frames: u64 = cells_in_frames(&video_ts).iter().sum();
    // 15 frames of black and four seconds at 29.97 (119 or 120 frames).
    assert!((134..=135).contains(&frames), "{frames} frames");
    decodes(&ffmpeg, &video_ts);
    let _ = std::fs::remove_dir_all(video_ts.parent().unwrap().parent().unwrap());
}

/// A clip that needs a transcode, trimmed to frames 37 to 100 of its file, plays those 63 frames, and a chapter in
/// it starts its cell at its own frame.
#[test]
fn a_clip_that_needs_a_transcode_plays_the_frames_it_is_trimmed_to() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let video = source(&ffmpeg);
    let (mut project, timeline) = project_of(&video);
    project.timelines.get_mut(&timeline).unwrap().video_trims = vec![ClipTrim {
        start: 37,
        end: Some(100),
    }];
    // Frame 20 of the title: frame 57 of the file, between the regular GOPs.
    project
        .add_chapter(timeline, Tick(20 * FRAME_TICKS), None)
        .unwrap();
    let video_ts = build_folder(&project, "transcoded-trim");
    assert_eq!(
        cells_in_frames(&video_ts),
        vec![20, 43],
        "the trimmed clip's 63 frames, its chapter's cell at frame 20"
    );
    decodes(&ffmpeg, &video_ts);
    // The clip's own sound comes with it (its trimmed part: a joined title brings none of its own).
    let streams = Command::new(&ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(video_ts.join("VTS_01_1.VOB"))
        .output()
        .unwrap();
    let streams = String::from_utf8_lossy(&streams.stderr);
    assert!(streams.contains("Audio: ac3"), "{streams}");
    let _ = std::fs::remove_dir_all(video_ts.parent().unwrap().parent().unwrap());
}

/// M6.493: a chapter in the second clip of a timeline (a Blu-ray's playlist imported is a timeline of several clips)
/// starts its cell at its own frame too, as one in the first clip does. Before, only the first clip's transcode got
/// GOPs at the chapters, and a later clip's chapter fell on the nearest regular GOP.
#[test]
fn a_chapter_in_a_later_clip_starts_its_cell_at_its_own_frame() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let video = source(&ffmpeg);
    let (mut project, timeline) = project_of(&video);
    let found = project.timelines.get_mut(&timeline).unwrap();
    // The first clip: frames 0 to 60 of the file; the second: the whole file again.
    found.more_video = vec![found.video];
    found.video_trims = vec![
        ClipTrim {
            start: 0,
            end: Some(60),
        },
        ClipTrim::default(),
    ];
    // Frame 97 of the title: frame 37 of the second clip's file, between the regular GOPs at 30 and 45.
    project
        .add_chapter(timeline, Tick(97 * FRAME_TICKS), None)
        .unwrap();
    let video_ts = build_folder(&project, "transcoded-later-chapter");
    let cells = cells_in_frames(&video_ts);
    assert_eq!(cells.len(), 2, "{cells:?}");
    assert_eq!(
        cells[0], 97,
        "the chapter's cell starts at frame 97: {cells:?}"
    );
    // 60 frames and four seconds at 29.97 (119 or 120 frames).
    assert!(
        (179..=180).contains(&cells.iter().sum::<u64>()),
        "{cells:?}"
    );
    decodes(&ffmpeg, &video_ts);
    let _ = std::fs::remove_dir_all(video_ts.parent().unwrap().parent().unwrap());
}
