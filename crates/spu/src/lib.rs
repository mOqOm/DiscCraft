//! DVD subpicture unit encoder and decoder.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::fmt;

mod decode;
mod encode;
mod rle;

pub use decode::decode_spu;
pub use encode::encode_spu;

pub(crate) const MAX_SPU_SIZE: usize = 53_220;
pub(crate) const MAX_WIDTH: u16 = 720;
pub(crate) const MAX_HEIGHT: u16 = 576;
pub(crate) const MAX_PIXELS: usize = 720 * 576;
pub(crate) const MAX_DCSQS: usize = 64;

/// Error returned by SPU encoding and decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpuError(pub String);

impl fmt::Display for SpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SpuError {}

pub(crate) fn spu_error(msg: &str) -> SpuError {
    SpuError(msg.to_string())
}

/// Row-major 2-bit subpicture bitmap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    /// Bitmap width in pixels.
    pub width: u16,
    /// Bitmap height in pixels.
    pub height: u16,
    /// Pixel values, each in `0..=3`.
    pub pixels: Vec<u8>,
}

/// Display parameters for one SPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpuParams {
    /// Left coordinate of the display area.
    pub x: u16,
    /// Top coordinate of the display area.
    pub y: u16,
    /// Palette index for pixel values 0..=3.
    pub colors: [u8; 4],
    /// Alpha value for pixel values 0..=3.
    pub alpha: [u8; 4],
    /// Whether display is forced.
    pub forced: bool,
    /// Optional delay for a stop command.
    pub stop_delay: Option<u16>,
}

/// A decoded SPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedSpu {
    /// Decoded bitmap.
    pub bitmap: Bitmap,
    /// Decoded display parameters.
    pub params: SpuParams,
}

/// Convert a 90 kHz timestamp to SPU delay units, saturating to `u16`.
pub fn spu_delay_from_90k(t: u32) -> u16 {
    let v = t / 1024;
    if v > u32::from(u16::MAX) {
        u16::MAX
    } else {
        v as u16
    }
}

#[cfg(test)]
mod tests {
    use crate::{SpuError, spu_delay_from_90k};

    #[test]
    fn delay_conversion() {
        assert_eq!(spu_delay_from_90k(0), 0);
        assert_eq!(spu_delay_from_90k(1023), 0);
        assert_eq!(spu_delay_from_90k(1024), 1);
        assert_eq!(spu_delay_from_90k(u32::MAX), u16::MAX);
    }

    #[test]
    fn error_display() {
        let e = SpuError("bad".to_string());
        assert_eq!(e.to_string(), "bad");
    }
}
