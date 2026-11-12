use crate::model::Project;
use serde_json::{Map, Value};
use std::fs;
use std::io::Write;
use std::path::Path;

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
    let project = serde_json::to_value(p).map_err(|e| ProjectError::Json(e.to_string()))?;
    let mut root = Map::new();
    root.insert("format".to_string(), Value::String(FORMAT.to_string()));
    root.insert("version".to_string(), Value::from(SCHEMA_VERSION));
    root.insert("project".to_string(), project);
    serde_json::to_string_pretty(&Value::Object(root))
        .map_err(|e| ProjectError::Json(e.to_string()))
}

/// Deserialize a project from the canonical JSON envelope.
pub fn from_json(s: &str) -> Result<Project, ProjectError> {
    let mut value: Value =
        serde_json::from_str(s).map_err(|e| ProjectError::Json(e.to_string()))?;

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

/// Atomically save a project to a path.
pub fn save(p: &Project, path: &Path) -> Result<(), ProjectError> {
    let json = to_json(p)?;
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
    from_json(&text)
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
    use super::{ProjectError, from_json, load, save, to_json};
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
