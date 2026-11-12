/// Elementary stream parsing error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EsError(
    /// Human-readable error message.
    pub String,
);

impl std::fmt::Display for EsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EsError {}

/// Timestamp clock used by this crate: 27 MHz.
pub const CLOCK_27M: i64 = 27_000_000;

/// Convert a 27 MHz timestamp to a 90 kHz timestamp.
pub fn to_90k(t: i64) -> i64 {
    t / 300
}

/// MPEG-2 video elementary stream parser.
pub mod mpeg2v;

/// AC-3 elementary stream parser.
pub mod ac3;

/// WAV to DVD LPCM parser.
pub mod wav;

pub use ac3::{Ac3Frame, Ac3Stream, parse_ac3};
pub use mpeg2v::{Picture, PictureType, SequenceInfo, VideoStream, parse_video};
pub use wav::{LpcmStream, parse_wav};
