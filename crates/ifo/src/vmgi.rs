use crate::IfoError;
use crate::attr::{AudioAttr, VideoAttr};
use crate::bytes::{Reader, SECTOR, Writer, u32_from_usize};
use crate::pgc::{Pgc, read_pgc, write_pgc};

const FP_PGC_OFFSET: usize = 0x400;
const TITLE_TYPE: u8 = 0x3C;
const VTS_ATR_SIZE: usize = 0x308;
const VTS_ATTR_BLOCK: usize = 0x300;
const MAX_TITLES: usize = 99;
const MAX_VTS: usize = 99;
const MAX_MENU_PGC: usize = 999;
const MENU_EXISTENCE: u8 = 0x80;
const TITLE_MENU_CATEGORY: u32 = 0x8200_0000;
const VMGM_LU_OFFSET: usize = 16;

/// One title entry in the VMG title search pointer table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleEntry {
    /// Number of chapters (PTTs) in this title.
    pub chapters: u16,
    /// VTS number, 1-based.
    pub vts: u8,
    /// Title number within the VTS.
    pub vts_ttn: u8,
    /// Start sector of the VTS, counted from the first sector of VIDEO_TS.IFO.
    pub vts_start_sector: u32,
}

/// One menu PGC in the VMGM PGC information unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuPgc {
    /// Whether this is the title-menu entry PGC.
    pub title_menu: bool,
    /// The PGC itself.
    pub pgc: Pgc,
}

/// One VMGM cell address entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellAddress {
    /// VOB identifier.
    pub vob_id: u16,
    /// Cell identifier.
    pub cell_id: u8,
    /// First sector of the cell, relative to the VMGM VOBS.
    pub first_sector: u32,
    /// Last sector of the cell, relative to the VMGM VOBS.
    pub last_sector: u32,
}

/// VMGM menu tables and VMGM VOBS size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmgMenus {
    /// ISO 639-1 language code for the menu language unit.
    pub language: [u8; 2],
    /// Menu PGCs in table order.
    pub pgcs: Vec<MenuPgc>,
    /// VMGM cell addresses.
    pub cells: Vec<CellAddress>,
    /// Relative start sectors of VMGM VOBUs.
    pub vobu_starts: Vec<u32>,
    /// Number of sectors occupied by VIDEO_TS.VOB.
    pub vobs_sectors: u32,
    /// The one VMGM audio stream (menus with sound), declared at VMGI_MAT 0x102 / 0x104.
    pub audio: Option<AudioAttr>,
}

/// VMGI model for M2/M3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vmgi {
    /// Titles in the title search pointer table.
    pub titles: Vec<TitleEntry>,
    /// Number of VTSs.
    pub vts_count: u8,
    /// Region mask: bit i set means region i+1 is prohibited.
    pub region_mask: u8,
    /// Provider id, written as ASCII and truncated to 32 bytes.
    pub provider: String,
    /// VMGM video attributes.
    pub video: VideoAttr,
    /// First play PGC.
    pub first_play: Pgc,
    /// One 0x300-byte VTS attribute block per VTS.
    pub vts_attrs: Vec<Vec<u8>>,
    /// Optional VMGM menu tables.
    pub menus: Option<VmgMenus>,
}

/// Return the number of sectors that `write_vmgi` will produce.
pub fn vmgi_sectors(v: &Vmgi) -> Result<usize, IfoError> {
    let data = write_vmgi(v)?;
    data.len()
        .checked_div(SECTOR)
        .ok_or_else(|| IfoError("sector count overflow".to_string()))
}

/// Write a VMGI file according to the M2/M3 layout.
pub fn write_vmgi(v: &Vmgi) -> Result<Vec<u8>, IfoError> {
    validate_vmgi(v)?;

    let mut w = Writer::new();
    write_vmgi_mat(&mut w, v)?;
    w.pad_to_sector();

    let tt_ptr = w.len() / SECTOR;
    write_tt_srpt(&mut w, &v.titles)?;
    w.pad_to_sector();

    let vts_ptr = w.len() / SECTOR;
    write_vts_atrt(&mut w, &v.vts_attrs)?;
    w.pad_to_sector();

    let mut menu_ptrs = MenuPointers::default();
    if let Some(menus) = &v.menus {
        menu_ptrs.pgci = w.len() / SECTOR;
        write_vmgm_pgci_ut(&mut w, menus)?;
        w.pad_to_sector();

        menu_ptrs.c_adt = w.len() / SECTOR;
        write_vmgm_c_adt(&mut w, &menus.cells)?;
        w.pad_to_sector();

        menu_ptrs.vobu = w.len() / SECTOR;
        write_vmgm_vobu_admap(&mut w, &menus.vobu_starts)?;
        w.pad_to_sector();
    }

    let ifo_sectors = w.len() / SECTOR;
    finish_vmgi(&mut w, v, ifo_sectors, tt_ptr, vts_ptr, menu_ptrs)?;
    Ok(w.into_bytes())
}

/// Read a VMGI file and reconstruct the M2/M3 model.
pub fn read_vmgi(data: &[u8]) -> Result<Vmgi, IfoError> {
    let r = Reader::new(data);
    if data.len() < SECTOR || !data.len().is_multiple_of(SECTOR) {
        return Err(IfoError("VMGI length must be whole sectors".to_string()));
    }

    let ifo_sectors = data.len() / SECTOR;
    let header = read_vmgi_header(&r, ifo_sectors)?;
    let pointers = read_vmgi_pointers(&r, ifo_sectors, header.last_set)?;

    let titles = read_tt_srpt(&r, pointers.tt, ifo_sectors, usize::from(header.vts_count))?;
    let tt_len = checked_mul_add(titles.len(), 12, 8)?;
    let tt_ptr = usize::try_from(pointers.tt)
        .map_err(|_| IfoError("sector pointer too large".to_string()))?;
    let vts_ptr = usize::try_from(pointers.vts)
        .map_err(|_| IfoError("sector pointer too large".to_string()))?;
    if tt_ptr != 1 {
        return Err(IfoError("bad VMGI table layout".to_string()));
    }
    let expected_vts_ptr = 1usize
        .checked_add(ceil_sectors(tt_len))
        .ok_or_else(|| IfoError("VMGI layout overflow".to_string()))?;
    if vts_ptr != expected_vts_ptr {
        return Err(IfoError("bad VMGI table layout".to_string()));
    }

    let vts_attrs = read_vts_atrt(&r, pointers.vts, ifo_sectors, usize::from(header.vts_count))?;
    let vts_len = checked_mul_add(usize::from(header.vts_count), VTS_ATR_SIZE + 4, 8)?;
    let mut menus = read_optional_menus(&r, &pointers, ifo_sectors, vts_ptr, vts_len)?;
    match (&mut menus, header.menu_audio) {
        (Some(menus), audio) => menus.audio = audio,
        (None, Some(_)) => return Err(IfoError("VMGM audio without menus".to_string())),
        (None, None) => {}
    }
    let first_play = read_fp_pgc(&r)?;

    let v = Vmgi {
        titles,
        vts_count: header.vts_count,
        region_mask: header.region_mask,
        provider: header.provider,
        video: header.video,
        first_play,
        vts_attrs,
        menus,
    };

    if ifo_sectors != vmgi_sectors(&v)? {
        return Err(IfoError("VMGI sector count mismatch".to_string()));
    }
    Ok(v)
}

#[derive(Default)]
struct MenuPointers {
    pgci: usize,
    c_adt: usize,
    vobu: usize,
}

struct VmgiHeader {
    vts_count: u8,
    region_mask: u8,
    provider: String,
    video: VideoAttr,
    last_set: u32,
    menu_audio: Option<AudioAttr>,
}

struct VmgiPointers {
    tt: u32,
    vts: u32,
    pgci: u32,
    c_adt: u32,
    vobu: u32,
    has_menus: bool,
    vobs_sectors: u32,
}

fn validate_vmgi(v: &Vmgi) -> Result<(), IfoError> {
    if v.titles.is_empty() || v.titles.len() > MAX_TITLES {
        return Err(IfoError("bad title count".to_string()));
    }
    if v.vts_count == 0 || usize::from(v.vts_count) > MAX_VTS {
        return Err(IfoError("bad VTS count".to_string()));
    }
    if v.vts_attrs.len() != usize::from(v.vts_count) {
        return Err(IfoError("VTS attribute count mismatch".to_string()));
    }
    for block in &v.vts_attrs {
        if block.len() != VTS_ATTR_BLOCK {
            return Err(IfoError("bad VTS attribute block size".to_string()));
        }
    }
    for t in &v.titles {
        if t.vts == 0 || usize::from(t.vts) > usize::from(v.vts_count) {
            return Err(IfoError("title VTS out of range".to_string()));
        }
        if t.vts_ttn == 0 {
            return Err(IfoError("title number must be non-zero".to_string()));
        }
    }

    if let Some(menus) = &v.menus {
        if menus.pgcs.is_empty() || menus.pgcs.len() > MAX_MENU_PGC {
            return Err(IfoError("bad menu PGC count".to_string()));
        }
        let title_menus = menus.pgcs.iter().filter(|m| m.title_menu).count();
        if title_menus > 1 {
            return Err(IfoError("too many title menus".to_string()));
        }
        for cell in &menus.cells {
            if cell.vob_id == 0 {
                return Err(IfoError("menu cell VOB id must be non-zero".to_string()));
            }
        }
    }

    Ok(())
}

fn write_vmgi_mat(w: &mut Writer, v: &Vmgi) -> Result<(), IfoError> {
    let pgc = write_pgc(&v.first_play)?;
    if pgc.len() > SECTOR - FP_PGC_OFFSET {
        return Err(IfoError("FP_PGC does not fit in sector 0".to_string()));
    }

    w.put_slice(0x000, b"DVDVIDEO-VMG");
    w.put_u16(0x020, 0x0011);
    w.put_u32(0x022, u32::from(v.region_mask) << 16);
    w.put_u16(0x026, 1);
    w.put_u16(0x028, 1);
    w.put_u8(0x02A, 1);
    w.put_u16(0x03E, u16::from(v.vts_count));
    write_provider(w, &v.provider);
    w.put_u32(0x084, 0x400);
    w.put_slice(0x100, &v.video.to_bytes());
    // VMGM audio: at most one stream, shared by every menu that has sound.
    match v.menus.as_ref().and_then(|menus| menus.audio.as_ref()) {
        Some(audio) => {
            w.put_u16(0x102, 1);
            w.put_slice(0x104, &audio.to_bytes());
        }
        None => w.put_u16(0x102, 0),
    }
    w.put_u16(0x154, 0);
    w.put_slice(FP_PGC_OFFSET, &pgc);

    let mat_end = FP_PGC_OFFSET
        .checked_add(pgc.len())
        .and_then(|e| e.checked_sub(1))
        .and_then(|e| u32::try_from(e).ok())
        .ok_or_else(|| IfoError("first play PGC too large".to_string()))?;
    w.put_u32(0x080, mat_end);
    Ok(())
}

fn finish_vmgi(
    w: &mut Writer,
    v: &Vmgi,
    ifo_sectors: usize,
    tt_ptr: usize,
    vts_ptr: usize,
    menu_ptrs: MenuPointers,
) -> Result<(), IfoError> {
    let last_vtsi = ifo_sectors
        .checked_sub(1)
        .ok_or_else(|| IfoError("empty VMGI".to_string()))?;
    let vobs_sectors = match &v.menus {
        Some(menus) => usize::try_from(menus.vobs_sectors)
            .map_err(|_| IfoError("VMGM VOBS size too large".to_string()))?,
        None => 0,
    };
    let base_last_set = ifo_sectors
        .checked_mul(2)
        .and_then(|x| x.checked_sub(1))
        .ok_or_else(|| IfoError("VMG set size overflow".to_string()))?;
    let last_set = base_last_set
        .checked_add(vobs_sectors)
        .ok_or_else(|| IfoError("VMG set size overflow".to_string()))?;

    w.put_u32(0x00C, u32_from_usize(last_set)?);
    w.put_u32(0x01C, u32_from_usize(last_vtsi)?);
    w.put_u32(0x0C4, u32_from_usize(tt_ptr)?);
    w.put_u32(0x0D0, u32_from_usize(vts_ptr)?);

    if v.menus.is_some() {
        w.put_u32(0x0C0, u32_from_usize(ifo_sectors)?);
        w.put_u32(0x0C8, u32_from_usize(menu_ptrs.pgci)?);
        w.put_u32(0x0D8, u32_from_usize(menu_ptrs.c_adt)?);
        w.put_u32(0x0DC, u32_from_usize(menu_ptrs.vobu)?);
        w.put_u16(0x154, 1);
    } else {
        w.put_u32(0x0C0, 0);
        w.put_u32(0x0C8, 0);
        w.put_u32(0x0D8, 0);
        w.put_u32(0x0DC, 0);
        w.put_u16(0x154, 0);
    }

    Ok(())
}

fn write_provider(w: &mut Writer, provider: &str) {
    let bytes = provider.as_bytes();
    let len = bytes.len().min(32);
    if let Some(slice) = bytes.get(..len) {
        w.put_slice(0x040, slice);
    }
}

fn write_tt_srpt(w: &mut Writer, titles: &[TitleEntry]) -> Result<(), IfoError> {
    let base = w.len();
    let n = titles.len();
    let body = checked_mul_add(n, 12, 0)?;
    let end = table_end_address(8, body)?;

    w.put_u16(
        base,
        u16::try_from(n).map_err(|_| IfoError("too many titles".to_string()))?,
    );
    w.put_u16(off(base, 2)?, 0);
    w.put_u32(off(base, 4)?, u32_from_usize(end)?);

    let mut pos = off(base, 8)?;
    for t in titles {
        w.put_u8(pos, TITLE_TYPE);
        w.put_u8(off(pos, 1)?, 1);
        w.put_u16(off(pos, 2)?, t.chapters);
        w.put_u16(off(pos, 4)?, 0);
        w.put_u8(off(pos, 6)?, t.vts);
        w.put_u8(off(pos, 7)?, t.vts_ttn);
        w.put_u32(off(pos, 8)?, t.vts_start_sector);
        pos = off(pos, 12)?;
    }
    Ok(())
}

fn write_vts_atrt(w: &mut Writer, attrs: &[Vec<u8>]) -> Result<(), IfoError> {
    let base = w.len();
    let n = attrs.len();
    let body = checked_mul_add(n, VTS_ATR_SIZE + 4, 0)?;
    let end = table_end_address(8, body)?;
    let atr_start = checked_mul_add(n, 4, 8)?;

    w.put_u16(
        base,
        u16::try_from(n).map_err(|_| IfoError("too many VTSs".to_string()))?,
    );
    w.put_u16(off(base, 2)?, 0);
    w.put_u32(off(base, 4)?, u32_from_usize(end)?);

    for (i, block) in attrs.iter().enumerate() {
        let offset = checked_mul_add(i, VTS_ATR_SIZE, atr_start)?;
        let offset_pos = off(base, checked_mul_add(i, 4, 8)?)?;
        w.put_u32(offset_pos, u32_from_usize(offset)?);

        let atr = off(base, offset)?;
        w.put_u32(atr, 0x307);
        w.put_u32(off(atr, 4)?, 0);
        w.put_slice(off(atr, 8)?, block);
    }
    Ok(())
}

fn write_vmgm_pgci_ut(w: &mut Writer, menus: &VmgMenus) -> Result<(), IfoError> {
    let base = w.len();
    let pgc_bytes = serialize_menu_pgcs(menus)?;
    let pgc_total = sum_lengths(&pgc_bytes)?;
    let entry_table_len = checked_mul_add(menus.pgcs.len(), 8, 8)?;
    let lu_len = entry_table_len
        .checked_add(pgc_total)
        .ok_or_else(|| IfoError("VMGM_PGCI_UT overflow".to_string()))?;
    let lu_end = lu_len
        .checked_sub(1)
        .ok_or_else(|| IfoError("VMGM_PGCI_UT overflow".to_string()))?;
    let table_end = table_end_address(16, lu_len)?;

    w.put_u16(base, 1);
    w.put_u16(off(base, 2)?, 0);
    w.put_u32(off(base, 4)?, u32_from_usize(table_end)?);

    write_lu_entry(w, base, menus)?;

    let lu = off(base, VMGM_LU_OFFSET)?;
    w.put_u16(
        lu,
        u16::try_from(menus.pgcs.len()).map_err(|_| IfoError("too many menu PGCs".to_string()))?,
    );
    w.put_u16(off(lu, 2)?, 0);
    w.put_u32(off(lu, 4)?, u32_from_usize(lu_end)?);

    write_pgc_entries(w, lu, menus, &pgc_bytes)?;
    write_pgc_data(w, lu, entry_table_len, &pgc_bytes)?;
    Ok(())
}

fn serialize_menu_pgcs(menus: &VmgMenus) -> Result<Vec<Vec<u8>>, IfoError> {
    let mut out = Vec::with_capacity(menus.pgcs.len());
    for menu in &menus.pgcs {
        out.push(write_pgc(&menu.pgc)?);
    }
    Ok(out)
}

fn sum_lengths(items: &[Vec<u8>]) -> Result<usize, IfoError> {
    let mut sum = 0usize;
    for item in items {
        sum = sum
            .checked_add(item.len())
            .ok_or_else(|| IfoError("VMGM_PGCI_UT overflow".to_string()))?;
    }
    Ok(sum)
}

fn menu_existence_byte(menus: &VmgMenus) -> u8 {
    if menus.pgcs.iter().any(|m| m.title_menu) {
        MENU_EXISTENCE
    } else {
        0
    }
}

fn write_lu_entry(w: &mut Writer, base: usize, menus: &VmgMenus) -> Result<(), IfoError> {
    w.put_u16(off(base, 8)?, u16::from_be_bytes(menus.language));
    w.put_u8(off(base, 10)?, 0);
    w.put_u8(off(base, 11)?, menu_existence_byte(menus));
    w.put_u32(off(base, 12)?, u32_from_usize(VMGM_LU_OFFSET)?);
    Ok(())
}

fn write_pgc_entries(
    w: &mut Writer,
    lu: usize,
    menus: &VmgMenus,
    pgc_bytes: &[Vec<u8>],
) -> Result<(), IfoError> {
    let mut pgc_off = checked_mul_add(menus.pgcs.len(), 8, 8)?;
    for (i, (menu, bytes)) in menus.pgcs.iter().zip(pgc_bytes.iter()).enumerate() {
        let entry = off(lu, checked_mul_add(i, 8, 8)?)?;
        let category = if menu.title_menu {
            TITLE_MENU_CATEGORY
        } else {
            0
        };
        w.put_u32(entry, category);
        w.put_u32(off(entry, 4)?, u32_from_usize(pgc_off)?);
        pgc_off = off(pgc_off, bytes.len())?;
    }
    Ok(())
}

fn write_pgc_data(
    w: &mut Writer,
    lu: usize,
    entry_table_len: usize,
    pgc_bytes: &[Vec<u8>],
) -> Result<(), IfoError> {
    let mut pos = off(lu, entry_table_len)?;
    for bytes in pgc_bytes {
        w.put_slice(pos, bytes);
        pos = off(pos, bytes.len())?;
    }
    Ok(())
}

fn write_vmgm_c_adt(w: &mut Writer, cells: &[CellAddress]) -> Result<(), IfoError> {
    let base = w.len();
    let sorted = sorted_cells(cells);
    let n = sorted.len();
    let n_vob_ids = distinct_vob_ids(&sorted)?;
    let body = checked_mul_add(n, 12, 0)?;
    let end = table_end_address(8, body)?;

    w.put_u16(
        base,
        u16::try_from(n_vob_ids).map_err(|_| IfoError("too many VOB ids".to_string()))?,
    );
    w.put_u16(off(base, 2)?, 0);
    w.put_u32(off(base, 4)?, u32_from_usize(end)?);

    let mut pos = off(base, 8)?;
    for cell in &sorted {
        w.put_u16(pos, cell.vob_id);
        // C_ADT entry: u16 VOB id, u8 cell id, u8 reserved, u32 first sector, u32 last sector.
        w.put_u8(off(pos, 2)?, cell.cell_id);
        w.put_u8(off(pos, 3)?, 0);
        w.put_u32(off(pos, 4)?, cell.first_sector);
        w.put_u32(off(pos, 8)?, cell.last_sector);
        pos = off(pos, 12)?;
    }
    Ok(())
}

fn sorted_cells(cells: &[CellAddress]) -> Vec<CellAddress> {
    let mut sorted = cells.to_vec();
    sorted.sort_by(|a, b| a.vob_id.cmp(&b.vob_id).then(a.cell_id.cmp(&b.cell_id)));
    sorted
}

fn distinct_vob_ids(cells: &[CellAddress]) -> Result<usize, IfoError> {
    let mut count = 0usize;
    let mut prev: Option<u16> = None;
    for cell in cells {
        if prev != Some(cell.vob_id) {
            count = count
                .checked_add(1)
                .ok_or_else(|| IfoError("VOB id count overflow".to_string()))?;
            prev = Some(cell.vob_id);
        }
    }
    Ok(count)
}

fn write_vmgm_vobu_admap(w: &mut Writer, starts: &[u32]) -> Result<(), IfoError> {
    let base = w.len();
    let body = checked_mul_add(starts.len(), 4, 0)?;
    let end = table_end_address(4, body)?;

    w.put_u32(base, u32_from_usize(end)?);
    let mut pos = off(base, 4)?;
    for &start in starts {
        w.put_u32(pos, start);
        pos = off(pos, 4)?;
    }
    Ok(())
}

fn read_vmgi_header(r: &Reader<'_>, ifo_sectors: usize) -> Result<VmgiHeader, IfoError> {
    if r.slice(0, 12)? != b"DVDVIDEO-VMG" {
        return Err(IfoError("bad VMGI magic".to_string()));
    }
    if r.u16(0x020)? != 0x0011 {
        return Err(IfoError("bad VMGI version".to_string()));
    }

    let expected_last_vtsi = u32_from_usize(
        ifo_sectors
            .checked_sub(1)
            .ok_or_else(|| IfoError("empty VMGI".to_string()))?,
    )?;
    if r.u32(0x01C)? != expected_last_vtsi {
        return Err(IfoError("bad VMGI last sector".to_string()));
    }

    let last_set = r.u32(0x00C)?;
    let category = r.u32(0x022)?;
    if category & !0x0000_ff00u32 != 0 {
        return Err(IfoError("bad VMG category".to_string()));
    }
    let region_mask = u8::try_from((category >> 16) & 0xff)
        .map_err(|_| IfoError("bad region mask".to_string()))?;

    if r.u16(0x026)? != 1 || r.u16(0x028)? != 1 || r.u8(0x02A)? != 1 {
        return Err(IfoError("bad volume fields".to_string()));
    }

    let vts_count_u16 = r.u16(0x03E)?;
    if vts_count_u16 == 0 || usize::from(vts_count_u16) > MAX_VTS {
        return Err(IfoError("bad VTS count".to_string()));
    }
    let vts_count =
        u8::try_from(vts_count_u16).map_err(|_| IfoError("bad VTS count".to_string()))?;

    let provider = read_provider(r)?;
    if r.u32(0x080)? < 0x400 || r.u32(0x084)? != 0x400 {
        return Err(IfoError("bad VMGI_MAT addresses".to_string()));
    }

    let video = VideoAttr::from_bytes(read_array2(r, 0x100)?)?;
    let menu_audio = match r.u16(0x102)? {
        0 => None,
        1 => {
            let bytes = r.slice(0x104, 8)?;
            let bytes = <[u8; 8]>::try_from(bytes)
                .map_err(|_| IfoError("bad VMGM audio attributes".to_string()))?;
            Some(AudioAttr::from_bytes(bytes)?)
        }
        _ => return Err(IfoError("bad VMGM stream counts".to_string())),
    };

    Ok(VmgiHeader {
        vts_count,
        region_mask,
        provider,
        video,
        last_set,
        menu_audio,
    })
}

fn read_vmgi_pointers(
    r: &Reader<'_>,
    ifo_sectors: usize,
    last_set: u32,
) -> Result<VmgiPointers, IfoError> {
    let vobs = r.u32(0x0C0)?;
    let tt = r.u32(0x0C4)?;
    let pgci = r.u32(0x0C8)?;
    let vts = r.u32(0x0D0)?;
    let c_adt = r.u32(0x0D8)?;
    let vobu = r.u32(0x0DC)?;

    if r.u32(0x0CC)? != 0 || r.u32(0x0D4)? != 0 {
        return Err(IfoError("unexpected VMGM table pointer".to_string()));
    }

    let has_menus = pgci != 0 || c_adt != 0 || vobu != 0;
    if has_menus {
        if pgci == 0 || c_adt == 0 || vobu == 0 {
            return Err(IfoError("incomplete VMGM menu pointers".to_string()));
        }
        if vobs != u32_from_usize(ifo_sectors)? {
            return Err(IfoError("bad VMGM VOBS start sector".to_string()));
        }
        if r.u16(0x154)? != 1 {
            return Err(IfoError("bad VMGM subpicture count".to_string()));
        }
    } else {
        if vobs != 0 {
            return Err(IfoError("unexpected VMGM VOBS start sector".to_string()));
        }
        if r.u16(0x154)? != 0 {
            return Err(IfoError("bad VMGM subpicture count".to_string()));
        }
    }

    read_zero_bytes(r, 0x156, 6)?;

    let expected_base = (ifo_sectors as u64)
        .checked_mul(2)
        .and_then(|x| x.checked_sub(1))
        .ok_or_else(|| IfoError("VMG set size overflow".to_string()))?;
    let last_set_u64 = u64::from(last_set);

    if !has_menus && last_set_u64 != expected_base {
        return Err(IfoError("bad VMG set last sector".to_string()));
    }
    if has_menus && last_set_u64 < expected_base {
        return Err(IfoError("bad VMG set last sector".to_string()));
    }

    let vobs_sectors = if has_menus {
        u32::try_from(
            last_set_u64
                .checked_sub(expected_base)
                .ok_or_else(|| IfoError("VMG set size overflow".to_string()))?,
        )
        .map_err(|_| IfoError("VMGM VOBS size too large".to_string()))?
    } else {
        0
    };

    Ok(VmgiPointers {
        tt,
        vts,
        pgci,
        c_adt,
        vobu,
        has_menus,
        vobs_sectors,
    })
}

fn read_optional_menus(
    r: &Reader<'_>,
    pointers: &VmgiPointers,
    ifo_sectors: usize,
    vts_ptr: usize,
    vts_len: usize,
) -> Result<Option<VmgMenus>, IfoError> {
    if !pointers.has_menus {
        return Ok(None);
    }

    let expected_pgci = vts_ptr
        .checked_add(ceil_sectors(vts_len))
        .ok_or_else(|| IfoError("VMGI layout overflow".to_string()))?;
    let pgci_base = table_start_exact(r, pointers.pgci, ifo_sectors, expected_pgci)?;
    let (language, pgcs, pgci_len) = read_vmgm_pgci_ut(r, pgci_base)?;

    let pgci_ptr = usize::try_from(pointers.pgci)
        .map_err(|_| IfoError("sector pointer too large".to_string()))?;
    let expected_c_adt = pgci_ptr
        .checked_add(ceil_sectors(pgci_len))
        .ok_or_else(|| IfoError("VMGI layout overflow".to_string()))?;
    let c_adt_base = table_start_exact(r, pointers.c_adt, ifo_sectors, expected_c_adt)?;
    let (cells, c_adt_len) = read_vmgm_c_adt(r, c_adt_base)?;

    let c_adt_ptr = usize::try_from(pointers.c_adt)
        .map_err(|_| IfoError("sector pointer too large".to_string()))?;
    let expected_vobu = c_adt_ptr
        .checked_add(ceil_sectors(c_adt_len))
        .ok_or_else(|| IfoError("VMGI layout overflow".to_string()))?;
    let vobu_base = table_start_exact(r, pointers.vobu, ifo_sectors, expected_vobu)?;
    let (vobu_starts, vobu_len) = read_vmgm_vobu_admap(r, vobu_base)?;

    let vobu_ptr = usize::try_from(pointers.vobu)
        .map_err(|_| IfoError("sector pointer too large".to_string()))?;
    let expected_ifo = vobu_ptr
        .checked_add(ceil_sectors(vobu_len))
        .ok_or_else(|| IfoError("VMGI layout overflow".to_string()))?;
    if expected_ifo != ifo_sectors {
        return Err(IfoError("bad VMGM menu table layout".to_string()));
    }

    // The audio attributes live in the VMGI_MAT; `read_vmgi` fills them in.
    Ok(Some(VmgMenus {
        audio: None,
        language,
        pgcs,
        cells,
        vobu_starts,
        vobs_sectors: pointers.vobs_sectors,
    }))
}

fn read_vmgm_pgci_ut(
    r: &Reader<'_>,
    base: usize,
) -> Result<([u8; 2], Vec<MenuPgc>, usize), IfoError> {
    let table_len = read_pgci_table_header(r, base)?;
    let (language, existence, lu_offset) = read_pgci_lu_entry(r, base)?;
    if lu_offset != VMGM_LU_OFFSET {
        return Err(IfoError("bad VMGM_PGCI_UT LU offset".to_string()));
    }

    let lu = off(base, lu_offset)?;
    let (n_pgcs, lu_len) = read_pgci_lu_header(r, lu)?;
    let expected_table_len = 16usize
        .checked_add(lu_len)
        .ok_or_else(|| IfoError("VMGM_PGCI_UT size overflow".to_string()))?;
    if table_len != expected_table_len {
        return Err(IfoError("bad VMGM_PGCI_UT end address".to_string()));
    }

    let entries = read_pgci_entries(r, lu, n_pgcs, lu_len, existence)?;
    let pgcs = read_pgci_pgcs(r, lu, lu_len, &entries)?;
    Ok((language, pgcs, table_len))
}

fn read_pgci_table_header(r: &Reader<'_>, base: usize) -> Result<usize, IfoError> {
    if usize::from(r.u16(base)?) != 1 {
        return Err(IfoError("bad VMGM_PGCI_UT language unit count".to_string()));
    }
    if r.u16(off(base, 2)?)? != 0 {
        return Err(IfoError("bad VMGM_PGCI_UT reserved field".to_string()));
    }
    read_table_len(r, off(base, 4)?, base)
}

fn read_pgci_lu_entry(r: &Reader<'_>, base: usize) -> Result<([u8; 2], u8, usize), IfoError> {
    let language = read_array2(r, off(base, 8)?)?;
    if r.u8(off(base, 10)?)? != 0 {
        return Err(IfoError("bad VMGM_PGCI_UT reserved byte".to_string()));
    }
    let existence = r.u8(off(base, 11)?)?;
    if existence != 0 && existence != MENU_EXISTENCE {
        return Err(IfoError("bad VMGM_PGCI_UT menu existence".to_string()));
    }
    let lu_offset = usize::try_from(r.u32(off(base, 12)?)?)
        .map_err(|_| IfoError("VMGM_PGCI_UT offset too large".to_string()))?;
    Ok((language, existence, lu_offset))
}

fn read_pgci_lu_header(r: &Reader<'_>, lu: usize) -> Result<(usize, usize), IfoError> {
    let n = usize::from(r.u16(lu)?);
    if n == 0 || n > MAX_MENU_PGC {
        return Err(IfoError("bad VMGM menu PGC count".to_string()));
    }
    if r.u16(off(lu, 2)?)? != 0 {
        return Err(IfoError("bad VMGM_PGCI_UT LU reserved field".to_string()));
    }
    let lu_len = read_table_len(r, off(lu, 4)?, lu)?;
    Ok((n, lu_len))
}

fn read_pgci_entries(
    r: &Reader<'_>,
    lu: usize,
    n: usize,
    lu_len: usize,
    existence: u8,
) -> Result<Vec<(bool, usize)>, IfoError> {
    let entry_table_end = checked_mul_add(n, 8, 8)?;
    if lu_len < entry_table_end {
        return Err(IfoError("bad VMGM_PGCI_UT LU length".to_string()));
    }

    let mut entries = Vec::with_capacity(n);
    let mut title_count = 0usize;
    for i in 0..n {
        let entry = off(lu, checked_mul_add(i, 8, 8)?)?;
        let category = r.u32(entry)?;
        if category != 0 && category != TITLE_MENU_CATEGORY {
            return Err(IfoError("bad VMGM menu PGC category".to_string()));
        }
        let is_title = category == TITLE_MENU_CATEGORY;
        if is_title {
            title_count = title_count
                .checked_add(1)
                .ok_or_else(|| IfoError("title menu count overflow".to_string()))?;
        }

        let offset = usize::try_from(r.u32(off(entry, 4)?)?)
            .map_err(|_| IfoError("VMGM menu PGC offset too large".to_string()))?;
        if offset < entry_table_end || offset >= lu_len {
            return Err(IfoError("bad VMGM menu PGC offset".to_string()));
        }
        entries.push((is_title, offset));
    }

    if title_count > 1 {
        return Err(IfoError("too many title menus".to_string()));
    }
    let expected_existence = if title_count > 0 { MENU_EXISTENCE } else { 0 };
    if existence != expected_existence {
        return Err(IfoError("bad VMGM_PGCI_UT menu existence".to_string()));
    }

    Ok(entries)
}

fn read_pgci_pgcs(
    r: &Reader<'_>,
    lu: usize,
    lu_len: usize,
    entries: &[(bool, usize)],
) -> Result<Vec<MenuPgc>, IfoError> {
    let entry_table_end = checked_mul_add(entries.len(), 8, 8)?;
    let mut pgcs = Vec::with_capacity(entries.len());
    let mut iter = entries.iter().peekable();
    let mut first = true;
    let mut prev_offset = 0usize;

    while let Some((is_title, offset)) = iter.next() {
        if first {
            if *offset != entry_table_end {
                return Err(IfoError("bad VMGM menu PGC first offset".to_string()));
            }
            first = false;
        } else if *offset <= prev_offset {
            return Err(IfoError("bad VMGM menu PGC offset order".to_string()));
        }

        let end = if let Some((_, next)) = iter.peek() {
            *next
        } else {
            lu_len
        };
        if *offset >= end {
            return Err(IfoError("bad VMGM menu PGC length".to_string()));
        }

        let len = end
            .checked_sub(*offset)
            .ok_or_else(|| IfoError("bad VMGM menu PGC length".to_string()))?;
        let slice = r.slice(off(lu, *offset)?, len)?;
        let pgc = read_pgc(slice)?;
        pgcs.push(MenuPgc {
            title_menu: *is_title,
            pgc,
        });
        prev_offset = *offset;
    }

    Ok(pgcs)
}

fn read_vmgm_c_adt(r: &Reader<'_>, base: usize) -> Result<(Vec<CellAddress>, usize), IfoError> {
    let n_vob_ids = usize::from(r.u16(base)?);
    if r.u16(off(base, 2)?)? != 0 {
        return Err(IfoError("bad VMGM_C_ADT reserved field".to_string()));
    }

    let table_len = read_table_len(r, off(base, 4)?, base)?;
    let body = table_len
        .checked_sub(8)
        .ok_or_else(|| IfoError("bad VMGM_C_ADT length".to_string()))?;
    if body % 12 != 0 {
        return Err(IfoError("bad VMGM_C_ADT entry count".to_string()));
    }

    let n_cells = body / 12;
    let mut cells = Vec::with_capacity(n_cells);
    let mut pos = off(base, 8)?;
    let mut prev_vob: Option<u16> = None;
    let mut prev_cell: Option<u8> = None;
    let mut distinct = 0usize;

    for _ in 0..n_cells {
        let cell = read_cell_address(r, pos)?;
        if cell.vob_id == 0 {
            return Err(IfoError("menu cell VOB id must be non-zero".to_string()));
        }
        update_sorted_cell(&mut prev_vob, &mut prev_cell, &mut distinct, cell)?;
        cells.push(cell);
        pos = off(pos, 12)?;
    }

    if n_vob_ids != distinct {
        return Err(IfoError("bad VMGM_C_ADT VOB id count".to_string()));
    }
    Ok((cells, table_len))
}

fn read_cell_address(r: &Reader<'_>, pos: usize) -> Result<CellAddress, IfoError> {
    let vob_id = r.u16(pos)?;
    let cell_id = r.u8(off(pos, 2)?)?;
    if r.u8(off(pos, 3)?)? != 0 {
        return Err(IfoError("bad VMGM_C_ADT reserved byte".to_string()));
    }
    let first_sector = r.u32(off(pos, 4)?)?;
    let last_sector = r.u32(off(pos, 8)?)?;
    Ok(CellAddress {
        vob_id,
        cell_id,
        first_sector,
        last_sector,
    })
}

fn update_sorted_cell(
    prev_vob: &mut Option<u16>,
    prev_cell: &mut Option<u8>,
    distinct: &mut usize,
    cell: CellAddress,
) -> Result<(), IfoError> {
    match prev_vob {
        None => {
            *distinct = distinct
                .checked_add(1)
                .ok_or_else(|| IfoError("VOB id count overflow".to_string()))?;
        }
        Some(pv) => {
            if cell.vob_id < *pv {
                return Err(IfoError("VMGM_C_ADT cells not sorted".to_string()));
            }
            if cell.vob_id == *pv {
                if let Some(pc) = prev_cell
                    && cell.cell_id <= *pc
                {
                    return Err(IfoError("VMGM_C_ADT cells not sorted".to_string()));
                }
            } else {
                *distinct = distinct
                    .checked_add(1)
                    .ok_or_else(|| IfoError("VOB id count overflow".to_string()))?;
            }
        }
    }
    *prev_vob = Some(cell.vob_id);
    *prev_cell = Some(cell.cell_id);
    Ok(())
}

fn read_vmgm_vobu_admap(r: &Reader<'_>, base: usize) -> Result<(Vec<u32>, usize), IfoError> {
    let table_len = read_table_len(r, base, base)?;
    let body = table_len
        .checked_sub(4)
        .ok_or_else(|| IfoError("bad VMGM_VOBU_ADMAP length".to_string()))?;
    if body % 4 != 0 {
        return Err(IfoError("bad VMGM_VOBU_ADMAP entry count".to_string()));
    }

    let n = body / 4;
    let mut starts = Vec::with_capacity(n);
    let mut pos = off(base, 4)?;
    for _ in 0..n {
        starts.push(r.u32(pos)?);
        pos = off(pos, 4)?;
    }
    Ok((starts, table_len))
}

fn read_table_len(r: &Reader<'_>, end_offset: usize, base: usize) -> Result<usize, IfoError> {
    let end = r.u32(end_offset)?;
    let len = usize::try_from(
        end.checked_add(1)
            .ok_or_else(|| IfoError("table length overflow".to_string()))?,
    )
    .map_err(|_| IfoError("table length too large".to_string()))?;
    if off(base, len)? > r.len() {
        return Err(IfoError("table out of bounds".to_string()));
    }
    Ok(len)
}

fn read_zero_bytes(r: &Reader<'_>, offset: usize, len: usize) -> Result<(), IfoError> {
    for i in 0..len {
        if r.u8(off(offset, i)?)? != 0 {
            return Err(IfoError("non-zero reserved bytes".to_string()));
        }
    }
    Ok(())
}

fn read_tt_srpt(
    r: &Reader<'_>,
    ptr: u32,
    ifo_sectors: usize,
    vts_count: usize,
) -> Result<Vec<TitleEntry>, IfoError> {
    let base = table_start(r, ptr, ifo_sectors, 1)?;
    let n = usize::from(r.u16(base)?);
    if n == 0 || n > MAX_TITLES {
        return Err(IfoError("bad title count".to_string()));
    }
    if r.u16(off(base, 2)?)? != 0 {
        return Err(IfoError("bad TT_SRPT reserved field".to_string()));
    }

    let body = checked_mul_add(n, 12, 0)?;
    let expected_end = table_end_address(8, body)?;
    if r.u32(off(base, 4)?)? != u32_from_usize(expected_end)? {
        return Err(IfoError("bad TT_SRPT end address".to_string()));
    }

    let table_len = expected_end
        .checked_add(1)
        .ok_or_else(|| IfoError("TT_SRPT length overflow".to_string()))?;
    if off(base, table_len)? > r.len() {
        return Err(IfoError("TT_SRPT out of bounds".to_string()));
    }

    let mut titles = Vec::with_capacity(n);
    let mut pos = off(base, 8)?;
    for _ in 0..n {
        if r.u8(pos)? != TITLE_TYPE {
            return Err(IfoError("bad title type".to_string()));
        }
        if r.u8(off(pos, 1)?)? != 1 {
            return Err(IfoError("bad angle count".to_string()));
        }
        let chapters = r.u16(off(pos, 2)?)?;
        if r.u16(off(pos, 4)?)? != 0 {
            return Err(IfoError("bad parental mask".to_string()));
        }
        let vts = r.u8(off(pos, 6)?)?;
        let vts_ttn = r.u8(off(pos, 7)?)?;
        let vts_start_sector = r.u32(off(pos, 8)?)?;

        if vts == 0 || usize::from(vts) > vts_count || vts_ttn == 0 {
            return Err(IfoError("bad title identifiers".to_string()));
        }

        titles.push(TitleEntry {
            chapters,
            vts,
            vts_ttn,
            vts_start_sector,
        });
        pos = off(pos, 12)?;
    }
    Ok(titles)
}

fn read_vts_atrt(
    r: &Reader<'_>,
    ptr: u32,
    ifo_sectors: usize,
    expected_count: usize,
) -> Result<Vec<Vec<u8>>, IfoError> {
    let base = table_start(r, ptr, ifo_sectors, 1)?;
    let n = usize::from(r.u16(base)?);
    if n == 0 || n > MAX_VTS || n != expected_count {
        return Err(IfoError("bad VTS_ATRT count".to_string()));
    }
    if r.u16(off(base, 2)?)? != 0 {
        return Err(IfoError("bad VTS_ATRT reserved field".to_string()));
    }

    let body = checked_mul_add(n, VTS_ATR_SIZE + 4, 0)?;
    let expected_end = table_end_address(8, body)?;
    if r.u32(off(base, 4)?)? != u32_from_usize(expected_end)? {
        return Err(IfoError("bad VTS_ATRT end address".to_string()));
    }

    let table_len = expected_end
        .checked_add(1)
        .ok_or_else(|| IfoError("VTS_ATRT length overflow".to_string()))?;
    if off(base, table_len)? > r.len() {
        return Err(IfoError("VTS_ATRT out of bounds".to_string()));
    }

    let atr_start = checked_mul_add(n, 4, 8)?;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let offset = checked_mul_add(i, VTS_ATR_SIZE, atr_start)?;
        let offset_pos = off(base, checked_mul_add(i, 4, 8)?)?;
        let got = usize::try_from(r.u32(offset_pos)?)
            .map_err(|_| IfoError("VTS_ATRT offset too large".to_string()))?;
        if got != offset {
            return Err(IfoError("bad VTS_ATRT offset".to_string()));
        }

        let atr = off(base, offset)?;
        if r.u32(atr)? != 0x307 {
            return Err(IfoError("bad VTS_ATR end address".to_string()));
        }
        if r.u32(off(atr, 4)?)? != 0 {
            return Err(IfoError("bad VTS category".to_string()));
        }

        let block = r.slice(off(atr, 8)?, VTS_ATTR_BLOCK)?.to_vec();
        out.push(block);
    }
    Ok(out)
}

fn read_fp_pgc(r: &Reader<'_>) -> Result<Pgc, IfoError> {
    let s = r.slice(FP_PGC_OFFSET, SECTOR - FP_PGC_OFFSET)?;
    read_pgc(s)
}

fn read_provider(r: &Reader<'_>) -> Result<String, IfoError> {
    let s = r.slice(0x040, 32)?;
    let mut end = s.len();
    while end > 0 {
        match s.get(end - 1) {
            Some(0) => end -= 1,
            _ => break,
        }
    }
    let trimmed = s.get(..end).unwrap_or(s);
    Ok(String::from_utf8_lossy(trimmed).into_owned())
}

fn read_array2(r: &Reader<'_>, offset: usize) -> Result<[u8; 2], IfoError> {
    let s = r.slice(offset, 2)?;
    let mut a = [0u8; 2];
    for (i, slot) in a.iter_mut().enumerate() {
        if let Some(b) = s.get(i) {
            *slot = *b;
        }
    }
    Ok(a)
}

fn table_start(
    r: &Reader<'_>,
    ptr: u32,
    ifo_sectors: usize,
    min_sector: usize,
) -> Result<usize, IfoError> {
    let ptr_usize =
        usize::try_from(ptr).map_err(|_| IfoError("sector pointer too large".to_string()))?;
    if ptr_usize < min_sector || ptr_usize >= ifo_sectors {
        return Err(IfoError("sector pointer outside VMGI".to_string()));
    }
    let start = ptr_usize
        .checked_mul(SECTOR)
        .ok_or_else(|| IfoError("sector offset overflow".to_string()))?;
    if start >= r.len() {
        return Err(IfoError("sector pointer outside file".to_string()));
    }
    Ok(start)
}

fn table_start_exact(
    r: &Reader<'_>,
    ptr: u32,
    ifo_sectors: usize,
    expected: usize,
) -> Result<usize, IfoError> {
    let ptr_usize =
        usize::try_from(ptr).map_err(|_| IfoError("sector pointer too large".to_string()))?;
    if ptr_usize != expected || ptr_usize >= ifo_sectors {
        return Err(IfoError("sector pointer outside VMGI".to_string()));
    }
    let start = ptr_usize
        .checked_mul(SECTOR)
        .ok_or_else(|| IfoError("sector offset overflow".to_string()))?;
    if start >= r.len() {
        return Err(IfoError("sector pointer outside file".to_string()));
    }
    Ok(start)
}

fn table_end_address(header: usize, body: usize) -> Result<usize, IfoError> {
    header
        .checked_add(body)
        .and_then(|x| x.checked_sub(1))
        .ok_or_else(|| IfoError("table length overflow".to_string()))
}

fn checked_mul_add(a: usize, b: usize, c: usize) -> Result<usize, IfoError> {
    a.checked_mul(b)
        .and_then(|x| x.checked_add(c))
        .ok_or_else(|| IfoError("size overflow".to_string()))
}

fn off(base: usize, delta: usize) -> Result<usize, IfoError> {
    base.checked_add(delta)
        .ok_or_else(|| IfoError("offset overflow".to_string()))
}

fn ceil_sectors(len: usize) -> usize {
    let full = len / SECTOR;
    if len.is_multiple_of(SECTOR) {
        full
    } else {
        full.saturating_add(1)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CellAddress, MenuPgc, TitleEntry, VTS_ATTR_BLOCK, VmgMenus, Vmgi, read_vmgi, vmgi_sectors,
        write_vmgi,
    };
    use crate::IfoError;
    use crate::attr::{AudioAttr, AudioCoding, BcdTime, Standard, VideoAttr, VmCommand};
    use crate::bytes::{Reader, SECTOR};
    use crate::pgc::{Cell, Pgc};

    fn sample_vmgi() -> Vmgi {
        let mut block_a = vec![0u8; VTS_ATTR_BLOCK];
        let mut block_b = vec![0u8; VTS_ATTR_BLOCK];
        if let Some(b) = block_a.get_mut(0) {
            *b = 0x40;
        }
        if let Some(b) = block_b.get_mut(0) {
            *b = 0x50;
        }

        Vmgi {
            titles: vec![
                TitleEntry {
                    chapters: 3,
                    vts: 1,
                    vts_ttn: 1,
                    vts_start_sector: 100,
                },
                TitleEntry {
                    chapters: 2,
                    vts: 2,
                    vts_ttn: 1,
                    vts_start_sector: 200,
                },
            ],
            vts_count: 2,
            region_mask: 0,
            provider: "DiscCraft".to_string(),
            video: VideoAttr {
                standard: Standard::Ntsc,
                wide: false,
            },
            first_play: Pgc {
                prohibited_ops: 0,
                pre: vec![VmCommand::jump_tt(1)],
                ..Pgc::default()
            },
            vts_attrs: vec![block_a, block_b],
            menus: None,
        }
    }

    fn menu_pgc(vob_id: u16, cell_id: u8, first: u32, last: u32) -> Pgc {
        Pgc {
            prohibited_ops: 0,
            subpicture_streams: 1,
            still_time: 0xFF,
            program_entry_cells: vec![1],
            cells: vec![Cell {
                vob_id,
                cell_id,
                stc_discontinuity: false,
                playback_time: BcdTime([0, 0, 0, 0]),
                first_sector: first,
                last_vobu_start: first,
                last_sector: last,
                still_time: 0xFF,
            }],
            post: vec![VmCommand::exit()],
            ..Pgc::default()
        }
    }

    fn hidden_pgc() -> Pgc {
        Pgc {
            prohibited_ops: 0,
            pre: vec![VmCommand::jump_tt(1)],
            ..Pgc::default()
        }
    }

    fn sample_menus() -> VmgMenus {
        VmgMenus {
            language: *b"en",
            pgcs: vec![
                MenuPgc {
                    title_menu: true,
                    pgc: menu_pgc(1, 1, 0, 9),
                },
                MenuPgc {
                    title_menu: false,
                    pgc: menu_pgc(2, 1, 10, 19),
                },
                MenuPgc {
                    title_menu: false,
                    pgc: hidden_pgc(),
                },
            ],
            cells: vec![
                CellAddress {
                    vob_id: 1,
                    cell_id: 1,
                    first_sector: 0,
                    last_sector: 9,
                },
                CellAddress {
                    vob_id: 2,
                    cell_id: 1,
                    first_sector: 10,
                    last_sector: 19,
                },
            ],
            vobu_starts: vec![0, 10],
            vobs_sectors: 20,
            audio: None,
        }
    }

    #[test]
    fn menu_audio_round_trips() -> Result<(), IfoError> {
        let mut v = sample_vmgi_with_menus();
        if let Some(menus) = v.menus.as_mut() {
            menus.audio = Some(AudioAttr {
                coding: AudioCoding::Lpcm,
                channels: 2,
                language: None,
            });
        }
        let data = write_vmgi(&v)?;
        assert_eq!(data.get(0x102..0x104), Some(&[0u8, 1][..]));
        assert_eq!(read_vmgi(&data)?, v);
        Ok(())
    }

    fn sample_vmgi_with_menus() -> Vmgi {
        let mut v = sample_vmgi();
        v.menus = Some(sample_menus());
        v
    }

    #[test]
    fn round_trip_two_titles() -> Result<(), IfoError> {
        let v = sample_vmgi();
        let data = write_vmgi(&v)?;
        let back = read_vmgi(&data)?;
        assert_eq!(back, v);
        Ok(())
    }

    #[test]
    fn round_trip_with_menus() -> Result<(), IfoError> {
        let v = sample_vmgi_with_menus();
        let data = write_vmgi(&v)?;
        let back = read_vmgi(&data)?;
        assert_eq!(back, v);
        Ok(())
    }

    #[test]
    fn sectors_match_written_length() -> Result<(), IfoError> {
        let v = sample_vmgi();
        let data = write_vmgi(&v)?;
        let sectors = vmgi_sectors(&v)?;
        let expected = sectors
            .checked_mul(SECTOR)
            .ok_or_else(|| IfoError("test overflow".to_string()))?;
        assert_eq!(data.len(), expected);
        Ok(())
    }

    #[test]
    fn menu_sector_and_category() -> Result<(), IfoError> {
        let v = sample_vmgi_with_menus();
        let sectors = vmgi_sectors(&v)?;
        let data = write_vmgi(&v)?;
        let expected = sectors
            .checked_mul(SECTOR)
            .ok_or_else(|| IfoError("test overflow".to_string()))?;
        assert_eq!(data.len(), expected);

        let r = Reader::new(&data);
        let c0 = r.u32(0x0C0)?;
        assert_eq!(
            c0,
            u32::try_from(sectors).map_err(|_| IfoError("test overflow".to_string()))?
        );

        let pgci_ptr = r.u32(0x0C8)?;
        let base = usize::try_from(pgci_ptr)
            .map_err(|_| IfoError("test overflow".to_string()))?
            .checked_mul(SECTOR)
            .ok_or_else(|| IfoError("test overflow".to_string()))?;
        assert_eq!(data.get(base + 24), Some(&0x82));
        Ok(())
    }

    #[test]
    fn region_mask_and_magic() -> Result<(), IfoError> {
        let mut v = sample_vmgi();
        v.region_mask = 0x05;
        let data = write_vmgi(&v)?;
        assert_eq!(data.get(0x23), Some(&0x05));
        assert_eq!(data.get(0..12), Some(&b"DVDVIDEO-VMG"[..]));
        Ok(())
    }

    #[test]
    fn hostile_prefixes_and_corruptions() -> Result<(), IfoError> {
        let v = sample_vmgi();
        let data = write_vmgi(&v)?;
        hostile_read(&data)
    }

    #[test]
    fn hostile_prefixes_and_corruptions_with_menus() -> Result<(), IfoError> {
        let v = sample_vmgi_with_menus();
        let data = write_vmgi(&v)?;
        hostile_read(&data)
    }

    /// Every prefix (step 97) and 200 deterministic single-byte corruptions must not panic.
    fn hostile_read(data: &[u8]) -> Result<(), IfoError> {
        let mut end = 0;
        while end < data.len() {
            let _ = read_vmgi(
                data.get(..end)
                    .ok_or_else(|| IfoError("prefix".to_string()))?,
            );
            end += 97;
        }
        let mut state = 0x2545_F491u32;
        for _ in 0..200 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let pos = (state as usize) % data.len().max(1);
            let mut copy = data.to_vec();
            if let Some(b) = copy.get_mut(pos) {
                *b ^= 0xFF;
            }
            let _ = read_vmgi(&copy);
        }
        Ok(())
    }
}
