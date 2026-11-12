//! DiscCraft DVD-Video VIDEO_TS builder for milestone M3.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod menus;
mod output;
mod prepare;
pub use prepare::{Estimate, TranscodeStatus, estimate, transcode_status};
mod source;
pub mod streams;
pub use streams::{
    SubtitleCue, SubtitleTrack, TitleAudio, TitleSource, TitleStreams, load_title_streams,
};
mod vmg;
mod vts;

use std::fs;
use std::path::{Path, PathBuf};

use disccraft_ifo::{IfoError, Standard, TitleEntry, VmCommand, VmgMenus};
use disccraft_nav::{ChapterPlaylistNav, Cmd, CompiledNav, NavError, TitleNav, compile};
use disccraft_project::{
    DiscKind, Project, Slideshow, Tick, Timeline, TimelineId, VideoStandard, has_errors,
};
use disccraft_psmux::es::{LpcmStream, VideoStream, parse_video, parse_wav};
use disccraft_psmux::mux::MuxError;
use thiserror::Error;

/// Errors returned while building a DVD-Video folder.
#[derive(Debug, Error)]
pub enum DvdError {
    /// Filesystem error.
    #[error("io error at {path:?}: {message}")]
    Io {
        /// Path involved in the failure.
        path: PathBuf,
        /// Human-readable message.
        message: String,
    },
    /// Input is understood but not supported.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// Input is invalid or inconsistent.
    #[error("invalid: {0}")]
    Invalid(String),
    /// Program stream muxing failed.
    #[error("mux error: {0}")]
    Mux(String),
    /// IFO/BUP writing or validation failed.
    #[error("ifo error: {0}")]
    Ifo(String),
}

impl From<MuxError> for DvdError {
    fn from(err: MuxError) -> Self {
        Self::Mux(err.to_string())
    }
}

impl From<IfoError> for DvdError {
    fn from(err: IfoError) -> Self {
        Self::Ifo(err.to_string())
    }
}

/// Maps navigation compilation errors to invalid project errors.
impl From<NavError> for DvdError {
    fn from(err: NavError) -> Self {
        Self::Invalid(err.to_string())
    }
}

/// Create an I/O error with path context.
pub(crate) fn io_error(path: &Path, err: std::io::Error) -> DvdError {
    DvdError::Io {
        path: path.to_path_buf(),
        message: err.to_string(),
    }
}

/// Convert navigation command bytes to IFO VM commands.
pub(crate) fn vm_commands(cmds: &[Cmd]) -> Vec<VmCommand> {
    cmds.iter().copied().map(VmCommand).collect()
}

/// Per-title build result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleReport {
    /// Timeline id for timeline titles, slideshow id for slideshow titles, chapter playlist id for chapter
    /// playlist titles.
    pub timeline: u64,
    /// One-based title number (the TT_SRPT entry).
    pub title: u8,
    /// One-based VTS number.
    pub vts: u8,
    /// Number of chapters / cells.
    pub chapters: usize,
    /// Title VOBS sector count.
    pub sectors: u32,
    /// Title duration in 90 kHz ticks.
    pub duration_90k: u32,
}

/// Complete VIDEO_TS build result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DvdBuildReport {
    /// Path to the written VIDEO_TS folder.
    pub video_ts: PathBuf,
    /// Titles in VTS order.
    pub titles: Vec<TitleReport>,
    /// Total bytes written.
    pub total_bytes: u64,
}

/// Built title reports and VMG inputs.
pub(crate) struct BuiltTitles {
    /// Per-title reports.
    pub(crate) reports: Vec<TitleReport>,
    /// VMG title search pointer entries.
    pub(crate) entries: Vec<TitleEntry>,
    /// VTS attribute blocks.
    pub(crate) attrs: Vec<Vec<u8>>,
    /// Total bytes written for all title sets.
    pub(crate) total_bytes: u64,
}

const MAX_INPUT_BYTES: u64 = 8_589_934_592;
const START_DTS: i64 = 8_100_000;
const MAX_CELLS: usize = 255;

/// Convert a project tick to a 27 MHz timestamp.
pub fn ticks_to_27m(t: Tick) -> i64 {
    t.0.checked_div(9408).unwrap_or(i64::MIN)
}

/// Convert allowed region codes to the DVD prohibited-region mask.
pub fn region_mask(regions: &[u8]) -> u8 {
    if regions.is_empty() {
        return 0;
    }

    let mut mask = 0xffu8;
    for &region in regions {
        if (1..=8).contains(&region)
            && let Some(bit) = 1u8.checked_shl(u32::from(region - 1))
        {
            mask &= !bit;
        }
    }
    mask
}

/// Build a DVD-Video folder from a project.
///
/// # Errors
///
/// Returns [`DvdError::Invalid`] for invalid projects or navigation,
/// [`DvdError::Unsupported`] for unsupported media, and I/O, mux, or IFO
/// errors when writing fails.
pub fn build_video_ts(project: &Project, out_dir: &Path) -> Result<DvdBuildReport, DvdError> {
    build_video_ts_with(
        project,
        out_dir,
        &out_dir.join("disccraft-cache"),
        &mut |_, _| {},
    )
}

/// Like [`build_video_ts`], with an explicit transcode cache folder and a progress callback
/// (`(asset name, fraction 0..=1)` while an asset is being transcoded).
pub fn build_video_ts_with(
    project: &Project,
    out_dir: &Path,
    cache_dir: &Path,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<DvdBuildReport, DvdError> {
    let (title_count, vts_count) = validate_project(project)?;
    let mut prep = PrepareCtx {
        cache_dir,
        budget: prepare::budget(project)?,
        progress,
    };
    let nav = compile(project)?;

    let video_ts = output::prepare_video_ts(out_dir)?;
    let standard = ifo_standard(project.settings.standard);

    let menus = build_menus_if_needed(project, &nav, &video_ts, standard, cache_dir)?;
    let menu_bytes = menus.as_ref().map_or(0, |(_, bytes)| *bytes);
    let menus = menus.map(|(menus, _)| menus);

    let vmg_set_sectors = vmg::vmg_set_sectors(title_count, vts_count, menus.as_ref())?;
    let built = build_titles(
        project,
        &nav,
        &video_ts,
        standard,
        vmg_set_sectors,
        &mut prep,
    )?;

    let first_play = vm_commands(&nav.first_play);
    let vmg_bytes = vmg::write_vmg(
        &video_ts,
        &built.entries,
        &built.attrs,
        (standard, project.menus_wide()),
        (&first_play, nav.first_play_prohibited_ops),
        menus,
        &project.settings.regions,
    )?;

    let total_bytes = built
        .total_bytes
        .checked_add(menu_bytes)
        .and_then(|x| x.checked_add(vmg_bytes))
        .ok_or_else(|| DvdError::Invalid("total byte count overflow".to_string()))?;

    Ok(DvdBuildReport {
        video_ts,
        titles: built.reports,
        total_bytes,
    })
}

/// Validate project-level constraints. Returns the title count (timelines, slideshows and chapter playlists)
/// and the title set count (timelines and slideshows: a chapter playlist shares its master's title set).
fn validate_project(project: &Project) -> Result<(usize, usize), DvdError> {
    if project.settings.kind != DiscKind::Dvd {
        return Err(DvdError::Unsupported(
            "only DVD-Video projects are supported".to_string(),
        ));
    }

    let problems = project.check();
    if has_errors(&problems) {
        let mut message = String::from("project check failed:");
        for problem in &problems {
            message.push_str(&format!(" {problem:?}"));
        }
        return Err(DvdError::Invalid(message));
    }

    let vts_count = project
        .timelines
        .len()
        .checked_add(project.slideshows.len())
        .ok_or_else(|| DvdError::Invalid("too many titles".to_string()))?;
    let title_count = vts_count
        .checked_add(project.chapter_playlists.len())
        .ok_or_else(|| DvdError::Invalid("too many titles".to_string()))?;

    if vts_count == 0 || title_count > 99 {
        return Err(DvdError::Invalid(
            "DVD projects require between 1 and 99 titles".to_string(),
        ));
    }

    Ok((title_count, vts_count))
}

/// Build menu tables only when menus or hidden PGCs exist.
fn build_menus_if_needed(
    project: &Project,
    nav: &CompiledNav,
    video_ts: &Path,
    standard: Standard,
    cache_dir: &Path,
) -> Result<Option<(VmgMenus, u64)>, DvdError> {
    if project.menus.is_empty() && nav.hidden.is_empty() && nav.menus.is_empty() {
        return Ok(None);
    }

    let (menus, bytes) = menus::build_menus(project, nav, video_ts, standard, cache_dir)?;
    Ok(Some((menus, bytes)))
}

/// Inputs for turning assets into DVD-compliant streams (see `prepare.rs`).
struct PrepareCtx<'a> {
    cache_dir: &'a Path,
    budget: prepare::Budget,
    progress: &'a mut dyn FnMut(&str, f32),
}

struct BuildAccumulator {
    next_start: u32,
    reports: Vec<TitleReport>,
    /// Title search pointer entries with their title numbers (title sets are built in VTS order, which is not
    /// title order once chapter playlists join their master's title set).
    entries: Vec<(u8, TitleEntry)>,
    attrs: Vec<Vec<u8>>,
    total_bytes: u64,
    index: usize,
}

fn build_titles(
    project: &Project,
    nav: &CompiledNav,
    video_ts: &Path,
    standard: Standard,
    vmg_set_sectors: u32,
    prep: &mut PrepareCtx<'_>,
) -> Result<BuiltTitles, DvdError> {
    let total_titles = project
        .timelines
        .len()
        .checked_add(project.slideshows.len())
        .ok_or_else(|| DvdError::Invalid("too many titles".to_string()))?;

    let mut acc = BuildAccumulator {
        next_start: vmg_set_sectors,
        reports: Vec::with_capacity(total_titles),
        entries: Vec::with_capacity(total_titles),
        attrs: Vec::with_capacity(total_titles),
        total_bytes: 0,
        index: 0,
    };

    build_timeline_titles(project, nav, video_ts, standard, prep, &mut acc)?;
    build_slideshow_titles(project, nav, video_ts, standard, prep, &mut acc)?;

    acc.entries.sort_by_key(|(title, _)| *title);
    for (index, (title, _)) in acc.entries.iter().enumerate() {
        if usize::from(*title) != index.saturating_add(1) {
            return Err(DvdError::Invalid(
                "title numbers are not contiguous".to_string(),
            ));
        }
    }
    acc.reports.sort_by_key(|report| report.title);

    Ok(BuiltTitles {
        reports: acc.reports,
        entries: acc.entries.into_iter().map(|(_, entry)| entry).collect(),
        attrs: acc.attrs,
        total_bytes: acc.total_bytes,
    })
}

fn build_timeline_titles(
    project: &Project,
    nav: &CompiledNav,
    video_ts: &Path,
    standard: Standard,
    prep: &mut PrepareCtx<'_>,
    acc: &mut BuildAccumulator,
) -> Result<(), DvdError> {
    for timeline in project.timelines.values() {
        let vts = next_vts(acc)?;
        let prepared = prepare::prepare_title(
            project,
            timeline,
            prep.cache_dir,
            &prep.budget,
            prep.progress,
        )?;
        let loaded = source::load_title(project, timeline, project.settings.standard, &prepared)?;
        let title_nav = nav_title(nav, timeline)?;
        let extras: Vec<&ChapterPlaylistNav> = nav
            .chapter_playlists
            .iter()
            .filter(|extra| extra.timeline == timeline.id)
            .collect();
        let built = vts::build_title_set(&loaded, video_ts, vts, standard, title_nav, &extras)?;

        let start = record_built(
            acc,
            timeline.id.0,
            title_nav.title,
            vts,
            built.title_set_sectors,
            built.chapters,
            built.chapters_u16,
            built.vobs_sectors,
            built.duration_90k,
            built.vts_attr_block,
            built.total_bytes,
        )?;
        for (extra, nav_extra) in built.extras.iter().zip(&extras) {
            acc.reports.push(TitleReport {
                timeline: nav_extra.chapter_playlist,
                title: nav_extra.title,
                vts,
                chapters: extra.chapters,
                sectors: built.vobs_sectors,
                duration_90k: extra.duration_90k,
            });
            acc.entries.push((
                nav_extra.title,
                TitleEntry {
                    chapters: extra.chapters_u16,
                    vts,
                    vts_ttn: nav_extra.vts_ttn,
                    vts_start_sector: start,
                },
            ));
        }
    }

    Ok(())
}

fn build_slideshow_titles(
    project: &Project,
    nav: &CompiledNav,
    video_ts: &Path,
    standard: Standard,
    prep: &mut PrepareCtx<'_>,
    acc: &mut BuildAccumulator,
) -> Result<(), DvdError> {
    for id in slideshow_ids(project) {
        let vts = next_vts(acc)?;
        let slideshow = project
            .slideshows
            .get(&id)
            .ok_or_else(|| DvdError::Invalid("slideshow does not exist".to_string()))?;

        let prepared = prepare::prepare_slideshow(
            project,
            slideshow,
            prep.cache_dir,
            &prep.budget,
            prep.progress,
        )?;
        let loaded =
            load_slideshow_title(project, slideshow, project.settings.standard, &prepared)?;
        let title_nav = slideshow_title_nav(nav, id)?;
        let built = vts::build_title_set(&loaded, video_ts, vts, standard, &title_nav, &[])?;

        let _ = record_built(
            acc,
            slideshow.id,
            title_nav.title,
            vts,
            built.title_set_sectors,
            built.chapters,
            built.chapters_u16,
            built.vobs_sectors,
            built.duration_90k,
            built.vts_attr_block,
            built.total_bytes,
        )?;
    }

    Ok(())
}

fn next_vts(acc: &mut BuildAccumulator) -> Result<u8, DvdError> {
    let vts = vts_number(acc.index)?;
    acc.index = acc
        .index
        .checked_add(1)
        .ok_or_else(|| DvdError::Invalid("title index overflow".to_string()))?;
    Ok(vts)
}

/// Records a built title set and its first title; returns the title set's start sector.
#[allow(clippy::too_many_arguments)]
fn record_built(
    acc: &mut BuildAccumulator,
    timeline_id: u64,
    title: u8,
    vts: u8,
    title_set_sectors: u32,
    chapters: usize,
    chapters_u16: u16,
    vobs_sectors: u32,
    duration_90k: u32,
    vts_attr_block: Vec<u8>,
    built_total_bytes: u64,
) -> Result<u32, DvdError> {
    let start = acc.next_start;
    acc.next_start = acc
        .next_start
        .checked_add(title_set_sectors)
        .ok_or_else(|| DvdError::Invalid("disc sector count overflow".to_string()))?;

    acc.reports.push(TitleReport {
        timeline: timeline_id,
        title,
        vts,
        chapters,
        sectors: vobs_sectors,
        duration_90k,
    });

    acc.entries.push((
        title,
        TitleEntry {
            chapters: chapters_u16,
            vts,
            vts_ttn: 1,
            vts_start_sector: start,
        },
    ));

    acc.attrs.push(vts_attr_block);

    acc.total_bytes = acc
        .total_bytes
        .checked_add(built_total_bytes)
        .ok_or_else(|| DvdError::Invalid("total byte count overflow".to_string()))?;

    Ok(start)
}

/// Find compiled navigation for one timeline.
fn nav_title<'a>(nav: &'a CompiledNav, timeline: &Timeline) -> Result<&'a TitleNav, DvdError> {
    nav.titles
        .iter()
        .find(|title| title.timeline == timeline.id)
        .ok_or_else(|| DvdError::Invalid("missing compiled title".to_string()))
}

fn slideshow_title_nav(nav: &CompiledNav, slideshow: u64) -> Result<TitleNav, DvdError> {
    let entry = nav
        .slideshows
        .iter()
        .find(|entry| entry.slideshow == slideshow)
        .ok_or_else(|| DvdError::Invalid("missing compiled slideshow".to_string()))?;

    Ok(TitleNav {
        timeline: TimelineId(slideshow),
        title: entry.title,
        pre: entry.pre.clone(),
        post: entry.post.clone(),
        // Slideshows have no menu remote.
        menu_remote: Vec::new(),
        prohibited_ops: 0,
    })
}

pub(crate) fn slideshow_ids(project: &Project) -> Vec<u64> {
    let mut ids: Vec<u64> = project.slideshows.keys().copied().collect();
    ids.sort_unstable();
    ids
}

pub(crate) fn load_slideshow_title(
    project: &Project,
    slideshow: &Slideshow,
    standard: VideoStandard,
    prepared: &prepare::PreparedTitle,
) -> Result<source::LoadedTitle, DvdError> {
    let video_data = read_path(&prepared.video_path)?;
    let video = parse_video(&video_data, START_DTS)
        .map_err(|err| DvdError::Invalid(format!("video parse error: {err}")))?;

    validate_video(&video, standard)?;
    let first_pts = first_pts(&video)?;
    let durations = prepare::effective_slide_durations(project, slideshow)?;
    let cell_starts = slideshow_cell_starts(&durations)?;

    let mut audio = Vec::new();
    let wav = prepared
        .transcoded_audio
        .as_ref()
        .ok_or_else(|| DvdError::Invalid("slideshow audio missing".to_string()))?;
    let stream = parse_wav(&read_path(wav)?)
        .map_err(|err| DvdError::Invalid(format!("slideshow audio: {err}")))?;
    validate_lpcm(&stream)?;
    audio.push(source::LoadedAudio::Lpcm {
        stream,
        language: None,
    });

    Ok(source::LoadedTitle {
        video,
        video_data,
        audio,
        cell_starts,
        first_pts,
        ntsc: standard == VideoStandard::Ntsc,
        wide: project.settings.widescreen,
        subpictures: Vec::new(),
        subtitle_languages: Vec::new(),
        // 手動で進める: 各スライドのセルを無限の静止にして、リモコンの次へ / 決定で進む。
        cell_still: if slideshow.manual_advance { 0xFF } else { 0 },
    })
}

/// The cells' start times (27 MHz) from the slides' (built) durations.
fn slideshow_cell_starts(durations: &[Tick]) -> Result<Vec<i64>, DvdError> {
    if durations.is_empty() {
        return Err(DvdError::Invalid("slideshow has no slides".to_string()));
    }
    if durations.len() > MAX_CELLS {
        return Err(DvdError::Unsupported("too many slides".to_string()));
    }

    let mut starts = Vec::with_capacity(durations.len());
    let mut elapsed = 0i64;

    for (index, duration) in durations.iter().enumerate() {
        if duration.0 < 0 {
            return Err(DvdError::Invalid(
                "slide duration must not be negative".to_string(),
            ));
        }

        if index == 0 {
            starts.push(0);
        } else {
            let time = ticks_to_27m(Tick(elapsed));
            if time < 0 {
                return Err(DvdError::Invalid("slide start time overflow".to_string()));
            }
            if let Some(prev) = starts.last().copied()
                && prev >= time
            {
                return Err(DvdError::Invalid(
                    "slide start times must be strictly increasing".to_string(),
                ));
            }
            starts.push(time);
        }

        elapsed = elapsed
            .checked_add(duration.0)
            .ok_or_else(|| DvdError::Invalid("slide duration overflow".to_string()))?;
    }

    Ok(starts)
}

fn validate_video(video: &VideoStream, standard: VideoStandard) -> Result<(), DvdError> {
    let sequence = &video.sequence;
    let (want_width, want_height, want_rate) = match standard {
        VideoStandard::Ntsc => (720u16, 480u16, 4u8),
        VideoStandard::Pal => (720u16, 576u16, 3u8),
    };

    if sequence.width != want_width {
        return Err(DvdError::Unsupported(format!(
            "video width {} is not {want_width} for {standard:?}",
            sequence.width
        )));
    }
    if sequence.height != want_height {
        return Err(DvdError::Unsupported(format!(
            "video height {} is not {want_height} for {standard:?}",
            sequence.height
        )));
    }
    if sequence.frame_rate_code != want_rate {
        return Err(DvdError::Unsupported(format!(
            "video frame_rate_code {} is not {want_rate} for {standard:?}",
            sequence.frame_rate_code
        )));
    }
    if sequence.aspect_code != 2 && sequence.aspect_code != 3 {
        return Err(DvdError::Unsupported(
            "DVD video must be 4:3 (aspect_code 2) or 16:9 (aspect_code 3)".to_string(),
        ));
    }

    let bit_rate = u64::from(sequence.bit_rate_400)
        .checked_mul(400)
        .ok_or_else(|| DvdError::Invalid("video bit rate overflow".to_string()))?;
    if bit_rate > 9_800_000 {
        return Err(DvdError::Unsupported(format!(
            "video bit rate {bit_rate} exceeds DVD maximum 9800000"
        )));
    }

    Ok(())
}

fn first_pts(video: &VideoStream) -> Result<i64, DvdError> {
    video
        .pictures
        .iter()
        .map(|picture| picture.pts)
        .min()
        .ok_or_else(|| DvdError::Invalid("video has no pictures".to_string()))
}

fn validate_lpcm(stream: &LpcmStream) -> Result<(), DvdError> {
    if stream.sample_rate != 48_000 {
        return Err(DvdError::Unsupported(format!(
            "LPCM sample rate {} is not 48 kHz",
            stream.sample_rate
        )));
    }
    if stream.bits != 16 {
        return Err(DvdError::Unsupported(
            "M2 supports only 16-bit LPCM".to_string(),
        ));
    }

    let channels = source::channels_u8(stream.channels)?;
    if channels == 0 || channels > 8 {
        return Err(DvdError::Unsupported(
            "LPCM channel count must be 1..=8".to_string(),
        ));
    }
    if stream.frame_bytes == 0 {
        return Err(DvdError::Invalid("LPCM frame size is zero".to_string()));
    }

    Ok(())
}

fn read_path(path: &Path) -> Result<Vec<u8>, DvdError> {
    let metadata = fs::metadata(path).map_err(|err| crate::io_error(path, err))?;

    if !metadata.is_file() {
        return Err(DvdError::Invalid(format!(
            "asset is not a file: {}",
            path.display()
        )));
    }
    if metadata.len() > MAX_INPUT_BYTES {
        return Err(DvdError::Unsupported(format!(
            "asset larger than 8 GiB: {}",
            path.display()
        )));
    }

    fs::read(path).map_err(|err| crate::io_error(path, err))
}

/// Convert a zero-based timeline index to a one-based VTS number.
fn vts_number(index: usize) -> Result<u8, DvdError> {
    let value = index
        .checked_add(1)
        .ok_or_else(|| DvdError::Invalid("timeline index overflow".to_string()))?;

    u8::try_from(value)
        .map_err(|_| DvdError::Invalid("too many timelines for one byte VTS ids".to_string()))
}

/// Convert the project video standard to the IFO standard.
fn ifo_standard(standard: VideoStandard) -> Standard {
    match standard {
        VideoStandard::Ntsc => Standard::Ntsc,
        VideoStandard::Pal => Standard::Pal,
    }
}

#[cfg(test)]
mod tests {
    use super::{region_mask, slideshow_cell_starts, ticks_to_27m};
    use disccraft_project::{Project, Tick};
    use std::error::Error;
    use std::path::Path;

    const TICKS_PER_SECOND: i64 = 254_016_000_000;

    #[test]
    fn ticks_to_27m_one_second() {
        assert_eq!(ticks_to_27m(Tick(254_016_000_000)), 27_000_000);
    }

    #[test]
    fn region_masks() {
        assert_eq!(region_mask(&[]), 0x00);
        assert_eq!(region_mask(&[2]), 0xFD);
    }

    #[test]
    fn slideshow_cell_starts_cumulative() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(Path::new("slide.png"))?;
        let id = project.add_slideshow(None)?;

        project.add_slide(id, asset, Tick(2 * TICKS_PER_SECOND))?;
        project.add_slide(id, asset, Tick(3 * TICKS_PER_SECOND))?;
        project.add_slide(id, asset, Tick(4 * TICKS_PER_SECOND))?;

        let starts = {
            let slideshow = project.slideshow(id).ok_or("missing slideshow")?;
            let durations: Vec<Tick> = slideshow
                .slides
                .iter()
                .map(|slide| slide.duration)
                .collect();
            slideshow_cell_starts(&durations)?
        };

        assert_eq!(
            starts,
            vec![
                0,
                ticks_to_27m(Tick(2 * TICKS_PER_SECOND)),
                ticks_to_27m(Tick(5 * TICKS_PER_SECOND)),
            ]
        );

        Ok(())
    }
}
