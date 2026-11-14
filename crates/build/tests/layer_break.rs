//! M14.2 check: a manual layer break of a dual-layer DVD (DVD9) set at a frame of a timeline gets a cell of its
//! own there (not a chapter), the image breaks at that cell's first sector, and the build reads back from the image
//! the title, cell and time of the video at the break. A break before the middle of the image (layer 0 smaller than
//! layer 1) or past the timeline's end fails the build. Runs when ffmpeg (which makes the source video) is
//! installed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, BuildReport, OutputKind, build};
use disccraft_project::{Capacity, LayerBreakPoint, Link, Project, Tick, TimelineId};
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

/// Two seconds (59 frames at 29.97) of H.264 with AAC sound: transcoded by the build (the fixture of
/// `chapter_frames.rs`).
fn source(ffmpeg: &Path) -> PathBuf {
    let video = fixtures().join("chapter_frames.mp4");
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

/// A DVD9 project of the video with chapters at frames 0 and 37 and the layer break at `frame`.
fn project(video: &Path, frame: i64) -> (Project, TimelineId) {
    let mut project = Project::default();
    let asset = project.add_asset(video).unwrap();
    let timeline = project
        .add_timeline(asset, Some("Main".to_string()))
        .unwrap();
    project
        .add_chapter(timeline, Tick(37 * FRAME_TICKS), None)
        .unwrap();
    project.settings.first_play = Link::Timeline {
        id: timeline,
        chapter: 0,
    };
    project.settings.capacity = Capacity::Dvd9;
    project.settings.layer_break = Some(LayerBreakPoint {
        timeline,
        // A little after the frame: the build takes the nearest one.
        time: Tick(frame * FRAME_TICKS + FRAME_TICKS / 4),
    });
    (project, timeline)
}

fn build_iso(project: &Project, name: &str) -> (Result<BuildReport, String>, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "disccraft-layer-break-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let result = build(
        project,
        &BuildOptions {
            output: OutputKind::Iso,
            path: dir.join("disc.iso"),
            cache: Some(fixtures().join("layer-break-cache")),
            audio_language: Some("en".to_owned()),
        },
    );
    (result, dir)
}

/// The title's cells' lengths in frames, read from the built VTS IFO, its program map and its cells' seamless
/// playback flags.
fn cells(video_ts: &Path) -> (Vec<u64>, Vec<u8>, Vec<bool>) {
    let vtsi =
        disccraft_ifo::read_vtsi(&std::fs::read(video_ts.join("VTS_01_0.IFO")).unwrap()).unwrap();
    let pgc = vtsi.pgcs.first().unwrap();
    let frames = pgc
        .cells
        .iter()
        .map(|cell| (cell.playback_time.to_90k().unwrap() + FRAME_90K / 2) / FRAME_90K)
        .collect();
    let seamless = pgc.cells.iter().map(|cell| cell.seamless).collect();
    (frames, pgc.program_entry_cells.clone(), seamless)
}

#[test]
fn a_manual_layer_break_lands_on_its_frame() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let video = source(&ffmpeg);
    let (project, timeline) = project(&video, 45);
    let (result, dir) = build_iso(&project, "frame45");
    let report = result.unwrap();
    // The break is no chapter: still two.
    assert_eq!(report.chapters, 2);
    let (lengths, programs, seamless) = cells(&dir.join("disc.files").join("VIDEO_TS"));
    assert_eq!(
        lengths,
        vec![37, 8, 14],
        "cells start at frames 0, 37 and 45"
    );
    assert_eq!(programs, vec![1, 2], "the break's cell starts no program");
    // The player may pause at the break's cell (not seamless) to change layers; the chapter's cell goes on.
    assert_eq!(seamless, vec![false, true, false]);

    let found = report.layer_break.expect("a layer break");
    assert!(found.manual);
    let place = found.place.clone().expect("the break is at a cell start");
    assert_eq!(
        (place.timeline, place.title, place.vts, place.cell),
        (timeline.0, 1, 1, 3)
    );
    // Read back from the navigation pack at the break sector in the image: frame 45 exactly.
    assert_eq!(place.time_90k, 45 * FRAME_90K);
    assert_eq!(found.requested_90k, Some(45 * FRAME_90K));
    assert_eq!(found.as_requested(), Some(true));
    assert!(
        found.sector >= found.total_sectors.div_ceil(2),
        "layer 0 ≥ layer 1"
    );
    assert_eq!(
        u64::from(found.total_sectors) * 2048,
        std::fs::metadata(dir.join("disc.iso")).unwrap().len()
    );
    let note = std::fs::read_to_string(dir.join("disc.layerbreak.txt")).unwrap();
    let wanted = [
        format!("LayerBreak={}", found.sector),
        "Mode=Manual".to_owned(),
        "Cell=3".to_owned(),
        "Frame=45".to_owned(),
        "AsRequested=yes".to_owned(),
    ];
    for line in wanted {
        assert!(note.lines().any(|l| l == line), "{line} in {note}");
    }
    oracle(&ffmpeg, &dir.join("disc.iso"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// ffmpeg's DVD navigation plays the whole title from the image (all 59 frames, through the break's cell) without
/// errors, and ffprobe sees the two chapters (the break's cell is none) at frames 0 and 37. (ffprobe ends the last
/// chapter, and the title, at the end of the chapter's first cell when it has more; it still plays them all.)
fn oracle(ffmpeg: &Path, iso: &Path) {
    let out = Command::new(ffmpeg)
        .args(["-v", "error", "-f", "dvdvideo", "-title", "1", "-i"])
        .arg(iso)
        .args(["-map", "0:v", "-f", "framemd5", "-"])
        .output()
        .unwrap();
    let frames = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.starts_with('#'))
        .count();
    assert_eq!(frames, 59, "ffmpeg plays every frame of the title");
    let out = Command::new(ffmpeg)
        .args(["-v", "error", "-f", "dvdvideo", "-title", "1", "-i"])
        .arg(iso)
        .args(["-map", "0", "-f", "null", "-"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    let errors: Vec<&str> = stderr
        .lines()
        .filter(|l| {
            !l.contains("libdvdcss")
                && !l.contains("for reading")
                && !l.contains("CSS auth")
                && !l.contains("device")
        })
        .collect();
    assert!(out.status.success() && errors.is_empty(), "{errors:?}");
    let Some(ffprobe) = std::env::var_os("DISCCRAFT_FFPROBE").map(PathBuf::from) else {
        return;
    };
    let out = Command::new(ffprobe)
        .args(["-v", "error", "-f", "dvdvideo", "-title", "1", "-i"])
        .arg(iso)
        .args(["-show_entries", "chapter=start_time", "-of", "csv=p=0"])
        .output()
        .unwrap();
    let starts: Vec<f64> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect();
    assert_eq!(starts.len(), 2, "{starts:?}");
    let frame = 1001.0 / 30000.0;
    assert!(
        starts.first().is_some_and(|s| s.abs() < frame / 2.0),
        "{starts:?}"
    );
    assert!(
        starts
            .get(1)
            .is_some_and(|s| (s - 37.0 * frame).abs() < frame / 2.0),
        "{starts:?}"
    );
}

#[test]
fn a_layer_break_before_the_middle_or_past_the_end_fails() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let video = source(&ffmpeg);
    // Frame 5 of 59: layer 0 would be far smaller than layer 1.
    let (early, dir) = build_iso(&project(&video, 5).0, "frame5");
    let error = early.expect_err("an early break fails");
    assert!(error.contains("before the middle"), "{error}");
    let _ = std::fs::remove_dir_all(&dir);
    // Frame 80: the video has 59.
    let (late, dir) = build_iso(&project(&video, 80).0, "frame80");
    let error = late.expect_err("a break past the end fails");
    assert!(error.contains("past the end"), "{error}");
    let _ = std::fs::remove_dir_all(&dir);
}
