//! DVD-Video ISO image writer (UDF 1.02 + ISO 9660 bridge).

use crate::desc::{self, Extent, Fid};
use crate::iso9660::{self, DirEntry, PathEntry, PvdParams};
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

const SECTOR_SIZE: u64 = 2048;
const SECTOR_SIZE_U32: u32 = 2048;
const PARTITION_START_BASE: u32 = 262;
const MAIN_VDS_SECTOR: u32 = 32;
const RESERVE_VDS_SECTOR: u32 = 48;
const LVID_SECTOR: u32 = 64;
const LVID_TD_SECTOR: u32 = 65;
const ANCHOR_SECTOR: u32 = 256;
const VDS_EXTENT_BYTES: u32 = 16 * 2048;
const LVID_EXTENT_BYTES: u32 = 2 * 2048;

static ZERO_CHUNK: [u8; 65536] = [0u8; 65536];

/// One file to place in `VIDEO_TS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageFile {
    /// File name, for example `VIDEO_TS.IFO`.
    pub name: String,
    /// Source file path.
    pub source: PathBuf,
}

/// Summary of a written image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IsoReport {
    /// Total sectors in the image.
    pub sectors: u32,
    /// Absolute sector of the first file data extent.
    pub first_file_sector: u32,
}

/// Errors produced by image writing and reading.
#[derive(Debug)]
pub enum UdfError {
    /// I/O error.
    Io {
        /// Path involved.
        path: String,
        /// Error message.
        message: String,
    },
    /// Invalid image or input.
    Invalid(String),
}

impl fmt::Display for UdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, message } => write!(f, "I/O error for {path}: {message}"),
            Self::Invalid(message) => write!(f, "invalid image: {message}"),
        }
    }
}

impl Error for UdfError {}

#[allow(dead_code)] // layout fields kept for diagnostics
struct FileMeta {
    name: String,
    iso_name: String,
    source: PathBuf,
    size: u64,
    fe_lbn: u32,
    data_lbn: u32,
    data_sectors: u32,
    iso_extent: u32,
}

struct VideoItem {
    name: String,
    fe_lbn: u32,
    iso_extent: u32,
    size: u64,
}

#[allow(dead_code)] // layout fields kept for diagnostics
struct IsoSizes {
    root_size: u64,
    audio_size: u64,
    video_size: u64,
    video_sectors: u32,
    partition_start: u32,
    path_table_size: u32,
}

struct UdfSizes {
    root_fids_sectors: u32,
    audio_fids_sectors: u32,
    video_fids_sectors: u32,
}

#[allow(dead_code)] // layout fields kept for diagnostics
struct UdfLbns {
    fsd: u32,
    td: u32,
    root_fe: u32,
    root_fids: u32,
    audio_fe: u32,
    audio_fids: u32,
    video_fe: u32,
    video_fids: u32,
    file_data: u32,
}

#[allow(dead_code)] // layout fields kept for diagnostics
struct Plan {
    lbns: UdfLbns,
    partition_start: u32,
    partition_length: u32,
    file_data_lbn: u32,
    data_sectors: u32,
    total_sectors: u32,
    first_file_sector: u32,
    file_count: u32,
    next_unique_id: u64,
}

struct IsoBytes {
    pvd: [u8; 2048],
    term: [u8; 2048],
    path_l: Vec<u8>,
    path_m: Vec<u8>,
    root_dir: Vec<u8>,
    audio_dir: Vec<u8>,
    video_dir: Vec<u8>,
}

/// Order DVD-Video file names according to the usual `VIDEO_TS` sequence.
pub fn dvd_file_order(names: &[String]) -> Vec<String> {
    let mut out = names.to_vec();
    out.sort_by_key(|a| order_key(a));
    out
}

/// Write a DVD-Video ISO image.
pub fn write_dvd_iso(
    video_ts: &[ImageFile],
    label: &str,
    out: &Path,
    unix_time: i64,
) -> Result<IsoReport, UdfError> {
    let out_path = out.to_string_lossy().into_owned();
    let mut metas = prepare_files(video_ts)?;
    let date = iso9660::ecma_date(unix_time);

    let iso_sizes = compute_iso_sizes(&metas, date)?;
    let fid_sizes = compute_fid_sizes(&metas)?;
    let plan = allocate_plan(iso_sizes.partition_start, &mut metas, &fid_sizes)?;
    let iso_bytes = build_iso_bytes(label, &plan, &metas, &iso_sizes, date, unix_time)?;

    let file = File::create(out).map_err(|e| io_error(&out_path, e))?;
    let mut writer = BufWriter::new(file);
    let mut cursor = 0u64;

    write_iso_sectors(
        &mut writer,
        &mut cursor,
        &iso_bytes,
        &plan,
        label,
        unix_time,
        &out_path,
    )?;
    write_udf_sectors(
        &mut writer,
        &mut cursor,
        &plan,
        label,
        unix_time,
        &metas,
        &out_path,
    )?;
    write_file_data(&mut writer, &mut cursor, &plan, &metas, &out_path)?;
    write_final_anchor(&mut writer, &mut cursor, &plan, &out_path)?;

    writer.flush().map_err(|e| io_error(&out_path, e))?;

    Ok(IsoReport {
        sectors: plan.total_sectors,
        first_file_sector: plan.first_file_sector,
    })
}

fn prepare_files(video_ts: &[ImageFile]) -> Result<Vec<FileMeta>, UdfError> {
    let mut metas = Vec::with_capacity(video_ts.len());

    for item in video_ts {
        let size = file_size(&item.source)?;
        let iso_name = iso9660::sanitize_file_name(&item.name);
        metas.push(FileMeta {
            name: item.name.clone(),
            iso_name,
            source: item.source.clone(),
            size,
            fe_lbn: 0,
            data_lbn: 0,
            data_sectors: 0,
            iso_extent: 0,
        });
    }

    Ok(metas)
}

fn file_size(path: &Path) -> Result<u64, UdfError> {
    let path_str = path.to_string_lossy().into_owned();
    let meta = std::fs::metadata(path).map_err(|e| io_error(&path_str, e))?;
    Ok(meta.len())
}

fn compute_iso_sizes(metas: &[FileMeta], date: [u8; 7]) -> Result<IsoSizes, UdfError> {
    let audio_entries: Vec<DirEntry> = Vec::new();
    let video_entries: Vec<DirEntry> = metas
        .iter()
        .map(|m| DirEntry {
            name: m.iso_name.clone(),
            extent: 0,
            size: m.size,
            is_dir: false,
        })
        .collect();

    let audio_size = iso9660::directory_records(
        iso9660::AUDIO_DIR_SECTOR,
        0,
        iso9660::ROOT_DIR_SECTOR,
        0,
        &audio_entries,
        date,
    )
    .len() as u64;

    let video_size = iso9660::directory_records(
        iso9660::VIDEO_DIR_SECTOR,
        0,
        iso9660::ROOT_DIR_SECTOR,
        0,
        &video_entries,
        date,
    )
    .len() as u64;

    let root_entries = vec![
        DirEntry {
            name: String::from("AUDIO_TS"),
            extent: iso9660::AUDIO_DIR_SECTOR,
            size: audio_size,
            is_dir: true,
        },
        DirEntry {
            name: String::from("VIDEO_TS"),
            extent: iso9660::VIDEO_DIR_SECTOR,
            size: video_size,
            is_dir: true,
        },
    ];

    let root_size = iso9660::directory_records(
        iso9660::ROOT_DIR_SECTOR,
        0,
        iso9660::ROOT_DIR_SECTOR,
        0,
        &root_entries,
        date,
    )
    .len() as u64;

    let video_sectors = u32_from_u64(video_size / SECTOR_SIZE, "VIDEO_TS ISO sectors")?;
    let after_video = checked_add_u32(
        iso9660::VIDEO_DIR_SECTOR,
        video_sectors,
        "ISO directory end",
    )?;
    let partition_start = if after_video > PARTITION_START_BASE {
        after_video
    } else {
        PARTITION_START_BASE
    };

    let path_entries = vec![
        PathEntry {
            name: vec![0u8],
            extent: iso9660::ROOT_DIR_SECTOR,
            parent: 1,
        },
        PathEntry {
            name: b"AUDIO_TS".to_vec(),
            extent: iso9660::AUDIO_DIR_SECTOR,
            parent: 1,
        },
        PathEntry {
            name: b"VIDEO_TS".to_vec(),
            extent: iso9660::VIDEO_DIR_SECTOR,
            parent: 1,
        },
    ];

    let path_table = iso9660::path_table(&path_entries, false);
    let path_table_size = u32_from_u64(path_table.len() as u64, "path table size")?;

    Ok(IsoSizes {
        root_size,
        audio_size,
        video_size,
        video_sectors,
        partition_start,
        path_table_size,
    })
}

fn compute_fid_sizes(metas: &[FileMeta]) -> Result<UdfSizes, UdfError> {
    let root_fids = vec![
        Fid {
            name: None,
            is_dir: true,
            icb_lbn: 0,
        },
        Fid {
            name: Some(String::from("AUDIO_TS")),
            is_dir: true,
            icb_lbn: 0,
        },
        Fid {
            name: Some(String::from("VIDEO_TS")),
            is_dir: true,
            icb_lbn: 0,
        },
    ];

    let audio_fids = vec![Fid {
        name: None,
        is_dir: true,
        icb_lbn: 0,
    }];

    let mut video_names: Vec<String> = metas.iter().map(|m| m.iso_name.clone()).collect();
    video_names.sort();

    let mut video_fids = vec![Fid {
        name: None,
        is_dir: true,
        icb_lbn: 0,
    }];
    for name in video_names {
        video_fids.push(Fid {
            name: Some(name),
            is_dir: false,
            icb_lbn: 0,
        });
    }

    let root_len = desc::file_identifier_descriptors(0, &root_fids).len();
    let audio_len = desc::file_identifier_descriptors(0, &audio_fids).len();
    let video_len = desc::file_identifier_descriptors(0, &video_fids).len();

    Ok(UdfSizes {
        root_fids_sectors: fid_sector_count(root_len)?,
        audio_fids_sectors: fid_sector_count(audio_len)?,
        video_fids_sectors: fid_sector_count(video_len)?,
    })
}

fn allocate_plan(
    partition_start: u32,
    metas: &mut [FileMeta],
    sizes: &UdfSizes,
) -> Result<Plan, UdfError> {
    let mut lbn = 0u32;

    let fsd = lbn;
    lbn = inc_u32(lbn, "UDF LBN")?;
    let td = lbn;
    lbn = inc_u32(lbn, "UDF LBN")?;
    let root_fe = lbn;
    lbn = inc_u32(lbn, "UDF LBN")?;
    let root_fids = lbn;
    lbn = checked_add_u32(lbn, sizes.root_fids_sectors, "UDF LBN")?;
    let audio_fe = lbn;
    lbn = inc_u32(lbn, "UDF LBN")?;
    let audio_fids = lbn;
    lbn = checked_add_u32(lbn, sizes.audio_fids_sectors, "UDF LBN")?;
    let video_fe = lbn;
    lbn = inc_u32(lbn, "UDF LBN")?;
    let video_fids = lbn;
    lbn = checked_add_u32(lbn, sizes.video_fids_sectors, "UDF LBN")?;

    for meta in metas.iter_mut() {
        meta.fe_lbn = lbn;
        lbn = inc_u32(lbn, "UDF LBN")?;
    }

    let file_data = lbn;
    let mut data_lbn = file_data;
    let mut data_sectors = 0u32;

    for meta in metas.iter_mut() {
        let sectors = sectors_for_size(meta.size)?;
        let sectors = u32_from_u64(sectors, "file data sectors")?;
        meta.data_lbn = data_lbn;
        meta.data_sectors = sectors;
        meta.iso_extent = checked_add_u32(partition_start, data_lbn, "ISO file extent")?;
        data_lbn = checked_add_u32(data_lbn, sectors, "file data LBN")?;
        data_sectors = checked_add_u32(data_sectors, sectors, "file data sectors")?;
    }

    let partition_length = checked_add_u32(file_data, data_sectors, "partition length")?;
    let total = checked_add_u64(
        partition_start as u64,
        partition_length as u64,
        "total sectors",
    )?;
    let total = total
        .checked_add(1)
        .ok_or_else(|| invalid("total sectors overflow"))?;
    let total = u32_from_u64(total, "total sectors")?;

    let first_file_sector = checked_add_u32(partition_start, file_data, "first file sector")?;
    let file_count = u32_from_u64(metas.len() as u64, "file count")?;
    let next_unique_id = 18u64
        .checked_add(metas.len() as u64)
        .ok_or_else(|| invalid("unique id overflow"))?;

    Ok(Plan {
        lbns: UdfLbns {
            fsd,
            td,
            root_fe,
            root_fids,
            audio_fe,
            audio_fids,
            video_fe,
            video_fids,
            file_data,
        },
        partition_start,
        partition_length,
        file_data_lbn: file_data,
        data_sectors,
        total_sectors: total,
        first_file_sector,
        file_count,
        next_unique_id,
    })
}

fn build_iso_bytes(
    label: &str,
    plan: &Plan,
    metas: &[FileMeta],
    sizes: &IsoSizes,
    date: [u8; 7],
    unix_time: i64,
) -> Result<IsoBytes, UdfError> {
    let video_items = build_video_items(metas);
    let video_entries = final_video_entries(&video_items);

    let root_entries = vec![
        DirEntry {
            name: String::from("AUDIO_TS"),
            extent: iso9660::AUDIO_DIR_SECTOR,
            size: sizes.audio_size,
            is_dir: true,
        },
        DirEntry {
            name: String::from("VIDEO_TS"),
            extent: iso9660::VIDEO_DIR_SECTOR,
            size: sizes.video_size,
            is_dir: true,
        },
    ];

    let root_dir = iso9660::directory_records(
        iso9660::ROOT_DIR_SECTOR,
        sizes.root_size,
        iso9660::ROOT_DIR_SECTOR,
        sizes.root_size,
        &root_entries,
        date,
    );

    let audio_dir = iso9660::directory_records(
        iso9660::AUDIO_DIR_SECTOR,
        sizes.audio_size,
        iso9660::ROOT_DIR_SECTOR,
        sizes.root_size,
        &[],
        date,
    );

    let video_dir = iso9660::directory_records(
        iso9660::VIDEO_DIR_SECTOR,
        sizes.video_size,
        iso9660::ROOT_DIR_SECTOR,
        sizes.root_size,
        &video_entries,
        date,
    );

    let path_entries = vec![
        PathEntry {
            name: vec![0u8],
            extent: iso9660::ROOT_DIR_SECTOR,
            parent: 1,
        },
        PathEntry {
            name: b"AUDIO_TS".to_vec(),
            extent: iso9660::AUDIO_DIR_SECTOR,
            parent: 1,
        },
        PathEntry {
            name: b"VIDEO_TS".to_vec(),
            extent: iso9660::VIDEO_DIR_SECTOR,
            parent: 1,
        },
    ];

    let path_l = iso9660::path_table(&path_entries, false);
    let path_m = iso9660::path_table(&path_entries, true);
    let volume_id = iso9660::sanitize_volume_id(label);

    let params = PvdParams {
        volume_id: &volume_id,
        volume_space_size: plan.total_sectors,
        path_table_size: sizes.path_table_size,
        root_extent: iso9660::ROOT_DIR_SECTOR,
        root_size: sizes.root_size,
        creation_time: unix_time,
        modification_time: unix_time,
    };

    Ok(IsoBytes {
        pvd: iso9660::pvd(&params),
        term: iso9660::terminator(),
        path_l,
        path_m,
        root_dir,
        audio_dir,
        video_dir,
    })
}

fn write_iso_sectors<W: Write>(
    w: &mut W,
    cursor: &mut u64,
    bytes: &IsoBytes,
    plan: &Plan,
    label: &str,
    unix_time: i64,
    path: &str,
) -> Result<(), UdfError> {
    write_iso_head(w, cursor, bytes, path)?;
    write_vds_regions(w, cursor, plan, label, unix_time, path)?;
    write_iso_bridge(w, cursor, bytes, path)?;
    Ok(())
}

fn write_iso_head<W: Write>(
    w: &mut W,
    cursor: &mut u64,
    bytes: &IsoBytes,
    path: &str,
) -> Result<(), UdfError> {
    write_zeros_to(w, cursor, sector_bytes(iso9660::PVD_SECTOR)?, path)?;
    write_sector(w, cursor, iso9660::PVD_SECTOR, &bytes.pvd, path)?;
    write_sector(w, cursor, iso9660::TERMINATOR_SECTOR, &bytes.term, path)?;

    write_sector(w, cursor, 18, &desc::vrs_sector(b"BEA01"), path)?;
    write_sector(w, cursor, 19, &desc::vrs_sector(b"NSR02"), path)?;
    write_sector(w, cursor, 20, &desc::vrs_sector(b"TEA01"), path)?;
    Ok(())
}

fn write_vds_regions<W: Write>(
    w: &mut W,
    cursor: &mut u64,
    plan: &Plan,
    label: &str,
    unix_time: i64,
    path: &str,
) -> Result<(), UdfError> {
    write_zeros_to(w, cursor, sector_bytes(MAIN_VDS_SECTOR)?, path)?;
    write_vds_set(w, cursor, MAIN_VDS_SECTOR, plan, label, unix_time, path)?;

    write_zeros_to(w, cursor, sector_bytes(RESERVE_VDS_SECTOR)?, path)?;
    write_vds_set(w, cursor, RESERVE_VDS_SECTOR, plan, label, unix_time, path)?;

    write_zeros_to(w, cursor, sector_bytes(LVID_SECTOR)?, path)?;
    let lvid = desc::logical_volume_integrity(
        LVID_SECTOR,
        unix_time,
        plan.next_unique_id,
        plan.file_count,
        3,
        plan.partition_length as u64,
    );
    write_sector(w, cursor, LVID_SECTOR, &lvid, path)?;

    let td = desc::terminating_descriptor(LVID_TD_SECTOR);
    write_sector(w, cursor, LVID_TD_SECTOR, &td, path)?;

    write_zeros_to(w, cursor, sector_bytes(ANCHOR_SECTOR)?, path)?;
    let anchor = desc::anchor(
        ANCHOR_SECTOR,
        (VDS_EXTENT_BYTES, MAIN_VDS_SECTOR),
        (VDS_EXTENT_BYTES, RESERVE_VDS_SECTOR),
    );
    write_sector(w, cursor, ANCHOR_SECTOR, &anchor, path)?;
    Ok(())
}

fn write_vds_set<W: Write>(
    w: &mut W,
    cursor: &mut u64,
    start: u32,
    plan: &Plan,
    label: &str,
    unix_time: i64,
    path: &str,
) -> Result<(), UdfError> {
    let pvd = desc::primary_volume_descriptor(start, label, "*DiscCraft", unix_time);
    write_sector(w, cursor, start, &pvd, path)?;

    let iuvd = desc::implementation_use_volume_descriptor(checked_add_u32(start, 1, "VDS sector")?);
    write_sector(
        w,
        cursor,
        checked_add_u32(start, 1, "VDS sector")?,
        &iuvd,
        path,
    )?;

    let pd_loc = checked_add_u32(start, 2, "VDS sector")?;
    let pd = desc::partition_descriptor(pd_loc, plan.partition_start, plan.partition_length);
    write_sector(w, cursor, pd_loc, &pd, path)?;

    let lvd_loc = checked_add_u32(start, 3, "VDS sector")?;
    let lvd = desc::logical_volume_descriptor(lvd_loc, label, 0, (LVID_EXTENT_BYTES, LVID_SECTOR));
    write_sector(w, cursor, lvd_loc, &lvd, path)?;

    let usd_loc = checked_add_u32(start, 4, "VDS sector")?;
    let usd = desc::unallocated_space_descriptor(usd_loc);
    write_sector(w, cursor, usd_loc, &usd, path)?;

    let td_loc = checked_add_u32(start, 5, "VDS sector")?;
    let td = desc::terminating_descriptor(td_loc);
    write_sector(w, cursor, td_loc, &td, path)?;
    Ok(())
}

fn write_iso_bridge<W: Write>(
    w: &mut W,
    cursor: &mut u64,
    bytes: &IsoBytes,
    path: &str,
) -> Result<(), UdfError> {
    write_bytes_at(w, cursor, iso9660::PATH_TABLE_L_SECTOR, &bytes.path_l, path)?;
    write_bytes_at(w, cursor, iso9660::PATH_TABLE_M_SECTOR, &bytes.path_m, path)?;
    write_bytes_at(w, cursor, iso9660::ROOT_DIR_SECTOR, &bytes.root_dir, path)?;
    write_bytes_at(w, cursor, iso9660::AUDIO_DIR_SECTOR, &bytes.audio_dir, path)?;
    write_bytes_at(w, cursor, iso9660::VIDEO_DIR_SECTOR, &bytes.video_dir, path)?;
    Ok(())
}

fn write_udf_sectors<W: Write>(
    w: &mut W,
    cursor: &mut u64,
    plan: &Plan,
    label: &str,
    unix_time: i64,
    metas: &[FileMeta],
    path: &str,
) -> Result<(), UdfError> {
    write_zeros_to(w, cursor, sector_bytes(plan.partition_start)?, path)?;

    let root_fids = build_root_fids_bytes(plan)?;
    let audio_fids = build_audio_fids_bytes(plan)?;
    let video_fids = build_video_fids_bytes(plan, metas)?;

    let fsd_loc = checked_add_u32(plan.partition_start, plan.lbns.fsd, "FSD sector")?;
    let fsd = desc::file_set_descriptor(fsd_loc, unix_time, label, plan.lbns.root_fe);
    write_sector(w, cursor, fsd_loc, &fsd, path)?;

    let td_loc = checked_add_u32(plan.partition_start, plan.lbns.td, "TD sector")?;
    let td = desc::terminating_descriptor(td_loc);
    write_sector(w, cursor, td_loc, &td, path)?;

    let root_fe_loc = checked_add_u32(plan.partition_start, plan.lbns.root_fe, "root FE sector")?;
    let root_fe = directory_entry_bytes(
        plan.partition_start,
        plan.lbns.root_fe,
        plan.lbns.root_fids,
        root_fids.len(),
        0,
        unix_time,
    )?;
    write_sector(w, cursor, root_fe_loc, &root_fe, path)?;
    write_bytes_at(
        w,
        cursor,
        checked_add_u32(plan.partition_start, plan.lbns.root_fids, "root FID sector")?,
        &root_fids,
        path,
    )?;

    let audio_fe_loc =
        checked_add_u32(plan.partition_start, plan.lbns.audio_fe, "audio FE sector")?;
    let audio_fe = directory_entry_bytes(
        plan.partition_start,
        plan.lbns.audio_fe,
        plan.lbns.audio_fids,
        audio_fids.len(),
        16,
        unix_time,
    )?;
    write_sector(w, cursor, audio_fe_loc, &audio_fe, path)?;
    write_bytes_at(
        w,
        cursor,
        checked_add_u32(
            plan.partition_start,
            plan.lbns.audio_fids,
            "audio FID sector",
        )?,
        &audio_fids,
        path,
    )?;

    let video_fe_loc =
        checked_add_u32(plan.partition_start, plan.lbns.video_fe, "video FE sector")?;
    let video_fe = directory_entry_bytes(
        plan.partition_start,
        plan.lbns.video_fe,
        plan.lbns.video_fids,
        video_fids.len(),
        17,
        unix_time,
    )?;
    write_sector(w, cursor, video_fe_loc, &video_fe, path)?;
    write_bytes_at(
        w,
        cursor,
        checked_add_u32(
            plan.partition_start,
            plan.lbns.video_fids,
            "video FID sector",
        )?,
        &video_fids,
        path,
    )?;

    let mut unique = 18u64;
    for meta in metas {
        let fe_loc = checked_add_u32(plan.partition_start, meta.fe_lbn, "file FE sector")?;
        let fe = file_entry_bytes(plan.partition_start, meta, unique, unix_time)?;
        write_sector(w, cursor, fe_loc, &fe, path)?;
        unique = unique
            .checked_add(1)
            .ok_or_else(|| invalid("unique id overflow"))?;
    }

    Ok(())
}

fn write_file_data<W: Write>(
    w: &mut W,
    cursor: &mut u64,
    plan: &Plan,
    metas: &[FileMeta],
    path: &str,
) -> Result<(), UdfError> {
    let mut target = plan.first_file_sector;

    for meta in metas {
        write_zeros_to(w, cursor, sector_bytes(target)?, path)?;
        copy_file(w, &meta.source, meta.size, path)?;

        let alloc_bytes =
            checked_mul_u64(meta.data_sectors as u64, SECTOR_SIZE, "file allocation")?;
        let pad = alloc_bytes
            .checked_sub(meta.size)
            .ok_or_else(|| invalid("file padding underflow"))?;
        write_zero_bytes(w, pad, path)?;
        *cursor = cursor.saturating_add(alloc_bytes);

        target = checked_add_u32(target, meta.data_sectors, "next file sector")?;
    }

    Ok(())
}

fn write_final_anchor<W: Write>(
    w: &mut W,
    cursor: &mut u64,
    plan: &Plan,
    path: &str,
) -> Result<(), UdfError> {
    let last = plan
        .total_sectors
        .checked_sub(1)
        .ok_or_else(|| invalid("empty image"))?;
    let anchor = desc::anchor(
        last,
        (VDS_EXTENT_BYTES, MAIN_VDS_SECTOR),
        (VDS_EXTENT_BYTES, RESERVE_VDS_SECTOR),
    );
    write_sector(w, cursor, last, &anchor, path)?;
    Ok(())
}

fn build_root_fids_bytes(plan: &Plan) -> Result<Vec<u8>, UdfError> {
    let first = checked_add_u32(plan.partition_start, plan.lbns.root_fids, "root FID LBN")?;
    let fids = vec![
        Fid {
            name: None,
            is_dir: true,
            icb_lbn: plan.lbns.root_fe,
        },
        Fid {
            name: Some(String::from("AUDIO_TS")),
            is_dir: true,
            icb_lbn: plan.lbns.audio_fe,
        },
        Fid {
            name: Some(String::from("VIDEO_TS")),
            is_dir: true,
            icb_lbn: plan.lbns.video_fe,
        },
    ];
    Ok(desc::file_identifier_descriptors(first, &fids))
}

fn build_audio_fids_bytes(plan: &Plan) -> Result<Vec<u8>, UdfError> {
    let first = checked_add_u32(plan.partition_start, plan.lbns.audio_fids, "audio FID LBN")?;
    let fids = vec![Fid {
        name: None,
        is_dir: true,
        icb_lbn: plan.lbns.root_fe,
    }];
    Ok(desc::file_identifier_descriptors(first, &fids))
}

fn build_video_fids_bytes(plan: &Plan, metas: &[FileMeta]) -> Result<Vec<u8>, UdfError> {
    let first = checked_add_u32(plan.partition_start, plan.lbns.video_fids, "video FID LBN")?;
    let items = build_video_items(metas);

    let mut fids = vec![Fid {
        name: None,
        is_dir: true,
        icb_lbn: plan.lbns.root_fe,
    }];

    for item in items {
        fids.push(Fid {
            name: Some(item.name),
            is_dir: false,
            icb_lbn: item.fe_lbn,
        });
    }

    Ok(desc::file_identifier_descriptors(first, &fids))
}

fn build_video_items(metas: &[FileMeta]) -> Vec<VideoItem> {
    let mut items: Vec<VideoItem> = metas
        .iter()
        .map(|m| VideoItem {
            name: m.iso_name.clone(),
            fe_lbn: m.fe_lbn,
            iso_extent: m.iso_extent,
            size: m.size,
        })
        .collect();
    items.sort_by(|a, b| a.name.cmp(&b.name));
    items
}

fn final_video_entries(items: &[VideoItem]) -> Vec<DirEntry> {
    items
        .iter()
        .map(|item| DirEntry {
            name: item.name.clone(),
            extent: item.iso_extent,
            size: item.size,
            is_dir: false,
        })
        .collect()
}

fn directory_entry_bytes(
    partition_start: u32,
    fe_lbn: u32,
    fids_lbn: u32,
    fids_len: usize,
    unique_id: u64,
    unix_time: i64,
) -> Result<Vec<u8>, UdfError> {
    let lbn = checked_add_u32(partition_start, fe_lbn, "directory FE LBN")?;
    let extents = if fids_len == 0 {
        Vec::new()
    } else {
        let len = u32::try_from(fids_len).map_err(|_| invalid("FID extent too large"))?;
        vec![Extent { lbn: fids_lbn, len }]
    };

    Ok(desc::file_entry(
        lbn, true, 0, &extents, unique_id, unix_time, 2,
    ))
}

fn file_entry_bytes(
    partition_start: u32,
    meta: &FileMeta,
    unique_id: u64,
    unix_time: i64,
) -> Result<Vec<u8>, UdfError> {
    let lbn = checked_add_u32(partition_start, meta.fe_lbn, "file FE LBN")?;
    let extent_len = if meta.size == 0 {
        SECTOR_SIZE_U32
    } else {
        u32::try_from(meta.size).map_err(|_| invalid("file extent too large"))?
    };

    let extents = vec![Extent {
        lbn: meta.data_lbn,
        len: extent_len,
    }];

    Ok(desc::file_entry(
        lbn, false, meta.size, &extents, unique_id, unix_time, 1,
    ))
}

fn copy_file<W: Write>(
    w: &mut W,
    source: &Path,
    size: u64,
    out_path: &str,
) -> Result<(), UdfError> {
    let source_path = source.to_string_lossy().into_owned();
    let mut file = File::open(source).map_err(|e| io_error(&source_path, e))?;
    let mut remaining = size;
    let mut buf = vec![0u8; 65536];

    while remaining > 0 {
        let want = remaining.min(buf.len() as u64) as usize;
        let n = file
            .read(&mut buf[..want])
            .map_err(|e| io_error(&source_path, e))?;
        if n == 0 {
            return Err(invalid(format!("unexpected EOF for {source_path}")));
        }
        w.write_all(&buf[..n]).map_err(|e| io_error(out_path, e))?;
        remaining = remaining.saturating_sub(n as u64);
    }

    Ok(())
}

fn write_zeros_to<W: Write>(
    w: &mut W,
    cursor: &mut u64,
    target: u64,
    path: &str,
) -> Result<(), UdfError> {
    if *cursor > target {
        return Err(invalid("write cursor moved backwards"));
    }
    let delta = target
        .checked_sub(*cursor)
        .ok_or_else(|| invalid("zero gap overflow"))?;
    write_zero_bytes(w, delta, path)?;
    *cursor = target;
    Ok(())
}

fn write_sector<W: Write>(
    w: &mut W,
    cursor: &mut u64,
    sector: u32,
    data: &[u8],
    path: &str,
) -> Result<(), UdfError> {
    let target = sector_bytes(sector)?;
    write_zeros_to(w, cursor, target, path)?;

    if data.len() > SECTOR_SIZE as usize {
        return Err(invalid("sector data too large"));
    }

    w.write_all(data).map_err(|e| io_error(path, e))?;
    let pad = (SECTOR_SIZE as usize).saturating_sub(data.len());
    write_zero_bytes(w, pad as u64, path)?;
    *cursor = target.saturating_add(SECTOR_SIZE);
    Ok(())
}

fn write_bytes_at<W: Write>(
    w: &mut W,
    cursor: &mut u64,
    sector: u32,
    data: &[u8],
    path: &str,
) -> Result<(), UdfError> {
    let target = sector_bytes(sector)?;
    write_zeros_to(w, cursor, target, path)?;
    w.write_all(data).map_err(|e| io_error(path, e))?;

    let rem = data.len() % (SECTOR_SIZE as usize);
    let pad = if rem == 0 {
        0
    } else {
        SECTOR_SIZE as usize - rem
    };
    write_zero_bytes(w, pad as u64, path)?;
    *cursor = target
        .saturating_add(data.len() as u64)
        .saturating_add(pad as u64);
    Ok(())
}

fn write_zero_bytes<W: Write>(w: &mut W, bytes: u64, path: &str) -> Result<(), UdfError> {
    let mut remaining = bytes;
    while remaining > 0 {
        let n = remaining.min(ZERO_CHUNK.len() as u64) as usize;
        w.write_all(&ZERO_CHUNK[..n])
            .map_err(|e| io_error(path, e))?;
        remaining = remaining.saturating_sub(n as u64);
    }
    Ok(())
}

fn sector_bytes(sector: u32) -> Result<u64, UdfError> {
    checked_mul_u64(sector as u64, SECTOR_SIZE, "sector offset")
}

fn sectors_for_size(size: u64) -> Result<u64, UdfError> {
    if size == 0 {
        Ok(1)
    } else {
        Ok(size.div_ceil(SECTOR_SIZE))
    }
}

fn fid_sector_count(len: usize) -> Result<u32, UdfError> {
    let sectors = (len as u64).div_ceil(SECTOR_SIZE);
    u32_from_u64(sectors, "FID sectors")
}

fn order_key(name: &str) -> (u8, u32, u8, u8, String) {
    let up = name.to_ascii_uppercase();
    match up.as_str() {
        "VIDEO_TS.IFO" => return (0, 0, 0, 0, up),
        "VIDEO_TS.VOB" => return (1, 0, 0, 0, up),
        "VIDEO_TS.BUP" => return (2, 0, 0, 0, up),
        _ => {}
    }

    if let Some((nn, kind, idx)) = parse_vts(&up) {
        let sub = match kind {
            0 => 0,
            1 => idx.saturating_add(1),
            2 => 11,
            _ => 20,
        };
        return (3, nn, sub, 0, up);
    }

    (4, 0, 0, 0, up)
}

fn parse_vts(name: &str) -> Option<(u32, u8, u8)> {
    let rest = name.strip_prefix("VTS_")?;
    let mut it = rest.chars().peekable();

    let mut nn: u32 = 0;
    let mut count = 0usize;
    while let Some(&c) = it.peek() {
        let Some(d) = c.to_digit(10) else {
            break;
        };
        nn = nn.checked_mul(10)?.checked_add(d)?;
        count = count.checked_add(1)?;
        it.next()?;
    }

    if count == 0 {
        return None;
    }
    if it.next()? != '_' {
        return None;
    }

    let idx_digit = it.next()?.to_digit(10)?;
    let idx = u8::try_from(idx_digit).ok()?;
    if it.next()? != '.' {
        return None;
    }

    let ext: String = it.collect();
    let kind = match ext.as_str() {
        "IFO" => 0,
        "VOB" => 1,
        "BUP" => 2,
        _ => return None,
    };

    Some((nn, kind, idx))
}

fn inc_u32(value: u32, what: &str) -> Result<u32, UdfError> {
    checked_add_u32(value, 1, what)
}

fn checked_add_u32(a: u32, b: u32, what: &str) -> Result<u32, UdfError> {
    a.checked_add(b)
        .ok_or_else(|| invalid(format!("{what} overflow")))
}

fn checked_add_u64(a: u64, b: u64, what: &str) -> Result<u64, UdfError> {
    a.checked_add(b)
        .ok_or_else(|| invalid(format!("{what} overflow")))
}

fn checked_mul_u64(a: u64, b: u64, what: &str) -> Result<u64, UdfError> {
    a.checked_mul(b)
        .ok_or_else(|| invalid(format!("{what} overflow")))
}

fn u32_from_u64(value: u64, what: &str) -> Result<u32, UdfError> {
    u32::try_from(value).map_err(|_| invalid(format!("{what} too large")))
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

#[cfg(test)]
mod tests {
    use super::{ImageFile, IsoReport, UdfError, dvd_file_order, invalid, io_error, write_dvd_iso};
    use crate::desc;
    use crate::read::{ListedFile, list_iso9660, list_udf};
    use std::fs;
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn dvd_file_order_sorts() {
        let names = vec![
            String::from("VTS_02_0.IFO"),
            String::from("VIDEO_TS.BUP"),
            String::from("VTS_01_1.VOB"),
            String::from("VIDEO_TS.IFO"),
            String::from("VTS_01_0.IFO"),
            String::from("OTHER.TXT"),
        ];

        let got = dvd_file_order(&names);
        let want = vec![
            String::from("VIDEO_TS.IFO"),
            String::from("VIDEO_TS.BUP"),
            String::from("VTS_01_0.IFO"),
            String::from("VTS_01_1.VOB"),
            String::from("VTS_02_0.IFO"),
            String::from("OTHER.TXT"),
        ];

        assert_eq!(got, want);
    }

    #[test]
    fn roundtrip_lists_and_data() -> Result<(), UdfError> {
        let dir = temp_dir("roundtrip")?;
        let result = roundtrip_inner(&dir);
        fs::remove_dir_all(&dir).ok();
        result
    }

    fn roundtrip_inner(dir: &Path) -> Result<(), UdfError> {
        let ifo = dir.join("VIDEO_TS.IFO");
        let bup = dir.join("VIDEO_TS.BUP");
        let vts_ifo = dir.join("VTS_01_0.IFO");
        let vts_vob = dir.join("VTS_01_1.VOB");

        let ifo_bytes = vec![0xAAu8; 4096];
        let bup_bytes = vec![0xBBu8; 4096];
        let vts_ifo_bytes = vec![0xCCu8; 2048];
        let vts_vob_bytes = vec![0xDDu8; 6244];

        write_test_file(&ifo, &ifo_bytes)?;
        write_test_file(&bup, &bup_bytes)?;
        write_test_file(&vts_ifo, &vts_ifo_bytes)?;
        write_test_file(&vts_vob, &vts_vob_bytes)?;

        let video_ts = vec![
            ImageFile {
                name: String::from("VIDEO_TS.IFO"),
                source: ifo.clone(),
            },
            ImageFile {
                name: String::from("VIDEO_TS.BUP"),
                source: bup.clone(),
            },
            ImageFile {
                name: String::from("VTS_01_0.IFO"),
                source: vts_ifo.clone(),
            },
            ImageFile {
                name: String::from("VTS_01_1.VOB"),
                source: vts_vob.clone(),
            },
        ];

        let out = dir.join("image.iso");
        let report: IsoReport = write_dvd_iso(&video_ts, "TEST", &out, 0)?;

        let udf = list_udf(&out)?;
        let iso = list_iso9660(&out)?;
        assert_eq!(udf, iso);

        let ifo_sector = find_sector(&udf, "VIDEO_TS/VIDEO_TS.IFO")?;
        let bup_sector = find_sector(&udf, "VIDEO_TS/VIDEO_TS.BUP")?;
        let vts_ifo_sector = find_sector(&udf, "VIDEO_TS/VTS_01_0.IFO")?;
        let vts_vob_sector = find_sector(&udf, "VIDEO_TS/VTS_01_1.VOB")?;

        assert_eq!(
            bup_sector,
            ifo_sector
                .checked_add(2)
                .ok_or_else(|| invalid("overflow"))?
        );
        assert_eq!(
            vts_ifo_sector,
            bup_sector
                .checked_add(2)
                .ok_or_else(|| invalid("overflow"))?
        );
        assert_eq!(
            vts_vob_sector,
            vts_ifo_sector
                .checked_add(1)
                .ok_or_else(|| invalid("overflow"))?
        );

        assert_eq!(
            read_test_bytes(&out, ifo_sector, ifo_bytes.len())?,
            ifo_bytes
        );
        assert_eq!(
            read_test_bytes(&out, bup_sector, bup_bytes.len())?,
            bup_bytes
        );
        assert_eq!(
            read_test_bytes(&out, vts_ifo_sector, vts_ifo_bytes.len())?,
            vts_ifo_bytes
        );
        assert_eq!(
            read_test_bytes(&out, vts_vob_sector, vts_vob_bytes.len())?,
            vts_vob_bytes
        );

        let anchor1 = read_test_sector(&out, 256)?;
        let anchor2 = read_test_sector(&out, report.sectors.saturating_sub(1))?;
        assert_eq!(tag_id_at(&anchor1, 0)?, 2);
        assert_eq!(tag_id_at(&anchor2, 0)?, 2);

        verify_vds_and_anchors(&out, report.sectors)?;
        Ok(())
    }

    fn temp_dir(tag: &str) -> Result<PathBuf, UdfError> {
        let base = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = base.join(format!(
            "disccraft-udf-{tag}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).map_err(|e| io_error(&dir.to_string_lossy(), e))?;
        Ok(dir)
    }

    fn write_test_file(path: &Path, bytes: &[u8]) -> Result<(), UdfError> {
        let path_str = path.to_string_lossy().into_owned();
        let mut file = fs::File::create(path).map_err(|e| io_error(&path_str, e))?;
        file.write_all(bytes).map_err(|e| io_error(&path_str, e))?;
        Ok(())
    }

    fn read_test_bytes(path: &Path, sector: u32, len: usize) -> Result<Vec<u8>, UdfError> {
        let path_str = path.to_string_lossy().into_owned();
        let mut file = fs::File::open(path).map_err(|e| io_error(&path_str, e))?;
        let offset = sector as u64 * 2048;
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| io_error(&path_str, e))?;
        let mut buf = vec![0u8; len];
        file.read_exact(&mut buf)
            .map_err(|e| io_error(&path_str, e))?;
        Ok(buf)
    }

    fn read_test_sector(path: &Path, sector: u32) -> Result<[u8; 2048], UdfError> {
        let path_str = path.to_string_lossy().into_owned();
        let mut file = fs::File::open(path).map_err(|e| io_error(&path_str, e))?;
        let offset = sector as u64 * 2048;
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| io_error(&path_str, e))?;
        let mut buf = [0u8; 2048];
        file.read_exact(&mut buf)
            .map_err(|e| io_error(&path_str, e))?;
        Ok(buf)
    }

    fn find_sector(list: &[ListedFile], path: &str) -> Result<u32, UdfError> {
        list.iter()
            .find(|f| f.path == path)
            .map(|f| f.sector)
            .ok_or_else(|| invalid(format!("missing {path}")))
    }

    fn verify_vds_and_anchors(path: &Path, sectors: u32) -> Result<(), UdfError> {
        for sector in 32..38 {
            verify_descriptor_sector(path, sector)?;
        }
        for sector in 48..54 {
            verify_descriptor_sector(path, sector)?;
        }
        verify_descriptor_sector(path, 64)?;
        verify_descriptor_sector(path, 65)?;
        verify_descriptor_sector(path, 256)?;
        verify_descriptor_sector(path, sectors.saturating_sub(1))?;
        Ok(())
    }

    fn verify_descriptor_sector(path: &Path, sector: u32) -> Result<(), UdfError> {
        let buf = read_test_sector(path, sector)?;
        verify_tag_at(&buf, 0)
    }

    fn tag_id_at(buf: &[u8], off: usize) -> Result<u16, UdfError> {
        let mut a = [0u8; 2];
        a.copy_from_slice(
            buf.get(off..off.checked_add(2).ok_or_else(|| invalid("short"))?)
                .ok_or_else(|| invalid("short"))?,
        );
        Ok(u16::from_le_bytes(a))
    }

    fn verify_tag_at(buf: &[u8], off: usize) -> Result<(), UdfError> {
        let tag = buf
            .get(off..off.checked_add(16).ok_or_else(|| invalid("short"))?)
            .ok_or_else(|| invalid("short"))?;

        let crc_len = read_u16_test(tag, 10)? as usize;
        let body_start = off.checked_add(16).ok_or_else(|| invalid("overflow"))?;
        let body_end = body_start
            .checked_add(crc_len)
            .ok_or_else(|| invalid("overflow"))?;
        let body = buf
            .get(body_start..body_end)
            .ok_or_else(|| invalid("short"))?;

        let stored_crc = read_u16_test(tag, 8)?;
        if desc::crc_itu(body) != stored_crc {
            return Err(invalid("bad CRC"));
        }

        let mut sum = 0u32;
        for b in tag.get(0..4).ok_or_else(|| invalid("short"))? {
            sum = sum.wrapping_add(u32::from(*b));
        }
        for b in tag.get(5..16).ok_or_else(|| invalid("short"))? {
            sum = sum.wrapping_add(u32::from(*b));
        }

        let checksum = tag.get(4).copied().ok_or_else(|| invalid("short"))?;
        if checksum != (sum & 0xFF) as u8 {
            return Err(invalid("bad checksum"));
        }

        Ok(())
    }

    fn read_u16_test(buf: &[u8], off: usize) -> Result<u16, UdfError> {
        let mut a = [0u8; 2];
        a.copy_from_slice(
            buf.get(off..off.checked_add(2).ok_or_else(|| invalid("short"))?)
                .ok_or_else(|| invalid("short"))?,
        );
        Ok(u16::from_le_bytes(a))
    }
}
