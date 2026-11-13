//! Compiles project links into DVD VM commands.

use crate::cmd::{self, Cmd};
use disccraft_project::{Link, MenuId, PlaybackOrder, PlaylistItem, Project, TimelineId};
use std::collections::BTreeMap;
use thiserror::Error;

/// Maximum number of VMGM PGCs.
const MAX_VTGM_PGCN: usize = 999;

/// Maximum number of titles.
const MAX_TITLES: usize = 99;

/// GPRM0: requested chapter for the next `JumpTT`.
const GPRM_REQUESTED: u8 = 0;

/// GPRM1: scratch copy of GPRM0 inside title pre commands.
const GPRM_SCRATCH: u8 = 1;

/// GPRM2: VMGM PGCN of the last menu shown.
const GPRM_LAST_MENU: u8 = 2;

/// GPRM3: active playlist number, one-based.
const GPRM_ACTIVE_PLAYLIST: u8 = 3;

/// GPRM4: current one-based playlist position.
const GPRM_POSITION: u8 = 4;

/// GPRM5: scratch register that is set to zero before a playlist check.
const GPRM_SCRATCH_ZERO: u8 = 5;

/// GPRM6: scratch register that holds the current title number.
const GPRM_SCRATCH_TITLE: u8 = 6;

/// GPRM7: pending override, one-based (0 = none). A timeline whose end action leads to another title sets it;
/// the next title to end follows the override instead of its own end action. Menus clear it.
const GPRM_OVERRIDE: u8 = 7;

/// GPRM8: how many times the motion menu now on screen has played (Encore's ループ回数).
const GPRM_MENU_LOOPS: u8 = 8;

/// GPRM10: the button a link asks the next menu to highlight, as SPRM8 holds it (button number × 1024); 0 when the
/// menu should highlight its own. The menu's pre commands use it and clear it.
const GPRM_SELECT: u8 = 10;

/// Pre and post commands of a motion menu that plays `times` times (2 or more), the later plays from `program`
/// (program 1, or 2 from its loop point) with LinkPGN, which does not run the pre commands again (the count and the
/// highlighted button stay); then `end`. The pre commands go after the menu's own.
pub fn counted_loop_to_program(
    times: u16,
    program: u8,
    end: &[cmd::Cmd],
) -> (Vec<cmd::Cmd>, Vec<cmd::Cmd>) {
    let pre = vec![cmd::set_gprm(GPRM_MENU_LOOPS, 0)];
    let mut post = vec![
        // 1
        cmd::add_gprm(GPRM_MENU_LOOPS, 1),
        // 2: played enough: on to line 4.
        cmd::if_eq_goto(GPRM_MENU_LOOPS, times, 4),
        // 3: once more, from the loop point.
        cmd::link_pgn(program, 0),
        // 4
        cmd::set_gprm(GPRM_MENU_LOOPS, 0),
    ];
    post.extend_from_slice(end);
    (pre, post)
}

/// Key for a hidden chapter-entry PGC.
type ChapterKey = (u8, u16);

/// Key for a hidden playlist step or end PGC.
type PlaylistStepKey = (u64, u16);

type MenuPgcMap = BTreeMap<MenuId, u16>;
type TimelineTitleMap = BTreeMap<TimelineId, u8>;
type SlideshowTitleMap = BTreeMap<u64, u8>;
type ChapterPlaylistTitleMap = BTreeMap<u64, u8>;
type ChapterCountMap = BTreeMap<TimelineId, usize>;
type ChapterEntryMap = BTreeMap<ChapterKey, u16>;
type PlaylistNumberMap = BTreeMap<u64, u8>;
type PlaylistItemMap = BTreeMap<u64, Vec<u8>>;
type PlaylistEndMap = BTreeMap<u64, Link>;
type PlaylistStartMap = BTreeMap<u64, (u16, usize)>;
type PlaylistRouterMap = BTreeMap<u64, u16>;
type PlaylistStepMap = BTreeMap<PlaylistStepKey, u16>;

/// Navigation compilation error.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum NavError {
    /// The project contains an invalid navigation target or structure.
    #[error("invalid navigation: {0}")]
    Invalid(String),

    /// The project contains a navigation target that this milestone does not support.
    #[error("unsupported navigation: {0}")]
    Unsupported(String),
}

/// Compiled navigation for one project.
#[allow(private_interfaces)]
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledNav {
    /// First-play pre commands.
    pub first_play: Vec<Cmd>,
    /// Prohibited user operations of the first-play PGC (the disc's).
    pub first_play_prohibited_ops: u32,
    /// One entry per project menu, in `MenuId` order.
    pub menus: Vec<MenuNav>,
    /// Hidden VMGM PGCs without cells.
    pub hidden: Vec<HiddenPgc>,
    /// One entry per timeline, in `TimelineId` order.
    pub titles: Vec<TitleNav>,
    /// One entry per slideshow, in slideshow id order.
    pub slideshows: Vec<SlideshowNav>,
    /// One entry per chapter playlist, in chapter playlist id order.
    pub chapter_playlists: Vec<ChapterPlaylistNav>,
}

/// Compiled navigation for one menu.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuNav {
    /// Menu id.
    pub menu: MenuId,
    /// VMGM PGCN for this menu.
    pub pgcn: u16,
    /// Menu pre commands.
    pub pre: Vec<Cmd>,
    /// One command per button, in button order.
    pub buttons: Vec<Cmd>,
    /// Post commands: the end action, run when a still menu's duration is over or a motion menu has played
    /// once. Empty when the menu has no end action.
    pub post: Vec<Cmd>,
    /// Whether this is the title menu.
    pub title_menu: bool,
    /// Prohibited user operations while the menu is shown (the DVD PGC field).
    pub prohibited_ops: u32,
}

/// Kind of hidden VMGM PGC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HiddenKind {
    /// Hidden dispatch PGC for a requested chapter.
    ChapterEntry {
        /// Target title number.
        title: u8,
        /// Target chapter number, one-based.
        chapter: u16,
    },
    /// Hidden PGC that returns to the last shown menu.
    LastMenu,
    /// Hidden PGC that starts a playlist.
    PlaylistStart {
        /// Playlist id.
        playlist: u64,
    },
    /// Hidden PGC that routes an active playlist to its router.
    PlaylistDispatch,
    /// Hidden PGC that routes a playlist position to a step or end.
    PlaylistRouter {
        /// Playlist id.
        playlist: u64,
    },
    /// Hidden PGC that advances a playlist to the next item.
    PlaylistStep {
        /// Playlist id.
        playlist: u64,
        /// Current one-based position.
        position: u16,
    },
    /// Hidden PGC that ends a playlist.
    PlaylistEnd {
        /// Playlist id.
        playlist: u64,
    },
    /// Hidden PGC that a title's root menu jumps to when its menu remote is not a menu.
    MenuRemote {
        /// Title number.
        title: u8,
    },
    /// Hidden PGC that routes a pending override (GPRM7) to its target.
    OverrideDispatch,
    /// Hidden PGC that clears the pending override and follows it.
    Override {
        /// One-based override number (the value of GPRM7).
        number: u16,
    },
    /// Hidden PGC a button with an override jumps to: it sets the pending override and follows the button's link.
    ButtonOverride {
        /// VMGM PGCN of the menu.
        menu: u16,
        /// Button number, one-based.
        button: u8,
    },
    /// Hidden PGC a button that selects an audio or subtitle track jumps to: `SetSTN`, then the button's link.
    ButtonStreams {
        /// VMGM PGCN of the menu.
        menu: u16,
        /// Button number, one-based.
        button: u8,
    },
    /// Hidden PGC a button whose link takes more than one command jumps to (a link to a menu's button: GPRM10, then
    /// the link).
    ButtonLink {
        /// VMGM PGCN of the menu.
        menu: u16,
        /// Button number, one-based.
        button: u8,
    },
}

/// Compiled hidden VMGM PGC.
#[derive(Clone, Debug, PartialEq)]
pub struct HiddenPgc {
    /// Hidden PGC kind.
    pub kind: HiddenKind,
    /// VMGM PGCN for this hidden PGC.
    pub pgcn: u16,
    /// Hidden PGC pre commands.
    pub pre: Vec<Cmd>,
}

/// Compiled navigation for one title.
#[derive(Clone, Debug, PartialEq)]
pub struct TitleNav {
    /// Timeline id.
    pub timeline: TimelineId,
    /// Title number, one-based.
    pub title: u8,
    /// Title pre commands.
    pub pre: Vec<Cmd>,
    /// Title post commands.
    pub post: Vec<Cmd>,
    /// Commands of the title set's root menu, which the remote's MENU key calls during the title. Empty when
    /// the timeline has no menu remote (the key then does nothing).
    pub menu_remote: Vec<Cmd>,
    /// Prohibited user operations while the title plays (the DVD PGC field).
    pub prohibited_ops: u32,
}

/// Compiled navigation for one slideshow.
#[derive(Clone, Debug, PartialEq)]
pub struct SlideshowNav {
    /// Slideshow id.
    pub slideshow: u64,
    /// Title number, one-based.
    pub title: u8,
    /// Slideshow pre commands.
    pub pre: Vec<Cmd>,
    /// Slideshow post commands.
    pub post: Vec<Cmd>,
}

/// Compiled navigation for one chapter playlist: a title of its own in the master timeline's title set.
#[derive(Clone, Debug, PartialEq)]
pub struct ChapterPlaylistNav {
    /// Chapter playlist id.
    pub chapter_playlist: u64,
    /// The master timeline.
    pub timeline: TimelineId,
    /// Title number, one-based.
    pub title: u8,
    /// Title number within the title set (VTS_TTN): 1 is the master timeline, chapter playlists follow in id
    /// order.
    pub vts_ttn: u8,
    /// The entries as one-based program (chapter) numbers of the master timeline.
    pub programs: Vec<u16>,
    /// How the entries play (the PGC's PG playback mode).
    pub order: PlaybackOrder,
    /// Title pre commands.
    pub pre: Vec<Cmd>,
    /// Title post commands.
    pub post: Vec<Cmd>,
    /// Prohibited user operations while it plays (the DVD PGC field).
    pub prohibited_ops: u32,
}

/// Mutable compiler state.
struct Context {
    menu_pgcn: MenuPgcMap,
    timeline_title: TimelineTitleMap,
    slideshow_title: SlideshowTitleMap,
    chapter_playlist_title: ChapterPlaylistTitleMap,
    chapter_counts: ChapterCountMap,
    menu_count: usize,
    hidden: Vec<HiddenPgc>,
    chapter_entries: ChapterEntryMap,
    last_menu: Option<u16>,
    current_menu: u16,
    playlist_number: PlaylistNumberMap,
    playlist_items: PlaylistItemMap,
    playlist_end: PlaylistEndMap,
    playlist_start: PlaylistStartMap,
    playlist_router: PlaylistRouterMap,
    playlist_step: PlaylistStepMap,
    playlist_dispatch: Option<u16>,
    /// Whether any timeline has an override (menus then clear the pending override).
    has_overrides: bool,
    /// How many buttons each menu has (a link can highlight one of them).
    menu_buttons: BTreeMap<MenuId, usize>,
}

impl Context {
    /// The override dispatch PGC, once built.
    fn override_dispatch(&self) -> Option<u16> {
        self.hidden
            .iter()
            .find(|hidden| hidden.kind == HiddenKind::OverrideDispatch)
            .map(|hidden| hidden.pgcn)
    }
}

/// Precomputed playlist data.
struct PlaylistData {
    number: PlaylistNumberMap,
    items: PlaylistItemMap,
    end: PlaylistEndMap,
}

/// Compiles project links into DVD VM commands.
///
/// # Errors
///
/// Returns [`NavError::Invalid`] for missing targets or structure limits, and
/// [`NavError::Unsupported`] for playlist-like targets.
pub fn compile(project: &Project) -> Result<CompiledNav, NavError> {
    let menu_pgcn = build_menu_pgcn(project)?;
    let (timeline_title, slideshow_title, chapter_playlist_title) = build_title_maps(project)?;
    let chapter_counts = build_chapter_counts(project);
    let title_menu = resolve_title_menu(project, &menu_pgcn)?;
    let playlists = build_playlist_data(
        project,
        &timeline_title,
        &slideshow_title,
        &chapter_playlist_title,
    )?;

    let mut ctx = Context {
        menu_pgcn,
        timeline_title,
        slideshow_title,
        chapter_playlist_title,
        chapter_counts,
        menu_count: project.menus.len(),
        hidden: Vec::new(),
        chapter_entries: BTreeMap::new(),
        last_menu: None,
        current_menu: 0,
        playlist_number: playlists.number,
        playlist_items: playlists.items,
        playlist_end: playlists.end,
        playlist_start: BTreeMap::new(),
        playlist_router: BTreeMap::new(),
        playlist_step: BTreeMap::new(),
        playlist_dispatch: None,
        has_overrides: !override_targets(project).is_empty(),
        menu_buttons: project
            .menus
            .iter()
            .map(|(id, menu)| (*id, menu.buttons.len()))
            .collect(),
    };

    let menus = compile_menus(project, &mut ctx, title_menu)?;
    let titles = compile_titles(project, &mut ctx)?;
    let slideshows = compile_slideshows(project, &mut ctx)?;
    let chapter_playlists = compile_chapter_playlists(project, &mut ctx)?;
    let mut first_play = Vec::new();
    // Encore's Set Audio / Set Subtitle: the disc picks the streams before anything plays.
    if let Some(select) = stream_selection(&project.settings) {
        first_play.push(select);
    }
    // The disc's override (Encore): the first play sets the pending override before it plays a title.
    let overrides = override_targets(project);
    let number = override_of(
        &project.settings.first_play,
        project.settings.first_play_override.as_ref(),
    )
    .and_then(|link| override_number_of(&overrides, link));
    if let Some(number) = number {
        let _ = ensure_override_dispatch(&mut ctx, &overrides)?;
        first_play.push(cmd::set_gprm(GPRM_OVERRIDE, number));
    }
    first_play.extend(compile_first_play(&mut ctx, &project.settings.first_play)?);
    check_pgc_limit(&ctx)?;

    Ok(CompiledNav {
        first_play,
        first_play_prohibited_ops: disccraft_project::dvd_uop_mask(
            &project.settings.prohibited_ops,
        ),
        menus,
        hidden: ctx.hidden,
        titles,
        slideshows,
        chapter_playlists,
    })
}

/// Builds menu id to VMGM PGCN mapping.
fn build_menu_pgcn(project: &Project) -> Result<MenuPgcMap, NavError> {
    let mut map = BTreeMap::new();

    for (index, id) in project.menus.keys().enumerate() {
        if index >= MAX_VTGM_PGCN {
            return Err(NavError::Invalid("too many menus".to_string()));
        }

        let pgcn = u16::try_from(
            index
                .checked_add(1)
                .ok_or_else(|| NavError::Invalid("menu count overflow".to_string()))?,
        )
        .map_err(|_| NavError::Invalid("too many menus".to_string()))?;

        map.insert(*id, pgcn);
    }

    Ok(map)
}

/// Builds the title number maps: timelines, then slideshows, then chapter playlists.
fn build_title_maps(
    project: &Project,
) -> Result<(TimelineTitleMap, SlideshowTitleMap, ChapterPlaylistTitleMap), NavError> {
    let mut timeline_title = TimelineTitleMap::new();
    let mut slideshow_title = SlideshowTitleMap::new();
    let mut next = 1usize;

    for id in project.timelines.keys() {
        let title = title_number(next)?;
        timeline_title.insert(*id, title);
        next = next
            .checked_add(1)
            .ok_or_else(|| NavError::Invalid("too many titles".to_string()))?;
    }

    for id in project.slideshows.keys() {
        let title = title_number(next)?;
        slideshow_title.insert(*id, title);
        next = next
            .checked_add(1)
            .ok_or_else(|| NavError::Invalid("too many titles".to_string()))?;
    }

    let mut chapter_playlist_title = ChapterPlaylistTitleMap::new();
    for id in project.chapter_playlists.keys() {
        let title = title_number(next)?;
        chapter_playlist_title.insert(*id, title);
        next = next
            .checked_add(1)
            .ok_or_else(|| NavError::Invalid("too many titles".to_string()))?;
    }

    Ok((timeline_title, slideshow_title, chapter_playlist_title))
}

/// Converts a one-based title index to a title number.
fn title_number(next: usize) -> Result<u8, NavError> {
    if next > MAX_TITLES {
        return Err(NavError::Invalid("too many titles".to_string()));
    }

    u8::try_from(next).map_err(|_| NavError::Invalid("too many titles".to_string()))
}

/// Builds timeline id to chapter count mapping.
fn build_chapter_counts(project: &Project) -> ChapterCountMap {
    project
        .timelines
        .iter()
        .map(|(id, timeline)| (*id, timeline.chapters.len()))
        .collect()
}

/// Builds playlist number, item-title, and end-action maps. Items are timelines, slideshows or chapter
/// playlists: all titles.
fn build_playlist_data(
    project: &Project,
    timeline_title: &TimelineTitleMap,
    slideshow_title: &SlideshowTitleMap,
    chapter_playlist_title: &ChapterPlaylistTitleMap,
) -> Result<PlaylistData, NavError> {
    let mut number = PlaylistNumberMap::new();
    let mut items = PlaylistItemMap::new();
    let mut end = PlaylistEndMap::new();

    for (index, (id, playlist)) in project.playlists.iter().enumerate() {
        let p = u8::try_from(
            index
                .checked_add(1)
                .ok_or_else(|| NavError::Invalid("too many playlists".to_string()))?,
        )
        .map_err(|_| NavError::Invalid("too many playlists".to_string()))?;

        let mut titles = Vec::with_capacity(playlist.items.len());
        for item in &playlist.items {
            let title = match item {
                PlaylistItem::Timeline(timeline) => timeline_title.get(timeline).copied(),
                PlaylistItem::Slideshow(slideshow) => slideshow_title.get(slideshow).copied(),
                PlaylistItem::ChapterPlaylist(list) => chapter_playlist_title.get(list).copied(),
            }
            .ok_or_else(|| {
                NavError::Invalid(format!("playlist {id} item {} does not exist", item.id()))
            })?;
            titles.push(title);
        }

        number.insert(*id, p);
        items.insert(*id, titles);
        end.insert(*id, playlist.end_action.clone());
    }

    Ok(PlaylistData { number, items, end })
}

/// Resolves the title menu id.
fn resolve_title_menu(
    project: &Project,
    menu_pgcn: &MenuPgcMap,
) -> Result<Option<MenuId>, NavError> {
    if let Link::Menu { id, .. } = &project.settings.title_remote {
        if menu_pgcn.contains_key(id) {
            return Ok(Some(*id));
        }

        return Err(NavError::Invalid(format!(
            "title remote menu {id} does not exist"
        )));
    }

    Ok(project.menus.keys().next().copied())
}

/// Compiles all menu navigation.
fn compile_menus(
    project: &Project,
    ctx: &mut Context,
    title_menu: Option<MenuId>,
) -> Result<Vec<MenuNav>, NavError> {
    let mut menus = Vec::with_capacity(project.menus.len());
    let overrides = override_targets(project);
    let _ = ensure_override_dispatch(ctx, &overrides)?;

    for (id, menu) in &project.menus {
        let pgcn = ctx
            .menu_pgcn
            .get(id)
            .copied()
            .ok_or_else(|| NavError::Invalid(format!("menu {id} does not exist")))?;

        ctx.current_menu = pgcn;
        // Lines 1 to 4: highlight the menu's own button, or the one a link asked for (Encore's link to a menu's
        // button). The HLI forces no button, so a motion menu's later plays (LinkPGN) keep the viewer's.
        let mut pre = Vec::new();
        if let Some(own) = default_highlight(menu) {
            pre.extend([
                cmd::set_hl_btnn(own),
                cmd::if_eq_goto(GPRM_SELECT, 0, 4),
                cmd::set_hl_btnn_gprm(GPRM_SELECT),
                cmd::set_gprm(GPRM_SELECT, 0),
            ]);
        }
        pre.extend([
            cmd::set_gprm(GPRM_ACTIVE_PLAYLIST, 0),
            cmd::set_gprm(GPRM_LAST_MENU, pgcn),
        ]);
        // Reaching a menu ends any pending override.
        if ctx.has_overrides {
            pre.push(cmd::set_gprm(GPRM_OVERRIDE, 0));
        }

        let mut buttons = Vec::with_capacity(menu.buttons.len());
        for (index, button) in menu.buttons.iter().enumerate() {
            // A button with an override, or one that selects an audio / subtitle track, goes through a hidden
            // PGC that sets those first (a button command is a single VM command).
            let number = override_of(&button.link, button.override_link.as_ref())
                .and_then(|link| override_number_of(&overrides, link));
            let streams = stn_command(button.audio, button.subtitle);
            if number.is_some() || streams.is_some() {
                let button_number = u8::try_from(index.saturating_add(1))
                    .map_err(|_| NavError::Invalid("too many buttons".to_string()))?;
                let kind = if number.is_some() {
                    HiddenKind::ButtonOverride {
                        menu: pgcn,
                        button: button_number,
                    }
                } else {
                    HiddenKind::ButtonStreams {
                        menu: pgcn,
                        button: button_number,
                    }
                };
                let (hidden, slot) = push_hidden(ctx, kind)?;
                let mut pre = Vec::new();
                if let Some(number) = number {
                    pre.push(cmd::set_gprm(GPRM_OVERRIDE, number));
                }
                pre.extend(streams);
                pre.extend(link_command(ctx, &button.link, false)?);
                set_hidden_pre(ctx, slot, pre)?;
                buttons.push(cmd::link_pgcn(hidden));
                continue;
            }
            let commands = link_command(ctx, &button.link, false)?;
            if commands.len() > 1 {
                // A link to a menu's button: GPRM10, then the link (a button command is a single VM command).
                let button_number = u8::try_from(index.saturating_add(1))
                    .map_err(|_| NavError::Invalid("too many buttons".to_string()))?;
                let (hidden, slot) = push_hidden(
                    ctx,
                    HiddenKind::ButtonLink {
                        menu: pgcn,
                        button: button_number,
                    },
                )?;
                set_hidden_pre(ctx, slot, commands)?;
                buttons.push(cmd::link_pgcn(hidden));
                continue;
            }
            buttons.push(single_command(commands)?);
        }
        // The end action; with an override (and an end action that plays a title), the pending override is set
        // first, as for a timeline.
        let post = match &menu.end_action {
            Link::NotSet => Vec::new(),
            link => {
                let mut post = Vec::new();
                let number = override_of(link, menu.override_link.as_ref())
                    .and_then(|target| override_number_of(&overrides, target));
                if let Some(number) = number {
                    post.push(cmd::set_gprm(GPRM_OVERRIDE, number));
                }
                post.extend(link_command(ctx, link, false)?);
                post
            }
        };

        menus.push(MenuNav {
            menu: *id,
            pgcn,
            pre,
            buttons,
            post,
            title_menu: title_menu == Some(*id),
            prohibited_ops: disccraft_project::dvd_uop_mask(&menu.prohibited_ops)
                | disccraft_project::dvd_uop_mask(&project.settings.prohibited_ops),
        });
    }

    Ok(menus)
}

/// Compiles all title navigation.
fn compile_titles(project: &Project, ctx: &mut Context) -> Result<Vec<TitleNav>, NavError> {
    let mut titles = Vec::with_capacity(project.timelines.len());
    let overrides = override_targets(project);
    let override_dispatch = ensure_override_dispatch(ctx, &overrides)?;

    for (id, timeline) in &project.timelines {
        let title = resolve_timeline(ctx, *id)?;
        let pre = compile_title_pre(timeline.chapters.len())?;

        let mut post = Vec::new();
        // A pending override (set by the title that led here) wins over this title's own end action.
        if let Some(dispatch) = override_dispatch {
            push_override_check(&mut post, dispatch);
        }
        if !ctx.playlist_number.is_empty() {
            let dispatch = ensure_playlist_dispatch(ctx)?;
            push_playlist_check(&mut post, title, dispatch);
        }
        if let Some(number) = override_number(&overrides, timeline) {
            post.push(cmd::set_gprm(GPRM_OVERRIDE, number));
        }
        post.extend(link_command(ctx, &timeline.end_action, true)?);
        let menu_remote = compile_menu_remote(ctx, &timeline.menu_remote, title)?;

        titles.push(TitleNav {
            timeline: *id,
            title,
            pre,
            post,
            menu_remote,
            prohibited_ops: disccraft_project::dvd_uop_mask(&timeline.prohibited_ops)
                | disccraft_project::dvd_uop_mask(&project.settings.prohibited_ops),
        });
    }

    Ok(titles)
}

/// Compiles a title's root menu (the remote's MENU key). The root menu lives in the title set, so it jumps to the
/// VMGM: straight to a menu, or through a hidden PGC that follows any other link like a button would.
fn compile_menu_remote(ctx: &mut Context, link: &Link, title: u8) -> Result<Vec<Cmd>, NavError> {
    let pgcn = match link {
        Link::NotSet => return Ok(Vec::new()),
        Link::Menu { id, .. } => resolve_menu(ctx, *id)?,
        Link::LastMenu => ensure_last_menu(ctx)?,
        other => {
            let pre = link_command(ctx, other, false)?;
            let (pgcn, index) = push_hidden(ctx, HiddenKind::MenuRemote { title })?;
            set_hidden_pre(ctx, index, pre)?;
            pgcn
        }
    };
    Ok(vec![cmd::jump_ss_vmgm_pgcn(pgcn)])
}

/// Compiles all slideshow navigation.
fn compile_slideshows(project: &Project, ctx: &mut Context) -> Result<Vec<SlideshowNav>, NavError> {
    let mut slideshows = Vec::with_capacity(project.slideshows.len());

    for (id, slideshow) in &project.slideshows {
        let title = resolve_slideshow(ctx, *id)?;
        let mut post = Vec::new();
        if let Some(dispatch) = ctx.override_dispatch() {
            push_override_check(&mut post, dispatch);
        }
        post.extend(link_command(ctx, &slideshow.end_action, true)?);

        slideshows.push(SlideshowNav {
            slideshow: *id,
            title,
            pre: Vec::new(),
            post,
        });
    }

    Ok(slideshows)
}

/// Compiles all chapter playlist navigation. A chapter playlist is a title in its master timeline's title set:
/// its pre commands dispatch a requested chapter like a timeline's, its post commands check overrides and
/// playlists, then follow its end action.
fn compile_chapter_playlists(
    project: &Project,
    ctx: &mut Context,
) -> Result<Vec<ChapterPlaylistNav>, NavError> {
    let mut out = Vec::with_capacity(project.chapter_playlists.len());
    let overrides = override_targets(project);
    let override_dispatch = ensure_override_dispatch(ctx, &overrides)?;
    let mut ttn_of_timeline: BTreeMap<TimelineId, u8> = BTreeMap::new();

    for (id, playlist) in &project.chapter_playlists {
        let title = resolve_chapter_playlist(ctx, *id)?;
        let timeline = project.timelines.get(&playlist.timeline).ok_or_else(|| {
            NavError::Invalid(format!(
                "chapter playlist {id} targets missing timeline {}",
                playlist.timeline
            ))
        })?;
        let programs = playlist
            .program_numbers(timeline.chapters.len())
            .ok_or_else(|| {
                NavError::Invalid(format!("chapter playlist {id} names a missing chapter"))
            })?;
        if programs.is_empty() {
            return Err(NavError::Invalid(format!(
                "chapter playlist {id} has no entries"
            )));
        }
        let next_ttn = ttn_of_timeline.entry(playlist.timeline).or_insert(1);
        *next_ttn = next_ttn
            .checked_add(1)
            .ok_or_else(|| NavError::Invalid("too many chapter playlists".to_string()))?;
        let vts_ttn = *next_ttn;

        let pre = compile_title_pre(programs.len())?;
        let mut post = Vec::new();
        if let Some(dispatch) = override_dispatch {
            push_override_check(&mut post, dispatch);
        }
        if !ctx.playlist_number.is_empty() {
            let dispatch = ensure_playlist_dispatch(ctx)?;
            push_playlist_check(&mut post, title, dispatch);
        }
        let number = override_of(&playlist.end_action, playlist.override_link.as_ref())
            .and_then(|target| override_number_of(&overrides, target));
        if let Some(number) = number {
            post.push(cmd::set_gprm(GPRM_OVERRIDE, number));
        }
        post.extend(link_command(ctx, &playlist.end_action, true)?);

        out.push(ChapterPlaylistNav {
            chapter_playlist: *id,
            timeline: playlist.timeline,
            title,
            vts_ttn,
            programs,
            order: playlist.order,
            pre,
            post,
            prohibited_ops: disccraft_project::dvd_uop_mask(&playlist.prohibited_ops)
                | disccraft_project::dvd_uop_mask(&project.settings.prohibited_ops),
        });
    }

    Ok(out)
}

/// Compiles title pre commands.
fn compile_title_pre(chapters: usize) -> Result<Vec<Cmd>, NavError> {
    let mut pre = vec![
        cmd::mov_gprm(GPRM_SCRATCH, GPRM_REQUESTED),
        cmd::set_gprm(GPRM_REQUESTED, 0),
    ];

    // LinkPGN takes an 8-bit program number (a DVD title has at most 99 chapters).
    let max =
        u8::try_from(chapters).map_err(|_| NavError::Invalid("too many chapters".to_string()))?;

    pre.extend(
        (2..=max).map(|chapter| cmd::if_eq_link_pgn(GPRM_SCRATCH, u16::from(chapter), chapter)),
    );

    Ok(pre)
}

/// Compiles first-play commands.
fn compile_first_play(ctx: &mut Context, link: &Link) -> Result<Vec<Cmd>, NavError> {
    match link {
        Link::Stop => Ok(vec![cmd::exit()]),
        Link::NotSet | Link::LastMenu => {
            if ctx.menu_count > 0 {
                Ok(vec![cmd::jump_ss_vmgm_pgcn(1)])
            } else {
                Ok(vec![cmd::jump_tt(1)])
            }
        }
        Link::Menu { id, .. } => {
            let pgcn = resolve_menu(ctx, *id)?;
            Ok(vec![cmd::jump_ss_vmgm_pgcn(pgcn)])
        }
        Link::Timeline { id, chapter } => timeline_first_play_commands(ctx, *id, *chapter),
        Link::Playlist { id } => {
            let pgcn = ensure_playlist_start(ctx, *id)?;
            Ok(vec![cmd::link_pgcn(pgcn)])
        }
        Link::Slideshow { id } => {
            let title = resolve_slideshow(ctx, *id)?;
            Ok(vec![cmd::jump_tt(title)])
        }
        Link::ChapterPlaylist { id } => {
            let title = resolve_chapter_playlist(ctx, *id)?;
            Ok(vec![cmd::jump_tt(title)])
        }
    }
}

/// The `SetSTN` for the disc's audio / subtitle defaults, when either is set.
fn stream_selection(settings: &disccraft_project::DiscSettings) -> Option<Cmd> {
    stn_command(settings.audio_default, settings.subtitle_default)
}

/// The `SetSTN` that selects audio track `audio` (1 to 8) and / or a subtitle, when either is given.
fn stn_command(audio: Option<u8>, subtitle: disccraft_project::SubtitleDefault) -> Option<Cmd> {
    use disccraft_project::SubtitleDefault;
    let audio = audio
        .filter(|track| (1..=8).contains(track))
        .map(|track| track - 1);
    let subpicture = match subtitle {
        SubtitleDefault::NoChange => None,
        SubtitleDefault::Off => Some((false, 0)),
        SubtitleDefault::Stream(track) if (1..=32).contains(&track) => Some((true, track - 1)),
        SubtitleDefault::Stream(_) => None,
    };
    (audio.is_some() || subpicture.is_some()).then(|| cmd::set_stn(audio, subpicture))
}

/// Compiles first-play-like commands for a timeline target.
fn timeline_first_play_commands(
    ctx: &Context,
    id: TimelineId,
    chapter: u32,
) -> Result<Vec<Cmd>, NavError> {
    let title = resolve_timeline(ctx, id)?;
    let chapter_number = validate_chapter(ctx, id, chapter)?;

    if chapter_number > 1 {
        return Ok(vec![
            cmd::set_gprm(GPRM_REQUESTED, chapter_number),
            cmd::jump_tt(title),
        ]);
    }

    Ok(vec![cmd::jump_tt(title)])
}

/// Resolves one link into one or more VM commands.
fn link_command(ctx: &mut Context, link: &Link, from_title: bool) -> Result<Vec<Cmd>, NavError> {
    match link {
        Link::Stop => Ok(vec![cmd::exit()]),
        Link::NotSet => {
            if from_title {
                Ok(vec![cmd::exit()])
            } else {
                Ok(vec![cmd::link_pgcn(ctx.current_menu)])
            }
        }
        Link::LastMenu => link_last_menu(ctx, from_title),
        Link::Menu { id, button } => link_menu(ctx, *id, *button, from_title),
        Link::Timeline { id, chapter } => link_timeline(ctx, *id, *chapter, from_title),
        Link::Playlist { id } => link_playlist(ctx, *id, from_title),
        Link::Slideshow { id } => {
            let title = resolve_slideshow(ctx, *id)?;
            Ok(vec![cmd::jump_tt(title)])
        }
        Link::ChapterPlaylist { id } => {
            let title = resolve_chapter_playlist(ctx, *id)?;
            Ok(vec![cmd::jump_tt(title)])
        }
    }
}

/// Resolves a last-menu link.
fn link_last_menu(ctx: &mut Context, from_title: bool) -> Result<Vec<Cmd>, NavError> {
    let pgcn = ensure_last_menu(ctx)?;

    if from_title {
        Ok(vec![cmd::call_ss_vmgm_pgcn(pgcn, 0)])
    } else {
        Ok(vec![cmd::link_pgcn(pgcn)])
    }
}

/// Resolves a menu link. A link to one of the menu's buttons (an index) asks the menu to highlight it (GPRM10).
fn link_menu(
    ctx: &Context,
    id: MenuId,
    button: Option<u32>,
    from_title: bool,
) -> Result<Vec<Cmd>, NavError> {
    let pgcn = resolve_menu(ctx, id)?;
    let count = ctx.menu_buttons.get(&id).copied().unwrap_or(0);
    let mut commands = Vec::new();
    if let Some(number) = button
        .and_then(|button| usize::try_from(button).ok())
        .filter(|button| *button < count)
        .and_then(|button| u8::try_from(button.saturating_add(1)).ok())
    {
        commands.push(cmd::set_gprm(GPRM_SELECT, cmd::highlight_value(number)));
    }
    commands.push(if from_title {
        cmd::call_ss_vmgm_pgcn(pgcn, 0)
    } else {
        cmd::link_pgcn(pgcn)
    });
    Ok(commands)
}

/// The button (one-based) a menu highlights when it is entered without one of its own: its default (the last
/// button when the default is out of range). `None` for a menu without buttons.
fn default_highlight(menu: &disccraft_project::Menu) -> Option<u8> {
    let count = menu.buttons.len();
    if count == 0 {
        return None;
    }
    let index = usize::try_from(menu.default_button)
        .unwrap_or(count)
        .min(count.saturating_sub(1));
    u8::try_from(index.saturating_add(1)).ok()
}

/// Resolves a timeline chapter link.
fn link_timeline(
    ctx: &mut Context,
    id: TimelineId,
    chapter: u32,
    from_title: bool,
) -> Result<Vec<Cmd>, NavError> {
    let title = resolve_timeline(ctx, id)?;
    let chapter_number = validate_chapter(ctx, id, chapter)?;

    if from_title {
        if chapter_number > 1 {
            return Ok(vec![
                cmd::set_gprm(GPRM_REQUESTED, chapter_number),
                cmd::jump_tt(title),
            ]);
        }

        return Ok(vec![cmd::jump_tt(title)]);
    }

    if chapter_number == 1 {
        return Ok(vec![cmd::jump_tt(title)]);
    }

    let pgcn = ensure_chapter_entry(ctx, title, chapter_number)?;
    Ok(vec![cmd::link_pgcn(pgcn)])
}

/// Resolves a playlist link.
fn link_playlist(ctx: &mut Context, id: u64, from_title: bool) -> Result<Vec<Cmd>, NavError> {
    let pgcn = ensure_playlist_start(ctx, id)?;

    if from_title {
        Ok(vec![cmd::call_ss_vmgm_pgcn(pgcn, 0)])
    } else {
        Ok(vec![cmd::link_pgcn(pgcn)])
    }
}

/// Ensures the hidden last-menu PGC exists.
fn ensure_last_menu(ctx: &mut Context) -> Result<u16, NavError> {
    if let Some(pgcn) = ctx.last_menu {
        return Ok(pgcn);
    }

    if ctx.menu_count == 0 {
        return Err(NavError::Invalid(
            "last menu requires at least one menu".to_string(),
        ));
    }

    let pgcn = next_hidden_pgcn(ctx)?;
    let menu_count = u16::try_from(ctx.menu_count)
        .map_err(|_| NavError::Invalid("too many menus".to_string()))?;

    let mut pre: Vec<Cmd> = (1..=menu_count)
        .map(|menu_pgcn| cmd::if_eq_link_pgcn(GPRM_LAST_MENU, menu_pgcn, menu_pgcn))
        .collect();
    pre.push(cmd::link_pgcn(1));

    ctx.hidden.push(HiddenPgc {
        kind: HiddenKind::LastMenu,
        pgcn,
        pre,
    });
    ctx.last_menu = Some(pgcn);

    Ok(pgcn)
}

/// Ensures a hidden chapter-entry PGC exists.
fn ensure_chapter_entry(ctx: &mut Context, title: u8, chapter: u16) -> Result<u16, NavError> {
    let key = (title, chapter);

    if let Some(&pgcn) = ctx.chapter_entries.get(&key) {
        return Ok(pgcn);
    }

    let pgcn = next_hidden_pgcn(ctx)?;
    let pre = vec![cmd::set_gprm(GPRM_REQUESTED, chapter), cmd::jump_tt(title)];

    ctx.hidden.push(HiddenPgc {
        kind: HiddenKind::ChapterEntry { title, chapter },
        pgcn,
        pre,
    });
    ctx.chapter_entries.insert(key, pgcn);

    Ok(pgcn)
}

/// Ensures a hidden playlist-start PGC exists.
fn ensure_playlist_start(ctx: &mut Context, id: u64) -> Result<u16, NavError> {
    if let Some(&(pgcn, _)) = ctx.playlist_start.get(&id) {
        return Ok(pgcn);
    }

    let (pgcn, index) = push_hidden(ctx, HiddenKind::PlaylistStart { playlist: id })?;
    ctx.playlist_start.insert(id, (pgcn, index));

    let pre = playlist_start_pre(ctx, id)?;
    set_hidden_pre(ctx, index, pre)?;

    Ok(pgcn)
}

/// Ensures the hidden playlist-dispatch PGC exists.
fn ensure_playlist_dispatch(ctx: &mut Context) -> Result<u16, NavError> {
    if let Some(pgcn) = ctx.playlist_dispatch {
        return Ok(pgcn);
    }

    let ids: Vec<u64> = ctx.playlist_number.keys().copied().collect();
    let mut routers = Vec::new();
    for id in &ids {
        routers.push(ensure_playlist_router(ctx, *id)?);
    }

    let (pgcn, index) = push_hidden(ctx, HiddenKind::PlaylistDispatch)?;
    ctx.playlist_dispatch = Some(pgcn);

    let mut pre = Vec::new();
    for (id, router) in ids.iter().zip(routers) {
        let p = playlist_number(ctx, *id)?;
        pre.push(cmd::if_eq_link_pgcn(
            GPRM_ACTIVE_PLAYLIST,
            u16::from(p),
            router,
        ));
    }
    pre.extend(fallback_commands(ctx));
    set_hidden_pre(ctx, index, pre)?;

    Ok(pgcn)
}

/// Ensures a hidden playlist-router PGC exists.
fn ensure_playlist_router(ctx: &mut Context, id: u64) -> Result<u16, NavError> {
    if let Some(&pgcn) = ctx.playlist_router.get(&id) {
        return Ok(pgcn);
    }

    let count = playlist_item_count(ctx, id)?;
    let (pgcn, index) = push_hidden(ctx, HiddenKind::PlaylistRouter { playlist: id })?;
    ctx.playlist_router.insert(id, pgcn);

    let mut pre = Vec::new();
    if count > 0 {
        for position in 1..=count {
            let target = ensure_playlist_step_or_end(ctx, id, position)?;
            pre.push(cmd::if_eq_link_pgcn(GPRM_POSITION, position, target));
        }
    }
    pre.extend(fallback_commands(ctx));
    set_hidden_pre(ctx, index, pre)?;

    Ok(pgcn)
}

/// Ensures a hidden playlist step or end PGC exists for one position.
fn ensure_playlist_step_or_end(ctx: &mut Context, id: u64, position: u16) -> Result<u16, NavError> {
    let key = (id, position);

    if let Some(&pgcn) = ctx.playlist_step.get(&key) {
        return Ok(pgcn);
    }

    let count = playlist_item_count(ctx, id)?;
    if position == 0 || position > count {
        return Err(NavError::Invalid(
            "playlist position out of range".to_string(),
        ));
    }

    let kind = if position < count {
        HiddenKind::PlaylistStep {
            playlist: id,
            position,
        }
    } else {
        HiddenKind::PlaylistEnd { playlist: id }
    };

    let (pgcn, index) = push_hidden(ctx, kind)?;
    ctx.playlist_step.insert(key, pgcn);

    let pre = if position < count {
        playlist_step_pre(ctx, id, position)?
    } else {
        playlist_end_pre(ctx, id)?
    };
    set_hidden_pre(ctx, index, pre)?;

    Ok(pgcn)
}

/// Builds playlist-start pre commands.
fn playlist_start_pre(ctx: &mut Context, id: u64) -> Result<Vec<Cmd>, NavError> {
    let p = playlist_number(ctx, id)?;
    let empty = ctx
        .playlist_items
        .get(&id)
        .map(|items| items.is_empty())
        .ok_or_else(|| NavError::Invalid(format!("playlist {id} does not exist")))?;

    if empty {
        return playlist_end_commands(ctx, id);
    }

    let first = ctx
        .playlist_items
        .get(&id)
        .and_then(|items| items.first().copied())
        .ok_or_else(|| NavError::Invalid(format!("playlist {id} has no first item")))?;

    Ok(vec![
        cmd::set_gprm(GPRM_ACTIVE_PLAYLIST, u16::from(p)),
        cmd::set_gprm(GPRM_POSITION, 1),
        cmd::set_gprm(GPRM_REQUESTED, 0),
        cmd::jump_tt(first),
    ])
}

/// Builds playlist-step pre commands.
fn playlist_step_pre(ctx: &Context, id: u64, position: u16) -> Result<Vec<Cmd>, NavError> {
    let next_position = position
        .checked_add(1)
        .ok_or_else(|| NavError::Invalid("playlist position overflow".to_string()))?;
    let next_title = playlist_title_at(ctx, id, position)?;

    Ok(vec![
        cmd::set_gprm(GPRM_POSITION, next_position),
        cmd::set_gprm(GPRM_REQUESTED, 0),
        cmd::jump_tt(next_title),
    ])
}

/// Builds playlist-end pre commands.
fn playlist_end_pre(ctx: &mut Context, id: u64) -> Result<Vec<Cmd>, NavError> {
    let mut pre = vec![
        cmd::set_gprm(GPRM_ACTIVE_PLAYLIST, 0),
        cmd::set_gprm(GPRM_POSITION, 0),
    ];
    pre.extend(playlist_end_commands(ctx, id)?);
    Ok(pre)
}

/// Builds playlist end-action commands for the VMGM domain.
fn playlist_end_commands(ctx: &mut Context, id: u64) -> Result<Vec<Cmd>, NavError> {
    let end = ctx
        .playlist_end
        .get(&id)
        .cloned()
        .ok_or_else(|| NavError::Invalid(format!("playlist {id} does not exist")))?;

    match end {
        Link::Stop | Link::NotSet => Ok(vec![cmd::exit()]),
        Link::LastMenu => {
            let pgcn = ensure_last_menu(ctx)?;
            Ok(vec![cmd::link_pgcn(pgcn)])
        }
        Link::Menu { id: menu_id, .. } => {
            let pgcn = resolve_menu(ctx, menu_id)?;
            Ok(vec![cmd::link_pgcn(pgcn)])
        }
        Link::Timeline { id, chapter } => timeline_first_play_commands(ctx, id, chapter),
        Link::Playlist { id: playlist_id } => {
            let pgcn = ensure_playlist_start(ctx, playlist_id)?;
            Ok(vec![cmd::link_pgcn(pgcn)])
        }
        Link::Slideshow { id: slideshow_id } => {
            let title = resolve_slideshow(ctx, slideshow_id)?;
            Ok(vec![cmd::jump_tt(title)])
        }
        Link::ChapterPlaylist { id } => {
            Err(NavError::Unsupported(format!("chapter playlist link {id}")))
        }
    }
}

/// Builds fallback commands for playlist dispatch and routers.
fn fallback_commands(ctx: &Context) -> Vec<Cmd> {
    let mut pre = vec![cmd::set_gprm(GPRM_ACTIVE_PLAYLIST, 0)];

    if ctx.menu_count > 0 {
        pre.push(cmd::link_pgcn(1));
    } else {
        pre.push(cmd::exit());
    }

    pre
}

/// The overrides in the project, without repeats, in timeline order. A timeline's override counts only when its
/// end action leads to another title (a timeline, chapter, slideshow or playlist): the override replaces that
/// title's end action.
fn override_targets(project: &Project) -> Vec<Link> {
    let mut targets: Vec<Link> = Vec::new();
    let mut add = |link: Option<&Link>| {
        if let Some(link) = link
            && !targets.contains(link)
        {
            targets.push(link.clone());
        }
    };
    add(override_of(
        &project.settings.first_play,
        project.settings.first_play_override.as_ref(),
    ));
    for timeline in project.timelines.values() {
        add(effective_override(timeline));
    }
    for playlist in project.chapter_playlists.values() {
        add(override_of(
            &playlist.end_action,
            playlist.override_link.as_ref(),
        ));
    }
    for menu in project.menus.values() {
        add(override_of(&menu.end_action, menu.override_link.as_ref()));
        for button in &menu.buttons {
            add(override_of(&button.link, button.override_link.as_ref()));
        }
    }
    targets
}

/// An override, when it applies: `link` (an end action or a button's link) plays a title and `override_link` is set.
fn override_of<'a>(link: &Link, override_link: Option<&'a Link>) -> Option<&'a Link> {
    let leads_to_title = matches!(
        link,
        Link::Timeline { .. }
            | Link::Slideshow { .. }
            | Link::Playlist { .. }
            | Link::ChapterPlaylist { .. }
    );
    override_link.filter(|link| leads_to_title && !matches!(link, Link::NotSet))
}

/// A timeline's override, when it applies (see [`override_targets`]).
fn effective_override(timeline: &disccraft_project::Timeline) -> Option<&Link> {
    override_of(&timeline.end_action, timeline.override_link.as_ref())
}

/// The one-based number of a timeline's override among `targets`.
fn override_number(targets: &[Link], timeline: &disccraft_project::Timeline) -> Option<u16> {
    override_number_of(targets, effective_override(timeline)?)
}

/// The one-based number of `link` among the override `targets`.
fn override_number_of(targets: &[Link], link: &Link) -> Option<u16> {
    let index = targets.iter().position(|target| target == link)?;
    u16::try_from(index.checked_add(1)?).ok()
}

/// Builds the hidden override PGCs (one per target, plus the dispatch that routes GPRM7 to them). `None` when the
/// project has no overrides.
fn ensure_override_dispatch(ctx: &mut Context, targets: &[Link]) -> Result<Option<u16>, NavError> {
    if targets.is_empty() {
        return Ok(None);
    }
    if let Some(pgcn) = ctx.override_dispatch() {
        return Ok(Some(pgcn));
    }
    let mut routes = Vec::with_capacity(targets.len());
    for (index, link) in targets.iter().enumerate() {
        let number = u16::try_from(index.saturating_add(1))
            .map_err(|_| NavError::Invalid("too many overrides".to_string()))?;
        let (pgcn, slot) = push_hidden(ctx, HiddenKind::Override { number })?;
        let mut pre = vec![cmd::set_gprm(GPRM_OVERRIDE, 0)];
        pre.extend(link_command(ctx, link, false)?);
        set_hidden_pre(ctx, slot, pre)?;
        routes.push((number, pgcn));
    }
    let (pgcn, slot) = push_hidden(ctx, HiddenKind::OverrideDispatch)?;
    let mut pre: Vec<Cmd> = routes
        .iter()
        .map(|(number, target)| cmd::if_eq_link_pgcn(GPRM_OVERRIDE, *number, *target))
        .collect();
    pre.push(cmd::set_gprm(GPRM_OVERRIDE, 0));
    pre.extend(fallback_commands(ctx));
    set_hidden_pre(ctx, slot, pre)?;
    Ok(Some(pgcn))
}

/// Pushes the override check to a title post command list: with an override pending, call the dispatch.
fn push_override_check(post: &mut Vec<Cmd>, dispatch: u16) {
    post.push(cmd::set_gprm(GPRM_SCRATCH_ZERO, 0));
    post.push(cmd::if_ne_regs_call_ss_vmgm_pgcn(
        GPRM_OVERRIDE,
        GPRM_SCRATCH_ZERO,
        dispatch,
    ));
}

/// Pushes the playlist-check commands to a title post command list.
fn push_playlist_check(post: &mut Vec<Cmd>, title: u8, dispatch: u16) {
    post.push(cmd::set_gprm(GPRM_SCRATCH_ZERO, 0));
    post.push(cmd::set_gprm(GPRM_SCRATCH_TITLE, u16::from(title)));
    post.push(cmd::if_ne_regs_call_ss_vmgm_pgcn(
        GPRM_ACTIVE_PLAYLIST,
        GPRM_SCRATCH_ZERO,
        dispatch,
    ));
}

/// Pushes a hidden PGC placeholder and returns its PGCN and vector index.
fn push_hidden(ctx: &mut Context, kind: HiddenKind) -> Result<(u16, usize), NavError> {
    let pgcn = next_hidden_pgcn(ctx)?;
    let index = ctx.hidden.len();
    ctx.hidden.push(HiddenPgc {
        kind,
        pgcn,
        pre: Vec::new(),
    });

    Ok((pgcn, index))
}

/// Sets the pre commands of a previously pushed hidden PGC.
fn set_hidden_pre(ctx: &mut Context, index: usize, pre: Vec<Cmd>) -> Result<(), NavError> {
    let Some(hidden) = ctx.hidden.get_mut(index) else {
        return Err(NavError::Invalid("hidden PGC disappeared".to_string()));
    };

    hidden.pre = pre;
    Ok(())
}

/// Returns the next hidden VMGM PGCN.
fn next_hidden_pgcn(ctx: &Context) -> Result<u16, NavError> {
    let total = ctx
        .menu_count
        .checked_add(ctx.hidden.len())
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| NavError::Invalid("too many VMGM PGCs".to_string()))?;

    if total > MAX_VTGM_PGCN {
        return Err(NavError::Invalid("too many VMGM PGCs".to_string()));
    }

    u16::try_from(total).map_err(|_| NavError::Invalid("too many VMGM PGCs".to_string()))
}

/// Checks the final VMGM PGC count.
fn check_pgc_limit(ctx: &Context) -> Result<(), NavError> {
    let total = ctx
        .menu_count
        .checked_add(ctx.hidden.len())
        .ok_or_else(|| NavError::Invalid("too many VMGM PGCs".to_string()))?;

    if total > MAX_VTGM_PGCN {
        return Err(NavError::Invalid("too many VMGM PGCs".to_string()));
    }

    Ok(())
}

/// Resolves a menu id to its VMGM PGCN.
fn resolve_menu(ctx: &Context, id: MenuId) -> Result<u16, NavError> {
    ctx.menu_pgcn
        .get(&id)
        .copied()
        .ok_or_else(|| NavError::Invalid(format!("menu {id} does not exist")))
}

/// Resolves a timeline id to its title number.
fn resolve_timeline(ctx: &Context, id: TimelineId) -> Result<u8, NavError> {
    ctx.timeline_title
        .get(&id)
        .copied()
        .ok_or_else(|| NavError::Invalid(format!("timeline {id} does not exist")))
}

/// Resolves a slideshow id to its title number.
fn resolve_slideshow(ctx: &Context, id: u64) -> Result<u8, NavError> {
    ctx.slideshow_title
        .get(&id)
        .copied()
        .ok_or_else(|| NavError::Invalid(format!("slideshow {id} does not exist")))
}

/// Resolves a chapter playlist id to its title number.
fn resolve_chapter_playlist(ctx: &Context, id: u64) -> Result<u8, NavError> {
    ctx.chapter_playlist_title
        .get(&id)
        .copied()
        .ok_or_else(|| NavError::Invalid(format!("chapter playlist {id} does not exist")))
}

/// Resolves a playlist id to its one-based playlist number.
fn playlist_number(ctx: &Context, id: u64) -> Result<u8, NavError> {
    ctx.playlist_number
        .get(&id)
        .copied()
        .ok_or_else(|| NavError::Invalid(format!("playlist {id} does not exist")))
}

/// Returns the item count of a playlist as a one-based position limit.
fn playlist_item_count(ctx: &Context, id: u64) -> Result<u16, NavError> {
    let len = ctx
        .playlist_items
        .get(&id)
        .map(|items| items.len())
        .ok_or_else(|| NavError::Invalid(format!("playlist {id} does not exist")))?;

    u16::try_from(len).map_err(|_| NavError::Invalid("playlist too long".to_string()))
}

/// Returns the title number of a playlist item by zero-based index.
fn playlist_title_at(ctx: &Context, id: u64, index: u16) -> Result<u8, NavError> {
    let index = usize::from(index);

    ctx.playlist_items
        .get(&id)
        .and_then(|items| items.get(index).copied())
        .ok_or_else(|| NavError::Invalid(format!("playlist {id} item {index} does not exist")))
}

/// Validates a zero-based chapter index and returns the one-based chapter number.
fn validate_chapter(ctx: &Context, id: TimelineId, chapter: u32) -> Result<u16, NavError> {
    let count = ctx
        .chapter_counts
        .get(&id)
        .copied()
        .ok_or_else(|| NavError::Invalid(format!("timeline {id} does not exist")))?;

    let index = usize::try_from(chapter)
        .map_err(|_| NavError::Invalid("chapter index too large".to_string()))?;

    if index >= count {
        return Err(NavError::Invalid(format!(
            "chapter {chapter} does not exist in timeline {id}"
        )));
    }

    let number = chapter
        .checked_add(1)
        .ok_or_else(|| NavError::Invalid("chapter number overflow".to_string()))?;

    u16::try_from(number).map_err(|_| NavError::Invalid("chapter number too large".to_string()))
}

/// Converts a command vector into exactly one command.
fn single_command(mut commands: Vec<Cmd>) -> Result<Cmd, NavError> {
    if commands.len() != 1 {
        return Err(NavError::Invalid(
            "button command must be a single instruction".to_string(),
        ));
    }

    commands
        .pop()
        .ok_or_else(|| NavError::Invalid("button command missing".to_string()))
}

#[cfg(test)]
mod tests {
    /// The first lines of a menu's pre commands: highlight button `own`, unless a link asked for another (GPRM10).
    fn highlight_lines(own: u8) -> Vec<crate::Cmd> {
        vec![
            cmd::set_hl_btnn(own),
            cmd::if_eq_goto(10, 0, 4),
            cmd::set_hl_btnn_gprm(10),
            cmd::set_gprm(10, 0),
        ]
    }

    #[test]
    fn a_link_to_a_menus_button_asks_the_menu_to_highlight_it()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let first = project.add_menu(Some("First".to_string()))?;
        let second = project.add_menu(Some("Second".to_string()))?;
        project.add_button(second, "A".to_string(), None, Link::Stop)?;
        project.add_button(second, "B".to_string(), None, Link::Stop)?;
        project.add_button(second, "C".to_string(), None, Link::Stop)?;
        let to = |button: Option<u32>| Link::Menu { id: second, button };
        project.add_button(first, "To C".to_string(), None, to(Some(2)))?;
        project.add_button(first, "To second".to_string(), None, to(None))?;
        project.add_button(first, "To nothing".to_string(), None, to(Some(9)))?;
        project.menu_mut(second).ok_or("menu")?.default_button = 1;
        project.menu_mut(first).ok_or("menu")?.end_action = to(Some(0));

        let nav = compile(&project)?;
        let menu_of = |id: disccraft_project::MenuId| {
            nav.menus
                .iter()
                .find(|menu| menu.menu == id)
                .ok_or("menu nav")
        };
        let (first_nav, second_nav) = (menu_of(first)?, menu_of(second)?);
        // The second menu highlights its default (button 2) unless asked.
        assert!(second_nav.pre.starts_with(&highlight_lines(2)));
        // A button links through a hidden PGC that asks for button 3; a plain link and a button the menu does not
        // have go straight there.
        let plain = cmd::link_pgcn(second_nav.pgcn);
        assert_eq!(first_nav.buttons.get(1), Some(&plain));
        assert_eq!(first_nav.buttons.get(2), Some(&plain));
        let hidden = nav
            .hidden
            .iter()
            .find(|hidden| matches!(hidden.kind, HiddenKind::ButtonLink { button: 1, .. }))
            .ok_or("hidden PGC")?;
        assert_eq!(
            first_nav.buttons.first(),
            Some(&cmd::link_pgcn(hidden.pgcn))
        );
        assert_eq!(
            hidden.pre,
            vec![cmd::set_gprm(10, cmd::highlight_value(3)), plain]
        );
        // The end action asks for button 1 in its post commands.
        assert_eq!(
            first_nav.post,
            vec![cmd::set_gprm(10, cmd::highlight_value(1)), plain]
        );
        Ok(())
    }

    use super::{HiddenKind, NavError, compile, compile_title_pre};
    use crate::cmd;
    use disccraft_project::{Link, Project, Tick};

    #[test]
    fn compiles_simple_project() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();

        let asset = project.add_asset(std::path::Path::new("movie.mp4"))?;
        let timeline = project.add_timeline(asset, None)?;
        project.add_chapter(timeline, Tick(100), None)?;
        project.add_chapter(timeline, Tick(200), None)?;
        project
            .timeline_mut(timeline)
            .ok_or("missing timeline")?
            .end_action = Link::LastMenu;

        let menu = project.add_menu(Some("Main".to_string()))?;
        project.add_button(
            menu,
            "Chapter 1".to_string(),
            None,
            Link::Timeline {
                id: timeline,
                chapter: 0,
            },
        )?;
        project.add_button(
            menu,
            "Chapter 2".to_string(),
            None,
            Link::Timeline {
                id: timeline,
                chapter: 1,
            },
        )?;
        project.add_button(
            menu,
            "Chapter 3".to_string(),
            None,
            Link::Timeline {
                id: timeline,
                chapter: 2,
            },
        )?;
        project.settings.first_play = Link::Menu {
            id: menu,
            button: None,
        };

        let nav = compile(&project)?;

        assert_eq!(nav.first_play, vec![cmd::jump_ss_vmgm_pgcn(1)]);

        let menu_nav = nav.menus.first().ok_or("missing menu nav")?;
        assert_eq!(menu_nav.menu, menu);
        assert_eq!(menu_nav.pgcn, 1);
        assert!(menu_nav.title_menu);
        assert_eq!(
            menu_nav.pre,
            [
                highlight_lines(1),
                vec![cmd::set_gprm(3, 0), cmd::set_gprm(2, 1)]
            ]
            .concat()
        );
        assert_eq!(menu_nav.buttons.len(), 3);
        assert_eq!(
            menu_nav.buttons.first().copied().ok_or("missing button")?,
            cmd::jump_tt(1)
        );

        let chapter2 = find_hidden(&nav, |kind| {
            matches!(kind, HiddenKind::ChapterEntry { chapter: 2, .. })
        })?;
        let chapter3 = find_hidden(&nav, |kind| {
            matches!(kind, HiddenKind::ChapterEntry { chapter: 3, .. })
        })?;
        let last_menu = find_hidden(&nav, |kind| matches!(kind, HiddenKind::LastMenu))?;

        assert_eq!(
            menu_nav.buttons.get(1).copied().ok_or("missing button")?,
            cmd::link_pgcn(chapter2)
        );
        assert_eq!(
            menu_nav.buttons.get(2).copied().ok_or("missing button")?,
            cmd::link_pgcn(chapter3)
        );

        let title_nav = nav.titles.first().ok_or("missing title nav")?;
        assert_eq!(title_nav.timeline, timeline);
        assert_eq!(title_nav.title, 1);
        assert_eq!(
            title_nav.pre,
            vec![
                cmd::mov_gprm(1, 0),
                cmd::set_gprm(0, 0),
                cmd::if_eq_link_pgn(1, 2, 2),
                cmd::if_eq_link_pgn(1, 3, 3),
            ]
        );
        assert_eq!(title_nav.post, vec![cmd::call_ss_vmgm_pgcn(last_menu, 0)]);
        assert_eq!(nav.hidden.len(), 3);

        Ok(())
    }

    #[test]
    fn compiles_playlist_project() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();

        let asset1 = project.add_asset(std::path::Path::new("a.mp4"))?;
        let asset2 = project.add_asset(std::path::Path::new("b.mp4"))?;
        let asset3 = project.add_asset(std::path::Path::new("c.mp4"))?;
        let t1 = project.add_timeline(asset1, None)?;
        let t2 = project.add_timeline(asset2, None)?;
        let t3 = project.add_timeline(asset3, None)?;

        let menu = project.add_menu(Some("Main".to_string()))?;
        let playlist = project.add_playlist(None)?;
        project.add_playlist_item(playlist, t1)?;
        project.add_playlist_item(playlist, t3)?;
        project
            .playlist_mut(playlist)
            .ok_or("missing playlist")?
            .end_action = Link::Menu {
            id: menu,
            button: None,
        };

        project.add_button(
            menu,
            "Playlist".to_string(),
            None,
            Link::Playlist { id: playlist },
        )?;
        project.add_button(
            menu,
            "Title 2".to_string(),
            None,
            Link::Timeline { id: t2, chapter: 0 },
        )?;

        let nav = compile(&project)?;

        assert_eq!(nav.menus.len(), 1);
        assert_eq!(nav.titles.len(), 3);
        assert!(nav.slideshows.is_empty());

        let dispatch = find_hidden(&nav, |kind| matches!(kind, HiddenKind::PlaylistDispatch))?;
        let start = find_hidden(&nav, |kind| {
            matches!(kind, HiddenKind::PlaylistStart { .. })
        })?;
        let router = find_hidden(&nav, |kind| {
            matches!(kind, HiddenKind::PlaylistRouter { .. })
        })?;
        let step = find_hidden(&nav, |kind| {
            matches!(kind, HiddenKind::PlaylistStep { position: 1, .. })
        })?;
        let end = find_hidden(&nav, |kind| matches!(kind, HiddenKind::PlaylistEnd { .. }))?;

        assert_eq!(nav.hidden.len(), 5);

        let menu_nav = nav.menus.first().ok_or("missing menu nav")?;
        assert_eq!(
            menu_nav.pre,
            [
                highlight_lines(1),
                vec![cmd::set_gprm(3, 0), cmd::set_gprm(2, 1)]
            ]
            .concat()
        );
        assert_eq!(
            menu_nav.buttons.first().copied().ok_or("missing button")?,
            cmd::link_pgcn(start)
        );
        assert_eq!(
            menu_nav.buttons.get(1).copied().ok_or("missing button")?,
            cmd::jump_tt(2)
        );

        assert_eq!(
            hidden_pre(&nav, start)?,
            vec![
                cmd::set_gprm(3, 1),
                cmd::set_gprm(4, 1),
                cmd::set_gprm(0, 0),
                cmd::jump_tt(1),
            ]
        );
        assert_eq!(
            hidden_pre(&nav, router)?,
            vec![
                cmd::if_eq_link_pgcn(4, 1, step),
                cmd::if_eq_link_pgcn(4, 2, end),
                cmd::set_gprm(3, 0),
                cmd::link_pgcn(1),
            ]
        );
        assert_eq!(
            hidden_pre(&nav, step)?,
            vec![cmd::set_gprm(4, 2), cmd::set_gprm(0, 0), cmd::jump_tt(3),]
        );
        assert_eq!(
            hidden_pre(&nav, end)?,
            vec![cmd::set_gprm(3, 0), cmd::set_gprm(4, 0), cmd::link_pgcn(1),]
        );
        assert_eq!(
            hidden_pre(&nav, dispatch)?,
            vec![
                cmd::if_eq_link_pgcn(3, 1, router),
                cmd::set_gprm(3, 0),
                cmd::link_pgcn(1),
            ]
        );

        for title_nav in &nav.titles {
            assert_eq!(
                title_nav.post.first().copied().ok_or("missing post")?,
                cmd::set_gprm(5, 0)
            );
            assert_eq!(
                title_nav.post.get(1).copied().ok_or("missing post")?,
                cmd::set_gprm(6, u16::from(title_nav.title))
            );
            assert_eq!(
                title_nav.post.get(2).copied().ok_or("missing post")?,
                cmd::if_ne_regs_call_ss_vmgm_pgcn(3, 5, dispatch)
            );
        }

        Ok(())
    }

    #[test]
    fn compiles_slideshow_title() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();

        let video = project.add_asset(std::path::Path::new("movie.mp4"))?;
        let _timeline = project.add_timeline(video, None)?;
        let _image = project.add_asset(std::path::Path::new("slide.png"))?;
        let slideshow = project.add_slideshow(None)?;
        let menu = project.add_menu(None)?;

        project.add_button(
            menu,
            "Slide".to_string(),
            None,
            Link::Slideshow { id: slideshow },
        )?;

        let nav = compile(&project)?;

        assert_eq!(nav.titles.first().ok_or("missing title")?.title, 1);

        let slide_nav = nav.slideshows.first().ok_or("missing slideshow")?;
        assert_eq!(slide_nav.slideshow, slideshow);
        assert_eq!(slide_nav.title, 2);
        assert!(slide_nav.pre.is_empty());
        assert_eq!(slide_nav.post, vec![cmd::exit()]);

        assert_eq!(
            nav.menus
                .first()
                .ok_or("missing menu")?
                .buttons
                .first()
                .copied()
                .ok_or("missing button")?,
            cmd::jump_tt(2)
        );

        Ok(())
    }

    #[test]
    fn missing_chapter_playlist_is_invalid() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        project.add_button(
            menu,
            "Chapter playlist".to_string(),
            None,
            Link::ChapterPlaylist { id: 1 },
        )?;

        let err = compile(&project).err().ok_or("expected error")?;
        assert!(matches!(err, NavError::Invalid(_)));

        Ok(())
    }

    /// A chapter playlist is a title after the timelines and slideshows, VTS title 2 of its master timeline; a
    /// button jumps to it, its pre commands dispatch a requested chapter among its own entries, and its end
    /// action (here: the menu) follows the override check.
    #[test]
    fn chapter_playlist_becomes_a_title_in_the_master_title_set()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(std::path::Path::new("a.m2v"))?;
        let timeline = project.add_timeline(asset, None)?;
        let _ = project.add_chapter(
            timeline,
            disccraft_project::Tick(254_016_000_000 * 10),
            None,
        )?;
        let _ = project.add_chapter(
            timeline,
            disccraft_project::Tick(254_016_000_000 * 20),
            None,
        )?;
        let slideshow = project.add_slideshow(None)?;
        let menu = project.add_menu(None)?;
        let playlist = project.add_chapter_playlist(timeline, None)?;
        let _ = project.add_chapter_playlist_entry(playlist, 2, None)?;
        let _ = project.add_chapter_playlist_entry(playlist, 0, None)?;
        let _ = project.add_chapter_playlist_entry(playlist, 2, None)?;
        project.add_button(
            menu,
            "Highlights".to_string(),
            None,
            Link::ChapterPlaylist { id: playlist },
        )?;
        if let Some(list) = project.chapter_playlist_mut(playlist) {
            list.end_action = Link::Menu {
                id: menu,
                button: None,
            };
        }
        project.settings.first_play = Link::ChapterPlaylist { id: playlist };
        let _ = slideshow;

        let nav = compile(&project)?;
        let compiled = nav
            .chapter_playlists
            .first()
            .ok_or("missing chapter playlist")?;
        assert_eq!(compiled.chapter_playlist, playlist);
        assert_eq!(compiled.timeline, timeline);
        assert_eq!(compiled.title, 3);
        assert_eq!(compiled.vts_ttn, 2);
        assert_eq!(compiled.programs, vec![3, 1, 3]);
        assert_eq!(compiled.pre, compile_title_pre(3)?);
        assert_eq!(compiled.post, vec![cmd::call_ss_vmgm_pgcn(1, 0)]);
        assert_eq!(
            nav.menus
                .first()
                .ok_or("missing menu")?
                .buttons
                .first()
                .copied(),
            Some(cmd::jump_tt(3))
        );
        assert_eq!(nav.first_play, vec![cmd::jump_tt(3)]);

        // An empty chapter playlist cannot be built.
        if let Some(list) = project.chapter_playlist_mut(playlist) {
            list.chapters.clear();
        }
        assert!(compile(&project).is_err());
        Ok(())
    }

    #[test]
    fn missing_timeline_is_invalid() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        project.add_button(
            menu,
            "Missing".to_string(),
            None,
            Link::Timeline {
                id: disccraft_project::TimelineId(999),
                chapter: 0,
            },
        )?;

        let err = compile(&project).err().ok_or("expected error")?;
        assert!(matches!(err, NavError::Invalid(_)));

        Ok(())
    }

    #[test]
    fn menu_remote_jumps_from_the_root_menu() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(std::path::Path::new("movie.mp4"))?;
        let first = project.add_timeline(asset, None)?;
        let second = project.add_timeline(asset, None)?;
        let third = project.add_timeline(asset, None)?;
        let menu = project.add_menu(Some("Main".to_string()))?;
        project
            .timeline_mut(first)
            .ok_or("missing timeline")?
            .menu_remote = Link::Menu {
            id: menu,
            button: None,
        };
        project
            .timeline_mut(second)
            .ok_or("missing timeline")?
            .menu_remote = Link::Timeline {
            id: first,
            chapter: 0,
        };
        let _ = third;

        let nav = compile(&project)?;
        let remote = |index: usize| nav.titles.get(index).map(|title| title.menu_remote.clone());
        assert_eq!(remote(0), Some(vec![cmd::jump_ss_vmgm_pgcn(1)]));
        let hidden = find_hidden(&nav, |kind| {
            matches!(kind, HiddenKind::MenuRemote { title: 2 })
        })?;
        assert_eq!(remote(1), Some(vec![cmd::jump_ss_vmgm_pgcn(hidden)]));
        let pre = nav
            .hidden
            .iter()
            .find(|pgc| pgc.pgcn == hidden)
            .map(|pgc| pgc.pre.clone());
        assert_eq!(pre, Some(vec![cmd::jump_tt(1)]));
        // No menu remote: the MENU key does nothing, so there is no root menu.
        assert_eq!(remote(2), Some(Vec::new()));
        Ok(())
    }

    #[test]
    fn an_override_replaces_the_next_titles_end_action() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(std::path::Path::new("movie.mp4"))?;
        let warning = project.add_timeline(asset, None)?;
        let feature = project.add_timeline(asset, None)?;
        let main = project.add_menu(Some("Main".to_string()))?;
        let extras = project.add_menu(Some("Extras".to_string()))?;
        {
            let first = project.timeline_mut(warning).ok_or("missing timeline")?;
            first.end_action = Link::Timeline {
                id: feature,
                chapter: 0,
            };
            first.override_link = Some(Link::Menu {
                id: extras,
                button: None,
            });
        }
        project
            .timeline_mut(feature)
            .ok_or("missing timeline")?
            .end_action = Link::Menu {
            id: main,
            button: None,
        };

        let nav = compile(&project)?;
        let target = find_hidden(&nav, |kind| {
            matches!(kind, HiddenKind::Override { number: 1 })
        })?;
        let dispatch = find_hidden(&nav, |kind| matches!(kind, HiddenKind::OverrideDispatch))?;
        let pre_of = |pgcn: u16| {
            nav.hidden
                .iter()
                .find(|hidden| hidden.pgcn == pgcn)
                .map(|hidden| hidden.pre.clone())
                .unwrap_or_default()
        };
        // The override PGC clears GPRM7 and goes to the Extras menu (VMGM PGCN 2).
        assert_eq!(
            pre_of(target),
            vec![cmd::set_gprm(super::GPRM_OVERRIDE, 0), cmd::link_pgcn(2)]
        );
        assert_eq!(
            pre_of(dispatch).first().copied(),
            Some(cmd::if_eq_link_pgcn(super::GPRM_OVERRIDE, 1, target))
        );
        let post = |id| {
            nav.titles
                .iter()
                .find(|title| title.timeline == id)
                .map(|title| title.post.clone())
                .unwrap_or_default()
        };
        let check = [
            cmd::set_gprm(super::GPRM_SCRATCH_ZERO, 0),
            cmd::if_ne_regs_call_ss_vmgm_pgcn(
                super::GPRM_OVERRIDE,
                super::GPRM_SCRATCH_ZERO,
                dispatch,
            ),
        ];
        // The warning sets override 1 and then plays the feature.
        assert_eq!(
            post(warning),
            [
                check.to_vec(),
                vec![cmd::set_gprm(super::GPRM_OVERRIDE, 1), cmd::jump_tt(2)]
            ]
            .concat()
        );
        // The feature checks for a pending override before its own end action (the Main menu).
        assert_eq!(
            post(feature),
            [check.to_vec(), vec![cmd::call_ss_vmgm_pgcn(1, 0)]].concat()
        );
        // Menus clear a pending override.
        assert!(
            nav.menus
                .iter()
                .all(|menu| menu.pre.contains(&cmd::set_gprm(super::GPRM_OVERRIDE, 0)))
        );
        Ok(())
    }

    #[test]
    fn an_override_needs_an_end_action_that_plays_a_title() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut project = Project::default();
        let asset = project.add_asset(std::path::Path::new("movie.mp4"))?;
        let only = project.add_timeline(asset, None)?;
        let menu = project.add_menu(None)?;
        {
            let timeline = project.timeline_mut(only).ok_or("missing timeline")?;
            timeline.end_action = Link::Menu {
                id: menu,
                button: None,
            };
            timeline.override_link = Some(Link::Stop);
        }
        let nav = compile(&project)?;
        assert!(
            nav.hidden
                .iter()
                .all(|hidden| !matches!(hidden.kind, HiddenKind::Override { .. }))
        );
        assert!(
            nav.menus
                .iter()
                .all(|menu| !menu.pre.contains(&cmd::set_gprm(super::GPRM_OVERRIDE, 0)))
        );
        Ok(())
    }

    #[test]
    fn prohibited_operations_become_the_pgc_masks() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(std::path::Path::new("movie.mp4"))?;
        let warning = project.add_timeline(asset, None)?;
        let feature = project.add_timeline(asset, None)?;
        let menu = project.add_menu(None)?;
        project
            .timeline_mut(warning)
            .ok_or("missing timeline")?
            .prohibited_ops = vec![
            disccraft_project::UserOp::FastForward,
            disccraft_project::UserOp::NextChapter,
            disccraft_project::UserOp::RootMenu,
        ];
        project.menu_mut(menu).ok_or("missing menu")?.prohibited_ops =
            vec![disccraft_project::UserOp::TitleMenu];

        let nav = compile(&project)?;
        let mask = |id| {
            nav.titles
                .iter()
                .find(|title| title.timeline == id)
                .map(|title| title.prohibited_ops)
        };
        assert_eq!(mask(warning), Some((1 << 7) | (1 << 8) | (1 << 11)));
        assert_eq!(mask(feature), Some(0));
        assert_eq!(
            nav.menus.first().map(|menu| menu.prohibited_ops),
            Some(1 << 10)
        );

        // The disc's operations add to every title, menu and first play.
        project.settings.prohibited_ops = vec![disccraft_project::UserOp::Rewind];
        let nav = compile(&project)?;
        assert_eq!(nav.first_play_prohibited_ops, 1 << 9);
        assert_eq!(
            nav.titles
                .iter()
                .find(|title| title.timeline == feature)
                .map(|title| title.prohibited_ops),
            Some(1 << 9)
        );
        assert_eq!(
            nav.menus.first().map(|menu| menu.prohibited_ops),
            Some((1 << 10) | (1 << 9))
        );
        Ok(())
    }

    #[test]
    fn the_disc_selects_its_streams_before_first_play() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(std::path::Path::new("movie.mp4"))?;
        let _ = project.add_timeline(asset, None)?;
        project.settings.audio_default = Some(2);
        project.settings.subtitle_default = disccraft_project::SubtitleDefault::Stream(1);
        let nav = compile(&project)?;
        assert_eq!(
            nav.first_play,
            vec![cmd::set_stn(Some(1), Some((true, 0))), cmd::jump_tt(1)]
        );
        project.settings.audio_default = None;
        project.settings.subtitle_default = disccraft_project::SubtitleDefault::Off;
        let nav = compile(&project)?;
        assert_eq!(
            nav.first_play.first(),
            Some(&cmd::set_stn(None, Some((false, 0))))
        );
        project.settings.subtitle_default = disccraft_project::SubtitleDefault::NoChange;
        let nav = compile(&project)?;
        assert_eq!(nav.first_play, vec![cmd::jump_tt(1)]);
        Ok(())
    }

    /// A button that selects an audio track and turns subtitles off goes through a hidden PGC: `SetSTN`, then
    /// the jump. With an override as well, the override's register is set first.
    #[test]
    fn a_button_selects_its_streams_before_it_jumps() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(std::path::Path::new("movie.mp4"))?;
        let timeline = project.add_timeline(asset, None)?;
        let menu = project.add_menu(None)?;
        project.add_button(
            menu,
            "Commentary".to_string(),
            None,
            Link::Timeline {
                id: timeline,
                chapter: 0,
            },
        )?;
        {
            let button = project
                .menu_mut(menu)
                .and_then(|menu| menu.buttons.first_mut())
                .ok_or("button")?;
            button.audio = Some(2);
            button.subtitle = disccraft_project::SubtitleDefault::Off;
        }
        let nav = compile(&project)?;
        let hidden = nav
            .hidden
            .iter()
            .find(|pgc| matches!(pgc.kind, HiddenKind::ButtonStreams { menu: 1, button: 1 }))
            .ok_or("missing stream PGC")?;
        assert_eq!(
            hidden.pre,
            vec![cmd::set_stn(Some(1), Some((false, 0))), cmd::jump_tt(1)]
        );
        assert_eq!(
            nav.menus.first().and_then(|m| m.buttons.first()).copied(),
            Some(cmd::link_pgcn(hidden.pgcn))
        );

        // With an override too, the override register comes first and the PGC is the override kind.
        project
            .menu_mut(menu)
            .and_then(|menu| menu.buttons.first_mut())
            .ok_or("button")?
            .override_link = Some(Link::Stop);
        let nav = compile(&project)?;
        let hidden = nav
            .hidden
            .iter()
            .find(|pgc| matches!(pgc.kind, HiddenKind::ButtonOverride { menu: 1, button: 1 }))
            .ok_or("missing override PGC")?;
        assert_eq!(
            hidden.pre.get(1),
            Some(&cmd::set_stn(Some(1), Some((false, 0))))
        );
        Ok(())
    }

    #[test]
    fn the_disc_override_is_set_before_the_first_play() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(std::path::Path::new("movie.mp4"))?;
        let feature = project.add_timeline(asset, None)?;
        let main = project.add_menu(Some("Main".to_string()))?;
        project.settings.first_play = Link::Timeline {
            id: feature,
            chapter: 0,
        };
        project.settings.first_play_override = Some(Link::Menu {
            id: main,
            button: None,
        });
        project
            .timeline_mut(feature)
            .ok_or("missing timeline")?
            .end_action = Link::Stop;

        let nav = compile(&project)?;
        assert_eq!(
            nav.first_play,
            vec![cmd::set_gprm(super::GPRM_OVERRIDE, 1), cmd::jump_tt(1)]
        );
        let dispatch = find_hidden(&nav, |kind| matches!(kind, HiddenKind::OverrideDispatch))?;
        assert!(nav.titles.first().is_some_and(|title| {
            title.post.contains(&cmd::if_ne_regs_call_ss_vmgm_pgcn(
                super::GPRM_OVERRIDE,
                super::GPRM_SCRATCH_ZERO,
                dispatch,
            ))
        }));
        Ok(())
    }

    #[test]
    fn a_menu_override_is_set_before_its_end_action() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(std::path::Path::new("movie.mp4"))?;
        let feature = project.add_timeline(asset, None)?;
        let main = project.add_menu(Some("Main".to_string()))?;
        let extras = project.add_menu(Some("Extras".to_string()))?;
        {
            let menu = project.menu_mut(main).ok_or("missing menu")?;
            menu.duration_seconds = Some(10);
            menu.end_action = Link::Timeline {
                id: feature,
                chapter: 0,
            };
            menu.override_link = Some(Link::Menu {
                id: extras,
                button: None,
            });
        }
        project
            .timeline_mut(feature)
            .ok_or("missing timeline")?
            .end_action = Link::Menu {
            id: main,
            button: None,
        };

        let nav = compile(&project)?;
        // The menu's post sets the pending override, then plays the feature.
        assert_eq!(
            nav.menus.first().map(|menu| menu.post.clone()),
            Some(vec![
                cmd::set_gprm(super::GPRM_OVERRIDE, 1),
                cmd::jump_tt(1)
            ])
        );
        // The feature checks the pending override before its own end action.
        let dispatch = find_hidden(&nav, |kind| matches!(kind, HiddenKind::OverrideDispatch))?;
        assert!(nav.titles.first().is_some_and(|title| {
            title.post.contains(&cmd::if_ne_regs_call_ss_vmgm_pgcn(
                super::GPRM_OVERRIDE,
                super::GPRM_SCRATCH_ZERO,
                dispatch,
            ))
        }));
        Ok(())
    }

    #[test]
    fn a_button_override_goes_through_a_hidden_pgc() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(std::path::Path::new("movie.mp4"))?;
        let feature = project.add_timeline(asset, None)?;
        let main = project.add_menu(Some("Main".to_string()))?;
        let extras = project.add_menu(Some("Extras".to_string()))?;
        project.add_button(
            main,
            "Play".to_string(),
            None,
            Link::Timeline {
                id: feature,
                chapter: 0,
            },
        )?;
        project
            .menu_mut(main)
            .and_then(|menu| menu.buttons.first_mut())
            .ok_or("missing button")?
            .override_link = Some(Link::Menu {
            id: extras,
            button: None,
        });
        project
            .timeline_mut(feature)
            .ok_or("missing timeline")?
            .end_action = Link::Menu {
            id: main,
            button: None,
        };

        let nav = compile(&project)?;
        let hidden = find_hidden(&nav, |kind| {
            matches!(kind, HiddenKind::ButtonOverride { menu: 1, button: 1 })
        })?;
        assert_eq!(
            nav.menus
                .first()
                .and_then(|menu| menu.buttons.first().copied()),
            Some(cmd::link_pgcn(hidden))
        );
        let pre = nav
            .hidden
            .iter()
            .find(|pgc| pgc.pgcn == hidden)
            .map(|pgc| pgc.pre.clone())
            .unwrap_or_default();
        assert_eq!(
            pre,
            vec![cmd::set_gprm(super::GPRM_OVERRIDE, 1), cmd::jump_tt(1)]
        );
        // The feature checks the pending override before its own end action.
        let dispatch = find_hidden(&nav, |kind| matches!(kind, HiddenKind::OverrideDispatch))?;
        assert!(nav.titles.first().is_some_and(|title| {
            title.post.contains(&cmd::if_ne_regs_call_ss_vmgm_pgcn(
                super::GPRM_OVERRIDE,
                super::GPRM_SCRATCH_ZERO,
                dispatch,
            ))
        }));
        Ok(())
    }

    fn find_hidden(
        nav: &super::CompiledNav,
        predicate: impl Fn(&HiddenKind) -> bool,
    ) -> Result<u16, Box<dyn std::error::Error>> {
        for hidden in &nav.hidden {
            if predicate(&hidden.kind) {
                return Ok(hidden.pgcn);
            }
        }

        Err("missing hidden PGC".into())
    }

    fn hidden_pre(
        nav: &super::CompiledNav,
        pgcn: u16,
    ) -> Result<Vec<cmd::Cmd>, Box<dyn std::error::Error>> {
        for hidden in &nav.hidden {
            if hidden.pgcn == pgcn {
                return Ok(hidden.pre.clone());
            }
        }

        Err("missing hidden PGC".into())
    }
}
