//! DiscCraft プロジェクト文書モデル（L1）。
//!
//! This crate owns the serializable document model for a DVD / Blu-ray authoring
//! project. It intentionally contains no engine, UI, or filesystem side effects
//! except for the explicit save/load helpers in [`io`].

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod arrange;
mod built_layer_break;
pub use built_layer_break::{ImageLayerBreak, LayerBreak, LayerBreakPlace};
mod chapter_index;
mod chapter_playlist;
mod chapter_refs;
mod check;
mod check_lists;
mod check_menu;
pub use check_menu::{has_overlapping_buttons, overlapping_buttons};
mod duplicate;
mod effects;
pub use duplicate::Duplicate;
mod folder;
mod graphic;
mod hd_frame;
mod ids;
mod io;
mod library;
mod library_layers;
mod link;
mod lists;
mod located;
mod menu;
mod model;
mod pages;
mod quantize;
mod remove;
mod replace;
mod rom;
mod subpicture;
mod text_span;
mod thumbnail;
mod timecode;
mod transition;

mod user_ops;
mod words;
pub use arrange::{
    Align, Arrange, Axis, align_rects, arranged_order, bounds, distribute_rects, free_offset,
    moved_order, safe_area,
};
pub use chapter_index::FillChapterIndexError;
pub use chapter_index::{CHAPTERS_PER_PAGE, ChapterIndexLabels, MAX_SINGLE_PAGE, chapter_grid};
pub use chapter_playlist::{ChapterPlaylist, MAX_CHAPTER_PLAYLIST_ENTRIES, PlaybackOrder};
pub use check::{Problem, Severity, has_errors};
pub use effects::{
    BevelEffect, GlowEffect, LayerEffects, MAX_EFFECT_SIZE, OverlayEffect, StrokeEffect,
};
pub use filmcraft_time::{FrameRate, Tick};
pub use folder::{Folder, MAX_FOLDERS};
pub use graphic::{
    FULL_OPACITY, GraphicKind, MAX_MENU_GRAPHICS, MenuGraphic, ShapeKind, checked_graphic_rect,
};
pub use hd_frame::{design_rect, hd_frame};
pub use ids::{AssetId, MenuId, TimelineId};
pub use io::{
    FORMAT, MAX_FILE_BYTES, ProjectError, SCHEMA_VERSION, from_json, load, save, to_json,
};
pub use library::{
    ButtonSpec, LIBRARY, LibraryItem, LibraryKind, MenuSpec, PresetButton, library_item,
    preset_rect,
};
pub use library_layers::{LAYER_PRESETS, LayerPreset, PresetLayer, layer_preset};
pub use link::Link;
pub use lists::{
    DEFAULT_SLIDE_DURATION, DEFAULT_SLIDE_SECONDS, DEFAULT_TRANSITION_DURATION, Playlist,
    PlaylistItem, Slide, SlideEffect, SlidePan, SlideScale, SlideStroke, SlideTransition,
    SlideZoom, Slideshow, TransitionKind,
};
pub use menu::{AUTO_LEADING, LINE_PITCHES, MAX_LINE_PITCH, MIN_LINE_PITCH};
pub use menu::{
    Button, DEFAULT_TEXT_SIZE, LETTERBOX_MOST_BUTTONS, MAX_BUTTONS, MAX_FONT_NAME_CHARS,
    MAX_LEADING, MAX_MENU_AUDIO_SECONDS, MAX_MENU_SECONDS, MAX_MENU_TEXTS, MAX_TEXT_CHARS,
    MAX_TEXT_SIZE, MAX_TRACKING, MIN_LEADING, MIN_TEXT_SIZE, MIN_TRACKING, MenuBackground,
    MenuText, Rect, TextAlign, TextCaps, TextFormat, auto_button_rect, frame_size,
    safe_area_guides,
};
pub use menu::{ButtonDefaults, ButtonKind, NO_PSD_PAGE, PopupBackground, RouteOptions, WrapMode};
pub use menu::{
    ColorSet, DEFAULT_ACTIVATED_COLOR, Guide, GuideOrientation, HighlightGroup, MAX_BUTTON_OFFSET,
    MAX_MENU_GUIDES,
};
pub use menu::{ContentBox, normalized_angle, rotated_bounds, rotated_bounds_about};
pub use menu::{
    DEFAULT_NORMAL_MORE, GroupColor, NormalColor, opacity_to_alpha, opacity_to_contrast, tint,
};
pub use menu::{
    DropShadow, MAX_BASELINE, MAX_SHADOW_DISTANCE, MAX_SHADOW_SIZE, MIN_BASELINE, Orientation,
};
pub use menu::{MAX_MENU_LOOPS, MOTION_BUTTONS_SECONDS, MenuLoops};
pub use menu::{
    MAX_TEXT_BOX, MAX_TSUME, MIN_TEXT_BOX, TextKinsoku, TextKinsokuMethod, TextMojikumi,
};
pub use menu::{MAX_TEXT_SCALE, MIN_TEXT_SCALE, TextAntialias, TextScript};
pub use model::BdVideo;
pub use model::{
    Asset, AssetKind, AudioClip, AudioTrack, Capacity, Chapter, DiscKind, DiscSettings,
    MAX_AUDIO_CLIPS, Menu, Project, SubtitleColor, SubtitleColorSet, SubtitleDefault,
    SubtitleGroup, SubtitleTrack, Timeline, VideoStandard,
};
pub use model::{
    BEST_QUALITY, BdCodec, BlurayTranscode, MAX_TRANSCODE_KBPS, MIN_QUALITY, MIN_TRANSCODE_KBPS,
    TranscodeAudio, TranscodeMode,
};
pub use model::{
    ClipTrim, CueImage, CueLook, ImageCueColors, LocatedTranscode, MAX_COLOR_SET_NAME,
    MAX_COLOR_SETS, MAX_SUBTITLE_CUES, MAX_TIMELINE_CLIPS, SubtitleCue, SubtitleJustify,
};
pub use model::{FLOWCHART_DISC, FLOWCHART_LIMIT, FlowPoint, LayerBreakPoint};
pub use model::{MAX_PARENTAL_CHAPTERS, ParentalChapter, ParentalVariant};
pub use quantize::reduce_colors;
pub use replace::{DesignButton, MenuDesign};
pub use rom::{MAX_ROM_DEPTH, MAX_ROM_ENTRIES, RomFile, rom_disc_bytes, rom_files};
pub use subpicture::{ColorLayer, MAX_LAYER_COLORS, MAX_SUBPICTURE_SIDE, Subpicture};
pub use text_span::{MAX_TEXT_SPANS, SpanStyle, TextSpan};
pub use thumbnail::{MenuThumbnail, THUMBNAIL_PARTS, ThumbnailArea, ThumbnailSource};
pub use timecode::{format_frames, format_time, parse_time};
pub use user_ops::{UserOp, dvd_uop_mask, normalized_ops};
pub use words::{Words, default_chapter_number};
