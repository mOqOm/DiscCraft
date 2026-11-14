//! M14.1 check: a chapter of a transcoded video starts its DVD cell at its own frame, not at the nearest of the
//! regular GOPs (every 15 frames): the encoder starts a closed GOP there. Read back from the built IFO when ffmpeg
//! (which makes the source video) is installed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_project::{Link, Project, Tick};
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

/// Two seconds (60 frames at 29.97) of H.264 with AAC sound: transcoded by the build.
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

/// The title's cells' lengths in frames, read from the built VTS IFO, and its program count.
fn cells_in_frames(video_ts: &Path) -> (Vec<u64>, usize) {
    let vtsi =
        disccraft_ifo::read_vtsi(&std::fs::read(video_ts.join("VTS_01_0.IFO")).unwrap()).unwrap();
    let pgc = vtsi.pgcs.first().unwrap();
    let frames = pgc
        .cells
        .iter()
        .map(|cell| {
            let ticks = cell.playback_time.to_90k().unwrap();
            (ticks + FRAME_90K / 2) / FRAME_90K
        })
        .collect();
    (frames, pgc.program_entry_cells.len())
}

#[test]
fn a_chapter_starts_its_cell_at_its_own_frame() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let video = source(&ffmpeg);
    let mut project = Project::default();
    let asset = project.add_asset(&video).unwrap();
    let timeline = project
        .add_timeline(asset, Some("Main".to_string()))
        .unwrap();
    // Frame 37: between the regular GOPs at 30 and 45 (nearest 30).
    project
        .add_chapter(timeline, Tick(37 * FRAME_TICKS), None)
        .unwrap();
    project.settings.first_play = Link::Timeline {
        id: timeline,
        chapter: 0,
    };
    let dir = std::env::temp_dir().join(format!("disccraft-chapter-frames-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let report = build(
        &project,
        &BuildOptions {
            output: OutputKind::Folder,
            path: dir.join("out"),
            cache: Some(dir.join("cache")),
            audio_language: Some("en".to_owned()),
        },
    )
    .unwrap();
    assert_eq!(report.chapters, 2);
    let (cells, programs) = cells_in_frames(&dir.join("out").join("VIDEO_TS"));
    assert_eq!(programs, 2);
    // Two seconds at 29.97 are 59 whole frames: the chapter's cell starts at frame 37 and runs to the end.
    assert_eq!(cells, vec![37, 22], "the chapter's cell starts at frame 37");
    let _ = std::fs::remove_dir_all(&dir);
}
