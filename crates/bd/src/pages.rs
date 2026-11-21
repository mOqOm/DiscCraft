//! A menu's pages built as one interactive composition (M10.26), as Encore builds them (docs/encore-behavior.md
//! 9.5g; GitHub issue #13): the menu's clip plays its background, and its IG has a page for each of the menu's
//! pages after a page 0 whose one auto-action button turns to the page the movie object asks for. A button that
//! links to another page of the same menu only turns the page, so the background video and the menu's sound play
//! on. What each page draws over the background (its texts, pictures, the buttons' texts and thumbnails) is a
//! picture of its own on its IG page (Encore leaves out a page's pictures that are not buttons; DiscCraft keeps
//! them).
//!
//! A menu's pages are built so when they share the background: the same picture or video, sound, length and loops
//! (Encore's pages are one menu; DiscCraft's pages could differ). Otherwise, and for a pop-up menu or more than
//! [`MAX_PAGES`] pages, each page is a menu of its own as before (and turning a page plays its own clip). However
//! large its pictures, as Encore builds them (GitHub issue #13: a 9-page menu with 21.8 MB of objects).
//!
//! A pop-up menu's pages are one composition in each title clip it pops up over ([`popup_page_groups`], GitHub
//! issue #13): there the title plays under them, so they need not share a background, and a link to another of its
//! pages turns the page over the playing title.

use disccraft_project::{Menu, MenuId, Project};

use crate::nav::MenuPages;

/// The most pages a menu's composition has: an IG has up to 255 pages, page 0 turns to the others, and each page
/// names a palette of its own (ids up to 254, M10.28). Encore's 9-page menu has 9 palettes (GitHub issue #13).
pub(crate) const MAX_PAGES: usize = 254;
/// The most buttons a page of a composition has ([`crate::nav::page_value`] keeps the button's index + 1 in a byte).
const MAX_BUTTONS: usize = 254;
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
            || !shares_background(&menus)
        {
            continue;
        }
        groups.push(indices);
    }
    groups
}

/// Which of the project's pop-up menus (by their index in the project's order) pop up with all their pages: each
/// group's menus, its first page first. Every page a pop-up with buttons, few enough pages and buttons (as
/// [`menu_page_groups`]); the pages need not share a background (the title plays under them). Empty for a DVD. The
/// preview reads the pop-up's page this way too.
#[must_use]
pub fn popup_page_groups(project: &Project) -> Vec<Vec<usize>> {
    if project.settings.kind != disccraft_project::DiscKind::BluRay {
        return Vec::new();
    }
    let ids: Vec<MenuId> = project.menus.keys().copied().collect();
    let index_of = |id: MenuId| ids.iter().position(|known| *known == id);
    let mut groups = Vec::new();
    for (index, menu) in project.menus.values().enumerate() {
        if !menu.popup || project.is_menu_page(menu.id) {
            continue;
        }
        let pages = project.menu_pages(menu.id);
        let menus: Vec<&Menu> = pages.iter().filter_map(|id| project.menu(*id)).collect();
        let indices: Vec<usize> = pages.iter().filter_map(|id| index_of(*id)).collect();
        let fits = |page: &&Menu| {
            page.popup
                && !page.animate_buttons
                && !page.buttons.is_empty()
                && page.buttons.len() <= MAX_BUTTONS
        };
        if pages.len() < 2
            || pages.len() > MAX_PAGES
            || menus.len() != pages.len()
            || indices.len() != pages.len()
            || indices.first() != Some(&index)
            || !menus.iter().all(fits)
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
/// first page's background, sound and play, and few enough.
fn shares_background(pages: &[&Menu]) -> bool {
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
}
