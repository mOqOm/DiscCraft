use crate::ids::{MenuId, TimelineId};
use serde::{Deserialize, Serialize};

/// A navigation target used by first play, end actions, buttons, and links.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(try_from = "LinkRepr", into = "LinkRepr")]
pub enum Link {
    /// No target has been assigned.
    #[default]
    NotSet,
    /// Stop playback.
    Stop,
    /// Return to the last menu.
    LastMenu,
    /// A chapter in a timeline.
    Timeline {
        /// Target timeline id.
        id: TimelineId,
        /// Chapter index, defaulting to zero.
        chapter: u32,
    },
    /// A menu, optionally with a selected button.
    Menu {
        /// Target menu id.
        id: MenuId,
        /// Optional button index.
        button: Option<u32>,
    },
    /// A playlist.
    Playlist {
        /// Playlist id.
        id: u64,
    },
    /// A chapter playlist.
    ChapterPlaylist {
        /// Chapter playlist id.
        id: u64,
    },
    /// A slideshow.
    Slideshow {
        /// Slideshow id.
        id: u64,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum LinkRepr {
    Name(String),
    Obj(LinkObj),
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LinkObj {
    #[serde(skip_serializing_if = "Option::is_none")]
    timeline: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    chapter: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    menu: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    button: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    playlist: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    chapter_playlist: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    slideshow: Option<u64>,
}

impl TryFrom<LinkRepr> for Link {
    type Error = String;

    fn try_from(repr: LinkRepr) -> Result<Self, Self::Error> {
        match repr {
            LinkRepr::Name(name) => match name.as_str() {
                "notSet" => Ok(Link::NotSet),
                "stop" => Ok(Link::Stop),
                "lastMenu" => Ok(Link::LastMenu),
                _ => Err(format!("unknown link name `{name}`")),
            },
            LinkRepr::Obj(obj) => {
                let target_count = usize::from(obj.timeline.is_some())
                    + usize::from(obj.menu.is_some())
                    + usize::from(obj.playlist.is_some())
                    + usize::from(obj.chapter_playlist.is_some())
                    + usize::from(obj.slideshow.is_some());
                if target_count != 1 {
                    return Err("exactly one link target key must be present".to_string());
                }
                if obj.chapter.is_some() && obj.timeline.is_none() {
                    return Err("chapter requires timeline".to_string());
                }
                if obj.button.is_some() && obj.menu.is_none() {
                    return Err("button requires menu".to_string());
                }

                if let Some(id) = obj.timeline {
                    return Ok(Link::Timeline {
                        id: TimelineId(id),
                        chapter: obj.chapter.unwrap_or(0),
                    });
                }
                if let Some(id) = obj.menu {
                    return Ok(Link::Menu {
                        id: MenuId(id),
                        button: obj.button,
                    });
                }
                if let Some(id) = obj.playlist {
                    return Ok(Link::Playlist { id });
                }
                if let Some(id) = obj.chapter_playlist {
                    return Ok(Link::ChapterPlaylist { id });
                }
                if let Some(id) = obj.slideshow {
                    return Ok(Link::Slideshow { id });
                }

                Err("exactly one link target key must be present".to_string())
            }
        }
    }
}

impl From<Link> for LinkRepr {
    fn from(link: Link) -> Self {
        match link {
            Link::NotSet => LinkRepr::Name("notSet".to_string()),
            Link::Stop => LinkRepr::Name("stop".to_string()),
            Link::LastMenu => LinkRepr::Name("lastMenu".to_string()),
            Link::Timeline { id, chapter } => LinkRepr::Obj(LinkObj {
                timeline: Some(id.0),
                chapter: (chapter != 0).then_some(chapter),
                ..Default::default()
            }),
            Link::Menu { id, button } => LinkRepr::Obj(LinkObj {
                menu: Some(id.0),
                button,
                ..Default::default()
            }),
            Link::Playlist { id } => LinkRepr::Obj(LinkObj {
                playlist: Some(id),
                ..Default::default()
            }),
            Link::ChapterPlaylist { id } => LinkRepr::Obj(LinkObj {
                chapter_playlist: Some(id),
                ..Default::default()
            }),
            Link::Slideshow { id } => LinkRepr::Obj(LinkObj {
                slideshow: Some(id),
                ..Default::default()
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Link;
    use crate::ids::{MenuId, TimelineId};
    use serde_json::{from_str, to_string};

    #[test]
    fn json_round_trip() {
        let cases: [(&str, Link); 9] = [
            ("\"notSet\"", Link::NotSet),
            ("\"stop\"", Link::Stop),
            ("\"lastMenu\"", Link::LastMenu),
            (
                "{\"timeline\":2,\"chapter\":1}",
                Link::Timeline {
                    id: TimelineId(2),
                    chapter: 1,
                },
            ),
            (
                "{\"timeline\":2}",
                Link::Timeline {
                    id: TimelineId(2),
                    chapter: 0,
                },
            ),
            (
                "{\"menu\":4,\"button\":0}",
                Link::Menu {
                    id: MenuId(4),
                    button: Some(0),
                },
            ),
            ("{\"playlist\":5}", Link::Playlist { id: 5 }),
            ("{\"chapterPlaylist\":6}", Link::ChapterPlaylist { id: 6 }),
            ("{\"slideshow\":7}", Link::Slideshow { id: 7 }),
        ];

        for (json, expected) in cases {
            let parsed = from_str::<Link>(json).unwrap_or(Link::NotSet);
            assert_eq!(parsed, expected);
            let serialized = to_string(&parsed).unwrap_or_default();
            assert_eq!(serialized, json);
        }
    }

    #[test]
    fn invalid_objects_and_names() {
        assert!(from_str::<Link>(r#"{"timeline":2,"menu":3}"#).is_err());
        assert!(from_str::<Link>("\"bogus\"").is_err());
    }
}
