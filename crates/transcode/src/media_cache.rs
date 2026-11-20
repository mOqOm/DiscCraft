//! Keeping large media quick to work with (ROADMAP M12.13). Opening a long Matroska or MP4 file reads the index of
//! the whole file: seconds for a file of tens of gigabytes, and every question about it (its length, a thumbnail,
//! the disc's size) used to open it again.
//!
//! - An opened source is kept and shared while the program runs ([`crate::source::Source`] opens through
//!   [`slot`]), the last few files.
//! - What a file was found to be (its length and picture size) is remembered on disk between runs, once the
//!   application names a folder for it ([`enable_probe_cache`]), by the file's path, size and time. So are the
//!   indexes craft-media builds of Matroska files and of transport and program streams.
//! - A file can be opened ahead on a thread of its own ([`warm`]) while the caller asks [`probe_ready`] instead
//!   of waiting.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::source::Source;

/// How many opened sources are kept.
const MAX_OPEN: usize = 8;
/// The most remembered facts; the oldest are dropped when the file is next loaded.
const MAX_FACTS: usize = 4000;

/// A file as it is now: its path, size and modification time.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FileKey {
    path: PathBuf,
    len: u64,
    modified_nanos: u128,
}

impl FileKey {
    /// The key of `path`; `None` when it cannot be read.
    pub(crate) fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        let modified_nanos = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_nanos());
        Some(Self {
            path: std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()),
            len: metadata.len(),
            modified_nanos,
        })
    }

    /// One line of text for the key (`None` when the path has a tab or a line break).
    fn text(&self) -> Option<String> {
        let path = self.path.to_str()?;
        if path.contains(['\t', '\n', '\r']) {
            return None;
        }
        Some(format!("{path}\t{}\t{}", self.len, self.modified_nanos))
    }
}

/// Where one file's opened source is kept: empty until it has been opened. Opening holds the slot, so a second
/// caller waits for the first instead of opening the file again.
pub(crate) type Slot = Arc<Mutex<Option<Source>>>;

static OPEN: Mutex<Vec<(FileKey, Slot)>> = Mutex::new(Vec::new());

/// The slot of the file `key`, the most recently used last; the least recently used is dropped past
/// [`MAX_OPEN`] (whoever still holds its source keeps it).
pub(crate) fn slot(key: &FileKey) -> Slot {
    let Ok(mut open) = OPEN.lock() else {
        return Arc::new(Mutex::new(None));
    };
    if let Some(at) = open.iter().position(|(other, _)| other == key) {
        let entry = open.remove(at);
        let slot = Arc::clone(&entry.1);
        open.push(entry);
        return slot;
    }
    let slot: Slot = Arc::new(Mutex::new(None));
    open.push((key.clone(), Arc::clone(&slot)));
    if open.len() > MAX_OPEN {
        let _ = open.remove(0);
    }
    slot
}

/// Whether `path` is open already (not being opened: that is not ready yet).
fn is_open(path: &Path) -> bool {
    let Some(key) = FileKey::of(path) else {
        return false;
    };
    let Ok(open) = OPEN.lock() else {
        return false;
    };
    open.iter()
        .find(|(other, _)| *other == key)
        .is_some_and(|(_, slot)| slot.try_lock().is_ok_and(|source| source.is_some()))
}

/// The remembered facts and the file they are kept in.
struct Facts {
    file: PathBuf,
    values: HashMap<String, String>,
}

static FACTS: Mutex<Option<Facts>> = Mutex::new(None);

/// Remembers what media files were found to be (length, picture size, thumbnails) in `dir` from now on, between
/// runs of the program. Without this call nothing is written (tests and one-off tools).
pub fn enable_probe_cache(dir: &Path) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let file = dir.join("media-facts.tsv");
    let mut lines: Vec<(String, String)> = std::fs::read_to_string(&file)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (key, value) = line.rsplit_once('\t')?;
            Some((key.to_string(), value.to_string()))
        })
        .collect();
    if lines.len() > MAX_FACTS {
        // Rewritten with the newest half.
        lines = lines.split_off(lines.len().saturating_sub(MAX_FACTS / 2));
        let text: String = lines
            .iter()
            .map(|(key, value)| format!("{key}\t{value}\n"))
            .collect();
        let _ = std::fs::write(&file, text);
    }
    if let Ok(mut facts) = FACTS.lock() {
        *facts = Some(Facts {
            file,
            values: lines.into_iter().collect(),
        });
    }
    // M12.27: the sample tables of Matroska files are kept here too, so a large MKV is not scanned again each
    // run (craft-media reads most of the file to build them).
    craft_media::matroska::set_index_store(Some(std::sync::Arc::new(IndexFiles {
        dir: dir.join(MKV_INDEX_DIR),
    })));
    // M12.31: and the indexes of transport and program streams (`.m2ts`, `.ts`, `.mpg`, `.vob`), which
    // craft-media builds by reading the whole file.
    craft_media::mpegts::set_index_store(Some(std::sync::Arc::new(IndexFiles {
        dir: dir.join(TS_INDEX_DIR),
    })));
}

/// The folder in the probe cache the Matroska indexes are kept in.
const MKV_INDEX_DIR: &str = "mkv-index";
/// The folder in the probe cache the transport and program stream indexes are kept in.
const TS_INDEX_DIR: &str = "ts-index";
/// The most bytes the indexes of one folder take: past it, the least lately written go.
const INDEX_BYTES: u64 = 1 << 30;

/// Indexes craft-media keeps between runs (its index stores: Matroska, M12.27; MPEG-2 transport and program
/// streams, M12.31), a file each in `dir`, named by the key craft-media gives (the file's length and hashes of
/// its start and end).
struct IndexFiles {
    dir: PathBuf,
}

impl IndexFiles {
    /// The file of `key`, when the key is one craft-media makes (letters, digits and dashes).
    fn file(&self, key: &str) -> Option<PathBuf> {
        (!key.is_empty()
            && key.len() <= 96
            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .then(|| self.dir.join(format!("{key}.idx")))
    }

    /// Takes the least lately written indexes away until they take at most [`INDEX_BYTES`].
    fn trim(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let meta = entry.metadata().ok()?;
                let path = entry.path();
                (meta.is_file() && path.extension().is_some_and(|ext| ext == "idx")).then(|| {
                    (
                        meta.modified().ok().unwrap_or(std::time::UNIX_EPOCH),
                        meta.len(),
                        path,
                    )
                })
            })
            .collect();
        let mut total: u64 = files.iter().map(|(_, len, _)| *len).sum();
        files.sort();
        for (_, len, path) in files {
            if total <= INDEX_BYTES {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(len);
            }
        }
    }

    /// The index saved under `key`.
    fn load_index(&self, key: &str) -> Option<Vec<u8>> {
        std::fs::read(self.file(key)?).ok()
    }

    /// Keeps `index` under `key` (written whole under another name first), then trims the folder.
    fn save_index(&self, key: &str, index: &[u8]) {
        let Some(file) = self.file(key) else {
            return;
        };
        if std::fs::create_dir_all(&self.dir).is_err() || index.len() as u64 > INDEX_BYTES {
            return;
        }
        let part = file.with_extension(format!("idx.{}.part", std::process::id()));
        if std::fs::write(&part, index).is_err() || std::fs::rename(&part, &file).is_err() {
            let _ = std::fs::remove_file(&part);
            return;
        }
        self.trim();
    }
}

impl craft_media::matroska::IndexStore for IndexFiles {
    fn load(&self, key: &str) -> Option<Vec<u8>> {
        self.load_index(key)
    }

    fn save(&self, key: &str, index: &[u8]) {
        self.save_index(key, index);
    }
}

impl craft_media::mpegts::IndexStore for IndexFiles {
    fn load(&self, key: &str) -> Option<Vec<u8>> {
        self.load_index(key)
    }

    fn save(&self, key: &str, index: &[u8]) {
        self.save_index(key, index);
    }
}

/// The folder what is remembered of media files is kept in ([`enable_probe_cache`]); `None` when nothing is kept.
#[must_use]
pub fn probe_cache_dir() -> Option<PathBuf> {
    cache_dir()
}

/// Forgets what was remembered of media files (Encore's Media Cache Database ▸ Clean): the facts and the
/// thumbnails in the cache folder. Files are read again when they are next asked about. Returns how many files
/// went.
pub fn clear_probe_cache() -> usize {
    let Some(dir) = cache_dir() else {
        return 0;
    };
    if let Ok(mut facts) = FACTS.lock()
        && let Some(facts) = facts.as_mut()
    {
        facts.values.clear();
    }
    let mut removed = 0usize;
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return 0;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        // Only what this cache wrote: the facts, and the blobs (`<kind>-<16 hex digits>.bin`).
        let ours = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name == "media-facts.tsv"
                    // The sound taken out of a transport stream (M6.366): `ts-audio-<16 hex digits>.<ext>`.
                    || name
                        .strip_prefix("ts-audio-")
                        .and_then(|rest| rest.split_once('.'))
                        .is_some_and(|(key, _)| {
                            key.len() == 16 && key.chars().all(|c| c.is_ascii_hexdigit())
                        })
                    || name.strip_suffix(".bin").is_some_and(|stem| {
                        stem.rsplit_once('-').is_some_and(|(kind, key)| {
                            !kind.is_empty()
                                && kind.chars().all(|c| c.is_ascii_alphanumeric())
                                && key.len() == 16
                                && key.chars().all(|c| c.is_ascii_hexdigit())
                        })
                    })
            });
        if ours && path.is_file() && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    // The Matroska indexes (M12.27) and the transport and program stream indexes (M12.31).
    for folder in [MKV_INDEX_DIR, TS_INDEX_DIR] {
        let Ok(entries) = std::fs::read_dir(dir.join(folder)) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "idx")
                && path.is_file()
                && std::fs::remove_file(&path).is_ok()
            {
                removed += 1;
            }
        }
    }
    removed
}

/// The folder of the remembered facts, when [`enable_probe_cache`] named one.
fn cache_dir() -> Option<PathBuf> {
    let facts = FACTS.lock().ok()?;
    facts.as_ref()?.file.parent().map(Path::to_path_buf)
}

fn fact_key(kind: &str, path: &Path) -> Option<String> {
    Some(format!("{kind}\t{}", FileKey::of(path)?.text()?))
}

/// The fact `kind` remembered of the file `path` as it is now.
pub(crate) fn fact(kind: &str, path: &Path) -> Option<String> {
    let key = fact_key(kind, path)?;
    let facts = FACTS.lock().ok()?;
    facts.as_ref()?.values.get(&key).cloned()
}

/// Remembers the fact `kind` of the file `path` (a value without tabs or line breaks).
pub(crate) fn remember(kind: &str, path: &Path, value: &str) {
    if value.contains(['\t', '\n', '\r']) {
        return;
    }
    let Some(key) = fact_key(kind, path) else {
        return;
    };
    let Ok(mut facts) = FACTS.lock() else {
        return;
    };
    let Some(facts) = facts.as_mut() else {
        return;
    };
    if facts
        .values
        .insert(key.clone(), value.to_string())
        .as_deref()
        == Some(value)
    {
        return;
    }
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&facts.file)
    {
        let _ = writeln!(file, "{key}\t{value}");
    }
}

fn blob_path(kind: &str, path: &Path) -> Option<PathBuf> {
    let key = fact_key(kind, path)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    Some(cache_dir()?.join(format!("{kind}-{:016x}.bin", hasher.finish())))
}

/// Bytes remembered of the file `path` under `kind` (a thumbnail), when the probe cache is on.
#[must_use]
pub fn cached_blob(kind: &str, path: &Path) -> Option<Vec<u8>> {
    std::fs::read(blob_path(kind, path)?).ok()
}

/// Remembers `bytes` of the file `path` under `kind` (letters and digits), when the probe cache is on.
pub fn store_blob(kind: &str, path: &Path, bytes: &[u8]) {
    if !kind.chars().all(|c| c.is_ascii_alphanumeric()) {
        return;
    }
    if let Some(file) = blob_path(kind, path) {
        let _ = std::fs::write(file, bytes);
    }
}

/// Whether the facts of the media file `path` can be given at once: it is open, or they are remembered. A Craft
/// document (a SoundCraft session, an EffectCraft project) always counts as ready: it is made on demand.
#[must_use]
pub fn probe_ready(path: &Path) -> bool {
    crate::session::is_document(path)
        || is_open(path)
        || fact("video", path).is_some()
        || fact("audio", path).is_some()
        || refused(path)
        // A Matroska file's header answers without the file being opened as media.
        || crate::mkv_probe::quick(path).is_some()
}

/// Whether the media file `path` is open, so its pictures can be read without the wait of opening it.
#[must_use]
pub fn media_open(path: &Path) -> bool {
    crate::session::is_document(path) || is_open(path) || refused(path)
}

static WARMING: Mutex<Option<HashSet<FileKey>>> = Mutex::new(None);
/// Files [`warm`] could not open as media: asked about again, they fail at once as before, and are not opened
/// ahead over and over.
static REFUSED: Mutex<Option<HashSet<FileKey>>> = Mutex::new(None);

fn refused(path: &Path) -> bool {
    let Some(key) = FileKey::of(path) else {
        // A file that is not there has nothing to wait for.
        return true;
    };
    refused_file(&key)
}

/// Whether [`warm`] found that the file `key` cannot be opened as media.
pub(crate) fn refused_file(key: &FileKey) -> bool {
    REFUSED
        .lock()
        .ok()
        .and_then(|refused| refused.as_ref().map(|set| set.contains(key)))
        .unwrap_or(false)
}

/// Opens the media file `path` on a thread of its own and remembers what it is, so that asking about it later
/// does not wait. Does nothing when it is open, being opened, or cannot be read.
pub fn warm(path: &Path) {
    if media_open(path) {
        return;
    }
    let Some(key) = FileKey::of(path) else {
        return;
    };
    {
        let Ok(mut warming) = WARMING.lock() else {
            return;
        };
        if !warming.get_or_insert_with(HashSet::new).insert(key.clone()) {
            return;
        }
    }
    let path = path.to_path_buf();
    let done = key.clone();
    let finish = move |key: &FileKey| {
        if let Ok(mut warming) = WARMING.lock()
            && let Some(warming) = warming.as_mut()
        {
            let _ = warming.remove(key);
        }
    };
    let spawned = std::thread::Builder::new()
        .name("media-open".to_string())
        .spawn(move || {
            // Opened here (its facts may be known from its header alone), then asked, which fills the caches. A
            // file that does not open is left alone.
            let opened = crate::source::Source::open(&path).is_ok()
                || crate::source::Source::open_audio(&path).is_ok();
            let video = crate::probe_video(&path).is_ok();
            let audio = crate::probe_audio_seconds(&path).is_ok();
            let known = video || audio;
            // Not openable: nothing to wait for, also when its header alone told its facts.
            if !opened
                && (!known
                    || crate::mkv_probe::quick(&path).is_some()
                    || crate::ts_probe::looks_like_ts(&path))
                && let Ok(mut refused) = REFUSED.lock()
            {
                let _ = refused
                    .get_or_insert_with(HashSet::new)
                    .insert(done.clone());
            }
            finish(&done);
        });
    if spawned.is_err() {
        finish(&key);
    }
}

/// Whether the media file `path` is being opened by [`warm`].
#[must_use]
pub fn warming(path: &Path) -> bool {
    let Some(key) = FileKey::of(path) else {
        return false;
    };
    WARMING
        .lock()
        .ok()
        .and_then(|warming| warming.as_ref().map(|set| set.contains(&key)))
        .unwrap_or(false)
}
