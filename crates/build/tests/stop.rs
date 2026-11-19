//! M6.259 check: a running build's transcodes stop when asked (Encore's Progress panel ▸ Stop): the build ends
//! with an error that says so, leaves no half-made transcode in its cache, and the next build runs as usual.
//! (One test in this file: the request is the process's.)

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{
    BuildOptions, OutputKind, build, build_with_progress, request_stop, was_stopped,
};
use disccraft_project::{Link, Project};
use std::path::PathBuf;

#[test]
fn a_build_stops_when_asked_and_the_next_one_runs() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
    let video = fixtures.join("sound_track.mp4");
    if !video.is_file() {
        eprintln!("sound_track.mp4 fixture not found; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("disccraft-stop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut project = Project::default();
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

    // Asked to stop once the transcode is a tenth done.
    let mut furthest = 0.0_f32;
    let stopped = build_with_progress(&project, &options, &mut |_, fraction| {
        furthest = furthest.max(fraction);
        if fraction > 0.1 {
            request_stop();
        }
    });
    let Err(error) = stopped else {
        panic!("the build was not stopped");
    };
    assert!(was_stopped(&error), "{error}");
    assert!(furthest < 0.9, "it ran on to {furthest}");
    // Nothing half-made is left for a later build to take for a transcode.
    let left: Vec<String> = std::fs::read_dir(dir.join("cache"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".m2v") || name.ends_with(".ok"))
                .collect()
        })
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
    assert!(!was_stopped("io error at movie.mp4: not found"));

    // The next build is not stopped by the old request.
    let report = build(&project, &options).unwrap();
    assert_eq!(report.titles, 1);

    // M6.268: a stop also ends a build between its other steps: here before the image is written.
    let image = BuildOptions {
        output: OutputKind::Iso,
        path: dir.join("stopped.iso"),
        cache: Some(dir.join("cache")),
        audio_language: None,
    };
    request_stop();
    // A request from before the build does not count: this one runs to its end.
    let whole = build(&project, &image).unwrap();
    assert!(whole.total_bytes > 0 && dir.join("stopped.iso").is_file());
    std::fs::remove_file(dir.join("stopped.iso")).unwrap();
    // Asked as the image step begins: the image is not written.
    let stopped = build_with_progress(&project, &image, &mut |step, _| {
        if step == disccraft_build::STEP_IMAGE {
            request_stop();
        }
    });
    let Err(error) = stopped else {
        panic!("the image build was not stopped");
    };
    assert!(was_stopped(&error), "{error}");
    assert!(!dir.join("stopped.iso").exists());

    // M6.278: asked while the image is being written (from another thread, as the Stop button does), the
    // writer gives up at its next chunk, the build ends as stopped and no half-written image is left. Only
    // checked when the request got in before the (small) image was done.
    let asker = std::thread::spawn(|| {
        for _ in 0..2000 {
            request_stop();
            std::thread::yield_now();
        }
    });
    let raced = build(&project, &image);
    asker.join().unwrap();
    if let Err(error) = raced {
        assert!(was_stopped(&error), "{error}");
        assert!(
            !dir.join("stopped.iso").exists(),
            "a half-written image was left"
        );
    }
    // And the next build is whole again.
    let whole = build(&project, &image).unwrap();
    assert!(whole.total_bytes > 0 && dir.join("stopped.iso").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}
