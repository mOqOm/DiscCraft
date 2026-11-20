//! A DVD built as a DDP image (Encore's build output "DDP Image"): a folder named after the disc holding the image
//! as MAIN.DAT (padded with zeros to a whole ECC block), CONTROL.DAT and DDPID, as Encore CS6 writes them from the
//! same image (`disccraft_ddp`), with the VIDEO_TS folder built beside it. The image in MAIN.DAT plays in ffmpeg's
//! DVD reader from the first frame to the last. A Blu-ray gets no DDP image (Encore makes none). Runs when ffmpeg
//! (which makes the source video and plays the image) is installed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_ddp::{Layout, control_data, ddp_id};
use disccraft_project::{DiscKind, Link, Project};
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

/// Two seconds (59 frames at 29.97) of H.264 with AAC sound: transcoded by the build (the fixture of
/// `chapter_frames.rs` and `layer_break.rs`).
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

fn project(video: &Path) -> Project {
    let mut project = Project::default();
    let asset = project.add_asset(video).unwrap();
    let timeline = project
        .add_timeline(asset, Some("Main".to_string()))
        .unwrap();
    project.settings.first_play = Link::Timeline {
        id: timeline,
        chapter: 0,
    };
    project
}

fn options(dir: &Path) -> BuildOptions {
    BuildOptions {
        output: OutputKind::Ddp,
        path: dir.join("Movie"),
        cache: Some(fixtures().join("ddp-cache")),
        audio_language: Some("en".to_owned()),
    }
}

#[test]
fn a_dvd_ddp_image_is_its_image_with_the_control_data_and_the_id() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let video = source(&ffmpeg);
    let dir = std::env::temp_dir().join(format!("disccraft-ddp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let report = build(&project(&video), &options(&dir)).unwrap();
    let ddp = dir.join("Movie");
    assert_eq!(report.output, ddp);
    assert!(dir.join("Movie.files").join("VIDEO_TS").is_dir());
    assert!(report.layer_break.is_none());

    let main = std::fs::read(ddp.join("MAIN.DAT")).unwrap();
    assert_eq!(report.total_bytes, main.len() as u64);
    assert_eq!(main.len() % (16 * 2048), 0, "whole ECC blocks");
    // The image's size, as the physical format information gives its last sector.
    let control = std::fs::read(ddp.join("CONTROL.DAT")).unwrap();
    let last = u32::from_be_bytes([0, control[15], control[16], control[17]]);
    let sectors = last - 0x03_0000 + 1;
    let image = sectors as usize * 2048;
    assert!(main.len() - image < 16 * 2048);
    assert!(
        main[image..].iter().all(|byte| *byte == 0),
        "zeros after the image"
    );
    let layout = Layout::single(sectors);
    assert_eq!(control, control_data(&layout).unwrap());
    assert_eq!(
        std::fs::read(ddp.join("DDPID")).unwrap(),
        ddp_id(&layout).unwrap()
    );

    // The image itself plays from the first frame to the last.
    let iso = dir.join("main.iso");
    std::fs::write(&iso, &main[..image]).unwrap();
    let out = Command::new(&ffmpeg)
        .args(["-v", "error", "-f", "dvdvideo", "-title", "1", "-i"])
        .arg(&iso)
        .args(["-map", "0:v", "-f", "framemd5", "-"])
        .output()
        .unwrap();
    let frames = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.starts_with('#'))
        .count();
    assert_eq!(frames, 59, "ffmpeg plays every frame of the title");

    // A rebuild into the same folder replaces the files.
    std::fs::write(ddp.join("DDPID"), b"old").unwrap();
    build(&project(&video), &options(&dir)).unwrap();
    assert_eq!(
        std::fs::read(ddp.join("DDPID")).unwrap(),
        ddp_id(&layout).unwrap()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_blu_ray_gets_no_ddp_image() {
    let dir = std::env::temp_dir().join(format!("disccraft-ddp-bd-{}", std::process::id()));
    let mut project = Project::default();
    project.settings.kind = DiscKind::BluRay;
    let error = build(&project, &options(&dir)).unwrap_err();
    assert!(error.contains("DVD only"), "{error}");
    assert!(!dir.join("Movie").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
