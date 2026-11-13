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
    from_value(value)
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

/// Writes each asset's path relative to `folder` ("relativePath"; left out when there is none, such as a file on
/// another drive).
fn add_relative_paths(value: &mut Value, folder: &Path) {
    for asset in assets_mut(value) {
        let relative = asset
            .get("path")
            .and_then(Value::as_str)
            .and_then(|path| relative_path(folder, Path::new(path)));
        match relative {
            Some(relative) => {
                asset.insert("relativePath".to_string(), Value::String(relative));
            }
            None => {
                let _ = asset.remove("relativePath");
            }
        }
    }
}

/// An asset whose recorded path is gone but whose relative path from `folder` names a file takes that file (and is
/// no longer offline): the project was moved together with its media.
fn find_moved_assets(value: &mut Value, folder: &Path) {
    for asset in assets_mut(value) {
        let gone = asset
            .get("path")
            .and_then(Value::as_str)
            .is_none_or(|path| !Path::new(path).exists());
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
        asset.insert(
            "path".to_string(),
            Value::String(found.to_string_lossy().into_owned()),
        );
        asset.insert("offline".to_string(), Value::Bool(false));
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
