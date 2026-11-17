//! ROM content (Encore's "DVD-ROM content" / "BD-ROM content" of the Build panel): a folder of files a computer
//! sees on the disc beside the video. The project names the folder (`DiscSettings::rom_content`); its files are
//! read when the disc is built or its size is estimated.

use std::path::{Path, PathBuf};

/// The most files and folders a ROM content folder may hold.
pub const MAX_ROM_ENTRIES: usize = 60_000;
/// The deepest a file may be below the folder (an ISO 9660 hierarchy has eight levels, the root being the first).
pub const MAX_ROM_DEPTH: usize = 7;
const SECTOR: u64 = 2048;

/// One file of the ROM content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RomFile {
    /// Its path on the disc: the names from the content folder down, with `/` between them.
    pub path: String,
    /// The file on this computer.
    pub source: PathBuf,
    /// Its size in bytes.
    pub size: u64,
}

/// The files of the ROM content folder `folder`, by path. Folders without files are left out.
///
/// # Errors
///
/// A message when the folder cannot be read, holds too many entries or goes too deep, or a name is not Unicode.
pub fn rom_files(folder: &Path) -> Result<Vec<RomFile>, String> {
    if !folder.is_dir() {
        return Err(format!(
            "the ROM content folder {} does not exist",
            folder.display()
        ));
    }
    let mut files = Vec::new();
    let mut entries = 0_usize;
    walk(folder, &mut Vec::new(), &mut files, &mut entries)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn walk(
    dir: &Path,
    names: &mut Vec<String>,
    files: &mut Vec<RomFile>,
    entries: &mut usize,
) -> Result<(), String> {
    let listing = std::fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    for entry in listing {
        let entry = entry.map_err(|error| format!("{}: {error}", dir.display()))?;
        let path = entry.path();
        *entries = entries.saturating_add(1);
        if *entries > MAX_ROM_ENTRIES {
            return Err(format!(
                "the ROM content folder holds more than {MAX_ROM_ENTRIES} files and folders"
            ));
        }
        let name = entry.file_name().into_string().map_err(|_| {
            format!(
                "ROM content: the name of {} cannot be written",
                path.display()
            )
        })?;
        let metadata =
            std::fs::metadata(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        if metadata.is_dir() {
            if names.len() >= MAX_ROM_DEPTH {
                return Err(format!(
                    "ROM content: {} is more than {MAX_ROM_DEPTH} folders deep",
                    path.display()
                ));
            }
            names.push(name);
            walk(&path, names, files, entries)?;
            let _ = names.pop();
        } else {
            let mut parts = names.clone();
            parts.push(name);
            files.push(RomFile {
                path: parts.join("/"),
                source: path,
                size: metadata.len(),
            });
        }
    }
    Ok(())
}

/// The bytes `files` take on a disc: each file's sectors and one more for its entry (their folders take a few
/// sectors more).
#[must_use]
pub fn rom_disc_bytes(files: &[RomFile]) -> u64 {
    files.iter().fold(0_u64, |total, file| {
        total
            .saturating_add(file.size.div_ceil(SECTOR).max(1).saturating_mul(SECTOR))
            .saturating_add(SECTOR)
    })
}

impl crate::Project {
    /// The files of the project's ROM content (none when it has none).
    ///
    /// # Errors
    ///
    /// As [`rom_files`].
    pub fn rom_files(&self) -> Result<Vec<RomFile>, String> {
        match self.settings.rom_content.as_deref() {
            Some(folder) if !folder.is_empty() => rom_files(Path::new(folder)),
            _ => Ok(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{rom_disc_bytes, rom_files};

    #[test]
    fn the_folder_is_listed_by_path() -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-romwalk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("docs/img"))?;
        std::fs::create_dir_all(dir.join("empty"))?;
        std::fs::write(dir.join("readme.txt"), b"hello")?;
        std::fs::write(dir.join("docs/manual.pdf"), vec![0_u8; 5000])?;
        std::fs::write(dir.join("docs/img/a.png"), b"")?;
        let files = rom_files(&dir)?;
        let listed: Vec<(&str, u64)> = files.iter().map(|f| (f.path.as_str(), f.size)).collect();
        assert_eq!(
            listed,
            [
                ("docs/img/a.png", 0),
                ("docs/manual.pdf", 5000),
                ("readme.txt", 5)
            ]
        );
        // One sector for the empty file, three for the manual, one for the text, and an entry each.
        assert_eq!(rom_disc_bytes(&files), (1 + 3 + 1 + 3) * 2048);

        // Eight folders deep is too deep; a folder that is not there is an error.
        std::fs::create_dir_all(dir.join("1/2/3/4/5/6/7/8"))?;
        std::fs::write(dir.join("1/2/3/4/5/6/7/8/x"), b"x")?;
        assert!(rom_files(&dir).is_err());
        assert!(rom_files(&dir.join("missing")).is_err());

        let mut project = crate::Project::default();
        assert!(project.rom_files()?.is_empty());
        project.settings.rom_content = Some(dir.join("docs").to_string_lossy().into_owned());
        assert_eq!(project.rom_files()?.len(), 2);
        // Kept in the project file only when set.
        let text = serde_json::to_string(&project.settings)?;
        assert!(text.contains("\"romContent\""), "{text}");
        let plain = serde_json::to_string(&crate::Project::default().settings)?;
        assert!(!plain.contains("romContent"), "{plain}");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
