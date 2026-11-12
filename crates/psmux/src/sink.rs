//! Sector sinks for DVD VOBS output.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// DVD sector size in bytes.
pub const SECTOR: usize = 2048;

/// Maximum sectors kept in one VOB file, keeping each file below 1 GiB.
pub const VOB_FILE_MAX_SECTORS: u32 = 524_287;

/// Output sink that receives complete DVD sectors.
pub trait SectorSink {
    /// Append one sector.
    fn write_sector(&mut self, sector: &[u8; SECTOR]) -> io::Result<()>;

    /// Overwrite one sector by index from the first sector ever written.
    fn overwrite_sector(&mut self, index: u32, sector: &[u8; SECTOR]) -> io::Result<()>;

    /// Flush buffered output.
    fn finish(&mut self) -> io::Result<()>;
}

/// In-memory sector sink, mainly for tests.
#[derive(Debug, Default)]
pub struct MemorySink {
    /// Written sectors.
    pub sectors: Vec<[u8; SECTOR]>,
}

impl SectorSink for MemorySink {
    fn write_sector(&mut self, sector: &[u8; SECTOR]) -> io::Result<()> {
        self.sectors.push(*sector);
        Ok(())
    }

    fn overwrite_sector(&mut self, index: u32, sector: &[u8; SECTOR]) -> io::Result<()> {
        let slot = self
            .sectors
            .get_mut(usize::try_from(index).map_err(|_| invalid_input("sector index"))?);
        match slot {
            Some(slot) => {
                *slot = *sector;
                Ok(())
            }
            None => Err(invalid_input("sector index out of range")),
        }
    }

    fn finish(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// File sink that writes `VTS_xx_n.VOB` files.
#[derive(Debug)]
pub struct VobFileSink {
    dir: PathBuf,
    vts: u8,
    current: Option<BufWriter<File>>,
    current_file: u32,
    sectors_written: u32,
    /// Maximum sectors per VOB file. Tests may lower this value.
    pub max_sectors: u32,
    created: Vec<u32>,
}

impl VobFileSink {
    /// Create a sink for title set `vts` in `dir`.
    pub fn new(dir: &Path, vts: u8) -> Self {
        Self {
            dir: dir.to_path_buf(),
            vts,
            current: None,
            current_file: 0,
            sectors_written: 0,
            max_sectors: VOB_FILE_MAX_SECTORS,
            created: Vec::new(),
        }
    }

    /// Return paths of VOB files created so far, in file-number order.
    pub fn files(&self) -> Vec<PathBuf> {
        self.created.iter().map(|&n| self.path(n)).collect()
    }

    fn path(&self, file_no: u32) -> PathBuf {
        self.dir
            .join(format!("VTS_{:02}_{}.VOB", self.vts, file_no))
    }

    fn file_no_for_sector(&self, index: u32) -> io::Result<u32> {
        if self.max_sectors == 0 {
            return Err(invalid_input("max_sectors must be non-zero"));
        }
        let file_no = index
            .checked_div(self.max_sectors)
            .and_then(|v| v.checked_add(1))
            .ok_or_else(|| invalid_input("sector index overflow"))?;
        if file_no > 9 {
            return Err(invalid_input("too many VOB files"));
        }
        Ok(file_no)
    }

    fn mark_created(&mut self, file_no: u32) {
        if !self.created.contains(&file_no) {
            self.created.push(file_no);
            self.created.sort_unstable();
        }
    }

    fn open_writer(&mut self, file_no: u32) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(self.path(file_no))?;
        self.current = Some(BufWriter::new(file));
        self.current_file = file_no;
        self.mark_created(file_no);
        Ok(())
    }

    fn rotate_to(&mut self, desired: u32) -> io::Result<()> {
        if let Some(writer) = self.current.as_mut() {
            writer.flush()?;
        }
        self.current = None;
        self.open_writer(desired)
    }
}

impl SectorSink for VobFileSink {
    fn write_sector(&mut self, sector: &[u8; SECTOR]) -> io::Result<()> {
        let desired = self.file_no_for_sector(self.sectors_written)?;
        if self.current.is_none() || self.current_file != desired {
            self.rotate_to(desired)?;
        }
        if let Some(writer) = self.current.as_mut() {
            writer.write_all(sector)?;
        } else {
            return Err(invalid_input("missing VOB writer"));
        }
        self.sectors_written = self
            .sectors_written
            .checked_add(1)
            .ok_or_else(|| invalid_input("sector count overflow"))?;
        Ok(())
    }

    fn overwrite_sector(&mut self, index: u32, sector: &[u8; SECTOR]) -> io::Result<()> {
        if let Some(writer) = self.current.as_mut() {
            writer.flush()?;
        }
        let file_no = self.file_no_for_sector(index)?;
        let offset = u64::from(index % self.max_sectors)
            .checked_mul(u64::try_from(SECTOR).map_err(|_| invalid_input("sector size"))?)
            .ok_or_else(|| invalid_input("sector offset overflow"))?;
        fs::create_dir_all(&self.dir)?;
        let path = self.path(file_no);
        self.mark_created(file_no);
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(sector)?;
        file.flush()?;
        Ok(())
    }

    fn finish(&mut self) -> io::Result<()> {
        if let Some(writer) = self.current.as_mut() {
            writer.flush()?;
        }
        self.current = None;
        Ok(())
    }
}

fn invalid_input(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg)
}
