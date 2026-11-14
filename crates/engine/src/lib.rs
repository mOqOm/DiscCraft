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
mod params;
mod player;
mod session;
pub mod waveform;

pub use player::{AudioFeed, FrameSource, Player, PlayerAudio, PlayerFrame, PlayerStatus};
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
