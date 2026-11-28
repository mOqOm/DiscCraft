//! M6.282 check: File ▸ Locate Transcoded File ▸ DVD…: a video asset whose source is gone builds from the files it
//! was transcoded to, its sound included, and counts as transcoded.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, TranscodeStatus, build, transcode_status};
use disccraft_project::{Link, LocatedTranscode, Project};
use disccraft_test_support::fixtures;

#[test]
fn located_transcodes_build_in_place_of_a_source_that_is_gone() {
    let (video, sound) = (fixtures().join("ntsc.m2v"), fixtures().join("ntsc.ac3"));
    if !video.is_file() || !sound.is_file() {
        eprintln!("ntsc.m2v / ntsc.ac3 fixtures not found; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("disccraft-located-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let options = |name: &str| BuildOptions {
        output: OutputKind::Folder,
        path: dir.join(name),
        cache: Some(dir.join("cache")),
        audio_language: Some("en".to_owned()),
    };

    let mut project = Project::default();
    let source = project.add_asset(&dir.join("deleted.mp4")).unwrap();
    let timeline = project
        .add_timeline(source, Some("Main".to_owned()))
        .unwrap();
    project.settings.first_play = Link::Timeline {
        id: timeline,
        chapter: 0,
    };
    // The source is gone: nothing to build from.
    assert!(build(&project, &options("before")).is_err());

    project.assets.get_mut(&source).unwrap().located_dvd = Some(LocatedTranscode {
        video: video.to_string_lossy().into_owned(),
        audio: Some(sound.to_string_lossy().into_owned()),
    });
    assert_eq!(
        transcode_status(&project, None).get(&source).copied(),
        Some(TranscodeStatus::Transcoded)
    );
    build(&project, &options("after")).unwrap();
    let video_ts = dir.join("after").join("VIDEO_TS");
    // The title has the located sound: the title set says one audio stream (VTSI_MAT 0x202).
    let ifo = std::fs::read(video_ts.join("VTS_01_0.IFO")).unwrap();
    assert_eq!(
        ifo.get(0x202..0x204),
        Some(&[0_u8, 1][..]),
        "audio streams of the title set"
    );
    // The located video went on as it is: the VOB holds its first sequence header's bytes.
    let vob = std::fs::read(video_ts.join("VTS_01_1.VOB")).unwrap();
    let m2v = std::fs::read(&video).unwrap();
    let head = m2v.get(..12).unwrap();
    assert!(vob.windows(head.len()).any(|window| window == head));
    // A Blu-ray build has nothing located: the source is still needed.
    let mut bluray = project.clone();
    bluray.settings.kind = disccraft_project::DiscKind::BluRay;
    assert_ne!(
        transcode_status(&bluray, None).get(&source).copied(),
        Some(TranscodeStatus::Transcoded)
    );
    let _ = std::fs::remove_dir_all(&dir);
}
