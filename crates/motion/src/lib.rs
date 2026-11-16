//! EffectCraft 連携: `.ecproj` のコンポジションをムービー（H.264 の MP4）にレンダリングする（モーションメニュー、
//! オープニングなどの映像素材）。レンダリングは EffectCraft 自身のエンジンとレンダーキューで行う。
//!
//! 層: L0（docs/architecture.md §5）。
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::path::Path;

use serde_json::{Value, json};

/// Errors of the EffectCraft helpers.
#[derive(Debug, thiserror::Error)]
pub enum MotionError {
    /// Filesystem or path-related failure.
    #[error("{path}: {message}")]
    Io {
        /// Path associated with the failure.
        path: String,
        /// Human-readable failure message.
        message: String,
    },
    /// EffectCraft engine failure.
    #[error("EffectCraft: {0}")]
    Engine(String),
}

/// What a render made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The composition that was rendered.
    pub composition: String,
}

/// How long a render may run before it is given up.
const RENDER_LIMIT: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

/// Whether the path is an EffectCraft project (`.ecproj`, case-insensitive).
#[must_use]
pub fn is_project(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ecproj"))
}

/// The compositions of an EffectCraft project, by name in the project's order, and the one the project has
/// active (what [`render`] takes when no name is given).
///
/// # Errors
///
/// When the project cannot be opened.
pub fn compositions(project: &Path) -> Result<(Vec<String>, Option<String>), MotionError> {
    if !project.is_file() {
        return Err(io_err(project, "project not found"));
    }
    let project_path = absolute(project)?;
    let mut session = effectcraft_host::session();
    let _ = session
        .execute("file.open", json!({ "path": project_path }))
        .map_err(|error| MotionError::Engine(error.to_string()))?;
    let name_of = |id| session.project.item(id).map(|item| item.name.clone());
    let names = session
        .project
        .comps()
        .filter_map(|(id, _)| name_of(*id))
        .collect();
    let active = session.state.active_comp.and_then(name_of);
    Ok((names, active))
}

/// Opens an EffectCraft project and renders a composition (`composition` by name, else the one the project has
/// active) to an H.264 MP4 at `movie_out`, with its sound when it has any. `progress` gets the part done, 0 to 1.
///
/// # Errors
///
/// When the project cannot be opened, has no composition to render, or the render fails.
pub fn render(
    project: &Path,
    movie_out: &Path,
    composition: Option<&str>,
    progress: &mut dyn FnMut(f32),
) -> Result<Rendered, MotionError> {
    if !project.is_file() {
        return Err(io_err(project, "project not found"));
    }
    let project_path = absolute(project)?;
    let out_path = absolute(movie_out)?;
    let engine = |error: String| MotionError::Engine(error);

    let mut session = effectcraft_host::session();
    let _ = session
        .execute("file.open", json!({ "path": project_path }))
        .map_err(|error| engine(error.to_string()))?;
    // Only the item added here renders: what the project already had queued stays as it is.
    for index in 0..session.project.render_queue.len() {
        let _ = session.execute(
            "renderQueue.setRender",
            json!({ "index": index.saturating_add(1), "render": false }),
        );
    }
    let mut params = json!({ "output": out_path, "format": "h264" });
    if let (Some(name), Some(map)) = (composition, params.as_object_mut()) {
        let _ = map.insert("comp".to_owned(), Value::String(name.to_owned()));
    }
    let added = session
        .execute("renderQueue.add", params)
        .map_err(|error| engine(error.to_string()))?;
    let name = added
        .get("compName")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let _ = session
        .execute("renderQueue.render", json!({ "wait": false }))
        .map_err(|error| engine(error.to_string()))?;

    let started = std::time::Instant::now();
    while session.is_rendering() {
        std::thread::sleep(std::time::Duration::from_millis(50));
        if let Some(state) = session.render_progress()
            && state.total > 0
        {
            // Frame counts as a fraction; the precision of f32 is plenty for a progress bar.
            #[allow(clippy::cast_precision_loss)]
            progress((state.done as f32 / state.total as f32).clamp(0.0, 1.0));
        }
        let _ = session.poll_render();
        if started.elapsed() > RENDER_LIMIT {
            let _ = session.execute("renderQueue.stop", json!({}));
            return Err(engine("the render took too long".to_owned()));
        }
    }
    let _ = session.poll_render();

    let failure = session
        .project
        .render_queue
        .iter()
        .rev()
        .find_map(|item| match &item.status {
            effectcraft_engine::project::render_queue::RenderStatus::Failed(message) => {
                Some(message.clone())
            }
            _ => None,
        });
    if let Some(message) = failure {
        return Err(engine(message));
    }
    if !movie_out.is_file() {
        return Err(io_err(movie_out, "render output missing"));
    }
    progress(1.0);
    Ok(Rendered { composition: name })
}

fn absolute(path: &Path) -> Result<String, MotionError> {
    std::path::absolute(path)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| io_err(path, &error.to_string()))
}

fn io_err(path: &Path, message: &str) -> MotionError {
    MotionError::Io {
        path: path.display().to_string(),
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{MotionError, is_project, render};
    use std::path::Path;

    #[test]
    fn is_project_checks_extension() {
        assert!(is_project(Path::new("intro.ecproj")));
        assert!(is_project(Path::new("INTRO.ECPROJ")));
        assert!(!is_project(Path::new("intro.mp4")));
    }

    #[test]
    fn a_missing_project_is_io() {
        let result = render(
            Path::new("definitely-missing-effectcraft-project.ecproj"),
            Path::new("out.mp4"),
            None,
            &mut |_| {},
        );
        assert!(matches!(result, Err(MotionError::Io { .. })));
    }
}
