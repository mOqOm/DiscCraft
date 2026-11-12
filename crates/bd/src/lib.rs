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
mod menus;
mod nav;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use disccraft_bdmv::{index, mobj, mpls};
use disccraft_dvd::{TitleSource, TitleStreams, load_title_streams};
use disccraft_project::{DiscKind, Link, Menu, PlaylistItem, Project, TimelineId, VideoStandard};

use clip::BuiltClip;
use menus::{MenuClip, MenuDirs};
use nav::{MenuNav, TitleMap, TitleNav};

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

    let dirs = prepare_directories(out_dir)?;
    let titles = load_title_streams(project, cache_dir, progress)
        .map_err(|error| BdError::Prepare(error.to_string()))?;

    let mut output = BuildOutput::default();
    let pending = write_title_clips(project, &titles, &dirs, &mut output)?;
    let next_playlist = write_project_playlists(project, &dirs, &mut output)?;
    let next_playlist = write_chapter_playlists(project, &dirs, &mut output, next_playlist)?;
    let menu_navs = build_menu_navs(project, next_playlist)?;

    let map = TitleMap {
        timelines: output.timeline_titles.clone(),
        slideshows: output.slideshow_titles.clone(),
        playlists: output.playlist_titles.clone(),
        chapter_playlists: output.chapter_playlist_titles.clone(),
    };
    let navigation = nav::build_navigation_with_top(
        &project.settings.first_play,
        &project.settings.title_remote,
        &output.navs,
        &menu_navs,
        &map,
    )?;

    write_popup_titles(project, &dirs, &mut output, &navigation, pending)?;
    write_menus(project, &dirs, &mut output, &navigation, next_playlist)?;
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
    /// Index of the menu among the project's menus.
    menu_index: usize,
    menu: Menu,
}

/// Writes every title clip and its playlist. A timeline whose remote menu is a menu gets that menu as a pop-up:
/// its clip is only prepared here and written by [`write_popup_titles`], after the navigation (and so the menu's
/// button commands) exists.
fn write_title_clips(
    project: &Project,
    titles: &[TitleStreams],
    dirs: &Directories,
    output: &mut BuildOutput,
) -> Result<Vec<PendingPopup>, BdError> {
    let mut pending = Vec::new();
    for (index, title) in titles.iter().enumerate() {
        let number = u16_from_usize(
            index
                .checked_add(1)
                .ok_or_else(|| invalid("too many titles"))?,
            "title number",
        )?;
        let popup = popup_menu_of(project, title);
        let built = match popup {
            Some((menu_index, menu)) => {
                let prepared = clip::prepare_clip(title, number, &project.settings)?;
                let planned = prepared.planned()?;
                pending.push(PendingPopup {
                    clip: prepared,
                    menu_index,
                    menu,
                });
                planned
            }
            None => {
                let built = clip::write_clip(
                    title,
                    number,
                    &project.settings,
                    &dirs.stream,
                    &dirs.clipinf,
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

/// The menu a timeline title shows on the remote's Pop-up Menu key (Encore's timeline "Remote Menu"), with its
/// index among the project's menus.
fn popup_menu_of(project: &Project, title: &TitleStreams) -> Option<(usize, Menu)> {
    let TitleSource::Timeline(id) = title.source else {
        return None;
    };
    let timeline = project.timelines.get(&TimelineId(id))?;
    let Link::Menu { id: menu_id, .. } = timeline.menu_remote else {
        return None;
    };
    let index = project.menus.keys().position(|key| *key == menu_id)?;
    let menu = project.menus.get(&menu_id)?;
    (!menu.buttons.is_empty()).then(|| (index, menu.clone()))
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
    let playlist = single_title_playlist(built)?;
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
    for item in pending {
        let commands = navigation
            .button_commands
            .get(item.menu_index)
            .ok_or_else(|| invalid("missing pop-up menu commands"))?;
        let (units, stream) =
            menus::popup_ig_units(project, &item.menu, commands, item.clip.start_90k())?;
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
            });
        }
    }

    Ok(())
}

fn write_project_playlists(
    project: &Project,
    dirs: &Directories,
    output: &mut BuildOutput,
) -> Result<u16, BdError> {
    let original_count = output.built.len();
    let mut next_title = u16_from_usize(
        original_count
            .checked_add(1)
            .ok_or_else(|| invalid("too many titles"))?,
        "title number",
    )?;
    let mut next_playlist = next_title;

    for playlist in project.playlists.values() {
        if playlist.items.is_empty() {
            continue;
        }

        let Some((items, marks, duration_90k)) = build_playlist_items(playlist, project, output)?
        else {
            continue;
        };

        let chapters = marks.len();
        let playlist_number = next_playlist;
        let playlist_name = format!("{playlist_number:05}.mpls");
        let playlist_path = dirs.playlist.join(&playlist_name);
        let playlist_bytes = mpls::write_mpls(&mpls::PlayList { items, marks })
            .map_err(|error| BdError::Bdmv(error.to_string()))?;
        let playlist_size = write_bytes(&playlist_path, &playlist_bytes)?;
        add_total(output, playlist_size)?;
        output.backup_files.push((
            playlist_path,
            Path::new("PLAYLIST").join(playlist_name),
            playlist_size,
        ));

        let title_number = next_title;
        output.reports.push(BdTitleReport {
            title: title_number,
            playlist: playlist_number,
            chapters,
            duration_90k,
        });
        output.navs.push(TitleNav {
            playlist: u32::from(playlist_number),
            end_action: playlist.end_action.clone(),
        });
        output.playlist_titles.insert(playlist.id, title_number);

        next_title = next_title
            .checked_add(1)
            .ok_or_else(|| invalid("too many titles"))?;
        next_playlist = next_playlist
            .checked_add(1)
            .ok_or_else(|| invalid("too many playlists"))?;
    }

    Ok(next_playlist)
}

/// Chapter playlists: one PlayList each, whose PlayItems are the chapter spans of the master clip in the chosen
/// order (random and shuffle play in order on a Blu-ray). Title and playlist numbers continue from `start`.
fn write_chapter_playlists(
    project: &Project,
    dirs: &Directories,
    output: &mut BuildOutput,
    start: u16,
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
        let playlist_bytes = mpls::write_mpls(&mpls::PlayList { items, marks })
            .map_err(|error| BdError::Bdmv(error.to_string()))?;
        let playlist_size = write_bytes(&playlist_path, &playlist_bytes)?;
        add_total(output, playlist_size)?;
        output.backup_files.push((
            playlist_path,
            Path::new("PLAYLIST").join(playlist_name),
            playlist_size,
        ));

        output.reports.push(BdTitleReport {
            title: number,
            playlist: number,
            chapters,
            duration_90k,
        });
        output.navs.push(TitleNav {
            playlist: u32::from(number),
            end_action: playlist.end_action.clone(),
        });
        output.chapter_playlist_titles.insert(playlist.id, number);

        next = next
            .checked_add(1)
            .ok_or_else(|| invalid("too many titles"))?;
    }

    Ok(next)
}

/// The PlayItems of a chapter playlist's entries over its master clip: one per entry, from the chapter's mark
/// to the next mark (or the clip's end).
fn chapter_span_items(built: &BuiltClip, chapters: &[u32]) -> Result<Vec<mpls::PlayItem>, BdError> {
    let mut items = Vec::with_capacity(chapters.len());
    for chapter in chapters {
        let chapter = usize::try_from(*chapter).map_err(|_| invalid("chapter index too large"))?;
        let in_time = built
            .marks
            .get(chapter)
            .copied()
            .ok_or_else(|| invalid("chapter playlist entry is out of range"))?;
        let out_time = built
            .marks
            .get(chapter.saturating_add(1))
            .copied()
            .unwrap_or(built.play_item.out_time);
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

/// The PlayItems of a project playlist: whole clips for timelines and slideshows, the chapter spans of the
/// master clip for chapter playlists. `None` when an item has not been built.
fn build_playlist_items(
    playlist: &disccraft_project::Playlist,
    project: &Project,
    output: &BuildOutput,
) -> Result<Option<(Vec<mpls::PlayItem>, Vec<mpls::PlayListMark>, u64)>, BdError> {
    let mut items = Vec::new();
    let mut marks = Vec::new();
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
                push_play_item(
                    &mut items,
                    &mut marks,
                    &mut duration_90k,
                    built.play_item.clone(),
                )?;
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
                    push_play_item(&mut items, &mut marks, &mut duration_90k, play_item)?;
                }
            }
        }
    }

    Ok(Some((items, marks, duration_90k)))
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

    for menu in project.menus.values() {
        if menu.buttons.is_empty() {
            return Err(invalid("menu has no buttons"));
        }

        let buttons = menu
            .buttons
            .iter()
            .map(|button| button.link.clone())
            .collect();
        navs.push(MenuNav {
            id: menu.id.0,
            playlist: u32::from(number),
            buttons,
        });
        number = number
            .checked_add(1)
            .ok_or_else(|| invalid("too many playlists"))?;
    }

    Ok(navs)
}

fn write_menus(
    project: &Project,
    dirs: &Directories,
    output: &mut BuildOutput,
    navigation: &nav::Navigation,
    start: u16,
) -> Result<(), BdError> {
    let mut number = start;
    let menu_dirs = MenuDirs {
        stream: &dirs.stream,
        clipinf: &dirs.clipinf,
        playlist: &dirs.playlist,
    };

    for (index, menu) in project.menus.values().enumerate() {
        let Some(commands) = navigation.button_commands.get(index) else {
            return Err(invalid("missing menu commands"));
        };

        let menu_clip = MenuClip {
            menu,
            number,
            commands: commands.as_slice(),
            popup: false,
        };
        let bytes = menus::write_menu_clip(project, &menu_clip, &menu_dirs)?;
        add_total(output, bytes)?;
        add_menu_backups(dirs, output, number)?;

        number = number
            .checked_add(1)
            .ok_or_else(|| invalid("too many playlists"))?;
    }

    Ok(())
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
