//! M12.20 check: Transcode Now makes a build's transcodes ahead, in the build's cache, for one asset or for the
//! whole project; the build that follows finds them.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{
    BuildOptions, OutputKind, TranscodeStatus, build, cache_dir, revert_to_original, transcode_now,
    transcode_status,
};
use disccraft_project::{BdVideo, DiscKind, Link, Project};
use disccraft_test_support::{ffmpeg, fixtures};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Two seconds of a small H.264 picture with AAC sound in an MP4 (a build transcodes it).
fn video(ffmpeg: &Path) -> PathBuf {
    let fixtures = fixtures();
    let video = fixtures.join("sound_track.mp4");
    if video.is_file() {
        return video;
    }
    std::fs::create_dir_all(&fixtures).unwrap();
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

#[test]
fn transcode_now_fills_the_builds_cache() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let source = video(&ffmpeg);
    let dir = std::env::temp_dir().join(format!("disccraft-transcode-now-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Two files of their own: each has its own transcode.
    let (first, second) = (dir.join("first.mp4"), dir.join("second.mp4"));
    std::fs::copy(&source, &first).unwrap();
    std::fs::copy(&source, &second).unwrap();

    let mut project = Project::default();
    let a = project.add_asset(&first).unwrap();
    let b = project.add_asset(&second).unwrap();
    let t = project.add_timeline(a, Some("A".to_string())).unwrap();
    let _ = project.add_timeline(b, Some("B".to_string())).unwrap();
    project.settings.first_play = Link::Timeline { id: t, chapter: 0 };
    let options = BuildOptions {
        output: OutputKind::Folder,
        path: dir.join("disc"),
        cache: None,
        audio_language: None,
    };
    let cache = cache_dir(&options);
    let status = |project: &Project| {
        let all = transcode_status(project, Some(&cache));
        (all[&a], all[&b])
    };
    assert_eq!(
        status(&project),
        (TranscodeStatus::Untranscoded, TranscodeStatus::Untranscoded)
    );

    // One asset: its timeline's transcode only. The progress names what is being made.
    let mut steps = Vec::new();
    transcode_now(&project, &options, Some(a), &mut |step, _| {
        if steps.last().map(String::as_str) != Some(step) {
            steps.push(step.to_owned());
        }
    })
    .unwrap();
    assert!(!steps.is_empty());
    assert_eq!(
        status(&project),
        (TranscodeStatus::Transcoded, TranscodeStatus::Untranscoded)
    );
    // No disc was written.
    assert!(!dir.join("disc").join("VIDEO_TS").exists());

    // The whole project; a second time there is nothing left to make.
    transcode_now(&project, &options, None, &mut |_, _| {}).unwrap();
    assert_eq!(
        status(&project),
        (TranscodeStatus::Transcoded, TranscodeStatus::Transcoded)
    );
    let made = std::time::Instant::now();
    transcode_now(&project, &options, None, &mut |_, _| {}).unwrap();
    let again = made.elapsed();
    // The build uses them.
    let report = build(&project, &options).unwrap();
    assert_eq!(report.titles, 2);
    assert!(again < std::time::Duration::from_secs(5), "{again:?}");

    // Revert to Original: one asset's transcodes go, the other's stay; the build makes them again.
    assert!(revert_to_original(&project, &options, a).unwrap() >= 2);
    assert_eq!(
        status(&project),
        (TranscodeStatus::Untranscoded, TranscodeStatus::Transcoded)
    );
    assert_eq!(revert_to_original(&project, &options, a).unwrap(), 0);
    transcode_now(&project, &options, Some(a), &mut |_, _| {}).unwrap();
    assert_eq!(status(&project).0, TranscodeStatus::Transcoded);

    // An HD Blu-ray: its titles' H.264 is made ahead in the same way.
    project.settings.kind = DiscKind::BluRay;
    project.settings.bd_video = BdVideo::Hd720p59;
    let hd = BuildOptions {
        path: dir.join("bd"),
        ..options
    };
    let hd_status = |project: &Project| {
        let all = transcode_status(project, Some(&cache_dir(&hd)));
        (all[&a], all[&b])
    };
    transcode_now(&project, &hd, Some(a), &mut |_, _| {}).unwrap();
    assert_eq!(
        hd_status(&project),
        (TranscodeStatus::Transcoded, TranscodeStatus::Untranscoded)
    );
    let streams = |dir: &Path| {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "h264"))
            .count()
    };
    assert_eq!(streams(&cache_dir(&hd)), 1);
    transcode_now(&project, &hd, None, &mut |_, _| {}).unwrap();
    assert_eq!(streams(&cache_dir(&hd)), 2);
    assert_eq!(
        hd_status(&project),
        (TranscodeStatus::Transcoded, TranscodeStatus::Transcoded)
    );
    assert!(revert_to_original(&project, &hd, b).unwrap() >= 2);
    assert_eq!(
        hd_status(&project),
        (TranscodeStatus::Transcoded, TranscodeStatus::Untranscoded)
    );
    transcode_now(&project, &hd, Some(b), &mut |_, _| {}).unwrap();
    assert_eq!(hd_status(&project).1, TranscodeStatus::Transcoded);
    // Trimming a clip asks for another encode.
    let second = *project.timelines.keys().nth(1).unwrap();
    project.timelines.get_mut(&second).unwrap().video_trims = vec![disccraft_project::ClipTrim {
        start: 5,
        end: None,
    }];
    assert_eq!(hd_status(&project).1, TranscodeStatus::Untranscoded);
    assert!(!dir.join("bd").join("BDMV").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
