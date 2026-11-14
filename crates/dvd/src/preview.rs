//! The navigation model of the disc a project builds, without building it (Encore's Project Preview): the same
//! first play, menu and hidden PGC commands, button routes and titles the DVD build writes, made from the compiled
//! navigation and the menus' button routes. No video is transcoded or muxed, so the preview opens at once; the
//! model equals the one [`disccraft_vm::load_video_ts`] reads back from the built `VIDEO_TS` (checked in the
//! tests), so the preview behaves as the disc will.
//!
//! A Blu-ray project previews through the same model: its menus, buttons, links, end actions and titles behave
//! the same on both discs (the Blu-ray build writes them as HDMV movie objects and interactive graphics instead of
//! DVD program chains). What only a Blu-ray does, a pop-up menu over the playing title, the preview adds on top.

use disccraft_project::Project;
use disccraft_vm::{ButtonModel, DiscModel, MenuPgcModel, TitleModel};

use disccraft_nav::compile;

use crate::{DvdError, slideshow_ids, validate_contents};

/// The disc model of `project` as the DVD build would write it.
///
/// # Errors
///
/// The errors of a build before any media is read: a project that fails its check, navigation that does not
/// compile, a slideshow without slides. A Blu-ray project gives the model of the same disc as a DVD.
pub fn preview_model(project: &Project) -> Result<DiscModel, DvdError> {
    let (title_count, _) = validate_contents(project)?;
    let nav = compile(project)?;
    let invalid = |message: &str| DvdError::Invalid(message.to_string());
    if project.menus.len() != nav.menus.len() {
        return Err(invalid("menu count mismatch"));
    }

    // VMGM: the menus in `MenuId` order, then the hidden PGCs (the order `build_menus` writes them in).
    let mut vmgm = Vec::with_capacity(nav.menus.len().saturating_add(nav.hidden.len()));
    for (menu, nav_menu) in project.menus.values().zip(&nav.menus) {
        let routes = disccraft_menu::button_routes(menu, &project.settings.routing);
        if routes.len() != nav_menu.buttons.len() {
            return Err(invalid("button command count mismatch"));
        }
        let buttons = nav_menu
            .buttons
            .iter()
            .zip(routes)
            .enumerate()
            .map(|(index, (command, (up, down, left, right)))| {
                // A route to nowhere stays on the button (as `hli_button` writes it).
                let own = u8::try_from(index.saturating_add(1)).unwrap_or(1);
                let stay = |target: u8| if target == 0 { own } else { target };
                ButtonModel {
                    command: *command,
                    up: stay(up),
                    down: stay(down),
                    left: stay(left),
                    right: stay(right),
                    auto_action: menu
                        .buttons
                        .get(index)
                        .is_some_and(|button| button.auto_activate),
                }
            })
            .collect();
        let (pre, post) = menu_commands(menu, nav_menu);
        vmgm.push(MenuPgcModel {
            pre,
            post,
            has_cells: true,
            buttons,
            default_button: 0,
            prohibited_ops: nav_menu.prohibited_ops | crate::UOP_TIME_PLAY,
        });
    }
    for hidden in &nav.hidden {
        vmgm.push(MenuPgcModel {
            pre: hidden.pre.clone(),
            post: Vec::new(),
            has_cells: false,
            buttons: Vec::new(),
            default_button: 0,
            prohibited_ops: crate::UOP_TIME_PLAY,
        });
    }
    let title_menu = nav
        .menus
        .iter()
        .position(|menu| menu.title_menu)
        .and_then(|index| u16::try_from(index.checked_add(1)?).ok());

    // Titles by number: each timeline, its chapter playlists (in its title set, sharing its root menu) and each
    // slideshow.
    // Timelines, slideshows and chapter playlists.
    let mut titles: Vec<Option<TitleModel>> = vec![None; title_count];
    let mut put = |title: u8, model: TitleModel| -> Result<(), DvdError> {
        let slot = usize::from(title)
            .checked_sub(1)
            .and_then(|index| titles.get_mut(index))
            .ok_or_else(|| invalid("title number out of range"))?;
        *slot = Some(model);
        Ok(())
    };
    for timeline in project.timelines.values() {
        let title_nav = nav
            .titles
            .iter()
            .find(|title| title.timeline == timeline.id)
            .ok_or_else(|| invalid("missing compiled title"))?;
        let chapters = u16::try_from(timeline.chapters.len())
            .map_err(|_| DvdError::Unsupported("too many chapters".to_string()))?;
        put(
            title_nav.title,
            TitleModel {
                pre: title_nav.pre.clone(),
                post: title_nav.post.clone(),
                chapters,
                menu_remote: title_nav.menu_remote.clone(),
                prohibited_ops: title_nav.prohibited_ops,
            },
        )?;
        for extra in nav
            .chapter_playlists
            .iter()
            .filter(|extra| extra.timeline == timeline.id)
        {
            put(
                extra.title,
                TitleModel {
                    pre: extra.pre.clone(),
                    post: extra.post.clone(),
                    chapters: u16::try_from(extra.programs.len())
                        .map_err(|_| DvdError::Unsupported("too many programs".to_string()))?,
                    menu_remote: title_nav.menu_remote.clone(),
                    prohibited_ops: extra.prohibited_ops,
                },
            )?;
        }
    }
    for id in slideshow_ids(project) {
        let slideshow = project
            .slideshows
            .get(&id)
            .ok_or_else(|| invalid("slideshow does not exist"))?;
        let entry = nav
            .slideshows
            .iter()
            .find(|entry| entry.slideshow == id)
            .ok_or_else(|| invalid("missing compiled slideshow"))?;
        if slideshow.slides.is_empty() {
            return Err(invalid("slideshow has no slides"));
        }
        put(
            entry.title,
            TitleModel {
                pre: entry.pre.clone(),
                post: entry.post.clone(),
                chapters: u16::try_from(slideshow.slides.len())
                    .map_err(|_| DvdError::Unsupported("too many slides".to_string()))?,
                // Slideshows have no menu remote.
                menu_remote: Vec::new(),
                prohibited_ops: 0,
            },
        )?;
    }
    let titles = titles
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| invalid("title numbers are not contiguous"))?;

    Ok(DiscModel {
        title_menu,
        first_play: nav.first_play.clone(),
        vmgm,
        titles,
    })
}

/// A menu PGC's pre and post commands, as `build_menus` writes them: a still menu's own; a moving menu (video
/// background, menu audio or motion buttons) also loops: back to itself (to program 2 from a loop point) forever,
/// once (then its end action, or holding the last picture), or so many times (a counter, then the end action).
fn menu_commands(
    menu: &disccraft_project::Menu,
    nav_menu: &disccraft_nav::MenuNav,
) -> (Vec<disccraft_vm::Cmd>, Vec<disccraft_vm::Cmd>) {
    if !menu.is_motion() {
        return (nav_menu.pre.clone(), nav_menu.post.clone());
    }
    let loop_program = if menu.effective_loop_point().is_some() {
        2
    } else {
        1
    };
    let mut pre = nav_menu.pre.clone();
    let post = match (menu.effective_loops(), nav_menu.post.is_empty()) {
        (disccraft_project::MenuLoops::Once, true) => Vec::new(),
        (disccraft_project::MenuLoops::Once, false) => nav_menu.post.clone(),
        (disccraft_project::MenuLoops::Times(times), false) => {
            let (loop_pre, loop_post) =
                disccraft_nav::counted_loop_to_program(times, loop_program, &nav_menu.post);
            pre.extend(loop_pre);
            loop_post
        }
        _ => vec![disccraft_nav::link_pgn(loop_program, 0)],
    };
    (pre, post)
}
