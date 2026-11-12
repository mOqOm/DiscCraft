//! Capacity and media support checks.

use crate::{error, warning};
use disccraft_dvd::Estimate;
use disccraft_project::{DiscKind, Problem, Project};

/// Minimum acceptable video bitrate in bits per second.
const LOW_BITRATE_THRESHOLD: u32 = 3_000_000;

/// Appends capacity and media problems for DVD projects.
pub(crate) fn check(project: &Project, problems: &mut Vec<Problem>) {
    if project.settings.kind != DiscKind::Dvd {
        return;
    }

    match disccraft_dvd::estimate(project) {
        Ok(estimate) => check_estimate(&estimate, problems),
        Err(err) => problems.push(capacity_error(err.to_string())),
    }
}

/// Appends problems from a successful estimate.
fn check_estimate(estimate: &Estimate, problems: &mut Vec<Problem>) {
    if estimate.estimated_bytes > estimate.capacity_bytes {
        problems.push(error(
            "capacityExceeded",
            format!(
                "estimated {} bytes exceeds capacity {} bytes",
                estimate.estimated_bytes, estimate.capacity_bytes
            ),
        ));
    }

    if estimate.video_bitrate > 0 && estimate.video_bitrate < LOW_BITRATE_THRESHOLD {
        problems.push(warning(
            "lowBitrate",
            format!(
                "video bitrate {} is below {} bits per second",
                estimate.video_bitrate, LOW_BITRATE_THRESHOLD
            ),
        ));
    }
}

/// Builds a capacity or media error from an estimate failure.
fn capacity_error(message: String) -> Problem {
    if is_capacity_message(&message) {
        error("capacityExceeded", message)
    } else {
        error("unsupportedMedia", message)
    }
}

/// Returns true when a message indicates a capacity problem.
fn is_capacity_message(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    lowered.contains("capacity") || lowered.contains("too long")
}

#[cfg(test)]
mod tests {
    use super::check;
    use disccraft_project::{Capacity, DiscKind, Link, Problem, Project};
    use std::error::Error;
    use std::path::{Path, PathBuf};

    fn has_code(problems: &[Problem], code: &str) -> bool {
        problems.iter().any(|problem| problem.code == code)
    }

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "disccraft-check-{label}-{}.m2v",
            std::process::id()
        ))
    }

    fn write_m2v(path: &Path) -> Result<(), Box<dyn Error>> {
        let mut bytes = vec![
            0x00, 0x00, 0x01, 0xB3, 0x2D, 0x01, 0xE0, 0x24, 0xFF, 0xFF, 0xE0, 0x18,
        ];
        bytes.resize(2048, 0);
        std::fs::write(path, bytes).map_err(|err| err.to_string())?;
        Ok(())
    }

    #[test]
    fn missing_asset_is_unsupported_media() -> Result<(), Box<dyn Error>> {
        let path = temp_path("missing");
        let _ = std::fs::remove_file(&path);

        let mut project = Project::default();
        project.settings.kind = DiscKind::Dvd;
        let asset = project.add_asset(&path)?;
        let timeline = project.add_timeline(asset, None)?;
        project.settings.first_play = Link::Timeline {
            id: timeline,
            chapter: 0,
        };

        let mut problems = Vec::new();
        check(&project, &mut problems);

        assert!(has_code(&problems, "unsupportedMedia"));
        Ok(())
    }

    #[test]
    fn capacity_exceeded_for_large_passthrough() -> Result<(), Box<dyn Error>> {
        let path = temp_path("capacity");
        write_m2v(&path)?;

        let mut project = Project::default();
        project.settings.kind = DiscKind::Dvd;
        project.settings.capacity = Capacity::Custom { bytes: 1000 };
        let asset = project.add_asset(&path)?;
        let timeline = project.add_timeline(asset, None)?;
        project.settings.first_play = Link::Timeline {
            id: timeline,
            chapter: 0,
        };

        let mut problems = Vec::new();
        check(&project, &mut problems);
        let _ = std::fs::remove_file(&path);

        assert!(has_code(&problems, "capacityExceeded"));
        Ok(())
    }
}
