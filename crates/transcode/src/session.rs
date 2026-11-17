//! Craft documents as media: a SoundCraft session (`.scraft`) is mixed down once to a WAV, an EffectCraft project
//! (`.ecproj`) rendered once to a movie, each by its own app's engine (through `disccraft-sound` and
//! `disccraft-motion`), and that file is what gets opened. So a session or a project works wherever an audio or a
//! video file does (length, pictures, waveform, playback, the build).
//!
//! The results are kept in a folder of the system's temporary folder. A document gets a new one when its file
//! changes (place, size, time), and for a session also when the audio files beside it do; the older ones of the
//! same document are removed then, and so is whatever has not been touched for a month.

use std::borrow::Cow;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::{TranscodeError, io_error};

/// The folder (in the system's temporary folder) that keeps the mixdowns and renders.
const CACHE_FOLDER: &str = "disccraft-sessions";
/// A kept file nobody has made or used for this long is removed.
const KEEP: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// The most files looked at beside a session (its audio files) when telling whether it changed.
const MAX_SIBLINGS: usize = 4096;
/// The folder SoundCraft keeps a session's audio files in, beside the session.
const AUDIO_FOLDER: &str = "Audio Files";

/// The composition to render of each EffectCraft project that has one chosen (by the project's full path).
static CHOSEN: std::sync::Mutex<Vec<(std::path::PathBuf, String)>> =
    std::sync::Mutex::new(Vec::new());

/// Says which composition each EffectCraft project is rendered as (the ones not named render the composition
/// their project has active). The list replaces the one before: the engine passes its project's choices.
pub fn set_compositions(chosen: Vec<(std::path::PathBuf, String)>) {
    let chosen = chosen
        .into_iter()
        .map(|(path, name)| (std::path::absolute(&path).unwrap_or(path), name))
        .collect();
    *CHOSEN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = chosen;
}

/// The composition chosen for the project at `absolute`.
fn chosen_composition(absolute: &Path) -> Option<String> {
    CHOSEN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .find(|(path, _)| path == absolute)
        .map(|(_, name)| name.clone())
}

/// Takes a media path apart: the file on disk, and the composition when the path names one under an EffectCraft
/// project file (`<project>.ecproj/<composition>.ecproj`, as `disccraft_project::Asset::media_path` writes it for
/// an asset shown as a chosen composition). Any other path is the file itself.
#[must_use]
pub fn document_of(path: &Path) -> (Cow<'_, Path>, Option<String>) {
    let named = || {
        let parent = path.parent()?;
        if !disccraft_motion::is_project(path) || !disccraft_motion::is_project(parent) {
            return None;
        }
        // A real file of that name (a project kept in a folder called `x.ecproj`) is itself.
        if path.is_file() || !parent.is_file() {
            return None;
        }
        let stem = path.file_stem()?.to_str()?;
        let name = stem
            .replace("%2F", "/")
            .replace("%5C", "\\")
            .replace("%25", "%");
        Some((parent.to_path_buf(), name))
    };
    match named() {
        Some((file, name)) => (Cow::Owned(file), Some(name)),
        None => (Cow::Borrowed(path), None),
    }
}

/// The metadata of the file a media path stands for (see [`document_of`]).
///
/// # Errors
///
/// As [`std::fs::metadata`].
pub fn document_metadata(path: &Path) -> std::io::Result<std::fs::Metadata> {
    std::fs::metadata(document_of(path).0)
}

/// Whether `path` names a Craft document (a SoundCraft session or an EffectCraft project, or a composition of
/// one), which is made into media on demand.
pub(crate) fn is_document(path: &Path) -> bool {
    Craft::of(&document_of(path).0).is_some() || Craft::of(path).is_some()
}

/// Held while a document is being mixed down or rendered.
static MAKING: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// Numbers the part files of this process.
static PART_NUMBER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The name a result is written under until it is finished: no two writers share one.
fn part_name(prefix: &str, process: u32, number: u64, extension: &str) -> String {
    format!("{prefix}part-{process}-{number}.{extension}")
}

/// The kind of Craft document a path names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Craft {
    /// A SoundCraft session: mixed down to a WAV.
    Session,
    /// An EffectCraft project: rendered to an MP4.
    Composition,
}

impl Craft {
    fn of(path: &Path) -> Option<Self> {
        if disccraft_sound::is_session(path) {
            Some(Self::Session)
        } else if disccraft_motion::is_project(path) {
            Some(Self::Composition)
        } else {
            None
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Session => "wav",
            Self::Composition => "mp4",
        }
    }
}

/// The file to open for `path`: the path itself, or for a SoundCraft session its mixdown and for an EffectCraft
/// project its render (made now if the cache has none for this version of the document).
pub(crate) fn resolved(media: &Path) -> Result<Cow<'_, Path>, TranscodeError> {
    let Some(craft) = Craft::of(media) else {
        return Ok(Cow::Borrowed(media));
    };
    // The composition the path itself names, before the one chosen for the whole file.
    let (document, named) = document_of(media);
    let path: &Path = &document;
    let metadata = std::fs::metadata(path).map_err(|error| io_error(path, error))?;
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut version = std::collections::hash_map::DefaultHasher::new();
    metadata.len().hash(&mut version);
    nanos(metadata.modified().ok()).hash(&mut version);
    if craft == Craft::Session {
        siblings(&absolute).hash(&mut version);
    }
    // A composition named by the path keeps results of its own: two assets of one project file showing different
    // compositions must not clear each other's render away (each would then be made again at every use).
    let place = if named.is_some() {
        std::path::absolute(media).unwrap_or_else(|_| media.to_path_buf())
    } else {
        absolute.clone()
    };
    let composition = (craft == Craft::Composition)
        .then(|| named.or_else(|| chosen_composition(&absolute)))
        .flatten();
    composition.hash(&mut version);
    let folder = std::env::temp_dir().join(CACHE_FOLDER);
    let prefix = name_prefix(&place);
    let kept = folder.join(format!(
        "{prefix}{:016x}.{}",
        version.finish(),
        craft.extension()
    ));
    if kept.is_file() {
        return Ok(Cow::Owned(kept));
    }
    // One at a time in this process: the length, the waveform and the playback of a document all ask for it at
    // once, and the second one finds what the first one made.
    let _making = MAKING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if kept.is_file() {
        return Ok(Cow::Owned(kept));
    }
    std::fs::create_dir_all(&folder).map_err(|error| io_error(&folder, error))?;
    // Written under a name of its own and renamed when finished, so a half-written file is never opened and
    // two renders of the same document (in two processes) do not write into each other.
    let part = folder.join(part_name(
        &prefix,
        std::process::id(),
        PART_NUMBER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        craft.extension(),
    ));
    let made = match craft {
        Craft::Session => disccraft_sound::mixdown(path, &part)
            .map(|_| ())
            .map_err(|error| TranscodeError::Decode(error.to_string())),
        Craft::Composition => {
            disccraft_motion::render(path, &part, composition.as_deref(), &mut |_| {})
                .map(|_| ())
                .map_err(|error| TranscodeError::Decode(error.to_string()))
        }
    };
    if let Err(error) = made {
        let _ = std::fs::remove_file(&part);
        return Err(error);
    }
    if std::fs::rename(&part, &kept).is_err() {
        // Another one finished first: its file is as good.
        let _ = std::fs::remove_file(&part);
        if !kept.is_file() {
            return Err(TranscodeError::Decode(format!(
                "{}: the result could not be kept",
                path.display()
            )));
        }
    }
    prune(&folder, &prefix, &kept, SystemTime::now());
    Ok(Cow::Owned(kept))
}

fn nanos(time: Option<SystemTime>) -> u128 {
    time.and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos())
}

/// The audio files beside a session (SoundCraft's "Audio Files" folder): their names, sizes and times, in name
/// order, so that replacing one of them makes a new mixdown although the session file itself did not change.
fn siblings(session: &Path) -> Vec<(String, u64, u128)> {
    let Some(folder) = session.parent().map(|parent| parent.join(AUDIO_FOLDER)) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<(String, u64, u128)> = entries
        .filter_map(Result::ok)
        .take(MAX_SIBLINGS)
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            metadata.is_file().then(|| {
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    metadata.len(),
                    nanos(metadata.modified().ok()),
                )
            })
        })
        .collect();
    files.sort();
    files
}

/// The start of the cache names of one document: its name (for people) and a number of its place.
fn name_prefix(absolute: &Path) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    absolute.hash(&mut hasher);
    let stem: String = absolute
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("document")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(40)
        .collect();
    format!("{stem}-{:016x}-", hasher.finish())
}

/// Whether a kept file goes when `kept` has just been made: an older result of the same document (its name starts
/// with `prefix`), or anything not touched for [`KEEP`].
fn outdated(name: &str, age: Option<Duration>, prefix: &str, kept: &str) -> bool {
    if name == kept {
        return false;
    }
    // A part file may be another process's render in progress: only its age removes it.
    let same_document = name.starts_with(prefix) && !name.contains("part-");
    same_document || age.is_some_and(|age| age > KEEP)
}

/// Removes what [`outdated`] names. Failures are left alone (a file in use stays until the next time).
fn prune(folder: &Path, prefix: &str, kept: &Path, now: SystemTime) {
    let kept_name = kept
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.filter_map(Result::ok).take(MAX_SIBLINGS) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let age = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| now.duration_since(modified).ok());
        if outdated(&name, age, prefix, &kept_name) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The cache file path of a document, for tests.
#[cfg(test)]
fn kept_path(folder: &Path, absolute: &Path, version: u64, craft: Craft) -> std::path::PathBuf {
    folder.join(format!(
        "{}{version:016x}.{}",
        name_prefix(absolute),
        craft.extension()
    ))
}

#[cfg(test)]
mod tests {
    use super::{Craft, KEEP, kept_path, name_prefix, outdated, prune, resolved, siblings};
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    /// Files that are not Craft documents are opened as they are.
    #[test]
    fn other_files_are_opened_as_they_are() -> Result<(), Box<dyn std::error::Error>> {
        let path = Path::new("somewhere/sound.wav");
        assert_eq!(resolved(path)?.as_ref(), path);
        assert_eq!(Craft::of(Path::new("a.SCRAFT")), Some(Craft::Session));
        assert_eq!(Craft::of(Path::new("a.ecproj")), Some(Craft::Composition));
        assert_eq!(Craft::of(Path::new("a.mp4")), None);
        Ok(())
    }

    /// A document that is not there is an error, not a crash.
    #[test]
    fn a_missing_document_is_an_error() {
        assert!(resolved(Path::new("definitely-missing-session.scraft")).is_err());
        assert!(resolved(Path::new("definitely-missing-project.ecproj")).is_err());
    }

    /// The names tell documents apart and are safe file names.
    #[test]
    fn the_name_follows_the_document() {
        let session = Path::new("C:/music/My Song (final).scraft");
        let prefix = name_prefix(session);
        assert_eq!(prefix, name_prefix(session));
        assert_ne!(
            prefix,
            name_prefix(Path::new("C:/other/My Song (final).scraft"))
        );
        assert!(prefix.starts_with("MySongfinal-"), "{prefix}");
        let kept = kept_path(Path::new("cache"), session, 7, Craft::Session);
        assert!(kept.to_string_lossy().ends_with("-0000000000000007.wav"));
    }

    /// Two compositions of one project file have cache names of their own, so neither clears the other away.
    #[test]
    fn compositions_of_one_file_keep_their_own_results() {
        let project = Path::new("C:/work/demo.ecproj");
        let first = super::name_prefix(&project.join("Lower Third.ecproj"));
        let second = super::name_prefix(&project.join("3D Showcase.ecproj"));
        assert_ne!(first, second);
        let kept = format!("{first}0000000000000001.mp4");
        // The other composition's file is not an older result of this one.
        assert!(!super::outdated(
            &format!("{second}0000000000000002.mp4"),
            Some(std::time::Duration::from_secs(60)),
            &first,
            &kept
        ));
        assert!(super::outdated(
            &format!("{first}0000000000000000.mp4"),
            Some(std::time::Duration::from_secs(60)),
            &first,
            &kept
        ));
    }

    /// A name under a project file is that file and the composition; anything else is itself.
    #[test]
    fn a_composition_under_a_project_file_is_taken_apart() -> Result<(), Box<dyn std::error::Error>>
    {
        let folder = std::env::temp_dir().join(format!("dc_document_of_{}", std::process::id()));
        std::fs::create_dir_all(&folder)?;
        let project = folder.join("demo.ecproj");
        std::fs::write(&project, b"{}")?;
        let media = project.join("Lower %2F Third 100%25.ecproj");
        let (file, name) = super::document_of(&media);
        assert_eq!(file.as_ref(), project.as_path());
        assert_eq!(name.as_deref(), Some("Lower / Third 100%"));
        assert_eq!(super::document_metadata(&media)?.len(), 2);
        // The project file, another file, and a name under something that is not a project file.
        assert_eq!(
            super::document_of(&project),
            (std::borrow::Cow::Borrowed(project.as_path()), None)
        );
        let other = folder.join("missing.ecproj").join("a.ecproj");
        assert_eq!(super::document_of(&other).1, None);
        let wav = folder.join("a.wav");
        assert_eq!(super::document_of(&wav).1, None);
        std::fs::remove_dir_all(&folder)?;
        Ok(())
    }

    /// A chosen composition is found by the project's full path, and the list is replaced as a whole.
    #[test]
    fn the_chosen_composition_is_found_by_path() {
        let project = std::path::absolute("some-folder/intro.ecproj").unwrap_or_default();
        super::set_compositions(vec![(
            std::path::PathBuf::from("some-folder/intro.ecproj"),
            "Lower Third".to_owned(),
        )]);
        assert_eq!(
            super::chosen_composition(&project).as_deref(),
            Some("Lower Third")
        );
        assert_eq!(super::chosen_composition(Path::new("other.ecproj")), None);
        super::set_compositions(Vec::new());
        assert_eq!(super::chosen_composition(&project), None);
    }

    /// Two writers never share a part file (the length and the waveform of a session used to collide in one
    /// process and both mixdowns failed).
    #[test]
    fn part_files_are_not_shared() {
        let first = super::part_name("Song-00aa-", 10, 0, "wav");
        assert_ne!(first, super::part_name("Song-00aa-", 10, 1, "wav"));
        assert_ne!(first, super::part_name("Song-00aa-", 11, 0, "wav"));
        assert!(first.contains("part-"), "{first}");
    }

    /// What goes when a new result is kept: the document's older results and month-old files; not the new
    /// result, other documents' fresh results, or a fresh part file.
    #[test]
    fn older_results_and_stale_files_go() {
        let day = Duration::from_secs(24 * 60 * 60);
        let (prefix, kept) = ("Song-00aa-", "Song-00aa-0002.wav");
        assert!(!outdated(kept, Some(day), prefix, kept));
        assert!(outdated("Song-00aa-0001.wav", Some(day), prefix, kept));
        assert!(!outdated("Other-00bb-0001.wav", Some(day), prefix, kept));
        assert!(outdated(
            "Other-00bb-0001.wav",
            Some(KEEP + day),
            prefix,
            kept
        ));
        assert!(!outdated("Song-00aa-part-123.wav", Some(day), prefix, kept));
        assert!(outdated(
            "Song-00aa-part-123.wav",
            Some(KEEP + day),
            prefix,
            kept
        ));
        assert!(!outdated("Other-00bb-0001.wav", None, prefix, kept));
    }

    /// On disk: pruning removes the document's older result and leaves the rest; the audio files beside a
    /// session are part of what tells its versions apart.
    #[test]
    fn pruning_and_siblings_on_disk() -> Result<(), Box<dyn std::error::Error>> {
        let dir =
            std::env::temp_dir().join(format!("disccraft-session-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Audio Files"))?;
        let session = dir.join("song.scraft");
        assert!(siblings(&session).is_empty());
        std::fs::write(dir.join("Audio Files").join("b.wav"), [0_u8; 4])?;
        std::fs::write(dir.join("Audio Files").join("a.wav"), [0_u8; 9])?;
        let listed = siblings(&session);
        let names: Vec<(&str, u64)> = listed.iter().map(|(n, s, _)| (n.as_str(), *s)).collect();
        assert_eq!(names, [("a.wav", 9), ("b.wav", 4)]);

        let cache = dir.join("cache");
        std::fs::create_dir_all(&cache)?;
        let prefix = name_prefix(&session);
        let old = cache.join(format!("{prefix}0001.wav"));
        let new = cache.join(format!("{prefix}0002.wav"));
        let other = cache.join("Other-00bb-0001.wav");
        for file in [&old, &new, &other] {
            std::fs::write(file, [0_u8; 1])?;
        }
        prune(&cache, &prefix, &new, SystemTime::now());
        assert!(!old.exists() && new.exists() && other.exists());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
