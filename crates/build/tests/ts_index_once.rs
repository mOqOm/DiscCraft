//! M12.23 check: importing and building a transport stream reads the file through once to index it, not once
//! for every question asked of it (the probe, whether the video goes on the disc as it is, the demux of the video
//! and of the sound). (One test in this file: the count is the process's.)

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_project::{BdVideo, DiscKind, Link, Project};
use disccraft_test_support::fixtures;

#[test]
fn a_transport_stream_is_indexed_once_for_a_build() {
    let video = fixtures().join("h264_1080i_tones.m2ts");
    if !video.is_file() {
        eprintln!("h264_1080i_tones.m2ts fixture not found; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("disccraft-ts-once-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let before = disccraft_transcode::ts_index::scans();

    // What an import asks: how long the video is and what shape.
    let probe = disccraft_transcode::probe_video(&video).unwrap();
    assert!(probe.seconds > 0.5 && probe.width == 1920, "{probe:?}");

    let mut project = Project::default();
    project.settings.kind = DiscKind::BluRay;
    project.settings.bd_video = BdVideo::Hd1080i59;
    let asset = project.add_asset(&video).unwrap();
    let timeline = project
        .add_timeline(asset, Some("Main".to_string()))
        .unwrap();
    project.settings.first_play = Link::Timeline {
        id: timeline,
        chapter: 0,
    };
    let options = BuildOptions {
        output: OutputKind::Folder,
        path: dir.join("disc"),
        cache: Some(dir.join("cache")),
        audio_language: None,
    };
    let report = build(&project, &options).unwrap();
    assert_eq!(report.titles, 1);
    let read = disccraft_transcode::ts_index::scans() - before;
    assert_eq!(read, 1, "the file was read through {read} times");
    let _ = std::fs::remove_dir_all(&dir);
}
