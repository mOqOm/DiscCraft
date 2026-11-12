//! DVD VM simulator for written VIDEO_TS folders.
//!
//! This crate loads a VIDEO_TS folder through the project IFO readers and
//! executes the navigation subset documented in `docs/formats/dvd-video.md`.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
#![forbid(unsafe_code)]
#![deny(clippy::cognitive_complexity)]
#![warn(missing_docs)]

use thiserror::Error;

mod exec;
mod explore;
mod model;

pub use exec::{Key, Location, Registers, Vm};
pub use explore::{Report, explore};
pub use model::{ButtonModel, Cmd, DiscModel, MenuPgcModel, TitleModel, load_video_ts};

/// Errors returned by the DVD VM simulator.
#[derive(Debug, Error)]
pub enum VmError {
    /// A file system error occurred while loading a VIDEO_TS folder.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// An IFO file could not be parsed.
    #[error("IFO error: {0}")]
    Ifo(String),

    /// Navigation data or a command argument is invalid.
    #[error("invalid: {0}")]
    Invalid(String),

    /// A VM command outside the supported subset was encountered.
    #[error("unsupported command: {0}")]
    Unsupported(String),

    /// Navigation fell off the end of a command chain without a final state.
    #[error("dead end")]
    DeadEnd,

    /// Navigation executed more than the allowed number of steps without user input.
    #[error("loop")]
    Loop,
}

pub(crate) fn invalid(msg: &str) -> VmError {
    VmError::Invalid(msg.to_string())
}
