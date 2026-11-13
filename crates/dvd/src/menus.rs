//! Menu VOB and VMGM menu table construction.

use std::path::Path;

use disccraft_ifo::{
    AudioAttr, AudioCoding, BcdTime, Cell, CellAddress, MenuPgc, Pgc, Standard, VmCommand, VmgMenus,
};
use disccraft_menu::{
    ButtonGeometry, MenuPictures, RenderedMenu, ThumbnailPicture, render_disc_menu,
};
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

use crate::thumbnails::{MovingThumbnails, pictures, still_thumbnails};
use crate::{DvdError, output, vm_commands};

const START_DTS: i64 = 8_100_000;
const QUANTISER_SCALE_CODE: u8 = 4;
const MAX_STILL_BYTES: usize = 224 * 1024;
const NTSC_FPS_NUM: f64 = 30000.0;
const NTSC_FPS_DEN: f64 = 1001.0;
const PAL_FPS: f64 = 25.0;

/// Built menu PGC, cell address, and VOBU starts.
type MenuBuild = (MenuPgc, Vec<CellAddress>, Vec<u32>);

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
        let (pgc, addresses, starts) = build_menu_vob(&context, menu, nav_menu, vob_id, &mut sink)?;

        pgcs.push(pgc);
        cells.extend(addresses);
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

/// Build one menu PGC, cell address, and VOBU starts. A video background, menu audio or motion buttons make a
/// moving menu (a still picture with audio loops like a motion menu).
fn build_menu_vob(
    context: &MenuContext<'_>,
    menu: &Menu,
    nav_menu: &MenuNav,
    vob_id: u16,
    sink: &mut MemorySink,
) -> Result<MenuBuild, DvdError> {
    if menu.is_motion() {
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
    let frames = still_thumbnails(project, menu);
    let thumbnails = pictures(&frames);
    let rendered = render_disc_menu(
        project,
        menu,
        MenuPictures {
            background: None,
            thumbnails: &thumbnails,
        },
    )
    .map_err(|err| invalid(&err.to_string()))?;
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
        vec![address],
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
    let standard = context.standard;
    let MotionMenu {
        first,
        video: video_data,
        audio,
    } = motion_parts(context, menu)?;

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
        cell_starts: motion_cell_starts(menu),
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

/// A motion menu's pictures and sound, encoded (DVD and Blu-ray menus share them).
pub struct MotionMenu {
    /// The first picture: the buttons' geometry and highlight for the whole menu.
    pub first: RenderedMenu,
    /// The MPEG-2 video elementary stream of one play, ending with a sequence end code.
    pub video: Vec<u8>,
    /// The menu audio as 48 kHz 16-bit stereo LPCM, cut or padded to the menu's length.
    pub audio: Option<LpcmStream>,
}

/// Encodes one play of a motion menu (see [`Menu::is_motion`]): its video background, or its still picture with
/// the motion buttons playing, and its audio (converted to WAV in `cache_dir`).
///
/// # Errors
///
/// Returns [`DvdError`] when a source cannot be read or the pictures cannot be encoded.
pub fn encode_motion_menu(
    project: &Project,
    menu: &Menu,
    cache_dir: &Path,
) -> Result<MotionMenu, DvdError> {
    let context = MenuContext {
        project,
        standard: crate::ifo_standard(project.settings.standard),
        cache_dir,
    };
    motion_parts(&context, menu)
}

/// One play of a motion menu for an HD Blu-ray disc: H.264 at the disc's HD size and rate.
pub struct HdMotionMenu {
    /// The first picture: the buttons' geometry and highlight for the whole menu.
    pub first: RenderedMenu,
    /// The H.264 elementary stream (Annex B).
    pub data: Vec<u8>,
    /// Its access units, time stamps in 90 kHz from 0.
    pub units: Vec<disccraft_transcode::hd::AccessUnit>,
    /// The menu audio as 48 kHz 16-bit stereo LPCM, cut or padded to the menu's length.
    pub audio: Option<LpcmStream>,
}

/// Encodes one play of a motion menu (see [`Menu::is_motion`]) as H.264 of `options`' size and rate (an HD
/// Blu-ray disc's menus): its video background (each frame read at that size), or its still picture with the motion
/// buttons playing, and its audio.
///
/// # Errors
///
/// Returns [`DvdError`] when a source cannot be read or the pictures cannot be encoded.
pub fn encode_hd_motion_menu(
    project: &Project,
    menu: &Menu,
    cache_dir: &Path,
    options: &disccraft_transcode::hd::HdOptions,
) -> Result<HdMotionMenu, DvdError> {
    let context = MenuContext {
        project,
        standard: crate::ifo_standard(project.settings.standard),
        cache_dir,
    };
    let source = motion_source(project, menu)?;
    let seconds = match source {
        MotionSource::Video { seconds, .. } | MotionSource::Still { seconds } => seconds,
    };
    let rate = (options.fps_num, options.fps_den);
    let frames = hd_frame_count(seconds, rate).map_err(|err| invalid_menu(menu, err))?;
    let mut encoder = disccraft_transcode::hd::H264Frames::new(options)
        .map_err(|err| unsupported_menu(menu, err))?;
    let moving = menu
        .plays_buttons()
        .then(|| MovingThumbnails::open(project, menu));
    let still = if moving.is_some() {
        Vec::new()
    } else {
        still_thumbnails(project, menu)
    };
    let (width, height) = disccraft_project::frame_size(&project.settings);
    let mut first: Option<RenderedMenu> = None;
    let mut frame = |index: u64, background: Option<&[u8]>| -> Result<(), String> {
        let now = moving
            .as_ref()
            .map(|moving| moving.frames_at(hd_seconds(index, rate)));
        let thumbnails = pictures(now.as_deref().unwrap_or(&still));
        let rendered = render_disc_menu(
            project,
            menu,
            MenuPictures {
                background: background.map(|rgb| (rgb, usize::from(width), usize::from(height))),
                thumbnails: &thumbnails,
            },
        )
        .map_err(|err| err.to_string())?;
        encoder
            .push_rgb(&rendered.rgb)
            .map_err(|err| err.to_string())?;
        if first.is_none() {
            first = Some(rendered);
        }
        Ok(())
    };
    match source {
        MotionSource::Video { path, .. } => {
            disccraft_transcode::hd::for_each_hd_frame(path, options, frames, &mut |index, rgb| {
                frame(index, Some(rgb))
            })
            .map_err(|err| unsupported_menu(menu, err))?;
        }
        MotionSource::Still { .. } => {
            for index in 0..frames {
                frame(index, None).map_err(|err| unsupported_menu(menu, err))?;
            }
        }
    }
    let first = first.ok_or_else(|| invalid_menu(menu, "motion menu produced no frames"))?;
    let (data, units) = encoder
        .finish()
        .map_err(|err| unsupported_menu(menu, err))?;
    let audio = menu_audio(&context, menu, seconds)?;
    Ok(HdMotionMenu {
        first,
        data,
        units,
        audio,
    })
}

/// The number of frames of `seconds` at `rate` (numerator, denominator).
fn hd_frame_count(seconds: f64, (num, den): (u32, u32)) -> Result<u64, DvdError> {
    if !seconds.is_finite() || seconds < 0.0 || num == 0 || den == 0 {
        return Err(invalid("motion menu duration invalid"));
    }
    let rounded = (seconds * f64::from(num) / f64::from(den)).round();
    if !(1.0..=200_000.0).contains(&rounded) {
        return Err(invalid("motion menu frame count out of range"));
    }
    // Inside 1..=200 000: exact.
    Ok(rounded as u64)
}

/// The time of frame `index` at `rate`, in seconds.
fn hd_seconds(index: u64, (num, den): (u32, u32)) -> f64 {
    // Frame counts of a menu are small; the float holds them exactly.
    index as f64 * f64::from(den) / f64::from(num.max(1))
}

/// The pictures and sound of one play of a motion menu.
fn motion_parts(context: &MenuContext<'_>, menu: &Menu) -> Result<MotionMenu, DvdError> {
    let (project, standard) = (context.project, context.standard);
    let source = motion_source(project, menu)?;
    let seconds = match source {
        MotionSource::Video { seconds, .. } | MotionSource::Still { seconds } => seconds,
    };
    let frames = frame_count(seconds, standard).map_err(|err| invalid_menu(menu, err))?;
    let (first, video) = match source {
        MotionSource::Video { path, .. } => {
            encode_motion_video(project, menu, path, frames, standard)?
        }
        MotionSource::Still { .. } => encode_still_loop(project, menu, frames, standard)?,
    };
    let audio = menu_audio(context, menu, seconds)?;
    Ok(MotionMenu {
        first,
        video,
        audio,
    })
}

/// The cells of a motion menu (27 MHz from the first picture): one, or two split at its loop point (the muxer
/// starts the second at the VOBU there).
fn motion_cell_starts(menu: &Menu) -> Vec<i64> {
    let mut starts = vec![0];
    if let Some(point) = menu.effective_loop_point() {
        // Seconds of a motion menu are small; the product fits i64.
        starts.push((point.seconds() * 27_000_000.0).round() as i64);
    }
    starts
}

/// Where a moving menu's pictures come from, and how long it loops.
#[derive(Clone, Copy)]
enum MotionSource<'a> {
    /// The first `seconds` of a video file.
    Video { path: &'a Path, seconds: f64 },
    /// The menu's still picture (with its motion buttons playing), held for the audio's length or the menu's
    /// motion length.
    Still { seconds: f64 },
}

fn motion_source<'a>(project: &'a Project, menu: &'a Menu) -> Result<MotionSource<'a>, DvdError> {
    if let MenuBackground::Video { .. } = menu.background {
        let (path, seconds) = video_source(project, menu)?;
        return Ok(MotionSource::Video { path, seconds });
    }
    if menu.audio.is_none() {
        // Motion buttons over a still background: the menu's duration (15 s by default, as in Encore).
        return Ok(MotionSource::Still {
            seconds: f64::from(menu.motion_buttons_seconds().clamp(1, MAX_MENU_SECONDS)),
        });
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
    if menu.plays_buttons() {
        return encode_moving_buttons(project, menu, frames, standard);
    }
    let still = still_thumbnails(project, menu);
    let thumbnails = pictures(&still);
    let rendered = render_disc_menu(
        project,
        menu,
        MenuPictures {
            background: None,
            thumbnails: &thumbnails,
        },
    )
    .map_err(|err| invalid_menu(menu, err))?;
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

/// Encode `frames` pictures of a still background whose motion buttons play (Encore's モーションボタン).
fn encode_moving_buttons(
    project: &Project,
    menu: &Menu,
    frames: u64,
    standard: Standard,
) -> Result<(RenderedMenu, Vec<u8>), DvdError> {
    let moving = MovingThumbnails::open(project, menu);
    let mut encoder = motion_encoder(standard, menu, project.menus_wide())?;
    let mut first: Option<RenderedMenu> = None;
    let mut video_data = Vec::new();
    for index in 0..frames {
        let pictures_now = moving.frames_at(frame_seconds(index, standard));
        let thumbnails = pictures(&pictures_now);
        let rendered = render_disc_menu(
            project,
            menu,
            MenuPictures {
                background: None,
                thumbnails: &thumbnails,
            },
        )
        .map_err(|err| invalid_menu(menu, err))?;
        let frame = rgb_to_frame420(rendered.width, rendered.height, &rendered.rgb)
            .map_err(|err| invalid_menu(menu, err))?;
        let au = encoder
            .encode_frame(&frame)
            .map_err(|err| invalid_menu(menu, err))?;
        video_data.extend_from_slice(&au);
        if first.is_none() {
            first = Some(rendered);
        }
    }
    let first = first.ok_or_else(|| invalid_menu(menu, "motion menu produced no frames"))?;
    video_data.extend_from_slice(&encoder.finish());
    ensure_sequence_end(&mut video_data);
    Ok((first, video_data))
}

/// The time of frame `index` of a menu, in seconds.
fn frame_seconds(index: u64, standard: Standard) -> f64 {
    // Frame counts of a menu are small; the float holds them exactly.
    let index = index as f64;
    match standard {
        Standard::Ntsc => index * NTSC_FPS_DEN / NTSC_FPS_NUM,
        Standard::Pal => index / PAL_FPS,
    }
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
    let moving = menu
        .plays_buttons()
        .then(|| MovingThumbnails::open(project, menu));
    let still = if moving.is_some() {
        Vec::new()
    } else {
        still_thumbnails(project, menu)
    };

    for_each_frame(
        path,
        transcode_standard(standard),
        frames,
        &mut |index, rgb| {
            let now = moving
                .as_ref()
                .map(|moving| moving.frames_at(frame_seconds(index, standard)));
            let thumbnails = pictures(now.as_deref().unwrap_or(&still));
            let au =
                encode_motion_frame(project, menu, &thumbnails, &mut encoder, &mut first, rgb)?;
            video_data.extend_from_slice(&au);
            Ok(())
        },
    )
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
    thumbnails: &[ThumbnailPicture<'_>],
    encoder: &mut Encoder,
    first: &mut Option<RenderedMenu>,
    rgb: &[u8],
) -> Result<Vec<u8>, String> {
    let (width, height) = disccraft_project::frame_size(&project.settings);
    let rendered = render_disc_menu(
        project,
        menu,
        MenuPictures {
            background: Some((rgb, usize::from(width), usize::from(height))),
            thumbnails,
        },
    )
    .map_err(|err| err.to_string())?;
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
    if layout.cells.is_empty() {
        return Err(invalid_menu(menu, "missing motion cell layout"));
    }

    let vobu_starts: Vec<u32> = layout.vobus.iter().map(|vobu| vobu.start_sector).collect();
    if vobu_starts.is_empty() {
        return Err(invalid_menu(menu, "missing motion VOBU starts"));
    }

    // One cell and program, or two (intro and loop) when the menu has a loop point.
    let mut cells = Vec::with_capacity(layout.cells.len());
    let mut addresses = Vec::with_capacity(layout.cells.len());
    let mut total = 0u64;
    for (index, cell_layout) in layout.cells.iter().enumerate() {
        let start = u64::from(cell_layout.start_ptm);
        let end = u64::from(cell_layout.end_ptm);
        let duration = end
            .checked_sub(start)
            .ok_or_else(|| invalid_menu(menu, "menu duration underflow"))?;
        total = total.saturating_add(duration);
        cells.push(Cell {
            vob_id,
            cell_id: cell_layout.cell_id,
            stc_discontinuity: index == 0,
            still_time: 0,
            playback_time: BcdTime::from_90k(duration, standard),
            first_sector: cell_layout.first_sector,
            last_vobu_start: cell_layout.last_vobu_start,
            last_sector: cell_layout.last_sector,
        });
        addresses.push(CellAddress {
            vob_id,
            cell_id: cell_layout.cell_id,
            first_sector: cell_layout.first_sector,
            last_sector: cell_layout.last_sector,
        });
    }
    let playback_time = BcdTime::from_90k(total, standard);
    let loop_program = u8::try_from(cells.len()).unwrap_or(1).min(2);

    // Encore's ループ回数: forever (back to itself), once (then the end action), or so many times (a counter in
    // GPRM8). Without an end action, "once" holds the last picture and a count loops forever.
    // The later plays go back with LinkPGN (to program 2 from a loop point, else program 1): the pre commands do not
    // run again, so the count and the viewer's highlighted button stay.
    let loop_back = VmCommand(disccraft_nav::link_pgn(loop_program, 0));
    let mut pre = vm_commands(&nav_menu.pre);
    let mut still = 0;
    let post = match (menu.effective_loops(), nav_menu.post.is_empty()) {
        (disccraft_project::MenuLoops::Once, true) => {
            still = 0xFF;
            Vec::new()
        }
        (disccraft_project::MenuLoops::Once, false) => vm_commands(&nav_menu.post),
        (disccraft_project::MenuLoops::Times(times), false) => {
            let (loop_pre, loop_post) =
                disccraft_nav::counted_loop_to_program(times, loop_program, &nav_menu.post);
            // After the menu's own: its first lines pick the highlighted button.
            pre.extend(vm_commands(&loop_pre));
            vm_commands(&loop_post)
        }
        _ => vec![loop_back],
    };
    if let Some(last) = cells.last_mut() {
        last.still_time = still;
    }
    let programs: Vec<u8> = (1..=u8::try_from(cells.len()).unwrap_or(1)).collect();

    let pgc = Pgc {
        prohibited_ops: nav_menu.prohibited_ops,
        playback_time,
        audio_streams: u8::from(audio),
        subpicture_streams: 1,
        program_entry_cells: programs,
        cells,
        pre,
        post,
        cell_commands: Vec::new(),
        palette,
        ..Pgc::default()
    };

    Ok((
        MenuPgc {
            title_menu: nav_menu.title_menu,
            pgc,
        },
        addresses,
        vobu_starts,
    ))
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

    let mut bitmap = Bitmap {
        width: rendered.highlight.width,
        height: rendered.highlight.height,
        pixels,
    };
    // A subpicture covers at most 478 lines (NTSC) / 573 (PAL).
    crate::fit_subpicture(&mut bitmap, rendered.highlight.y, rendered.height);

    // Outside the selected / activated button, the frame shows in the normal colour (Encore's 標準グループ;
    // contrast 0 by default, so nothing is seen). The layer covers the buttons only, so it is shown where it
    // lies on the screen (the highlight rectangles of the HLI are screen positions).
    let params = SpuParams {
        x: rendered.highlight.x,
        y: rendered.highlight.y,
        colors: rendered.normal_entries,
        alpha: rendered.normal_contrasts,
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
            let own = u8::try_from(index.saturating_add(1)).unwrap_or(1);
            hli_button(*command, geometry, auto, group, own)
        })
        .collect();

    // No forced button: the menu's pre commands highlight its own button or the one a link asked for, and a motion
    // menu's later plays keep the viewer's (SPRM8).
    Ok(Hli {
        start_ptm: 0,
        buttons,
        default_button: 0,
        button_offset: menu.button_offset,
        color_sets: rendered.color_sets,
    })
}

/// Convert one rendered button geometry and VM command to HLI.
/// `own` is the button's own number: a route to nowhere (0: a lone button, or "no move" set by hand) stays on the
/// button, as DVD needs a button number there.
fn hli_button(
    command: [u8; 8],
    geometry: &ButtonGeometry,
    auto_action: bool,
    group: u8,
    own: u8,
) -> HliButton {
    let stay = |target: u8| if target == 0 { own } else { target };
    HliButton {
        x1: geometry.x1,
        y1: geometry.y1,
        x2: geometry.x2,
        y2: geometry.y2,
        color_set: group.clamp(1, 3),
        auto_action,
        up: stay(geometry.up),
        down: stay(geometry.down),
        left: stay(geometry.left),
        right: stay(geometry.right),
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
    use super::{encode_menu_spu, frame_count};
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
        // A button down to the frame's last line: the subpicture stops at line 478.
        if let Some(button) = project
            .menu_mut(menu.id)
            .and_then(|m| m.buttons.first_mut())
        {
            button.rect = disccraft_project::Rect {
                x: 100,
                y: 440,
                w: 200,
                h: 40,
            };
        }
        let low = project.menu(menu.id).ok_or("menu")?.clone();
        let low_rendered = disccraft_menu::render_menu(&project, &low)?;
        let low_spu = disccraft_spu::decode_spu(&encode_menu_spu(&low_rendered)?)?;
        assert_eq!(
            u32::from(low_spu.params.y) + u32::from(low_spu.bitmap.height),
            478
        );
        // The subpicture lies over the buttons, where the highlight layer is.
        assert_ne!((rendered.highlight.x, rendered.highlight.y), (0, 0));
        assert_eq!(
            (hidden.params.x, hidden.params.y),
            (rendered.highlight.x, rendered.highlight.y)
        );
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
}
