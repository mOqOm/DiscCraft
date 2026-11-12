//! Menu VOB and VMGM menu table construction.

use std::path::Path;

use disccraft_ifo::{
    AudioAttr, AudioCoding, BcdTime, Cell, CellAddress, MenuPgc, Pgc, Standard, VmCommand, VmgMenus,
};
use disccraft_menu::{ButtonGeometry, RenderedMenu, render_menu, render_menu_over};
use disccraft_mpeg2venc::{
    Standard as StillStandard, StillOptions, encode_still,
    encoder::{Encoder, EncoderConfig},
    rgb_to_frame420,
};
use disccraft_nav::{CompiledNav, MenuNav};
use disccraft_project::{MAX_MENU_AUDIO_SECONDS, MAX_MENU_SECONDS, Menu, MenuBackground, Project};
use disccraft_psmux::es::{LpcmStream, parse_video, parse_wav};
use disccraft_psmux::mux::hli::{Hli, HliButton};
use disccraft_psmux::mux::menu::{MenuVobInput, mux_menu_vob};
use disccraft_psmux::mux::{
    AudioSource, MuxExtra, MuxInput, SubpictureUnit, VobsLayout, mux_vobs_with,
};
use disccraft_psmux::sink::MemorySink;
use disccraft_spu::{Bitmap, SpuParams, encode_spu};
use disccraft_transcode::{Standard as TranscodeStandard, for_each_frame};

use crate::{DvdError, output, vm_commands};

const START_DTS: i64 = 8_100_000;
const QUANTISER_SCALE_CODE: u8 = 4;
const MAX_STILL_BYTES: usize = 224 * 1024;
const NTSC_FPS_NUM: f64 = 30000.0;
const NTSC_FPS_DEN: f64 = 1001.0;
const PAL_FPS: f64 = 25.0;

/// Built menu PGC, cell address, and VOBU starts.
type MenuBuild = (MenuPgc, CellAddress, Vec<u32>);

/// Build VMGM menu tables and the optional `VIDEO_TS.VOB`.
pub(crate) fn build_menus(
    project: &Project,
    nav: &CompiledNav,
    video_ts: &Path,
    standard: Standard,
    cache_dir: &Path,
) -> Result<(VmgMenus, u64), DvdError> {
    if project.menus.len() != nav.menus.len() {
        return Err(invalid("menu count mismatch"));
    }

    let mut sink = MemorySink::default();
    let mut pgcs = Vec::new();
    let mut cells = Vec::new();
    let mut vobu_starts = Vec::new();

    for (index, (menu, nav_menu)) in project.menus.values().zip(nav.menus.iter()).enumerate() {
        let vob_id = menu_vob_id(index)?;
        let context = MenuContext {
            project,
            standard,
            cache_dir,
        };
        let (pgc, address, starts) = build_menu_vob(&context, menu, nav_menu, vob_id, &mut sink)?;

        pgcs.push(pgc);
        cells.push(address);
        vobu_starts.extend(starts);
    }

    for hidden in &nav.hidden {
        let pgc = Pgc {
            prohibited_ops: 0,
            pre: vm_commands(&hidden.pre),
            ..Pgc::default()
        };
        pgcs.push(MenuPgc {
            title_menu: false,
            pgc,
        });
    }

    let vobs_sectors =
        u32::try_from(sink.sectors.len()).map_err(|_| invalid("too many menu sectors"))?;
    let bytes = write_menu_vob(video_ts, &sink)?;

    // Menus with sound share the one VMGM audio stream: 48 kHz 16-bit stereo LPCM.
    let audio = project
        .menus
        .values()
        .any(|menu| menu.audio.is_some())
        .then_some(AudioAttr {
            coding: AudioCoding::Lpcm,
            channels: 2,
            language: None,
        });
    let menus = VmgMenus {
        language: *b"en",
        pgcs,
        cells,
        vobu_starts,
        vobs_sectors,
        audio,
    };

    Ok((menus, bytes))
}

/// What every menu of a build shares.
struct MenuContext<'a> {
    project: &'a Project,
    standard: Standard,
    /// Where menu audio is converted to WAV.
    cache_dir: &'a Path,
}

/// Build one menu PGC, cell address, and VOBU starts. A video background or menu audio makes a moving menu (a
/// still picture with audio loops like a motion menu).
fn build_menu_vob(
    context: &MenuContext<'_>,
    menu: &Menu,
    nav_menu: &MenuNav,
    vob_id: u16,
    sink: &mut MemorySink,
) -> Result<MenuBuild, DvdError> {
    if matches!(menu.background, MenuBackground::Video { .. }) || menu.audio.is_some() {
        return build_motion_menu_vob(context, menu, nav_menu, vob_id, sink);
    }

    build_still_menu_vob(
        context.project,
        menu,
        nav_menu,
        vob_id,
        context.standard,
        sink,
    )
}

/// Build one still menu PGC, cell address, and VOBU start.
fn build_still_menu_vob(
    project: &Project,
    menu: &Menu,
    nav_menu: &MenuNav,
    vob_id: u16,
    standard: Standard,
    sink: &mut MemorySink,
) -> Result<MenuBuild, DvdError> {
    let rendered = render_menu(project, menu).map_err(|err| invalid(&err.to_string()))?;
    let frame = rgb_to_frame420(rendered.width, rendered.height, &rendered.rgb)
        .map_err(|err| invalid(&err.to_string()))?;

    let options = StillOptions {
        standard: still_standard(standard),
        quantiser_scale_code: QUANTISER_SCALE_CODE,
        max_bytes: MAX_STILL_BYTES,
        wide: project.menus_wide(),
    };

    let mut still = encode_still(&frame, &options).map_err(|err| invalid(&err.to_string()))?;
    ensure_sequence_end(&mut still);

    let video = parse_video(&still, START_DTS).map_err(|err| invalid(&err.to_string()))?;
    let spu = encode_menu_spu(&rendered)?;
    let hli = build_hli(menu, nav_menu, &rendered)?;

    let first_sector =
        u32::try_from(sink.sectors.len()).map_err(|_| invalid("too many menu sectors"))?;

    let input = MenuVobInput {
        video: &video,
        video_data: &still,
        spu: &spu,
        hli: &hli,
        vob_id,
        ntsc: standard == Standard::Ntsc,
        first_sector,
    };

    let layout = mux_menu_vob(&input, sink).map_err(|err| DvdError::Mux(err.to_string()))?;
    let duration = layout
        .e_ptm
        .checked_sub(layout.s_ptm)
        .ok_or_else(|| invalid("menu duration underflow"))?;

    let playback_time = BcdTime::from_90k(u64::from(duration), standard);

    let cell = Cell {
        vob_id,
        cell_id: 1,
        stc_discontinuity: true,
        still_time: still_time(menu, nav_menu),
        playback_time,
        first_sector: layout.first_sector,
        last_vobu_start: layout.first_sector,
        last_sector: layout.last_sector,
    };

    let pgc = Pgc {
        prohibited_ops: nav_menu.prohibited_ops,
        playback_time,
        audio_streams: 0,
        subpicture_streams: 1,
        program_entry_cells: vec![1],
        cells: vec![cell],
        pre: vm_commands(&nav_menu.pre),
        // After the still time the player runs the post commands: the end action.
        post: vm_commands(&nav_menu.post),
        cell_commands: Vec::new(),
        palette: rendered.palette,
        ..Pgc::default()
    };

    let address = CellAddress {
        vob_id,
        cell_id: 1,
        first_sector: layout.first_sector,
        last_sector: layout.last_sector,
    };

    Ok((
        MenuPgc {
            title_menu: nav_menu.title_menu,
            pgc,
        },
        address,
        vec![layout.first_sector],
    ))
}

/// The still time of a still menu's cell: its duration when it has an end action to go to, otherwise held
/// (255) until a button is pressed.
fn still_time(menu: &Menu, nav_menu: &MenuNav) -> u8 {
    match menu.duration_seconds {
        Some(seconds) if !nav_menu.post.is_empty() => {
            u8::try_from(seconds.clamp(1, MAX_MENU_SECONDS)).unwrap_or(0xFF)
        }
        _ => 0xFF,
    }
}

/// Build one motion menu PGC, cell address, and VOBU starts: the video background (or the still picture, for a
/// still menu with audio) for the loop's length, and the menu audio cut or padded to it.
fn build_motion_menu_vob(
    context: &MenuContext<'_>,
    menu: &Menu,
    nav_menu: &MenuNav,
    vob_id: u16,
    sink: &mut MemorySink,
) -> Result<MenuBuild, DvdError> {
    let (project, standard) = (context.project, context.standard);
    let source = motion_source(project, menu)?;
    let seconds = match source {
        MotionSource::Video { seconds, .. } | MotionSource::Still { seconds } => seconds,
    };
    let frames = frame_count(seconds, standard).map_err(|err| invalid_menu(menu, err))?;
    let (first, video_data) = match source {
        MotionSource::Video { path, .. } => {
            encode_motion_video(project, menu, path, frames, standard)?
        }
        MotionSource::Still { .. } => encode_still_loop(project, menu, frames, standard)?,
    };
    let audio = menu_audio(context, menu, seconds)?;

    let video = parse_video(&video_data, START_DTS).map_err(|err| mux_menu_error(menu, err))?;
    let first_pts = video
        .pictures
        .iter()
        .map(|p| p.pts)
        .min()
        .ok_or_else(|| invalid_menu(menu, "motion menu has no pictures"))?;
    let spu = encode_menu_spu(&first).map_err(|err| invalid_menu(menu, err))?;
    let hli = build_hli(menu, nav_menu, &first).map_err(|err| invalid_menu(menu, err))?;

    let base_sector = u32::try_from(sink.sectors.len())
        .map_err(|_| invalid_menu(menu, "too many menu sectors"))?;

    let input = MuxInput {
        video: &video,
        video_data: &video_data,
        audio: audio.iter().map(AudioSource::Lpcm).collect(),
        audio_start_pts: first_pts,
        cell_starts: vec![0],
        vob_id,
        ntsc: standard == Standard::Ntsc,
        subpictures: vec![vec![SubpictureUnit {
            pts: first_pts,
            data: spu,
        }]],
    };

    let extra = MuxExtra {
        highlight: Some(&hli),
        base_sector,
    };

    let layout = mux_vobs_with(&input, &extra, sink).map_err(|err| mux_menu_error(menu, err))?;
    let pgc = MotionPgc {
        vob_id,
        standard,
        palette: first.palette,
        audio: audio.is_some(),
    };
    build_motion_pgc(menu, nav_menu, &pgc, &layout)
}

/// Where a moving menu's pictures come from, and how long it loops.
#[derive(Clone, Copy)]
enum MotionSource<'a> {
    /// The first `seconds` of a video file.
    Video { path: &'a Path, seconds: f64 },
    /// The menu's still picture, held for the audio's length.
    Still { seconds: f64 },
}

fn motion_source<'a>(project: &'a Project, menu: &'a Menu) -> Result<MotionSource<'a>, DvdError> {
    if let MenuBackground::Video { .. } = menu.background {
        let (path, seconds) = video_source(project, menu)?;
        return Ok(MotionSource::Video { path, seconds });
    }
    let asset = menu_audio_path(project, menu)?;
    let length = disccraft_transcode::probe_audio_seconds(asset)
        .map_err(|err| unsupported_menu(menu, err))?;
    let seconds = length.clamp(1.0, f64::from(MAX_MENU_AUDIO_SECONDS));
    Ok(MotionSource::Still { seconds })
}

fn menu_audio_path<'a>(project: &'a Project, menu: &Menu) -> Result<&'a Path, DvdError> {
    let asset = menu
        .audio
        .and_then(|id| project.assets.get(&id))
        .ok_or_else(|| invalid_menu(menu, "missing menu audio asset"))?;
    Ok(Path::new(asset.path.as_str()))
}

/// The menu audio as 48 kHz 16-bit stereo LPCM, cut or padded with silence to `seconds`.
fn menu_audio(
    context: &MenuContext<'_>,
    menu: &Menu,
    seconds: f64,
) -> Result<Option<LpcmStream>, DvdError> {
    if menu.audio.is_none() {
        return Ok(None);
    }
    let source = menu_audio_path(context.project, menu)?;
    std::fs::create_dir_all(context.cache_dir)
        .map_err(|err| crate::io_error(context.cache_dir, err))?;
    let wav = context
        .cache_dir
        .join(format!("menu-{}-audio.wav", menu.id.0));
    disccraft_transcode::transcode_audio(source, &wav, Some(seconds))
        .map_err(|err| unsupported_menu(menu, err))?;
    let data = std::fs::read(&wav).map_err(|err| crate::io_error(&wav, err))?;
    let stream = parse_wav(&data).map_err(|err| invalid_menu(menu, err))?;
    Ok(Some(stream))
}

/// Encode the menu's still picture (background and button text) `frames` times.
fn encode_still_loop(
    project: &Project,
    menu: &Menu,
    frames: u64,
    standard: Standard,
) -> Result<(RenderedMenu, Vec<u8>), DvdError> {
    let rendered = render_menu(project, menu).map_err(|err| invalid_menu(menu, err))?;
    let frame = rgb_to_frame420(rendered.width, rendered.height, &rendered.rgb)
        .map_err(|err| invalid_menu(menu, err))?;
    let mut encoder = motion_encoder(standard, menu, project.menus_wide())?;
    let mut video_data = Vec::new();
    for _ in 0..frames {
        let au = encoder
            .encode_frame(&frame)
            .map_err(|err| invalid_menu(menu, err))?;
        video_data.extend_from_slice(&au);
    }
    video_data.extend_from_slice(&encoder.finish());
    ensure_sequence_end(&mut video_data);
    Ok((rendered, video_data))
}

/// Resolve the video asset path and duration for a motion menu.
fn video_source<'a>(project: &'a Project, menu: &'a Menu) -> Result<(&'a Path, f64), DvdError> {
    match &menu.background {
        MenuBackground::Video { asset, seconds, .. } => {
            let asset_data = project
                .assets
                .get(asset)
                .ok_or_else(|| invalid_menu(menu, "missing video asset"))?;
            Ok((Path::new(asset_data.path.as_str()), f64::from(*seconds)))
        }
        _ => Err(invalid_menu(menu, "menu is not a motion menu")),
    }
}

/// Convert a duration in seconds to a frame count for the disc standard.
fn frame_count(seconds: f64, standard: Standard) -> Result<u64, DvdError> {
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(invalid("motion menu duration invalid"));
    }

    let rounded = match standard {
        Standard::Ntsc => (seconds * NTSC_FPS_NUM / NTSC_FPS_DEN).round(),
        Standard::Pal => (seconds * PAL_FPS).round(),
    };

    if !rounded.is_finite() {
        return Err(invalid("motion menu frame count out of range"));
    }

    if !(0.0..=100_000.0).contains(&rounded) {
        return Err(invalid("motion menu frame count out of range"));
    }
    let frames = rounded as u64;
    if frames == 0 {
        return Err(invalid("motion menu has no frames"));
    }

    Ok(frames)
}

/// Encode motion menu frames into an MPEG-2 video elementary stream.
fn encode_motion_video(
    project: &Project,
    menu: &Menu,
    path: &Path,
    frames: u64,
    standard: Standard,
) -> Result<(RenderedMenu, Vec<u8>), DvdError> {
    let mut encoder = motion_encoder(standard, menu, project.menus_wide())?;
    let mut first: Option<RenderedMenu> = None;
    let mut video_data = Vec::new();

    for_each_frame(path, transcode_standard(standard), frames, &mut |_, rgb| {
        let au = encode_motion_frame(project, menu, &mut encoder, &mut first, rgb)?;
        video_data.extend_from_slice(&au);
        Ok(())
    })
    .map_err(|err| unsupported_menu(menu, err))?;

    let first = first.ok_or_else(|| invalid_menu(menu, "motion menu produced no frames"))?;
    video_data.extend_from_slice(&encoder.finish());
    ensure_sequence_end(&mut video_data);

    Ok((first, video_data))
}

/// Encode one motion menu background frame.
fn encode_motion_frame(
    project: &Project,
    menu: &Menu,
    encoder: &mut Encoder,
    first: &mut Option<RenderedMenu>,
    rgb: &[u8],
) -> Result<Vec<u8>, String> {
    let rendered = render_menu_over(project, menu, rgb).map_err(|err| err.to_string())?;
    if first.is_none() {
        *first = Some(rendered.clone());
    }

    let frame = rgb_to_frame420(rendered.width, rendered.height, &rendered.rgb)
        .map_err(|err| err.to_string())?;
    encoder.encode_frame(&frame).map_err(|err| err.to_string())
}

/// Create the motion menu encoder.
fn motion_encoder(standard: Standard, menu: &Menu, wide: bool) -> Result<Encoder, DvdError> {
    let (encoder_standard, gop_size) = match standard {
        Standard::Ntsc => (StillStandard::Ntsc, 15),
        Standard::Pal => (StillStandard::Pal, 12),
    };

    let config = EncoderConfig {
        standard: encoder_standard,
        gop_size,
        bitrate: 6_000_000,
        max_bitrate: 8_000_000,
        wide,
    };

    Encoder::new(config).map_err(|err| unsupported_menu(menu, err))
}

/// Map the IFO standard to the transcoder standard.
fn transcode_standard(standard: Standard) -> TranscodeStandard {
    match standard {
        Standard::Ntsc => TranscodeStandard::Ntsc,
        Standard::Pal => TranscodeStandard::Pal,
    }
}

/// The settings of one moving menu's PGC.
struct MotionPgc {
    vob_id: u16,
    standard: Standard,
    palette: [u32; 16],
    /// Whether the VOB carries the menu audio stream.
    audio: bool,
}

/// Build the motion menu PGC and VMGM cell data.
fn build_motion_pgc(
    menu: &Menu,
    nav_menu: &MenuNav,
    settings: &MotionPgc,
    layout: &VobsLayout,
) -> Result<MenuBuild, DvdError> {
    let MotionPgc {
        vob_id,
        standard,
        palette,
        audio,
    } = *settings;
    let cell_layout = layout
        .cells
        .first()
        .ok_or_else(|| invalid_menu(menu, "missing motion cell layout"))?;

    let vobu_starts: Vec<u32> = layout.vobus.iter().map(|vobu| vobu.start_sector).collect();
    if vobu_starts.is_empty() {
        return Err(invalid_menu(menu, "missing motion VOBU starts"));
    }

    let start = u64::from(cell_layout.start_ptm);
    let end = u64::from(cell_layout.end_ptm);
    let duration = end
        .checked_sub(start)
        .ok_or_else(|| invalid_menu(menu, "menu duration underflow"))?;
    let playback_time = BcdTime::from_90k(duration, standard);

    let cell = Cell {
        vob_id,
        cell_id: 1,
        stc_discontinuity: true,
        still_time: 0,
        playback_time,
        first_sector: cell_layout.first_sector,
        last_vobu_start: cell_layout.last_vobu_start,
        last_sector: cell_layout.last_sector,
    };

    // Without an end action the motion loops; with one, it runs after the motion has played once.
    let post = if nav_menu.post.is_empty() {
        vec![link_pgcn_command(vob_id).map_err(|err| invalid_menu(menu, err))?]
    } else {
        vm_commands(&nav_menu.post)
    };

    let pgc = Pgc {
        prohibited_ops: nav_menu.prohibited_ops,
        playback_time,
        audio_streams: u8::from(audio),
        subpicture_streams: 1,
        program_entry_cells: vec![1],
        cells: vec![cell],
        pre: vm_commands(&nav_menu.pre),
        post,
        cell_commands: Vec::new(),
        palette,
        ..Pgc::default()
    };

    let address = CellAddress {
        vob_id,
        cell_id: 1,
        first_sector: cell_layout.first_sector,
        last_sector: cell_layout.last_sector,
    };

    Ok((
        MenuPgc {
            title_menu: nav_menu.title_menu,
            pgc,
        },
        address,
        vobu_starts,
    ))
}

/// Build a VMGM link-to-PGC command.
fn link_pgcn_command(pgcn: u16) -> Result<VmCommand, DvdError> {
    let high = u8::try_from(pgcn >> 8).map_err(|_| invalid("menu PGC number out of range"))?;
    let low = u8::try_from(pgcn & 0xFF).map_err(|_| invalid("menu PGC number out of range"))?;

    Ok(VmCommand([0x20, 0x04, 0, 0, 0, 0, high, low]))
}

/// Encode the highlight layer as a forced DVD subpicture unit.
fn encode_menu_spu(rendered: &RenderedMenu) -> Result<Vec<u8>, DvdError> {
    let pixels = rendered.highlight.pixels.clone();
    let expected = usize::from(rendered.highlight.width)
        .checked_mul(usize::from(rendered.highlight.height))
        .ok_or_else(|| invalid("highlight size overflow"))?;

    if pixels.len() != expected {
        return Err(invalid("highlight pixel count mismatch"));
    }

    let bitmap = Bitmap {
        width: rendered.highlight.width,
        height: rendered.highlight.height,
        pixels,
    };

    // Outside the selected / activated button, the frame shows in the normal colour (Encore's 標準グループ;
    // contrast 0 by default, so nothing is seen).
    let params = SpuParams {
        x: 0,
        y: 0,
        colors: [0, disccraft_menu::NORMAL_PALETTE_ENTRY, 2, 3],
        alpha: [0, rendered.normal_alpha, 0, 0],
        forced: true,
        stop_delay: None,
    };

    encode_spu(&bitmap, &params).map_err(|err| invalid(&err.to_string()))
}

/// Build highlight information for one menu VOBU.
fn build_hli(menu: &Menu, nav_menu: &MenuNav, rendered: &RenderedMenu) -> Result<Hli, DvdError> {
    if nav_menu.buttons.len() != rendered.buttons.len() {
        return Err(invalid("button command count mismatch"));
    }

    let buttons = nav_menu
        .buttons
        .iter()
        .zip(rendered.buttons.iter())
        .enumerate()
        .map(|(index, (command, geometry))| {
            let (auto, group) = menu.buttons.get(index).map_or((false, 1), |button| {
                (button.auto_activate, button.highlight_group)
            });
            hli_button(*command, geometry, auto, group)
        })
        .collect();

    let default_button = menu
        .default_button
        .checked_add(1)
        .and_then(|b| u8::try_from(b).ok())
        .ok_or_else(|| invalid("default button out of range"))?;

    Ok(Hli {
        start_ptm: 0,
        buttons,
        default_button,
        button_offset: menu.button_offset,
        color_sets: rendered.color_sets,
    })
}

/// Convert one rendered button geometry and VM command to HLI.
fn hli_button(
    command: [u8; 8],
    geometry: &ButtonGeometry,
    auto_action: bool,
    group: u8,
) -> HliButton {
    HliButton {
        x1: geometry.x1,
        y1: geometry.y1,
        x2: geometry.x2,
        y2: geometry.y2,
        color_set: group.clamp(1, 3),
        auto_action,
        up: geometry.up,
        down: geometry.down,
        left: geometry.left,
        right: geometry.right,
        command,
    }
}

/// Map the IFO standard to the still-picture encoder standard.
fn still_standard(standard: Standard) -> StillStandard {
    match standard {
        Standard::Ntsc => StillStandard::Ntsc,
        Standard::Pal => StillStandard::Pal,
    }
}

/// Ensure the elementary stream ends with a sequence end code.
fn ensure_sequence_end(data: &mut Vec<u8>) {
    if !data.ends_with(&[0x00, 0x00, 0x01, 0xB7]) {
        data.extend_from_slice(&[0x00, 0x00, 0x01, 0xB7]);
    }
}

/// Write the accumulated menu sectors to `VIDEO_TS.VOB`.
fn write_menu_vob(video_ts: &Path, sink: &MemorySink) -> Result<u64, DvdError> {
    if sink.sectors.is_empty() {
        return Ok(0);
    }

    let mut data = Vec::new();
    for sector in &sink.sectors {
        data.extend_from_slice(sector);
    }

    output::write_file(&video_ts.join("VIDEO_TS.VOB"), &data)
}

/// Convert a menu index to its one-based VOB id.
fn menu_vob_id(index: usize) -> Result<u16, DvdError> {
    let value = index
        .checked_add(1)
        .ok_or_else(|| invalid("menu index overflow"))?;

    u16::try_from(value).map_err(|_| invalid("too many menus"))
}

/// Create an invalid-input error.
fn invalid(msg: &str) -> DvdError {
    DvdError::Invalid(msg.to_string())
}

/// Create an invalid-input error with menu context.
fn invalid_menu(menu: &Menu, message: impl std::fmt::Display) -> DvdError {
    DvdError::Invalid(format!("menu {}: {message}", menu.name))
}

/// Create an unsupported-input error with menu context.
fn unsupported_menu(menu: &Menu, message: impl std::fmt::Display) -> DvdError {
    DvdError::Unsupported(format!("menu {}: {message}", menu.name))
}

/// Create a mux error with menu context.
fn mux_menu_error(menu: &Menu, message: impl std::fmt::Display) -> DvdError {
    DvdError::Mux(format!("menu {}: {message}", menu.name))
}

#[cfg(test)]
mod tests {
    use super::{encode_menu_spu, frame_count, link_pgcn_command};
    use crate::DvdError;
    use disccraft_ifo::Standard;

    #[test]
    fn frame_count_for_standards() -> Result<(), DvdError> {
        assert_eq!(frame_count(10.0, Standard::Ntsc)?, 300);
        assert_eq!(frame_count(10.0, Standard::Pal)?, 250);
        Ok(())
    }

    #[test]
    fn the_normal_colour_is_the_subpicture_colour() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = disccraft_project::Project::default();
        let menu = project.add_menu(None)?;
        project.add_button(menu, "Play".to_owned(), None, disccraft_project::Link::Stop)?;
        let mut menu = project.menu(menu).ok_or("menu")?.clone();
        let mut rendered = disccraft_menu::render_menu(&project, &menu)?;
        let hidden = disccraft_spu::decode_spu(&encode_menu_spu(&rendered)?)?;
        assert_eq!(hidden.params.alpha, [0, 0, 0, 0]);
        let mut set = menu.color_set_or_default();
        set.normal = disccraft_project::NormalColor {
            color: [9, 9, 9],
            opacity: 60,
        };
        menu.color_set = Some(set);
        rendered = disccraft_menu::render_menu(&project, &menu)?;
        let shown = disccraft_spu::decode_spu(&encode_menu_spu(&rendered)?)?;
        assert_eq!(shown.params.colors[1], disccraft_menu::NORMAL_PALETTE_ENTRY);
        assert_eq!(shown.params.alpha, [0, 9, 0, 0]);
        Ok(())
    }

    #[test]
    fn loop_post_command_bytes() -> Result<(), DvdError> {
        let command = link_pgcn_command(3)?;
        assert_eq!(command.0, [0x20, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03]);
        Ok(())
    }
}
