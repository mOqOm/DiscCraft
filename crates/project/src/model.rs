use crate::ids::{AssetId, MenuId, TimelineId};
use crate::io::ProjectError;
use crate::link::Link;
use filmcraft_time::{FrameRate, Tick};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Disc family being authored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum DiscKind {
    /// DVD-Video.
    #[default]
    Dvd,
    /// Blu-ray.
    BluRay,
}

/// Broadcast video standard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum VideoStandard {
    /// NTSC.
    #[default]
    Ntsc,
    /// PAL.
    Pal,
}

/// Target disc capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Capacity {
    /// Single-layer DVD.
    #[default]
    Dvd5,
    /// Dual-layer DVD.
    Dvd9,
    /// Single-layer Blu-ray.
    Bd25,
    /// Dual-layer Blu-ray.
    Bd50,
    /// BDXL, three layers (100 GB).
    Bd100,
    /// BDXL, single-sided four layers (128 GB).
    Bd128,
    /// User-supplied byte capacity.
    Custom {
        /// Exact byte limit.
        bytes: u64,
    },
}

impl Capacity {
    /// Approximate usable byte capacity.
    pub fn bytes(&self) -> u64 {
        match self {
            Capacity::Dvd5 => 4_700_000_000,
            Capacity::Dvd9 => 8_540_000_000,
            Capacity::Bd25 => 25_025_314_816,
            Capacity::Bd50 => 50_050_629_632,
            Capacity::Bd100 => 100_103_356_416,
            Capacity::Bd128 => 128_001_769_472,
            Capacity::Custom { bytes } => *bytes,
        }
    }

    /// The capacities offered for a disc kind, smallest first (DVD: 1 or 2 layers; Blu-ray: 1, 2, and BDXL 3 or
    /// 4 layers).
    pub fn choices(kind: DiscKind) -> &'static [Capacity] {
        match kind {
            DiscKind::Dvd => &[Capacity::Dvd5, Capacity::Dvd9],
            DiscKind::BluRay => &[
                Capacity::Bd25,
                Capacity::Bd50,
                Capacity::Bd100,
                Capacity::Bd128,
            ],
        }
    }
}

/// Global disc settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiscSettings {
    /// Disc name (Encore's Disc "Name"). Also the volume label of an ISO image when it has ASCII letters or digits.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Free description of the disc (kept in the project only).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Disc family.
    #[serde(default)]
    pub kind: DiscKind,
    /// Video standard.
    #[serde(default)]
    pub standard: VideoStandard,
    /// Capacity target.
    #[serde(default)]
    pub capacity: Capacity,
    /// Region codes; empty means region free.
    #[serde(default)]
    pub regions: Vec<u8>,
    /// First-play link.
    #[serde(default)]
    pub first_play: Link,
    /// Title-remote link.
    #[serde(default)]
    pub title_remote: Link,
    /// Where to go when the title the first play leads to has finished, instead of that title's end action
    /// (Encore's disc Override). Used when the first play plays a title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_play_override: Option<Link>,
    /// 16:9 (anamorphic) titles instead of 4:3. Menus stay 4:3.
    #[serde(default)]
    pub widescreen: bool,
    /// Remote operations prohibited on the whole disc (first play, every title and every menu; DVD).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prohibited_ops: Vec<crate::UserOp>,
    /// The audio track (1 to 8) the disc selects when it starts; `None` leaves the player's choice (Encore's
    /// "Set Audio").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_default: Option<u8>,
    /// The language (ISO 639-1, `ja`) of audio that has none of its own: a video's own soundtrack, a slideshow's
    /// audio and an audio track whose language is empty (Encore writes the language of every audio stream).
    /// `None` follows DiscCraft's display language when the disc is built (the build is told it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_language: Option<String>,
    /// Where a dual-layer DVD (DVD-9) goes from layer 0 to layer 1 (Encore's ブレークポイント 手動): a point of a
    /// timeline, where the build starts a cell (and a GOP) so the break can fall there. `None` lets the build
    /// choose (自動: the first cell boundary past the middle of the image).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_break: Option<LayerBreakPoint>,
    /// The subtitle the disc selects when it starts (Encore's "Set Subtitle").
    #[serde(default, skip_serializing_if = "SubtitleDefault::is_no_change")]
    pub subtitle_default: SubtitleDefault,
    /// How automatic routing moves between buttons (Encore's Preferences ▸ Menus ▸ Button Routing; DiscCraft keeps
    /// it with the project so every computer builds the same disc).
    #[serde(default, skip_serializing_if = "crate::menu::RouteOptions::is_default")]
    pub routing: crate::menu::RouteOptions,
    /// How new buttons are named (Encore's Preferences ▸ Menus ▸ Button Sync Defaults).
    #[serde(
        default,
        skip_serializing_if = "crate::menu::ButtonDefaults::is_default"
    )]
    pub button_defaults: crate::menu::ButtonDefaults,
    /// The video of a Blu-ray disc's titles (Encore's Blu-ray transcode settings): SD MPEG-2, or HD H.264.
    #[serde(default, skip_serializing_if = "BdVideo::is_sd")]
    pub bd_video: BdVideo,
}

/// The video format of a Blu-ray disc's titles, encoded to H.264 when HD. 1080p 23.976 / 24 and the 720p formats are
/// the progressive ones the Blu-ray standard allows; 1080p 25 / 29.97 / 50 / 59.94 are outside it (computer players
/// play them, home players may not).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BdVideo {
    /// SD MPEG-2 at the TV standard's size (720 x 480 or 720 x 576), as on DVD.
    #[default]
    #[serde(rename = "sd")]
    Sd,
    /// 1920 x 1080, 23.976 frames per second.
    #[serde(rename = "1080p23.976")]
    Hd1080p23,
    /// 1920 x 1080, 24 frames per second.
    #[serde(rename = "1080p24")]
    Hd1080p24,
    /// 1920 x 1080, 25 frames per second (outside the Blu-ray standard).
    #[serde(rename = "1080p25")]
    Hd1080p25,
    /// 1920 x 1080, 29.97 frames per second (outside the Blu-ray standard).
    #[serde(rename = "1080p29.97")]
    Hd1080p29,
    /// 1920 x 1080, 50 frames per second (outside the Blu-ray standard).
    #[serde(rename = "1080p50")]
    Hd1080p50,
    /// 1920 x 1080, 59.94 frames per second (outside the Blu-ray standard).
    #[serde(rename = "1080p59.94")]
    Hd1080p59,
    /// 1280 x 720, 59.94 frames per second.
    #[serde(rename = "720p59.94")]
    Hd720p59,
    /// 1280 x 720, 50 frames per second.
    #[serde(rename = "720p50")]
    Hd720p50,
    /// 1280 x 720, 23.976 frames per second.
    #[serde(rename = "720p23.976")]
    Hd720p23,
    /// 1280 x 720, 24 frames per second.
    #[serde(rename = "720p24")]
    Hd720p24,
}

impl BdVideo {
    /// Every format, SD first.
    pub const ALL: [BdVideo; 11] = [
        BdVideo::Sd,
        BdVideo::Hd1080p23,
        BdVideo::Hd1080p24,
        BdVideo::Hd1080p25,
        BdVideo::Hd1080p29,
        BdVideo::Hd1080p50,
        BdVideo::Hd1080p59,
        BdVideo::Hd720p59,
        BdVideo::Hd720p50,
        BdVideo::Hd720p23,
        BdVideo::Hd720p24,
    ];

    /// Whether the Blu-ray standard allows the format (SD, 1080p 23.976 / 24, 720p).
    pub fn in_standard(self) -> bool {
        !matches!(
            self,
            BdVideo::Hd1080p25 | BdVideo::Hd1080p29 | BdVideo::Hd1080p50 | BdVideo::Hd1080p59
        )
    }

    /// Whether this is SD (nothing written to the project file).
    pub fn is_sd(&self) -> bool {
        *self == BdVideo::Sd
    }

    /// The name in project files and commands.
    pub fn id(self) -> &'static str {
        match self {
            BdVideo::Sd => "sd",
            BdVideo::Hd1080p23 => "1080p23.976",
            BdVideo::Hd1080p24 => "1080p24",
            BdVideo::Hd1080p25 => "1080p25",
            BdVideo::Hd1080p29 => "1080p29.97",
            BdVideo::Hd1080p50 => "1080p50",
            BdVideo::Hd1080p59 => "1080p59.94",
            BdVideo::Hd720p59 => "720p59.94",
            BdVideo::Hd720p50 => "720p50",
            BdVideo::Hd720p23 => "720p23.976",
            BdVideo::Hd720p24 => "720p24",
        }
    }

    /// The format named `id`.
    pub fn from_id(id: &str) -> Option<BdVideo> {
        BdVideo::ALL.into_iter().find(|video| video.id() == id)
    }

    /// The HD frame size and rate (numerator, denominator); `None` for SD.
    pub fn hd(self) -> Option<(u32, u32, u32, u32)> {
        match self {
            BdVideo::Sd => None,
            BdVideo::Hd1080p23 => Some((1920, 1080, 24_000, 1001)),
            BdVideo::Hd1080p24 => Some((1920, 1080, 24, 1)),
            BdVideo::Hd1080p25 => Some((1920, 1080, 25, 1)),
            BdVideo::Hd1080p29 => Some((1920, 1080, 30_000, 1001)),
            BdVideo::Hd1080p50 => Some((1920, 1080, 50, 1)),
            BdVideo::Hd1080p59 => Some((1920, 1080, 60_000, 1001)),
            BdVideo::Hd720p59 => Some((1280, 720, 60_000, 1001)),
            BdVideo::Hd720p50 => Some((1280, 720, 50, 1)),
            BdVideo::Hd720p23 => Some((1280, 720, 24_000, 1001)),
            BdVideo::Hd720p24 => Some((1280, 720, 24, 1)),
        }
    }
}

/// What a disc does with subtitles when it starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SubtitleDefault {
    /// Leave the player's choice.
    #[default]
    NoChange,
    /// Turn subtitles off.
    Off,
    /// Show subtitle track n (1 to 32).
    Stream(u8),
}

impl SubtitleDefault {
    /// Whether this is the default (nothing written to the project file).
    pub fn is_no_change(&self) -> bool {
        *self == SubtitleDefault::NoChange
    }
}

impl DiscSettings {
    /// Frame rate implied by the video standard.
    pub fn frame_rate(&self) -> FrameRate {
        match self.standard {
            VideoStandard::Ntsc => FrameRate::FPS_29_97,
            VideoStandard::Pal => FrameRate::FPS_25,
        }
    }
}

/// Broad asset category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AssetKind {
    /// Video material.
    Video,
    /// Audio material.
    Audio,
    /// Still image material.
    Image,
    /// Subtitle material.
    Subtitle,
    /// Unknown or other material.
    #[default]
    Other,
}

impl AssetKind {
    /// Guess an asset kind from a file extension.
    pub fn guess_from_path(path: &Path) -> Self {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        match ext.as_deref() {
            Some("mp4") | Some("mov") | Some("mkv") | Some("mxf") | Some("m2v") | Some("mpg")
            | Some("mpeg") | Some("vob") | Some("m2ts") | Some("ts") | Some("avi")
            // EffectCraft compositions are rendered to video.
            | Some("ecproj") => {
                AssetKind::Video
            }
            Some("wav") | Some("ac3") | Some("aif") | Some("aiff") | Some("mp3") | Some("m4a")
            // SoundCraft sessions are mixed down to audio.
            | Some("flac") | Some("scraft") => AssetKind::Audio,
            Some("png") | Some("jpg") | Some("jpeg") | Some("bmp") | Some("tif") | Some("tiff")
            // Vector drawings are rendered at the size they are shown at.
            | Some("psd") | Some("svg") => AssetKind::Image,
            // Subtitle files; a .txt is read as a timecoded subtitle script.
            Some("srt") | Some("vtt") | Some("txt") => AssetKind::Subtitle,
            _ => AssetKind::Other,
        }
    }
}

/// Imported media asset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    /// Shared object id.
    pub id: AssetId,
    /// Display name.
    pub name: String,
    /// Stored path string.
    pub path: String,
    /// Asset category.
    pub kind: AssetKind,
    /// Whether the file was unavailable when added.
    pub offline: bool,
    /// Free description (kept in the project only).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// Timeline chapter marker.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    /// Chapter start time.
    #[serde(default)]
    pub time: Tick,
    /// Chapter name.
    #[serde(default)]
    pub name: String,
    /// Poster frame (Encore): the frame that stands for the chapter in thumbnails. `None` uses the chapter's
    /// own frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<Tick>,
}

/// Audio track reference.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioTrack {
    /// Referenced asset.
    pub asset: AssetId,
    /// Language code.
    #[serde(default)]
    pub language: String,
}

/// Subtitle track reference.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleTrack {
    /// Referenced asset.
    pub asset: AssetId,
    /// Language code.
    #[serde(default)]
    pub language: String,
}

/// One authored title timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
    /// Shared object id.
    pub id: TimelineId,
    /// Display name.
    pub name: String,
    /// Primary video asset.
    pub video: AssetId,
    /// Audio tracks.
    #[serde(default)]
    pub audio: Vec<AudioTrack>,
    /// Subtitle tracks.
    #[serde(default)]
    pub subtitles: Vec<SubtitleTrack>,
    /// Chapters, ascending, first at zero.
    #[serde(default)]
    pub chapters: Vec<Chapter>,
    /// Action at timeline end.
    #[serde(default)]
    pub end_action: Link,
    /// Menu remote target.
    #[serde(default)]
    pub menu_remote: Link,
    /// Optional override link.
    #[serde(default)]
    pub override_link: Option<Link>,
    /// Free description (kept in the project only).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Remote operations prohibited while the timeline plays (DVD).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prohibited_ops: Vec<crate::UserOp>,
    /// 16:9 (anamorphic) instead of 4:3 for this timeline (Encore's timeline aspect). A timeline made by a command
    /// takes its video's aspect; `None` follows the disc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widescreen: Option<bool>,
}

impl Timeline {
    /// Whether this timeline is 16:9: its own choice, else the disc's.
    pub fn wide(&self, settings: &DiscSettings) -> bool {
        self.widescreen.unwrap_or(settings.widescreen)
    }
}

pub use crate::menu::Menu;

/// Complete project document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// Disc settings.
    #[serde(default)]
    pub settings: DiscSettings,
    /// Assets by id.
    #[serde(default)]
    pub assets: BTreeMap<AssetId, Asset>,
    /// Timelines by id.
    #[serde(default)]
    pub timelines: BTreeMap<TimelineId, Timeline>,
    /// Menus by id.
    #[serde(default)]
    pub menus: BTreeMap<MenuId, Menu>,
    /// Next shared id.
    /// Slideshows (still images played as a title).
    #[serde(default)]
    pub slideshows: BTreeMap<u64, crate::lists::Slideshow>,
    /// Playlists (timelines played in sequence).
    #[serde(default)]
    pub playlists: BTreeMap<u64, crate::lists::Playlist>,
    /// Chapter playlists (the chapters of one timeline in any order).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub chapter_playlists: BTreeMap<u64, crate::chapter_playlist::ChapterPlaylist>,
    /// Folders of the Project panel.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub folders: BTreeMap<u64, crate::folder::Folder>,
    /// Which folder each item sits in (item id → folder id); items not listed are at the root.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub placements: BTreeMap<u64, u64>,
    /// Where the Flowchart panel draws the nodes that were moved by hand (item id, [`FLOWCHART_DISC`] for the
    /// disc → the node's top-left). Nodes not listed are laid out automatically.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub flowchart: BTreeMap<u64, FlowPoint>,
    #[serde(default = "default_next_id")]
    pub next_id: u64,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            settings: DiscSettings::default(),
            assets: BTreeMap::new(),
            timelines: BTreeMap::new(),
            menus: BTreeMap::new(),
            slideshows: BTreeMap::new(),
            playlists: BTreeMap::new(),
            chapter_playlists: BTreeMap::new(),
            folders: BTreeMap::new(),
            placements: BTreeMap::new(),
            flowchart: BTreeMap::new(),
            next_id: 1,
        }
    }
}

fn default_next_id() -> u64 {
    1
}

/// A manual layer break: the time of a timeline where layer 1 begins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerBreakPoint {
    /// The timeline.
    pub timeline: TimelineId,
    /// The time in the timeline (the build takes the frame nearest to it).
    pub time: Tick,
}

/// The key of the disc node (First Play / Title button) in [`Project::flowchart`]; item ids start at 1.
pub const FLOWCHART_DISC: u64 = 0;
/// The largest coordinate of a moved flowchart node, in points.
pub const FLOWCHART_LIMIT: i32 = 100_000;

/// A moved flowchart node's top-left, in points from the top-left of the flowchart (0 to [`FLOWCHART_LIMIT`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowPoint {
    pub x: i32,
    pub y: i32,
}

impl Project {
    /// Whether `id` names a node of the flowchart: the disc ([`FLOWCHART_DISC`]), a menu, timeline, slideshow,
    /// playlist or chapter playlist.
    pub fn flowchart_node_exists(&self, id: u64) -> bool {
        id == FLOWCHART_DISC
            || self.menus.contains_key(&MenuId(id))
            || self.timelines.contains_key(&TimelineId(id))
            || self.slideshows.contains_key(&id)
            || self.playlists.contains_key(&id)
            || self.chapter_playlists.contains_key(&id)
    }
}

impl Project {
    /// Allocate the next shared object id.
    pub fn alloc_id(&mut self) -> Result<u64, ProjectError> {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(ProjectError::IdOverflow)?;
        Ok(id)
    }

    /// Add an asset from a filesystem path.
    /// Points an asset at another file (Encore's "Replace Asset" and "Locate Asset"): the links, name and
    /// description stay; the file must be the same kind of media. An asset found again is no longer offline.
    pub fn relink_asset(&mut self, id: AssetId, path: &Path) -> Result<(), ProjectError> {
        if !path.is_file() {
            return Err(ProjectError::Invalid(format!(
                "file does not exist: {}",
                path.display()
            )));
        }
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let kind = AssetKind::guess_from_path(&absolute);
        let asset = self
            .assets
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("asset does not exist".to_string()))?;
        if kind != asset.kind {
            return Err(ProjectError::Invalid(
                "the new file must be the same kind of media".to_string(),
            ));
        }
        asset.path = absolute.to_string_lossy().into_owned();
        asset.offline = false;
        Ok(())
    }

    pub fn add_asset(&mut self, path: &Path) -> Result<AssetId, ProjectError> {
        let id = crate::ids::AssetId(self.alloc_id()?);
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let path_string = absolute.to_string_lossy().into_owned();
        let name = absolute
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path_string.clone());
        let kind = AssetKind::guess_from_path(&absolute);
        let offline = !path.is_file();

        self.assets.insert(
            id,
            Asset {
                id,
                name,
                path: path_string,
                kind,
                offline,
                description: String::new(),
            },
        );
        Ok(id)
    }

    /// Add a timeline referencing an existing video asset.
    pub fn add_timeline(
        &mut self,
        video: AssetId,
        name: Option<String>,
    ) -> Result<TimelineId, ProjectError> {
        if !self.assets.contains_key(&video) {
            return Err(ProjectError::Invalid(
                "video asset does not exist".to_string(),
            ));
        }

        let id = crate::ids::TimelineId(self.alloc_id()?);
        let default_name = self
            .assets
            .get(&video)
            .map(|a| a.name.clone())
            .unwrap_or_default();
        let name = name.unwrap_or(default_name);

        self.timelines.insert(
            id,
            Timeline {
                id,
                name,
                video,
                audio: Vec::new(),
                subtitles: Vec::new(),
                chapters: vec![Chapter {
                    time: Tick::ZERO,
                    name: "Chapter 1".to_string(),
                    poster: None,
                }],
                end_action: Link::NotSet,
                menu_remote: Link::NotSet,
                override_link: None,
                prohibited_ops: Vec::new(),
                description: String::new(),
                widescreen: None,
            },
        );
        Ok(id)
    }

    /// Add a chapter at a positive time, keeping ascending order.
    pub fn add_chapter(
        &mut self,
        tl: TimelineId,
        time: Tick,
        name: Option<String>,
    ) -> Result<usize, ProjectError> {
        let timeline = self
            .timelines
            .get_mut(&tl)
            .ok_or_else(|| ProjectError::Invalid("timeline does not exist".to_string()))?;

        if time <= Tick::ZERO {
            return Err(ProjectError::Invalid(
                "chapter time must be positive".to_string(),
            ));
        }
        if timeline.chapters.iter().any(|c| c.time == time) {
            return Err(ProjectError::Invalid(
                "chapter time already exists".to_string(),
            ));
        }

        let pos = timeline
            .chapters
            .iter()
            .position(|c| c.time > time)
            .unwrap_or(timeline.chapters.len());
        let name = name.unwrap_or_else(|| format!("Chapter {}", pos.saturating_add(1)));
        timeline.chapters.insert(
            pos,
            Chapter {
                time,
                name,
                poster: None,
            },
        );
        Ok(pos)
    }

    /// Remove a chapter by index; the first chapter cannot be removed.
    pub fn remove_chapter(&mut self, tl: TimelineId, index: usize) -> Result<(), ProjectError> {
        let timeline = self
            .timelines
            .get_mut(&tl)
            .ok_or_else(|| ProjectError::Invalid("timeline does not exist".to_string()))?;

        if index == 0 {
            return Err(ProjectError::Invalid(
                "cannot remove the first chapter".to_string(),
            ));
        }
        if index >= timeline.chapters.len() {
            return Err(ProjectError::Invalid(
                "chapter index out of range".to_string(),
            ));
        }

        timeline.chapters.remove(index);
        self.drop_chapter_from_playlists(tl, index);
        Ok(())
    }

    /// Move a chapter to a new time; the first chapter cannot move.
    pub fn move_chapter(
        &mut self,
        tl: TimelineId,
        index: usize,
        time: Tick,
    ) -> Result<usize, ProjectError> {
        let timeline = self
            .timelines
            .get_mut(&tl)
            .ok_or_else(|| ProjectError::Invalid("timeline does not exist".to_string()))?;

        if index == 0 {
            return Err(ProjectError::Invalid(
                "cannot move the first chapter".to_string(),
            ));
        }
        if index >= timeline.chapters.len() {
            return Err(ProjectError::Invalid(
                "chapter index out of range".to_string(),
            ));
        }
        if time <= Tick::ZERO {
            return Err(ProjectError::Invalid(
                "chapter time must be positive".to_string(),
            ));
        }
        if timeline
            .chapters
            .iter()
            .enumerate()
            .any(|(i, c)| i != index && c.time == time)
        {
            return Err(ProjectError::Invalid(
                "chapter time already exists".to_string(),
            ));
        }

        let Some(old) = timeline.chapters.get(index).cloned() else {
            return Err(ProjectError::Invalid(
                "chapter index out of range".to_string(),
            ));
        };

        timeline.chapters.remove(index);
        let pos = timeline
            .chapters
            .iter()
            .position(|c| c.time > time)
            .unwrap_or(timeline.chapters.len());
        timeline.chapters.insert(
            pos,
            Chapter {
                time,
                name: old.name,
                poster: old.poster,
            },
        );
        Ok(pos)
    }

    /// Rename a chapter. Surrounding whitespace is dropped; an empty name falls back to the numbered label in the
    /// user interface.
    pub fn rename_chapter(
        &mut self,
        tl: TimelineId,
        index: usize,
        name: &str,
    ) -> Result<(), ProjectError> {
        let timeline = self
            .timelines
            .get_mut(&tl)
            .ok_or_else(|| ProjectError::Invalid("timeline does not exist".to_string()))?;
        let chapter = timeline
            .chapters
            .get_mut(index)
            .ok_or_else(|| ProjectError::Invalid("chapter index out of range".to_string()))?;
        chapter.name = name.trim().to_string();
        Ok(())
    }

    /// Set or clear a chapter's poster frame.
    pub fn set_poster_frame(
        &mut self,
        tl: TimelineId,
        index: usize,
        poster: Option<Tick>,
    ) -> Result<(), ProjectError> {
        let timeline = self
            .timelines
            .get_mut(&tl)
            .ok_or_else(|| ProjectError::Invalid("timeline does not exist".to_string()))?;
        let chapter = timeline
            .chapters
            .get_mut(index)
            .ok_or_else(|| ProjectError::Invalid("chapter index out of range".to_string()))?;
        if poster.is_some_and(|poster| poster < Tick::ZERO) {
            return Err(ProjectError::Invalid(
                "poster frame must not be negative".to_string(),
            ));
        }
        chapter.poster = poster;
        Ok(())
    }

    /// Borrow a timeline.
    pub fn timeline(&self, id: TimelineId) -> Option<&Timeline> {
        self.timelines.get(&id)
    }

    /// Mutably borrow a timeline.
    pub fn timeline_mut(&mut self, id: TimelineId) -> Option<&mut Timeline> {
        self.timelines.get_mut(&id)
    }
}

#[cfg(test)]
mod flowchart_tests {
    use super::{FLOWCHART_DISC, FlowPoint, Project};
    use std::path::Path;

    #[test]
    fn moved_flowchart_nodes_are_saved_only_when_there_are_any()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let text = serde_json::to_string(&project)?;
        assert!(!text.contains("flowchart"), "{text}");

        let asset = project.add_asset(Path::new("movie.m2v"))?;
        let timeline = project.add_timeline(asset, None)?;
        assert!(project.flowchart_node_exists(FLOWCHART_DISC));
        assert!(project.flowchart_node_exists(timeline.0));
        assert!(!project.flowchart_node_exists(asset.0));
        assert!(!project.flowchart_node_exists(999));

        project
            .flowchart
            .insert(timeline.0, FlowPoint { x: 40, y: 300 });
        let text = serde_json::to_string(&project)?;
        assert!(
            text.contains(&format!(
                "\"flowchart\":{{\"{}\":{{\"x\":40,\"y\":300}}}}",
                timeline.0
            )),
            "{text}"
        );
        let back: Project = serde_json::from_str(&text)?;
        assert_eq!(
            back.flowchart.get(&timeline.0),
            Some(&FlowPoint { x: 40, y: 300 })
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn bdxl_capacities_are_offered_for_blu_ray() -> Result<(), Box<dyn std::error::Error>> {
        use super::{Capacity, DiscKind};
        assert_eq!(
            Capacity::choices(DiscKind::BluRay).last(),
            Some(&Capacity::Bd128)
        );
        assert!(!Capacity::choices(DiscKind::Dvd).contains(&Capacity::Bd100));
        assert_eq!(Capacity::Bd128.bytes(), 128_001_769_472);
        // Saved as "bd100" / "bd128".
        assert_eq!(serde_json::to_string(&Capacity::Bd100)?, "\"bd100\"");
        let parsed: Capacity = serde_json::from_str("\"bd128\"")?;
        assert_eq!(parsed, Capacity::Bd128);
        Ok(())
    }

    use super::{AssetKind, Project, Tick, TimelineId};
    use crate::AssetId;
    use std::error::Error;
    use std::path::Path;

    #[test]
    fn shared_ids_and_chapters() -> Result<(), Box<dyn Error>> {
        let mut p = Project::default();

        let asset = p.add_asset(Path::new("movie.mp4"))?;
        assert_eq!(asset, AssetId(1));

        let tl = p.add_timeline(asset, None)?;
        assert_eq!(tl, TimelineId(2));
        assert_eq!(p.alloc_id()?, 3);

        let idx = p.add_chapter(tl, Tick(100), None)?;
        assert_eq!(idx, 1);
        let idx = p.add_chapter(tl, Tick(50), None)?;
        assert_eq!(idx, 1);
        let idx = p.add_chapter(tl, Tick(150), None)?;
        assert_eq!(idx, 3);

        let tl_ref = p.timeline(tl).ok_or("missing timeline")?;
        let times: Vec<Tick> = tl_ref.chapters.iter().map(|c| c.time).collect();
        assert_eq!(times, vec![Tick::ZERO, Tick(50), Tick(100), Tick(150)]);

        assert!(p.add_chapter(tl, Tick(50), None).is_err());
        assert!(p.remove_chapter(tl, 0).is_err());
        assert!(p.move_chapter(tl, 0, Tick(200)).is_err());

        let new_idx = p.move_chapter(tl, 3, Tick(75))?;
        assert_eq!(new_idx, 2);

        let tl_ref = p.timeline(tl).ok_or("missing timeline")?;
        let times: Vec<Tick> = tl_ref.chapters.iter().map(|c| c.time).collect();
        assert_eq!(times, vec![Tick::ZERO, Tick(50), Tick(75), Tick(100)]);

        Ok(())
    }

    #[test]
    fn rename_chapter_trims_and_checks_the_index() -> Result<(), Box<dyn Error>> {
        let mut p = Project::default();
        let asset = p.add_asset(Path::new("movie.mp4"))?;
        let tl = p.add_timeline(asset, None)?;
        let idx = p.add_chapter(tl, Tick(100), None)?;

        p.rename_chapter(tl, idx, "  Opening  ")?;
        let name = p
            .timeline(tl)
            .and_then(|timeline| timeline.chapters.get(idx))
            .map(|chapter| chapter.name.clone());
        assert_eq!(name.as_deref(), Some("Opening"));

        assert!(p.rename_chapter(tl, 9, "x").is_err());
        assert!(p.rename_chapter(TimelineId(99), 0, "x").is_err());
        Ok(())
    }

    #[test]
    fn asset_kind_guess() {
        assert_eq!(
            AssetKind::guess_from_path(Path::new("a.MP4")),
            AssetKind::Video
        );
        assert_eq!(
            AssetKind::guess_from_path(Path::new("a.wav")),
            AssetKind::Audio
        );
        assert_eq!(
            AssetKind::guess_from_path(Path::new("a.PNG")),
            AssetKind::Image
        );
        assert_eq!(
            AssetKind::guess_from_path(Path::new("intro.ecproj")),
            AssetKind::Video
        );
        assert_eq!(
            AssetKind::guess_from_path(Path::new("score.scraft")),
            AssetKind::Audio
        );
        assert_eq!(
            AssetKind::guess_from_path(Path::new("menu.psd")),
            AssetKind::Image
        );
        assert_eq!(
            AssetKind::guess_from_path(Path::new("a.srt")),
            AssetKind::Subtitle
        );
        assert_eq!(
            AssetKind::guess_from_path(Path::new("a.unknown")),
            AssetKind::Other
        );
    }
}
