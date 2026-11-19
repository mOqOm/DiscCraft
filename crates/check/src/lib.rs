//! DiscCraft whole-project checks (L2).
//!
//! This crate combines document validation with reachability, navigation,
//! capacity, and platform-support checks before a build starts.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::cognitive_complexity,
    clippy::indexing_slicing
)]

mod capacity;
mod reach;
mod streams;

pub use disccraft_dvd::Estimate;
pub use disccraft_project::{Problem, Severity, has_errors};

use disccraft_project::Project;

/// Checks the whole project before building.
pub fn check_project(project: &Project) -> Vec<Problem> {
    // Transcoded files located for the disc's kind are what the build takes (a source that is gone with them is
    // fine, as in Encore after Locate Transcoded File).
    let located;
    let project = if project.has_located_transcodes() {
        located = project
            .with_located_transcodes()
            .unwrap_or_else(|_| project.clone());
        &located
    } else {
        project
    };
    let mut problems = project.check();
    let base_errors = has_errors(&problems);

    reach::check(project, &mut problems);
    // A video the build would have to convert but whose picture does not decode: the build would stop at it.
    problems.extend(
        disccraft_dvd::undecodable_videos(project)
            .into_iter()
            .map(|(asset, name)| {
                error(
                    "undecodableVideo",
                    format!(
                        "asset {} \"{name}\": the picture cannot be decoded, so the video cannot be converted for this disc",
                        asset.0
                    ),
                )
            }),
    );

    if !base_errors {
        check_navigation(project, &mut problems);
        capacity::check(project, &mut problems);
        streams::check(project, &mut problems);
        problems.extend(
            disccraft_dvd::passthrough_chapter_problems(project)
                .into_iter()
                .map(|message| error("chapterNotAtClosedGop", message)),
        );
    }

    problems
}

/// Returns the estimated disc usage for a project.
pub fn usage(project: &Project) -> Result<Estimate, String> {
    // Button transitions are titles on the disc too.
    let expanded = project
        .with_transitions_expanded()
        .and_then(|expanded| expanded.with_located_transcodes())
        .map_err(|err| err.to_string())?;
    disccraft_dvd::estimate(&expanded).map_err(|err| err.to_string())
}

/// Appends a navigation problem if compilation fails.
fn check_navigation(project: &Project, problems: &mut Vec<Problem>) {
    if let Err(err) = disccraft_nav::compile(project) {
        problems.push(error("navigation", err.to_string()));
    }
}

/// Builds an error problem.
pub(crate) fn error(code: &str, message: String) -> Problem {
    Problem {
        severity: Severity::Error,
        code: code.to_string(),
        message,
    }
}

/// Builds a warning problem.
pub(crate) fn warning(code: &str, message: String) -> Problem {
    Problem {
        severity: Severity::Warning,
        code: code.to_string(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::{check_project, usage};
    use disccraft_project::{DiscKind, Link, Problem, Project, Severity};
    use std::error::Error;
    use std::path::Path;

    fn has_warning_code(problems: &[Problem], code: &str) -> bool {
        problems
            .iter()
            .any(|problem| problem.code == code && problem.severity == Severity::Warning)
    }

    #[test]
    fn bluray_project_with_menu_has_no_bluray_warning() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        project.settings.kind = DiscKind::BluRay;
        let asset = project.add_asset(Path::new("movie.mp4"))?;
        let timeline = project.add_timeline(asset, None)?;
        project.settings.first_play = Link::Timeline {
            id: timeline,
            chapter: 0,
        };
        project.add_menu(None)?;

        let problems = check_project(&project);
        assert!(!has_warning_code(&problems, "blurayMenusIgnored"));
        assert!(!has_warning_code(&problems, "blurayNotSupported"));
        Ok(())
    }

    #[test]
    fn usage_reports_missing_asset() -> Result<(), Box<dyn Error>> {
        let path = std::env::temp_dir().join(format!(
            "disccraft-check-lib-missing-{}.m2v",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let mut project = Project::default();
        let asset = project.add_asset(&path)?;
        let _timeline = project.add_timeline(asset, None)?;

        assert!(usage(&project).is_err());
        Ok(())
    }
}
