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

    /// MAIN.DAT's size in sectors: the image, followed by zeros up to a whole ECC block (as Encore writes it).
    ///
    /// # Errors
    ///
    /// When the image is empty or larger than the sector numbers reach.
    pub fn main_sectors(&self) -> Result<u32, DdpError> {
        self.last_sector()?;
        self.sectors
            .checked_next_multiple_of(ECC_SECTORS)
            .ok_or_else(|| too_large(self.sectors))
    }

    /// The physical sector number of the image's last sector.
    fn last_sector(&self) -> Result<u32, DdpError> {
        if self.sectors == 0 {
            return Err(DdpError::Layout("the image is empty".to_owned()));
        }
        if self.layer_break.is_some() {
            return Err(DdpError::Layout(
                "a dual-layer disc (DVD-9) cannot be written as a DDP image yet".to_owned(),
            ));
        }
        DATA_ZONE
            .checked_add(self.sectors - 1)
            .filter(|last| *last <= SECTOR_NUMBER_MAX)
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
/// information (sector 1) and the content provider information (sectors 2 to 15) are zeros. Encore sets the last
/// CPR_MAI byte of the content provider sectors to C0h (the standard leaves CPR_MAI to the application, §16.3).
///
/// # Errors
///
/// When the layout does not fit a disc ([`Layout::main_sectors`]).
pub fn control_data(layout: &Layout) -> Result<Vec<u8>, DdpError> {
    let format = physical_format(layout)?;
    let mut data = vec![0_u8; CONTROL_SECTORS as usize * CONTROL_SECTOR_BYTES];
    put(&mut data, CPR_MAI_BYTES, &format);
    for sector in 2..CONTROL_SECTORS as usize {
        put(
            &mut data,
            sector * CONTROL_SECTOR_BYTES + CPR_MAI_BYTES - 1,
            &[0xC0],
        );
    }
    Ok(data)
}

/// The physical format information (ECMA-267 §26.5.1, table 6): its first 17 bytes, the rest zeros.
fn physical_format(layout: &Layout) -> Result<[u8; 17], DdpError> {
    let last = layout.last_sector()?.to_be_bytes();
    let first = DATA_ZONE.to_be_bytes();
    Ok([
        // Disk category 0000 (read-only), version 0001.
        0x01, // Disk size 0000 (120 mm), maximum transfer rate 0010 (10.08 Mbit/s).
        0x02, // Type A (one layer), track path 0, a read-only layer (0001).
        0x01, // Channel bit length 0.133 µm, track pitch 0.74 µm.
        0x00, // The data zone: its first sector (030000h) and its last.
        0x00, first[1], first[2], first[3], 0x00, last[1], last[2], last[3],
        // The last sector of layer 0: 0 for one layer.
        0x00, 0x00, 0x00, 0x00, // No burst cutting area.
        0x00,
    ])
}

/// DDPID as Encore writes it: an identification record (`DDP 2.00`, a DVD), then one map packet for each file —
/// CONTROL.DAT (2054-byte sectors at the control data zone) and MAIN.DAT (2048-byte sectors at the data zone),
/// each with its length in sectors and its first physical sector, in decimal — all in ASCII padded with spaces.
///
/// # Errors
///
/// When the layout does not fit a disc ([`Layout::main_sectors`]).
pub fn ddp_id(layout: &Layout) -> Result<Vec<u8>, DdpError> {
    let main = layout.main_sectors()?;
    let mut id = vec![b' '; RECORD_BYTES * 3];
    put(&mut id, 0, b"DDP 2.00");
    put(&mut id, 87, b"DV1010IB00");
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
/// MAIN.DAT with zeros to a whole ECC block and writes CONTROL.DAT and DDPID beside it.
///
/// # Errors
///
/// When MAIN.DAT is not the image's size, the layout does not fit a disc, or a file cannot be written.
pub fn finish(dir: &Path, layout: &Layout) -> Result<(), DdpError> {
    let padded = u64::from(layout.main_sectors()?) * SECTOR_BYTES;
    let main = dir.join(MAIN_FILE);
    let held = std::fs::metadata(&main)?.len();
    let image = u64::from(layout.sectors) * SECTOR_BYTES;
    if held != image {
        return Err(DdpError::Layout(format!(
            "{MAIN_FILE} holds {held} bytes, not the image's {image}"
        )));
    }
    // Growing a file fills it with zeros.
    OpenOptions::new()
        .write(true)
        .open(&main)?
        .set_len(padded)?;
    std::fs::write(dir.join(CONTROL_FILE), control_data(layout)?)?;
    std::fs::write(dir.join(ID_FILE), ddp_id(layout)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CONTROL_SECTOR_BYTES, Layout, MAIN_FILE, control_data, ddp_id, finish};
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
        let dual = Layout {
            sectors: 3_000_000,
            layer_break: Some(1_500_000),
        };
        assert!(ddp_id(&dual).is_err());
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
