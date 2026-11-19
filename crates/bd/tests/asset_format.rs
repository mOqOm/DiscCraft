//! M6.325: a video's own Blu-ray format (Encore's Transcode Settings ▸ Blu-ray ▸ Codec / Dimensions / Frame Rate):
//! on an HD Blu-ray each title is made in its video's format, the others in the project's ("Use Project Settings"),
//! and a 1080i title is MPEG-2 on a disc whose other titles are H.264.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_bd::build_bdmv;
use disccraft_project::{BdVideo, DiscKind, Link, Project};
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

/// The codec, size and frame rate of `file`'s first video stream, as ffprobe reads them.
fn video_of(ffprobe: &Path, file: &Path) -> String {
    let output = Command::new(ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries"])
        .args([
            "stream=codec_name,width,height,r_frame_rate",
            "-of",
            "csv=p=0",
        ])
        .arg(file)
        .output()
        .unwrap();
    // A transport stream's stream is listed under its program as well: the first line.
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .trim_end_matches(',')
        .to_owned()
}

#[test]
fn each_title_is_made_in_its_videos_format() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-asset-format-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let make = |name: &str| {
        let video = dir.join(name);
        let made = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc2=size=640x360:rate=30000/1001")
            .args([
                "-t",
                "1",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&video)
            .status();
        assert!(made.is_ok_and(|status| status.success()));
        video
    };

    let mut project = Project::default();
    project.settings.kind = DiscKind::BluRay;
    project.settings.capacity = disccraft_project::Capacity::Bd25;
    project.settings.bd_video = BdVideo::Hd1080p23;
    let plain = project.add_asset(&make("plain.mp4")).unwrap();
    let small = project.add_asset(&make("small.mp4")).unwrap();
    let fields = project.add_asset(&make("fields.mp4")).unwrap();
    let first = project
        .add_timeline(plain, Some("Plain".to_string()))
        .unwrap();
    let _ = project
        .add_timeline(small, Some("Small".to_string()))
        .unwrap();
    let _ = project
        .add_timeline(fields, Some("Fields".to_string()))
        .unwrap();
    project.settings.first_play = Link::Timeline {
        id: first,
        chapter: 0,
    };
    for (asset, video) in [(small, BdVideo::Hd720p59), (fields, BdVideo::Hd1080i59)] {
        if let Some(found) = project.assets.get_mut(&asset) {
            found.bluray_video = Some(video);
        }
    }
    assert_eq!(project.asset_bd_video(plain), BdVideo::Hd1080p23);
    assert_eq!(project.asset_bd_video(small), BdVideo::Hd720p59);

    let out = dir.join("bd");
    let report = build_bdmv(&project, &out).unwrap();
    assert_eq!(report.titles.len(), 3);
    let streams = out.join("BDMV/STREAM");
    assert_eq!(
        video_of(&ffprobe, &streams.join("00001.m2ts")),
        "h264,1920,1080,24000/1001"
    );
    assert_eq!(
        video_of(&ffprobe, &streams.join("00002.m2ts")),
        "h264,1280,720,60000/1001"
    );
    assert_eq!(
        video_of(&ffprobe, &streams.join("00003.m2ts")),
        "mpeg2video,1920,1080,30000/1001"
    );
    // Each decodes without an error.
    for clip in ["00001", "00002", "00003"] {
        let decoded = Command::new(&ffmpeg)
            .args(["-v", "error", "-xerror", "-i"])
            .arg(streams.join(format!("{clip}.m2ts")))
            .args(["-f", "null", "-"])
            .output()
            .unwrap();
        assert!(
            decoded.status.success() && decoded.stderr.is_empty(),
            "{clip}: {}",
            String::from_utf8_lossy(&decoded.stderr)
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
