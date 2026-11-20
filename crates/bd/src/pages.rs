//! A menu's pages built as one interactive composition (M10.26), as Encore builds them (docs/encore-behavior.md
//! 9.5g; GitHub issue #13): the menu's clip plays its background, and its IG has a page for each of the menu's
//! pages after a page 0 whose one auto-action button turns to the page the movie object asks for. A button that
//! links to another page of the same menu only turns the page, so the background video and the menu's sound play
//! on. What each page draws over the background (its texts, pictures, the buttons' texts and thumbnails) is a
//! picture of its own on its IG page (Encore leaves out a page's pictures that are not buttons; DiscCraft keeps
//! them).
//!
//! A menu's pages are built so when they share the background: the same picture or video, sound, length and loops
//! (Encore's pages are one menu; DiscCraft's pages could differ). Otherwise, and for a pop-up menu, more than
//! [`MAX_PAGES`] pages, or pictures too large for the player's interactive graphics, each page is a menu of its own
//! as before (and turning a page plays its own clip).

use disccraft_project::{Menu, MenuId, Project, Rect};

use crate::nav::MenuPages;

/// The most pages a menu's composition has (each names a palette of its own; with page 0, at most 9 palettes).
pub(crate) const MAX_PAGES: usize = 8;
/// The most buttons a page of a composition has ([`crate::nav::page_value`] keeps the button's index + 1 in a byte).
const MAX_BUTTONS: usize = 254;
/// The most bytes of decoded objects a menu's composition takes: the pages' pictures and every button's states, all
/// decoded before the menu's first picture (M10.28: 64 Mbit/s, so 2 MiB in about 0.26 s of the clip's 0.3 s before
/// it).
const OBJECT_BUDGET: u64 = 2 << 20;

/// Which of the project's menus (by their index in the project's order) are built as pages of one composition:
/// each group's menus, its first page first. Empty for a DVD. The preview reads the playing page this way too.
#[must_use]
pub fn menu_page_groups(project: &Project) -> Vec<Vec<usize>> {
    if project.settings.kind != disccraft_project::DiscKind::BluRay {
        return Vec::new();
    }
    let ids: Vec<MenuId> = project.menus.keys().copied().collect();
    let index_of = |id: MenuId| ids.iter().position(|known| *known == id);
    let mut groups = Vec::new();
    for (index, menu) in project.menus.values().enumerate() {
        if project.is_menu_page(menu.id) {
            continue;
        }
        let pages = project.menu_pages(menu.id);
        if pages.len() < 2 {
            continue;
        }
        let menus: Vec<&Menu> = pages.iter().filter_map(|id| project.menu(*id)).collect();
        let indices: Vec<usize> = pages.iter().filter_map(|id| index_of(*id)).collect();
        if menus.len() != pages.len()
            || indices.len() != pages.len()
            || indices.first() != Some(&index)
            || !shares_background(project, &menus)
        {
            continue;
        }
        groups.push(indices);
    }
    groups
}

/// Each menu's place among the pages of a composition (by the menus' index), for the navigation.
pub(crate) fn menu_pages(project: &Project) -> Vec<Option<MenuPages>> {
    let mut places = vec![None; project.menus.len()];
    let menus: Vec<&Menu> = project.menus.values().collect();
    for group in menu_page_groups(project) {
        let Some(&first) = group.first() else {
            continue;
        };
        let buttons: Vec<usize> = group
            .iter()
            .map(|index| menus.get(*index).map_or(0, |menu| menu.buttons.len()))
            .collect();
        for (page, index) in group.iter().enumerate() {
            let Ok(page) = u8::try_from(page + 1) else {
                continue;
            };
            if let Some(place) = places.get_mut(*index) {
                *place = Some(MenuPages {
                    first,
                    page,
                    buttons: buttons.clone(),
                });
            }
        }
    }
    places
}

/// Whether `pages` (the first page first) can be one composition: always-on menus with buttons that share the
/// first page's background, sound and play, few enough, and small enough for the decoder.
fn shares_background(project: &Project, pages: &[&Menu]) -> bool {
    let Some(first) = pages.first() else {
        return false;
    };
    if pages.len() > MAX_PAGES {
        return false;
    }
    let same = |page: &&Menu| {
        !page.popup
            && !page.animate_buttons
            && !page.buttons.is_empty()
            && page.buttons.len() <= MAX_BUTTONS
            && page.background == first.background
            && page.audio == first.audio
            && page.duration_seconds == first.duration_seconds
            && page.loops == first.loops
            && page.loop_point == first.loop_point
            && page.widescreen == first.widescreen
            && page.is_motion() == first.is_motion()
    };
    pages.iter().all(same)
        && pages
            .iter()
            .map(|page| object_bytes(project, page))
            .sum::<u64>()
            <= OBJECT_BUDGET
}

/// About how many bytes a page's objects take decoded: its picture (the area its texts, pictures, buttons and page of
/// its PSD cover) and each button's normal, selected and activated states.
fn object_bytes(project: &Project, menu: &Menu) -> u64 {
    let area = |rect: Rect| u64::from(rect.w) * u64::from(rect.h);
    let mut cover: Option<(u32, u32, u32, u32)> = None;
    let mut add = |rect: Rect| {
        if rect.w == 0 || rect.h == 0 {
            return;
        }
        let (x1, y1) = (u32::from(rect.x), u32::from(rect.y));
        let (x2, y2) = (x1 + u32::from(rect.w), y1 + u32::from(rect.h));
        cover = Some(match cover {
            None => (x1, y1, x2, y2),
            Some((a, b, c, d)) => (a.min(x1), b.min(y1), c.max(x2), d.max(y2)),
        });
    };
    for text in &menu.texts {
        add(disccraft_menu::text_bounds(menu, text));
    }
    for graphic in &menu.graphics {
        add(disccraft_menu::graphic_bounds(graphic));
    }
    for button in &menu.buttons {
        add(button.rect);
    }
    if let Some(rect) = disccraft_menu::psd_page_bounds(project, menu) {
        add(rect);
    }
    let picture = cover.map_or(0, |(x1, y1, x2, y2)| {
        u64::from(x2.saturating_sub(x1)) * u64::from(y2.saturating_sub(y1))
    });
    let states: u64 = menu
        .buttons
        .iter()
        .map(|button| 3 * area(button.rect))
        .sum();
    picture + states
}
