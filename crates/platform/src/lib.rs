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

pub mod displays;
pub mod hw_video;
pub mod memory;

pub use displays::{Display, displays};
pub use hw_video::{
    HwCodec, HwPacket, HwVideoConfig, HwVideoEncoder, hardware_encoders, hardware_h264_encoders,
    use_for_transcodes,
};
pub use memory::installed_memory_bytes;
