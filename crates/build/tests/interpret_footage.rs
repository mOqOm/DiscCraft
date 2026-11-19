//! M6.252 check: a pixel aspect ratio chosen for an asset (Encore's Interpret Footage) is how a build sees its
//! pictures. A 640 x 480 file of square pixels fills a 4:3 DVD frame; taken as 4:3 pixels (16:9 shown) it is
//! letterboxed, and the transcode in the cache is made anew. (One test in this file: the choice is the process's.)

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, TranscodeStatus, build, transcode_status};
use disccraft_project::{Link, Project};
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

/// The mean brightness of each of the 480 rows of the first picture of `vob` (720 pixels a row).
fn rows(ffmpeg: &Path, vob: &Path) -> Vec<u32> {
    let output = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(vob)
        .args(["-map", "0:v:0", "-frames:v", "1", "-f", "rawvideo"])
        .args(["-pix_fmt", "gray", "-"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{vob:?}");
    assert_eq!(output.stdout.len(), 720 * 480);
    output
        .stdout
        .chunks(720)
        .map(|row| row.iter().map(|pixel| u32::from(*pixel)).sum::<u32>() / 720)
        .collect()
}

#[test]
fn a_chosen_pixel_aspect_ratio_is_how_the_build_sees_the_pictures() {
    let Some(ffmpeg) = tool("DISCCRAFT_FFMPEG", "ffmpeg") else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-interpret-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // A bright picture of square pixels, 4:3.
    let video = dir.join("square.mp4");
    let made = Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("color=c=white:size=640x480:rate=30000/1001")
        .args(["-t", "1", "-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(&video)
        .status();
    if !made.is_ok_and(|status| status.success()) {
        eprintln!("ffmpeg cannot make the fixture (no libx264?); skipping");
        return;
    }

    let mut project = Project::default();
    let asset = project.add_asset(&video).unwrap();
    let timeline = project
        .add_timeline(asset, Some("Main".to_string()))
        .unwrap();
    project.settings.first_play = Link::Timeline {
        id: timeline,
        chapter: 0,
    };
    let cache = dir.join("cache");
    let build_to = |project: &Project, name: &str| {
        let out = dir.join(name);
        let report = build(
            project,
            &BuildOptions {
                output: OutputKind::Folder,
                path: out.clone(),
                cache: Some(cache.clone()),
                audio_language: None,
            },
        )
        .unwrap();
        assert_eq!(report.titles, 1);
        out.join("VIDEO_TS/VTS_01_1.VOB")
    };

    // As the file says: the white picture fills the frame.
    assert_eq!(disccraft_transcode::pixel_aspect(&video).unwrap(), (1, 1));
    let plain = rows(&ffmpeg, &build_to(&project, "plain"));
    assert!(plain.iter().all(|row| *row > 200), "{plain:?}");
    assert!(!disccraft_transcode::probe_video(&video).unwrap().wide);
    assert_eq!(
        transcode_status(&project, Some(&cache))[&asset],
        TranscodeStatus::Transcoded
    );

    // Taken as pixels a third wider than high: shown 16:9, so letterboxed on the 4:3 title (360 of the 480
    // rows, 60 black rows above and below), and transcoded anew.
    project.set_asset_pixel_aspect(asset, Some((4, 3))).unwrap();
    assert_eq!(
        transcode_status(&project, Some(&cache))[&asset],
        TranscodeStatus::Untranscoded
    );
    let wide = rows(&ffmpeg, &build_to(&project, "wide"));
    assert!(disccraft_transcode::probe_video(&video).unwrap().wide);
    let (top, middle, bottom) = (&wide[..52], &wide[68..412], &wide[428..]);
    assert!(top.iter().chain(bottom).all(|row| *row < 40), "{wide:?}");
    assert!(middle.iter().all(|row| *row > 200), "{wide:?}");

    // Back to the file's own: the first transcode is in the cache still.
    project.set_asset_pixel_aspect(asset, None).unwrap();
    assert_eq!(
        transcode_status(&project, Some(&cache))[&asset],
        TranscodeStatus::Transcoded
    );
    assert!(!disccraft_transcode::probe_video(&video).unwrap().wide);
    // M6.320: a still the size of a DVD frame (720 x 480) has SD pixels, as Encore takes it (its Interpret Footage
    // says "SD NTSC (0.9091)"): as a slide it fills the 4:3 frame. Taken as square pixels (1.5:1 shown) it is
    // letterboxed, and the slideshow is made anew.
    let still = dir.join("white.png");
    let made = Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("color=c=white:size=720x480")
        .args(["-frames:v", "1"])
        .arg(&still)
        .status();
    assert!(made.is_ok_and(|status| status.success()));
    let picture = project.add_asset(&still).unwrap();
    assert_eq!(
        disccraft_transcode::still_pixel_aspect(&still).unwrap(),
        (8, 9)
    );
    let show = project.add_slideshow(Some("Show".to_string())).unwrap();
    project
        .add_slide(show, picture, disccraft_project::Tick(2 * 254_016_000_000))
        .unwrap();
    let show_to = |project: &Project, name: &str| {
        let out = dir.join(name);
        let report = build(
            project,
            &BuildOptions {
                output: OutputKind::Folder,
                path: out.clone(),
                cache: Some(cache.clone()),
                audio_language: None,
            },
        )
        .unwrap();
        assert_eq!(report.titles, 2);
        out.join("VIDEO_TS/VTS_02_1.VOB")
    };
    let slide = rows(&ffmpeg, &show_to(&project, "show-sd"));
    assert!(slide.iter().all(|row| *row > 200), "{slide:?}");
    // 1.5:1 in a 4:3 frame: 427 of the 480 rows, about 27 black ones above and below.
    project
        .set_asset_pixel_aspect(picture, Some((1, 1)))
        .unwrap();
    let slide = rows(&ffmpeg, &show_to(&project, "show-square"));
    let (top, middle, bottom) = (&slide[..20], &slide[40..440], &slide[460..]);
    assert!(top.iter().chain(bottom).all(|row| *row < 40), "{slide:?}");
    assert!(middle.iter().all(|row| *row > 200), "{slide:?}");
    project.set_asset_pixel_aspect(picture, None).unwrap();

    // A ratio that is no pixel's is refused.
    assert!(project.set_asset_pixel_aspect(asset, Some((0, 1))).is_err());
    assert!(
        project
            .set_asset_pixel_aspect(asset, Some((100, 1)))
            .is_err()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
