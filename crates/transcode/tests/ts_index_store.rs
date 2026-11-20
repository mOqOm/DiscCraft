//! M12.31 check: with the probe cache on, the index of a transport stream (craft-media's, built by reading the
//! whole file) is kept in the cache folder, an open in a later run takes it from there, and the cache's Clean
//! removes it. Needs the `h264_1080i_short.m2ts` fixture (made by the Blu-ray tests with ffmpeg); skipped without.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

fn open(path: &Path) -> craft_media::mpegts::File {
    let source = craft_media::ReadSeekSource::new(std::fs::File::open(path).unwrap()).unwrap();
    craft_media::mpegts::open(&source).unwrap()
}

/// What an open gives that DiscCraft reads: each stream's units.
fn units(file: &craft_media::mpegts::File) -> Vec<usize> {
    file.streams
        .iter()
        .map(|stream| stream.units.len())
        .collect()
}

/// One test: the cache folder and the index store are the process's.
#[test]
fn transport_stream_indexes_are_kept_in_the_cache() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/fixtures/h264_1080i_short.m2ts");
    if !fixture.is_file() {
        eprintln!("h264_1080i_short.m2ts fixture not found; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("disccraft-tsindex-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let video = dir.join("camera.m2ts");
    std::fs::copy(&fixture, &video).unwrap();
    // Read through, as without the cache.
    let scanned = units(&open(&video));
    assert!(scanned.iter().any(|count| *count > 0), "{scanned:?}");

    let cache = dir.join("cache");
    disccraft_transcode::enable_probe_cache(&cache);
    // Asked about: its index goes to the cache folder.
    assert!(disccraft_transcode::probe_video(&video).is_ok());
    let indexes = || -> Vec<PathBuf> {
        std::fs::read_dir(cache.join("ts-index"))
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

    // A later open (as the next run's) takes it from there: the same units, nothing written again.
    assert_eq!(units(&open(&video)), scanned);
    assert_eq!(
        std::fs::metadata(&kept[0]).unwrap().modified().unwrap(),
        written
    );

    // Clean (Encore's Media Cache Database) takes it away.
    assert!(disccraft_transcode::clear_probe_cache() >= 1);
    assert!(indexes().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
