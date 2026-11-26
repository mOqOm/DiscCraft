//! A Blu-ray's menu as a DVD menu (M6.430): the menu a Blu-ray shows, made a motion menu of the project with a button
//! for each of its buttons that plays something, linked to the timelines of what they play.
//!
//! The disc's own navigation is followed in the HDMV interpreter ([`disccraft_vm::BdVm`]): from its first playback,
//! past what it plays first (warnings, logos), to the first menu it shows. Each button of that menu's page is then
//! pressed in a copy of that state, and where the disc goes is the button's target: a playlist at a mark becomes a
//! link to the chapter of a timeline of that playlist (one the project has, or one imported for it, as
//! `timeline.importBluray` makes them); a button that only changes the menu (a subtitle or commentary switch, another
//! page) has nothing to link on a DVD and is left out. The buttons keep their places, scaled from the menu's picture
//! (1920 x 1080) to the DVD frame, and their highlights are the shapes of their selected pictures (the IG objects'
//! opaque pixels). The background is the first 30 seconds (a DVD motion menu's loop at most) of the menu playlist's
//! video, with its sound. The menu is a draft: a DVD menu of the same buttons, to be arranged further by hand or with
//! an AI agent (MCP).

use std::collections::HashMap;
use std::path::Path;

use super::bluray::{
    Layout, answer_subtitles, clip_file, disc_title, import_playlist, layout, mark_frame,
    open_disc, outline_of,
};
use super::{always_enabled, object};
use crate::params::{opt_bool, req_str};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_bdmv::outline::Outline;
use disccraft_pgs::ig::{IgButton, IgObject};
use disccraft_project::{Link, MenuBackground, MenuId, MenuLoops, Rect, Subpicture, TimelineId};
use disccraft_vm::{BdLocation, BdVm, Key};
use serde_json::Value;

/// The most playlists a disc plays (warnings, logos) before its first menu is looked for.
const MAX_BEFORE_MENU: usize = 32;
/// A DVD motion menu's longest loop, seconds.
const MOTION_SECONDS: u32 = 30;
/// The alpha from which a highlight picture's pixel counts as part of its shape.
const OPAQUE: u8 = 128;
/// The opacity of a selected button's highlight, per cent.
const HIGHLIGHT_OPACITY: u8 = 50;
/// The opacity of an activated button's highlight, per cent.
const ACTIVATED_OPACITY: u8 = 70;

pub(crate) const COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "menu.importBluray",
    label: "Import Blu-ray Menu",
    menu: &["File", "Import"],
    shortcut: None,
    params: r#"{"path": string, "subtitlesLater"?: bool (true: the subtitles are not taken out but returned as subtitleJobs)} → {"menu", "buttons", "timelines": [id], "leftOut": [string], "assets": [id], "subtitleTracks", "subtitlesLeftOut": [string], "subtitleJobs": [job]}"#,
    enabled: always_enabled,
    run: run_import_menu,
    journal: true,
}];

/// The DVD subpicture of a button's highlight picture `object` scaled to `width` x `height`: a pixel is the
/// highlight (colour 1) where any opaque pixel of the picture falls in it, so thin strokes stay.
fn highlight(
    object: &IgObject,
    alpha: &[u8; 256],
    (width, height): (u16, u16),
) -> Option<Subpicture> {
    let (source_width, source_height) = (usize::from(object.width), usize::from(object.height));
    let (width, height) = (usize::from(width), usize::from(height));
    if source_width == 0 || source_height == 0 || width == 0 || height == 0 {
        return None;
    }
    let opaque = |x: usize, y: usize| {
        object
            .pixels
            .get(y * source_width + x)
            .is_some_and(|index| alpha[usize::from(*index)] >= OPAQUE)
    };
    let mut values = Vec::with_capacity(width * height);
    for y in 0..height {
        let (top, bottom) = (
            y * source_height / height,
            ((y + 1) * source_height).div_ceil(height),
        );
        for x in 0..width {
            let (left, right) = (
                x * source_width / width,
                ((x + 1) * source_width).div_ceil(width),
            );
            let any = (top..bottom.min(source_height))
                .any(|sy| (left..right.min(source_width)).any(|sx| opaque(sx, sy)));
            values.push(u8::from(any));
        }
    }
    Subpicture::from_values(
        u16::try_from(width).ok()?,
        u16::try_from(height).ok()?,
        &values,
    )
}

/// `value` of a picture `from` wide (or high) on a frame `to` wide.
fn scaled(value: u16, from: u16, to: u16) -> u16 {
    let scaled =
        (u32::from(value) * u32::from(to) + u32::from(from.max(1)) / 2) / u32::from(from.max(1));
    u16::try_from(scaled).unwrap_or(u16::MAX)
}

/// The chapter of `timeline` where `frame` is: the last that starts at or before it.
fn chapter_at(session: &Session, timeline: TimelineId, frame: u32) -> u32 {
    let tick = session
        .project
        .settings
        .frame_rate()
        .tick_of(i64::from(frame));
    session
        .project
        .timeline(timeline)
        .and_then(|found| {
            found
                .chapters
                .iter()
                .rposition(|chapter| chapter.time <= tick)
        })
        .and_then(|index| u32::try_from(index).ok())
        .unwrap_or(0)
}

/// A target playlist laid out as its timeline.
struct Target {
    timeline: TimelineId,
    outline: Outline,
    layout: Layout,
}

/// The DVD route from IG button `from` towards `step` (its neighbour in one direction), past the buttons left out:
/// the one-based number of the DVD button it reaches, 0 for none.
fn route(
    buttons: &HashMap<u16, &IgButton>,
    kept: &HashMap<u16, usize>,
    from: u16,
    step: impl Fn(&IgButton) -> u16,
) -> u8 {
    let mut at = from;
    for _ in 0..buttons.len() {
        let Some(button) = buttons.get(&at) else {
            return 0;
        };
        let next = step(button);
        if next == at || next == from {
            return 0;
        }
        if let Some(index) = kept.get(&next) {
            return u8::try_from(index + 1).unwrap_or(0);
        }
        at = next;
    }
    0
}

/// `menu.importBluray`: the first menu the disc at `path` shows, as a DVD menu of the project.
fn run_import_menu(session: &mut Session, params: &Value) -> CommandResult {
    let path = req_str(params, "path")?;
    let later = opt_bool(params, "subtitlesLater")?.unwrap_or(false);
    let disc = open_disc(&path)?;
    let navigation = disccraft_vm::load_bdmv(&disc.bdmv)
        .map_err(|error| format!("the disc's navigation cannot be read: {error}"))?;
    let follow =
        |error: disccraft_vm::VmError| format!("the disc's navigation cannot be followed: {error}");
    let mut vm = BdVm::start(&navigation).map_err(follow)?;
    for _ in 0..MAX_BEFORE_MENU {
        if vm.menu().is_some() || vm.state().location == BdLocation::Stopped {
            break;
        }
        vm.finish().map_err(follow)?;
    }
    let Some((menu, view)) = vm.menu() else {
        return Err("the disc shows no menu".to_string());
    };
    let BdLocation::Playing {
        playlist: menu_playlist,
        ..
    } = vm.state().location
    else {
        return Err("the disc shows no menu".to_string());
    };
    let page = menu
        .pages
        .iter()
        .find(|page| page.id == view.page)
        .ok_or_else(|| "the disc's menu has no page to show".to_string())?;

    // Where each button goes: pressed in a copy of the menu's state.
    let shown = vm.state().clone();
    let mut pressed: Vec<(&IgButton, Option<(u32, u32)>)> = Vec::new();
    for button in &page.buttons {
        let mut state = shown.clone();
        if let Some(menu_view) = state.menu.as_mut() {
            menu_view.button = button.id;
        }
        let mut probe = BdVm::restore(&navigation, state);
        let target = probe
            .press(Key::Enter)
            .ok()
            .and_then(|()| match probe.state().location {
                BdLocation::Playing { playlist, mark } if playlist != menu_playlist => {
                    Some((playlist, mark))
                }
                _ => None,
            });
        pressed.push((button, target));
    }

    // The timelines of what the buttons play: the project's, or imported.
    let rate = session.project.settings.frame_rate();
    let mut targets: HashMap<u32, Target> = HashMap::new();
    let mut assets: Vec<u64> = Vec::new();
    let mut left_out: Vec<String> = Vec::new();
    // M6.491: the subtitles of the timelines imported.
    let mut subtitle_jobs = Vec::new();
    let mut subtitles_left_out: Vec<String> = Vec::new();
    for (_, target) in &pressed {
        let Some((playlist, _)) = target else {
            continue;
        };
        if targets.contains_key(playlist) {
            continue;
        }
        match import_playlist(session, &disc, *playlist, None) {
            Ok(imported) => {
                assets.extend(imported.assets.iter().copied());
                subtitle_jobs.extend(imported.subtitle_jobs);
                subtitles_left_out.extend(imported.subtitles_left_out);
                let outline = outline_of(&disc, *playlist)?;
                let layout = layout(rate, &disc, &outline, false)?;
                let _ = targets.insert(
                    *playlist,
                    Target {
                        timeline: imported.timeline,
                        outline,
                        layout,
                    },
                );
            }
            Err(error) => left_out.push(format!("playlist {playlist:05}: {error}")),
        }
    }

    // The background: the first seconds of the menu playlist's video, with its sound.
    let menu_outline = outline_of(&disc, menu_playlist)?;
    let first = menu_outline
        .items
        .first()
        .ok_or_else(|| "the menu plays nothing".to_string())?;
    let background_file = clip_file(&disc.bdmv, &first.clip);
    let background = match session
        .project
        .assets
        .values()
        .find(|asset| Path::new(&asset.path) == background_file)
    {
        Some(asset) => asset.id,
        None => {
            let asset = session
                .project
                .add_asset(&background_file)
                .map_err(|error| error.to_string())?;
            assets.push(asset.0);
            asset
        }
    };
    // `as` saturates; a menu's loop is minutes long at most.
    let seconds = ((f64::from(first.length()) / 45_000.0).floor() as u32).clamp(1, MOTION_SECONDS);
    let has_sound = first
        .audio
        .iter()
        .any(|stream| matches!(stream.coding, 0x80 | 0x81));

    let name = disc_title(&disc.bdmv).unwrap_or_else(|| "Blu-ray".to_string());
    let menu_id: MenuId = session
        .project
        .add_menu(Some(name))
        .map_err(|error| error.to_string())?;
    let frame = disccraft_project::frame_size(&session.project.settings);
    let (source_width, source_height) = menu.size;
    let mut alpha = [0u8; 256];
    if let Some((_, entries)) = menu
        .palettes
        .iter()
        .find(|(id, _)| *id == page.palette_id)
        .or_else(|| menu.palettes.first())
    {
        for entry in entries {
            alpha[usize::from(entry.id)] = entry.alpha;
        }
    }

    // The buttons that play something, at their places, with their highlights.
    let mut kept: HashMap<u16, usize> = HashMap::new();
    for (button, target) in &pressed {
        let Some((playlist, mark)) = target else {
            left_out.push(format!("button {}", button.id));
            continue;
        };
        let Some(target) = targets.get(playlist) else {
            continue;
        };
        let frame_of_mark = usize::try_from(*mark)
            .ok()
            .and_then(|mark| target.outline.marks.get(mark))
            .and_then(|mark| mark_frame(rate, &target.outline, &target.layout, mark))
            .unwrap_or(0);
        let chapter = chapter_at(session, target.timeline, frame_of_mark);
        let picture = menu
            .objects
            .iter()
            .find(|object| object.id == button.selected_object)
            .or_else(|| {
                menu.objects
                    .iter()
                    .find(|object| object.id == button.normal_object)
            });
        let (width, height) = picture.map_or((64, 32), |object| (object.width, object.height));
        let rect = Rect {
            x: scaled(button.x, source_width, frame.0),
            y: scaled(button.y, source_height, frame.1),
            w: scaled(width, source_width, frame.0).max(2),
            h: scaled(height, source_height, frame.1).max(2),
        };
        let link = Link::Timeline {
            id: target.timeline,
            chapter,
        };
        // Named after what it plays: a chapter's name (one without a name of its own as "Chapter N" in the display
        // language, M6.433), or the title's for its start (ALL PLAY, an extra title).
        let chapter_name = disccraft_project::Menu::link_target_name(&session.project, &link);
        let index = session
            .project
            .add_button(menu_id, String::new(), Some(rect), link)
            .map_err(|error| error.to_string())?;
        let subpicture = picture.and_then(|object| highlight(object, &alpha, (rect.w, rect.h)));
        if let Some(found) = session
            .project
            .menu_mut(menu_id)
            .and_then(|menu| menu.buttons.get_mut(index))
        {
            if let Some(name) = chapter_name.filter(|name| !name.is_empty()) {
                found.name = name;
            }
            found.subpicture = subpicture;
        }
        let _ = kept.insert(button.id, index);
    }
    if kept.is_empty() {
        return Err("no button of the disc's menu plays anything a DVD can carry".to_string());
    }

    // The routes the disc gives its buttons, past the ones left out.
    let by_id: HashMap<u16, &IgButton> = page
        .buttons
        .iter()
        .map(|button| (button.id, button))
        .collect();
    let routes: Vec<(usize, [u8; 4])> = kept
        .iter()
        .map(|(id, index)| {
            (
                *index,
                [
                    route(&by_id, &kept, *id, |button| button.upper),
                    route(&by_id, &kept, *id, |button| button.lower),
                    route(&by_id, &kept, *id, |button| button.left),
                    route(&by_id, &kept, *id, |button| button.right),
                ],
            )
        })
        .collect();
    let default_button = kept
        .get(&page.default_selected)
        .or_else(|| kept.get(&view.button))
        .copied()
        .unwrap_or(0);
    if let Some(found) = session.project.menu_mut(menu_id) {
        for (index, route) in routes {
            if let Some(button) = found.buttons.get_mut(index) {
                button.routes = Some(route);
            }
        }
        found.auto_route = false;
        found.default_button = u32::try_from(default_button).unwrap_or(0);
        found.background = MenuBackground::Video {
            asset: background,
            seconds,
            layers: None,
        };
        found.audio = has_sound.then_some(background);
        found.widescreen = u32::from(source_width) * 9 >= u32::from(source_height) * 16;
        found.loops = Some(MenuLoops::Forever);
        // At a DVD's size a button's lettering merges into one shape: half see-through, the words under the
        // highlight stay readable (as a highlighter pen marks them).
        let mut colors = found.color_set_or_default();
        if let Some(group) = colors.groups.first_mut() {
            group.selected_opacity = HIGHLIGHT_OPACITY;
            group.activated_opacity = ACTIVATED_OPACITY;
        }
        found.color_set = Some(colors);
    }

    // The disc starts with the menu, and its titles go back to it, unless the project says otherwise.
    let menu_link = Link::Menu {
        id: menu_id,
        button: None,
    };
    let imported_timelines: Vec<TimelineId> =
        targets.values().map(|target| target.timeline).collect();
    let first_play = &session.project.settings.first_play;
    let first_is_ours = matches!(first_play, Link::NotSet)
        || matches!(first_play, Link::Timeline { id, .. } if imported_timelines.contains(id));
    if first_is_ours {
        session.project.settings.first_play = menu_link.clone();
    }
    if session.project.settings.title_remote == Link::NotSet {
        session.project.settings.title_remote = menu_link.clone();
    }
    for timeline in &imported_timelines {
        if let Some(found) = session.project.timeline_mut(*timeline)
            && found.end_action == Link::NotSet
        {
            found.end_action = menu_link.clone();
        }
    }

    let mut timelines: Vec<u64> = imported_timelines
        .iter()
        .map(|timeline| timeline.0)
        .collect();
    timelines.sort_unstable();
    assets.sort_unstable();
    assets.dedup();
    let (subtitle_tracks, subtitles_left_out, subtitle_jobs) =
        answer_subtitles(session, later, &subtitle_jobs, subtitles_left_out);
    Ok(object(&[
        ("menu", Value::from(menu_id.0)),
        ("buttons", Value::from(kept.len())),
        ("timelines", Value::from(timelines)),
        ("leftOut", Value::from(left_out)),
        ("assets", Value::from(assets)),
        ("background", Value::from(background.0)),
        ("seconds", Value::from(seconds)),
        ("subtitleTracks", subtitle_tracks),
        ("subtitlesLeftOut", subtitles_left_out),
        ("subtitleJobs", subtitle_jobs),
    ]))
}

#[cfg(test)]
mod tests {
    use super::{highlight, scaled};
    use disccraft_pgs::ig::IgObject;

    #[test]
    fn a_highlight_keeps_thin_strokes_when_it_is_made_smaller() {
        // A one-pixel line across a 40 x 8 picture of index 1 (opaque) on index 0 (clear).
        let mut pixels = vec![0u8; 40 * 8];
        for x in 0..40 {
            pixels[3 * 40 + x] = 1;
        }
        let object = IgObject {
            id: 0,
            width: 40,
            height: 8,
            pixels,
        };
        let mut alpha = [0u8; 256];
        alpha[1] = 255;
        let sub = highlight(&object, &alpha, (15, 4)).unwrap();
        assert_eq!((sub.width, sub.height), (15, 4));
        // All clear: no highlight.
        assert!(highlight(&object, &[0u8; 256], (15, 4)).is_none());
        assert_eq!(scaled(1920, 1920, 720), 720);
        assert_eq!(scaled(522, 1920, 720), 196);
        assert_eq!(scaled(1080, 1080, 480), 480);
    }
}
