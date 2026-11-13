//! DVD VM instruction constructors.

/// One DVD VM command.
pub type Cmd = [u8; 8];

/// Returns the high byte of a big-endian 16-bit value.
fn hi(value: u16) -> u8 {
    (value >> 8) as u8
}

/// Returns the low byte of a big-endian 16-bit value.
fn lo(value: u16) -> u8 {
    value as u8
}

/// Masks a general-purpose register number to the instruction field.
fn reg_byte(reg: u8) -> u8 {
    reg & 0x0F
}

/// Encodes a button number into the highlight byte field.
fn button_byte(button: u8) -> u8 {
    ((u16::from(button) & 0x3F) << 2) as u8
}

/// Constructs `Exit`.
pub fn exit() -> Cmd {
    [0x30, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
}

/// Constructs `JumpTT t`.
pub fn jump_tt(t: u8) -> Cmd {
    [0x30, 0x02, 0x00, 0x00, 0x00, t, 0x00, 0x00]
}

/// Constructs `JumpSS VMGM pgcn p`.
pub fn jump_ss_vmgm_pgcn(p: u16) -> Cmd {
    [0x30, 0x06, hi(p), lo(p), 0x00, 0xC0, 0x00, 0x00]
}

/// Constructs `CallSS VMGM pgcn p, rsm_cell r`.
pub fn call_ss_vmgm_pgcn(p: u16, rsm_cell: u8) -> Cmd {
    [0x30, 0x08, hi(p), lo(p), rsm_cell & 0x0F, 0xC0, 0x00, 0x00]
}

/// Constructs `LinkPGCN p`.
pub fn link_pgcn(p: u16) -> Cmd {
    [0x20, 0x04, 0x00, 0x00, 0x00, 0x00, hi(p), lo(p)]
}

/// Constructs `LinkPGN g [, button b]`.
pub fn link_pgn(g: u8, button: u8) -> Cmd {
    [0x20, 0x06, 0x00, 0x00, 0x00, 0x00, button_byte(button), g]
}

/// Constructs `if (GPRM r == v) LinkPGN g`.
pub fn if_eq_link_pgn(reg: u8, value: u16, g: u8) -> Cmd {
    [
        0x20,
        0xA6,
        0x00,
        reg_byte(reg),
        hi(value),
        lo(value),
        0x00,
        g,
    ]
}

/// Constructs `if (GPRM r == v) LinkPGCN p`.
pub fn if_eq_link_pgcn(reg: u8, value: u16, p: u16) -> Cmd {
    [
        0x20,
        0xA4,
        0x00,
        reg_byte(reg),
        hi(value),
        lo(value),
        hi(p),
        lo(p),
    ]
}

/// Constructs `GPRM r = v` with an immediate value.
pub fn set_gprm(reg: u8, value: u16) -> Cmd {
    [
        0x71,
        0x00,
        0x00,
        reg_byte(reg),
        hi(value),
        lo(value),
        0x00,
        0x00,
    ]
}

/// Constructs `GPRM r += v` with an immediate value.
pub fn add_gprm(reg: u8, value: u16) -> Cmd {
    [
        0x73,
        0x00,
        0x00,
        reg_byte(reg),
        hi(value),
        lo(value),
        0x00,
        0x00,
    ]
}

/// Constructs `if (GPRM r == v) Goto line` (lines count from 1 in the command list).
pub fn if_eq_goto(reg: u8, value: u16, line: u8) -> Cmd {
    [
        0x00,
        0xA1,
        0x00,
        reg_byte(reg),
        hi(value),
        lo(value),
        0x00,
        line,
    ]
}

/// Constructs `GPRM d = GPRM s`.
pub fn mov_gprm(dst: u8, src: u8) -> Cmd {
    [
        0x61,
        0x00,
        0x00,
        reg_byte(dst),
        0x00,
        reg_byte(src),
        0x00,
        0x00,
    ]
}

/// Constructs `SetHL_BTNN b`.
pub fn set_hl_btnn(button: u8) -> Cmd {
    [
        0x56,
        0x00,
        0x00,
        0x00,
        button_byte(button),
        0x00,
        0x00,
        0x00,
    ]
}

/// Constructs `SetHL_BTNN GPRM r`: highlight the button GPRM `r` holds, as SPRM8 holds it (button number × 1024).
pub fn set_hl_btnn_gprm(reg: u8) -> Cmd {
    [0x46, 0x00, 0x00, 0x00, 0x00, reg_byte(reg), 0x00, 0x00]
}

/// The value of SPRM8 (and of a GPRM for [`set_hl_btnn_gprm`]) that highlights `button` (one-based).
pub fn highlight_value(button: u8) -> u16 {
    u16::from(button & 0x3F) << 10
}

/// Constructs `SetSTN`: select the audio stream (0-based, `None` keeps the player's), and the subpicture stream
/// (0-based) with whether it is shown. Byte 3 = `0x80 | audio`, byte 4 = `0x80 | (on << 6) | subpicture`; a clear
/// top bit leaves that stream alone.
pub fn set_stn(audio: Option<u8>, subpicture: Option<(bool, u8)>) -> Cmd {
    let a = audio.map_or(0, |stream| 0x80 | (stream & 0x07));
    let s = subpicture.map_or(0, |(on, stream)| {
        0x80 | if on { 0x40 } else { 0 } | (stream & 0x1F)
    });
    [0x51, 0x01, 0x00, a, s, 0x00, 0x00, 0x00]
}

/// Constructs `if (GPRM a != GPRM b) CallSS VMGM pgcn p`.
pub fn if_ne_regs_call_ss_vmgm_pgcn(a: u8, b: u8, p: u16) -> Cmd {
    [
        0x30,
        0x38,
        hi(p),
        lo(p),
        0x00,
        0xC0,
        reg_byte(a),
        reg_byte(b),
    ]
}

/// Constructs `if (GPRM a == GPRM b) JumpTT t`.
#[allow(dead_code)]
pub fn if_eq_regs_jump_tt(a: u8, b: u8, t: u8) -> Cmd {
    [0x30, 0x22, 0x00, 0x00, 0x00, t, reg_byte(a), reg_byte(b)]
}

#[cfg(test)]
mod tests {
    use super::{
        call_ss_vmgm_pgcn, exit, if_eq_link_pgcn, if_eq_link_pgn, if_eq_regs_jump_tt,
        if_ne_regs_call_ss_vmgm_pgcn, jump_ss_vmgm_pgcn, jump_tt, link_pgcn, link_pgn, mov_gprm,
        set_gprm, set_hl_btnn,
    };

    #[test]
    fn set_stn_matches_document() {
        assert_eq!(
            super::set_stn(Some(1), None),
            [0x51, 0x01, 0x00, 0x81, 0x00, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            super::set_stn(None, Some((true, 2))),
            [0x51, 0x01, 0x00, 0x00, 0xC2, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            super::set_stn(None, Some((false, 0))),
            [0x51, 0x01, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn constructors_match_document() {
        assert_eq!(exit(), [0x30, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        assert_eq!(jump_tt(2), [0x30, 0x02, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00]);
        assert_eq!(
            jump_ss_vmgm_pgcn(0x0102),
            [0x30, 0x06, 0x01, 0x02, 0x00, 0xC0, 0x00, 0x00]
        );
        assert_eq!(
            call_ss_vmgm_pgcn(4, 0),
            [0x30, 0x08, 0x00, 0x04, 0x00, 0xC0, 0x00, 0x00]
        );
        assert_eq!(
            link_pgcn(0x0304),
            [0x20, 0x04, 0x00, 0x00, 0x00, 0x00, 0x03, 0x04]
        );
        assert_eq!(
            link_pgn(5, 3),
            [0x20, 0x06, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x05]
        );
        assert_eq!(
            if_eq_link_pgn(1, 3, 3),
            [0x20, 0xA6, 0x00, 0x01, 0x00, 0x03, 0x00, 0x03]
        );
        assert_eq!(
            if_eq_link_pgcn(2, 5, 7),
            [0x20, 0xA4, 0x00, 0x02, 0x00, 0x05, 0x00, 0x07]
        );
        assert_eq!(
            set_gprm(2, 5),
            [0x71, 0x00, 0x00, 0x02, 0x00, 0x05, 0x00, 0x00]
        );
        assert_eq!(
            mov_gprm(1, 0),
            [0x61, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            set_hl_btnn(3),
            [0x56, 0x00, 0x00, 0x00, 0x0C, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            if_ne_regs_call_ss_vmgm_pgcn(3, 5, 7),
            [0x30, 0x38, 0x00, 0x07, 0x00, 0xC0, 0x03, 0x05]
        );
        assert_eq!(
            if_eq_regs_jump_tt(3, 5, 2),
            [0x30, 0x22, 0x00, 0x00, 0x00, 0x02, 0x03, 0x05]
        );
    }

    #[test]
    fn register_fields_are_masked() {
        assert_eq!(
            set_gprm(16, 1),
            [0x71, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00]
        );
        assert_eq!(
            mov_gprm(16, 17),
            [0x61, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00]
        );
    }
}
