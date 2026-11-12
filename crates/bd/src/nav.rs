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
}

/// Navigation information for one generated menu.
pub(crate) struct MenuNav {
    /// Menu id.
    pub(crate) id: u64,
    /// Playlist number played by this menu.
    pub(crate) playlist: u32,
    /// Links for menu buttons.
    pub(crate) buttons: Vec<Link>,
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
    build_navigation_with_top(first_play, &Link::NotSet, titles, menus, map)
}

/// Build index and movie-object navigation. `title_remote` (the disc's Title button) chooses the menu the
/// remote's Top Menu key opens; when it is not a menu, the first menu is used.
pub(crate) fn build_navigation_with_top(
    first_play: &Link,
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
    let mut builder = NavBuilder {
        titles,
        map,
        base: counts.chapter_base,
        extra: Vec::new(),
        memo: BTreeMap::new(),
        menu_ids,
        menu_base: counts.menu_base,
        dispatcher: counts.dispatcher,
    };

    let links = DiscLinks {
        first_play,
        title_remote,
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

        let (playlist, end_action) = {
            let title_nav = self
                .titles
                .get(title_index)
                .ok_or_else(|| invalid("title does not exist"))?;
            (title_nav.playlist, title_nav.end_action.clone())
        };

        let mut commands = vec![NavCommand::play_pl_mark(playlist, chapter)];
        commands.extend(link_commands(&end_action, self)?);
        Ok(commands)
    }
}

/// The disc-level links: First Play and the Title button.
struct DiscLinks<'a> {
    first_play: &'a Link,
    title_remote: &'a Link,
}

fn build_objects(
    links: &DiscLinks<'_>,
    titles: &[TitleNav],
    menus: &[MenuNav],
    builder: &mut NavBuilder<'_>,
) -> Result<Vec<MovieObject>, BdError> {
    let mut objects = Vec::new();

    objects.push(finish_object(first_play_commands(
        links.first_play,
        builder,
    )?));
    objects.push(finish_object(top_menu_commands(
        links.title_remote,
        builder,
    )?));

    for title in titles {
        objects.push(title_object(title, builder)?);
    }

    for (index, menu) in menus.iter().enumerate() {
        objects.push(menu_object(menu, index)?);
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
        Link::Menu { id, .. } => {
            let commands = menu_commands(id.0, builder)?;
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
    if let Link::Menu { id, .. } = title_remote
        && let Some(index) = builder.menu_index(id.0)
    {
        let object = builder.menu_object(index)?;
        return Ok(vec![NavCommand::jump_object(u32::from(object))]);
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
    commands.extend(link_commands(&title.end_action, builder)?);
    Ok(finish_object(commands))
}

fn link_commands(link: &Link, builder: &mut NavBuilder<'_>) -> Result<Vec<NavCommand>, BdError> {
    match link {
        Link::Timeline { id, chapter } => timeline_commands(*chapter, id.0, builder),
        Link::Playlist { id } => Ok(playlist_commands(*id, builder)),
        Link::ChapterPlaylist { id } => Ok(chapter_playlist_commands(*id, builder)),
        Link::Slideshow { id } => Ok(slideshow_commands(*id, builder)),
        Link::Menu { id, .. } => menu_commands(id.0, builder),
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

fn menu_commands(id: u64, builder: &NavBuilder<'_>) -> Result<Vec<NavCommand>, BdError> {
    match builder.menu_index(id) {
        Some(index) => {
            let object = builder.menu_object(index)?;
            Ok(vec![NavCommand::jump_object(u32::from(object))])
        }
        None => Ok(Vec::new()),
    }
}

fn last_menu_commands(builder: &NavBuilder<'_>) -> Result<Vec<NavCommand>, BdError> {
    match builder.dispatcher {
        Some(object) => Ok(vec![NavCommand::jump_object(u32::from(object))]),
        None => Ok(Vec::new()),
    }
}

fn menu_object(menu: &MenuNav, index: usize) -> Result<MovieObject, BdError> {
    let value = menu_value(index)?;
    let commands = vec![
        NavCommand::set_reg(0, u32::from(value)),
        NavCommand::play_pl(menu.playlist),
    ];
    Ok(finish_object(commands))
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
        for link in &menu.buttons {
            commands.push(link_commands(link, builder)?);
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
        BdError, MenuNav, NavCommand, TitleMap, TitleNav, build_navigation,
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
                playlist: 1,
                end_action: Link::Stop,
            },
            TitleNav {
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
                playlist: 1,
                end_action: Link::Timeline {
                    id: TimelineId(2),
                    chapter: 2,
                },
            },
            TitleNav {
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
            playlist: 1,
            end_action: Link::Stop,
        }];
        let menus = vec![
            MenuNav {
                id: 7,
                playlist: 100,
                buttons: vec![Link::Stop],
            },
            MenuNav {
                id: 9,
                playlist: 101,
                buttons: vec![Link::Stop],
            },
        ];
        let title_remote = Link::Menu {
            id: MenuId(9),
            button: None,
        };

        let navigation =
            build_navigation_with_top(&Link::Stop, &title_remote, &titles, &menus, &map)?;
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
            playlist: 1,
            end_action: Link::Stop,
        }];
        let menus = vec![MenuNav {
            id: 7,
            playlist: 100,
            buttons: vec![Link::Stop],
        }];

        let navigation = build_navigation(&Link::Stop, &titles, &menus, &map)?;
        assert_eq!(navigation.objects.len(), 5);

        let object = navigation
            .objects
            .get(3)
            .ok_or_else(|| invalid("missing menu object"))?;
        assert_eq!(object.commands.len(), 2);
        assert_eq!(
            object.commands.first().map(|command| command.0),
            Some(NavCommand::set_reg(0, 1).0)
        );
        assert_eq!(
            object.commands.get(1).map(|command| command.0),
            Some(NavCommand::play_pl(100).0)
        );
        Ok(())
    }

    #[test]
    fn dispatcher_order() -> Result<(), BdError> {
        let map = TitleMap::default();
        let titles = vec![TitleNav {
            playlist: 1,
            end_action: Link::Stop,
        }];
        let menus = vec![
            MenuNav {
                id: 10,
                playlist: 10,
                buttons: vec![Link::Stop],
            },
            MenuNav {
                id: 20,
                playlist: 20,
                buttons: vec![Link::Stop],
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
            playlist: 1,
            end_action: Link::LastMenu,
        }];
        let menus = vec![MenuNav {
            id: 7,
            playlist: 100,
            buttons: vec![Link::Stop],
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
            playlist: 10,
            end_action: Link::Stop,
        }];
        let menus = vec![MenuNav {
            id: 7,
            playlist: 20,
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
            playlist: 1,
            end_action: Link::Stop,
        }];
        let menus = vec![MenuNav {
            id: 7,
            playlist: 100,
            buttons: vec![Link::Stop],
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
