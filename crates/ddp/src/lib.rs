//! The DDP image of a DVD (Adobe Encore CS6's build output "DDP Image", made for replication): the disc image as
//! `MAIN.DAT`, the lead-in's control data as `CONTROL.DAT`, and `DDPID`, which says what the two files are and
//! where they go on the disc.
//!
//! The DDP specification itself is not used: it comes with a contract with DCA that does not suit an open-source
//! program (docs/backlog.md). The files are made as Encore CS6 makes them from the same image, compared byte for
//! byte as the output of a black box, and the control data is laid out as ECMA-267 (3rd edition, April 2001)
//! §26.5 describes it. Sources and what was compared: crates/ddp/README.md and docs/formats/ddp.md.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::fmt;
use std::fs::OpenOptions;
use std::io;
use std::path::Path;

/// The image's file in the DDP folder.
pub const MAIN_FILE: &str = "MAIN.DAT";
/// The control data's file.
pub const CONTROL_FILE: &str = "CONTROL.DAT";
/// The file that describes the other two.
pub const ID_FILE: &str = "DDPID";
/// A dual-layer disc's folders, one for each layer, as Encore names them (M14.4): the first holds layer 0's part of
/// the image, CONTROL.DAT and DDPID, the second layer 1's part and its own DDPID.
pub const LAYER_FOLDERS: [&str; 2] = ["Layer 1", "Layer 2"];

/// Bytes of a sector's main data (a sector of the image).
pub const SECTOR_BYTES: u64 = 2048;
/// Sectors of an ECC block (ECMA-267 §18): MAIN.DAT ends on a whole one.
const ECC_SECTORS: u32 = 16;
/// The first physical sector of the data zone (ECMA-267 §25: 030000h), where the image starts.
const DATA_ZONE: u32 = 0x03_0000;
/// The first physical sector of the control data zone (ECMA-267 figure 27: 02F200h).
const CONTROL_ZONE: u32 = 0x02_F200;
/// A control data block: one ECC block (ECMA-267 §26.5), which the disc repeats 192 times.
const CONTROL_SECTORS: u32 = 16;
/// The copyright management information (CPR_MAI, ECMA-267 §16.3) that comes before each control data sector's
/// main data in CONTROL.DAT.
const CPR_MAI_BYTES: usize = 6;
/// A control data sector in CONTROL.DAT: its CPR_MAI and its main data.
const CONTROL_SECTOR_BYTES: usize = CPR_MAI_BYTES + 2048;
/// Physical sector numbers have 24 bits.
const SECTOR_NUMBER_MAX: u32 = 0xFF_FFFF;
/// The size of DDPID's identification record and of each of its map packets.
const RECORD_BYTES: usize = 128;

/// Why a DDP image cannot be made.
#[derive(Debug)]
pub enum DdpError {
    /// The image does not fit the layout a DDP image describes (empty, too large, or a layer break it cannot
    /// take).
    Layout(String),
    /// Reading or writing the files.
    Io(io::Error),
}

impl fmt::Display for DdpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Layout(message) => write!(f, "DDP: {message}"),
            Self::Io(error) => write!(f, "DDP: {error}"),
        }
    }
}

impl std::error::Error for DdpError {}

impl From<io::Error> for DdpError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// The disc image a DDP image carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The image's size in sectors.
    pub sectors: u32,
    /// A dual-layer disc (DVD-9): the first sector of layer 1 in the image (the size of layer 0). `None` for one
    /// layer.
    pub layer_break: Option<u32>,
}

impl Layout {
    /// A single-layer disc of `sectors` sectors.
    #[must_use]
    pub fn single(sectors: u32) -> Self {
        Self {
            sectors,
            layer_break: None,
        }
    }

    /// The MAIN.DAT sectors of every layer: the image (each layer's part of it), the last followed by zeros up to a
    /// whole ECC block (as Encore writes it; layer 0 of a dual-layer disc is whole ECC blocks already).
    ///
    /// # Errors
    ///
    /// When the layout does not fit a disc ([`Layout::check`]).
    pub fn layer_sectors(&self) -> Result<Vec<u32>, DdpError> {
        self.check()?;
        let (first, rest) = match self.layer_break {
            None => (self.sectors, None),
            Some(layer_break) => (layer_break, Some(self.sectors - layer_break)),
        };
        let pad = |sectors: u32| {
            sectors
                .checked_next_multiple_of(ECC_SECTORS)
                .ok_or_else(|| too_large(self.sectors))
        };
        match rest {
            None => Ok(vec![pad(first)?]),
            Some(rest) => Ok(vec![first, pad(rest)?]),
        }
    }

    /// All of MAIN.DAT's sectors (every layer's).
    ///
    /// # Errors
    ///
    /// As [`Layout::layer_sectors`].
    pub fn main_sectors(&self) -> Result<u32, DdpError> {
        self.layer_sectors()?
            .into_iter()
            .try_fold(0_u32, u32::checked_add)
            .ok_or_else(|| too_large(self.sectors))
    }

    /// Whether the layout fits a disc: an image that is not empty, within the 24-bit sector numbers, and for a
    /// dual-layer disc (opposite track path, ECMA-267 §25) a layer break inside the image on a whole ECC block, with
    /// no more in layer 1 than in layer 0.
    ///
    /// # Errors
    ///
    /// [`DdpError::Layout`] saying what does not fit.
    pub fn check(&self) -> Result<(), DdpError> {
        if self.sectors == 0 {
            return Err(DdpError::Layout("the image is empty".to_owned()));
        }
        if let Some(layer_break) = self.layer_break {
            if layer_break == 0 || layer_break >= self.sectors {
                return Err(DdpError::Layout(format!(
                    "the layer break ({layer_break}) is outside the image of {} sectors",
                    self.sectors
                )));
            }
            if !layer_break.is_multiple_of(ECC_SECTORS) {
                return Err(DdpError::Layout(format!(
                    "the layer break ({layer_break}) is not on a whole ECC block (16 sectors)"
                )));
            }
            if self.sectors - layer_break > layer_break {
                return Err(DdpError::Layout(
                    "layer 1 holds more of the image than layer 0".to_owned(),
                ));
            }
        }
        self.data_end().map(|_| ())
    }

    /// The physical sector number of layer 0's last sector: of the image's last sector on a single-layer disc.
    fn layer0_last(&self) -> Result<u32, DdpError> {
        let sectors = self.layer_break.unwrap_or(self.sectors);
        DATA_ZONE
            .checked_add(sectors.saturating_sub(1))
            .filter(|last| *last <= SECTOR_NUMBER_MAX)
            .ok_or_else(|| too_large(self.sectors))
    }

    /// The physical sector number of the data zone's last sector (the image's last): on a dual-layer disc in layer
    /// 1, whose first sector number is layer 0's last with its bits inverted (ECMA-267 §25).
    fn data_end(&self) -> Result<u32, DdpError> {
        let last = self.layer0_last()?;
        let Some(layer_break) = self.layer_break else {
            return Ok(last);
        };
        (!last & SECTOR_NUMBER_MAX)
            .checked_add(self.sectors - layer_break - 1)
            .filter(|end| *end <= SECTOR_NUMBER_MAX)
            .ok_or_else(|| too_large(self.sectors))
    }
}

fn too_large(sectors: u32) -> DdpError {
    DdpError::Layout(format!(
        "an image of {sectors} sectors is larger than a DVD's sector numbers reach"
    ))
}

/// CONTROL.DAT: one control data block of the lead-in (ECMA-267 §26.5), each of its 16 sectors as 6 bytes of
/// CPR_MAI and 2048 bytes of main data. Sector 0 is the physical format information; the disc manufacturing
/// information (sector 1) and the content provider information (sectors 2 to 15) are zeros. For a single-layer
/// disc Encore sets the last CPR_MAI byte of the content provider sectors to C0h, for a dual-layer one it leaves them
/// zero (the standard leaves CPR_MAI to the application, §16.3).
///
/// # Errors
///
/// When the layout does not fit a disc ([`Layout::check`]).
pub fn control_data(layout: &Layout) -> Result<Vec<u8>, DdpError> {
    let format = physical_format(layout)?;
    let mut data = vec![0_u8; CONTROL_SECTORS as usize * CONTROL_SECTOR_BYTES];
    put(&mut data, CPR_MAI_BYTES, &format);
    if layout.layer_break.is_none() {
        for sector in 2..CONTROL_SECTORS as usize {
            put(
                &mut data,
                sector * CONTROL_SECTOR_BYTES + CPR_MAI_BYTES - 1,
                &[0xC0],
            );
        }
    }
    Ok(data)
}

/// The physical format information (ECMA-267 §26.5.1, table 6): its first 17 bytes, the rest zeros.
fn physical_format(layout: &Layout) -> Result<[u8; 17], DdpError> {
    layout.check()?;
    let last = layout.data_end()?.to_be_bytes();
    let first = DATA_ZONE.to_be_bytes();
    let (structure, density, layer0) = match layout.layer_break {
        // Type A (one layer), track path 0, a read-only layer (0001); channel bit length 0.133 µm, track pitch
        // 0.74 µm; no layer 0 end.
        None => (0x01, 0x00, [0; 3]),
        // Two layers (01), opposite track path (1), read-only layers (0001); channel bit length 0.147 µm (0001),
        // track pitch 0.74 µm; the last sector of layer 0.
        Some(_) => {
            let end = layout.layer0_last()?.to_be_bytes();
            (0x31, 0x10, [end[1], end[2], end[3]])
        }
    };
    Ok([
        // Disk category 0000 (read-only), version 0001.
        0x01, // Disk size 0000 (120 mm), maximum transfer rate 0010 (10.08 Mbit/s).
        0x02, structure, density,
        // The data zone: its first sector (030000h) and its last.
        0x00, first[1], first[2], first[3], 0x00, last[1], last[2], last[3], 0x00, layer0[0],
        layer0[1], layer0[2], // No burst cutting area.
        0x00,
    ])
}

/// DDPID as Encore writes it (of layer 0 on a dual-layer disc): an identification record (`DDP 2.00`, a DVD: one
/// layer `DV1010IB00`, layer 0 of two `DV1020OB00`), then one map packet for each file — CONTROL.DAT (2054-byte
/// sectors at the control data zone) and MAIN.DAT (2048-byte sectors at the data zone), each with its length in
/// sectors and its first physical sector, in decimal — all in ASCII padded with spaces.
///
/// # Errors
///
/// When the layout does not fit a disc ([`Layout::check`]).
pub fn ddp_id(layout: &Layout) -> Result<Vec<u8>, DdpError> {
    let sectors = layout.layer_sectors()?;
    let main = sectors.first().copied().unwrap_or(0);
    let mut id = vec![b' '; RECORD_BYTES * 3];
    put(&mut id, 0, b"DDP 2.00");
    let kind: &[u8; 10] = if layout.layer_break.is_some() {
        b"DV1020OB00"
    } else {
        b"DV1010IB00"
    };
    put(&mut id, 87, kind);
    map_packet(
        &mut id,
        RECORD_BYTES,
        (b"D2", b"DV10"),
        (CONTROL_SECTORS, CONTROL_ZONE),
        CONTROL_FILE,
    );
    map_packet(
        &mut id,
        RECORD_BYTES * 2,
        (b"D0", b"DV00"),
        (main, DATA_ZONE),
        MAIN_FILE,
    );
    Ok(id)
}

/// The DDPID of layer 1 of a dual-layer disc, as Encore writes it (M14.4): `DV1021OB00` and one map packet, its
/// MAIN.DAT with layer 0's last physical sector as its first (Encore's number: not the inverted one ECMA-267 gives
/// the sector). `None` for a single-layer disc.
///
/// # Errors
///
/// When the layout does not fit a disc ([`Layout::check`]).
pub fn second_layer_id(layout: &Layout) -> Result<Option<Vec<u8>>, DdpError> {
    let sectors = layout.layer_sectors()?;
    let Some(main) = sectors.get(1).copied() else {
        return Ok(None);
    };
    let mut id = vec![b' '; RECORD_BYTES * 2];
    put(&mut id, 0, b"DDP 2.00");
    put(&mut id, 87, b"DV1021OB00");
    map_packet(
        &mut id,
        RECORD_BYTES,
        (b"D0", b"DV00"),
        (main, layout.layer0_last()?),
        MAIN_FILE,
    );
    Ok(Some(id))
}

/// A map packet at `at`: its stream's two codes, its length in sectors and first physical sector, and its file.
fn map_packet(
    id: &mut [u8],
    at: usize,
    (stream, kind): (&[u8; 2], &[u8; 4]),
    (sectors, start): (u32, u32),
    file: &str,
) {
    put(id, at, b"VVVM");
    put(id, at + 4, stream);
    put(id, at + 14, format!("{sectors:08}{start:08}").as_bytes());
    put(id, at + 38, kind);
    put(id, at + 71, b"017");
    put(id, at + 74, file.as_bytes());
}

/// Copies `bytes` into `out` at `at` (nothing past the end).
fn put(out: &mut [u8], at: usize, bytes: &[u8]) {
    if let Some(slot) = at
        .checked_add(bytes.len())
        .and_then(|end| out.get_mut(at..end))
    {
        slot.copy_from_slice(bytes);
    }
}

/// Makes the folder `dir`, whose MAIN.DAT holds the image (written there by the image writer), a DDP image: pads
/// MAIN.DAT with zeros to a whole ECC block and writes CONTROL.DAT and DDPID beside it. A dual-layer disc's image is
/// split between the folders [`LAYER_FOLDERS`] in `dir` (M14.4, as Encore writes it): layer 0's part with
/// CONTROL.DAT and DDPID in the first, layer 1's part (padded) and its DDPID in the second.
///
/// # Errors
///
/// When MAIN.DAT is not the image's size, the layout does not fit a disc, or a file cannot be written.
pub fn finish(dir: &Path, layout: &Layout) -> Result<(), DdpError> {
    let sectors = layout.layer_sectors()?;
    let main = dir.join(MAIN_FILE);
    let held = std::fs::metadata(&main)?.len();
    let image = u64::from(layout.sectors) * SECTOR_BYTES;
    if held != image {
        return Err(DdpError::Layout(format!(
            "{MAIN_FILE} holds {held} bytes, not the image's {image}"
        )));
    }
    let Some(layer_break) = layout.layer_break else {
        let padded = u64::from(sectors.first().copied().unwrap_or(0)) * SECTOR_BYTES;
        // Growing a file fills it with zeros.
        OpenOptions::new()
            .write(true)
            .open(&main)?
            .set_len(padded)?;
        std::fs::write(dir.join(CONTROL_FILE), control_data(layout)?)?;
        std::fs::write(dir.join(ID_FILE), ddp_id(layout)?)?;
        return Ok(());
    };
    let [first, second] = LAYER_FOLDERS.map(|folder| dir.join(folder));
    std::fs::create_dir_all(&first)?;
    std::fs::create_dir_all(&second)?;
    // Layer 1's part first, copied from the end of the image; then the image, moved, is cut to layer 0's.
    let split = u64::from(layer_break) * SECTOR_BYTES;
    {
        let mut source = std::fs::File::open(&main)?;
        io::Seek::seek(&mut source, io::SeekFrom::Start(split))?;
        let mut tail = std::fs::File::create(second.join(MAIN_FILE))?;
        io::copy(&mut io::Read::take(source, image - split), &mut tail)?;
        let padded = u64::from(sectors.get(1).copied().unwrap_or(0)) * SECTOR_BYTES;
        tail.set_len(padded)?;
    }
    let moved = first.join(MAIN_FILE);
    std::fs::rename(&main, &moved)?;
    OpenOptions::new()
        .write(true)
        .open(&moved)?
        .set_len(split)?;
    std::fs::write(first.join(CONTROL_FILE), control_data(layout)?)?;
    std::fs::write(first.join(ID_FILE), ddp_id(layout)?)?;
    if let Some(id) = second_layer_id(layout)? {
        std::fs::write(second.join(ID_FILE), id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CONTROL_SECTOR_BYTES, LAYER_FOLDERS, Layout, MAIN_FILE, control_data, ddp_id, finish,
        second_layer_id,
    };
    use std::error::Error;

    /// The DDPID Encore CS6 wrote for an image of 7789 sectors (MAIN.DAT 7792), field by field.
    fn encores_id(main: &str) -> Vec<u8> {
        let mut id = vec![b' '; 384];
        let mut at = |offset: usize, text: &str| {
            id[offset..offset + text.len()].copy_from_slice(text.as_bytes());
        };
        at(0, "DDP 2.00");
        at(87, "DV1010IB00");
        at(128, "VVVMD2");
        at(142, "0000001600193024");
        at(166, "DV10");
        at(199, "017CONTROL.DAT");
        at(256, "VVVMD0");
        at(270, main);
        at(294, "DV00");
        at(327, "017MAIN.DAT");
        id
    }

    #[test]
    fn the_id_is_encores() -> Result<(), Box<dyn Error>> {
        assert_eq!(
            ddp_id(&Layout::single(7789))?,
            encores_id("0000779200196608")
        );
        // An image of whole ECC blocks is not padded.
        assert_eq!(
            ddp_id(&Layout::single(5168))?,
            encores_id("0000516800196608")
        );
        Ok(())
    }

    #[test]
    fn the_control_data_is_encores() -> Result<(), Box<dyn Error>> {
        let data = control_data(&Layout::single(7789))?;
        assert_eq!(data.len(), 32_864);
        // The physical format information after the first sector's CPR_MAI: ends at sector 031E6Ch.
        assert_eq!(
            data[..18],
            [
                0, 0, 0, 0, 0, 0, 0x01, 0x02, 0x01, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x03, 0x1E,
                0x6C
            ]
        );
        let nonzero: Vec<usize> = data
            .iter()
            .enumerate()
            .filter(|(_, byte)| **byte != 0)
            .map(|(at, _)| at)
            .collect();
        let mut expected = vec![6, 7, 8, 11, 15, 16, 17];
        expected.extend((2..16).map(|sector| sector * CONTROL_SECTOR_BYTES + 5));
        assert_eq!(nonzero, expected);
        assert!(expected[7..].iter().all(|at| data[*at] == 0xC0));
        let other = control_data(&Layout::single(5168))?;
        assert_eq!(other[15..18], [0x03, 0x14, 0x2F]);
        Ok(())
    }

    #[test]
    fn layouts_that_do_not_fit_are_refused() {
        assert!(Layout::single(0).main_sectors().is_err());
        assert!(Layout::single(u32::MAX).main_sectors().is_err());
        // The last sector number has 24 bits.
        assert!(
            Layout::single(0xFF_FFFF - 0x03_0000 + 1)
                .main_sectors()
                .is_ok()
        );
        assert!(
            Layout::single(0xFF_FFFF - 0x03_0000 + 2)
                .main_sectors()
                .is_err()
        );
        // M14.4: a dual-layer disc needs its layer break inside the image, on a whole ECC block, with no more after
        // it than before.
        let dual = |sectors: u32, layer_break: u32| Layout {
            sectors,
            layer_break: Some(layer_break),
        };
        assert!(ddp_id(&dual(3_000_000, 1_500_000)).is_ok());
        for refused in [
            dual(3_000_000, 1_500_001),
            dual(3_000_000, 1_400_000),
            dual(3_000_000, 3_000_000),
            dual(3_000_000, 0),
        ] {
            assert!(ddp_id(&refused).is_err(), "{refused:?}");
        }
    }

    /// M14.4: the files of a dual-layer disc as Encore CS6 wrote them (2026-11-21, an 8.32 GB project of two
    /// timelines, layer 0 of 2,046,208 sectors and layer 1 of 2,017,712): layer 0's DDPID (`DV1020OB00`, its MAIN.DAT
    /// from 030000h), layer 1's (`DV1021OB00`, its MAIN.DAT with layer 0's last sector 2238FFh as its first), and the
    /// control data (two layers, opposite track path, channel bit length 0.147 µm, the data zone ending in layer 1 at
    /// FC90AFh, layer 0 ending at 2238FFh, no C0h in the content provider sectors).
    #[test]
    fn the_dual_layer_files_are_encores() -> Result<(), Box<dyn Error>> {
        let layout = Layout {
            sectors: 4_063_920,
            layer_break: Some(2_046_208),
        };
        let mut first = vec![b' '; 384];
        let at = |id: &mut Vec<u8>, offset: usize, text: &str| {
            id[offset..offset + text.len()].copy_from_slice(text.as_bytes());
        };
        for (offset, text) in [
            (0, "DDP 2.00"),
            (87, "DV1020OB00"),
            (128, "VVVMD2"),
            (142, "0000001600193024"),
            (166, "DV10"),
            (199, "017CONTROL.DAT"),
            (256, "VVVMD0"),
            (270, "0204620800196608"),
            (294, "DV00"),
            (327, "017MAIN.DAT"),
        ] {
            at(&mut first, offset, text);
        }
        assert_eq!(ddp_id(&layout)?, first);
        let mut second = vec![b' '; 256];
        for (offset, text) in [
            (0, "DDP 2.00"),
            (87, "DV1021OB00"),
            (128, "VVVMD0"),
            (142, "0201771202242815"),
            (166, "DV00"),
            (199, "017MAIN.DAT"),
        ] {
            at(&mut second, offset, text);
        }
        assert_eq!(second_layer_id(&layout)?, Some(second));
        assert_eq!(second_layer_id(&Layout::single(7789))?, None);
        let data = control_data(&layout)?;
        assert_eq!(data.len(), 32_864);
        assert_eq!(
            data[6..23],
            [
                0x01, 0x02, 0x31, 0x10, 0x00, 0x03, 0x00, 0x00, 0x00, 0xFC, 0x90, 0xAF, 0x00, 0x22,
                0x38, 0xFF, 0x00
            ]
        );
        let nonzero: Vec<usize> = data
            .iter()
            .enumerate()
            .filter(|(_, byte)| **byte != 0)
            .map(|(at, _)| at)
            .collect();
        assert_eq!(nonzero, [6, 7, 8, 9, 11, 15, 16, 17, 19, 20, 21]);
        assert_eq!(layout.layer_sectors()?, [2_046_208, 2_017_712]);
        Ok(())
    }

    /// M14.4: a dual-layer disc's image is split between the two layers' folders, layer 1's part padded to a whole
    /// ECC block, with each layer's DDPID and layer 0's control data.
    #[test]
    fn a_dual_layer_image_is_split_between_its_layers() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-ddp-dual-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        let image: Vec<u8> = (0..40 * 2048).map(|i| (i % 253) as u8 + 1).collect();
        std::fs::write(dir.join(MAIN_FILE), &image)?;
        let layout = Layout {
            sectors: 40,
            layer_break: Some(32),
        };
        finish(&dir, &layout)?;
        assert!(!dir.join(MAIN_FILE).exists());
        let [first, second] = LAYER_FOLDERS.map(|folder| dir.join(folder));
        assert_eq!(std::fs::read(first.join(MAIN_FILE))?, image[..32 * 2048]);
        let rest = std::fs::read(second.join(MAIN_FILE))?;
        assert_eq!(rest.len(), 16 * 2048);
        assert_eq!(rest[..8 * 2048], image[32 * 2048..]);
        assert!(rest[8 * 2048..].iter().all(|byte| *byte == 0));
        assert_eq!(
            std::fs::read(first.join("CONTROL.DAT"))?,
            control_data(&layout)?
        );
        assert_eq!(std::fs::read(first.join("DDPID"))?, ddp_id(&layout)?);
        assert_eq!(
            Some(std::fs::read(second.join("DDPID"))?),
            second_layer_id(&layout)?
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn finishing_pads_the_image_and_writes_the_other_files() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-ddp-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let image: Vec<u8> = (0..13 * 2048).map(|i| (i % 251) as u8 + 1).collect();
        std::fs::write(dir.join(MAIN_FILE), &image)?;
        let layout = Layout::single(13);
        finish(&dir, &layout)?;
        let main = std::fs::read(dir.join(MAIN_FILE))?;
        assert_eq!(main.len(), 16 * 2048);
        assert_eq!(main[..image.len()], image[..]);
        assert!(main[image.len()..].iter().all(|byte| *byte == 0));
        assert_eq!(
            std::fs::read(dir.join("CONTROL.DAT"))?,
            control_data(&layout)?
        );
        assert_eq!(std::fs::read(dir.join("DDPID"))?, ddp_id(&layout)?);
        // A MAIN.DAT that is not the image is not taken.
        assert!(finish(&dir, &Layout::single(14)).is_err());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
