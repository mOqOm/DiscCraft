//! Slideshow and playlist document model.

use crate::ids::{AssetId, TimelineId};
use crate::io::ProjectError;
use crate::link::Link;
use crate::model::{AssetKind, Project};
use filmcraft_time::Tick;
use serde::{Deserialize, Serialize};

/// Default slide duration in seconds.
pub const DEFAULT_SLIDE_SECONDS: u64 = 6;

/// [`DEFAULT_SLIDE_SECONDS`] in ticks.
pub const DEFAULT_SLIDE_DURATION: Tick = Tick(DEFAULT_SLIDE_SECONDS as i64 * 254_016_000_000);

fn default_slide_duration() -> Tick {
    DEFAULT_SLIDE_DURATION
}

/// Default transition length (Encore's 2 seconds).
pub const DEFAULT_TRANSITION_DURATION: Tick = Tick(2 * 254_016_000_000);
/// Shortest transition (0.1 s).
pub(crate) const MIN_TRANSITION_DURATION: Tick = Tick(25_401_600_000);
/// Longest transition (10 s).
pub(crate) const MAX_TRANSITION_DURATION: Tick = Tick(2_540_160_000_000);

fn default_transition_duration() -> Tick {
    DEFAULT_TRANSITION_DURATION
}

/// The transition between consecutive slides (Encore's slideshow transitions).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransitionKind {
    /// A hard cut.
    #[default]
    None,
    /// The next slide fades in over the previous one.
    CrossDissolve,
    /// The previous slide fades to black, then the next fades in.
    DipToBlack,
    /// The next slide wipes in from the left.
    Wipe,
}

impl TransitionKind {
    /// Every kind, in the order of Encore's list.
    pub const ALL: [TransitionKind; 4] = [
        TransitionKind::None,
        TransitionKind::CrossDissolve,
        TransitionKind::DipToBlack,
        TransitionKind::Wipe,
    ];

    /// The name in command parameters.
    pub fn as_str(self) -> &'static str {
        match self {
            TransitionKind::None => "none",
            TransitionKind::CrossDissolve => "crossDissolve",
            TransitionKind::DipToBlack => "dipToBlack",
            TransitionKind::Wipe => "wipe",
        }
    }

    /// Parses a command parameter.
    pub fn parse(text: &str) -> Option<TransitionKind> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

/// Minimum slide duration.
pub(crate) const MIN_SLIDE_DURATION: Tick = Tick(127_008_000_000); // 0.5 s

/// Maximum slide duration.
pub(crate) const MAX_SLIDE_DURATION: Tick = Tick(914_457_600_000_000); // 1 h

/// Maximum slides in one slideshow.
const MAX_SLIDES: usize = 99;

/// Maximum items in one playlist.
const MAX_PLAYLIST_ITEMS: usize = 99;

/// How a slide's picture fills the frame (Encore's slide "Scale").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SlideScale {
    /// Scaled to fit, the rest black.
    #[default]
    ScaleAndMatte,
    /// Scaled to cover the frame, the overflow cut off.
    ScaleAndCrop,
    /// Pixel for pixel, centred.
    NoScale,
}

impl SlideScale {
    /// Every choice, in Encore's order.
    pub const ALL: [SlideScale; 3] = [
        SlideScale::ScaleAndMatte,
        SlideScale::ScaleAndCrop,
        SlideScale::NoScale,
    ];

    /// The name in command parameters.
    pub fn as_str(self) -> &'static str {
        match self {
            SlideScale::ScaleAndMatte => "scaleAndMatte",
            SlideScale::ScaleAndCrop => "scaleAndCrop",
            SlideScale::NoScale => "noScale",
        }
    }

    /// Parses a command parameter.
    pub fn parse(text: &str) -> Option<SlideScale> {
        Self::ALL.into_iter().find(|scale| scale.as_str() == text)
    }
}

fn is_default_scale(scale: &SlideScale) -> bool {
    *scale == SlideScale::ScaleAndMatte
}

/// A slide's own transition (Encore's per-slide Transition tab), replacing the show's for the cut into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlideTransition {
    /// Which transition.
    pub kind: TransitionKind,
    /// Its length, centred on the cut.
    pub duration: Tick,
}

impl Slideshow {
    /// The audio clips in playing order (the single `audio` of an older file counts as one clip).
    pub fn audio_list(&self) -> Vec<AssetId> {
        if self.audio_clips.is_empty() {
            self.audio.into_iter().collect()
        } else {
            self.audio_clips.clone()
        }
    }
}

/// One still image slide.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slide {
    /// Image asset.
    pub asset: AssetId,
    /// Display duration.
    pub duration: Tick,
    /// How the picture fills the frame.
    #[serde(default, skip_serializing_if = "is_default_scale")]
    pub scale: SlideScale,
    /// The transition into this slide when it differs from the show's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<SlideTransition>,
}

/// Slideshow document object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slideshow {
    /// Shared object id.
    pub id: u64,
    /// Display name.
    pub name: String,
    /// Slides.
    #[serde(default)]
    pub slides: Vec<Slide>,
    /// Optional audio asset (the first clip; kept for older files).
    #[serde(default)]
    pub audio: Option<AssetId>,
    /// Audio clips played one after another (Encore's audio list). When empty, `audio` alone plays.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio_clips: Vec<AssetId>,
    /// Encore's "Loop Audio": the clips start over until the show ends.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub loop_audio: bool,
    /// Encore's "Fit Slideshow to Audio Duration": the slides stretch (or shrink) so the show lasts as long as
    /// the audio.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fit_to_audio: bool,
    /// Action at slideshow end.
    #[serde(default)]
    pub end_action: Link,
    /// Free description (kept in the project only).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The slideshow's slide duration (Encore's "Slide Duration"): new slides get it, and slides that match it
    /// follow when it changes.
    #[serde(default = "default_slide_duration")]
    pub slide_duration: Tick,
    /// The transition at every cut (Encore's "Default Transition").
    #[serde(default, skip_serializing_if = "is_no_transition")]
    pub transition: TransitionKind,
    /// The transition's length, centred on the cut.
    #[serde(default = "default_transition_duration")]
    pub transition_duration: Tick,
    /// Encore's "Manual Advance": every slide holds until the viewer presses Next (or Enter). On a DVD each
    /// slide's cell gets an infinite still time; the slide durations then only set how long the picture plays
    /// before it holds.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub manual_advance: bool,
}

fn is_no_transition(kind: &TransitionKind) -> bool {
    *kind == TransitionKind::None
}

/// One playlist item: a timeline, a slideshow or a chapter playlist (Encore's playlists hold all three). In
/// the project file a timeline is a bare id (the only kind older files have); the others are
/// `{"slideshow": id}` / `{"chapterPlaylist": id}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "PlaylistItemRepr", into = "PlaylistItemRepr")]
pub enum PlaylistItem {
    /// A timeline, played from its start.
    Timeline(TimelineId),
    /// A slideshow.
    Slideshow(u64),
    /// A chapter playlist.
    ChapterPlaylist(u64),
}

impl PlaylistItem {
    /// The item's object id.
    pub fn id(self) -> u64 {
        match self {
            PlaylistItem::Timeline(id) => id.0,
            PlaylistItem::Slideshow(id) | PlaylistItem::ChapterPlaylist(id) => id,
        }
    }

    /// The link that plays this item.
    pub fn link(self) -> Link {
        match self {
            PlaylistItem::Timeline(id) => Link::Timeline { id, chapter: 0 },
            PlaylistItem::Slideshow(id) => Link::Slideshow { id },
            PlaylistItem::ChapterPlaylist(id) => Link::ChapterPlaylist { id },
        }
    }

    /// The item a link names, if the link plays a title (a chapter link plays its timeline from the start).
    pub fn from_link(link: &Link) -> Option<Self> {
        match link {
            Link::Timeline { id, .. } => Some(PlaylistItem::Timeline(*id)),
            Link::Slideshow { id } => Some(PlaylistItem::Slideshow(*id)),
            Link::ChapterPlaylist { id } => Some(PlaylistItem::ChapterPlaylist(*id)),
            Link::NotSet
            | Link::Stop
            | Link::LastMenu
            | Link::Menu { .. }
            | Link::Playlist { .. } => None,
        }
    }

    /// Whether the item's object exists in `project`.
    pub fn exists_in(self, project: &Project) -> bool {
        match self {
            PlaylistItem::Timeline(id) => project.timelines.contains_key(&id),
            PlaylistItem::Slideshow(id) => project.slideshows.contains_key(&id),
            PlaylistItem::ChapterPlaylist(id) => project.chapter_playlists.contains_key(&id),
        }
    }

    /// The item's display name in `project`.
    pub fn name_in(self, project: &Project) -> Option<String> {
        match self {
            PlaylistItem::Timeline(id) => project.timelines.get(&id).map(|t| t.name.clone()),
            PlaylistItem::Slideshow(id) => project.slideshows.get(&id).map(|s| s.name.clone()),
            PlaylistItem::ChapterPlaylist(id) => {
                project.chapter_playlists.get(&id).map(|c| c.name.clone())
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum PlaylistItemRepr {
    Timeline(u64),
    Object(PlaylistItemObject),
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistItemObject {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timeline: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    slideshow: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chapter_playlist: Option<u64>,
}

impl From<PlaylistItemRepr> for PlaylistItem {
    fn from(repr: PlaylistItemRepr) -> Self {
        match repr {
            PlaylistItemRepr::Timeline(id) => PlaylistItem::Timeline(TimelineId(id)),
            PlaylistItemRepr::Object(object) => {
                if let Some(id) = object.slideshow {
                    PlaylistItem::Slideshow(id)
                } else if let Some(id) = object.chapter_playlist {
                    PlaylistItem::ChapterPlaylist(id)
                } else {
                    // An unknown object names no timeline; the project check reports it as missing.
                    PlaylistItem::Timeline(TimelineId(object.timeline.unwrap_or(0)))
                }
            }
        }
    }
}

impl From<PlaylistItem> for PlaylistItemRepr {
    fn from(item: PlaylistItem) -> Self {
        match item {
            PlaylistItem::Timeline(id) => PlaylistItemRepr::Timeline(id.0),
            PlaylistItem::Slideshow(id) => PlaylistItemRepr::Object(PlaylistItemObject {
                slideshow: Some(id),
                ..PlaylistItemObject::default()
            }),
            PlaylistItem::ChapterPlaylist(id) => PlaylistItemRepr::Object(PlaylistItemObject {
                chapter_playlist: Some(id),
                ..PlaylistItemObject::default()
            }),
        }
    }
}

/// Playlist document object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    /// Shared object id.
    pub id: u64,
    /// Display name.
    pub name: String,
    /// Items in play order.
    #[serde(default)]
    pub items: Vec<PlaylistItem>,
    /// Action at playlist end.
    #[serde(default)]
    pub end_action: Link,
}

impl Project {
    /// Adds a slideshow.
    pub fn add_slideshow(&mut self, name: Option<String>) -> Result<u64, ProjectError> {
        let id = self.alloc_id()?;
        let name = name
            .unwrap_or_else(|| format!("Slideshow {}", self.slideshows.len().saturating_add(1)));

        self.slideshows.insert(
            id,
            Slideshow {
                id,
                name,
                slides: Vec::new(),
                audio: None,
                end_action: Link::NotSet,
                description: String::new(),
                slide_duration: DEFAULT_SLIDE_DURATION,
                transition: TransitionKind::None,
                transition_duration: DEFAULT_TRANSITION_DURATION,
                manual_advance: false,
                audio_clips: Vec::new(),
                loop_audio: false,
                fit_to_audio: false,
            },
        );

        Ok(id)
    }

    /// Borrows a slideshow.
    pub fn slideshow(&self, id: u64) -> Option<&Slideshow> {
        self.slideshows.get(&id)
    }

    /// Mutably borrows a slideshow.
    pub fn slideshow_mut(&mut self, id: u64) -> Option<&mut Slideshow> {
        self.slideshows.get_mut(&id)
    }

    /// Adds a slide to a slideshow.
    pub fn add_slide(
        &mut self,
        id: u64,
        asset: AssetId,
        duration: Tick,
    ) -> Result<usize, ProjectError> {
        let index = slideshow_slide_count(self, id)?;

        if index >= MAX_SLIDES {
            return Err(ProjectError::Invalid(
                "slideshow has too many slides".to_string(),
            ));
        }

        ensure_image_asset(self, asset)?;
        ensure_slide_duration(duration)?;

        let slideshow = self
            .slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?;

        slideshow.slides.push(Slide {
            asset,
            duration,
            scale: SlideScale::default(),
            transition: None,
        });
        Ok(index)
    }

    /// Removes a slide by index.
    pub fn remove_slide(&mut self, id: u64, index: usize) -> Result<(), ProjectError> {
        let slideshow = self
            .slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?;

        if index >= slideshow.slides.len() {
            return Err(ProjectError::Invalid(
                "slide index out of range".to_string(),
            ));
        }

        slideshow.slides.remove(index);
        Ok(())
    }

    /// Changes one slide's duration.
    pub fn set_slide_duration(
        &mut self,
        id: u64,
        index: usize,
        duration: Tick,
    ) -> Result<(), ProjectError> {
        ensure_slide_duration(duration)?;
        let slideshow = self
            .slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?;
        let slide = slideshow
            .slides
            .get_mut(index)
            .ok_or_else(|| ProjectError::Invalid("slide index out of range".to_string()))?;
        slide.duration = duration;
        Ok(())
    }

    /// Gives one slide its own transition, or `None` to follow the show again.
    pub fn set_slide_transition(
        &mut self,
        id: u64,
        index: usize,
        transition: Option<SlideTransition>,
    ) -> Result<(), ProjectError> {
        if let Some(transition) = transition
            && (transition.duration < MIN_TRANSITION_DURATION
                || transition.duration > MAX_TRANSITION_DURATION)
        {
            return Err(ProjectError::Invalid(
                "transition duration must be between 0.1 and 10 seconds".to_string(),
            ));
        }
        let slideshow = self
            .slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?;
        let slide = slideshow
            .slides
            .get_mut(index)
            .ok_or_else(|| ProjectError::Invalid("slide index out of range".to_string()))?;
        slide.transition = transition;
        Ok(())
    }

    /// Changes how one slide fills the frame.
    pub fn set_slide_scale(
        &mut self,
        id: u64,
        index: usize,
        scale: SlideScale,
    ) -> Result<(), ProjectError> {
        let slideshow = self
            .slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?;
        let slide = slideshow
            .slides
            .get_mut(index)
            .ok_or_else(|| ProjectError::Invalid("slide index out of range".to_string()))?;
        slide.scale = scale;
        Ok(())
    }

    /// Changes the slideshow's slide duration; the slides that matched the old one follow (Encore's "match
    /// slideshow setting").
    pub fn set_slideshow_slide_duration(
        &mut self,
        id: u64,
        duration: Tick,
    ) -> Result<(), ProjectError> {
        ensure_slide_duration(duration)?;
        let slideshow = self
            .slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?;
        let old = slideshow.slide_duration;
        slideshow.slide_duration = duration;
        for slide in &mut slideshow.slides {
            if slide.duration == old {
                slide.duration = duration;
            }
        }
        Ok(())
    }

    /// Sets the slideshow's transition and its length.
    pub fn set_transition(
        &mut self,
        id: u64,
        kind: TransitionKind,
        duration: Tick,
    ) -> Result<(), ProjectError> {
        if duration < MIN_TRANSITION_DURATION || duration > MAX_TRANSITION_DURATION {
            return Err(ProjectError::Invalid(
                "transition duration must be between 0.1 and 10 seconds".to_string(),
            ));
        }
        let slideshow = self
            .slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?;
        slideshow.transition = kind;
        slideshow.transition_duration = duration;
        Ok(())
    }

    /// Appends an audio clip to a slideshow (an older single `audio` becomes the first clip).
    pub fn add_slideshow_audio(&mut self, id: u64, asset: AssetId) -> Result<usize, ProjectError> {
        ensure_audio_asset(self, asset)?;
        let slideshow = self
            .slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?;
        if slideshow.audio_clips.len() >= MAX_SLIDES {
            return Err(ProjectError::Invalid(
                "slideshow has too many audio clips".to_string(),
            ));
        }
        if slideshow.audio_clips.is_empty()
            && let Some(first) = slideshow.audio
        {
            slideshow.audio_clips.push(first);
        }
        slideshow.audio_clips.push(asset);
        slideshow.audio = slideshow.audio_clips.first().copied();
        Ok(slideshow.audio_clips.len().saturating_sub(1))
    }

    /// Removes one audio clip of a slideshow.
    pub fn remove_slideshow_audio(&mut self, id: u64, index: usize) -> Result<(), ProjectError> {
        let slideshow = self
            .slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?;
        if slideshow.audio_clips.is_empty()
            && let Some(first) = slideshow.audio
        {
            slideshow.audio_clips.push(first);
        }
        if index >= slideshow.audio_clips.len() {
            return Err(ProjectError::Invalid(
                "audio clip index out of range".to_string(),
            ));
        }
        let _ = slideshow.audio_clips.remove(index);
        slideshow.audio = slideshow.audio_clips.first().copied();
        Ok(())
    }

    /// Turns manual advance on or off.
    pub fn set_manual_advance(&mut self, id: u64, manual: bool) -> Result<(), ProjectError> {
        let slideshow = self
            .slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?;
        slideshow.manual_advance = manual;
        Ok(())
    }

    /// Adds a playlist.
    pub fn add_playlist(&mut self, name: Option<String>) -> Result<u64, ProjectError> {
        let id = self.alloc_id()?;
        let name =
            name.unwrap_or_else(|| format!("Playlist {}", self.playlists.len().saturating_add(1)));

        self.playlists.insert(
            id,
            Playlist {
                id,
                name,
                items: Vec::new(),
                end_action: Link::NotSet,
            },
        );

        Ok(id)
    }

    /// Borrows a playlist.
    pub fn playlist(&self, id: u64) -> Option<&Playlist> {
        self.playlists.get(&id)
    }

    /// Mutably borrows a playlist.
    pub fn playlist_mut(&mut self, id: u64) -> Option<&mut Playlist> {
        self.playlists.get_mut(&id)
    }

    /// Adds a timeline to the end of a playlist.
    pub fn add_playlist_item(
        &mut self,
        id: u64,
        timeline: TimelineId,
    ) -> Result<usize, ProjectError> {
        self.add_playlist_entry(id, PlaylistItem::Timeline(timeline), None)
    }

    /// Adds an item (a timeline, slideshow or chapter playlist) to a playlist, at `at` or the end; returns its
    /// index.
    pub fn add_playlist_entry(
        &mut self,
        id: u64,
        item: PlaylistItem,
        at: Option<usize>,
    ) -> Result<usize, ProjectError> {
        let count = playlist_item_count(self, id)?;

        if count >= MAX_PLAYLIST_ITEMS {
            return Err(ProjectError::Invalid(
                "playlist has too many items".to_string(),
            ));
        }
        if !item.exists_in(self) {
            return Err(ProjectError::Invalid(match item {
                PlaylistItem::Timeline(_) => "timeline does not exist".to_string(),
                PlaylistItem::Slideshow(_) => "slideshow does not exist".to_string(),
                PlaylistItem::ChapterPlaylist(_) => "chapter playlist does not exist".to_string(),
            }));
        }
        let at = at.unwrap_or(count);
        if at > count {
            return Err(ProjectError::Invalid(
                "playlist item index out of range".to_string(),
            ));
        }

        let playlist = self
            .playlists
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("playlist does not exist".to_string()))?;

        playlist.items.insert(at, item);
        Ok(at)
    }

    /// Moves playlist item `from` so that it sits at `to`.
    pub fn move_playlist_item(
        &mut self,
        id: u64,
        from: usize,
        to: usize,
    ) -> Result<(), ProjectError> {
        let playlist = self
            .playlists
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("playlist does not exist".to_string()))?;
        if from >= playlist.items.len() || to >= playlist.items.len() {
            return Err(ProjectError::Invalid(
                "playlist item index out of range".to_string(),
            ));
        }
        let item = playlist.items.remove(from);
        playlist.items.insert(to, item);
        Ok(())
    }

    /// Removes a playlist item by index.
    pub fn remove_playlist_item(&mut self, id: u64, index: usize) -> Result<(), ProjectError> {
        let playlist = self
            .playlists
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("playlist does not exist".to_string()))?;

        if index >= playlist.items.len() {
            return Err(ProjectError::Invalid(
                "playlist item index out of range".to_string(),
            ));
        }

        playlist.items.remove(index);
        Ok(())
    }
}

fn slideshow_slide_count(project: &Project, id: u64) -> Result<usize, ProjectError> {
    project
        .slideshows
        .get(&id)
        .map(|slideshow| slideshow.slides.len())
        .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))
}

fn playlist_item_count(project: &Project, id: u64) -> Result<usize, ProjectError> {
    project
        .playlists
        .get(&id)
        .map(|playlist| playlist.items.len())
        .ok_or_else(|| ProjectError::Invalid("playlist does not exist".to_string()))
}

fn ensure_audio_asset(project: &Project, asset: AssetId) -> Result<(), ProjectError> {
    match project.assets.get(&asset) {
        Some(item) if item.kind == AssetKind::Audio => Ok(()),
        Some(_) => Err(ProjectError::Invalid("asset is not audio".to_string())),
        None => Err(ProjectError::Invalid("asset does not exist".to_string())),
    }
}

fn ensure_image_asset(project: &Project, asset: AssetId) -> Result<(), ProjectError> {
    let kind = project
        .assets
        .get(&asset)
        .map(|asset| asset.kind)
        .ok_or_else(|| ProjectError::Invalid("asset does not exist".to_string()))?;

    if kind != AssetKind::Image {
        return Err(ProjectError::Invalid("asset is not an image".to_string()));
    }

    Ok(())
}

fn ensure_slide_duration(duration: Tick) -> Result<(), ProjectError> {
    if duration < MIN_SLIDE_DURATION || duration > MAX_SLIDE_DURATION {
        return Err(ProjectError::Invalid(
            "slide duration must be between 0.5 seconds and 1 hour".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AssetId, Project, Tick, TimelineId};
    use super::{Link, Playlist, PlaylistItem};
    use std::error::Error;
    use std::path::Path;

    #[test]
    fn add_and_remove_slides() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let image = project.add_asset(Path::new("slide.png"))?;
        let id = project.add_slideshow(None)?;

        assert_eq!(
            project.slideshow(id).ok_or("missing slideshow")?.name,
            "Slideshow 1"
        );

        let index = project.add_slide(id, image, Tick(5000 * 254_016_000))?;
        assert_eq!(index, 0);

        let slideshow = project.slideshow(id).ok_or("missing slideshow")?;
        assert_eq!(slideshow.slides.len(), 1);
        assert_eq!(
            slideshow.slides.first().ok_or("missing slide")?.asset,
            image
        );
        assert_eq!(
            slideshow.slides.first().ok_or("missing slide")?.duration,
            Tick(5000 * 254_016_000)
        );

        project.remove_slide(id, 0)?;
        assert!(
            project
                .slideshow(id)
                .ok_or("missing slideshow")?
                .slides
                .is_empty()
        );
        assert!(project.remove_slide(id, 0).is_err());

        Ok(())
    }

    #[test]
    fn slide_validation() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let image = project.add_asset(Path::new("slide.png"))?;
        let video = project.add_asset(Path::new("movie.mp4"))?;
        let id = project.add_slideshow(Some("Show".to_string()))?;

        assert_eq!(
            project.slideshow(id).ok_or("missing slideshow")?.name,
            "Show"
        );

        assert!(
            project
                .add_slide(99, image, Tick(5000 * 254_016_000))
                .is_err()
        );
        assert!(
            project
                .add_slide(id, AssetId(99), Tick(5000 * 254_016_000))
                .is_err()
        );
        assert!(
            project
                .add_slide(id, video, Tick(5000 * 254_016_000))
                .is_err()
        );
        assert!(project.add_slide(id, image, Tick(0)).is_err());
        assert!(
            project
                .add_slide(id, image, Tick(499 * 254_016_000))
                .is_err()
        );
        assert!(
            project
                .add_slide(id, image, Tick(3600001 * 254_016_000))
                .is_err()
        );
        assert!(
            project
                .add_slide(id, image, Tick(500 * 254_016_000))
                .is_ok()
        );
        assert!(
            project
                .add_slide(id, image, Tick(3600000 * 254_016_000))
                .is_ok()
        );

        Ok(())
    }

    #[test]
    fn max_slides() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let image = project.add_asset(Path::new("slide.png"))?;
        let id = project.add_slideshow(None)?;

        for _ in 0..99 {
            project.add_slide(id, image, Tick(5000 * 254_016_000))?;
        }

        assert_eq!(
            project
                .slideshow(id)
                .ok_or("missing slideshow")?
                .slides
                .len(),
            99
        );
        assert!(
            project
                .add_slide(id, image, Tick(5000 * 254_016_000))
                .is_err()
        );

        Ok(())
    }

    #[test]
    fn add_and_remove_playlist_items() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let video = project.add_asset(Path::new("movie.mp4"))?;
        let timeline = project.add_timeline(video, None)?;
        let id = project.add_playlist(None)?;

        assert_eq!(
            project.playlist(id).ok_or("missing playlist")?.name,
            "Playlist 1"
        );

        let index = project.add_playlist_item(id, timeline)?;
        assert_eq!(index, 0);

        let playlist = project.playlist(id).ok_or("missing playlist")?;
        assert_eq!(playlist.items.len(), 1);
        assert_eq!(
            playlist.items.first().ok_or("missing item")?,
            &PlaylistItem::Timeline(timeline)
        );

        project.remove_playlist_item(id, 0)?;
        assert!(
            project
                .playlist(id)
                .ok_or("missing playlist")?
                .items
                .is_empty()
        );
        assert!(project.remove_playlist_item(id, 0).is_err());

        Ok(())
    }

    #[test]
    fn playlist_validation() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let video = project.add_asset(Path::new("movie.mp4"))?;
        let timeline = project.add_timeline(video, None)?;
        let id = project.add_playlist(Some("List".to_string()))?;

        assert_eq!(project.playlist(id).ok_or("missing playlist")?.name, "List");

        assert!(project.add_playlist_item(99, timeline).is_err());
        assert!(project.add_playlist_item(id, TimelineId(99)).is_err());
        assert!(project.add_playlist_item(id, timeline).is_ok());

        Ok(())
    }

    /// Slideshows and chapter playlists are items too; they round-trip through the project file next to the
    /// bare timeline ids of older files, and can be inserted and moved.
    #[test]
    fn playlist_items_of_every_kind() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let video = project.add_asset(Path::new("movie.mp4"))?;
        let timeline = project.add_timeline(video, None)?;
        let slideshow = project.add_slideshow(None)?;
        let chapter_playlist = project.add_chapter_playlist(timeline, None)?;
        let id = project.add_playlist(None)?;

        project.add_playlist_item(id, timeline)?;
        assert_eq!(
            project.add_playlist_entry(id, PlaylistItem::Slideshow(slideshow), None)?,
            1
        );
        assert_eq!(
            project.add_playlist_entry(
                id,
                PlaylistItem::ChapterPlaylist(chapter_playlist),
                Some(0)
            )?,
            0
        );
        assert!(
            project
                .add_playlist_entry(id, PlaylistItem::Slideshow(999), None)
                .is_err()
        );
        assert!(
            project
                .add_playlist_entry(id, PlaylistItem::Timeline(timeline), Some(9))
                .is_err()
        );
        project.move_playlist_item(id, 0, 2)?;
        assert!(project.move_playlist_item(id, 0, 3).is_err());

        let json = serde_json::to_string(project.playlist(id).ok_or("missing playlist")?)?;
        assert!(json.contains(&format!("\"items\":[{},{{\"slideshow\":{slideshow}}},{{\"chapterPlaylist\":{chapter_playlist}}}]", timeline.0)));
        let back: Playlist = serde_json::from_str(&json)?;
        assert_eq!(
            back.items,
            vec![
                PlaylistItem::Timeline(timeline),
                PlaylistItem::Slideshow(slideshow),
                PlaylistItem::ChapterPlaylist(chapter_playlist),
            ]
        );
        assert_eq!(
            PlaylistItem::from_link(&Link::Timeline {
                id: timeline,
                chapter: 2
            }),
            Some(PlaylistItem::Timeline(timeline))
        );
        assert_eq!(PlaylistItem::from_link(&Link::Stop), None);

        // Removing the slideshow or the chapter playlist drops its items.
        project.remove_slideshow(slideshow)?;
        project.remove_chapter_playlist(chapter_playlist)?;
        assert_eq!(
            project.playlist(id).map(|list| list.items.clone()),
            Some(vec![PlaylistItem::Timeline(timeline)])
        );
        Ok(())
    }

    #[test]
    fn max_playlist_items() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let video = project.add_asset(Path::new("movie.mp4"))?;
        let timeline = project.add_timeline(video, None)?;
        let id = project.add_playlist(None)?;

        for _ in 0..99 {
            project.add_playlist_item(id, timeline)?;
        }

        assert_eq!(
            project.playlist(id).ok_or("missing playlist")?.items.len(),
            99
        );
        assert!(project.add_playlist_item(id, timeline).is_err());

        Ok(())
    }

    #[test]
    fn missing_object_accessors() {
        let mut project = Project::default();
        assert!(project.slideshow(99).is_none());
        assert!(project.playlist(99).is_none());
        assert!(project.slideshow_mut(99).is_none());
        assert!(project.playlist_mut(99).is_none());
    }
}
