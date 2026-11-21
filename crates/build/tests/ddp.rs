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

/// M14.4: a dual-layer DVD (DVD9, its layer break set at frame 45 as in `layer_break.rs`) as a DDP image: the image
/// split at the layer break between the folders `Layer 1` (layer 0's part, CONTROL.DAT, DDPID) and `Layer 2` (layer
/// 1's part padded to a whole ECC block, its DDPID), as Encore CS6 writes a dual-layer DDP image; the two parts
/// joined are the image, which plays in ffmpeg from the first frame to the last.
#[test]
fn a_dual_layer_ddp_image_has_a_folder_for_each_layer() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let video = source(&ffmpeg);
    let mut dual = project(&video);
    let timeline = *dual.timelines.keys().next().unwrap();
    dual.add_chapter(timeline, disccraft_project::Tick(37 * 8_475_667_200), None)
        .unwrap();
    dual.settings.capacity = disccraft_project::Capacity::Dvd9;
    dual.settings.layer_break = Some(disccraft_project::LayerBreakPoint {
        timeline,
        time: disccraft_project::Tick(45 * 8_475_667_200 + 8_475_667_200 / 4),
    });
    let dir = std::env::temp_dir().join(format!("disccraft-ddp-dual-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let report = build(&dual, &options(&dir)).unwrap();
    let found = report.layer_break.clone().expect("a layer break");
    let ddp = dir.join("Movie");
    assert!(!ddp.join("MAIN.DAT").exists());
    let (first, second) = (ddp.join("Layer 1"), ddp.join("Layer 2"));
    let layer0 = std::fs::read(first.join("MAIN.DAT")).unwrap();
    let layer1 = std::fs::read(second.join("MAIN.DAT")).unwrap();
    assert_eq!(layer0.len() as u64, u64::from(found.sector) * 2048);
    assert_eq!(layer1.len() % (16 * 2048), 0, "whole ECC blocks");
    assert_eq!(report.total_bytes, (layer0.len() + layer1.len()) as u64);
    let layout = Layout {
        sectors: found.total_sectors,
        layer_break: Some(found.sector),
    };
    assert_eq!(
        std::fs::read(first.join("CONTROL.DAT")).unwrap(),
        control_data(&layout).unwrap()
    );
    assert_eq!(
        std::fs::read(first.join("DDPID")).unwrap(),
        ddp_id(&layout).unwrap()
    );
    assert_eq!(
        Some(std::fs::read(second.join("DDPID")).unwrap()),
        disccraft_ddp::second_layer_id(&layout).unwrap()
    );
    // The two parts joined (without layer 1's padding) are the image, which plays every frame.
    let rest = (found.total_sectors - found.sector) as usize * 2048;
    assert!(layer1[rest..].iter().all(|byte| *byte == 0));
    let iso = dir.join("joined.iso");
    let mut joined = layer0;
    joined.extend_from_slice(&layer1[..rest]);
    std::fs::write(&iso, &joined).unwrap();
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
