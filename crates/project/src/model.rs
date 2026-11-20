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

    /// The sizes a custom capacity can be on a disc kind, in bytes (Encore's Size ▸ Custom): from 1 MB to a
    /// single-layer DVD (a larger DVD is DVD9, whose layer break is set) or a four-layer BDXL.
    pub fn custom_range(kind: DiscKind) -> std::ops::RangeInclusive<u64> {
        match kind {
            DiscKind::Dvd => 1_000_000..=Capacity::Dvd5.bytes(),
            DiscKind::BluRay => 1_000_000..=Capacity::Bd128.bytes(),
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
    /// The parental level of a DVD (1 to 8; 0: none). A player set to a lower level asks for its password before
    /// the disc plays, and stops when it is not given.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub parental_level: u8,
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
    /// How a Blu-ray disc's HD titles are coded (Encore's Project Settings ▸ Codec, M12.28): automatically (H.264,
    /// MPEG-2 for 1080i), H.264 (1080i too, coded interlaced), or MPEG-2 (for 1080i; the progressive formats stay
    /// H.264).
    #[serde(default, skip_serializing_if = "BdCodec::is_auto")]
    pub bd_codec: BdCodec,
    /// The folder whose files go on the disc beside the video, for a computer to read (Encore's Build panel:
    /// DVD-ROM content / BD-ROM content). Its files are in the disc's root, its folders with them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rom_content: Option<String>,
}

/// The coding of a Blu-ray disc's HD video (Encore's Project Settings ▸ Codec, M12.28).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BdCodec {
    /// H.264, and MPEG-2 for the interlaced 1080 formats (as before M12.28).
    #[default]
    Auto,
    /// H.264 for every format, the interlaced ones coded as interlaced frames.
    H264,
    /// MPEG-2 for the interlaced 1080 formats; DiscCraft codes the progressive formats as H.264 still.
    Mpeg2,
}

impl BdCodec {
    /// Every choice, in Encore's order after the automatic one.
    pub const ALL: [BdCodec; 3] = [BdCodec::Auto, BdCodec::H264, BdCodec::Mpeg2];

    /// The default: automatic.
    #[must_use]
    pub fn is_auto(&self) -> bool {
        *self == BdCodec::Auto
    }

    /// Whether HD video `video` is coded as MPEG-2 on a disc of this choice (else H.264).
    #[must_use]
    pub fn mpeg2(self, video: BdVideo) -> bool {
        match self {
            BdCodec::Auto | BdCodec::Mpeg2 => video.interlaced(),
            BdCodec::H264 => false,
        }
    }

    /// The name in command parameters.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            BdCodec::Auto => "auto",
            BdCodec::H264 => "h264",
            BdCodec::Mpeg2 => "mpeg2",
        }
    }

    /// The choice named `id` ([`BdCodec::id`]).
    #[must_use]
    pub fn from_id(id: &str) -> Option<BdCodec> {
        BdCodec::ALL.into_iter().find(|codec| codec.id() == id)
    }
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
    /// 1920 x 1080 interlaced, 59.94 fields (29.97 frames) per second, MPEG-2.
    /// (`1080p29.97` in a project file is read as this: the progressive format DiscCraft once offered is outside
    /// the Blu-ray standard, which has 29.97 frames a second at 1920 x 1080 only as 59.94i.)
    #[serde(rename = "1080i59.94", alias = "1080p29.97")]
    Hd1080i59,
    /// 1920 x 1080 interlaced, 50 fields (25 frames) per second, MPEG-2.
    /// (`1080p25` in a project file is read as this.)
    #[serde(rename = "1080i50", alias = "1080p25")]
    Hd1080i50,
    /// 1280 x 720, 59.94 frames per second. (`1080p59.94` in a project file, a format outside the Blu-ray standard
    /// that DiscCraft once offered, is read as this: the standard's format with that many frames a second.)
    #[serde(rename = "720p59.94", alias = "1080p59.94")]
    Hd720p59,
    /// 1280 x 720, 50 frames per second. (`1080p50` in a project file is read as this.)
    #[serde(rename = "720p50", alias = "1080p50")]
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
    pub const ALL: [BdVideo; 9] = [
        BdVideo::Sd,
        BdVideo::Hd1080p23,
        BdVideo::Hd1080p24,
        BdVideo::Hd1080i59,
        BdVideo::Hd1080i50,
        BdVideo::Hd720p59,
        BdVideo::Hd720p50,
        BdVideo::Hd720p23,
        BdVideo::Hd720p24,
    ];

    /// Whether the format is interlaced 1920 x 1080 (59.94i / 50i): its frames, 29.97 or 25 a second, are coded
    /// as MPEG-2 (the H.264 encoder codes progressive frames only).
    pub fn interlaced(self) -> bool {
        matches!(self, BdVideo::Hd1080i59 | BdVideo::Hd1080i50)
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
            BdVideo::Hd1080i59 => "1080i59.94",
            BdVideo::Hd1080i50 => "1080i50",
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

    /// The HD frame size and frame rate (numerator, denominator); `None` for SD. An interlaced format's rate is
    /// its frames' (29.97 for 59.94i, 25 for 50i).
    pub fn hd(self) -> Option<(u32, u32, u32, u32)> {
        match self {
            BdVideo::Sd => None,
            BdVideo::Hd1080p23 => Some((1920, 1080, 24_000, 1001)),
            BdVideo::Hd1080p24 => Some((1920, 1080, 24, 1)),
            BdVideo::Hd1080i59 => Some((1920, 1080, 30_000, 1001)),
            BdVideo::Hd1080i50 => Some((1920, 1080, 25, 1)),
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

impl Asset {
    /// The path the asset's media is opened by: the file's path, or for an EffectCraft project shown as a chosen
    /// composition a name under the project file (`<project>.ecproj/<composition>.ecproj`; no such file exists).
    /// The media reader (`disccraft-transcode`) takes that name apart again, so two assets of one project file can
    /// show different compositions. Characters that would split the name (`/`, `\`, `%`) are written `%XX`.
    #[must_use]
    pub fn media_path(&self) -> std::borrow::Cow<'_, str> {
        let is_composition_project = std::path::Path::new(&self.path)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("ecproj"));
        match self.composition.as_deref() {
            Some(name) if is_composition_project && !name.is_empty() => {
                let mut media = String::with_capacity(self.path.len().saturating_add(name.len()));
                media.push_str(&self.path);
                media.push(std::path::MAIN_SEPARATOR);
                for character in name.chars() {
                    match character {
                        '/' => media.push_str("%2F"),
                        '\\' => media.push_str("%5C"),
                        '%' => media.push_str("%25"),
                        other => media.push(other),
                    }
                }
                media.push_str(".ecproj");
                std::borrow::Cow::Owned(media)
            }
            _ => std::borrow::Cow::Borrowed(&self.path),
        }
    }

    /// [`Asset::media_path`] as a path.
    #[must_use]
    pub fn media_file(&self) -> std::borrow::Cow<'_, std::path::Path> {
        match self.media_path() {
            std::borrow::Cow::Borrowed(path) => {
                std::borrow::Cow::Borrowed(std::path::Path::new(path))
            }
            std::borrow::Cow::Owned(path) => {
                std::borrow::Cow::Owned(std::path::PathBuf::from(path))
            }
        }
    }
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
            | Some("mpeg") | Some("vob") | Some("m2ts") | Some("ts") | Some("avi") | Some("m4v")
            | Some("webm") | Some("mts") | Some("m2t") | Some("m1v") | Some("mpv")
            // EffectCraft compositions are rendered to video.
            | Some("ecproj") => {
                AssetKind::Video
            }
            Some("wav") | Some("ac3") | Some("dts") | Some("aif") | Some("aiff") | Some("mp3") | Some("mpa")
            | Some("m4a")
            // SoundCraft sessions are mixed down to audio.
            | Some("flac") | Some("scraft")
            // More sound files a tester may have: all go through the same decoders.
            | Some("aac") | Some("mka") | Some("mp2") | Some("ogg") | Some("oga") | Some("opus")
            => AssetKind::Audio,
            Some("png") | Some("jpg") | Some("jpeg") | Some("bmp") | Some("tif") | Some("tiff")
            // Vector drawings are rendered at the size they are shown at; PhotoCraft documents are composited.
            | Some("psd") | Some("psb") | Some("pcraft") | Some("svg") => AssetKind::Image,
            // Subtitle files; a .txt is read as a timecoded subtitle script. VobSub (.idx, with its .sub) and PGS
            // (.sup) are a DVD's and a Blu-ray's own subtitles, passed through.
            Some("srt") | Some("vtt") | Some("txt") | Some("idx") | Some("sup") => AssetKind::Subtitle,
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
    /// A subtitle asset's cues as edited in the project (Encore's subtitle clips added and trimmed in the Monitor):
    /// they take the place of the file's from the first edit on, and the file is left as it is. `None`: the file's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cues: Option<Vec<SubtitleCue>>,
    /// The composition of an EffectCraft project (`.ecproj`) this asset shows; `None`: the one the project has
    /// active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition: Option<String>,
    /// The video bit rate (kbit/s) this asset is transcoded at (Encore's Transcode Settings: a quality preset of
    /// the asset); `None` is Automatic: the rate that fits the disc. It means nothing for an asset that goes on
    /// the disc as it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcode_kbps: Option<u32>,
    /// The pixel aspect ratio (width over height of a pixel) this asset's pictures are taken to have (Encore's
    /// Interpret Footage ▸ Specify pixel aspect ratio); `None` uses the file's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pixel_aspect: Option<(u32, u32)>,
    /// The files this video was transcoded to for a DVD (Encore's Locate Transcoded File ▸ DVD…): a DVD build
    /// takes them in place of the asset's own file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub located_dvd: Option<LocatedTranscode>,
    /// The same for a Blu-ray disc (Locate Transcoded File ▸ Blu-ray…).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub located_bluray: Option<LocatedTranscode>,
    /// A picture's alpha channel is ignored (Encore's Interpret Footage ▸ Ignore Alpha Channel): every pixel is
    /// opaque, in menus and slides.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ignore_alpha: bool,
    /// How a video asset's bit rate is kept when it is transcoded (Encore's quality presets: VBR 1 pass, VBR 2
    /// pass, CBR).
    #[serde(default, skip_serializing_if = "TranscodeMode::is_default")]
    pub transcode_mode: TranscodeMode,
    /// What a video asset's sound is made when it is transcoded for a DVD (Encore's transcode settings: Dolby
    /// Digital or PCM).
    #[serde(default, skip_serializing_if = "TranscodeAudio::is_default")]
    pub transcode_audio: TranscodeAudio,
    /// The name of the DVD quality preset the DVD setting above was chosen as (Encore's Transcode Settings ▸ DVD
    /// ▸ Quality Preset), for showing it; `None`: set otherwise or automatic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcode_preset: Option<String>,
    /// A Blu-ray transcode setting of the asset's own (Encore's Transcode Settings ▸ Blu-ray ▸ Quality Preset);
    /// `None`: automatic (the rate that fits the disc).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bluray_transcode: Option<BlurayTranscode>,
    /// The DVD setting is film (Encore's NTSC DV 23.976 presets): the video is made 23.976 progressive pictures a
    /// second, shown by 3:2 pulldown (an NTSC DVD only; with a rate of its own only).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub transcode_film: bool,
    /// The HD format this video's titles are made in on an HD Blu-ray (Encore's Transcode Settings ▸ Blu-ray ▸
    /// Codec / Dimensions / Frame Rate); `None`: the project's ("Use Project Settings"). Never SD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bluray_video: Option<BdVideo>,
    /// How hard the DVD transcode searches for motion: the quality of the DVD quality preset it was chosen as
    /// (Encore's "High quality" presets' 5, "Low quality" ones' 2), [`MIN_QUALITY`] to [`BEST_QUALITY`].
    #[serde(default = "best_quality", skip_serializing_if = "is_best_quality")]
    pub transcode_quality: u8,
    /// Encore's "Use Maximum Render Quality": its transcodes scale the pictures with the filtered scaler (fine
    /// detail averages out instead of aliasing).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub max_render_quality: bool,
    /// Encore's "Use Frame Blending": where its transcodes' pictures fall between two of its frames (another frame
    /// rate), the two are mixed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub frame_blend: bool,
}

/// The best transcode quality (Encore's presets' 5): the encoder's whole motion search.
pub const BEST_QUALITY: u8 = 5;

/// The lowest transcode quality (Encore's quality slider's 1).
pub const MIN_QUALITY: u8 = 1;

fn best_quality() -> u8 {
    BEST_QUALITY
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's skip_serializing_if takes a reference
fn is_best_quality(quality: &u8) -> bool {
    *quality == BEST_QUALITY
}

/// A Blu-ray transcode setting of an asset's own: its video bit rates and the preset it was chosen as.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlurayTranscode {
    /// The target video bit rate, kbit/s ([`MIN_TRANSCODE_KBPS`] to [`MAX_TRANSCODE_KBPS`]).
    pub kbps: u32,
    /// The peak video bit rate, kbit/s (at least [`BlurayTranscode::kbps`]).
    pub max_kbps: u32,
    /// The name of the quality preset it was chosen as (empty: none).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub preset: String,
    /// How hard an MPEG-2 encode searches for motion: the preset's quality (Encore's "Long Form" MPEG-2 presets'
    /// 3, the others' 5).
    #[serde(default = "best_quality", skip_serializing_if = "is_best_quality")]
    pub quality: u8,
    /// How the rate is kept: one-pass VBR (Encore's Blu-ray presets), two-pass VBR or CBR (a preset of the user's).
    #[serde(default, skip_serializing_if = "TranscodeMode::is_default")]
    pub mode: TranscodeMode,
}

/// What a video's sound is made when it is transcoded for a DVD (Encore's transcode settings name it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TranscodeAudio {
    /// Dolby Digital (AC-3), stereo at 192 kbit/s.
    #[default]
    DolbyDigital,
    /// Linear PCM, 48 kHz 16-bit stereo, as converted (1.536 Mbit/s: it takes more of the disc and leaves the
    /// video less).
    Pcm,
}

impl TranscodeAudio {
    /// Every choice.
    pub const ALL: [TranscodeAudio; 2] = [TranscodeAudio::DolbyDigital, TranscodeAudio::Pcm];

    /// The name in command parameters.
    pub fn as_str(self) -> &'static str {
        match self {
            TranscodeAudio::DolbyDigital => "dolbyDigital",
            TranscodeAudio::Pcm => "pcm",
        }
    }

    /// Parses a command parameter.
    pub fn parse(text: &str) -> Option<TranscodeAudio> {
        Self::ALL.into_iter().find(|audio| audio.as_str() == text)
    }

    /// Dolby Digital (nothing to save).
    pub fn is_default(&self) -> bool {
        *self == TranscodeAudio::DolbyDigital
    }
}

/// How a video's bit rate is kept when it is transcoded (Encore's quality presets name it: "VBR 2 Pass", "CBR 1
/// Pass").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TranscodeMode {
    /// Variable, in one pass: the rate averages the target, up to the maximum.
    #[default]
    Vbr,
    /// Variable, in two passes: a first pass measures the pictures, the second shares the bits by what it measured
    /// (a hard scene gets more, an easy one fewer; it takes twice as long).
    Vbr2Pass,
    /// Constant: the target is also the maximum.
    Cbr,
}

impl TranscodeMode {
    /// Every choice.
    pub const ALL: [TranscodeMode; 3] = [
        TranscodeMode::Vbr,
        TranscodeMode::Vbr2Pass,
        TranscodeMode::Cbr,
    ];

    /// The name in command parameters.
    pub fn as_str(self) -> &'static str {
        match self {
            TranscodeMode::Vbr => "vbr",
            TranscodeMode::Vbr2Pass => "vbr2Pass",
            TranscodeMode::Cbr => "cbr",
        }
    }

    /// Parses a command parameter.
    pub fn parse(text: &str) -> Option<TranscodeMode> {
        Self::ALL.into_iter().find(|mode| mode.as_str() == text)
    }

    /// One-pass VBR (nothing to save).
    pub fn is_default(&self) -> bool {
        *self == TranscodeMode::Vbr
    }
}

/// Files a video was transcoded to elsewhere (or by an earlier build whose source is gone), pointed at with
/// Encore's Locate Transcoded File.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocatedTranscode {
    /// The video (for a DVD, MPEG-2 the disc takes as it is).
    pub video: String,
    /// Its sound (AC-3, WAV …), which the timelines that play the video without audio tracks of their own take as
    /// their soundtrack. `None`: the video's own sound, if it has any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
}

impl Asset {
    /// The transcoded files located for a disc of `kind`.
    #[must_use]
    pub fn located(&self, kind: DiscKind) -> Option<&LocatedTranscode> {
        match kind {
            DiscKind::Dvd => self.located_dvd.as_ref(),
            DiscKind::BluRay => self.located_bluray.as_ref(),
        }
    }

    /// The transcoded files located for a disc of `kind`, to change.
    pub fn located_mut(&mut self, kind: DiscKind) -> &mut Option<LocatedTranscode> {
        match kind {
            DiscKind::Dvd => &mut self.located_dvd,
            DiscKind::BluRay => &mut self.located_bluray,
        }
    }
}

/// The lowest video bit rate an asset's transcode setting takes, kbit/s.
pub const MIN_TRANSCODE_KBPS: u32 = 1_000;
/// The highest video bit rate an asset's transcode setting takes, kbit/s (a Blu-ray's video; a DVD's stops at
/// what its multiplex leaves the video).
pub const MAX_TRANSCODE_KBPS: u32 = 40_000;

/// One subtitle (a cue) edited in the project: shown from `start` until `end`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleCue {
    /// First tick at which it is shown.
    pub start: Tick,
    /// First tick at which it is no longer shown.
    pub end: Tick,
    /// The text, one line per line of the subtitle.
    pub text: String,
    /// The subtitle's own colour group, stroke and place (Encore's subtitle clip properties).
    #[serde(default, flatten)]
    pub look: CueLook,
    /// The picture this subtitle shows instead of text (one read from an image script or a FAB image script);
    /// `None` for a text subtitle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<CueImage>,
}

/// A subtitle that is a picture (Encore's Timeline ▸ Import Subtitles ▸ Image Script / FAB Image Script): shown as
/// it is drawn, in the subtitle colours its own colours are mapped to. Its text cannot be edited, as in Encore.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CueImage {
    /// The picture file (JPG, GIF, PNG, TIF or BMP).
    pub path: String,
    /// Where it stands: left, top, right and bottom in the frame's pixels. The picture's top left corner is put
    /// at the left top; what runs past the right or the bottom is cut.
    pub area: [u16; 4],
    /// Which of its colours are the background, the fill and the outline.
    pub colors: ImageCueColors,
}

/// Encore's Map Colors for picture subtitles: the picture's colour taken as the background (transparent), as the
/// fill (Color 1) and as the outline (Color 2). Any other colour of the picture is transparent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageCueColors {
    /// The colour shown transparent.
    pub background: [u8; 3],
    /// The colour drawn in the colour group's fill.
    pub fill: [u8; 3],
    /// The colour drawn in the colour group's stroke.
    pub outline: [u8; 3],
}

/// How one subtitle is drawn where it differs from its track: Encore's Highlight (colour group), Stroke and
/// Alignment of a subtitle clip. `None` takes the track's group, the medium stroke and the bottom.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CueLook {
    /// The colour group (from 0) of the timeline colour set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<u8>,
    /// The outline around the letters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<crate::SlideStroke>,
    /// Where it sits from the top (0) to the bottom (100), in percent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<u8>,
    /// Which side its lines are set to (Encore: the paragraph alignment of the subtitle's text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub justify: Option<SubtitleJustify>,
}

/// The side a subtitle's lines are set to, inside the title-safe width.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SubtitleJustify {
    /// From the left edge of the title-safe area (Encore's default for a new subtitle).
    Left,
    /// Centred (DiscCraft's default, as subtitle files are usually shown).
    #[default]
    Center,
    /// To the right edge of the title-safe area.
    Right,
}

impl SubtitleJustify {
    /// Every choice, left to right.
    pub const ALL: [SubtitleJustify; 3] = [
        SubtitleJustify::Left,
        SubtitleJustify::Center,
        SubtitleJustify::Right,
    ];

    /// The name in command parameters.
    pub fn as_str(self) -> &'static str {
        match self {
            SubtitleJustify::Left => "left",
            SubtitleJustify::Center => "center",
            SubtitleJustify::Right => "right",
        }
    }

    /// Parses a command parameter.
    pub fn parse(text: &str) -> Option<SubtitleJustify> {
        Self::ALL.into_iter().find(|side| side.as_str() == text)
    }
}

impl CueLook {
    /// Whether the values are ones a subtitle can have (a group of the set, at most 100 percent).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.group
            .is_none_or(|group| group < SubtitleColorSet::GROUPS)
            && self.align.is_none_or(|align| align <= 100)
    }

    /// The outline's width in pixels (the medium stroke unless the subtitle has its own).
    #[must_use]
    pub fn outline(&self) -> u8 {
        self.stroke.unwrap_or_default().pixels()
    }

    /// The place from the top in percent (the bottom unless the subtitle has its own).
    #[must_use]
    pub fn vertical(&self) -> u8 {
        self.align.unwrap_or(100).min(100)
    }
}

/// The most cues a subtitle asset keeps in the project.
pub const MAX_SUBTITLE_CUES: usize = 20_000;

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
    /// Where the sound starts on the timeline, in frames of the disc's standard rate (Encore: an audio clip moved
    /// along its track). The track is silent before it.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub start: u32,
    /// How much of the file's head the clip leaves out, in frames of the disc's standard rate (Encore: an audio
    /// clip's in point trimmed).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub trim_start: u32,
    /// The frame of the file (of the disc's standard rate) the clip ends in front of (its out point trimmed);
    /// `None` plays to the end of the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim_end: Option<u32>,
    /// The clips after the first on this track (Encore: several audio clips on one track), in any order; the
    /// fields above are the first clip's. The clips of a track do not overlap ([`AudioTrack::clips`] gives them
    /// in the order they sound).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub more: Vec<AudioClip>,
}

/// One clip of an audio track: a part of a sound file at a place on the timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClip {
    /// The sound file.
    pub asset: AssetId,
    /// Where the clip starts on the timeline, in frames of the disc's standard rate.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub start: u32,
    /// How much of the file's head the clip leaves out, frames.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub trim_start: u32,
    /// The frame of the file the clip ends in front of; `None` plays to the end of the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim_end: Option<u32>,
}

/// The most clips on one audio track.
pub const MAX_AUDIO_CLIPS: usize = 99;

impl AudioTrack {
    /// A track of `asset`: the whole file from the start of the timeline, with no language.
    #[must_use]
    pub fn of(asset: AssetId) -> Self {
        Self {
            asset,
            language: String::new(),
            start: 0,
            trim_start: 0,
            trim_end: None,
            more: Vec::new(),
        }
    }

    /// Whether the track is one clip, the whole file from the start of the timeline: neither moved nor trimmed
    /// (a file that a disc carries as it is can then go on it so).
    #[must_use]
    pub fn is_whole(&self) -> bool {
        self.start == 0 && self.trim_start == 0 && self.trim_end.is_none() && self.more.is_empty()
    }

    /// The first clip: the track's own fields.
    #[must_use]
    pub fn first_clip(&self) -> AudioClip {
        AudioClip {
            asset: self.asset,
            start: self.start,
            trim_start: self.trim_start,
            trim_end: self.trim_end,
        }
    }

    /// Every clip of the track with its number (0 the first clip, `n` the clip `more[n - 1]`), in the order
    /// they start on the timeline.
    #[must_use]
    pub fn clips(&self) -> Vec<(usize, AudioClip)> {
        let mut clips: Vec<(usize, AudioClip)> = std::iter::once(self.first_clip())
            .chain(self.more.iter().copied())
            .enumerate()
            .collect();
        clips.sort_by_key(|(number, clip)| (clip.start, *number));
        clips
    }

    /// Clip `number` (see [`AudioTrack::clips`]).
    #[must_use]
    pub fn clip(&self, number: usize) -> Option<AudioClip> {
        match number.checked_sub(1) {
            None => Some(self.first_clip()),
            Some(index) => self.more.get(index).copied(),
        }
    }

    /// Puts `clip` in the place of clip `number`; `false` when there is no such clip.
    pub fn set_clip(&mut self, number: usize, clip: AudioClip) -> bool {
        match number.checked_sub(1) {
            None => {
                self.asset = clip.asset;
                self.start = clip.start;
                self.trim_start = clip.trim_start;
                self.trim_end = clip.trim_end;
                true
            }
            Some(index) => match self.more.get_mut(index) {
                Some(slot) => {
                    *slot = clip;
                    true
                }
                None => false,
            },
        }
    }

    /// Takes clip `number` off the track; the second clip becomes the first when the first goes. `false` when
    /// there is no such clip or it is the track's only one.
    pub fn remove_clip(&mut self, number: usize) -> bool {
        match number.checked_sub(1) {
            None => {
                if self.more.is_empty() {
                    return false;
                }
                let next = self.more.remove(0);
                let _ = self.set_clip(0, next);
                true
            }
            Some(index) if index < self.more.len() => {
                let _ = self.more.remove(index);
                true
            }
            Some(_) => false,
        }
    }

    /// Every sound file the track uses.
    pub fn assets(&self) -> impl Iterator<Item = AssetId> + '_ {
        std::iter::once(self.asset).chain(self.more.iter().map(|clip| clip.asset))
    }

    /// Whether the clip's in or out point is trimmed.
    #[must_use]
    pub fn is_trimmed(&self) -> bool {
        self.trim_start > 0 || self.trim_end.is_some()
    }
}

/// For `skip_serializing_if`: a field left out of the file at its default of 0.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero_u32(value: &u32) -> bool {
    *value == 0
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
    /// The colour group (from 0) of the timeline colour set that the track's subtitles use.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub group: u8,
}

/// One colour of a subtitle: its RGB value and its opacity in percent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleColor {
    /// Red, green, blue.
    pub rgb: [u8; 3],
    /// Opacity, 0 to 100 percent.
    pub opacity: u8,
}

/// One colour group of a timeline colour set: the letters and their stroke.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleGroup {
    /// The letters.
    pub fill: SubtitleColor,
    /// The outline around them.
    pub stroke: SubtitleColor,
    /// The colour of the half-covered pixels at the letters' edges (Encore's Anti-alias); `None` makes it from the
    /// fill and the stroke, as Encore's "Create anti-alias color from fill and stroke" does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub antialias: Option<SubtitleColor>,
}

impl SubtitleGroup {
    /// The anti-alias colour: the group's own, or halfway between the fill and the stroke (Encore's default
    /// set has it there: fill 234 and stroke 16 give 125).
    #[must_use]
    pub fn antialias_color(&self) -> SubtitleColor {
        self.antialias.unwrap_or_else(|| {
            let mid =
                |a: u8, b: u8| u8::try_from((u16::from(a) + u16::from(b)) / 2).unwrap_or(u8::MAX);
            let [fr, fg, fb] = self.fill.rgb;
            let [sr, sg, sb] = self.stroke.rgb;
            SubtitleColor {
                rgb: [mid(fr, sr), mid(fg, sg), mid(fb, sb)],
                opacity: mid(self.fill.opacity, self.stroke.opacity),
            }
        })
    }
}

/// A timeline colour set (Encore's Timeline Color Set): three colour groups; each subtitle uses one of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleColorSet {
    /// Groups 1 to 3.
    pub groups: [SubtitleGroup; 3],
}

impl Default for SubtitleColorSet {
    /// Encore CS6's "Timeline Default" set, as its Timeline Color Set dialog shows it: pale yellow, pale pink and
    /// pale blue letters, each with a near-black stroke, all fully opaque (colours read from the dialog's swatches).
    fn default() -> Self {
        let group = |rgb: [u8; 3]| SubtitleGroup {
            fill: SubtitleColor { rgb, opacity: 100 },
            stroke: SubtitleColor {
                rgb: [16, 16, 16],
                opacity: 100,
            },
            antialias: None,
        };
        Self {
            groups: [
                group([234, 234, 176]),
                group([234, 199, 234]),
                group([196, 196, 233]),
            ],
        }
    }
}

impl SubtitleColorSet {
    /// How many groups a set has.
    pub const GROUPS: u8 = 3;

    /// The group numbered `index` from 0 (a number past the last counts as the first).
    #[must_use]
    pub fn group(&self, index: u8) -> SubtitleGroup {
        let [first, second, third] = self.groups;
        match index {
            1 => second,
            2 => third,
            _ => first,
        }
    }
}

fn is_zero_u8(value: &u8) -> bool {
    *value == 0
}

impl Project {
    /// Sets the pixel aspect ratio an asset's pictures are taken to have; `None`: the file's own.
    ///
    /// # Errors
    ///
    /// When the asset does not exist, or the ratio is not between 1:10 and 10:1 in numbers up to 10000.
    pub fn set_asset_pixel_aspect(
        &mut self,
        id: AssetId,
        aspect: Option<(u32, u32)>,
    ) -> Result<(), ProjectError> {
        if let Some((width, height)) = aspect {
            let sane = (1..=10_000).contains(&width)
                && (1..=10_000).contains(&height)
                && width <= height.saturating_mul(10)
                && height <= width.saturating_mul(10);
            if !sane {
                return Err(ProjectError::Invalid(
                    "the pixel aspect ratio must be between 1:10 and 10:1".to_string(),
                ));
            }
        }
        let asset = self
            .assets
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("asset does not exist".to_string()))?;
        asset.pixel_aspect = aspect;
        Ok(())
    }

    /// The files whose pictures have a pixel aspect ratio of the project's choosing, for the media reader
    /// (`disccraft_transcode::set_pixel_aspects`).
    #[must_use]
    pub fn pixel_aspects(&self) -> Vec<(std::path::PathBuf, (u32, u32))> {
        self.assets
            .values()
            .filter_map(|asset| Some((asset.media_file().into_owned(), asset.pixel_aspect?)))
            .collect()
    }

    /// The Blu-ray format the titles of video asset `asset` are made in: on an HD Blu-ray its own HD format
    /// (Encore's Transcode Settings ▸ Blu-ray ▸ Codec / Dimensions / Frame Rate), else the project's ("Use Project
    /// Settings"); on an SD Blu-ray (or a DVD) the project's.
    #[must_use]
    pub fn asset_bd_video(&self, asset: AssetId) -> BdVideo {
        let project = self.settings.bd_video;
        if project.hd().is_none() {
            return project;
        }
        self.assets
            .get(&asset)
            .and_then(|found| found.bluray_video)
            .filter(|video| video.hd().is_some())
            .unwrap_or(project)
    }

    /// The Blu-ray format timeline `timeline`'s title is made in: its video's ([`Project::asset_bd_video`]).
    #[must_use]
    pub fn title_bd_video(&self, timeline: &Timeline) -> BdVideo {
        self.asset_bd_video(timeline.video)
    }

    /// Sets the video bit rate (kbit/s) an asset is transcoded at; `None`: automatic.
    ///
    /// # Errors
    ///
    /// When the asset does not exist, or the rate is outside [`MIN_TRANSCODE_KBPS`]..=[`MAX_TRANSCODE_KBPS`].
    pub fn set_asset_transcode(
        &mut self,
        id: AssetId,
        kbps: Option<u32>,
    ) -> Result<(), ProjectError> {
        if kbps.is_some_and(|kbps| !(MIN_TRANSCODE_KBPS..=MAX_TRANSCODE_KBPS).contains(&kbps)) {
            return Err(ProjectError::Invalid(format!(
                "the bit rate must be between {MIN_TRANSCODE_KBPS} and {MAX_TRANSCODE_KBPS} kbit/s"
            )));
        }
        let asset = self
            .assets
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("asset does not exist".to_string()))?;
        asset.transcode_kbps = kbps;
        Ok(())
    }

    /// Chooses the composition an EffectCraft project asset shows (`None`: the project's active one).
    pub fn set_asset_composition(
        &mut self,
        id: AssetId,
        composition: Option<String>,
    ) -> Result<(), ProjectError> {
        let asset = self
            .assets
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("asset does not exist".to_string()))?;
        asset.composition = composition
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty());
        Ok(())
    }

    /// The colour set of a timeline's subtitles: its own, or the default.
    #[must_use]
    pub fn timeline_subtitle_colors(&self, id: TimelineId) -> SubtitleColorSet {
        self.timeline(id)
            .and_then(|timeline| timeline.subtitle_colors)
            .unwrap_or_default()
    }

    /// Gives a timeline its own colour set (`None`: back to the default).
    pub fn set_timeline_subtitle_colors(
        &mut self,
        id: TimelineId,
        colors: Option<SubtitleColorSet>,
    ) -> Result<(), ProjectError> {
        if let Some(set) = &colors
            && set.groups.iter().any(|group| {
                group.fill.opacity > 100
                    || group.stroke.opacity > 100
                    || group.antialias.is_some_and(|color| color.opacity > 100)
            })
        {
            return Err(ProjectError::Invalid(
                "opacity is 0 to 100 percent".to_string(),
            ));
        }
        let timeline = self
            .timelines
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("timeline does not exist".to_string()))?;
        timeline.subtitle_colors = colors.filter(|set| *set != SubtitleColorSet::default());
        Ok(())
    }

    /// The colour group (from 0) a subtitle track's subtitles use.
    pub fn set_subtitle_track_group(
        &mut self,
        id: TimelineId,
        track: usize,
        group: u8,
    ) -> Result<(), ProjectError> {
        if group >= SubtitleColorSet::GROUPS {
            return Err(ProjectError::Invalid(
                "a colour set has three groups".to_string(),
            ));
        }
        let track = self
            .timelines
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("timeline does not exist".to_string()))?
            .subtitles
            .get_mut(track)
            .ok_or_else(|| ProjectError::Invalid("subtitle track does not exist".to_string()))?;
        track.group = group;
        Ok(())
    }
}

/// The part of a video clip that plays, in frames of its file (Encore: a clip's trimmed in and out points, which
/// fall on GOP starts). The default is the whole clip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipTrim {
    /// The first frame that plays.
    #[serde(default)]
    pub start: u32,
    /// The frame after the last that plays; `None` plays to the end of the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<u32>,
}

/// The most video clips on one timeline.
pub const MAX_TIMELINE_CLIPS: usize = 99;

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
    /// The video clips after the first, each starting where the one before ends (Encore: more videos dropped on
    /// the video track are butted on to the end).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub more_video: Vec<AssetId>,
    /// How each video clip is trimmed, by clip (0 = the first); clips past the end of the list are whole.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub video_trims: Vec<ClipTrim>,
    /// The empty stretch in front of each video clip, by clip, in frames of the disc's standard (Encore: a clip
    /// moved along the track leaves a gap, which plays black); clips past the end of the list have none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub video_gaps: Vec<u32>,
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
    /// The timeline's own colour set for its subtitles (`None`: the default set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle_colors: Option<SubtitleColorSet>,
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
    /// The title's own parental level (M6.363): `None` the disc's, `Some(0)` none, `Some(1..=8)` that level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parental_level: Option<u8>,
    /// What plays in the title's place for a player below its parental level that is not raised (a parental
    /// block, M6.363); not set: the player stops.
    #[serde(default, skip_serializing_if = "Link::is_not_set")]
    pub parental_alternative: Link,
}

impl Timeline {
    /// The parental level the title asks (1 to 8), its own or the disc's `disc`; `None` when it asks none.
    pub fn effective_parental_level(&self, disc: u8) -> Option<u8> {
        Some(self.parental_level.unwrap_or(disc)).filter(|level| (1..=8).contains(level))
    }

    /// How clip `clip` (0 = the first) is trimmed.
    pub fn video_trim(&self, clip: usize) -> ClipTrim {
        self.video_trims.get(clip).copied().unwrap_or_default()
    }

    /// The frames of empty track in front of clip `clip` (0 = the first).
    pub fn video_gap(&self, clip: usize) -> u32 {
        self.video_gaps.get(clip).copied().unwrap_or(0)
    }

    /// Whether any clip has an empty stretch in front of it.
    pub fn has_video_gaps(&self) -> bool {
        self.video_gaps.iter().any(|gap| *gap > 0)
    }

    /// The video clips in playing order: the first and the ones butted on to it.
    pub fn videos(&self) -> Vec<AssetId> {
        std::iter::once(self.video)
            .chain(self.more_video.iter().copied())
            .collect()
    }

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
    /// The project's named subtitle colour sets (Encore: the colour sets made with New in the Timeline Color Set
    /// dialog), by name. A timeline or a slideshow takes a copy of one.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub subtitle_color_sets: BTreeMap<String, SubtitleColorSet>,
    /// The project's named menu colour sets (Encore: the sets made with New in the Menu Color Set dialog), by
    /// name. A menu takes a copy of one.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub menu_color_sets: BTreeMap<String, crate::ColorSet>,
    #[serde(default = "default_next_id")]
    pub next_id: u64,
}

/// The most named colour sets a project keeps.
pub const MAX_COLOR_SETS: usize = 64;
/// The longest name of a colour set, in characters.
pub const MAX_COLOR_SET_NAME: usize = 64;

impl Project {
    /// Keeps `set` in the project under `name` (replacing a set of that name).
    pub fn save_subtitle_color_set(
        &mut self,
        name: &str,
        set: SubtitleColorSet,
    ) -> Result<(), ProjectError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > MAX_COLOR_SET_NAME {
            return Err(ProjectError::Invalid(
                "a colour set needs a name of at most 64 characters".to_string(),
            ));
        }
        if !self.subtitle_color_sets.contains_key(name)
            && self.subtitle_color_sets.len() >= MAX_COLOR_SETS
        {
            return Err(ProjectError::Invalid(
                "the project has too many colour sets".to_string(),
            ));
        }
        let _ = self.subtitle_color_sets.insert(name.to_owned(), set);
        Ok(())
    }

    /// Keeps the menu colour set `set` in the project under `name` (replacing a set of that name).
    pub fn save_menu_color_set(
        &mut self,
        name: &str,
        set: crate::ColorSet,
    ) -> Result<(), ProjectError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > MAX_COLOR_SET_NAME {
            return Err(ProjectError::Invalid(
                "a colour set needs a name of at most 64 characters".to_string(),
            ));
        }
        if !set.is_valid() {
            return Err(ProjectError::Invalid(
                "every opacity must be 0 to 100".to_string(),
            ));
        }
        if !self.menu_color_sets.contains_key(name) && self.menu_color_sets.len() >= MAX_COLOR_SETS
        {
            return Err(ProjectError::Invalid(
                "the project has too many colour sets".to_string(),
            ));
        }
        let _ = self.menu_color_sets.insert(name.to_owned(), set);
        Ok(())
    }

    /// Takes the named menu colour set out of the project (the menus that took a copy keep their colours).
    pub fn remove_menu_color_set(&mut self, name: &str) -> Result<(), ProjectError> {
        self.menu_color_sets
            .remove(name.trim())
            .map(|_| ())
            .ok_or_else(|| ProjectError::Invalid(format!("there is no colour set \"{name}\"")))
    }

    /// Takes the named colour set out of the project (the timelines that took a copy keep their colours).
    pub fn remove_subtitle_color_set(&mut self, name: &str) -> Result<(), ProjectError> {
        self.subtitle_color_sets
            .remove(name.trim())
            .map(|_| ())
            .ok_or_else(|| ProjectError::Invalid(format!("there is no colour set \"{name}\"")))
    }
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
            subtitle_color_sets: BTreeMap::new(),
            menu_color_sets: BTreeMap::new(),
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
                cues: None,
                composition: None,
                description: String::new(),
                transcode_kbps: None,
                pixel_aspect: None,
                located_dvd: None,
                located_bluray: None,
                ignore_alpha: false,
                transcode_mode: Default::default(),
                transcode_audio: Default::default(),
                transcode_preset: None,
                bluray_transcode: None,
                transcode_film: false,
                bluray_video: None,
                transcode_quality: BEST_QUALITY,
                max_render_quality: false,
                frame_blend: false,
            },
        );
        Ok(id)
    }

    /// Add a timeline referencing an existing video asset.
    /// Makes `timeline` the disc's first play when none is set yet, as Encore does for the first timeline
    /// brought into a project.
    pub fn adopt_first_play(&mut self, timeline: TimelineId) {
        if self.settings.first_play == Link::NotSet && self.timelines.contains_key(&timeline) {
            self.settings.first_play = Link::Timeline {
                id: timeline,
                chapter: 0,
            };
        }
    }

    /// Makes `menu` (its default button) the disc's title button when none is set yet, as Encore does for the
    /// first menu made in a project.
    pub fn adopt_title_remote(&mut self, menu: crate::ids::MenuId) {
        if self.settings.title_remote == Link::NotSet && self.menus.contains_key(&menu) {
            self.settings.title_remote = Link::Menu {
                id: menu,
                button: None,
            };
        }
    }

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
                parental_level: None,
                parental_alternative: Link::NotSet,
                id,
                name,
                video,
                more_video: Vec::new(),
                video_gaps: Vec::new(),
                subtitle_colors: None,
                video_trims: Vec::new(),
                audio: Vec::new(),
                subtitles: Vec::new(),
                chapters: vec![Chapter {
                    time: Tick::ZERO,
                    name: "Chapter 1".to_string(),
                    poster: None,
                }],
                end_action: Link::NotSet,
                // Encore CS6: a new timeline's Menu Remote is "Return to Last Menu".
                menu_remote: Link::LastMenu,
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

    /// Butts video `asset` on to the end of timeline `tl` (Encore: a video dropped on the video track goes to the
    /// end of the clips, wherever it is dropped) and puts a chapter at the join, `join` (the length of the clips
    /// so far), unless one is there already. Returns the index of the chapter at the join.
    pub fn append_timeline_video(
        &mut self,
        tl: TimelineId,
        asset: AssetId,
        join: Tick,
    ) -> Result<usize, ProjectError> {
        let is_video = self
            .assets
            .get(&asset)
            .is_some_and(|found| found.kind == AssetKind::Video);
        if !is_video {
            return Err(ProjectError::Invalid(
                "only a video asset goes on the video track".to_string(),
            ));
        }
        if join <= Tick::ZERO {
            return Err(ProjectError::Invalid(
                "the clips before the new one have no length".to_string(),
            ));
        }
        let timeline = self
            .timelines
            .get_mut(&tl)
            .ok_or_else(|| ProjectError::Invalid("timeline does not exist".to_string()))?;
        if timeline.more_video.len() >= MAX_TIMELINE_CLIPS.saturating_sub(1) {
            return Err(ProjectError::Invalid(format!(
                "a timeline holds at most {MAX_TIMELINE_CLIPS} video clips"
            )));
        }
        timeline.more_video.push(asset);
        if let Some(index) = timeline.chapters.iter().position(|c| c.time == join) {
            return Ok(index);
        }
        self.add_chapter(tl, join, None)
    }

    /// Takes video clip `clip` (1 = the second; the first stays) off timeline `tl`. The clip plays from `start` for
    /// `duration`: its chapters go with it, and the clips and chapters after it move up by `duration`.
    pub fn remove_timeline_video(
        &mut self,
        tl: TimelineId,
        clip: usize,
        (start, duration): (Tick, Tick),
    ) -> Result<(), ProjectError> {
        let timeline = self
            .timelines
            .get_mut(&tl)
            .ok_or_else(|| ProjectError::Invalid("timeline does not exist".to_string()))?;
        if clip > timeline.more_video.len() {
            return Err(ProjectError::Invalid(
                "there is no such video clip to remove".to_string(),
            ));
        }
        match clip.checked_sub(1) {
            Some(index) => {
                let _ = timeline.more_video.remove(index);
            }
            // The first clip goes and the second takes its place; a timeline keeps at least one clip.
            None => {
                if timeline.more_video.is_empty() {
                    return Err(ProjectError::Invalid(
                        "a timeline's only video clip cannot be removed".to_string(),
                    ));
                }
                timeline.video = timeline.more_video.remove(0);
            }
        }
        if clip < timeline.video_trims.len() {
            let _ = timeline.video_trims.remove(clip);
        }
        // The empty stretch in front of the clip goes with it (the caller's range covers it).
        if clip < timeline.video_gaps.len() {
            let _ = timeline.video_gaps.remove(clip);
        }
        self.cut_timeline_range(tl, (start, duration), false)
    }

    /// Removes clip `clip` and leaves its place empty (Encore: Clear on a clip, as against Ripple Delete): the
    /// clips after it stay where they are, the one that followed getting `frames` more of empty track in front of
    /// it (the clip's length and the gap that was in front of it). The chapters inside `(start, duration)`, the
    /// time the clip and its gap took, go; the others stay. The last clip just goes: nothing follows it.
    pub fn clear_timeline_video(
        &mut self,
        tl: TimelineId,
        clip: usize,
        (start, duration): (Tick, Tick),
        frames: u32,
    ) -> Result<(), ProjectError> {
        let timeline = self
            .timelines
            .get(&tl)
            .ok_or_else(|| ProjectError::Invalid("timeline does not exist".to_string()))?;
        if clip >= timeline.more_video.len() {
            // The last clip (or none such): as a plain removal.
            return self.remove_timeline_video(tl, clip, (start, duration));
        }
        // Nothing moves: the range taken out is empty, so only the clip's own entries go.
        let end = Tick(start.0.saturating_add(duration.0));
        let inside: Vec<usize> = timeline
            .chapters
            .iter()
            .enumerate()
            .filter(|(at, chapter)| *at > 0 && chapter.time >= start && chapter.time < end)
            .map(|(at, _)| at)
            .collect();
        self.remove_timeline_video(tl, clip, (start, Tick(0)))?;
        for at in inside.into_iter().rev() {
            self.remove_chapter(tl, at)?;
        }
        let Some(timeline) = self.timelines.get_mut(&tl) else {
            return Ok(());
        };
        // The clip that followed is now clip `clip`.
        if timeline.video_gaps.len() <= clip {
            timeline.video_gaps.resize(clip.saturating_add(1), 0);
        }
        if let Some(gap) = timeline.video_gaps.get_mut(clip) {
            *gap = gap.saturating_add(frames);
        }
        Ok(())
    }

    /// Moves clip `clip` along the track (Encore: a clip dragged to another place): `gap` frames of empty track are
    /// in front of it now, and `next_gap` in front of the clip after it (so that one stays where it is). The
    /// chapters inside the clip, which plays at `span` (start, length), move with it by `by`; the first chapter, at
    /// the very start of the timeline, stays.
    pub fn move_timeline_video(
        &mut self,
        tl: TimelineId,
        clip: usize,
        (gap, next_gap): (u32, Option<u32>),
        span: (Tick, Tick),
        by: Tick,
    ) -> Result<(), ProjectError> {
        let timeline = self
            .timelines
            .get_mut(&tl)
            .ok_or_else(|| ProjectError::Invalid("timeline does not exist".to_string()))?;
        if clip > timeline.more_video.len() {
            return Err(ProjectError::Invalid(
                "there is no such video clip".to_string(),
            ));
        }
        let mut set = |index: usize, frames: u32| {
            if timeline.video_gaps.len() <= index {
                timeline.video_gaps.resize(index.saturating_add(1), 0);
            }
            if let Some(slot) = timeline.video_gaps.get_mut(index) {
                *slot = frames;
            }
        };
        set(clip, gap);
        if let Some(next) = next_gap {
            set(clip.saturating_add(1), next);
        }
        while timeline.video_gaps.last() == Some(&0) {
            let _ = timeline.video_gaps.pop();
        }
        let end = span.0.0.saturating_add(span.1.0);
        for chapter in &mut timeline.chapters {
            if chapter.time.0 > 0 && chapter.time.0 >= span.0.0 && chapter.time.0 < end {
                chapter.time = Tick(chapter.time.0.saturating_add(by.0).max(0));
            }
        }
        timeline.chapters.sort_by_key(|chapter| chapter.time);
        timeline.chapters.dedup_by_key(|chapter| chapter.time);
        Ok(())
    }

    /// Sets how clip `clip` (0 = the first) of timeline `tl` is trimmed. `cut` is the part of the timeline that
    /// goes away, as (start, length): its chapters go with it (one sitting exactly on its start stays when
    /// `keep_first`: the chapter at a clip's head), and what follows moves up.
    pub fn trim_timeline_video(
        &mut self,
        tl: TimelineId,
        clip: usize,
        trim: ClipTrim,
        cut: Option<((Tick, Tick), bool)>,
    ) -> Result<(), ProjectError> {
        let timeline = self
            .timelines
            .get_mut(&tl)
            .ok_or_else(|| ProjectError::Invalid("timeline does not exist".to_string()))?;
        if clip > timeline.more_video.len() {
            return Err(ProjectError::Invalid(
                "there is no such video clip".to_string(),
            ));
        }
        if trim.end.is_some_and(|end| end <= trim.start) {
            return Err(ProjectError::Invalid(
                "a trimmed clip must keep some frames".to_string(),
            ));
        }
        if timeline.video_trims.len() <= clip {
            timeline
                .video_trims
                .resize(clip.saturating_add(1), ClipTrim::default());
        }
        if let Some(slot) = timeline.video_trims.get_mut(clip) {
            *slot = trim;
        }
        while timeline.video_trims.last() == Some(&ClipTrim::default()) {
            let _ = timeline.video_trims.pop();
        }
        match cut {
            Some((range, keep_first)) => self.cut_timeline_range(tl, range, keep_first),
            None => Ok(()),
        }
    }

    /// Makes room in timeline `tl`: the chapters at or after `from` (after it only, when `after_only`: the chapter
    /// at a clip's head stays) move later by `by`. The first chapter never moves.
    pub fn shift_timeline_chapters(
        &mut self,
        tl: TimelineId,
        from: Tick,
        by: Tick,
        after_only: bool,
    ) -> Result<(), ProjectError> {
        let timeline = self
            .timelines
            .get_mut(&tl)
            .ok_or_else(|| ProjectError::Invalid("timeline does not exist".to_string()))?;
        for chapter in timeline.chapters.iter_mut().skip(1) {
            let moves = if after_only {
                chapter.time > from
            } else {
                chapter.time >= from
            };
            if moves {
                chapter.time = Tick(chapter.time.0.saturating_add(by.0));
                chapter.poster = chapter
                    .poster
                    .map(|poster| Tick(poster.0.saturating_add(by.0)));
            }
        }
        Ok(())
    }

    /// Takes `duration` out of timeline `tl` at `start`: the chapters in it go (the first chapter never does; one
    /// exactly at `start` stays when `keep_first`), and the chapters after it move up by `duration`.
    fn cut_timeline_range(
        &mut self,
        tl: TimelineId,
        (start, duration): (Tick, Tick),
        keep_first: bool,
    ) -> Result<(), ProjectError> {
        let timeline = self
            .timelines
            .get_mut(&tl)
            .ok_or_else(|| ProjectError::Invalid("timeline does not exist".to_string()))?;
        let end = Tick(start.0.saturating_add(duration.0));
        let gone: Vec<usize> = timeline
            .chapters
            .iter()
            .enumerate()
            .filter(|(at, chapter)| {
                let inside = chapter.time >= start && chapter.time < end;
                *at > 0 && inside && !(keep_first && chapter.time == start)
            })
            .map(|(at, _)| at)
            .collect();
        for at in gone.into_iter().rev() {
            self.remove_chapter(tl, at)?;
        }
        let Some(timeline) = self.timelines.get_mut(&tl) else {
            return Ok(());
        };
        for chapter in &mut timeline.chapters {
            if chapter.time >= end {
                chapter.time = Tick(chapter.time.0.saturating_sub(duration.0));
                chapter.poster = chapter
                    .poster
                    .map(|poster| Tick(poster.0.saturating_sub(duration.0)));
            }
        }
        // A chapter that moved up on to one that stayed: one of the two is enough.
        let doubled: Vec<usize> = timeline
            .chapters
            .windows(2)
            .enumerate()
            .filter(|(_, pair)| matches!(pair, [a, b] if a.time == b.time))
            .map(|(at, _)| at.saturating_add(1))
            .collect();
        for at in doubled.into_iter().rev() {
            self.remove_chapter(tl, at)?;
        }
        Ok(())
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
    /// The sound and video files testers bring are taken as audio and video, whatever the case of the name.
    #[test]
    fn common_media_extensions_are_recognised() {
        use std::path::Path;
        for name in [
            "a.wav", "a.ac3", "a.dts", "a.mp3", "a.AAC", "a.m4a", "a.mka", "a.mp2", "a.ogg",
            "a.oga", "a.opus", "a.flac", "a.aiff",
        ] {
            assert_eq!(
                AssetKind::guess_from_path(Path::new(name)),
                AssetKind::Audio,
                "{name}"
            );
        }
        for name in [
            "v.mp4", "v.m4v", "v.mov", "v.MKV", "v.webm", "v.avi", "v.mpg", "v.vob", "v.m2ts",
            "v.mts", "v.m2t", "v.ts",
        ] {
            assert_eq!(
                AssetKind::guess_from_path(Path::new(name)),
                AssetKind::Video,
                "{name}"
            );
        }
        // Files the decoders cannot read stay out.
        for name in ["a.wma", "a.eac3", "v.wmv", "v.flv", "notes.docx"] {
            assert_eq!(
                AssetKind::guess_from_path(Path::new(name)),
                AssetKind::Other,
                "{name}"
            );
        }
    }

    /// An EffectCraft project shown as a chosen composition is opened by a name under the project file; any
    /// other asset by its own path.
    #[test]
    fn a_chosen_composition_is_part_of_the_media_path() {
        let mut project = Project::default();
        let Ok(id) = project.add_asset(std::path::Path::new("demo.ecproj")) else {
            return;
        };
        let path = project.assets.get(&id).map(|asset| asset.path.clone());
        let media = |project: &Project| {
            project
                .assets
                .get(&id)
                .map(|asset| asset.media_path().into_owned())
        };
        assert_eq!(media(&project), path);
        if let Some(asset) = project.assets.get_mut(&id) {
            asset.composition = Some("Lower/Third 100%".to_owned());
        }
        let separator = std::path::MAIN_SEPARATOR;
        assert_eq!(
            media(&project),
            path.as_ref()
                .map(|path| format!("{path}{separator}Lower%2FThird 100%25.ecproj"))
        );
        // A composition on a file that is not an EffectCraft project changes nothing.
        if let Some(asset) = project.assets.get_mut(&id) {
            asset.path = "clip.mp4".to_owned();
        }
        assert_eq!(media(&project).as_deref(), Some("clip.mp4"));
    }

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
