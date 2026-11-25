//! The SCSI Multi-Media Commands DiscCraft sends to a recorder itself, and what it reads from their answers (M9.45):
//! the bytes only, so that they are tested on every system; the system's disc writing sends them (IMAPI2's
//! `IDiscRecorder2Ex` on Windows, in [`crate::burn`]).
//!
//! From SCSI Multi-Media Commands - 5 (MMC-5), T10/1675-D revision 4 (24 October 2006): READ DISC INFORMATION
//! (6.22, Tables 374 to 377), READ TRACK INFORMATION (6.27, Tables 497 to 499), CLOSE TRACK SESSION (6.3, Tables 240
//! and 241; 6.3.3.5 for DVD+R DL), SET STREAMING (6.39, Tables 617 and 619; 6.39.3.2 for its RDD bit), SYNCHRONIZE
//! CACHE (6.41, Table 632),
//! TEST UNIT READY (6.42), WRITE (10) (6.44, Table 640), the busy drive of 4.1.6.2 and the timeout groups of 4.1.9.
//! A command that fails leaves sense data: the fixed format (response code 70h / 71h: sense key in byte 2, ASC and
//! ASCQ in bytes 12 and 13) or the descriptor format (72h / 73h: bytes 1 to 3), as SCSI's primary commands define
//! them; REQUEST SENSE (6.30, opcode 03h, its CDB as SPC-3 lays it out: the allocation length in byte 4) asks the
//! drive for it after a command that came back without it (M9.47).
//!
//! The media ID of a DVD+R / DVD+R DL (M9.47) is in its ADIP information, which READ DISC STRUCTURE returns with the
//! format code 11h (MMC-5 6.23.3.2.17, Table 425: after a four-byte header) as the disc carries it: the Disk
//! Manufacturer ID in bytes 19 to 26, the Media Type ID in bytes 27 to 29 (both ECMA-43 G0 characters and spaces,
//! unused bytes 00h) and the product revision number in bits 5 to 0 of byte 30 (ECMA-364 3rd edition, December 2007,
//! 14.4.2, Table 3; bytes 0 to 30 are the same on both layers).

use std::fmt;

/// The bytes of the sense data a command asks for (the fixed format's 18).
pub const SENSE_BYTES: usize = 18;

/// The bytes asked of READ DISC INFORMATION: the Disc Information Block up to its Last Possible Lead-out Start
/// Address.
pub const DISC_INFORMATION_BYTES: u16 = 34;

/// The bytes asked of READ TRACK INFORMATION: the whole Track Information Block of MMC-5.
pub const TRACK_INFORMATION_BYTES: u16 = 48;

/// The Logical Track number that stands for the invisible or incomplete track of a DVD+R (Table 498, with the
/// Address/Number Type 01b).
pub const INCOMPLETE_TRACK: u32 = 255;

/// READ DISC INFORMATION, Standard Disc Information (data type 000b), `length` bytes.
#[must_use]
pub fn read_disc_information(length: u16) -> [u8; 10] {
    let [high, low] = length.to_be_bytes();
    [0x51, 0, 0, 0, 0, 0, 0, high, low, 0]
}

/// READ TRACK INFORMATION of the Logical Track `track` (Address/Number Type 01b), `length` bytes.
#[must_use]
pub fn read_track_information(track: u32, length: u16) -> [u8; 10] {
    let [t0, t1, t2, t3] = track.to_be_bytes();
    let [high, low] = length.to_be_bytes();
    [0x52, 0b01, t0, t1, t2, t3, 0, high, low, 0]
}

/// TEST UNIT READY.
#[must_use]
pub fn test_unit_ready() -> [u8; 6] {
    [0; 6]
}

/// REQUEST SENSE of [`SENSE_BYTES`] (the fixed format: Desc zero).
#[must_use]
pub fn request_sense() -> [u8; 6] {
    [
        0x03,
        0,
        0,
        0,
        u8::try_from(SENSE_BYTES).unwrap_or(u8::MAX),
        0,
    ]
}

/// The READ DISC STRUCTURE format code of a DVD+R's ADIP information (Table 425).
pub const ADIP_INFORMATION: u8 = 0x11;

/// The bytes asked of READ DISC STRUCTURE for the ADIP information: the header and the 256-byte block.
pub const ADIP_BYTES: u16 = 4 + 256;

/// The media ID of a DVD+R / DVD+R DL (M9.47): who made the disc and which of their kinds it is, as recorders tell
/// discs apart to choose how to write them ("MKM-003-00").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaId {
    /// The Disk Manufacturer ID.
    pub manufacturer: String,
    /// The Media Type ID.
    pub media_type: String,
    /// The product revision number.
    pub revision: u8,
}

impl MediaId {
    /// The media ID in `structure` (READ DISC STRUCTURE format 11h, its header first), `None` when the disc carries
    /// none.
    #[must_use]
    pub fn parse(structure: &[u8]) -> Option<Self> {
        let block = structure.get(4..)?;
        let text = |bytes: &[u8]| -> String {
            bytes
                .iter()
                .take_while(|byte| **byte != 0)
                .map(|byte| {
                    if (0x20..=0x7E).contains(byte) {
                        char::from(*byte)
                    } else {
                        '?'
                    }
                })
                .collect::<String>()
                .trim()
                .to_owned()
        };
        let manufacturer = text(block.get(19..27)?);
        let media_type = text(block.get(27..30)?);
        let revision = block.get(30)? & 0x3F;
        (!manufacturer.is_empty()).then_some(Self {
            manufacturer,
            media_type,
            revision,
        })
    }
}

impl fmt::Display for MediaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}-{}-{:02}",
            self.manufacturer, self.media_type, self.revision
        )
    }
}

/// WRITE (10) of `blocks` sectors at `lba` (to the drive's cache: FUA zero).
#[must_use]
pub fn write10(lba: u32, blocks: u16) -> [u8; 10] {
    let [a0, a1, a2, a3] = lba.to_be_bytes();
    let [high, low] = blocks.to_be_bytes();
    [0x2A, 0, a0, a1, a2, a3, 0, high, low, 0]
}

/// SYNCHRONIZE CACHE: what the drive's cache holds goes on the disc; with `immed`, it answers at once and is busy
/// until that is done.
#[must_use]
pub fn synchronize_cache(immed: bool) -> [u8; 10] {
    [0x35, if immed { 0b10 } else { 0 }, 0, 0, 0, 0, 0, 0, 0, 0]
}

/// What CLOSE TRACK SESSION closes on a DVD+R DL (6.3.3.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Close {
    /// 001b: the fragment of the track (the incomplete one, after it is written, gets its place in the session).
    Track,
    /// 101b: finalizes the disc with the minimal radius, the close function MMC-5 recommends for DVD players: the
    /// lead-in, both middle zones (the L0 one where layer 0 ends) and the lead-out.
    FinalizeMinimalRadius,
}

/// CLOSE TRACK SESSION of `close` for the Logical Track `track`; with `immed`, the drive answers at once and is busy
/// until it is done.
#[must_use]
pub fn close_track_session(immed: bool, close: Close, track: u16) -> [u8; 10] {
    let function = match close {
        Close::Track => 0b001,
        Close::FinalizeMinimalRadius => 0b101,
    };
    let [high, low] = track.to_be_bytes();
    [0x5B, u8::from(immed), function, 0, high, low, 0, 0, 0, 0]
}

/// The bytes of a performance descriptor (Table 619).
pub const PERFORMANCE_BYTES: u16 = 28;

/// SET STREAMING with a performance descriptor (type 0) of [`PERFORMANCE_BYTES`].
#[must_use]
pub fn set_streaming() -> [u8; 12] {
    let [high, low] = PERFORMANCE_BYTES.to_be_bytes();
    [0xB6, 0, 0, 0, 0, 0, 0, 0, 0, high, low, 0]
}

/// The performance descriptor asking `kilobytes_per_second` (1000 bytes) for reading and writing from sector 0 to
/// `end_lba` (as near as the drive can: Exact zero).
#[must_use]
pub fn performance(end_lba: u32, kilobytes_per_second: u32) -> [u8; 28] {
    let mut descriptor = [0u8; 28];
    let fields = [
        (4, 0),
        (8, end_lba),
        (12, kilobytes_per_second),
        (16, 1000),
        (20, kilobytes_per_second),
        (24, 1000),
    ];
    for (at, value) in fields {
        if let Some(slot) = descriptor.get_mut(at..at + 4) {
            slot.copy_from_slice(&value.to_be_bytes());
        }
    }
    descriptor
}

/// The RDD (Restore Drive Defaults) bit of a performance descriptor's byte 0 (Table 619).
const RESTORE_DRIVE_DEFAULTS: u8 = 0b100;

/// The performance descriptor that sends the drive back to its own speeds (M9.48): RDD set, the rest ignored
/// (6.39.3.2). [`performance`] asks its speed for reading as well as for writing, so after a writing the disc was read
/// back (verified) no faster than it was written, and on the inner part of a layer slower still.
#[must_use]
pub fn performance_defaults() -> [u8; 28] {
    let mut descriptor = [0u8; 28];
    if let Some(first) = descriptor.first_mut() {
        *first = RESTORE_DRIVE_DEFAULTS;
    }
    descriptor
}

/// A whole number of 1000-byte kilobytes a second for `sectors_per_second` of 2048 bytes.
#[must_use]
pub fn kilobytes_per_second(sectors_per_second: u32) -> u32 {
    u32::try_from(u64::from(sectors_per_second) * 2048 / 1000).unwrap_or(u32::MAX)
}

/// The sense data of a command that did not end well: its sense key, additional sense code and qualifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sense {
    /// The sense key (2 = NOT READY, 3 = MEDIUM ERROR, 5 = ILLEGAL REQUEST, 6 = UNIT ATTENTION, …).
    pub key: u8,
    /// The additional sense code.
    pub asc: u8,
    /// The additional sense code qualifier.
    pub ascq: u8,
}

impl Sense {
    /// The sense in `data` (fixed or descriptor format), `None` when there is none (no response code, or NO SENSE
    /// with nothing more).
    #[must_use]
    pub fn parse(data: &[u8]) -> Option<Self> {
        let (key, asc, ascq) = match data.first()? & 0x7F {
            0x70 | 0x71 => (data.get(2)? & 0x0F, *data.get(12)?, *data.get(13)?),
            0x72 | 0x73 => (data.get(1)? & 0x0F, *data.get(2)?, *data.get(3)?),
            _ => return None,
        };
        (key != 0 || asc != 0 || ascq != 0).then_some(Self { key, asc, ascq })
    }

    /// The command did its work after all (NO SENSE, or RECOVERED ERROR: the drive mended what went wrong).
    #[must_use]
    pub fn recovered(self) -> bool {
        self.key <= 1
    }

    /// The drive is busy and the same command is to be sent again a little later: NOT READY with LOGICAL UNIT NOT
    /// READY (becoming ready, format, operation or long write in progress, 4.1.6.2: also a write whose cache is
    /// full) or INSUFFICIENT TIME FOR OPERATION (4.1.9.2: a Group 1 command is retried).
    #[must_use]
    pub fn busy(self) -> bool {
        self.key == 2
            && ((self.asc == 0x04 && matches!(self.ascq, 0x01 | 0x04 | 0x07 | 0x08))
                || self.asc == 0x2E)
    }
}

impl fmt::Display for Sense {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match (self.key, self.asc, self.ascq) {
            (2, 0x04, 0x01) => " (not ready: becoming ready)",
            (2, 0x04, 0x04) => " (not ready: format in progress)",
            (2, 0x04, 0x08) => " (not ready: long write in progress)",
            (2, 0x04, 0x07) => " (not ready: operation in progress)",
            (2, 0x2E, _) => " (not ready: insufficient time for operation)",
            (2, 0x3A, _) => " (not ready: medium not present)",
            (3, 0x0C, _) => " (medium error: write error)",
            (3, 0x73, _) => " (medium error: power calibration area error)",
            (5, 0x21, 0x02) => " (illegal request: invalid address for write)",
            (5, 0x24, _) => " (illegal request: invalid field in CDB)",
            (5, 0x2C, _) => " (illegal request: command sequence error)",
            (5, 0x30, 0x05) => " (illegal request: cannot write medium, incompatible format)",
            (5, 0x72, _) => " (illegal request: session fixation error)",
            (6, 0x28, _) => " (unit attention: medium may have changed)",
            (6, 0x29, _) => " (unit attention: reset)",
            // M9.47: the kind at least, for a code not named here.
            (1, _, _) => " (recovered error)",
            (2, _, _) => " (not ready)",
            (3, _, _) => " (medium error)",
            (4, _, _) => " (hardware error)",
            (5, _, _) => " (illegal request)",
            (6, _, _) => " (unit attention)",
            (0x0B, _, _) => " (aborted command)",
            _ => "",
        };
        write!(
            f,
            "sense {:X}h/{:02X}h/{:02X}h{name}",
            self.key, self.asc, self.ascq
        )
    }
}

/// What READ DISC INFORMATION tells (the Standard Disc Information of Table 376).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiscInformation {
    /// The Disc Status: 0 empty, 1 incomplete (appendable), 2 finalized, 3 others.
    pub status: u8,
    /// The State of Last Session: 0 empty, 1 incomplete, 2 reserved or damaged, 3 complete.
    pub last_session: u8,
    /// The disc is rewritable and the drive writes it.
    pub erasable: bool,
    /// The number of sessions.
    pub sessions: u16,
    /// The first Logical Track number in the last session.
    pub first_track_in_last_session: u16,
    /// The last Logical Track number in the last session.
    pub last_track_in_last_session: u16,
    /// The Last Possible Lead-out Start Address.
    pub last_possible_lead_out: u32,
}

impl DiscInformation {
    /// The Disc Information Block in `data` (at least its first 24 bytes).
    #[must_use]
    pub fn parse(data: &[u8]) -> Option<Self> {
        let byte = |at: usize| data.get(at).copied();
        let pair = |high: usize, low: usize| Some(u16::from_be_bytes([byte(high)?, byte(low)?]));
        let state = byte(2)?;
        Some(Self {
            status: state & 0b11,
            last_session: (state >> 2) & 0b11,
            erasable: state & 0x10 != 0,
            sessions: pair(9, 4)?,
            first_track_in_last_session: pair(10, 5)?,
            last_track_in_last_session: pair(11, 6)?,
            last_possible_lead_out: be32(data, 20)?,
        })
    }
}

/// What READ TRACK INFORMATION tells of one Logical Track (Table 499).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrackInformation {
    /// Its Logical Track number.
    pub track: u16,
    /// The number of its session.
    pub session: u16,
    /// It is reserved (RT).
    pub reserved: bool,
    /// Nothing is written in it (Blank).
    pub blank: bool,
    /// Its Logical Track Start Address.
    pub start: u32,
    /// Its Next Writable Address, when it has one (NWA_V).
    pub next_writable: Option<u32>,
    /// Its Free Blocks.
    pub free_blocks: u32,
    /// Its Logical Track Size.
    pub size: u32,
    /// Its Last Recorded Address, when given (LRA_V).
    pub last_recorded: Option<u32>,
}

impl TrackInformation {
    /// The Track Information Block in `data` (at least its first 34 bytes).
    #[must_use]
    pub fn parse(data: &[u8]) -> Option<Self> {
        let byte = |at: usize| data.get(at).copied();
        let flags = byte(6)?;
        let valid = byte(7)?;
        Some(Self {
            track: u16::from_be_bytes([byte(32)?, byte(2)?]),
            session: u16::from_be_bytes([byte(33)?, byte(3)?]),
            reserved: flags & 0x80 != 0,
            blank: flags & 0x40 != 0,
            start: be32(data, 8)?,
            next_writable: be32(data, 12).filter(|_| valid & 0b01 != 0),
            free_blocks: be32(data, 16)?,
            size: be32(data, 24)?,
            last_recorded: be32(data, 28).filter(|_| valid & 0b10 != 0),
        })
    }
}

/// The big-endian `u32` at `at` of `data`.
fn be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        data.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// The writes of an image of `sectors` sectors, `per_command` at a time (the last one shorter): each as its first
/// sector and how many.
pub fn writes(sectors: u32, per_command: u16) -> impl Iterator<Item = (u32, u16)> {
    let step = u32::from(per_command.max(1));
    (0..sectors.div_ceil(step)).filter_map(move |index| {
        let lba = index.checked_mul(step)?;
        let count = u16::try_from(sectors.checked_sub(lba)?.min(step)).ok()?;
        Some((lba, count))
    })
}

/// The most sectors one WRITE carries (M9.46): 32 (64 KiB, two ECC blocks of a DVD).
pub const MAX_WRITE_SECTORS: u16 = 32;

/// The sectors one WRITE carries to a drive that takes `max_bytes` at a time (its largest transfer, as the system
/// tells it): [`MAX_WRITE_SECTORS`] or fewer, whole ECC blocks (16 sectors) when it takes one, at least one.
#[must_use]
pub fn sectors_per_write(max_bytes: u32) -> u16 {
    let fits = u16::try_from(max_bytes / 2048).unwrap_or(u16::MAX);
    let sectors = fits.min(MAX_WRITE_SECTORS);
    if sectors >= 16 {
        sectors - sectors % 16
    } else {
        sectors.max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ADIP_BYTES, Close, DiscInformation, INCOMPLETE_TRACK, MAX_WRITE_SECTORS, MediaId, Sense,
        TrackInformation, close_track_session, kilobytes_per_second, performance,
        performance_defaults, read_track_information, request_sense, sectors_per_write,
        set_streaming, synchronize_cache, write10, writes,
    };

    /// M9.47: REQUEST SENSE asks for the fixed format's 18 bytes.
    #[test]
    fn request_sense_asks_for_the_fixed_format() {
        assert_eq!(request_sense(), [0x03, 0, 0, 0, 18, 0]);
    }

    /// M9.47: the media ID from the ADIP information (after the four-byte header), as recorders show it; the
    /// layer-structure bits of byte 30 are not part of the revision, and a disc without one has none.
    #[test]
    fn the_media_id_is_read_from_the_adip_information() {
        let mut structure = vec![0u8; usize::from(ADIP_BYTES)];
        structure[1] = 254;
        structure[4..5].copy_from_slice(&[0xE1]);
        structure[4 + 19..4 + 22].copy_from_slice(b"MKM");
        structure[4 + 27..4 + 30].copy_from_slice(b"003");
        structure[4 + 30] = 0b0100_0000;
        let id = MediaId::parse(&structure);
        assert_eq!(
            id,
            Some(MediaId {
                manufacturer: "MKM".to_owned(),
                media_type: "003".to_owned(),
                revision: 0,
            })
        );
        assert_eq!(id.map(|id| id.to_string()).as_deref(), Some("MKM-003-00"));
        structure[4 + 19..4 + 27].copy_from_slice(b"RICOHJPN");
        structure[4 + 27..4 + 30].copy_from_slice(b"D01");
        structure[4 + 30] = 0x45;
        assert_eq!(
            MediaId::parse(&structure)
                .map(|id| id.to_string())
                .as_deref(),
            Some("RICOHJPN-D01-05")
        );
        structure[4 + 19..4 + 27].fill(0);
        assert_eq!(MediaId::parse(&structure), None);
        assert_eq!(MediaId::parse(&structure[..30]), None);
    }

    /// M9.47: a code without a name of its own still says its kind.
    #[test]
    fn sense_names_say_at_least_the_kind() {
        let reset = Sense {
            key: 2,
            asc: 0x04,
            ascq: 0x01,
        };
        assert!(reset.busy());
        assert_eq!(
            reset.to_string(),
            "sense 2h/04h/01h (not ready: becoming ready)"
        );
        let hardware = Sense {
            key: 4,
            asc: 0x09,
            ascq: 0x01,
        };
        assert_eq!(hardware.to_string(), "sense 4h/09h/01h (hardware error)");
        let other = Sense {
            key: 0x0E,
            asc: 0,
            ascq: 0,
        };
        assert_eq!(other.to_string(), "sense Eh/00h/00h");
    }

    /// The CDBs as MMC-5's tables lay them out.
    #[test]
    fn the_commands_are_laid_out_as_mmc5_says() {
        assert_eq!(
            write10(0x0012_3456, 32),
            [0x2A, 0, 0x00, 0x12, 0x34, 0x56, 0, 0, 32, 0]
        );
        assert_eq!(
            synchronize_cache(true),
            [0x35, 0b10, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(synchronize_cache(false)[1], 0);
        assert_eq!(
            close_track_session(true, Close::Track, 1),
            [0x5B, 1, 0b001, 0, 0, 1, 0, 0, 0, 0]
        );
        assert_eq!(
            close_track_session(false, Close::FinalizeMinimalRadius, 0),
            [0x5B, 0, 0b101, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            read_track_information(INCOMPLETE_TRACK, 48),
            [0x52, 1, 0, 0, 0, 255, 0, 0, 48, 0]
        );
        assert_eq!(set_streaming(), [0xB6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 28, 0]);
    }

    /// 2.4x of a DVD (1632 sectors a second) is 3342 kB/s, asked from sector 0 to the image's last, each second.
    #[test]
    fn the_write_speed_is_asked_in_kilobytes_a_second() {
        assert_eq!(kilobytes_per_second(1632), 3342);
        let descriptor = performance(3_832_288, 3342);
        assert_eq!(descriptor.get(0..8), Some(&[0u8; 8][..]));
        assert_eq!(descriptor.get(8..12), Some(&3_832_288u32.to_be_bytes()[..]));
        assert_eq!(descriptor.get(12..16), Some(&3342u32.to_be_bytes()[..]));
        assert_eq!(descriptor.get(16..20), Some(&1000u32.to_be_bytes()[..]));
        assert_eq!(descriptor.get(20..24), Some(&3342u32.to_be_bytes()[..]));
        assert_eq!(descriptor.get(24..28), Some(&1000u32.to_be_bytes()[..]));
    }

    /// M9.48: the descriptor that restores the drive's own speeds has RDD (byte 0, bit 2) set and nothing else.
    #[test]
    fn the_drive_defaults_are_asked_by_the_rdd_bit() {
        let descriptor = performance_defaults();
        assert_eq!(descriptor.first(), Some(&0b100));
        assert!(descriptor.iter().skip(1).all(|byte| *byte == 0));
    }

    /// Sense data in either format; a full cache or a long write says "busy", a write error does not.
    #[test]
    fn sense_data_tells_a_busy_drive_from_a_failure() {
        let mut fixed = [0u8; 18];
        fixed[0] = 0x70;
        fixed[2] = 0x02;
        fixed[12] = 0x04;
        fixed[13] = 0x08;
        let busy = Sense::parse(&fixed);
        assert_eq!(
            busy,
            Some(Sense {
                key: 2,
                asc: 4,
                ascq: 8
            })
        );
        assert!(busy.is_some_and(Sense::busy));
        assert_eq!(
            busy.map(|sense| sense.to_string()).as_deref(),
            Some("sense 2h/04h/08h (not ready: long write in progress)")
        );
        let failed = Sense::parse(&[0x72, 0x03, 0x0C, 0x00]);
        assert!(failed.is_some_and(|sense| !sense.busy() && !sense.recovered()));
        assert_eq!(Sense::parse(&[0u8; 18]), None);
        let mut nothing = [0u8; 18];
        nothing[0] = 0x70;
        assert_eq!(Sense::parse(&nothing), None);
        assert_eq!(Sense::parse(&[0x70, 0, 0x02]), None);
    }

    /// A blank DVD+R DL's disc and its invisible track, and a track reserved and part written.
    #[test]
    fn disc_and_track_information_are_read() {
        let mut disc = [0u8; 34];
        disc[2] = 0b0001_0000 | 0b0100 | 0b01;
        disc[3] = 1;
        disc[4] = 1;
        disc[5] = 1;
        disc[6] = 2;
        disc[20..24].copy_from_slice(&3_832_320u32.to_be_bytes());
        assert_eq!(
            DiscInformation::parse(&disc),
            Some(DiscInformation {
                status: 1,
                last_session: 1,
                erasable: true,
                sessions: 1,
                first_track_in_last_session: 1,
                last_track_in_last_session: 2,
                last_possible_lead_out: 3_832_320,
            })
        );
        assert_eq!(DiscInformation::parse(&disc[..20]), None);

        let mut track = [0u8; 48];
        track[2] = 1;
        track[3] = 1;
        track[6] = 0x80;
        track[7] = 0b11;
        track[12..16].copy_from_slice(&613_376u32.to_be_bytes());
        track[16..20].copy_from_slice(&3_218_944u32.to_be_bytes());
        track[24..28].copy_from_slice(&3_832_320u32.to_be_bytes());
        track[28..32].copy_from_slice(&613_375u32.to_be_bytes());
        assert_eq!(
            TrackInformation::parse(&track),
            Some(TrackInformation {
                track: 1,
                session: 1,
                reserved: true,
                blank: false,
                start: 0,
                next_writable: Some(613_376),
                free_blocks: 3_218_944,
                size: 3_832_320,
                last_recorded: Some(613_375),
            })
        );
        track[7] = 0;
        assert_eq!(
            TrackInformation::parse(&track).map(|info| (info.next_writable, info.last_recorded)),
            Some((None, None))
        );
        assert_eq!(TrackInformation::parse(&track[..30]), None);
    }

    /// The writes cover the image once, the last one shorter.
    #[test]
    fn the_writes_cover_the_image_once() {
        let all: Vec<(u32, u16)> = writes(70, 32).collect();
        assert_eq!(all, vec![(0, 32), (32, 32), (64, 6)]);
        assert_eq!(writes(64, 32).count(), 2);
        assert_eq!(writes(0, 32).count(), 0);
        let total: u64 = writes(3_832_289, 32)
            .map(|(_, count)| u64::from(count))
            .sum();
        assert_eq!(total, 3_832_289);
    }

    /// M9.46: a WRITE carries 64 KiB, less to a drive that takes less (whole ECC blocks while it can), never 0.
    #[test]
    fn a_write_carries_what_the_drive_takes() {
        assert_eq!(sectors_per_write(1 << 20), MAX_WRITE_SECTORS);
        assert_eq!(sectors_per_write(65_536), 32);
        assert_eq!(sectors_per_write(61_440), 16);
        assert_eq!(sectors_per_write(32_768), 16);
        assert_eq!(sectors_per_write(16_384), 8);
        assert_eq!(sectors_per_write(0), 1);
    }
}
