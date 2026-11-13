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
        menu_base: counts.menu_base,
        dispatcher: counts.dispatcher,
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
    menu_base: u16,
    dispatcher: Option<u16>,
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

        let (playlist, end_action, end_override) = {
            let title_nav = self
                .titles
                .get(title_index)
                .ok_or_else(|| invalid("title does not exist"))?;
            (
                title_nav.playlist,
                title_nav.end_action.clone(),
                title_nav.end_override.clone(),
            )
        };

        let mut commands = vec![NavCommand::play_pl_mark(playlist, chapter)];
        commands.extend(self.title_end_commands(&end_action, end_override.as_ref())?);
        Ok(commands)
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

    let mut first = links.first_streams.to_vec();
    first.extend(builder.set_override(links.first_override));
    first.extend(first_play_commands(links.first_play, builder)?);
    objects.push(finish_object(first));
    objects.push(finish_object(top_menu_commands(
        links.title_remote,
        builder,
    )?));

    for title in titles {
        objects.push(title_object(title, builder)?);
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
        Link::NotSet | Link::LastMenu => menu_or_title_commands(builder),
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

fn title_object(title: &TitleNav, builder: &mut NavBuilder<'_>) -> Result<MovieObject, BdError> {
    let mut commands = vec![NavCommand::play_pl(title.playlist)];
    commands.extend(builder.title_end_commands(&title.end_action, title.end_override.as_ref())?);
    Ok(finish_object(commands))
}

fn link_commands(link: &Link, builder: &mut NavBuilder<'_>) -> Result<Vec<NavCommand>, BdError> {
    match link {
        Link::Timeline { id, chapter } => timeline_commands(*chapter, id.0, builder),
        Link::Playlist { id } => Ok(playlist_commands(*id, builder)),
        Link::ChapterPlaylist { id } => Ok(chapter_playlist_commands(*id, builder)),
        Link::Slideshow { id } => Ok(slideshow_commands(*id, builder)),
        Link::Menu { id, button } => menu_commands(id.0, *button, builder),
        Link::LastMenu => last_menu_commands(builder),
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
    let object = builder.menu_object(index)?;
    let mut commands = Vec::new();
    let count = builder.menu_buttons.get(index).copied().unwrap_or(0);
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
            let mut button = menu.button_prefix.get(index).cloned().unwrap_or_default();
            let own = menu.button_overrides.get(index).and_then(Option::as_ref);
            button.extend(builder.set_override(own));
            button.extend(link_commands(link, builder)?);
            commands.push(button);
        }
        result.push(commands);
    }

    Ok(result)
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
        BdError, MenuMotion, MenuNav, NavCommand, TitleMap, TitleNav, build_navigation,
        build_navigation_with_top, invalid,
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
        };
        let titles = vec![
            TitleNav {
                end_override: None,
                playlist: 1,
                end_action: Link::Stop,
            },
            TitleNav {
                end_override: None,
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

    #[test]
    fn chapter_end_action_creates_extra_object() -> Result<(), BdError> {
        let map = TitleMap {
            timelines: [(1_u64, 1_u16)].into_iter().collect(),
            slideshows: BTreeMap::new(),
            playlists: BTreeMap::new(),
            chapter_playlists: BTreeMap::new(),
        };
        let titles = vec![TitleNav {
            end_override: None,
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
        };
        let titles = vec![
            TitleNav {
                end_override: None,
                playlist: 1,
                end_action: Link::Timeline {
                    id: TimelineId(2),
                    chapter: 2,
                },
            },
            TitleNav {
                end_override: None,
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
            },
            TitleNav {
                playlist: 2,
                end_action: Link::Stop,
                end_override: None,
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

    #[test]
    fn button_commands_for_links() -> Result<(), BdError> {
        let map = TitleMap {
            timelines: [(1_u64, 1_u16)].into_iter().collect(),
            slideshows: BTreeMap::new(),
            playlists: BTreeMap::new(),
            chapter_playlists: BTreeMap::new(),
        };
        let titles = vec![TitleNav {
            end_override: None,
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
}
