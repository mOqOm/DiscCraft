//! Menu and button document model.

use crate::ids::{AssetId, MenuId};
use crate::io::ProjectError;
use crate::link::Link;
use crate::model::{DiscSettings, Project, VideoStandard};
use serde::{Deserialize, Serialize};

/// Maximum number of buttons on one menu.
pub const MAX_BUTTONS: usize = 36;

/// Longest time a still menu can stay before its end action, in seconds (a DVD cell's still time).
pub const MAX_MENU_SECONDS: u32 = 254;

/// Longest loop of a still menu with audio, in seconds (the audio is cut there). A motion menu loops its video's
/// length (1 to 30 seconds) and the audio follows it.
pub const MAX_MENU_AUDIO_SECONDS: u32 = 120;

/// Rectangle in menu frame coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    /// Left edge.
    pub x: u16,
    /// Top edge.
    pub y: u16,
    /// Width.
    pub w: u16,
    /// Height.
    pub h: u16,
}

/// Menu background.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MenuBackground {
    /// Solid color background.
    Color {
        /// RGB color.
        color: [u8; 3],
    },
    /// Image background.
    Image {
        /// Image asset id.
        asset: AssetId,
    },
    /// Motion background: the first `seconds` of a video asset, looped (motion menu).
    Video {
        /// Video asset id.
        asset: AssetId,
        /// Loop length in seconds (1..=30).
        seconds: u32,
    },
}

impl Default for MenuBackground {
    fn default() -> Self {
        Self::Color {
            color: [32, 32, 64],
        }
    }
}

/// Menu button.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Button {
    /// Internal name.
    #[serde(default)]
    pub name: String,
    /// Visible label.
    #[serde(default)]
    pub label: String,
    /// Button rectangle.
    #[serde(default)]
    pub rect: Rect,
    /// Navigation target.
    #[serde(default)]
    pub link: Link,
    /// Whether moving the highlight onto the button already activates it (no Enter needed).
    #[serde(default)]
    pub auto_activate: bool,
    /// Where to go when the title this button plays has finished, instead of that title's end action (Encore's
    /// button Override). Used when the link plays a title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_link: Option<Link>,
    /// Which highlight group of the menu's colour set draws this button (1 to 3; Encore's button "Highlight").
    #[serde(
        default = "default_highlight_group",
        skip_serializing_if = "is_default_highlight_group"
    )]
    pub highlight_group: u8,
    /// Encore's "Sync Button Text and Name": the name follows the button text.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub sync_name: bool,
    /// Encore's "Set Name from Link": the name follows the link's target.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub name_from_link: bool,
    /// Hand-set routing (up, down, left, right; one-based button numbers, 0 = nowhere), used when the menu does
    /// not route automatically. `None` keeps the automatic routing for this button.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routes: Option<[u8; 4]>,
    /// The audio track (1 to 8) the button selects before it jumps (Encore's link dialog "Audio"); `None` leaves
    /// the player's choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<u8>,
    /// The subtitle the button selects before it jumps (Encore's link dialog "Subtitle").
    #[serde(
        default,
        skip_serializing_if = "crate::model::SubtitleDefault::is_no_change"
    )]
    pub subtitle: crate::model::SubtitleDefault,
    /// A drop shadow behind the button's text (Encore's Object ▸ Drop Shadow…).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<DropShadow>,
    /// What the button is for (Encore's button 種類: normal / chapter / next / previous / main).
    #[serde(default, skip_serializing_if = "ButtonKind::is_normal")]
    pub kind: ButtonKind,
    /// Mirrored or turned (Encore's Object ▸ Flip / Rotate).
    #[serde(default, skip_serializing_if = "Orientation::is_identity")]
    pub orientation: Orientation,
}

impl Button {
    /// Whether the button selects an audio or subtitle track when it is activated.
    pub fn selects_streams(&self) -> bool {
        self.audio.is_some() || self.subtitle != crate::model::SubtitleDefault::NoChange
    }
}

/// Menu document object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Menu {
    /// Shared object id.
    pub id: MenuId,
    /// Display name.
    pub name: String,
    /// Background.
    #[serde(default)]
    pub background: MenuBackground,
    /// Text color.
    #[serde(default = "default_text_color")]
    pub text_color: [u8; 3],
    /// Highlight color.
    #[serde(default = "default_highlight_color")]
    pub highlight_color: [u8; 3],
    /// Buttons.
    #[serde(default)]
    pub buttons: Vec<Button>,
    /// Default button index.
    #[serde(default)]
    pub default_button: u32,
    /// What happens when a still menu's duration is over, or a motion menu has played once. Not set keeps the
    /// menu on screen (a motion menu loops).
    #[serde(default)]
    pub end_action: Link,
    /// Where to go when the title the end action plays has finished, instead of that title's end action
    /// (Encore's menu Override). Used when the end action plays a title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_link: Option<Link>,
    /// How long a still menu stays before its end action, in seconds (1 to [`MAX_MENU_SECONDS`]). `None` holds
    /// it until a button is pressed.
    #[serde(default)]
    pub duration_seconds: Option<u32>,
    /// Audio played with the menu (looped with it). A still menu with audio loops for the audio's length, up to
    /// [`MAX_MENU_AUDIO_SECONDS`].
    #[serde(default)]
    pub audio: Option<AssetId>,
    /// Text objects drawn into the menu picture (Encore's text tool), under the buttons.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<MenuText>,
    /// Remote operations prohibited while the menu is shown (DVD).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prohibited_ops: Vec<crate::UserOp>,
    /// Font family of the button texts; `None` uses the default Japanese font.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_font: Option<String>,
    /// Free description (kept in the project only).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// 16:9 (anamorphic) menu instead of 4:3 (Encore's menu aspect ratio). A DVD's menus all share the first
    /// menu's aspect ratio.
    #[serde(default, skip_serializing_if = "is_false")]
    pub widescreen: bool,
    /// Guides in the menu viewer (editing help only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<Guide>,
    /// Graphic layers (images and shapes), bottom to top: over the background, under the texts and buttons.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graphics: Vec<crate::graphic::MenuGraphic>,
    /// The colour set (three highlight groups); `None` uses the highlight colour for all groups.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_set: Option<ColorSet>,
    /// Encore's "Automatically Route Buttons": the remote's arrows follow the layout. Off, each button's own
    /// `routes` apply.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub auto_route: bool,
    /// Encore's menu "Offset": the remote's number keys count the buttons from `button_offset + 1` (a second
    /// page of chapters can start at 7). 0 to [`MAX_BUTTON_OFFSET`].
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub button_offset: u8,
}

/// The largest button number offset (36 button numbers on a DVD menu).
pub const MAX_BUTTON_OFFSET: u8 = 35;

fn is_zero_u8(value: &u8) -> bool {
    *value == 0
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// The most text objects on one menu.
pub const MAX_MENU_TEXTS: usize = 64;
/// The longest text of a text object, in characters.
pub const MAX_TEXT_CHARS: usize = 1000;
/// The smallest text size, in pixels.
pub const MIN_TEXT_SIZE: u16 = 8;
/// The largest text size, in pixels.
pub const MAX_TEXT_SIZE: u16 = 200;
/// The size of a new text object, in pixels.
pub const DEFAULT_TEXT_SIZE: u16 = 32;

/// Farthest a drop shadow is moved from its object (pixels).
pub const MAX_SHADOW_DISTANCE: u8 = 100;
/// Widest blur of a drop shadow (pixels).
pub const MAX_SHADOW_SIZE: u8 = 100;

/// A drop shadow behind a text, a button's text or a graphic (Encore's Object ▸ Drop Shadow…).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DropShadow {
    /// The shadow's colour.
    pub color: [u8; 3],
    /// Opacity in per cent (0 to 100).
    pub opacity: u8,
    /// Where the light comes from, in degrees counter-clockwise from the right (0 to 359); the shadow falls
    /// the opposite way (120 = light from the upper left, shadow to the lower right).
    pub angle: u16,
    /// How far the shadow is moved, in pixels (0 to [`MAX_SHADOW_DISTANCE`]).
    pub distance: u8,
    /// How wide the shadow's edge is, in pixels (0 to [`MAX_SHADOW_SIZE`]).
    pub size: u8,
    /// How much of the size is solid before the edge softens, in per cent (0 to 100).
    pub spread: u8,
}

impl Default for DropShadow {
    /// Encore's defaults: black, 75 %, 120 degrees, 5 pixels, 5 pixels, 0 %.
    fn default() -> Self {
        Self {
            color: [0, 0, 0],
            opacity: 75,
            angle: 120,
            distance: 5,
            size: 5,
            spread: 0,
        }
    }
}

impl DropShadow {
    /// How far the shadow is moved right and down, in whole pixels.
    pub fn offset(&self) -> (i32, i32) {
        let radians = f64::from(self.angle % 360).to_radians();
        let distance = f64::from(self.distance.min(MAX_SHADOW_DISTANCE));
        let dx = (-radians.cos() * distance).round();
        let dy = (radians.sin() * distance).round();
        // Both are within ±100, so the casts cannot truncate.
        (dx as i32, dy as i32)
    }

    /// Whether every value is in range.
    pub fn is_valid(&self) -> bool {
        self.opacity <= 100
            && self.angle < 360
            && self.distance <= MAX_SHADOW_DISTANCE
            && self.size <= MAX_SHADOW_SIZE
            && self.spread <= 100
    }
}

/// A text object on a menu: drawn into the menu picture, not a button (Encore's text tool).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuText {
    /// The text. Line breaks start new lines.
    pub text: String,
    /// Left edge of the text, in frame pixels.
    pub x: u16,
    /// Top edge of the text, in frame pixels.
    pub y: u16,
    /// Text size in pixels ([`MIN_TEXT_SIZE`] to [`MAX_TEXT_SIZE`]).
    #[serde(default = "default_text_size")]
    pub size: u16,
    /// Its own colour; `None` uses the menu's text colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    /// Font family; `None` (or a font this computer does not have) uses the default Japanese font.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// Style of the family ("Bold", "Light", ...; Encore's font style); `None` is the regular face. A style the
    /// family does not have uses its nearest weight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_style: Option<String>,
    /// Character and paragraph formatting (Encore's Character panel).
    #[serde(default, skip_serializing_if = "TextFormat::is_default")]
    pub format: TextFormat,
    /// A drop shadow behind the text (Encore's Object ▸ Drop Shadow…).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<DropShadow>,
    /// Mirrored or turned (Encore's Object ▸ Flip / Rotate).
    #[serde(default, skip_serializing_if = "Orientation::is_identity")]
    pub orientation: Orientation,
}

/// Character and paragraph formatting of a [`MenuText`] (the parts of Encore's Character panel DiscCraft draws):
/// faux bold / italic, underline, strikethrough, caps, alignment, tracking, leading, kerning and baseline shift.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextFormat {
    /// Faux bold.
    pub bold: bool,
    /// Faux italic.
    pub italic: bool,
    /// Underline.
    pub underline: bool,
    /// Strikethrough.
    pub strikethrough: bool,
    /// All caps / small caps.
    pub caps: TextCaps,
    /// Line alignment of a multi-line text.
    pub align: TextAlign,
    /// Tracking in 1/1000 em ([`MIN_TRACKING`] to [`MAX_TRACKING`]).
    pub tracking: i16,
    /// Extra line spacing in pixels ([`MIN_LEADING`] to [`MAX_LEADING`]).
    pub leading: i16,
    /// Metric kerning from the font (Encore's カーニング: メトリクス); off = none.
    pub kerning: bool,
    /// Baseline shift in pixels, positive up ([`MIN_BASELINE`] to [`MAX_BASELINE`]).
    pub baseline: i16,
    /// Vertical scale in per cent ([`MIN_TEXT_SCALE`] to [`MAX_TEXT_SCALE`]; Encore's 垂直比率).
    pub vertical_scale: u16,
    /// Horizontal scale in per cent ([`MIN_TEXT_SCALE`] to [`MAX_TEXT_SCALE`]; Encore's 水平比率).
    pub horizontal_scale: u16,
    /// Superscript or subscript: the text set smaller, above or below where it would sit.
    pub script: TextScript,
    /// How the text's edges are smoothed (Encore's アンチエイリアス).
    pub antialias: TextAntialias,
}

impl Default for TextFormat {
    fn default() -> Self {
        Self {
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            caps: TextCaps::Normal,
            align: TextAlign::Left,
            tracking: 0,
            leading: 0,
            kerning: true,
            baseline: 0,
            vertical_scale: 100,
            horizontal_scale: 100,
            script: TextScript::Normal,
            antialias: TextAntialias::Crisp,
        }
    }
}

/// How a text's edges are smoothed (Encore's anti-aliasing list).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextAntialias {
    /// Hard edges: every pixel is either text or not.
    None,
    /// The plain smoothed edge (Encore's default, 鮮明).
    #[default]
    Crisp,
    /// Heavier: edge pixels are filled more (強く).
    Strong,
    /// Softer: the edge is spread over a little more (滑らかに).
    Smooth,
    /// Sharper: the edge is narrowed (シャープ).
    Sharp,
}

impl TextAntialias {
    /// Every choice, in Encore's order.
    pub const ALL: [TextAntialias; 5] = [
        TextAntialias::None,
        TextAntialias::Crisp,
        TextAntialias::Strong,
        TextAntialias::Smooth,
        TextAntialias::Sharp,
    ];

    /// The name in command parameters.
    pub fn as_str(self) -> &'static str {
        match self {
            TextAntialias::None => "none",
            TextAntialias::Crisp => "crisp",
            TextAntialias::Strong => "strong",
            TextAntialias::Smooth => "smooth",
            TextAntialias::Sharp => "sharp",
        }
    }

    /// Parses a command parameter.
    pub fn parse(text: &str) -> Option<TextAntialias> {
        Self::ALL.into_iter().find(|mode| mode.as_str() == text)
    }
}

/// Smallest vertical or horizontal text scale (per cent).
pub const MIN_TEXT_SCALE: u16 = 1;
/// Largest vertical or horizontal text scale (per cent).
pub const MAX_TEXT_SCALE: u16 = 1000;

/// Superscript / subscript of a text (Encore's Character panel T¹ / T₁).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextScript {
    /// On the line.
    #[default]
    Normal,
    /// Smaller, raised.
    Superscript,
    /// Smaller, lowered.
    Subscript,
}

impl TextScript {
    /// Every choice.
    pub const ALL: [TextScript; 3] = [
        TextScript::Normal,
        TextScript::Superscript,
        TextScript::Subscript,
    ];

    /// The name in command parameters.
    pub fn as_str(self) -> &'static str {
        match self {
            TextScript::Normal => "normal",
            TextScript::Superscript => "superscript",
            TextScript::Subscript => "subscript",
        }
    }

    /// Parses a command parameter.
    pub fn parse(text: &str) -> Option<TextScript> {
        Self::ALL.into_iter().find(|script| script.as_str() == text)
    }
}

/// How an object is mirrored and turned (Encore's Object ▸ Flip Horizontal / Flip Vertical / Rotate ▸): first
/// mirrored left to right when `flip`, then turned `turns` quarter turns clockwise about its centre.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Orientation {
    /// Mirrored left to right (before the turns).
    pub flip: bool,
    /// Quarter turns clockwise (0 to 3).
    pub turns: u8,
}

impl Orientation {
    /// Whether the object is drawn as it is (nothing to save).
    pub fn is_identity(&self) -> bool {
        !self.flip && self.turns.is_multiple_of(4)
    }

    /// Whether the object lies on its side (its drawn width is its upright height).
    pub fn swaps_sides(&self) -> bool {
        self.turns % 2 == 1
    }

    /// The orientation after mirroring the drawn object left to right.
    #[must_use]
    pub fn flipped_horizontally(self) -> Self {
        Self {
            flip: !self.flip,
            turns: (4 - self.turns % 4) % 4,
        }
    }

    /// The orientation after mirroring the drawn object top to bottom (a left-right mirror and half a turn).
    #[must_use]
    pub fn flipped_vertically(self) -> Self {
        Self {
            flip: !self.flip,
            turns: (6 - self.turns % 4) % 4,
        }
    }

    /// The orientation after turning the drawn object `quarter_turns` quarter turns clockwise.
    #[must_use]
    pub fn rotated(self, quarter_turns: u8) -> Self {
        Self {
            flip: self.flip,
            turns: (self.turns % 4 + quarter_turns % 4) % 4,
        }
    }

    /// The linear part as `[a, b, c, d]`: a point (x, y) relative to the object's centre is drawn at
    /// (a x + c y, b x + d y), in screen coordinates (y down).
    pub fn matrix(&self) -> [f32; 4] {
        let (mut a, mut b, mut c, mut d) = (if self.flip { -1.0 } else { 1.0 }, 0.0, 0.0, 1.0);
        for _ in 0..self.turns % 4 {
            // A quarter turn clockwise on screen takes (x, y) to (-y, x).
            (a, b, c, d) = (-b, a, -d, c);
        }
        [a, b, c, d]
    }

    /// The pixel of the upright picture shown at pixel (`x`, `y`) of the drawn one, which is `w` x `h`.
    pub fn upright_pixel(&self, x: usize, y: usize, w: usize, h: usize) -> (usize, usize) {
        let (mut x, mut y, mut w, mut h) = (x, y, w, h);
        for _ in 0..self.turns % 4 {
            // Undo one clockwise quarter turn: (x, y) of a w x h picture comes from (y, w - 1 - x) of an h x w one.
            (x, y, w, h) = (y, w.saturating_sub(1).saturating_sub(x), h, w);
        }
        let _ = h;
        if self.flip {
            x = w.saturating_sub(1).saturating_sub(x);
        }
        (x, y)
    }
}

/// Lowest baseline shift (pixels).
pub const MIN_BASELINE: i16 = -100;
/// Highest baseline shift (pixels).
pub const MAX_BASELINE: i16 = 100;

/// Tightest tracking (1/1000 em).
pub const MIN_TRACKING: i16 = -1000;
/// Loosest tracking (1/1000 em).
pub const MAX_TRACKING: i16 = 1000;
/// Smallest leading (pixels added to the line height).
pub const MIN_LEADING: i16 = -100;
/// Largest leading (pixels added to the line height).
pub const MAX_LEADING: i16 = 200;

impl TextFormat {
    /// Whether every setting is the default (nothing to save).
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Capitalisation of a text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextCaps {
    /// As typed.
    #[default]
    Normal,
    /// All capitals.
    All,
    /// Small capitals.
    Small,
}

impl TextCaps {
    /// Every choice.
    pub const ALL: [TextCaps; 3] = [TextCaps::Normal, TextCaps::All, TextCaps::Small];

    /// The name in command parameters.
    pub fn as_str(self) -> &'static str {
        match self {
            TextCaps::Normal => "normal",
            TextCaps::All => "all",
            TextCaps::Small => "small",
        }
    }

    /// Parses a command parameter.
    pub fn parse(text: &str) -> Option<TextCaps> {
        Self::ALL.into_iter().find(|caps| caps.as_str() == text)
    }
}

/// Line alignment of a multi-line text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextAlign {
    /// Lines start at the left.
    #[default]
    Left,
    /// Lines are centred.
    Center,
    /// Lines end at the right.
    Right,
}

impl TextAlign {
    /// Every choice, in the Character panel's order.
    pub const ALL: [TextAlign; 3] = [TextAlign::Left, TextAlign::Center, TextAlign::Right];

    /// The name in command parameters.
    pub fn as_str(self) -> &'static str {
        match self {
            TextAlign::Left => "left",
            TextAlign::Center => "center",
            TextAlign::Right => "right",
        }
    }

    /// Parses a command parameter.
    pub fn parse(text: &str) -> Option<TextAlign> {
        Self::ALL.into_iter().find(|align| align.as_str() == text)
    }
}

/// The longest font family name, in characters.
pub const MAX_FONT_NAME_CHARS: usize = 128;

fn default_highlight_group() -> u8 {
    1
}

fn default_true() -> bool {
    true
}

fn is_true(flag: &bool) -> bool {
    *flag
}

fn is_default_highlight_group(group: &u8) -> bool {
    *group == 1
}

/// The colour of an activated button when the menu has no colour set of its own.
pub const DEFAULT_ACTIVATED_COLOR: [u8; 3] = [255, 255, 255];

/// One highlight group of a menu's colour set (Encore's Color Set): the colour over a button while it is selected
/// and while it is activated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HighlightGroup {
    /// While selected (the remote's cursor is on the button).
    pub selected: [u8; 3],
    /// While activated (Enter was pressed).
    pub activated: [u8; 3],
    /// Opacity of the selected colour in per cent (0 to 100; Encore's 不透明度).
    #[serde(default = "full_percent", skip_serializing_if = "is_full_percent")]
    pub selected_opacity: u8,
    /// Opacity of the activated colour in per cent (0 to 100).
    #[serde(default = "full_percent", skip_serializing_if = "is_full_percent")]
    pub activated_opacity: u8,
    /// Encore's "Use Selected Colors as Activated Colors": an activated button looks as it does when selected.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub same_activated: bool,
}

fn full_percent() -> u8 {
    100
}

fn is_full_percent(value: &u8) -> bool {
    *value >= 100
}

impl HighlightGroup {
    /// A group of two fully opaque colours.
    pub fn new(selected: [u8; 3], activated: [u8; 3]) -> Self {
        Self {
            selected,
            activated,
            selected_opacity: 100,
            activated_opacity: 100,
            same_activated: false,
        }
    }

    /// The activated colour and opacity as they are drawn (the selected ones when `same_activated`).
    pub fn activated_look(&self) -> ([u8; 3], u8) {
        if self.same_activated {
            (self.selected, self.selected_opacity)
        } else {
            (self.activated, self.activated_opacity)
        }
    }
}

/// Encore's 標準グループ colour: how a button's highlight looks while it is neither selected nor activated. At
/// opacity 0 (the default) it is not seen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NormalColor {
    /// The colour.
    pub color: [u8; 3],
    /// Opacity in per cent (0 to 100).
    pub opacity: u8,
}

impl NormalColor {
    /// Whether it is the default (black, not seen).
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Encore's colour set: three highlight groups (every button picks one, `Button::highlight_group`) and the
/// normal colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorSet {
    /// Groups 1 to 3.
    pub groups: [HighlightGroup; 3],
    /// The normal state (Encore's 標準グループ).
    #[serde(default, skip_serializing_if = "NormalColor::is_default")]
    pub normal: NormalColor,
}

impl ColorSet {
    /// Whether every opacity is 0 to 100.
    pub fn is_valid(&self) -> bool {
        self.normal.opacity <= 100
            && self
                .groups
                .iter()
                .all(|group| group.selected_opacity <= 100 && group.activated_opacity <= 100)
    }
}

/// An opacity in per cent (0 to 100) as a DVD contrast (0 to 15).
pub fn opacity_to_contrast(opacity: u8) -> u8 {
    u8::try_from((u16::from(opacity.min(100)) * 15 + 50) / 100).unwrap_or(15)
}

/// An opacity in per cent (0 to 100) as an 8-bit alpha (0 to 255).
pub fn opacity_to_alpha(opacity: u8) -> u8 {
    u8::try_from((u16::from(opacity.min(100)) * 255 + 50) / 100).unwrap_or(255)
}

impl Menu {
    /// The menu's colour set: its own, or one made of the highlight colour (selected) and white (activated) for
    /// all three groups.
    pub fn color_set_or_default(&self) -> ColorSet {
        self.color_set.unwrap_or(ColorSet {
            groups: [HighlightGroup::new(self.highlight_color, DEFAULT_ACTIVATED_COLOR); 3],
            normal: NormalColor::default(),
        })
    }

    /// The name of what `link` leads to (Encore's "Set Name from Link"): a timeline, a chapter of one, a menu, a
    /// slideshow or a playlist. `None` for links without a named target.
    pub fn link_target_name(project: &Project, link: &Link) -> Option<String> {
        match link {
            Link::Timeline { id, chapter } => {
                let timeline = project.timelines.get(id)?;
                if *chapter == 0 {
                    return Some(timeline.name.clone());
                }
                let point = timeline.chapters.get(usize::try_from(*chapter).ok()?)?;
                Some(if point.name.trim().is_empty() {
                    format!("Chapter {}", chapter.saturating_add(1))
                } else {
                    point.name.clone()
                })
            }
            Link::Menu { id, .. } => project.menus.get(id).map(|menu| menu.name.clone()),
            Link::Slideshow { id } => project.slideshows.get(id).map(|show| show.name.clone()),
            Link::Playlist { id } => project.playlists.get(id).map(|list| list.name.clone()),
            Link::ChapterPlaylist { id } => project
                .chapter_playlists
                .get(id)
                .map(|list| list.name.clone()),
            Link::NotSet | Link::Stop | Link::LastMenu => None,
        }
    }

    /// Highlight group `group` (1 to 3; anything else counts as 1).
    pub fn highlight_group(&self, group: u8) -> HighlightGroup {
        let set = self.color_set_or_default();
        let index = usize::from(group.clamp(1, 3).saturating_sub(1));
        set.groups.get(index).copied().unwrap_or(set.groups[0])
    }
}

/// What a button is for (Encore's button 種類). A chapter index marks its chapter buttons and its previous /
/// next page buttons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ButtonKind {
    /// An ordinary button (標準).
    #[default]
    Normal,
    /// Plays a chapter (チャプター).
    Chapter,
    /// Goes to the next page (次へ).
    Next,
    /// Goes to the previous page (前へ).
    Previous,
    /// Goes back to the main menu (メイン).
    Main,
}

impl ButtonKind {
    /// Every kind, in Encore's order.
    pub const ALL: [ButtonKind; 5] = [
        ButtonKind::Normal,
        ButtonKind::Chapter,
        ButtonKind::Next,
        ButtonKind::Previous,
        ButtonKind::Main,
    ];

    /// The name in command parameters.
    pub fn as_str(self) -> &'static str {
        match self {
            ButtonKind::Normal => "normal",
            ButtonKind::Chapter => "chapter",
            ButtonKind::Next => "next",
            ButtonKind::Previous => "previous",
            ButtonKind::Main => "main",
        }
    }

    /// Parses a command parameter.
    pub fn parse(text: &str) -> Option<ButtonKind> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }

    /// Whether it is an ordinary button (nothing to save).
    pub fn is_normal(&self) -> bool {
        *self == ButtonKind::Normal
    }
}

/// Where an arrow key goes from the last button of a column (up / down) or a row (left / right) when it wraps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WrapMode {
    /// Round to the other end of the same column / row (Encore's 同じ列内で折り返し / 同じ行内で折り返し).
    #[default]
    SameLine,
    /// On to the next column / row (Encore's 次の列に折り返し / 次の行に折り返し); after the last, the first.
    NextLine,
}

impl WrapMode {
    /// The name in command parameters.
    pub fn as_str(self) -> &'static str {
        match self {
            WrapMode::SameLine => "same",
            WrapMode::NextLine => "next",
        }
    }

    /// Parses a command parameter.
    pub fn parse(text: &str) -> Option<WrapMode> {
        [WrapMode::SameLine, WrapMode::NextLine]
            .into_iter()
            .find(|mode| mode.as_str() == text)
    }
}

/// How automatic routing moves between buttons (Encore's Preferences ▸ Menus ▸ Button Routing).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RouteOptions {
    /// Up / down move between buttons (off: they stay on the button).
    pub vertical: bool,
    /// Up from the top / down from the bottom wraps around.
    pub vertical_wrap: bool,
    /// Where up / down wraps to.
    pub vertical_wrap_mode: WrapMode,
    /// Left / right move between buttons.
    pub horizontal: bool,
    /// Left from the left end / right from the right end wraps around.
    pub horizontal_wrap: bool,
    /// Where left / right wraps to.
    pub horizontal_wrap_mode: WrapMode,
}

impl Default for RouteOptions {
    /// Encore's: both ways on, wrapping in the same column / row.
    fn default() -> Self {
        Self {
            vertical: true,
            vertical_wrap: true,
            vertical_wrap_mode: WrapMode::SameLine,
            horizontal: true,
            horizontal_wrap: true,
            horizontal_wrap_mode: WrapMode::SameLine,
        }
    }
}

impl RouteOptions {
    /// Whether these are the defaults (nothing to save).
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// How new buttons are named (Encore's Preferences ▸ Menus ▸ Button Sync Defaults).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ButtonDefaults {
    /// "Sync Button Text and Name" of a new button.
    pub sync_name: bool,
    /// "Set Name from Link" of a new button.
    pub name_from_link: bool,
}

impl Default for ButtonDefaults {
    fn default() -> Self {
        Self {
            sync_name: true,
            name_from_link: false,
        }
    }
}

impl ButtonDefaults {
    /// Whether these are the defaults (nothing to save).
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// The most guides on one menu.
pub const MAX_MENU_GUIDES: usize = 64;

/// A guide line in the menu viewer (Encore's guides): buttons snap to it while dragged. Guides are kept with the
/// menu and never reach the disc.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Guide {
    /// Which way the line runs.
    pub orientation: GuideOrientation,
    /// Its position in frame pixels: the y of a horizontal guide, the x of a vertical one.
    pub position: u16,
}

/// Which way a [`Guide`] runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GuideOrientation {
    /// Across the picture, at a y.
    Horizontal,
    /// Down the picture, at an x.
    Vertical,
}

fn default_text_size() -> u16 {
    DEFAULT_TEXT_SIZE
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            id: MenuId(0),
            name: String::new(),
            background: MenuBackground::default(),
            text_color: default_text_color(),
            highlight_color: default_highlight_color(),
            buttons: Vec::new(),
            default_button: 0,
            end_action: Link::NotSet,
            override_link: None,
            duration_seconds: None,
            texts: Vec::new(),
            prohibited_ops: Vec::new(),
            label_font: None,
            description: String::new(),
            widescreen: false,
            audio: None,
            guides: Vec::new(),
            graphics: Vec::new(),
            color_set: None,
            auto_route: true,
            button_offset: 0,
        }
    }
}

fn default_text_color() -> [u8; 3] {
    [255, 255, 255]
}

fn default_highlight_color() -> [u8; 3] {
    [255, 208, 0]
}

/// Returns the menu frame size for disc settings.
pub fn frame_size(settings: &DiscSettings) -> (u16, u16) {
    match settings.standard {
        VideoStandard::Ntsc => (720, 480),
        VideoStandard::Pal => (720, 576),
    }
}

/// Returns an auto-placed button rectangle.
pub fn auto_button_rect(index: usize, settings: &DiscSettings) -> Rect {
    let (_, frame_h) = frame_size(settings);
    let h: u16 = 48;
    let y = auto_button_y(index, frame_h, h);
    Rect {
        x: 160,
        y,
        w: 400,
        h,
    }
}

fn auto_button_y(index: usize, frame_h: u16, h: u16) -> u16 {
    let index = u32::try_from(index).unwrap_or(u32::MAX);
    let desired = 120u32.saturating_add(64u32.saturating_mul(index));
    let max = u32::from(frame_h).saturating_sub(u32::from(h));
    let y = desired.min(max);
    u16::try_from(y).unwrap_or(frame_h.saturating_sub(h))
}

impl Project {
    /// Adds a menu.
    pub fn add_menu(&mut self, name: Option<String>) -> Result<MenuId, ProjectError> {
        let id = MenuId(self.alloc_id()?);
        let name = name.unwrap_or_else(|| format!("Menu {}", id.0));
        self.menus.insert(
            id,
            Menu {
                id,
                name,
                // A new menu takes the disc's aspect ratio.
                widescreen: self.settings.widescreen,
                ..Default::default()
            },
        );
        Ok(id)
    }

    /// Whether the disc's menus are 16:9: the first menu's choice (a DVD's menus share one aspect ratio).
    pub fn menus_wide(&self) -> bool {
        self.menus
            .values()
            .next()
            .is_some_and(|menu| menu.widescreen)
    }

    /// Borrows a menu.
    pub fn menu(&self, id: MenuId) -> Option<&Menu> {
        self.menus.get(&id)
    }

    /// Mutably borrows a menu.
    pub fn menu_mut(&mut self, id: MenuId) -> Option<&mut Menu> {
        self.menus.get_mut(&id)
    }

    /// Adds a button.
    pub fn add_button(
        &mut self,
        menu: MenuId,
        label: String,
        rect: Option<Rect>,
        link: Link,
    ) -> Result<usize, ProjectError> {
        let index = self
            .menu(menu)
            .ok_or_else(|| ProjectError::Invalid("menu does not exist".to_string()))?
            .buttons
            .len();

        if index >= MAX_BUTTONS {
            return Err(ProjectError::Invalid(
                "menu has too many buttons".to_string(),
            ));
        }

        let rect = rect.unwrap_or_else(|| auto_button_rect(index, &self.settings));
        let defaults = self.settings.button_defaults;
        // Encore と同じく、ボタンの名前はボタンテキスト（空なら番号）。環境設定で「リンク先から名前を設定」なら
        // リンク先の名前。
        let linked = defaults
            .name_from_link
            .then(|| Menu::link_target_name(self, &link))
            .flatten();
        let name = match linked {
            Some(name) => name,
            None if label.trim().is_empty() => format!("Button {}", index.saturating_add(1)),
            None => label.clone(),
        };

        let menu = self
            .menu_mut(menu)
            .ok_or_else(|| ProjectError::Invalid("menu does not exist".to_string()))?;

        menu.buttons.push(Button {
            name,
            label,
            rect,
            link,
            auto_activate: false,
            override_link: None,
            highlight_group: 1,
            sync_name: defaults.sync_name,
            name_from_link: defaults.name_from_link,
            routes: None,
            audio: None,
            subtitle: crate::model::SubtitleDefault::NoChange,
            kind: Default::default(),
            shadow: None,
            orientation: Default::default(),
        });

        Ok(index)
    }

    /// Removes a button.
    pub fn remove_button(&mut self, menu: MenuId, index: usize) -> Result<(), ProjectError> {
        let menu = self
            .menu_mut(menu)
            .ok_or_else(|| ProjectError::Invalid("menu does not exist".to_string()))?;

        if index >= menu.buttons.len() {
            return Err(ProjectError::Invalid(
                "button index out of range".to_string(),
            ));
        }

        menu.buttons.remove(index);

        let len = u32::try_from(menu.buttons.len()).unwrap_or(u32::MAX);
        if menu.default_button >= len {
            menu.default_button = 0;
        }

        Ok(())
    }
}

#[cfg(test)]
mod color_set_tests {
    use super::{ColorSet, HighlightGroup, NormalColor, opacity_to_alpha, opacity_to_contrast};

    #[test]
    fn an_older_colour_set_reads_as_fully_opaque() -> Result<(), serde_json::Error> {
        let group = r#"{"selected": [1, 2, 3], "activated": [4, 5, 6]}"#;
        let text = format!(r#"{{"groups": [{group}, {group}, {group}]}}"#);
        let set: ColorSet = serde_json::from_str(&text)?;
        assert_eq!(set.groups[0], HighlightGroup::new([1, 2, 3], [4, 5, 6]));
        assert_eq!(set.normal, NormalColor::default());
        // Written back, it has no new fields.
        assert_eq!(
            serde_json::to_string(&set.groups[0])?,
            r#"{"selected":[1,2,3],"activated":[4,5,6]}"#
        );
        Ok(())
    }

    #[test]
    fn opacities_become_contrasts_and_alphas() {
        assert_eq!(opacity_to_contrast(0), 0);
        assert_eq!(opacity_to_contrast(47), 7);
        assert_eq!(opacity_to_contrast(100), 15);
        assert_eq!(opacity_to_contrast(250), 15);
        assert_eq!(opacity_to_alpha(47), 120);
        assert_eq!(opacity_to_alpha(100), 255);
    }

    #[test]
    fn the_activated_look_follows_the_check_box() {
        let mut group = HighlightGroup::new([1, 1, 1], [2, 2, 2]);
        group.selected_opacity = 40;
        assert_eq!(group.activated_look(), ([2, 2, 2], 100));
        group.same_activated = true;
        assert_eq!(group.activated_look(), ([1, 1, 1], 40));
    }
}

#[cfg(test)]
mod orientation_tests {
    use super::Orientation;

    /// Where the corners of a 3 x 2 picture land, by the matrix (relative to the centre, doubled to stay whole).
    fn corners(o: Orientation) -> Vec<(i32, i32)> {
        let [a, b, c, d] = o.matrix();
        [(-3.0f32, -2.0f32), (3.0, -2.0), (3.0, 2.0), (-3.0, 2.0)]
            .iter()
            .map(|(x, y)| ((a * x + c * y) as i32, (b * x + d * y) as i32))
            .collect()
    }

    #[test]
    fn mirrors_and_turns_compose() {
        let id = Orientation::default();
        assert!(
            id.flipped_horizontally()
                .flipped_horizontally()
                .is_identity()
        );
        assert!(id.flipped_vertically().flipped_vertically().is_identity());
        assert!(id.rotated(1).rotated(3).is_identity());
        assert_eq!(
            id.flipped_horizontally().flipped_vertically(),
            id.rotated(2)
        );
        // A left-right mirror of a turned object mirrors what is drawn, not the upright picture.
        let turned = id.rotated(1).flipped_horizontally();
        assert_eq!(corners(turned), {
            let drawn = corners(id.rotated(1));
            drawn.iter().map(|(x, y)| (-x, *y)).collect::<Vec<_>>()
        });
        let turned = id.rotated(1).flipped_vertically();
        assert_eq!(corners(turned), {
            let drawn = corners(id.rotated(1));
            drawn.iter().map(|(x, y)| (*x, -y)).collect::<Vec<_>>()
        });
    }

    #[test]
    fn a_quarter_turn_moves_the_top_left_to_the_top_right() {
        let quarter = Orientation::default().rotated(1);
        assert_eq!(corners(quarter)[0], (2, -3));
        // The drawn picture is 2 x 3; its top-right pixel shows the upright top-left one.
        assert_eq!(quarter.upright_pixel(1, 0, 2, 3), (0, 0));
        assert_eq!(quarter.upright_pixel(0, 0, 2, 3), (0, 1));
        let mirrored = Orientation {
            flip: true,
            turns: 0,
        };
        assert_eq!(mirrored.upright_pixel(0, 0, 3, 2), (2, 0));
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_BUTTONS, Rect, auto_button_rect, frame_size};
    use crate::ids::MenuId;
    use crate::link::Link;
    use crate::model::{DiscSettings, Project, VideoStandard};
    use std::error::Error;

    #[test]
    fn add_menu_and_buttons() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        assert_eq!(menu, MenuId(1));
        assert_eq!(project.menu(menu).ok_or("missing menu")?.name, "Menu 1");

        let first = project.add_button(menu, "Play".to_string(), None, Link::Stop)?;
        let second = project.add_button(menu, "Quit".to_string(), None, Link::NotSet)?;
        assert_eq!(first, 0);
        assert_eq!(second, 1);

        let menu = project.menu(menu).ok_or("missing menu")?;
        let y_values: Vec<u16> = menu.buttons.iter().map(|button| button.rect.y).collect();
        assert_eq!(y_values, vec![120, 184]);
        // Encore と同じく、名前はボタンテキスト（空なら番号）。
        assert_eq!(menu.buttons.first().ok_or("missing button")?.name, "Play");

        Ok(())
    }

    #[test]
    fn remove_button_fixes_default_button() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(Some("Main".to_string()))?;
        project.add_button(menu, "A".to_string(), None, Link::Stop)?;
        project.add_button(menu, "B".to_string(), None, Link::Stop)?;

        project.menu_mut(menu).ok_or("missing menu")?.default_button = 1;
        project.remove_button(menu, 1)?;

        let menu = project.menu(menu).ok_or("missing menu")?;
        assert_eq!(menu.default_button, 0);

        Ok(())
    }

    #[test]
    fn max_buttons() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;

        for index in 0..MAX_BUTTONS {
            project.add_button(menu, format!("Button {index}"), None, Link::Stop)?;
        }

        assert!(
            project
                .add_button(menu, "One too many".to_string(), None, Link::Stop)
                .is_err()
        );

        Ok(())
    }

    #[test]
    fn frame_and_auto_rect() {
        let mut settings = DiscSettings::default();
        assert_eq!(frame_size(&settings), (720, 480));

        let rect = auto_button_rect(0, &settings);
        assert_eq!(
            rect,
            Rect {
                x: 160,
                y: 120,
                w: 400,
                h: 48
            }
        );

        settings.standard = VideoStandard::Pal;
        assert_eq!(frame_size(&settings), (720, 576));

        let rect = auto_button_rect(100, &settings);
        assert_eq!(rect.y, 528);
    }
}
