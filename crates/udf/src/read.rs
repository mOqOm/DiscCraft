//! Read back file listings from DVD-Video bridge images.

use crate::desc;
use crate::image::UdfError;
use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub use crate::image::UdfError as Error;

const SECTOR_SIZE: u64 = 2048;
const SECTOR_SIZE_U32: u32 = 2048;
const SECTOR_SIZE_USIZE: usize = 2048;
const MAX_READ_BYTES: usize = 64 * 1024 * 1024;

const TAG_LEN: usize = 16;

const ANCHOR_MAIN_LENGTH_OFFSET: usize = 16;
const ANCHOR_MAIN_LOCATION_OFFSET: usize = 20;

const PD_START_OFFSET: usize = 188;
const PD_LENGTH_OFFSET: usize = 192;

const LVD_FSD_LBN_OFFSET: usize = 252;

const FSD_ROOT_ICB_LBN_OFFSET: usize = 404;

const FE_FILE_TYPE_OFFSET: usize = 27;
const FE_INFO_LEN_OFFSET: usize = 56;
const FE_L_EA_OFFSET: usize = 168;
const FE_L_AD_OFFSET: usize = 172;
const FE_AD_START_OFFSET: usize = 176;
const FE_MIN_LEN: usize = 176;

const FID_CHARACTERISTICS_OFFSET: usize = 18;
const FID_L_FI_OFFSET: usize = 19;
const FID_ICB_LBN_OFFSET: usize = 24;
const FID_L_IU_OFFSET: usize = 36;
const FID_IDENT_OFFSET: usize = 38;
const FID_MIN_LEN: usize = 38;

const ISO_PVD_ROOT_RECORD_OFFSET: usize = 156;
const ISO_DIR_RECORD_EXTENT_OFFSET: usize = 2;
const ISO_DIR_RECORD_SIZE_OFFSET: usize = 10;
const ISO_DIR_RECORD_FLAGS_OFFSET: usize = 25;
const ISO_DIR_RECORD_NAME_LEN_OFFSET: usize = 32;
const ISO_DIR_RECORD_NAME_OFFSET: usize = 33;
const ISO_DIR_RECORD_MIN_LEN: usize = 33;

/// A file found in an image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedFile {
    /// Slash-separated path, for example `VIDEO_TS/VIDEO_TS.IFO`.
    pub path: String,
    /// Absolute sector containing the first byte of file data.
    pub sector: u32,
    /// Information length in bytes.
    pub size: u64,
}

/// List files through the UDF 1.02 metadata.
#[allow(clippy::cognitive_complexity)]
pub fn list_udf(image: &Path) -> Result<Vec<ListedFile>, UdfError> {
    let path = image.to_string_lossy().into_owned();
    let mut file = File::open(image).map_err(|e| io_error(&path, e))?;
    let sectors = file_sectors(&mut file, &path)?;

    if sectors <= 256 {
        return Err(invalid("image too small for anchor"));
    }

    let anchor = read_sector(&mut file, 256, &path)?;
    verify_tag_at(&anchor, 0)?;
    if tag_id(&anchor, 0)? != 2 {
        return Err(invalid("anchor tag id mismatch"));
    }

    // Anchor main VDS extent_ad: length in bytes @16, location @20.
    let main_bytes =
        read_u32(&anchor, ANCHOR_MAIN_LENGTH_OFFSET).ok_or_else(|| invalid("short anchor"))?;
    let main_loc =
        read_u32(&anchor, ANCHOR_MAIN_LOCATION_OFFSET).ok_or_else(|| invalid("short anchor"))?;

    if main_bytes == 0 || main_bytes % SECTOR_SIZE_U32 != 0 {
        return Err(invalid("bad main VDS length"));
    }
    let main_len = main_bytes / SECTOR_SIZE_U32;
    if main_len == 0 || main_len > 1024 {
        return Err(invalid("bad main VDS length"));
    }

    let (partition_start, partition_length, fsd_lbn) =
        read_vds(&mut file, main_loc, main_len, sectors, &path)?;

    if u64::from(partition_start) >= sectors || partition_length < 2 {
        return Err(invalid("bad partition"));
    }
    if checked_add_u64(
        u64::from(partition_start),
        u64::from(partition_length),
        "partition end",
    )? > sectors
    {
        return Err(invalid("partition out of range"));
    }
    if fsd_lbn >= partition_length {
        return Err(invalid("FSD out of partition"));
    }

    let fsd_abs = checked_add_u64(u64::from(partition_start), u64::from(fsd_lbn), "FSD sector")?;
    if fsd_abs >= sectors {
        return Err(invalid("FSD out of range"));
    }

    let fsd = read_sector(&mut file, fsd_abs, &path)?;
    verify_tag_at(&fsd, 0)?;
    if tag_id(&fsd, 0)? != 256 {
        return Err(invalid("FSD tag id mismatch"));
    }

    // FSD root directory ICB long_ad: length @400, LBN @404, partition @408.
    let root_icb_lbn =
        read_u32(&fsd, FSD_ROOT_ICB_LBN_OFFSET).ok_or_else(|| invalid("short FSD"))?;
    if root_icb_lbn >= partition_length {
        return Err(invalid("root ICB out of partition"));
    }

    let root_fe_abs = checked_add_u64(
        u64::from(partition_start),
        u64::from(root_icb_lbn),
        "root FE",
    )?;
    if root_fe_abs >= sectors {
        return Err(invalid("root FE out of range"));
    }

    let mut reader = UdfReader {
        file: &mut file,
        sectors,
        partition_start,
        partition_length,
        path: &path,
    };

    let mut list = Vec::new();
    let mut visited = HashSet::new();
    reader.visit_directory(String::new(), root_fe_abs, &mut visited, &mut list)?;
    list.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(list)
}

/// List files through the ISO 9660 metadata.
pub fn list_iso9660(image: &Path) -> Result<Vec<ListedFile>, UdfError> {
    let path = image.to_string_lossy().into_owned();
    let mut file = File::open(image).map_err(|e| io_error(&path, e))?;
    let sectors = file_sectors(&mut file, &path)?;

    if sectors <= 16 {
        return Err(invalid("image too small for PVD"));
    }

    let pvd = read_sector(&mut file, 16, &path)?;
    verify_pvd(&pvd)?;

    let root_extent = read_u32(
        &pvd,
        ISO_PVD_ROOT_RECORD_OFFSET
            .checked_add(ISO_DIR_RECORD_EXTENT_OFFSET)
            .ok_or_else(|| invalid("short PVD"))?,
    )
    .ok_or_else(|| invalid("short PVD"))?;
    let root_size = u64::from(
        read_u32(
            &pvd,
            ISO_PVD_ROOT_RECORD_OFFSET
                .checked_add(ISO_DIR_RECORD_SIZE_OFFSET)
                .ok_or_else(|| invalid("short PVD"))?,
        )
        .ok_or_else(|| invalid("short PVD"))?,
    );

    let mut reader = IsoReader {
        file: &mut file,
        sectors,
        path: &path,
    };

    let mut list = Vec::new();
    let mut visited = HashSet::new();
    reader.visit_directory(
        String::new(),
        root_extent,
        root_size,
        &mut visited,
        &mut list,
    )?;
    list.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(list)
}

struct UdfReader<'a> {
    file: &'a mut File,
    sectors: u64,
    partition_start: u32,
    partition_length: u32,
    path: &'a str,
}

impl UdfReader<'_> {
    #[allow(clippy::cognitive_complexity)]
    fn visit_directory(
        &mut self,
        prefix: String,
        fe_abs: u64,
        visited: &mut HashSet<u64>,
        list: &mut Vec<ListedFile>,
    ) -> Result<(), UdfError> {
        if fe_abs >= self.sectors {
            return Err(invalid("FE out of range"));
        }
        if !visited.insert(fe_abs) {
            return Ok(());
        }

        let sector = self.read_sector(fe_abs)?;
        let fe = parse_file_entry(&sector)?;
        if !fe.is_dir {
            return Ok(());
        }

        let fids = self.read_fids(&fe.extents)?;
        for fid in fids {
            if fid.parent || fid.name.is_none() {
                continue;
            }

            let name = match fid.name.as_ref() {
                Some(s) => safe_path_name(s),
                None => String::new(),
            };
            if name.is_empty() {
                continue;
            }

            if fid.icb_lbn >= self.partition_length {
                return Err(invalid("FID ICB out of partition"));
            }

            let child_fe = checked_add_u64(
                u64::from(self.partition_start),
                u64::from(fid.icb_lbn),
                "child FE",
            )?;

            if fid.is_dir {
                let child_prefix = format!("{prefix}{name}/");
                self.visit_directory(child_prefix, child_fe, visited, list)?;
            } else {
                let file_path = format!("{prefix}{name}");
                self.add_file(file_path, fid.icb_lbn, list)?;
            }
        }

        Ok(())
    }

    fn add_file(
        &mut self,
        path: String,
        icb_lbn: u32,
        list: &mut Vec<ListedFile>,
    ) -> Result<(), UdfError> {
        let fe_abs = checked_add_u64(
            u64::from(self.partition_start),
            u64::from(icb_lbn),
            "file FE",
        )?;
        if fe_abs >= self.sectors {
            return Err(invalid("file FE out of range"));
        }

        let sector = self.read_sector(fe_abs)?;
        let fe = parse_file_entry(&sector)?;
        if fe.is_dir {
            return Ok(());
        }

        let sector_abs = match fe.extents.iter().find(|extent| extent.len > 0) {
            Some(extent) => {
                check_extent(extent.lbn, extent.len, self.partition_length)?;
                checked_add_u64(
                    u64::from(self.partition_start),
                    u64::from(extent.lbn),
                    "file data",
                )?
            }
            None => fe_abs,
        };

        if sector_abs >= self.sectors {
            return Err(invalid("file data out of range"));
        }

        list.push(ListedFile {
            path,
            sector: u32::try_from(sector_abs).map_err(|_| invalid("sector too large"))?,
            size: fe.info_len,
        });
        Ok(())
    }

    fn read_fids(&mut self, extents: &[desc::Extent]) -> Result<Vec<ParsedFid>, UdfError> {
        let mut non_empty = 0usize;
        let mut only_extent: Option<&desc::Extent> = None;
        let mut total = 0usize;

        for extent in extents {
            if extent.len == 0 {
                continue;
            }

            check_extent(extent.lbn, extent.len, self.partition_length)?;
            let len = usize::try_from(extent.len).map_err(|_| invalid("FID extent too large"))?;
            total = total
                .checked_add(len)
                .ok_or_else(|| invalid("FID data too large"))?;
            if total > MAX_READ_BYTES {
                return Err(invalid("read too large"));
            }

            non_empty = non_empty.saturating_add(1);
            only_extent = Some(extent);
        }

        if non_empty == 0 {
            return Ok(Vec::new());
        }

        if non_empty == 1
            && let Some(extent) = only_extent
        {
            let abs = checked_add_u64(
                u64::from(self.partition_start),
                u64::from(extent.lbn),
                "FID sector",
            )?;
            let len = usize::try_from(extent.len).map_err(|_| invalid("FID extent too large"))?;
            let bytes = self.read_bytes(abs, len)?;
            return parse_fids(&bytes);
        }

        let mut bytes = Vec::with_capacity(total);
        for extent in extents {
            if extent.len == 0 {
                continue;
            }

            let abs = checked_add_u64(
                u64::from(self.partition_start),
                u64::from(extent.lbn),
                "FID sector",
            )?;
            let len = usize::try_from(extent.len).map_err(|_| invalid("FID extent too large"))?;
            let part = self.read_bytes(abs, len)?;
            bytes.extend_from_slice(&part);
        }

        parse_fids(&bytes)
    }

    fn read_sector(&mut self, sector: u64) -> Result<[u8; 2048], UdfError> {
        read_sector(self.file, sector, self.path)
    }

    fn read_bytes(&mut self, sector: u64, len: usize) -> Result<Vec<u8>, UdfError> {
        read_bytes(self.file, sector, len, self.sectors, self.path)
    }
}

struct IsoReader<'a> {
    file: &'a mut File,
    sectors: u64,
    path: &'a str,
}

impl IsoReader<'_> {
    #[allow(clippy::cognitive_complexity)]
    fn visit_directory(
        &mut self,
        prefix: String,
        extent: u32,
        size: u64,
        visited: &mut HashSet<u32>,
        list: &mut Vec<ListedFile>,
    ) -> Result<(), UdfError> {
        if size == 0 {
            return Ok(());
        }
        if !visited.insert(extent) {
            return Ok(());
        }

        check_iso_extent(extent, size, self.sectors)?;
        let len = usize::try_from(size).map_err(|_| invalid("directory too large"))?;
        let data = read_bytes(self.file, extent as u64, len, self.sectors, self.path)?;

        let mut off = 0usize;
        while off < data.len() {
            let rec_len = match read_u8(&data, off) {
                Some(v) => usize::from(v),
                None => break,
            };

            if rec_len == 0 {
                off = next_sector(off);
                continue;
            }
            if rec_len < ISO_DIR_RECORD_MIN_LEN {
                off = next_sector(off);
                continue;
            }

            let rec_end = off
                .checked_add(rec_len)
                .ok_or_else(|| invalid("record overflow"))?;
            if rec_end > data.len() {
                return Err(invalid("record out of range"));
            }

            let child_extent = read_u32(
                &data,
                off.checked_add(ISO_DIR_RECORD_EXTENT_OFFSET)
                    .ok_or_else(|| invalid("short"))?,
            )
            .ok_or_else(|| invalid("short"))?;
            let child_size = u64::from(
                read_u32(
                    &data,
                    off.checked_add(ISO_DIR_RECORD_SIZE_OFFSET)
                        .ok_or_else(|| invalid("short"))?,
                )
                .ok_or_else(|| invalid("short"))?,
            );
            let flags = read_u8(
                &data,
                off.checked_add(ISO_DIR_RECORD_FLAGS_OFFSET)
                    .ok_or_else(|| invalid("short"))?,
            )
            .ok_or_else(|| invalid("short"))?;
            let name_len_raw = usize::from(
                read_u8(
                    &data,
                    off.checked_add(ISO_DIR_RECORD_NAME_LEN_OFFSET)
                        .ok_or_else(|| invalid("short"))?,
                )
                .ok_or_else(|| invalid("short"))?,
            );

            let max_name = rec_len.saturating_sub(ISO_DIR_RECORD_NAME_OFFSET);
            let name_len = name_len_raw.min(max_name);

            let name_start = off
                .checked_add(ISO_DIR_RECORD_NAME_OFFSET)
                .ok_or_else(|| invalid("overflow"))?;
            let name_end = name_start
                .checked_add(name_len)
                .ok_or_else(|| invalid("overflow"))?;
            let name_bytes = data
                .get(name_start..name_end)
                .ok_or_else(|| invalid("short"))?;

            if name_len == 1 {
                let skip = matches!(name_bytes.first().copied(), Some(0 | 1));
                if skip {
                    off = rec_end;
                    continue;
                }
            }

            let mut name = String::from_utf8_lossy(name_bytes).into_owned();
            if let Some(stripped) = name.strip_suffix(";1") {
                name = stripped.to_string();
            }
            let name = safe_path_name(&name);
            if name.is_empty() {
                off = rec_end;
                continue;
            }

            if flags & 0x02 != 0 {
                let child_prefix = format!("{prefix}{name}/");
                self.visit_directory(child_prefix, child_extent, child_size, visited, list)?;
            } else {
                check_iso_extent(child_extent, child_size, self.sectors)?;
                list.push(ListedFile {
                    path: format!("{prefix}{name}"),
                    sector: child_extent,
                    size: child_size,
                });
            }

            off = rec_end;
        }

        Ok(())
    }
}

struct FeInfo {
    is_dir: bool,
    info_len: u64,
    extents: Vec<desc::Extent>,
}

struct ParsedFid {
    name: Option<String>,
    is_dir: bool,
    parent: bool,
    icb_lbn: u32,
}

fn read_vds(
    file: &mut File,
    main_loc: u32,
    main_len: u32,
    sectors: u64,
    path: &str,
) -> Result<(u32, u32, u32), UdfError> {
    let start = u64::from(main_loc);
    let end = checked_add_u64(start, u64::from(main_len), "main VDS end")?;
    if end > sectors {
        return Err(invalid("main VDS out of range"));
    }

    let mut partition_start = None;
    let mut partition_length = None;
    let mut fsd_lbn = None;

    for i in 0..main_len {
        let sector_no = checked_add_u64(start, u64::from(i), "main VDS sector")?;
        let sector = read_sector(file, sector_no, path)?;
        let id = tag_id(&sector, 0)?;
        if id == 0 {
            continue;
        }
        verify_tag_at(&sector, 0)?;

        match id {
            5 => {
                partition_start =
                    Some(read_u32(&sector, PD_START_OFFSET).ok_or_else(|| invalid("short PD"))?);
                partition_length =
                    Some(read_u32(&sector, PD_LENGTH_OFFSET).ok_or_else(|| invalid("short PD"))?);
            }
            6 => {
                fsd_lbn = Some(
                    read_u32(&sector, LVD_FSD_LBN_OFFSET).ok_or_else(|| invalid("short LVD"))?,
                );
            }
            _ => {}
        }
    }

    let partition_start = partition_start.ok_or_else(|| invalid("missing partition descriptor"))?;
    let partition_length =
        partition_length.ok_or_else(|| invalid("missing partition descriptor"))?;
    let fsd_lbn = fsd_lbn.ok_or_else(|| invalid("missing logical volume descriptor"))?;
    Ok((partition_start, partition_length, fsd_lbn))
}

fn parse_file_entry(buf: &[u8]) -> Result<FeInfo, UdfError> {
    verify_tag_at(buf, 0)?;
    if tag_id(buf, 0)? != 261 {
        return Err(invalid("FE tag id mismatch"));
    }

    let crc_len = usize::from(read_u16(buf, 10).ok_or_else(|| invalid("short FE"))?);
    let desc_len = TAG_LEN
        .checked_add(crc_len)
        .ok_or_else(|| invalid("FE overflow"))?;
    if desc_len > buf.len() {
        return Err(invalid("FE out of range"));
    }
    if desc_len < FE_MIN_LEN {
        return Err(invalid("short FE"));
    }

    let file_type = read_u8(buf, FE_FILE_TYPE_OFFSET).ok_or_else(|| invalid("short FE"))?;
    let is_dir = match file_type {
        4 => true,
        5 => false,
        _ => return Err(invalid("bad FE file type")),
    };

    let info_len = read_u64(buf, FE_INFO_LEN_OFFSET).ok_or_else(|| invalid("short FE"))?;
    let l_ea = usize::try_from(read_u32(buf, FE_L_EA_OFFSET).ok_or_else(|| invalid("short FE"))?)
        .map_err(|_| invalid("FE overflow"))?;
    let l_ad = usize::try_from(read_u32(buf, FE_L_AD_OFFSET).ok_or_else(|| invalid("short FE"))?)
        .map_err(|_| invalid("FE overflow"))?;

    if l_ad % 8 != 0 {
        return Err(invalid("bad FE allocation length"));
    }

    let ad_start = FE_AD_START_OFFSET
        .checked_add(l_ea)
        .ok_or_else(|| invalid("FE overflow"))?;
    let ad_end = ad_start
        .checked_add(l_ad)
        .ok_or_else(|| invalid("FE overflow"))?;
    if ad_end > desc_len {
        return Err(invalid("FE extents out of range"));
    }

    let count = l_ad / 8;
    if count > 1000 {
        return Err(invalid("too many extents"));
    }

    let mut extents = Vec::with_capacity(count);
    for i in 0..count {
        let off = ad_start
            .checked_add(i.checked_mul(8).ok_or_else(|| invalid("extent overflow"))?)
            .ok_or_else(|| invalid("extent overflow"))?;
        let len = read_u32(buf, off).ok_or_else(|| invalid("short extent"))?;
        let loc = read_u32(
            buf,
            off.checked_add(4).ok_or_else(|| invalid("short extent"))?,
        )
        .ok_or_else(|| invalid("short extent"))?;
        extents.push(desc::Extent { lbn: loc, len });
    }

    Ok(FeInfo {
        is_dir,
        info_len,
        extents,
    })
}

fn parse_fids(buf: &[u8]) -> Result<Vec<ParsedFid>, UdfError> {
    let mut out = Vec::new();
    let mut off = 0usize;

    while off < buf.len() {
        if off
            .checked_add(TAG_LEN)
            .ok_or_else(|| invalid("overflow"))?
            > buf.len()
        {
            break;
        }

        let id = read_u16(buf, off).ok_or_else(|| invalid("short FID"))?;
        if id == 0 {
            break;
        }
        if id != 257 {
            return Err(invalid("FID tag id mismatch"));
        }

        let parsed = parse_fid_at(buf, off)?;
        out.push(parsed.0);
        off = parsed.1;
    }

    Ok(out)
}

fn parse_fid_at(buf: &[u8], off: usize) -> Result<(ParsedFid, usize), UdfError> {
    verify_tag_at(buf, off)?;
    if tag_id(buf, off)? != 257 {
        return Err(invalid("FID tag id mismatch"));
    }

    let crc_len = usize::from(
        read_u16(
            buf,
            off.checked_add(10).ok_or_else(|| invalid("short FID"))?,
        )
        .ok_or_else(|| invalid("short FID"))?,
    );
    let total = TAG_LEN
        .checked_add(crc_len)
        .ok_or_else(|| invalid("FID overflow"))?;
    if total < FID_MIN_LEN {
        return Err(invalid("short FID"));
    }

    let end = off
        .checked_add(total)
        .ok_or_else(|| invalid("FID overflow"))?;
    if end > buf.len() {
        return Err(invalid("FID out of range"));
    }

    let characteristics = read_u8(
        buf,
        off.checked_add(FID_CHARACTERISTICS_OFFSET)
            .ok_or_else(|| invalid("short FID"))?,
    )
    .ok_or_else(|| invalid("short FID"))?;
    let l_fi = usize::from(
        read_u8(
            buf,
            off.checked_add(FID_L_FI_OFFSET)
                .ok_or_else(|| invalid("short FID"))?,
        )
        .ok_or_else(|| invalid("short FID"))?,
    );
    let l_iu = usize::from(
        read_u16(
            buf,
            off.checked_add(FID_L_IU_OFFSET)
                .ok_or_else(|| invalid("short FID"))?,
        )
        .ok_or_else(|| invalid("short FID"))?,
    );
    let icb_lbn = read_u32(
        buf,
        off.checked_add(FID_ICB_LBN_OFFSET)
            .ok_or_else(|| invalid("short FID"))?,
    )
    .ok_or_else(|| invalid("short FID"))?;

    let ident_start = off
        .checked_add(FID_IDENT_OFFSET)
        .and_then(|x| x.checked_add(l_iu))
        .ok_or_else(|| invalid("FID overflow"))?;
    let ident_end = ident_start
        .checked_add(l_fi)
        .ok_or_else(|| invalid("FID overflow"))?;
    if ident_end > end {
        return Err(invalid("FID identifier out of range"));
    }

    let name = if l_fi == 0 {
        None
    } else {
        let ident = buf
            .get(ident_start..ident_end)
            .ok_or_else(|| invalid("short FID"))?;
        Some(decode_fid_ident(ident))
    };

    let next = if end % 4 == 0 {
        end
    } else {
        end.checked_add(3)
            .map(|x| x & !3)
            .ok_or_else(|| invalid("FID align"))?
    };
    if next <= off {
        return Err(invalid("FID alignment"));
    }

    Ok((
        ParsedFid {
            name,
            is_dir: characteristics & 0x02 != 0,
            parent: characteristics & 0x08 != 0,
            icb_lbn,
        },
        next,
    ))
}

fn decode_fid_ident(ident: &[u8]) -> String {
    let body = if ident.is_empty() { &[] } else { &ident[1..] };
    String::from_utf8_lossy(body).into_owned()
}

fn verify_pvd(buf: &[u8]) -> Result<(), UdfError> {
    let typ = read_u8(buf, 0).ok_or_else(|| invalid("short PVD"))?;
    if typ != 1 {
        return Err(invalid("bad PVD type"));
    }
    let id = buf.get(1..6).ok_or_else(|| invalid("short PVD"))?;
    if id != b"CD001" {
        return Err(invalid("bad PVD id"));
    }
    let ver = read_u8(buf, 6).ok_or_else(|| invalid("short PVD"))?;
    if ver != 1 {
        return Err(invalid("bad PVD version"));
    }
    Ok(())
}

fn verify_tag_at(buf: &[u8], off: usize) -> Result<(), UdfError> {
    let tag = buf
        .get(
            off..off
                .checked_add(TAG_LEN)
                .ok_or_else(|| invalid("short tag"))?,
        )
        .ok_or_else(|| invalid("short tag"))?;

    let crc_len = usize::from(read_u16(tag, 10).ok_or_else(|| invalid("short tag"))?);
    let body_start = off
        .checked_add(TAG_LEN)
        .ok_or_else(|| invalid("tag overflow"))?;
    let body_end = body_start
        .checked_add(crc_len)
        .ok_or_else(|| invalid("tag overflow"))?;
    let body = buf
        .get(body_start..body_end)
        .ok_or_else(|| invalid("short tag body"))?;

    let stored_crc = read_u16(tag, 8).ok_or_else(|| invalid("short tag"))?;
    if desc::crc_itu(body) != stored_crc {
        return Err(invalid("bad tag CRC"));
    }

    let mut sum = 0u32;
    for b in tag.get(0..4).ok_or_else(|| invalid("short tag"))? {
        sum = sum.wrapping_add(u32::from(*b));
    }
    for b in tag.get(5..16).ok_or_else(|| invalid("short tag"))? {
        sum = sum.wrapping_add(u32::from(*b));
    }

    let checksum = tag.get(4).copied().ok_or_else(|| invalid("short tag"))?;
    if checksum != (sum & 0xFF) as u8 {
        return Err(invalid("bad tag checksum"));
    }

    Ok(())
}

fn tag_id(buf: &[u8], off: usize) -> Result<u16, UdfError> {
    read_u16(buf, off).ok_or_else(|| invalid("short tag"))
}

fn read_sector(file: &mut File, sector: u64, path: &str) -> Result<[u8; 2048], UdfError> {
    let offset = checked_mul_u64(sector, SECTOR_SIZE, "sector offset")?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| io_error(path, e))?;
    let mut buf = [0u8; 2048];
    file.read_exact(&mut buf).map_err(|e| io_error(path, e))?;
    Ok(buf)
}

fn read_bytes(
    file: &mut File,
    sector: u64,
    len: usize,
    sectors: u64,
    path: &str,
) -> Result<Vec<u8>, UdfError> {
    if len == 0 {
        return Ok(Vec::new());
    }
    if len > MAX_READ_BYTES {
        return Err(invalid("read too large"));
    }

    let offset = checked_mul_u64(sector, SECTOR_SIZE, "sector offset")?;
    let len_u64 = u64::try_from(len).map_err(|_| invalid("read overflow"))?;
    let end = offset
        .checked_add(len_u64)
        .ok_or_else(|| invalid("read overflow"))?;
    let file_len = checked_mul_u64(sectors, SECTOR_SIZE, "file length")?;
    if end > file_len {
        return Err(invalid("read out of range"));
    }

    file.seek(SeekFrom::Start(offset))
        .map_err(|e| io_error(path, e))?;
    let mut buf = vec![0u8; len];
    file.read_exact(&mut buf).map_err(|e| io_error(path, e))?;
    Ok(buf)
}

fn file_sectors(file: &mut File, path: &str) -> Result<u64, UdfError> {
    let meta = file.metadata().map_err(|e| io_error(path, e))?;
    Ok(meta.len() / SECTOR_SIZE)
}

fn check_extent(lbn: u32, len: u32, partition_length: u32) -> Result<(), UdfError> {
    if lbn > partition_length {
        return Err(invalid("extent LBN out of partition"));
    }
    if len == 0 {
        return Ok(());
    }
    let sectors = u64::from(len).div_ceil(SECTOR_SIZE);
    let end = checked_add_u64(u64::from(lbn), sectors, "extent end")?;
    if end > u64::from(partition_length) {
        return Err(invalid("extent out of partition"));
    }
    Ok(())
}

fn check_iso_extent(extent: u32, size: u64, sectors: u64) -> Result<(), UdfError> {
    if u64::from(extent) >= sectors {
        return Err(invalid("ISO extent out of range"));
    }
    let needed = size.div_ceil(SECTOR_SIZE);
    let end = checked_add_u64(u64::from(extent), needed, "ISO extent end")?;
    if end > sectors {
        return Err(invalid("ISO extent out of range"));
    }
    Ok(())
}

fn safe_path_name(name: &str) -> String {
    name.chars()
        .map(|c| if c == '/' { '_' } else { c })
        .collect()
}

fn next_sector(off: usize) -> usize {
    let sector = SECTOR_SIZE_USIZE;
    ((off / sector).saturating_add(1)).saturating_mul(sector)
}

fn read_u8(buf: &[u8], off: usize) -> Option<u8> {
    buf.get(off).copied()
}

fn read_u16(buf: &[u8], off: usize) -> Option<u16> {
    let end = off.checked_add(2)?;
    let s = buf.get(off..end)?;
    let mut a = [0u8; 2];
    a.copy_from_slice(s);
    Some(u16::from_le_bytes(a))
}

fn read_u32(buf: &[u8], off: usize) -> Option<u32> {
    let end = off.checked_add(4)?;
    let s = buf.get(off..end)?;
    let mut a = [0u8; 4];
    a.copy_from_slice(s);
    Some(u32::from_le_bytes(a))
}

fn read_u64(buf: &[u8], off: usize) -> Option<u64> {
    let end = off.checked_add(8)?;
    let s = buf.get(off..end)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(s);
    Some(u64::from_le_bytes(a))
}

fn checked_add_u64(a: u64, b: u64, what: &str) -> Result<u64, UdfError> {
    a.checked_add(b)
        .ok_or_else(|| invalid(format!("{what} overflow")))
}

fn checked_mul_u64(a: u64, b: u64, what: &str) -> Result<u64, UdfError> {
    a.checked_mul(b)
        .ok_or_else(|| invalid(format!("{what} overflow")))
}

fn invalid(message: impl Into<String>) -> UdfError {
    UdfError::Invalid(message.into())
}

fn io_error(path: &str, err: std::io::Error) -> UdfError {
    UdfError::Io {
        path: path.to_string(),
        message: err.to_string(),
    }
}
