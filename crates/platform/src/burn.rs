//! Writing a disc image to a recordable disc: Encore's Build panel output DVD Disc / Blu-ray Disc (Recorder, Write
//! Speed, Copies, Auto-erase rewritable discs).
//!
//! Windows: IMAPI2, the Image Mastering API v2 of the Windows API documentation. `IDiscMaster2` lists the
//! recorders, `IDiscRecorder2` opens one, `IDiscFormat2Data` writes a stream of 2048-byte sectors from the start of
//! a blank disc and closes the disc, `IDiscFormat2Erase` blanks a rewritable one. DiscCraft's image (UDF with the ISO
//! 9660 bridge, as the build writes it) goes on the disc as it is, through an `IStream` over the file that tells how
//! much has been read and stops when asked. The IMAPI2 work runs on a thread of its own, in the multithreaded
//! apartment, whatever thread asks. Other systems: no recorders yet.
//!
//! A dual-layer DVD (M9.42): the image's layer break (the sectors it puts on layer 0, as the build notes it beside
//! the image) is given to the drive before writing. A blank DVD+R DL's layer 0 holds as much as the disc allows
//! unless the host makes it smaller, once, with SEND DISC STRUCTURE format 20h (the L0 Data Area Capacity, a
//! multiple of 16 sectors); READ DISC STRUCTURE format 20h tells it, and whether it is fixed already (its Init
//! Status; a DVD-R DL's is always fixed). Written as the drive likes, the disc would change layers wherever its
//! layer 0 is full, mostly not where the image's break cell starts. After writing, the physical format information
//! (format 00h: the last sector in layer 0) tells where the layers really meet. SCSI Multi-Media Commands - 5,
//! T10/1675-D revision 4 (24 October 2006), 6.23.3.2.1 (Table 399), 6.23.3.2.22 (Table 430) and 6.34.3.2.4
//! (Table 580); sent as they are through IMAPI2's `IDiscRecorder2Ex`.
//!
//! Verifying (M9.38, DiscCraft's own: Encore's panel has no such box): after the disc is closed, and before it comes
//! out, the disc is read back through its drive letter's volume (`\\.\E:`, unbuffered, and past the end its file
//! system gives: `FSCTL_ALLOW_EXTENDED_DASD_IO`, for the UDF anchor in the image's last sector lies outside the UDF
//! partition) and compared with the image sector by sector.

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The bytes in a sector of a data disc.
pub const SECTOR_BYTES: u64 = 2048;
/// The bytes read at a time when a disc is verified (512 sectors).
const VERIFY_CHUNK: usize = 1 << 20;
/// What the verify buffers are aligned to (unbuffered reads need the sector size or more).
const VERIFY_ALIGN: usize = 4096;

/// A disc recorder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recorder {
    /// The system's unique id of the recorder, to open it again.
    pub id: String,
    /// Vendor and product, as the drive reports them.
    pub name: String,
    /// Its drive letters or mount points ("E:\").
    pub drives: Vec<String>,
}

impl Recorder {
    /// What the Build panel's Recorder list shows: "E: PIONEER BD-RW BDR-XD07" (Encore's form).
    #[must_use]
    pub fn label(&self) -> String {
        match self.drives.first() {
            Some(drive) => format!("{} {}", drive.trim_end_matches('\\'), self.name),
            None => self.name.clone(),
        }
    }
}

/// The kind of disc in a recorder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    /// CD-R.
    CdR,
    /// CD-RW.
    CdRw,
    /// DVD-RAM.
    DvdRam,
    /// DVD+R.
    DvdPlusR,
    /// DVD+RW.
    DvdPlusRw,
    /// DVD+R DL.
    DvdPlusRDualLayer,
    /// DVD-R.
    DvdMinusR,
    /// DVD-RW.
    DvdMinusRw,
    /// DVD-R DL.
    DvdMinusRDualLayer,
    /// DVD+RW DL.
    DvdPlusRwDualLayer,
    /// BD-R.
    BdR,
    /// BD-RE.
    BdRe,
    /// A CD the drive only reads: pressed, or a CD-R written and closed.
    CdRom,
    /// A DVD the drive only reads: pressed, or a DVD±R written and closed (the drive reports a closed DVD-R as
    /// DVD-ROM; M9.38).
    DvdRom,
    /// A Blu-ray disc the drive only reads: pressed, or a BD-R written and closed.
    BdRom,
    /// A disc IMAPI2 does not name.
    Other,
}

impl MediaKind {
    /// The kind's usual name ("DVD+R", "BD-RE").
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::CdR => "CD-R",
            Self::CdRw => "CD-RW",
            Self::DvdRam => "DVD-RAM",
            Self::DvdPlusR => "DVD+R",
            Self::DvdPlusRw => "DVD+RW",
            Self::DvdPlusRDualLayer => "DVD+R DL",
            Self::DvdMinusR => "DVD-R",
            Self::DvdMinusRw => "DVD-RW",
            Self::DvdMinusRDualLayer => "DVD-R DL",
            Self::DvdPlusRwDualLayer => "DVD+RW DL",
            Self::BdR => "BD-R",
            Self::BdRe => "BD-RE",
            Self::CdRom => "CD-ROM",
            Self::DvdRom => "DVD-ROM",
            Self::BdRom => "BD-ROM",
            Self::Other => "?",
        }
    }

    /// Whether the disc can be written again after it is erased or overwritten.
    #[must_use]
    pub fn rewritable(self) -> bool {
        matches!(
            self,
            Self::CdRw
                | Self::DvdRam
                | Self::DvdPlusRw
                | Self::DvdMinusRw
                | Self::DvdPlusRwDualLayer
                | Self::BdRe
        )
    }

    /// Whether the drive only reads it: a pressed disc, or a write-once one already written and closed (M9.38).
    #[must_use]
    pub fn read_only(self) -> bool {
        matches!(self, Self::CdRom | Self::DvdRom | Self::BdRom)
    }

    /// Whether it has two recordable layers (M9.42).
    #[must_use]
    pub fn dual_layer(self) -> bool {
        matches!(
            self,
            Self::DvdPlusRDualLayer | Self::DvdMinusRDualLayer | Self::DvdPlusRwDualLayer
        )
    }

    /// Whether a written disc of this kind is blanked before it is written again (the others are written over).
    #[must_use]
    pub fn erased_first(self) -> bool {
        matches!(self, Self::CdRw | Self::DvdMinusRw)
    }

    /// Whether it is a DVD.
    #[must_use]
    pub fn is_dvd(self) -> bool {
        matches!(
            self,
            Self::DvdRam
                | Self::DvdPlusR
                | Self::DvdPlusRw
                | Self::DvdPlusRDualLayer
                | Self::DvdMinusR
                | Self::DvdMinusRw
                | Self::DvdMinusRDualLayer
                | Self::DvdPlusRwDualLayer
                | Self::DvdRom
        )
    }

    /// Whether it is a Blu-ray disc.
    #[must_use]
    pub fn is_blu_ray(self) -> bool {
        matches!(self, Self::BdR | Self::BdRe | Self::BdRom)
    }

    /// The sectors a second at 1x for the kind (IMAPI2's: CD 75, DVD 680, BD 2195).
    #[must_use]
    pub fn sectors_per_second_at_1x(self) -> u32 {
        if self.is_blu_ray() {
            2195
        } else if self.is_dvd() {
            680
        } else {
            75
        }
    }

    /// A speed in sectors a second as a multiple of 1x ("8" for 8x), to the nearest tenth.
    #[must_use]
    pub fn speed_factor(self, sectors_per_second: u32) -> f64 {
        let factor = f64::from(sectors_per_second) / f64::from(self.sectors_per_second_at_1x());
        (factor * 10.0).round() / 10.0
    }
}

/// A disc in a recorder.
#[derive(Clone, Debug, PartialEq)]
pub struct Media {
    /// What kind of disc it is.
    pub kind: MediaKind,
    /// Whether IMAPI2 can write data on it in this recorder.
    pub supported: bool,
    /// Nothing written on it yet.
    pub blank: bool,
    /// Bytes free.
    pub free_bytes: u64,
    /// Bytes in all.
    pub total_bytes: u64,
    /// The write speeds offered for it, in sectors a second, fastest first.
    pub write_speeds: Vec<u32>,
    /// A dual-layer DVD's layer 0 as the drive reports it (M9.42).
    pub layer_boundary: Option<LayerBoundary>,
}

/// Where a recordable dual-layer DVD's first layer ends (M9.42).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerBoundary {
    /// The sectors of the data area on layer 0.
    pub layer0_sectors: u32,
    /// Whether it is fixed: set already, or not changeable (a DVD-R DL's); else the host may make it smaller once.
    pub fixed: bool,
}

/// What [`burn`] writes, and how.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BurnRequest {
    /// The image file (a whole number of 2048-byte sectors).
    pub image: PathBuf,
    /// The recorder's [`Recorder::id`].
    pub recorder: String,
    /// The write speed in sectors a second, or the fastest.
    pub speed: Option<u32>,
    /// Whether a rewritable disc with something on it is erased (or written over) first; else it is refused.
    pub erase_rewritable: bool,
    /// Whether the disc comes out when it is done.
    pub eject: bool,
    /// Whether the disc is read back after it is closed and compared with the image (M9.38), before it comes out.
    pub verify: bool,
    /// The sectors the image puts on layer 0 of a dual-layer disc (its layer break: `LayerBreak=` of the
    /// `<name>.layerbreak.txt` the build writes beside it, M9.42); `None` for an image of one layer.
    pub layer_break: Option<u32>,
}

/// What [`burn`] is doing, for its progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BurnStep {
    /// Erasing a rewritable disc.
    Erasing,
    /// Writing the image.
    Writing,
    /// Closing the disc after the last sector.
    Closing,
    /// Reading the disc back and comparing it with the image (M9.38).
    Verifying,
}

/// Why a disc could not be written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BurnError {
    /// This system has no way to write discs from DiscCraft yet.
    Unsupported,
    /// The recorder is gone (unplugged, or not a recorder).
    NoRecorder,
    /// The recorder has no disc in it.
    NoMedia,
    /// The disc in the recorder cannot take data (a pressed disc, or one of a kind the recorder does not write).
    MediaNotSupported(MediaKind),
    /// The disc has something on it and cannot be written again (or erasing was not asked).
    NotBlank,
    /// The image does not fit on the disc.
    TooLarge {
        /// The image's bytes.
        needed: u64,
        /// The disc's free bytes.
        free: u64,
    },
    /// The disc is write-protected.
    WriteProtected,
    /// The recorder is busy (another program, or the disc is still spinning up).
    Busy,
    /// The writing was stopped.
    Cancelled,
    /// The image could not be read.
    Image(String),
    /// The disc read back differs from the image, from this sector (counted from 0; M9.38).
    VerifyMismatch {
        /// The first sector that differs.
        sector: u64,
    },
    /// The disc could not be read back to verify it (M9.38).
    VerifyRead(String),
    /// The image's layer break cannot be given to the dual-layer disc (M9.42).
    LayerBreak {
        /// The sectors the image puts on layer 0.
        wanted: u32,
        /// The sectors of the disc's layer 0 (0 when the drive does not tell).
        disc: u32,
        /// The disc's layer 0 is fixed (set already, or a DVD-R DL's), rather than too small.
        fixed: bool,
    },
    /// The written disc's layer 0 does not end where the image's layer break is (M9.42).
    LayerBreakMoved {
        /// The sectors the image puts on layer 0.
        wanted: u32,
        /// The sectors on the written disc's layer 0.
        written: u32,
    },
    /// Anything else the system reported, with its code.
    System {
        /// The `HRESULT`.
        code: i32,
        /// The system's words for it.
        message: String,
    },
}

impl fmt::Display for BurnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "writing discs is not available on this system"),
            Self::NoRecorder => write!(f, "the recorder is not available"),
            Self::NoMedia => write!(f, "there is no disc in the recorder"),
            Self::MediaNotSupported(kind) => write!(
                f,
                "the disc in the recorder ({}) cannot be written",
                kind.name()
            ),
            Self::NotBlank => write!(f, "the disc is not blank"),
            Self::TooLarge { needed, free } => {
                write!(
                    f,
                    "the image ({needed} bytes) does not fit on the disc ({free} bytes free)"
                )
            }
            Self::WriteProtected => write!(f, "the disc is write-protected"),
            Self::Busy => write!(f, "the recorder is busy"),
            Self::Cancelled => write!(f, "writing was stopped"),
            Self::Image(message) => write!(f, "the image cannot be read: {message}"),
            Self::VerifyMismatch { sector } => {
                write!(f, "the disc differs from the image from sector {sector}")
            }
            Self::VerifyRead(message) => {
                write!(f, "the disc cannot be read back to verify it: {message}")
            }
            Self::LayerBreak {
                wanted, disc: 0, ..
            } => write!(
                f,
                "the drive does not tell where the dual-layer disc's first layer ends, so the layer break                  (sector {wanted}) cannot be placed"
            ),
            Self::LayerBreak {
                wanted,
                disc,
                fixed: true,
            } => write!(
                f,
                "the disc's first layer is fixed at {disc} sectors; the image's layer break is at sector {wanted}                  (use a blank DVD+R DL)"
            ),
            Self::LayerBreak {
                wanted,
                disc,
                fixed: false,
            } => write!(
                f,
                "the image's first layer ({wanted} sectors) is larger than the disc's ({disc} sectors)"
            ),
            Self::LayerBreakMoved { wanted, written } => write!(
                f,
                "the written disc's first layer ends after {written} sectors, not at the image's layer break                  ({wanted})"
            ),
            Self::System { code, message } => write!(f, "{message} (0x{code:08X})"),
        }
    }
}

impl std::error::Error for BurnError {}

/// The recorders that can write data discs, in the system's order (empty on a computer without one).
///
/// # Errors
///
/// When the system's disc writing cannot be reached.
pub fn recorders() -> Result<Vec<Recorder>, BurnError> {
    imp::recorders()
}

/// The disc in the recorder `recorder` ([`Recorder::id`]), `None` when it is empty.
///
/// # Errors
///
/// When the recorder cannot be opened or asked.
pub fn media(recorder: &str) -> Result<Option<Media>, BurnError> {
    imp::media(recorder)
}

/// Writes `request`'s image to the disc in its recorder and closes the disc: erases (or writes over) a rewritable
/// disc first when asked, tells `progress` how far it is (0 to 1 for each step), and stops when `cancel` is set
/// (a write-once disc stopped while it is written cannot be used again).
///
/// # Errors
///
/// As [`BurnError`].
pub fn burn(
    request: &BurnRequest,
    progress: Arc<dyn Fn(BurnStep, f32) + Send + Sync>,
    cancel: Arc<AtomicBool>,
) -> Result<(), BurnError> {
    imp::burn(request.clone(), progress, cancel)
}

/// Reads the disc in the recorder `recorder` back and compares it with `image` sector by sector (M9.38: a disc
/// written before, or by another program), telling `progress` how far it is (0 to 1) and stopping when `cancel` is set.
///
/// # Errors
///
/// [`BurnError::VerifyMismatch`] at the first sector that differs, [`BurnError::VerifyRead`] when the disc cannot be
/// read (or ends before the image does), and as [`BurnError`].
pub fn verify(
    recorder: &str,
    image: &Path,
    progress: Arc<dyn Fn(f32) + Send + Sync>,
    cancel: Arc<AtomicBool>,
) -> Result<(), BurnError> {
    imp::verify(recorder, image, progress, cancel)
}

/// A buffer of `len` bytes starting at a multiple of [`VERIFY_ALIGN`] in memory, inside `storage`.
fn aligned(storage: &mut Vec<u8>, len: usize) -> &mut [u8] {
    storage.clear();
    storage.resize(len.saturating_add(VERIFY_ALIGN), 0);
    let offset = storage
        .as_ptr()
        .align_offset(VERIFY_ALIGN)
        .min(VERIFY_ALIGN);
    let end = offset.saturating_add(len).min(storage.len());
    storage.get_mut(offset..end).unwrap_or_default()
}

/// Reads `buffer` full from `from`, or as much as there is: the bytes read.
fn read_full(from: &mut impl Read, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut done = 0;
    while let Some(rest) = buffer.get_mut(done..).filter(|rest| !rest.is_empty()) {
        match from.read(rest) {
            Ok(0) => break,
            Ok(read) => done += read,
            Err(failure) if failure.kind() == std::io::ErrorKind::Interrupted => {}
            Err(failure) => return Err(failure),
        }
    }
    Ok(done)
}

/// Compares the first `size` bytes (a whole number of sectors) of `disc` with `image`, [`VERIFY_CHUNK`] at a time
/// into buffers aligned for unbuffered reads, telling `progress` how far (0 to 1).
///
/// # Errors
///
/// [`BurnError::VerifyMismatch`] at the first sector that differs; [`BurnError::VerifyRead`] when the disc cannot be
/// read or ends first; [`BurnError::Image`] when the image cannot be read; [`BurnError::Cancelled`] when `cancel` is set.
fn compare_with_image(
    image: &mut impl Read,
    disc: &mut impl Read,
    size: u64,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> Result<(), BurnError> {
    let (mut image_storage, mut disc_storage) = (Vec::new(), Vec::new());
    let sector_bytes = usize::try_from(SECTOR_BYTES).unwrap_or(2048);
    let mut done = 0u64;
    progress(0.0);
    while done < size {
        if cancel.load(Ordering::Relaxed) {
            return Err(BurnError::Cancelled);
        }
        let want = usize::try_from((size - done).min(VERIFY_CHUNK as u64)).unwrap_or(VERIFY_CHUNK);
        let expected = aligned(&mut image_storage, want);
        let from_image =
            read_full(image, expected).map_err(|failure| BurnError::Image(failure.to_string()))?;
        if from_image < want {
            return Err(BurnError::Image(format!(
                "it ends at byte {}, before {size}",
                done + from_image as u64
            )));
        }
        let found = aligned(&mut disc_storage, want);
        let from_disc = read_full(disc, found).map_err(|failure| {
            BurnError::VerifyRead(format!("sector {}: {failure}", done / SECTOR_BYTES))
        })?;
        let compared = from_disc.min(want);
        let (expected, found) = (
            expected.get(..compared).unwrap_or_default(),
            found.get(..compared).unwrap_or_default(),
        );
        if let Some(at) = expected
            .chunks(sector_bytes)
            .zip(found.chunks(sector_bytes))
            .position(|(a, b)| a != b)
        {
            return Err(BurnError::VerifyMismatch {
                sector: done / SECTOR_BYTES + at as u64,
            });
        }
        if from_disc < want {
            return Err(BurnError::VerifyRead(format!(
                "the disc ends at sector {}, the image at sector {}",
                (done + from_disc as u64) / SECTOR_BYTES,
                size / SECTOR_BYTES
            )));
        }
        done += want as u64;
        // `as` is exact enough for a fraction.
        progress((done as f64 / size as f64) as f32);
    }
    Ok(())
}

/// Opens the tray of the recorder `recorder` (for the next copy).
///
/// # Errors
///
/// When the recorder cannot be opened or does not open.
pub fn eject(recorder: &str) -> Result<(), BurnError> {
    imp::eject(recorder)
}

#[cfg(not(windows))]
mod imp {
    use super::{BurnError, BurnRequest, BurnStep, Media, Recorder};
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    pub(super) fn recorders() -> Result<Vec<Recorder>, BurnError> {
        Ok(Vec::new())
    }

    pub(super) fn verify(
        _recorder: &str,
        _image: &Path,
        _progress: Arc<dyn Fn(f32) + Send + Sync>,
        _cancel: Arc<AtomicBool>,
    ) -> Result<(), BurnError> {
        Err(BurnError::Unsupported)
    }

    pub(super) fn media(_recorder: &str) -> Result<Option<Media>, BurnError> {
        Err(BurnError::Unsupported)
    }

    pub(super) fn burn(
        _request: BurnRequest,
        _progress: Arc<dyn Fn(BurnStep, f32) + Send + Sync>,
        _cancel: Arc<AtomicBool>,
    ) -> Result<(), BurnError> {
        Err(BurnError::Unsupported)
    }

    pub(super) fn eject(_recorder: &str) -> Result<(), BurnError> {
        Err(BurnError::Unsupported)
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use super::{
        BurnError, BurnRequest, BurnStep, LayerBoundary, Media, MediaKind, Recorder, SECTOR_BYTES,
        compare_with_image,
    };
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom};
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use windows::Win32::Foundation::{E_ABORT, E_NOTIMPL, E_POINTER, S_FALSE, S_OK, VARIANT_BOOL};
    use windows::Win32::Storage::Imapi::{
        IDiscFormat2Data, IDiscFormat2Erase, IDiscMaster2, IDiscRecorder2, IDiscRecorder2Ex,
        IMAPI_MEDIA_PHYSICAL_TYPE, IMAPI_MEDIA_TYPE_BDR, IMAPI_MEDIA_TYPE_BDRE,
        IMAPI_MEDIA_TYPE_BDROM, IMAPI_MEDIA_TYPE_CDR, IMAPI_MEDIA_TYPE_CDROM,
        IMAPI_MEDIA_TYPE_CDRW, IMAPI_MEDIA_TYPE_DVDDASHR, IMAPI_MEDIA_TYPE_DVDDASHR_DUALLAYER,
        IMAPI_MEDIA_TYPE_DVDDASHRW, IMAPI_MEDIA_TYPE_DVDPLUSR, IMAPI_MEDIA_TYPE_DVDPLUSR_DUALLAYER,
        IMAPI_MEDIA_TYPE_DVDPLUSRW, IMAPI_MEDIA_TYPE_DVDPLUSRW_DUALLAYER, IMAPI_MEDIA_TYPE_DVDRAM,
        IMAPI_MEDIA_TYPE_DVDROM, MsftDiscFormat2Data, MsftDiscFormat2Erase, MsftDiscMaster2,
        MsftDiscRecorder2,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
        ISequentialStream_Impl, IStream, IStream_Impl, LOCKTYPE, SAFEARRAY, STATFLAG, STATSTG,
        STGC, STGTY_STREAM, STREAM_SEEK, STREAM_SEEK_CUR, STREAM_SEEK_END, STREAM_SEEK_SET,
    };
    use windows::Win32::System::Ole::{
        SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
        SafeArrayUnaccessData,
    };
    use windows::Win32::System::Variant::{VARIANT, VT_BSTR, VT_I4, VT_UI4};
    use windows::core::{BSTR, HRESULT, Interface, implement};

    // IMAPI2's errors (imapi2error.h, as the Windows bindings carry them).
    const NO_MEDIA: HRESULT = HRESULT(0xC0AA_0202_u32 as i32);
    const MEDIA_INCOMPATIBLE: HRESULT = HRESULT(0xC0AA_0203_u32 as i32);
    const MEDIA_UPSIDE_DOWN: HRESULT = HRESULT(0xC0AA_0204_u32 as i32);
    const MEDIA_BECOMING_READY: HRESULT = HRESULT(0xC0AA_0205_u32 as i32);
    const MEDIA_BUSY: HRESULT = HRESULT(0xC0AA_0207_u32 as i32);
    const MEDIA_WRITE_PROTECTED: HRESULT = HRESULT(0xC0AA_0209_u32 as i32);
    const RECORDER_LOCKED: HRESULT = HRESULT(0xC0AA_0210_u32 as i32);
    const STREAM_TOO_LARGE: HRESULT = HRESULT(0xC0AA_0404_u32 as i32);
    const MEDIA_NOT_BLANK: HRESULT = HRESULT(0xC0AA_0405_u32 as i32);
    const MEDIA_IS_NOT_SUPPORTED: HRESULT = HRESULT(0xC0AA_0406_u32 as i32);
    const REQUEST_CANCELLED: HRESULT = HRESULT(0xC0AA_0002_u32 as i32);

    /// The most recorders listed.
    const MAX_RECORDERS: i32 = 32;
    /// The most write speeds or mount points read from one list.
    const MAX_LIST: i32 = 64;
    /// The name IMAPI2 shows as the program writing the disc.
    const CLIENT: &str = "DiscCraft";
    /// How long a disc just closed may take to be readable before it is verified, in seconds.
    const VERIFY_WAIT_SECONDS: u32 = 120;
    /// `FILE_FLAG_NO_BUFFERING`: the disc's sectors are read from the disc, not from the system's cache.
    const NO_BUFFERING: u32 = 0x2000_0000;

    /// Runs `work` on a thread of its own in the multithreaded apartment (any thread may ask, the UI's single-threaded
    /// one too), and waits for it.
    fn on_com_thread<T: Send + 'static>(
        work: impl FnOnce() -> Result<T, BurnError> + Send + 'static,
    ) -> Result<T, BurnError> {
        let thread = std::thread::Builder::new()
            .name("disccraft-imapi2".to_owned())
            .spawn(move || {
                // SAFETY: no reserved pointer is passed; the thread is new, so it is not in another apartment.
                let initialised = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
                let result = work();
                if initialised {
                    // SAFETY: balances the successful CoInitializeEx above, on the same thread, after every COM
                    // object of `work` has been released.
                    unsafe { CoUninitialize() };
                }
                result
            })
            .map_err(|error| BurnError::System {
                code: 0,
                message: error.to_string(),
            })?;
        thread.join().unwrap_or_else(|_| {
            Err(BurnError::System {
                code: 0,
                message: "the disc writing stopped unexpectedly".to_owned(),
            })
        })
    }

    /// The error for an IMAPI2 failure.
    fn error(failure: &windows::core::Error) -> BurnError {
        let code = failure.code();
        match code {
            NO_MEDIA | MEDIA_UPSIDE_DOWN => BurnError::NoMedia,
            MEDIA_INCOMPATIBLE | MEDIA_IS_NOT_SUPPORTED => {
                BurnError::MediaNotSupported(MediaKind::Other)
            }
            MEDIA_NOT_BLANK => BurnError::NotBlank,
            MEDIA_WRITE_PROTECTED => BurnError::WriteProtected,
            MEDIA_BECOMING_READY | MEDIA_BUSY | RECORDER_LOCKED => BurnError::Busy,
            REQUEST_CANCELLED | E_ABORT => BurnError::Cancelled,
            _ => BurnError::System {
                code: code.0,
                message: failure.message(),
            },
        }
    }

    fn truthy(value: VARIANT_BOOL) -> bool {
        value.0 != 0
    }

    fn variant_bool(value: bool) -> VARIANT_BOOL {
        VARIANT_BOOL(if value { -1 } else { 0 })
    }

    /// The elements of a one-dimensional SAFEARRAY of VARIANTs, each read by `read`; the array is destroyed.
    fn variants<T>(array: *mut SAFEARRAY, read: impl Fn(&VARIANT) -> Option<T>) -> Vec<T> {
        if array.is_null() {
            return Vec::new();
        }
        let mut found = Vec::new();
        // SAFETY: `array` is a SAFEARRAY the caller received from IMAPI2 and owns; its bounds are read before its data
        // is touched.
        let bounds = unsafe { (SafeArrayGetLBound(array, 1), SafeArrayGetUBound(array, 1)) };
        if let (Ok(low), Ok(high)) = bounds {
            let count = usize::try_from(
                high.saturating_sub(low)
                    .saturating_add(1)
                    .clamp(0, MAX_LIST),
            )
            .unwrap_or(0);
            let mut data: *mut core::ffi::c_void = core::ptr::null_mut();
            // SAFETY: locks the array's data for reading and gives a pointer to its first element.
            if unsafe { SafeArrayAccessData(array, &raw mut data) }.is_ok() && !data.is_null() {
                let first = data.cast::<VARIANT>();
                for index in 0..count {
                    // SAFETY: the array holds at least `count` VARIANTs (from its bounds, capped) from `first`, locked
                    // above; each is only read.
                    let variant = unsafe { &*first.add(index) };
                    if let Some(value) = read(variant) {
                        found.push(value);
                    }
                }
                // SAFETY: unlocks what SafeArrayAccessData locked.
                let _ = unsafe { SafeArrayUnaccessData(array) };
            }
        }
        // SAFETY: the array is the caller's, given up here, and no reference into it is kept.
        let _ = unsafe { SafeArrayDestroy(array) };
        found
    }

    /// A VARIANT's whole number (VT_I4 / VT_UI4), not negative.
    fn variant_number(variant: &VARIANT) -> Option<u32> {
        // SAFETY: the tag says which member of the union holds the value; only that one is read.
        unsafe {
            let inner = &variant.Anonymous.Anonymous;
            if inner.vt == VT_UI4 {
                Some(inner.Anonymous.ulVal)
            } else if inner.vt == VT_I4 {
                u32::try_from(inner.Anonymous.lVal).ok()
            } else {
                None
            }
        }
    }

    /// A VARIANT's text (VT_BSTR).
    fn variant_text(variant: &VARIANT) -> Option<String> {
        // SAFETY: the tag says which member of the union holds the value; only that one is read, and the string is
        // copied, not taken.
        unsafe {
            let inner = &variant.Anonymous.Anonymous;
            (inner.vt == VT_BSTR).then(|| inner.Anonymous.bstrVal.to_string())
        }
    }

    fn create<T: Interface>(class: &windows::core::GUID) -> Result<T, BurnError> {
        // SAFETY: creates an in-process IMAPI2 object of a class the system registers; no outer object.
        unsafe { CoCreateInstance(class, None, CLSCTX_ALL) }.map_err(|failure| error(&failure))
    }

    /// The recorder with the unique id `id`.
    fn open(id: &str) -> Result<IDiscRecorder2, BurnError> {
        let recorder: IDiscRecorder2 = create(&MsftDiscRecorder2)?;
        // SAFETY: a plain COM call on the new recorder object with a valid BSTR.
        unsafe { recorder.InitializeDiscRecorder(&BSTR::from(id)) }
            .map_err(|_| BurnError::NoRecorder)?;
        Ok(recorder)
    }

    /// READ DISC STRUCTURE (MMC-5 6.23).
    const READ_DISC_STRUCTURE: u8 = 0xAD;
    /// SEND DISC STRUCTURE (MMC-5 6.34).
    const SEND_DISC_STRUCTURE: u8 = 0xBF;
    /// The disc structure format of the physical format information (Table 399).
    const PHYSICAL_FORMAT: u8 = 0x00;
    /// The disc structure format of a dual-layer DVD±R's layer boundary (Tables 430 and 580).
    const LAYER_BOUNDARY: u8 = 0x20;
    /// The first physical sector of a DVD's data area (Table 405).
    const DATA_AREA_START: u32 = 0x3_0000;
    /// How long a disc structure command may take, seconds.
    const COMMAND_SECONDS: u32 = 30;

    /// The disc structure of `format` (media type DVD, layer 0): `length` bytes, its four-byte header first.
    fn read_disc_structure(recorder: &IDiscRecorder2, format: u8, length: u16) -> Option<Vec<u8>> {
        let device: IDiscRecorder2Ex = recorder.cast().ok()?;
        let [high, low] = length.to_be_bytes();
        let cdb = [
            READ_DISC_STRUCTURE,
            0,
            0,
            0,
            0,
            0,
            0,
            format,
            high,
            low,
            0,
            0,
        ];
        let mut sense = [0u8; 18];
        let mut data = vec![0u8; usize::from(length)];
        let mut fetched = 0u32;
        // SAFETY: the CDB, sense buffer and data buffer are live and as long as the call is told; the drive writes
        // at most `length` bytes into `data` and the count into `fetched`.
        unsafe {
            device.SendCommandGetDataFromDevice(
                &cdb,
                &mut sense,
                COMMAND_SECONDS,
                &mut data,
                &raw mut fetched,
            )
        }
        .ok()?;
        data.truncate(usize::try_from(fetched).ok()?);
        Some(data)
    }

    /// A big-endian `u32` at `at` of `data`.
    fn be32(data: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
    }

    /// A dual-layer DVD±R's layer boundary (format 20h): Init Status (byte 4, bit 7) and the L0 Data Area Capacity
    /// (bytes 8 to 11).
    fn layer_boundary(recorder: &IDiscRecorder2) -> Option<LayerBoundary> {
        let data = read_disc_structure(recorder, LAYER_BOUNDARY, 12)?;
        Some(LayerBoundary {
            layer0_sectors: be32(&data, 8)?,
            fixed: data.get(4)? & 0x80 != 0,
        })
    }

    /// Makes a blank DVD+R DL's layer 0 `sectors` long (format 20h): the parameter list is its length (10), two
    /// reserved bytes, four reserved bytes and the L0 Data Area Capacity. The recorder is held for it alone.
    fn set_layer_boundary(recorder: &IDiscRecorder2, sectors: u32) -> Result<(), BurnError> {
        let device: IDiscRecorder2Ex = recorder.cast().map_err(|failure| error(&failure))?;
        let cdb = [
            SEND_DISC_STRUCTURE,
            0,
            0,
            0,
            0,
            0,
            0,
            LAYER_BOUNDARY,
            0,
            12,
            0,
            0,
        ];
        let mut list = [0u8; 12];
        list[1] = 10;
        list[8..12].copy_from_slice(&sectors.to_be_bytes());
        let mut sense = [0u8; 18];
        // SAFETY: plain COM calls on the live recorder; the CDB, sense buffer and parameter list are live and as long
        // as the call is told.
        unsafe {
            recorder
                .AcquireExclusiveAccess(variant_bool(false), &BSTR::from(CLIENT))
                .map_err(|failure| error(&failure))?;
            let sent = device.SendCommandSendDataToDevice(&cdb, &mut sense, COMMAND_SECONDS, &list);
            let _ = recorder.ReleaseExclusiveAccess();
            sent.map_err(|failure| error(&failure))
        }
    }

    /// Gives a blank dual-layer disc the image's layer break, `wanted` sectors on layer 0, and checks it took it.
    fn place_layer_break(recorder: &IDiscRecorder2, wanted: u32) -> Result<(), BurnError> {
        let refused = |disc: u32, fixed: bool| BurnError::LayerBreak {
            wanted,
            disc,
            fixed,
        };
        let current = layer_boundary(recorder).ok_or_else(|| refused(0, true))?;
        if current.layer0_sectors == wanted {
            return Ok(());
        }
        if current.fixed {
            return Err(refused(current.layer0_sectors, true));
        }
        if wanted > current.layer0_sectors {
            return Err(refused(current.layer0_sectors, false));
        }
        set_layer_boundary(recorder, wanted)?;
        match layer_boundary(recorder) {
            Some(now) if now.layer0_sectors == wanted => Ok(()),
            Some(now) => Err(refused(now.layer0_sectors, true)),
            None => Err(refused(0, true)),
        }
    }

    /// The sectors on a written dual-layer disc's layer 0, from its physical format information (format 00h): the
    /// last sector in layer 0 (bytes 16 to 19) after the first of the data area (bytes 8 to 11), for a disc of two
    /// layers (byte 6, bits 6 and 5).
    fn written_layer0(recorder: &IDiscRecorder2) -> Option<u32> {
        let data = read_disc_structure(recorder, PHYSICAL_FORMAT, 24)?;
        let layers = (data.get(6)? >> 5) & 0b11;
        if layers != 1 {
            return None;
        }
        let start = be32(&data, 8)? & 0x00FF_FFFF;
        let end = be32(&data, 16)? & 0x00FF_FFFF;
        (start == DATA_AREA_START && end >= start).then(|| end - start + 1)
    }

    /// The data writer for `recorder`.
    fn writer(recorder: &IDiscRecorder2) -> Result<IDiscFormat2Data, BurnError> {
        let format: IDiscFormat2Data = create(&MsftDiscFormat2Data)?;
        // SAFETY: plain COM calls on live objects with valid arguments.
        unsafe {
            format
                .SetRecorder(recorder)
                .map_err(|failure| error(&failure))?;
            format
                .SetClientName(&BSTR::from(CLIENT))
                .map_err(|failure| error(&failure))?;
        }
        Ok(format)
    }

    fn kind(value: IMAPI_MEDIA_PHYSICAL_TYPE) -> MediaKind {
        match value {
            IMAPI_MEDIA_TYPE_CDR => MediaKind::CdR,
            IMAPI_MEDIA_TYPE_CDRW => MediaKind::CdRw,
            IMAPI_MEDIA_TYPE_DVDRAM => MediaKind::DvdRam,
            IMAPI_MEDIA_TYPE_DVDPLUSR => MediaKind::DvdPlusR,
            IMAPI_MEDIA_TYPE_DVDPLUSRW => MediaKind::DvdPlusRw,
            IMAPI_MEDIA_TYPE_DVDPLUSR_DUALLAYER => MediaKind::DvdPlusRDualLayer,
            IMAPI_MEDIA_TYPE_DVDDASHR => MediaKind::DvdMinusR,
            IMAPI_MEDIA_TYPE_DVDDASHRW => MediaKind::DvdMinusRw,
            IMAPI_MEDIA_TYPE_DVDDASHR_DUALLAYER => MediaKind::DvdMinusRDualLayer,
            IMAPI_MEDIA_TYPE_DVDPLUSRW_DUALLAYER => MediaKind::DvdPlusRwDualLayer,
            IMAPI_MEDIA_TYPE_BDR => MediaKind::BdR,
            IMAPI_MEDIA_TYPE_BDRE => MediaKind::BdRe,
            IMAPI_MEDIA_TYPE_CDROM => MediaKind::CdRom,
            IMAPI_MEDIA_TYPE_DVDROM => MediaKind::DvdRom,
            IMAPI_MEDIA_TYPE_BDROM => MediaKind::BdRom,
            _ => MediaKind::Other,
        }
    }

    pub(super) fn recorders() -> Result<Vec<Recorder>, BurnError> {
        on_com_thread(|| {
            let master: IDiscMaster2 = create(&MsftDiscMaster2)?;
            let probe: IDiscFormat2Data = create(&MsftDiscFormat2Data)?;
            // SAFETY: plain COM calls on the live master object.
            let (supported, count) = unsafe { (master.IsSupportedEnvironment(), master.Count()) };
            if !supported.map(truthy).unwrap_or(false) {
                return Ok(Vec::new());
            }
            let count = count
                .map_err(|failure| error(&failure))?
                .clamp(0, MAX_RECORDERS);
            let mut found = Vec::new();
            for index in 0..count {
                // SAFETY: `index` is below the count the master gave.
                let Ok(id) = (unsafe { master.get_Item(index) }) else {
                    continue;
                };
                let id = id.to_string();
                let Ok(recorder) = open(&id) else {
                    continue;
                };
                // A drive that only reads (or a virtual one) is no recorder.
                // SAFETY: plain COM calls on live objects.
                let writes = unsafe { probe.IsRecorderSupported(&recorder) }
                    .map(truthy)
                    .unwrap_or(false);
                if !writes {
                    continue;
                }
                // SAFETY: plain COM calls on the live recorder; the SAFEARRAY is handed to `variants`, which owns it.
                let (vendor, product, paths) = unsafe {
                    (
                        recorder.VendorId(),
                        recorder.ProductId(),
                        recorder.VolumePathNames(),
                    )
                };
                let name = [vendor, product]
                    .into_iter()
                    .filter_map(Result::ok)
                    .map(|text| text.to_string().trim().to_owned())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                let drives = paths
                    .map(|array| variants(array, variant_text))
                    .unwrap_or_default();
                found.push(Recorder { id, name, drives });
            }
            Ok(found)
        })
    }

    pub(super) fn media(id: &str) -> Result<Option<Media>, BurnError> {
        let id = id.to_owned();
        on_com_thread(move || {
            let recorder = open(&id)?;
            let format: IDiscFormat2Data = create(&MsftDiscFormat2Data)?;
            // SAFETY: plain COM calls on live objects.
            let supported = unsafe { format.IsCurrentMediaSupported(&recorder) }
                .map(truthy)
                .unwrap_or(false);
            // SAFETY: as above.
            unsafe { format.SetRecorder(&recorder) }.map_err(|failure| error(&failure))?;
            // SAFETY: as above.
            let physical = match unsafe { format.CurrentPhysicalMediaType() } {
                Ok(value) => kind(value),
                Err(failure)
                    if failure.code() == NO_MEDIA || failure.code() == MEDIA_UPSIDE_DOWN =>
                {
                    return Ok(None);
                }
                Err(failure) => return Err(error(&failure)),
            };
            if !supported {
                return Ok(Some(Media {
                    kind: physical,
                    supported: false,
                    blank: false,
                    free_bytes: 0,
                    total_bytes: 0,
                    write_speeds: Vec::new(),
                    layer_boundary: None,
                }));
            }
            // SAFETY: plain COM calls on the live writer; the SAFEARRAY is handed to `variants`, which owns it.
            let (blank, free, total, speeds) = unsafe {
                (
                    format.MediaHeuristicallyBlank(),
                    format.FreeSectorsOnMedia(),
                    format.TotalSectorsOnMedia(),
                    format.SupportedWriteSpeeds(),
                )
            };
            let bytes = |sectors: windows::core::Result<i32>| {
                sectors
                    .ok()
                    .and_then(|sectors| u64::try_from(sectors).ok())
                    .map_or(0, |sectors| sectors.saturating_mul(SECTOR_BYTES))
            };
            let mut write_speeds = speeds
                .map(|array| variants(array, variant_number))
                .unwrap_or_default();
            write_speeds.retain(|speed| *speed > 0);
            write_speeds.sort_unstable_by(|a, b| b.cmp(a));
            write_speeds.dedup();
            Ok(Some(Media {
                kind: physical,
                supported: true,
                blank: blank.map(truthy).unwrap_or(false),
                free_bytes: bytes(free),
                total_bytes: bytes(total),
                write_speeds,
                layer_boundary: physical
                    .dual_layer()
                    .then(|| layer_boundary(&recorder))
                    .flatten(),
            }))
        })
    }

    pub(super) fn eject(id: &str) -> Result<(), BurnError> {
        let id = id.to_owned();
        on_com_thread(move || {
            let recorder = open(&id)?;
            // SAFETY: a plain COM call on the live recorder.
            unsafe { recorder.EjectMedia() }.map_err(|failure| error(&failure))
        })
    }

    pub(super) fn burn(
        request: BurnRequest,
        progress: Arc<dyn Fn(BurnStep, f32) + Send + Sync>,
        cancel: Arc<AtomicBool>,
    ) -> Result<(), BurnError> {
        on_com_thread(move || write_image(&request, &progress, &cancel))
    }

    fn write_image(
        request: &BurnRequest,
        progress: &Arc<dyn Fn(BurnStep, f32) + Send + Sync>,
        cancel: &Arc<AtomicBool>,
    ) -> Result<(), BurnError> {
        let file =
            File::open(&request.image).map_err(|failure| BurnError::Image(failure.to_string()))?;
        let size = file
            .metadata()
            .map_err(|failure| BurnError::Image(failure.to_string()))?
            .len();
        if size == 0 || size % SECTOR_BYTES != 0 {
            return Err(BurnError::Image(format!(
                "{size} bytes is not a whole number of {SECTOR_BYTES}-byte sectors"
            )));
        }
        let recorder = open(&request.recorder)?;
        let format = writer(&recorder)?;
        // SAFETY: plain COM calls on live objects.
        let physical = match unsafe { format.CurrentPhysicalMediaType() } {
            Ok(value) => kind(value),
            Err(failure) => return Err(error(&failure)),
        };
        // SAFETY: as above.
        let supported = unsafe { format.IsCurrentMediaSupported(&recorder) }
            .map(truthy)
            .unwrap_or(false);
        if !supported {
            return Err(BurnError::MediaNotSupported(physical));
        }
        // SAFETY: as above.
        let blank = unsafe { format.MediaHeuristicallyBlank() }
            .map(truthy)
            .unwrap_or(false);
        let mut format = format;
        if !blank {
            if !physical.rewritable() || !request.erase_rewritable {
                return Err(BurnError::NotBlank);
            }
            if physical.erased_first() {
                progress(BurnStep::Erasing, 0.0);
                erase(&recorder)?;
                progress(BurnStep::Erasing, 1.0);
                // A fresh writer sees the erased disc.
                format = writer(&recorder)?;
            } else {
                // DVD+RW, DVD-RAM and BD-RE are written over from the start.
                // SAFETY: a plain COM call on the live writer.
                unsafe { format.SetForceOverwrite(variant_bool(true)) }
                    .map_err(|failure| error(&failure))?;
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(BurnError::Cancelled);
        }
        // SAFETY: a plain COM call on the live writer.
        let free = unsafe { format.FreeSectorsOnMedia() }
            .ok()
            .and_then(|sectors| u64::try_from(sectors).ok())
            .map_or(0, |sectors| sectors.saturating_mul(SECTOR_BYTES));
        // Written over: the whole disc is free.
        // SAFETY: a plain COM call on the live writer.
        let total = unsafe { format.TotalSectorsOnMedia() }
            .ok()
            .and_then(|sectors| u64::try_from(sectors).ok())
            .map_or(0, |sectors| sectors.saturating_mul(SECTOR_BYTES));
        let room = if blank { free } else { free.max(total) };
        if room < size {
            return Err(BurnError::TooLarge {
                needed: size,
                free: room,
            });
        }
        // M9.42: the image's layer break, when it has one past which it goes on, given to a dual-layer disc first.
        let image_sectors = size / SECTOR_BYTES;
        let layer_break = request
            .layer_break
            .filter(|sectors| *sectors > 0 && u64::from(*sectors) < image_sectors);
        if let Some(wanted) = layer_break
            && physical.dual_layer()
        {
            place_layer_break(&recorder, wanted)?;
        }
        // SAFETY: plain COM calls on the live writer with valid arguments.
        unsafe {
            format
                .SetForceMediaToBeClosed(variant_bool(true))
                .map_err(|failure| error(&failure))?;
            if let Some(speed) = request.speed.and_then(|speed| i32::try_from(speed).ok()) {
                format
                    .SetWriteSpeed(speed, variant_bool(false))
                    .map_err(|failure| error(&failure))?;
            }
        }
        progress(BurnStep::Writing, 0.0);
        let stream: IStream = ImageStream {
            file: Mutex::new(file),
            size,
            position: AtomicU64::new(0),
            read: AtomicU64::new(0),
            progress: Arc::clone(progress),
            cancel: Arc::clone(cancel),
        }
        .into();
        // SAFETY: a plain COM call on the live writer; it reads the stream to its end, on threads of its own (the
        // stream is free-threaded), and returns when the disc is closed.
        let written = unsafe { format.Write(&stream) };
        if cancel.load(Ordering::Relaxed) {
            return Err(BurnError::Cancelled);
        }
        written.map_err(|failure| match error(&failure) {
            BurnError::System { .. } if failure.code() == STREAM_TOO_LARGE => BurnError::TooLarge {
                needed: size,
                free: room,
            },
            other => other,
        })?;
        progress(BurnStep::Closing, 1.0);
        // M9.42: where the written disc's layers meet; it is not verified further when that is not the image's break.
        if let Some(wanted) = layer_break
            && physical.dual_layer()
            && let Some(written) = written_layer0(&recorder)
            && written != wanted
        {
            if request.eject {
                // SAFETY: a plain COM call on the live recorder.
                let _ = unsafe { recorder.EjectMedia() };
            }
            return Err(BurnError::LayerBreakMoved { wanted, written });
        }
        // M9.38: read back before the disc comes out (a tray the system cannot close would need a hand to put it back).
        let verified = if request.verify {
            let report = Arc::clone(progress);
            verify_disc(
                &recorder,
                &request.image,
                &move |fraction| report(BurnStep::Verifying, fraction),
                cancel,
            )
        } else {
            Ok(())
        };
        if request.eject {
            // SAFETY: a plain COM call on the live recorder; a tray that does not open is no failure of the writing.
            let _ = unsafe { recorder.EjectMedia() };
        }
        verified
    }

    pub(super) fn verify(
        id: &str,
        image: &Path,
        progress: Arc<dyn Fn(f32) + Send + Sync>,
        cancel: Arc<AtomicBool>,
    ) -> Result<(), BurnError> {
        let (id, image) = (id.to_owned(), image.to_path_buf());
        on_com_thread(move || {
            let recorder = open(&id)?;
            verify_disc(&recorder, &image, &|fraction| progress(fraction), &cancel)
        })
    }

    /// Compares the disc in `recorder` with `image` (M9.38), waiting up to [`VERIFY_WAIT_SECONDS`] for a disc just
    /// closed to be readable.
    fn verify_disc(
        recorder: &IDiscRecorder2,
        image: &Path,
        progress: &dyn Fn(f32),
        cancel: &AtomicBool,
    ) -> Result<(), BurnError> {
        let mut file =
            File::open(image).map_err(|failure| BurnError::Image(failure.to_string()))?;
        let size = file
            .metadata()
            .map_err(|failure| BurnError::Image(failure.to_string()))?
            .len();
        progress(0.0);
        let mut waited = 0;
        let mut disc = loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(BurnError::Cancelled);
            }
            match open_disc(recorder) {
                Ok(disc) => break disc,
                Err(failure) if waited >= VERIFY_WAIT_SECONDS => return Err(failure),
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    waited += 1;
                }
            }
        };
        file.seek(SeekFrom::Start(0))
            .map_err(|failure| BurnError::Image(failure.to_string()))?;
        compare_with_image(&mut file, &mut disc, size, progress, cancel)
    }

    /// The disc in `recorder` opened to read its sectors from the start: its first drive letter's volume, unbuffered,
    /// readable past the end its file system gives, and with its first sector read (a disc not yet ready fails).
    fn open_disc(recorder: &IDiscRecorder2) -> Result<File, BurnError> {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::IO::DeviceIoControl;
        use windows::Win32::System::Ioctl::FSCTL_ALLOW_EXTENDED_DASD_IO;
        // SAFETY: a plain COM call on the live recorder; the SAFEARRAY is handed to `variants`, which owns it.
        let paths = unsafe { recorder.VolumePathNames() }
            .map(|array| variants(array, variant_text))
            .unwrap_or_default();
        let letter = paths
            .iter()
            .map(|path| path.trim_end_matches('\\'))
            .find(|path| path.len() == 2 && path.ends_with(':'))
            .ok_or_else(|| BurnError::VerifyRead("the disc has no drive letter".to_owned()))?;
        let volume = format!(r"\\.\{letter}");
        let open = |volume: &str| {
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(NO_BUFFERING)
                .open(volume)
                .map_err(|failure| BurnError::VerifyRead(format!("{letter}: {failure}")))
        };
        let disc = open(&volume)?;
        let mut returned = 0u32;
        // SAFETY: the handle is the open volume's, alive for the call; the control takes no buffers. A volume that
        // refuses it is still read, up to its file system's end.
        let _ = unsafe {
            DeviceIoControl(
                HANDLE(disc.as_raw_handle()),
                FSCTL_ALLOW_EXTENDED_DASD_IO,
                None,
                0,
                None,
                0,
                Some(&raw mut returned),
                None,
            )
        };
        let mut storage = Vec::new();
        let first = super::aligned(&mut storage, super::VERIFY_ALIGN);
        (&disc)
            .read_exact(first)
            .map_err(|failure| BurnError::VerifyRead(format!("{letter}: {failure}")))?;
        (&disc)
            .seek(SeekFrom::Start(0))
            .map_err(|failure| BurnError::VerifyRead(format!("{letter}: {failure}")))?;
        Ok(disc)
    }

    /// Quickly erases the rewritable disc in `recorder`.
    fn erase(recorder: &IDiscRecorder2) -> Result<(), BurnError> {
        let eraser: IDiscFormat2Erase = create(&MsftDiscFormat2Erase)?;
        // SAFETY: plain COM calls on live objects with valid arguments; EraseMedia returns when the disc is blank.
        unsafe {
            eraser
                .SetRecorder(recorder)
                .map_err(|failure| error(&failure))?;
            eraser
                .SetClientName(&BSTR::from(CLIENT))
                .map_err(|failure| error(&failure))?;
            eraser
                .SetFullErase(variant_bool(false))
                .map_err(|failure| error(&failure))?;
            eraser.EraseMedia().map_err(|failure| error(&failure))
        }
    }

    /// The image file as the `IStream` IMAPI2 reads the disc's sectors from: it tells how far it has been read and
    /// fails its reads once the writing is to stop.
    #[implement(IStream)]
    struct ImageStream {
        file: Mutex<File>,
        size: u64,
        position: AtomicU64,
        read: AtomicU64,
        progress: Arc<dyn Fn(BurnStep, f32) + Send + Sync>,
        cancel: Arc<AtomicBool>,
    }

    impl ISequentialStream_Impl for ImageStream_Impl {
        fn Read(&self, pv: *mut core::ffi::c_void, cb: u32, pcbread: *mut u32) -> HRESULT {
            if pv.is_null() {
                return E_POINTER;
            }
            if self.cancel.load(Ordering::Relaxed) {
                return E_ABORT;
            }
            let Ok(mut file) = self.file.lock() else {
                return E_ABORT;
            };
            let position = self.position.load(Ordering::Relaxed);
            let wanted = u64::from(cb).min(self.size.saturating_sub(position));
            let Ok(wanted) = usize::try_from(wanted) else {
                return E_ABORT;
            };
            // SAFETY: IMAPI2 gives a buffer of `cb` bytes at `pv` for the call; `wanted` is no more than `cb`.
            let buffer = unsafe { core::slice::from_raw_parts_mut(pv.cast::<u8>(), wanted) };
            let mut done = 0;
            if file.seek(SeekFrom::Start(position)).is_err() {
                return E_ABORT;
            }
            while done < wanted {
                match file.read(buffer.get_mut(done..).unwrap_or_default()) {
                    Ok(0) => break,
                    Ok(count) => done += count,
                    Err(_) => return E_ABORT,
                }
            }
            let count = u64::try_from(done).unwrap_or(0);
            self.position
                .store(position.saturating_add(count), Ordering::Relaxed);
            let total = self
                .read
                .fetch_add(count, Ordering::Relaxed)
                .saturating_add(count);
            // Up to 99 % while reading: the drive writes what is read a little later, and then closes the disc.
            let fraction = (total as f64 / self.size.max(1) as f64).min(0.99);
            (self.progress)(BurnStep::Writing, fraction as f32);
            if !pcbread.is_null() {
                // SAFETY: IMAPI2 passes a valid out-pointer for the count when it wants it.
                unsafe { *pcbread = u32::try_from(done).unwrap_or(0) };
            }
            if u64::try_from(done).unwrap_or(0) < u64::from(cb) {
                S_FALSE
            } else {
                S_OK
            }
        }

        fn Write(&self, _pv: *const core::ffi::c_void, _cb: u32, _pcbwritten: *mut u32) -> HRESULT {
            E_NOTIMPL
        }
    }

    impl IStream_Impl for ImageStream_Impl {
        fn Seek(
            &self,
            dlibmove: i64,
            dworigin: STREAM_SEEK,
            plibnewposition: *mut u64,
        ) -> windows::core::Result<()> {
            let current = self.position.load(Ordering::Relaxed);
            let base = match dworigin {
                STREAM_SEEK_SET => 0_i128,
                STREAM_SEEK_CUR => i128::from(current),
                STREAM_SEEK_END => i128::from(self.size),
                _ => return Err(E_NOTIMPL.into()),
            };
            let target = base.saturating_add(i128::from(dlibmove));
            let target = u64::try_from(target.clamp(0, i128::from(self.size))).unwrap_or(0);
            self.position.store(target, Ordering::Relaxed);
            if !plibnewposition.is_null() {
                // SAFETY: the caller passes a valid out-pointer for the new position when it wants it.
                unsafe { *plibnewposition = target };
            }
            Ok(())
        }

        fn SetSize(&self, _libnewsize: u64) -> windows::core::Result<()> {
            Err(E_NOTIMPL.into())
        }

        fn CopyTo(
            &self,
            _pstm: windows::core::Ref<IStream>,
            _cb: u64,
            _pcbread: *mut u64,
            _pcbwritten: *mut u64,
        ) -> windows::core::Result<()> {
            Err(E_NOTIMPL.into())
        }

        fn Commit(&self, _grfcommitflags: &STGC) -> windows::core::Result<()> {
            Ok(())
        }

        fn Revert(&self) -> windows::core::Result<()> {
            Err(E_NOTIMPL.into())
        }

        fn LockRegion(
            &self,
            _liboffset: u64,
            _cb: u64,
            _dwlocktype: &LOCKTYPE,
        ) -> windows::core::Result<()> {
            Err(E_NOTIMPL.into())
        }

        fn UnlockRegion(
            &self,
            _liboffset: u64,
            _cb: u64,
            _dwlocktype: u32,
        ) -> windows::core::Result<()> {
            Err(E_NOTIMPL.into())
        }

        fn Stat(
            &self,
            pstatstg: *mut STATSTG,
            _grfstatflag: &STATFLAG,
        ) -> windows::core::Result<()> {
            if pstatstg.is_null() {
                return Err(E_POINTER.into());
            }
            let stat = STATSTG {
                r#type: u32::try_from(STGTY_STREAM.0).unwrap_or(2),
                cbSize: self.size,
                ..STATSTG::default()
            };
            // SAFETY: the caller passes a valid STATSTG to fill; no name is given (no allocation to free).
            unsafe { *pstatstg = stat };
            Ok(())
        }

        fn Clone(&self) -> windows::core::Result<IStream> {
            Err(E_NOTIMPL.into())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{BurnStep, ImageStream};
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use std::sync::{Arc, Mutex};
        use windows::Win32::Foundation::{E_ABORT, S_FALSE, S_OK};
        use windows::Win32::System::Com::{IStream, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET};

        /// The image goes to IMAPI2 as an `IStream`: its size, its bytes in order (a short last read), seeking, how
        /// far it has been read (never 100 % before the disc is closed), and a stop that fails the next read.
        #[test]
        fn the_image_is_read_as_a_stream_that_stops_when_asked()
        -> Result<(), Box<dyn std::error::Error>> {
            let path = std::env::temp_dir()
                .join(format!("disccraft-burn-stream-{}.iso", std::process::id()));
            let bytes: Vec<u8> = (0..3 * 2048_u32)
                .map(|i| u8::try_from(i % 251).unwrap_or(0))
                .collect();
            std::fs::write(&path, &bytes)?;
            let seen: Arc<Mutex<Vec<(BurnStep, f32)>>> = Arc::new(Mutex::new(Vec::new()));
            let log = Arc::clone(&seen);
            let cancel = Arc::new(AtomicBool::new(false));
            let stream: IStream = ImageStream {
                file: Mutex::new(std::fs::File::open(&path)?),
                size: 6144,
                position: AtomicU64::new(0),
                read: AtomicU64::new(0),
                progress: Arc::new(move |step, fraction| {
                    if let Ok(mut list) = log.lock() {
                        list.push((step, fraction));
                    }
                }),
                cancel: Arc::clone(&cancel),
            }
            .into();
            let mut stat = STATSTG::default();
            // SAFETY: a valid STATSTG for the stream to fill, without a name.
            unsafe { stream.Stat(&raw mut stat, STATFLAG_NONAME) }?;
            assert_eq!(stat.cbSize, 6144);

            let mut buffer = vec![0_u8; 4096];
            let mut count = 0_u32;
            // SAFETY: the buffer holds 4096 bytes and the count is a valid out-pointer.
            let read =
                unsafe { stream.Read(buffer.as_mut_ptr().cast(), 4096, Some(&raw mut count)) };
            assert_eq!((read, count), (S_OK, 4096));
            assert_eq!(buffer.as_slice(), bytes.get(..4096).unwrap_or_default());
            // SAFETY: as above; only 2048 bytes are left.
            let read =
                unsafe { stream.Read(buffer.as_mut_ptr().cast(), 4096, Some(&raw mut count)) };
            assert_eq!((read, count), (S_FALSE, 2048));
            assert_eq!(buffer.get(..2048), bytes.get(4096..));

            // SAFETY: a plain seek with no out-pointer.
            unsafe { stream.Seek(2048, STREAM_SEEK_SET, None) }?;
            // SAFETY: as above.
            let read =
                unsafe { stream.Read(buffer.as_mut_ptr().cast(), 2048, Some(&raw mut count)) };
            assert_eq!((read, count), (S_OK, 2048));
            assert_eq!(buffer.get(..2048), bytes.get(2048..4096));
            let fractions: Vec<f32> = seen
                .lock()
                .map(|list| list.iter().map(|(_, f)| *f).collect())
                .unwrap_or_default();
            assert_eq!(fractions.len(), 3);
            assert!(
                fractions
                    .iter()
                    .all(|fraction| *fraction > 0.0 && *fraction <= 0.99)
            );

            cancel.store(true, Ordering::Relaxed);
            // SAFETY: as above.
            let read =
                unsafe { stream.Read(buffer.as_mut_ptr().cast(), 2048, Some(&raw mut count)) };
            assert_eq!(read, E_ABORT);
            drop(stream);
            std::fs::remove_file(&path)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BurnError, MediaKind, Recorder, SECTOR_BYTES, compare_with_image};

    /// M9.42: the dual-layer kinds, and what a misplaced layer break says.
    #[test]
    fn dual_layer_discs_and_their_layer_break_errors() {
        for kind in [
            MediaKind::DvdPlusRDualLayer,
            MediaKind::DvdMinusRDualLayer,
            MediaKind::DvdPlusRwDualLayer,
        ] {
            assert!(kind.dual_layer(), "{kind:?}");
        }
        assert!(!MediaKind::DvdPlusR.dual_layer());
        assert!(!MediaKind::BdR.dual_layer());
        let fixed = BurnError::LayerBreak {
            wanted: 2_030_528,
            disc: 2_084_960,
            fixed: true,
        }
        .to_string();
        assert!(
            fixed.contains("fixed at 2084960") && fixed.contains("2030528"),
            "{fixed}"
        );
        let small = BurnError::LayerBreak {
            wanted: 2_090_000,
            disc: 2_086_912,
            fixed: false,
        }
        .to_string();
        assert!(small.contains("larger"), "{small}");
        let moved = BurnError::LayerBreakMoved {
            wanted: 2_030_528,
            written: 2_086_912,
        }
        .to_string();
        assert!(
            moved.contains("2086912") && moved.contains("2030528"),
            "{moved}"
        );
    }
    use std::io::Cursor;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;

    /// An image of `sectors` sectors, each filled with its number.
    fn image(sectors: u64) -> Vec<u8> {
        (0..sectors)
            .flat_map(|sector| vec![(sector % 251) as u8; SECTOR_BYTES as usize])
            .collect()
    }

    fn compare(image: &[u8], disc: &[u8], cancel: bool) -> (Result<(), BurnError>, Vec<f32>) {
        let seen = Mutex::new(Vec::new());
        let result = compare_with_image(
            &mut Cursor::new(image),
            &mut Cursor::new(disc),
            image.len() as u64,
            &|fraction| seen.lock().unwrap().push(fraction),
            &AtomicBool::new(cancel),
        );
        (result, seen.into_inner().unwrap())
    }

    /// M9.38: a disc the same as the image passes, with the progress from 0 to 1; a disc with more on it after the
    /// image (a rewritable one written over) passes too.
    #[test]
    fn a_disc_like_its_image_verifies() {
        let written = image(1_300);
        let (result, seen) = compare(&written, &written, false);
        assert_eq!(result, Ok(()));
        assert_eq!(seen.first(), Some(&0.0));
        assert_eq!(seen.last(), Some(&1.0));
        assert!(seen.windows(2).all(|pair| pair[0] <= pair[1]));
        let mut longer = written.clone();
        longer.extend(image(40));
        assert_eq!(compare(&written, &longer, false).0, Ok(()));
    }

    /// M9.38: the first sector that differs is named, in the first chunk or a later one, and a disc that ends early
    /// (its last sector unreadable) fails.
    #[test]
    fn a_disc_unlike_its_image_fails_at_the_first_sector_that_differs() {
        let written = image(1_300);
        for sector in [0usize, 511, 512, 1_299] {
            let mut disc = written.clone();
            disc[sector * SECTOR_BYTES as usize + 7] ^= 1;
            assert_eq!(
                compare(&written, &disc, false).0,
                Err(BurnError::VerifyMismatch {
                    sector: sector as u64
                })
            );
        }
        let short = &written[..written.len() - SECTOR_BYTES as usize];
        assert!(matches!(
            compare(&written, short, false).0,
            Err(BurnError::VerifyRead(message)) if message.contains("1299")
        ));
        // A difference before the end is the one named.
        let mut both = short.to_vec();
        both[3 * SECTOR_BYTES as usize] ^= 1;
        assert_eq!(
            compare(&written, &both, false).0,
            Err(BurnError::VerifyMismatch { sector: 3 })
        );
        assert_eq!(
            compare(&written, &written, true).0,
            Err(BurnError::Cancelled)
        );
    }

    #[test]
    fn a_recorder_is_listed_with_its_drive() {
        let recorder = Recorder {
            id: "x".to_owned(),
            name: "PIONEER BD-RW BDR-XD07".to_owned(),
            drives: vec!["E:\\".to_owned()],
        };
        assert_eq!(recorder.label(), "E: PIONEER BD-RW BDR-XD07");
        let bare = Recorder {
            drives: Vec::new(),
            ..recorder
        };
        assert_eq!(bare.label(), "PIONEER BD-RW BDR-XD07");
    }

    #[test]
    fn speeds_are_told_as_multiples_of_1x() {
        assert!((MediaKind::DvdPlusR.speed_factor(5440) - 8.0).abs() < f64::EPSILON);
        assert!((MediaKind::BdR.speed_factor(13170) - 6.0).abs() < f64::EPSILON);
        assert!((MediaKind::CdR.speed_factor(3600) - 48.0).abs() < f64::EPSILON);
    }

    #[test]
    fn rewritable_kinds_are_erased_or_written_over() {
        assert!(MediaKind::DvdMinusRw.rewritable() && MediaKind::DvdMinusRw.erased_first());
        assert!(MediaKind::BdRe.rewritable() && !MediaKind::BdRe.erased_first());
        assert!(!MediaKind::DvdPlusR.rewritable());
    }

    /// M9.38: a DVD-R written and closed is reported by the drive as a DVD-ROM: a read-only DVD, not "?".
    #[test]
    fn closed_and_pressed_discs_are_read_only() {
        for kind in [MediaKind::CdRom, MediaKind::DvdRom, MediaKind::BdRom] {
            assert!(kind.read_only() && !kind.rewritable());
        }
        assert_eq!(MediaKind::DvdRom.name(), "DVD-ROM");
        assert!(MediaKind::DvdRom.is_dvd() && MediaKind::BdRom.is_blu_ray());
        assert!(!MediaKind::DvdMinusR.read_only() && !MediaKind::Other.read_only());
    }

    /// The recorders are listed without failing on any computer (none on one without a recorder; a virtual drive
    /// that only reads is no recorder).
    #[test]
    fn the_recorders_are_listed() {
        let listed = super::recorders();
        assert!(listed.is_ok(), "{listed:?}");
    }
}
