//! Intra-only MPEG-2 still-picture encoder for DVD menu backgrounds.
//!
//! This milestone encodes one 4:2:0 frame as an MP@ML I picture.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod bitwriter;
mod color;
mod dct;
pub mod encoder;
mod motion;
mod quant;
mod still;
mod vlc;

use std::error::Error;
use std::fmt;

/// Error returned by the encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodeError(pub String);

impl fmt::Display for EncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for EncodeError {}

/// Broadcast standard used for the still picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standard {
    /// 720x480 at 30000/1001 fps.
    Ntsc,
    /// 720x576 at 25 fps.
    Pal,
}

/// 4:2:0 studio-range frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame420 {
    /// Width in pixels.
    pub width: u16,
    /// Height in pixels.
    pub height: u16,
    /// Luma plane, row-major.
    pub y: Vec<u8>,
    /// Cb plane, row-major, half resolution.
    pub cb: Vec<u8>,
    /// Cr plane, row-major, half resolution.
    pub cr: Vec<u8>,
}

/// Options for encoding a single I-picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StillOptions {
    /// Target broadcast standard.
    pub standard: Standard,
    /// Initial quantiser scale code, 1..=31.
    pub quantiser_scale_code: u8,
    /// Maximum accepted encoded size in bytes.
    pub max_bytes: usize,
    /// 16:9 display (aspect_ratio_information 3) instead of 4:3 (2).
    pub wide: bool,
}

impl Default for StillOptions {
    /// Default DVD-menu still options: NTSC, quantiser scale code 4, 224 KiB.
    fn default() -> Self {
        Self {
            standard: Standard::Ntsc,
            quantiser_scale_code: 4,
            max_bytes: 224 * 1024,
            wide: false,
        }
    }
}

/// Convert RGB24 row-major data to a studio-range BT.601 4:2:0 frame.
pub fn rgb_to_frame420(width: u16, height: u16, rgb: &[u8]) -> Result<Frame420, EncodeError> {
    color::rgb_to_frame420(width, height, rgb)
}

/// Encode one 4:2:0 frame as an MPEG-2 I picture.
pub fn encode_still(frame: &Frame420, options: &StillOptions) -> Result<Vec<u8>, EncodeError> {
    still::encode_still(frame, options)
}
