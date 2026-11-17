//! DVD-ROM content of a DVD-Video image: the folders and files a computer sees beside `VIDEO_TS` (Encore's
//! "DVD-ROM content"). They are laid out after the video files, so the video part of the image is where it is
//! without them: for every folder a UDF file entry, its file identifiers and its ISO 9660 directory, then the
//! files' entries, then their data.
//!
//! Sources: ECMA-167 3rd edition (4/14.4 file identifier descriptor, 4/14.9 file entry, 4/14.14.1 short
//! allocation descriptor), OSTA UDF 1.02 (2.1.1 the compressed Unicode of identifiers), ECMA-119 (6.8 directory
//! hierarchy, 6.9 path table, 7.5 / 7.6 file and directory identifiers, 9.1 directory record).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::desc::{self, Extent, Fid};
use crate::image::UdfError;
use crate::iso9660::{self, DirEntry, PathEntry};
use crate::tree::TreeFile;

const SECTOR: u64 = 2048;
/// The longest file an ISO 9660 directory record describes (a 32-bit data length).
const MAX_FILE_BYTES: u64 = u32::MAX as u64;
/// The longest extent of a short allocation descriptor that ends on a sector (its length is 30 bits).
const EXTENT_CHUNK: u64 = 0x3FFF_F800;
/// The deepest folder below the root: an ISO 9660 hierarchy has at most eight levels, the root being the first.
const MAX_DEPTH: usize = 7;
/// The most folders and the most files (a path table's parent numbers are 16 bits).
const MAX_ENTRIES: usize = 60_000;
/// The longest name: a UDF identifier is at most 255 bytes, one of them its compression ID, and a character
/// outside Latin-1 makes every character two bytes.
const MAX_NAME_UNITS_16: usize = 127;
const MAX_NAME_CHARS_8: usize = 254;

/// A folder of the content.
pub(crate) struct RomDir {
    /// Its name (UDF).
    pub(crate) name: String,
    /// Its ISO 9660 name (level 1, unique in its folder).
    pub(crate) iso_name: String,
    /// The folder it is in (`None`: the root).
    pub(crate) parent: Option<usize>,
    /// Its folders and files, by name.
    dirs: Vec<usize>,
    files: Vec<usize>,
    /// Partition blocks: its file entry, its file identifiers, its ISO 9660 directory.
    pub(crate) fe_lbn: u32,
    fids_lbn: u32,
    fids_len: usize,
    iso_lbn: u32,
    iso_size: u64,
    unique_id: u64,
}

/// A file of the content.
pub(crate) struct RomFile {
    pub(crate) name: String,
    pub(crate) iso_name: String,
    pub(crate) parent: Option<usize>,
    pub(crate) source: PathBuf,
    pub(crate) size: u64,
    pub(crate) fe_lbn: u32,
    pub(crate) data_lbn: u32,
    pub(crate) sectors: u32,
    unique_id: u64,
}

/// The content as a tree, with where everything goes once [`RomTree::layout`] has run.
#[derive(Default)]
pub(crate) struct RomTree {
    pub(crate) dirs: Vec<RomDir>,
    pub(crate) files: Vec<RomFile>,
    root_dirs: Vec<usize>,
    root_files: Vec<usize>,
}

/// What the content's folders point back to: the root directory's file entry and its ISO 9660 directory.
#[derive(Clone, Copy)]
pub(crate) struct RootRefs {
    /// The partition's first sector.
    pub(crate) partition_start: u32,
    /// The root's UDF file entry, a partition block.
    pub(crate) root_fe_lbn: u32,
    /// The root's ISO 9660 directory: its sector and size in bytes.
    pub(crate) root_iso: (u32, u64),
    /// The ISO 9660 date of the records.
    pub(crate) date: [u8; 7],
    /// The UDF time of the entries.
    pub(crate) unix_time: i64,
}

fn invalid(message: impl Into<String>) -> UdfError {
    UdfError::Invalid(message.into())
}

fn add(a: u32, b: u32) -> Result<u32, UdfError> {
    a.checked_add(b)
        .ok_or_else(|| invalid("the DVD-ROM content does not fit the image"))
}

fn sectors_of(bytes: u64) -> Result<u32, UdfError> {
    u32::try_from(bytes.div_ceil(SECTOR)).map_err(|_| invalid("DVD-ROM content too large"))
}

/// Checks one name of a path.
fn check_name(name: &str, path: &str) -> Result<(), UdfError> {
    let wide = name.chars().any(|c| u32::from(c) > 0xFF);
    let too_long = if wide {
        name.encode_utf16().count() > MAX_NAME_UNITS_16
    } else {
        name.chars().count() > MAX_NAME_CHARS_8
    };
    if name.is_empty() || name == "." || name == ".." || name.contains(['\\', '\0']) || too_long {
        return Err(invalid(format!(
            "DVD-ROM content: \"{path}\" has a name that cannot be written"
        )));
    }
    Ok(())
}

/// `wanted` when no name in `taken` is the same, else the name with its end replaced by a number.
fn unique_iso_name(wanted: &str, taken: &BTreeSet<String>) -> String {
    if !taken.contains(wanted) {
        return wanted.to_string();
    }
    let (base, extension) = match wanted.rfind('.') {
        Some(dot) => wanted.split_at(dot),
        None => (wanted, ""),
    };
    for number in 1_u32..100_000_000 {
        let digits = number.to_string();
        let keep = 8_usize.saturating_sub(digits.len());
        let head: String = base.chars().take(keep).collect();
        let name = format!("{head}{digits}{extension}");
        if !taken.contains(&name) {
            return name;
        }
    }
    wanted.to_string()
}

impl RomTree {
    /// The tree of `files` (paths with `/` between the names). `reserved` are the root's own names (`VIDEO_TS`),
    /// which the content may not use.
    ///
    /// # Errors
    ///
    /// A name that cannot be written, the same path twice, a folder and a file of one name, a path deeper than
    /// ISO 9660 allows, a file of 4 GiB or more, too many entries, or a file that cannot be read.
    pub(crate) fn build(files: &[TreeFile], reserved: &[&str]) -> Result<Self, UdfError> {
        let mut tree = RomTree::default();
        if files.len() > MAX_ENTRIES {
            return Err(invalid("DVD-ROM content: too many files"));
        }
        // (parent, name) to the folder's index; the names of each folder's files.
        let mut dir_index: BTreeMap<(Option<usize>, String), usize> = BTreeMap::new();
        let mut file_names: BTreeSet<(Option<usize>, String)> = BTreeSet::new();
        for file in files {
            let names: Vec<&str> = file.path.split('/').collect();
            let Some((file_name, folders)) = names.split_last() else {
                continue;
            };
            if folders.len() > MAX_DEPTH {
                return Err(invalid(format!(
                    "DVD-ROM content: \"{}\" is more than {MAX_DEPTH} folders deep",
                    file.path
                )));
            }
            let mut parent = None;
            for folder in folders {
                check_name(folder, &file.path)?;
                let key = (parent, (*folder).to_string());
                let index = match dir_index.get(&key) {
                    Some(index) => *index,
                    None => {
                        let index = tree.dirs.len();
                        if index >= MAX_ENTRIES {
                            return Err(invalid("DVD-ROM content: too many folders"));
                        }
                        tree.dirs.push(RomDir {
                            name: (*folder).to_string(),
                            iso_name: String::new(),
                            parent,
                            dirs: Vec::new(),
                            files: Vec::new(),
                            fe_lbn: 0,
                            fids_lbn: 0,
                            fids_len: 0,
                            iso_lbn: 0,
                            iso_size: 0,
                            unique_id: 0,
                        });
                        let _ = dir_index.insert(key, index);
                        index
                    }
                };
                parent = Some(index);
            }
            check_name(file_name, &file.path)?;
            if !file_names.insert((parent, (*file_name).to_string())) {
                return Err(invalid(format!(
                    "DVD-ROM content: \"{}\" is there twice",
                    file.path
                )));
            }
            let size = std::fs::metadata(&file.source)
                .map_err(|error| UdfError::Io {
                    path: file.source.to_string_lossy().into_owned(),
                    message: error.to_string(),
                })?
                .len();
            if size >= MAX_FILE_BYTES {
                return Err(invalid(format!(
                    "DVD-ROM content: \"{}\" is 4 GB or more, too large for a DVD's file system",
                    file.path
                )));
            }
            tree.files.push(RomFile {
                name: (*file_name).to_string(),
                iso_name: String::new(),
                parent,
                source: file.source.clone(),
                size,
                fe_lbn: 0,
                data_lbn: 0,
                sectors: sectors_of(size)?.max(1),
                unique_id: 0,
            });
        }
        for (parent, name) in &file_names {
            if dir_index.contains_key(&(*parent, name.clone())) {
                return Err(invalid(format!(
                    "DVD-ROM content: \"{name}\" is both a folder and a file"
                )));
            }
        }
        tree.link(reserved)?;
        Ok(tree)
    }

    /// Fills each folder's lists (by name), the ISO 9660 names and the sizes that follow from the names.
    fn link(&mut self, reserved: &[&str]) -> Result<(), UdfError> {
        for index in 0..self.dirs.len() {
            match self.dirs.get(index).and_then(|dir| dir.parent) {
                Some(parent) => {
                    if let Some(parent) = self.dirs.get_mut(parent) {
                        parent.dirs.push(index);
                    }
                }
                None => self.root_dirs.push(index),
            }
        }
        for index in 0..self.files.len() {
            match self.files.get(index).and_then(|file| file.parent) {
                Some(parent) => {
                    if let Some(parent) = self.dirs.get_mut(parent) {
                        parent.files.push(index);
                    }
                }
                None => self.root_files.push(index),
            }
        }
        for name in self.root_names() {
            if reserved.iter().any(|own| own.eq_ignore_ascii_case(&name.0)) {
                return Err(invalid(format!(
                    "DVD-ROM content: \"{}\" is the disc's own folder",
                    name.0
                )));
            }
        }

        // ISO 9660 names, unique in each folder (the root's beside the disc's own).
        let mut taken: BTreeSet<String> = reserved.iter().map(|name| (*name).to_string()).collect();
        let (root_dirs, root_files) = (self.root_dirs.clone(), self.root_files.clone());
        self.name_children(&root_dirs, &root_files, &mut taken);
        for index in 0..self.dirs.len() {
            let Some((dirs, files)) = self
                .dirs
                .get(index)
                .map(|dir| (dir.dirs.clone(), dir.files.clone()))
            else {
                continue;
            };
            self.name_children(&dirs, &files, &mut BTreeSet::new());
        }

        // Sizes that depend on the names only.
        let date = [0_u8; 7];
        for index in 0..self.dirs.len() {
            let fids_len = desc::file_identifier_descriptors(0, &self.dir_fids(index, 0)).len();
            let iso_size =
                iso9660::directory_records(0, 0, 0, 0, &self.dir_iso_entries(index, 0), date).len();
            if let Some(dir) = self.dirs.get_mut(index) {
                dir.fids_len = fids_len;
                dir.iso_size = iso_size as u64;
            }
        }
        Ok(())
    }

    fn name_children(&mut self, dirs: &[usize], files: &[usize], taken: &mut BTreeSet<String>) {
        let mut dirs = dirs.to_vec();
        dirs.sort_by(|a, b| self.dir_name(*a).cmp(self.dir_name(*b)));
        for index in dirs {
            let wanted = iso9660::sanitize_dir_name(self.dir_name(index));
            let name = unique_iso_name(&wanted, taken);
            let _ = taken.insert(name.clone());
            if let Some(dir) = self.dirs.get_mut(index) {
                dir.iso_name = name;
            }
        }
        let mut files = files.to_vec();
        files.sort_by(|a, b| self.file_name(*a).cmp(self.file_name(*b)));
        for index in files {
            let wanted = iso9660::sanitize_file_name(self.file_name(index));
            let name = unique_iso_name(&wanted, taken);
            let _ = taken.insert(name.clone());
            if let Some(file) = self.files.get_mut(index) {
                file.iso_name = name;
            }
        }
    }

    fn dir_name(&self, index: usize) -> &str {
        self.dirs.get(index).map_or("", |dir| dir.name.as_str())
    }

    fn file_name(&self, index: usize) -> &str {
        self.files.get(index).map_or("", |file| file.name.as_str())
    }

    /// The root's own entries of the content: name and whether it is a folder.
    pub(crate) fn root_names(&self) -> Vec<(String, bool)> {
        let dirs = self
            .root_dirs
            .iter()
            .map(|index| (self.dir_name(*index).to_string(), true));
        let files = self
            .root_files
            .iter()
            .map(|index| (self.file_name(*index).to_string(), false));
        dirs.chain(files).collect()
    }

    /// The number of folders at the root (each adds a link to the root directory).
    pub(crate) fn root_dir_count(&self) -> usize {
        self.root_dirs.len()
    }

    /// Places everything from partition block `start`: the folders (entry, identifiers, ISO 9660 directory),
    /// `extra` blocks for the caller (returned first: a moved root directory or path tables), the files'
    /// entries, the files' data. Unique IDs count from `unique`. Returns the caller's block, the block after the
    /// content and the next unique ID.
    pub(crate) fn layout(
        &mut self,
        start: u32,
        extra: u32,
        unique: u64,
    ) -> Result<(u32, u32, u64), UdfError> {
        let mut lbn = start;
        let mut unique = unique;
        let mut next_unique = || {
            let id = unique;
            unique = unique.saturating_add(1);
            id
        };
        for dir in &mut self.dirs {
            dir.unique_id = next_unique();
            dir.fe_lbn = lbn;
            lbn = add(lbn, 1)?;
            dir.fids_lbn = lbn;
            lbn = add(lbn, sectors_of(dir.fids_len as u64)?)?;
            dir.iso_lbn = lbn;
            lbn = add(lbn, sectors_of(dir.iso_size)?)?;
        }
        let extra_lbn = lbn;
        lbn = add(lbn, extra)?;
        for file in &mut self.files {
            file.unique_id = next_unique();
            file.fe_lbn = lbn;
            lbn = add(lbn, 1)?;
        }
        for file in &mut self.files {
            file.data_lbn = lbn;
            lbn = add(lbn, file.sectors)?;
        }
        Ok((extra_lbn, lbn, unique))
    }

    /// The root directory's file identifiers of the content.
    pub(crate) fn root_fids(&self) -> Vec<Fid> {
        self.child_fids(&self.root_dirs, &self.root_files)
    }

    /// The root's ISO 9660 directory entries of the content.
    pub(crate) fn root_iso_entries(&self, partition_start: u32) -> Vec<DirEntry> {
        self.child_iso_entries(&self.root_dirs, &self.root_files, partition_start)
    }

    fn child_fids(&self, dirs: &[usize], files: &[usize]) -> Vec<Fid> {
        let mut fids: Vec<Fid> = dirs
            .iter()
            .filter_map(|index| self.dirs.get(*index))
            .map(|dir| Fid {
                name: Some(dir.name.clone()),
                is_dir: true,
                icb_lbn: dir.fe_lbn,
            })
            .chain(
                files
                    .iter()
                    .filter_map(|index| self.files.get(*index))
                    .map(|file| Fid {
                        name: Some(file.name.clone()),
                        is_dir: false,
                        icb_lbn: file.fe_lbn,
                    }),
            )
            .collect();
        fids.sort_by(|a, b| a.name.cmp(&b.name));
        fids
    }

    fn child_iso_entries(
        &self,
        dirs: &[usize],
        files: &[usize],
        partition_start: u32,
    ) -> Vec<DirEntry> {
        let dirs = dirs
            .iter()
            .filter_map(|index| self.dirs.get(*index))
            .map(|dir| DirEntry {
                name: dir.iso_name.clone(),
                extent: partition_start.saturating_add(dir.iso_lbn),
                size: dir.iso_size,
                is_dir: true,
            });
        let files = files
            .iter()
            .filter_map(|index| self.files.get(*index))
            .map(|file| DirEntry {
                name: file.iso_name.clone(),
                extent: partition_start.saturating_add(file.data_lbn),
                size: file.size,
                is_dir: false,
            });
        dirs.chain(files).collect()
    }

    /// Folder `index`'s file identifiers: its parent (`parent_fe`) and its entries.
    fn dir_fids(&self, index: usize, parent_fe: u32) -> Vec<Fid> {
        let mut fids = vec![Fid {
            name: None,
            is_dir: true,
            icb_lbn: parent_fe,
        }];
        if let Some(dir) = self.dirs.get(index) {
            fids.extend(self.child_fids(&dir.dirs, &dir.files));
        }
        fids
    }

    fn dir_iso_entries(&self, index: usize, partition_start: u32) -> Vec<DirEntry> {
        self.dirs.get(index).map_or_else(Vec::new, |dir| {
            self.child_iso_entries(&dir.dirs, &dir.files, partition_start)
        })
    }

    /// The path table's entries of the content's folders after the root (number 1) and the disc's own
    /// `own` folders (name and sector, all in the root): every level's folders by parent number then name
    /// (ECMA-119 6.9.1), each with the number of its parent.
    pub(crate) fn path_entries(
        &self,
        root_extent: u32,
        own: &[(&str, u32)],
        partition_start: u32,
    ) -> Vec<PathEntry> {
        let mut entries = vec![PathEntry {
            name: vec![0_u8],
            extent: root_extent,
            parent: 1,
        }];
        // (name, sector, the content's folder index) of the first level.
        let mut level: Vec<(String, u32, Option<usize>, u16)> = own
            .iter()
            .map(|(name, sector)| ((*name).to_string(), *sector, None, 1_u16))
            .collect();
        level.extend(self.root_dirs.iter().filter_map(|index| {
            let dir = self.dirs.get(*index)?;
            Some((
                dir.iso_name.clone(),
                partition_start.saturating_add(dir.iso_lbn),
                Some(*index),
                1_u16,
            ))
        }));
        while !level.is_empty() {
            level.sort_by(|a, b| (a.3, &a.0).cmp(&(b.3, &b.0)));
            let mut next = Vec::new();
            for (name, extent, index, parent) in level {
                entries.push(PathEntry {
                    name: name.into_bytes(),
                    extent,
                    parent,
                });
                let number = u16::try_from(entries.len()).unwrap_or(u16::MAX);
                let children = index
                    .and_then(|index| self.dirs.get(index))
                    .map_or(&[][..], |dir| dir.dirs.as_slice());
                for child in children {
                    if let Some(dir) = self.dirs.get(*child) {
                        next.push((
                            dir.iso_name.clone(),
                            partition_start.saturating_add(dir.iso_lbn),
                            Some(*child),
                            number,
                        ));
                    }
                }
            }
            level = next;
        }
        entries
    }

    /// Everything of the content but the files' data, as (partition block, bytes), in block order: each folder's
    /// file entry, file identifiers and ISO 9660 directory, then each file's entry.
    pub(crate) fn metadata(&self, root: RootRefs) -> Result<Vec<(u32, Vec<u8>)>, UdfError> {
        let mut out = Vec::new();
        for (index, dir) in self.dirs.iter().enumerate() {
            let parent = dir.parent.and_then(|parent| self.dirs.get(parent));
            let parent_fe = parent.map_or(root.root_fe_lbn, |parent| parent.fe_lbn);
            let parent_iso = parent.map_or(root.root_iso, |parent| {
                (
                    root.partition_start.saturating_add(parent.iso_lbn),
                    parent.iso_size,
                )
            });
            let fids =
                desc::file_identifier_descriptors(dir.fids_lbn, &self.dir_fids(index, parent_fe));
            let fids_len = u32::try_from(fids.len())
                .map_err(|_| invalid("DVD-ROM content: a folder has too many entries"))?;
            // Its entry in its parent and the parent identifier of each of its folders (ECMA-167 4/14.9.6).
            let links = u16::try_from(dir.dirs.len().saturating_add(1)).unwrap_or(u16::MAX);
            out.push((
                dir.fe_lbn,
                desc::file_entry(
                    dir.fe_lbn,
                    true,
                    u64::from(fids_len),
                    &[Extent {
                        lbn: dir.fids_lbn,
                        len: fids_len,
                    }],
                    dir.unique_id,
                    root.unix_time,
                    links,
                ),
            ));
            out.push((dir.fids_lbn, fids));
            out.push((
                dir.iso_lbn,
                iso9660::directory_records(
                    root.partition_start.saturating_add(dir.iso_lbn),
                    dir.iso_size,
                    parent_iso.0,
                    parent_iso.1,
                    &self.dir_iso_entries(index, root.partition_start),
                    root.date,
                ),
            ));
        }
        for file in &self.files {
            out.push((
                file.fe_lbn,
                desc::file_entry(
                    file.fe_lbn,
                    false,
                    file.size,
                    &file_extents(file.data_lbn, file.size)?,
                    file.unique_id,
                    root.unix_time,
                    1,
                ),
            ));
        }
        out.sort_by_key(|(lbn, _)| *lbn);
        Ok(out)
    }
}

/// The allocation descriptors of `size` bytes from block `lbn`: one for every 2^30 - 2048 bytes (an empty file
/// has none).
fn file_extents(lbn: u32, size: u64) -> Result<Vec<Extent>, UdfError> {
    let mut extents = Vec::new();
    let mut lbn = lbn;
    let mut left = size;
    while left > 0 {
        let len = left.min(EXTENT_CHUNK);
        extents.push(Extent {
            lbn,
            len: u32::try_from(len).map_err(|_| invalid("extent too large"))?,
        });
        lbn = add(lbn, sectors_of(len)?)?;
        left = left.saturating_sub(len);
    }
    Ok(extents)
}

#[cfg(test)]
mod tests {
    use super::{RomTree, file_extents, unique_iso_name};
    use crate::tree::TreeFile;
    use std::collections::BTreeSet;

    fn files(dir: &std::path::Path, paths: &[&str]) -> Vec<TreeFile> {
        std::fs::create_dir_all(dir).unwrap();
        let source = dir.join("data.bin");
        std::fs::write(&source, vec![7_u8; 3000]).unwrap();
        paths
            .iter()
            .map(|path| TreeFile {
                path: (*path).to_string(),
                source: source.clone(),
            })
            .collect()
    }

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("disccraft-rom-{name}-{}", std::process::id()))
    }

    #[test]
    fn iso_names_are_unique_in_a_folder() {
        let mut taken = BTreeSet::new();
        for expected in ["MY_DOCUM.TXT", "MY_DOCU1.TXT", "MY_DOCU2.TXT"] {
            let name = unique_iso_name("MY_DOCUM.TXT", &taken);
            assert_eq!(name, expected);
            assert!(taken.insert(name));
        }
        let dir = temp("names");
        let tree = RomTree::build(
            &files(
                &dir,
                &["My Document 1.txt", "My Document 2.txt", "video_ts.txt"],
            ),
            &["VIDEO_TS"],
        )
        .unwrap();
        let names: Vec<&str> = tree.files.iter().map(|f| f.iso_name.as_str()).collect();
        assert_eq!(names, ["MY_DOCUM.TXT", "MY_DOCU1.TXT", "VIDEO_TS.TXT"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn bad_content_is_refused() {
        let dir = temp("bad");
        for paths in [
            &["video_ts/extra.txt"][..],
            &["a.txt", "a.txt"],
            &["a/b", "a"],
            &["1/2/3/4/5/6/7/8/deep.txt"],
            &["a//b.txt"],
            &["../b.txt"],
        ] {
            let result = RomTree::build(&files(&dir, paths), &["VIDEO_TS"]);
            assert!(result.is_err(), "{paths:?}");
        }
        // Seven folders deep is the limit, and fine.
        assert!(RomTree::build(&files(&dir, &["1/2/3/4/5/6/7/ok.txt"]), &["VIDEO_TS"]).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_layout_puts_folders_then_entries_then_data() {
        let dir = temp("layout");
        let mut tree = RomTree::build(
            &files(&dir, &["readme.txt", "docs/a.pdf", "docs/img/b.png"]),
            &["VIDEO_TS"],
        )
        .unwrap();
        let (extra, end, unique) = tree.layout(1000, 2, 50).unwrap();
        // Two folders of three blocks each (entry, identifiers, ISO directory), the caller's two, three file
        // entries, then three files of two sectors.
        assert_eq!(extra, 1006);
        assert_eq!(end, 1000 + 6 + 2 + 3 + 6);
        assert_eq!(unique, 55);
        assert_eq!(tree.root_names().len(), 2);
        assert_eq!(tree.root_dir_count(), 1);
        // The path table: root, VIDEO_TS and DOCS at the first level by name, then DOCS's IMG with its number.
        let table = tree.path_entries(259, &[("VIDEO_TS", 260)], 300);
        let rows: Vec<(String, u16)> = table
            .iter()
            .skip(1)
            .map(|e| (String::from_utf8_lossy(&e.name).into_owned(), e.parent))
            .collect();
        assert_eq!(
            rows,
            [
                ("DOCS".to_string(), 1),
                ("VIDEO_TS".to_string(), 1),
                ("IMG".to_string(), 2)
            ]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn long_files_take_several_extents() {
        assert!(file_extents(10, 0).unwrap().is_empty());
        let extents = file_extents(10, 0x3FFF_F800 + 5).unwrap();
        assert_eq!(extents.len(), 2);
        assert_eq!((extents[0].lbn, extents[0].len), (10, 0x3FFF_F800));
        assert_eq!((extents[1].lbn, extents[1].len), (10 + 0x7_FFFF, 5));
    }
}
