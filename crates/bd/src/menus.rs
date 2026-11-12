use std::path::{Path, PathBuf};
use std::slice;

use disccraft_bdmv::mobj::NavCommand;
use disccraft_bdmv::mpls;
use disccraft_bdmv::{Stream, StreamCoding};
use disccraft_m2tsmux::{EsInput, EsUnit, IG_PID, M2tsLayout, StreamKind, VIDEO_PID};
use disccraft_menu::{ButtonGeometry, HighlightLayer, RenderedMenu, render_menu};
use disccraft_mpeg2venc::{Standard, StillOptions, encode_still, rgb_to_frame420};
use disccraft_pgs::ig::{IgButton, IgObject, IgPage, InteractiveComposition, display_set};
use disccraft_pgs::{PaletteEntry, TimedSegment, rgba_to_entry};
use disccraft_project::{Menu, Project, VideoStandard, opacity_to_alpha};
use disccraft_psmux::es::parse_video;

use crate::clip;
use crate::{BdError, frame_rate, invalid, io_error};

const QUANTISER_SCALE_CODE: u8 = 4;
const MAX_STILL_BYTES: usize = 1024 * 1024;
const START_DTS_27MHZ: i64 = 8_100_000;
const SEQUENCE_END: [u8; 4] = [0x00, 0x00, 0x01, 0xB7];
const STILL_MODE_HOLD: u8 = 2;

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
) -> Result<u64, BdError> {
    let rendered = render_menu(project, menu_clip.menu).map_err(menu_error)?;
    let video = prepare_video(project, &rendered)?;
    let ig_units = build_ig_units(project, &rendered, menu_clip, video.pts_90k)?;
    let streams = menu_streams(project);

    let inputs = mux_inputs(video.data, video.pts_90k, video.dts_90k, ig_units);
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
    let playlist_bytes = write_menu_playlist(
        dirs,
        menu_clip.number,
        streams,
        video.presentation_start,
        video.presentation_end,
    )?;

    add_bytes(add_bytes(m2ts_bytes, clpi_bytes)?, playlist_bytes)
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
    ig: Stream,
}

struct ObjectIds {
    selected: u16,
    activated: u16,
}

fn prepare_video(project: &Project, rendered: &RenderedMenu) -> Result<VideoPrepared, BdError> {
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
            coding: clip::video_coding(&project.settings, project.menus_wide()),
        },
        ig: Stream {
            pid: IG_PID,
            coding: StreamCoding::Igs { language: *b"und" },
        },
    }
}

fn mux_inputs(video_data: Vec<u8>, pts: u64, dts: u64, ig_units: Vec<EsUnit>) -> Vec<EsInput> {
    vec![
        EsInput {
            pid: VIDEO_PID,
            kind: StreamKind::Mpeg2Video,
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
    let info = clip::build_clip_info(
        layout,
        &streams.video,
        slice::from_ref(&streams.ig),
        presentation_start,
        presentation_end,
    )?;
    clip::write_clpi(&path, &info)
}

fn write_menu_playlist(
    dirs: &MenuDirs<'_>,
    number: u16,
    streams: MenuStreams,
    in_time: u32,
    out_time: u32,
) -> Result<u64, BdError> {
    let path = playlist_path(dirs, number);
    let playlist = menu_playlist(number, streams, in_time, out_time)?;
    let bytes = mpls::write_mpls(&playlist).map_err(bdmv_error)?;
    write_bytes(&path, &bytes)
}

fn menu_playlist(
    number: u16,
    streams: MenuStreams,
    in_time: u32,
    out_time: u32,
) -> Result<mpls::PlayList, BdError> {
    let play_item = mpls::PlayItem {
        clip: clip::clip_name(number)?,
        in_time,
        out_time,
        streams: mpls::StnTable {
            video: vec![streams.video],
            audio: Vec::new(),
            pg: Vec::new(),
            ig: vec![streams.ig],
        },
        still_mode: STILL_MODE_HOLD,
        still_time: 0,
    };

    Ok(mpls::PlayList {
        items: vec![play_item],
        marks: vec![mpls::PlayListMark {
            play_item: 0,
            time: in_time,
        }],
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
    let palette = build_palette(menu_clip.menu);
    let mut objects = build_objects(rendered, menu_clip.menu, menu_clip.commands.len())?;
    if menu_clip.popup {
        objects.extend(build_label_objects(rendered, menu_clip)?);
    } else if normal_shown(menu_clip.menu) {
        objects.extend(build_normal_objects(rendered, menu_clip.commands.len())?);
    }
    let composition = build_composition(project, rendered, menu_clip)?;
    let segments = display_set(&composition, &palette, &objects, pts).map_err(video_error)?;
    segments.into_iter().map(segment_unit).collect()
}

/// The IG stream of `menu` as a pop-up over a title, and its stream entry.
pub(crate) fn popup_ig_units(
    project: &Project,
    menu: &Menu,
    commands: &[Vec<NavCommand>],
    pts: u64,
) -> Result<(Vec<EsUnit>, Stream), BdError> {
    let rendered = render_menu(project, menu).map_err(menu_error)?;
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
            crop_highlight(&rendered.highlight, geometry, width, height, NORMAL_PALETTE)?
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
                if alpha > 127 {
                    let i = pixel_index(width, row, col)?;
                    if let Some(pixel) = pixels.get_mut(i) {
                        *pixel = LABEL_PALETTE;
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

/// Palette id of the normal frame (Encore's 標準グループ).
const NORMAL_PALETTE: u8 = 7;
/// Palette id of a pop-up button's text in its normal state.
const LABEL_PALETTE: u8 = 8;

/// The palette ids of highlight group `index` (0 to 2): selected 1 / 3 / 5, activated 2 / 4 / 6.
fn group_palette(index: usize) -> (u8, u8) {
    let group = u8::try_from(index.min(2)).unwrap_or(0);
    (1 + group * 2, 2 + group * 2)
}

/// The palette ids of button `index`'s highlight group.
fn button_palette(menu: &Menu, index: usize) -> (u8, u8) {
    let group = menu
        .buttons
        .get(index)
        .map_or(1, |button| button.highlight_group.clamp(1, 3));
    group_palette(usize::from(group.saturating_sub(1)))
}

/// Whether the menu's normal colour can be seen.
fn normal_shown(menu: &Menu) -> bool {
    menu.color_set_or_default().normal.opacity > 0
}

/// The IG palette: clear, each highlight group's selected / activated colour with its opacity, the normal colour
/// and white for a pop-up's text.
fn build_palette(menu: &Menu) -> Vec<PaletteEntry> {
    let set = menu.color_set_or_default();
    let mut entries = vec![rgba_to_entry(0, 0, 0, 0, 0)];
    for (index, group) in set.groups.iter().enumerate() {
        let (selected_id, activated_id) = group_palette(index);
        let [red, green, blue] = group.selected;
        entries.push(rgba_to_entry(
            selected_id,
            red,
            green,
            blue,
            opacity_to_alpha(group.selected_opacity),
        ));
        let ([red, green, blue], opacity) = group.activated_look();
        entries.push(rgba_to_entry(
            activated_id,
            red,
            green,
            blue,
            opacity_to_alpha(opacity),
        ));
    }
    let [red, green, blue] = set.normal.color;
    entries.push(rgba_to_entry(
        NORMAL_PALETTE,
        red,
        green,
        blue,
        opacity_to_alpha(set.normal.opacity),
    ));
    entries.push(rgba_to_entry(LABEL_PALETTE, 255, 255, 255, 255));
    entries
}

/// The normal-state objects of an always-on menu whose normal colour is seen: each button's frame.
fn build_normal_objects(
    rendered: &RenderedMenu,
    button_count: usize,
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
            pixels: crop_highlight(&rendered.highlight, geometry, width, height, NORMAL_PALETTE)?,
        });
    }
    Ok(objects)
}

fn build_objects(
    rendered: &RenderedMenu,
    menu: &Menu,
    button_count: usize,
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
        let (selected_value, activated_value) = button_palette(menu, index);

        let selected_pixels =
            crop_highlight(&rendered.highlight, geometry, width, height, selected_value)?;
        let activated_pixels = crop_highlight(
            &rendered.highlight,
            geometry,
            width,
            height,
            activated_value,
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

fn crop_highlight(
    layer: &HighlightLayer,
    geometry: &ButtonGeometry,
    width: u16,
    height: u16,
    value: u8,
) -> Result<Vec<u8>, BdError> {
    let size = pixel_count(width, height)?;
    let mut pixels = vec![0_u8; size];

    for row in 0..usize::from(height) {
        crop_row(layer, geometry, width, row, value, &mut pixels)?;
    }

    Ok(pixels)
}

fn crop_row(
    layer: &HighlightLayer,
    geometry: &ButtonGeometry,
    width: u16,
    row: usize,
    value: u8,
    pixels: &mut [u8],
) -> Result<(), BdError> {
    let object_y = add_u16_usize(geometry.y1, row)?;

    for col in 0..usize::from(width) {
        let object_x = add_u16_usize(geometry.x1, col)?;
        if highlight_at(layer, object_x, object_y)? {
            let index = pixel_index(width, row, col)?;
            if let Some(pixel) = pixels.get_mut(index) {
                *pixel = value;
            }
        }
    }

    Ok(())
}

fn highlight_at(layer: &HighlightLayer, x: u16, y: u16) -> Result<bool, BdError> {
    match layer_index(layer, x, y)? {
        Some(index) => Ok(layer.pixels.get(index).copied() == Some(1)),
        None => Ok(false),
    }
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
    let default_selected = default_selected(menu_clip.menu.default_button, buttons.len())?;
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
    use super::{build_objects, build_palette, normal_shown};
    use disccraft_project::{ColorSet, HighlightGroup, Link, NormalColor, Project};

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
        });
        let menu = menu.clone();
        let palette = build_palette(&menu);
        let selected = palette
            .iter()
            .find(|entry| entry.id == 1)
            .ok_or("entry 1")?;
        assert_eq!(selected.alpha, 128);
        let rendered = disccraft_menu::render_menu(&project, &menu)?;
        let objects = build_objects(&rendered, &menu, 2)?;
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
}
