//! DiscCraft エンジン（L4）。
//!
//! This crate owns the interactive editing session, the command registry, and
//! undo/redo state. It operates on the serializable document model from
//! `disccraft-project` and exposes commands in the same shape as FilmCraft's
//! `CommandSpec`.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use serde_json::Value;

mod commands;
mod gop;
pub mod metadata;
pub mod metadata_edit;
mod params;
mod player;
mod session;
pub mod waveform;
pub mod xmp_schemas;

/// Keeps what media files were found to be (length, picture size, thumbnails) in `dir` between runs, so a long
/// video is not read again each time (`disccraft_transcode::enable_probe_cache`). Called by an application once
/// at start.
pub fn enable_media_cache(dir: &std::path::Path) {
    disccraft_transcode::enable_probe_cache(dir);
}

/// The folder of the media cache ([`enable_media_cache`]), when one is kept.
#[must_use]
pub fn media_cache_dir() -> Option<std::path::PathBuf> {
    disccraft_transcode::probe_cache_dir()
}

/// Empties the media cache (Encore's Preferences ▸ Media ▸ Media Cache Database ▸ Clean): what was remembered of
/// media files is forgotten and read again when next needed. Returns how many files went.
pub fn clear_media_cache() -> usize {
    disccraft_transcode::clear_probe_cache()
}

pub use player::{
    AudioFeed, FrameLoader, FrameRequest, FrameSource, Player, PlayerAudio, PlayerAudioPart,
    PlayerFrame, PlayerStatus, audio_track_parts,
};
pub use session::{Session, UNDO_LIMIT};

/// Result type returned by engine commands.
pub type CommandResult = Result<Value, String>;

/// A declarative engine command.
#[derive(Clone, Copy)]
pub struct CommandSpec {
    /// Stable command id, for example `timeline.addChapter`.
    pub id: &'static str,
    /// Human-readable label.
    pub label: &'static str,
    /// Menu paths. An empty slice means the command is hidden from menus.
    pub menu: &'static [&'static str],
    /// Optional keyboard shortcut.
    pub shortcut: Option<&'static str>,
    /// Short parameter description intended for agents.
    pub params: &'static str,
    /// Returns `Err(reason)` when the command cannot currently run.
    pub enabled: fn(&Session) -> Result<(), String>,
    /// Executes the command.
    pub run: fn(&mut Session, &Value) -> CommandResult,
    /// Whether the command is an undoable project edit.
    pub journal: bool,
}

pub use commands::{commands, find_command};
