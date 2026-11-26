use disccraft_project::{
    AssetKind, Capacity, DiscKind, Link, Project, Severity, VideoStandard, format_time,
};
use std::fmt::Write;

pub fn human_summary(project: &Project) -> Result<String, String> {
    let mut out = String::new();

    write_header(project, &mut out)?;
    write_assets(project, &mut out)?;
    write_timelines(project, &mut out)?;
    write_check(project, &mut out)?;

    if out.ends_with('\n') {
        out.truncate(out.len() - 1);
    }

    Ok(out)
}

fn write_header(project: &Project, out: &mut String) -> Result<(), String> {
    writeln!(
        out,
        "Disc: {} / {} / {} ({} GiB) / regions: {}",
        kind_label(project.settings.kind),
        standard_label(project.settings.standard),
        capacity_label(project.settings.capacity),
        format_gb(project.settings.capacity.bytes()),
        regions_label(&project.settings.regions)
    )
    .map_err(|error| error.to_string())?;

    writeln!(
        out,
        "First Play: {}",
        link_label(&project.settings.first_play)
    )
    .map_err(|error| error.to_string())?;

    Ok(())
}

fn write_assets(project: &Project, out: &mut String) -> Result<(), String> {
    writeln!(out, "Assets ({})", project.assets.len()).map_err(|error| error.to_string())?;

    for asset in project.assets.values() {
        let offline = if asset.offline { " (offline)" } else { "" };
        writeln!(
            out,
            "  [{}] {} ({}) {}{}",
            asset.id.0,
            asset.name,
            asset_kind_label(asset.kind),
            asset.path,
            offline
        )
        .map_err(|error| error.to_string())?;
    }

    Ok(())
}

fn write_timelines(project: &Project, out: &mut String) -> Result<(), String> {
    writeln!(out, "Timelines ({})", project.timelines.len()).map_err(|error| error.to_string())?;

    for timeline in project.timelines.values() {
        writeln!(
            out,
            "  [{}] {}  video: [{}]  chapters: {}  end action: {}",
            timeline.id.0,
            timeline.name,
            timeline.video.0,
            timeline.chapters.len(),
            link_label(&timeline.end_action)
        )
        .map_err(|error| error.to_string())?;

        for (index, chapter) in timeline.chapters.iter().enumerate() {
            writeln!(
                out,
                "    {}  {}  {}",
                index + 1,
                format_time(chapter.time, &project.settings),
                chapter.name
            )
            .map_err(|error| error.to_string())?;
        }
    }

    Ok(())
}

fn write_check(project: &Project, out: &mut String) -> Result<(), String> {
    // The whole check, as the GUI, MCP and a build run it (capacity, navigation and the media too).
    let problems = disccraft_check::check_project(project);
    let errors = problems
        .iter()
        .filter(|problem| problem.severity == Severity::Error)
        .count();
    let warnings = problems
        .iter()
        .filter(|problem| problem.severity == Severity::Warning)
        .count();

    writeln!(out, "Check: {} errors / {} warnings", errors, warnings)
        .map_err(|error| error.to_string())?;

    for problem in problems {
        writeln!(
            out,
            "  {} {}: {}",
            severity_label(problem.severity),
            problem.code,
            problem.message
        )
        .map_err(|error| error.to_string())?;
    }

    Ok(())
}

fn kind_label(kind: DiscKind) -> &'static str {
    match kind {
        DiscKind::Dvd => "DVD",
        DiscKind::BluRay => "Blu-ray",
    }
}

fn standard_label(standard: VideoStandard) -> &'static str {
    match standard {
        VideoStandard::Ntsc => "NTSC",
        VideoStandard::Pal => "PAL",
    }
}

fn capacity_label(capacity: Capacity) -> &'static str {
    match capacity {
        Capacity::Dvd5 => "DVD-5",
        Capacity::Dvd9 => "DVD-9",
        Capacity::Bd25 => "BD-25",
        Capacity::Bd50 => "BD-50",
        Capacity::Bd100 => "BD-100 (BDXL)",
        Capacity::Bd128 => "BD-128 (BDXL)",
        Capacity::Custom { bytes: _ } => "Custom",
    }
}

/// `bytes` in GiB to two places (M6.494: it was billions of bytes, named GB).
fn format_gb(bytes: u64) -> String {
    let hundredths = (u128::from(bytes) * 100 + (1 << 29)) >> 30;
    format!("{}.{:02}", hundredths / 100, hundredths % 100)
}

fn regions_label(regions: &[u8]) -> String {
    if regions.is_empty() {
        return "all".to_string();
    }

    let mut out = String::new();
    for (index, region) in regions.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&region.to_string());
    }
    out
}

fn link_label(link: &Link) -> String {
    match link {
        Link::NotSet => "not set".to_string(),
        Link::Stop => "stop".to_string(),
        Link::LastMenu => "last menu".to_string(),
        Link::Timeline { id, chapter } => {
            format!("timeline [{}] chapter {}", id.0, chapter.saturating_add(1))
        }
        Link::Menu { id, button } => match button {
            Some(button) => format!("menu [{}] button {}", id.0, button.saturating_add(1)),
            None => format!("menu [{}]", id.0),
        },
        Link::Playlist { id } => format!("playlist [{}]", id),
        Link::ChapterPlaylist { id } => format!("chapter playlist [{}]", id),
        Link::Slideshow { id } => format!("slideshow [{}]", id),
    }
}

fn asset_kind_label(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Video => "video",
        AssetKind::Audio => "audio",
        AssetKind::Image => "image",
        AssetKind::Subtitle => "subtitle",
        AssetKind::Other => "other",
    }
}

fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}
