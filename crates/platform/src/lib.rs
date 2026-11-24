//! ディスク書き込みの OS API（unsafe を許す唯一の crate）。
//!
//! 層: L5（docs/architecture.md §5）。
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod burn;
pub mod displays;
pub mod hw_video;
pub mod memory;

pub use burn::{BurnError, BurnRequest, BurnStep, LayerBoundary, Media, MediaKind, Recorder};
pub use displays::{Display, displays, keep_on_top};
pub use hw_video::{
    HwCodec, HwPacket, HwVideoConfig, HwVideoEncoder, hardware_encoders, hardware_h264_encoders,
    use_for_transcodes,
};
pub use memory::installed_memory_bytes;
