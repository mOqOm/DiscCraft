//! index.bdmv / MovieObject.bdmv / MPLS / CLPI の読み書き。
//!
//! 層: L0（docs/architecture.md §5）。資料は crate の README を参照。
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod clpi;
pub mod index;
pub mod mobj;
pub mod mpls;

use std::fmt;

/// Error raised while writing or reading a BDMV file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BdmvError(pub String);

impl fmt::Display for BdmvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BdmvError {}

/// Version string written after the type indicator of every file ("0200").
pub const VERSION: &[u8; 4] = b"0200";

/// Coding of one elementary stream: `stream_coding_type` and its attributes.
///
/// The attribute codes are the ones of the BD-ROM application format:
/// `video_format` 1 = 480i, 2 = 576i, 3 = 480p, 4 = 1080i, 5 = 720p, 6 = 1080p, 7 = 576p;
/// `frame_rate` 1 = 23.976, 2 = 24, 3 = 25, 4 = 29.97, 6 = 50, 7 = 59.94;
/// `aspect` 2 = 4:3, 3 = 16:9;
/// `audio_format` 1 = mono, 3 = stereo, 6 = multi-channel;
/// `sample_rate` 1 = 48 kHz, 4 = 96 kHz, 5 = 192 kHz;
/// `language` is an ISO 639-2 code such as `*b"jpn"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamCoding {
    /// MPEG-2 video (`stream_coding_type` 0x02).
    Mpeg2Video {
        /// Video format code.
        video_format: u8,
        /// Frame rate code.
        frame_rate: u8,
        /// Aspect ratio code.
        aspect: u8,
    },
    /// H.264 / AVC video (`stream_coding_type` 0x1B).
    H264Video {
        /// Video format code.
        video_format: u8,
        /// Frame rate code.
        frame_rate: u8,
        /// Aspect ratio code.
        aspect: u8,
    },
    /// LPCM audio (`stream_coding_type` 0x80).
    Lpcm {
        /// Audio format (channel layout) code.
        audio_format: u8,
        /// Sample rate code.
        sample_rate: u8,
        /// ISO 639-2 language code.
        language: [u8; 3],
    },
    /// AC-3 audio (`stream_coding_type` 0x81).
    Ac3 {
        /// Audio format (channel layout) code.
        audio_format: u8,
        /// Sample rate code.
        sample_rate: u8,
        /// ISO 639-2 language code.
        language: [u8; 3],
    },
    /// Presentation graphics (`stream_coding_type` 0x90).
    Pgs {
        /// ISO 639-2 language code.
        language: [u8; 3],
    },
    /// Interactive graphics (`stream_coding_type` 0x91).
    Igs {
        /// ISO 639-2 language code.
        language: [u8; 3],
    },
}

impl StreamCoding {
    /// Return the `stream_coding_type` byte.
    #[must_use]
    pub fn coding_type(&self) -> u8 {
        match self {
            Self::Mpeg2Video { .. } => 0x02,
            Self::H264Video { .. } => 0x1B,
            Self::Lpcm { .. } => 0x80,
            Self::Ac3 { .. } => 0x81,
            Self::Pgs { .. } => 0x90,
            Self::Igs { .. } => 0x91,
        }
    }
}

/// One elementary stream of a clip: its PID and coding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stream {
    /// Transport stream PID.
    pub pid: u16,
    /// Coding and attributes.
    pub coding: StreamCoding,
}

/// Convert a byte length to `u32`, failing instead of truncating.
pub(crate) fn len_u32(len: usize) -> Result<u32, BdmvError> {
    u32::try_from(len).map_err(|_| BdmvError(format!("length {len} does not fit in 32 bits")))
}

/// Overwrite the big-endian `u32` at `at`.
pub(crate) fn patch_u32(buf: &mut [u8], at: usize, value: u32) -> Result<(), BdmvError> {
    let end = at
        .checked_add(4)
        .ok_or_else(|| BdmvError("patch offset overflow".to_string()))?;
    let slot = buf
        .get_mut(at..end)
        .ok_or_else(|| BdmvError(format!("patch offset {at} out of range")))?;
    slot.copy_from_slice(&value.to_be_bytes());
    Ok(())
}

/// Big-endian reader over a byte slice that reports truncation as an error.
#[derive(Debug, Clone)]
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Start reading `data` at `pos`.
    pub(crate) fn at(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }

    /// Current position.
    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    /// Read `n` bytes.
    pub(crate) fn bytes(&mut self, n: usize) -> Result<&'a [u8], BdmvError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| BdmvError("read offset overflow".to_string()))?;
        let out = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| BdmvError(format!("truncated at byte {}", self.pos)))?;
        self.pos = end;
        Ok(out)
    }

    /// Skip `n` bytes.
    pub(crate) fn skip(&mut self, n: usize) -> Result<(), BdmvError> {
        self.bytes(n).map(|_| ())
    }

    /// Read one byte.
    pub(crate) fn u8(&mut self) -> Result<u8, BdmvError> {
        let b = self.bytes(1)?;
        b.first()
            .copied()
            .ok_or_else(|| BdmvError("truncated".to_string()))
    }

    /// Read a big-endian `u16`.
    pub(crate) fn u16(&mut self) -> Result<u16, BdmvError> {
        let b = self.bytes(2)?;
        let arr: [u8; 2] = b
            .try_into()
            .map_err(|_| BdmvError("truncated".to_string()))?;
        Ok(u16::from_be_bytes(arr))
    }

    /// Read a big-endian `u32`.
    pub(crate) fn u32(&mut self) -> Result<u32, BdmvError> {
        let b = self.bytes(4)?;
        let arr: [u8; 4] = b
            .try_into()
            .map_err(|_| BdmvError("truncated".to_string()))?;
        Ok(u32::from_be_bytes(arr))
    }
}

#[cfg(test)]
mod tests {
    use super::{Reader, patch_u32};

    #[test]
    fn reader_reports_truncation() {
        let data = [1, 2, 3];
        let mut r = Reader::at(&data, 0);
        assert_eq!(r.u16(), Ok(0x0102));
        assert!(r.u16().is_err());
    }

    #[test]
    fn patch_writes_big_endian() {
        let mut buf = vec![0; 6];
        assert!(patch_u32(&mut buf, 1, 0x0A0B_0C0D).is_ok());
        assert_eq!(buf, [0, 0x0A, 0x0B, 0x0C, 0x0D, 0]);
        assert!(patch_u32(&mut buf, 3, 1).is_err());
    }
}
