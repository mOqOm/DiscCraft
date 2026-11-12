//! VMG IFO/BUP construction.

use std::path::Path;

use disccraft_ifo::{
    Pgc, Standard, TitleEntry, VideoAttr, VmCommand, VmgMenus, Vmgi, vmgi_sectors, write_vmgi,
};

use crate::{DvdError, output, region_mask};

/// Return the number of sectors occupied by the VMG set for `title_count` titles in `vts_count` title sets.
pub(crate) fn vmg_set_sectors(
    title_count: usize,
    vts_count: usize,
    menus: Option<&VmgMenus>,
) -> Result<u32, DvdError> {
    let count = u8::try_from(vts_count)
        .map_err(|_| DvdError::Invalid("too many title sets".to_string()))?;

    let probe = Vmgi {
        titles: vec![
            TitleEntry {
                chapters: 1,
                vts: 1,
                vts_ttn: 1,
                vts_start_sector: 0,
            };
            title_count
        ],
        vts_count: count,
        region_mask: 0,
        provider: String::new(),
        video: VideoAttr {
            standard: Standard::Ntsc,
            wide: false,
        },
        first_play: Pgc::default(),
        vts_attrs: vec![vec![0; 0x300]; vts_count],
        menus: menus.cloned(),
    };

    let sectors = vmgi_sectors(&probe)?;
    let sectors =
        u32::try_from(sectors).map_err(|_| DvdError::Invalid("VMGI too large".to_string()))?;

    let vobs = menus.map_or(0, |menus| menus.vobs_sectors);

    sectors
        .checked_mul(2)
        .and_then(|x| x.checked_add(vobs))
        .ok_or_else(|| DvdError::Invalid("VMG set sector overflow".to_string()))
}

/// Write VIDEO_TS.IFO and VIDEO_TS.BUP. `titles` has one entry per title (in title order) and `vts_attrs` one
/// block per title set.
pub(crate) fn write_vmg(
    video_ts: &Path,
    titles: &[TitleEntry],
    vts_attrs: &[Vec<u8>],
    video: (Standard, bool),
    first_play: (&[VmCommand], u32),
    menus: Option<VmgMenus>,
    regions: &[u8],
) -> Result<u64, DvdError> {
    let (standard, menus_wide) = video;

    let vts_count = u8::try_from(vts_attrs.len())
        .map_err(|_| DvdError::Invalid("too many VTSs".to_string()))?;

    let vmgi = Vmgi {
        titles: titles.to_vec(),
        vts_count,
        region_mask: region_mask(regions),
        provider: "DiscCraft".to_string(),
        video: VideoAttr {
            standard,
            wide: menus_wide,
        },
        first_play: Pgc {
            prohibited_ops: first_play.1,
            pre: first_play.0.to_vec(),
            ..Pgc::default()
        },
        vts_attrs: vts_attrs.to_vec(),
        menus,
    };

    let data = write_vmgi(&vmgi)?;
    let ifo_bytes = output::write_file(&video_ts.join("VIDEO_TS.IFO"), &data)?;
    let bup_bytes = output::write_file(&video_ts.join("VIDEO_TS.BUP"), &data)?;

    ifo_bytes
        .checked_add(bup_bytes)
        .ok_or_else(|| DvdError::Invalid("VMG byte count overflow".to_string()))
}
