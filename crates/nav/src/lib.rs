//! DiscCraft navigation compiler (L2).
//!
//! Compiles project links into DVD VM commands for the M3 navigation model.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::cognitive_complexity
)]
#![forbid(unsafe_code)]

mod cmd;
mod compile;

pub use cmd::{
    Cmd, add_gprm, call_ss_vmgm_pgcn, exit, if_eq_goto, if_eq_link_pgcn, if_eq_link_pgn,
    jump_ss_vmgm_pgcn, jump_tt, link_pgcn, link_pgn, mov_gprm, set_gprm, set_hl_btnn,
};
pub use compile::{
    ChapterPlaylistNav, CompiledNav, HiddenKind, HiddenPgc, MenuNav, NavError, TitleNav, compile,
    counted_loop_to_program,
};
