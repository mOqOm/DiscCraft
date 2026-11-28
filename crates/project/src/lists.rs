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

/// `seconds` of timecode at the disc's frame rate, as Encore counts a new slideshow's slide and transition lengths:
/// NTSC's 00;00;06;00 is 180 frames (6.006 s), so three slides make 00;00;18;00.
fn timecode_seconds(settings: &crate::DiscSettings, seconds: u64) -> Tick {
    let per_second: i64 = match settings.standard {
        crate::VideoStandard::Ntsc => 30,
        crate::VideoStandard::Pal => 25,
    };
    let seconds = i64::try_from(seconds).unwrap_or(0);
    settings
        .frame_rate()
        .tick_of(seconds.saturating_mul(per_second))
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

/// Which way a slide's picture travels while it is up (Encore's slide effect "Pan").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SlidePan {
    /// The picture stays where it is.
    #[default]
    None,
    /// The picture moves to the left.
    Left,
    /// The picture moves to the right.
    Right,
    /// The picture moves up.
    Up,
    /// The picture moves down.
    Down,
}

impl SlidePan {
    /// Every choice.
    pub const ALL: [SlidePan; 5] = [
        SlidePan::None,
        SlidePan::Left,
        SlidePan::Right,
        SlidePan::Up,
        SlidePan::Down,
    ];

    /// The name in command parameters.
    pub fn as_str(self) -> &'static str {
        match self {
            SlidePan::None => "none",
            SlidePan::Left => "left",
            SlidePan::Right => "right",
            SlidePan::Up => "up",
            SlidePan::Down => "down",
        }
    }

    /// Parses a command parameter.
    pub fn parse(text: &str) -> Option<SlidePan> {
        Self::ALL.into_iter().find(|pan| pan.as_str() == text)
    }
}

/// How a slide's picture is scaled while it is up (Encore's slide effect "Zoom").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SlideZoom {
    /// The picture grows (Encore's default).
    #[default]
    In,
    /// The picture shrinks back to its size.
    Out,
    /// The picture keeps its size.
    None,
}

impl SlideZoom {
    /// Every choice.
    pub const ALL: [SlideZoom; 3] = [SlideZoom::In, SlideZoom::Out, SlideZoom::None];

    /// The name in command parameters.
    pub fn as_str(self) -> &'static str {
        match self {
            SlideZoom::In => "in",
            SlideZoom::Out => "out",
            SlideZoom::None => "none",
        }
    }

    /// Parses a command parameter.
    pub fn parse(text: &str) -> Option<SlideZoom> {
        Self::ALL.into_iter().find(|zoom| zoom.as_str() == text)
    }
}

/// A slide's pan and zoom (Encore's slide "Effects": Pan & Zoom).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlideEffect {
    /// Which way the picture travels.
    #[serde(default)]
    pub pan: SlidePan,
    /// How the picture is scaled.
    #[serde(default)]
    pub zoom: SlideZoom,
}

impl SlideEffect {
    /// The effect Encore's "Random Pan & Zoom" gives the slide at `index`: one of the pans with a zoom in or out,
    /// the same for the same place in the show (so a build can be repeated).
    #[must_use]
    pub fn random(index: usize) -> SlideEffect {
        // A small integer hash of the place (multiply and fold), enough to spread the choices.
        let mixed = (index as u64)
            .wrapping_add(1)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let folded = (mixed >> 32) ^ (mixed & 0xFFFF_FFFF);
        let pan = match folded % 5 {
            0 => SlidePan::None,
            1 => SlidePan::Left,
            2 => SlidePan::Right,
            3 => SlidePan::Up,
            _ => SlidePan::Down,
        };
        let zoom = if (folded / 5).is_multiple_of(2) {
            SlideZoom::In
        } else {
            SlideZoom::Out
        };
        SlideEffect { pan, zoom }
    }
}

/// The outline of a slide's subtitle (Encore's Stroke).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SlideStroke {
    /// No outline.
    None,
    /// A thin outline.
    Thin,
    /// A medium outline (Encore's default).
    #[default]
    Medium,
    /// A thick outline.
    Thick,
    /// The thickest outline (Encore's 極太).
    ExtraThick,
}

impl SlideStroke {
    /// Every choice, thinnest first.
    pub const ALL: [SlideStroke; 5] = [
        SlideStroke::None,
        SlideStroke::Thin,
        SlideStroke::Medium,
        SlideStroke::Thick,
        SlideStroke::ExtraThick,
    ];

    /// The name in command parameters.
    pub fn as_str(self) -> &'static str {
        match self {
            SlideStroke::None => "none",
            SlideStroke::Thin => "thin",
            SlideStroke::Medium => "medium",
            SlideStroke::Thick => "thick",
            SlideStroke::ExtraThick => "extraThick",
        }
    }

    /// Parses a command parameter.
    pub fn parse(text: &str) -> Option<SlideStroke> {
        Self::ALL.into_iter().find(|stroke| stroke.as_str() == text)
    }

    /// The outline's width in pixels.
    pub fn pixels(self) -> u8 {
        match self {
            SlideStroke::None => 0,
            SlideStroke::Thin => 1,
            SlideStroke::Medium => 2,
            SlideStroke::Thick => 3,
            SlideStroke::ExtraThick => 4,
        }
    }
}

fn is_default_stroke(stroke: &SlideStroke) -> bool {
    *stroke == SlideStroke::Medium
}

fn is_first_group(group: &u8) -> bool {
    *group == 0
}

fn bottom_aligned() -> u8 {
    100
}

fn is_bottom_aligned(percent: &u8) -> bool {
    *percent == 100
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
    /// Which slides stay up until the viewer moves on: all of them with the show's Manual Advance, else the
    /// slides that have their own.
    #[must_use]
    pub fn held_slides(&self) -> Vec<bool> {
        self.slides
            .iter()
            .map(|slide| self.manual_advance || slide.manual_advance)
            .collect()
    }

    /// The pan and zoom the slide at `index` plays with: its own, else a random one when the show asks for them.
    #[must_use]
    pub fn slide_effect(&self, index: usize) -> Option<SlideEffect> {
        let slide = self.slides.get(index)?;
        slide
            .effect
            .or_else(|| self.random_pan_zoom.then(|| SlideEffect::random(index)))
    }

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
    /// Encore's "Create Subtitle": the slide's name is shown as a subtitle while the slide is up.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub subtitle: bool,
    /// The slide's own name; empty means the picture's name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The slide's description.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Encore's "Include: Name" turned off: the subtitle leaves the name out.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub subtitle_without_name: bool,
    /// Encore's "Include: Description": the subtitle shows the description.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub subtitle_description: bool,
    /// Encore's Stroke of the subtitle: the outline around its letters.
    #[serde(default, skip_serializing_if = "is_default_stroke")]
    pub subtitle_stroke: SlideStroke,
    /// Encore's Alignment of the subtitle: where it sits from the top (0) to the bottom (100), in percent.
    #[serde(default = "bottom_aligned", skip_serializing_if = "is_bottom_aligned")]
    pub subtitle_align: u8,
    /// Encore's Highlight of the subtitle: the colour group (from 0) it uses.
    #[serde(default, skip_serializing_if = "is_first_group")]
    pub subtitle_group: u8,
    /// Encore's "Pan & Zoom" on the slide: the picture moves while the slide is up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<SlideEffect>,
    /// Encore's "Manual Advance" on the slide: it stays up until the viewer moves on (the show's own Manual
    /// Advance holds every slide).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub manual_advance: bool,
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
    /// The show's own colour set for its slides' subtitles (`None`: the default set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle_colors: Option<crate::SubtitleColorSet>,
    /// Action at slideshow end.
    #[serde(default)]
    pub end_action: Link,
    /// The title's own parental level (M6.363): `None` the disc's, `Some(0)` none, `Some(1..=8)` that level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parental_level: Option<u8>,
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
    /// Encore's "Random Pan & Zoom": every slide without an effect of its own gets one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub random_pan_zoom: bool,
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
            | Link::Resume
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
        let name = name.unwrap_or_else(|| {
            self.words
                .slideshow(self.slideshows.len().saturating_add(1))
        });

        self.slideshows.insert(
            id,
            Slideshow {
                parental_level: None,
                id,
                name,
                slides: Vec::new(),
                audio: None,
                end_action: Link::NotSet,
                description: String::new(),
                // Encore counts them in timecode: 00;00;06;00 and 00;00;02;00 at the disc's rate.
                slide_duration: timecode_seconds(&self.settings, DEFAULT_SLIDE_SECONDS),
                transition: TransitionKind::None,
                transition_duration: timecode_seconds(&self.settings, 2),
                manual_advance: false,
                random_pan_zoom: false,
                audio_clips: Vec::new(),
                loop_audio: false,
                fit_to_audio: false,
                subtitle_colors: None,
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
            subtitle: false,
            name: String::new(),
            description: String::new(),
            subtitle_without_name: false,
            subtitle_description: false,
            subtitle_stroke: SlideStroke::default(),
            subtitle_align: 100,
            subtitle_group: 0,
            effect: None,
            manual_advance: false,
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

    /// Encore's "Pan & Zoom" on a slide: `None` turns it off.
    pub fn set_slide_effect(
        &mut self,
        id: u64,
        index: usize,
        effect: Option<SlideEffect>,
    ) -> Result<(), ProjectError> {
        let slide = self
            .slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?
            .slides
            .get_mut(index)
            .ok_or_else(|| ProjectError::Invalid("slide index out of range".to_string()))?;
        slide.effect = effect;
        Ok(())
    }

    /// Encore's "Random Pan & Zoom" of a slideshow.
    pub fn set_slideshow_random_pan_zoom(&mut self, id: u64, on: bool) -> Result<(), ProjectError> {
        self.slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?
            .random_pan_zoom = on;
        Ok(())
    }

    /// Encore's "Create Subtitle" on a slide: its name becomes a subtitle shown while the slide is up.
    pub fn set_slide_subtitle(
        &mut self,
        id: u64,
        index: usize,
        on: bool,
    ) -> Result<(), ProjectError> {
        let slide = self
            .slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?
            .slides
            .get_mut(index)
            .ok_or_else(|| ProjectError::Invalid("slide index out of range".to_string()))?;
        slide.subtitle = on;
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

    /// Turns one slide's manual advance on or off.
    pub fn set_slide_manual_advance(
        &mut self,
        id: u64,
        slide: usize,
        manual: bool,
    ) -> Result<(), ProjectError> {
        let slideshow = self
            .slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?;
        let slide = slideshow
            .slides
            .get_mut(slide)
            .ok_or_else(|| ProjectError::Invalid("slide does not exist".to_string()))?;
        slide.manual_advance = manual;
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
            name.unwrap_or_else(|| self.words.playlist(self.playlists.len().saturating_add(1)));

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
    fn a_new_slideshow_counts_its_lengths_in_timecode() -> Result<(), Box<dyn Error>> {
        // NTSC: 00;00;06;00 is 180 frames, so three slides make 00;00;18;00 (as in Encore), not 00;00;17;29.
        let mut project = Project::default();
        let image = project.add_asset(Path::new("slide.png"))?;
        let id = project.add_slideshow(None)?;
        let duration = project.slideshow(id).ok_or("slideshow")?.slide_duration;
        for _ in 0..3 {
            let _ = project.add_slide(id, image, duration)?;
        }
        let show = project.slideshow(id).ok_or("slideshow")?;
        let total = Tick(show.slides.iter().map(|slide| slide.duration.0).sum());
        assert_eq!(
            crate::format_time(show.slide_duration, &project.settings),
            "00;00;06;00"
        );
        assert_eq!(
            crate::format_time(show.transition_duration, &project.settings),
            "00;00;02;00"
        );
        assert_eq!(crate::format_time(total, &project.settings), "00;00;18;00");
        // PAL: whole seconds.
        project.settings.standard = crate::VideoStandard::Pal;
        let pal = project.add_slideshow(None)?;
        let pal_length = project.slideshow(pal).ok_or("slideshow")?.slide_duration;
        assert_eq!(pal_length, super::DEFAULT_SLIDE_DURATION);
        Ok(())
    }

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

impl Project {
    /// A slide's name: its own, or the picture's when it has none (Encore gives a slide the picture's name and
    /// keeps it when the picture is replaced).
    #[must_use]
    pub fn slide_name(&self, slide: &Slide) -> String {
        let own = slide.name.trim();
        if own.is_empty() {
            self.assets
                .get(&slide.asset)
                .map(|asset| asset.name.trim().to_owned())
                .unwrap_or_default()
        } else {
            own.to_owned()
        }
    }

    /// The subtitle of a slide with Encore's "Create Subtitle": its name and its description, as "Include" says
    /// (the name on the first line). Empty when the slide has no subtitle or nothing to show.
    #[must_use]
    pub fn slide_subtitle_text(&self, slide: &Slide) -> String {
        if !slide.subtitle {
            return String::new();
        }
        let mut lines = Vec::new();
        if !slide.subtitle_without_name {
            lines.push(self.slide_name(slide));
        }
        if slide.subtitle_description {
            lines.push(slide.description.trim().to_owned());
        }
        lines.retain(|line| !line.is_empty());
        lines.join("\n")
    }

    /// A slide's name and description (`None` leaves one as it is).
    pub fn set_slide_text(
        &mut self,
        id: u64,
        index: usize,
        name: Option<String>,
        description: Option<String>,
    ) -> Result<(), ProjectError> {
        let slide = self
            .slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?
            .slides
            .get_mut(index)
            .ok_or_else(|| ProjectError::Invalid("slide index out of range".to_string()))?;
        if let Some(name) = name {
            slide.name = name;
        }
        if let Some(description) = description {
            slide.description = description;
        }
        Ok(())
    }

    /// Gives a slideshow its own colour set for its slides' subtitles (`None`: back to the default).
    pub fn set_slideshow_subtitle_colors(
        &mut self,
        id: u64,
        colors: Option<crate::SubtitleColorSet>,
    ) -> Result<(), ProjectError> {
        if let Some(set) = &colors
            && set
                .groups
                .iter()
                .any(|group| group.fill.opacity > 100 || group.stroke.opacity > 100)
        {
            return Err(ProjectError::Invalid(
                "opacity is 0 to 100 percent".to_string(),
            ));
        }
        let slideshow = self
            .slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?;
        slideshow.subtitle_colors = colors.filter(|set| *set != crate::SubtitleColorSet::default());
        Ok(())
    }

    /// Encore's Highlight of a slide's subtitle: its colour group (from 0).
    pub fn set_slide_subtitle_group(
        &mut self,
        id: u64,
        index: usize,
        group: u8,
    ) -> Result<(), ProjectError> {
        if group >= crate::SubtitleColorSet::GROUPS {
            return Err(ProjectError::Invalid(
                "a colour set has three groups".to_string(),
            ));
        }
        let slide = self
            .slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?
            .slides
            .get_mut(index)
            .ok_or_else(|| ProjectError::Invalid("slide index out of range".to_string()))?;
        slide.subtitle_group = group;
        Ok(())
    }

    /// Encore's Stroke and Alignment of a slide's subtitle (`None` leaves one as it is; the alignment is a percentage
    /// from the top, at most 100).
    pub fn set_slide_subtitle_look(
        &mut self,
        id: u64,
        index: usize,
        stroke: Option<SlideStroke>,
        align: Option<u8>,
    ) -> Result<(), ProjectError> {
        if align.is_some_and(|align| align > 100) {
            return Err(ProjectError::Invalid(
                "subtitle alignment is 0 to 100 percent".to_string(),
            ));
        }
        let slide = self
            .slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?
            .slides
            .get_mut(index)
            .ok_or_else(|| ProjectError::Invalid("slide index out of range".to_string()))?;
        if let Some(stroke) = stroke {
            slide.subtitle_stroke = stroke;
        }
        if let Some(align) = align {
            slide.subtitle_align = align;
        }
        Ok(())
    }

    /// Encore's "Include" of a slide's subtitle: the name, the description.
    pub fn set_slide_subtitle_parts(
        &mut self,
        id: u64,
        index: usize,
        name: bool,
        description: bool,
    ) -> Result<(), ProjectError> {
        let slide = self
            .slideshows
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("slideshow does not exist".to_string()))?
            .slides
            .get_mut(index)
            .ok_or_else(|| ProjectError::Invalid("slide index out of range".to_string()))?;
        slide.subtitle_without_name = !name;
        slide.subtitle_description = description;
        Ok(())
    }
}

#[cfg(test)]
mod slide_text_tests {
    use crate::model::Project;
    use filmcraft_time::Tick;

    /// Encore: a slide takes the picture's name; "Include" chooses the name and the description.
    #[test]
    fn a_slides_subtitle_is_its_name_and_description() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let dir = std::env::temp_dir();
        let image = project.add_asset(&dir.join("disccraft-slide-text.png"))?;
        let show = project.add_slideshow(None)?;
        let index = project.add_slide(show, image, Tick(filmcraft_time::TICKS_PER_SECOND))?;
        let text = |project: &Project| {
            project
                .slideshow(show)
                .and_then(|show| show.slides.get(index))
                .map(|slide| project.slide_subtitle_text(slide))
                .unwrap_or_default()
        };
        assert_eq!(text(&project), "");
        project.set_slide_subtitle(show, index, true)?;
        assert_eq!(text(&project), "disccraft-slide-text");
        project.set_slide_text(show, index, None, Some("By the sea".to_owned()))?;
        assert_eq!(text(&project), "disccraft-slide-text");
        project.set_slide_subtitle_parts(show, index, true, true)?;
        assert_eq!(text(&project), "disccraft-slide-text\nBy the sea");
        project.set_slide_text(show, index, Some("Beach".to_owned()), None)?;
        project.set_slide_subtitle_parts(show, index, false, true)?;
        assert_eq!(text(&project), "By the sea");
        project.set_slide_subtitle_parts(show, index, false, false)?;
        assert_eq!(text(&project), "");
        // The look: a stroke and a place from the top; more than 100 % is refused.
        project.set_slide_subtitle_look(show, index, Some(crate::SlideStroke::Thick), Some(0))?;
        assert!(
            project
                .set_slide_subtitle_look(show, index, None, Some(101))
                .is_err()
        );
        let slide = project
            .slideshow(show)
            .and_then(|show| show.slides.get(index))
            .ok_or("no slide")?;
        assert_eq!(slide.subtitle_stroke.pixels(), 3);
        assert_eq!(slide.subtitle_align, 0);
        // The show's own colour set: kept when it differs from the default, refused past 100 % opacity.
        let mut colors = crate::SubtitleColorSet::default();
        project.set_slideshow_subtitle_colors(show, Some(colors))?;
        assert!(
            project
                .slideshow(show)
                .ok_or("no show")?
                .subtitle_colors
                .is_none()
        );
        if let Some(group) = colors.groups.first_mut() {
            group.fill.rgb = [255, 0, 0];
        }
        project.set_slideshow_subtitle_colors(show, Some(colors))?;
        assert_eq!(
            project.slideshow(show).ok_or("no show")?.subtitle_colors,
            Some(colors)
        );
        if let Some(group) = colors.groups.first_mut() {
            group.fill.opacity = 101;
        }
        assert!(
            project
                .set_slideshow_subtitle_colors(show, Some(colors))
                .is_err()
        );
        Ok(())
    }
}
