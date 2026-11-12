use crate::IfoError;
use crate::attr::{BcdTime, VmCommand};
use crate::bytes::{Reader, Writer};

const PGC_HEADER: usize = 0xEC;
const AUDIO_OFFSET: usize = 0x0C;
const AUDIO_ENTRIES: usize = 8;
const SUBPICTURE_OFFSET: usize = 0x1C;
const SUBPICTURE_ENTRIES: usize = 32;
const PALETTE_OFFSET: usize = 0xA4;
const PALETTE_ENTRIES: usize = 16;
const MAX_COMMANDS: usize = 8191;
const DEFAULT_PALETTE_ENTRY: u32 = 0x0010_8080;

/// One PGC cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// VOB identifier.
    pub vob_id: u16,
    /// Cell identifier, normally 1-based.
    pub cell_id: u8,
    /// Whether this cell starts with an STC discontinuity.
    pub stc_discontinuity: bool,
    /// Cell playback time.
    pub playback_time: BcdTime,
    /// First VOBU start sector, relative to the title VOBS.
    pub first_sector: u32,
    /// Last VOBU start sector, relative to the title VOBS.
    pub last_vobu_start: u32,
    /// Last sector of the cell, relative to the title VOBS.
    pub last_sector: u32,
    /// Cell still time; `0xFF` means infinite.
    pub still_time: u8,
}

/// Program chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pgc {
    /// PGC playback time.
    pub playback_time: BcdTime,
    /// Number of audio streams controlled by this PGC, 0..=8.
    pub audio_streams: u8,
    /// Number of subpicture streams controlled by this PGC, 0..=32.
    pub subpicture_streams: u8,
    /// PGC still time; `0xFF` means infinite.
    pub still_time: u8,
    /// PG playback mode (byte 0xA2): 0 plays the programs in order; `0x80 | (n - 1)` plays `n` programs at
    /// random, repeating; `0xC0 | (n - 1)` shuffles `n` programs, each once.
    pub playback_mode: u8,
    /// PGC palette, each entry encoded as `00 Y Cr Cb`.
    pub palette: [u32; PALETTE_ENTRIES],
    /// Program map: entry cell number for each program, 1-based.
    pub program_entry_cells: Vec<u8>,
    /// Cells in playback order.
    pub cells: Vec<Cell>,
    /// Pre commands.
    pub pre: Vec<VmCommand>,
    /// Post commands.
    pub post: Vec<VmCommand>,
    /// Cell commands.
    pub cell_commands: Vec<VmCommand>,
    /// Prohibited user operations (PGC byte 0x08; UOP0 is bit 0). 0 allows everything.
    pub prohibited_ops: u32,
}

impl Pgc {
    /// Return the default M2 grey palette.
    pub fn default_palette() -> [u32; PALETTE_ENTRIES] {
        [DEFAULT_PALETTE_ENTRY; PALETTE_ENTRIES]
    }
}

impl Default for Pgc {
    fn default() -> Self {
        Self {
            playback_time: BcdTime([0, 0, 0, 0]),
            audio_streams: 0,
            subpicture_streams: 0,
            still_time: 0,
            playback_mode: 0,
            palette: Self::default_palette(),
            program_entry_cells: Vec::new(),
            cells: Vec::new(),
            pre: Vec::new(),
            post: Vec::new(),
            cell_commands: Vec::new(),
            prohibited_ops: 0,
        }
    }
}

/// Write a PGC according to the M2/M3 layout.
pub fn write_pgc(pgc: &Pgc) -> Result<Vec<u8>, IfoError> {
    validate_pgc(pgc)?;

    let programs = u8::try_from(pgc.program_entry_cells.len())
        .map_err(|_| IfoError("too many programs".to_string()))?;
    let cells =
        u8::try_from(pgc.cells.len()).map_err(|_| IfoError("too many cells".to_string()))?;

    let mut w = Writer::new();
    w.put_u8(0x02, programs);
    w.put_u8(0x03, cells);
    write_bcd(&mut w, 0x04, pgc.playback_time);
    w.put_u32(0x08, pgc.prohibited_ops);
    write_audio_control(&mut w, pgc.audio_streams)?;
    write_subpicture_control(&mut w, pgc.subpicture_streams)?;
    w.put_u8(0xA2, pgc.playback_mode);
    w.put_u8(0xA3, pgc.still_time);
    write_palette(&mut w, &pgc.palette)?;

    let mut offset = PGC_HEADER;
    let cmd_total = command_total(pgc)?;
    let cmd_offset = write_command_section(&mut w, &mut offset, pgc, cmd_total)?;
    let prog_offset = write_program_section(&mut w, &mut offset, pgc, programs)?;
    let cell_play_offset = write_cell_playback_section(&mut w, &mut offset, pgc, cells)?;
    let cell_pos_offset = write_cell_position_section(&mut w, &mut offset, pgc, cells)?;

    if w.len() > 0x10000 {
        return Err(IfoError("PGC too large".to_string()));
    }

    w.put_u16(0xE4, cmd_offset);
    w.put_u16(0xE6, prog_offset);
    w.put_u16(0xE8, cell_play_offset);
    w.put_u16(0xEA, cell_pos_offset);

    Ok(w.into_bytes())
}

/// Read a PGC from its byte representation.
pub fn read_pgc(data: &[u8]) -> Result<Pgc, IfoError> {
    let r = Reader::new(data);
    if data.len() < PGC_HEADER {
        return Err(IfoError("PGC too short".to_string()));
    }

    let programs = r.u8(0x02)?;
    let cells = r.u8(0x03)?;
    let playback_time = read_bcd(&r, 0x04)?;
    let prohibited_ops = r.u32(0x08)?;
    let audio_streams = read_audio_streams(&r)?;
    let subpicture_streams = read_subpicture_streams(&r)?;
    let playback_mode = r.u8(0xA2)?;
    let still_time = r.u8(0xA3)?;
    let palette = read_palette(&r)?;

    let cmd_offset = r.u16(0xE4)?;
    let prog_offset = r.u16(0xE6)?;
    let cell_play_offset = r.u16(0xE8)?;
    let cell_pos_offset = r.u16(0xEA)?;

    if (programs == 0) != (prog_offset == 0) {
        return Err(IfoError("program map offset inconsistent".to_string()));
    }
    if (cells == 0) != (cell_play_offset == 0) || (cells == 0) != (cell_pos_offset == 0) {
        return Err(IfoError("cell table offset inconsistent".to_string()));
    }

    let (pre, post, cell_commands) = if cmd_offset == 0 {
        (Vec::new(), Vec::new(), Vec::new())
    } else {
        read_command_table(&r, cmd_offset)?
    };

    let program_entry_cells = if programs == 0 {
        Vec::new()
    } else {
        read_program_map(&r, prog_offset, usize::from(programs))?
    };

    let cell_list = if cells == 0 {
        Vec::new()
    } else {
        read_cells(&r, cell_play_offset, cell_pos_offset, usize::from(cells))?
    };

    for &entry in &program_entry_cells {
        if entry == 0 || entry > cells {
            return Err(IfoError("program entry cell out of range".to_string()));
        }
    }

    Ok(Pgc {
        prohibited_ops,
        playback_time,
        audio_streams,
        subpicture_streams,
        still_time,
        playback_mode,
        palette,
        program_entry_cells,
        cells: cell_list,
        pre,
        post,
        cell_commands,
    })
}

fn validate_pgc(pgc: &Pgc) -> Result<(), IfoError> {
    if usize::from(pgc.audio_streams) > AUDIO_ENTRIES {
        return Err(IfoError("too many audio streams".to_string()));
    }
    if usize::from(pgc.subpicture_streams) > SUBPICTURE_ENTRIES {
        return Err(IfoError("too many subpicture streams".to_string()));
    }
    if pgc.program_entry_cells.len() > 255 {
        return Err(IfoError("too many programs".to_string()));
    }
    if pgc.cells.len() > 255 {
        return Err(IfoError("too many cells".to_string()));
    }

    let cells =
        u8::try_from(pgc.cells.len()).map_err(|_| IfoError("too many cells".to_string()))?;
    for &entry in &pgc.program_entry_cells {
        if entry == 0 || entry > cells {
            return Err(IfoError("program entry cell out of range".to_string()));
        }
    }

    if command_total(pgc)? > MAX_COMMANDS {
        return Err(IfoError("too many VM commands".to_string()));
    }
    if pgc.playback_mode != 0 {
        let count = usize::from(pgc.playback_mode & 0x3F).saturating_add(1);
        if pgc.playback_mode & 0x80 == 0 || count > pgc.program_entry_cells.len() {
            return Err(IfoError("invalid PG playback mode".to_string()));
        }
    }

    for &color in &pgc.palette {
        if color & 0xff00_0000 != 0 {
            return Err(IfoError("invalid palette entry".to_string()));
        }
    }

    Ok(())
}

fn command_total(pgc: &Pgc) -> Result<usize, IfoError> {
    pgc.pre
        .len()
        .checked_add(pgc.post.len())
        .and_then(|v| v.checked_add(pgc.cell_commands.len()))
        .ok_or_else(|| IfoError("command count overflow".to_string()))
}

fn write_bcd(w: &mut Writer, offset: usize, time: BcdTime) {
    w.put_slice(offset, &time.0);
}

fn read_bcd(r: &Reader<'_>, offset: usize) -> Result<BcdTime, IfoError> {
    let s = r.slice(offset, 4)?;
    let mut a = [0u8; 4];
    for (i, slot) in a.iter_mut().enumerate() {
        if let Some(b) = s.get(i) {
            *slot = *b;
        }
    }
    Ok(BcdTime(a))
}

fn write_audio_control(w: &mut Writer, streams: u8) -> Result<(), IfoError> {
    for i in 0..usize::from(streams) {
        let delta = i
            .checked_mul(2)
            .ok_or_else(|| IfoError("offset overflow".to_string()))?;
        let pos = off(AUDIO_OFFSET, delta)?;
        let stream =
            u8::try_from(i).map_err(|_| IfoError("audio stream index too large".to_string()))?;
        w.put_u8(pos, 0x80 | stream);
        w.put_u8(off(pos, 1)?, 0);
    }
    Ok(())
}

fn write_subpicture_control(w: &mut Writer, streams: u8) -> Result<(), IfoError> {
    for i in 0..usize::from(streams) {
        let delta = i
            .checked_mul(4)
            .ok_or_else(|| IfoError("offset overflow".to_string()))?;
        let pos = off(SUBPICTURE_OFFSET, delta)?;
        let stream = u8::try_from(i)
            .map_err(|_| IfoError("subpicture stream index too large".to_string()))?;
        // Same stream for 4:3, wide, letterbox and pan&scan display.
        w.put_u8(pos, 0x80 | stream);
        w.put_u8(off(pos, 1)?, stream);
        w.put_u8(off(pos, 2)?, stream);
        w.put_u8(off(pos, 3)?, stream);
    }
    Ok(())
}

fn write_palette(w: &mut Writer, palette: &[u32; PALETTE_ENTRIES]) -> Result<(), IfoError> {
    for (i, &color) in palette.iter().enumerate() {
        let delta = i
            .checked_mul(4)
            .ok_or_else(|| IfoError("offset overflow".to_string()))?;
        w.put_u32(off(PALETTE_OFFSET, delta)?, color);
    }
    Ok(())
}

fn write_command_section(
    w: &mut Writer,
    offset: &mut usize,
    pgc: &Pgc,
    total: usize,
) -> Result<u16, IfoError> {
    if total == 0 {
        return Ok(0);
    }
    let cmd_offset =
        u16::try_from(*offset).map_err(|_| IfoError("PGC offset too large".to_string()))?;
    write_command_table(w, *offset, pgc)?;
    *offset = w.len();
    Ok(cmd_offset)
}

fn write_program_section(
    w: &mut Writer,
    offset: &mut usize,
    pgc: &Pgc,
    programs: u8,
) -> Result<u16, IfoError> {
    if programs == 0 {
        return Ok(0);
    }
    let prog_offset =
        u16::try_from(*offset).map_err(|_| IfoError("PGC offset too large".to_string()))?;
    write_program_map(w, *offset, pgc)?;
    *offset = w.len();
    Ok(prog_offset)
}

fn write_cell_playback_section(
    w: &mut Writer,
    offset: &mut usize,
    pgc: &Pgc,
    cells: u8,
) -> Result<u16, IfoError> {
    if cells == 0 {
        return Ok(0);
    }
    let cell_play_offset =
        u16::try_from(*offset).map_err(|_| IfoError("PGC offset too large".to_string()))?;
    write_cell_playback(w, *offset, pgc)?;
    *offset = w.len();
    Ok(cell_play_offset)
}

fn write_cell_position_section(
    w: &mut Writer,
    offset: &mut usize,
    pgc: &Pgc,
    cells: u8,
) -> Result<u16, IfoError> {
    if cells == 0 {
        return Ok(0);
    }
    let cell_pos_offset =
        u16::try_from(*offset).map_err(|_| IfoError("PGC offset too large".to_string()))?;
    write_cell_position(w, *offset, pgc)?;
    Ok(cell_pos_offset)
}

fn write_command_table(w: &mut Writer, offset: usize, pgc: &Pgc) -> Result<(), IfoError> {
    let n_pre =
        u16::try_from(pgc.pre.len()).map_err(|_| IfoError("too many pre commands".to_string()))?;
    let n_post = u16::try_from(pgc.post.len())
        .map_err(|_| IfoError("too many post commands".to_string()))?;
    let n_cell = u16::try_from(pgc.cell_commands.len())
        .map_err(|_| IfoError("too many cell commands".to_string()))?;

    let total = usize::from(n_pre)
        .checked_add(usize::from(n_post))
        .and_then(|v| v.checked_add(usize::from(n_cell)))
        .ok_or_else(|| IfoError("command count overflow".to_string()))?;
    let end = total
        .checked_mul(8)
        .and_then(|v| v.checked_add(7))
        .ok_or_else(|| IfoError("command table overflow".to_string()))?;

    w.put_u16(offset, n_pre);
    w.put_u16(off(offset, 2)?, n_post);
    w.put_u16(off(offset, 4)?, n_cell);
    w.put_u16(
        off(offset, 6)?,
        u16::try_from(end).map_err(|_| IfoError("command table too large".to_string()))?,
    );

    let mut pos = off(offset, 8)?;
    for cmd in pgc
        .pre
        .iter()
        .chain(pgc.post.iter())
        .chain(pgc.cell_commands.iter())
    {
        w.put_slice(pos, &cmd.0);
        pos = off(pos, 8)?;
    }
    Ok(())
}

fn write_program_map(w: &mut Writer, offset: usize, pgc: &Pgc) -> Result<(), IfoError> {
    let mut pos = offset;
    for &entry in &pgc.program_entry_cells {
        w.put_u8(pos, entry);
        pos = off(pos, 1)?;
    }
    if pgc.program_entry_cells.len() % 2 == 1 {
        w.put_u8(pos, 0);
    }
    Ok(())
}

fn write_cell_playback(w: &mut Writer, offset: usize, pgc: &Pgc) -> Result<(), IfoError> {
    let mut pos = offset;
    for cell in &pgc.cells {
        let category = if cell.stc_discontinuity { 0x02 } else { 0x00 };
        w.put_u8(pos, category);
        w.put_u8(off(pos, 1)?, 0);
        w.put_u8(off(pos, 2)?, cell.still_time);
        w.put_u8(off(pos, 3)?, 0);
        write_bcd(w, off(pos, 4)?, cell.playback_time);
        w.put_u32(off(pos, 8)?, cell.first_sector);
        w.put_u32(off(pos, 12)?, 0);
        w.put_u32(off(pos, 16)?, cell.last_vobu_start);
        w.put_u32(off(pos, 20)?, cell.last_sector);
        pos = off(pos, 24)?;
    }
    Ok(())
}

fn write_cell_position(w: &mut Writer, offset: usize, pgc: &Pgc) -> Result<(), IfoError> {
    let mut pos = offset;
    for cell in &pgc.cells {
        w.put_u16(pos, cell.vob_id);
        w.put_u8(off(pos, 2)?, 0);
        w.put_u8(off(pos, 3)?, cell.cell_id);
        pos = off(pos, 4)?;
    }
    Ok(())
}

fn read_audio_streams(r: &Reader<'_>) -> Result<u8, IfoError> {
    let mut mask = 0u8;
    for i in 0..AUDIO_ENTRIES {
        let delta = i
            .checked_mul(2)
            .ok_or_else(|| IfoError("offset overflow".to_string()))?;
        let pos = off(AUDIO_OFFSET, delta)?;
        let b0 = r.u8(pos)?;
        let b1 = r.u8(off(pos, 1)?)?;
        if b0 == 0 && b1 == 0 {
            continue;
        }
        let stream =
            u8::try_from(i).map_err(|_| IfoError("audio stream index too large".to_string()))?;
        if b0 == (0x80 | stream) && b1 == 0 {
            mask |= 1 << i;
        } else {
            return Err(IfoError("invalid audio stream control".to_string()));
        }
    }

    let mut count = 0usize;
    while count < AUDIO_ENTRIES && (mask & (1 << count)) != 0 {
        count += 1;
    }
    let expected = if count == AUDIO_ENTRIES {
        0xff
    } else {
        (1u16 << count) as u8 - 1
    };
    if mask != expected {
        return Err(IfoError("non-contiguous audio stream control".to_string()));
    }
    u8::try_from(count).map_err(|_| IfoError("audio stream count too large".to_string()))
}

fn read_subpicture_streams(r: &Reader<'_>) -> Result<u8, IfoError> {
    let mut mask = 0u32;
    for i in 0..SUBPICTURE_ENTRIES {
        let delta = i
            .checked_mul(4)
            .ok_or_else(|| IfoError("offset overflow".to_string()))?;
        let pos = off(SUBPICTURE_OFFSET, delta)?;
        let b0 = r.u8(pos)?;
        let b1 = r.u8(off(pos, 1)?)?;
        let b2 = r.u8(off(pos, 2)?)?;
        let b3 = r.u8(off(pos, 3)?)?;
        if b0 == 0 && b1 == 0 && b2 == 0 && b3 == 0 {
            continue;
        }
        let stream = u8::try_from(i)
            .map_err(|_| IfoError("subpicture stream index too large".to_string()))?;
        if b0 == (0x80 | stream) && b1 == stream && b2 == stream && b3 == stream {
            mask |= 1u32 << i;
        } else {
            return Err(IfoError("invalid subpicture stream control".to_string()));
        }
    }

    let mut count = 0usize;
    while count < SUBPICTURE_ENTRIES && (mask & (1u32 << count)) != 0 {
        count += 1;
    }
    let expected = if count == SUBPICTURE_ENTRIES {
        0xffff_ffff
    } else {
        (1u32 << count) - 1
    };
    if mask != expected {
        return Err(IfoError(
            "non-contiguous subpicture stream control".to_string(),
        ));
    }
    u8::try_from(count).map_err(|_| IfoError("subpicture stream count too large".to_string()))
}

fn read_palette(r: &Reader<'_>) -> Result<[u32; PALETTE_ENTRIES], IfoError> {
    let mut palette = [0u32; PALETTE_ENTRIES];
    for (i, slot) in palette.iter_mut().enumerate() {
        let delta = i
            .checked_mul(4)
            .ok_or_else(|| IfoError("offset overflow".to_string()))?;
        let color = r.u32(off(PALETTE_OFFSET, delta)?)?;
        if color & 0xff00_0000 != 0 {
            return Err(IfoError("invalid palette entry".to_string()));
        }
        *slot = color;
    }
    Ok(palette)
}

fn read_command_table(
    r: &Reader<'_>,
    offset: u16,
) -> Result<(Vec<VmCommand>, Vec<VmCommand>, Vec<VmCommand>), IfoError> {
    let base = usize::from(offset);
    if base < PGC_HEADER {
        return Err(IfoError("command table offset too small".to_string()));
    }

    let n_pre = r.u16(base)?;
    let n_post = r.u16(off(base, 2)?)?;
    let n_cell = r.u16(off(base, 4)?)?;
    let end = r.u16(off(base, 6)?)?;

    let total = usize::from(n_pre)
        .checked_add(usize::from(n_post))
        .and_then(|v| v.checked_add(usize::from(n_cell)))
        .ok_or_else(|| IfoError("command count overflow".to_string()))?;
    if total == 0 {
        return Err(IfoError("empty command table".to_string()));
    }

    let expected_end = total
        .checked_mul(8)
        .and_then(|v| v.checked_add(7))
        .ok_or_else(|| IfoError("command table overflow".to_string()))?;
    if u16::try_from(expected_end).map_err(|_| IfoError("command table too large".to_string()))?
        != end
    {
        return Err(IfoError("command table end address mismatch".to_string()));
    }

    let table_len = 8usize
        .checked_add(
            total
                .checked_mul(8)
                .ok_or_else(|| IfoError("command table overflow".to_string()))?,
        )
        .ok_or_else(|| IfoError("command table overflow".to_string()))?;
    if off(base, table_len)? > r.len() {
        return Err(IfoError("command table out of bounds".to_string()));
    }

    let mut pre = Vec::with_capacity(usize::from(n_pre));
    let mut post = Vec::with_capacity(usize::from(n_post));
    let mut cell_commands = Vec::with_capacity(usize::from(n_cell));

    let mut pos = off(base, 8)?;
    for _ in 0..n_pre {
        pre.push(read_vm_command(r, pos)?);
        pos = off(pos, 8)?;
    }
    for _ in 0..n_post {
        post.push(read_vm_command(r, pos)?);
        pos = off(pos, 8)?;
    }
    for _ in 0..n_cell {
        cell_commands.push(read_vm_command(r, pos)?);
        pos = off(pos, 8)?;
    }

    Ok((pre, post, cell_commands))
}

fn read_vm_command(r: &Reader<'_>, offset: usize) -> Result<VmCommand, IfoError> {
    let s = r.slice(offset, 8)?;
    let mut a = [0u8; 8];
    for (i, slot) in a.iter_mut().enumerate() {
        if let Some(b) = s.get(i) {
            *slot = *b;
        }
    }
    Ok(VmCommand(a))
}

fn read_program_map(r: &Reader<'_>, offset: u16, programs: usize) -> Result<Vec<u8>, IfoError> {
    let base = usize::from(offset);
    if base < PGC_HEADER {
        return Err(IfoError("program map offset too small".to_string()));
    }
    let padded = if programs % 2 == 1 {
        programs
            .checked_add(1)
            .ok_or_else(|| IfoError("offset overflow".to_string()))?
    } else {
        programs
    };
    if off(base, padded)? > r.len() {
        return Err(IfoError("program map out of bounds".to_string()));
    }

    let mut out = Vec::with_capacity(programs);
    for i in 0..programs {
        out.push(r.u8(off(base, i)?)?);
    }
    Ok(out)
}

fn read_cells(
    r: &Reader<'_>,
    play_offset: u16,
    pos_offset: u16,
    count: usize,
) -> Result<Vec<Cell>, IfoError> {
    let play_base = usize::from(play_offset);
    let pos_base = usize::from(pos_offset);
    if play_base < PGC_HEADER || pos_base < PGC_HEADER {
        return Err(IfoError("cell table offset too small".to_string()));
    }

    let play_len = count
        .checked_mul(24)
        .ok_or_else(|| IfoError("cell count overflow".to_string()))?;
    let pos_len = count
        .checked_mul(4)
        .ok_or_else(|| IfoError("cell count overflow".to_string()))?;
    if off(play_base, play_len)? > r.len() {
        return Err(IfoError("cell playback table out of bounds".to_string()));
    }
    if off(pos_base, pos_len)? > r.len() {
        return Err(IfoError("cell position table out of bounds".to_string()));
    }

    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let p = off(
            play_base,
            i.checked_mul(24)
                .ok_or_else(|| IfoError("offset overflow".to_string()))?,
        )?;
        let q = off(
            pos_base,
            i.checked_mul(4)
                .ok_or_else(|| IfoError("offset overflow".to_string()))?,
        )?;
        out.push(read_cell_at(r, p, q)?);
    }
    Ok(out)
}

fn read_cell_at(r: &Reader<'_>, p: usize, q: usize) -> Result<Cell, IfoError> {
    let category = r.u8(p)?;
    if category != 0x00 && category != 0x02 {
        return Err(IfoError("invalid cell category".to_string()));
    }
    if r.u8(off(p, 1)?)? != 0 || r.u8(off(p, 3)?)? != 0 {
        return Err(IfoError(
            "non-zero cell playback reserved bytes".to_string(),
        ));
    }

    let still_time = r.u8(off(p, 2)?)?;
    let playback_time = read_bcd(r, off(p, 4)?)?;
    let first_sector = r.u32(off(p, 8)?)?;
    let ilvu = r.u32(off(p, 12)?)?;
    if ilvu != 0 {
        return Err(IfoError("non-zero first ILVU end sector".to_string()));
    }
    let last_vobu_start = r.u32(off(p, 16)?)?;
    let last_sector = r.u32(off(p, 20)?)?;

    let vob_id = r.u16(q)?;
    if r.u8(off(q, 2)?)? != 0 {
        return Err(IfoError("non-zero cell position reserved byte".to_string()));
    }
    let cell_id = r.u8(off(q, 3)?)?;

    Ok(Cell {
        vob_id,
        cell_id,
        stc_discontinuity: category == 0x02,
        playback_time,
        first_sector,
        last_vobu_start,
        last_sector,
        still_time,
    })
}

fn off(base: usize, delta: usize) -> Result<usize, IfoError> {
    base.checked_add(delta)
        .ok_or_else(|| IfoError("offset overflow".to_string()))
}

#[cfg(test)]
mod uop_tests {
    use super::{Pgc, read_pgc, write_pgc};
    use std::error::Error;

    #[test]
    fn prohibited_operations_are_written_at_byte_8() -> Result<(), Box<dyn Error>> {
        let pgc = Pgc {
            prohibited_ops: 0x0000_0B00,
            ..Pgc::default()
        };
        let bytes = write_pgc(&pgc)?;
        assert_eq!(bytes.get(8..12), Some(&[0x00, 0x00, 0x0B, 0x00][..]));
        assert_eq!(read_pgc(&bytes)?.prohibited_ops, 0x0000_0B00);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Cell, Pgc, read_pgc, write_pgc};
    use crate::IfoError;
    use crate::attr::BcdTime;

    #[test]
    fn pgc_new_fields_round_trip() -> Result<(), IfoError> {
        let mut palette = Pgc::default_palette();
        if let Some(first) = palette.get_mut(0) {
            *first = 0x0012_3456;
        }

        let pgc = Pgc {
            prohibited_ops: 0,
            still_time: 0xFF,
            subpicture_streams: 1,
            palette,
            ..Pgc::default()
        };

        let data = write_pgc(&pgc)?;
        assert_eq!(data.get(0xA3), Some(&0xFF));
        assert_eq!(data.get(0x1C), Some(&0x80));

        let back = read_pgc(&data)?;
        assert_eq!(back, pgc);
        Ok(())
    }

    #[test]
    fn cell_still_time_round_trip() -> Result<(), IfoError> {
        let cell = Cell {
            vob_id: 1,
            cell_id: 1,
            stc_discontinuity: false,
            playback_time: BcdTime([0, 0, 0, 0]),
            first_sector: 0,
            last_vobu_start: 0,
            last_sector: 9,
            still_time: 0xFF,
        };

        let pgc = Pgc {
            prohibited_ops: 0,
            program_entry_cells: vec![1],
            cells: vec![cell],
            ..Pgc::default()
        };

        let data = write_pgc(&pgc)?;
        let back = read_pgc(&data)?;
        assert_eq!(back, pgc);
        Ok(())
    }
}
