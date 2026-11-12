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
        "ディスク: {} / {} / {} ({} GB) / リージョン: {}",
        kind_label(project.settings.kind),
        standard_label(project.settings.standard),
        capacity_label(project.settings.capacity),
        format_gb(project.settings.capacity.bytes()),
        regions_label(&project.settings.regions)
    )
    .map_err(|error| error.to_string())?;

    writeln!(
        out,
        "ファーストプレイ: {}",
        link_label(&project.settings.first_play)
    )
    .map_err(|error| error.to_string())?;

    Ok(())
}

fn write_assets(project: &Project, out: &mut String) -> Result<(), String> {
    writeln!(out, "アセット ({})", project.assets.len()).map_err(|error| error.to_string())?;

    for asset in project.assets.values() {
        let offline = if asset.offline {
            " (オフライン)"
        } else {
            ""
        };
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
    writeln!(out, "タイムライン ({})", project.timelines.len())
        .map_err(|error| error.to_string())?;

    for timeline in project.timelines.values() {
        writeln!(
            out,
            "  [{}] {}  映像: [{}]  チャプター: {}  終了時: {}",
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
    let problems = project.check();
    let errors = problems
        .iter()
        .filter(|problem| problem.severity == Severity::Error)
        .count();
    let warnings = problems
        .iter()
        .filter(|problem| problem.severity == Severity::Warning)
        .count();

    writeln!(out, "チェック: エラー {} / 警告 {}", errors, warnings)
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

fn format_gb(bytes: u64) -> String {
    let q = bytes / 10_000_000;
    let whole = q / 100;
    let frac = q % 100;
    format!("{whole}.{frac:02}")
}

fn regions_label(regions: &[u8]) -> String {
    if regions.is_empty() {
        return "フリー".to_string();
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
        Link::NotSet => "未設定".to_string(),
        Link::Stop => "停止".to_string(),
        Link::LastMenu => "最後のメニュー".to_string(),
        Link::Timeline { id, chapter } => {
            format!(
                "タイムライン [{}] チャプター {}",
                id.0,
                chapter.saturating_add(1)
            )
        }
        Link::Menu { id, button } => match button {
            Some(button) => format!("メニュー [{}] ボタン {}", id.0, button.saturating_add(1)),
            None => format!("メニュー [{}]", id.0),
        },
        Link::Playlist { id } => format!("プレイリスト [{}]", id),
        Link::ChapterPlaylist { id } => format!("チャプタープレイリスト [{}]", id),
        Link::Slideshow { id } => format!("スライドショー [{}]", id),
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
        Severity::Error => "エラー",
        Severity::Warning => "警告",
    }
}
