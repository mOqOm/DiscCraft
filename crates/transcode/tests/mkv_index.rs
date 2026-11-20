//! M12.27 check: with the probe cache on, the sample tables of a Matroska file (craft-media's index, whose scan
//! reads most of the file) are kept in the cache folder, an open in a later run takes them from there, and the
//! cache's Clean removes them. Needs the `h264_1080i.mkv` fixture (made by the Blu-ray tests with ffmpeg); skipped
//! without.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

/// One test: the cache folder and the index store are the process's.
#[test]
fn matroska_indexes_are_kept_in_the_cache() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures/h264_1080i.mkv");
    if !fixture.is_file() {
        eprintln!("h264_1080i.mkv fixture not found; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("disccraft-mkvindex-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let video = dir.join("camera.mkv");
    std::fs::copy(&fixture, &video).unwrap();
    let cache = dir.join("cache");
    disccraft_transcode::enable_probe_cache(&cache);

    // Opened as media: its index goes to the cache folder.
    disccraft_transcode::warm(&video);
    let start = Instant::now();
    while disccraft_transcode::warming(&video) || !disccraft_transcode::media_open(&video) {
        assert!(start.elapsed() < Duration::from_secs(60), "never opened");
        std::thread::sleep(Duration::from_millis(20));
    }
    let indexes = || -> Vec<PathBuf> {
        std::fs::read_dir(cache.join("mkv-index"))
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| path.extension().is_some_and(|ext| ext == "idx"))
                    .collect()
            })
            .unwrap_or_default()
    };
    let kept = indexes();
    assert_eq!(kept.len(), 1, "{kept:?}");
    let written = std::fs::metadata(&kept[0]).unwrap().modified().unwrap();

    // A later open (as the next run's) takes the tables from there: the same tables, nothing written again.
    let file = std::fs::File::open(&video).unwrap();
    let opened = craft_media::matroska::open(file).unwrap();
    assert!(opened.indexed);
    let video_track = opened
        .tracks
        .iter()
        .find(|track| track.kind == craft_media::matroska::TrackKind::Video)
        .unwrap();
    assert_eq!(video_track.samples.len(), 90);
    assert_eq!(
        std::fs::metadata(&kept[0]).unwrap().modified().unwrap(),
        written
    );

    // Clean (Encore's Media Cache Database) takes them away.
    assert!(disccraft_transcode::clear_probe_cache() >= 1);
    assert!(indexes().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
