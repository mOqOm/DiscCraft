//! FilmCraft 連携: FilmCraft のプロジェクト（`.fcproj`）からシーケンスとマーカーを読む。マーカーはタイムラインの
//! チャプターポイントになる（Encore が Premiere Pro のチャプターマーカーをチャプターにするのと同じ役目）。
//!
//! プロジェクトのファイルは FilmCraft の `filmcraft-format`（スキーマの版の確認と移行）で読む。
//!
//! 層: L1（docs/architecture.md §5）。
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::path::Path;

use filmcraft_project::MarkerKind;
use filmcraft_time::Tick;

/// The largest project file read (FilmCraft embeds LUTs and transcripts, so projects can be large).
const MAX_PROJECT_BYTES: u64 = 512 * 1024 * 1024;
/// The most markers taken from one sequence.
const MAX_MARKERS: usize = 10_000;

/// Errors of the FilmCraft helpers.
#[derive(Debug, thiserror::Error)]
pub enum FilmError {
    /// Filesystem failure.
    #[error("{path}: {message}")]
    Io {
        /// Path associated with the failure.
        path: String,
        /// Human-readable failure message.
        message: String,
    },
    /// The file is not a FilmCraft project this version reads.
    #[error("FilmCraft: {0}")]
    Format(String),
}

/// One marker of a sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilmMarker {
    /// Its time from the start of the sequence.
    pub time: Tick,
    /// Its name (may be empty).
    pub name: String,
    /// A chapter marker (the others are comments, segmentation, web links and cue points).
    pub chapter: bool,
}

/// One sequence of a FilmCraft project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilmSequence {
    /// The sequence's item id in the project (what FilmCraft's commands take).
    pub id: u64,
    /// The sequence's name in the Project panel.
    pub name: String,
    /// How long it is.
    pub duration: Tick,
    /// Its markers in time order.
    pub markers: Vec<FilmMarker>,
}

/// Whether `path` names a FilmCraft project (`.fcproj`).
#[must_use]
pub fn is_project(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("fcproj"))
}

/// The sequences of the FilmCraft project at `path`, in the project's order, each with its markers.
///
/// # Errors
///
/// When the file cannot be read, is too large, or is not a project this version of FilmCraft's format reads.
pub fn read_sequences(path: &Path) -> Result<Vec<FilmSequence>, FilmError> {
    let io = |message: String| FilmError::Io {
        path: path.display().to_string(),
        message,
    };
    let size = std::fs::metadata(path)
        .map_err(|error| io(error.to_string()))?
        .len();
    if size > MAX_PROJECT_BYTES {
        return Err(io("the project file is too large".to_string()));
    }
    let bytes = std::fs::read(path).map_err(|error| io(error.to_string()))?;
    sequences_of(&bytes)
}

/// [`read_sequences`] of a project file's bytes.
///
/// # Errors
///
/// When the bytes are not a project this version of FilmCraft's format reads.
pub fn sequences_of(bytes: &[u8]) -> Result<Vec<FilmSequence>, FilmError> {
    let loaded =
        filmcraft_format::decode(bytes).map_err(|error| FilmError::Format(error.to_string()))?;
    let project = loaded.project;
    let mut sequences = Vec::new();
    for item in project.sequences() {
        let Some(sequence) = project.sequence(item.id) else {
            continue;
        };
        let mut markers: Vec<FilmMarker> = sequence
            .markers
            .iter()
            .take(MAX_MARKERS)
            .map(|marker| FilmMarker {
                time: marker.start,
                name: marker.name.trim().to_owned(),
                chapter: marker.kind == MarkerKind::Chapter,
            })
            .collect();
        markers.sort_by_key(|marker| marker.time);
        sequences.push(FilmSequence {
            id: item.id.0,
            name: item.name.clone(),
            duration: sequence.duration(),
            markers,
        });
    }
    Ok(sequences)
}

/// The sequence meant by `wanted`: the one of that name; without a name, the one called like one of `hints` (the
/// timeline's or its video's name), else the only sequence. `None` when the choice is open.
#[must_use]
pub fn choose_sequence<'a>(
    sequences: &'a [FilmSequence],
    wanted: Option<&str>,
    hints: &[&str],
) -> Option<&'a FilmSequence> {
    let named = |name: &str| {
        sequences
            .iter()
            .find(|sequence| sequence.name.eq_ignore_ascii_case(name.trim()))
    };
    if let Some(wanted) = wanted {
        return named(wanted);
    }
    if let Some(found) = hints.iter().find_map(|hint| named(hint)) {
        return Some(found);
    }
    match sequences {
        [only] => Some(only),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{FilmSequence, choose_sequence, sequences_of};
    use filmcraft_project::{MarkerKind, Project, SequenceSettings};
    use filmcraft_time::{TICKS_PER_SECOND, Tick};

    fn project_bytes() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut project = Project::new("Film");
        let main = project.new_sequence("Main", SequenceSettings::default(), 1, 1, None);
        let _ = project.new_sequence("Extras", SequenceSettings::default(), 1, 1, None);
        let label = serde_json::to_value(filmcraft_project::Label::Violet)?;
        let mut markers = Vec::new();
        for (id, seconds, name, kind) in [
            (901_u64, 20_i64, "Second", MarkerKind::Chapter),
            (902, 5, "Note", MarkerKind::Comment),
            (903, 10, " First ", MarkerKind::Chapter),
        ] {
            markers.push(serde_json::from_value(serde_json::json!({
                "id": id,
                "start": seconds * TICKS_PER_SECOND,
                "duration": 0,
                "name": name,
                "comment": "",
                "kind": kind,
                "color": label,
            }))?);
        }
        project.sequence_mut(main).ok_or("no sequence")?.markers = markers;
        Ok(filmcraft_format::encode(&project, false))
    }

    /// A project written by FilmCraft's own writer reads back: its sequences, the markers in time order, the
    /// chapter markers told apart.
    #[test]
    fn a_projects_sequences_and_markers_are_read() -> Result<(), Box<dyn std::error::Error>> {
        let sequences = sequences_of(&project_bytes()?)?;
        let names: Vec<&str> = sequences.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Main", "Extras"]);
        let main = sequences.first().ok_or("no sequence")?;
        let read: Vec<(i64, &str, bool)> = main
            .markers
            .iter()
            .map(|m| (m.time.0 / TICKS_PER_SECOND, m.name.as_str(), m.chapter))
            .collect();
        assert_eq!(
            read,
            [
                (5, "Note", false),
                (10, "First", true),
                (20, "Second", true)
            ]
        );
        assert!(sequences.get(1).is_some_and(|s| s.markers.is_empty()));
        Ok(())
    }

    /// What is not a project is refused, not a crash.
    #[test]
    fn other_files_are_refused() {
        assert!(sequences_of(b"").is_err());
        assert!(sequences_of(b"{\"format\": \"something else\"}").is_err());
        assert!(sequences_of(&[0xFF; 64]).is_err());
    }

    #[test]
    fn the_sequence_is_chosen_by_name_then_by_hint_then_alone() {
        let sequence = |name: &str| FilmSequence {
            id: 1,
            name: name.to_owned(),
            duration: Tick(0),
            markers: Vec::new(),
        };
        let two = [sequence("Main"), sequence("Extras")];
        assert_eq!(
            choose_sequence(&two, Some("extras"), &[]).map(|s| s.name.as_str()),
            Some("Extras")
        );
        assert!(choose_sequence(&two, Some("Other"), &["Main"]).is_none());
        assert_eq!(
            choose_sequence(&two, None, &["movie", "Main"]).map(|s| s.name.as_str()),
            Some("Main")
        );
        assert!(choose_sequence(&two, None, &["movie"]).is_none());
        let one = [sequence("Cut")];
        assert_eq!(
            choose_sequence(&one, None, &[]).map(|s| s.name.as_str()),
            Some("Cut")
        );
        assert!(choose_sequence(&[], None, &[]).is_none());
    }
}
