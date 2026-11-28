//! H.263 decoding (ITU-T H.263 (01/2005)), the baseline: the video of old mobile phones' 3GP files and early video
//! calls, as a 3GP, MOV, AVI or Matroska file carries it.
//!
//! - [`Decoder::decode`]: one picture (I or P) of a standard format (sub-QCIF, QCIF, CIF, 4CIF, 16CIF) as 8-bit
//!   Y'CbCr 4:2:0.
//! - [`is_intra`]: whether a picture is an I-picture (the P-pictures after it are decoded from it).
//!
//! The optional modes of the annexes and H.263 version 2's extended picture types (custom sizes) are reported as not
//! supported. Written from ITU-T H.263 alone; no other implementation was read (AGENTS.md rule 3). Every code and
//! number read from a picture is checked: a broken picture is an error, never a panic.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod picture;
mod vlc;

use std::error::Error as StdError;
use std::fmt;

use picture::{Planes, Tables};

/// Why a picture could not be decoded.
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
            Self::Corrupt(what) => write!(f, "H.263: {what}"),
            Self::Unsupported(what) => write!(f, "{what} is not supported"),
        }
    }
}

impl StdError for Error {}

/// A decoded picture: 8-bit Y'CbCr 4:2:0 (ITU-R BT.601 levels), its chroma half as wide and high.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The luma, row after row.
    pub y: Vec<u8>,
    /// Cb, half as wide and high.
    pub cb: Vec<u8>,
    /// Cr, half as wide and high.
    pub cr: Vec<u8>,
    /// Whether it was an I-picture.
    pub intra: bool,
    /// The pixel aspect ratio (12:11 for the standard formats).
    pub pixel_aspect: (u32, u32),
}

/// A decoder of one H.263 stream: it keeps the last picture for the P-pictures after it.
pub struct Decoder {
    tables: Tables,
    reference: Option<Planes>,
}

impl fmt::Debug for Decoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Decoder")
            .field("has_reference", &self.reference.is_some())
            .finish()
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    /// A decoder with no picture yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tables: Tables::new(),
            reference: None,
        }
    }

    /// The picture coded in `data` (from its picture start code). A P-picture needs the pictures before it since
    /// the last I-picture to have been decoded by this decoder.
    ///
    /// # Errors
    ///
    /// When the picture is broken, is a P-picture with nothing before it, or uses an optional mode.
    pub fn decode(&mut self, data: &[u8]) -> Result<Picture, Error> {
        let (picture, planes) = picture::decode(&self.tables, data, self.reference.as_ref())?;
        self.reference = Some(planes);
        Ok(picture)
    }
}

/// Whether `data` is an I-picture.
#[must_use]
pub fn is_intra(data: &[u8]) -> bool {
    picture::is_intra(data)
}

#[cfg(test)]
mod tests {
    use super::{Decoder, is_intra};

    /// Hostile bytes, with and without a picture start code in front: errors or pictures, never a panic.
    #[test]
    fn noise_never_panics() {
        let mut state = 7u64;
        for round in 0..300usize {
            let mut bytes: Vec<u8> = (0..(round * 31) % 2000 + 1)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    (state >> 24) as u8
                })
                .collect();
            if round % 2 == 0 && bytes.len() > 6 {
                // A start code and an I- or P-picture header of a QCIF picture, then noise.
                bytes.splice(
                    0..5,
                    [0, 0, 0x80, 0x02, if round % 4 == 0 { 0x08 } else { 0x0a }],
                );
            }
            let mut decoder = Decoder::new();
            let _ = is_intra(&bytes);
            let _ = decoder.decode(&bytes);
            let _ = decoder.decode(&bytes);
        }
    }
}
