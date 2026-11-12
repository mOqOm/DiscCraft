//! DiscCraft build orchestration (L3).
//!
//! This crate turns a project document into a disc output folder. It is a thin
//! layer over format-specific builders and keeps engine/UI code free of build
//! implementation details.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use disccraft_udf::image::{ImageFile, dvd_file_order, write_dvd_iso};
use disccraft_udf::tree::{TreeFile, write_udf250_iso};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::Instant;

use disccraft_bd::{BdBuildReport, build_bdmv};
use disccraft_dvd::{DvdBuildReport, DvdError, build_video_ts};
pub use disccraft_dvd::{TranscodeStatus, transcode_status};
use disccraft_project::{DiscKind, Project};

/// Supported build output kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputKind {
    /// Disc folder: `VIDEO_TS` for a DVD project, `BDMV` for a Blu-ray project.
    Folder,
    /// Disc image. DVD: UDF 1.02 + ISO 9660 bridge; the folder is built next to it (`<name>.files`).
    Iso,
}

/// Build request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildOptions {
    /// Output kind.
    pub output: OutputKind,
    /// Output directory.
    pub path: PathBuf,
}

/// Build result.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildReport {
    /// Written output path.
    pub output: PathBuf,
    /// Number of titles.
    pub titles: usize,
    /// Total chapters across titles.
    pub chapters: usize,
    /// Total bytes written.
    pub total_bytes: u64,
    /// Wall-clock build time in seconds.
    pub seconds: f64,
    /// Layer-break sector of a dual-layer ISO (DVD-9), written next to it as `<name>.layerbreak.txt`.
    pub layer_break: Option<u32>,
}

/// The transcode cache a build with `options` uses: `disccraft-cache` inside the disc folder (for an image,
/// inside the `<name>.files` folder built next to it).
#[must_use]
pub fn cache_dir(options: &BuildOptions) -> PathBuf {
    let folder = match options.output {
        OutputKind::Folder => options.path.clone(),
        OutputKind::Iso => {
            let stem = options
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("disc");
            options.path.with_file_name(format!("{stem}.files"))
        }
    };
    folder.join("disccraft-cache")
}

/// Build a disc output from a project.
pub fn build(project: &Project, options: &BuildOptions) -> Result<BuildReport, String> {
    let started = Instant::now();

    match (project.settings.kind, options.output) {
        (DiscKind::Dvd, OutputKind::Folder) => build_dvd_folder(project, &options.path, started),
        (DiscKind::Dvd, OutputKind::Iso) => build_dvd_iso(project, &options.path, started),
        (DiscKind::BluRay, OutputKind::Folder) => build_bd_folder(project, &options.path, started),
        (DiscKind::BluRay, OutputKind::Iso) => build_bd_iso(project, &options.path, started),
    }
}

fn build_bd_folder(
    project: &Project,
    path: &Path,
    started: Instant,
) -> Result<BuildReport, String> {
    match catch_unwind(AssertUnwindSafe(|| build_bdmv(project, path))) {
        Ok(Ok(report)) => Ok(report_from_bd(report, started)),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err("internal error during build".to_string()),
    }
}

/// Build the `BDMV` folder next to the image (`<name>.files`), then write a UDF 2.50 image of it.
fn build_bd_iso(project: &Project, iso: &Path, started: Instant) -> Result<BuildReport, String> {
    let stem = iso.file_stem().and_then(|s| s.to_str()).unwrap_or("disc");
    let folder = iso.with_file_name(format!("{stem}.files"));
    let mut report = build_bd_folder(project, &folder, started)?;
    let mut files = Vec::new();
    collect_tree(&folder, &folder.join("BDMV"), &mut files)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let iso_report = catch_unwind(AssertUnwindSafe(|| {
        write_udf250_iso(&files, &disc_label(project, stem), iso, now)
    }))
    .map_err(|_| "internal error while writing the ISO image".to_string())?
    .map_err(|e| format!("ISO: {e}"))?;
    report.output = iso.to_path_buf();
    report.total_bytes = u64::from(iso_report.sectors) * 2048;
    report.seconds = started.elapsed().as_secs_f64();
    Ok(report)
}

/// List every file below `dir` with its path relative to `root` ('/' separated).
fn collect_tree(root: &Path, dir: &Path, out: &mut Vec<TreeFile>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        if path.is_dir() {
            collect_tree(root, &path, out)?;
        } else {
            let relative = path
                .strip_prefix(root)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let parts: Vec<String> = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            out.push(TreeFile {
                path: parts.join("/"),
                source: path,
            });
        }
    }
    Ok(())
}

/// The volume label of an image: the disc name when it has ASCII letters or digits, otherwise the file name.
fn disc_label(project: &Project, stem: &str) -> String {
    let from_name = volume_label(project.settings.name.trim());
    if from_name.chars().any(|c| c.is_ascii_alphanumeric()) {
        from_name
    } else {
        volume_label(stem)
    }
}

/// Upper-case ASCII volume label of at most 32 characters.
fn volume_label(stem: &str) -> String {
    stem.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .take(32)
        .collect()
}

fn report_from_bd(report: BdBuildReport, started: Instant) -> BuildReport {
    // Report the disc root (the folder that holds BDMV): players open that folder, not BDMV itself.
    let output = report
        .bdmv
        .parent()
        .map_or_else(|| report.bdmv.clone(), Path::to_path_buf);
    BuildReport {
        output,
        titles: report.titles.len(),
        chapters: report.titles.iter().map(|title| title.chapters).sum(),
        total_bytes: report.total_bytes,
        seconds: started.elapsed().as_secs_f64(),
        layer_break: None,
    }
}

fn build_dvd_folder(
    project: &Project,
    path: &Path,
    started: Instant,
) -> Result<BuildReport, String> {
    let result = catch_unwind(AssertUnwindSafe(|| build_video_ts(project, path)));

    match result {
        Ok(Ok(report)) => Ok(report_from_dvd(report, started)),
        Ok(Err(err)) => Err(dvd_error_message(project, &err)),
        Err(_) => Err("internal error during build".to_string()),
    }
}

fn build_dvd_iso(project: &Project, iso: &Path, started: Instant) -> Result<BuildReport, String> {
    let stem = iso.file_stem().and_then(|s| s.to_str()).unwrap_or("disc");
    let folder = iso.with_file_name(format!("{stem}.files"));
    let mut report = build_dvd_folder(project, &folder, started)?;
    let video_ts = folder.join("VIDEO_TS");
    let names: Vec<String> = std::fs::read_dir(&video_ts)
        .map_err(|e| format!("{}: {e}", video_ts.display()))?
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    let files: Vec<ImageFile> = dvd_file_order(&names)
        .into_iter()
        .map(|name| ImageFile {
            source: video_ts.join(&name),
            name,
        })
        .collect();
    let label = disc_label(project, stem);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let iso_report = catch_unwind(AssertUnwindSafe(|| write_dvd_iso(&files, &label, iso, now)))
        .map_err(|_| "internal error while writing the ISO image".to_string())?
        .map_err(|e| format!("ISO: {e}"))?;
    report.output = iso.to_path_buf();
    report.total_bytes = u64::from(iso_report.sectors) * 2048;
    if iso_report.sectors > SINGLE_LAYER_SECTORS {
        let starts = iso_cell_starts(iso, &video_ts)?;
        let lb = choose_layer_break(iso_report.sectors, &starts).ok_or_else(|| {
            "the image needs a dual-layer disc but no cell boundary fits the layer break (add chapters near the middle)".to_string()
        })?;
        let note = iso.with_extension("layerbreak.txt");
        std::fs::write(
            &note,
            format!(
                "LayerBreak={lb}
TotalSectors={}
",
                iso_report.sectors
            ),
        )
        .map_err(|e| format!("{}: {e}", note.display()))?;
        report.layer_break = Some(lb);
    }
    report.seconds = started.elapsed().as_secs_f64();
    Ok(report)
}

fn report_from_dvd(report: DvdBuildReport, started: Instant) -> BuildReport {
    let chapters = report.titles.iter().map(|title| title.chapters).sum();

    BuildReport {
        output: report.video_ts,
        titles: report.titles.len(),
        chapters,
        total_bytes: report.total_bytes,
        seconds: started.elapsed().as_secs_f64(),
        layer_break: None,
    }
}

fn dvd_error_message(_project: &Project, err: &DvdError) -> String {
    err.to_string()
}

#[cfg(test)]
mod tests {
    use super::{BuildOptions, OutputKind, build, cache_dir, disc_label};
    use disccraft_project::{DiscKind, Project};
    use std::error::Error;
    use std::path::{Path, PathBuf};

    #[test]
    fn the_volume_label_comes_from_the_disc_name() {
        let mut project = Project::default();
        assert_eq!(disc_label(&project, "my-disc"), "MY_DISC");
        project.settings.name = "Summer Trip 2026".to_owned();
        assert_eq!(disc_label(&project, "my-disc"), "SUMMER_TRIP_2026");
        // A name without ASCII letters or digits falls back to the file name.
        project.settings.name = "夏の旅行".to_owned();
        assert_eq!(disc_label(&project, "trip"), "TRIP");
        project.settings.name = "x".repeat(40);
        assert_eq!(disc_label(&project, "trip").len(), 32);
    }

    #[test]
    fn cache_dir_follows_the_output() {
        let folder = BuildOptions {
            output: OutputKind::Folder,
            path: PathBuf::from("out"),
        };
        assert_eq!(cache_dir(&folder), Path::new("out").join("disccraft-cache"));
        let iso = BuildOptions {
            output: OutputKind::Iso,
            path: Path::new("out").join("movie.iso"),
        };
        assert_eq!(
            cache_dir(&iso),
            Path::new("out").join("movie.files").join("disccraft-cache")
        );
    }

    #[test]
    fn blu_ray_project_without_titles_fails() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        project.settings.kind = DiscKind::BluRay;
        let dir = std::env::temp_dir().join("disccraft-build-bd-empty");
        let options = BuildOptions {
            output: OutputKind::Folder,
            path: dir.clone(),
        };
        let result = build(&project, &options);
        let _ = std::fs::remove_dir_all(&dir);
        match result {
            Ok(_) => Err("expected an empty Blu-ray project to fail".into()),
            Err(_) => Ok(()),
        }
    }
}

/// Sectors a single-layer DVD holds (4.7 GB); larger DVD-9 images need a layer break.
pub const SINGLE_LAYER_SECTORS: u32 = 2_295_104;
/// Largest layer-0 size of a dual-layer DVD±R in sectors.
pub const LAYER0_MAX_SECTORS: u32 = 2_086_912;

/// Choose the layer break of a dual-layer image: the smallest cell start that keeps layer 0 at least as large as layer 1
/// (break ≥ half of the image) and fits layer 0. `cell_starts` are absolute image sectors.
pub fn choose_layer_break(total_sectors: u32, cell_starts: &[u32]) -> Option<u32> {
    let half = total_sectors.div_ceil(2);
    cell_starts
        .iter()
        .copied()
        .filter(|&s| s >= half && s <= LAYER0_MAX_SECTORS)
        .min()
}

/// Absolute cell start sectors of every title VOBS in a written ISO (from the VTS IFOs and the UDF file listing).
fn iso_cell_starts(iso: &Path, video_ts: &Path) -> Result<Vec<u32>, String> {
    let listing = disccraft_udf::read::list_udf(iso).map_err(|e| format!("ISO: {e}"))?;
    let mut starts = Vec::new();
    for vts in 1..=99u8 {
        let ifo = video_ts.join(format!("VTS_{vts:02}_0.IFO"));
        if !ifo.is_file() {
            break;
        }
        let bytes = std::fs::read(&ifo).map_err(|e| format!("{}: {e}", ifo.display()))?;
        let vtsi =
            disccraft_ifo::read_vtsi(&bytes).map_err(|e| format!("{}: {e}", ifo.display()))?;
        let vob = format!("VIDEO_TS/VTS_{vts:02}_1.VOB");
        let Some(first) = listing.iter().find(|f| f.path == vob) else {
            continue;
        };
        starts.extend(
            vtsi.address_cells()
                .iter()
                .map(|c| first.sector.saturating_add(c.first_sector)),
        );
    }
    Ok(starts)
}

#[cfg(test)]
mod layer_tests {
    use super::choose_layer_break;

    #[test]
    fn layer_break_is_the_first_cell_past_half() {
        let cells = [300, 1_500_000, 1_900_000, 2_050_000, 2_100_000];
        assert_eq!(choose_layer_break(3_800_000, &cells), Some(1_900_000));
        assert_eq!(choose_layer_break(4_000_000, &cells), Some(2_050_000));
        assert_eq!(choose_layer_break(4_400_000, &cells), None); // 2_200_000 > layer 0
    }
}
