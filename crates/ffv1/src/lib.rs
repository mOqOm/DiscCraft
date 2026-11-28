//! FFV1 decoding (IETF RFC 9043): the lossless intra-frame video format of archives and of video captured to keep,
//! versions 0, 1 and 3, as an AVI, a Matroska or a QuickTime file carries it.
//!
//! - [`Decoder::new`]: a decoder for a stream of a frame size, with its configuration record (version 3).
//! - [`Decoder::decode`]: one frame's picture: Y'CbCr (4:2:0, 4:2:2, 4:4:4 and others), grey or RGB, with or without
//!   transparency, 8 to 16 bits a sample.
//! - [`is_keyframe`]: whether a frame starts afresh (the frames after it carry its adaptive states on).
//!
//! Written from RFC 9043 (August 2021) alone; no other FFV1 implementation was read (AGENTS.md rule 3). What the RFC
//! leaves open was settled by decoding streams ffmpeg made and comparing with ffmpeg's own decoding (rule 9, an
//! outside oracle); the README says what. Every number read from a stream is checked: a broken frame is an error,
//! never a panic.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod coder;
mod decode;
mod params;

use std::error::Error as StdError;
use std::fmt;

use coder::{RangeDecoder, Transitions};
use decode::SliceState;
use params::Parameters;

/// Why a stream or a frame could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The data breaks the format.
    Corrupt(&'static str),
    /// The data uses something this decoder does not decode.
    Unsupported(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupt(what) => write!(f, "FFV1: {what}"),
            Self::Unsupported(what) => write!(f, "FFV1: {what} is not supported"),
        }
    }
}

impl StdError for Error {}

/// How a picture's planes are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Y', Cb, Cr: the chroma planes `1 << h_shift` and `1 << v_shift` times smaller across and down (rounded up).
    Yuv {
        /// log2 of the chroma's horizontal subsampling.
        h_shift: u32,
        /// log2 of the chroma's vertical subsampling.
        v_shift: u32,
    },
    /// Y' alone.
    Gray,
    /// Green, blue, red (back from the reversible colour transform), each the picture's size.
    Gbr,
}

/// A decoded picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bits of each sample (8 to 16).
    pub bits: u32,
    /// The planes' meaning.
    pub layout: Layout,
    /// Whether the last plane is transparency.
    pub alpha: bool,
    /// The planes, row after row: as [`Layout`] says, then transparency.
    pub planes: Vec<Vec<u16>>,
    /// The first slice's picture structure (0 unknown, 1 top field first, 2 bottom field first, 3 progressive).
    pub picture_structure: u32,
    /// The sample aspect ratio the frame gives, if any.
    pub sample_aspect: Option<(u32, u32)>,
}

/// A decoder of one FFV1 stream.
pub struct Decoder {
    size: (usize, usize),
    record: Option<Parameters>,
    kept: Option<Parameters>,
    slices: Vec<SliceState>,
}

impl fmt::Debug for Decoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Decoder")
            .field("size", &self.size)
            .field(
                "version",
                &self
                    .record
                    .as_ref()
                    .or(self.kept.as_ref())
                    .map(|p| p.version),
            )
            .finish()
    }
}

impl Decoder {
    /// A decoder for frames `width` x `height` (the container's size), with the stream's configuration record when
    /// the container has one (version 3: an AVI's format after its BITMAPINFOHEADER, a Matroska track's private
    /// data, a QuickTime `glbl` box).
    ///
    /// # Errors
    ///
    /// When the record is broken or of a kind not decoded.
    pub fn new(width: u32, height: u32, record: Option<&[u8]>) -> Result<Self, Error> {
        let size = (
            usize::try_from(width).unwrap_or(0),
            usize::try_from(height).unwrap_or(0),
        );
        let record = match record.filter(|record| !record.is_empty()) {
            Some(record) => Some(params::read_configuration_record(record)?),
            None => None,
        };
        Ok(Self {
            size,
            record,
            kept: None,
            slices: Vec::new(),
        })
    }

    /// The picture of `frame`. A frame that is not a key frame needs the frames before it since the last key frame
    /// to have been decoded by this decoder.
    ///
    /// # Errors
    ///
    /// When the frame is broken, comes before any key frame, or uses something not decoded.
    pub fn decode(&mut self, frame: &[u8]) -> Result<Picture, Error> {
        decode::decode_frame(
            frame,
            self.size,
            self.record.as_ref(),
            &mut self.kept,
            &mut self.slices,
        )
    }
}

/// Whether `frame` is a key frame (its first symbol, §4.4).
#[must_use]
pub fn is_keyframe(frame: &[u8]) -> bool {
    let mut state = 128u8;
    !frame.is_empty() && RangeDecoder::new(frame).bit(&mut state, &Transitions::default_tables())
}

#[cfg(test)]
mod tests {
    use super::{Decoder, is_keyframe};

    /// Bytes from a fixed xorshift generator.
    fn noise(seed: u64, len: usize) -> Vec<u8> {
        let mut state = seed | 1;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect()
    }

    /// Hostile bytes as a configuration record and as frames (with no record, and frame sizes from tiny to large):
    /// errors or pictures, never a panic.
    #[test]
    fn noise_never_panics() {
        for seed in 1..400u64 {
            let bytes = noise(seed, (seed as usize * 37) % 3000 + 1);
            let _ = Decoder::new(64, 48, Some(&bytes));
            let _ = is_keyframe(&bytes);
            for (width, height) in [(1, 1), (17, 9), (64, 48)] {
                if let Ok(mut decoder) = Decoder::new(width, height, None) {
                    let _ = decoder.decode(&bytes);
                    let _ = decoder.decode(&bytes[..bytes.len() / 2]);
                }
            }
        }
    }
}
