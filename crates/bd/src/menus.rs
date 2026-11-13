use std::path::{Path, PathBuf};
use std::slice;

use disccraft_bdmv::mobj::NavCommand;
use disccraft_bdmv::mpls;
use disccraft_bdmv::{Stream, StreamCoding};
use disccraft_m2tsmux::{
    AUDIO_PID, EsInput, EsUnit, IG_PID, M2tsLayout, StreamKind, VIDEO_PID, bd_lpcm_units,
};
use disccraft_menu::{
    ButtonGeometry, HighlightLayer, MenuPictures, RenderedMenu, render_disc_menu,
};
use disccraft_mpeg2venc::{Standard, StillOptions, encode_still, rgb_to_frame420};
use disccraft_pgs::ig::{
    CompositionState, IgButton, IgObject, IgPage, InteractiveComposition, display_set_as,
};
use disccraft_pgs::{PaletteEntry, TimedSegment, rgba_to_entry};
use disccraft_project::{Link, Menu, MenuLoops, Project, VideoStandard, opacity_to_alpha};
use disccraft_psmux::es::{PictureType, VideoStream, parse_video};

use crate::clip;
use crate::{BdError, frame_rate, invalid, io_error};

const QUANTISER_SCALE_CODE: u8 = 4;
const MAX_STILL_BYTES: usize = 1024 * 1024;
const START_DTS_27MHZ: i64 = 8_100_000;
const SEQUENCE_END: [u8; 4] = [0x00, 0x00, 0x01, 0xB7];
/// PlayItem still mode: none (the playlist ends after its last picture).
const STILL_MODE_NONE: u8 = 0;
const STILL_MODE_HOLD: u8 = 2;
/// PlayItem still mode: hold for `still_time` seconds, then end.
const STILL_MODE_TIME: u8 = 1;

/// Directories used while writing one menu clip.
pub(crate) struct MenuDirs<'a> {
    /// `BDMV/STREAM` directory.
    pub(crate) stream: &'a Path,
    /// `BDMV/CLIPINF` directory.
    pub(crate) clipinf: &'a Path,
    /// `BDMV/PLAYLIST` directory.
    pub(crate) playlist: &'a Path,
}

/// One menu to write as a still-video clip with an IG stream, or to lay over a title as a pop-up menu.
pub(crate) struct MenuClip<'a> {
    /// Project menu.
    pub(crate) menu: &'a Menu,
    /// Clip and playlist number.
    pub(crate) number: u16,
    /// HDMV commands for each button.
    pub(crate) commands: &'a [Vec<NavCommand>],
    /// A pop-up menu (shown over a title on the remote's Pop-up Menu key): the buttons carry their text as the
    /// normal state, since there is no menu picture under them.
    pub(crate) popup: bool,
}

/// Write a menu clip and return the total number of bytes written.
///
/// # Errors
///
/// Returns [`BdError`] if rendering, encoding, muxing, or writing fails.
pub(crate) fn write_menu_clip(
    project: &Project,
    menu_clip: &MenuClip<'_>,
    dirs: &MenuDirs<'_>,
    cache_dir: &Path,
) -> Result<u64, BdError> {
    if menu_clip.menu.is_motion() {
        return write_motion_menu_clip(project, menu_clip, dirs, cache_dir);
    }
    let rendered = render_with_thumbnails(project, menu_clip.menu)?;
    let video = prepare_video(project, &rendered)?;
    let ig_units = build_ig_units(project, &rendered, menu_clip, video.pts_90k)?;
    let streams = menu_streams(project);

    let inputs = mux_inputs(
        project.settings.bd_video.hd().is_some(),
        (video.data, video.pts_90k, video.dts_90k),
        ig_units,
    );
    let m2ts_path = stream_path(dirs, menu_clip.number);
    let (layout, m2ts_bytes) = clip::write_m2ts(&m2ts_path, &inputs)?;

    let clpi_bytes = write_menu_clpi(
        dirs,
        menu_clip.number,
        &layout,
        &streams,
        video.presentation_start,
        video.presentation_end,
    )?;
    let (still_mode, still_time) = still_play(menu_clip.menu.hold_seconds());
    let play = MenuPlay {
        in_time: video.presentation_start,
        out_time: video.presentation_end,
        still_mode,
        still_time,
        loop_mark: None,
        uo_mask: menu_uo_mask(project, menu_clip.menu),
    };
    let playlist_bytes = write_menu_playlist(dirs, menu_clip.number, streams, &play)?;

    add_bytes(add_bytes(m2ts_bytes, clpi_bytes)?, playlist_bytes)
}

/// Write a motion menu clip (a video background, menu audio or motion buttons): one play of the menu with its
/// audio, the IG display set at its start and again at its loop point (for a player that starts playing there),
/// and a playlist with a mark at the loop point. The menu's movie object plays it again (`nav::MenuMotion`).
fn write_motion_menu_clip(
    project: &Project,
    menu_clip: &MenuClip<'_>,
    dirs: &MenuDirs<'_>,
    cache_dir: &Path,
) -> Result<u64, BdError> {
    let menu = menu_clip.menu;
    if let Some((width, height, fps_num, fps_den)) = project.settings.bd_video.hd() {
        let options = disccraft_transcode::hd::HdOptions {
            width,
            height,
            fps_num,
            fps_den,
            target_kbps: 15_000,
            max_kbps: 30_000,
        };
        return write_hd_motion_menu_clip(project, menu_clip, (dirs, cache_dir), &options);
    }
    let motion = disccraft_dvd::encode_motion_menu(project, menu, cache_dir)
        .map_err(|error| BdError::Prepare(error.to_string()))?;
    let parsed = parse_video(&motion.video, START_DTS_27MHZ).map_err(video_error)?;
    let start = parsed
        .pictures
        .iter()
        .map(|picture| picture.pts)
        .min()
        .ok_or_else(|| invalid("motion menu has no pictures"))?;
    let end = clip::video_end(&parsed)?;
    let start_90k = clip::to_90k(start)?;
    let loop_pts = loop_picture(menu, &parsed, start);

    let mut ig_units = build_ig_units(project, &motion.first, menu_clip, start_90k)?;
    if let Some(pts) = loop_pts {
        ig_units.extend(build_ig_units_as(
            project,
            &motion.first,
            menu_clip,
            clip::to_90k(pts)?,
            CompositionState::AcquisitionPoint,
        )?);
    }

    let mut streams = menu_streams(project);
    let mut inputs = vec![EsInput {
        pid: VIDEO_PID,
        kind: StreamKind::Mpeg2Video,
        units: clip::picture_units(&parsed, &motion.video)?,
    }];
    if let Some(audio) = &motion.audio {
        let units = bd_lpcm_units(&audio.data, audio.channels, start_90k).map_err(video_error)?;
        inputs.push(EsInput {
            pid: AUDIO_PID,
            kind: StreamKind::Lpcm,
            units,
        });
        streams.audio = Some(Stream {
            pid: AUDIO_PID,
            coding: clip::lpcm_coding(audio, None)?,
        });
    }
    inputs.push(EsInput {
        pid: IG_PID,
        kind: StreamKind::Igs,
        units: ig_units,
    });

    let m2ts_path = stream_path(dirs, menu_clip.number);
    let (layout, m2ts_bytes) = clip::write_m2ts(&m2ts_path, &inputs)?;
    let in_time = clip::to_45k(start)?;
    let out_time = clip::to_45k(end)?;
    let clpi_bytes = write_menu_clpi(dirs, menu_clip.number, &layout, &streams, in_time, out_time)?;
    // Played once without an end action, the last picture stays (as on DVD).
    let hold = menu.effective_loops() == MenuLoops::Once && menu.end_action == Link::NotSet;
    let play = MenuPlay {
        in_time,
        out_time,
        still_mode: if hold {
            STILL_MODE_HOLD
        } else {
            STILL_MODE_NONE
        },
        still_time: 0,
        loop_mark: loop_pts.map(clip::to_45k).transpose()?,
        uo_mask: menu_uo_mask(project, menu),
    };
    let playlist_bytes = write_menu_playlist(dirs, menu_clip.number, streams, &play)?;

    add_bytes(add_bytes(m2ts_bytes, clpi_bytes)?, playlist_bytes)
}

/// An HD Blu-ray disc's motion menu: one play as H.264 of the disc's HD format (each frame drawn at the HD size),
/// laid out as the SD one: the IG display set at its start and again at the IDR picture at or before its loop point,
/// its audio, and a playlist with a mark there.
fn write_hd_motion_menu_clip(
    project: &Project,
    menu_clip: &MenuClip<'_>,
    (dirs, cache_dir): (&MenuDirs<'_>, &Path),
    options: &disccraft_transcode::hd::HdOptions,
) -> Result<u64, BdError> {
    let menu = menu_clip.menu;
    let motion = disccraft_dvd::encode_hd_motion_menu(project, menu, cache_dir, options)
        .map_err(|error| BdError::Prepare(error.to_string()))?;
    let start_90k = clip::to_90k(START_DTS_27MHZ)?;
    let start = i64::try_from(start_90k).map_err(|_| invalid("start too large"))?;
    let at = |time: i64| {
        start
            .checked_add(time)
            .and_then(|value| u64::try_from(value).ok())
            .ok_or_else(|| invalid("motion menu time stamp out of range"))
    };
    let mut units = Vec::with_capacity(motion.units.len());
    for unit in &motion.units {
        let begin = usize::try_from(unit.offset).map_err(|_| invalid("unit offset too large"))?;
        let end = begin
            .checked_add(usize::try_from(unit.len).map_err(|_| invalid("unit too large"))?)
            .ok_or_else(|| invalid("unit range overflow"))?;
        units.push(EsUnit {
            pts: at(unit.pts)?,
            dts: at(unit.dts)?,
            data: motion
                .data
                .get(begin..end)
                .ok_or_else(|| invalid("unit outside its stream"))?
                .to_vec(),
            random_access: unit.keyframe,
        });
    }
    let frame = i64::from(options.fps_den)
        .saturating_mul(90_000)
        .checked_div(i64::from(options.fps_num.max(1)))
        .unwrap_or(3003);
    let last = motion
        .units
        .iter()
        .map(|unit| unit.pts)
        .max()
        .ok_or_else(|| invalid("motion menu has no pictures"))?;
    let end_90k = at(last.saturating_add(frame))?;
    // The IDR picture at or before the loop point: where a player can start decoding the later plays.
    let loop_90k = match menu.effective_loop_point() {
        Some(point) => {
            // Loop points are seconds into a short menu; the product fits i64.
            let target = (point.seconds() * 90_000.0).round() as i64;
            motion
                .units
                .iter()
                .filter(|unit| unit.keyframe && unit.pts <= target)
                .map(|unit| unit.pts)
                .max()
                .map(at)
                .transpose()?
        }
        None => None,
    };

    let mut ig_units = build_ig_units(project, &motion.first, menu_clip, start_90k)?;
    if let Some(pts) = loop_90k {
        ig_units.extend(build_ig_units_as(
            project,
            &motion.first,
            menu_clip,
            pts,
            CompositionState::AcquisitionPoint,
        )?);
    }
    let mut streams = menu_streams(project);
    let mut inputs = vec![EsInput {
        pid: VIDEO_PID,
        kind: StreamKind::H264Video,
        units,
    }];
    if let Some(audio) = &motion.audio {
        let units = bd_lpcm_units(&audio.data, audio.channels, start_90k).map_err(video_error)?;
        inputs.push(EsInput {
            pid: AUDIO_PID,
            kind: StreamKind::Lpcm,
            units,
        });
        streams.audio = Some(Stream {
            pid: AUDIO_PID,
            coding: clip::lpcm_coding(audio, None)?,
        });
    }
    inputs.push(EsInput {
        pid: IG_PID,
        kind: StreamKind::Igs,
        units: ig_units,
    });

    let m2ts_path = stream_path(dirs, menu_clip.number);
    let (layout, m2ts_bytes) = clip::write_m2ts(&m2ts_path, &inputs)?;
    let to_45k = |time: u64| u32::try_from(time / 2).map_err(|_| invalid("time too large"));
    let in_time = to_45k(start_90k)?;
    let out_time = to_45k(end_90k)?;
    let clpi_bytes = write_menu_clpi(dirs, menu_clip.number, &layout, &streams, in_time, out_time)?;
    // Played once without an end action, the last picture stays (as on DVD).
    let hold = menu.effective_loops() == MenuLoops::Once && menu.end_action == Link::NotSet;
    let play = MenuPlay {
        in_time,
        out_time,
        still_mode: if hold {
            STILL_MODE_HOLD
        } else {
            STILL_MODE_NONE
        },
        still_time: 0,
        loop_mark: loop_90k.map(to_45k).transpose()?,
        uo_mask: menu_uo_mask(project, menu),
    };
    let playlist_bytes = write_menu_playlist(dirs, menu_clip.number, streams, &play)?;

    add_bytes(add_bytes(m2ts_bytes, clpi_bytes)?, playlist_bytes)
}

/// Where a motion menu's later plays start (27 MHz): the GOP start at or before its loop point, where a player can
/// start decoding (DVD starts the loop cell at that VOBU too). `None` without a loop point.
fn loop_picture(menu: &Menu, video: &VideoStream, start: i64) -> Option<i64> {
    let point = menu.effective_loop_point()?;
    // Loop points are seconds into a short menu; the product fits i64.
    let target = start.checked_add((point.seconds() * 27_000_000.0).round() as i64)?;
    video
        .pictures
        .iter()
        .filter(|picture| {
            matches!(picture.kind, PictureType::I) && picture.gop_start && picture.pts <= target
        })
        .map(|picture| picture.pts)
        .max()
        .filter(|pts| *pts > start)
}

struct VideoPrepared {
    data: Vec<u8>,
    pts_90k: u64,
    dts_90k: u64,
    presentation_start: u32,
    presentation_end: u32,
}

struct MenuStreams {
    video: Stream,
    /// The menu audio of a motion menu.
    audio: Option<Stream>,
    ig: Stream,
}

/// How a menu's playlist plays its clip.
struct MenuPlay {
    in_time: u32,
    out_time: u32,
    still_mode: u8,
    still_time: u16,
    /// A motion menu's loop point (45 kHz): the playlist's mark 1, where its later plays start.
    loop_mark: Option<u32>,
    /// The user operations prohibited while the menu is shown (the menu's and the disc's).
    uo_mask: u64,
}

struct ObjectIds {
    selected: u16,
    activated: u16,
}

fn prepare_video(project: &Project, rendered: &RenderedMenu) -> Result<VideoPrepared, BdError> {
    if let Some((_, _, fps_num, fps_den)) = project.settings.bd_video.hd() {
        return prepare_hd_video(rendered, (fps_num, fps_den));
    }
    let frame =
        rgb_to_frame420(rendered.width, rendered.height, &rendered.rgb).map_err(video_error)?;
    let options = StillOptions {
        standard: standard(project.settings.standard),
        quantiser_scale_code: QUANTISER_SCALE_CODE,
        max_bytes: MAX_STILL_BYTES,
        wide: project.menus_wide(),
    };
    let mut data = encode_still(&frame, &options).map_err(video_error)?;
    append_sequence_end(&mut data);

    let (pts, dts, duration) = parse_timestamps(&data)?;
    let pts_90k = clip::to_90k(pts)?;
    let dts_90k = clip::to_90k(dts)?;
    let presentation_start = clip::to_45k(pts)?;
    let presentation_end = clip::to_45k(checked_add_i64(pts, duration)?)?;

    Ok(VideoPrepared {
        data,
        pts_90k,
        dts_90k,
        presentation_start,
        presentation_end,
    })
}

/// An HD menu's still: one H.264 IDR picture at the menu clip's start, lasting one frame (the play item holds it).
fn prepare_hd_video(
    rendered: &RenderedMenu,
    (fps_num, fps_den): (u32, u32),
) -> Result<VideoPrepared, BdError> {
    let options = disccraft_transcode::hd::HdOptions {
        width: u32::from(rendered.width),
        height: u32::from(rendered.height),
        fps_num,
        fps_den,
        target_kbps: 20_000,
        max_kbps: 40_000,
    };
    let mut data = disccraft_transcode::hd::encode_h264_still(&rendered.rgb, &options)
        .map_err(|error| BdError::Unsupported(format!("HD menu: {error}")))?;
    // End of sequence (NAL unit type 10): the still is shown at once instead of waiting for more pictures, as an
    // MPEG-2 still ends with a sequence end code.
    data.extend_from_slice(&H264_END_OF_SEQUENCE);
    let start = START_DTS_27MHZ;
    let frame = i64::from(fps_den)
        .saturating_mul(27_000_000)
        .checked_div(i64::from(fps_num.max(1)))
        .unwrap_or(1_126_125);
    Ok(VideoPrepared {
        data,
        pts_90k: clip::to_90k(start)?,
        dts_90k: clip::to_90k(start)?,
        presentation_start: clip::to_45k(start)?,
        presentation_end: clip::to_45k(checked_add_i64(start, frame)?)?,
    })
}

/// An H.264 end-of-sequence NAL unit.
const H264_END_OF_SEQUENCE: [u8; 4] = [0, 0, 1, 0x0A];

fn parse_timestamps(data: &[u8]) -> Result<(i64, i64, i64), BdError> {
    let parsed = parse_video(data, START_DTS_27MHZ).map_err(video_error)?;
    let picture = parsed
        .pictures
        .first()
        .ok_or_else(|| invalid("video has no pictures"))?;

    Ok((picture.pts, picture.dts, picture.duration))
}

fn append_sequence_end(data: &mut Vec<u8>) {
    if !data.ends_with(&SEQUENCE_END) {
        data.extend_from_slice(&SEQUENCE_END);
    }
}

fn standard(value: VideoStandard) -> Standard {
    match value {
        VideoStandard::Ntsc => Standard::Ntsc,
        VideoStandard::Pal => Standard::Pal,
    }
}

fn menu_streams(project: &Project) -> MenuStreams {
    MenuStreams {
        video: Stream {
            pid: VIDEO_PID,
            coding: menu_video_coding(project),
        },
        audio: None,
        ig: Stream {
            pid: IG_PID,
            coding: StreamCoding::Igs { language: *b"und" },
        },
    }
}

/// A menu clip's video coding: SD MPEG-2, or H.264 at the disc's HD format.
fn menu_video_coding(project: &Project) -> StreamCoding {
    match project.settings.bd_video.hd() {
        Some((_, height, fps_num, fps_den)) => StreamCoding::H264Video {
            video_format: if height >= 1080 { 6 } else { 5 },
            frame_rate: match (fps_num, fps_den) {
                (24_000, 1001) => 1,
                (24, 1) => 2,
                (25, 1) => 3,
                (50, 1) => 6,
                (60_000, 1001) => 7,
                _ => 4,
            },
            aspect: 3,
        },
        None => clip::video_coding(&project.settings, project.menus_wide()),
    }
}

fn mux_inputs(
    hd: bool,
    (video_data, pts, dts): (Vec<u8>, u64, u64),
    ig_units: Vec<EsUnit>,
) -> Vec<EsInput> {
    vec![
        EsInput {
            pid: VIDEO_PID,
            kind: if hd {
                StreamKind::H264Video
            } else {
                StreamKind::Mpeg2Video
            },
            units: vec![EsUnit {
                pts,
                dts,
                data: video_data,
                random_access: true,
            }],
        },
        EsInput {
            pid: IG_PID,
            kind: StreamKind::Igs,
            units: ig_units,
        },
    ]
}

fn write_menu_clpi(
    dirs: &MenuDirs<'_>,
    number: u16,
    layout: &M2tsLayout,
    streams: &MenuStreams,
    presentation_start: u32,
    presentation_end: u32,
) -> Result<u64, BdError> {
    let path = clipinf_path(dirs, number);
    let others: Vec<Stream> = streams
        .audio
        .iter()
        .chain(slice::from_ref(&streams.ig))
        .copied()
        .collect();
    let info = clip::build_clip_info(
        layout,
        &streams.video,
        &others,
        presentation_start,
        presentation_end,
    )?;
    clip::write_clpi(&path, &info)
}

/// The UO mask of a menu's playlist: the menu's and the disc's prohibited operations.
fn menu_uo_mask(project: &Project, menu: &Menu) -> u64 {
    crate::uo::uo_mask(&[&menu.prohibited_ops, &project.settings.prohibited_ops])
}

/// A still menu's still mode: `hold` is how long the still stays before the playlist ends (the menu's end action
/// follows); `None` holds it.
fn still_play(hold: Option<u32>) -> (u8, u16) {
    match hold {
        Some(seconds) => (
            STILL_MODE_TIME,
            u16::try_from(seconds.max(1)).unwrap_or(u16::MAX),
        ),
        None => (STILL_MODE_HOLD, 0),
    }
}

fn write_menu_playlist(
    dirs: &MenuDirs<'_>,
    number: u16,
    streams: MenuStreams,
    play: &MenuPlay,
) -> Result<u64, BdError> {
    let path = playlist_path(dirs, number);
    let mut playlist = menu_playlist(number, streams, play)?;
    playlist.uo_mask = play.uo_mask;
    let bytes = mpls::write_mpls(&playlist).map_err(bdmv_error)?;
    write_bytes(&path, &bytes)
}

fn menu_playlist(
    number: u16,
    streams: MenuStreams,
    play: &MenuPlay,
) -> Result<mpls::PlayList, BdError> {
    let play_item = mpls::PlayItem {
        clip: clip::clip_name(number)?,
        in_time: play.in_time,
        out_time: play.out_time,
        streams: mpls::StnTable {
            video: vec![streams.video],
            audio: streams.audio.into_iter().collect(),
            pg: Vec::new(),
            ig: vec![streams.ig],
        },
        still_mode: play.still_mode,
        still_time: play.still_time,
    };
    let marks = std::iter::once(play.in_time)
        .chain(play.loop_mark)
        .map(|time| mpls::PlayListMark { play_item: 0, time })
        .collect();

    Ok(mpls::PlayList {
        items: vec![play_item],
        marks,
        uo_mask: 0,
        playback: mpls::Playback::Sequential,
    })
}

fn stream_path(dirs: &MenuDirs<'_>, number: u16) -> PathBuf {
    dirs.stream.join(format!("{number:05}.m2ts"))
}

fn clipinf_path(dirs: &MenuDirs<'_>, number: u16) -> PathBuf {
    dirs.clipinf.join(format!("{number:05}.clpi"))
}

fn playlist_path(dirs: &MenuDirs<'_>, number: u16) -> PathBuf {
    dirs.playlist.join(format!("{number:05}.mpls"))
}

pub(crate) fn build_ig_units(
    project: &Project,
    rendered: &RenderedMenu,
    menu_clip: &MenuClip<'_>,
    pts: u64,
) -> Result<Vec<EsUnit>, BdError> {
    build_ig_units_as(
        project,
        rendered,
        menu_clip,
        pts,
        CompositionState::EpochStart,
    )
}

/// The menu's IG display set at `pts` with the given composition state.
fn build_ig_units_as(
    project: &Project,
    rendered: &RenderedMenu,
    menu_clip: &MenuClip<'_>,
    pts: u64,
    state: CompositionState,
) -> Result<Vec<EsUnit>, BdError> {
    let colours = IgColours::new(menu_clip.menu, &rendered.highlight);
    let palette = colours.palette(menu_clip.menu);
    let mut objects = build_objects(rendered, menu_clip.menu, menu_clip.commands.len(), &colours)?;
    if menu_clip.popup {
        objects.extend(build_label_objects(rendered, menu_clip, &colours)?);
    } else if normal_shown(menu_clip.menu) {
        objects.extend(build_normal_objects(
            rendered,
            menu_clip.commands.len(),
            &colours,
        )?);
    }
    let composition = build_composition(project, rendered, menu_clip)?;
    let segments =
        display_set_as(&composition, &palette, &objects, pts, state).map_err(video_error)?;
    segments.into_iter().map(segment_unit).collect()
}

/// The menu's picture with its video thumbnails' still pictures (Encore's `(%)` layers).
fn render_with_thumbnails(project: &Project, menu: &Menu) -> Result<RenderedMenu, BdError> {
    let frames = disccraft_dvd::still_thumbnails(project, menu);
    let thumbnails: Vec<_> = frames
        .iter()
        .map(disccraft_dvd::ThumbnailFrame::picture)
        .collect();
    render_disc_menu(
        project,
        menu,
        MenuPictures {
            background: None,
            thumbnails: &thumbnails,
        },
    )
    .map_err(menu_error)
}

/// The IG stream of `menu` as a pop-up over a title, and its stream entry.
pub(crate) fn popup_ig_units(
    project: &Project,
    menu: &Menu,
    commands: &[Vec<NavCommand>],
    pts: u64,
) -> Result<(Vec<EsUnit>, Stream), BdError> {
    let rendered = render_with_thumbnails(project, menu)?;
    let menu_clip = MenuClip {
        menu,
        number: 0,
        commands,
        popup: true,
    };
    let units = build_ig_units(project, &rendered, &menu_clip, pts)?;
    Ok((units, menu_streams(project).ig))
}

/// A pop-up menu's normal-state objects: each button's text in white, cropped to the button.
fn build_label_objects(
    rendered: &RenderedMenu,
    menu_clip: &MenuClip<'_>,
    colours: &IgColours,
) -> Result<Vec<IgObject>, BdError> {
    let count = menu_clip.commands.len();
    let mut objects = Vec::with_capacity(count);
    for index in 0..count {
        let geometry = rendered
            .buttons
            .get(index)
            .ok_or_else(|| invalid("missing button geometry"))?;
        let button = menu_clip
            .menu
            .buttons
            .get(index)
            .ok_or_else(|| invalid("missing button"))?;
        let width = rect_width(geometry.x1, geometry.x2)?;
        let height = rect_height(geometry.y1, geometry.y2)?;
        let rgba =
            disccraft_menu::label_rgba(menu_clip.menu, button, rendered.width, rendered.height);
        // The normal frame (when it is seen) under the text.
        let mut pixels = if normal_shown(menu_clip.menu) {
            crop_highlight(
                &rendered.highlight,
                geometry,
                (width, height),
                colours.slots(State::Normal, 0),
                colours,
            )?
        } else {
            vec![0_u8; pixel_count(width, height)?]
        };
        for row in 0..usize::from(height) {
            for col in 0..usize::from(width) {
                let x = usize::from(geometry.x1).saturating_add(col);
                let y = usize::from(geometry.y1).saturating_add(row);
                let alpha = y
                    .checked_mul(usize::from(rendered.width))
                    .and_then(|n| n.checked_add(x))
                    .and_then(|n| n.checked_mul(4))
                    .and_then(|n| n.checked_add(3))
                    .and_then(|n| rgba.get(n).copied())
                    .unwrap_or(0);
                if let Some(id) = colours.shade(LABEL_PALETTE, alpha) {
                    let i = pixel_index(width, row, col)?;
                    if let Some(pixel) = pixels.get_mut(i) {
                        *pixel = id;
                    }
                }
            }
        }
        objects.push(IgObject {
            id: normal_object_id(index, count)?,
            width,
            height,
            pixels,
        });
    }
    Ok(objects)
}

/// The object id of a button's normal state (a pop-up's text, or the normal frame): after the selected /
/// activated pairs.
fn normal_object_id(index: usize, count: usize) -> Result<u16, BdError> {
    count
        .checked_mul(2)
        .and_then(|base| base.checked_add(index))
        .and_then(|id| u16::try_from(id).ok())
        .ok_or_else(|| invalid("too many buttons"))
}

fn segment_unit(segment: TimedSegment) -> Result<EsUnit, BdError> {
    Ok(EsUnit {
        pts: segment.pts,
        dts: segment.dts,
        data: segment.data,
        random_access: false,
    })
}

/// An IG page's default selected button when it names none (the player keeps its selected button).
const NO_DEFAULT_BUTTON: u16 = 0xFFFF;

/// Colour slot of the normal frame (Encore's 標準グループ colour 1).
const NORMAL_PALETTE: u8 = 7;
/// Colour slot of a pop-up button's text in its normal state.
const LABEL_PALETTE: u8 = 8;

/// Alpha steps of a rich-colour highlight (Encore's ハイライトをリッチカラーで書き出し): each colour has up to this many
/// palette entries, from faint to its own opacity, so the highlight keeps the smooth edges of its shape.
const RICH_STEPS: u8 = 15;

/// A button state, for its colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Selected,
    Activated,
    Normal,
}

/// How a menu's highlight colours become IG palette ids. Every colour has a slot: 1 to 6 colour 1 of each highlight
/// group's selected / activated state, 7 the normal colour 1, 8 a pop-up's text, then colours 2 and 3 (Encore's
/// カラー 2・3, a subpicture's `(=2)` / `(=3)` layers) of the highlight groups the buttons use and of the normal
/// state, when the highlight has them. A plain highlight's palette id is its slot; a rich one gives every slot
/// `steps` alpha steps, as many as fit the 255 ids.
struct IgColours {
    rich: bool,
    steps: u8,
    /// The slots of colours 2 and 3, by state, highlight group (0 to 2; 0 for the normal state) and colour.
    more: Vec<((State, usize, u8), u8)>,
}

impl IgColours {
    fn new(menu: &Menu, layer: &HighlightLayer) -> Self {
        let mut used_groups = [false; 3];
        for index in 0..menu.buttons.len() {
            if let Some(used) = used_groups.get_mut(button_group(menu, index)) {
                *used = true;
            }
        }
        let mut more = Vec::new();
        let mut last = LABEL_PALETTE;
        for n in [2_u8, 3] {
            if !layer.pixels.contains(&n) {
                continue;
            }
            for (group, used) in used_groups.iter().enumerate() {
                if !used {
                    continue;
                }
                for state in [State::Selected, State::Activated] {
                    last = last.saturating_add(1);
                    more.push(((state, group, n), last));
                }
            }
            last = last.saturating_add(1);
            more.push(((State::Normal, 0, n), last));
        }
        let steps = if menu.rich_highlights {
            RICH_STEPS.min(u8::MAX / last.max(1))
        } else {
            1
        };
        Self {
            rich: menu.rich_highlights,
            steps,
            more,
        }
    }

    /// The slot of colour `n` (1 to 3) in `state` of highlight group `group` (0 to 2); 0 when it has none.
    fn slot(&self, state: State, group: usize, n: u8) -> u8 {
        if n <= 1 {
            let (selected, activated) = group_palette(group);
            return match state {
                State::Selected => selected,
                State::Activated => activated,
                State::Normal => NORMAL_PALETTE,
            };
        }
        let group = if state == State::Normal {
            0
        } else {
            group.min(2)
        };
        self.more
            .iter()
            .find(|(key, _)| *key == (state, group, n))
            .map_or(0, |(_, slot)| *slot)
    }

    /// The slots of the highlight's pixel values (0 to 3) in `state` of highlight group `group`.
    fn slots(&self, state: State, group: usize) -> [u8; 4] {
        [
            0,
            self.slot(state, group, 1),
            self.slot(state, group, 2),
            self.slot(state, group, 3),
        ]
    }

    /// The palette id that draws slot `slot` where a highlight covers `coverage` (0 to 255) of a pixel; `None` where
    /// nothing is drawn. A plain highlight draws its slot on pixels covered by half or more; a rich one picks the
    /// alpha step of the coverage.
    fn shade(&self, slot: u8, coverage: u8) -> Option<u8> {
        if slot == 0 {
            return None;
        }
        if !self.rich {
            return (coverage > 127).then_some(slot);
        }
        let level = (u16::from(coverage) * u16::from(self.steps) + 127) / 255;
        let level = u8::try_from(level).ok().filter(|level| *level > 0)?;
        self.rich_id(slot, level)
    }

    /// The palette id of alpha step `level` (1 to `steps`) of slot `slot`.
    fn rich_id(&self, slot: u8, level: u8) -> Option<u8> {
        slot.checked_sub(1)?
            .checked_mul(self.steps)?
            .checked_add(level)
    }

    /// The IG palette: clear, then every slot's colour with its opacity (in alpha steps for a rich highlight).
    fn palette(&self, menu: &Menu) -> Vec<PaletteEntry> {
        let set = menu.color_set_or_default();
        // (slot, colour, alpha) of each colour.
        let mut colours: Vec<(u8, [u8; 3], u8)> = Vec::new();
        for (index, group) in set.groups.iter().enumerate() {
            let (selected_id, activated_id) = group_palette(index);
            let (colour, opacity) = group.selected_color(1);
            colours.push((selected_id, colour, opacity_to_alpha(opacity)));
            let (colour, opacity) = group.activated_color(1);
            colours.push((activated_id, colour, opacity_to_alpha(opacity)));
        }
        colours.push((
            NORMAL_PALETTE,
            set.normal.color,
            opacity_to_alpha(set.normal.opacity),
        ));
        colours.push((LABEL_PALETTE, [255, 255, 255], 255));
        for &((state, group, n), slot) in &self.more {
            let (colour, opacity) = match state {
                State::Selected => set
                    .groups
                    .get(group)
                    .map_or(([0; 3], 0), |group| group.selected_color(n)),
                State::Activated => set
                    .groups
                    .get(group)
                    .map_or(([0; 3], 0), |group| group.activated_color(n)),
                State::Normal => {
                    let normal = set.normal_color(n);
                    (normal.color, normal.opacity)
                }
            };
            colours.push((slot, colour, opacity_to_alpha(opacity)));
        }

        let mut entries = vec![rgba_to_entry(0, 0, 0, 0, 0)];
        for (slot, [red, green, blue], alpha) in colours {
            if !self.rich {
                entries.push(rgba_to_entry(slot, red, green, blue, alpha));
                continue;
            }
            for level in 1..=self.steps {
                let Some(id) = self.rich_id(slot, level) else {
                    continue;
                };
                // At most 255 × 15: well inside u16, and the quotient fits u8.
                let step = u16::from(alpha) * u16::from(level) / u16::from(self.steps.max(1));
                let step = u8::try_from(step).unwrap_or(alpha);
                entries.push(rgba_to_entry(id, red, green, blue, step));
            }
        }
        entries
    }
}

/// The colour-1 slots of highlight group `index` (0 to 2): selected 1 / 3 / 5, activated 2 / 4 / 6.
fn group_palette(index: usize) -> (u8, u8) {
    let group = u8::try_from(index.min(2)).unwrap_or(0);
    (1 + group * 2, 2 + group * 2)
}

/// Button `index`'s highlight group (0 to 2).
fn button_group(menu: &Menu, index: usize) -> usize {
    let group = menu
        .buttons
        .get(index)
        .map_or(1, |button| button.highlight_group.clamp(1, 3));
    usize::from(group.saturating_sub(1))
}

/// Whether one of the menu's normal colours can be seen.
fn normal_shown(menu: &Menu) -> bool {
    menu.color_set_or_default()
        .normal_colors()
        .iter()
        .any(|color| color.opacity > 0)
}

/// The normal-state objects of an always-on menu whose normal colour is seen: each button's frame.
fn build_normal_objects(
    rendered: &RenderedMenu,
    button_count: usize,
    colours: &IgColours,
) -> Result<Vec<IgObject>, BdError> {
    let mut objects = Vec::with_capacity(button_count);
    for index in 0..button_count {
        let geometry = rendered
            .buttons
            .get(index)
            .ok_or_else(|| invalid("missing button geometry"))?;
        let width = rect_width(geometry.x1, geometry.x2)?;
        let height = rect_height(geometry.y1, geometry.y2)?;
        objects.push(IgObject {
            id: normal_object_id(index, button_count)?,
            width,
            height,
            pixels: crop_highlight(
                &rendered.highlight,
                geometry,
                (width, height),
                colours.slots(State::Normal, 0),
                colours,
            )?,
        });
    }
    Ok(objects)
}

fn build_objects(
    rendered: &RenderedMenu,
    menu: &Menu,
    button_count: usize,
    colours: &IgColours,
) -> Result<Vec<IgObject>, BdError> {
    let mut objects = Vec::new();

    for index in 0..button_count {
        let geometry = rendered
            .buttons
            .get(index)
            .ok_or_else(|| invalid("missing button geometry"))?;
        let ids = object_ids(index)?;
        let width = rect_width(geometry.x1, geometry.x2)?;
        let height = rect_height(geometry.y1, geometry.y2)?;
        let group = button_group(menu, index);

        let selected_pixels = crop_highlight(
            &rendered.highlight,
            geometry,
            (width, height),
            colours.slots(State::Selected, group),
            colours,
        )?;
        let activated_pixels = crop_highlight(
            &rendered.highlight,
            geometry,
            (width, height),
            colours.slots(State::Activated, group),
            colours,
        )?;

        objects.push(IgObject {
            id: ids.selected,
            width,
            height,
            pixels: selected_pixels,
        });
        objects.push(IgObject {
            id: ids.activated,
            width,
            height,
            pixels: activated_pixels,
        });
    }

    Ok(objects)
}

fn object_ids(index: usize) -> Result<ObjectIds, BdError> {
    let selected = u16::try_from(
        index
            .checked_mul(2)
            .ok_or_else(|| invalid("too many buttons"))?,
    )
    .map_err(|_| invalid("too many buttons"))?;
    let activated = selected
        .checked_add(1)
        .ok_or_else(|| invalid("too many buttons"))?;
    Ok(ObjectIds {
        selected,
        activated,
    })
}

/// The button's part of the highlight layer as an IG object, each pixel value (1 to 3) in its slot of `slots`:
/// plain, or (rich) with the alpha steps of its coverage.
fn crop_highlight(
    layer: &HighlightLayer,
    geometry: &ButtonGeometry,
    (width, height): (u16, u16),
    slots: [u8; 4],
    colours: &IgColours,
) -> Result<Vec<u8>, BdError> {
    let size = pixel_count(width, height)?;
    let mut pixels = vec![0_u8; size];

    for row in 0..usize::from(height) {
        crop_row(layer, geometry, (width, row), (slots, colours), &mut pixels)?;
    }

    Ok(pixels)
}

fn crop_row(
    layer: &HighlightLayer,
    geometry: &ButtonGeometry,
    (width, row): (u16, usize),
    (slots, colours): ([u8; 4], &IgColours),
    pixels: &mut [u8],
) -> Result<(), BdError> {
    let object_y = add_u16_usize(geometry.y1, row)?;

    for col in 0..usize::from(width) {
        let object_x = add_u16_usize(geometry.x1, col)?;
        let (value, coverage) = pixel_at(layer, object_x, object_y)?;
        let id = if colours.rich {
            // The faint edge of a text highlight (value 0) is in colour 1.
            let slot = slots.get(usize::from(value.max(1))).copied().unwrap_or(0);
            colours.shade(slot, coverage)
        } else if value > 0 {
            slots
                .get(usize::from(value))
                .copied()
                .filter(|slot| *slot != 0)
        } else {
            None
        };
        if let Some(id) = id {
            let index = pixel_index(width, row, col)?;
            if let Some(pixel) = pixels.get_mut(index) {
                *pixel = id;
            }
        }
    }

    Ok(())
}

/// The highlight's pixel value (0 to 3) at (`x`, `y`) and how much of the pixel it covers (0 to 255).
fn pixel_at(layer: &HighlightLayer, x: u16, y: u16) -> Result<(u8, u8), BdError> {
    Ok(match layer_index(layer, x, y)? {
        Some(index) => (
            layer.pixels.get(index).copied().unwrap_or(0),
            layer.coverage.get(index).copied().unwrap_or(0),
        ),
        None => (0, 0),
    })
}

fn layer_index(layer: &HighlightLayer, x: u16, y: u16) -> Result<Option<usize>, BdError> {
    if x < layer.x || y < layer.y {
        return Ok(None);
    }

    let local_x = x
        .checked_sub(layer.x)
        .ok_or_else(|| invalid("invalid highlight layer"))?;
    let local_y = y
        .checked_sub(layer.y)
        .ok_or_else(|| invalid("invalid highlight layer"))?;

    if local_x >= layer.width || local_y >= layer.height {
        return Ok(None);
    }

    let index = usize::from(local_y)
        .checked_mul(usize::from(layer.width))
        .and_then(|value| value.checked_add(usize::from(local_x)))
        .ok_or_else(|| invalid("highlight layer too large"))?;
    Ok(Some(index))
}

fn pixel_count(width: u16, height: u16) -> Result<usize, BdError> {
    usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| invalid("button too large"))
}

fn pixel_index(width: u16, row: usize, col: usize) -> Result<usize, BdError> {
    usize::from(width)
        .checked_mul(row)
        .and_then(|value| value.checked_add(col))
        .ok_or_else(|| invalid("button too large"))
}

fn add_u16_usize(base: u16, offset: usize) -> Result<u16, BdError> {
    let offset = u16::try_from(offset).map_err(|_| invalid("button too large"))?;
    base.checked_add(offset)
        .ok_or_else(|| invalid("button too large"))
}

fn rect_width(x1: u16, x2: u16) -> Result<u16, BdError> {
    if x2 < x1 {
        return Err(invalid("invalid button rectangle"));
    }
    x2.checked_sub(x1)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| invalid("button too large"))
}

fn rect_height(y1: u16, y2: u16) -> Result<u16, BdError> {
    if y2 < y1 {
        return Err(invalid("invalid button rectangle"));
    }
    y2.checked_sub(y1)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| invalid("button too large"))
}

fn build_composition(
    project: &Project,
    rendered: &RenderedMenu,
    menu_clip: &MenuClip<'_>,
) -> Result<InteractiveComposition, BdError> {
    let buttons = build_buttons(rendered, menu_clip)?;
    // A menu's movie object selects its button (its default, or the one a link asks for), and a motion menu's later
    // plays keep the button the viewer is on: no default (the player's selected button register). A pop-up menu
    // over a title has no movie object of its own: it names its default.
    let default_selected = if !menu_clip.popup {
        NO_DEFAULT_BUTTON
    } else {
        default_selected(menu_clip.menu.default_button, buttons.len())?
    };
    let page = IgPage {
        id: 0,
        default_selected,
        palette_id: 0,
        buttons,
    };

    Ok(InteractiveComposition {
        width: rendered.width,
        height: rendered.height,
        frame_rate_id: frame_rate(project.settings.standard),
        number: 0,
        popup: menu_clip.popup,
        pages: vec![page],
    })
}

fn build_buttons(
    rendered: &RenderedMenu,
    menu_clip: &MenuClip<'_>,
) -> Result<Vec<IgButton>, BdError> {
    let mut buttons = Vec::new();

    for index in 0..rendered.buttons.len() {
        let geometry = rendered
            .buttons
            .get(index)
            .ok_or_else(|| invalid("missing button geometry"))?;
        let commands = menu_clip
            .commands
            .get(index)
            .ok_or_else(|| invalid("missing button commands"))?;
        let auto = menu_clip
            .menu
            .buttons
            .get(index)
            .is_some_and(|button| button.auto_activate);
        let normal = if menu_clip.popup || normal_shown(menu_clip.menu) {
            normal_object_id(index, rendered.buttons.len())?
        } else {
            0xFFFF
        };
        buttons.push(build_button(index, geometry, commands, auto, normal)?);
    }

    Ok(buttons)
}

fn build_button(
    index: usize,
    geometry: &ButtonGeometry,
    commands: &[NavCommand],
    auto_action: bool,
    normal_object: u16,
) -> Result<IgButton, BdError> {
    let id = u16::try_from(index).map_err(|_| invalid("too many buttons"))?;
    let numeric_value = id
        .checked_add(1)
        .ok_or_else(|| invalid("too many buttons"))?;
    let ids = object_ids(index)?;
    let command_bytes = command_bytes(commands)?;

    Ok(IgButton {
        id,
        numeric_value,
        auto_action,
        x: geometry.x1,
        y: geometry.y1,
        upper: neighbour(geometry.up, index)?,
        lower: neighbour(geometry.down, index)?,
        left: neighbour(geometry.left, index)?,
        right: neighbour(geometry.right, index)?,
        normal_object,
        selected_object: ids.selected,
        activated_object: ids.activated,
        commands: command_bytes,
    })
}

fn command_bytes(commands: &[NavCommand]) -> Result<Vec<[u8; 12]>, BdError> {
    commands.iter().map(|command| Ok(command.0)).collect()
}

fn neighbour(value: u8, index: usize) -> Result<u16, BdError> {
    let number = if value == 0 {
        index
    } else {
        usize::from(value)
            .checked_sub(1)
            .ok_or_else(|| invalid("invalid neighbour"))?
    };
    u16::try_from(number).map_err(|_| invalid("too many buttons"))
}

fn default_selected(default: u32, count: usize) -> Result<u16, BdError> {
    let last_index = count
        .checked_sub(1)
        .ok_or_else(|| invalid("menu has no buttons"))?;
    let last = u32::try_from(last_index).map_err(|_| invalid("too many buttons"))?;
    let selected = if default >= last {
        last_index
    } else {
        usize::try_from(default).map_err(|_| invalid("too many buttons"))?
    };
    u16::try_from(selected).map_err(|_| invalid("too many buttons"))
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<u64, BdError> {
    std::fs::write(path, bytes).map_err(|error| io_error(path, error))?;
    u64::try_from(bytes.len()).map_err(|_| invalid("file size overflow"))
}

fn add_bytes(left: u64, right: u64) -> Result<u64, BdError> {
    left.checked_add(right)
        .ok_or_else(|| invalid("total size overflow"))
}

fn checked_add_i64(left: i64, right: i64) -> Result<i64, BdError> {
    left.checked_add(right)
        .ok_or_else(|| invalid("timestamp overflow"))
}

fn menu_error(error: impl std::fmt::Display) -> BdError {
    BdError::Invalid(error.to_string())
}

fn video_error(error: impl std::fmt::Display) -> BdError {
    BdError::Mux(error.to_string())
}

fn bdmv_error(error: impl std::fmt::Display) -> BdError {
    BdError::Bdmv(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{IgColours, State, build_objects, normal_shown};
    use disccraft_menu::HighlightLayer;
    use disccraft_project::{ColorSet, GroupColor, HighlightGroup, Link, NormalColor, Project};

    #[test]
    fn a_rich_highlight_keeps_the_smooth_edges_of_its_text()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        project.add_button(menu_id, "Play".to_owned(), None, Link::Stop)?;
        let plain = project.menu(menu_id).ok_or("menu")?.clone();
        let mut rich = plain.clone();
        rich.rich_highlights = true;
        let ids = |menu: &disccraft_project::Menu| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
            let rendered = disccraft_menu::render_menu(&project, menu)?;
            let colours = IgColours::new(menu, &rendered.highlight);
            let objects = build_objects(&rendered, menu, 1, &colours)?;
            let selected = objects.first().ok_or("selected object")?;
            let mut ids: Vec<u8> = selected
                .pixels
                .iter()
                .copied()
                .filter(|id| *id != 0)
                .collect();
            ids.sort_unstable();
            ids.dedup();
            Ok(ids)
        };
        // Plain: the one colour of group 1's selected state.
        assert_eq!(ids(&plain)?, vec![1]);
        // Rich: its alpha steps (palette 1 to 15), the faint ones at the text's edges.
        let steps = ids(&rich)?;
        assert!(steps.len() > 3, "{steps:?}");
        assert!(steps.iter().all(|id| (1..=15).contains(id)), "{steps:?}");
        assert!(steps.contains(&15));
        // 15 steps for each of the 8 colours, after the clear entry; the full step has the colour's own alpha.
        let rendered = disccraft_menu::render_menu(&project, &rich)?;
        let palette = IgColours::new(&rich, &rendered.highlight).palette(&rich);
        assert_eq!(palette.len(), 1 + 8 * 15);
        let full = palette
            .iter()
            .find(|entry| entry.id == 15)
            .ok_or("entry 15")?;
        let plain_palette = IgColours::new(&plain, &rendered.highlight).palette(&plain);
        let one = plain_palette
            .iter()
            .find(|entry| entry.id == 1)
            .ok_or("entry 1")?;
        assert_eq!(full.alpha, one.alpha);
        Ok(())
    }

    #[test]
    fn each_button_uses_its_groups_colours_and_opacities() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        project.add_button(menu_id, "One".to_owned(), None, Link::Stop)?;
        project.add_button(menu_id, "Two".to_owned(), None, Link::Stop)?;
        let menu = project.menu_mut(menu_id).ok_or("menu")?;
        if let Some(button) = menu.buttons.get_mut(1) {
            button.highlight_group = 2;
        }
        let mut group = HighlightGroup::new([255, 0, 0], [0, 0, 255]);
        group.selected_opacity = 50;
        menu.color_set = Some(ColorSet {
            groups: [group; 3],
            normal: NormalColor::default(),
            normal_more: None,
        });
        let menu = menu.clone();
        let rendered = disccraft_menu::render_menu(&project, &menu)?;
        let colours = IgColours::new(&menu, &rendered.highlight);
        let palette = colours.palette(&menu);
        let selected = palette
            .iter()
            .find(|entry| entry.id == 1)
            .ok_or("entry 1")?;
        assert_eq!(selected.alpha, 128);
        let objects = build_objects(&rendered, &menu, 2, &colours)?;
        let used = |id: u16| {
            objects
                .iter()
                .find(|object| object.id == id)
                .and_then(|object| object.pixels.iter().copied().find(|pixel| *pixel != 0))
        };
        // Button 1 (group 1): palette 1 / 2. Button 2 (group 2): palette 3 / 4.
        assert_eq!((used(0), used(1)), (Some(1), Some(2)));
        assert_eq!((used(2), used(3)), (Some(3), Some(4)));
        assert!(!normal_shown(&menu));
        Ok(())
    }

    #[test]
    fn colours_two_and_three_get_slots_of_their_own() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        project.add_button(menu_id, "One".to_owned(), None, Link::Stop)?;
        let menu = project.menu_mut(menu_id).ok_or("menu")?;
        let mut group = HighlightGroup::new([255, 0, 0], [0, 0, 255]);
        let more = GroupColor {
            selected: [0, 255, 0],
            selected_opacity: 50,
            activated: [9, 9, 9],
            activated_opacity: 100,
        };
        group.more = Some([more, more]);
        menu.color_set = Some(ColorSet {
            groups: [group; 3],
            normal: NormalColor::default(),
            normal_more: None,
        });
        let mut menu = menu.clone();
        // A one-row highlight with colours 1 to 3 (as a subpicture's layers make).
        let layer = HighlightLayer {
            x: 0,
            y: 0,
            width: 4,
            height: 1,
            pixels: vec![0, 1, 2, 3],
            coverage: vec![0, 255, 255, 255],
        };
        let colours = IgColours::new(&menu, &layer);
        // Group 1 (the only one used): colour 2 in slots 9 / 10, the normal colour 2 in 11, colour 3 in 12 to 14.
        assert_eq!(colours.slots(State::Selected, 0), [0, 1, 9, 12]);
        assert_eq!(colours.slots(State::Activated, 0), [0, 2, 10, 13]);
        assert_eq!(colours.slots(State::Normal, 0), [0, 7, 11, 14]);
        assert_eq!(colours.slots(State::Selected, 1), [0, 3, 0, 0]);
        let palette = colours.palette(&menu);
        assert_eq!(palette.len(), 1 + 14);
        let green = palette
            .iter()
            .find(|entry| entry.id == 9)
            .ok_or("entry 9")?;
        assert_eq!(green.alpha, 128);

        // Rich: 14 slots of 15 steps fit (210 ids).
        menu.rich_highlights = true;
        let rich = IgColours::new(&menu, &layer);
        assert_eq!(rich.steps, 15);
        assert_eq!(rich.palette(&menu).len(), 1 + 14 * 15);
        // Three groups used: 22 slots, 11 steps each.
        menu.buttons.push(menu.buttons[0].clone());
        menu.buttons.push(menu.buttons[0].clone());
        menu.buttons[1].highlight_group = 2;
        menu.buttons[2].highlight_group = 3;
        let crowded = IgColours::new(&menu, &layer);
        assert_eq!(crowded.steps, 11);
        let ids = crowded.palette(&menu);
        assert!(ids.iter().all(|entry| entry.id <= 22 * 11));
        Ok(())
    }
}
