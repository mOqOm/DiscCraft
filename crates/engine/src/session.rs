use crate::CommandResult;
use crate::commands::Preview;
use crate::commands::find_command;
use disccraft_project::{Project, Severity, has_errors, load, save as project_save};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// Maximum number of undo snapshots retained.
pub const UNDO_LIMIT: usize = 100;

/// Interactive editing session.
#[derive(Default)]
pub struct Session {
    /// Current project document.
    pub project: Project,
    /// Path of the loaded or saved project file.
    pub path: Option<PathBuf>,
    /// Whether the project has unsaved changes.
    pub dirty: bool,
    /// VM preview of a built disc (not part of the document, not undoable).
    pub(crate) preview: Preview,
    undo: Vec<Project>,
    redo: Vec<Project>,
    /// The label of the command behind each undo snapshot (same order as `undo`).
    undo_labels: Vec<&'static str>,
    /// The label of the command behind each redo snapshot (same order as `redo`).
    redo_labels: Vec<&'static str>,
}

impl Session {
    /// Creates a new empty session.
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens a project file.
    pub fn open(path: &Path) -> Result<Self, String> {
        let project = load(path).map_err(|error| error.to_string())?;
        Ok(Self {
            project,
            path: Some(path.to_path_buf()),
            dirty: false,
            preview: Preview::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            undo_labels: Vec::new(),
            redo_labels: Vec::new(),
        })
    }

    /// Saves the project to the given path, or to the current path.
    pub fn save(&mut self, path: Option<&Path>) -> Result<PathBuf, String> {
        let target = match path {
            Some(path) => path.to_path_buf(),
            None => self
                .path
                .clone()
                .ok_or_else(|| "no path known".to_string())?,
        };

        project_save(&self.project, &target).map_err(|error| error.to_string())?;
        self.path = Some(target.clone());
        self.dirty = false;
        Ok(target)
    }

    /// Executes a command by id.
    pub fn execute(&mut self, id: &str, params: &Value) -> CommandResult {
        let Some(spec) = find_command(id) else {
            return Err(format!("unknown command: {id}"));
        };

        if let Err(reason) = (spec.enabled)(self) {
            return Err(format!("{}: {}", spec.id, reason));
        }

        let empty = Value::Object(Map::new());
        let params = if params.is_null() { &empty } else { params };

        if spec.journal {
            let snapshot = self.project.clone();
            let result = (spec.run)(self, params);

            match result {
                Ok(value) => {
                    // Reject only errors this command introduced, so a project loaded with
                    // existing errors can still be edited (and repaired).
                    let before = snapshot.check();
                    let new_errors: Vec<_> = self
                        .project
                        .check()
                        .into_iter()
                        .filter(|p| p.severity == Severity::Error && !before.contains(p))
                        .collect();
                    if has_errors(&new_errors) {
                        self.project = snapshot;
                        let details: Vec<String> = new_errors
                            .iter()
                            .map(|problem| format!("{}: {}", problem.code, problem.message))
                            .collect();
                        return Err(format!(
                            "{}: validation failed: {}",
                            spec.id,
                            details.join("; ")
                        ));
                    }

                    if self.project != snapshot {
                        self.undo.push(snapshot);
                        self.undo_labels.push(spec.label);
                        if self.undo.len() > UNDO_LIMIT {
                            self.undo.remove(0);
                            if !self.undo_labels.is_empty() {
                                let _ = self.undo_labels.remove(0);
                            }
                        }
                        self.redo.clear();
                        self.redo_labels.clear();
                        self.dirty = true;
                    }

                    Ok(value)
                }
                Err(message) => {
                    self.project = snapshot;
                    Err(message)
                }
            }
        } else {
            (spec.run)(self, params)
        }
    }

    /// Returns true when an undo snapshot is available.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Returns true when a redo snapshot is available.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The label of the command the next undo takes back ("ボタンを追加"), like Encore's "〜の取り消し".
    pub fn undo_label(&self) -> Option<&'static str> {
        self.undo_labels.last().copied()
    }

    /// The label of the command the next redo applies again.
    pub fn redo_label(&self) -> Option<&'static str> {
        self.redo_labels.last().copied()
    }

    /// The labels of the steps that can be undone, oldest first (the last is the next undo).
    pub fn undo_labels(&self) -> &[&'static str] {
        &self.undo_labels
    }

    /// The labels of the steps that can be redone, farthest first (the last is the next redo).
    pub fn redo_labels(&self) -> &[&'static str] {
        &self.redo_labels
    }

    /// How many steps can be undone.
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// How many steps can be redone.
    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// Restores the previous project snapshot.
    pub fn undo(&mut self) -> Result<(), String> {
        let Some(previous) = self.undo.pop() else {
            return Err("nothing to undo".to_string());
        };

        self.redo.push(self.project.clone());
        if let Some(label) = self.undo_labels.pop() {
            self.redo_labels.push(label);
        }
        self.project = previous;
        self.dirty = true;
        Ok(())
    }

    /// Re-applies the next project snapshot.
    pub fn redo(&mut self) -> Result<(), String> {
        let Some(next) = self.redo.pop() else {
            return Err("nothing to redo".to_string());
        };

        self.undo.push(self.project.clone());
        if let Some(label) = self.redo_labels.pop() {
            self.undo_labels.push(label);
        }
        self.project = next;
        self.dirty = true;
        Ok(())
    }

    /// Replaces the project and clears undo/redo history.
    pub(crate) fn reset(&mut self, project: Project) {
        self.project = project;
        self.undo.clear();
        self.redo.clear();
        self.undo_labels.clear();
        self.redo_labels.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::Session;
    use disccraft_project::{FrameRate, Link};
    use serde_json::{Map, Value};
    use std::error::Error;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn temp_dir(name: &str) -> Result<PathBuf, Box<dyn Error>> {
        let mut path = std::env::temp_dir();
        path.push(format!("disccraft-engine-{}-{name}", std::process::id()));
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    fn string_value(path: &Path) -> Value {
        Value::String(path.to_string_lossy().into_owned())
    }

    fn obj(entries: &[(&str, Value)]) -> Value {
        let mut map = Map::new();
        for (key, value) in entries {
            map.insert((*key).to_string(), (*value).clone());
        }
        Value::Object(map)
    }

    #[test]
    fn remove_clears_links_and_undo_restores() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("remove_flow")?;
        let movie = dir.join("movie.mp4");
        fs::write(&movie, b"fake")?;

        let mut session = Session::new();
        session.execute("asset.import", &obj(&[("path", string_value(&movie))]))?;
        session.execute("timeline.create", &obj(&[("asset", Value::from(1_u64))]))?;
        session.execute(
            "nav.setFirstPlay",
            &obj(&[("target", serde_json::json!({ "timeline": 2 }))]),
        )?;

        // The asset is in use by the timeline and cannot be removed.
        let in_use = session.execute(
            "project.remove",
            &obj(&[("kind", Value::from("asset")), ("id", Value::from(1_u64))]),
        );
        assert!(in_use.is_err());

        session.execute(
            "project.remove",
            &obj(&[
                ("kind", Value::from("timeline")),
                ("id", Value::from(2_u64)),
            ]),
        )?;
        assert!(session.project.timelines.is_empty());
        assert_eq!(session.project.settings.first_play, Link::NotSet);

        session.execute("edit.undo", &Value::Null)?;
        assert_eq!(session.project.timelines.len(), 1);
        assert_ne!(session.project.settings.first_play, Link::NotSet);
        Ok(())
    }

    #[test]
    fn full_flow() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("full_flow")?;
        let movie = dir.join("movie.mp4");
        fs::write(&movie, b"fake")?;

        let mut session = Session::new();

        let import = session.execute("asset.import", &obj(&[("path", string_value(&movie))]))?;
        assert_eq!(import.get("asset").and_then(Value::as_u64), Some(1));

        let create = session.execute("timeline.create", &obj(&[("asset", Value::from(1_u64))]))?;
        assert_eq!(create.get("timeline").and_then(Value::as_u64), Some(2));

        let add = session.execute(
            "timeline.addChapter",
            &obj(&[
                ("timeline", Value::from(2_u64)),
                ("timecode", Value::String("00:10:00;00".to_string())),
                ("name", Value::String("第2章".to_string())),
            ]),
        )?;
        assert_eq!(add.get("chapter").and_then(Value::as_u64), Some(1));
        assert_eq!(
            add.get("time").and_then(Value::as_i64),
            Some(FrameRate::FPS_29_97.tick_of(17982).0)
        );

        let save_path = dir.join("movie.dcproj");
        let saved = session.execute("file.save", &obj(&[("path", string_value(&save_path))]))?;
        assert_eq!(
            saved.get("path").and_then(Value::as_str),
            Some(save_path.to_string_lossy().as_ref())
        );

        let mut reopened = Session::open(&save_path)?;
        let list_reopened = reopened.execute("timeline.list", &Value::Null)?;
        let list_session = session.execute("timeline.list", &Value::Null)?;
        assert_eq!(list_reopened, list_session);

        fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn undo_and_redo_know_their_command() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        assert_eq!(session.undo_label(), None);
        session.execute("menu.create", &obj(&[]))?;
        session.execute("disc.setName", &serde_json::json!({"name": "Trip"}))?;
        assert_eq!(session.undo_label(), Some("ディスクの名前を設定"));
        session.undo()?;
        assert_eq!(session.undo_label(), Some("メニューを作成"));
        assert_eq!(session.redo_label(), Some("ディスクの名前を設定"));
        session.redo()?;
        assert_eq!(session.undo_label(), Some("ディスクの名前を設定"));
        assert_eq!(session.redo_label(), None);
        Ok(())
    }

    #[test]
    fn undo_redo_and_new_edit_clears_redo() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("undo_redo")?;
        let movie = dir.join("movie.mp4");
        fs::write(&movie, b"fake")?;

        let mut session = Session::new();
        session.execute("asset.import", &obj(&[("path", string_value(&movie))]))?;
        session.execute("timeline.create", &obj(&[("asset", Value::from(1_u64))]))?;

        assert_eq!(session.project.assets.len(), 1);
        assert_eq!(session.project.timelines.len(), 1);

        session.undo()?;
        session.undo()?;
        assert!(session.project.assets.is_empty());
        assert!(session.project.timelines.is_empty());

        session.redo()?;
        session.redo()?;
        assert_eq!(session.project.assets.len(), 1);
        assert_eq!(session.project.timelines.len(), 1);

        session.undo()?;
        assert!(session.can_redo());

        let movie2 = dir.join("movie2.mp4");
        fs::write(&movie2, b"fake")?;
        session.execute("asset.import", &obj(&[("path", string_value(&movie2))]))?;
        assert!(!session.can_redo());

        fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn failed_command_leaves_state_unchanged() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("failed_command")?;
        let movie = dir.join("movie.mp4");
        fs::write(&movie, b"fake")?;

        let mut session = Session::new();
        session.execute("asset.import", &obj(&[("path", string_value(&movie))]))?;
        session.execute("timeline.create", &obj(&[("asset", Value::from(1_u64))]))?;

        let before = session.project.clone();
        let undo_len = session.undo.len();
        let redo_len = session.redo.len();
        let dirty = session.dirty;

        let result = session.execute(
            "timeline.addChapter",
            &obj(&[
                ("timeline", Value::from(99_u64)),
                ("time", Value::from(100_i64)),
            ]),
        );
        assert!(result.is_err());
        assert_eq!(session.project, before);
        assert_eq!(session.undo.len(), undo_len);
        assert_eq!(session.redo.len(), redo_len);
        assert_eq!(session.dirty, dirty);

        session.execute(
            "timeline.addChapter",
            &obj(&[
                ("timeline", Value::from(2_u64)),
                ("time", Value::from(100_i64)),
            ]),
        )?;

        let before_duplicate = session.project.clone();
        let result = session.execute(
            "timeline.addChapter",
            &obj(&[
                ("timeline", Value::from(2_u64)),
                ("time", Value::from(100_i64)),
            ]),
        );
        assert!(result.is_err());
        assert_eq!(session.project, before_duplicate);

        fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn validation_rollback() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("validation_rollback")?;
        let movie = dir.join("movie.mp4");
        fs::write(&movie, b"fake")?;

        let mut session = Session::new();
        session.execute("asset.import", &obj(&[("path", string_value(&movie))]))?;
        session.execute("timeline.create", &obj(&[("asset", Value::from(1_u64))]))?;

        let before = session.project.clone();
        let target = obj(&[("timeline", Value::from(99_u64))]);
        let result = session.execute(
            "timeline.setEndAction",
            &obj(&[("timeline", Value::from(2_u64)), ("target", target)]),
        );

        assert!(result.is_err());
        assert_eq!(session.project, before);

        fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn unknown_command_is_error() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        match session.execute("bogus", &Value::Null) {
            Err(message) => assert_eq!(message, "unknown command: bogus"),
            Ok(_) => return Err("expected unknown command error".to_string().into()),
        }
        Ok(())
    }

    #[test]
    fn undo_on_fresh_session_is_error() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        match session.execute("edit.undo", &Value::Null) {
            Err(message) => assert_eq!(message, "edit.undo: nothing to undo"),
            Ok(_) => return Err("expected undo error".to_string().into()),
        }
        Ok(())
    }

    #[test]
    fn null_params_work_for_project_check() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let result = session.execute("project.check", &Value::Null)?;
        assert_eq!(result.get("errors").and_then(Value::as_u64), Some(0));
        Ok(())
    }
}
