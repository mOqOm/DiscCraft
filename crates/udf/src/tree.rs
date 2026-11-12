//! UDF 2.50 tree layout and streaming image writer.

use crate::image::{IsoReport, UdfError};
use crate::udf250;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

const SECTOR_SIZE: usize = udf250::SECTOR_SIZE;
const SECTOR_SIZE_U32: u32 = udf250::SECTOR_SIZE_U32;
const SECTOR_SIZE_U64: u64 = udf250::SECTOR_SIZE_U64;
const PARTITION_START: u32 = 512;
const MAIN_VDS_SECTOR: u32 = 32;
const RESERVE_VDS_SECTOR: u32 = 48;
const LVID_SECTOR: u32 = 64;
const ANCHOR_SECTOR: u32 = 256;
const VDS_EXTENT_BYTES: u32 = 6 * SECTOR_SIZE_U32;
const CHUNK_SIZE: usize = 1024 * 1024;

static ZERO_CHUNK: [u8; 65536] = [0u8; 65536];

/// One file of the image: its path inside the image and the local source.
#[derive(Debug, Clone)]
pub struct TreeFile {
    /// Path inside the image, for example `BDMV/STREAM/00001.m2ts`.
    pub path: String,
    /// Local source file path.
    pub source: PathBuf,
}

/// A node in the image tree.
struct Node {
    /// Node name, empty for the root.
    name: String,
    /// Whether this node is a directory.
    is_dir: bool,
    /// Parent node index.
    parent: usize,
    /// Children sorted by name.
    children: BTreeMap<String, usize>,
    /// UDF unique identifier.
    unique_id: u64,
    /// Source path for files.
    source: PathBuf,
    /// Source size for files.
    size: u64,
    /// Metadata partition block of this node's file entry.
    fe_block: u32,
    /// First metadata partition block of directory FIDs.
    fid_first_block: u32,
    /// Directory FID byte length.
    fid_len: u32,
    /// Directory FID block count.
    fid_blocks: u32,
    /// Physical partition block where file data starts.
    data_start_block: u32,
    /// File data block count.
    data_blocks: u32,
    /// Allocation descriptors for the file entry.
    extents: Vec<udf250::EntryExtent>,
}

impl Node {
    fn new_dir(name: String, parent: usize) -> Self {
        Self {
            name,
            is_dir: true,
            parent,
            children: BTreeMap::new(),
            unique_id: 0,
            source: PathBuf::new(),
            size: 0,
            fe_block: 0,
            fid_first_block: 0,
            fid_len: 0,
            fid_blocks: 0,
            data_start_block: 0,
            data_blocks: 0,
            extents: Vec::new(),
        }
    }

    fn new_file(name: String, parent: usize, source: PathBuf, size: u64) -> Self {
        Self {
            name,
            is_dir: false,
            parent,
            children: BTreeMap::new(),
            unique_id: 0,
            source,
            size,
            fe_block: 0,
            fid_first_block: 0,
            fid_len: 0,
            fid_blocks: 0,
            data_start_block: 0,
            data_blocks: 0,
            extents: Vec::new(),
        }
    }
}

/// Built image tree.
struct Tree {
    /// Nodes, root at index 0.
    nodes: Vec<Node>,
    /// Next unique identifier.
    next_unique_id: u64,
}

/// Layout plan.
struct Plan {
    /// Total image sectors.
    image_sectors: u32,
    /// Physical partition blocks.
    partition_blocks: u32,
    /// Rounded metadata blocks.
    metadata_blocks: u32,
    /// Raw metadata blocks before rounding.
    raw_metadata_blocks: u32,
    /// First file data block in the physical partition.
    /// Absolute sector of the first file data extent.
    first_file_sector: u32,
    /// Directory indices in breadth-first order.
    dirs: Vec<usize>,
    /// File indices in tree order.
    files: Vec<usize>,
}

fn invalid(msg: &str) -> UdfError {
    UdfError::Invalid(msg.to_string())
}

fn io_error(path: &str, err: std::io::Error) -> UdfError {
    UdfError::Io {
        path: path.to_string(),
        message: err.to_string(),
    }
}

fn checked_add_u32(lhs: u32, rhs: u32) -> Result<u32, UdfError> {
    lhs.checked_add(rhs)
        .ok_or_else(|| invalid("sector overflow"))
}

fn checked_sub_u32(lhs: u32, rhs: u32) -> Result<u32, UdfError> {
    lhs.checked_sub(rhs)
        .ok_or_else(|| invalid("sector underflow"))
}

fn checked_mul_u64(lhs: u64, rhs: u64) -> Result<u64, UdfError> {
    lhs.checked_mul(rhs)
        .ok_or_else(|| invalid("byte count overflow"))
}

/// Write a UDF 2.50 image (Blu-ray) of `files`. Directories are created from the paths.
pub fn write_udf250_iso(
    files: &[TreeFile],
    label: &str,
    out: &Path,
    unix_time: i64,
) -> Result<IsoReport, UdfError> {
    let mut tree = build_tree(files)?;
    assign_unique_ids(&mut tree)?;
    let plan = plan_layout(&mut tree)?;

    let out_path = out.display().to_string();
    let file = File::create(out).map_err(|err| io_error(&out_path, err))?;
    let mut writer = BufWriter::new(file);
    write_image(&mut writer, &tree, &plan, label, unix_time)?;
    writer.flush().map_err(|err| io_error(&out_path, err))?;

    Ok(IsoReport {
        sectors: plan.image_sectors,
        first_file_sector: plan.first_file_sector,
    })
}

fn validate_path(path: &str) -> Result<Vec<&str>, UdfError> {
    if path.contains('\\') {
        return Err(invalid("backslash in image path"));
    }

    let mut components = Vec::new();
    for component in path.split('/') {
        if component.is_empty() {
            return Err(invalid("empty path component"));
        }
        if component == ".." {
            return Err(invalid("parent path component"));
        }
        if component.chars().count() > 254 {
            return Err(invalid("name longer than 254 characters"));
        }
        components.push(component);
    }

    if components.is_empty() {
        return Err(invalid("empty path"));
    }
    Ok(components)
}

fn find_child(tree: &Tree, parent: usize, name: &str) -> Result<Option<usize>, UdfError> {
    let node = tree
        .nodes
        .get(parent)
        .ok_or_else(|| invalid("missing parent node"))?;
    Ok(node.children.get(name).copied())
}

fn build_tree(files: &[TreeFile]) -> Result<Tree, UdfError> {
    if files.is_empty() {
        return Err(invalid("no files"));
    }

    let mut tree = Tree {
        nodes: vec![Node::new_dir(String::new(), 0)],
        next_unique_id: 16,
    };
    let mut seen = HashSet::new();

    for file in files {
        let components = validate_path(&file.path)?;
        let normalized = components.join("/");
        if !seen.insert(normalized) {
            return Err(invalid("duplicate path"));
        }

        let source_path = file.source.display().to_string();
        let size = fs::metadata(&file.source)
            .map_err(|err| io_error(&source_path, err))?
            .len();

        let mut parent = 0usize;
        for (depth, component) in components.iter().enumerate() {
            let is_last = depth
                .checked_add(1)
                .ok_or_else(|| invalid("path too deep"))?
                == components.len();
            if let Some(child) = find_child(&tree, parent, component)? {
                let node = tree
                    .nodes
                    .get(child)
                    .ok_or_else(|| invalid("missing child node"))?;
                if node.is_dir != !is_last {
                    return Err(invalid("path type conflict"));
                }
                parent = child;
            } else {
                let child = tree.nodes.len();
                let node = if is_last {
                    Node::new_file((*component).to_string(), parent, file.source.clone(), size)
                } else {
                    Node::new_dir((*component).to_string(), parent)
                };
                tree.nodes.push(node);
                let parent_node = tree
                    .nodes
                    .get_mut(parent)
                    .ok_or_else(|| invalid("missing parent node"))?;
                parent_node.children.insert((*component).to_string(), child);
                parent = child;
            }
        }
    }

    Ok(tree)
}

fn assign_unique_ids(tree: &mut Tree) -> Result<(), UdfError> {
    let mut queue = VecDeque::new();
    queue.push_back(0);
    let mut next = 16u64;

    while let Some(index) = queue.pop_front() {
        if index != 0 {
            let node = tree
                .nodes
                .get_mut(index)
                .ok_or_else(|| invalid("missing node"))?;
            node.unique_id = next;
            next = next
                .checked_add(1)
                .ok_or_else(|| invalid("unique id overflow"))?;
        }

        let children: Vec<usize> = tree
            .nodes
            .get(index)
            .ok_or_else(|| invalid("missing node"))?
            .children
            .values()
            .copied()
            .collect();
        for child in children {
            queue.push_back(child);
        }
    }

    tree.next_unique_id = next;
    Ok(())
}

fn collect_order(tree: &Tree) -> Result<(Vec<usize>, Vec<usize>), UdfError> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    let mut queue = VecDeque::new();
    queue.push_back(0);

    while let Some(index) = queue.pop_front() {
        let node = tree
            .nodes
            .get(index)
            .ok_or_else(|| invalid("missing node"))?;
        if node.is_dir {
            dirs.push(index);
            for child in node.children.values() {
                queue.push_back(*child);
            }
        } else {
            files.push(index);
        }
    }

    Ok((dirs, files))
}

fn fid_len_for_name(name: Option<&str>) -> Result<u32, UdfError> {
    let id_len = match name {
        Some(text) => {
            let chars =
                u32::try_from(text.chars().count()).map_err(|_| invalid("name too long"))?;
            chars
                .checked_add(1)
                .ok_or_else(|| invalid("name too long"))?
        }
        // The parent entry has no identifier at all (L_FI = 0).
        None => 0,
    };
    let raw = 38u32
        .checked_add(id_len)
        .ok_or_else(|| invalid("fid overflow"))?;
    udf250::round_up_u32(raw, 4)
}

fn directory_fid_len(tree: &Tree, index: usize) -> Result<u32, UdfError> {
    let node = tree
        .nodes
        .get(index)
        .ok_or_else(|| invalid("missing directory"))?;
    let mut total = fid_len_for_name(None)?;
    for name in node.children.keys() {
        let child_len = fid_len_for_name(Some(name))?;
        total = total
            .checked_add(child_len)
            .ok_or_else(|| invalid("fid overflow"))?;
    }
    Ok(total)
}

fn build_file_extents(size: u64, start_block: u32) -> Result<Vec<udf250::EntryExtent>, UdfError> {
    let mut extents = Vec::new();
    let mut offset = 0u64;
    let mut block = start_block;

    while offset < size {
        if extents.len() >= 100 {
            return Err(invalid("too many allocation extents"));
        }
        let remaining = size
            .checked_sub(offset)
            .ok_or_else(|| invalid("size underflow"))?;
        let chunk = if remaining > udf250::MAX_EXTENT_BYTES {
            udf250::MAX_EXTENT_BYTES
        } else {
            remaining
        };
        let chunk_u32 = u32::try_from(chunk).map_err(|_| invalid("extent too large"))?;
        let chunk_blocks = udf250::blocks_for_bytes(chunk)?;

        extents.push(udf250::EntryExtent::Long {
            block,
            length: chunk_u32,
            partition: 0,
        });

        block = block
            .checked_add(chunk_blocks)
            .ok_or_else(|| invalid("partition overflow"))?;
        offset = offset
            .checked_add(chunk)
            .ok_or_else(|| invalid("size overflow"))?;
    }

    Ok(extents)
}

fn plan_layout(tree: &mut Tree) -> Result<Plan, UdfError> {
    let (dirs, files) = collect_order(tree)?;

    let mut block = 2u32;
    for index in &dirs {
        let fid_len = directory_fid_len(tree, *index)?;
        let fid_blocks = udf250::blocks_for_u32_bytes(fid_len)?;
        let node = tree
            .nodes
            .get_mut(*index)
            .ok_or_else(|| invalid("missing directory"))?;
        node.fe_block = block;
        block = checked_add_u32(block, 1)?;
        node.fid_first_block = block;
        node.fid_len = fid_len;
        node.fid_blocks = fid_blocks;
        node.extents = vec![udf250::EntryExtent::Short {
            block: node.fid_first_block,
            length: fid_len,
        }];
        block = checked_add_u32(block, fid_blocks)?;
    }

    let mut raw_metadata_blocks = block;
    for index in &files {
        let node = tree
            .nodes
            .get_mut(*index)
            .ok_or_else(|| invalid("missing file"))?;
        node.fe_block = raw_metadata_blocks;
        raw_metadata_blocks = checked_add_u32(raw_metadata_blocks, 1)?;
    }

    let metadata_blocks = udf250::round_up_u32(raw_metadata_blocks, 32)?;
    let file_data_start = checked_add_u32(32, metadata_blocks)?
        .checked_add(metadata_blocks)
        .ok_or_else(|| invalid("partition overflow"))?;

    let mut data_block = file_data_start;
    let mut total_file_blocks = 0u32;
    for index in &files {
        let size = tree
            .nodes
            .get(*index)
            .ok_or_else(|| invalid("missing file"))?
            .size;
        let blocks = udf250::blocks_for_bytes(size)?;
        let extents = build_file_extents(size, data_block)?;
        let node = tree
            .nodes
            .get_mut(*index)
            .ok_or_else(|| invalid("missing file"))?;
        node.data_start_block = data_block;
        node.data_blocks = blocks;
        node.extents = extents;
        data_block = checked_add_u32(data_block, blocks)?;
        total_file_blocks = checked_add_u32(total_file_blocks, blocks)?;
    }

    let partition_blocks = data_block;
    let image_sectors = checked_add_u32(PARTITION_START, partition_blocks)?
        .checked_add(1)
        .ok_or_else(|| invalid("image too large"))?;
    let first_file_sector = if total_file_blocks > 0 {
        checked_add_u32(PARTITION_START, file_data_start)?
    } else {
        0
    };

    Ok(Plan {
        image_sectors,
        partition_blocks,
        metadata_blocks,
        raw_metadata_blocks,
        first_file_sector,
        dirs,
        files,
    })
}

fn write_zero_bytes<W: Write>(w: &mut W, bytes: u64) -> Result<(), UdfError> {
    let mut remaining = bytes;
    while remaining > 0 {
        let take = if remaining > ZERO_CHUNK.len() as u64 {
            ZERO_CHUNK.len() as u64
        } else {
            remaining
        };
        let take_usize = usize::try_from(take).map_err(|_| invalid("zero chunk too large"))?;
        let src = ZERO_CHUNK
            .get(..take_usize)
            .ok_or_else(|| invalid("zero chunk out of bounds"))?;
        w.write_all(src).map_err(|err| io_error("image", err))?;
        remaining = remaining
            .checked_sub(take)
            .ok_or_else(|| invalid("zero underflow"))?;
    }
    Ok(())
}

fn write_zero_sectors<W: Write>(w: &mut W, sectors: u32) -> Result<(), UdfError> {
    let bytes = checked_mul_u64(u64::from(sectors), SECTOR_SIZE_U64)?;
    write_zero_bytes(w, bytes)
}

fn write_zero_until<W: Write>(w: &mut W, current: u32, target: u32) -> Result<u32, UdfError> {
    if target < current {
        return Err(invalid("sector target before current"));
    }
    let count = checked_sub_u32(target, current)?;
    write_zero_sectors(w, count)?;
    Ok(target)
}

fn write_sector<W: Write>(w: &mut W, buf: &[u8; SECTOR_SIZE]) -> Result<(), UdfError> {
    w.write_all(buf).map_err(|err| io_error("image", err))
}

fn write_entry_sector<W: Write>(w: &mut W, entry: &[u8]) -> Result<(), UdfError> {
    w.write_all(entry).map_err(|err| io_error("image", err))?;
    let pad = SECTOR_SIZE
        .checked_sub(entry.len())
        .ok_or_else(|| invalid("entry too large"))?;
    write_zero_bytes(w, pad as u64)
}

fn write_vrs<W: Write>(w: &mut W, sector: u32, ident: &[u8; 5]) -> Result<u32, UdfError> {
    let buf = udf250::volume_recognition_sector(ident);
    write_sector(w, &buf)?;
    checked_add_u32(sector, 1)
}

fn write_avdp<W: Write>(w: &mut W, sector: u32) -> Result<(), UdfError> {
    let buf = udf250::avdp(
        sector,
        (MAIN_VDS_SECTOR, VDS_EXTENT_BYTES),
        (RESERVE_VDS_SECTOR, VDS_EXTENT_BYTES),
    )?;
    write_sector(w, &buf)
}

fn write_main_vds<W: Write>(
    w: &mut W,
    sector: u32,
    vds_number: u32,
    plan: &Plan,
    label: &str,
    unix_time: i64,
) -> Result<u32, UdfError> {
    let pvd = udf250::pvd(sector, vds_number, label, label, unix_time)?;
    write_sector(w, &pvd)?;

    let iuvd = udf250::iuvd(checked_add_u32(sector, 1)?, vds_number, label)?;
    write_sector(w, &iuvd)?;

    let pd = udf250::pd(
        checked_add_u32(sector, 2)?,
        vds_number,
        PARTITION_START,
        plan.partition_blocks,
    )?;
    write_sector(w, &pd)?;

    let lvd = udf250::lvd(&udf250::LvdParams {
        location: checked_add_u32(sector, 3)?,
        vds_number,
        label,
        // The file set descriptor is the first block of the metadata partition.
        fsd_block: 0,
        integrity_location: LVID_SECTOR,
        integrity_length: 2 * SECTOR_SIZE_U32,
        metadata_file_location: 0,
        metadata_mirror_location: 1,
    })?;
    write_sector(w, &lvd)?;

    let usd = udf250::usd(checked_add_u32(sector, 4)?, vds_number)?;
    write_sector(w, &usd)?;

    let td = udf250::td(checked_add_u32(sector, 5)?)?;
    write_sector(w, &td)?;

    checked_add_u32(sector, 6)
}

fn write_lvid_sector<W: Write>(
    w: &mut W,
    sector: u32,
    tree: &Tree,
    plan: &Plan,
    unix_time: i64,
) -> Result<u32, UdfError> {
    let files = u32::try_from(plan.files.len()).map_err(|_| invalid("too many files"))?;
    let dirs = u32::try_from(plan.dirs.len()).map_err(|_| invalid("too many directories"))?;
    let lvid = udf250::lvid(&udf250::LvidParams {
        location: sector,
        unix_time,
        next_unique_id: tree.next_unique_id,
        files,
        dirs,
        physical_blocks: plan.partition_blocks,
        metadata_blocks: plan.metadata_blocks,
    })?;
    write_sector(w, &lvid)?;
    checked_add_u32(sector, 1)
}

fn write_image<W: Write>(
    w: &mut W,
    tree: &Tree,
    plan: &Plan,
    label: &str,
    unix_time: i64,
) -> Result<(), UdfError> {
    let mut sector = 0u32;

    sector = write_zero_until(w, sector, 16)?;
    sector = write_vrs(w, sector, b"BEA01")?;
    sector = write_vrs(w, sector, b"NSR03")?;
    sector = write_vrs(w, sector, b"TEA01")?;

    sector = write_zero_until(w, sector, MAIN_VDS_SECTOR)?;
    sector = write_main_vds(w, sector, 0, plan, label, unix_time)?;

    sector = write_zero_until(w, sector, RESERVE_VDS_SECTOR)?;
    sector = write_main_vds(w, sector, 1, plan, label, unix_time)?;

    sector = write_zero_until(w, sector, LVID_SECTOR)?;
    sector = write_lvid_sector(w, sector, tree, plan, unix_time)?;
    let td = udf250::td(sector)?;
    write_sector(w, &td)?;
    sector = checked_add_u32(sector, 1)?;

    sector = write_zero_until(w, sector, ANCHOR_SECTOR)?;
    write_avdp(w, sector)?;
    sector = checked_add_u32(sector, 1)?;

    write_zero_until(w, sector, PARTITION_START)?;
    write_partition(w, tree, plan, label, unix_time)?;

    let expected_last = checked_add_u32(PARTITION_START, plan.partition_blocks)?;
    let last = checked_sub_u32(plan.image_sectors, 1)?;
    if expected_last != last {
        return Err(invalid("image layout mismatch"));
    }
    write_avdp(w, last)?;
    Ok(())
}

fn metadata_entry_params(
    location: u32,
    file_type: u8,
    unique_id: u64,
    extents: &[udf250::EntryExtent],
    metadata_bytes: u32,
    metadata_blocks: u32,
    unix_time: i64,
) -> udf250::EntryParams<'_> {
    udf250::EntryParams {
        location,
        file_type,
        icb_flags: 0,
        parent_partition: 0,
        parent_block: 0,
        info_len: u64::from(metadata_bytes),
        logical_blocks: u64::from(metadata_blocks),
        unique_id,
        link_count: 1,
        unix_time,
        extents,
    }
}

fn write_partition<W: Write>(
    w: &mut W,
    tree: &Tree,
    plan: &Plan,
    label: &str,
    unix_time: i64,
) -> Result<(), UdfError> {
    let metadata_bytes_u64 = checked_mul_u64(u64::from(plan.metadata_blocks), SECTOR_SIZE_U64)?;
    let metadata_bytes =
        u32::try_from(metadata_bytes_u64).map_err(|_| invalid("metadata too large"))?;
    let mirror_block = checked_add_u32(32, plan.metadata_blocks)?;

    let meta_extent = [udf250::EntryExtent::Short {
        block: 32,
        length: metadata_bytes,
    }];
    let mirror_extent = [udf250::EntryExtent::Short {
        block: mirror_block,
        length: metadata_bytes,
    }];
    let meta_params = metadata_entry_params(
        0,
        250,
        1,
        &meta_extent,
        metadata_bytes,
        plan.metadata_blocks,
        unix_time,
    );
    let meta_entry = udf250::file_entry(&meta_params)?;
    write_entry_sector(w, &meta_entry)?;

    let mirror_params = metadata_entry_params(
        1,
        251,
        2,
        &mirror_extent,
        metadata_bytes,
        plan.metadata_blocks,
        unix_time,
    );
    let mirror_entry = udf250::file_entry(&mirror_params)?;
    write_entry_sector(w, &mirror_entry)?;

    write_zero_sectors(w, 30)?;
    write_metadata_content(w, tree, plan, label, unix_time)?;
    write_metadata_content(w, tree, plan, label, unix_time)?;

    for index in &plan.files {
        let node = tree
            .nodes
            .get(*index)
            .ok_or_else(|| invalid("missing file"))?;
        write_file_data(w, node)?;
    }
    Ok(())
}

fn count_subdirs(node: &Node, tree: &Tree) -> Result<u32, UdfError> {
    let mut count = 0u32;
    for child in node.children.values() {
        if let Some(child_node) = tree.nodes.get(*child)
            && child_node.is_dir
        {
            count = checked_add_u32(count, 1)?;
        }
    }
    Ok(count)
}

fn dir_entry_params<'a>(
    node: &'a Node,
    tree: &'a Tree,
    unix_time: i64,
) -> Result<udf250::EntryParams<'a>, UdfError> {
    let parent_block = if node.parent == 0 && node.name.is_empty() {
        node.fe_block
    } else {
        tree.nodes
            .get(node.parent)
            .ok_or_else(|| invalid("missing parent"))?
            .fe_block
    };
    let subdirs = count_subdirs(node, tree)?;
    let link_count =
        u16::try_from(checked_add_u32(subdirs, 1)?).map_err(|_| invalid("too many links"))?;

    Ok(udf250::EntryParams {
        location: node.fe_block,
        file_type: 4,
        icb_flags: 0,
        parent_partition: 1,
        parent_block,
        info_len: u64::from(node.fid_len),
        logical_blocks: u64::from(node.fid_blocks),
        unique_id: node.unique_id,
        link_count,
        unix_time,
        extents: &node.extents,
    })
}

fn file_entry_params<'a>(
    node: &'a Node,
    tree: &'a Tree,
    unix_time: i64,
) -> Result<udf250::EntryParams<'a>, UdfError> {
    let parent_block = tree
        .nodes
        .get(node.parent)
        .ok_or_else(|| invalid("missing parent"))?
        .fe_block;
    Ok(udf250::EntryParams {
        location: node.fe_block,
        file_type: 5,
        icb_flags: 1,
        parent_partition: 1,
        parent_block,
        info_len: node.size,
        logical_blocks: u64::from(node.data_blocks),
        unique_id: node.unique_id,
        link_count: 1,
        unix_time,
        extents: &node.extents,
    })
}

/// Write one copy of the metadata partition: FSD, TD, every directory (FE + FIDs), every file FE, then zero padding.
fn write_metadata_content<W: Write>(
    w: &mut W,
    tree: &Tree,
    plan: &Plan,
    label: &str,
    unix_time: i64,
) -> Result<(), UdfError> {
    let root = tree.nodes.first().ok_or_else(|| invalid("empty tree"))?;
    write_sector(w, &udf250::fsd(0, label, root.fe_block, unix_time)?)?;
    write_sector(w, &udf250::td(1)?)?;
    let mut written = 2u32;
    for index in &plan.dirs {
        let node = tree
            .nodes
            .get(*index)
            .ok_or_else(|| invalid("missing directory"))?;
        let entry = udf250::file_entry(&dir_entry_params(node, tree, unix_time)?)?;
        write_entry_sector(w, &entry)?;
        let fids = directory_fids(node, tree)?;
        write_padded(w, &fids, node.fid_blocks)?;
        written = checked_add_u32(checked_add_u32(written, 1)?, node.fid_blocks)?;
    }
    for index in &plan.files {
        let node = tree
            .nodes
            .get(*index)
            .ok_or_else(|| invalid("missing file"))?;
        let entry = udf250::file_entry(&file_entry_params(node, tree, unix_time)?)?;
        write_entry_sector(w, &entry)?;
        written = checked_add_u32(written, 1)?;
    }
    if written != plan.raw_metadata_blocks {
        return Err(invalid("metadata layout mismatch"));
    }
    write_zero_sectors(w, checked_sub_u32(plan.metadata_blocks, written)?)
}

/// Build the FIDs of one directory: the parent entry, then the children sorted by name.
fn directory_fids(node: &Node, tree: &Tree) -> Result<Vec<u8>, UdfError> {
    let parent = tree
        .nodes
        .get(node.parent)
        .ok_or_else(|| invalid("missing parent"))?;
    let mut out = Vec::new();
    push_fid(&mut out, node.fid_first_block, 0x0A, None, parent)?;
    for (name, child) in &node.children {
        let child_node = tree
            .nodes
            .get(*child)
            .ok_or_else(|| invalid("missing child"))?;
        let characteristics = if child_node.is_dir { 0x02 } else { 0x00 };
        push_fid(
            &mut out,
            node.fid_first_block,
            characteristics,
            Some(name),
            child_node,
        )?;
    }
    if u32::try_from(out.len()).ok() != Some(node.fid_len) {
        return Err(invalid("directory size mismatch"));
    }
    Ok(out)
}

/// Append one FID; its tag location is the metadata block that holds its first byte.
fn push_fid(
    out: &mut Vec<u8>,
    first_block: u32,
    characteristics: u8,
    name: Option<&str>,
    target: &Node,
) -> Result<(), UdfError> {
    let offset =
        u32::try_from(out.len() / SECTOR_SIZE).map_err(|_| invalid("directory too large"))?;
    let fid = udf250::fid(&udf250::FidParams {
        location: checked_add_u32(first_block, offset)?,
        characteristics,
        name,
        icb_partition: 1,
        icb_block: target.fe_block,
        icb_unique_id: u32::try_from(target.unique_id & 0xFFFF_FFFF).unwrap_or(0),
    })?;
    out.extend_from_slice(&fid);
    Ok(())
}

/// Write `bytes` followed by zeros up to `blocks` whole blocks.
fn write_padded<W: Write>(w: &mut W, bytes: &[u8], blocks: u32) -> Result<(), UdfError> {
    w.write_all(bytes).map_err(|e| io_error("image", e))?;
    let total = checked_mul_u64(u64::from(blocks), SECTOR_SIZE_U64)?;
    let len = u64::try_from(bytes.len()).map_err(|_| invalid("block too large"))?;
    let rest = total
        .checked_sub(len)
        .ok_or_else(|| invalid("data larger than its blocks"))?;
    write_zero_bytes(w, rest)
}

/// Copy one file's data in 1 MiB chunks and pad it to whole blocks; fail if the source size changed.
fn write_file_data<W: Write>(w: &mut W, node: &Node) -> Result<(), UdfError> {
    let path = node.source.to_string_lossy().into_owned();
    let mut file = std::fs::File::open(&node.source).map_err(|e| io_error(&path, e))?;
    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut copied = 0u64;
    loop {
        let n = std::io::Read::read(&mut file, &mut buf).map_err(|e| io_error(&path, e))?;
        if n == 0 {
            break;
        }
        let chunk = buf.get(..n).ok_or_else(|| invalid("read overflow"))?;
        w.write_all(chunk).map_err(|e| io_error("image", e))?;
        copied = copied
            .checked_add(u64::try_from(n).map_err(|_| invalid("read overflow"))?)
            .ok_or_else(|| invalid("file too large"))?;
        if copied > node.size {
            break;
        }
    }
    if copied != node.size {
        return Err(invalid(
            "a source file changed size while the image was written",
        ));
    }
    let total = checked_mul_u64(u64::from(node.data_blocks), SECTOR_SIZE_U64)?;
    let rest = total
        .checked_sub(copied)
        .ok_or_else(|| invalid("file larger than its blocks"))?;
    write_zero_bytes(w, rest)
}

#[cfg(test)]
mod tests {
    use super::{TreeFile, write_udf250_iso};

    fn sector(image: &[u8], n: usize) -> &[u8] {
        &image[n * 2048..(n + 1) * 2048]
    }

    fn tag(image: &[u8], n: usize) -> (u16, u16, u32) {
        let s = sector(image, n);
        (
            u16::from_le_bytes([s[0], s[1]]),
            u16::from_le_bytes([s[2], s[3]]),
            u32::from_le_bytes([s[12], s[13], s[14], s[15]]),
        )
    }

    #[test]
    fn writes_a_udf250_image_of_a_small_tree() {
        let dir = std::env::temp_dir().join("disccraft-udf250-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut files = Vec::new();
        let paths = [
            "BDMV/index.bdmv",
            "BDMV/MovieObject.bdmv",
            "BDMV/STREAM/00001.m2ts",
        ];
        for (i, path) in paths.into_iter().enumerate() {
            let source = dir.join(format!("f{i}"));
            let byte = b"ABC"[i];
            std::fs::write(&source, vec![byte; 3000 + i]).unwrap();
            files.push(TreeFile {
                path: path.to_string(),
                source,
            });
        }
        let out = dir.join("t.iso");
        let report = write_udf250_iso(&files, "TEST", &out, 0).unwrap();
        let image = std::fs::read(&out).unwrap();
        let sectors = usize::try_from(report.sectors).unwrap();
        assert_eq!(image.len(), sectors * 2048);
        assert_eq!(&sector(&image, 16)[1..6], b"BEA01");
        assert_eq!(&sector(&image, 17)[1..6], b"NSR03");
        assert_eq!(&sector(&image, 18)[1..6], b"TEA01");
        assert_eq!(tag(&image, 256), (2, 3, 256));
        assert_eq!(tag(&image, sectors - 1).0, 2);
        assert_eq!(tag(&image, 35).0, 6);
        assert_eq!(sector(&image, 35)[446], 2);
        assert_eq!(sector(&image, 512)[27], 250);
        assert_eq!(sector(&image, 513)[27], 251);
        assert_eq!(tag(&image, 512 + 32), (256, 3, 0));
        // The first file's data starts at the reported sector.
        let first = usize::try_from(report.first_file_sector).unwrap();
        assert!(
            sector(&image, first)
                .iter()
                .take(3000)
                .all(|b| b"ABC".contains(b))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
