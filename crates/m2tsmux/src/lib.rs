//! BDAV TS（m2ts）の多重化。
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

mod lpcm;
mod mux;
mod packet;
mod psi;

pub use lpcm::bd_lpcm_units;
pub use mux::mux_m2ts;

use std::fmt;

/// PID of the program map table.
pub const PMT_PID: u16 = 0x0100;
/// PID that carries the program clock reference (adaptation-field-only packets).
pub const PCR_PID: u16 = 0x1001;
/// PID of the primary video stream.
pub const VIDEO_PID: u16 = 0x1011;
/// PID of the first primary audio stream (the n-th uses `AUDIO_PID + n`).
pub const AUDIO_PID: u16 = 0x1100;
/// PID of the first presentation graphics stream.
pub const PG_PID: u16 = 0x1200;
/// PID of the first interactive graphics stream.
pub const IG_PID: u16 = 0x1400;

/// Size of one source packet (`TP_extra_header` + transport packet).
pub const SOURCE_PACKET_SIZE: usize = 192;
/// Number of source packets in one aligned unit (6144 bytes).
pub const ALIGNED_UNIT_PACKETS: u32 = 32;

/// Error raised while multiplexing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M2tsError(pub String);

impl fmt::Display for M2tsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for M2tsError {}

/// Kind of an elementary stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    /// MPEG-2 video.
    Mpeg2Video,
    /// H.264 / AVC video (Annex B access units).
    H264Video,
    /// BD LPCM audio (units already carry the 4-byte BD LPCM header).
    Lpcm,
    /// AC-3 audio.
    Ac3,
    /// Presentation graphics (one segment per unit).
    Pgs,
    /// Interactive graphics (one segment per unit).
    Igs,
}

impl StreamKind {
    /// Return the PMT `stream_type`.
    #[must_use]
    pub fn stream_type(self) -> u8 {
        match self {
            Self::Mpeg2Video => 0x02,
            Self::H264Video => 0x1B,
            Self::Lpcm => 0x80,
            Self::Ac3 => 0x81,
            Self::Pgs => 0x90,
            Self::Igs => 0x91,
        }
    }

    /// Return the PES `stream_id`.
    #[must_use]
    pub fn stream_id(self) -> u8 {
        match self {
            Self::Mpeg2Video | Self::H264Video => 0xE0,
            Self::Lpcm | Self::Ac3 | Self::Pgs | Self::Igs => 0xBD,
        }
    }

    /// Return whether this is a video stream (unbounded PES, EP map).
    #[must_use]
    pub fn is_video(self) -> bool {
        matches!(self, Self::Mpeg2Video | Self::H264Video)
    }
}

/// One access unit (or audio frame, or graphics segment) to put in one PES packet.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EsUnit {
    /// Presentation time stamp, 90 kHz.
    pub pts: u64,
    /// Decoding time stamp, 90 kHz (equal to `pts` when there is no separate DTS).
    pub dts: u64,
    /// PES payload.
    pub data: Vec<u8>,
    /// Whether decoding can start here (an I picture with its sequence header).
    pub random_access: bool,
}

/// One elementary stream of the clip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EsInput {
    /// Transport stream PID.
    pub pid: u16,
    /// Stream kind.
    pub kind: StreamKind,
    /// Units in decoding order.
    pub units: Vec<EsUnit>,
}

/// Multiplexer settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MuxConfig {
    /// Transport rate in bits per second (BD-ROM maximum 48 Mbps).
    pub mux_rate: u32,
}

impl Default for MuxConfig {
    fn default() -> Self {
        Self {
            mux_rate: 48_000_000,
        }
    }
}

/// One entry point of the video stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpEntry {
    /// PTS of the random access unit, 90 kHz.
    pub pts: u64,
    /// Number of the source packet that starts its PES packet.
    pub spn: u32,
}

/// What the multiplexer wrote.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct M2tsLayout {
    /// Number of source packets written (a multiple of 32).
    pub source_packets: u32,
    /// Entry points of the first video stream.
    pub ep_map: Vec<EpEntry>,
    /// PID of the stream the EP map refers to.
    pub ep_pid: u16,
    /// Packets that arrived after their decoding time (should be 0).
    pub late_packets: u32,
}
