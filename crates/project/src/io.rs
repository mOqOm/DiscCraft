use crate::model::Project;
use serde_json::{Map, Value};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

/// File format marker.
pub const FORMAT: &str = "disccraft-project";
/// Current schema version.
pub const SCHEMA_VERSION: u32 = 1;
/// Maximum accepted project file size in bytes.
pub const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// Errors produced by project operations.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ProjectError {
    /// Filesystem error.
    #[error("io error at {path}: {message}")]
    Io {
        /// Path involved.
        path: String,
        /// OS error message.
        message: String,
    },
    /// JSON serialization or deserialization error.
    #[error("json error: {0}")]
    Json(String),
    /// File format marker is missing or wrong.
    #[error("wrong project format: {0}")]
    WrongFormat(String),
    /// File schema version is newer than supported.
    #[error("unsupported project schema version: {0}")]
    UnsupportedVersion(u32),
    /// File is larger than [`MAX_FILE_BYTES`].
    #[error("project file is too large: {0} bytes")]
    TooLarge(u64),
    /// Shared id space is exhausted.
    #[error("project id space is exhausted")]
    IdOverflow,
    /// Timecode parsing failed.
    #[error("invalid timecode: {0}")]
    Timecode(String),
    /// Invalid project operation or state.
    #[error("invalid project: {0}")]
    Invalid(String),
}

/// Serialize a project to the canonical JSON envelope.
pub fn to_json(p: &Project) -> Result<String, ProjectError> {
    pretty(&envelope(p)?)
}

/// The canonical JSON envelope of a project.
fn envelope(p: &Project) -> Result<Value, ProjectError> {
    let project = serde_json::to_value(p).map_err(|e| ProjectError::Json(e.to_string()))?;
    let mut root = Map::new();
    root.insert("format".to_string(), Value::String(FORMAT.to_string()));
    root.insert("version".to_string(), Value::from(SCHEMA_VERSION));
    root.insert("project".to_string(), project);
    Ok(Value::Object(root))
}

fn pretty(value: &Value) -> Result<String, ProjectError> {
    serde_json::to_string_pretty(value).map_err(|e| ProjectError::Json(e.to_string()))
}

/// Deserialize a project from the canonical JSON envelope.
pub fn from_json(s: &str) -> Result<Project, ProjectError> {
    let value: Value = serde_json::from_str(s).map_err(|e| ProjectError::Json(e.to_string()))?;
    from_value(value)
}

/// [`from_json`] from an already parsed envelope.
fn from_value(mut value: Value) -> Result<Project, ProjectError> {
    let format = value
        .get("format")
        .and_then(Value::as_str)
        .ok_or_else(|| ProjectError::WrongFormat("missing format".to_string()))?;
    if format != FORMAT {
        return Err(ProjectError::WrongFormat(format.to_string()));
    }

    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProjectError::WrongFormat("missing version".to_string()))?;
    if version > u64::from(SCHEMA_VERSION) {
        return Err(ProjectError::UnsupportedVersion(version as u32));
    }
    let version = version as u32;

    migrate(&mut value, version)?;

    let project = value
        .get("project")
        .cloned()
        .ok_or_else(|| ProjectError::WrongFormat("missing project".to_string()))?;
    serde_json::from_value(project).map_err(|e| ProjectError::Json(e.to_string()))
}

/// Atomically save a project to a path. Each asset's path is also written relative to the project file's folder
/// ("relativePath"), so a project moved or copied together with its media (a zip of a sample folder, another
/// computer) still finds them; the absolute "path" stays first, as before.
pub fn save(p: &Project, path: &Path) -> Result<(), ProjectError> {
    let mut value = envelope(p)?;
    if let Some(folder) = folder_of(path) {
        add_relative_paths(&mut value, &folder);
    }
    let json = pretty(&value)?;
    let base = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project.dcproj".to_string());
    let tmp = path.with_file_name(format!("{base}.tmp{}", std::process::id()));

    let result = write_atomic(&json, &tmp, path);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Load a project from a path.
pub fn load(path: &Path) -> Result<Project, ProjectError> {
    let metadata = fs::metadata(path).map_err(|e| io_error(path, e))?;
    let len = metadata.len();
    if len > MAX_FILE_BYTES {
        return Err(ProjectError::TooLarge(len));
    }

    let bytes = fs::read(path).map_err(|e| io_error(path, e))?;
    let text = String::from_utf8(bytes).map_err(|e| ProjectError::Json(e.to_string()))?;
    let mut value: Value =
        serde_json::from_str(&text).map_err(|e| ProjectError::Json(e.to_string()))?;
    if let Some(folder) = folder_of(path) {
        find_moved_assets(&mut value, &folder);
    }
    let mut project = from_value(value)?;
    // A menu's PSD a project from before M6.399 lists among its assets is the menu's own file (M6.400).
    project.mark_menu_files();
    Ok(project)
}

/// Longest relative path accepted from a project file.
const MAX_RELATIVE_PATH: usize = 4096;

/// The folder a project file is in (made absolute against the current folder).
fn folder_of(path: &Path) -> Option<PathBuf> {
    std::path::absolute(path)
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

/// The asset objects of a project envelope.
fn assets_mut(value: &mut Value) -> Vec<&mut Map<String, Value>> {
    match value
        .get_mut("project")
        .and_then(|project| project.get_mut("assets"))
    {
        Some(Value::Object(assets)) => assets
            .values_mut()
            .filter_map(Value::as_object_mut)
            .collect(),
        Some(Value::Array(assets)) => assets.iter_mut().filter_map(Value::as_object_mut).collect(),
        _ => Vec::new(),
    }
}

/// The disc settings object of a project envelope.
fn settings_mut(value: &mut Value) -> Option<&mut Map<String, Value>> {
    value
        .get_mut("project")?
        .get_mut("settings")?
        .as_object_mut()
}

/// Writes each asset's path relative to `folder` ("relativePath"; left out when there is none, such as a file on
/// another drive), and the disc label's ("relativeDiscLabel", M6.509).
fn add_relative_paths(value: &mut Value, folder: &Path) {
    if let Some(settings) = settings_mut(value) {
        set_relative(settings, "discLabel", "relativeDiscLabel", folder);
    }
    for asset in assets_mut(value) {
        set_relative(asset, "path", "relativePath", folder);
        // The transcoded files located for the asset (Locate Transcoded File), likewise.
        for key in LOCATED_KEYS {
            if let Some(located) = asset.get_mut(key).and_then(Value::as_object_mut) {
                set_relative(located, "video", "relativeVideo", folder);
                set_relative(located, "audio", "relativeAudio", folder);
            }
        }
    }
}

/// The asset fields that hold located transcoded files.
const LOCATED_KEYS: [&str; 2] = ["locatedDvd", "locatedBluray"];

/// Writes the path in `object[key]` relative to `folder` as `object[relative_key]` (left out when there is none).
fn set_relative(object: &mut Map<String, Value>, key: &str, relative_key: &str, folder: &Path) {
    let relative = object
        .get(key)
        .and_then(Value::as_str)
        .and_then(|path| relative_path(folder, Path::new(path)));
    match relative {
        Some(relative) => {
            object.insert(relative_key.to_string(), Value::String(relative));
        }
        None => {
            let _ = object.remove(relative_key);
        }
    }
}

/// Points `object[key]` at the file its relative path from `folder` names, when the recorded file is gone and that
/// one is there.
fn find_moved(object: &mut Map<String, Value>, key: &str, relative_key: &str, folder: &Path) {
    let gone = object
        .get(key)
        .and_then(Value::as_str)
        .is_some_and(|path| !Path::new(path).exists());
    if !gone {
        return;
    }
    if let Some(found) = object
        .get(relative_key)
        .and_then(Value::as_str)
        .and_then(|relative| resolve_relative(folder, relative))
        .filter(|candidate| candidate.is_file())
    {
        object.insert(
            key.to_string(),
            Value::String(found.to_string_lossy().into_owned()),
        );
    }
}

/// An asset whose recorded path is gone but whose relative path from `folder` names a file takes that file (and is
/// no longer offline): the project was moved together with its media. The pictures of its picture subtitles (an
/// image script's) that are gone too are looked for in the same place relative to the moved file.
fn find_moved_assets(value: &mut Value, folder: &Path) {
    // The disc label (M6.509), likewise.
    if let Some(settings) = settings_mut(value) {
        find_moved(settings, "discLabel", "relativeDiscLabel", folder);
    }
    for asset in assets_mut(value) {
        // Located transcoded files are looked for whether or not the source is (it is often gone on purpose).
        for key in LOCATED_KEYS {
            if let Some(located) = asset.get_mut(key).and_then(Value::as_object_mut) {
                find_moved(located, "video", "relativeVideo", folder);
                find_moved(located, "audio", "relativeAudio", folder);
            }
        }
        let old = asset.get("path").and_then(Value::as_str).map(PathBuf::from);
        let gone = old.as_ref().is_none_or(|path| !path.exists());
        if !gone {
            continue;
        }
        let Some(found) = asset
            .get("relativePath")
            .and_then(Value::as_str)
            .and_then(|relative| resolve_relative(folder, relative))
            .filter(|candidate| candidate.is_file())
        else {
            continue;
        };
        if let (Some(old), Some(cues)) = (old, asset.get_mut("cues").and_then(Value::as_array_mut))
        {
            move_cue_pictures(cues, &old, &found);
        }
        asset.insert(
            "path".to_string(),
            Value::String(found.to_string_lossy().into_owned()),
        );
        asset.insert("offline".to_string(), Value::Bool(false));
    }
}

/// Points the pictures of picture subtitles whose files are gone at the files that stand where they stood
/// relative to their subtitle file, now that the subtitle file moved from `old` to `new`.
fn move_cue_pictures(cues: &mut [Value], old: &Path, new: &Path) {
    let (Some(old_folder), Some(new_folder)) = (old.parent(), new.parent()) else {
        return;
    };
    for cue in cues {
        let Some(image) = cue.get_mut("image").and_then(Value::as_object_mut) else {
            continue;
        };
        let moved = image
            .get("path")
            .and_then(Value::as_str)
            .map(Path::new)
            .filter(|picture| !picture.exists())
            .and_then(|picture| relative_path(old_folder, picture))
            .and_then(|relative| resolve_relative(new_folder, &relative))
            .filter(|candidate| candidate.is_file());
        if let Some(moved) = moved {
            image.insert(
                "path".to_string(),
                Value::String(moved.to_string_lossy().into_owned()),
            );
        }
    }
}

/// `target` relative to the folder `base`, with forward slashes ("media/clip.m2v", "../shared/logo.png"). `None`
/// when either is not absolute or they are on different drives.
pub(crate) fn relative_path(base: &Path, target: &Path) -> Option<String> {
    if !base.is_absolute() || !target.is_absolute() {
        return None;
    }
    let base: Vec<Component<'_>> = base.components().collect();
    let target: Vec<Component<'_>> = target.components().collect();
    // Windows paths do not care about case.
    let same = |a: &Component<'_>, b: &Component<'_>| {
        let text = |c: &Component<'_>| c.as_os_str().to_string_lossy().into_owned();
        if cfg!(windows) {
            text(a).eq_ignore_ascii_case(&text(b))
        } else {
            a == b
        }
    };
    match (base.first(), target.first()) {
        (Some(a), Some(b)) if same(a, b) => {}
        _ => return None,
    }
    let common = base
        .iter()
        .zip(&target)
        .take_while(|(a, b)| same(a, b))
        .count();
    let mut parts: Vec<String> = Vec::new();
    for component in base.get(common..)? {
        match component {
            Component::Normal(_) => parts.push("..".to_string()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    for component in target.get(common..)? {
        match component {
            Component::Normal(name) => parts.push(name.to_string_lossy().into_owned()),
            Component::ParentDir => parts.push("..".to_string()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// `relative` (forward slashes) from the folder `base`. `None` for anything that is not a plain relative path
/// (a drive, a root, a backslash) or climbs above the root.
fn resolve_relative(base: &Path, relative: &str) -> Option<PathBuf> {
    if relative.is_empty()
        || relative.len() > MAX_RELATIVE_PATH
        || relative.starts_with('/')
        || relative.contains(['\\', ':'])
    {
        return None;
    }
    let mut path = base.to_path_buf();
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if !path.pop() {
                    return None;
                }
            }
            name => path.push(name),
        }
    }
    Some(path)
}

fn migrate(_value: &mut Value, version: u32) -> Result<(), ProjectError> {
    match version {
        1 => Ok(()),
        other => Err(ProjectError::UnsupportedVersion(other)),
    }
}

fn write_atomic(json: &str, tmp: &Path, target: &Path) -> Result<(), ProjectError> {
    let mut file = fs::File::create(tmp).map_err(|e| io_error(tmp, e))?;
    file.write_all(json.as_bytes())
        .map_err(|e| io_error(tmp, e))?;
    file.sync_all().map_err(|e| io_error(tmp, e))?;
    drop(file);
    fs::rename(tmp, target).map_err(|e| io_error(target, e))?;
    Ok(())
}

fn io_error(path: &Path, error: std::io::Error) -> ProjectError {
    ProjectError::Io {
        path: path.to_string_lossy().into_owned(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{ProjectError, from_json, load, relative_path, resolve_relative, save, to_json};
    use crate::Project;
    use std::error::Error;
    use std::fs;

    #[test]
    fn json_round_trip() -> Result<(), Box<dyn Error>> {
        let p = Project::default();
        let json = to_json(&p)?;
        let back = from_json(&json)?;
        assert_eq!(back, p);
        Ok(())
    }

    #[test]
    fn version_and_format_errors() -> Result<(), Box<dyn Error>> {
        let version_err = from_json(r#"{"format":"disccraft-project","version":2,"project":{}}"#);
        assert!(matches!(
            version_err,
            Err(ProjectError::UnsupportedVersion(2))
        ));

        let format_err = from_json(r#"{"format":"x","version":1,"project":{}}"#);
        assert!(matches!(format_err, Err(ProjectError::WrongFormat(_))));
        Ok(())
    }

    #[test]
    fn truncated_files_are_errors() -> Result<(), Box<dyn Error>> {
        let valid = to_json(&Project::default())?;
        assert!(!valid.is_empty());

        let mut i = 0;
        while i < valid.len() {
            let mut end = i.saturating_add(7);
            if end > valid.len() {
                end = valid.len();
            }
            while end < valid.len() && !valid.is_char_boundary(end) {
                end = end.saturating_add(1);
            }
            if end >= valid.len() {
                break;
            }
            assert!(from_json(&valid[..end]).is_err(), "prefix {end} parsed");
            if end == i {
                break;
            }
            i = end;
        }
        Ok(())
    }

    /// A fresh folder in the temp folder.
    fn temp_folder(name: &str) -> Result<std::path::PathBuf, Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!(
            "disccraft-project-io-{}-{name}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    #[test]
    fn a_moved_project_finds_its_media() -> Result<(), Box<dyn Error>> {
        let root = temp_folder("moved")?;
        let first = root.join("sample");
        fs::create_dir_all(first.join("media"))?;
        let clip = first.join("media").join("clip.png");
        fs::write(&clip, b"not really a picture")?;
        let mut project = Project::default();
        let asset = project.add_asset(&clip)?;
        let file = first.join("sample.dcproj");
        save(&project, &file)?;
        let text = fs::read_to_string(&file)?;
        assert!(
            text.contains("\"relativePath\": \"media/clip.png\""),
            "{text}"
        );

        // Moved (renamed) together with its media: the old path is gone, the relative one is found.
        let moved = root.join("elsewhere");
        fs::rename(&first, &moved)?;
        let loaded = load(&moved.join("sample.dcproj"))?;
        let found = loaded.assets.get(&asset).ok_or("asset")?;
        assert_eq!(
            std::path::Path::new(&found.path),
            moved.join("media").join("clip.png")
        );
        assert!(!found.offline);

        // A copy whose original is still there keeps the recorded path (as Premiere and Encore do).
        let copy = root.join("copy");
        fs::create_dir_all(copy.join("media"))?;
        fs::copy(
            moved.join("media").join("clip.png"),
            copy.join("media").join("clip.png"),
        )?;
        save(&loaded, &moved.join("sample.dcproj"))?;
        fs::copy(moved.join("sample.dcproj"), copy.join("sample.dcproj"))?;
        let copied = load(&copy.join("sample.dcproj"))?;
        assert_eq!(
            copied.assets.get(&asset).map(|asset| asset.path.clone()),
            Some(found.path.clone())
        );
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    /// M6.509: the disc label is saved with its path relative to the project, and found again when the project is
    /// moved together with it; a label on another drive (no relative path) or gone keeps the recorded path.
    #[test]
    fn a_moved_project_finds_its_disc_label() -> Result<(), Box<dyn Error>> {
        let root = temp_folder("moved-label")?;
        let first = root.join("sample");
        fs::create_dir_all(first.join("Labels"))?;
        let label = first.join("Labels").join("sample Disc Label.designcraft");
        fs::write(&label, b"a label")?;
        let mut project = Project::default();
        project.settings.disc_label = Some(label.to_string_lossy().into_owned());
        save(&project, &first.join("sample.dcproj"))?;
        let text = fs::read_to_string(first.join("sample.dcproj"))?;
        assert!(
            text.contains("\"relativeDiscLabel\": \"Labels/sample Disc Label.designcraft\""),
            "{text}"
        );
        let moved = root.join("elsewhere");
        fs::rename(&first, &moved)?;
        let loaded = load(&moved.join("sample.dcproj"))?;
        assert_eq!(
            loaded
                .settings
                .disc_label
                .as_deref()
                .map(std::path::Path::new),
            Some(
                moved
                    .join("Labels")
                    .join("sample Disc Label.designcraft")
                    .as_path()
            )
        );
        // Saved again: still relative, and nothing left over from the old folder.
        save(&loaded, &moved.join("sample.dcproj"))?;
        let again = fs::read_to_string(moved.join("sample.dcproj"))?;
        assert!(
            again.contains("\"relativeDiscLabel\": \"Labels/sample Disc Label.designcraft\""),
            "{again}"
        );

        // Gone from the moved folder too: the recorded path stays (the menu says it is missing).
        fs::remove_file(moved.join("Labels").join("sample Disc Label.designcraft"))?;
        let gone = load(&moved.join("sample.dcproj"))?;
        assert_eq!(gone.settings.disc_label, loaded.settings.disc_label);

        // No label: nothing written.
        save(&Project::default(), &moved.join("plain.dcproj"))?;
        let plain = fs::read_to_string(moved.join("plain.dcproj"))?;
        assert!(
            !plain.contains("discLabel") && !plain.contains("DiscLabel"),
            "{plain}"
        );
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    /// Located transcoded files are found again in a moved project, even when the source is long gone.
    #[test]
    fn a_moved_project_finds_its_located_transcodes() -> Result<(), Box<dyn Error>> {
        let root = temp_folder("moved-located")?;
        let first = root.join("sample");
        fs::create_dir_all(first.join("Transcodes"))?;
        let (video, audio) = (
            first.join("Transcodes").join("movie.m2v"),
            first.join("Transcodes").join("movie.ac3"),
        );
        fs::write(&video, b"video")?;
        fs::write(&audio, b"audio")?;
        let mut project = Project::default();
        let asset = project.add_asset(&root.join("deleted-source.mp4"))?;
        project.assets.get_mut(&asset).ok_or("asset")?.located_dvd =
            Some(crate::LocatedTranscode {
                video: video.to_string_lossy().into_owned(),
                audio: Some(audio.to_string_lossy().into_owned()),
            });
        save(&project, &first.join("sample.dcproj"))?;
        let text = fs::read_to_string(first.join("sample.dcproj"))?;
        assert!(
            text.contains("\"relativeVideo\": \"Transcodes/movie.m2v\""),
            "{text}"
        );
        let moved = root.join("elsewhere");
        fs::rename(&first, &moved)?;
        let loaded = load(&moved.join("sample.dcproj"))?;
        let located = loaded
            .assets
            .get(&asset)
            .and_then(|asset| asset.located_dvd.clone())
            .ok_or("located")?;
        assert_eq!(
            std::path::Path::new(&located.video),
            moved.join("Transcodes").join("movie.m2v")
        );
        assert_eq!(
            located.audio.as_deref().map(std::path::Path::new),
            Some(moved.join("Transcodes").join("movie.ac3").as_path())
        );
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    /// A moved image script finds its pictures where they stood beside it.
    #[test]
    fn a_moved_image_script_finds_its_pictures() -> Result<(), Box<dyn Error>> {
        let root = temp_folder("moved-pictures")?;
        let first = root.join("sample");
        fs::create_dir_all(first.join("subs").join("pics"))?;
        let script = first.join("subs").join("script.txt");
        let picture = first.join("subs").join("pics").join("001.png");
        fs::write(&script, b"1 00;00;01;00 00;00;02;00 001.png\n")?;
        fs::write(&picture, b"a picture")?;
        let mut project = Project::default();
        let asset = project.add_asset(&script)?;
        let image = crate::CueImage {
            path: picture.to_string_lossy().into_owned(),
            area: [0, 0, 720, 480],
            colors: crate::ImageCueColors {
                background: [0, 0, 255],
                fill: [255, 255, 255],
                outline: [0, 0, 0],
            },
        };
        project.assets.get_mut(&asset).ok_or("asset")?.cues = Some(vec![crate::SubtitleCue {
            start: crate::Tick(0),
            end: crate::Tick(10),
            text: String::new(),
            look: crate::CueLook::default(),
            image: Some(image),
        }]);
        save(&project, &first.join("sample.dcproj"))?;
        let moved = root.join("elsewhere");
        fs::rename(&first, &moved)?;
        let loaded = load(&moved.join("sample.dcproj"))?;
        let path = loaded
            .assets
            .get(&asset)
            .and_then(|asset| asset.cues.as_ref())
            .and_then(|cues| cues.first())
            .and_then(|cue| cue.image.as_ref())
            .map(|image| image.path.clone())
            .ok_or("picture")?;
        assert_eq!(
            std::path::Path::new(&path),
            moved.join("subs").join("pics").join("001.png")
        );
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn relative_paths_climb_and_refuse_odd_ones() -> Result<(), Box<dyn Error>> {
        let base = temp_folder("relative")?;
        let project = base.join("projects").join("one");
        let shared = base.join("shared").join("logo.png");
        assert_eq!(
            relative_path(&project, &shared).as_deref(),
            Some("../../shared/logo.png")
        );
        assert_eq!(
            relative_path(&project, std::path::Path::new("logo.png")),
            None
        );
        if cfg!(windows) {
            assert_eq!(
                relative_path(
                    std::path::Path::new("C:\\a"),
                    std::path::Path::new("D:\\b\\c.png")
                ),
                None
            );
        }
        assert_eq!(
            resolve_relative(&project, "../../shared/logo.png"),
            Some(shared.clone())
        );
        for odd in ["C:/x.png", "/x.png", "a\\b.png", ""] {
            assert_eq!(resolve_relative(&project, odd), None, "{odd}");
        }
        fs::remove_dir_all(&base)?;
        Ok(())
    }

    #[test]
    fn save_load_round_trip() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!(
            "disccraft-project-io-{}-save_load",
            std::process::id()
        ));
        fs::create_dir_all(&dir)?;
        let path = dir.join("movie.dcproj");

        let p = Project::default();
        save(&p, &path)?;
        let loaded = load(&path)?;
        assert_eq!(loaded, p);

        let mut temp_left = false;
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().contains(".tmp") {
                temp_left = true;
            }
        }
        assert!(!temp_left);

        fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
