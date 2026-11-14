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
mod chapter_index;
mod chapter_playlist;
mod check;
mod check_lists;
mod check_menu;
mod folder;
mod graphic;
mod hd_frame;
mod ids;
mod io;
mod library;
mod link;
mod lists;
mod menu;
mod model;
mod pages;
mod remove;
mod replace;
mod subpicture;
mod thumbnail;
mod timecode;
mod transition;

mod user_ops;
pub use arrange::{
    Align, Arrange, Axis, align_rects, arranged_order, bounds, distribute_rects, free_offset,
    moved_order, safe_area,
};
pub use chapter_index::FillChapterIndexError;
pub use chapter_index::{CHAPTERS_PER_PAGE, ChapterIndexLabels, MAX_SINGLE_PAGE, chapter_grid};
pub use chapter_playlist::{ChapterPlaylist, MAX_CHAPTER_PLAYLIST_ENTRIES, PlaybackOrder};
pub use check::{Problem, Severity, has_errors};
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
pub use link::Link;
pub use lists::{
    DEFAULT_SLIDE_DURATION, DEFAULT_SLIDE_SECONDS, DEFAULT_TRANSITION_DURATION, Playlist,
    PlaylistItem, Slide, SlideScale, SlideTransition, Slideshow, TransitionKind,
};
pub use menu::{
    Button, DEFAULT_TEXT_SIZE, MAX_BUTTONS, MAX_FONT_NAME_CHARS, MAX_LEADING,
    MAX_MENU_AUDIO_SECONDS, MAX_MENU_SECONDS, MAX_MENU_TEXTS, MAX_TEXT_CHARS, MAX_TEXT_SIZE,
    MAX_TRACKING, MIN_LEADING, MIN_TEXT_SIZE, MIN_TRACKING, MenuBackground, MenuText, Rect,
    TextAlign, TextCaps, TextFormat, auto_button_rect, frame_size,
};
pub use menu::{ButtonDefaults, ButtonKind, PopupBackground, RouteOptions, WrapMode};
pub use menu::{
    ColorSet, DEFAULT_ACTIVATED_COLOR, Guide, GuideOrientation, HighlightGroup, MAX_BUTTON_OFFSET,
    MAX_MENU_GUIDES,
};
pub use menu::{
    DEFAULT_NORMAL_MORE, GroupColor, NormalColor, opacity_to_alpha, opacity_to_contrast, tint,
};
pub use menu::{
    DropShadow, MAX_BASELINE, MAX_SHADOW_DISTANCE, MAX_SHADOW_SIZE, MIN_BASELINE, Orientation,
};
pub use menu::{MAX_MENU_LOOPS, MOTION_BUTTONS_SECONDS, MenuLoops};
pub use menu::{MAX_TEXT_SCALE, MIN_TEXT_SCALE, TextAntialias, TextScript};
pub use model::BdVideo;
pub use model::{
    Asset, AssetKind, AudioTrack, Capacity, Chapter, DiscKind, DiscSettings, Menu, Project,
    SubtitleDefault, SubtitleTrack, Timeline, VideoStandard,
};
pub use model::{FLOWCHART_DISC, FLOWCHART_LIMIT, FlowPoint, LayerBreakPoint};
pub use replace::{DesignButton, MenuDesign};
pub use subpicture::{MAX_SUBPICTURE_SIDE, Subpicture};
pub use thumbnail::{MenuThumbnail, THUMBNAIL_PARTS, ThumbnailArea, ThumbnailSource};
pub use timecode::{format_frames, format_time, parse_time};
pub use user_ops::{UserOp, dvd_uop_mask, normalized_ops};
