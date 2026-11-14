//! UDF 1.02 descriptor builders.

const CS0_IDENTIFIER: &[u8] = b"OSTA Compressed Unicode";
const UDF_SUFFIX: [u8; 8] = [0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
/// The domain identifier's suffix (UDF 2.1.5.3): UDF 1.02, then the domain flags: hard and soft write-protected
/// (a pressed or finalised disc is read-only), as Encore writes it.
const DOMAIN_SUFFIX: [u8; 8] = [0x02, 0x01, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00];

/// A byte extent described by a partition-relative logical block number and a length in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extent {
    /// Partition-relative logical block number.
    pub lbn: u32,
    /// Length in bytes.
    pub len: u32,
}

/// A file identifier descriptor input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fid {
    /// File name, or `None` for the parent directory entry.
    pub name: Option<String>,
    /// Whether the referenced file entry is a directory.
    pub is_dir: bool,
    /// Logical block number of the referenced file entry.
    pub icb_lbn: u32,
}

/// CRC-16 used by UDF descriptor tags.
///
/// Polynomial `0x1021`, initial value `0`, MSB first, no final XOR.
pub fn crc_itu(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for byte in data {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Finish a descriptor tag at the beginning of `buf`.
///
/// Writes the 16-byte tag, computes the body CRC over `buf[16..16 + body_len]`,
/// and finally computes the tag checksum.
pub fn finish_tag(buf: &mut [u8], tag_id: u16, location: u32, body_len: usize) {
    finish_tag_version(buf, tag_id, 2, location, body_len);
}

/// Like [`finish_tag`] with an explicit descriptor version (2 for NSR02 / UDF 1.02, 3 for NSR03 / UDF 2.00 and later).
pub fn finish_tag_version(
    buf: &mut [u8],
    tag_id: u16,
    version: u16,
    location: u32,
    body_len: usize,
) {
    put_u16(buf, 0, tag_id);
    put_u16(buf, 2, version);
    put_u8(buf, 4, 0);
    put_u8(buf, 5, 0);
    put_u16(buf, 6, 0);

    let crc = match 16usize.checked_add(body_len) {
        Some(end) => match buf.get(16..end) {
            Some(body) => crc_itu(body),
            None => 0,
        },
        None => 0,
    };
    put_u16(buf, 8, crc);

    let crc_len = if body_len > u16::MAX as usize {
        u16::MAX
    } else {
        body_len as u16
    };
    put_u16(buf, 10, crc_len);
    put_u32(buf, 12, location);

    let mut sum = 0u32;
    if let Some(s) = buf.get(0..4) {
        for b in s {
            sum = sum.wrapping_add(u32::from(*b));
        }
    }
    if let Some(s) = buf.get(5..16) {
        for b in s {
            sum = sum.wrapping_add(u32::from(*b));
        }
    }
    put_u8(buf, 4, (sum & 0xFF) as u8);
}

/// Build a dstring field of `field_len` bytes in OSTA CS0 (UDF 2.1.1, 2.1.3).
///
/// Byte 0 is the compression ID: `8` (one byte a character) when every character is in Latin-1 (U+0000 to U+00FF),
/// else `16` (UCS-2, two bytes a character, big-endian), so a Japanese disc name keeps its characters; a character
/// outside the Basic Multilingual Plane becomes `_`. Characters that do not fit are dropped, the field is zero padded,
/// and the last byte is the number of used bytes including the compression byte. Empty text produces an all-zero
/// field.
pub fn dstring(text: &str, field_len: usize) -> Vec<u8> {
    if field_len == 0 {
        return Vec::new();
    }
    if text.is_empty() {
        return vec![0u8; field_len];
    }
    if field_len == 1 {
        return vec![1u8];
    }

    let mut out = vec![0u8; field_len];
    // Room for characters: all but the compression byte and the length byte.
    let room = field_len - 2;
    let latin1 = text.chars().all(|ch| u32::from(ch) <= 0xFF);
    let mut used = 1usize;
    if latin1 {
        put_bytes(&mut out, 0, &[8]);
        for (index, ch) in text.chars().take(room).enumerate() {
            let byte = u8::try_from(u32::from(ch)).unwrap_or(b'_');
            put_bytes(&mut out, 1 + index, &[byte]);
            used = used.saturating_add(1);
        }
    } else {
        put_bytes(&mut out, 0, &[16]);
        for (index, ch) in text.chars().take(room / 2).enumerate() {
            let unit = u16::try_from(u32::from(ch)).unwrap_or(u16::from(b'_'));
            put_bytes(&mut out, 1 + 2 * index, &unit.to_be_bytes());
            used = used.saturating_add(2);
        }
    }

    let len_byte = u8::try_from(used).unwrap_or(u8::MAX);
    put_bytes(&mut out, field_len - 1, &[len_byte]);
    out
}

/// Build a file identifier field.
///
/// This is a compression byte `8` followed by ASCII characters, with non-ASCII
/// characters replaced by `_`. There is no length byte and no padding.
pub fn fid_identifier(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.chars().count().saturating_add(1));
    out.push(8);
    for ch in text.chars() {
        out.push(if ch.is_ascii() { ch as u8 } else { b'_' });
    }
    out
}

/// Build a 12-byte UDF timestamp from UNIX seconds.
///
/// The timestamp type is `1`, time zone offset is `0`, and the civil date is
/// computed in UTC without external crates.
pub fn timestamp(unix_secs: i64) -> [u8; 12] {
    let mut out = [0u8; 12];

    let days = floor_div(unix_secs, 86400);
    let secs_of_day = unix_secs
        .saturating_sub(days.saturating_mul(86400))
        .clamp(0, 86399);

    let (year, month, day) = civil_from_days(days);
    let year_u16 = if year < 0 {
        0
    } else if year > u16::MAX as i64 {
        u16::MAX
    } else {
        year as u16
    };

    let hour = (secs_of_day / 3600) as u8;
    let minute = ((secs_of_day % 3600) / 60) as u8;
    let second = (secs_of_day % 60) as u8;

    put_u16(&mut out, 0, 0x1000);
    put_u16(&mut out, 2, year_u16);
    put_u8(&mut out, 4, month);
    put_u8(&mut out, 5, day);
    put_u8(&mut out, 6, hour);
    put_u8(&mut out, 7, minute);
    put_u8(&mut out, 8, second);
    out
}

/// Build a 32-byte entity identifier.
///
/// Flags are zero, the identifier is ASCII and zero padded to 23 bytes, and
/// the suffix is copied or truncated to 8 bytes.
pub fn entity_id(id: &str, suffix: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];

    if let Some(ident) = out.get_mut(1..24) {
        for (slot, ch) in ident.iter_mut().zip(id.chars()) {
            *slot = if ch.is_ascii() { ch as u8 } else { b'_' };
        }
    }

    let n = suffix.len().min(8);
    if let Some(src) = suffix.get(0..n) {
        put_bytes(&mut out, 24, src);
    }
    out
}

/// Build a volume recognition sector.
///
/// Structure type is `0`, the identifier is copied, and version is `1`.
pub fn vrs_sector(ident: &[u8; 5]) -> [u8; 2048] {
    let mut out = [0u8; 2048];
    put_bytes(&mut out, 1, ident);
    put_bytes(&mut out, 6, &[1]);
    out
}

/// Build a primary volume descriptor sector.
pub fn primary_volume_descriptor(
    location: u32,
    label: &str,
    set_id: &str,
    unix_time: i64,
) -> [u8; 2048] {
    let mut out = [0u8; 2048];

    put_u32(&mut out, 16, 0);
    put_u32(&mut out, 20, 0);
    put_dstring_at(&mut out, 24, label, 32);
    put_u16(&mut out, 56, 1);
    put_u16(&mut out, 58, 1);
    put_u16(&mut out, 60, 2);
    put_u16(&mut out, 62, 2);
    put_u32(&mut out, 64, 1);
    put_u32(&mut out, 68, 1);
    put_dstring_at(&mut out, 72, set_id, 128);
    put_charspec_cs0_at(&mut out, 200);
    put_charspec_cs0_at(&mut out, 264);
    put_entity_at(&mut out, 344, "*DiscCraft", &[]);
    put_timestamp_at(&mut out, 376, unix_time);
    put_entity_at(&mut out, 388, "*DiscCraft", &[]);

    finish_tag(&mut out, 1, location, 496);
    out
}

/// Build an implementation use volume descriptor sector (UDF 2.2.7: the logical volume information, whose
/// identifier is the logical volume's, `label`).
pub fn implementation_use_volume_descriptor(location: u32, label: &str) -> [u8; 2048] {
    let mut out = [0u8; 2048];

    put_u32(&mut out, 16, 1);
    put_entity_at(&mut out, 20, "*UDF LV Info", &UDF_SUFFIX);
    put_charspec_cs0_at(&mut out, 52);
    put_dstring_at(&mut out, 116, label, 128);
    put_dstring_at(&mut out, 244, "", 36);
    put_dstring_at(&mut out, 280, "", 36);
    put_dstring_at(&mut out, 316, "", 36);
    put_entity_at(&mut out, 352, "*DiscCraft", &[]);

    finish_tag(&mut out, 4, location, 496);
    out
}

/// Build a partition descriptor sector.
pub fn partition_descriptor(location: u32, start_sector: u32, length_sectors: u32) -> [u8; 2048] {
    let mut out = [0u8; 2048];

    put_u32(&mut out, 16, 2);
    put_u16(&mut out, 20, 1);
    put_u16(&mut out, 22, 0);
    put_entity_at(&mut out, 24, "+NSR02", &[]);
    put_u32(&mut out, 184, 1);
    put_u32(&mut out, 188, start_sector);
    put_u32(&mut out, 192, length_sectors);
    put_entity_at(&mut out, 196, "*DiscCraft", &[]);

    finish_tag(&mut out, 5, location, 496);
    out
}

/// Build a logical volume descriptor sector.
pub fn logical_volume_descriptor(
    location: u32,
    label: &str,
    fsd_lbn: u32,
    lvid_extent: (u32, u32),
) -> [u8; 2048] {
    let mut out = [0u8; 2048];

    put_u32(&mut out, 16, 3);
    put_charspec_cs0_at(&mut out, 20);
    put_dstring_at(&mut out, 84, label, 128);
    put_u32(&mut out, 212, 2048);
    put_entity_at(&mut out, 216, "*OSTA UDF Compliant", &DOMAIN_SUFFIX);
    put_long_ad_at(&mut out, 248, 2048, fsd_lbn, 0);
    put_u32(&mut out, 264, 6);
    put_u32(&mut out, 268, 1);
    put_entity_at(&mut out, 272, "*DiscCraft", &[]);
    put_extent_ad_at(&mut out, 432, lvid_extent.0, lvid_extent.1);
    put_u8(&mut out, 440, 1);
    put_u8(&mut out, 441, 6);
    put_u16(&mut out, 442, 1);
    put_u16(&mut out, 444, 0);

    finish_tag(&mut out, 6, location, 430);
    out
}

/// Build an unallocated space descriptor sector.
pub fn unallocated_space_descriptor(location: u32) -> [u8; 2048] {
    let mut out = [0u8; 2048];

    put_u32(&mut out, 16, 4);
    put_u32(&mut out, 20, 0);

    finish_tag(&mut out, 7, location, 8);
    out
}

/// Build a terminating descriptor sector.
pub fn terminating_descriptor(location: u32) -> [u8; 2048] {
    let mut out = [0u8; 2048];

    put_u32(&mut out, 16, 5);

    finish_tag(&mut out, 8, location, 496);
    out
}

/// Build an anchor volume descriptor pointer sector.
pub fn anchor(location: u32, main_vds: (u32, u32), reserve_vds: (u32, u32)) -> [u8; 2048] {
    let mut out = [0u8; 2048];

    put_extent_ad_at(&mut out, 16, main_vds.0, main_vds.1);
    put_extent_ad_at(&mut out, 24, reserve_vds.0, reserve_vds.1);

    finish_tag(&mut out, 2, location, 496);
    out
}

/// Build a logical volume integrity descriptor sector.
pub fn logical_volume_integrity(
    location: u32,
    unix_time: i64,
    next_unique_id: u64,
    files: u32,
    dirs: u32,
    partition_size: u64,
) -> [u8; 2048] {
    let mut out = [0u8; 2048];

    put_timestamp_at(&mut out, 16, unix_time);
    put_u32(&mut out, 28, 1);
    put_extent_ad_at(&mut out, 32, 0, 0);
    put_u64(&mut out, 40, next_unique_id);
    put_u32(&mut out, 72, 1);
    put_u32(&mut out, 76, 46);
    put_u32(&mut out, 80, 0);
    put_u32(&mut out, 84, u32_from_u64(partition_size));
    put_entity_at(&mut out, 88, "*DiscCraft", &[]);
    put_u32(&mut out, 120, files);
    put_u32(&mut out, 124, dirs);
    put_u16(&mut out, 128, 0x0102);
    put_u16(&mut out, 130, 0x0102);
    put_u16(&mut out, 132, 0x0102);

    finish_tag(&mut out, 9, location, 118);
    out
}

/// Build a file set descriptor sector.
pub fn file_set_descriptor(lbn: u32, unix_time: i64, label: &str, root_icb_lbn: u32) -> [u8; 2048] {
    let mut out = [0u8; 2048];

    put_timestamp_at(&mut out, 16, unix_time);
    put_u16(&mut out, 28, 3);
    put_u16(&mut out, 30, 3);
    put_u32(&mut out, 32, 1);
    put_u32(&mut out, 36, 1);
    put_u32(&mut out, 40, 0);
    put_u32(&mut out, 44, 0);
    put_charspec_cs0_at(&mut out, 48);
    put_dstring_at(&mut out, 112, label, 128);
    put_charspec_cs0_at(&mut out, 240);
    put_dstring_at(&mut out, 304, label, 32);
    put_dstring_at(&mut out, 336, "", 32);
    put_dstring_at(&mut out, 368, "", 32);
    put_long_ad_at(&mut out, 400, 2048, root_icb_lbn, 0);
    put_entity_at(&mut out, 416, "*OSTA UDF Compliant", &DOMAIN_SUFFIX);
    put_long_ad_at(&mut out, 448, 0, 0, 0);

    finish_tag(&mut out, 256, lbn, 496);
    out
}

/// Build a file entry descriptor.
///
/// The descriptor is limited to 100 allocation extents so that it fits in one
/// 2048-byte sector.
pub fn file_entry(
    lbn: u32,
    is_dir: bool,
    info_len: u64,
    extents: &[Extent],
    unique_id: u64,
    unix_time: i64,
    link_count: u16,
) -> Vec<u8> {
    let used = extents.len().min(100);

    let mut blocks = 0u64;
    for (i, e) in extents.iter().enumerate() {
        if i >= used {
            break;
        }
        blocks = blocks.saturating_add(u64::from(e.len).div_ceil(2048));
    }

    let len = 176usize.saturating_add(8usize.saturating_mul(used));
    let mut out = vec![0u8; len];

    put_u32(&mut out, 16, 0);
    put_u16(&mut out, 20, 4);
    put_u16(&mut out, 22, 0);
    put_u16(&mut out, 24, 1);
    put_u8(&mut out, 26, 0);
    put_u8(&mut out, 27, if is_dir { 4 } else { 5 });
    put_u16(&mut out, 34, 0);

    put_u32(&mut out, 36, 0xFFFF_FFFF);
    put_u32(&mut out, 40, 0xFFFF_FFFF);
    put_u32(&mut out, 44, 0x14A5);
    put_u16(&mut out, 48, link_count);
    put_u8(&mut out, 50, 0);
    put_u8(&mut out, 51, 0);
    put_u32(&mut out, 52, 0);
    put_u64(&mut out, 56, info_len);
    put_u64(&mut out, 64, blocks);
    put_timestamp_at(&mut out, 72, unix_time);
    put_timestamp_at(&mut out, 84, unix_time);
    put_timestamp_at(&mut out, 96, unix_time);
    put_u32(&mut out, 108, 1);
    put_entity_at(&mut out, 128, "*DiscCraft", &[]);
    put_u64(&mut out, 160, unique_id);
    put_u32(&mut out, 168, 0);

    let l_ad = (8usize.saturating_mul(used)) as u32;
    put_u32(&mut out, 172, l_ad);

    let mut pos = 176usize;
    for (i, e) in extents.iter().enumerate() {
        if i >= used {
            break;
        }
        put_u32(&mut out, pos, e.len);
        let Some(off) = pos.checked_add(4) else {
            break;
        };
        put_u32(&mut out, off, e.lbn);
        pos = pos.saturating_add(8);
    }

    let body_len = out.len().saturating_sub(16);
    finish_tag(&mut out, 261, lbn, body_len);
    out
}

/// Build a sequence of file identifier descriptors.
///
/// Each FID is padded to a multiple of four bytes. The tag location of each
/// FID is the logical block containing its first byte.
pub fn file_identifier_descriptors(first_lbn: u32, fids: &[Fid]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut offset = 0usize;

    for fid in fids {
        let mut ident = match fid.name.as_ref() {
            Some(s) => fid_identifier(s),
            None => Vec::new(),
        };
        if ident.len() > 255 {
            ident.truncate(255);
        }

        let ident_len = ident.len();
        let base = 38usize;
        let unpadded = base.saturating_add(ident_len);
        let pad = (4 - (unpadded % 4)) % 4;
        let fid_len = unpadded.saturating_add(pad);

        let mut buf = vec![0u8; fid_len];

        let characteristics = if fid.name.is_none() {
            0x08
        } else if fid.is_dir {
            0x02
        } else {
            0
        };

        put_u16(&mut buf, 16, 1);
        put_u8(&mut buf, 18, characteristics);
        put_u8(&mut buf, 19, ident_len as u8);
        put_long_ad_at(&mut buf, 20, 2048, fid.icb_lbn, 0);
        put_u16(&mut buf, 36, 0);
        put_bytes(&mut buf, 38, &ident);

        let block = if offset / 2048 > u32::MAX as usize {
            u32::MAX
        } else {
            (offset / 2048) as u32
        };
        let loc = first_lbn.wrapping_add(block);

        finish_tag(&mut buf, 257, loc, fid_len.saturating_sub(16));
        out.extend_from_slice(&buf);
        offset = offset.saturating_add(fid_len);
    }

    out
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
    let Some(s) = buf.get_mut(offset..end) else {
        return;
    };
    s.copy_from_slice(&[value]);
}

/// Write a little-endian u16 at an offset when the destination exists.
fn put_u16(buf: &mut [u8], offset: usize, value: u16) {
    let Some(end) = offset.checked_add(2) else {
        return;
    };
    let Some(s) = buf.get_mut(offset..end) else {
        return;
    };
    s.copy_from_slice(&value.to_le_bytes());
}

/// Write a little-endian u32 at an offset when the destination exists.
fn put_u32(buf: &mut [u8], offset: usize, value: u32) {
    let Some(end) = offset.checked_add(4) else {
        return;
    };
    let Some(s) = buf.get_mut(offset..end) else {
        return;
    };
    s.copy_from_slice(&value.to_le_bytes());
}

/// Write a little-endian u64 at an offset when the destination exists.
fn put_u64(buf: &mut [u8], offset: usize, value: u64) {
    let Some(end) = offset.checked_add(8) else {
        return;
    };
    let Some(s) = buf.get_mut(offset..end) else {
        return;
    };
    s.copy_from_slice(&value.to_le_bytes());
}

/// Write an extent_ad at an offset.
fn put_extent_ad_at(buf: &mut [u8], offset: usize, length: u32, location: u32) {
    put_u32(buf, offset, length);
    if let Some(off) = offset.checked_add(4) {
        put_u32(buf, off, location);
    }
}

/// Write a long_ad at an offset.
fn put_long_ad_at(buf: &mut [u8], offset: usize, length: u32, location: u32, partition: u16) {
    put_u32(buf, offset, length);
    if let Some(off) = offset.checked_add(4) {
        put_u32(buf, off, location);
    }
    if let Some(off) = offset.checked_add(8) {
        put_u16(buf, off, partition);
    }
}

/// Write a d-string at an offset.
fn put_dstring_at(buf: &mut [u8], offset: usize, text: &str, field_len: usize) {
    let data = dstring(text, field_len);
    put_bytes(buf, offset, &data);
}

/// Write an entity identifier at an offset.
fn put_entity_at(buf: &mut [u8], offset: usize, id: &str, suffix: &[u8]) {
    let ent = entity_id(id, suffix);
    put_bytes(buf, offset, &ent);
}

/// Write a timestamp at an offset.
fn put_timestamp_at(buf: &mut [u8], offset: usize, unix_secs: i64) {
    let ts = timestamp(unix_secs);
    put_bytes(buf, offset, &ts);
}

/// Build an OSTA CS0 character spec.
fn charspec_cs0() -> [u8; 64] {
    let mut out = [0u8; 64];
    let n = CS0_IDENTIFIER.len().min(63);
    if let Some(src) = CS0_IDENTIFIER.get(0..n) {
        put_bytes(&mut out, 1, src);
    }
    out
}

/// Write an OSTA CS0 character spec at an offset.
fn put_charspec_cs0_at(buf: &mut [u8], offset: usize) {
    let cs = charspec_cs0();
    put_bytes(buf, offset, &cs);
}

/// Floor division for signed integers.
fn floor_div(a: i64, b: i64) -> i64 {
    let q = a / b;
    let r = a % b;
    if r != 0 && ((r < 0) != (b < 0)) {
        q.saturating_sub(1)
    } else {
        q
    }
}

/// Convert days since 1970-01-01 to a civil date.
fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let z = days.saturating_add(719468);
    let era = floor_div(z, 146097);

    let doe = match z.checked_sub(era.saturating_mul(146097)) {
        Some(v) if (0..=146096).contains(&v) => v as u32,
        _ => 0,
    };

    let yoe = doe
        .saturating_sub(doe / 1460)
        .saturating_add(doe / 36524)
        .saturating_sub(doe / 146096)
        / 365;

    let y = i64::from(yoe).saturating_add(era.saturating_mul(400));

    let term = (365 * yoe)
        .saturating_add(yoe / 4)
        .saturating_sub(yoe / 100);
    let doy = doe.saturating_sub(term);

    let mp = doy.saturating_mul(5).saturating_add(2) / 153;
    let d = doy.saturating_sub((153 * mp + 2) / 5).saturating_add(1) as u8;
    let m = if mp < 10 {
        mp.saturating_add(3)
    } else {
        mp.saturating_sub(9)
    } as u8;

    let y = if m <= 2 { y.saturating_add(1) } else { y };
    (y, m, d)
}

/// Saturating u64 to u32 conversion.
fn u32_from_u64(value: u64) -> u32 {
    if value > u64::from(u32::MAX) {
        u32::MAX
    } else {
        value as u32
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Extent, Fid, anchor, crc_itu, dstring, file_entry, file_identifier_descriptors,
        file_set_descriptor, finish_tag, implementation_use_volume_descriptor,
        logical_volume_descriptor, logical_volume_integrity, partition_descriptor,
        primary_volume_descriptor, terminating_descriptor, timestamp, unallocated_space_descriptor,
    };

    fn le16(buf: &[u8], off: usize) -> u16 {
        u16::from_le_bytes([buf[off], buf[off + 1]])
    }

    fn le32(buf: &[u8], off: usize) -> u32 {
        u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
    }

    fn le64(buf: &[u8], off: usize) -> u64 {
        u64::from_le_bytes([
            buf[off],
            buf[off + 1],
            buf[off + 2],
            buf[off + 3],
            buf[off + 4],
            buf[off + 5],
            buf[off + 6],
            buf[off + 7],
        ])
    }

    #[test]
    fn crc_itu_vector() {
        assert_eq!(crc_itu(&[0x70, 0x6A, 0x77]), 0x3299);
    }

    #[test]
    fn tag_checksum_and_crc_recomputed() {
        let mut buf = [0u8; 64];
        if let Some(body) = buf.get_mut(16..32) {
            for (i, b) in body.iter_mut().enumerate() {
                *b = (i as u8).wrapping_mul(3);
            }
        }

        finish_tag(&mut buf, 1, 0x1234_5678, 16);

        assert_eq!(le16(&buf, 0), 1);
        assert_eq!(le16(&buf, 2), 2);
        assert_eq!(buf[5], 0);
        assert_eq!(le16(&buf, 6), 0);
        assert_eq!(le16(&buf, 10), 16);
        assert_eq!(le32(&buf, 12), 0x1234_5678);

        let mut sum = 0u32;
        if let Some(s) = buf.get(0..4) {
            for b in s {
                sum = sum.wrapping_add(u32::from(*b));
            }
        }
        if let Some(s) = buf.get(5..16) {
            for b in s {
                sum = sum.wrapping_add(u32::from(*b));
            }
        }
        assert_eq!(buf[4], (sum & 0xFF) as u8);

        assert_eq!(le16(&buf, 8), crc_itu(&buf[16..32]));
    }

    #[test]
    fn dstrings_keep_latin1_and_japanese() {
        // Latin-1 stays one byte a character.
        let d = dstring("Café", 32);
        assert_eq!(&d[0..5], &[8, b'C', b'a', b'f', 0xE9]);
        assert_eq!(d[31], 5);
        // Anything else is UCS-2, big-endian: 夏の旅行 (U+590F U+306E U+65C5 U+884C).
        let d = dstring("夏の旅行 2026", 32);
        assert_eq!(d[0], 16);
        assert_eq!(&d[1..9], &[0x59, 0x0F, 0x30, 0x6E, 0x65, 0xC5, 0x88, 0x4C]);
        assert_eq!(&d[9..11], &[0x00, b' ']);
        assert_eq!(d[31], 1 + 2 * 9);
        // At most 15 characters fit a 32-byte field; one outside the BMP becomes `_`.
        let d = dstring(&"あ".repeat(20), 32);
        assert_eq!(d[31], 31);
        assert_eq!(&d[29..31], &[0x30, 0x42]);
        let d = dstring("🎬映画", 32);
        assert_eq!(&d[1..3], &[0x00, b'_']);
        assert_eq!(&d[3..5], &[0x66, 0x20]);
    }

    #[test]
    fn dstring_layout() {
        let d = dstring("DVD", 32);
        assert_eq!(d.len(), 32);
        assert_eq!(d[0], 8);
        assert_eq!(d[1], b'D');
        assert_eq!(d[2], b'V');
        assert_eq!(d[3], b'D');
        assert_eq!(d[31], 4);
        for &b in d.iter().skip(4).take(27) {
            assert_eq!(b, 0);
        }
    }

    #[test]
    fn timestamp_epoch() {
        let ts = timestamp(0);
        assert_eq!(le16(&ts, 0), 0x1000);
        assert_eq!(le16(&ts, 2), 1970);
        assert_eq!(ts[4], 1);
        assert_eq!(ts[5], 1);
        assert_eq!(ts[6], 0);
        assert_eq!(ts[7], 0);
        assert_eq!(ts[8], 0);
        assert_eq!(ts[9], 0);
        assert_eq!(ts[10], 0);
        assert_eq!(ts[11], 0);
    }

    #[test]
    fn timestamp_known_unix_seconds() {
        let ts = timestamp(1_700_000_000);
        assert_eq!(le16(&ts, 0), 0x1000);
        assert_eq!(le16(&ts, 2), 2023);
        assert_eq!(ts[4], 11);
        assert_eq!(ts[5], 14);
        assert_eq!(ts[6], 22);
        assert_eq!(ts[7], 13);
        assert_eq!(ts[8], 20);
    }

    #[test]
    fn anchor_fields_at_offsets() {
        let a = anchor(10, (512, 2), (1024, 3));
        assert_eq!(le16(&a, 0), 2);
        assert_eq!(le16(&a, 2), 2);
        assert_eq!(le32(&a, 12), 10);
        assert_eq!(le32(&a, 16), 512);
        assert_eq!(le32(&a, 20), 2);
        assert_eq!(le32(&a, 24), 1024);
        assert_eq!(le32(&a, 28), 3);
        assert_eq!(le16(&a, 10), 496);
        assert_eq!(le16(&a, 8), crc_itu(&a[16..512]));
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn primary_volume_descriptor_offsets() {
        let p = primary_volume_descriptor(0x1000, "DVD", "SET", 0);

        assert_eq!(le16(&p, 0), 1);
        assert_eq!(le16(&p, 2), 2);
        assert_eq!(le16(&p, 10), 496);
        assert_eq!(le32(&p, 12), 0x1000);
        assert_eq!(le32(&p, 16), 0);
        assert_eq!(le32(&p, 20), 0);

        assert_eq!(p[24], 8);
        assert_eq!(p[25], b'D');
        assert_eq!(p[26], b'V');
        assert_eq!(p[27], b'D');
        assert_eq!(p[55], 4);

        assert_eq!(le16(&p, 56), 1);
        assert_eq!(le16(&p, 58), 1);
        assert_eq!(le16(&p, 60), 2);
        assert_eq!(le16(&p, 62), 2);
        assert_eq!(le32(&p, 64), 1);
        assert_eq!(le32(&p, 68), 1);

        assert_eq!(p[72], 8);
        assert_eq!(p[73], b'S');
        assert_eq!(p[74], b'E');
        assert_eq!(p[75], b'T');
        assert_eq!(p[199], 4);

        assert_eq!(p[200], 0);
        assert_eq!(&p[201..224], b"OSTA Compressed Unicode");
        assert_eq!(p[264], 0);
        assert_eq!(&p[265..288], b"OSTA Compressed Unicode");

        assert_eq!(p[344], 0);
        assert_eq!(&p[345..355], b"*DiscCraft");
        assert_eq!(le16(&p, 376), 0x1000);
        assert_eq!(p[388], 0);
        assert_eq!(&p[389..399], b"*DiscCraft");

        assert_eq!(le16(&p, 8), crc_itu(&p[16..512]));
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn implementation_use_volume_descriptor_offsets() {
        let i = implementation_use_volume_descriptor(1, "DVD");

        assert_eq!(le16(&i, 0), 4);
        assert_eq!(le16(&i, 10), 496);
        assert_eq!(le32(&i, 16), 1);

        assert_eq!(i[20], 0);
        assert_eq!(&i[21..33], b"*UDF LV Info");
        assert_eq!(i[52], 0);
        assert_eq!(&i[53..76], b"OSTA Compressed Unicode");
        // The logical volume identifier: the volume's name (8-bit compressed, its length in the last byte).
        assert_eq!(&i[116..120], b"DVD");
        assert_eq!(i[243], 4);
        assert_eq!(i[244], 0);
        assert_eq!(i[280], 0);
        assert_eq!(i[316], 0);
        assert_eq!(i[352], 0);
        assert_eq!(&i[353..363], b"*DiscCraft");

        assert_eq!(le16(&i, 8), crc_itu(&i[16..512]));
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn partition_descriptor_offsets() {
        let pd = partition_descriptor(2, 0x1234, 0x5678);

        assert_eq!(le16(&pd, 0), 5);
        assert_eq!(le16(&pd, 10), 496);
        assert_eq!(le32(&pd, 16), 2);
        assert_eq!(le16(&pd, 20), 1);
        assert_eq!(le16(&pd, 22), 0);
        assert_eq!(pd[24], 0);
        assert_eq!(&pd[25..31], b"+NSR02");
        assert_eq!(le32(&pd, 184), 1);
        assert_eq!(le32(&pd, 188), 0x1234);
        assert_eq!(le32(&pd, 192), 0x5678);
        assert_eq!(pd[196], 0);
        assert_eq!(&pd[197..207], b"*DiscCraft");

        assert_eq!(le16(&pd, 8), crc_itu(&pd[16..512]));
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn logical_volume_descriptor_offsets() {
        let lvd = logical_volume_descriptor(3, "DVD", 0x20, (10, 0x30));

        assert_eq!(le16(&lvd, 0), 6);
        assert_eq!(le16(&lvd, 10), 430);
        assert_eq!(le32(&lvd, 16), 3);
        assert_eq!(lvd[20], 0);
        assert_eq!(&lvd[21..44], b"OSTA Compressed Unicode");

        assert_eq!(lvd[84], 8);
        assert_eq!(lvd[85], b'D');
        assert_eq!(lvd[86], b'V');
        assert_eq!(lvd[87], b'D');
        assert_eq!(lvd[211], 4);

        assert_eq!(le32(&lvd, 212), 2048);
        assert_eq!(lvd[216], 0);
        assert_eq!(&lvd[217..236], b"*OSTA UDF Compliant");

        assert_eq!(le32(&lvd, 248), 2048);
        assert_eq!(le32(&lvd, 252), 0x20);
        assert_eq!(le16(&lvd, 256), 0);

        assert_eq!(le32(&lvd, 264), 6);
        assert_eq!(le32(&lvd, 268), 1);
        assert_eq!(lvd[272], 0);
        assert_eq!(&lvd[273..283], b"*DiscCraft");

        assert_eq!(le32(&lvd, 432), 10);
        assert_eq!(le32(&lvd, 436), 0x30);
        assert_eq!(lvd[440], 1);
        assert_eq!(lvd[441], 6);
        assert_eq!(le16(&lvd, 442), 1);
        assert_eq!(le16(&lvd, 444), 0);

        assert_eq!(le16(&lvd, 8), crc_itu(&lvd[16..446]));
    }

    #[test]
    fn unallocated_space_descriptor_offsets() {
        let u = unallocated_space_descriptor(4);

        assert_eq!(le16(&u, 0), 7);
        assert_eq!(le16(&u, 10), 8);
        assert_eq!(le32(&u, 16), 4);
        assert_eq!(le32(&u, 20), 0);
        assert_eq!(le16(&u, 8), crc_itu(&u[16..24]));
    }

    #[test]
    fn terminating_descriptor_offsets() {
        let t = terminating_descriptor(5);

        assert_eq!(le16(&t, 0), 8);
        assert_eq!(le16(&t, 10), 496);
        assert_eq!(le32(&t, 16), 5);
        assert_eq!(le16(&t, 8), crc_itu(&t[16..512]));
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn logical_volume_integrity_offsets() {
        let l = logical_volume_integrity(6, 0, 99, 3, 2, 0x1000);

        assert_eq!(le16(&l, 0), 9);
        assert_eq!(le16(&l, 10), 118);
        assert_eq!(le16(&l, 16), 0x1000);
        assert_eq!(le32(&l, 28), 1);
        assert_eq!(le32(&l, 32), 0);
        assert_eq!(le32(&l, 36), 0);
        assert_eq!(le64(&l, 40), 99);
        assert_eq!(le32(&l, 72), 1);
        assert_eq!(le32(&l, 76), 46);
        assert_eq!(le32(&l, 80), 0);
        assert_eq!(le32(&l, 84), 0x1000);
        assert_eq!(l[88], 0);
        assert_eq!(&l[89..99], b"*DiscCraft");
        assert_eq!(le32(&l, 120), 3);
        assert_eq!(le32(&l, 124), 2);
        assert_eq!(le16(&l, 128), 0x0102);
        assert_eq!(le16(&l, 130), 0x0102);
        assert_eq!(le16(&l, 132), 0x0102);

        assert_eq!(le16(&l, 8), crc_itu(&l[16..134]));
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn file_set_descriptor_offsets() {
        let f = file_set_descriptor(7, 0, "DVD", 0x40);

        assert_eq!(le16(&f, 0), 256);
        assert_eq!(le16(&f, 10), 496);
        assert_eq!(le32(&f, 12), 7);
        assert_eq!(le16(&f, 16), 0x1000);
        assert_eq!(le16(&f, 28), 3);
        assert_eq!(le16(&f, 30), 3);
        assert_eq!(le32(&f, 32), 1);
        assert_eq!(le32(&f, 36), 1);
        assert_eq!(le32(&f, 40), 0);
        assert_eq!(le32(&f, 44), 0);

        assert_eq!(f[48], 0);
        assert_eq!(&f[49..72], b"OSTA Compressed Unicode");

        assert_eq!(f[112], 8);
        assert_eq!(f[113], b'D');
        assert_eq!(f[114], b'V');
        assert_eq!(f[115], b'D');
        assert_eq!(f[239], 4);

        assert_eq!(f[240], 0);
        assert_eq!(&f[241..264], b"OSTA Compressed Unicode");

        assert_eq!(f[304], 8);
        assert_eq!(f[305], b'D');
        assert_eq!(f[306], b'V');
        assert_eq!(f[307], b'D');
        assert_eq!(f[335], 4);
        assert_eq!(f[336], 0);
        assert_eq!(f[368], 0);

        assert_eq!(le32(&f, 400), 2048);
        assert_eq!(le32(&f, 404), 0x40);
        assert_eq!(le16(&f, 408), 0);
        assert_eq!(f[416], 0);
        assert_eq!(&f[417..436], b"*OSTA UDF Compliant");
        assert_eq!(le32(&f, 448), 0);
        assert_eq!(le32(&f, 452), 0);
        assert_eq!(le16(&f, 456), 0);

        assert_eq!(le16(&f, 8), crc_itu(&f[16..512]));
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn file_entry_two_extents_blocks_recorded() {
        let exts = [Extent { lbn: 0, len: 1 }, Extent { lbn: 1, len: 2048 }];
        let fe = file_entry(5, false, 2049, &exts, 42, 0, 1);

        assert_eq!(fe.len(), 192);
        assert_eq!(le16(&fe, 0), 261);
        assert_eq!(le16(&fe, 2), 2);
        assert_eq!(le32(&fe, 12), 5);

        assert_eq!(le32(&fe, 16), 0);
        assert_eq!(le16(&fe, 20), 4);
        assert_eq!(le16(&fe, 22), 0);
        assert_eq!(le16(&fe, 24), 1);
        assert_eq!(fe[26], 0);
        assert_eq!(fe[27], 5);
        assert_eq!(le16(&fe, 34), 0);

        assert_eq!(le32(&fe, 36), 0xFFFF_FFFF);
        assert_eq!(le32(&fe, 40), 0xFFFF_FFFF);
        assert_eq!(le32(&fe, 44), 0x14A5);
        assert_eq!(le16(&fe, 48), 1);
        assert_eq!(fe[50], 0);
        assert_eq!(fe[51], 0);
        assert_eq!(le32(&fe, 52), 0);

        assert_eq!(le64(&fe, 56), 2049);
        assert_eq!(le64(&fe, 64), 2);
        assert_eq!(le16(&fe, 72), 0x1000);
        assert_eq!(le16(&fe, 84), 0x1000);
        assert_eq!(le16(&fe, 96), 0x1000);
        assert_eq!(le32(&fe, 108), 1);

        assert_eq!(fe[128], 0);
        assert_eq!(&fe[129..139], b"*DiscCraft");
        assert_eq!(le64(&fe, 160), 42);
        assert_eq!(le32(&fe, 168), 0);
        assert_eq!(le32(&fe, 172), 16);

        assert_eq!(le32(&fe, 176), 1);
        assert_eq!(le32(&fe, 180), 0);
        assert_eq!(le32(&fe, 184), 2048);
        assert_eq!(le32(&fe, 188), 1);

        assert_eq!(le16(&fe, 10), (fe.len() - 16) as u16);
        assert_eq!(le16(&fe, 8), crc_itu(&fe[16..]));

        let mut sum = 0u32;
        if let Some(s) = fe.get(0..4) {
            for b in s {
                sum = sum.wrapping_add(u32::from(*b));
            }
        }
        if let Some(s) = fe.get(5..16) {
            for b in s {
                sum = sum.wrapping_add(u32::from(*b));
            }
        }
        assert_eq!(fe[4], (sum & 0xFF) as u8);
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn fid_sequence_parent_and_two_entries() {
        let fids = vec![
            Fid {
                name: None,
                is_dir: true,
                icb_lbn: 1,
            },
            Fid {
                name: Some(String::from("A")),
                is_dir: false,
                icb_lbn: 2,
            },
            Fid {
                name: Some(String::from("B")),
                is_dir: true,
                icb_lbn: 3,
            },
        ];

        let data = file_identifier_descriptors(0, &fids);
        let mut off = 0usize;
        let mut count = 0usize;

        while off < data.len() {
            let crc_len = le16(&data, off + 10) as usize;
            let len = crc_len + 16;
            assert!(len >= 16);
            assert_eq!(len % 4, 0);

            assert_eq!(le16(&data, off), 257);
            assert_eq!(le16(&data, off + 2), 2);
            assert_eq!(le32(&data, off + 12), (off / 2048) as u32);
            assert_eq!(le16(&data, off + 16), 1);

            let expected_char: u8 = if count == 0 {
                0x08
            } else if count == 2 {
                0x02
            } else {
                0
            };
            assert_eq!(data[off + 18], expected_char);

            let l_fi = usize::from(data[off + 19]);
            assert_eq!(le32(&data, off + 20), 2048);

            let expected_icb = match count {
                0 => 1,
                1 => 2,
                _ => 3,
            };
            assert_eq!(le32(&data, off + 24), expected_icb);
            assert_eq!(le16(&data, off + 28), 0);
            assert_eq!(le16(&data, off + 36), 0);

            let ident = &data[off + 38..off + 38 + l_fi];
            if count == 0 {
                assert_eq!(l_fi, 0);
                assert!(ident.is_empty());
            } else {
                assert_eq!(l_fi, 2);
                assert_eq!(ident[0], 8);
                let body = if ident.len() > 1 { &ident[1..] } else { &[] };
                let decoded: String = body.iter().map(|&b| b as char).collect();
                let expected = if count == 1 { "A" } else { "B" };
                assert_eq!(decoded, expected);
            }

            assert_eq!(le16(&data, off + 8), crc_itu(&data[off + 16..off + len]));

            off += len;
            count += 1;
        }

        assert_eq!(count, 3);
        assert_eq!(off, data.len());
    }
}
