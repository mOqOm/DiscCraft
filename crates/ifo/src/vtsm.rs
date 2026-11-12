//! VTSM_PGCI_UT holding one command-only root menu.
//!
//! A title set's root menu is what the remote's MENU key calls during a title of that set. DiscCraft keeps every
//! menu in the VMGM, so the root menu has no cells: its pre commands jump to the VMGM (the timeline's "menu
//! remote"). With no cells there is no VTSM_VOBS, VTSM_C_ADT or VTSM_VOBU_ADMAP.
//!
//! Layout (DVD-Video IFO tables as for the VMGM_PGCI_UT): the table header (language unit count, reserved, end
//! address), one language unit search pointer (language code, extension, menu existence flags, unit offset), then
//! the language unit (PGCI_SRP count, reserved, end address, one search pointer with the root-menu entry
//! category, the PGC).

use crate::IfoError;
use crate::bytes::{Reader, Writer, u32_from_usize};
use crate::pgc::{Pgc, read_pgc, write_pgc};

/// Menu existence flag of a language unit: the root menu exists.
const ROOT_EXISTS: u8 = 0x80;
/// PGCI_SRP category of the root menu: entry PGC (bit 31) with menu type 3 (root).
const ROOT_MENU_CATEGORY: u32 = 0x8300_0000;
/// The language unit starts right after the table header and its one search pointer.
const LU_OFFSET: usize = 16;
/// The PGC starts right after the unit header and its one search pointer.
const PGC_OFFSET: usize = 16;

/// The root menu of a title set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VtsRootMenu {
    /// ISO 639-1 language code of the menu language unit.
    pub language: [u8; 2],
    /// The root-menu PGC. It must have no cells.
    pub pgc: Pgc,
}

/// Write the VTSM_PGCI_UT at the end of `w`.
pub(crate) fn write_vtsm_pgci_ut(w: &mut Writer, menu: &VtsRootMenu) -> Result<(), IfoError> {
    if !menu.pgc.cells.is_empty() || !menu.pgc.program_entry_cells.is_empty() {
        return Err(IfoError(
            "the VTSM root menu must not have cells".to_string(),
        ));
    }
    let base = w.len();
    let pgc = write_pgc(&menu.pgc)?;
    let lu_len = PGC_OFFSET
        .checked_add(pgc.len())
        .ok_or_else(|| IfoError("VTSM_PGCI_UT overflow".to_string()))?;
    let table_len = LU_OFFSET
        .checked_add(lu_len)
        .ok_or_else(|| IfoError("VTSM_PGCI_UT overflow".to_string()))?;

    w.put_u16(base, 1);
    w.put_u16(at(base, 2)?, 0);
    w.put_u32(at(base, 4)?, end_address(table_len)?);
    w.put_u16(at(base, 8)?, u16::from_be_bytes(menu.language));
    w.put_u8(at(base, 10)?, 0);
    w.put_u8(at(base, 11)?, ROOT_EXISTS);
    w.put_u32(at(base, 12)?, u32_from_usize(LU_OFFSET)?);

    let lu = at(base, LU_OFFSET)?;
    w.put_u16(lu, 1);
    w.put_u16(at(lu, 2)?, 0);
    w.put_u32(at(lu, 4)?, end_address(lu_len)?);
    w.put_u32(at(lu, 8)?, ROOT_MENU_CATEGORY);
    w.put_u32(at(lu, 12)?, u32_from_usize(PGC_OFFSET)?);
    w.put_slice(at(lu, PGC_OFFSET)?, &pgc);
    Ok(())
}

/// Read a VTSM_PGCI_UT written by [`write_vtsm_pgci_ut`] at byte `base`.
pub(crate) fn read_vtsm_pgci_ut(r: &Reader<'_>, base: usize) -> Result<VtsRootMenu, IfoError> {
    let bad = |what: &str| IfoError(format!("bad VTSM_PGCI_UT {what}"));
    if r.u16(base)? != 1 || r.u16(at(base, 2)?)? != 0 {
        return Err(bad("header"));
    }
    let table_len = length_from_end(r.u32(at(base, 4)?)?)?;
    let language = [r.u8(at(base, 8)?)?, r.u8(at(base, 9)?)?];
    if r.u8(at(base, 10)?)? != 0 || r.u8(at(base, 11)?)? != ROOT_EXISTS {
        return Err(bad("menu existence"));
    }
    if usize::try_from(r.u32(at(base, 12)?)?).ok() != Some(LU_OFFSET) {
        return Err(bad("language unit offset"));
    }

    let lu = at(base, LU_OFFSET)?;
    if r.u16(lu)? != 1 || r.u16(at(lu, 2)?)? != 0 {
        return Err(bad("language unit header"));
    }
    let lu_len = length_from_end(r.u32(at(lu, 4)?)?)?;
    if LU_OFFSET.checked_add(lu_len) != Some(table_len) || lu_len <= PGC_OFFSET {
        return Err(bad("length"));
    }
    if r.u32(at(lu, 8)?)? != ROOT_MENU_CATEGORY {
        return Err(bad("menu category"));
    }
    if usize::try_from(r.u32(at(lu, 12)?)?).ok() != Some(PGC_OFFSET) {
        return Err(bad("PGC offset"));
    }
    let pgc = read_pgc(r.slice(at(lu, PGC_OFFSET)?, lu_len - PGC_OFFSET)?)?;
    if !pgc.cells.is_empty() {
        return Err(bad("root menu cells"));
    }
    Ok(VtsRootMenu { language, pgc })
}

fn at(base: usize, delta: usize) -> Result<usize, IfoError> {
    base.checked_add(delta)
        .ok_or_else(|| IfoError("offset overflow".to_string()))
}

/// The end address of a table of `len` bytes (the offset of its last byte).
fn end_address(len: usize) -> Result<u32, IfoError> {
    u32_from_usize(
        len.checked_sub(1)
            .ok_or_else(|| IfoError("empty table".to_string()))?,
    )
}

fn length_from_end(end: u32) -> Result<usize, IfoError> {
    usize::try_from(end)
        .ok()
        .and_then(|end| end.checked_add(1))
        .ok_or_else(|| IfoError("table length overflow".to_string()))
}

#[cfg(test)]
mod tests {
    use super::{VtsRootMenu, read_vtsm_pgci_ut, write_vtsm_pgci_ut};
    use crate::attr::VmCommand;
    use crate::bytes::{Reader, Writer};
    use crate::pgc::Pgc;
    use std::error::Error;

    fn root_menu() -> VtsRootMenu {
        VtsRootMenu {
            language: *b"en",
            pgc: Pgc {
                prohibited_ops: 0,
                // JumpSS VMGM PGCN 2
                pre: vec![VmCommand([0x30, 0x06, 0x00, 0x02, 0x00, 0xC0, 0x00, 0x00])],
                ..Pgc::default()
            },
        }
    }

    #[test]
    fn root_menu_round_trips() -> Result<(), Box<dyn Error>> {
        let mut w = Writer::new();
        w.put_slice(0, &[0xAA; 6]);
        write_vtsm_pgci_ut(&mut w, &root_menu())?;
        let bytes = w.into_bytes();
        let r = Reader::new(&bytes);
        // One language unit, the root menu flag, and the entry category of a root menu.
        assert_eq!(bytes.get(6..8), Some(&[0, 1][..]));
        assert_eq!(bytes.get(6 + 11), Some(&0x80));
        assert_eq!(bytes.get(6 + 16 + 8), Some(&0x83));
        assert_eq!(read_vtsm_pgci_ut(&r, 6)?, root_menu());
        Ok(())
    }

    #[test]
    fn a_root_menu_with_cells_is_refused() {
        let mut menu = root_menu();
        menu.pgc.program_entry_cells = vec![1];
        let mut w = Writer::new();
        assert!(write_vtsm_pgci_ut(&mut w, &menu).is_err());
    }

    #[test]
    fn damaged_tables_are_errors_not_panics() -> Result<(), Box<dyn Error>> {
        let mut w = Writer::new();
        write_vtsm_pgci_ut(&mut w, &root_menu())?;
        let bytes = w.into_bytes();
        for cut in [0, 1, 7, 15, 20, 40, bytes.len() - 1] {
            let short = bytes.get(..cut).unwrap_or_default();
            assert!(read_vtsm_pgci_ut(&Reader::new(short), 0).is_err());
        }
        let mut wrong = bytes.clone();
        if let Some(byte) = wrong.get_mut(4 + 3) {
            *byte = 0xFF;
        }
        assert!(read_vtsm_pgci_ut(&Reader::new(&wrong), 0).is_err());
        Ok(())
    }
}
