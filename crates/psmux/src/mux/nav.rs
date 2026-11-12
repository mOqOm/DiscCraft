//! NAV pack construction and backfill.

use super::pack::{self, put_slice, put_u8, put_u16_be, put_u32_be};
use super::{CellInfo, MuxError, MuxInput, VobuRecord, invalid};
use crate::mux::hli::{Hli, write_hli};
use crate::sink::{SECTOR, SectorSink};

const SYSTEM_HEADER: &[u8] = &[
    0x00, 0x00, 0x01, 0xBB, 0x00, 0x12, 0x80, 0xC4, 0xE1, 0x04, 0xE1, 0xFF, 0xB9, 0xE0, 0xE8, 0xB8,
    0xE0, 0x08, 0xBD, 0xE0, 0x3A, 0xBF, 0xE0, 0x02,
];
const PCI_HEADER: &[u8] = &[0x00, 0x00, 0x01, 0xBF, 0x03, 0xD4, 0x00];
const DSI_HEADER: &[u8] = &[0x00, 0x00, 0x01, 0xBF, 0x03, 0xFA, 0x01];
const PCI_DATA: usize = 38 + PCI_HEADER.len();
const DSI_DATA: usize = 1024 + DSI_HEADER.len();
const SRI_NONE: u32 = 0x3FFF_FFFF;
const SRI_FLAG: u32 = 0x8000_0000;
const HALF_SEC_90K: u64 = 45_000;

const FORWARD_STEPS: [u32; 19] = [
    240, 120, 60, 20, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1,
];
const BACKWARD_STEPS: [u32; 19] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 20, 60, 120, 240,
];

/// Backfill all NAV packs after the VOBS has been written.
pub(crate) fn backfill(
    input: &MuxInput<'_>,
    records: &[VobuRecord],
    cells: &[CellInfo],
    highlight: Option<&Hli>,
    sink: &mut dyn SectorSink,
) -> Result<(), MuxError> {
    if records.is_empty() {
        return Ok(());
    }

    let video_start = records
        .first()
        .ok_or_else(|| invalid("no VOBUs"))?
        .info
        .s_ptm;
    let video_end = records
        .last()
        .ok_or_else(|| invalid("no VOBUs"))?
        .info
        .e_ptm;

    for (idx, record) in records.iter().enumerate() {
        let mut sector = [0u8; SECTOR];
        let ctx = NavCtx {
            input,
            records,
            cells,
            video_start,
            video_end,
        };
        build_nav_pack(&ctx, record, idx, &mut sector)?;
        if let Some(hli) = highlight {
            write_highlight(hli, record.info.s_ptm, &mut sector)?;
        }
        sink.overwrite_sector(record.info.start_sector, &sector)?;
    }

    Ok(())
}

/// Copy the menu highlight into this NAV pack's PCI (data starts at byte 45, 979 bytes), starting at the VOBU's start PTM.
fn write_highlight(hli: &Hli, s_ptm: u32, sector: &mut [u8; SECTOR]) -> Result<(), MuxError> {
    let mut this = hli.clone();
    this.start_ptm = s_ptm;
    let pci = sector
        .get_mut(45..45 + 979)
        .ok_or_else(|| invalid("NAV pack too short"))?;
    write_hli(&this, pci)
}

/// Values shared by every NAV pack of one VOBS.
#[derive(Clone, Copy)]
struct NavCtx<'a> {
    input: &'a MuxInput<'a>,
    records: &'a [VobuRecord],
    cells: &'a [CellInfo],
    video_start: u32,
    video_end: u32,
}

fn build_nav_pack(
    ctx: &NavCtx<'_>,
    record: &VobuRecord,
    idx: usize,
    sector: &mut [u8; SECTOR],
) -> Result<(), MuxError> {
    let NavCtx { input, records, .. } = *ctx;
    pack::write_pack_header(sector, record.nav_scr)?;
    put_slice(sector, 14, SYSTEM_HEADER)?;
    put_slice(sector, 38, PCI_HEADER)?;
    put_slice(sector, 1024, DSI_HEADER)?;

    let elapsed = record
        .info
        .s_ptm
        .checked_sub(record.cell_start_ptm)
        .ok_or_else(|| invalid("negative cell elapsed time"))?;
    let celtm = bcd_time(elapsed, input.ntsc);

    write_pci(record, idx, records, celtm, sector)?;
    write_dsi(ctx, record, idx, celtm, sector)
}

fn write_pci(
    record: &VobuRecord,
    idx: usize,
    records: &[VobuRecord],
    celtm: u32,
    sector: &mut [u8; SECTOR],
) -> Result<(), MuxError> {
    put_u32_be(sector, PCI_DATA, record.info.start_sector)?;
    put_u32_be(sector, PCI_DATA + 0x0C, record.info.s_ptm)?;
    put_u32_be(sector, PCI_DATA + 0x10, record.info.e_ptm)?;
    let se_e_ptm = if idx + 1 == records.len() {
        record.info.e_ptm
    } else {
        0
    };
    put_u32_be(sector, PCI_DATA + 0x14, se_e_ptm)?;
    put_u32_be(sector, PCI_DATA + 0x18, celtm)
}

fn write_dsi(
    ctx: &NavCtx<'_>,
    record: &VobuRecord,
    idx: usize,
    celtm: u32,
    sector: &mut [u8; SECTOR],
) -> Result<(), MuxError> {
    let NavCtx {
        input,
        records,
        cells,
        video_start,
        video_end,
    } = *ctx;
    let scr_base = record.nav_scr / 300;
    let scr_low =
        u32::try_from(scr_base & 0xFFFF_FFFF).map_err(|_| invalid("NAV SCR out of range"))?;

    put_u32_be(sector, DSI_DATA, scr_low)?;
    put_u32_be(sector, DSI_DATA + 0x04, record.info.start_sector)?;

    let ea = record
        .info
        .last_sector
        .checked_sub(record.info.start_sector)
        .ok_or_else(|| invalid("bad VOBU range"))?;
    put_u32_be(sector, DSI_DATA + 0x08, ea)?;

    for order in 0..3 {
        let value = if let Some(last) = record.ref_last.get(order).copied().flatten() {
            last.checked_sub(record.info.start_sector)
                .ok_or_else(|| invalid("bad reference offset"))?
        } else {
            0
        };
        put_u32_be(sector, DSI_DATA + 0x0C + 4 * order, value)?;
    }

    put_u16_be(sector, DSI_DATA + 0x18, input.vob_id)?;
    let cell_id = u8::try_from(record.info.cell + 1).map_err(|_| invalid("too many cells"))?;
    put_u8(sector, DSI_DATA + 0x1B, cell_id)?;
    put_u32_be(sector, DSI_DATA + 0x1C, celtm)?;
    put_u32_be(sector, DSI_DATA + 0x2C, video_start)?;
    put_u32_be(sector, DSI_DATA + 0x30, video_end)?;

    let sri = sri_entries(records, cells, idx)?;
    for (i, value) in sri.iter().enumerate() {
        put_u32_be(sector, DSI_DATA + 0xEA + 4 * i, *value)?;
    }

    for i in 0..8 {
        let value = if let Some(first) = record.first_audio.get(i).copied().flatten() {
            let offset = first
                .checked_sub(record.info.start_sector)
                .ok_or_else(|| invalid("bad audio offset"))?;
            if offset > 0x3FFF {
                return Err(invalid("A_SYNCA offset too large"));
            }
            offset as u16
        } else {
            0x3FFF
        };
        put_u16_be(sector, DSI_DATA + 0x192 + 2 * i, value)?;
    }

    write_sp_synca(record, sector)
}

/// Write DSI SP_SYNCA entries.
fn write_sp_synca(record: &VobuRecord, sector: &mut [u8; SECTOR]) -> Result<(), MuxError> {
    for (i, slot) in record.first_subpicture.iter().enumerate() {
        let value = sp_synca_value(*slot, record.info.start_sector)?;
        put_u32_be(sector, DSI_DATA + 0x1A2 + 4 * i, value)?;
    }
    Ok(())
}

/// Compute one SP_SYNCA entry value.
fn sp_synca_value(first: Option<u32>, nav_sector: u32) -> Result<u32, MuxError> {
    let Some(first) = first else {
        return Ok(0);
    };
    first
        .checked_sub(nav_sector)
        .ok_or_else(|| invalid("bad subpicture offset"))
}

fn sri_entries(
    records: &[VobuRecord],
    cells: &[CellInfo],
    idx: usize,
) -> Result<[u32; 42], MuxError> {
    let mut out = [SRI_NONE; 42];
    let record = records.get(idx).ok_or_else(|| invalid("missing VOBU"))?;
    let cell = cells
        .get(record.info.cell)
        .ok_or_else(|| invalid("bad cell index"))?;
    let start = cell.start_vobu;
    let end = cell.end_vobu;
    if start == usize::MAX || end == 0 || idx < start || idx >= end {
        return Err(invalid("bad cell VOBU range"));
    }

    let cur_nav = record.info.start_sector;

    if idx + 1 < end
        && let Some(next) = records.get(idx + 1)
    {
        let value = sri_entry(cur_nav, next.info.start_sector)?;
        if let Some(slot) = out.get_mut(0) {
            *slot = value;
        }
        if let Some(slot) = out.get_mut(20) {
            *slot = value;
        }
    }

    if idx > start
        && let Some(prev) = records.get(idx - 1)
    {
        let value = sri_entry(cur_nav, prev.info.start_sector)?;
        if let Some(slot) = out.get_mut(21) {
            *slot = value;
        }
        if let Some(slot) = out.get_mut(41) {
            *slot = value;
        }
    }

    for (pos, step) in FORWARD_STEPS.iter().enumerate() {
        if let Some(target) = find_forward(records, start, end, record.info.s_ptm, *step)
            && let Some(trec) = records.get(target)
            && let Some(slot) = out.get_mut(1 + pos)
        {
            *slot = sri_entry(cur_nav, trec.info.start_sector)?;
        }
    }

    for (pos, step) in BACKWARD_STEPS.iter().enumerate() {
        if let Some(target) = find_backward(records, start, end, record.info.s_ptm, *step)
            && let Some(trec) = records.get(target)
            && let Some(slot) = out.get_mut(22 + pos)
        {
            *slot = sri_entry(cur_nav, trec.info.start_sector)?;
        }
    }

    Ok(out)
}

fn sri_entry(cur_nav: u32, target_nav: u32) -> Result<u32, MuxError> {
    let offset = if target_nav > cur_nav {
        target_nav
            .checked_sub(cur_nav)
            .ok_or_else(|| invalid("SRI offset overflow"))?
    } else if target_nav < cur_nav {
        cur_nav
            .checked_sub(target_nav)
            .ok_or_else(|| invalid("SRI offset overflow"))?
    } else {
        return Ok(SRI_NONE);
    };

    if offset >= SRI_FLAG {
        return Err(invalid("SRI offset too large"));
    }
    Ok(SRI_FLAG | offset)
}

fn find_forward(
    records: &[VobuRecord],
    lo: usize,
    hi: usize,
    cur_ptm: u32,
    step: u32,
) -> Option<usize> {
    let add = u64::from(step).checked_mul(HALF_SEC_90K)?;
    let target = u64::from(cur_ptm).checked_add(add)?;
    let mut low = lo;
    let mut high = hi;
    while low < high {
        let mid = low + (high - low) / 2;
        let ptm = records.get(mid)?.info.s_ptm as u64;
        if ptm >= target {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    if low < hi { Some(low) } else { None }
}

fn find_backward(
    records: &[VobuRecord],
    lo: usize,
    hi: usize,
    cur_ptm: u32,
    step: u32,
) -> Option<usize> {
    let sub = u64::from(step).checked_mul(HALF_SEC_90K)?;
    let cur = u64::from(cur_ptm);
    if sub > cur {
        return None;
    }
    let target = cur - sub;
    let mut low = lo;
    let mut high = hi;
    while low < high {
        let mid = low + (high - low) / 2;
        let ptm = records.get(mid)?.info.s_ptm as u64;
        if ptm <= target {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    if low > lo { Some(low - 1) } else { None }
}

fn bcd_time(elapsed_90k: u32, ntsc: bool) -> u32 {
    let ticks = u64::from(elapsed_90k);
    let seconds = ticks / 90_000;
    let rem = ticks % 90_000;
    let frames = if ntsc { rem / 3_003 } else { rem / 3_600 };

    let hours = (seconds / 3_600) % 100;
    let minutes = (seconds / 60) % 60;
    let secs = seconds % 60;

    let h = bcd2(hours as u32);
    let m = bcd2(minutes as u32);
    let s = bcd2(secs as u32);
    let rate = if ntsc { 3 } else { 1 };
    let frame_byte = (rate << 6) | bcd2(frames as u32);

    (h << 24) | (m << 16) | (s << 8) | frame_byte
}

fn bcd2(value: u32) -> u32 {
    let value = value % 100;
    ((value / 10) << 4) | (value % 10)
}
