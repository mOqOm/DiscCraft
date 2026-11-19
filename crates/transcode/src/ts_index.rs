//! The index of a program or transport stream (`.m2ts`, `.ts`, `.mpg`, `.vob`), made once.
//!
//! Indexing such a file reads all of it (every packet says which stream it belongs to), which for a camcorder's
//! or a recorder's file of many gigabytes takes as long as copying it. One import and one build asked for the
//! index several times over: the probe, the test of whether the video goes on the disc as it is, the key
//! pictures for trimming, and the demux of the video and of each sound stream each read the whole file again.
//! Here the index is kept while the file stays as it is (its size and modification time), so all of them share
//! one reading; two threads that ask at the same time wait for the same one.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use craft_media::ReadSeekSource;
use craft_media::mpegts::{self, File};

/// The most indexes kept: the files of a few titles (an index is a table of a file's access units, some tens of
/// megabytes for a file of many hours).
const KEPT: usize = 8;

/// A file as it was when its index was made: its path, size and modification time.
type Key = (PathBuf, u64, u128);
/// An index being made or made (`None`: not such a stream, or unreadable).
type Slot = Arc<OnceLock<Option<Arc<File>>>>;

#[derive(Default)]
struct Kept {
    slots: HashMap<Key, Slot>,
    /// The keys, least lately used first.
    order: Vec<Key>,
}

fn kept() -> &'static Mutex<Kept> {
    static KEPT_INDEXES: OnceLock<Mutex<Kept>> = OnceLock::new();
    KEPT_INDEXES.get_or_init(Mutex::default)
}

/// How many times a file was read through to make its index (for tests and logs).
static SCANS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The number of whole-file readings made so far in this process.
#[must_use]
pub fn scans() -> u64 {
    SCANS.load(std::sync::atomic::Ordering::Relaxed)
}

fn key_of(path: &Path) -> Option<Key> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos());
    // One file, however its path is written (relative, with `..`, through a link).
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    Some((path, metadata.len(), modified))
}

/// The slot of `key`, marked as the most lately used; the least lately used ones go when there are too many.
fn slot_of(key: &Key) -> Option<Slot> {
    let mut kept = kept().lock().ok()?;
    kept.order.retain(|known| known != key);
    kept.order.push(key.clone());
    while kept.order.len() > KEPT {
        let oldest = kept.order.remove(0);
        let _ = kept.slots.remove(&oldest);
    }
    Some(Arc::clone(kept.slots.entry(key.clone()).or_default()))
}

/// Opens the program or transport stream `path`: the file to read its units from, and its index. `None` when the
/// file cannot be read or is not such a stream.
#[must_use]
pub fn open(path: &Path) -> Option<(ReadSeekSource<fs::File>, Arc<File>)> {
    let key = key_of(path)?;
    let source = ReadSeekSource::new(fs::File::open(path).ok()?).ok()?;
    let slot = slot_of(&key)?;
    let index = slot
        .get_or_init(|| {
            let _ = SCANS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            mpegts::open(&source).ok().map(Arc::new)
        })
        .clone()?;
    Some((source, index))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::{open, scans};
    use std::io::Write;

    /// A transport stream of `packets` null packets (PID 0x1FFF): a file the scanner reads through.
    fn null_stream(path: &std::path::Path, packets: usize) {
        let mut file = std::fs::File::create(path).unwrap();
        let mut packet = [0xFF_u8; 188];
        packet[0] = 0x47;
        packet[1] = 0x1F;
        packet[2] = 0xFF;
        packet[3] = 0x10;
        for _ in 0..packets {
            file.write_all(&packet).unwrap();
        }
    }

    #[test]
    fn a_file_is_read_through_once_until_it_changes() {
        let dir = std::env::temp_dir().join(format!("disccraft-ts-index-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("nulls.ts");
        null_stream(&path, 64);
        let before = scans();
        let first = open(&path).map(|(_, index)| index);
        let again = open(&path).map(|(_, index)| index);
        assert_eq!(scans() - before, 1);
        // The same index, not another like it.
        match (&first, &again) {
            (Some(first), Some(again)) => assert!(std::sync::Arc::ptr_eq(first, again)),
            (None, None) => {}
            other => panic!("{other:?}"),
        }
        // Several threads asking at once share one reading of another file.
        let other = dir.join("more.ts");
        null_stream(&other, 128);
        let before = scans();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let _ = scope.spawn(|| {
                    let _ = open(&other);
                });
            }
        });
        assert_eq!(scans() - before, 1);
        // A file that changed is read again.
        null_stream(&path, 96);
        let before = scans();
        let _ = open(&path);
        assert_eq!(scans() - before, 1);
        // What is not there, and what is not such a stream, is no index (and asked again, not read again).
        assert!(open(&dir.join("none.ts")).is_none());
        let text = dir.join("text.ts");
        std::fs::write(&text, b"not a transport stream at all").unwrap();
        let before = scans();
        assert!(open(&text).is_none());
        assert!(open(&text).is_none());
        assert_eq!(scans() - before, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
