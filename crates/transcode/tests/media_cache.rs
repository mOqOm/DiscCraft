//! M12.13 check: a media file is opened once and shared, what it was found to be is remembered on disk between
//! runs, and a file can be opened ahead on its own thread while the caller asks whether it is ready. Needs the
//! `ntsc.m2v` fixture (made by the other tests with ffmpeg); skipped without.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_test_support::fixtures;
use disccraft_transcode::{
    cached_blob, enable_probe_cache, media_open, probe_ready, probe_video, store_blob, warm,
    warming,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn fixture() -> Option<PathBuf> {
    let path = fixtures().join("ntsc.m2v");
    path.is_file().then_some(path)
}

fn wait_until(what: &str, done: &dyn Fn() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < Duration::from_secs(60), "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// One test: the cache folder is the process's.
#[test]
fn media_is_opened_once_remembered_and_warmed() {
    let Some(fixture) = fixture() else {
        eprintln!("ntsc.m2v fixture not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-mediacache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let video = dir.join("clip.m2v");
    std::fs::copy(&fixture, &video).unwrap();

    // Nothing is known of a file that was never opened; a file that is not there has nothing to wait for.
    assert!(!probe_ready(&video) && !media_open(&video));
    assert!(probe_ready(&dir.join("missing.mkv")));

    // Opened ahead on its own thread: ready a moment later, and then asked about at once.
    warm(&video);
    warm(&video);
    wait_until("the file was never ready", &|| probe_ready(&video));
    wait_until("the opening never ended", &|| !warming(&video));
    assert!(media_open(&video));
    let start = Instant::now();
    let first = probe_video(&video).unwrap();
    let again = probe_video(&video).unwrap();
    assert_eq!(first, again);
    assert!((first.width, first.height) == (720, 480) && first.seconds > 1.0);
    assert!(start.elapsed() < Duration::from_secs(2));

    // With a cache folder, what the file is goes to disk, keyed by its path, size and time...
    let cache = dir.join("cache");
    enable_probe_cache(&cache);
    let copy = dir.join("copy.m2v");
    std::fs::copy(&fixture, &copy).unwrap();
    let probed = probe_video(&copy).unwrap();
    let facts = std::fs::read_to_string(cache.join("media-facts.tsv")).unwrap();
    let line = facts
        .lines()
        .find(|line| line.starts_with("video\t") && line.contains("copy.m2v"))
        .unwrap_or_else(|| panic!("{facts}"));
    assert!(
        line.ends_with(&format!(",{},{},", 720, 480)) || line.contains(",720,480,"),
        "{line}"
    );
    // ...and is read back by the next run (here: the folder named again) without opening the file.
    enable_probe_cache(&cache);
    let never_opened = dir.join("moved.m2v");
    std::fs::rename(&copy, &never_opened).unwrap();
    assert!(!probe_ready(&never_opened), "another path is another file");
    std::fs::rename(&never_opened, &copy).unwrap();
    assert!(probe_ready(&copy));
    assert_eq!(probe_video(&copy).unwrap(), probed);
    assert_eq!(
        disccraft_transcode::probe_seconds(&copy).unwrap(),
        probed.seconds
    );
    // A changed file is not the file that was remembered.
    let mut bytes = std::fs::read(&copy).unwrap();
    bytes.extend_from_slice(&[0, 0, 1, 0xB7]);
    std::fs::write(&copy, bytes).unwrap();
    assert!(disccraft_transcode::cached_blob("thumb", &copy).is_none());

    // Thumbnails are kept beside the facts.
    assert_eq!(cached_blob("thumb", &video), None);
    store_blob("thumb", &video, &[1, 2, 3]);
    assert_eq!(cached_blob("thumb", &video), Some(vec![1, 2, 3]));
    store_blob("../evil", &video, &[9]);
    assert!(!dir.join("evil").exists() && cached_blob("../evil", &video).is_none());

    // Cleaning the cache (Preferences ▸ Media) forgets the facts and the thumbnails, and nothing else in the
    // folder; the file is read again when next asked about.
    assert_eq!(disccraft_transcode::probe_cache_dir(), Some(cache.clone()));
    let foreign = cache.join("notes.txt");
    std::fs::write(&foreign, b"kept").unwrap();
    assert!(disccraft_transcode::clear_probe_cache() >= 2);
    assert!(!cache.join("media-facts.tsv").exists());
    assert_eq!(cached_blob("thumb", &video), None);
    assert!(foreign.is_file());
    assert_eq!(disccraft_transcode::clear_probe_cache(), 0);
    store_blob("thumb", &video, &[4]);
    assert_eq!(cached_blob("thumb", &video), Some(vec![4]));

    // A file that is no media is given up on once: it counts as ready (asking fails at once, as before).
    let junk = dir.join("junk.mkv");
    std::fs::write(&junk, b"this is not a movie").unwrap();
    assert!(!probe_ready(&junk));
    warm(&junk);
    wait_until("the junk file was never given up on", &|| {
        probe_ready(&junk)
    });
    assert!(probe_video(Path::new(&junk)).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
