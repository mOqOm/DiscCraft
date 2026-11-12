//! User operations (Encore's "User Operations"): what a viewer may do with the remote during a title or a menu.
//! A timeline or a menu can prohibit some of them, for example fast-forwarding through a warning.
//!
//! The DVD bit of each operation is its UOP number in a PGC's prohibited-user-operations field (PGC byte 0x08,
//! 32 bits, UOP0 = bit 0), as in the public DVD-Video structure references (see docs/formats/dvd-video.md).

use serde::{Deserialize, Serialize};

/// One remote operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UserOp {
    /// Play or search by time (UOP0).
    TimePlay,
    /// Play or search by chapter (UOP1).
    ChapterPlay,
    /// Play a title (UOP2).
    TitlePlay,
    /// Stop (UOP3).
    Stop,
    /// Go up (UOP4).
    GoUp,
    /// Time or chapter search within the title (UOP5).
    TimeSearch,
    /// Previous chapter or top of the chapter (UOP6).
    PreviousChapter,
    /// Next chapter (UOP7).
    NextChapter,
    /// Fast forward (UOP8).
    FastForward,
    /// Rewind (UOP9).
    Rewind,
    /// The Title (top menu) key (UOP10).
    TitleMenu,
    /// The Menu (root menu) key (UOP11).
    RootMenu,
    /// The subtitle menu (UOP12).
    SubtitleMenu,
    /// The audio menu (UOP13).
    AudioMenu,
    /// The angle menu (UOP14).
    AngleMenu,
    /// The chapter menu (UOP15).
    ChapterMenu,
    /// Resume (UOP16).
    Resume,
    /// Selecting or activating a button (UOP17).
    ButtonSelect,
    /// Leaving a still (UOP18).
    StillOff,
    /// Pause (UOP19).
    Pause,
    /// Changing the audio stream (UOP20).
    AudioChange,
    /// Changing the subtitle stream (UOP21).
    SubtitleChange,
    /// Changing the angle (UOP22).
    AngleChange,
    /// Changing the karaoke mix (UOP23).
    KaraokeChange,
    /// Changing the video presentation mode (UOP24).
    VideoModeChange,
}

impl UserOp {
    /// Every operation, in UOP order.
    pub const ALL: [UserOp; 25] = [
        UserOp::TimePlay,
        UserOp::ChapterPlay,
        UserOp::TitlePlay,
        UserOp::Stop,
        UserOp::GoUp,
        UserOp::TimeSearch,
        UserOp::PreviousChapter,
        UserOp::NextChapter,
        UserOp::FastForward,
        UserOp::Rewind,
        UserOp::TitleMenu,
        UserOp::RootMenu,
        UserOp::SubtitleMenu,
        UserOp::AudioMenu,
        UserOp::AngleMenu,
        UserOp::ChapterMenu,
        UserOp::Resume,
        UserOp::ButtonSelect,
        UserOp::StillOff,
        UserOp::Pause,
        UserOp::AudioChange,
        UserOp::SubtitleChange,
        UserOp::AngleChange,
        UserOp::KaraokeChange,
        UserOp::VideoModeChange,
    ];

    /// The operation's UOP number (its bit in the DVD field).
    pub fn uop(self) -> u32 {
        let index = UserOp::ALL
            .iter()
            .position(|op| *op == self)
            .unwrap_or_default();
        u32::try_from(index).unwrap_or_default()
    }

    /// The name used in project files and commands ("fastForward").
    pub fn name(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    /// The operation named `name` ("fastForward"), if there is one.
    pub fn parse(name: &str) -> Option<UserOp> {
        UserOp::ALL.into_iter().find(|op| op.name() == name)
    }
}

/// The DVD prohibited-user-operations field for `prohibited` (0 allows everything).
pub fn dvd_uop_mask(prohibited: &[UserOp]) -> u32 {
    prohibited
        .iter()
        .fold(0, |mask, op| mask | (1u32 << op.uop()))
}

/// `ops` without repeats, in UOP order.
pub fn normalized_ops(ops: &[UserOp]) -> Vec<UserOp> {
    UserOp::ALL
        .into_iter()
        .filter(|op| ops.contains(op))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{UserOp, dvd_uop_mask, normalized_ops};

    #[test]
    fn the_mask_has_one_bit_per_uop() {
        assert_eq!(dvd_uop_mask(&[]), 0);
        assert_eq!(dvd_uop_mask(&[UserOp::TimePlay]), 1);
        assert_eq!(dvd_uop_mask(&[UserOp::FastForward, UserOp::Rewind]), 0x300);
        assert_eq!(dvd_uop_mask(&[UserOp::RootMenu]), 1 << 11);
        assert_eq!(dvd_uop_mask(&UserOp::ALL), 0x01FF_FFFF);
    }

    #[test]
    fn names_round_trip() {
        for op in UserOp::ALL {
            assert_eq!(UserOp::parse(&op.name()), Some(op));
        }
        assert_eq!(UserOp::FastForward.name(), "fastForward");
        assert_eq!(UserOp::parse("warp"), None);
    }

    #[test]
    fn normalizing_sorts_and_drops_repeats() {
        assert_eq!(
            normalized_ops(&[UserOp::Rewind, UserOp::Stop, UserOp::Rewind]),
            vec![UserOp::Stop, UserOp::Rewind]
        );
    }
}
