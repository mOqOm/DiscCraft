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

use disccraft_udf::image::{DVD_ROOT_NAMES, ImageFile, dvd_file_order, write_dvd_iso_with_rom};
use disccraft_udf::tree::{TreeFile, write_udf250_iso_with_dirs};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::Instant;

use disccraft_bd::{BdBuildReport, build_bdmv_with};
pub use disccraft_dvd::TranscodeStatus;
/// Whether a video asset fits the project's disc as it is (a DVD preset of its own transcodes it all the same).
pub use disccraft_dvd::video_fits_as_is;
/// Whether a video asset would fit a Blu-ray title in a format as it is (for a format being chosen).
pub use disccraft_dvd::video_fits_bluray_as;
/// Whether a video asset would fit a disc of a kind as it is (for the column of the other kind of disc).
pub use disccraft_dvd::video_fits_disc;
use disccraft_dvd::{DvdBuildReport, DvdError, build_video_ts_with};
/// A slide's pan and zoom as the build moves its picture (the preview shows it the same way).
pub use disccraft_dvd::{SlideMotion, slide_motion};

/// The transcode status of every asset in `project` (`disccraft_dvd::transcode_status`). On an HD Blu-ray, a
/// video whose H.264 for the project's format is in `cache_dir` is [`TranscodeStatus::Transcoded`] (the DVD
/// status knows the MPEG-2 of an SD disc only).
#[must_use]
pub fn transcode_status(
    project: &Project,
    cache_dir: Option<&Path>,
) -> std::collections::BTreeMap<disccraft_project::AssetId, TranscodeStatus> {
    disccraft_dvd::sync_media_choices(project);
    let mut all = disccraft_dvd::transcode_status(project, cache_dir);
    let hd = project.settings.kind == DiscKind::BluRay && project.settings.bd_video.hd().is_some();
    if let (true, Some(cache)) = (hd, cache_dir) {
        for (asset, status) in &mut all {
            let video = project
                .assets
                .get(asset)
                .is_some_and(|asset| asset.kind == disccraft_project::AssetKind::Video);
            if video && *status == TranscodeStatus::Untranscoded {
                if disccraft_bd::hd_transcoded(project, cache, *asset) {
                    *status = TranscodeStatus::Transcoded;
                }
            } else if video && *status == TranscodeStatus::Transcoded {
                // An SD transcode left in the cache says nothing about the HD title.
                *status = TranscodeStatus::Untranscoded;
            }
        }
    }
    // A video whose transcoded files were located for this kind of disc (and are there) is transcoded, as in
    // Encore after Locate Transcoded File.
    for asset in project.assets.values() {
        let located = asset
            .located(project.settings.kind)
            .is_some_and(|located| Path::new(&located.video).is_file());
        if located {
            all.insert(asset.id, TranscodeStatus::Transcoded);
        }
    }
    all
}

/// The project as a build sees it: button transitions become hidden titles (Encore's button Transition), and
/// transcoded files located for the disc's kind take their assets' place (Encore's Locate Transcoded File).
fn for_build(project: &Project) -> Result<Project, String> {
    project
        .with_transitions_expanded()
        .and_then(|expanded| expanded.with_located_transcodes())
        .map_err(|error| error.to_string())
}
use disccraft_project::{DiscKind, Project};

mod layer;
pub use layer::{LayerBreak, LayerBreakPlace, frame_90k, millis, time_text};

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
    /// Where transcoded media is kept between builds; `None` for `disccraft-cache` next to the output (see
    /// [`cache_dir`]). A cache of its own lets a folder and an image of the same project, or builds into a fresh
    /// output, share their transcodes.
    pub cache: Option<PathBuf>,
    /// DiscCraft's display language (ISO 639-1): the language of audio without its own when the project names
    /// none (`DiscSettings::audio_language`). `None` leaves such audio without a language.
    pub audio_language: Option<String>,
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
    /// The layer break of a dual-layer DVD image (DVD-9), read back from the image and written next to it as
    /// `<name>.layerbreak.txt`.
    pub layer_break: Option<LayerBreak>,
}

/// Asks the transcodes of the running build or Transcode Now to stop (Encore's Progress panel ▸ Stop). The job
/// ends with an error that [`was_stopped`] tells from a failure; what it had transcoded so far stays in the cache.
pub fn request_stop() {
    disccraft_dvd::request_stop();
    // An image that is being written stops at its next chunk (M6.278).
    disccraft_udf::request_cancel();
}

/// Whether `error` (of [`build`], [`build_with_progress`] or [`transcode_now`]) says the job was stopped.
#[must_use]
pub fn was_stopped(error: &str) -> bool {
    error.contains(disccraft_dvd::STOPPED) || error.contains(disccraft_udf::CANCELLED)
}

/// Progress panel ▸ Stop between the steps of a build that are not transcodes (before the image is written,
/// between the files of the ROM content): the build ends there. Writing the image itself is not interrupted.
fn stop_before_step() -> Result<(), String> {
    if disccraft_dvd::stop_requested() {
        return Err(disccraft_dvd::STOPPED.to_string());
    }
    Ok(())
}

/// Sets how many transcodes a build (and Transcode Now) runs at a time; 0 runs one for every core of the CPU.
pub fn set_parallel_transcodes(most: usize) {
    disccraft_dvd::set_parallel_transcodes(most);
}

/// The transcode cache a build with `options` uses: `disccraft-cache` inside the disc folder (for an image,
/// inside the `<name>.files` folder built next to it).
#[must_use]
pub fn cache_dir(options: &BuildOptions) -> PathBuf {
    if let Some(cache) = &options.cache {
        return cache.clone();
    }
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
    build_with_progress(project, options, &mut |_, _| {})
}

/// Encore's Transcode Now: makes the transcodes a build with `options` would make, in that build's cache
/// ([`cache_dir`]), without writing a disc. With `only`, just those of the timelines that play that asset.
/// `progress` is called as in [`build_with_progress`].
///
/// # Errors
///
/// The first transcode that fails, as a build would say it.
pub fn transcode_now(
    project: &Project,
    options: &BuildOptions,
    only: Option<disccraft_project::AssetId>,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<(), String> {
    disccraft_dvd::clear_stop();
    disccraft_udf::clear_cancel();
    disccraft_dvd::sync_media_choices(project);
    let expanded = for_build(project)?;
    let cache = cache_dir(options);
    match expanded.settings.kind {
        DiscKind::Dvd => disccraft_dvd::transcode_now(&expanded, &cache, only, progress)
            .map_err(|error| error.to_string()),
        DiscKind::BluRay => disccraft_bd::transcode_now(&expanded, &cache, only, progress)
            .map_err(|error| error.to_string()),
    }
}

/// What File ▸ Render makes ahead of a build (Encore: Render ▸ Project / Motion Menu / Slideshow).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Render {
    /// Everything a build would transcode (as Transcode Now of every asset).
    Project,
    /// The motion menus' pictures (`Some`: one menu). An HD Blu-ray's are made at the build.
    MotionMenus(Option<disccraft_project::MenuId>),
    /// The slideshows (`Some`: one).
    Slideshows(Option<u64>),
}

/// The motion menu `menu` as File ▸ Render ▸ Motion Menu (or a build with `options`) left it in the build's cache,
/// when it is there and still the menu as it is: its MPEG-2 video, for the project preview to play (M6.334). `None`
/// for an HD Blu-ray (its motion menus are made at the build) and a menu not rendered.
#[must_use]
pub fn rendered_motion_menu(
    project: &Project,
    options: &BuildOptions,
    menu: disccraft_project::MenuId,
) -> Option<std::path::PathBuf> {
    let expanded = for_build(project).ok()?;
    if expanded.settings.kind == DiscKind::BluRay && expanded.settings.bd_video.hd().is_some() {
        return None;
    }
    disccraft_dvd::rendered_motion_menu(&expanded, menu, &cache_dir(options))
}

/// File ▸ Render: makes `what` into the cache a build with `options` uses, so the build takes it as it is.
/// Returns how many items were made or found ready (0 for [`Render::Project`]).
///
/// # Errors
///
/// The first one that cannot be made, as a build would say it; stopped as [`was_stopped`] tells.
pub fn render(
    project: &Project,
    options: &BuildOptions,
    what: Render,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<usize, String> {
    if what == Render::Project {
        return transcode_now(project, options, None, progress).map(|()| 0);
    }
    disccraft_dvd::clear_stop();
    disccraft_udf::clear_cancel();
    disccraft_dvd::sync_media_choices(project);
    let expanded = for_build(project)?;
    let cache = cache_dir(options);
    let hd =
        expanded.settings.kind == DiscKind::BluRay && expanded.settings.bd_video.hd().is_some();
    match what {
        Render::Project => Ok(0),
        Render::MotionMenus(_) if hd => {
            Err("an HD Blu-ray's motion menus are made when the disc is built".to_string())
        }
        Render::MotionMenus(only) => {
            disccraft_dvd::render_motion_menus(&expanded, &cache, only, progress)
                .map_err(|error| error.to_string())
        }
        Render::Slideshows(only) => match expanded.settings.kind {
            DiscKind::Dvd => disccraft_dvd::render_slideshows(&expanded, &cache, only, progress)
                .map_err(|error| error.to_string()),
            DiscKind::BluRay => disccraft_bd::render_slideshows(&expanded, &cache, only, progress)
                .map_err(|error| error.to_string()),
        },
    }
}

/// Encore's Revert to Original: removes `asset`'s transcodes from the cache a build with `options` uses, so that
/// its status is "Untranscoded" again and the next build (or Transcode Now) converts it anew. Returns how many
/// files went.
///
/// # Errors
///
/// When the asset does not exist or the cache folder cannot be read.
pub fn revert_to_original(
    project: &Project,
    options: &BuildOptions,
    asset: disccraft_project::AssetId,
) -> Result<usize, String> {
    let cache = cache_dir(options);
    match project.settings.kind {
        DiscKind::Dvd => disccraft_dvd::revert_transcodes(project, &cache, asset)
            .map_err(|error| error.to_string()),
        DiscKind::BluRay => disccraft_bd::revert_transcodes(project, &cache, asset)
            .map_err(|error| error.to_string()),
    }
}

/// The step [`build_with_progress`] names while it writes the disc image (after the folder).
pub const STEP_IMAGE: &str = "disc image";

/// Like [`build`], telling `progress` what it is doing as it goes: the name of the media it is transcoding (or
/// [`STEP_IMAGE`]) and how far that is, from 0 to 1. `progress` is called on the calling thread.
pub fn build_with_progress(
    project: &Project,
    options: &BuildOptions,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<BuildReport, String> {
    let started = Instant::now();
    disccraft_dvd::clear_stop();
    disccraft_udf::clear_cancel();
    disccraft_dvd::sync_media_choices(project);
    let mut expanded = for_build(project)?;
    if expanded.settings.audio_language.is_none() {
        expanded
            .settings
            .audio_language
            .clone_from(&options.audio_language);
    }
    let project = &expanded;

    let cache = cache_dir(options);
    let at = (cache.as_path(), started);
    // The ROM content is read before the long part of the build, so a folder that is gone is said at once.
    let rom = rom_tree(project)?;
    match (project.settings.kind, options.output) {
        (DiscKind::Dvd, OutputKind::Folder) => {
            let mut report = build_dvd_folder(project, &options.path, at, progress)?;
            report.total_bytes =
                report
                    .total_bytes
                    .saturating_add(copy_rom(project, &rom, &options.path)?);
            Ok(report)
        }
        (DiscKind::Dvd, OutputKind::Iso) => {
            build_dvd_iso(project, (&options.path, &rom), at, progress)
        }
        (DiscKind::BluRay, OutputKind::Folder) => {
            let mut report = build_bd_folder(project, &options.path, at, progress)?;
            report.total_bytes =
                report
                    .total_bytes
                    .saturating_add(copy_rom(project, &rom, &options.path)?);
            Ok(report)
        }
        (DiscKind::BluRay, OutputKind::Iso) => {
            build_bd_iso(project, (&options.path, &rom), at, progress)
        }
    }
}

/// The names in the root of a Blu-ray disc that ROM content may not use.
const BD_ROOT_NAMES: [&str; 2] = ["BDMV", "CERTIFICATE"];

/// The project's ROM content (Encore's DVD-ROM / BD-ROM content: files a computer sees beside the video) as files
/// of the disc's root; none when the project has none.
///
/// # Errors
///
/// When the folder cannot be read, or its top holds a name the disc uses itself (`VIDEO_TS`, `BDMV`).
fn rom_tree(project: &Project) -> Result<Vec<TreeFile>, String> {
    let reserved: &[&str] = match project.settings.kind {
        DiscKind::Dvd => &DVD_ROOT_NAMES,
        DiscKind::BluRay => &BD_ROOT_NAMES,
    };
    let files = project.rom_files()?;
    for file in &files {
        let top = file.path.split('/').next().unwrap_or_default();
        if reserved.iter().any(|name| name.eq_ignore_ascii_case(top)) {
            return Err(format!(
                "ROM content: \"{top}\" is the disc's own folder; take it out of the ROM content folder"
            ));
        }
    }
    Ok(files
        .into_iter()
        .map(|file| TreeFile {
            path: file.path,
            source: file.source,
        })
        .collect())
}

/// Copies the ROM content into the disc folder `root` (beside `VIDEO_TS` / `BDMV`) and returns its bytes.
fn copy_rom(project: &Project, rom: &[TreeFile], root: &Path) -> Result<u64, String> {
    if rom.is_empty() {
        return Ok(0);
    }
    // A disc folder inside the content folder would become part of the content of the next build.
    let content = project.settings.rom_content.as_deref().unwrap_or_default();
    if let (Ok(content), Ok(root)) = (Path::new(content).canonicalize(), root.canonicalize())
        && root.starts_with(&content)
    {
        return Err(
            "the output folder is inside the ROM content folder; choose another place".to_string(),
        );
    }
    let mut bytes = 0_u64;
    for file in rom {
        stop_before_step()?;
        let mut target = root.to_path_buf();
        target.extend(file.path.split('/'));
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let same = matches!(
            (file.source.canonicalize(), target.canonicalize()),
            (Ok(source), Ok(target)) if source == target
        );
        let copied = if same {
            std::fs::metadata(&target).map(|metadata| metadata.len())
        } else {
            std::fs::copy(&file.source, &target)
        }
        .map_err(|e| format!("ROM content: {}: {e}", file.source.display()))?;
        bytes = bytes.saturating_add(copied);
    }
    Ok(bytes)
}

fn build_bd_folder(
    project: &Project,
    path: &Path,
    (cache, started): (&Path, Instant),
    progress: &mut dyn FnMut(&str, f32),
) -> Result<BuildReport, String> {
    match catch_unwind(AssertUnwindSafe(|| {
        build_bdmv_with(project, path, cache, &mut *progress)
    })) {
        Ok(Ok(report)) => Ok(report_from_bd(report, started)),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err("internal error during build".to_string()),
    }
}

/// Build the `BDMV` folder next to the image (`<name>.files`), then write a UDF 2.50 image of it.
fn build_bd_iso(
    project: &Project,
    (iso, rom): (&Path, &[TreeFile]),
    (cache, started): (&Path, Instant),
    progress: &mut dyn FnMut(&str, f32),
) -> Result<BuildReport, String> {
    let stem = iso.file_stem().and_then(|s| s.to_str()).unwrap_or("disc");
    let folder = iso.with_file_name(format!("{stem}.files"));
    let mut report = build_bd_folder(project, &folder, (cache, started), progress)?;
    progress(STEP_IMAGE, 0.0);
    stop_before_step()?;
    let mut files = Vec::new();
    collect_tree(&folder, &folder.join("BDMV"), &mut files)?;
    // The directories that stay empty (AUXDATA, BDJO, JAR, META, CERTIFICATE) are in the image as in the folder.
    let mut dirs = Vec::new();
    collect_dirs(&folder, &folder.join("BDMV"), &mut dirs)?;
    let certificate = folder.join("CERTIFICATE");
    if certificate.is_dir() {
        dirs.push("CERTIFICATE".to_string());
        collect_dirs(&folder, &certificate, &mut dirs)?;
    }
    files.extend(rom.iter().cloned());
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let iso_report = catch_unwind(AssertUnwindSafe(|| {
        write_udf250_iso_with_dirs(&files, &dirs, &disc_label(project, stem), iso, now)
    }))
    .map_err(|_| "internal error while writing the ISO image".to_string())
    .and_then(|written| written.map_err(|e| format!("ISO: {e}")))
    .inspect_err(|_| {
        // What was written of the image is no image: it is not left for a player to open.
        let _ = std::fs::remove_file(iso);
    })?;
    progress(STEP_IMAGE, 1.0);
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

/// The directories under `dir` (paths relative to `root`, with `/`), whether files are in them or not.
fn collect_dirs(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        if path.is_dir() {
            let relative = path
                .strip_prefix(root)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let parts: Vec<String> = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            out.push(parts.join("/"));
            collect_dirs(root, &path, out)?;
        }
    }
    Ok(())
}

/// The volume label of an image: the disc name as it is, Japanese included (the UDF volume identifier keeps its
/// case, spaces and characters, as Encore writes it; the image writer makes the ISO 9660 one of d-characters from it,
/// or from the file name), at most 30 characters; the file name when the disc has no name.
fn disc_label(project: &Project, stem: &str) -> String {
    let name = project.settings.name.trim();
    if name.is_empty() {
        volume_label(stem)
    } else {
        name.chars().take(30).collect()
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
    (cache, started): (&Path, Instant),
    progress: &mut dyn FnMut(&str, f32),
) -> Result<BuildReport, String> {
    let report = build_video_ts_checked(project, path, cache, progress)?;
    Ok(report_from_dvd(&report, started))
}

/// The VIDEO_TS build, with a panic reported as an error.
fn build_video_ts_checked(
    project: &Project,
    path: &Path,
    cache: &Path,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<DvdBuildReport, String> {
    let result = catch_unwind(AssertUnwindSafe(|| {
        build_video_ts_with(project, path, cache, &mut *progress)
    }));
    match result {
        Ok(Ok(report)) => Ok(report),
        Ok(Err(err)) => Err(dvd_error_message(project, &err)),
        Err(_) => Err("internal error during build".to_string()),
    }
}

fn build_dvd_iso(
    project: &Project,
    (iso, rom): (&Path, &[TreeFile]),
    (cache, started): (&Path, Instant),
    progress: &mut dyn FnMut(&str, f32),
) -> Result<BuildReport, String> {
    let stem = iso.file_stem().and_then(|s| s.to_str()).unwrap_or("disc");
    let folder = iso.with_file_name(format!("{stem}.files"));
    let dvd = build_video_ts_checked(project, &folder, cache, progress)?;
    let mut report = report_from_dvd(&dvd, started);
    progress(STEP_IMAGE, 0.0);
    stop_before_step()?;
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
    let iso_report = catch_unwind(AssertUnwindSafe(|| {
        write_dvd_iso_with_rom(&files, rom, &label, iso, now)
    }))
    .map_err(|_| "internal error while writing the ISO image".to_string())
    .and_then(|written| written.map_err(|e| format!("ISO: {e}")))
    .inspect_err(|_| {
        // What was written of the image is no image: it is not left for a player to open.
        let _ = std::fs::remove_file(iso);
    })?;
    progress(STEP_IMAGE, 1.0);
    report.output = iso.to_path_buf();
    report.total_bytes = u64::from(iso_report.sectors) * 2048;
    report.layer_break = layer::layer_break(project, &dvd, iso, iso_report.sectors)?;
    report.seconds = started.elapsed().as_secs_f64();
    Ok(report)
}

fn report_from_dvd(report: &DvdBuildReport, started: Instant) -> BuildReport {
    let chapters = report.titles.iter().map(|title| title.chapters).sum();

    BuildReport {
        output: report.video_ts.clone(),
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
        // The disc name as it is (the UDF volume identifier; the ISO 9660 one is made from it by the image writer).
        project.settings.name = "Summer Trip 2026".to_owned();
        assert_eq!(disc_label(&project, "my-disc"), "Summer Trip 2026");
        // A Japanese name is kept (UDF holds it; ISO 9660 takes the file name).
        project.settings.name = "夏の旅行".to_owned();
        assert_eq!(disc_label(&project, "trip"), "夏の旅行");
        // No name: the file name.
        project.settings.name = "  ".to_owned();
        assert_eq!(disc_label(&project, "trip"), "TRIP");
        // At most 30 characters (a UDF dstring of 32 bytes).
        project.settings.name = "x".repeat(40);
        assert_eq!(disc_label(&project, "trip").len(), 30);
    }

    #[test]
    fn cache_dir_follows_the_output() {
        let folder = BuildOptions {
            output: OutputKind::Folder,
            path: PathBuf::from("out"),
            cache: None,
            audio_language: None,
        };
        assert_eq!(cache_dir(&folder), Path::new("out").join("disccraft-cache"));
        let iso = BuildOptions {
            output: OutputKind::Iso,
            path: Path::new("out").join("movie.iso"),
            cache: None,
            audio_language: None,
        };
        // A cache of its own (shared by builds).
        let shared = BuildOptions {
            cache: Some(PathBuf::from("cache")),
            ..iso.clone()
        };
        assert_eq!(cache_dir(&shared), Path::new("cache"));
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
            cache: None,
            audio_language: None,
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
