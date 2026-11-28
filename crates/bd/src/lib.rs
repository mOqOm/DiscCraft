//! プロジェクト → BDMV。
//!
//! 層: L2（docs/architecture.md §5）。
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod clip;
mod hd;
mod menus;
mod nav;
mod pages;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use disccraft_bdmv::{index, mobj, mpls};
use disccraft_dvd::{TitleSource, TitleStreams, load_title_streams, load_title_streams_for_hd};
use disccraft_project::{
    DiscKind, Link, Menu, MenuLoops, PlaylistItem, Project, TimelineId, VideoStandard,
};

use clip::BuiltClip;
use menus::{MenuClip, MenuDirs};
mod uo;

pub use menus::PAGE_PICTURE_BUTTON;
use nav::{MenuMotion, MenuNav, TitleMap, TitleNav};
pub use pages::{menu_page_groups, popup_page_groups};

/// Errors produced while building a BDMV folder.
#[derive(Debug, thiserror::Error)]
pub enum BdError {
    /// The project cannot be represented as a Blu-ray.
    #[error("invalid project: {0}")]
    Invalid(String),

    /// A media or navigation feature is not supported.
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// Preparing or loading title streams failed.
    #[error("{0}")]
    Prepare(String),

    /// Multiplexing elementary streams failed.
    #[error("mux: {0}")]
    Mux(String),

    /// Writing a BDMV table failed.
    #[error("bdmv: {0}")]
    Bdmv(String),

    /// File system access failed.
    #[error("{path}: {message}")]
    Io {
        /// Path involved in the failure.
        path: PathBuf,
        /// Operating-system error message.
        message: String,
    },
}

/// Report for one generated Blu-ray title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BdTitleReport {
    /// Title number, 1-based.
    pub title: u16,
    /// Playlist number used by this title.
    pub playlist: u16,
    /// Number of chapter marks.
    pub chapters: usize,
    /// Title duration in 90 kHz units.
    pub duration_90k: u64,
}

/// Report for a complete BDMV build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BdBuildReport {
    /// Path to the generated `BDMV` folder.
    pub bdmv: PathBuf,
    /// Reports for all generated titles.
    pub titles: Vec<BdTitleReport>,
    /// Total size of all written files in bytes.
    pub total_bytes: u64,
}

/// Build a BDMV folder using a cache directory inside `out_dir`.
///
/// # Errors
///
/// Returns [`BdError`] if the project is invalid, media cannot be prepared, muxing fails,
/// BDMV tables cannot be written, or file system access fails.
pub fn build_bdmv(project: &Project, out_dir: &Path) -> Result<BdBuildReport, BdError> {
    let cache_dir = out_dir.join("disccraft-cache");
    build_bdmv_with(
        project,
        out_dir,
        &cache_dir,
        &mut |_name: &str, _progress: f32| {},
    )
}

/// Whether the HD video a build of `project` makes from `asset` is already in `cache_dir` (see
/// [`transcode_now`]): every timeline clip that plays it, encoded for the project's format and bitrates.
#[must_use]
pub fn hd_transcoded(
    project: &Project,
    cache_dir: &Path,
    asset: disccraft_project::AssetId,
) -> bool {
    project.settings.kind == DiscKind::BluRay && hd::hd_transcoded(project, cache_dir, asset)
}

/// Encore's Revert to Original for a Blu-ray: removes `asset`'s transcodes from `cache_dir` (the HD video of the
/// clips that play it, its converted sound, and its SD video on an SD disc); returns how many files went.
///
/// # Errors
///
/// When the cache folder cannot be read.
pub fn revert_transcodes(
    project: &Project,
    cache_dir: &Path,
    asset: disccraft_project::AssetId,
) -> Result<usize, BdError> {
    let hd = if project.settings.bd_encodes_titles() {
        hd::revert_hd(project, cache_dir, asset)
    } else {
        0
    };
    disccraft_dvd::revert_transcodes(project, cache_dir, asset)
        .map(|removed| removed + hd)
        .map_err(|error| BdError::Prepare(error.to_string()))
}

/// Encore's Transcode Now for a Blu-ray: makes the video and sound a build of `project` would make and keeps them
/// in `cache_dir`, where the build then finds them. With `only`, just those of the timelines that play that asset.
///
/// # Errors
///
/// As [`build_bdmv_with`]'s transcodes.
pub fn transcode_now(
    project: &Project,
    cache_dir: &Path,
    only: Option<disccraft_project::AssetId>,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<(), BdError> {
    if project.settings.kind != DiscKind::BluRay {
        return Err(invalid("project is not a Blu-ray project"));
    }
    // An SD disc's MPEG-2 is a DVD's; HD and SD H.264 (M10.41) are made here.
    if !project.settings.bd_encodes_titles() {
        return disccraft_dvd::transcode_now(project, cache_dir, only, progress)
            .map_err(|error| BdError::Prepare(error.to_string()));
    }
    // The rates are the whole project's; the titles made are the ones asked for.
    let hd_budget =
        disccraft_dvd::hd_budget(project).map_err(|error| BdError::Prepare(error.to_string()))?;
    let mut wanted = project.clone();
    if let Some(asset) = only {
        wanted
            .timelines
            .retain(|_, timeline| disccraft_dvd::timeline_uses(timeline, asset));
        wanted.slideshows.clear();
    }
    let titles = load_title_streams_for_hd(&wanted, cache_dir, progress)
        .map_err(|error| BdError::Prepare(error.to_string()))?;
    for title in &titles {
        let rates = hd::title_rates(&wanted, title, hd_budget.as_ref());
        let _ = hd::hd_title(&wanted, title, cache_dir, rates, progress)?;
    }
    Ok(())
}

/// File ▸ Render ▸ Slideshow on a Blu-ray: transcodes the slideshows (`only`: one of them) into `cache_dir` as a
/// build would (SD: as a DVD's; HD: at the disc's HD size and the rate the whole project gives its titles).
/// Returns how many.
///
/// # Errors
///
/// When a slideshow cannot be made.
pub fn render_slideshows(
    project: &Project,
    cache_dir: &Path,
    only: Option<u64>,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<usize, BdError> {
    if !project.settings.bd_encodes_titles() {
        return disccraft_dvd::render_slideshows(project, cache_dir, only, progress)
            .map_err(|error| BdError::Prepare(error.to_string()));
    }
    let hd_budget =
        disccraft_dvd::hd_budget(project).map_err(|error| BdError::Prepare(error.to_string()))?;
    let mut wanted = project.clone();
    wanted.timelines.clear();
    wanted.chapter_playlists.clear();
    wanted
        .slideshows
        .retain(|id, _| only.is_none_or(|wanted| *id == wanted));
    let count = wanted.slideshows.len();
    let titles = load_title_streams_for_hd(&wanted, cache_dir, progress)
        .map_err(|error| BdError::Prepare(error.to_string()))?;
    for title in &titles {
        let rates = hd::title_rates(&wanted, title, hd_budget.as_ref());
        let _ = hd::hd_title(&wanted, title, cache_dir, rates, progress)?;
    }
    Ok(count)
}

/// File ▸ Render ▸ Motion Menu: encodes the motion menus (`only`: one of them) into `cache_dir` as the build would
/// (SD: as a DVD's; HD: at the disc's HD size and its menus' rate, M6.387), so the build takes them as they are and
/// the project preview plays them. Returns how many.
///
/// # Errors
///
/// When a menu cannot be encoded.
pub fn render_motion_menus(
    project: &Project,
    cache_dir: &Path,
    only: Option<disccraft_project::MenuId>,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<usize, BdError> {
    let (choice, video) = (project.settings.bd_codec, project.settings.bd_video);
    match hd::options(choice, video, hd::menu_rates(choice, video)) {
        Some(options) => {
            disccraft_dvd::render_hd_motion_menus(project, cache_dir, only, &options, progress)
        }
        None => disccraft_dvd::render_motion_menus(project, cache_dir, only, progress),
    }
    .map_err(|error| BdError::Prepare(error.to_string()))
}

/// The motion menu `menu` rendered into `cache_dir` (File ▸ Render ▸ Motion Menu, or a build) and still the menu as
/// it is, for the project preview to play: an SD disc's as a DVD's (its MPEG-2 video), an HD disc's as its pictures
/// in a transport stream (M6.387).
#[must_use]
pub fn rendered_motion_menu(
    project: &Project,
    menu: disccraft_project::MenuId,
    cache_dir: &Path,
) -> Option<std::path::PathBuf> {
    let (choice, video) = (project.settings.bd_codec, project.settings.bd_video);
    match hd::options(choice, video, hd::menu_rates(choice, video)) {
        Some(options) => disccraft_dvd::rendered_hd_motion_menu(project, menu, cache_dir, &options),
        None => disccraft_dvd::rendered_motion_menu(project, menu, cache_dir),
    }
}

/// Build a BDMV folder with an explicit cache directory and progress callback.
///
/// # Errors
///
/// Returns [`BdError`] if the project is invalid, media cannot be prepared, muxing fails,
/// BDMV tables cannot be written, or file system access fails.
pub fn build_bdmv_with(
    project: &Project,
    out_dir: &Path,
    cache_dir: &Path,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<BdBuildReport, BdError> {
    if project.settings.kind != DiscKind::BluRay {
        return Err(invalid("project is not a Blu-ray project"));
    }
    // M6.548: the videos that play in place of parental chapters, as hidden titles (their clips).
    let (expanded, parental_clips) = project
        .with_parental_clips()
        .map_err(|error| invalid(&error.to_string()))?;
    let project = &expanded;

    let dirs = prepare_directories(out_dir)?;
    // In HD (and in SD as H.264, M10.41) the timelines' video is encoded below; their SD MPEG-2 would be made only to
    // be thrown away.
    let titles = if project.settings.bd_encodes_titles() {
        load_title_streams_for_hd(project, cache_dir, progress)
    } else {
        load_title_streams(project, cache_dir, progress)
    }
    .map_err(|error| BdError::Prepare(error.to_string()))?;

    // The HD video's bitrate: the format's usual one, or less when the titles would not fit the disc at it (each
    // title at its own format's codec's).
    let hd_budget =
        disccraft_dvd::hd_budget(project).map_err(|error| BdError::Prepare(error.to_string()))?;
    let hd_titles = hd::hd_titles(project, &titles, cache_dir, hd_budget.as_ref(), progress)?;

    let mut output = BuildOutput::default();
    let pending = write_title_clips(project, (&titles, &hd_titles), &dirs, &mut output)?;
    let next_title = write_project_playlists(project, &mut output)?;
    let first_playlist = u16_from_usize(
        output
            .built
            .len()
            .checked_add(1)
            .ok_or_else(|| invalid("too many titles"))?,
        "playlist number",
    )?;
    let next_playlist =
        write_chapter_playlists(project, &dirs, &mut output, (next_title, first_playlist))?;
    let next_playlist =
        write_parental_versions(project, &parental_clips, &dirs, &mut output, next_playlist)?;
    let menu_navs = build_menu_navs(project, next_playlist)?;

    let map = TitleMap {
        timelines: output.timeline_titles.clone(),
        slideshows: output.slideshow_titles.clone(),
        playlists: output.playlist_titles.clone(),
        chapter_playlists: output.chapter_playlist_titles.clone(),
        disc_age: disccraft_project::bd_parental_age(project.settings.parental_level),
    };
    let first_streams: Vec<disccraft_bdmv::mobj::NavCommand> = stream_command(
        project.settings.audio_default,
        project.settings.subtitle_default,
    )
    .into_iter()
    .collect();
    let first_override = effective_override(
        &project.settings.first_play,
        project.settings.first_play_override.as_ref(),
    );
    let navigation = nav::build_navigation_with_top(
        (
            &project.settings.first_play,
            &first_streams,
            first_override.as_ref(),
        ),
        &project.settings.title_remote,
        &output.navs,
        &menu_navs,
        &map,
    )?;

    write_popup_titles(project, &dirs, &mut output, &navigation, pending)?;
    write_menus(
        project,
        &dirs,
        &mut output,
        &navigation,
        (next_playlist, cache_dir),
    )?;
    write_navigation(project, &dirs, &mut output, navigation)?;
    copy_backups(&dirs, &mut output)?;

    Ok(BdBuildReport {
        bdmv: dirs.bdmv,
        titles: output.reports,
        total_bytes: output.total_bytes,
    })
}

struct Directories {
    bdmv: PathBuf,
    playlist: PathBuf,
    clipinf: PathBuf,
    stream: PathBuf,
    backup: PathBuf,
}

#[derive(Default)]
struct BuildOutput {
    reports: Vec<BdTitleReport>,
    navs: Vec<TitleNav>,
    built: Vec<BuiltClip>,
    timeline_titles: BTreeMap<u64, u16>,
    slideshow_titles: BTreeMap<u64, u16>,
    playlist_titles: BTreeMap<u64, u16>,
    chapter_playlist_titles: BTreeMap<u64, u16>,
    /// Each title's UO mask (its timeline's and the disc's prohibited operations), by title number.
    title_uo: BTreeMap<u16, u64>,
    /// The titles that are slideshows advanced by hand (Encore's 手動で進める): a still after every slide.
    /// The slideshow titles with slides that are advanced by hand: which of each title's slides are held.
    manual_titles: std::collections::BTreeMap<u16, Vec<bool>>,
    backup_files: Vec<(PathBuf, PathBuf, u64)>,
    total_bytes: u64,
}

fn prepare_directories(out_dir: &Path) -> Result<Directories, BdError> {
    let bdmv = out_dir.join("BDMV");
    remove_existing(&bdmv)?;

    let playlist = bdmv.join("PLAYLIST");
    let clipinf = bdmv.join("CLIPINF");
    let stream = bdmv.join("STREAM");
    let auxdata = bdmv.join("AUXDATA");
    let bdjo = bdmv.join("BDJO");
    let jar = bdmv.join("JAR");
    let meta = bdmv.join("META");
    let backup = bdmv.join("BACKUP");
    let backup_playlist = backup.join("PLAYLIST");
    let backup_clipinf = backup.join("CLIPINF");
    let backup_bdjo = backup.join("BDJO");
    // Beside BDMV: the certificate directory of a disc (empty on an unprotected one), which players look for.
    let certificate = out_dir.join("CERTIFICATE");
    let certificate_backup = certificate.join("BACKUP");

    let dirs = [
        playlist.as_path(),
        clipinf.as_path(),
        stream.as_path(),
        auxdata.as_path(),
        bdjo.as_path(),
        jar.as_path(),
        meta.as_path(),
        backup.as_path(),
        backup_playlist.as_path(),
        backup_clipinf.as_path(),
        backup_bdjo.as_path(),
        certificate.as_path(),
        certificate_backup.as_path(),
    ];

    for dir in dirs {
        std::fs::create_dir_all(dir).map_err(|error| io_error(dir, error))?;
    }

    Ok(Directories {
        bdmv,
        playlist,
        clipinf,
        stream,
        backup,
    })
}

fn remove_existing(path: &Path) -> Result<(), BdError> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(path, error)),
    }
}

/// A title clip waiting for its pop-up menu (whose button commands exist once the navigation is built).
struct PendingPopup {
    clip: clip::PreparedClip,
    /// The pop-up's pages: their menus' indices among the project's menus, the first page first (just the menu
    /// unless its pages pop up together, [`pages::popup_page_groups`]).
    pages: Vec<usize>,
}

/// Writes every title clip and its playlist. A timeline whose remote menu is a menu gets that menu as a pop-up:
/// its clip is only prepared here and written by [`write_popup_titles`], after the navigation (and so the menu's
/// button commands) exists.
fn write_title_clips(
    project: &Project,
    (titles, hd_titles): (&[TitleStreams], &[Option<hd::HdTitle>]),
    dirs: &Directories,
    output: &mut BuildOutput,
) -> Result<Vec<PendingPopup>, BdError> {
    let mut pending = Vec::new();
    let popup_groups = pages::popup_page_groups(project);
    for (index, title) in titles.iter().enumerate() {
        // Progress panel ▸ Stop while the titles are multiplexed: before the next title's clip is written.
        if disccraft_transcode::stop_requested() {
            return Err(BdError::Prepare(disccraft_transcode::STOPPED.to_string()));
        }
        let hd = hd_titles.get(index).and_then(Option::as_ref);
        let number = u16_from_usize(
            index
                .checked_add(1)
                .ok_or_else(|| invalid("too many titles"))?,
            "title number",
        )?;
        if let TitleSource::Slideshow(id) = title.source
            && let Some(held) = project
                .slideshows
                .get(&id)
                .map(disccraft_project::Slideshow::held_slides)
                .filter(|held| held.contains(&true))
        {
            let _ = output.manual_titles.insert(number, held);
        }
        let own: &[disccraft_project::UserOp] = match title.source {
            TitleSource::Timeline(id) => project
                .timelines
                .get(&TimelineId(id))
                .map_or(&[], |timeline| timeline.prohibited_ops.as_slice()),
            TitleSource::Slideshow(_) => &[],
        };
        let _ = output.title_uo.insert(
            number,
            uo::uo_mask(&[own, &project.settings.prohibited_ops]),
        );
        let popup = popup_pages_of(project, title, &popup_groups);
        let built = match popup {
            Some(pages) => {
                let prepared = clip::prepare_clip(title, hd, number, &project.settings)?;
                let planned = prepared.planned()?;
                pending.push(PendingPopup {
                    clip: prepared,
                    pages,
                });
                planned
            }
            None => {
                let built = clip::write_clip(
                    title,
                    hd,
                    number,
                    &project.settings,
                    (&dirs.stream, &dirs.clipinf),
                )?;
                add_total(output, built.bytes)?;
                record_title_files(dirs, output, number, &built)?;
                built
            }
        };

        output.reports.push(BdTitleReport {
            title: number,
            playlist: number,
            chapters: title.chapter_starts.len(),
            duration_90k: built.duration_90k,
        });
        push_title_nav(project, title, number, output)?;
        output.built.push(built);
    }

    Ok(pending)
}

/// The menu a timeline title shows on the remote's Pop-up Menu key (Encore's timeline "Remote Menu"), as the indices
/// of its pages among the project's menus: all of a pop-up whose pages pop up together (one of `groups`, from its
/// first page), else the menu alone.
fn popup_pages_of(
    project: &Project,
    title: &TitleStreams,
    groups: &[Vec<usize>],
) -> Option<Vec<usize>> {
    let TitleSource::Timeline(id) = title.source else {
        return None;
    };
    let timeline = project.timelines.get(&TimelineId(id))?;
    let Link::Menu { id: menu_id, .. } = timeline.menu_remote else {
        return None;
    };
    let index = project.menus.keys().position(|key| *key == menu_id)?;
    let menu = project.menus.get(&menu_id)?;
    // Encore's "Set as Blu-ray Pop-up Menu": only flagged menus pop up over the title.
    if !menu.popup || menu.buttons.is_empty() {
        return None;
    }
    Some(
        groups
            .iter()
            .find(|group| group.contains(&index))
            .cloned()
            .unwrap_or_else(|| vec![index]),
    )
}

/// The commands of a pop-up's pages' buttons (`pages`: the menus' indices, the first page first): the menus' own
/// (from the navigation), except that a link to one of the pop-up's pages turns to it over the playing title
/// (Set_ButtonPage after the button's audio and subtitle choice, GitHub issue #13) instead of playing that page's
/// menu.
fn popup_commands(
    menus: &[&Menu],
    pages: &[usize],
    navigation: &nav::Navigation,
) -> Result<Vec<Vec<Vec<mobj::NavCommand>>>, BdError> {
    let page_of = |id: disccraft_project::MenuId| {
        pages
            .iter()
            .position(|index| menus.get(*index).is_some_and(|menu| menu.id == id))
    };
    pages
        .iter()
        .map(|index| {
            let menu = menus
                .get(*index)
                .ok_or_else(|| invalid("missing pop-up menu"))?;
            let own = navigation
                .button_commands
                .get(*index)
                .ok_or_else(|| invalid("missing pop-up menu commands"))?;
            menu.buttons
                .iter()
                .enumerate()
                .map(|(number, button)| {
                    // M6.528: Resume on a pop-up closes it; the title under it plays on.
                    if button.link == Link::Resume {
                        return Ok(stream_command(button.audio, button.subtitle)
                            .into_iter()
                            .chain([mobj::NavCommand::popup_off()])
                            .collect());
                    }
                    let own = || {
                        own.get(number)
                            .cloned()
                            .ok_or_else(|| invalid("missing pop-up button commands"))
                    };
                    let Link::Menu { id, button: wanted } = &button.link else {
                        return own();
                    };
                    let Some(page) = page_of(*id) else {
                        return own();
                    };
                    let count = pages
                        .get(page)
                        .and_then(|index| menus.get(*index))
                        .map_or(0, |menu| menu.buttons.len());
                    let page = u16::try_from(page).map_err(|_| invalid("too many pages"))?;
                    let turn = match wanted
                        .and_then(|wanted| usize::try_from(wanted).ok())
                        .filter(|wanted| *wanted < count)
                    {
                        Some(wanted) => mobj::NavCommand::set_button_page(
                            u16::try_from(wanted).map_err(|_| invalid("too many buttons"))?,
                            page,
                        ),
                        None => mobj::NavCommand::set_page(page),
                    };
                    Ok(stream_command(button.audio, button.subtitle)
                        .into_iter()
                        .chain([turn])
                        .collect())
                })
                .collect()
        })
        .collect()
}

/// Records a written title clip's CLPI backup and writes its single-item playlist.
fn record_title_files(
    dirs: &Directories,
    output: &mut BuildOutput,
    number: u16,
    built: &BuiltClip,
) -> Result<(), BdError> {
    let clpi_name = format!("{number:05}.clpi");
    let clpi_path = dirs.clipinf.join(&clpi_name);
    let clpi_size = file_size(&clpi_path)?;
    output
        .backup_files
        .push((clpi_path, Path::new("CLIPINF").join(clpi_name), clpi_size));

    let playlist_name = format!("{number:05}.mpls");
    let playlist_path = dirs.playlist.join(&playlist_name);
    let mut playlist = if let Some(held) = output.manual_titles.get(&number) {
        manual_slides_playlist(built, held)?
    } else {
        single_title_playlist(built)?
    };
    playlist.uo_mask = output.title_uo.get(&number).copied().unwrap_or(0);
    let playlist_bytes =
        mpls::write_mpls(&playlist).map_err(|error| BdError::Bdmv(error.to_string()))?;
    let playlist_size = write_bytes(&playlist_path, &playlist_bytes)?;
    add_total(output, playlist_size)?;
    output.backup_files.push((
        playlist_path,
        Path::new("PLAYLIST").join(playlist_name),
        playlist_size,
    ));
    Ok(())
}

/// Writes the title clips that carry a pop-up menu: the menu's IG stream (its buttons' commands from the
/// navigation) joins the clip, and the playlist lists the IG stream so the Pop-up Menu key can show it.
fn write_popup_titles(
    project: &Project,
    dirs: &Directories,
    output: &mut BuildOutput,
    navigation: &nav::Navigation,
    pending: Vec<PendingPopup>,
) -> Result<(), BdError> {
    let menus: Vec<&Menu> = project.menus.values().collect();
    for item in pending {
        let commands = popup_commands(&menus, &item.pages, navigation)?;
        let pages = item
            .pages
            .iter()
            .zip(&commands)
            .map(|(index, commands)| {
                let menu = menus
                    .get(*index)
                    .copied()
                    .ok_or_else(|| invalid("missing pop-up menu"))?;
                Ok((menu, commands.as_slice()))
            })
            .collect::<Result<Vec<_>, BdError>>()?;
        let (units, stream) = menus::popup_ig_units(project, &pages, item.clip.start_90k())?;
        let number = item.clip.number();
        let mut clip = item.clip;
        clip.add_ig(units, stream);
        let built = clip::write_prepared(clip, &dirs.stream, &dirs.clipinf)?;
        add_total(output, built.bytes)?;
        record_title_files(dirs, output, number, &built)?;
        // The playlists built so far reference the clip without its IG stream; refresh the record.
        if let Some(slot) = usize::from(number)
            .checked_sub(1)
            .and_then(|index| output.built.get_mut(index))
        {
            *slot = built;
        }
    }
    Ok(())
}

/// PlayItem still mode: hold the last picture until the viewer moves on.
const STILL_UNTIL_USER: u8 = 2;

/// A slideshow advanced by hand (Encore's 手動で進める): one PlayItem per slide (the spans between its marks). A
/// slide that is `held` keeps its last picture until the viewer moves on, as DVD holds that slide's cell; the
/// others play on.
fn manual_slides_playlist(built: &BuiltClip, held: &[bool]) -> Result<mpls::PlayList, BdError> {
    let slides: Vec<u32> = (0..built.marks.len())
        .map(|index| u32::try_from(index).map_err(|_| invalid("too many slides")))
        .collect::<Result<_, _>>()?;
    let mut items = Vec::with_capacity(slides.len());
    let mut marks = Vec::with_capacity(slides.len());
    let mut duration_90k = 0_u64;
    for (slide, mut item) in chapter_span_items(built, &slides)?.into_iter().enumerate() {
        if held.get(slide).copied().unwrap_or(false) {
            item.still_mode = STILL_UNTIL_USER;
            item.still_time = 0;
        }
        push_play_item(&mut items, &mut marks, &mut duration_90k, item)?;
    }
    Ok(mpls::PlayList {
        items,
        marks,
        uo_mask: 0,
        playback: mpls::Playback::Sequential,
    })
}

fn single_title_playlist(built: &BuiltClip) -> Result<mpls::PlayList, BdError> {
    let marks = built
        .marks
        .iter()
        .map(|time| mpls::PlayListMark {
            play_item: 0,
            time: *time,
        })
        .collect();

    Ok(mpls::PlayList {
        items: vec![built.play_item.clone()],
        marks,
        uo_mask: 0,
        playback: mpls::Playback::Sequential,
    })
}

fn push_title_nav(
    project: &Project,
    title: &TitleStreams,
    number: u16,
    output: &mut BuildOutput,
) -> Result<(), BdError> {
    match title.source {
        TitleSource::Timeline(id) => {
            let timeline = project
                .timelines
                .get(&TimelineId(id))
                .ok_or_else(|| invalid("timeline does not exist"))?;
            output.timeline_titles.insert(id, number);
            output.navs.push(TitleNav {
                playlist: u32::from(number),
                end_action: timeline.end_action.clone(),
                end_override: effective_override(
                    &timeline.end_action,
                    timeline.override_link.as_ref(),
                ),
                parental: title_parental(
                    project,
                    timeline.parental_level,
                    &timeline.parental_alternative,
                ),
                versions: Vec::new(),
                items: Vec::new(),
            });
        }
        TitleSource::Slideshow(id) => {
            let slideshow = project
                .slideshows
                .get(&id)
                .ok_or_else(|| invalid("slideshow does not exist"))?;
            output.slideshow_titles.insert(id, number);
            output.navs.push(TitleNav {
                playlist: u32::from(number),
                end_action: slideshow.end_action.clone(),
                end_override: None,
                parental: title_parental(project, slideshow.parental_level, &Link::NotSet),
                versions: Vec::new(),
                items: Vec::new(),
            });
        }
    }

    Ok(())
}

/// Project playlists (M6.555): a title each, whose movie object plays its items' titles in turn as a DVD's playlist
/// does (`TitleNav::items`): each item asks its own parental level and plays its version for the player's age, a
/// timeline keeps its chapters, a chapter playlist its Random or Shuffle, a slideshow its held slides. A playlist
/// writes no PlayList of its own. Title numbers continue after the built titles; returns the next title number.
fn write_project_playlists(project: &Project, output: &mut BuildOutput) -> Result<u16, BdError> {
    let mut next_title = u16_from_usize(
        output
            .built
            .len()
            .checked_add(1)
            .ok_or_else(|| invalid("too many titles"))?,
        "title number",
    )?;

    for playlist in project.playlists.values() {
        if playlist.items.is_empty() {
            continue;
        }
        let Some((chapters, duration_90k)) = playlist_extent(playlist, project, output)? else {
            continue;
        };

        let title_number = next_title;
        output.reports.push(BdTitleReport {
            title: title_number,
            playlist: 0,
            chapters,
            duration_90k,
        });
        output.navs.push(TitleNav {
            playlist: 0,
            end_action: playlist.end_action.clone(),
            end_override: None,
            parental: None,
            versions: Vec::new(),
            items: playlist.items.clone(),
        });
        output.playlist_titles.insert(playlist.id, title_number);

        next_title = next_title
            .checked_add(1)
            .ok_or_else(|| invalid("too many titles"))?;
    }

    Ok(next_title)
}

/// Chapter playlists: one PlayList each, whose PlayItems are the chapter spans of the master clip in the chosen
/// order (Random and Shuffle as the PlayList's playback type, M6.166). Title and playlist numbers continue from
/// `start` (M6.555: apart, as the project playlists before them take titles but no playlists); returns the next
/// playlist number.
fn write_chapter_playlists(
    project: &Project,
    dirs: &Directories,
    output: &mut BuildOutput,
    (mut next_title, start): (u16, u16),
) -> Result<u16, BdError> {
    let mut next = start;

    for playlist in project.chapter_playlists.values() {
        if playlist.chapters.is_empty() {
            continue;
        }
        let Some(master) = output.timeline_titles.get(&playlist.timeline.0).copied() else {
            continue;
        };
        let Some(built) = built_clip(output, master)? else {
            continue;
        };

        let mut items = Vec::with_capacity(playlist.chapters.len());
        let mut marks = Vec::with_capacity(playlist.chapters.len());
        let mut duration_90k = 0_u64;
        for play_item in chapter_span_items(built, &playlist.chapters)? {
            push_play_item(&mut items, &mut marks, &mut duration_90k, play_item)?;
        }

        let number = next;
        let playlist_name = format!("{number:05}.mpls");
        let playlist_path = dirs.playlist.join(&playlist_name);
        let chapters = marks.len();
        let playlist_bytes = mpls::write_mpls(&mpls::PlayList {
            items,
            marks,
            uo_mask: uo::uo_mask(&[&playlist.prohibited_ops, &project.settings.prohibited_ops]),
            playback: chapter_playback(playlist.order, chapters),
        })
        .map_err(|error| BdError::Bdmv(error.to_string()))?;
        let playlist_size = write_bytes(&playlist_path, &playlist_bytes)?;
        add_total(output, playlist_size)?;
        output.backup_files.push((
            playlist_path,
            Path::new("PLAYLIST").join(playlist_name),
            playlist_size,
        ));

        output.reports.push(BdTitleReport {
            title: next_title,
            playlist: number,
            chapters,
            duration_90k,
        });
        output.navs.push(TitleNav {
            playlist: u32::from(number),
            end_action: playlist.end_action.clone(),
            end_override: effective_override(&playlist.end_action, playlist.override_link.as_ref()),
            parental: title_parental(project, playlist.parental_level, &Link::NotSet),
            versions: Vec::new(),
            items: Vec::new(),
        });
        output
            .chapter_playlist_titles
            .insert(playlist.id, next_title);

        next_title = next_title
            .checked_add(1)
            .ok_or_else(|| invalid("too many titles"))?;
        next = next
            .checked_add(1)
            .ok_or_else(|| invalid("too many playlists"))?;
    }

    Ok(next)
}

/// M6.548: the playlists of the parental versions below the top one of each timeline with parental chapters (the
/// title's chapters in order, each the master clip's span or, where the version replaces it, the whole clip of its
/// alternative's hidden title `clips`), numbered from `start`; each title's navigation gets its versions, the top
/// one (its own playlist) first, each with the age a player's setting must reach for it.
fn write_parental_versions(
    project: &Project,
    clips: &[disccraft_project::ParentalClip],
    dirs: &Directories,
    output: &mut BuildOutput,
    start: u16,
) -> Result<u16, BdError> {
    let mut next = start;
    for timeline in project.timelines.values() {
        let variants = timeline.parental_variants();
        if variants.len() < 2 || !clips.iter().any(|clip| clip.timeline == timeline.id) {
            continue;
        }
        let Some(title) = output.timeline_titles.get(&timeline.id.0).copied() else {
            continue;
        };
        let age = |level: u8| disccraft_project::bd_parental_age(level).unwrap_or(0);
        // Each version's play items and marks, read from the built clips before the playlists are written.
        let mut lists: Vec<(u8, Vec<mpls::PlayItem>, Vec<mpls::PlayListMark>)> = Vec::new();
        {
            let Some(master) = built_clip(output, title)? else {
                continue;
            };
            for variant in variants.iter().skip(1) {
                let mut items = Vec::new();
                let mut marks = Vec::new();
                let mut duration_90k = 0_u64;
                for chapter in 0..master.marks.len() {
                    let replaced = timeline
                        .parental_chapters
                        .iter()
                        .zip(&variant.replaced)
                        .find(|(parental, replaced)| **replaced && parental.chapter == chapter)
                        .and_then(|(parental, _)| {
                            clips.iter().find(|clip| {
                                clip.timeline == timeline.id && clip.asset == parental.alternative
                            })
                        });
                    let item = match replaced {
                        Some(clip) => {
                            let hidden = output
                                .timeline_titles
                                .get(&clip.hidden.0)
                                .copied()
                                .ok_or_else(|| invalid("a parental alternative was not built"))?;
                            built_clip(output, hidden)?
                                .map(|built| built.play_item.clone())
                                .ok_or_else(|| invalid("a parental alternative was not built"))?
                        }
                        None => {
                            let chapter =
                                u32::try_from(chapter).map_err(|_| invalid("too many chapters"))?;
                            chapter_span_items(master, &[chapter])?
                                .into_iter()
                                .next()
                                .ok_or_else(|| invalid("empty chapter"))?
                        }
                    };
                    push_play_item(&mut items, &mut marks, &mut duration_90k, item)?;
                }
                lists.push((age(variant.levels.0), items, marks));
            }
        }
        let mut versions = vec![(
            age(variants.first().map_or(1, |variant| variant.levels.0)),
            u32::from(title),
        )];
        for (version_age, items, marks) in lists {
            let number = next;
            let playlist_name = format!("{number:05}.mpls");
            let playlist_path = dirs.playlist.join(&playlist_name);
            let playlist_bytes = mpls::write_mpls(&mpls::PlayList {
                items,
                marks,
                uo_mask: uo::uo_mask(&[&timeline.prohibited_ops, &project.settings.prohibited_ops]),
                playback: mpls::Playback::Sequential,
            })
            .map_err(|error| BdError::Bdmv(error.to_string()))?;
            let playlist_size = write_bytes(&playlist_path, &playlist_bytes)?;
            add_total(output, playlist_size)?;
            output.backup_files.push((
                playlist_path,
                Path::new("PLAYLIST").join(playlist_name),
                playlist_size,
            ));
            versions.push((version_age, u32::from(number)));
            next = next
                .checked_add(1)
                .ok_or_else(|| invalid("too many playlists"))?;
        }
        let index = usize::from(
            title
                .checked_sub(1)
                .ok_or_else(|| invalid("invalid title number"))?,
        );
        if let Some(nav) = output.navs.get_mut(index) {
            nav.versions = versions;
        }
    }
    Ok(next)
}

/// The PlayItems of a chapter playlist's entries over its master clip: one per entry, from the chapter's mark
/// to the next mark (or the clip's end).
fn chapter_span_items(built: &BuiltClip, chapters: &[u32]) -> Result<Vec<mpls::PlayItem>, BdError> {
    // A chapter's span starts at the GOP its mark is in, as Encore's chapters do: a play item that starts within a
    // GOP makes the player (and libbluray) start reading pictures that refer back before it.
    let gop_start = |time: u32| -> u32 {
        built
            .entry_points
            .iter()
            .copied()
            .filter(|point| *point <= time && *point >= built.play_item.in_time)
            .max()
            .unwrap_or(time)
    };
    let mut items = Vec::with_capacity(chapters.len());
    for chapter in chapters {
        let chapter = usize::try_from(*chapter).map_err(|_| invalid("chapter index too large"))?;
        let mark = built
            .marks
            .get(chapter)
            .copied()
            .ok_or_else(|| invalid("chapter playlist entry is out of range"))?;
        let next = built.marks.get(chapter.saturating_add(1)).copied();
        let (mut in_time, mut out_time) = (
            gop_start(mark),
            next.map_or(built.play_item.out_time, gop_start),
        );
        // Two marks in one GOP: keep the marks as they are rather than an empty span.
        if out_time <= in_time {
            in_time = mark;
            out_time = next.unwrap_or(built.play_item.out_time);
        }
        if out_time <= in_time {
            return Err(invalid("chapter playlist entry is empty"));
        }
        items.push(mpls::PlayItem {
            in_time,
            out_time,
            ..built.play_item.clone()
        });
    }
    Ok(items)
}

/// Appends a PlayItem with a mark at its start and adds its length to the running duration.
fn push_play_item(
    items: &mut Vec<mpls::PlayItem>,
    marks: &mut Vec<mpls::PlayListMark>,
    duration_90k: &mut u64,
    play_item: mpls::PlayItem,
) -> Result<(), BdError> {
    let play_item_index = u16::try_from(items.len()).map_err(|_| invalid("playlist too long"))?;
    marks.push(mpls::PlayListMark {
        play_item: play_item_index,
        time: play_item.in_time,
    });
    *duration_90k = add_item_duration(*duration_90k, &play_item)?;
    items.push(play_item);
    Ok(())
}

/// A project playlist's chapters and length, as its items' titles play them (for the build report): a timeline's or
/// slideshow's marks and clip, a chapter playlist's entries and their spans. `None` when an item has not been built.
fn playlist_extent(
    playlist: &disccraft_project::Playlist,
    project: &Project,
    output: &BuildOutput,
) -> Result<Option<(usize, u64)>, BdError> {
    let mut chapters = 0_usize;
    let mut duration_90k = 0_u64;

    for item in &playlist.items {
        match item {
            PlaylistItem::Timeline(_) | PlaylistItem::Slideshow(_) => {
                let title_number = match item {
                    PlaylistItem::Timeline(id) => output.timeline_titles.get(&id.0).copied(),
                    _ => output.slideshow_titles.get(&item.id()).copied(),
                };
                let Some(title_number) = title_number else {
                    return Ok(None);
                };
                let Some(built) = built_clip(output, title_number)? else {
                    return Ok(None);
                };
                chapters = chapters.saturating_add(built.marks.len().max(1));
                duration_90k = add_item_duration(duration_90k, &built.play_item)?;
            }
            PlaylistItem::ChapterPlaylist(id) => {
                let Some(chapter_playlist) = project.chapter_playlists.get(id) else {
                    return Ok(None);
                };
                let Some(master) = output
                    .timeline_titles
                    .get(&chapter_playlist.timeline.0)
                    .copied()
                else {
                    return Ok(None);
                };
                let Some(built) = built_clip(output, master)? else {
                    return Ok(None);
                };
                for play_item in chapter_span_items(built, &chapter_playlist.chapters)? {
                    chapters = chapters.saturating_add(1);
                    duration_90k = add_item_duration(duration_90k, &play_item)?;
                }
            }
        }
    }

    Ok(Some((chapters, duration_90k)))
}

fn built_clip(output: &BuildOutput, title_number: u16) -> Result<Option<&BuiltClip>, BdError> {
    let offset = title_number
        .checked_sub(1)
        .ok_or_else(|| invalid("invalid title number"))?;
    let index = usize::from(offset);
    Ok(output.built.get(index))
}

fn add_item_duration(current: u64, item: &mpls::PlayItem) -> Result<u64, BdError> {
    let delta = item
        .out_time
        .checked_sub(item.in_time)
        .ok_or_else(|| invalid("play item out time before in time"))?;
    let item_duration = u64::from(delta)
        .checked_mul(2)
        .ok_or_else(|| invalid("duration overflow"))?;
    current
        .checked_add(item_duration)
        .ok_or_else(|| invalid("duration overflow"))
}

fn build_menu_navs(project: &Project, start: u16) -> Result<Vec<MenuNav>, BdError> {
    let mut navs = Vec::new();
    let mut number = start;
    // Menus whose pages are one composition (M10.26).
    let places = pages::menu_pages(project);

    for (index, menu) in project.menus.values().enumerate() {
        if menu.buttons.is_empty() {
            return Err(invalid("menu has no buttons"));
        }

        let buttons = menu
            .buttons
            .iter()
            .map(|button| button.link.clone())
            .collect();
        let motion = menu.is_motion().then(|| menu_motion(menu));
        // Only a menu that is shown for a time (a timed still, or a motion menu that plays so many times) moves on
        // by itself.
        let timed = match motion {
            Some(motion) => motion.plays.is_some() && menu.end_action != Link::NotSet,
            None => menu.hold_seconds().is_some(),
        };
        navs.push(MenuNav {
            id: menu.id.0,
            playlist: u32::from(number),
            buttons,
            end_action: if timed {
                menu.end_action.clone()
            } else {
                Link::NotSet
            },
            motion,
            end_override: effective_override(&menu.end_action, menu.override_link.as_ref()),
            button_overrides: menu
                .buttons
                .iter()
                .map(|button| effective_override(&button.link, button.override_link.as_ref()))
                .collect(),
            button_prefix: menu
                .buttons
                .iter()
                .map(|button| {
                    stream_command(button.audio, button.subtitle)
                        .into_iter()
                        .collect()
                })
                .collect(),
            select: default_select(menu),
            pages: places.get(index).cloned().flatten(),
        });
        number = number
            .checked_add(1)
            .ok_or_else(|| invalid("too many playlists"))?;
    }

    Ok(navs)
}

/// How a motion menu plays (Encore's ループ回数 and ループポイント, as on DVD): so many times then the end action, or
/// forever (a count without an end action loops forever too). Once without an end action holds the last picture.
fn menu_motion(menu: &Menu) -> MenuMotion {
    let plays = match (menu.effective_loops(), menu.end_action == Link::NotSet) {
        (MenuLoops::Once, _) => Some(1),
        (MenuLoops::Times(times), false) => Some(times),
        _ => None,
    };
    MenuMotion {
        plays,
        loop_mark: menu.effective_loop_point().is_some(),
    }
}

/// How a chapter playlist plays its entries (`count` play items): in order, or (Encore's random / shuffle) that
/// many picked at random or shuffled, as on DVD.
fn chapter_playback(order: disccraft_project::PlaybackOrder, count: usize) -> mpls::Playback {
    let count = u16::try_from(count).unwrap_or(u16::MAX);
    match order {
        disccraft_project::PlaybackOrder::Sequential => mpls::Playback::Sequential,
        disccraft_project::PlaybackOrder::Random => mpls::Playback::Random { count },
        disccraft_project::PlaybackOrder::Shuffle => mpls::Playback::Shuffle { count },
    }
}

/// M6.543: what a title with its own parental level `own` (`None`: the disc's, as on a DVD) asks on a Blu-ray: the
/// age of its level ([`disccraft_project::bd_parental_age`]) and what plays in its place below it (`refused`).
fn title_parental(project: &Project, own: Option<u8>, refused: &Link) -> Option<(u8, Link)> {
    let level = own.unwrap_or(project.settings.parental_level);
    disccraft_project::bd_parental_age(level)
        .filter(|age| *age > 0)
        .map(|age| (age, refused.clone()))
}

/// The override of a link (Encore's 上書き), when it applies: the link plays a title, and the override is set.
fn effective_override(link: &Link, override_link: Option<&Link>) -> Option<Link> {
    let leads_to_title = matches!(
        link,
        Link::Timeline { .. }
            | Link::Slideshow { .. }
            | Link::Playlist { .. }
            | Link::ChapterPlaylist { .. }
    );
    override_link
        .filter(|target| leads_to_title && !matches!(target, Link::NotSet))
        .cloned()
}

/// The SetStream command that selects audio track `audio` (1 to 32) and the subtitle choice (Encore's Set Audio /
/// Set Subtitle, and a button's Audio / Subtitle); `None` when it changes nothing.
fn stream_command(
    audio: Option<u8>,
    subtitle: disccraft_project::SubtitleDefault,
) -> Option<disccraft_bdmv::mobj::NavCommand> {
    use disccraft_project::SubtitleDefault;
    let audio = audio
        .filter(|track| (1..=32).contains(track))
        .map(u16::from);
    let pg = match subtitle {
        SubtitleDefault::NoChange => None,
        // Hidden: the first stream, not shown.
        SubtitleDefault::Off => Some((false, 1)),
        SubtitleDefault::Stream(track) if (1..=32).contains(&track) => {
            Some((true, u16::from(track)))
        }
        SubtitleDefault::Stream(_) => None,
    };
    (audio.is_some() || pg.is_some())
        .then(|| disccraft_bdmv::mobj::NavCommand::set_stream(audio, pg))
}

/// The button a menu selects when it is entered without one: its default (the last button when the default is
/// out of range).
fn default_select(menu: &Menu) -> u16 {
    let last = menu.buttons.len().saturating_sub(1);
    let index = usize::try_from(menu.default_button)
        .unwrap_or(last)
        .min(last);
    u16::try_from(index).unwrap_or(0)
}

fn write_menus(
    project: &Project,
    dirs: &Directories,
    output: &mut BuildOutput,
    navigation: &nav::Navigation,
    (start, cache_dir): (u16, &Path),
) -> Result<(), BdError> {
    let mut number = start;
    let menu_dirs = MenuDirs {
        stream: &dirs.stream,
        clipinf: &dirs.clipinf,
        playlist: &dirs.playlist,
    };

    // Each menu's clip has its own number; they are made side by side (M6.381: an HD motion menu is an encode),
    // then counted in order. A menu whose pages are one composition (M10.26) has one clip, at its first page's
    // number: its later pages' numbers have none.
    let menus: Vec<&Menu> = project.menus.values().collect();
    let groups = pages::menu_page_groups(project);
    let mut jobs = Vec::with_capacity(project.menus.len());
    for (index, menu) in menus.iter().enumerate() {
        let Some(commands) = navigation.button_commands.get(index) else {
            return Err(invalid("missing menu commands"));
        };
        let group = groups.iter().find(|group| group.contains(&index));
        let job = match group {
            Some(group) if group.first() != Some(&index) => None,
            Some(group) => {
                let pages = group
                    .iter()
                    .map(|page| {
                        let menu = menus
                            .get(*page)
                            .copied()
                            .ok_or_else(|| invalid("missing menu"))?;
                        let commands = navigation
                            .button_commands
                            .get(*page)
                            .ok_or_else(|| invalid("missing menu commands"))?;
                        Ok((menu, commands.as_slice()))
                    })
                    .collect::<Result<Vec<_>, BdError>>()?;
                let counts: Vec<usize> = pages.iter().map(|(menu, _)| menu.buttons.len()).collect();
                Some((
                    menus::background_only(menu),
                    number,
                    commands.as_slice(),
                    Some(menus::MenuPagesClip {
                        pages,
                        dispatcher: nav::page_dispatcher(&counts)?,
                    }),
                ))
            }
            None => Some(((*menu).clone(), number, commands.as_slice(), None)),
        };
        jobs.extend(job);
        number = number
            .checked_add(1)
            .ok_or_else(|| invalid("too many playlists"))?;
    }
    let results = side_by_side(&jobs, |(menu, number, commands, pages)| {
        let menu_clip = MenuClip {
            menu,
            number: *number,
            commands,
            pages: pages.as_ref(),
        };
        menus::write_menu_clip(project, &menu_clip, &menu_dirs, cache_dir)
            .map(|bytes| (*number, bytes))
    });
    for result in results {
        let (number, bytes) = result?;
        add_total(output, bytes)?;
        add_menu_backups(dirs, output, number)?;
    }

    Ok(())
}

/// The most jobs [`side_by_side`] runs at once.
const MOST_AT_ONCE: usize = 4;

/// Runs `work` on every item, several at once (one for every eight cores, up to [`MOST_AT_ONCE`]: an encode uses
/// cores of its own), and returns the results in the items' order (M6.381).
fn side_by_side<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .div_euclid(8)
        .clamp(1, MOST_AT_ONCE)
        .min(items.len());
    if workers <= 1 {
        return items.iter().map(&work).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::with_capacity(items.len()));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let (next, results, work) = (&next, &results, &work);
            let _ = scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let Some(item) = items.get(index) else {
                        break;
                    };
                    let result = work(item);
                    if let Ok(mut results) = results.lock() {
                        results.push((index, result));
                    }
                }
            });
        }
    });
    let mut results = results
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, result)| result).collect()
}

fn add_menu_backups(
    dirs: &Directories,
    output: &mut BuildOutput,
    number: u16,
) -> Result<(), BdError> {
    let clpi_name = format!("{number:05}.clpi");
    let clpi_path = dirs.clipinf.join(&clpi_name);
    let clpi_size = file_size(&clpi_path)?;
    output
        .backup_files
        .push((clpi_path, Path::new("CLIPINF").join(clpi_name), clpi_size));

    let playlist_name = format!("{number:05}.mpls");
    let playlist_path = dirs.playlist.join(&playlist_name);
    let playlist_size = file_size(&playlist_path)?;
    output.backup_files.push((
        playlist_path,
        Path::new("PLAYLIST").join(playlist_name),
        playlist_size,
    ));

    Ok(())
}

fn write_navigation(
    project: &Project,
    dirs: &Directories,
    output: &mut BuildOutput,
    navigation: nav::Navigation,
) -> Result<(), BdError> {
    let index_path = dirs.bdmv.join("index.bdmv");
    let index_table = index::IndexTable {
        first_playback: navigation.first_playback,
        top_menu: navigation.top_menu,
        titles: navigation.titles,
        video_format: video_format(project.settings.standard),
        frame_rate: frame_rate(project.settings.standard),
    };
    let index_bytes =
        index::write_index(&index_table).map_err(|error| BdError::Bdmv(error.to_string()))?;
    let index_size = write_bytes(&index_path, &index_bytes)?;
    add_total(output, index_size)?;
    output
        .backup_files
        .push((index_path, PathBuf::from("index.bdmv"), index_size));

    let object_path = dirs.bdmv.join("MovieObject.bdmv");
    let object_bytes = mobj::write_movie_objects(&navigation.objects)
        .map_err(|error| BdError::Bdmv(error.to_string()))?;
    let object_size = write_bytes(&object_path, &object_bytes)?;
    add_total(output, object_size)?;
    output
        .backup_files
        .push((object_path, PathBuf::from("MovieObject.bdmv"), object_size));

    Ok(())
}

fn copy_backups(dirs: &Directories, output: &mut BuildOutput) -> Result<(), BdError> {
    let files = std::mem::take(&mut output.backup_files);

    for (source, relative, size) in files {
        let destination = dirs.backup.join(relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
        }
        std::fs::copy(&source, &destination).map_err(|error| io_error(&destination, error))?;
        add_total(output, size)?;
    }

    Ok(())
}

fn add_total(output: &mut BuildOutput, size: u64) -> Result<(), BdError> {
    output.total_bytes = output
        .total_bytes
        .checked_add(size)
        .ok_or_else(|| invalid("total size overflow"))?;
    Ok(())
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<u64, BdError> {
    std::fs::write(path, bytes).map_err(|error| io_error(path, error))?;
    u64::try_from(bytes.len()).map_err(|_| invalid("file size overflow"))
}

fn file_size(path: &Path) -> Result<u64, BdError> {
    let metadata = std::fs::metadata(path).map_err(|error| io_error(path, error))?;
    Ok(metadata.len())
}

pub(crate) fn io_error(path: &Path, error: std::io::Error) -> BdError {
    BdError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

pub(crate) fn invalid(message: &str) -> BdError {
    BdError::Invalid(message.to_string())
}

fn u16_from_usize(value: usize, what: &str) -> Result<u16, BdError> {
    u16::try_from(value).map_err(|_| invalid(what))
}

pub(crate) fn video_format(standard: VideoStandard) -> u8 {
    match standard {
        VideoStandard::Ntsc => 1,
        VideoStandard::Pal => 2,
    }
}

pub(crate) fn frame_rate(standard: VideoStandard) -> u8 {
    match standard {
        VideoStandard::Ntsc => 4,
        VideoStandard::Pal => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::{frame_rate, invalid, u16_from_usize, video_format};
    use disccraft_project::VideoStandard;

    /// M6.528: a pop-up's Resume button closes the pop-up (Set_PopupOff) over the playing title, after its audio and
    /// subtitle choice; its other buttons keep the navigation's commands.
    #[test]
    fn a_popups_resume_closes_it() -> Result<(), Box<dyn std::error::Error>> {
        use disccraft_bdmv::mobj::NavCommand;
        use disccraft_project::{Link, Project};
        let mut project = Project::default();
        let menu = project.add_menu(Some("Pop".to_string()))?;
        project.add_button(menu, "Back".to_string(), None, Link::Resume)?;
        project.add_button(menu, "Stop".to_string(), None, Link::Stop)?;
        let found = project.menu(menu).ok_or("no menu")?;
        let own = vec![vec![NavCommand::nop()], vec![NavCommand::jump_title(1)]];
        let navigation = super::nav::Navigation {
            first_playback: 0,
            top_menu: 1,
            titles: Vec::new(),
            objects: Vec::new(),
            button_commands: vec![own.clone()],
        };
        let commands = super::popup_commands(&[found], &[0], &navigation)?;
        let buttons = commands.first().ok_or("no page")?;
        assert_eq!(buttons.first(), Some(&vec![NavCommand::popup_off()]));
        assert_eq!(buttons.get(1), own.get(1));
        Ok(())
    }

    #[test]
    fn video_format_and_frame_rate() {
        assert_eq!(video_format(VideoStandard::Ntsc), 1);
        assert_eq!(video_format(VideoStandard::Pal), 2);
        assert_eq!(frame_rate(VideoStandard::Ntsc), 4);
        assert_eq!(frame_rate(VideoStandard::Pal), 3);
    }

    #[test]
    fn invalid_error_display() {
        let error = invalid("bad");
        assert_eq!(error.to_string(), "invalid project: bad");
    }

    #[test]
    fn u16_conversion_rejects_large_values() {
        assert!(u16_from_usize(usize::MAX, "value").is_err());
    }
}
