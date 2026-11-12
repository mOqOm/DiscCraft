//! Loading a VIDEO_TS folder into a compact navigation model.

use std::convert::TryFrom;
use std::fs::File;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;

use disccraft_ifo::MenuPgc;
use disccraft_ifo::Pgc;
use disccraft_ifo::TitleEntry;
use disccraft_ifo::VmCommand;
use disccraft_ifo::Vmgi;
use disccraft_ifo::Vtsi;
use disccraft_ifo::read_vmgi;
use disccraft_ifo::read_vtsi;
use disccraft_psmux::mux::hli::read_hli;

use crate::VmError;
use crate::invalid;

/// Eight-byte DVD VM command.
pub type Cmd = [u8; 8];

const SECTOR_SIZE: usize = 2048;
const MAX_FILE_BYTES: u64 = 1_073_741_824;
const PCI_DATA_OFFSET: usize = 45;
const PCI_DATA_LEN: usize = 979;

/// One menu button as needed by the VM simulator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonModel {
    /// Command executed when the button is selected.
    pub command: Cmd,
    /// Up adjacent button, 1-based.
    pub up: u8,
    /// Down adjacent button, 1-based.
    pub down: u8,
    /// Left adjacent button, 1-based.
    pub left: u8,
    /// Right adjacent button, 1-based.
    pub right: u8,
    /// Whether highlighting the button already activates it.
    pub auto_action: bool,
}

/// One VMGM menu PGC as needed by the VM simulator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuPgcModel {
    /// Pre commands.
    pub pre: Vec<Cmd>,
    /// Post commands.
    pub post: Vec<Cmd>,
    /// Whether this PGC has at least one cell.
    pub has_cells: bool,
    /// Highlight buttons, in button-number order.
    pub buttons: Vec<ButtonModel>,
    /// Default button, 1-based; `0` means none.
    pub default_button: u8,
    /// Prohibited user operations while the menu is shown (UOP0 is bit 0).
    pub prohibited_ops: u32,
}

/// One title as needed by the VM simulator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleModel {
    /// Title pre commands.
    pub pre: Vec<Cmd>,
    /// Title post commands.
    pub post: Vec<Cmd>,
    /// Number of chapters / programs.
    pub chapters: u16,
    /// Pre commands of the title set's root menu (what the MENU key runs); empty when there is none.
    pub menu_remote: Vec<Cmd>,
    /// Prohibited user operations while the title plays (UOP0 is bit 0).
    pub prohibited_ops: u32,
}

/// Navigation model for one VIDEO_TS folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscModel {
    /// VMGM PGCN of the title menu (the remote's Title key), if the disc has one.
    pub title_menu: Option<u16>,
    /// First play pre commands.
    pub first_play: Vec<Cmd>,
    /// VMGM menu PGCs, indexed by `PGCN - 1`.
    pub vmgm: Vec<MenuPgcModel>,
    /// Titles, indexed by `title - 1`.
    pub titles: Vec<TitleModel>,
}

/// Load a VIDEO_TS folder into a [`DiscModel`].
pub fn load_video_ts(dir: &Path) -> Result<DiscModel, VmError> {
    let vmgi = read_vmgi_file(dir)?;
    let first_play = commands_from(&vmgi.first_play.pre);
    let vmgm = load_vmgm(dir, &vmgi)?;
    let titles = load_titles(dir, &vmgi)?;

    let title_menu = vmgi.menus.as_ref().and_then(|menus| {
        let index = menus.pgcs.iter().position(|menu| menu.title_menu)?;
        u16::try_from(index.checked_add(1)?).ok()
    });

    Ok(DiscModel {
        title_menu,
        first_play,
        vmgm,
        titles,
    })
}

fn read_vmgi_file(dir: &Path) -> Result<Vmgi, VmError> {
    let data = read_file(&dir.join("VIDEO_TS.IFO"))?;
    read_vmgi(&data).map_err(|e| VmError::Ifo(e.0))
}

fn load_vmgm(dir: &Path, vmgi: &Vmgi) -> Result<Vec<MenuPgcModel>, VmError> {
    let Some(menus) = vmgi.menus.as_ref() else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    for menu in &menus.pgcs {
        out.push(load_menu_pgc(dir, menu)?);
    }
    Ok(out)
}

fn load_menu_pgc(dir: &Path, menu: &MenuPgc) -> Result<MenuPgcModel, VmError> {
    let pgc = &menu.pgc;
    let has_cells = !pgc.cells.is_empty();

    let (buttons, default_button) = if has_cells {
        load_buttons(dir, pgc)?
    } else {
        (Vec::new(), 0)
    };

    Ok(MenuPgcModel {
        pre: commands_from(&pgc.pre),
        post: commands_from(&pgc.post),
        has_cells,
        buttons,
        default_button,
        prohibited_ops: pgc.prohibited_ops,
    })
}

fn load_buttons(dir: &Path, pgc: &Pgc) -> Result<(Vec<ButtonModel>, u8), VmError> {
    let cell = pgc
        .cells
        .first()
        .ok_or_else(|| invalid("menu PGC has no cells"))?;

    let sector = read_sector(&dir.join("VIDEO_TS.VOB"), cell.first_sector)?;
    validate_nav(&sector)?;

    let pci = sector
        .get(PCI_DATA_OFFSET..PCI_DATA_OFFSET + PCI_DATA_LEN)
        .ok_or_else(|| invalid("PCI data out of range"))?;

    let hli = read_hli(pci).map_err(|_| invalid("HLI error"))?;
    let Some(hli) = hli else {
        return Ok((Vec::new(), 0));
    };

    let mut buttons = Vec::new();
    for button in &hli.buttons {
        buttons.push(ButtonModel {
            command: button.command,
            up: button.up,
            down: button.down,
            left: button.left,
            right: button.right,
            auto_action: button.auto_action,
        });
    }

    let default_button = hli.default_button;
    if default_button != 0 && usize::from(default_button) > buttons.len() {
        return Err(invalid("default button out of range"));
    }

    Ok((buttons, default_button))
}

fn validate_nav(sector: &[u8]) -> Result<(), VmError> {
    let pack = sector
        .get(0..4)
        .ok_or_else(|| invalid("NAV pack too short"))?;
    if pack != [0x00, 0x00, 0x01, 0xBA] {
        return Err(invalid("bad NAV pack header"));
    }

    let pci = sector
        .get(38..42)
        .ok_or_else(|| invalid("NAV pack too short"))?;
    if pci != [0x00, 0x00, 0x01, 0xBF] {
        return Err(invalid("bad PCI packet header"));
    }

    let sub = sector
        .get(44)
        .copied()
        .ok_or_else(|| invalid("NAV pack too short"))?;
    if sub != 0x00 {
        return Err(invalid("bad PCI sub stream"));
    }

    Ok(())
}

fn load_titles(dir: &Path, vmgi: &Vmgi) -> Result<Vec<TitleModel>, VmError> {
    let mut out = Vec::new();
    for entry in &vmgi.titles {
        out.push(load_title(dir, entry)?);
    }
    Ok(out)
}

fn load_title(dir: &Path, entry: &TitleEntry) -> Result<TitleModel, VmError> {
    let vts = entry.vts;
    if vts == 0 || vts > 99 {
        return Err(invalid("VTS number out of range"));
    }

    let name = format!("VTS_{vts:02}_0.IFO");
    let data = read_file(&dir.join(name))?;
    let vtsi: Vtsi = read_vtsi(&data).map_err(|e| VmError::Ifo(e.0))?;
    // A title set holds one entry PGC per VTS title: the master timeline, then its chapter playlists.
    let pgc = vtsi
        .title_pgc(entry.vts_ttn)
        .ok_or_else(|| invalid("VTS title number out of range"))?;

    let chapters =
        u16::try_from(pgc.program_entry_cells.len()).map_err(|_| invalid("too many chapters"))?;

    Ok(TitleModel {
        pre: commands_from(&pgc.pre),
        post: commands_from(&pgc.post),
        chapters,
        menu_remote: vtsi
            .root_menu
            .as_ref()
            .map(|menu| commands_from(&menu.pgc.pre))
            .unwrap_or_default(),
        prohibited_ops: pgc.prohibited_ops,
    })
}

fn commands_from(cmds: &[VmCommand]) -> Vec<Cmd> {
    cmds.iter().map(|cmd| cmd.0).collect()
}

fn read_file(path: &Path) -> Result<Vec<u8>, VmError> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    if len > MAX_FILE_BYTES {
        return Err(invalid("file larger than 1 GiB"));
    }

    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

fn read_sector(path: &Path, sector: u32) -> Result<Vec<u8>, VmError> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    if len > MAX_FILE_BYTES {
        return Err(invalid("VOB larger than 1 GiB"));
    }

    let offset = u64::from(sector)
        .checked_mul(SECTOR_SIZE as u64)
        .ok_or_else(|| invalid("sector offset overflow"))?;
    let end = offset
        .checked_add(SECTOR_SIZE as u64)
        .ok_or_else(|| invalid("sector end overflow"))?;

    if end > len {
        return Err(invalid("sector beyond end of file"));
    }

    file.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; SECTOR_SIZE];
    file.read_exact(&mut buf)?;
    Ok(buf)
}
