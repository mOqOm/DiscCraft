use std::path::{Path, PathBuf};

use disccraft_ifo::{
    AudioAttr, AudioCoding, BcdTime, Cell, Pgc, SECTOR, Standard, TimeMap, VideoAttr, VtsRootMenu,
    Vtsi, vts_attr_block, write_vtsi,
};
use disccraft_nav::{ChapterPlaylistNav, TitleNav};
use disccraft_project::PlaybackOrder;
use disccraft_psmux::mux::{AudioSource, MuxInput, VobsLayout, mux_vobs};
use disccraft_psmux::sink::VobFileSink;

use crate::source::{LoadedAudio, LoadedTitle, channels_u8};
use crate::{DvdError, output, vm_commands};

/// Result of building one title set.
pub(crate) struct BuiltVts {
    /// Number of chapters / cells.
    pub(crate) chapters: usize,
    /// Number of chapters as `u16`.
    pub(crate) chapters_u16: u16,
    /// Title VOBS sector count.
    pub(crate) vobs_sectors: u32,
    /// Title duration in 90 kHz ticks.
    pub(crate) duration_90k: u32,
    /// Total sectors in the title set.
    pub(crate) title_set_sectors: u32,
    /// VTS attribute block for VMGI.
    pub(crate) vts_attr_block: Vec<u8>,
    /// Total bytes written for this title set.
    pub(crate) total_bytes: u64,
    /// The chapter playlists built into this title set, in the order they were given.
    pub(crate) extras: Vec<BuiltExtraTitle>,
}

/// One chapter playlist built as another title of a title set.
pub(crate) struct BuiltExtraTitle {
    /// Number of chapters / entries.
    pub(crate) chapters: usize,
    /// Number of chapters as `u16`.
    pub(crate) chapters_u16: u16,
    /// Title duration in 90 kHz ticks.
    pub(crate) duration_90k: u32,
}

/// Mux and write one title set. `extras` are the chapter playlists of the title, in VTS title order (their
/// PGCs reuse the cells of the master PGC).
pub(crate) fn build_title_set(
    loaded: &LoadedTitle,
    video_ts: &Path,
    vts: u8,
    standard: Standard,
    nav: &TitleNav,
    extras: &[&ChapterPlaylistNav],
) -> Result<BuiltVts, DvdError> {
    let mut sink = VobFileSink::new(video_ts, vts);
    let input = mux_input(loaded);
    let layout = mux_vobs(&input, &mut sink)?;

    let vob_bytes = sum_sizes(&sink.files())?;
    let duration = layout
        .video_end_ptm
        .checked_sub(layout.video_start_ptm)
        .ok_or_else(|| invalid("video duration underflow"))?;

    let audio_attrs = build_audio_attrs(&loaded.audio)?;
    let pgc = build_pgc(
        &layout,
        duration,
        audio_attrs.len(),
        loaded.subpictures.len(),
        standard,
        nav,
        loaded.cell_still,
    )?;
    let (tmap_unit, tmap) = build_tmap(&layout)?;
    let vobu_starts = layout.vobus.iter().map(|vobu| vobu.start_sector).collect();

    let mut pgcs = vec![pgc];
    let mut tmaps = vec![TimeMap {
        unit: tmap_unit,
        entries: tmap,
    }];
    let mut built_extras = Vec::with_capacity(extras.len());
    for (index, extra) in extras.iter().enumerate() {
        let expected_ttn = u8::try_from(index.saturating_add(2))
            .map_err(|_| invalid("too many chapter playlists"))?;
        if extra.vts_ttn != expected_ttn {
            return Err(invalid("chapter playlist title numbers are out of order"));
        }
        let master = pgcs.first().ok_or_else(|| invalid("missing master PGC"))?;
        let (pgc, duration) = build_playlist_pgc(&layout, master, extra, standard)?;
        let (unit, entries) = build_playlist_tmap(&layout, &extra.programs)?;
        built_extras.push(BuiltExtraTitle {
            chapters: extra.programs.len(),
            chapters_u16: u16::try_from(extra.programs.len())
                .map_err(|_| invalid("too many chapter playlist entries"))?,
            duration_90k: duration,
        });
        pgcs.push(pgc);
        tmaps.push(TimeMap { unit, entries });
    }

    let vtsi = Vtsi {
        video: VideoAttr {
            standard,
            wide: loaded.wide,
        },
        audio: audio_attrs,
        pgcs,
        title_vobs_sectors: layout.total_sectors,
        tmaps,
        vobu_starts,
        subpictures: loaded.subtitle_languages.clone(),
        root_menu: root_menu(nav),
    };

    let ifo = write_vtsi(&vtsi)?;
    let ifo_path = video_ts.join(format!("VTS_{vts:02}_0.IFO"));
    let bup_path = video_ts.join(format!("VTS_{vts:02}_0.BUP"));

    let ifo_bytes = output::write_file(&ifo_path, &ifo)?;
    let bup_bytes = output::write_file(&bup_path, &ifo)?;
    let attr_block = vts_attr_block(&ifo)?;

    let ifo_sectors = u32::try_from(ifo.len() / SECTOR)
        .map_err(|_| DvdError::Invalid("VTSI too large".to_string()))?;
    let title_set_sectors = ifo_sectors
        .checked_mul(2)
        .and_then(|x| x.checked_add(layout.total_sectors))
        .ok_or_else(|| invalid("title set sector overflow"))?;

    let total_bytes = vob_bytes
        .checked_add(ifo_bytes)
        .and_then(|x| x.checked_add(bup_bytes))
        .ok_or_else(|| invalid("title byte count overflow"))?;

    let chapters = layout.cells.len();
    let chapters_u16 =
        u16::try_from(chapters).map_err(|_| DvdError::Invalid("too many cells".to_string()))?;

    Ok(BuiltVts {
        chapters,
        chapters_u16,
        vobs_sectors: layout.total_sectors,
        duration_90k: duration,
        title_set_sectors,
        vts_attr_block: attr_block,
        total_bytes,
        extras: built_extras,
    })
}

/// The PGC of a chapter playlist: the master's cells in the playlist's order (a cell may repeat). Every cell that
/// does not directly follow the previous one in the VOB starts with an STC discontinuity, so the player resets
/// its clock at the jump. Returns the PGC and its playback time in 90 kHz ticks.
fn build_playlist_pgc(
    layout: &VobsLayout,
    master: &Pgc,
    nav: &ChapterPlaylistNav,
    standard: Standard,
) -> Result<(Pgc, u32), DvdError> {
    if nav.programs.is_empty() {
        return Err(invalid("chapter playlist has no entries"));
    }
    if nav.programs.len() > 99 {
        return Err(invalid("chapter playlist has too many entries"));
    }

    let mut cells = Vec::with_capacity(nav.programs.len());
    let mut duration = 0u32;
    let mut previous: Option<u8> = None;
    for &program in &nav.programs {
        let index = usize::from(program)
            .checked_sub(1)
            .ok_or_else(|| invalid("chapter playlist entry is zero"))?;
        let cell = master
            .cells
            .get(index)
            .copied()
            .ok_or_else(|| invalid("chapter playlist entry is out of range"))?;
        let span = layout
            .cells
            .get(index)
            .ok_or_else(|| invalid("chapter playlist entry is out of range"))?;
        let cell_duration = span
            .end_ptm
            .checked_sub(span.start_ptm)
            .ok_or_else(|| invalid("cell duration underflow"))?;
        duration = duration
            .checked_add(cell_duration)
            .ok_or_else(|| invalid("chapter playlist duration overflow"))?;
        let follows = previous.is_some_and(|last| last.checked_add(1) == Some(cell.cell_id));
        cells.push(Cell {
            stc_discontinuity: !follows,
            ..cell
        });
        previous = Some(cell.cell_id);
    }

    let programs = (1..=cells.len())
        .map(|i| u8::try_from(i).map_err(|_| invalid("too many programs")))
        .collect::<Result<Vec<_>, DvdError>>()?;

    let pgc = Pgc {
        prohibited_ops: nav.prohibited_ops,
        playback_time: BcdTime::from_90k(u64::from(duration), standard),
        program_entry_cells: programs,
        cells,
        pre: vm_commands(&nav.pre),
        post: vm_commands(&nav.post),
        playback_mode: playback_mode(nav.order, nav.programs.len()),
        ..master.clone()
    };
    Ok((pgc, duration))
}

/// The PGC's PG playback mode byte for an order over `count` programs: random and shuffle hold the number of
/// programs to play (minus one) in the low six bits, so at most 64 take part.
fn playback_mode(order: PlaybackOrder, count: usize) -> u8 {
    let n = u8::try_from(count.clamp(1, 64))
        .unwrap_or(64)
        .saturating_sub(1);
    match order {
        PlaybackOrder::Sequential => 0,
        PlaybackOrder::Random => 0x80 | n,
        PlaybackOrder::Shuffle => 0xC0 | n,
    }
}

/// The time map of a chapter playlist PGC: its VOBUs in play order, with the playlist's own running time.
fn build_playlist_tmap(layout: &VobsLayout, programs: &[u16]) -> Result<(u8, Vec<u32>), DvdError> {
    let mut points = Vec::new();
    let mut offset = 0u64;
    for &program in programs {
        let index = usize::from(program)
            .checked_sub(1)
            .ok_or_else(|| invalid("chapter playlist entry is zero"))?;
        let span = layout
            .cells
            .get(index)
            .ok_or_else(|| invalid("chapter playlist entry is out of range"))?;
        for vobu in layout.vobus.iter().filter(|vobu| vobu.cell == index) {
            let within = u64::from(vobu.s_ptm.saturating_sub(span.start_ptm));
            let ptm = offset
                .checked_add(within)
                .ok_or_else(|| invalid("TMAP time overflow"))?;
            points.push((vobu.start_sector, ptm));
        }
        let cell_duration = span
            .end_ptm
            .checked_sub(span.start_ptm)
            .ok_or_else(|| invalid("cell duration underflow"))?;
        offset = offset
            .checked_add(u64::from(cell_duration))
            .ok_or_else(|| invalid("TMAP time overflow"))?;
    }
    let duration = u32::try_from(offset).map_err(|_| invalid("chapter playlist is too long"))?;
    build_tmap_from(&points, 0, duration)
}

/// The title set's root menu: the commands the remote's MENU key runs during the title (the timeline's menu
/// remote). `None` when the timeline has none.
fn root_menu(nav: &TitleNav) -> Option<VtsRootMenu> {
    (!nav.menu_remote.is_empty()).then(|| VtsRootMenu {
        // Same language unit as the VMGM menus.
        language: *b"en",
        pgc: Pgc {
            prohibited_ops: 0,
            pre: vm_commands(&nav.menu_remote),
            ..Pgc::default()
        },
    })
}

/// Build VTS_TMAPTI entries from a mux layout.
pub(crate) fn build_tmap(layout: &VobsLayout) -> Result<(u8, Vec<u32>), DvdError> {
    let duration = layout
        .video_end_ptm
        .checked_sub(layout.video_start_ptm)
        .ok_or_else(|| invalid("video duration underflow"))?;
    let points: Vec<(u32, u64)> = layout
        .vobus
        .iter()
        .map(|vobu| (vobu.start_sector, u64::from(vobu.s_ptm)))
        .collect();
    build_tmap_from(&points, u64::from(layout.video_start_ptm), duration)
}

/// Build a time map from `(VOBU start sector, VOBU start time)` points in play order: one entry per time unit,
/// naming the last VOBU that starts at or before `start_ptm + (i + 1) * unit`.
fn build_tmap_from(
    points: &[(u32, u64)],
    start_ptm: u64,
    duration: u32,
) -> Result<(u8, Vec<u32>), DvdError> {
    if points.is_empty() {
        return Err(invalid("no VOBUs"));
    }

    let unit = tmap_unit(duration)?;
    let duration_s = u64::from(duration) / 90_000;
    let entries = duration_s / u64::from(unit);
    let entries_usize = usize::try_from(entries).map_err(|_| invalid("too many TMAP entries"))?;

    let mut tmap = Vec::with_capacity(entries_usize);
    for i in 1..=entries {
        let step = i
            .checked_mul(u64::from(unit))
            .and_then(|x| x.checked_mul(90_000))
            .ok_or_else(|| invalid("TMAP target overflow"))?;
        let target = start_ptm
            .checked_add(step)
            .ok_or_else(|| invalid("TMAP target overflow"))?;

        let pos = points.partition_point(|(_, ptm)| *ptm <= target);
        let idx = pos.saturating_sub(1);
        let (sector, _) = points
            .get(idx)
            .or_else(|| points.first())
            .ok_or_else(|| invalid("no VOBUs"))?;
        tmap.push(*sector);
    }

    Ok((unit, tmap))
}

/// Build mux input from loaded title data.
fn mux_input<'a>(loaded: &'a LoadedTitle) -> MuxInput<'a> {
    let audio = loaded
        .audio
        .iter()
        .map(|item| match item {
            LoadedAudio::Ac3 { stream, data, .. } => AudioSource::Ac3 {
                stream,
                data: data.as_slice(),
            },
            LoadedAudio::Lpcm { stream, .. } => AudioSource::Lpcm(stream),
        })
        .collect();

    MuxInput {
        video: &loaded.video,
        video_data: &loaded.video_data,
        audio,
        audio_start_pts: loaded.first_pts,
        cell_starts: loaded.cell_starts.clone(),
        vob_id: 1,
        ntsc: loaded.ntsc,
        subpictures: loaded.subpictures.clone(),
    }
}

/// Sum file sizes.
fn sum_sizes(paths: &[PathBuf]) -> Result<u64, DvdError> {
    let mut total = 0u64;
    for path in paths {
        total = total
            .checked_add(output::file_size(path)?)
            .ok_or_else(|| invalid("VOB byte count overflow"))?;
    }
    Ok(total)
}

/// Build audio attributes from loaded audio streams.
fn build_audio_attrs(audio: &[LoadedAudio]) -> Result<Vec<AudioAttr>, DvdError> {
    let mut out = Vec::with_capacity(audio.len());

    for item in audio {
        let (coding, channels, language) = match item {
            LoadedAudio::Ac3 {
                stream, language, ..
            } => (AudioCoding::Ac3, channels_u8(stream.channels)?, *language),
            LoadedAudio::Lpcm { stream, language } => {
                (AudioCoding::Lpcm, channels_u8(stream.channels)?, *language)
            }
        };

        out.push(AudioAttr {
            coding,
            channels,
            language,
        });
    }

    Ok(out)
}

/// Build the title PGC from mux layout and compiled navigation. `cell_still` is every cell's still time
/// (`0xFF` for a manual-advance slideshow).
#[allow(clippy::too_many_arguments)]
fn build_pgc(
    layout: &VobsLayout,
    duration: u32,
    audio_streams: usize,
    subtitle_streams: usize,
    standard: Standard,
    nav: &TitleNav,
    cell_still: u8,
) -> Result<Pgc, DvdError> {
    if layout.cells.is_empty() {
        return Err(invalid("no cells"));
    }
    if layout.cells.len() > 255 {
        return Err(invalid("too many cells"));
    }

    let cells = layout
        .cells
        .iter()
        .enumerate()
        .map(|(idx, cell)| {
            let cell_duration = cell
                .end_ptm
                .checked_sub(cell.start_ptm)
                .ok_or_else(|| invalid("cell duration underflow"))?;
            Ok(Cell {
                vob_id: 1,
                cell_id: cell.cell_id,
                stc_discontinuity: idx == 0,
                still_time: cell_still,
                playback_time: BcdTime::from_90k(u64::from(cell_duration), standard),
                first_sector: cell.first_sector,
                last_vobu_start: cell.last_vobu_start,
                last_sector: cell.last_sector,
            })
        })
        .collect::<Result<Vec<_>, DvdError>>()?;

    let programs = (1..=cells.len())
        .map(|i| u8::try_from(i).map_err(|_| invalid("too many programs")))
        .collect::<Result<Vec<_>, DvdError>>()?;

    Ok(Pgc {
        prohibited_ops: nav.prohibited_ops,
        playback_time: BcdTime::from_90k(u64::from(duration), standard),
        audio_streams: u8::try_from(audio_streams)
            .map_err(|_| DvdError::Invalid("too many audio streams".to_string()))?,
        program_entry_cells: programs,
        cells,
        pre: vm_commands(&nav.pre),
        post: vm_commands(&nav.post),
        cell_commands: Vec::new(),
        subpicture_streams: u8::try_from(subtitle_streams)
            .map_err(|_| DvdError::Invalid("too many subtitle streams".to_string()))?,
        palette: SUBTITLE_PALETTE,
        ..Pgc::default()
    })
}

/// Title CLUT: 0 black, 1 white (subtitle text), 2 black (outline), rest black. Entries are `00 Y Cr Cb`.
const SUBTITLE_PALETTE: [u32; 16] = [
    0x0010_8080,
    0x00EB_8080,
    0x0010_8080,
    0x0010_8080,
    0x0010_8080,
    0x0010_8080,
    0x0010_8080,
    0x0010_8080,
    0x0010_8080,
    0x0010_8080,
    0x0010_8080,
    0x0010_8080,
    0x0010_8080,
    0x0010_8080,
    0x0010_8080,
    0x0010_8080,
];

/// Compute the TMAP time unit.
fn tmap_unit(duration_90k: u32) -> Result<u8, DvdError> {
    let denom = 2048u64
        .checked_mul(90_000)
        .ok_or_else(|| invalid("TMAP unit overflow"))?;

    let unit = if duration_90k == 0 {
        1
    } else {
        u64::from(duration_90k)
            .checked_add(denom)
            .and_then(|x| x.checked_sub(1))
            .and_then(|x| x.checked_div(denom))
            .ok_or_else(|| invalid("TMAP unit overflow"))?
    };

    u8::try_from(unit).map_err(|_| {
        DvdError::Unsupported("title is too long for the M2 TMAP unit range".to_string())
    })
}

/// Create an invalid-input error.
fn invalid(msg: &str) -> DvdError {
    DvdError::Invalid(msg.to_string())
}

#[cfg(test)]
mod tests {
    use super::{build_playlist_pgc, build_playlist_tmap, build_tmap, playback_mode};
    use crate::DvdError;
    use disccraft_ifo::{BcdTime, Cell, Pgc, Standard};
    use disccraft_nav::ChapterPlaylistNav;
    use disccraft_project::{PlaybackOrder, TimelineId};
    use disccraft_psmux::mux::{CellLayout, VobsLayout, VobuInfo};

    fn vobu(start_sector: u32, s_ptm: u32) -> VobuInfo {
        VobuInfo {
            start_sector,
            last_sector: start_sector,
            s_ptm,
            e_ptm: s_ptm.saturating_add(90_000),
            cell: 0,
        }
    }

    /// Three one-second cells of one VOB, one VOBU each.
    fn three_cell_layout() -> (VobsLayout, Pgc) {
        let cells = (0..3u32)
            .map(|i| CellLayout {
                cell_id: u8::try_from(i.saturating_add(1)).unwrap_or(1),
                start_ptm: i.saturating_mul(90_000),
                end_ptm: i.saturating_add(1).saturating_mul(90_000),
                first_sector: i.saturating_mul(10),
                last_vobu_start: i.saturating_mul(10),
                last_sector: i.saturating_mul(10).saturating_add(9),
            })
            .collect::<Vec<_>>();
        let vobus = (0..3usize)
            .map(|i| VobuInfo {
                cell: i,
                ..vobu(
                    u32::try_from(i).unwrap_or(0).saturating_mul(10),
                    u32::try_from(i).unwrap_or(0).saturating_mul(90_000),
                )
            })
            .collect();
        let master = Pgc {
            program_entry_cells: vec![1, 2, 3],
            cells: cells
                .iter()
                .enumerate()
                .map(|(i, c)| Cell {
                    vob_id: 1,
                    cell_id: c.cell_id,
                    stc_discontinuity: i == 0,
                    playback_time: BcdTime::from_90k(90_000, Standard::Ntsc),
                    first_sector: c.first_sector,
                    last_vobu_start: c.last_vobu_start,
                    last_sector: c.last_sector,
                    still_time: 0,
                })
                .collect(),
            ..Pgc::default()
        };
        (
            VobsLayout {
                vobus,
                cells,
                total_sectors: 30,
                video_start_ptm: 0,
                video_end_ptm: 270_000,
            },
            master,
        )
    }

    #[test]
    fn playlist_pgc_reorders_cells_and_marks_jumps() -> Result<(), DvdError> {
        let (layout, master) = three_cell_layout();
        let nav = ChapterPlaylistNav {
            chapter_playlist: 7,
            timeline: TimelineId(1),
            title: 2,
            vts_ttn: 2,
            programs: vec![3, 1, 2, 3],
            order: PlaybackOrder::Shuffle,
            pre: Vec::new(),
            post: Vec::new(),
            prohibited_ops: 0,
        };
        let (pgc, duration) = build_playlist_pgc(&layout, &master, &nav, Standard::Ntsc)?;
        assert_eq!(duration, 360_000);
        let ids: Vec<u8> = pgc.cells.iter().map(|c| c.cell_id).collect();
        assert_eq!(ids, vec![3, 1, 2, 3]);
        let stc: Vec<bool> = pgc.cells.iter().map(|c| c.stc_discontinuity).collect();
        // Cells 1 → 2 → 3 follow each other in the VOB, so those steps keep the clock; the jumps reset it.
        assert_eq!(stc, vec![true, true, false, false]);
        assert_eq!(pgc.program_entry_cells, vec![1, 2, 3, 4]);
        assert_eq!(pgc.playback_mode, 0xC3);
        assert_eq!(pgc.playback_time.to_90k().ok(), Some(360_000));

        let (unit, tmap) = build_playlist_tmap(&layout, &nav.programs)?;
        assert_eq!(unit, 1);
        // Seconds 1..=4 of the playlist start in cells 1, 2, 3, 3 (the last VOBU at or before each second).
        assert_eq!(tmap, vec![0, 10, 20, 20]);
        Ok(())
    }

    #[test]
    fn playback_mode_bytes() {
        assert_eq!(playback_mode(PlaybackOrder::Sequential, 5), 0);
        assert_eq!(playback_mode(PlaybackOrder::Random, 1), 0x80);
        assert_eq!(playback_mode(PlaybackOrder::Shuffle, 5), 0xC4);
        assert_eq!(playback_mode(PlaybackOrder::Random, 200), 0xBF);
    }

    #[test]
    fn tmap_selects_last_vobu_at_or_before() -> Result<(), DvdError> {
        let layout = VobsLayout {
            vobus: vec![vobu(0, 0), vobu(3, 100_000), vobu(7, 200_000)],
            cells: Vec::new(),
            total_sectors: 10,
            video_start_ptm: 0,
            video_end_ptm: 300_000,
        };

        let (unit, tmap) = build_tmap(&layout)?;
        assert_eq!(unit, 1);
        assert_eq!(tmap, vec![0, 3, 7]);
        Ok(())
    }
}
