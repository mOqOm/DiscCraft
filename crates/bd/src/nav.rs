use std::collections::BTreeMap;

use disccraft_bdmv::index::IndexTitle;
use disccraft_bdmv::mobj::{MovieObject, NavCommand};
use disccraft_project::Link;

use crate::{BdError, invalid};

/// Mapping from project objects to generated Blu-ray title numbers.
#[derive(Default)]
pub(crate) struct TitleMap {
    /// Timeline id to title number.
    pub(crate) timelines: BTreeMap<u64, u16>,
    /// Slideshow id to title number.
    pub(crate) slideshows: BTreeMap<u64, u16>,
    /// Project playlist id to title number.
    pub(crate) playlists: BTreeMap<u64, u16>,
    /// Chapter playlist id to title number.
    pub(crate) chapter_playlists: BTreeMap<u64, u16>,
    /// M6.543: the age a player's parental setting (PSR13) must reach for the disc to start (the disc's parental
    /// level as [`disccraft_project::bd_parental_age`] gives it); `None` (or 0) when it asks none.
    pub(crate) disc_age: Option<u8>,
}

/// Navigation information for one generated title.
pub(crate) struct TitleNav {
    /// Playlist number played by this title.
    pub(crate) playlist: u32,
    /// Action taken when the title ends.
    pub(crate) end_action: Link,
    /// Where the title its end action plays goes when that one ends, instead of its own end action (Encore's
    /// override); `None` without one.
    pub(crate) end_override: Option<Link>,
    /// M6.543: the age a player's parental setting (PSR13) must reach to play the title, and what plays in its
    /// place on one below it (not set: nothing, the player stops); `None` when it asks none.
    pub(crate) parental: Option<(u8, Link)>,
    /// M6.548: the title's parental versions (of its parental chapters), the top one (`playlist`) first, each the
    /// age a player's setting (PSR13) must reach for it and its playlist; empty without parental chapters.
    pub(crate) versions: Vec<(u8, u32)>,
    /// M6.555: a project playlist's items, whose titles it plays in turn, each as that title starts (its parental
    /// level asked, its version for the player's age) but going on to the next item rather than to its end action;
    /// `playlist` is then unused. Empty for a title that plays its own playlist.
    pub(crate) items: Vec<disccraft_project::PlaylistItem>,
}

/// Navigation information for one generated menu.
pub(crate) struct MenuNav {
    /// Menu id.
    pub(crate) id: u64,
    /// Playlist number played by this menu.
    pub(crate) playlist: u32,
    /// Links for menu buttons.
    pub(crate) buttons: Vec<Link>,
    /// Where the menu goes when its time is up (a timed still, or a motion menu's last play); `NotSet` when it
    /// stays.
    pub(crate) end_action: Link,
    /// How a motion menu plays; `None` for a still menu.
    pub(crate) motion: Option<MenuMotion>,
    /// Commands each button runs before its link (selecting the audio and subtitle streams); empty for most.
    pub(crate) button_prefix: Vec<Vec<NavCommand>>,
    /// The override of the menu's end action (see [`TitleNav::end_override`]).
    pub(crate) end_override: Option<Link>,
    /// Each button's override (empty or one per button).
    pub(crate) button_overrides: Vec<Option<Link>>,
    /// The button selected when the menu is entered without a button of its own (its IG names no default: the
    /// movie object selects one, so a link can choose another and a motion menu's later plays keep the viewer's).
    pub(crate) select: u16,
    /// The menu's place among its pages built as one interactive composition (M10.26); `None` for a menu of its
    /// own.
    pub(crate) pages: Option<MenuPages>,
}

/// A page of a menu whose pages are built as one interactive composition (M10.26, as Encore builds them): page 0
/// of the composition has one auto-action button that turns to the page and button [`PAGE_GPR`] names, the
/// menu's pages follow, and a link from one to another only turns the page (the playlist plays on).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MenuPages {
    /// The index (among the menus) of the first page, whose movie object plays the composition.
    pub(crate) first: usize,
    /// This menu's page in the composition (1 for the first page).
    pub(crate) page: u8,
    /// How many buttons each page has, the first page first.
    pub(crate) buttons: Vec<usize>,
}

/// How a motion menu's movie object plays its playlist (Encore's ループ回数 and ループポイント).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MenuMotion {
    /// How many times it plays before the end action; `None` loops until a button is pressed.
    pub(crate) plays: Option<u16>,
    /// Whether the later plays start at the loop point (the playlist's mark 1) instead of the start.
    pub(crate) loop_mark: bool,
}

/// The GPR that counts a motion menu's plays left.
const LOOP_GPR: u32 = 1;

/// The GPR that carries the button a link asks the next menu to select (its index + 1); 0 when the menu should
/// select its own.
const SELECT_GPR: u32 = 2;

/// The GPR that holds the pending override (its number, 1-based; 0 for none): the title that ends next goes there
/// instead of to its own end action. Menus clear it.
const OVERRIDE_GPR: u32 = 3;

/// M6.555: the GPR that holds which parental version of an item a project playlist plays (0 the top one).
const VERSION_GPR: u32 = 5;

/// The GPR that names the page (and button) of a menu whose pages are one composition (M10.26): the page × 256 +
/// the button's index + 1, or the page × 256 for the page's default button ([`page_value`]).
pub(crate) const PAGE_GPR: u32 = 4;

/// The value of [`PAGE_GPR`] that asks for `page` (1 for the first) with button `button` (an index) selected, or the
/// page's default button.
pub(crate) fn page_value(page: u8, button: Option<usize>) -> Result<u32, BdError> {
    let button = match button {
        Some(index) => u32::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(1))
            .filter(|value| *value <= 0xFF)
            .ok_or_else(|| invalid("too many buttons on a page"))?,
        None => 0,
    };
    Ok((u32::from(page) << 8) | button)
}

/// Every page and button a menu with these pages (their button counts) can be asked for.
fn page_values(buttons: &[usize]) -> Result<Vec<(u8, Option<usize>)>, BdError> {
    let mut values = Vec::new();
    for (index, count) in buttons.iter().enumerate() {
        let page = u8::try_from(index + 1).map_err(|_| invalid("too many pages"))?;
        values.push((page, None));
        values.extend((0..*count).map(|button| (page, Some(button))));
    }
    Ok(values)
}

/// The commands of the auto-action button on page 0 of a menu's composition: it turns to the page (and button)
/// [`PAGE_GPR`] names. The menu's movie object has made it one of them; each is set with immediate operands (one
/// Set_ButtonPage runs).
pub(crate) fn page_dispatcher(buttons: &[usize]) -> Result<Vec<NavCommand>, BdError> {
    let mut commands = Vec::new();
    for (page, button) in page_values(buttons)? {
        commands.push(NavCommand::if_eq(PAGE_GPR, page_value(page, button)?));
        commands.push(match button {
            Some(index) => NavCommand::set_button_page(
                u16::try_from(index).map_err(|_| invalid("too many buttons"))?,
                u16::from(page),
            ),
            None => NavCommand::set_page(u16::from(page)),
        });
    }
    Ok(commands)
}

/// One line of a movie object being written with labels in place of line numbers ([`assemble`]).
enum Line {
    Command(NavCommand),
    GoTo(usize),
    Label(usize),
}

/// The commands of `lines`, each GoTo to its label's line.
fn assemble(lines: Vec<Line>) -> Result<Vec<NavCommand>, BdError> {
    let mut labels = BTreeMap::new();
    let mut at = 0_u32;
    for line in &lines {
        match line {
            Line::Label(label) => {
                let _ = labels.insert(*label, at);
            }
            Line::Command(_) | Line::GoTo(_) => {
                at = at
                    .checked_add(1)
                    .ok_or_else(|| invalid("too many commands"))?;
            }
        }
    }
    lines
        .into_iter()
        .filter_map(|line| match line {
            Line::Command(command) => Some(Ok(command)),
            Line::GoTo(label) => Some(
                labels
                    .get(&label)
                    .map(|line| NavCommand::goto_line(*line))
                    .ok_or_else(|| invalid("missing label")),
            ),
            Line::Label(_) => None,
        })
        .collect()
}

/// Lines that make [`PAGE_GPR`] one of `values` (the first page when it is not), then go on at label `after`.
fn page_check(lines: &mut Vec<Line>, values: &[u32], after: usize) {
    for value in values {
        lines.push(Line::Command(NavCommand::if_eq(PAGE_GPR, *value)));
        lines.push(Line::GoTo(after));
    }
    lines.push(Line::Command(NavCommand::set_reg(PAGE_GPR, 1 << 8)));
    lines.push(Line::Label(after));
}

/// Lines that put the page and button on screen into [`PAGE_GPR`] (when the menu's playlist has ended, for its next
/// play): PSR 11 × 256 + PSR 10 + 1.
fn page_on_screen(lines: &mut Vec<Line>) {
    use disccraft_bdmv::mobj::PSR_OPERAND;
    lines.extend([
        Line::Command(NavCommand::move_reg(PAGE_GPR, PSR_OPERAND | 11)),
        Line::Command(NavCommand::mul_reg(PAGE_GPR, 256)),
        Line::Command(NavCommand::add_reg_reg(PAGE_GPR, PSR_OPERAND | 10)),
        Line::Command(NavCommand::add_reg(PAGE_GPR, 1)),
    ]);
}

/// Generated BDMV navigation tables.
pub(crate) struct Navigation {
    /// First playback title number, or 0 when none.
    pub(crate) first_playback: u16,
    /// Top menu title number, or 0 when none.
    pub(crate) top_menu: u16,
    /// Index titles.
    pub(crate) titles: Vec<IndexTitle>,
    /// Movie objects.
    pub(crate) objects: Vec<MovieObject>,
    /// Commands for menu buttons, per menu and button.
    pub(crate) button_commands: Vec<Vec<Vec<NavCommand>>>,
}

/// Build navigation with no title button setting (the top menu is the first menu).
#[cfg(test)]
pub(crate) fn build_navigation(
    first_play: &Link,
    titles: &[TitleNav],
    menus: &[MenuNav],
    map: &TitleMap,
) -> Result<Navigation, BdError> {
    build_navigation_with_top((first_play, &[], None), &Link::NotSet, titles, menus, map)
}

/// Build index and movie-object navigation. `title_remote` (the disc's Title button) chooses the menu the
/// remote's Top Menu key opens; when it is not a menu, the first menu is used.
pub(crate) fn build_navigation_with_top(
    (first_play, first_streams, first_override): (&Link, &[NavCommand], Option<&Link>),
    title_remote: &Link,
    titles: &[TitleNav],
    menus: &[MenuNav],
    map: &TitleMap,
) -> Result<Navigation, BdError> {
    if titles.is_empty() {
        return Err(invalid("no titles"));
    }
    if titles.len() > 999 {
        return Err(invalid("too many titles"));
    }

    let counts = ObjectCounts::new(titles.len(), menus.len())?;
    let menu_ids = menus.iter().map(|menu| menu.id).collect();
    let menu_buttons = menus.iter().map(|menu| menu.buttons.len()).collect();
    let mut builder = NavBuilder {
        titles,
        map,
        base: counts.chapter_base,
        extra: Vec::new(),
        memo: BTreeMap::new(),
        menu_ids,
        overrides: collect_overrides(first_override, titles, menus),
        override_objects: BTreeMap::new(),
        menu_buttons,
        menu_pages: menus.iter().map(|menu| menu.pages.clone()).collect(),
        resumes: menus
            .iter()
            .any(|menu| menu.end_action == Link::Resume || menu.buttons.contains(&Link::Resume)),
        menu_base: counts.menu_base,
        dispatcher: counts.dispatcher,
        refused: Vec::new(),
        versions: Vec::new(),
    };

    let links = DiscLinks {
        first_play,
        title_remote,
        first_streams,
        first_override,
    };
    let mut objects = build_objects(&links, titles, menus, &mut builder)?;
    let button_commands = build_button_commands(menus, &mut builder)?;
    objects.extend(builder.extra);

    let index_titles = index_titles(titles.len())?;
    Ok(Navigation {
        first_playback: FIRST_PLAYBACK_OBJECT,
        top_menu: TOP_MENU_OBJECT,
        titles: index_titles,
        objects,
        button_commands,
    })
}

/// Movie object run at disc insertion.
const FIRST_PLAYBACK_OBJECT: u16 = 0;
/// Movie object of the top menu.
const TOP_MENU_OBJECT: u16 = 1;

struct ObjectCounts {
    menu_base: u16,
    dispatcher: Option<u16>,
    chapter_base: u16,
}

impl ObjectCounts {
    fn new(title_count: usize, menu_count: usize) -> Result<Self, BdError> {
        let title_count = u16::try_from(title_count).map_err(|_| invalid("too many titles"))?;
        let menu_count = u16::try_from(menu_count).map_err(|_| invalid("too many menus"))?;

        let menu_base = 2_u16
            .checked_add(title_count)
            .ok_or_else(|| invalid("too many titles"))?;
        let dispatcher = if menu_count == 0 {
            None
        } else {
            Some(
                menu_base
                    .checked_add(menu_count)
                    .ok_or_else(|| invalid("too many menus"))?,
            )
        };
        // Chapter objects follow the dispatcher (or the titles when there are no menus).
        let chapter_base = match dispatcher {
            Some(number) => number
                .checked_add(1)
                .ok_or_else(|| invalid("too many objects"))?,
            None => menu_base,
        };

        Ok(Self {
            menu_base,
            dispatcher,
            chapter_base,
        })
    }
}

struct NavBuilder<'a> {
    titles: &'a [TitleNav],
    map: &'a TitleMap,
    base: u16,
    extra: Vec<MovieObject>,
    memo: BTreeMap<(u16, u32), u16>,
    menu_ids: Vec<u64>,
    /// The overrides (Encore's 上書き) the disc uses, in the order of their numbers.
    overrides: Vec<Link>,
    /// The movie object that follows each override.
    override_objects: BTreeMap<usize, u16>,
    /// How many buttons each menu has.
    menu_buttons: Vec<usize>,
    /// Each menu's place among pages built as one composition.
    menu_pages: Vec<Option<MenuPages>>,
    menu_base: u16,
    dispatcher: Option<u16>,
    /// Whether a menu resumes (M6.528): the titles' movie objects then have the resume intention, so that the Top
    /// Menu key suspends a title for Branch_Resume to go back to.
    resumes: bool,
    /// M6.543: the movie objects that play in a title's place on a player below its parental level, by link.
    refused: Vec<(Link, u16)>,
    /// M6.548: the movie objects of titles' parental versions, by title, version and chapter.
    versions: Vec<((usize, usize, Option<u32>), u16)>,
}

impl<'a> NavBuilder<'a> {
    fn menu_object(&self, index: usize) -> Result<u16, BdError> {
        let offset = u16::try_from(index).map_err(|_| invalid("too many menus"))?;
        self.menu_base
            .checked_add(offset)
            .ok_or_else(|| invalid("too many menus"))
    }

    fn menu_index(&self, id: u64) -> Option<usize> {
        self.menu_ids.iter().position(|menu_id| *menu_id == id)
    }

    fn chapter_object(&mut self, title: u16, chapter: u32) -> Result<u16, BdError> {
        if let Some(object_number) = self.memo.get(&(title, chapter)).copied() {
            return Ok(object_number);
        }

        let object_number = self.next_object_number()?;
        self.memo.insert((title, chapter), object_number);

        let index = self.extra.len();
        self.extra.push(MovieObject {
            resume_intention: false,
            menu_call_mask: false,
            title_search_mask: false,
            commands: Vec::new(),
        });

        let commands = self.chapter_commands(title, chapter)?;
        if let Some(object) = self.extra.get_mut(index) {
            object.commands = commands;
        }

        Ok(object_number)
    }

    fn next_object_number(&self) -> Result<u16, BdError> {
        let offset = u16::try_from(self.extra.len()).map_err(|_| invalid("too many objects"))?;
        self.base
            .checked_add(offset)
            .ok_or_else(|| invalid("too many objects"))
    }

    fn chapter_commands(&mut self, title: u16, chapter: u32) -> Result<Vec<NavCommand>, BdError> {
        let title_index = usize::from(
            title
                .checked_sub(1)
                .ok_or_else(|| invalid("invalid title number"))?,
        );

        let parental = self
            .titles
            .get(title_index)
            .ok_or_else(|| invalid("title does not exist"))?
            .parental
            .clone();

        // M6.543: a chapter of a title with a parental level asks it as the title does.
        let mut commands = self.parental_guard(parental.as_ref())?;
        commands.extend(self.version_commands(title_index, 0, Some(chapter))?);
        Ok(commands)
    }

    /// M6.548: what plays version `version` of title `title_index` (from `chapter`'s mark, or from its start), then
    /// its end action: below the version's age, the next version's movie object instead (the last version is for
    /// all ages). A title without parental versions plays its own playlist.
    fn version_commands(
        &mut self,
        title_index: usize,
        version: usize,
        chapter: Option<u32>,
    ) -> Result<Vec<NavCommand>, BdError> {
        let (playlist, end_action, end_override, versions) = {
            let title_nav = self
                .titles
                .get(title_index)
                .ok_or_else(|| invalid("title does not exist"))?;
            (
                title_nav.playlist,
                title_nav.end_action.clone(),
                title_nav.end_override.clone(),
                title_nav.versions.clone(),
            )
        };
        let mut commands = Vec::new();
        let playlist = match versions.get(version) {
            Some((age, playlist)) => {
                if version + 1 < versions.len() && *age > 0 {
                    let object = self.version_object(title_index, version + 1, chapter)?;
                    commands.push(NavCommand::if_lt(
                        disccraft_bdmv::mobj::PSR_OPERAND | 13,
                        u32::from(*age),
                    ));
                    commands.push(NavCommand::jump_object(u32::from(object)));
                }
                *playlist
            }
            None => playlist,
        };
        commands.push(match chapter {
            Some(chapter) => NavCommand::play_pl_mark(playlist, chapter),
            None => NavCommand::play_pl(playlist),
        });
        commands.extend(self.title_end_commands(&end_action, end_override.as_ref())?);
        Ok(commands)
    }

    /// M6.548: the movie object that plays version `version` of title `title_index` (from `chapter`), made once.
    fn version_object(
        &mut self,
        title_index: usize,
        version: usize,
        chapter: Option<u32>,
    ) -> Result<u16, BdError> {
        if let Some((_, object)) = self
            .versions
            .iter()
            .find(|(key, _)| *key == (title_index, version, chapter))
        {
            return Ok(*object);
        }
        let object_number = self.next_object_number()?;
        self.versions
            .push(((title_index, version, chapter), object_number));
        let index = self.extra.len();
        self.extra.push(MovieObject {
            resume_intention: self.resumes,
            menu_call_mask: false,
            title_search_mask: false,
            commands: Vec::new(),
        });
        let commands = self.version_commands(title_index, version, chapter)?;
        if let Some(object) = self.extra.get_mut(index) {
            object.commands = commands;
        }
        Ok(object_number)
    }

    /// M6.555: the commands of a project playlist's items: each item's title in turn, as that title starts (its
    /// parental guard, then its version for the player's age, [`version_choice`]) but without its end action. The
    /// movie object waits on each playlist it plays and goes on with the next command when it ends.
    fn sequence_commands(
        &mut self,
        items: &[disccraft_project::PlaylistItem],
    ) -> Result<Vec<NavCommand>, BdError> {
        use disccraft_project::PlaylistItem;
        let mut commands = Vec::new();
        for item in items {
            let title = match item {
                PlaylistItem::Timeline(id) => self.map.timelines.get(&id.0),
                PlaylistItem::Slideshow(id) => self.map.slideshows.get(id),
                PlaylistItem::ChapterPlaylist(id) => self.map.chapter_playlists.get(id),
            };
            let Some(nav) = title
                .and_then(|title| usize::from(*title).checked_sub(1))
                .and_then(|index| self.titles.get(index))
            else {
                continue;
            };
            let (parental, playlist, versions) =
                (nav.parental.clone(), nav.playlist, nav.versions.clone());
            commands.extend(self.parental_guard(parental.as_ref())?);
            commands.extend(version_choice(playlist, &versions));
        }
        Ok(commands)
    }

    /// M6.543: the commands that ask a player's parental setting (PSR13, an age) for `parental` (the age and what
    /// plays in the title's place): below the age, jump to the movie object of what plays instead (one that does
    /// nothing when nothing is set, so the player stops); nothing for none, or for age 0 (all ages).
    fn parental_guard(
        &mut self,
        parental: Option<&(u8, Link)>,
    ) -> Result<Vec<NavCommand>, BdError> {
        let Some((age, link)) = parental.filter(|(age, _)| *age > 0) else {
            return Ok(Vec::new());
        };
        let object = self.refused_object(link)?;
        Ok(vec![
            NavCommand::if_lt(disccraft_bdmv::mobj::PSR_OPERAND | 13, u32::from(*age)),
            NavCommand::jump_object(u32::from(object)),
        ])
    }

    /// The movie object that runs `link` in a title's place on a player below its parental level (M6.543): a NOP
    /// for nothing (the object ends and the player stops).
    fn refused_object(&mut self, link: &Link) -> Result<u16, BdError> {
        if let Some((_, object)) = self.refused.iter().find(|(known, _)| known == link) {
            return Ok(*object);
        }
        let object_number = self.next_object_number()?;
        self.refused.push((link.clone(), object_number));
        let index = self.extra.len();
        self.extra.push(MovieObject {
            resume_intention: false,
            menu_call_mask: false,
            title_search_mask: false,
            commands: Vec::new(),
        });
        let mut commands = link_commands(link, self)?;
        if commands.is_empty() {
            commands.push(NavCommand::nop());
        }
        if let Some(object) = self.extra.get_mut(index) {
            object.commands = commands;
        }
        Ok(object_number)
    }

    /// `set GPR3 = n` for a link with override `link` (number n); nothing without one.
    fn set_override(&self, link: Option<&Link>) -> Option<NavCommand> {
        let number = self
            .overrides
            .iter()
            .position(|known| Some(known) == link)?;
        let number = u32::try_from(number).ok()?.checked_add(1)?;
        Some(NavCommand::set_reg(OVERRIDE_GPR, number))
    }

    /// The movie object that clears the pending override `index` and follows it.
    fn override_object(&mut self, index: usize) -> Result<u16, BdError> {
        if let Some(object) = self.override_objects.get(&index).copied() {
            return Ok(object);
        }
        let object_number = self.next_object_number()?;
        let _ = self.override_objects.insert(index, object_number);
        let slot = self.extra.len();
        self.extra.push(finish_object(Vec::new()));
        let target = self
            .overrides
            .get(index)
            .cloned()
            .ok_or_else(|| invalid("missing override"))?;
        let mut commands = vec![NavCommand::set_reg(OVERRIDE_GPR, 0)];
        commands.extend(link_commands(&target, self)?);
        if let Some(object) = self.extra.get_mut(slot) {
            *object = finish_object(commands);
        }
        Ok(object_number)
    }

    /// What a title does when it ends: a pending override wins (Encore's 上書き); otherwise its own override is set
    /// for the next title and its end action follows.
    fn title_end_commands(
        &mut self,
        end_action: &Link,
        end_override: Option<&Link>,
    ) -> Result<Vec<NavCommand>, BdError> {
        let mut commands = Vec::new();
        for index in 0..self.overrides.len() {
            let object = self.override_object(index)?;
            let number = u32::try_from(index)
                .ok()
                .and_then(|index| index.checked_add(1))
                .ok_or_else(|| invalid("too many overrides"))?;
            commands.push(NavCommand::if_eq(OVERRIDE_GPR, number));
            commands.push(NavCommand::jump_object(u32::from(object)));
        }
        commands.extend(self.set_override(end_override));
        commands.extend(link_commands(end_action, self)?);
        Ok(commands)
    }
}

/// The overrides a disc uses (each once, in a fixed order): the first play's, the titles', the menus' end actions'
/// and the buttons'.
fn collect_overrides(
    first_override: Option<&Link>,
    titles: &[TitleNav],
    menus: &[MenuNav],
) -> Vec<Link> {
    let mut overrides: Vec<Link> = Vec::new();
    let all = first_override
        .into_iter()
        .chain(
            titles
                .iter()
                .filter_map(|title| title.end_override.as_ref()),
        )
        .chain(menus.iter().flat_map(|menu| {
            menu.end_override
                .iter()
                .chain(menu.button_overrides.iter().flatten())
        }));
    for link in all {
        if !overrides.contains(link) {
            overrides.push(link.clone());
        }
    }
    overrides
}

/// The disc-level links: First Play and the Title button.
struct DiscLinks<'a> {
    first_play: &'a Link,
    title_remote: &'a Link,
    /// Commands the disc runs before its first play (Encore's Set Audio / Set Subtitle).
    first_streams: &'a [NavCommand],
    /// The first play's override.
    first_override: Option<&'a Link>,
}

fn build_objects(
    links: &DiscLinks<'_>,
    titles: &[TitleNav],
    menus: &[MenuNav],
    builder: &mut NavBuilder<'_>,
) -> Result<Vec<MovieObject>, BdError> {
    let mut objects = Vec::new();

    // M6.543: the disc's parental level is asked before anything plays (a player below it stops).
    let disc = builder.map.disc_age.map(|age| (age, Link::NotSet));
    let mut first = builder.parental_guard(disc.as_ref())?;
    first.extend_from_slice(links.first_streams);
    first.extend(builder.set_override(links.first_override));
    first.extend(first_play_commands(links.first_play, builder)?);
    objects.push(finish_object(first));
    objects.push(finish_object(top_menu_commands(
        links.title_remote,
        builder,
    )?));

    for (index, title) in titles.iter().enumerate() {
        objects.push(title_object((index, title), builder)?);
    }

    for (index, menu) in menus.iter().enumerate() {
        objects.push(menu_object(menu, index, builder)?);
    }

    if builder.dispatcher.is_some() {
        objects.push(dispatcher_object(builder)?);
    }

    Ok(objects)
}

fn first_play_commands(
    first_play: &Link,
    builder: &mut NavBuilder<'_>,
) -> Result<Vec<NavCommand>, BdError> {
    match first_play {
        Link::Timeline { id, chapter } => timeline_commands(*chapter, id.0, builder),
        Link::Playlist { id } => Ok(playlist_commands(*id, builder)),
        Link::ChapterPlaylist { id } => Ok(chapter_playlist_commands(*id, builder)),
        Link::Slideshow { id } => Ok(slideshow_commands(*id, builder)),
        // Nothing has played yet to resume: as an unset first play.
        Link::NotSet | Link::LastMenu | Link::Resume => menu_or_title_commands(builder),
        // A menu that does not exist falls back to the first menu (or title 1).
        Link::Menu { id, button } => {
            let commands = menu_commands(id.0, *button, builder)?;
            if commands.is_empty() {
                menu_or_title_commands(builder)
            } else {
                Ok(commands)
            }
        }
        Link::Stop => Ok(Vec::new()),
    }
}

fn top_menu_commands(
    title_remote: &Link,
    builder: &NavBuilder<'_>,
) -> Result<Vec<NavCommand>, BdError> {
    if let Link::Menu { id, button } = title_remote {
        let commands = menu_commands(id.0, *button, builder)?;
        if !commands.is_empty() {
            return Ok(commands);
        }
    }
    menu_or_title_commands(builder)
}

fn menu_or_title_commands(builder: &NavBuilder<'_>) -> Result<Vec<NavCommand>, BdError> {
    if builder.menu_ids.is_empty() {
        Ok(vec![NavCommand::jump_title(1)])
    } else {
        let object = builder.menu_object(0)?;
        Ok(vec![NavCommand::jump_object(u32::from(object))])
    }
}

/// M6.555: plays the version of a title for the player's age where the commands go on after it (a project
/// playlist's item): with versions, [`VERSION_GPR`] counts those whose age the player's (PSR13) is below (the versions
/// go down in age, the last for all ages) and the version it names plays; without, the title's own playlist.
fn version_choice(playlist: u32, versions: &[(u8, u32)]) -> Vec<NavCommand> {
    let number = |index: usize| u32::try_from(index).unwrap_or(u32::MAX);
    if versions.len() < 2 {
        return vec![NavCommand::play_pl(
            versions.first().map_or(playlist, |(_, top)| *top),
        )];
    }
    let mut commands = vec![NavCommand::set_reg(VERSION_GPR, 0)];
    for (index, (age, _)) in versions.iter().enumerate().take(versions.len() - 1) {
        if *age > 0 {
            commands.push(NavCommand::if_lt(
                disccraft_bdmv::mobj::PSR_OPERAND | 13,
                u32::from(*age),
            ));
            commands.push(NavCommand::set_reg(VERSION_GPR, number(index + 1)));
        }
    }
    for (index, (_, version)) in versions.iter().enumerate() {
        commands.push(NavCommand::if_eq(VERSION_GPR, number(index)));
        commands.push(NavCommand::play_pl(*version));
    }
    commands
}

fn title_object(
    (index, title): (usize, &TitleNav),
    builder: &mut NavBuilder<'_>,
) -> Result<MovieObject, BdError> {
    // M6.555: a project playlist plays its items' titles in turn, then its end action.
    if !title.items.is_empty() {
        let mut commands = builder.sequence_commands(&title.items)?;
        commands
            .extend(builder.title_end_commands(&title.end_action, title.end_override.as_ref())?);
        let mut object = finish_object(commands);
        object.resume_intention = builder.resumes;
        return Ok(object);
    }
    // M6.543: a title asks its parental level before it plays (as a DVD's does); M6.548: then plays the version of
    // its parental chapters for the player's age.
    let mut commands = builder.parental_guard(title.parental.as_ref())?;
    commands.extend(builder.version_commands(index, 0, None)?);
    let mut object = finish_object(commands);
    // M6.528: with a menu that resumes, the Top Menu key suspends the title instead of ending it.
    object.resume_intention = builder.resumes;
    Ok(object)
}

fn link_commands(link: &Link, builder: &mut NavBuilder<'_>) -> Result<Vec<NavCommand>, BdError> {
    match link {
        Link::Timeline { id, chapter } => timeline_commands(*chapter, id.0, builder),
        Link::Playlist { id } => Ok(playlist_commands(*id, builder)),
        Link::ChapterPlaylist { id } => Ok(chapter_playlist_commands(*id, builder)),
        Link::Slideshow { id } => Ok(slideshow_commands(*id, builder)),
        Link::Menu { id, button } => menu_commands(id.0, *button, builder),
        Link::LastMenu => last_menu_commands(builder),
        // M6.528: back to the title the Top Menu key suspended (a pop-up's button closes it instead, see
        // `popup_commands`).
        Link::Resume => Ok(vec![NavCommand::resume()]),
        Link::NotSet | Link::Stop => Ok(Vec::new()),
    }
}

fn timeline_commands(
    chapter: u32,
    id: u64,
    builder: &mut NavBuilder<'_>,
) -> Result<Vec<NavCommand>, BdError> {
    let Some(title) = builder.map.timelines.get(&id).copied() else {
        return Ok(Vec::new());
    };

    if chapter == 0 {
        Ok(vec![NavCommand::jump_title(u32::from(title))])
    } else {
        let object = builder.chapter_object(title, chapter)?;
        Ok(vec![NavCommand::jump_object(u32::from(object))])
    }
}

fn playlist_commands(id: u64, builder: &NavBuilder<'_>) -> Vec<NavCommand> {
    if let Some(title) = builder.map.playlists.get(&id).copied() {
        vec![NavCommand::jump_title(u32::from(title))]
    } else {
        Vec::new()
    }
}

fn chapter_playlist_commands(id: u64, builder: &NavBuilder<'_>) -> Vec<NavCommand> {
    if let Some(title) = builder.map.chapter_playlists.get(&id).copied() {
        vec![NavCommand::jump_title(u32::from(title))]
    } else {
        Vec::new()
    }
}

fn slideshow_commands(id: u64, builder: &NavBuilder<'_>) -> Vec<NavCommand> {
    if let Some(title) = builder.map.slideshows.get(&id).copied() {
        vec![NavCommand::jump_title(u32::from(title))]
    } else {
        Vec::new()
    }
}

/// Go to menu `id`, with `button` (an index) selected when it names one of the menu's buttons (Encore's link to a
/// menu's button); otherwise the menu selects its own.
fn menu_commands(
    id: u64,
    button: Option<u32>,
    builder: &NavBuilder<'_>,
) -> Result<Vec<NavCommand>, BdError> {
    let Some(index) = builder.menu_index(id) else {
        return Ok(Vec::new());
    };
    let count = builder.menu_buttons.get(index).copied().unwrap_or(0);
    // A page of a menu built as one composition: its first page's movie object, with the page and button in
    // PAGE_GPR (M10.26).
    if let Some(Some(pages)) = builder.menu_pages.get(index) {
        let button = button
            .and_then(|button| usize::try_from(button).ok())
            .filter(|button| *button < count);
        return Ok(vec![
            NavCommand::set_reg(PAGE_GPR, page_value(pages.page, button)?),
            NavCommand::jump_object(u32::from(builder.menu_object(pages.first)?)),
        ]);
    }
    let object = builder.menu_object(index)?;
    let mut commands = Vec::new();
    if let Some(button) = button.and_then(|button| u16::try_from(button).ok())
        && usize::from(button) < count
    {
        commands.push(NavCommand::set_reg(SELECT_GPR, u32::from(button) + 1));
    }
    commands.push(NavCommand::jump_object(u32::from(object)));
    Ok(commands)
}

fn last_menu_commands(builder: &NavBuilder<'_>) -> Result<Vec<NavCommand>, BdError> {
    match builder.dispatcher {
        Some(object) => Ok(vec![NavCommand::jump_object(u32::from(object))]),
        None => Ok(Vec::new()),
    }
}

fn menu_object(
    menu: &MenuNav,
    index: usize,
    builder: &mut NavBuilder<'_>,
) -> Result<MovieObject, BdError> {
    if let Some(pages) = &menu.pages {
        if pages.first == index {
            return pages_object(menu, index, &pages.buttons, builder);
        }
        // A later page: the first page's object, on this page.
        return Ok(finish_object(vec![
            NavCommand::set_reg(PAGE_GPR, page_value(pages.page, None)?),
            NavCommand::jump_object(u32::from(builder.menu_object(pages.first)?)),
        ]));
    }
    let value = menu_value(index)?;
    let mut commands = vec![
        NavCommand::set_reg(0, u32::from(value)),
        // The button a link asked for, or else the menu's own (Set_ButtonPage with immediate operands: libbluray
        // does not take the button from a register).
        NavCommand::if_eq(SELECT_GPR, 0),
        NavCommand::set_button_page(menu.select, 0),
    ];
    for button in 0..menu.buttons.len() {
        let id = u16::try_from(button).map_err(|_| invalid("too many buttons"))?;
        commands.push(NavCommand::if_eq(SELECT_GPR, u32::from(id) + 1));
        commands.push(NavCommand::set_button_page(id, 0));
    }
    commands.push(NavCommand::set_reg(SELECT_GPR, 0));
    // Reaching a menu ends a pending override.
    if !builder.overrides.is_empty() {
        commands.push(NavCommand::set_reg(OVERRIDE_GPR, 0));
    }
    let mut end: Vec<NavCommand> = builder
        .set_override(menu.end_override.as_ref())
        .into_iter()
        .collect();
    end.extend(link_commands(&menu.end_action, builder)?);
    match menu.motion {
        None => {
            // A timed still ends the playlist: on to the end action (Encore's menu end action and duration).
            commands.push(NavCommand::play_pl(menu.playlist));
            commands.extend(end);
        }
        Some(motion) => {
            let start = u32::try_from(commands.len()).map_err(|_| invalid("too many commands"))?;
            commands.extend(motion_commands(menu.playlist, motion, start, end)?);
        }
    }
    Ok(finish_object(commands))
}

/// The movie object of a menu whose pages are one composition (M10.26), on its first page: as a menu's object, but
/// [`PAGE_GPR`] is made a page and button the composition has before it plays, and a motion menu's later plays come
/// back to the page and button on screen.
fn pages_object(
    menu: &MenuNav,
    index: usize,
    buttons: &[usize],
    builder: &mut NavBuilder<'_>,
) -> Result<MovieObject, BdError> {
    let values = page_values(buttons)?
        .into_iter()
        .map(|(page, button)| page_value(page, button))
        .collect::<Result<Vec<_>, _>>()?;
    let mut lines = vec![
        Line::Command(NavCommand::set_reg(0, u32::from(menu_value(index)?))),
        Line::Command(NavCommand::set_reg(SELECT_GPR, 0)),
    ];
    // Reaching a menu ends a pending override.
    if !builder.overrides.is_empty() {
        lines.push(Line::Command(NavCommand::set_reg(OVERRIDE_GPR, 0)));
    }
    let mut end: Vec<NavCommand> = builder
        .set_override(menu.end_override.as_ref())
        .into_iter()
        .collect();
    end.extend(link_commands(&menu.end_action, builder)?);
    let end = || end.iter().copied().map(Line::Command);
    const CHECKED: usize = 1;
    const AGAIN: usize = 2;
    const RECHECKED: usize = 3;
    const COUNT: usize = 4;
    const END: usize = 5;
    page_check(&mut lines, &values, CHECKED);
    let play = NavCommand::play_pl(menu.playlist);
    match menu.motion {
        None => {
            lines.push(Line::Command(play));
            lines.extend(end());
        }
        Some(motion) => {
            let again = if motion.loop_mark {
                NavCommand::play_pl_mark(menu.playlist, 1)
            } else {
                play
            };
            match motion.plays {
                None => {
                    lines.push(Line::Command(play));
                    lines.push(Line::Label(AGAIN));
                    page_on_screen(&mut lines);
                    page_check(&mut lines, &values, RECHECKED);
                    lines.push(Line::Command(again));
                    lines.push(Line::GoTo(AGAIN));
                }
                Some(0 | 1) => {
                    lines.push(Line::Command(play));
                    lines.extend(end());
                }
                Some(plays) => {
                    lines.push(Line::Command(NavCommand::set_reg(
                        LOOP_GPR,
                        u32::from(plays.saturating_sub(1)),
                    )));
                    lines.push(Line::Command(play));
                    lines.push(Line::Label(COUNT));
                    lines.push(Line::Command(NavCommand::if_eq(LOOP_GPR, 0)));
                    lines.push(Line::GoTo(END));
                    lines.push(Line::Command(NavCommand::sub_reg(LOOP_GPR, 1)));
                    page_on_screen(&mut lines);
                    page_check(&mut lines, &values, RECHECKED);
                    lines.push(Line::Command(again));
                    lines.push(Line::GoTo(COUNT));
                    lines.push(Line::Label(END));
                    // Somewhere for the GoTo to land.
                    lines.push(Line::Command(NavCommand::nop()));
                    lines.extend(end());
                }
            }
        }
    }
    Ok(finish_object(assemble(lines)?))
}

/// A motion menu's commands from line `start` of its movie object: play the menu, then play it again (from the
/// loop point's mark when it has one) forever, or until it has played `plays` times (counted down in
/// [`LOOP_GPR`]), then the end action `end`.
fn motion_commands(
    playlist: u32,
    motion: MenuMotion,
    start: u32,
    end: Vec<NavCommand>,
) -> Result<Vec<NavCommand>, BdError> {
    let again = if motion.loop_mark {
        NavCommand::play_pl_mark(playlist, 1)
    } else {
        NavCommand::play_pl(playlist)
    };
    let line = |offset: u32| {
        start
            .checked_add(offset)
            .ok_or_else(|| invalid("too many commands"))
    };
    let mut commands = Vec::new();
    match motion.plays {
        None => commands.extend([
            NavCommand::play_pl(playlist),
            again,
            NavCommand::goto_line(line(1)?),
        ]),
        Some(0 | 1) => {
            commands.push(NavCommand::play_pl(playlist));
            commands.extend(end);
        }
        Some(plays) => {
            commands.extend([
                NavCommand::set_reg(LOOP_GPR, u32::from(plays.saturating_sub(1))),
                NavCommand::play_pl(playlist),
                NavCommand::if_eq(LOOP_GPR, 0),
                NavCommand::goto_line(line(7)?),
                NavCommand::sub_reg(LOOP_GPR, 1),
                again,
                NavCommand::goto_line(line(2)?),
            ]);
            if end.is_empty() {
                // Somewhere for the last GoTo to land.
                commands.push(NavCommand::nop());
            }
            commands.extend(end);
        }
    }
    Ok(commands)
}

fn dispatcher_object(builder: &NavBuilder<'_>) -> Result<MovieObject, BdError> {
    let mut commands = Vec::new();

    for index in 0..builder.menu_ids.len() {
        let value = menu_value(index)?;
        let object = builder.menu_object(index)?;
        commands.push(NavCommand::if_eq(0, u32::from(value)));
        commands.push(NavCommand::jump_object(u32::from(object)));
    }

    let first = builder.menu_object(0)?;
    commands.push(NavCommand::jump_object(u32::from(first)));
    Ok(finish_object(commands))
}

fn menu_value(index: usize) -> Result<u16, BdError> {
    let value = index
        .checked_add(1)
        .ok_or_else(|| invalid("too many menus"))?;
    u16::try_from(value).map_err(|_| invalid("too many menus"))
}

fn build_button_commands(
    menus: &[MenuNav],
    builder: &mut NavBuilder<'_>,
) -> Result<Vec<Vec<Vec<NavCommand>>>, BdError> {
    let mut result = Vec::new();

    for menu in menus {
        let mut commands = Vec::new();
        for (index, link) in menu.buttons.iter().enumerate() {
            let mut button = Vec::new();
            // A page of a composition (M10.26): PAGE_GPR names this button, so the menu comes back to it (a title's
            // end, Return).
            if let Some(pages) = &menu.pages {
                button.push(NavCommand::set_reg(
                    PAGE_GPR,
                    page_value(pages.page, Some(index))?,
                ));
            }
            button.extend(menu.button_prefix.get(index).cloned().unwrap_or_default());
            let own = menu.button_overrides.get(index).and_then(Option::as_ref);
            button.extend(builder.set_override(own));
            match turn_page(menu, link, builder)? {
                Some(turn) => button.extend(turn),
                None => button.extend(link_commands(link, builder)?),
            }
            commands.push(button);
        }
        result.push(commands);
    }

    Ok(result)
}

/// A link from a page of a composition to a page of the same one (M10.26): the page turns, with the button the link
/// names (else the page's default) selected, and PAGE_GPR says so; `None` for any other link.
fn turn_page(
    menu: &MenuNav,
    link: &Link,
    builder: &NavBuilder<'_>,
) -> Result<Option<Vec<NavCommand>>, BdError> {
    let (Some(pages), Link::Menu { id, button }) = (&menu.pages, link) else {
        return Ok(None);
    };
    let Some(target) = builder.menu_index(id.0) else {
        return Ok(None);
    };
    let Some(Some(other)) = builder.menu_pages.get(target) else {
        return Ok(None);
    };
    if other.first != pages.first {
        return Ok(None);
    }
    let count = builder.menu_buttons.get(target).copied().unwrap_or(0);
    let button = button
        .and_then(|button| usize::try_from(button).ok())
        .filter(|button| *button < count);
    let turn = match button {
        Some(index) => NavCommand::set_button_page(
            u16::try_from(index).map_err(|_| invalid("too many buttons"))?,
            u16::from(other.page),
        ),
        None => NavCommand::set_page(u16::from(other.page)),
    };
    Ok(Some(vec![
        NavCommand::set_reg(PAGE_GPR, page_value(other.page, button)?),
        turn,
    ]))
}

fn finish_object(commands: Vec<NavCommand>) -> MovieObject {
    let commands = if commands.is_empty() {
        vec![NavCommand::nop()]
    } else {
        commands
    };

    MovieObject {
        resume_intention: false,
        menu_call_mask: false,
        title_search_mask: false,
        commands,
    }
}

fn index_titles(count: usize) -> Result<Vec<IndexTitle>, BdError> {
    let mut titles = Vec::new();

    for index in 0..count {
        let movie_object = u16::try_from(
            2_usize
                .checked_add(index)
                .ok_or_else(|| invalid("too many titles"))?,
        )
        .map_err(|_| invalid("too many titles"))?;
        titles.push(IndexTitle {
            movie_object,
            interactive: false,
        });
    }

    Ok(titles)
}

#[cfg(test)]
mod tests {
    use super::{
        BdError, MenuMotion, MenuNav, MenuPages, NavCommand, PAGE_GPR, TitleMap, TitleNav,
        build_navigation, build_navigation_with_top, invalid, page_dispatcher,
    };
    use disccraft_project::{Link, MenuId, TimelineId};
    use std::collections::BTreeMap;

    #[test]
    fn first_play_targets_title_two() -> Result<(), BdError> {
        let map = TitleMap {
            timelines: [(1_u64, 2_u16)].into_iter().collect(),
            slideshows: BTreeMap::new(),
            playlists: BTreeMap::new(),
            chapter_playlists: BTreeMap::new(),
            disc_age: None,
        };
        let titles = vec![
            TitleNav {
                end_override: None,
                parental: None,
                versions: Vec::new(),
                items: Vec::new(),
                playlist: 1,
                end_action: Link::Stop,
            },
            TitleNav {
                end_override: None,
                parental: None,
                versions: Vec::new(),
                items: Vec::new(),
                playlist: 2,
                end_action: Link::Stop,
            },
        ];

        let navigation = build_navigation(
            &Link::Timeline {
                id: TimelineId(1),
                chapter: 0,
            },
            &titles,
            &[],
            &map,
        )?;

        assert_eq!(navigation.first_playback, 0);
        assert_eq!(navigation.objects.len(), 4);
        let first = navigation
            .objects
            .first()
            .ok_or_else(|| invalid("no objects"))?;
        assert_eq!(
            first.commands.first().map(|command| command.0),
            Some(NavCommand::jump_title(2).0)
        );
        Ok(())
    }

    /// M6.555: a project playlist's movie object plays its items' titles in turn: an item with parental versions
    /// counts in GPR 5 the versions whose age the player's (PSR13) is below and plays the one it names, an item with
    /// a parental level asks it (below it, what plays in the title's place), and each plays its own playlist.
    #[test]
    fn a_project_playlist_plays_its_items_titles_in_turn() -> Result<(), BdError> {
        use disccraft_bdmv::mobj::PSR_OPERAND;
        use disccraft_project::PlaylistItem;
        let map = TitleMap {
            timelines: [(1_u64, 1_u16), (2, 2)].into_iter().collect(),
            slideshows: BTreeMap::new(),
            playlists: [(9_u64, 3_u16)].into_iter().collect(),
            chapter_playlists: BTreeMap::new(),
            disc_age: None,
        };
        let title = |playlist, parental, versions, items| TitleNav {
            end_override: None,
            parental,
            versions,
            items,
            playlist,
            end_action: Link::Stop,
        };
        let titles = vec![
            title(1, None, vec![(17, 1), (13, 5), (0, 6)], Vec::new()),
            title(2, Some((17, Link::NotSet)), Vec::new(), Vec::new()),
            title(
                0,
                None,
                Vec::new(),
                vec![
                    PlaylistItem::Timeline(TimelineId(1)),
                    PlaylistItem::Timeline(TimelineId(2)),
                ],
            ),
        ];
        let navigation = build_navigation(&Link::Playlist { id: 9 }, &titles, &[], &map)?;
        let object = navigation
            .titles
            .get(2)
            .and_then(|title| navigation.objects.get(usize::from(title.movie_object)))
            .ok_or_else(|| invalid("no playlist object"))?;
        let age = PSR_OPERAND | 13;
        let expected = [
            NavCommand::set_reg(5, 0),
            NavCommand::if_lt(age, 17),
            NavCommand::set_reg(5, 1),
            NavCommand::if_lt(age, 13),
            NavCommand::set_reg(5, 2),
            NavCommand::if_eq(5, 0),
            NavCommand::play_pl(1),
            NavCommand::if_eq(5, 1),
            NavCommand::play_pl(5),
            NavCommand::if_eq(5, 2),
            NavCommand::play_pl(6),
            NavCommand::if_lt(age, 17),
        ];
        assert_eq!(object.commands.get(..expected.len()), Some(&expected[..]));
        let jump = object
            .commands
            .get(expected.len())
            .ok_or_else(|| invalid("no jump"))?;
        assert_eq!(jump.0[..4], NavCommand::jump_object(0).0[..4]);
        assert_eq!(
            object.commands.get(expected.len() + 1),
            Some(&NavCommand::play_pl(2))
        );
        Ok(())
    }

    #[test]
    fn chapter_end_action_creates_extra_object() -> Result<(), BdError> {
        let map = TitleMap {
            timelines: [(1_u64, 1_u16)].into_iter().collect(),
            slideshows: BTreeMap::new(),
            playlists: BTreeMap::new(),
            chapter_playlists: BTreeMap::new(),
            disc_age: None,
        };
        let titles = vec![TitleNav {
            end_override: None,
            parental: None,
            versions: Vec::new(),
            items: Vec::new(),
            playlist: 10,
            end_action: Link::Timeline {
                id: TimelineId(1),
                chapter: 3,
            },
        }];

        let navigation = build_navigation(&Link::Stop, &titles, &[], &map)?;
        assert_eq!(navigation.objects.len(), 4);

        let extra = navigation
            .objects
            .get(3)
            .ok_or_else(|| invalid("missing object"))?;
        let expected = NavCommand::play_pl_mark(10, 3);

        // The chapter object plays from the mark, then runs the title's end action (itself again).
        assert_eq!(extra.commands.len(), 2);
        assert_eq!(
            extra.commands.first().map(|command| command.0),
            Some(expected.0)
        );
        assert_eq!(
            extra.commands.get(1).map(|command| command.0),
            Some(NavCommand::jump_object(3).0)
        );
        Ok(())
    }

    #[test]
    fn chapter_cycle_terminates() -> Result<(), BdError> {
        let map = TitleMap {
            timelines: [(1_u64, 1_u16), (2_u64, 2_u16)].into_iter().collect(),
            slideshows: BTreeMap::new(),
            playlists: BTreeMap::new(),
            chapter_playlists: BTreeMap::new(),
            disc_age: None,
        };
        let titles = vec![
            TitleNav {
                end_override: None,
                parental: None,
                versions: Vec::new(),
                items: Vec::new(),
                playlist: 1,
                end_action: Link::Timeline {
                    id: TimelineId(2),
                    chapter: 2,
                },
            },
            TitleNav {
                end_override: None,
                parental: None,
                versions: Vec::new(),
                items: Vec::new(),
                playlist: 2,
                end_action: Link::Timeline {
                    id: TimelineId(1),
                    chapter: 1,
                },
            },
        ];

        let navigation = build_navigation(&Link::Stop, &titles, &[], &map)?;
        assert_eq!(navigation.objects.len(), 6);
        Ok(())
    }

    #[test]
    fn title_button_chooses_top_menu() -> Result<(), BdError> {
        let map = TitleMap::default();
        let titles = vec![TitleNav {
            end_override: None,
            parental: None,
            versions: Vec::new(),
            items: Vec::new(),
            playlist: 1,
            end_action: Link::Stop,
        }];
        let menus = vec![
            MenuNav {
                end_override: None,
                button_overrides: Vec::new(),
                id: 7,
                playlist: 100,
                end_action: Link::NotSet,
                buttons: vec![Link::Stop],
                motion: None,
                button_prefix: Vec::new(),
                select: 0,
                pages: None,
            },
            MenuNav {
                end_override: None,
                button_overrides: Vec::new(),
                id: 9,
                playlist: 101,
                end_action: Link::NotSet,
                buttons: vec![Link::Stop],
                motion: None,
                button_prefix: Vec::new(),
                select: 0,
                pages: None,
            },
        ];
        let title_remote = Link::Menu {
            id: MenuId(9),
            button: None,
        };

        let navigation = build_navigation_with_top(
            (&Link::Stop, &[], None),
            &title_remote,
            &titles,
            &menus,
            &map,
        )?;
        let top = navigation
            .objects
            .get(usize::from(navigation.top_menu))
            .ok_or_else(|| invalid("missing top menu object"))?;
        // Objects: first play, top menu, one title, then the two menus (7 at 3, 9 at 4).
        assert_eq!(
            top.commands.first().map(|command| command.0),
            Some(NavCommand::jump_object(4).0)
        );

        let default_top = build_navigation(&Link::Stop, &titles, &menus, &map)?;
        let top = default_top
            .objects
            .get(usize::from(default_top.top_menu))
            .ok_or_else(|| invalid("missing top menu object"))?;
        assert_eq!(
            top.commands.first().map(|command| command.0),
            Some(NavCommand::jump_object(3).0)
        );
        Ok(())
    }

    #[test]
    fn empty_titles_is_error() {
        let map = TitleMap::default();
        assert!(build_navigation(&Link::Stop, &[], &[], &map).is_err());
    }

    #[test]
    fn menu_object_bytes() -> Result<(), BdError> {
        let map = TitleMap::default();
        let titles = vec![TitleNav {
            end_override: None,
            parental: None,
            versions: Vec::new(),
            items: Vec::new(),
            playlist: 1,
            end_action: Link::Stop,
        }];
        let menus = vec![MenuNav {
            end_override: None,
            button_overrides: Vec::new(),
            id: 7,
            playlist: 100,
            end_action: Link::NotSet,
            buttons: vec![Link::Stop],
            motion: None,
            button_prefix: Vec::new(),
            select: 0,
            pages: None,
        }];

        let navigation = build_navigation(&Link::Stop, &titles, &menus, &map)?;
        assert_eq!(navigation.objects.len(), 5);

        let object = navigation
            .objects
            .get(3)
            .ok_or_else(|| invalid("missing menu object"))?;
        // The menu value, its own button unless a link asked for one, then its playlist.
        let expected: Vec<[u8; 12]> = [
            NavCommand::set_reg(0, 1),
            NavCommand::if_eq(2, 0),
            NavCommand::set_button_page(0, 0),
            NavCommand::if_eq(2, 1),
            NavCommand::set_button_page(0, 0),
            NavCommand::set_reg(2, 0),
            NavCommand::play_pl(100),
        ]
        .iter()
        .map(|command| command.0)
        .collect();
        let got: Vec<[u8; 12]> = object.commands.iter().map(|command| command.0).collect();
        assert_eq!(got, expected);
        Ok(())
    }

    #[test]
    fn an_override_replaces_the_next_titles_end_action() -> Result<(), BdError> {
        // Title 1 plays title 2 when it ends, with the menu as override: after title 2, the menu.
        let mut map = TitleMap::default();
        let _ = map.timelines.insert(5, 1);
        let _ = map.timelines.insert(6, 2);
        let menu_link = Link::Menu {
            id: MenuId(7),
            button: None,
        };
        let titles = vec![
            TitleNav {
                playlist: 1,
                end_action: Link::Timeline {
                    id: TimelineId(6),
                    chapter: 0,
                },
                end_override: Some(menu_link.clone()),
                parental: None,
                versions: Vec::new(),
                items: Vec::new(),
            },
            TitleNav {
                playlist: 2,
                end_action: Link::Stop,
                end_override: None,
                parental: None,
                versions: Vec::new(),
                items: Vec::new(),
            },
        ];
        let menus = vec![MenuNav {
            id: 7,
            playlist: 100,
            end_action: Link::NotSet,
            buttons: vec![Link::Stop],
            motion: None,
            button_prefix: Vec::new(),
            end_override: None,
            button_overrides: Vec::new(),
            select: 0,
            pages: None,
        }];
        let navigation = build_navigation(&Link::Stop, &titles, &menus, &map)?;
        let object = |number: usize| {
            navigation
                .objects
                .get(number)
                .map(|object| object.commands.clone())
                .ok_or_else(|| invalid("missing object"))
        };
        // Objects: first play, top menu, titles 1 and 2, the menu, the dispatcher, then the override's object.
        let (first_title, second_title, menu) = (object(2)?, object(3)?, object(4)?);
        let follow = object(6)?;
        let check = [NavCommand::if_eq(3, 1), NavCommand::jump_object(6)];
        assert!(first_title.starts_with(&[NavCommand::play_pl(1)]));
        assert_eq!(first_title.get(1..3), Some(&check[..]));
        assert_eq!(
            first_title.get(3..),
            Some(&[NavCommand::set_reg(3, 1), NavCommand::jump_title(2)][..])
        );
        assert_eq!(second_title.get(1..3), Some(&check[..]));
        assert_eq!(
            follow,
            vec![NavCommand::set_reg(3, 0), NavCommand::jump_object(4)]
        );
        assert!(menu.contains(&NavCommand::set_reg(3, 0)));
        Ok(())
    }

    #[test]
    fn streams_are_chosen_before_the_first_play_and_a_buttons_link() -> Result<(), BdError> {
        let mut map = TitleMap::default();
        let _ = map.timelines.insert(5, 1);
        let titles = vec![TitleNav {
            end_override: None,
            parental: None,
            versions: Vec::new(),
            items: Vec::new(),
            playlist: 1,
            end_action: Link::Stop,
        }];
        let play = Link::Timeline {
            id: TimelineId(5),
            chapter: 0,
        };
        let audio_two = NavCommand::set_stream(Some(2), None);
        let menus = vec![MenuNav {
            end_override: None,
            button_overrides: Vec::new(),
            id: 7,
            playlist: 100,
            end_action: Link::NotSet,
            buttons: vec![play.clone(), play.clone()],
            motion: None,
            button_prefix: vec![vec![audio_two], Vec::new()],
            select: 0,
            pages: None,
        }];
        let subtitles_off = NavCommand::set_stream(None, Some((false, 1)));
        let navigation = build_navigation_with_top(
            (&play, &[subtitles_off], None),
            &Link::NotSet,
            &titles,
            &menus,
            &map,
        )?;
        let first = navigation
            .objects
            .first()
            .ok_or_else(|| invalid("missing first play"))?;
        assert_eq!(first.commands.first(), Some(&subtitles_off));
        assert_eq!(first.commands.get(1), Some(&NavCommand::jump_title(1)));
        let buttons = navigation
            .button_commands
            .first()
            .ok_or_else(|| invalid("missing buttons"))?;
        assert_eq!(
            buttons.first(),
            Some(&vec![audio_two, NavCommand::jump_title(1)])
        );
        assert_eq!(buttons.get(1), Some(&vec![NavCommand::jump_title(1)]));
        Ok(())
    }

    #[test]
    fn a_link_to_a_menus_button_asks_the_menu_to_select_it() -> Result<(), BdError> {
        let map = TitleMap::default();
        let titles = vec![TitleNav {
            end_override: None,
            parental: None,
            versions: Vec::new(),
            items: Vec::new(),
            playlist: 1,
            end_action: Link::Stop,
        }];
        let to = |button: Option<u32>| Link::Menu {
            id: MenuId(9),
            button,
        };
        let menus = vec![
            MenuNav {
                end_override: None,
                button_overrides: Vec::new(),
                id: 7,
                playlist: 100,
                end_action: Link::NotSet,
                // To button 2 of the second menu, to its own choice, and to a button it does not have.
                buttons: vec![to(Some(1)), to(None), to(Some(5))],
                motion: None,
                button_prefix: Vec::new(),
                select: 0,
                pages: None,
            },
            MenuNav {
                end_override: None,
                button_overrides: Vec::new(),
                id: 9,
                playlist: 101,
                end_action: Link::NotSet,
                buttons: vec![Link::Stop, Link::Stop],
                motion: None,
                button_prefix: Vec::new(),
                select: 0,
                pages: None,
            },
        ];
        let navigation = build_navigation(&Link::Stop, &titles, &menus, &map)?;
        let commands = navigation
            .button_commands
            .first()
            .ok_or_else(|| invalid("missing button commands"))?;
        let bytes = |index: usize| -> Vec<[u8; 12]> {
            commands
                .get(index)
                .map(|list| list.iter().map(|command| command.0).collect())
                .unwrap_or_default()
        };
        let jump = bytes(1);
        assert_eq!(jump.len(), 1);
        let mut expected = vec![NavCommand::set_reg(2, 2).0];
        expected.extend(jump.iter().copied());
        assert_eq!(bytes(0), expected);
        assert_eq!(bytes(2), jump);
        Ok(())
    }

    #[test]
    fn a_menu_with_an_end_action_goes_on_after_its_playlist() -> Result<(), BdError> {
        let mut map = TitleMap::default();
        let _ = map.timelines.insert(5, 1);
        let titles = vec![TitleNav {
            end_override: None,
            parental: None,
            versions: Vec::new(),
            items: Vec::new(),
            playlist: 1,
            end_action: Link::Stop,
        }];
        let menus = vec![MenuNav {
            end_override: None,
            button_overrides: Vec::new(),
            id: 7,
            playlist: 100,
            end_action: Link::Timeline {
                id: disccraft_project::TimelineId(5),
                chapter: 0,
            },
            buttons: vec![Link::Stop],
            motion: None,
            button_prefix: Vec::new(),
            select: 0,
            pages: None,
        }];
        let navigation = build_navigation(&Link::Stop, &titles, &menus, &map)?;
        let object = navigation
            .objects
            .get(3)
            .ok_or_else(|| invalid("missing menu object"))?;
        assert!(object.commands.len() > 7, "the end action follows PlayPL");
        assert_eq!(
            object.commands.get(6).map(|command| command.0),
            Some(NavCommand::play_pl(100).0)
        );
        Ok(())
    }

    #[test]
    fn a_motion_menu_loops_its_playlist_from_the_loop_point() -> Result<(), BdError> {
        // It selects its button itself; the later plays keep the viewer's button.
        let mut map = TitleMap::default();
        let _ = map.timelines.insert(5, 1);
        let titles = vec![TitleNav {
            end_override: None,
            parental: None,
            versions: Vec::new(),
            items: Vec::new(),
            playlist: 1,
            end_action: Link::Stop,
        }];
        let menu = |end_action: Link, motion: MenuMotion| MenuNav {
            end_override: None,
            button_overrides: Vec::new(),
            id: 7,
            playlist: 100,
            end_action,
            buttons: vec![Link::Stop],
            motion: Some(motion),
            button_prefix: Vec::new(),
            select: 2,
            pages: None,
        };
        let commands_of = |menus: &[MenuNav]| -> Result<Vec<[u8; 12]>, BdError> {
            let navigation = build_navigation(&Link::Stop, &titles, menus, &map)?;
            let object = navigation
                .objects
                .get(3)
                .ok_or_else(|| invalid("missing menu object"))?;
            Ok(object.commands.iter().map(|command| command.0).collect())
        };
        // Forever: play, then from the loop point's mark again and again.
        let forever = commands_of(&[menu(
            Link::NotSet,
            MenuMotion {
                plays: None,
                loop_mark: true,
            },
        )])?;
        // The menu selects button 3 (index 2) unless a link asked for another.
        let select = [
            NavCommand::set_reg(0, 1),
            NavCommand::if_eq(2, 0),
            NavCommand::set_button_page(2, 0),
            NavCommand::if_eq(2, 1),
            NavCommand::set_button_page(0, 0),
            NavCommand::set_reg(2, 0),
        ];
        let expected: Vec<[u8; 12]> = select
            .iter()
            .chain(&[
                NavCommand::play_pl(100),
                NavCommand::play_pl_mark(100, 1),
                NavCommand::goto_line(7),
            ])
            .map(|command| command.0)
            .collect();
        assert_eq!(forever, expected);
        // Three plays, then the end action (the timeline's title).
        let end = Link::Timeline {
            id: disccraft_project::TimelineId(5),
            chapter: 0,
        };
        let three = commands_of(&[menu(
            end,
            MenuMotion {
                plays: Some(3),
                loop_mark: false,
            },
        )])?;
        let expected: Vec<[u8; 12]> = select
            .iter()
            .chain(&[
                NavCommand::set_reg(1, 2),
                NavCommand::play_pl(100),
                NavCommand::if_eq(1, 0),
                NavCommand::goto_line(13),
                NavCommand::sub_reg(1, 1),
                NavCommand::play_pl(100),
                NavCommand::goto_line(8),
                NavCommand::jump_title(1),
            ])
            .map(|command| command.0)
            .collect();
        assert_eq!(three, expected);
        Ok(())
    }

    #[test]
    fn dispatcher_order() -> Result<(), BdError> {
        let map = TitleMap::default();
        let titles = vec![TitleNav {
            end_override: None,
            parental: None,
            versions: Vec::new(),
            items: Vec::new(),
            playlist: 1,
            end_action: Link::Stop,
        }];
        let menus = vec![
            MenuNav {
                end_override: None,
                button_overrides: Vec::new(),
                id: 10,
                playlist: 10,
                end_action: Link::NotSet,
                buttons: vec![Link::Stop],
                motion: None,
                button_prefix: Vec::new(),
                select: 0,
                pages: None,
            },
            MenuNav {
                end_override: None,
                button_overrides: Vec::new(),
                id: 20,
                playlist: 20,
                end_action: Link::NotSet,
                buttons: vec![Link::Stop],
                motion: None,
                button_prefix: Vec::new(),
                select: 0,
                pages: None,
            },
        ];

        let navigation = build_navigation(&Link::Stop, &titles, &menus, &map)?;
        assert_eq!(navigation.objects.len(), 6);

        let dispatcher = navigation
            .objects
            .get(5)
            .ok_or_else(|| invalid("missing dispatcher"))?;
        assert_eq!(dispatcher.commands.len(), 5);
        assert_eq!(
            dispatcher.commands.first().map(|command| command.0),
            Some(NavCommand::if_eq(0, 1).0)
        );
        assert_eq!(
            dispatcher.commands.get(1).map(|command| command.0),
            Some(NavCommand::jump_object(3).0)
        );
        assert_eq!(
            dispatcher.commands.get(2).map(|command| command.0),
            Some(NavCommand::if_eq(0, 2).0)
        );
        assert_eq!(
            dispatcher.commands.get(3).map(|command| command.0),
            Some(NavCommand::jump_object(4).0)
        );
        assert_eq!(
            dispatcher.commands.get(4).map(|command| command.0),
            Some(NavCommand::jump_object(3).0)
        );
        Ok(())
    }

    #[test]
    fn title_last_menu_jumps_to_dispatcher() -> Result<(), BdError> {
        let map = TitleMap::default();
        let titles = vec![TitleNav {
            end_override: None,
            parental: None,
            versions: Vec::new(),
            items: Vec::new(),
            playlist: 1,
            end_action: Link::LastMenu,
        }];
        let menus = vec![MenuNav {
            end_override: None,
            button_overrides: Vec::new(),
            id: 7,
            playlist: 100,
            end_action: Link::NotSet,
            buttons: vec![Link::Stop],
            motion: None,
            button_prefix: Vec::new(),
            select: 0,
            pages: None,
        }];

        let navigation = build_navigation(&Link::Stop, &titles, &menus, &map)?;
        assert_eq!(navigation.objects.len(), 5);

        let title_object = navigation
            .objects
            .get(2)
            .ok_or_else(|| invalid("missing title object"))?;
        assert_eq!(title_object.commands.len(), 2);
        assert_eq!(
            title_object.commands.first().map(|command| command.0),
            Some(NavCommand::play_pl(1).0)
        );
        assert_eq!(
            title_object.commands.get(1).map(|command| command.0),
            Some(NavCommand::jump_object(4).0)
        );
        Ok(())
    }

    /// M6.528: a menu button's Resume is Branch_Resume, and with a menu that resumes the titles' movie objects have
    /// the resume intention (the Top Menu key suspends the title); without one, they do not (discs as before).
    #[test]
    fn resume_goes_back_to_the_suspended_title() -> Result<(), BdError> {
        let map = TitleMap {
            timelines: [(1_u64, 1_u16)].into_iter().collect(),
            slideshows: BTreeMap::new(),
            playlists: BTreeMap::new(),
            chapter_playlists: BTreeMap::new(),
            disc_age: None,
        };
        let titles = vec![TitleNav {
            end_override: None,
            parental: None,
            versions: Vec::new(),
            items: Vec::new(),
            playlist: 10,
            end_action: Link::Stop,
        }];
        let menu = |buttons: Vec<Link>| MenuNav {
            end_override: None,
            button_overrides: Vec::new(),
            id: 7,
            playlist: 20,
            end_action: Link::NotSet,
            buttons,
            motion: None,
            button_prefix: Vec::new(),
            select: 0,
            pages: None,
        };
        let resuming = vec![menu(vec![
            Link::Resume,
            Link::Timeline {
                id: TimelineId(1),
                chapter: 0,
            },
        ])];
        let navigation = build_navigation(&Link::Stop, &titles, &resuming, &map)?;
        let first = navigation
            .button_commands
            .first()
            .and_then(|commands| commands.first())
            .ok_or_else(|| invalid("missing button commands"))?;
        assert_eq!(first, &vec![NavCommand::resume()]);
        // Title 1's movie object (after the first playback and the top menu).
        let title = navigation
            .objects
            .get(2)
            .ok_or_else(|| invalid("missing title object"))?;
        assert!(title.resume_intention);

        let plain = vec![menu(vec![Link::Timeline {
            id: TimelineId(1),
            chapter: 0,
        }])];
        let navigation = build_navigation(&Link::Stop, &titles, &plain, &map)?;
        assert!(
            navigation
                .objects
                .iter()
                .all(|object| !object.resume_intention)
        );
        Ok(())
    }

    #[test]
    fn button_commands_for_links() -> Result<(), BdError> {
        let map = TitleMap {
            timelines: [(1_u64, 1_u16)].into_iter().collect(),
            slideshows: BTreeMap::new(),
            playlists: BTreeMap::new(),
            chapter_playlists: BTreeMap::new(),
            disc_age: None,
        };
        let titles = vec![TitleNav {
            end_override: None,
            parental: None,
            versions: Vec::new(),
            items: Vec::new(),
            playlist: 10,
            end_action: Link::Stop,
        }];
        let menus = vec![MenuNav {
            end_override: None,
            button_overrides: Vec::new(),
            id: 7,
            playlist: 20,
            end_action: Link::NotSet,
            buttons: vec![
                Link::Timeline {
                    id: TimelineId(1),
                    chapter: 0,
                },
                Link::Timeline {
                    id: TimelineId(1),
                    chapter: 2,
                },
                Link::Menu {
                    id: MenuId(7),
                    button: None,
                },
            ],
            motion: None,
            button_prefix: Vec::new(),
            select: 0,
            pages: None,
        }];

        let navigation = build_navigation(&Link::Stop, &titles, &menus, &map)?;
        assert_eq!(navigation.objects.len(), 6);
        assert_eq!(navigation.button_commands.len(), 1);

        let menu_commands = navigation
            .button_commands
            .first()
            .ok_or_else(|| invalid("missing menu commands"))?;
        assert_eq!(menu_commands.len(), 3);

        let first = menu_commands
            .first()
            .ok_or_else(|| invalid("missing first button commands"))?;
        assert_eq!(
            first.first().map(|command| command.0),
            Some(NavCommand::jump_title(1).0)
        );

        let second = menu_commands
            .get(1)
            .ok_or_else(|| invalid("missing second button commands"))?;
        assert_eq!(
            second.first().map(|command| command.0),
            Some(NavCommand::jump_object(5).0)
        );

        let third = menu_commands
            .get(2)
            .ok_or_else(|| invalid("missing third button commands"))?;
        assert_eq!(
            third.first().map(|command| command.0),
            Some(NavCommand::jump_object(3).0)
        );
        Ok(())
    }

    #[test]
    fn first_play_not_set_targets_first_menu() -> Result<(), BdError> {
        let map = TitleMap::default();
        let titles = vec![TitleNav {
            end_override: None,
            parental: None,
            versions: Vec::new(),
            items: Vec::new(),
            playlist: 1,
            end_action: Link::Stop,
        }];
        let menus = vec![MenuNav {
            end_override: None,
            button_overrides: Vec::new(),
            id: 7,
            playlist: 100,
            end_action: Link::NotSet,
            buttons: vec![Link::Stop],
            motion: None,
            button_prefix: Vec::new(),
            select: 0,
            pages: None,
        }];

        let navigation = build_navigation(&Link::NotSet, &titles, &menus, &map)?;
        assert_eq!(navigation.objects.len(), 5);

        let first = navigation
            .objects
            .first()
            .ok_or_else(|| invalid("missing first object"))?;
        assert_eq!(
            first.commands.first().map(|command| command.0),
            Some(NavCommand::jump_object(3).0)
        );
        Ok(())
    }

    /// M10.26: a menu whose two pages are one composition. Its first page's object makes PAGE_GPR a page and button
    /// the composition has (page 1 when not), plays the menu and, a looping motion menu, puts the page and button on
    /// screen back into PAGE_GPR before each later play; every GoTo lands in the object. The second page's object
    /// goes to the first's on page 2. A link to the other page turns it (no jump); a link from a title to a page
    /// names it in PAGE_GPR. Page 0's dispatcher sets each page and button with immediate operands.
    #[test]
    fn a_menus_pages_in_one_composition() -> Result<(), BdError> {
        use disccraft_bdmv::mobj::PSR_OPERAND;
        let map = TitleMap {
            timelines: [(1_u64, 1_u16)].into_iter().collect(),
            ..TitleMap::default()
        };
        let titles = vec![TitleNav {
            playlist: 1,
            end_action: Link::Menu {
                id: MenuId(8),
                button: Some(0),
            },
            end_override: None,
            parental: None,
            versions: Vec::new(),
            items: Vec::new(),
        }];
        let page = |id: u64, page: u8, buttons: Vec<Link>, motion: Option<MenuMotion>| MenuNav {
            end_override: None,
            button_overrides: Vec::new(),
            id,
            playlist: 100,
            end_action: Link::NotSet,
            buttons,
            motion,
            button_prefix: Vec::new(),
            select: 0,
            pages: Some(MenuPages {
                first: 0,
                page,
                buttons: vec![2, 1],
            }),
        };
        let menus = vec![
            page(
                7,
                1,
                vec![
                    Link::Timeline {
                        id: TimelineId(1),
                        chapter: 0,
                    },
                    Link::Menu {
                        id: MenuId(8),
                        button: None,
                    },
                ],
                Some(MenuMotion {
                    plays: None,
                    loop_mark: true,
                }),
            ),
            page(
                8,
                2,
                vec![Link::Menu {
                    id: MenuId(7),
                    button: Some(1),
                }],
                None,
            ),
        ];
        let navigation = build_navigation(&Link::Stop, &titles, &menus, &map)?;
        // Objects: first play, top menu, title 1, the two pages, the dispatcher.
        let first = navigation
            .objects
            .get(3)
            .ok_or_else(|| invalid("missing menu object"))?;
        let commands: Vec<[u8; 12]> = first.commands.iter().map(|command| command.0).collect();
        let count = u32::try_from(commands.len()).map_err(|_| invalid("too many"))?;
        for command in &first.commands {
            if command.opcode() == NavCommand::goto_line(0).opcode() {
                assert!(command.dst() < count, "GoTo {} of {count}", command.dst());
            }
        }
        let position =
            |wanted: NavCommand| commands.iter().position(|command| *command == wanted.0);
        // Checked against every value (page 1, its two buttons, page 2, its button), then page 1.
        for value in [0x100, 0x101, 0x102, 0x200, 0x201] {
            assert!(
                position(NavCommand::if_eq(PAGE_GPR, value)).is_some(),
                "{value:#x}"
            );
        }
        assert!(position(NavCommand::set_reg(PAGE_GPR, 0x100)).is_some());
        let play = position(NavCommand::play_pl(100)).ok_or_else(|| invalid("no play"))?;
        let restore = position(NavCommand::move_reg(PAGE_GPR, PSR_OPERAND | 11))
            .ok_or_else(|| invalid("no restore"))?;
        let again =
            position(NavCommand::play_pl_mark(100, 1)).ok_or_else(|| invalid("no later play"))?;
        assert!(play < restore && restore < again);
        // The second page's object: page 2, then the first page's object.
        let second = navigation
            .objects
            .get(4)
            .ok_or_else(|| invalid("missing page object"))?;
        assert_eq!(
            second
                .commands
                .iter()
                .map(|command| command.0)
                .collect::<Vec<_>>(),
            vec![
                NavCommand::set_reg(PAGE_GPR, 0x200).0,
                NavCommand::jump_object(3).0
            ]
        );
        // Buttons: each names itself first; Next turns to page 2 (its default button), Back to page 1's second
        // button.
        let buttons = &navigation.button_commands;
        assert_eq!(
            buttons.first().and_then(|page| page.get(1)).cloned(),
            Some(vec![
                NavCommand::set_reg(PAGE_GPR, 0x102),
                NavCommand::set_reg(PAGE_GPR, 0x200),
                NavCommand::set_page(2),
            ])
        );
        assert_eq!(
            buttons.get(1).and_then(|page| page.first()).cloned(),
            Some(vec![
                NavCommand::set_reg(PAGE_GPR, 0x201),
                NavCommand::set_reg(PAGE_GPR, 0x102),
                NavCommand::set_button_page(1, 1),
            ])
        );
        // The title's end: page 2, its first button, through the first page's object.
        let title = navigation
            .objects
            .get(2)
            .ok_or_else(|| invalid("missing title object"))?;
        assert!(
            title
                .commands
                .contains(&NavCommand::set_reg(PAGE_GPR, 0x201))
        );
        assert!(title.commands.contains(&NavCommand::jump_object(3)));
        // Page 0's dispatcher.
        assert_eq!(
            page_dispatcher(&[2, 1])?,
            vec![
                NavCommand::if_eq(PAGE_GPR, 0x100),
                NavCommand::set_page(1),
                NavCommand::if_eq(PAGE_GPR, 0x101),
                NavCommand::set_button_page(0, 1),
                NavCommand::if_eq(PAGE_GPR, 0x102),
                NavCommand::set_button_page(1, 1),
                NavCommand::if_eq(PAGE_GPR, 0x200),
                NavCommand::set_page(2),
                NavCommand::if_eq(PAGE_GPR, 0x201),
                NavCommand::set_button_page(0, 2),
            ]
        );
        Ok(())
    }
}
