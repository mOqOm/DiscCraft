//! UDF 2.50 descriptor builders for Blu-ray images.

use crate::desc;
use crate::image::UdfError;

/// Size of an extended file entry before its allocation descriptors.
const EFE_BASE: usize = 216;

/// Sector size in bytes.
pub(crate) const SECTOR_SIZE: usize = 2048;
/// Sector size as `u32`.
pub(crate) const SECTOR_SIZE_U32: u32 = 2048;
/// Sector size as `u64`.
pub(crate) const SECTOR_SIZE_U64: u64 = 2048;
/// Maximum allocation extent length in bytes.
pub(crate) const MAX_EXTENT_BYTES: u64 = 0x3FFF_F800;
/// UDF 2.50 entity suffix.
pub(crate) const SUFFIX_250: [u8; 8] = [0x50, 0x02, 0, 0, 0, 0, 0, 0];
/// Domain identifier.
pub(crate) const DOMAIN_ID: &str = "*OSTA UDF Compliant";
/// Logical volume information identifier.
pub(crate) const LV_INFO_ID: &str = "*UDF LV Info";
/// Metadata partition identifier.
pub(crate) const METADATA_PARTITION_ID: &str = "*UDF Metadata Partition";
/// Implementation identifier.
pub(crate) const IMPLEMENTATION_ID: &str = "*DiscCraft";
/// Partition contents identifier.
pub(crate) const PARTITION_CONTENTS_ID: &str = "+NSR03";
/// UDF 2.50 revision value.
pub(crate) const REVISION_250: u16 = 0x0250;

/// Allocation descriptor used by a file entry.
#[derive(Debug, Clone, Copy)]
pub(crate) enum EntryExtent {
    /// Short allocation descriptor.
    Short {
        /// Partition-relative block number.
        block: u32,
        /// Length in bytes.
        length: u32,
    },
    /// Long allocation descriptor.
    Long {
        /// Partition-relative block number.
        block: u32,
        /// Length in bytes.
        length: u32,
        /// Partition reference.
        partition: u16,
    },
}

/// Parameters for a file entry descriptor.
pub(crate) struct EntryParams<'a> {
    /// Descriptor location.
    pub location: u32,
    /// UDF file type.
    pub file_type: u8,
    /// ICB flags.
    pub icb_flags: u16,
    /// Parent partition reference.
    pub parent_partition: u16,
    /// Parent file entry block.
    pub parent_block: u32,
    /// Information length in bytes.
    pub info_len: u64,
    /// Logical blocks recorded.
    pub logical_blocks: u64,
    /// Unique identifier.
    pub unique_id: u64,
    /// File link count.
    pub link_count: u16,
    /// UNIX timestamp.
    pub unix_time: i64,
    /// Allocation descriptors.
    pub extents: &'a [EntryExtent],
}

/// Parameters for a file identifier descriptor.
pub(crate) struct FidParams<'a> {
    /// Descriptor location.
    pub location: u32,
    /// File characteristics.
    pub characteristics: u8,
    /// File name, or `None` for the parent entry.
    pub name: Option<&'a str>,
    /// ICB partition reference.
    pub icb_partition: u16,
    /// ICB block number.
    pub icb_block: u32,
    /// Child unique identifier stored in implementation use.
    pub icb_unique_id: u32,
}

/// Parameters for a logical volume descriptor.
pub(crate) struct LvdParams<'a> {
    /// Descriptor location.
    pub location: u32,
    /// Volume descriptor space number.
    pub vds_number: u32,
    /// Logical volume label.
    pub label: &'a str,
    /// Metadata file set descriptor block.
    pub fsd_block: u32,
    /// Integrity sequence location.
    pub integrity_location: u32,
    /// Integrity sequence length in bytes.
    pub integrity_length: u32,
    /// Metadata file entry block.
    pub metadata_file_location: u32,
    /// Metadata mirror file entry block.
    pub metadata_mirror_location: u32,
}

/// Parameters for a logical volume integrity descriptor.
pub(crate) struct LvidParams {
    /// Descriptor location.
    pub location: u32,
    /// UNIX timestamp.
    pub unix_time: i64,
    /// Next unique identifier.
    pub next_unique_id: u64,
    /// Number of files.
    pub files: u32,
    /// Number of directories.
    pub dirs: u32,
    /// Physical partition length in blocks.
    pub physical_blocks: u32,
    /// Metadata file length in blocks.
    pub metadata_blocks: u32,
}

fn invalid(msg: &str) -> UdfError {
    UdfError::Invalid(msg.to_string())
}

fn out_of_bounds() -> UdfError {
    invalid("descriptor buffer out of bounds")
}

fn put_u8(buf: &mut [u8], off: usize, val: u8) -> Result<(), UdfError> {
    let end = off.checked_add(1).ok_or_else(out_of_bounds)?;
    let dst = buf.get_mut(off..end).ok_or_else(out_of_bounds)?;
    dst.copy_from_slice(&[val]);
    Ok(())
}

fn put_u16(buf: &mut [u8], off: usize, val: u16) -> Result<(), UdfError> {
    let end = off.checked_add(2).ok_or_else(out_of_bounds)?;
    let dst = buf.get_mut(off..end).ok_or_else(out_of_bounds)?;
    dst.copy_from_slice(&val.to_le_bytes());
    Ok(())
}

fn put_u32(buf: &mut [u8], off: usize, val: u32) -> Result<(), UdfError> {
    let end = off.checked_add(4).ok_or_else(out_of_bounds)?;
    let dst = buf.get_mut(off..end).ok_or_else(out_of_bounds)?;
    dst.copy_from_slice(&val.to_le_bytes());
    Ok(())
}

fn put_u64(buf: &mut [u8], off: usize, val: u64) -> Result<(), UdfError> {
    let end = off.checked_add(8).ok_or_else(out_of_bounds)?;
    let dst = buf.get_mut(off..end).ok_or_else(out_of_bounds)?;
    dst.copy_from_slice(&val.to_le_bytes());
    Ok(())
}

fn put_bytes(buf: &mut [u8], off: usize, src: &[u8]) -> Result<(), UdfError> {
    let end = off.checked_add(src.len()).ok_or_else(out_of_bounds)?;
    let dst = buf.get_mut(off..end).ok_or_else(out_of_bounds)?;
    dst.copy_from_slice(src);
    Ok(())
}

/// Round a `u32` value up to a multiple of `unit`.
pub(crate) fn round_up_u32(value: u32, unit: u32) -> Result<u32, UdfError> {
    if unit == 0 {
        return Err(invalid("rounding unit is zero"));
    }
    let rem = value % unit;
    if rem == 0 {
        return Ok(value);
    }
    value
        .checked_add(unit - rem)
        .ok_or_else(|| invalid("rounding overflow"))
}

/// Number of 2048-byte blocks needed for `bytes`.
pub(crate) fn blocks_for_bytes(bytes: u64) -> Result<u32, UdfError> {
    if bytes == 0 {
        return Ok(0);
    }
    let blocks = bytes
        .checked_add(SECTOR_SIZE_U64 - 1)
        .ok_or_else(|| invalid("block count overflow"))?
        / SECTOR_SIZE_U64;
    u32::try_from(blocks).map_err(|_| invalid("block count too large"))
}

/// Number of 2048-byte blocks needed for `bytes`.
pub(crate) fn blocks_for_u32_bytes(bytes: u32) -> Result<u32, UdfError> {
    blocks_for_bytes(u64::from(bytes))
}

fn write_charspec(buf: &mut [u8], off: usize) -> Result<(), UdfError> {
    put_u8(buf, off, 0)?;
    put_bytes(
        buf,
        off.checked_add(1).ok_or_else(out_of_bounds)?,
        b"OSTA Compressed Unicode",
    )
}

fn write_regid(buf: &mut [u8], off: usize, id: &str, suffix: &[u8]) -> Result<(), UdfError> {
    let reg = desc::entity_id(id, suffix);
    put_bytes(buf, off, &reg)
}

fn write_dstring(buf: &mut [u8], off: usize, text: &str, field_len: usize) -> Result<(), UdfError> {
    let ds = desc::dstring(text, field_len);
    put_bytes(buf, off, &ds)
}

fn write_long_ad(
    buf: &mut [u8],
    off: usize,
    length: u32,
    block: u32,
    partition: u16,
) -> Result<(), UdfError> {
    put_u32(buf, off, length)?;
    put_u32(buf, off.checked_add(4).ok_or_else(out_of_bounds)?, block)?;
    put_u16(
        buf,
        off.checked_add(8).ok_or_else(out_of_bounds)?,
        partition,
    )
}

fn write_short_ad(buf: &mut [u8], off: usize, length: u32, block: u32) -> Result<(), UdfError> {
    put_u32(buf, off, length)?;
    put_u32(buf, off.checked_add(4).ok_or_else(out_of_bounds)?, block)
}

fn write_extent_ad(buf: &mut [u8], off: usize, length: u32, location: u32) -> Result<(), UdfError> {
    put_u32(buf, off, length)?;
    put_u32(buf, off.checked_add(4).ok_or_else(out_of_bounds)?, location)
}

fn write_lb_addr(buf: &mut [u8], off: usize, partition: u16, block: u32) -> Result<(), UdfError> {
    put_u16(buf, off, partition)?;
    put_u32(buf, off.checked_add(2).ok_or_else(out_of_bounds)?, block)
}

/// Build a volume recognition sector.
pub(crate) fn volume_recognition_sector(ident: &[u8; 5]) -> [u8; SECTOR_SIZE] {
    desc::vrs_sector(ident)
}

/// Build a primary volume descriptor.
pub(crate) fn pvd(
    location: u32,
    vds_number: u32,
    label: &str,
    set_id: &str,
    unix_time: i64,
) -> Result<[u8; SECTOR_SIZE], UdfError> {
    let mut buf = [0u8; SECTOR_SIZE];
    put_u32(&mut buf, 16, vds_number)?;
    put_u32(&mut buf, 20, 0)?;
    write_dstring(&mut buf, 24, label, 32)?;
    put_u16(&mut buf, 56, 1)?;
    put_u16(&mut buf, 58, 1)?;
    put_u16(&mut buf, 60, 2)?;
    put_u16(&mut buf, 62, 2)?;
    put_u32(&mut buf, 64, 1)?;
    put_u32(&mut buf, 68, 1)?;
    write_dstring(&mut buf, 72, set_id, 128)?;
    write_charspec(&mut buf, 200)?;
    write_charspec(&mut buf, 264)?;
    write_regid(&mut buf, 344, IMPLEMENTATION_ID, &[])?;
    let ts = desc::timestamp(unix_time);
    put_bytes(&mut buf, 376, &ts)?;
    write_regid(&mut buf, 388, IMPLEMENTATION_ID, &[])?;
    put_u32(&mut buf, 484, 0)?;
    put_u16(&mut buf, 488, 0)?;
    desc::finish_tag_version(&mut buf, 1, 3, location, 496);
    Ok(buf)
}

/// Build an implementation use volume descriptor.
pub(crate) fn iuvd(
    location: u32,
    vds_number: u32,
    label: &str,
) -> Result<[u8; SECTOR_SIZE], UdfError> {
    let mut buf = [0u8; SECTOR_SIZE];
    put_u32(&mut buf, 16, vds_number)?;
    write_regid(&mut buf, 20, LV_INFO_ID, &SUFFIX_250)?;
    write_charspec(&mut buf, 52)?;
    write_dstring(&mut buf, 116, label, 128)?;
    write_dstring(&mut buf, 244, "", 36)?;
    write_dstring(&mut buf, 280, "", 36)?;
    write_dstring(&mut buf, 316, "", 36)?;
    write_regid(&mut buf, 352, IMPLEMENTATION_ID, &[])?;
    desc::finish_tag_version(&mut buf, 4, 3, location, 496);
    Ok(buf)
}

/// Build a partition descriptor.
pub(crate) fn pd(
    location: u32,
    vds_number: u32,
    start_sector: u32,
    length_sectors: u32,
) -> Result<[u8; SECTOR_SIZE], UdfError> {
    let mut buf = [0u8; SECTOR_SIZE];
    put_u32(&mut buf, 16, vds_number)?;
    put_u16(&mut buf, 20, 1)?;
    put_u16(&mut buf, 22, 0)?;
    write_regid(&mut buf, 24, PARTITION_CONTENTS_ID, &SUFFIX_250)?;
    put_u32(&mut buf, 184, 1)?;
    put_u32(&mut buf, 188, start_sector)?;
    put_u32(&mut buf, 192, length_sectors)?;
    write_regid(&mut buf, 196, IMPLEMENTATION_ID, &[])?;
    desc::finish_tag_version(&mut buf, 5, 3, location, 496);
    Ok(buf)
}

fn write_partition_maps(
    buf: &mut [u8],
    off: usize,
    metadata_file_location: u32,
    metadata_mirror_location: u32,
) -> Result<(), UdfError> {
    put_u8(buf, off, 1)?;
    put_u8(buf, off.checked_add(1).ok_or_else(out_of_bounds)?, 6)?;
    put_u16(buf, off.checked_add(2).ok_or_else(out_of_bounds)?, 1)?;
    put_u16(buf, off.checked_add(4).ok_or_else(out_of_bounds)?, 0)?;

    let map2 = off.checked_add(6).ok_or_else(out_of_bounds)?;
    put_u8(buf, map2, 2)?;
    put_u8(buf, map2.checked_add(1).ok_or_else(out_of_bounds)?, 64)?;
    write_regid(
        buf,
        map2.checked_add(4).ok_or_else(out_of_bounds)?,
        METADATA_PARTITION_ID,
        &SUFFIX_250,
    )?;
    put_u16(buf, map2.checked_add(36).ok_or_else(out_of_bounds)?, 1)?;
    put_u16(buf, map2.checked_add(38).ok_or_else(out_of_bounds)?, 0)?;
    put_u32(
        buf,
        map2.checked_add(40).ok_or_else(out_of_bounds)?,
        metadata_file_location,
    )?;
    put_u32(
        buf,
        map2.checked_add(44).ok_or_else(out_of_bounds)?,
        metadata_mirror_location,
    )?;
    put_u32(
        buf,
        map2.checked_add(48).ok_or_else(out_of_bounds)?,
        0xFFFF_FFFF,
    )?;
    put_u32(buf, map2.checked_add(52).ok_or_else(out_of_bounds)?, 32)?;
    put_u16(buf, map2.checked_add(56).ok_or_else(out_of_bounds)?, 32)?;
    put_u8(buf, map2.checked_add(58).ok_or_else(out_of_bounds)?, 0)
}

/// Build a logical volume descriptor.
pub(crate) fn lvd(params: &LvdParams<'_>) -> Result<[u8; SECTOR_SIZE], UdfError> {
    let mut buf = [0u8; SECTOR_SIZE];
    put_u32(&mut buf, 16, params.vds_number)?;
    write_charspec(&mut buf, 20)?;
    write_dstring(&mut buf, 84, params.label, 128)?;
    put_u32(&mut buf, 212, SECTOR_SIZE_U32)?;
    write_regid(&mut buf, 216, DOMAIN_ID, &SUFFIX_250)?;
    write_long_ad(&mut buf, 248, SECTOR_SIZE_U32, params.fsd_block, 1)?;
    put_u32(&mut buf, 264, 70)?;
    put_u32(&mut buf, 268, 2)?;
    write_regid(&mut buf, 272, IMPLEMENTATION_ID, &[])?;
    write_extent_ad(
        &mut buf,
        432,
        params.integrity_length,
        params.integrity_location,
    )?;
    write_partition_maps(
        &mut buf,
        440,
        params.metadata_file_location,
        params.metadata_mirror_location,
    )?;
    desc::finish_tag_version(&mut buf, 6, 3, params.location, 494);
    Ok(buf)
}

/// Build an unallocated space descriptor.
pub(crate) fn usd(location: u32, vds_number: u32) -> Result<[u8; SECTOR_SIZE], UdfError> {
    let mut buf = [0u8; SECTOR_SIZE];
    put_u32(&mut buf, 16, vds_number)?;
    put_u32(&mut buf, 20, 0)?;
    desc::finish_tag_version(&mut buf, 7, 3, location, 8);
    Ok(buf)
}

/// Build a terminating descriptor.
pub(crate) fn td(location: u32) -> Result<[u8; SECTOR_SIZE], UdfError> {
    let mut buf = [0u8; SECTOR_SIZE];
    desc::finish_tag_version(&mut buf, 8, 3, location, 496);
    Ok(buf)
}

/// Build an anchor volume descriptor pointer.
pub(crate) fn avdp(
    location: u32,
    main_vds: (u32, u32),
    reserve_vds: (u32, u32),
) -> Result<[u8; SECTOR_SIZE], UdfError> {
    let mut buf = [0u8; SECTOR_SIZE];
    write_extent_ad(&mut buf, 16, main_vds.1, main_vds.0)?;
    write_extent_ad(&mut buf, 24, reserve_vds.1, reserve_vds.0)?;
    desc::finish_tag_version(&mut buf, 2, 3, location, 496);
    Ok(buf)
}

/// Build a logical volume integrity descriptor.
pub(crate) fn lvid(params: &LvidParams) -> Result<[u8; SECTOR_SIZE], UdfError> {
    let mut buf = [0u8; SECTOR_SIZE];
    let ts = desc::timestamp(params.unix_time);
    put_bytes(&mut buf, 16, &ts)?;
    put_u32(&mut buf, 28, 1)?;
    put_u64(&mut buf, 40, params.next_unique_id)?;
    put_u32(&mut buf, 72, 2)?;
    put_u32(&mut buf, 76, 46)?;
    put_u32(&mut buf, 80, 0)?;
    put_u32(&mut buf, 84, 0)?;
    put_u32(&mut buf, 88, params.physical_blocks)?;
    put_u32(&mut buf, 92, params.metadata_blocks)?;
    write_regid(&mut buf, 96, IMPLEMENTATION_ID, &[])?;
    put_u32(&mut buf, 128, params.files)?;
    put_u32(&mut buf, 132, params.dirs)?;
    put_u16(&mut buf, 136, REVISION_250)?;
    put_u16(&mut buf, 138, REVISION_250)?;
    put_u16(&mut buf, 140, REVISION_250)?;
    desc::finish_tag_version(&mut buf, 9, 3, params.location, 126);
    Ok(buf)
}

/// Build a file set descriptor.
pub(crate) fn fsd(
    location: u32,
    label: &str,
    root_icb_block: u32,
    unix_time: i64,
) -> Result<[u8; SECTOR_SIZE], UdfError> {
    let mut buf = [0u8; SECTOR_SIZE];
    let ts = desc::timestamp(unix_time);
    put_bytes(&mut buf, 16, &ts)?;
    put_u16(&mut buf, 28, 3)?;
    put_u16(&mut buf, 30, 3)?;
    put_u32(&mut buf, 32, 1)?;
    put_u32(&mut buf, 36, 1)?;
    put_u32(&mut buf, 40, 0)?;
    put_u32(&mut buf, 44, 0)?;
    write_charspec(&mut buf, 48)?;
    write_dstring(&mut buf, 112, label, 128)?;
    write_charspec(&mut buf, 240)?;
    write_dstring(&mut buf, 304, label, 32)?;
    write_dstring(&mut buf, 336, "", 32)?;
    write_dstring(&mut buf, 368, "", 32)?;
    write_long_ad(&mut buf, 400, SECTOR_SIZE_U32, root_icb_block, 1)?;
    write_regid(&mut buf, 416, DOMAIN_ID, &SUFFIX_250)?;
    desc::finish_tag_version(&mut buf, 256, 3, location, 496);
    Ok(buf)
}

fn extent_size(extent: &EntryExtent) -> usize {
    match extent {
        EntryExtent::Short { .. } => 8,
        EntryExtent::Long { .. } => 16,
    }
}

fn write_entry_extent(buf: &mut [u8], off: usize, extent: &EntryExtent) -> Result<(), UdfError> {
    match extent {
        EntryExtent::Short { block, length } => write_short_ad(buf, off, *length, *block),
        EntryExtent::Long {
            block,
            length,
            partition,
        } => write_long_ad(buf, off, *length, *block, *partition),
    }
}

/// Build an extended file entry (ECMA-167 4/14.17, tag 266): UDF 2.50 readers expect EFEs, the metadata files in particular.
pub(crate) fn file_entry(params: &EntryParams<'_>) -> Result<Vec<u8>, UdfError> {
    let mut l_ad = 0usize;
    for extent in params.extents {
        l_ad = l_ad
            .checked_add(extent_size(extent))
            .ok_or_else(|| invalid("extent overflow"))?;
    }
    let len = EFE_BASE
        .checked_add(l_ad)
        .ok_or_else(|| invalid("file entry too large"))?;
    if len > SECTOR_SIZE {
        return Err(invalid("file entry too large"));
    }

    let mut buf = vec![0u8; len];
    put_u32(&mut buf, 16, 0)?;
    put_u16(&mut buf, 20, 4)?;
    put_u16(&mut buf, 22, 0)?;
    put_u16(&mut buf, 24, 1)?;
    put_u8(&mut buf, 26, 0)?;
    put_u8(&mut buf, 27, params.file_type)?;
    write_lb_addr(&mut buf, 28, params.parent_partition, params.parent_block)?;
    put_u16(&mut buf, 34, params.icb_flags)?;
    put_u32(&mut buf, 36, 0xFFFF_FFFF)?;
    put_u32(&mut buf, 40, 0xFFFF_FFFF)?;
    put_u32(&mut buf, 44, 0x14A5)?;
    put_u16(&mut buf, 48, params.link_count)?;
    put_u8(&mut buf, 50, 0)?;
    put_u8(&mut buf, 51, 0)?;
    put_u32(&mut buf, 52, 0)?;
    put_u64(&mut buf, 56, params.info_len)?;
    // Object size (no named streams, so the same as the information length).
    put_u64(&mut buf, 64, params.info_len)?;
    put_u64(&mut buf, 72, params.logical_blocks)?;

    // Access, modification, creation and attribute times.
    let ts = desc::timestamp(params.unix_time);
    for at in [80, 92, 104, 116] {
        put_bytes(&mut buf, at, &ts)?;
    }

    put_u32(&mut buf, 128, 1)?;
    // 132 reserved, 136 extended attribute ICB and 152 stream directory ICB stay zero.
    write_regid(&mut buf, 168, IMPLEMENTATION_ID, &[])?;
    put_u64(&mut buf, 200, params.unique_id)?;
    put_u32(&mut buf, 208, 0)?;
    let l_ad_u32 = u32::try_from(l_ad).map_err(|_| invalid("allocation length too large"))?;
    put_u32(&mut buf, 212, l_ad_u32)?;

    let mut off = EFE_BASE;
    for extent in params.extents {
        write_entry_extent(&mut buf, off, extent)?;
        off = off
            .checked_add(extent_size(extent))
            .ok_or_else(out_of_bounds)?;
    }

    let body_len = len
        .checked_sub(16)
        .ok_or_else(|| invalid("file entry too short"))?;
    desc::finish_tag_version(&mut buf, 266, 3, params.location, body_len);
    Ok(buf)
}

/// Build a file identifier descriptor.
pub(crate) fn fid(params: &FidParams<'_>) -> Result<Vec<u8>, UdfError> {
    // The parent entry has no identifier (L_FI = 0).
    let id = match params.name {
        Some(name) => desc::fid_identifier(name),
        None => Vec::new(),
    };
    let id_len = u8::try_from(id.len()).map_err(|_| invalid("file identifier too long"))?;
    let raw = 38usize
        .checked_add(id.len())
        .ok_or_else(|| invalid("file identifier overflow"))?;
    let raw_u32 = u32::try_from(raw).map_err(|_| invalid("file identifier overflow"))?;
    let len = usize::try_from(round_up_u32(raw_u32, 4)?)
        .map_err(|_| invalid("file identifier overflow"))?;

    let mut buf = vec![0u8; len];
    put_u16(&mut buf, 16, 1)?;
    put_u8(&mut buf, 18, params.characteristics)?;
    put_u8(&mut buf, 19, id_len)?;
    write_long_ad(
        &mut buf,
        20,
        SECTOR_SIZE_U32,
        params.icb_block,
        params.icb_partition,
    )?;
    put_u32(&mut buf, 32, params.icb_unique_id)?;
    put_u16(&mut buf, 36, 0)?;
    put_bytes(&mut buf, 38, &id)?;

    let body_len = len
        .checked_sub(16)
        .ok_or_else(|| invalid("file identifier too short"))?;
    desc::finish_tag_version(&mut buf, 257, 3, params.location, body_len);
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag_id(buf: &[u8]) -> u16 {
        u16::from_le_bytes([buf[0], buf[1]])
    }

    fn tag_version(buf: &[u8]) -> u16 {
        u16::from_le_bytes([buf[2], buf[3]])
    }

    fn crc_len(buf: &[u8]) -> u16 {
        u16::from_le_bytes([buf[10], buf[11]])
    }

    fn u32_at(buf: &[u8], off: usize) -> u32 {
        u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
    }

    #[test]
    fn descriptors_use_version_3() {
        let pvd = pvd(32, 0, "TEST", "TEST", 0).unwrap();
        let iuvd = iuvd(33, 0, "TEST").unwrap();
        let pd = pd(34, 0, 512, 100).unwrap();
        let lvd = lvd(&LvdParams {
            location: 35,
            vds_number: 0,
            label: "TEST",
            fsd_block: 0,
            integrity_location: 64,
            integrity_length: 4096,
            metadata_file_location: 0,
            metadata_mirror_location: 1,
        })
        .unwrap();
        let usd = usd(36, 0).unwrap();
        let td = td(37).unwrap();
        let avdp = avdp(256, (32, 12288), (48, 12288)).unwrap();
        let lvid = lvid(&LvidParams {
            location: 64,
            unix_time: 0,
            next_unique_id: 16,
            files: 1,
            dirs: 1,
            physical_blocks: 100,
            metadata_blocks: 32,
        })
        .unwrap();
        let fsd = fsd(0, "TEST", 2, 0).unwrap();
        let entry = file_entry(&EntryParams {
            location: 0,
            file_type: 5,
            icb_flags: 1,
            parent_partition: 1,
            parent_block: 2,
            info_len: 1,
            logical_blocks: 1,
            unique_id: 16,
            link_count: 1,
            unix_time: 0,
            extents: &[EntryExtent::Long {
                block: 0,
                length: 1,
                partition: 0,
            }],
        })
        .unwrap();
        let fid = fid(&FidParams {
            location: 3,
            characteristics: 0,
            name: Some("A"),
            icb_partition: 1,
            icb_block: 4,
            icb_unique_id: 17,
        })
        .unwrap();

        assert_eq!(tag_version(&pvd), 3);
        assert_eq!(tag_version(&iuvd), 3);
        assert_eq!(tag_version(&pd), 3);
        assert_eq!(tag_version(&lvd), 3);
        assert_eq!(tag_version(&usd), 3);
        assert_eq!(tag_version(&td), 3);
        assert_eq!(tag_version(&avdp), 3);
        assert_eq!(tag_version(&lvid), 3);
        assert_eq!(tag_version(&fsd), 3);
        assert_eq!(tag_version(&entry), 3);
        assert_eq!(tag_version(&fid), 3);
    }

    #[test]
    fn lvid_and_lvd_lengths() {
        let lvid = lvid(&LvidParams {
            location: 64,
            unix_time: 0,
            next_unique_id: 16,
            files: 1,
            dirs: 1,
            physical_blocks: 100,
            metadata_blocks: 32,
        })
        .unwrap();
        let lvd = lvd(&LvdParams {
            location: 35,
            vds_number: 0,
            label: "TEST",
            fsd_block: 0,
            integrity_location: 64,
            integrity_length: 4096,
            metadata_file_location: 0,
            metadata_mirror_location: 1,
        })
        .unwrap();

        assert_eq!(crc_len(&lvid), 126);
        assert_eq!(crc_len(&lvd), 494);
        assert_eq!(u32_at(&lvd, 264), 70);
        assert_eq!(u32_at(&lvd, 268), 2);
        assert_eq!(lvd[446], 2);
        assert_eq!(lvd[447], 64);
    }

    #[test]
    fn tag_ids_are_expected() {
        let td = td(65).unwrap();
        assert_eq!(tag_id(&td), 8);
        let avdp = avdp(256, (32, 12288), (48, 12288)).unwrap();
        assert_eq!(tag_id(&avdp), 2);
    }
}
