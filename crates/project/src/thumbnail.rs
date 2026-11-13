//! Video thumbnails of buttons (Encore's `(%)` layer in a button's layer set): the area of a button that shows the
//! video its link plays. The still picture is the linked chapter's poster frame, or the chapter itself when it has
//! none; Encore stretches the whole frame over the area.

use crate::Tick;
use crate::ids::AssetId;
use crate::link::Link;
use crate::lists::PlaylistItem;
use crate::menu::{Button, Menu, Rect};
use crate::model::{AssetKind, Project};
use serde::{Deserialize, Serialize};

/// The fractions of [`ThumbnailArea`] count in this many parts of the button.
pub const THUMBNAIL_PARTS: u16 = 10_000;

/// Where a button's video thumbnail lies in the button: its left, top, width and height as fractions of the
/// button's rectangle in 1/[`THUMBNAIL_PARTS`], so it moves and scales with the button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailArea {
    /// Left edge.
    pub x: u16,
    /// Top edge.
    pub y: u16,
    /// Width.
    pub w: u16,
    /// Height.
    pub h: u16,
}

impl ThumbnailArea {
    /// The whole button.
    pub const WHOLE: Self = Self {
        x: 0,
        y: 0,
        w: THUMBNAIL_PARTS,
        h: THUMBNAIL_PARTS,
    };

    /// `area` (a rectangle of the frame) as a part of `button`, clipped to the button. `None` when they do not
    /// overlap.
    pub fn within(button: Rect, area: Rect) -> Option<Self> {
        let (bx0, by0) = (u32::from(button.x), u32::from(button.y));
        let (bx1, by1) = (
            bx0.saturating_add(u32::from(button.w)),
            by0.saturating_add(u32::from(button.h)),
        );
        let x0 = u32::from(area.x).clamp(bx0, bx1);
        let y0 = u32::from(area.y).clamp(by0, by1);
        let x1 = u32::from(area.x)
            .saturating_add(u32::from(area.w))
            .clamp(bx0, bx1);
        let y1 = u32::from(area.y)
            .saturating_add(u32::from(area.h))
            .clamp(by0, by1);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let part = |value: u32, length: u16| -> u16 {
            let scaled =
                value.saturating_mul(u32::from(THUMBNAIL_PARTS)) / u32::from(length.max(1));
            u16::try_from(scaled.min(u32::from(THUMBNAIL_PARTS))).unwrap_or(THUMBNAIL_PARTS)
        };
        Some(Self {
            x: part(x0 - bx0, button.w),
            y: part(y0 - by0, button.h),
            w: part(x1 - x0, button.w).max(1),
            h: part(y1 - y0, button.h).max(1),
        })
    }

    /// The area in the frame for a button at `button`: inside the button and at least 1 x 1 (fractions out of
    /// range, as a hand-edited project may have, are clamped).
    pub fn rect_in(&self, button: Rect) -> Rect {
        let parts = u32::from(THUMBNAIL_PARTS);
        let scale = |part: u16, length: u16| -> u32 {
            u32::from(part.min(THUMBNAIL_PARTS)) * u32::from(length) / parts
        };
        let x = scale(self.x, button.w).min(u32::from(button.w.saturating_sub(1)));
        let y = scale(self.y, button.h).min(u32::from(button.h.saturating_sub(1)));
        let w = scale(self.w, button.w)
            .max(1)
            .min(u32::from(button.w).saturating_sub(x).max(1));
        let h = scale(self.h, button.h)
            .max(1)
            .min(u32::from(button.h).saturating_sub(y).max(1));
        // Each part is at most the button's side, a u16.
        let to_u16 = |value: u32| u16::try_from(value).unwrap_or(u16::MAX);
        Rect {
            x: button.x.saturating_add(to_u16(x)),
            y: button.y.saturating_add(to_u16(y)),
            w: to_u16(w),
            h: to_u16(h),
        }
    }
}

impl Button {
    /// The frame rectangle of the button's video thumbnail, when it has one.
    pub fn thumbnail_rect(&self) -> Option<Rect> {
        self.thumbnail.map(|area| area.rect_in(self.rect))
    }

    /// Where the button's text goes: the part of the button its video thumbnail leaves (below it, else above it),
    /// or the whole button.
    pub fn label_rect(&self) -> Rect {
        let rect = self.rect;
        let Some(area) = self.thumbnail_rect() else {
            return rect;
        };
        let bottom = rect.y.saturating_add(rect.h);
        let area_bottom = area.y.saturating_add(area.h);
        let below = bottom.saturating_sub(area_bottom);
        let above = area.y.saturating_sub(rect.y);
        // A strip thinner than this cannot hold a readable line.
        const MIN_STRIP: u16 = 8;
        if below >= above && below >= MIN_STRIP {
            Rect {
                y: area_bottom,
                h: below,
                ..rect
            }
        } else if above >= MIN_STRIP {
            Rect { h: above, ..rect }
        } else {
            rect
        }
    }
}

/// Where a button's video thumbnail takes its picture: a video asset and the time of its still picture, which is
/// also where a moving thumbnail starts and loops back to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThumbnailSource {
    /// The video.
    pub asset: AssetId,
    /// The still picture: the chapter's poster frame, or the chapter.
    pub start: Tick,
    /// Whether the chapter has a poster frame. Without one, a still thumbnail shows the chapter's first bright
    /// (non-black) frame from `start`, and a moving one plays from `start` (Encore).
    pub poster: bool,
    /// Whether the asset is a still picture (a slideshow's first slide): the thumbnail shows it as it is, and it
    /// does not move.
    pub still: bool,
}

/// One video thumbnail of a menu: the button, its area in the frame and its picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuThumbnail {
    /// The button's index.
    pub button: usize,
    /// The thumbnail's area in the frame.
    pub rect: Rect,
    /// Where its picture comes from.
    pub source: ThumbnailSource,
}

impl Project {
    /// What a video thumbnail linked to `link` shows (Encore): a timeline's chapter (its poster frame, or the
    /// chapter) of a video that is online, a slideshow's first slide, or the first entry of a playlist or a chapter
    /// playlist. `None` for other links (and empty lists), which leave the thumbnail's own pixels.
    pub fn thumbnail_source(&self, link: &Link) -> Option<ThumbnailSource> {
        let (id, chapter) = match link {
            Link::Timeline { id, chapter } => (id, chapter),
            Link::Slideshow { id } => {
                let slide = self.slideshows.get(id)?.slides.first()?;
                let picture = self
                    .assets
                    .get(&slide.asset)
                    .filter(|asset| asset.kind == AssetKind::Image && !asset.offline)?;
                return Some(ThumbnailSource {
                    asset: picture.id,
                    start: Tick::ZERO,
                    poster: true,
                    still: true,
                });
            }
            Link::Playlist { id } => {
                let first = match *self.playlists.get(id)?.items.first()? {
                    PlaylistItem::Timeline(timeline) => Link::Timeline {
                        id: timeline,
                        chapter: 0,
                    },
                    PlaylistItem::Slideshow(id) => Link::Slideshow { id },
                    PlaylistItem::ChapterPlaylist(id) => Link::ChapterPlaylist { id },
                };
                return self.thumbnail_source(&first);
            }
            Link::ChapterPlaylist { id } => {
                let list = self.chapter_playlists.get(id)?;
                return self.thumbnail_source(&Link::Timeline {
                    id: list.timeline,
                    chapter: *list.chapters.first()?,
                });
            }
            _ => return None,
        };
        let timeline = self.timelines.get(id)?;
        let video = self
            .assets
            .get(&timeline.video)
            .filter(|asset| asset.kind == AssetKind::Video && !asset.offline)?;
        let (start, poster) = match usize::try_from(*chapter)
            .ok()
            .and_then(|index| timeline.chapters.get(index))
        {
            Some(found) => (found.poster.unwrap_or(found.time), found.poster.is_some()),
            None if *chapter == 0 => (Tick::ZERO, false),
            None => return None,
        };
        Some(ThumbnailSource {
            asset: video.id,
            start,
            poster,
            still: false,
        })
    }

    /// The video thumbnails of `menu` that have a picture: each button with a thumbnail area and a link to a video.
    pub fn menu_thumbnails(&self, menu: &Menu) -> Vec<MenuThumbnail> {
        menu.buttons
            .iter()
            .enumerate()
            .filter_map(|(button, found)| {
                Some(MenuThumbnail {
                    button,
                    rect: found.thumbnail_rect()?,
                    source: self.thumbnail_source(&found.link)?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{ThumbnailArea, ThumbnailSource};
    use crate::{Link, Project, Rect, Tick, TimelineId};

    #[test]
    fn an_area_moves_and_scales_with_its_button() {
        let button = Rect {
            x: 100,
            y: 50,
            w: 200,
            h: 100,
        };
        let area = ThumbnailArea::within(
            button,
            Rect {
                x: 110,
                y: 60,
                w: 180,
                h: 60,
            },
        );
        assert_eq!(
            area,
            Some(ThumbnailArea {
                x: 500,
                y: 1000,
                w: 9000,
                h: 6000
            })
        );
        let area = area.unwrap_or(ThumbnailArea::WHOLE);
        assert_eq!(
            area.rect_in(button),
            Rect {
                x: 110,
                y: 60,
                w: 180,
                h: 60
            }
        );
        // Moved and twice as large.
        assert_eq!(
            area.rect_in(Rect {
                x: 0,
                y: 0,
                w: 400,
                h: 200
            }),
            Rect {
                x: 20,
                y: 20,
                w: 360,
                h: 120
            }
        );
        // Outside the button: nothing; fractions out of range stay inside the button.
        assert_eq!(
            ThumbnailArea::within(
                button,
                Rect {
                    x: 400,
                    y: 0,
                    w: 10,
                    h: 10
                }
            ),
            None
        );
        let wild = ThumbnailArea {
            x: u16::MAX,
            y: u16::MAX,
            w: u16::MAX,
            h: u16::MAX,
        };
        let rect = wild.rect_in(button);
        assert!(rect.x < 300 && rect.y < 150 && rect.w >= 1 && rect.h >= 1);
    }

    #[test]
    fn a_thumbnail_shows_the_linked_chapters_poster_frame() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = std::env::temp_dir().join(format!("disccraft-thumbnail-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let video = dir.join("clip.m2v");
        std::fs::write(&video, b"")?;
        let mut project = Project::default();
        let asset = project.add_asset(&video)?;
        if let Some(found) = project.assets.get_mut(&asset) {
            found.kind = crate::AssetKind::Video;
        }
        let timeline = project.add_timeline(asset, Some("Movie".to_owned()))?;
        let _ = project.add_chapter(timeline, Tick(1000), None)?;
        let at = |chapter: u32| Link::Timeline {
            id: timeline,
            chapter,
        };
        assert_eq!(
            project.thumbnail_source(&at(0)),
            Some(ThumbnailSource {
                asset,
                start: Tick::ZERO,
                poster: false,
                still: false,
            })
        );
        let second = project.thumbnail_source(&at(1)).map(|source| source.start);
        assert_eq!(second, Some(Tick(1000)));
        project.set_poster_frame(timeline, 1, Some(Tick(1200)))?;
        let second = project
            .thumbnail_source(&at(1))
            .map(|source| (source.start, source.poster));
        // A poster frame is shown as it is (without one, a still thumbnail looks for a bright frame).
        assert_eq!(second, Some((Tick(1200), true)));
        assert_eq!(project.thumbnail_source(&at(9)), None);
        assert_eq!(project.thumbnail_source(&Link::Stop), None);

        // A slideshow shows its first slide; a playlist and a chapter playlist their first entry (Encore).
        let picture = dir.join("slide.png");
        std::fs::write(&picture, b"")?;
        let slide = project.add_asset(&picture)?;
        if let Some(found) = project.assets.get_mut(&slide) {
            found.kind = crate::AssetKind::Image;
        }
        let show = project.add_slideshow(None)?;
        assert_eq!(
            project.thumbnail_source(&Link::Slideshow { id: show }),
            None
        );
        let _ = project.add_slide(show, slide, Tick(5000 * 254_016_000))?;
        let still = project.thumbnail_source(&Link::Slideshow { id: show });
        assert_eq!(
            still,
            Some(ThumbnailSource {
                asset: slide,
                start: Tick::ZERO,
                poster: true,
                still: true,
            })
        );
        let list = project.add_playlist(None)?;
        assert_eq!(project.thumbnail_source(&Link::Playlist { id: list }), None);
        let _ = project.add_playlist_entry(list, crate::PlaylistItem::Slideshow(show), None)?;
        let _ = project.add_playlist_item(list, timeline)?;
        assert_eq!(
            project.thumbnail_source(&Link::Playlist { id: list }),
            still
        );
        let chapters = project.add_chapter_playlist(timeline, None)?;
        let _ = project.add_chapter_playlist_entry(chapters, 1, None)?;
        let first = project
            .thumbnail_source(&Link::ChapterPlaylist { id: chapters })
            .map(|source| source.start);
        assert_eq!(first, Some(Tick(1200)));
        assert_eq!(
            project.thumbnail_source(&Link::Timeline {
                id: TimelineId(999),
                chapter: 0
            }),
            None
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
