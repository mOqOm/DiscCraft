//! Chapter index menus: one button per chapter of a timeline, each jumping to its chapter.
//!
//! The layout (a centered grid of plain rectangles with previous / next buttons on multi-page indexes) is
//! DiscCraft's own. It is not taken from any other product's templates.

use crate::ids::{MenuId, TimelineId};
use crate::io::ProjectError;
use crate::library::library_item;
use crate::link::Link;
use crate::menu::{MenuBackground, Rect, frame_size};
use crate::model::Project;

/// Most chapter buttons on one page of a multi-page index.
pub const CHAPTERS_PER_PAGE: usize = 6;
/// Most chapter buttons a single-page index can hold.
pub const MAX_SINGLE_PAGE: usize = 18;

/// Library item whose colors the index menus use.
const COLORS: &str = "menu.chapter-grid";
/// Height of a chapter button (720x480 coordinates).
const BUTTON_H: u16 = 40;
/// Top edge of the first row.
const GRID_TOP: u16 = 72;
/// Distance between the tops of two rows.
const ROW_PITCH: u16 = 52;
/// Horizontal gap between two columns.
const COLUMN_GAP: u16 = 20;
/// Width of the 720x480 frame the layout is defined in.
const BASE_W: u16 = 720;
/// Height of the 720x480 frame the layout is defined in.
const BASE_H: u32 = 480;
/// The previous-page button.
const PREVIOUS: Rect = Rect {
    x: 60,
    y: 400,
    w: 160,
    h: 40,
};
/// The next-page button.
const NEXT: Rect = Rect {
    x: 500,
    y: 400,
    w: 160,
    h: 40,
};

/// Text for the buttons of a chapter index (the caller translates them; what it leaves empty is in the project's
/// [`crate::Words`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChapterIndexLabels {
    /// Label per chapter, by chapter index. A missing or empty entry falls back to the chapter's own name, then to
    /// "Chapter N" (N = index + 1).
    pub chapters: Vec<String>,
    /// Label of the "previous page" button (empty: "Previous" / "前へ").
    pub previous: String,
    /// Label of the "next page" button (empty: "Next" / "次へ").
    pub next: String,
}

/// Columns and button width for `count` buttons on a page.
fn grid_shape(count: usize) -> (u16, u16) {
    match count {
        0..=3 => (1, 320),
        4..=12 => (2, 280),
        _ => (3, 200),
    }
}

/// Rectangles of `count` chapter buttons on one page, in a 720x480 frame, row-major (left to right, then down).
/// At most [`MAX_SINGLE_PAGE`] rectangles are produced.
pub fn chapter_grid(count: usize) -> Vec<Rect> {
    let count = count.min(MAX_SINGLE_PAGE);
    let (columns, w) = grid_shape(count);
    let total = columns
        .saturating_mul(w)
        .saturating_add(columns.saturating_sub(1).saturating_mul(COLUMN_GAP));
    let left = BASE_W.saturating_sub(total) / 2;

    (0..count)
        .map(|index| {
            let index = u16::try_from(index).unwrap_or(u16::MAX);
            let column = index % columns.max(1);
            let row = index / columns.max(1);
            Rect {
                x: left.saturating_add(column.saturating_mul(w.saturating_add(COLUMN_GAP))),
                y: GRID_TOP.saturating_add(row.saturating_mul(ROW_PITCH)),
                w,
                h: BUTTON_H,
            }
        })
        .collect()
}

/// `rect` on the frame of `settings`: on SD x and w unchanged, y and h scaled by frame_h / 480 (rounded down); on
/// HD in the middle of the frame ([`crate::design_rect`]).
fn placed(rect: Rect, settings: &crate::model::DiscSettings) -> Rect {
    if crate::hd_frame::hd_frame(settings) {
        return crate::hd_frame::design_rect(settings, rect);
    }
    scaled(rect, frame_size(settings).1)
}

/// `rect` in a frame `frame_h` pixels high: x and w unchanged, y and h scaled by frame_h / 480 (rounded down).
fn scaled(rect: Rect, frame_h: u16) -> Rect {
    let scale = |value: u16| {
        let value = u32::from(value).saturating_mul(u32::from(frame_h)) / BASE_H;
        u16::try_from(value).unwrap_or(u16::MAX)
    };
    Rect {
        x: rect.x,
        y: scale(rect.y),
        w: rect.w,
        h: scale(rect.h).max(1),
    }
}

/// Builds an invalid-project error.
fn invalid(message: &str) -> ProjectError {
    ProjectError::Invalid(message.to_string())
}

/// Chapter indices per page: one page for `None`, pages of `n` for `Some(n)`. Validates the page size.
fn pages(chapters: usize, per_page: Option<usize>) -> Result<Vec<Vec<usize>>, ProjectError> {
    let size = match per_page {
        None if chapters > MAX_SINGLE_PAGE => {
            return Err(invalid(
                "too many chapters for one page; use a multi-page chapter index",
            ));
        }
        None => chapters.max(1),
        Some(n) if n == 0 || n > MAX_SINGLE_PAGE => {
            return Err(invalid("chapters per page must be between 1 and 18"));
        }
        Some(n) => n,
    };
    let all: Vec<usize> = (0..chapters).collect();
    Ok(all.chunks(size).map(<[usize]>::to_vec).collect())
}

/// The label of chapter `index`: the given label, else the chapter's name ("Chapter N" in `words` for a chapter
/// without a name of its own, M6.433).
fn chapter_text(
    labels: &ChapterIndexLabels,
    names: &[String],
    index: usize,
    words: crate::Words,
) -> String {
    match labels
        .chapters
        .get(index)
        .filter(|label| !label.trim().is_empty())
    {
        Some(text) => text.clone(),
        None => words.chapter_name(names.get(index).map_or("", String::as_str), index),
    }
}

/// `text`, or `fallback` when it is empty.
fn or_default(text: &str, fallback: &str) -> String {
    if text.trim().is_empty() {
        fallback.to_string()
    } else {
        text.to_string()
    }
}

/// Why a menu cannot be filled as a chapter index (Encore's messages).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillChapterIndexError {
    /// The menu does not exist.
    NoMenu,
    /// The menu has no chapter buttons.
    NoChapterButtons,
    /// None of its chapter buttons links to a timeline's chapter.
    NoLinkedChapterButton,
}

impl Project {
    /// Encore's チャプターインデックスを作成 on a menu that has chapter buttons (a template): the first chapter button
    /// linked to a timeline's chapter sets the timeline and where the index starts, and every chapter button, in
    /// button order, links to the chapter that follows from there (the ones before it to the chapters before).
    /// Buttons outside the timeline's chapters keep their links. A button set to take its name from its link takes
    /// the chapter's name. Returns how many buttons were linked.
    pub fn fill_chapter_index(&mut self, menu: MenuId) -> Result<usize, FillChapterIndexError> {
        let found = self.menu(menu).ok_or(FillChapterIndexError::NoMenu)?;
        let chapter_buttons: Vec<usize> = found
            .buttons
            .iter()
            .enumerate()
            .filter(|(_, button)| button.kind == crate::menu::ButtonKind::Chapter)
            .map(|(index, _)| index)
            .collect();
        if chapter_buttons.is_empty() {
            return Err(FillChapterIndexError::NoChapterButtons);
        }
        let (position, timeline, chapter) = chapter_buttons
            .iter()
            .enumerate()
            .find_map(|(position, &index)| match found.buttons.get(index)?.link {
                Link::Timeline { id, chapter } if self.timelines.contains_key(&id) => {
                    Some((position, id, chapter))
                }
                _ => None,
            })
            .ok_or(FillChapterIndexError::NoLinkedChapterButton)?;
        let chapters = self
            .timelines
            .get(&timeline)
            .map_or(0, |timeline| timeline.chapters.len().max(1));
        let start = i64::from(chapter) - i64::try_from(position).unwrap_or(i64::MAX);
        let mut linked = 0;
        for (offset, &index) in chapter_buttons.iter().enumerate() {
            let Some(chapter) = i64::try_from(offset)
                .ok()
                .and_then(|offset| start.checked_add(offset))
                .and_then(|chapter| u32::try_from(chapter).ok())
                .filter(|chapter| {
                    usize::try_from(*chapter).is_ok_and(|chapter| chapter < chapters)
                })
            else {
                continue;
            };
            let link = Link::Timeline {
                id: timeline,
                chapter,
            };
            let name = crate::menu::Menu::link_target_name(self, &link);
            if let Some(button) = self
                .menu_mut(menu)
                .and_then(|menu| menu.buttons.get_mut(index))
            {
                button.link = link;
                if button.name_from_link
                    && let Some(name) = name
                {
                    if button.sync_name {
                        button.label.clone_from(&name);
                    }
                    button.name = name;
                }
                linked += 1;
            }
        }
        Ok(linked)
    }

    /// Encore's 複数ページのチャプターインデックスを作成 on a menu that has chapter buttons (a template): the menu is
    /// filled as [`Self::fill_chapter_index`] fills it, and copied for the chapters its buttons do not reach, as
    /// pages of the menu (M6.333: Encore makes them pages of the same menu, "ページ 1", "ページ 2", not menus of their
    /// own), each copy's chapter buttons going on where the page before stopped. A 次へ (Next) button links to the
    /// page after and a 前へ (Previous) button to the page before. As in Encore, what a page has no use for is taken
    /// off it: the chapter buttons past the last chapter, the Next button of the last page and the Previous button
    /// of the first (Encore hides them and they stop being buttons; one of these that already links somewhere
    /// outside the pages is kept). Returns the pages, the menu itself first.
    ///
    /// # Errors
    ///
    /// As [`Self::fill_chapter_index`]; also `NoMenu` when a copy cannot be made.
    pub fn fill_chapter_index_pages(
        &mut self,
        menu: MenuId,
    ) -> Result<Vec<MenuId>, FillChapterIndexError> {
        use crate::menu::ButtonKind;
        let _ = self.fill_chapter_index(menu)?;
        let found = self.menu(menu).ok_or(FillChapterIndexError::NoMenu)?;
        let chapter_buttons: Vec<usize> = found
            .buttons
            .iter()
            .enumerate()
            .filter(|(_, button)| button.kind == ButtonKind::Chapter)
            .map(|(index, _)| index)
            .collect();
        // The timeline, and the chapter each page's first chapter button stands for (the first page's may be
        // before the timeline's first chapter, when a later button was the one linked).
        let (timeline, first) = chapter_buttons
            .iter()
            .enumerate()
            .find_map(|(position, &index)| match found.buttons.get(index)?.link {
                Link::Timeline { id, chapter } if self.timelines.contains_key(&id) => Some((
                    id,
                    i64::from(chapter) - i64::try_from(position).unwrap_or(i64::MAX),
                )),
                _ => None,
            })
            .ok_or(FillChapterIndexError::NoLinkedChapterButton)?;
        let chapters = self
            .timelines
            .get(&timeline)
            .map_or(1, |timeline| timeline.chapters.len().max(1));
        let chapters = i64::try_from(chapters).unwrap_or(i64::MAX);
        let per_page = i64::try_from(chapter_buttons.len())
            .unwrap_or(i64::MAX)
            .max(1);
        // Pages until the last chapter has a button (at most as many as chapters).
        let count = chapters
            .saturating_sub(first)
            .saturating_add(per_page - 1)
            .checked_div(per_page)
            .unwrap_or(1)
            .clamp(1, chapters);
        let mut pages = vec![menu];
        // The copies are pages of the menu's first page, with its name, in no folder of their own.
        let root = self.menu_root(menu);
        let root_name = self.menu(root).map(|found| found.name.clone());
        for _ in 1..count {
            let crate::Duplicate::Menu(copy) = self
                .duplicate(crate::Duplicate::Menu(menu))
                .map_err(|_| FillChapterIndexError::NoMenu)?
            else {
                return Err(FillChapterIndexError::NoMenu);
            };
            if let Some(page) = self.menu_mut(copy) {
                page.page_of = Some(root);
                if let Some(name) = &root_name {
                    page.name.clone_from(name);
                }
            }
            let _ = self.placements.remove(&copy.0);
            pages.push(copy);
        }

        for (number, &page) in pages.iter().enumerate() {
            let page_first = i64::try_from(number)
                .ok()
                .and_then(|number| number.checked_mul(per_page))
                .and_then(|offset| first.checked_add(offset))
                .unwrap_or(i64::MAX);
            let mut unused = Vec::new();
            for (offset, &index) in chapter_buttons.iter().enumerate() {
                let chapter = i64::try_from(offset)
                    .ok()
                    .and_then(|offset| page_first.checked_add(offset));
                let Some(chapter) = chapter
                    .filter(|chapter| (0..chapters).contains(chapter))
                    .and_then(|chapter| u32::try_from(chapter).ok())
                else {
                    // Past the last chapter: the button goes. (Before the first, on the first page: it stays, as
                    // the single page's does.)
                    if chapter.is_none_or(|chapter| chapter >= chapters) {
                        unused.push(index);
                    }
                    continue;
                };
                let link = Link::Timeline {
                    id: timeline,
                    chapter,
                };
                let name = crate::menu::Menu::link_target_name(self, &link);
                if let Some(button) = self
                    .menu_mut(page)
                    .and_then(|menu| menu.buttons.get_mut(index))
                {
                    button.link = link;
                    if button.name_from_link
                        && let Some(name) = name
                    {
                        if button.sync_name {
                            button.label.clone_from(&name);
                        }
                        button.name = name;
                    }
                }
            }
            // The page buttons: to the page after and the page before, or off the page that has neither.
            let before = number.checked_sub(1).and_then(|at| pages.get(at)).copied();
            let after = pages.get(number.saturating_add(1)).copied();
            let turns: Vec<(usize, ButtonKind, Link)> = self
                .menu(page)
                .map(|menu| {
                    menu.buttons
                        .iter()
                        .enumerate()
                        .filter(|(_, button)| {
                            matches!(button.kind, ButtonKind::Next | ButtonKind::Previous)
                        })
                        .map(|(index, button)| (index, button.kind, button.link.clone()))
                        .collect()
                })
                .unwrap_or_default();
            for (index, kind, link) in turns {
                let to = if kind == ButtonKind::Next {
                    after
                } else {
                    before
                };
                match to {
                    Some(to) => {
                        if let Some(button) = self
                            .menu_mut(page)
                            .and_then(|menu| menu.buttons.get_mut(index))
                        {
                            button.link = Link::Menu {
                                id: to,
                                button: None,
                            };
                        }
                    }
                    None => {
                        // Kept when it leads somewhere of its own (a Previous that goes back to the main menu).
                        let own = match link {
                            Link::NotSet => false,
                            Link::Menu { id, .. } => !pages.contains(&id),
                            _ => true,
                        };
                        if !own {
                            unused.push(index);
                        }
                    }
                }
            }
            if !unused.is_empty() {
                self.remove_buttons(page, &unused)
                    .map_err(|_| FillChapterIndexError::NoMenu)?;
            }
        }
        Ok(pages)
    }

    /// Creates a chapter index for `timeline`. `per_page`: `None` = one page holding every chapter (error when the
    /// timeline has more than [`MAX_SINGLE_PAGE`] chapters); `Some(n)` = pages of `n` chapters (1..=18, else error)
    /// with previous / next buttons. Returns the new menus in page order.
    pub fn create_chapter_index(
        &mut self,
        timeline: TimelineId,
        name: &str,
        per_page: Option<usize>,
        labels: &ChapterIndexLabels,
    ) -> Result<Vec<MenuId>, ProjectError> {
        let names: Vec<String> = self
            .timeline(timeline)
            .ok_or_else(|| invalid("timeline does not exist"))?
            .chapters
            .iter()
            .map(|chapter| chapter.name.clone())
            .collect();
        let pages = pages(names.len().max(1), per_page)?;
        let menus = self.add_index_menus(name, pages.len())?;

        for (page, chapters) in pages.iter().enumerate() {
            let Some(&menu) = menus.get(page) else {
                continue;
            };
            self.add_chapter_buttons(menu, timeline, chapters, &names, labels)?;
            let previous = page.checked_sub(1).and_then(|before| menus.get(before));
            let next = menus.get(page.saturating_add(1));
            self.add_page_buttons(menu, previous.copied(), next.copied(), labels)?;
        }
        Ok(menus)
    }

    /// Creates `count` empty index menus with the index colors. A single page is named `name`; several pages are
    /// named "`name` 1", "`name` 2", ...
    fn add_index_menus(&mut self, name: &str, count: usize) -> Result<Vec<MenuId>, ProjectError> {
        let colors =
            library_item(COLORS).ok_or_else(|| invalid("chapter index colors are missing"))?;
        let mut menus = Vec::with_capacity(count);
        for page in 1..=count {
            let page_name = if count == 1 {
                name.to_string()
            } else {
                format!("{name} {page}")
            };
            let id = self.add_menu(Some(page_name))?;
            let menu = self
                .menu_mut(id)
                .ok_or_else(|| invalid("menu does not exist"))?;
            menu.background = MenuBackground::Color {
                color: colors.background,
            };
            menu.text_color = colors.text;
            menu.highlight_color = colors.highlight;
            menu.default_button = 0;
            menus.push(id);
        }
        Ok(menus)
    }

    /// Adds one button per chapter in `chapters` to `menu`, laid out with [`chapter_grid`].
    fn add_chapter_buttons(
        &mut self,
        menu: MenuId,
        timeline: TimelineId,
        chapters: &[usize],
        names: &[String],
        labels: &ChapterIndexLabels,
    ) -> Result<(), ProjectError> {
        for (&chapter, rect) in chapters.iter().zip(chapter_grid(chapters.len())) {
            let link = Link::Timeline {
                id: timeline,
                chapter: u32::try_from(chapter)
                    .map_err(|_| invalid("chapter index is too large"))?,
            };
            let label = chapter_text(labels, names, chapter, self.words);
            let index = self.add_button(menu, label, Some(placed(rect, &self.settings)), link)?;
            self.set_button_kind_quietly(menu, index, crate::menu::ButtonKind::Chapter);
        }
        Ok(())
    }

    /// Marks button `index` of `menu` as `kind` (a button just added, so it exists).
    fn set_button_kind_quietly(
        &mut self,
        menu: MenuId,
        index: usize,
        kind: crate::menu::ButtonKind,
    ) {
        if let Some(button) = self
            .menu_mut(menu)
            .and_then(|menu| menu.buttons.get_mut(index))
        {
            button.kind = kind;
        }
    }

    /// Adds the previous / next page buttons that apply to `menu`.
    fn add_page_buttons(
        &mut self,
        menu: MenuId,
        previous: Option<MenuId>,
        next: Option<MenuId>,
        labels: &ChapterIndexLabels,
    ) -> Result<(), ProjectError> {
        if let Some(target) = previous {
            let link = Link::Menu {
                id: target,
                button: None,
            };
            let label = or_default(&labels.previous, self.words.previous());
            let index =
                self.add_button(menu, label, Some(placed(PREVIOUS, &self.settings)), link)?;
            self.set_button_kind_quietly(menu, index, crate::menu::ButtonKind::Previous);
        }
        if let Some(target) = next {
            let link = Link::Menu {
                id: target,
                button: None,
            };
            let label = or_default(&labels.next, self.words.next());
            let index = self.add_button(menu, label, Some(placed(NEXT, &self.settings)), link)?;
            self.set_button_kind_quietly(menu, index, crate::menu::ButtonKind::Next);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ChapterIndexLabels, FillChapterIndexError, MAX_SINGLE_PAGE, chapter_grid};
    use crate::Tick;
    use crate::ids::{MenuId, TimelineId};
    use crate::link::Link;
    use crate::model::{Project, VideoStandard};
    use std::error::Error;
    use std::path::Path;

    /// A project with one timeline holding `chapters` chapters (the first at zero).
    fn project_with_chapters(chapters: i64) -> Result<(Project, TimelineId), Box<dyn Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(Path::new("movie.mp4"))?;
        let timeline = project.add_timeline(asset, None)?;
        for n in 1..chapters {
            project.add_chapter(timeline, Tick(n.saturating_mul(1000)), None)?;
        }
        Ok((project, timeline))
    }

    /// Links of the buttons of `menu`.
    fn links(project: &Project, menu: MenuId) -> Vec<Link> {
        project
            .menu(menu)
            .map(|menu| {
                menu.buttons
                    .iter()
                    .map(|button| button.link.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn the_grid_stays_inside_the_frame_without_overlaps() {
        assert!(chapter_grid(0).is_empty());
        for count in 1..=MAX_SINGLE_PAGE {
            let rects = chapter_grid(count);
            assert_eq!(rects.len(), count);
            for (i, a) in rects.iter().enumerate() {
                assert!(u32::from(a.x) + u32::from(a.w) <= 720, "{count}: {a:?}");
                assert!(u32::from(a.y) + u32::from(a.h) <= 392, "{count}: {a:?}");
                for b in rects.iter().skip(i + 1) {
                    let apart = a.x + a.w <= b.x
                        || b.x + b.w <= a.x
                        || a.y + a.h <= b.y
                        || b.y + b.h <= a.y;
                    assert!(apart, "{count}: {a:?} overlaps {b:?}");
                }
            }
        }
        assert_eq!(chapter_grid(40).len(), MAX_SINGLE_PAGE);
    }

    #[test]
    fn a_single_page_links_every_chapter() -> Result<(), Box<dyn Error>> {
        let (mut project, timeline) = project_with_chapters(4)?;
        let labels = ChapterIndexLabels {
            chapters: vec!["Opening".to_string(), "Storm".to_string()],
            ..ChapterIndexLabels::default()
        };
        let menus = project.create_chapter_index(timeline, "Scenes", None, &labels)?;
        assert_eq!(menus.len(), 1);
        let menu = menus.first().copied().ok_or("no menu")?;

        let expected: Vec<Link> = (0..4)
            .map(|chapter| Link::Timeline {
                id: timeline,
                chapter,
            })
            .collect();
        assert_eq!(links(&project, menu), expected);

        let menu = project.menu(menu).ok_or("missing menu")?;
        assert_eq!(menu.name, "Scenes");
        let labels: Vec<&str> = menu.buttons.iter().map(|b| b.label.as_str()).collect();
        // The model names new chapters "Chapter N", so the fallback and the own name agree here.
        assert_eq!(labels, ["Opening", "Storm", "Chapter 3", "Chapter 4"]);
        Ok(())
    }

    #[test]
    fn chapter_and_page_buttons_have_their_kinds() -> Result<(), Box<dyn Error>> {
        use crate::menu::ButtonKind;
        let (mut project, timeline) = project_with_chapters(8)?;
        let menus = project.create_chapter_index(
            timeline,
            "Scenes",
            Some(4),
            &ChapterIndexLabels::default(),
        )?;
        let kinds = |id| -> Vec<ButtonKind> {
            project
                .menu(id)
                .map(|menu| menu.buttons.iter().map(|button| button.kind).collect())
                .unwrap_or_default()
        };
        let first = kinds(*menus.first().ok_or("no page")?);
        assert_eq!(
            first
                .iter()
                .filter(|kind| **kind == ButtonKind::Chapter)
                .count(),
            4
        );
        assert_eq!(first.last(), Some(&ButtonKind::Next));
        let second = kinds(*menus.get(1).ok_or("no second page")?);
        assert!(second.contains(&ButtonKind::Previous));
        Ok(())
    }

    #[test]
    fn pages_are_chained_with_previous_and_next() -> Result<(), Box<dyn Error>> {
        let (mut project, timeline) = project_with_chapters(8)?;
        let labels = ChapterIndexLabels {
            previous: "Back".to_string(),
            next: "More".to_string(),
            ..ChapterIndexLabels::default()
        };
        let menus = project.create_chapter_index(timeline, "Index", Some(6), &labels)?;
        let (first, second) = match menus.as_slice() {
            [first, second] => (*first, *second),
            other => return Err(format!("expected two pages, got {other:?}").into()),
        };
        assert_eq!(
            project.menu(first).map(|menu| menu.name.as_str()),
            Some("Index 1")
        );
        assert_eq!(
            project.menu(second).map(|menu| menu.name.as_str()),
            Some("Index 2")
        );

        let first_links = links(&project, first);
        assert_eq!(first_links.len(), 7);
        assert_eq!(
            first_links.last(),
            Some(&Link::Menu {
                id: second,
                button: None
            })
        );
        let second_links = links(&project, second);
        assert_eq!(second_links.len(), 3);
        assert_eq!(
            second_links.first(),
            Some(&Link::Timeline {
                id: timeline,
                chapter: 6
            })
        );
        assert_eq!(
            second_links.last(),
            Some(&Link::Menu {
                id: first,
                button: None
            })
        );
        let back = project
            .menu(second)
            .and_then(|menu| menu.buttons.last())
            .map(|button| button.label.clone());
        assert_eq!(back.as_deref(), Some("Back"));
        Ok(())
    }

    /// M6.404 (GitHub issue #13): the Library's multi-page presets are chapter index templates of pages: six chapter
    /// buttons, Previous, Main Menu and Next; eight chapters make two pages linked to each other.
    #[test]
    fn the_multi_page_presets_make_chapter_index_pages() -> Result<(), Box<dyn Error>> {
        use crate::menu::ButtonKind;
        for preset in ["menu.chapter-pages", "menu.scene-pages"] {
            let (mut project, timeline) = project_with_chapters(8)?;
            let menu = project.add_menu_from_preset(preset, None, &[])?;
            if let Some(first) = project
                .menu_mut(menu)
                .and_then(|menu| menu.buttons.first_mut())
            {
                first.link = Link::Timeline {
                    id: timeline,
                    chapter: 0,
                };
            }
            let pages = project
                .fill_chapter_index_pages(menu)
                .map_err(|error| format!("{preset}: {error:?}"))?;
            assert_eq!(pages.len(), 2, "{preset}");
            let shape = |page: MenuId| -> Vec<(ButtonKind, Link)> {
                project
                    .menu(page)
                    .map(|menu| {
                        menu.buttons
                            .iter()
                            .map(|button| (button.kind, button.link.clone()))
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let chapter = |chapter: u32| {
                (
                    ButtonKind::Chapter,
                    Link::Timeline {
                        id: timeline,
                        chapter,
                    },
                )
            };
            let page = |index: usize| Link::Menu {
                id: pages[index],
                button: None,
            };
            let mut first: Vec<(ButtonKind, Link)> = (0..6).map(chapter).collect();
            first.push((ButtonKind::Main, Link::NotSet));
            first.push((ButtonKind::Next, page(1)));
            assert_eq!(shape(pages[0]), first, "{preset}");
            assert_eq!(
                shape(pages[1]),
                vec![
                    chapter(6),
                    chapter(7),
                    (ButtonKind::Previous, page(0)),
                    (ButtonKind::Main, Link::NotSet),
                ],
                "{preset}"
            );
        }
        Ok(())
    }

    /// M6.255: a template menu with fewer chapter buttons than chapters becomes pages, as Encore's Create
    /// Multi-Page Chapter Index makes them.
    #[test]
    fn a_template_menu_becomes_pages_for_all_the_chapters() -> Result<(), Box<dyn Error>> {
        use crate::menu::ButtonKind;
        let (mut project, timeline) = project_with_chapters(8)?;
        let main = project.add_menu(Some("Main".to_owned()))?;
        let menu = project.add_menu(Some("Scenes".to_owned()))?;
        let at = |chapter: u32| Link::Timeline {
            id: timeline,
            chapter,
        };
        // Three chapter buttons (the first linked to chapter 1), Previous, Next, and a button back to the main
        // menu.
        let kinds = [
            ("C1", ButtonKind::Chapter, at(0)),
            ("C2", ButtonKind::Chapter, Link::NotSet),
            ("C3", ButtonKind::Chapter, Link::NotSet),
            ("Previous", ButtonKind::Previous, Link::NotSet),
            ("Next", ButtonKind::Next, Link::NotSet),
            (
                "Main",
                ButtonKind::Main,
                Link::Menu {
                    id: main,
                    button: None,
                },
            ),
        ];
        for (name, kind, link) in kinds {
            let index = project.add_button(menu, name.to_owned(), None, link)?;
            if let Some(button) = project
                .menu_mut(menu)
                .and_then(|menu| menu.buttons.get_mut(index))
            {
                button.kind = kind;
            }
        }
        let pages = project
            .fill_chapter_index_pages(menu)
            .map_err(|error| format!("{error:?}"))?;
        // Eight chapters on three buttons a page: three pages, the menu itself first.
        assert_eq!(pages.len(), 3);
        assert_eq!(pages.first(), Some(&menu));
        let shape = |project: &Project, page: MenuId| -> Vec<(ButtonKind, Link)> {
            project
                .menu(page)
                .map(|menu| {
                    menu.buttons
                        .iter()
                        .map(|button| (button.kind, button.link.clone()))
                        .collect()
                })
                .unwrap_or_default()
        };
        let to = |page: usize| Link::Menu {
            id: pages[page],
            button: None,
        };
        let home = Link::Menu {
            id: main,
            button: None,
        };
        // The first page has no Previous, the last no Next and no button for a ninth chapter.
        assert_eq!(
            shape(&project, pages[0]),
            vec![
                (ButtonKind::Chapter, at(0)),
                (ButtonKind::Chapter, at(1)),
                (ButtonKind::Chapter, at(2)),
                (ButtonKind::Next, to(1)),
                (ButtonKind::Main, home.clone()),
            ]
        );
        assert_eq!(
            shape(&project, pages[1]),
            vec![
                (ButtonKind::Chapter, at(3)),
                (ButtonKind::Chapter, at(4)),
                (ButtonKind::Chapter, at(5)),
                (ButtonKind::Previous, to(0)),
                (ButtonKind::Next, to(2)),
                (ButtonKind::Main, home.clone()),
            ]
        );
        assert_eq!(
            shape(&project, pages[2]),
            vec![
                (ButtonKind::Chapter, at(6)),
                (ButtonKind::Chapter, at(7)),
                (ButtonKind::Previous, to(1)),
                (ButtonKind::Main, home.clone()),
            ]
        );
        // M6.333: the copies are pages of the menu (Encore's ページ 1, ページ 2), with its name, not menus of their own.
        let names: Vec<String> = pages
            .iter()
            .filter_map(|page| project.menu(*page).map(|menu| menu.name.clone()))
            .collect();
        assert_eq!(names, ["Scenes", "Scenes", "Scenes"]);
        assert_eq!(project.menu_pages(menu), pages);
        assert_eq!(project.menu_page_number(pages[2]), 2);
        assert!(project.is_menu_page(pages[1]) && !project.is_menu_page(menu));

        // A Previous that already goes somewhere of its own stays on the first page; chapters that fit one page
        // make no copy.
        let (mut project, timeline) = project_with_chapters(2)?;
        let main = project.add_menu(None)?;
        let menu = project.add_menu(None)?;
        for (name, kind, link) in [
            (
                "C1",
                ButtonKind::Chapter,
                Link::Timeline {
                    id: timeline,
                    chapter: 0,
                },
            ),
            ("C2", ButtonKind::Chapter, Link::NotSet),
            ("C3", ButtonKind::Chapter, Link::NotSet),
            (
                "Back",
                ButtonKind::Previous,
                Link::Menu {
                    id: main,
                    button: None,
                },
            ),
            ("Next", ButtonKind::Next, Link::NotSet),
        ] {
            let index = project.add_button(menu, name.to_owned(), None, link)?;
            if let Some(button) = project
                .menu_mut(menu)
                .and_then(|menu| menu.buttons.get_mut(index))
            {
                button.kind = kind;
            }
        }
        let pages = project
            .fill_chapter_index_pages(menu)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(pages, vec![menu]);
        let kinds: Vec<ButtonKind> = shape(&project, menu)
            .into_iter()
            .map(|(kind, _)| kind)
            .collect();
        assert_eq!(
            kinds,
            [
                ButtonKind::Chapter,
                ButtonKind::Chapter,
                ButtonKind::Previous
            ]
        );
        // A menu without a linked chapter button is refused, as the single page is.
        let empty = project.add_menu(None)?;
        assert_eq!(
            project.fill_chapter_index_pages(empty),
            Err(FillChapterIndexError::NoChapterButtons)
        );
        Ok(())
    }

    #[test]
    fn a_template_menus_chapter_buttons_follow_the_linked_one() -> Result<(), Box<dyn Error>> {
        let (mut project, timeline) = project_with_chapters(4)?;
        let menu = project.add_menu(None)?;
        let at = |chapter: u32| Link::Timeline {
            id: timeline,
            chapter,
        };
        // A main button, then four chapter buttons; none linked yet.
        let _ = project.add_button(menu, "Main".to_owned(), None, Link::Stop)?;
        for n in 0..5 {
            let _ = project.add_button(menu, format!("C{n}"), None, Link::NotSet)?;
        }
        assert_eq!(
            project.fill_chapter_index(menu),
            Err(FillChapterIndexError::NoChapterButtons)
        );
        if let Some(found) = project.menu_mut(menu) {
            for button in found.buttons.iter_mut().skip(1) {
                button.kind = crate::menu::ButtonKind::Chapter;
            }
        }
        assert_eq!(
            project.fill_chapter_index(menu),
            Err(FillChapterIndexError::NoLinkedChapterButton)
        );
        // The second chapter button links to chapter 2 (index 1): the first gets chapter 1, the rest 3 and 4; the
        // fifth is past the last chapter and keeps its link.
        if let Some(button) = project
            .menu_mut(menu)
            .and_then(|found| found.buttons.get_mut(2))
        {
            button.link = at(1);
            button.name_from_link = true;
        }
        if let Some(button) = project
            .menu_mut(menu)
            .and_then(|found| found.buttons.get_mut(3))
        {
            button.name_from_link = true;
        }
        assert_eq!(project.fill_chapter_index(menu), Ok(4));
        assert_eq!(
            links(&project, menu),
            [Link::Stop, at(0), at(1), at(2), at(3), Link::NotSet]
        );
        let names: Vec<String> = project
            .menu(menu)
            .map(|found| {
                found
                    .buttons
                    .iter()
                    .map(|button| button.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        // Only the buttons set to take their name from the link are renamed.
        assert_eq!(names[1], "C0");
        assert_eq!(names[3], "Chapter 3");
        assert_eq!(
            project.fill_chapter_index(MenuId(999)),
            Err(FillChapterIndexError::NoMenu)
        );
        Ok(())
    }

    /// M6.433 (a tester's report: menus mixed English and Japanese): with Japanese words, a chapter index's chapters
    /// without names of their own, its Previous / Next, a button added without text and a template's chapter button
    /// named from its link are Japanese; a chapter's own name is kept.
    #[test]
    fn made_up_words_follow_the_display_language() -> Result<(), Box<dyn Error>> {
        let (mut project, timeline) = project_with_chapters(3)?;
        project.words = crate::Words::Japanese;
        if let Some(chapter) = project
            .timeline_mut(timeline)
            .and_then(|found| found.chapters.get_mut(1))
        {
            chapter.name = "オープニング".to_owned();
        }
        let menus =
            project.create_chapter_index(timeline, "x", Some(2), &ChapterIndexLabels::default())?;
        let labels = |project: &Project, menu: Option<&MenuId>| -> Vec<String> {
            menu.and_then(|menu| project.menu(*menu))
                .map(|found| {
                    found
                        .buttons
                        .iter()
                        .map(|button| button.label.clone())
                        .collect()
                })
                .unwrap_or_default()
        };
        assert_eq!(
            labels(&project, menus.first()),
            ["チャプター 1", "オープニング", "次へ"]
        );
        assert_eq!(labels(&project, menus.get(1)), ["チャプター 3", "前へ"]);

        let menu = project.add_menu(None)?;
        let _ = project.add_button(menu, String::new(), None, Link::NotSet)?;
        let name = |project: &Project| {
            project
                .menu(menu)
                .and_then(|found| found.buttons.last())
                .map(|button| button.name.clone())
        };
        assert_eq!(name(&project).as_deref(), Some("ボタン 1"));
        if let Some(button) = project
            .menu_mut(menu)
            .and_then(|found| found.buttons.last_mut())
        {
            button.kind = crate::menu::ButtonKind::Chapter;
            button.name_from_link = true;
            button.link = Link::Timeline {
                id: timeline,
                chapter: 2,
            };
        }
        assert_eq!(project.fill_chapter_index(menu), Ok(1));
        assert_eq!(name(&project).as_deref(), Some("チャプター 3"));
        assert!(
            project
                .menu(menu)
                .is_some_and(|found| found.name.starts_with("メニュー "))
        );
        // M6.436: a folder's and a chapter playlist's default names too.
        let folder = project.add_folder(None, None)?;
        assert_eq!(
            project
                .folders
                .get(&folder)
                .map(|found| found.name.as_str()),
            Some("フォルダー 1")
        );
        let list = project.add_chapter_playlist(timeline, None)?;
        assert!(
            project
                .chapter_playlists
                .get(&list)
                .is_some_and(|found| found.name.ends_with(" チャプタープレイリスト"))
        );
        Ok(())
    }

    #[test]
    fn bad_requests_are_errors() -> Result<(), Box<dyn Error>> {
        let (mut project, timeline) = project_with_chapters(19)?;
        let labels = ChapterIndexLabels::default();
        assert!(
            project
                .create_chapter_index(timeline, "x", Some(0), &labels)
                .is_err()
        );
        assert!(
            project
                .create_chapter_index(timeline, "x", Some(19), &labels)
                .is_err()
        );
        assert!(
            project
                .create_chapter_index(timeline, "x", None, &labels)
                .is_err()
        );
        assert!(
            project
                .create_chapter_index(TimelineId(999), "x", Some(6), &labels)
                .is_err()
        );
        assert!(project.menus.is_empty());
        Ok(())
    }

    #[test]
    fn pal_scales_the_rows() -> Result<(), Box<dyn Error>> {
        let (mut project, timeline) = project_with_chapters(2)?;
        project.settings.standard = VideoStandard::Pal;
        let menus =
            project.create_chapter_index(timeline, "x", None, &ChapterIndexLabels::default())?;
        let rect = menus
            .first()
            .and_then(|menu| project.menu(*menu))
            .and_then(|menu| menu.buttons.first())
            .map(|button| button.rect)
            .ok_or("no button")?;
        assert_eq!(u32::from(rect.y), 72 * 576 / 480);
        assert_eq!(u32::from(rect.h), 40 * 576 / 480);
        Ok(())
    }
}
