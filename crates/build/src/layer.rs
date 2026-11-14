//! The layer break of a dual-layer DVD image (DVD-9): chosen by the build (自動) or set in the project (手動),
//! checked against the image, and read back from it (the title, cell and time of the video at the break).

use crate::{LAYER0_MAX_SECTORS, SINGLE_LAYER_SECTORS, choose_layer_break};
use disccraft_dvd::DvdBuildReport;
use disccraft_project::{Capacity, DiscKind, Project, VideoStandard};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Where a dual-layer DVD image goes from layer 0 to layer 1, read back from the written image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerBreak {
    /// The first sector of layer 1 in the image: the size of layer 0.
    pub sector: u32,
    /// The image's size in sectors.
    pub total_sectors: u32,
    /// Whether the project set it (手動) rather than the build choosing it (自動).
    pub manual: bool,
    /// The title and cell the image's video starts at the break, read back from the image; `None` when the sector
    /// is no title cell's start (a build never chooses such a sector).
    pub place: Option<LayerBreakPlace>,
    /// For a manual break: the time asked for, at the frame nearest to it (90 kHz from the title's start).
    pub requested_90k: Option<u64>,
}

/// The title and cell at a layer break.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerBreakPlace {
    /// The project's id of the title's timeline (a slideshow or chapter playlist id for those titles).
    pub timeline: u64,
    /// The title number (1-based).
    pub title: u8,
    /// The title set (1-based).
    pub vts: u8,
    /// The cell of the title's program chain (1-based) that starts at the break.
    pub cell: u8,
    /// The time from the title's start of the video at the break (90 kHz), from the navigation packs in the image.
    pub time_90k: u64,
}

impl LayerBreak {
    /// The size of layer 1 in sectors.
    #[must_use]
    pub fn layer1_sectors(&self) -> u32 {
        self.total_sectors.saturating_sub(self.sector)
    }

    /// For a manual break: whether the image breaks at the frame asked for.
    #[must_use]
    pub fn as_requested(&self) -> Option<bool> {
        let requested = self.requested_90k?;
        Some(
            self.place
                .as_ref()
                .is_some_and(|place| place.time_90k == requested),
        )
    }
}

/// 90 kHz ticks of one frame at the disc's rate.
#[must_use]
pub fn frame_90k(standard: VideoStandard) -> u64 {
    match standard {
        VideoStandard::Ntsc => 3_003,
        VideoStandard::Pal => 3_600,
    }
}

/// Where the image breaks: the project's manual break when it sets one for a dual-layer disc (checked: layer 0
/// at least as large as layer 1 and no larger than a layer holds), else the build's choice when the image needs two
/// layers, else none. Writes `<name>.layerbreak.txt` next to the image for a break.
pub(crate) fn layer_break(
    project: &Project,
    dvd: &DvdBuildReport,
    iso: &Path,
    total_sectors: u32,
) -> Result<Option<LayerBreak>, String> {
    let video_ts = dvd.video_ts.as_path();
    let manual = manual_target(project, dvd)?;
    let sector = match manual {
        Some((vts, cell, requested)) => {
            let sector = cell_sector(iso, video_ts, vts, cell)?;
            check_manual(project, sector, total_sectors, requested)?;
            sector
        }
        None if total_sectors > SINGLE_LAYER_SECTORS => {
            let starts = iso_cell_starts(iso, video_ts)?;
            choose_layer_break(total_sectors, &starts).ok_or_else(|| {
                "the image needs a dual-layer disc but no cell boundary fits the layer break (add chapters near the middle)".to_string()
            })?
        }
        None => return Ok(None),
    };
    let found = LayerBreak {
        sector,
        total_sectors,
        manual: manual.is_some(),
        place: describe(iso, video_ts, sector, dvd)?,
        requested_90k: manual.map(|(_, _, requested)| requested),
    };
    let note = iso.with_extension("layerbreak.txt");
    std::fs::write(&note, note_text(&found, project.settings.standard))
        .map_err(|e| format!("{}: {e}", note.display()))?;
    Ok(Some(found))
}

/// The manual break's title set, cell and the time asked for (90 kHz, at its frame), when the project sets one
/// for a dual-layer DVD.
fn manual_target(project: &Project, dvd: &DvdBuildReport) -> Result<Option<(u8, u8, u64)>, String> {
    let Some(point) = project.settings.layer_break else {
        return Ok(None);
    };
    if project.settings.kind != DiscKind::Dvd || project.settings.capacity != Capacity::Dvd9 {
        return Ok(None);
    }
    let (_, requested) = disccraft_dvd::frame_of(point.time.0, project.settings.standard);
    dvd.titles
        .iter()
        .find_map(|title| Some((title.vts, title.layer_break_cell?, requested)))
        .map(Some)
        .ok_or_else(|| {
            format!(
                "the layer break's timeline {} is not a title of the disc",
                point.timeline.0
            )
        })
}

/// A manual break must leave layer 0 at least as large as layer 1 and fit in layer 0.
fn check_manual(
    project: &Project,
    sector: u32,
    total_sectors: u32,
    requested_90k: u64,
) -> Result<(), String> {
    let at = time_text(requested_90k, project.settings.standard);
    let half = total_sectors.div_ceil(2);
    if sector < half {
        return Err(format!(
            "the layer break at {at} falls at sector {sector}, before the middle of the image ({half} of {total_sectors} sectors): layer 0 must be at least as large as layer 1, so move it later"
        ));
    }
    if sector > LAYER0_MAX_SECTORS {
        return Err(format!(
            "the layer break at {at} falls at sector {sector}, past the end of layer 0 ({LAYER0_MAX_SECTORS} sectors), so move it earlier"
        ));
    }
    Ok(())
}

/// The image sector where cell `cell` (1-based) of title set `vts`'s title program chain starts.
fn cell_sector(iso: &Path, video_ts: &Path, vts: u8, cell: u8) -> Result<u32, String> {
    let listing = disccraft_udf::read::list_udf(iso).map_err(|e| format!("ISO: {e}"))?;
    let vob = vob_sector(&listing, vts)?;
    let vtsi = read_vtsi(video_ts, vts)?;
    let index = usize::from(cell).saturating_sub(1);
    let first = vtsi
        .pgcs
        .first()
        .and_then(|pgc| pgc.cells.get(index))
        .map(|cell| cell.first_sector)
        .ok_or_else(|| format!("title set {vts} has no cell {cell}"))?;
    Ok(vob.saturating_add(first))
}

/// The title, cell and time of the video at image sector `sector`, when a title's cell starts there.
fn describe(
    iso: &Path,
    video_ts: &Path,
    sector: u32,
    dvd: &DvdBuildReport,
) -> Result<Option<LayerBreakPlace>, String> {
    let listing = disccraft_udf::read::list_udf(iso).map_err(|e| format!("ISO: {e}"))?;
    for title in &dvd.titles {
        let Ok(vob) = vob_sector(&listing, title.vts) else {
            continue;
        };
        let vtsi = read_vtsi(video_ts, title.vts)?;
        let Some(pgc) = vtsi.pgcs.first() else {
            continue;
        };
        let Some(index) = pgc
            .cells
            .iter()
            .position(|cell| vob.saturating_add(cell.first_sector) == sector)
        else {
            continue;
        };
        let first = pgc
            .cells
            .first()
            .map_or(vob, |cell| vob.saturating_add(cell.first_sector));
        let start = nav_start_ptm(iso, first)?;
        let at = nav_start_ptm(iso, sector)?;
        return Ok(Some(LayerBreakPlace {
            timeline: title.timeline,
            title: title.title,
            vts: title.vts,
            cell: u8::try_from(index.saturating_add(1)).unwrap_or(u8::MAX),
            time_90k: u64::from(at.wrapping_sub(start)),
        }));
    }
    Ok(None)
}

/// The image sector of `VIDEO_TS/VTS_xx_1.VOB`.
fn vob_sector(listing: &[disccraft_udf::read::ListedFile], vts: u8) -> Result<u32, String> {
    let vob = format!("VIDEO_TS/VTS_{vts:02}_1.VOB");
    listing
        .iter()
        .find(|file| file.path == vob)
        .map(|file| file.sector)
        .ok_or_else(|| format!("the image has no {vob}"))
}

fn read_vtsi(video_ts: &Path, vts: u8) -> Result<disccraft_ifo::Vtsi, String> {
    let ifo = video_ts.join(format!("VTS_{vts:02}_0.IFO"));
    let bytes = std::fs::read(&ifo).map_err(|e| format!("{}: {e}", ifo.display()))?;
    disccraft_ifo::read_vtsi(&bytes).map_err(|e| format!("{}: {e}", ifo.display()))
}

/// The start time (VOBU_S_PTM, 90 kHz) of the VOBU whose navigation pack is image sector `sector`: the pack's
/// presentation control information (PCI) gives it. An error when the sector is no navigation pack.
fn nav_start_ptm(iso: &Path, sector: u32) -> Result<u32, String> {
    const SECTOR: u64 = 2048;
    // A pack header (14 bytes) and a system header (24) come before the PCI packet (stream 0xBF, substream 0x00);
    // the PCI's general information starts after its 7 header bytes, and VOBU_S_PTM is 12 bytes into it.
    const PCI_PACKET: usize = 38;
    const S_PTM: usize = PCI_PACKET + 7 + 12;
    let mut pack = [0u8; 2048];
    let mut file = std::fs::File::open(iso).map_err(|e| format!("{}: {e}", iso.display()))?;
    file.seek(SeekFrom::Start(u64::from(sector) * SECTOR))
        .and_then(|_| file.read_exact(&mut pack))
        .map_err(|e| format!("{}: sector {sector}: {e}", iso.display()))?;
    let is_nav = pack.get(..4) == Some(&[0, 0, 1, 0xBA])
        && pack.get(PCI_PACKET..PCI_PACKET + 4) == Some(&[0, 0, 1, 0xBF])
        && pack.get(PCI_PACKET + 6) == Some(&0);
    let ptm = pack
        .get(S_PTM..S_PTM + 4)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .filter(|_| is_nav)
        .ok_or_else(|| format!("sector {sector} of the image is not a navigation pack"))?;
    Ok(u32::from_be_bytes(ptm))
}

/// Absolute cell start sectors of every title VOBS in a written ISO (from the VTS IFOs and the UDF file listing).
fn iso_cell_starts(iso: &Path, video_ts: &Path) -> Result<Vec<u32>, String> {
    let listing = disccraft_udf::read::list_udf(iso).map_err(|e| format!("ISO: {e}"))?;
    let mut starts = Vec::new();
    for vts in 1..=99u8 {
        if !video_ts.join(format!("VTS_{vts:02}_0.IFO")).is_file() {
            break;
        }
        let vtsi = read_vtsi(video_ts, vts)?;
        let Ok(first) = vob_sector(&listing, vts) else {
            continue;
        };
        starts.extend(
            vtsi.address_cells()
                .iter()
                .map(|c| first.saturating_add(c.first_sector)),
        );
    }
    Ok(starts)
}

/// A time (90 kHz) as frames and milliseconds: `frame 45 (1501.50 ms)`.
#[must_use]
pub fn time_text(time_90k: u64, standard: VideoStandard) -> String {
    let frame = time_90k.saturating_add(frame_90k(standard) / 2) / frame_90k(standard);
    format!("frame {frame} ({:.2} ms)", millis(time_90k))
}

/// Milliseconds of a 90 kHz time.
#[must_use]
pub fn millis(time_90k: u64) -> f64 {
    // `as` is exact for times below 2^53 ticks (more than 3000 years).
    time_90k as f64 / 90.0
}

/// `<name>.layerbreak.txt`: the break sector first (as before), then the layers and where the break is.
fn note_text(found: &LayerBreak, standard: VideoStandard) -> String {
    let mut text = format!(
        "LayerBreak={}\nTotalSectors={}\nLayer0Sectors={}\nLayer1Sectors={}\nMode={}\n",
        found.sector,
        found.total_sectors,
        found.sector,
        found.layer1_sectors(),
        if found.manual { "Manual" } else { "Auto" },
    );
    if let Some(place) = &found.place {
        let frame_ticks = frame_90k(standard);
        text.push_str(&format!(
            "Title={}\nVTS={}\nCell={}\nFrame={}\nTimeMs={:.2}\n",
            place.title,
            place.vts,
            place.cell,
            place.time_90k.saturating_add(frame_ticks / 2) / frame_ticks,
            millis(place.time_90k),
        ));
    }
    if let Some(requested) = found.requested_90k {
        text.push_str(&format!(
            "RequestedFrame={}\nAsRequested={}\n",
            requested / frame_90k(standard),
            if found.as_requested() == Some(true) {
                "yes"
            } else {
                "no"
            },
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{LayerBreak, LayerBreakPlace, note_text, time_text};
    use disccraft_project::VideoStandard;

    #[test]
    fn the_note_and_the_check() {
        let mut found = LayerBreak {
            sector: 600,
            total_sectors: 1000,
            manual: true,
            place: Some(LayerBreakPlace {
                timeline: 5,
                title: 1,
                vts: 1,
                cell: 3,
                time_90k: 45 * 3003,
            }),
            requested_90k: Some(45 * 3003),
        };
        assert_eq!(found.layer1_sectors(), 400);
        assert_eq!(found.as_requested(), Some(true));
        let note = note_text(&found, VideoStandard::Ntsc);
        assert!(
            note.starts_with("LayerBreak=600\nTotalSectors=1000\n"),
            "{note}"
        );
        for line in [
            "Layer1Sectors=400",
            "Mode=Manual",
            "Cell=3",
            "Frame=45",
            "TimeMs=1501.50",
            "AsRequested=yes",
        ] {
            assert!(note.lines().any(|l| l == line), "{line} in {note}");
        }
        found.requested_90k = Some(46 * 3003);
        assert_eq!(found.as_requested(), Some(false));
        found.manual = false;
        found.requested_90k = None;
        assert_eq!(found.as_requested(), None);
        assert_eq!(
            time_text(25 * 3600, VideoStandard::Pal),
            "frame 25 (1000.00 ms)"
        );
    }
}
