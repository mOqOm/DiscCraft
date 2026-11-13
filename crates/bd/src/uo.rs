//! Blu-ray user operation masks (the UO mask table of a playlist) from DiscCraft's user operations, which follow
//! DVD's UOP0 to UOP24 (Encore's ユーザー操作 dialog).

use disccraft_project::UserOp;

/// The UO mask table's fields, from its first (most significant) bit.
const MENU_CALL: u32 = 0;
const TITLE_SEARCH: u32 = 1;
const CHAPTER_SEARCH: u32 = 2;
const TIME_SEARCH: u32 = 3;
const SKIP_TO_NEXT_POINT: u32 = 4;
const SKIP_TO_PREV_POINT: u32 = 5;
const STOP: u32 = 7;
const PAUSE_ON: u32 = 8;
const STILL_OFF: u32 = 10;
const FORWARD_PLAY: u32 = 11;
const BACKWARD_PLAY: u32 = 12;
const RESUME: u32 = 13;
/// Move up / down / left / right, select, activate, select and activate.
const BUTTON_FIELDS: [u32; 7] = [14, 15, 16, 17, 18, 19, 20];
const PRIMARY_AUDIO_CHANGE: u32 = 21;
const ANGLE_CHANGE: u32 = 23;
const POPUP_ON: u32 = 24;
const PG_ENABLE_DISABLE: u32 = 26;
const PG_CHANGE: u32 = 27;

/// The UO mask table fields a DiscCraft user operation covers on a Blu-ray. The DVD menu keys without a Blu-ray
/// key (subtitle / audio / angle / chapter menu), Go Up, karaoke and video mode have none; the Title key is the
/// Top Menu key (menu call) and the Menu key the Pop-up Menu key.
fn fields(op: UserOp) -> &'static [u32] {
    match op {
        UserOp::TimePlay => &[TIME_SEARCH],
        UserOp::ChapterPlay => &[CHAPTER_SEARCH],
        UserOp::TitlePlay => &[TITLE_SEARCH],
        UserOp::Stop => &[STOP],
        UserOp::TimeSearch => &[TIME_SEARCH, CHAPTER_SEARCH],
        UserOp::PreviousChapter => &[SKIP_TO_PREV_POINT],
        UserOp::NextChapter => &[SKIP_TO_NEXT_POINT],
        UserOp::FastForward => &[FORWARD_PLAY],
        UserOp::Rewind => &[BACKWARD_PLAY],
        UserOp::TitleMenu => &[MENU_CALL],
        UserOp::RootMenu => &[POPUP_ON],
        UserOp::Resume => &[RESUME],
        UserOp::ButtonSelect => &BUTTON_FIELDS,
        UserOp::StillOff => &[STILL_OFF],
        UserOp::Pause => &[PAUSE_ON],
        UserOp::AudioChange => &[PRIMARY_AUDIO_CHANGE],
        UserOp::SubtitleChange => &[PG_ENABLE_DISABLE, PG_CHANGE],
        UserOp::AngleChange => &[ANGLE_CHANGE],
        UserOp::GoUp
        | UserOp::SubtitleMenu
        | UserOp::AudioMenu
        | UserOp::AngleMenu
        | UserOp::ChapterMenu
        | UserOp::KaraokeChange
        | UserOp::VideoModeChange => &[],
    }
}

/// The UO mask table (field 0 in the most significant bit) that prohibits the operations of every list in `lists`
/// (an item's own and the disc's).
pub(crate) fn uo_mask(lists: &[&[UserOp]]) -> u64 {
    lists
        .iter()
        .flat_map(|ops| ops.iter())
        .flat_map(|op| fields(*op).iter())
        .fold(0, |mask, field| mask | (1_u64 << (63 - field)))
}

#[cfg(test)]
mod tests {
    use super::uo_mask;
    use disccraft_project::UserOp;

    #[test]
    fn user_operations_become_the_blu_ray_uo_mask() {
        assert_eq!(uo_mask(&[]), 0);
        // The Title key is the menu call (the table's first bit), next chapter the skip to the next point.
        assert_eq!(uo_mask(&[&[UserOp::TitleMenu]]), 1 << 63);
        assert_eq!(uo_mask(&[&[UserOp::NextChapter]]), 1 << 59);
        // An item's own and the disc's operations add up; a DVD-only one adds nothing.
        assert_eq!(
            uo_mask(&[&[UserOp::Stop], &[UserOp::RootMenu, UserOp::AudioMenu]]),
            (1 << 56) | (1 << 39)
        );
        // Selecting buttons covers the seven button operations.
        assert_eq!(uo_mask(&[&[UserOp::ButtonSelect]]).count_ones(), 7);
    }
}
