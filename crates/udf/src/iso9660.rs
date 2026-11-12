//! ISO 9660 (ECMA-119) descriptor builders for the DVD-Video bridge.

use crate::desc;

/// ISO 9660 logical sector size.
pub const SECTOR_SIZE: usize = 2048;

/// Primary volume descriptor sector.
pub const PVD_SECTOR: u32 = 16;
/// Volume descriptor set terminator sector.
pub const TERMINATOR_SECTOR: u32 = 17;
/// Little-endian path table sector.
pub const PATH_TABLE_L_SECTOR: u32 = 257;
/// Big-endian path table sector.
pub const PATH_TABLE_M_SECTOR: u32 = 258;
/// Root directory extent sector.
pub const ROOT_DIR_SECTOR: u32 = 259;
/// AUDIO_TS directory extent sector.
pub const AUDIO_DIR_SECTOR: u32 = 260;
/// VIDEO_TS directory extent sector.
pub const VIDEO_DIR_SECTOR: u32 = 261;

/// Offset of the root directory record in the PVD used by this bridge.
pub const PVD_ROOT_RECORD_OFFSET: usize = 156;

const PVD_TYPE_OFFSET: usize = 0;
const PVD_ID_OFFSET: usize = 1;
const PVD_VERSION_OFFSET: usize = 6;
const PVD_SYSTEM_ID_OFFSET: usize = 8;
const PVD_VOLUME_ID_OFFSET: usize = 40;
const PVD_VOLUME_SPACE_SIZE_OFFSET: usize = 80;
const PVD_VOLUME_SET_SIZE_OFFSET: usize = 120;
const PVD_SEQUENCE_NUMBER_OFFSET: usize = 124;
const PVD_BLOCK_SIZE_OFFSET: usize = 128;
const PVD_PATH_TABLE_SIZE_OFFSET: usize = 132;
const PVD_L_PATH_OFFSET: usize = 140;
const PVD_M_PATH_OFFSET: usize = 148;
const PVD_VOLUME_SET_ID_OFFSET: usize = 190;
const PVD_PUBLISHER_ID_OFFSET: usize = 318;
const PVD_PREPARER_ID_OFFSET: usize = 446;
const PVD_APPLICATION_ID_OFFSET: usize = 574;
const PVD_COPYRIGHT_FILE_ID_OFFSET: usize = 702;
const PVD_ABSTRACT_FILE_ID_OFFSET: usize = 739;
const PVD_BIBLIOGRAPHIC_FILE_ID_OFFSET: usize = 776;
const PVD_CREATION_DATE_OFFSET: usize = 813;
const PVD_MODIFICATION_DATE_OFFSET: usize = 830;
const PVD_EXPIRATION_DATE_OFFSET: usize = 847;
const PVD_EFFECTIVE_DATE_OFFSET: usize = 864;
const PVD_FILE_STRUCTURE_VERSION_OFFSET: usize = 881;

/// Parameters for the primary volume descriptor.
#[derive(Debug, Clone, Copy)]
pub struct PvdParams<'a> {
    /// Sanitized volume identifier.
    pub volume_id: &'a str,
    /// Total number of sectors in the image.
    pub volume_space_size: u32,
    /// Size in bytes of one path table.
    pub path_table_size: u32,
    /// Absolute sector of the root directory extent.
    pub root_extent: u32,
    /// Size in bytes of the root directory extent.
    pub root_size: u64,
    /// UNIX creation time.
    pub creation_time: i64,
    /// UNIX modification time.
    pub modification_time: i64,
}

/// One directory entry to be written into an ISO 9660 directory extent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Display name without the `;1` version suffix.
    pub name: String,
    /// Absolute extent location.
    pub extent: u32,
    /// Data length in bytes.
    pub size: u64,
    /// Whether the entry is a directory.
    pub is_dir: bool,
}

/// One path table entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathEntry {
    /// Directory identifier bytes.
    pub name: Vec<u8>,
    /// Absolute extent location.
    pub extent: u32,
    /// Parent directory number.
    pub parent: u16,
}

/// Sanitize a volume identifier to ISO 9660 d-characters.
pub fn sanitize_volume_id(label: &str) -> String {
    let mut out = sanitize_a_d_chars(label, 32);
    if out.is_empty() {
        out = String::from("DISCCRAFT");
    }
    out
}

/// Sanitize a directory name to ISO 9660 interchange level 1.
pub fn sanitize_dir_name(name: &str) -> String {
    let mut out = sanitize_a_d_chars(name, 8);
    if out.is_empty() {
        out = String::from("DIR");
    }
    out
}

/// Sanitize a file name to ISO 9660 interchange level 1, without `;1`.
pub fn sanitize_file_name(name: &str) -> String {
    let upper = name.to_ascii_uppercase();
    let (base, ext) = match upper.rfind('.') {
        Some(pos) => {
            let base = upper.get(0..pos).unwrap_or_default();
            let ext = upper.get(pos.saturating_add(1)..).unwrap_or_default();
            (base, ext)
        }
        None => (upper.as_str(), ""),
    };

    let mut base = sanitize_a_d_chars(base, 8);
    if base.is_empty() {
        base = String::from("FILE");
    }

    let ext = sanitize_a_d_chars(ext, 3);
    if ext.is_empty() {
        base
    } else {
        format!("{base}.{ext}")
    }
}

/// Build an ECMA-119 7-byte directory record date.
pub fn ecma_date(unix_time: i64) -> [u8; 7] {
    let ts = desc::timestamp(unix_time);
    let year = read_u16(&ts, 2).unwrap_or(1970);
    let month = read_u8(&ts, 4).unwrap_or(1);
    let day = read_u8(&ts, 5).unwrap_or(1);
    let hour = read_u8(&ts, 6).unwrap_or_default();
    let minute = read_u8(&ts, 7).unwrap_or_default();
    let second = read_u8(&ts, 8).unwrap_or_default();

    let year_byte = if year >= 1900 {
        let v = year.saturating_sub(1900);
        if v > 255 { 255 } else { v as u8 }
    } else {
        0
    };

    [year_byte, month, day, hour, minute, second, 0]
}

/// Build a primary volume descriptor sector.
pub fn pvd(params: &PvdParams<'_>) -> [u8; SECTOR_SIZE] {
    let mut out = [0u8; SECTOR_SIZE];

    put_u8(&mut out, PVD_TYPE_OFFSET, 1);
    put_bytes(&mut out, PVD_ID_OFFSET, b"CD001");
    put_u8(&mut out, PVD_VERSION_OFFSET, 1);

    put_identifier(&mut out, PVD_SYSTEM_ID_OFFSET, 32, "");
    put_identifier(&mut out, PVD_VOLUME_ID_OFFSET, 32, params.volume_id);

    put_u32_le(
        &mut out,
        PVD_VOLUME_SPACE_SIZE_OFFSET,
        params.volume_space_size,
    );
    put_u32_be(
        &mut out,
        PVD_VOLUME_SPACE_SIZE_OFFSET + 4,
        params.volume_space_size,
    );

    put_u16_le(&mut out, PVD_VOLUME_SET_SIZE_OFFSET, 1);
    put_u16_be(&mut out, PVD_VOLUME_SET_SIZE_OFFSET + 2, 1);

    put_u16_le(&mut out, PVD_SEQUENCE_NUMBER_OFFSET, 1);
    put_u16_be(&mut out, PVD_SEQUENCE_NUMBER_OFFSET + 2, 1);

    put_u16_le(&mut out, PVD_BLOCK_SIZE_OFFSET, 2048);
    put_u16_be(&mut out, PVD_BLOCK_SIZE_OFFSET + 2, 2048);

    put_u32_le(&mut out, PVD_PATH_TABLE_SIZE_OFFSET, params.path_table_size);
    put_u32_be(
        &mut out,
        PVD_PATH_TABLE_SIZE_OFFSET + 4,
        params.path_table_size,
    );

    put_u32_le(&mut out, PVD_L_PATH_OFFSET, PATH_TABLE_L_SECTOR);
    put_u32_be(&mut out, PVD_M_PATH_OFFSET, PATH_TABLE_M_SECTOR);

    let date = ecma_date(params.creation_time);
    let root = root_directory_record(params.root_extent, params.root_size, date);
    put_bytes(&mut out, PVD_ROOT_RECORD_OFFSET, &root);

    put_identifier(&mut out, PVD_VOLUME_SET_ID_OFFSET, 128, "");
    put_identifier(&mut out, PVD_PUBLISHER_ID_OFFSET, 128, "");
    put_identifier(&mut out, PVD_PREPARER_ID_OFFSET, 128, "");
    put_identifier(&mut out, PVD_APPLICATION_ID_OFFSET, 128, "DISCCRAFT");
    put_identifier(&mut out, PVD_COPYRIGHT_FILE_ID_OFFSET, 37, "");
    put_identifier(&mut out, PVD_ABSTRACT_FILE_ID_OFFSET, 37, "");
    put_identifier(&mut out, PVD_BIBLIOGRAPHIC_FILE_ID_OFFSET, 37, "");

    put_date_string(&mut out, PVD_CREATION_DATE_OFFSET, params.creation_time);
    put_date_string(
        &mut out,
        PVD_MODIFICATION_DATE_OFFSET,
        params.modification_time,
    );
    put_zero_date_string(&mut out, PVD_EXPIRATION_DATE_OFFSET);
    put_zero_date_string(&mut out, PVD_EFFECTIVE_DATE_OFFSET);

    put_u8(&mut out, PVD_FILE_STRUCTURE_VERSION_OFFSET, 1);
    out
}

/// Build a volume descriptor set terminator sector.
pub fn terminator() -> [u8; SECTOR_SIZE] {
    let mut out = [0u8; SECTOR_SIZE];
    put_u8(&mut out, 0, 255);
    put_bytes(&mut out, 1, b"CD001");
    put_u8(&mut out, 6, 1);
    out
}

/// Build an ISO 9660 path table.
pub fn path_table(entries: &[PathEntry], big_endian: bool) -> Vec<u8> {
    let mut out = Vec::new();

    for entry in entries {
        let name_len = entry.name.len().min(255);
        let mut len = 8usize.saturating_add(name_len);
        if len % 2 != 0 {
            len = len.saturating_add(1);
        }

        let mut buf = vec![0u8; len];
        put_u8(&mut buf, 0, name_len as u8);

        if big_endian {
            put_u32_be(&mut buf, 2, entry.extent);
            put_u16_be(&mut buf, 6, entry.parent);
        } else {
            put_u32_le(&mut buf, 2, entry.extent);
            put_u16_le(&mut buf, 6, entry.parent);
        }

        let name_end = 8usize.saturating_add(name_len);
        if let Some(dst) = buf.get_mut(8..name_end)
            && let Some(src) = entry.name.get(0..name_len)
        {
            dst.copy_from_slice(src);
        }

        out.extend_from_slice(&buf);
    }

    out
}

/// Build a sector-aligned ISO 9660 directory extent.
pub fn directory_records(
    self_extent: u32,
    self_size: u64,
    parent_extent: u32,
    parent_size: u64,
    entries: &[DirEntry],
    date: [u8; 7],
) -> Vec<u8> {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));

    let mut out = Vec::new();
    let mut cursor = 0usize;

    push_record(
        &mut out,
        &mut cursor,
        &directory_record(".", self_extent, self_size, true, date),
    );
    push_record(
        &mut out,
        &mut cursor,
        &directory_record("..", parent_extent, parent_size, true, date),
    );

    for entry in sorted {
        push_record(
            &mut out,
            &mut cursor,
            &directory_record(&entry.name, entry.extent, entry.size, entry.is_dir, date),
        );
    }

    let rem = out.len() % SECTOR_SIZE;
    if rem != 0 {
        let pad = SECTOR_SIZE.saturating_sub(rem);
        out.resize(out.len().saturating_add(pad), 0);
    }

    out
}

/// Build one ISO 9660 directory record.
pub fn directory_record(
    name: &str,
    extent: u32,
    size: u64,
    is_dir: bool,
    date: [u8; 7],
) -> Vec<u8> {
    let record_name = if is_dir || name.ends_with(";1") {
        String::from(name)
    } else {
        format!("{name};1")
    };

    let mut name_bytes: Vec<u8> = match record_name.as_str() {
        "." => vec![0u8],
        ".." => vec![1u8],
        _ => record_name.as_bytes().to_vec(),
    };

    if name_bytes.len() > 221 {
        name_bytes.truncate(221);
    }

    let mut len = 33usize.saturating_add(name_bytes.len());
    if !len.is_multiple_of(2) {
        len = len.saturating_add(1);
    }
    if len > 255 {
        len = 255;
    }

    // The record may carry one padding byte after the name; the name length is the name's own length.
    let name_len = name_bytes.len();
    let mut buf = vec![0u8; len];

    put_u8(&mut buf, 0, len as u8);
    put_u32_le(&mut buf, 2, extent);
    put_u32_be(&mut buf, 6, extent);

    let iso_size = if size > u32::MAX as u64 {
        u32::MAX
    } else {
        size as u32
    };
    put_u32_le(&mut buf, 10, iso_size);
    put_u32_be(&mut buf, 14, iso_size);

    if let Some(dst) = buf.get_mut(18..25) {
        dst.copy_from_slice(&date);
    }

    put_u8(&mut buf, 25, if is_dir { 0x02 } else { 0 });
    put_u16_le(&mut buf, 28, 1);
    put_u16_be(&mut buf, 30, 1);
    put_u8(&mut buf, 32, name_len as u8);

    let name_end = 33usize.saturating_add(name_len);
    if let Some(dst) = buf.get_mut(33..name_end)
        && let Some(src) = name_bytes.get(0..name_len)
    {
        dst.copy_from_slice(src);
    }

    buf
}

/// Build the 34-byte root directory record stored in the PVD.
pub fn root_directory_record(extent: u32, size: u64, date: [u8; 7]) -> [u8; 34] {
    let rec = directory_record(".", extent, size, true, date);
    let mut out = [0u8; 34];
    if let Some(dst) = out.get_mut(0..34)
        && let Some(src) = rec.get(0..34)
    {
        dst.copy_from_slice(src);
    }
    out
}

/// Sanitize text to ISO 9660 A/D characters.
fn sanitize_a_d_chars(text: &str, max: usize) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if out.len() >= max {
            break;
        }
        let up = ch.to_ascii_uppercase();
        if up.is_ascii_uppercase() || up.is_ascii_digit() || up == '_' {
            out.push(up);
        } else {
            out.push('_');
        }
    }
    out
}

/// Write a space-padded ISO 9660 identifier field.
fn put_identifier(buf: &mut [u8], offset: usize, len: usize, text: &str) {
    let mut field = vec![b' '; len];
    let clean = sanitize_a_d_chars(text, len);
    for (slot, ch) in field.iter_mut().zip(clean.chars()) {
        *slot = ch as u8;
    }
    put_bytes(buf, offset, &field);
}

/// Write a 17-byte ISO 9660 date string.
fn put_date_string(buf: &mut [u8], offset: usize, unix_time: i64) {
    let ts = desc::timestamp(unix_time);
    let year = read_u16(&ts, 2).unwrap_or(1970);
    let month = read_u8(&ts, 4).unwrap_or(1);
    let day = read_u8(&ts, 5).unwrap_or(1);
    let hour = read_u8(&ts, 6).unwrap_or_default();
    let minute = read_u8(&ts, 7).unwrap_or_default();
    let second = read_u8(&ts, 8).unwrap_or_default();

    put_four(buf, offset, year);
    put_two(buf, offset.saturating_add(4), month);
    put_two(buf, offset.saturating_add(6), day);
    put_two(buf, offset.saturating_add(8), hour);
    put_two(buf, offset.saturating_add(10), minute);
    put_two(buf, offset.saturating_add(12), second);
    put_two(buf, offset.saturating_add(14), 0);
    put_u8(buf, offset.saturating_add(16), 0);
}

/// Write a zero-filled 17-byte ISO 9660 date string.
fn put_zero_date_string(buf: &mut [u8], offset: usize) {
    for i in 0..16 {
        put_u8(buf, offset.saturating_add(i), b'0');
    }
    put_u8(buf, offset.saturating_add(16), 0);
}

/// Write four decimal digits.
fn put_four(buf: &mut [u8], offset: usize, value: u16) {
    let v = value.min(9999);
    put_u8(buf, offset, b'0' + ((v / 1000) % 10) as u8);
    put_u8(buf, offset.saturating_add(1), b'0' + ((v / 100) % 10) as u8);
    put_u8(buf, offset.saturating_add(2), b'0' + ((v / 10) % 10) as u8);
    put_u8(buf, offset.saturating_add(3), b'0' + (v % 10) as u8);
}

/// Write two decimal digits.
fn put_two(buf: &mut [u8], offset: usize, value: u8) {
    put_u8(buf, offset, b'0' + (value / 10));
    put_u8(buf, offset.saturating_add(1), b'0' + (value % 10));
}

/// Append a directory record, padding to a sector boundary if needed.
fn push_record(out: &mut Vec<u8>, cursor: &mut usize, rec: &[u8]) {
    let rem = *cursor % SECTOR_SIZE;
    if rem.saturating_add(rec.len()) > SECTOR_SIZE {
        let pad = SECTOR_SIZE.saturating_sub(rem);
        out.resize(out.len().saturating_add(pad), 0);
        *cursor = cursor.saturating_add(pad);
    }
    out.extend_from_slice(rec);
    *cursor = cursor.saturating_add(rec.len());
}

/// Write bytes at an offset when the destination range exists.
fn put_bytes(buf: &mut [u8], offset: usize, data: &[u8]) {
    let Some(end) = offset.checked_add(data.len()) else {
        return;
    };
    let Some(dst) = buf.get_mut(offset..end) else {
        return;
    };
    dst.copy_from_slice(data);
}

/// Write one byte at an offset when the destination exists.
fn put_u8(buf: &mut [u8], offset: usize, value: u8) {
    let Some(end) = offset.checked_add(1) else {
        return;
    };
    let Some(dst) = buf.get_mut(offset..end) else {
        return;
    };
    dst.copy_from_slice(&[value]);
}

/// Write a little-endian u16 at an offset when the destination exists.
fn put_u16_le(buf: &mut [u8], offset: usize, value: u16) {
    put_bytes(buf, offset, &value.to_le_bytes());
}

/// Write a big-endian u16 at an offset when the destination exists.
fn put_u16_be(buf: &mut [u8], offset: usize, value: u16) {
    put_bytes(buf, offset, &value.to_be_bytes());
}

/// Write a little-endian u32 at an offset when the destination exists.
fn put_u32_le(buf: &mut [u8], offset: usize, value: u32) {
    put_bytes(buf, offset, &value.to_le_bytes());
}

/// Write a big-endian u32 at an offset when the destination exists.
fn put_u32_be(buf: &mut [u8], offset: usize, value: u32) {
    put_bytes(buf, offset, &value.to_be_bytes());
}

/// Read one byte as an option.
fn read_u8(buf: &[u8], offset: usize) -> Option<u8> {
    buf.get(offset).copied()
}

/// Read a little-endian u16 as an option.
fn read_u16(buf: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let s = buf.get(offset..end)?;
    let mut a = [0u8; 2];
    a.copy_from_slice(s);
    Some(u16::from_le_bytes(a))
}

#[cfg(test)]
mod tests {
    use super::{
        DirEntry, PathEntry, PvdParams, directory_record, directory_records, ecma_date, path_table,
        pvd, sanitize_dir_name, sanitize_file_name, sanitize_volume_id, terminator,
    };

    fn le16(buf: &[u8], off: usize) -> u16 {
        u16::from_le_bytes([buf[off], buf[off + 1]])
    }

    fn be16(buf: &[u8], off: usize) -> u16 {
        u16::from_be_bytes([buf[off], buf[off + 1]])
    }

    fn le32(buf: &[u8], off: usize) -> u32 {
        u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
    }

    fn be32(buf: &[u8], off: usize) -> u32 {
        u32::from_be_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn pvd_offsets() {
        let params = PvdParams {
            volume_id: "DISCCRAFT",
            volume_space_size: 1000,
            path_table_size: 16,
            root_extent: 259,
            root_size: 34,
            creation_time: 0,
            modification_time: 0,
        };
        let p = pvd(&params);

        assert_eq!(p[0], 1);
        assert_eq!(&p[1..6], b"CD001");
        assert_eq!(p[6], 1);

        assert_eq!(p[8], b' ');
        assert_eq!(p[40], b'D');
        assert_eq!(p[48], b'T');
        assert_eq!(p[49], b' ');

        assert_eq!(le32(&p, 80), 1000);
        assert_eq!(be32(&p, 84), 1000);

        assert_eq!(le16(&p, 120), 1);
        assert_eq!(be16(&p, 122), 1);
        assert_eq!(le16(&p, 124), 1);
        assert_eq!(be16(&p, 126), 1);
        assert_eq!(le16(&p, 128), 2048);
        assert_eq!(be16(&p, 130), 2048);

        assert_eq!(le32(&p, 132), 16);
        assert_eq!(be32(&p, 136), 16);
        assert_eq!(le32(&p, 140), 257);
        assert_eq!(be32(&p, 148), 258);

        assert_eq!(p[156], 34);
        assert_eq!(le32(&p, 158), 259);
        assert_eq!(be32(&p, 162), 259);
        assert_eq!(le32(&p, 166), 34);
        assert_eq!(be32(&p, 170), 34);
        assert_eq!(p[181], 0x02);

        assert_eq!(p[190], b' ');
        assert_eq!(p[318], b' ');
        assert_eq!(p[446], b' ');
        assert_eq!(p[574], b'D');
        assert_eq!(p[702], b' ');
        assert_eq!(p[739], b' ');
        assert_eq!(p[776], b' ');

        assert_eq!(&p[813..829], b"1970010100000000");
        assert_eq!(p[829], 0);
        assert_eq!(&p[830..846], b"1970010100000000");
        assert_eq!(p[846], 0);
        assert_eq!(&p[847..863], b"0000000000000000");
        assert_eq!(p[863], 0);
        assert_eq!(&p[864..880], b"0000000000000000");
        assert_eq!(p[880], 0);
        assert_eq!(p[881], 1);
    }

    #[test]
    fn terminator_sector() {
        let t = terminator();
        assert_eq!(t[0], 255);
        assert_eq!(&t[1..6], b"CD001");
        assert_eq!(t[6], 1);
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn path_table_little_and_big_endian() {
        let entries = vec![
            PathEntry {
                name: Vec::new(),
                extent: 259,
                parent: 1,
            },
            PathEntry {
                name: b"VIDEO_TS".to_vec(),
                extent: 261,
                parent: 1,
            },
        ];

        let l = path_table(&entries, false);
        assert_eq!(l.len(), 24);
        assert_eq!(l[0], 0);
        assert_eq!(l[1], 0);
        assert_eq!(le32(&l, 2), 259);
        assert_eq!(le16(&l, 6), 1);

        assert_eq!(l[8], 8);
        assert_eq!(l[9], 0);
        assert_eq!(le32(&l, 10), 261);
        assert_eq!(le16(&l, 14), 1);
        assert_eq!(&l[16..24], b"VIDEO_TS");

        let m = path_table(&entries, true);
        assert_eq!(be32(&m, 2), 259);
        assert_eq!(be16(&m, 6), 1);
        assert_eq!(be32(&m, 10), 261);
        assert_eq!(be16(&m, 14), 1);
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn directory_record_even_name() {
        let date = ecma_date(0);
        let rec = directory_record("VIDEO_TS.IFO;1", 262, 1024, false, date);

        assert_eq!(rec.len(), 48);
        assert_eq!(rec[0], 48);
        assert_eq!(rec[1], 0);
        assert_eq!(le32(&rec, 2), 262);
        assert_eq!(be32(&rec, 6), 262);
        assert_eq!(le32(&rec, 10), 1024);
        assert_eq!(be32(&rec, 14), 1024);
        assert_eq!(&rec[18..25], &date[..]);
        assert_eq!(rec[25], 0);
        assert_eq!(rec[26], 0);
        assert_eq!(rec[27], 0);
        assert_eq!(le16(&rec, 28), 1);
        assert_eq!(be16(&rec, 30), 1);
        assert_eq!(rec[32], 14);
        assert_eq!(&rec[33..47], b"VIDEO_TS.IFO;1");
        assert_eq!(rec[47], 0);
    }

    #[test]
    fn directory_record_directory() {
        let date = ecma_date(0);
        let rec = directory_record("VIDEO_TS", 261, 2048, true, date);

        assert_eq!(rec.len(), 42);
        assert_eq!(rec[0], 42);
        assert_eq!(rec[25], 0x02);
        assert_eq!(rec[32], 8);
        assert_eq!(&rec[33..41], b"VIDEO_TS");
        assert_eq!(rec[41], 0);
    }

    #[test]
    fn directory_records_pads_sector() {
        let date = ecma_date(0);
        let entries = vec![DirEntry {
            name: String::from("VIDEO_TS.IFO"),
            extent: 262,
            size: 1024,
            is_dir: false,
        }];

        let data = directory_records(259, 34, 259, 34, &entries, date);
        assert_eq!(data.len(), 2048);
        assert_eq!(data[0], 34);
        assert_eq!(data[34], 34);
        assert_eq!(data[68], 48);
        assert_eq!(data[68 + 32], 14);
    }

    #[test]
    fn sanitize_functions() {
        assert_eq!(sanitize_volume_id("My Disc"), "MY_DISC");
        assert_eq!(sanitize_dir_name("audio_ts"), "AUDIO_TS");
        assert_eq!(sanitize_file_name("video_ts.ifo"), "VIDEO_TS.IFO");
        assert_eq!(sanitize_file_name("no_ext"), "NO_EXT");
    }

    #[test]
    fn ecma_date_epoch() {
        assert_eq!(ecma_date(0), [70, 1, 1, 0, 0, 0, 0]);
    }
}
