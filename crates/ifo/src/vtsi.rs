use crate::IfoError;
use crate::attr::{AudioAttr, VideoAttr};
use crate::bytes::{Reader, SECTOR, Writer, u32_from_usize};
use crate::pgc::{self, Cell, Pgc};
use crate::vtsm::{VtsRootMenu, read_vtsm_pgci_ut, write_vtsm_pgci_ut};

const VTSI_MAT_END: usize = 0x400;
/// The most titles (entry PGCs) in one title set.
const MAX_VTS_TITLES: usize = 99;

/// VTS information for one title set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vtsi {
    /// Title video attributes.
    pub video: VideoAttr,
    /// Title audio attributes.
    pub audio: Vec<AudioAttr>,
    /// The title PGCs: PGC `n` is the entry PGC of VTS title `n` (VTS_TTN). The first is the master timeline; the
    /// others are chapter playlists that list the same cells in another order.
    pub pgcs: Vec<Pgc>,
    /// Sector count of the title VOBS.
    pub title_vobs_sectors: u32,
    /// One VTS_TMAP per PGC, in PGC order.
    pub tmaps: Vec<TimeMap>,
    /// Relative start sector of every VOBU in the title VOBS.
    pub vobu_starts: Vec<u32>,
    /// Subpicture (subtitle) streams: optional ISO 639-1 language each (max 32).
    pub subpictures: Vec<Option<[u8; 2]>>,
    /// The root menu the remote's MENU key calls during the title (command-only, in the VTSM_PGCI_UT).
    pub root_menu: Option<VtsRootMenu>,
}

/// One VTS_TMAP: the VOBU that contains every multiple of `unit` seconds of a PGC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeMap {
    /// Time unit in seconds (at least 1).
    pub unit: u8,
    /// Relative start sector of the VOBU at `(i + 1) * unit` seconds.
    pub entries: Vec<u32>,
}

impl Vtsi {
    /// The entry PGC of VTS title `ttn` (one-based).
    pub fn title_pgc(&self, ttn: u8) -> Option<&Pgc> {
        self.pgcs.get(usize::from(ttn).checked_sub(1)?)
    }

    /// Every distinct cell of the title VOBS (the VTS_C_ADT list), sorted by VOB id and cell id.
    pub fn address_cells(&self) -> Vec<Cell> {
        address_cells(&self.pgcs)
    }
}

/// The distinct cells of `pgcs`, sorted by VOB id and cell id (a cell shared by several PGCs is listed once).
fn address_cells(pgcs: &[Pgc]) -> Vec<Cell> {
    let mut cells: Vec<Cell> = pgcs
        .iter()
        .flat_map(|pgc| pgc.cells.iter().copied())
        .collect();
    cells.sort_by_key(|c| (c.vob_id, c.cell_id));
    cells.dedup_by_key(|c| (c.vob_id, c.cell_id));
    cells
}

/// Write a VTSI file according to the M2 layout.
pub fn write_vtsi(v: &Vtsi) -> Result<Vec<u8>, IfoError> {
    validate_vtsi(v)?;

    let mut w = Writer::new();
    w.put_slice(0x000, b"DVDVIDEO-VTS");
    w.put_u16(0x020, 0x0011);
    w.put_u32(0x022, 0);
    w.put_u32(0x080, 0x3FF);
    w.put_u32(0x0C0, 0);
    w.put_u32(0x0D0, 0);
    w.put_u32(0x0D8, 0);
    w.put_u32(0x0DC, 0);

    let video = v.video.to_bytes();
    w.put_slice(0x100, &video);
    w.put_slice(0x200, &video);

    w.put_u16(
        0x202,
        u16::try_from(v.audio.len()).map_err(|_| IfoError("too many audio streams".to_string()))?,
    );
    for (i, a) in v.audio.iter().enumerate() {
        let attr_off = 0x204 + i * 8;
        w.put_slice(attr_off, &a.to_bytes());
    }
    w.put_u16(
        0x254,
        u16::try_from(v.subpictures.len())
            .map_err(|_| IfoError("too many subpicture streams".to_string()))?,
    );
    for (i, lang) in v.subpictures.iter().enumerate() {
        let attr_off = 0x256 + i * 6;
        if let Some(code) = lang {
            w.put_u8(attr_off, 0x01); // coding 0 (2-bit RLE), language type 1 (present)
            w.put_slice(attr_off + 2, code);
        }
    }

    w.pad_to_sector();

    let ptt_ptr = w.len() / SECTOR;
    write_ptt_srpt(&mut w, v)?;
    w.pad_to_sector();

    let pgciti_ptr = w.len() / SECTOR;
    write_pgciti(&mut w, v)?;
    w.pad_to_sector();

    if let Some(menu) = &v.root_menu {
        let pgci_ut_ptr = w.len() / SECTOR;
        write_vtsm_pgci_ut(&mut w, menu)?;
        w.pad_to_sector();
        w.put_u32(0x0D0, u32_from_usize(pgci_ut_ptr)?);
    }

    let tmapti_ptr = w.len() / SECTOR;
    write_tmapti(&mut w, v)?;
    w.pad_to_sector();

    let cadt_ptr = w.len() / SECTOR;
    write_c_adt(&mut w, v)?;
    w.pad_to_sector();

    let vobu_ptr = w.len() / SECTOR;
    write_vobu_admap(&mut w, v)?;
    w.pad_to_sector();

    let ifo_sectors = w.len() / SECTOR;
    let title_vobs = usize::try_from(v.title_vobs_sectors)
        .map_err(|_| IfoError("title VOBS sector count too large".to_string()))?;
    let last_title_set = ifo_sectors
        .checked_mul(2)
        .and_then(|x| x.checked_add(title_vobs))
        .and_then(|x| x.checked_add(crate::BUP_GAP_SECTORS as usize))
        .and_then(|x| x.checked_sub(1))
        .ok_or_else(|| IfoError("title set size overflow".to_string()))?;

    w.put_u32(0x00C, u32_from_usize(last_title_set)?);
    w.put_u32(
        0x01C,
        u32_from_usize(
            ifo_sectors
                .checked_sub(1)
                .ok_or_else(|| IfoError("empty IFO".to_string()))?,
        )?,
    );
    w.put_u32(0x0C4, u32_from_usize(ifo_sectors)?);
    w.put_u32(0x0C8, u32_from_usize(ptt_ptr)?);
    w.put_u32(0x0CC, u32_from_usize(pgciti_ptr)?);
    w.put_u32(0x0D4, u32_from_usize(tmapti_ptr)?);
    w.put_u32(0x0E0, u32_from_usize(cadt_ptr)?);
    w.put_u32(0x0E4, u32_from_usize(vobu_ptr)?);

    Ok(w.into_bytes())
}

/// Read a VTSI file and reconstruct the M2 model.
pub fn read_vtsi(data: &[u8]) -> Result<Vtsi, IfoError> {
    let r = Reader::new(data);
    if data.len() < SECTOR || !data.len().is_multiple_of(SECTOR) {
        return Err(IfoError("VTSI length must be whole sectors".to_string()));
    }
    if r.slice(0, 12)? != b"DVDVIDEO-VTS" {
        return Err(IfoError("bad VTSI magic".to_string()));
    }
    if r.u16(0x020)? != 0x0011 {
        return Err(IfoError("bad VTSI version".to_string()));
    }
    if r.u32(0x080)? != u32_from_usize(VTSI_MAT_END - 1)? {
        return Err(IfoError("bad VTSI_MAT end address".to_string()));
    }
    if r.u32(0x022)? != 0 {
        return Err(IfoError("bad VTS category".to_string()));
    }
    // Only a command-only root menu: no menu VOBS, cell addresses or VOBU map.
    if r.u32(0x0C0)? != 0 || r.u32(0x0D8)? != 0 || r.u32(0x0DC)? != 0 {
        return Err(IfoError("unexpected VTSM table pointer".to_string()));
    }

    let ifo_sectors = data.len() / SECTOR;
    let expected_last_vtsi = u32_from_usize(
        ifo_sectors
            .checked_sub(1)
            .ok_or_else(|| IfoError("empty IFO".to_string()))?,
    )?;
    if r.u32(0x01C)? != expected_last_vtsi {
        return Err(IfoError("bad VTSI last sector".to_string()));
    }

    let title_vobs_sectors = derive_title_vobs(r.u32(0x00C)?, ifo_sectors)?;
    if r.u32(0x0C4)? != u32_from_usize(ifo_sectors)? {
        return Err(IfoError("bad VTST_VOBS start sector".to_string()));
    }

    let video = read_video(&r)?;
    let audio = read_audio(&r)?;
    let subpictures = read_subpicture_attrs(&r)?;

    let ptt_ptr = r.u32(0x0C8)?;
    let pgciti_ptr = r.u32(0x0CC)?;
    let tmapti_ptr = r.u32(0x0D4)?;
    let cadt_ptr = r.u32(0x0E0)?;
    let vobu_ptr = r.u32(0x0E4)?;

    let pgcs = read_pgciti(&r, pgciti_ptr, ifo_sectors)?;
    let root_menu = match r.u32(0x0D0)? {
        0 => None,
        ptr => Some(read_vtsm_pgci_ut(&r, table_start(&r, ptr, ifo_sectors)?)?),
    };
    let mut programs = Vec::with_capacity(pgcs.len());
    for pgc in &pgcs {
        if pgc.cells.is_empty() {
            return Err(IfoError("no cells".to_string()));
        }
        if pgc.program_entry_cells.len() > 99 {
            return Err(IfoError("too many programs".to_string()));
        }
        programs.push(pgc.program_entry_cells.len());
    }

    read_ptt_srpt(&r, ptt_ptr, ifo_sectors, &programs)?;
    let tmaps = read_tmapti(&r, tmapti_ptr, ifo_sectors)?;
    if tmaps.len() != pgcs.len() {
        return Err(IfoError(
            "VTS_TMAPTI count does not match the PGCs".to_string(),
        ));
    }
    read_c_adt(&r, cadt_ptr, ifo_sectors, &address_cells(&pgcs))?;
    let vobu_starts = read_vobu_admap(&r, vobu_ptr, ifo_sectors)?;

    Ok(Vtsi {
        video,
        audio,
        pgcs,
        title_vobs_sectors,
        tmaps,
        vobu_starts,
        subpictures,
        root_menu,
    })
}

/// Return the 0x300-byte VTSI_MAT attribute block used by VMG VTS_ATRT.
pub fn vts_attr_block(vtsi: &[u8]) -> Result<Vec<u8>, IfoError> {
    let r = Reader::new(vtsi);
    let block = r.slice(0x100, VTSI_MAT_END - 0x100)?;
    Ok(block.to_vec())
}

fn validate_vtsi(v: &Vtsi) -> Result<(), IfoError> {
    if v.audio.len() > 8 {
        return Err(IfoError("too many audio streams".to_string()));
    }
    if v.pgcs.is_empty() || v.pgcs.len() > MAX_VTS_TITLES {
        return Err(IfoError("a VTS needs 1 to 99 title PGCs".to_string()));
    }
    for pgc in &v.pgcs {
        if pgc.cells.is_empty() {
            return Err(IfoError("no cells".to_string()));
        }
        if pgc.cells.len() > 255 {
            return Err(IfoError("too many cells".to_string()));
        }
        if pgc.program_entry_cells.len() > 99 {
            return Err(IfoError("too many programs".to_string()));
        }

        let max_cell =
            u8::try_from(pgc.cells.len()).map_err(|_| IfoError("too many cells".to_string()))?;
        for &entry in &pgc.program_entry_cells {
            if entry == 0 || entry > max_cell {
                return Err(IfoError("program entry cell out of range".to_string()));
            }
        }
    }

    if v.tmaps.len() != v.pgcs.len() {
        return Err(IfoError("one VTS_TMAP per PGC is required".to_string()));
    }
    for tmap in &v.tmaps {
        if tmap.unit == 0 {
            return Err(IfoError("TMAP unit must be at least 1".to_string()));
        }
        if tmap.entries.len() > usize::from(u16::MAX) {
            return Err(IfoError("too many TMAP entries".to_string()));
        }
    }

    Ok(())
}

fn write_ptt_srpt(w: &mut Writer, v: &Vtsi) -> Result<(), IfoError> {
    let base = w.len();
    let titles = v.pgcs.len();
    let titles_u16 = u16::try_from(titles).map_err(|_| IfoError("too many titles".to_string()))?;
    let mut total_programs = 0usize;
    for pgc in &v.pgcs {
        total_programs = total_programs
            .checked_add(pgc.program_entry_cells.len())
            .ok_or_else(|| IfoError("PTT overflow".to_string()))?;
    }
    let lists_start = titles
        .checked_mul(4)
        .and_then(|x| x.checked_add(8))
        .ok_or_else(|| IfoError("PTT overflow".to_string()))?;
    let end = total_programs
        .checked_mul(4)
        .and_then(|x| x.checked_add(lists_start))
        .and_then(|x| x.checked_sub(1))
        .ok_or_else(|| IfoError("PTT overflow".to_string()))?;

    w.put_u16(base, titles_u16);
    w.put_u16(off(base, 2)?, 0);
    w.put_u32(off(base, 4)?, u32_from_usize(end)?);

    let mut list_pos = lists_start;
    for (index, pgc) in v.pgcs.iter().enumerate() {
        let pgcn = u16::try_from(
            index
                .checked_add(1)
                .ok_or_else(|| IfoError("PTT overflow".to_string()))?,
        )
        .map_err(|_| IfoError("too many titles".to_string()))?;
        let chapters = u16::try_from(pgc.program_entry_cells.len())
            .map_err(|_| IfoError("too many chapters".to_string()))?;
        w.put_u32(
            off(
                base,
                off(
                    8,
                    index
                        .checked_mul(4)
                        .ok_or_else(|| IfoError("PTT overflow".to_string()))?,
                )?,
            )?,
            u32_from_usize(list_pos)?,
        );
        for k in 1..=chapters {
            let pos = off(base, list_pos)?;
            w.put_u16(pos, pgcn);
            w.put_u16(off(pos, 2)?, k);
            list_pos = off(list_pos, 4)?;
        }
    }
    Ok(())
}

fn write_pgciti(w: &mut Writer, v: &Vtsi) -> Result<(), IfoError> {
    let base = w.len();
    let count = v.pgcs.len();
    let count_u16 = u16::try_from(count).map_err(|_| IfoError("too many PGCs".to_string()))?;
    let header = count
        .checked_mul(8)
        .and_then(|x| x.checked_add(8))
        .ok_or_else(|| IfoError("PGCITI overflow".to_string()))?;

    let mut datas = Vec::with_capacity(count);
    let mut offset = header;
    let mut offsets = Vec::with_capacity(count);
    for pgc in &v.pgcs {
        let data = pgc::write_pgc(pgc)?;
        offsets.push(offset);
        offset = offset
            .checked_add(data.len())
            .ok_or_else(|| IfoError("PGCITI overflow".to_string()))?;
        datas.push(data);
    }
    let end = offset
        .checked_sub(1)
        .ok_or_else(|| IfoError("PGCITI overflow".to_string()))?;

    w.put_u16(base, count_u16);
    w.put_u16(off(base, 2)?, 0);
    w.put_u32(off(base, 4)?, u32_from_usize(end)?);
    for (index, (data, pgc_offset)) in datas.iter().zip(&offsets).enumerate() {
        // Every PGC is the entry PGC of VTS title index + 1.
        let ttn = u32::try_from(
            index
                .checked_add(1)
                .ok_or_else(|| IfoError("PGCITI overflow".to_string()))?,
        )
        .map_err(|_| IfoError("too many PGCs".to_string()))?;
        let category = (0x80 | ttn) << 24;
        let entry = off(
            base,
            off(
                8,
                index
                    .checked_mul(8)
                    .ok_or_else(|| IfoError("PGCITI overflow".to_string()))?,
            )?,
        )?;
        w.put_u32(entry, category);
        w.put_u32(off(entry, 4)?, u32_from_usize(*pgc_offset)?);
        w.put_slice(off(base, *pgc_offset)?, data);
    }
    Ok(())
}

fn write_tmapti(w: &mut Writer, v: &Vtsi) -> Result<(), IfoError> {
    let base = w.len();
    let count = v.tmaps.len();
    let count_u16 = u16::try_from(count).map_err(|_| IfoError("too many TMAPs".to_string()))?;
    let header = count
        .checked_mul(4)
        .and_then(|x| x.checked_add(8))
        .ok_or_else(|| IfoError("TMAP overflow".to_string()))?;

    let mut offset = header;
    let mut offsets = Vec::with_capacity(count);
    for tmap in &v.tmaps {
        offsets.push(offset);
        let len = tmap
            .entries
            .len()
            .checked_mul(4)
            .and_then(|x| x.checked_add(4))
            .ok_or_else(|| IfoError("TMAP overflow".to_string()))?;
        offset = offset
            .checked_add(len)
            .ok_or_else(|| IfoError("TMAP overflow".to_string()))?;
    }
    let end = offset
        .checked_sub(1)
        .ok_or_else(|| IfoError("TMAP overflow".to_string()))?;

    w.put_u16(base, count_u16);
    w.put_u16(off(base, 2)?, 0);
    w.put_u32(off(base, 4)?, u32_from_usize(end)?);
    for (index, (tmap, tmap_offset)) in v.tmaps.iter().zip(&offsets).enumerate() {
        let entries_u16 = u16::try_from(tmap.entries.len())
            .map_err(|_| IfoError("too many TMAP entries".to_string()))?;
        w.put_u32(
            off(
                base,
                off(
                    8,
                    index
                        .checked_mul(4)
                        .ok_or_else(|| IfoError("TMAP overflow".to_string()))?,
                )?,
            )?,
            u32_from_usize(*tmap_offset)?,
        );
        let tpos = off(base, *tmap_offset)?;
        w.put_u8(tpos, tmap.unit);
        w.put_u8(off(tpos, 1)?, 0);
        w.put_u16(off(tpos, 2)?, entries_u16);
        let mut pos = off(tpos, 4)?;
        for &s in &tmap.entries {
            w.put_u32(pos, s);
            pos = off(pos, 4)?;
        }
    }
    Ok(())
}

fn write_c_adt(w: &mut Writer, v: &Vtsi) -> Result<(), IfoError> {
    let sorted = address_cells(&v.pgcs);

    let base = w.len();
    let cells = sorted.len();
    let end = 8usize
        .checked_add(
            cells
                .checked_mul(12)
                .ok_or_else(|| IfoError("C_ADT overflow".to_string()))?,
        )
        .and_then(|x| x.checked_sub(1))
        .ok_or_else(|| IfoError("C_ADT overflow".to_string()))?;

    let mut distinct = 0usize;
    let mut prev: Option<u16> = None;
    for c in &sorted {
        if prev != Some(c.vob_id) {
            distinct = distinct
                .checked_add(1)
                .ok_or_else(|| IfoError("C_ADT overflow".to_string()))?;
            prev = Some(c.vob_id);
        }
    }

    w.put_u16(
        base,
        u16::try_from(distinct).map_err(|_| IfoError("too many VOB ids".to_string()))?,
    );
    w.put_u16(
        base.checked_add(2)
            .ok_or_else(|| IfoError("offset overflow".to_string()))?,
        0,
    );
    w.put_u32(
        base.checked_add(4)
            .ok_or_else(|| IfoError("offset overflow".to_string()))?,
        u32_from_usize(end)?,
    );

    let mut pos = base
        .checked_add(8)
        .ok_or_else(|| IfoError("offset overflow".to_string()))?;
    for c in &sorted {
        w.put_u16(pos, c.vob_id);
        w.put_u8(
            pos.checked_add(2)
                .ok_or_else(|| IfoError("offset overflow".to_string()))?,
            c.cell_id,
        );
        w.put_u8(
            pos.checked_add(3)
                .ok_or_else(|| IfoError("offset overflow".to_string()))?,
            0,
        );
        w.put_u32(
            pos.checked_add(4)
                .ok_or_else(|| IfoError("offset overflow".to_string()))?,
            c.first_sector,
        );
        w.put_u32(
            pos.checked_add(8)
                .ok_or_else(|| IfoError("offset overflow".to_string()))?,
            c.last_sector,
        );
        pos = pos
            .checked_add(12)
            .ok_or_else(|| IfoError("offset overflow".to_string()))?;
    }
    Ok(())
}

fn write_vobu_admap(w: &mut Writer, v: &Vtsi) -> Result<(), IfoError> {
    let base = w.len();
    let n = v.vobu_starts.len();
    let end = 4usize
        .checked_add(
            n.checked_mul(4)
                .ok_or_else(|| IfoError("VOBU_ADMAP overflow".to_string()))?,
        )
        .and_then(|x| x.checked_sub(1))
        .ok_or_else(|| IfoError("VOBU_ADMAP overflow".to_string()))?;

    w.put_u32(base, u32_from_usize(end)?);
    let mut pos = base
        .checked_add(4)
        .ok_or_else(|| IfoError("offset overflow".to_string()))?;
    for &s in &v.vobu_starts {
        w.put_u32(pos, s);
        pos = pos
            .checked_add(4)
            .ok_or_else(|| IfoError("offset overflow".to_string()))?;
    }
    Ok(())
}

fn derive_title_vobs(last_title_set: u32, ifo_sectors: usize) -> Result<u32, IfoError> {
    let two_ifo = u64::try_from(ifo_sectors)
        .map_err(|_| IfoError("IFO too large".to_string()))?
        .checked_mul(2)
        .and_then(|x| x.checked_add(u64::from(crate::BUP_GAP_SECTORS)))
        .ok_or_else(|| IfoError("VTS size overflow".to_string()))?;
    let total = u64::from(last_title_set)
        .checked_add(1)
        .ok_or_else(|| IfoError("VTS size overflow".to_string()))?;
    let vobs = total
        .checked_sub(two_ifo)
        .ok_or_else(|| IfoError("bad title set last sector".to_string()))?;
    u32::try_from(vobs).map_err(|_| IfoError("title VOBS too large".to_string()))
}

fn read_video(r: &Reader<'_>) -> Result<VideoAttr, IfoError> {
    let vmgm = VideoAttr::from_bytes(read_array2(r, 0x100)?)?;
    let vts = VideoAttr::from_bytes(read_array2(r, 0x200)?)?;
    if vmgm != vts {
        return Err(IfoError("video attributes mismatch".to_string()));
    }
    Ok(vts)
}

fn read_subpicture_attrs(r: &Reader<'_>) -> Result<Vec<Option<[u8; 2]>>, IfoError> {
    let count = usize::from(r.u16(0x254)?);
    if count > 32 {
        return Err(IfoError("bad VTS subpicture count".to_string()));
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = 0x256 + i * 6;
        let lang = if r.u8(base)? & 0x03 == 1 {
            Some([r.u8(base + 2)?, r.u8(base + 3)?])
        } else {
            None
        };
        out.push(lang);
    }
    Ok(out)
}

fn read_audio(r: &Reader<'_>) -> Result<Vec<AudioAttr>, IfoError> {
    let count = r.u16(0x202)?;
    if count > 8 {
        return Err(IfoError("too many audio streams".to_string()));
    }

    let mut out = Vec::with_capacity(usize::from(count));
    for i in 0..count {
        let attr_off = 0x204 + usize::from(i) * 8;
        out.push(AudioAttr::from_bytes(read_array8(r, attr_off)?)?);
    }
    Ok(out)
}

fn table_start(r: &Reader<'_>, ptr: u32, ifo_sectors: usize) -> Result<usize, IfoError> {
    let ptr_usize =
        usize::try_from(ptr).map_err(|_| IfoError("sector pointer too large".to_string()))?;
    if ptr_usize == 0 || ptr_usize >= ifo_sectors {
        return Err(IfoError("sector pointer outside IFO".to_string()));
    }
    let start = ptr_usize
        .checked_mul(SECTOR)
        .ok_or_else(|| IfoError("sector offset overflow".to_string()))?;
    if start >= r.len() {
        return Err(IfoError("sector pointer outside file".to_string()));
    }
    Ok(start)
}

fn table_end(r: &Reader<'_>, base: usize, end_address: u32) -> Result<usize, IfoError> {
    let len = u64::from(end_address)
        .checked_add(1)
        .and_then(|x| usize::try_from(x).ok())
        .ok_or_else(|| IfoError("table length too large".to_string()))?;
    let end = base
        .checked_add(len)
        .ok_or_else(|| IfoError("table offset overflow".to_string()))?;
    if end > r.len() {
        return Err(IfoError("table out of bounds".to_string()));
    }
    Ok(end)
}

fn read_pgciti(r: &Reader<'_>, ptr: u32, ifo_sectors: usize) -> Result<Vec<Pgc>, IfoError> {
    let base = table_start(r, ptr, ifo_sectors)?;
    let count = usize::from(r.u16(base)?);
    if count == 0 || count > MAX_VTS_TITLES {
        return Err(IfoError("VTS_PGCITI must contain 1 to 99 PGCs".to_string()));
    }
    if r.u16(off(base, 2)?)? != 0 {
        return Err(IfoError("bad VTS_PGCITI reserved field".to_string()));
    }

    let end = r.u32(off(base, 4)?)?;
    let end_pos = table_end(r, base, end)?;
    let header = count
        .checked_mul(8)
        .and_then(|x| x.checked_add(8))
        .ok_or_else(|| IfoError("PGCITI overflow".to_string()))?;

    let mut starts = Vec::with_capacity(count);
    for index in 0..count {
        let entry = off(
            base,
            off(
                8,
                index
                    .checked_mul(8)
                    .ok_or_else(|| IfoError("PGCITI overflow".to_string()))?,
            )?,
        )?;
        let ttn = u32::try_from(
            index
                .checked_add(1)
                .ok_or_else(|| IfoError("PGCITI overflow".to_string()))?,
        )
        .map_err(|_| IfoError("too many PGCs".to_string()))?;
        if r.u32(entry)? != (0x80 | ttn) << 24 {
            return Err(IfoError("bad PGC category".to_string()));
        }
        let offset = usize::try_from(r.u32(off(entry, 4)?)?)
            .map_err(|_| IfoError("PGC offset too large".to_string()))?;
        if offset < header {
            return Err(IfoError("bad PGC offset".to_string()));
        }
        let start = off(base, offset)?;
        if start >= end_pos {
            return Err(IfoError("PGC offset outside table".to_string()));
        }
        if starts.last().is_some_and(|&previous| start <= previous) {
            return Err(IfoError("PGC offsets out of order".to_string()));
        }
        starts.push(start);
    }

    let mut pgcs = Vec::with_capacity(count);
    for (index, &start) in starts.iter().enumerate() {
        let stop = starts
            .get(index.saturating_add(1))
            .copied()
            .unwrap_or(end_pos);
        let pgc_len = stop
            .checked_sub(start)
            .ok_or_else(|| IfoError("PGC length overflow".to_string()))?;
        pgcs.push(pgc::read_pgc(r.slice(start, pgc_len)?)?);
    }
    Ok(pgcs)
}

fn read_ptt_srpt(
    r: &Reader<'_>,
    ptr: u32,
    ifo_sectors: usize,
    programs: &[usize],
) -> Result<(), IfoError> {
    let base = table_start(r, ptr, ifo_sectors)?;
    let titles = usize::from(r.u16(base)?);
    if titles != programs.len() {
        return Err(IfoError(
            "VTS_PTT_SRPT title count does not match the PGCs".to_string(),
        ));
    }
    if r.u16(off(base, 2)?)? != 0 {
        return Err(IfoError("bad VTS_PTT_SRPT reserved field".to_string()));
    }

    let end = r.u32(off(base, 4)?)?;
    let lists_start = titles
        .checked_mul(4)
        .and_then(|x| x.checked_add(8))
        .ok_or_else(|| IfoError("PTT overflow".to_string()))?;
    let mut total_programs = 0usize;
    for &count in programs {
        total_programs = total_programs
            .checked_add(count)
            .ok_or_else(|| IfoError("PTT overflow".to_string()))?;
    }
    let expected_end = total_programs
        .checked_mul(4)
        .and_then(|x| x.checked_add(lists_start))
        .and_then(|x| x.checked_sub(1))
        .ok_or_else(|| IfoError("PTT overflow".to_string()))?;
    if end != u32_from_usize(expected_end)? {
        return Err(IfoError("bad VTS_PTT_SRPT end address".to_string()));
    }

    let end_pos = table_end(r, base, end)?;
    let mut list_pos = lists_start;
    for (index, &count) in programs.iter().enumerate() {
        let offset = usize::try_from(
            r.u32(off(
                base,
                off(
                    8,
                    index
                        .checked_mul(4)
                        .ok_or_else(|| IfoError("PTT overflow".to_string()))?,
                )?,
            )?)?,
        )
        .map_err(|_| IfoError("PTT offset too large".to_string()))?;
        if offset != list_pos {
            return Err(IfoError("bad PTT offset".to_string()));
        }
        let pgcn = u16::try_from(
            index
                .checked_add(1)
                .ok_or_else(|| IfoError("PTT overflow".to_string()))?,
        )
        .map_err(|_| IfoError("too many titles".to_string()))?;
        for k in 0..count {
            let pos = off(base, list_pos)?;
            if off(pos, 4)? > end_pos {
                return Err(IfoError("PTT list out of bounds".to_string()));
            }
            if r.u16(pos)? != pgcn {
                return Err(IfoError("bad PTT PGC number".to_string()));
            }
            let expected = u16::try_from(
                k.checked_add(1)
                    .ok_or_else(|| IfoError("PTT overflow".to_string()))?,
            )
            .map_err(|_| IfoError("too many programs".to_string()))?;
            if r.u16(off(pos, 2)?)? != expected {
                return Err(IfoError("bad PTT program number".to_string()));
            }
            list_pos = off(list_pos, 4)?;
        }
    }
    if off(base, list_pos)? != end_pos {
        return Err(IfoError("bad VTS_PTT_SRPT length".to_string()));
    }

    Ok(())
}

fn read_tmapti(r: &Reader<'_>, ptr: u32, ifo_sectors: usize) -> Result<Vec<TimeMap>, IfoError> {
    let base = table_start(r, ptr, ifo_sectors)?;
    let count = usize::from(r.u16(base)?);
    if count == 0 || count > MAX_VTS_TITLES {
        return Err(IfoError(
            "VTS_TMAPTI must contain 1 to 99 TMAPs".to_string(),
        ));
    }
    if r.u16(off(base, 2)?)? != 0 {
        return Err(IfoError("bad VTS_TMAPTI reserved field".to_string()));
    }

    let end = r.u32(off(base, 4)?)?;
    let end_pos = table_end(r, base, end)?;
    let mut pos = count
        .checked_mul(4)
        .and_then(|x| x.checked_add(8))
        .ok_or_else(|| IfoError("TMAP overflow".to_string()))?;

    let mut tmaps = Vec::with_capacity(count);
    for index in 0..count {
        let offset = usize::try_from(
            r.u32(off(
                base,
                off(
                    8,
                    index
                        .checked_mul(4)
                        .ok_or_else(|| IfoError("TMAP overflow".to_string()))?,
                )?,
            )?)?,
        )
        .map_err(|_| IfoError("TMAP offset too large".to_string()))?;
        if offset != pos {
            return Err(IfoError("bad TMAP offset".to_string()));
        }
        let tpos = off(base, pos)?;
        if off(tpos, 4)? > end_pos {
            return Err(IfoError("TMAP header out of bounds".to_string()));
        }

        let unit = r.u8(tpos)?;
        if unit == 0 {
            return Err(IfoError("TMAP unit must be at least 1".to_string()));
        }
        if r.u8(off(tpos, 1)?)? != 0 {
            return Err(IfoError("bad TMAP reserved byte".to_string()));
        }
        let entries = usize::from(r.u16(off(tpos, 2)?)?);
        let mut map = Vec::with_capacity(entries.min(4096));
        let mut epos = off(tpos, 4)?;
        for _ in 0..entries {
            if off(epos, 4)? > end_pos {
                return Err(IfoError("TMAP entry out of bounds".to_string()));
            }
            map.push(r.u32(epos)?);
            epos = off(epos, 4)?;
        }
        pos = epos
            .checked_sub(base)
            .ok_or_else(|| IfoError("TMAP overflow".to_string()))?;
        tmaps.push(TimeMap { unit, entries: map });
    }
    if off(base, pos)? != end_pos {
        return Err(IfoError("bad VTS_TMAP length".to_string()));
    }

    Ok(tmaps)
}

fn read_c_adt(
    r: &Reader<'_>,
    ptr: u32,
    ifo_sectors: usize,
    cells: &[Cell],
) -> Result<(), IfoError> {
    let base = table_start(r, ptr, ifo_sectors)?;
    let n_vob_ids = usize::from(r.u16(base)?);
    if r.u16(off(base, 2)?)? != 0 {
        return Err(IfoError("bad VTS_C_ADT reserved field".to_string()));
    }

    let end = r.u32(off(base, 4)?)?;
    let body = cells
        .len()
        .checked_mul(12)
        .ok_or_else(|| IfoError("C_ADT overflow".to_string()))?;
    let expected_end = 8usize
        .checked_add(body)
        .and_then(|x| x.checked_sub(1))
        .ok_or_else(|| IfoError("C_ADT overflow".to_string()))?;
    if end != u32_from_usize(expected_end)? {
        return Err(IfoError("bad VTS_C_ADT end address".to_string()));
    }

    let end_pos = table_end(r, base, end)?;
    let sorted: Vec<Cell> = cells.to_vec();

    if n_vob_ids != distinct_vob_ids(&sorted)? {
        return Err(IfoError("bad VTS_C_ADT VOB id count".to_string()));
    }

    let mut pos = off(base, 8)?;
    for c in &sorted {
        if pos
            .checked_add(12)
            .ok_or_else(|| IfoError("C_ADT overflow".to_string()))?
            > end_pos
        {
            return Err(IfoError("C_ADT entry out of bounds".to_string()));
        }
        if r.u16(pos)? != c.vob_id {
            return Err(IfoError("bad C_ADT VOB id".to_string()));
        }
        if r.u8(off(pos, 2)?)? != c.cell_id {
            return Err(IfoError("bad C_ADT cell id".to_string()));
        }
        if r.u8(off(pos, 3)?)? != 0 {
            return Err(IfoError("bad C_ADT reserved byte".to_string()));
        }
        if r.u32(off(pos, 4)?)? != c.first_sector {
            return Err(IfoError("bad C_ADT first sector".to_string()));
        }
        if r.u32(off(pos, 8)?)? != c.last_sector {
            return Err(IfoError("bad C_ADT last sector".to_string()));
        }
        pos = off(pos, 12)?;
    }
    if pos != end_pos {
        return Err(IfoError("bad VTS_C_ADT length".to_string()));
    }

    Ok(())
}

fn distinct_vob_ids(cells: &[Cell]) -> Result<usize, IfoError> {
    let mut count = 0usize;
    let mut prev: Option<u16> = None;
    for c in cells {
        if prev != Some(c.vob_id) {
            count = count
                .checked_add(1)
                .ok_or_else(|| IfoError("C_ADT overflow".to_string()))?;
            prev = Some(c.vob_id);
        }
    }
    Ok(count)
}

fn read_vobu_admap(r: &Reader<'_>, ptr: u32, ifo_sectors: usize) -> Result<Vec<u32>, IfoError> {
    let base = table_start(r, ptr, ifo_sectors)?;
    let end = r.u32(base)?;
    let end_pos = table_end(r, base, end)?;

    let body = end_pos
        .checked_sub(base)
        .ok_or_else(|| IfoError("VOBU_ADMAP overflow".to_string()))?;
    if body < 4 {
        return Err(IfoError("VOBU_ADMAP too short".to_string()));
    }
    let entries_len = body
        .checked_sub(4)
        .ok_or_else(|| IfoError("VOBU_ADMAP overflow".to_string()))?;
    if entries_len % 4 != 0 {
        return Err(IfoError("bad VOBU_ADMAP length".to_string()));
    }

    let entries = entries_len / 4;
    let expected_body = 4usize
        .checked_add(
            entries
                .checked_mul(4)
                .ok_or_else(|| IfoError("VOBU_ADMAP overflow".to_string()))?,
        )
        .ok_or_else(|| IfoError("VOBU_ADMAP overflow".to_string()))?;
    if body != expected_body {
        return Err(IfoError("bad VOBU_ADMAP length".to_string()));
    }
    let expected_end = expected_body
        .checked_sub(1)
        .ok_or_else(|| IfoError("VOBU_ADMAP overflow".to_string()))?;
    if end != u32_from_usize(expected_end)? {
        return Err(IfoError("bad VTS_VOBU_ADMAP end address".to_string()));
    }

    let mut out = Vec::new();
    let mut pos = off(base, 4)?;
    for _ in 0..entries {
        if pos
            .checked_add(4)
            .ok_or_else(|| IfoError("VOBU_ADMAP overflow".to_string()))?
            > end_pos
        {
            return Err(IfoError("VOBU_ADMAP entry out of bounds".to_string()));
        }
        out.push(r.u32(pos)?);
        pos = off(pos, 4)?;
    }
    if pos != end_pos {
        return Err(IfoError("bad VTS_VOBU_ADMAP length".to_string()));
    }

    Ok(out)
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

fn read_array8(r: &Reader<'_>, offset: usize) -> Result<[u8; 8], IfoError> {
    let s = r.slice(offset, 8)?;
    let mut a = [0u8; 8];
    for (i, slot) in a.iter_mut().enumerate() {
        if let Some(b) = s.get(i) {
            *slot = *b;
        }
    }
    Ok(a)
}

fn off(base: usize, delta: usize) -> Result<usize, IfoError> {
    base.checked_add(delta)
        .ok_or_else(|| IfoError("offset overflow".to_string()))
}

#[cfg(test)]
mod tests {
    use super::{TimeMap, VTSI_MAT_END, Vtsi, read_vtsi, vts_attr_block, write_vtsi};
    use crate::IfoError;
    use crate::attr::{AudioAttr, AudioCoding, BcdTime, Standard, VideoAttr, VmCommand};
    use crate::bytes::{Reader, SECTOR};
    use crate::pgc::{Cell, Pgc};

    fn sample_vtsi() -> Vtsi {
        Vtsi {
            video: VideoAttr {
                standard: Standard::Ntsc,
                wide: false,
            },
            audio: vec![AudioAttr {
                coding: AudioCoding::Ac3,
                channels: 2,
                language: Some(*b"ja"),
            }],
            pgcs: vec![Pgc {
                prohibited_ops: 0,
                playback_time: BcdTime([0, 0, 0, 0]),
                audio_streams: 1,
                program_entry_cells: vec![1, 2],
                cells: vec![
                    Cell {
                        vob_id: 1,
                        cell_id: 1,
                        stc_discontinuity: true,
                        seamless: false,
                        still_time: 0,
                        playback_time: BcdTime([0, 0, 0, 0]),
                        first_sector: 0,
                        last_vobu_start: 0,
                        last_sector: 9,
                    },
                    Cell {
                        vob_id: 1,
                        cell_id: 2,
                        stc_discontinuity: false,
                        seamless: true,
                        still_time: 0,
                        playback_time: BcdTime([0, 0, 0, 0]),
                        first_sector: 10,
                        last_vobu_start: 10,
                        last_sector: 19,
                    },
                ],
                pre: Vec::new(),
                post: vec![VmCommand::exit()],
                cell_commands: Vec::new(),
                still_time: 0,
                playback_mode: 0,
                subpicture_streams: 0,
                palette: Pgc::default_palette(),
            }],
            title_vobs_sectors: 20,
            tmaps: vec![TimeMap {
                unit: 1,
                entries: vec![0, 10],
            }],
            vobu_starts: vec![0, 10],
            subpictures: Vec::new(),
            root_menu: None,
        }
    }

    #[test]
    fn root_menu_round_trips_and_sets_the_vtsm_pointer() -> Result<(), IfoError> {
        let mut v = sample_vtsi();
        v.root_menu = Some(crate::vtsm::VtsRootMenu {
            language: *b"en",
            pgc: Pgc {
                prohibited_ops: 0,
                pre: vec![VmCommand([0x30, 0x06, 0x00, 0x03, 0x00, 0xC0, 0x00, 0x00])],
                ..Pgc::default()
            },
        });
        let data = write_vtsi(&v)?;
        let r = Reader::new(&data);
        assert_ne!(r.u32(0x0D0)?, 0);
        assert_eq!(r.u32(0x0C0)?, 0);
        assert_eq!(read_vtsi(&data)?, v);
        Ok(())
    }

    #[test]
    fn round_trip() -> Result<(), IfoError> {
        let v = sample_vtsi();
        let data = write_vtsi(&v)?;
        let back = read_vtsi(&data)?;
        assert_eq!(back, v);
        Ok(())
    }

    /// A second title PGC (a chapter playlist) lists the same cells in another order: both titles come back,
    /// the PTT and TMAP tables have one list each, and the cell address table still lists each cell once.
    #[test]
    fn two_titles_share_the_cells() -> Result<(), IfoError> {
        let mut v = sample_vtsi();
        let master = v
            .pgcs
            .first()
            .cloned()
            .ok_or_else(|| IfoError("no pgc".to_string()))?;
        let mut reversed = master.cells.clone();
        reversed.reverse();
        v.pgcs.push(Pgc {
            program_entry_cells: vec![1, 2],
            cells: reversed,
            playback_mode: 0xC1,
            ..master
        });
        v.tmaps.push(TimeMap {
            unit: 2,
            entries: vec![10],
        });

        let data = write_vtsi(&v)?;
        let back = read_vtsi(&data)?;
        assert_eq!(back, v);
        assert_eq!(back.address_cells().len(), 2);
        assert_eq!(
            back.title_pgc(2)
                .map(|pgc| pgc.cells.first().map(|c| c.cell_id)),
            Some(Some(2))
        );
        assert_eq!(back.title_pgc(3), None);

        let r = Reader::new(&data);
        let ptt = usize::try_from(r.u32(0x0C8)?)
            .map_err(|_| IfoError("test overflow".to_string()))?
            .checked_mul(SECTOR)
            .ok_or_else(|| IfoError("test overflow".to_string()))?;
        assert_eq!(r.u16(ptt)?, 2);
        // Title 2's list starts after the two offsets and title 1's two entries.
        assert_eq!(r.u32(super::off(ptt, 12)?)?, 16 + 8);
        assert_eq!(r.u16(super::off(ptt, 24)?)?, 2);

        // A TMAP count that disagrees with the PGCs is refused.
        let _ = v.tmaps.pop();
        assert!(write_vtsi(&v).is_err());
        Ok(())
    }

    #[test]
    fn magic_and_layout() -> Result<(), IfoError> {
        let v = sample_vtsi();
        let data = write_vtsi(&v)?;

        assert_eq!(data.get(0..12), Some(&b"DVDVIDEO-VTS"[..]));
        assert_eq!(data.len() % SECTOR, 0);

        let r = Reader::new(&data);
        assert_eq!(
            r.u32(0x0C4)?,
            u32::try_from(data.len() / SECTOR)
                .map_err(|_| IfoError("test overflow".to_string()))?
        );

        let cadt_ptr = r.u32(0x0E0)?;
        let base = usize::try_from(cadt_ptr)
            .map_err(|_| IfoError("test overflow".to_string()))?
            .checked_mul(SECTOR)
            .ok_or_else(|| IfoError("test overflow".to_string()))?;
        let end = r.u32(super::off(base, 4)?)?;
        let entries = u64::from(end)
            .checked_add(1)
            .and_then(|x| x.checked_sub(8))
            .ok_or_else(|| IfoError("test overflow".to_string()))?;
        let entries =
            usize::try_from(entries / 12).map_err(|_| IfoError("test overflow".to_string()))?;
        assert_eq!(entries, v.address_cells().len());

        let block = vts_attr_block(&data)?;
        assert_eq!(block.len(), VTSI_MAT_END - 0x100);
        // MPEG-2 NTSC 4:3, neither pan/scan nor letterbox (0x43).
        assert_eq!(block.first(), Some(&0x43));
        assert_eq!(block.get(0x100), Some(&0x43));
        assert_eq!(block.get(0x2FF), Some(&0x00));
        assert_eq!(block.get(0x300), None);

        Ok(())
    }

    #[test]
    fn hostile_prefixes_and_corruptions() -> Result<(), IfoError> {
        let v = sample_vtsi();
        let data = write_vtsi(&v)?;

        let mut len = 0usize;
        while len < data.len() {
            if let Some(prefix) = data.get(..len) {
                let _ = read_vtsi(prefix);
            }
            len = len.saturating_add(97);
        }
        if let Some(prefix) = data.get(..data.len()) {
            let _ = read_vtsi(prefix);
        }

        if !data.is_empty() {
            for i in 0..200usize {
                let mut corrupted = data.clone();
                if let Some(b) = corrupted.get_mut(i % data.len()) {
                    *b = b.wrapping_add(1);
                }
                let _ = read_vtsi(&corrupted);
            }
        }

        Ok(())
    }
}
