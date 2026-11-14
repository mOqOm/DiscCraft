//! DVD IFO/BUP writer and reader for DiscCraft milestone M2.3.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::error::Error;
use std::fmt;

/// Error returned by IFO/BUP parsing and writing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfoError(pub String);

impl fmt::Display for IfoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for IfoError {}

/// Unused sectors between the last VOB of a set (or its IFO, when it has none) and its BUP, counted in the set's
/// last sector. The IFO and its backup must not share an ECC block (16 sectors): a gap of a whole block keeps
/// them apart whatever the VOBs between them, as Encore lays sets out. The disc image leaves the same gap
/// (`disccraft_udf`'s `BUP_GAP_SECTORS`).
pub const BUP_GAP_SECTORS: u32 = 16;

mod attr;
mod bytes;
mod pgc;
mod vmgi;
mod vtsi;
mod vtsm;

pub use attr::{AudioAttr, AudioCoding, BcdTime, Standard, VideoAttr, VmCommand};
pub use bytes::SECTOR;
pub use pgc::{Cell, Pgc, read_pgc, write_pgc};
pub use vmgi::{
    CellAddress, MenuPgc, TitleEntry, VmgMenus, Vmgi, read_vmgi, vmgi_sectors, write_vmgi,
};
pub use vtsi::{TimeMap, Vtsi, read_vtsi, vts_attr_block, write_vtsi};
pub use vtsm::VtsRootMenu;
