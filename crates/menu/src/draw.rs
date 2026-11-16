//! Drawing helpers for menu frames.

use crate::MenuError;
use disccraft_project::{
    AssetId, Button, DropShadow, MAX_TEXT_SCALE, MAX_TEXT_SIZE, MIN_TEXT_SCALE, MIN_TEXT_SIZE,
    Menu, MenuBackground, MenuText, Orientation, Project, Rect, TextAntialias, TextFormat,
    TextScript,
};
use filmcraft_text::{Caps, Layout, Mask, ParagraphStyle, TextStyle, families, layout, render};
use png::{BitDepth, ColorType, Decoder, Transformations};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

const MAX_IMAGE_DIMENSION: usize = 8192;

const FONT_FAMILIES: [&str; 7] = [
    "Yu Gothic UI",
    "Yu Gothic",
    "Meiryo",
    "Noto Sans CJK JP",
    "Noto Sans JP",
    "Hiragino Sans",
    "Inter",
];

pub(crate) struct SourceImage {
    pub(crate) data: Vec<u8>,
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) channels: usize,
}

pub(crate) fn new_frame(width: u16, height: u16) -> Result<Vec<u8>, MenuError> {
    let w = usize::from(width);
    let h = usize::from(height);
    let len = w
        .checked_mul(h)
        .and_then(|n| n.checked_mul(3))
        .ok_or_else(|| MenuError::Invalid("frame too large".to_string()))?;

    Ok(vec![0u8; len])
}

pub(crate) fn draw_background(
    project: &Project,
    menu: &Menu,
    width: u16,
    height: u16,
    rgb: &mut [u8],
) -> Result<(), MenuError> {
    match &menu.background {
        MenuBackground::Color { color } => {
            fill_color(rgb, *color);
            Ok(())
        }
        MenuBackground::Image { asset } => {
            let source = load_image(project, *asset, width, height)?;
            scale_bilinear(&source, width, height, rgb);
            Ok(())
        }
        // Motion menus get their frames from the caller (render_menu_over); a still render shows black.
        MenuBackground::Video { .. } => {
            fill_color(rgb, [0, 0, 0]);
            Ok(())
        }
    }
}

/// Draws the menu's text objects, each in its own colour or the menu's text colour.
/// How much text is squeezed horizontally on a 16:9 menu, so it keeps its shape when the anamorphic picture
/// is stretched to 16:9 on the screen (16:9 pixels are 4/3 as wide as 4:3 pixels).
const WIDE_SQUEEZE: f32 = 0.75;

/// The horizontal scale of text on `menu`.
fn squeeze(menu: &Menu) -> f32 {
    if menu.widescreen { WIDE_SQUEEZE } else { 1.0 }
}

/// The transform that draws `laid` with the centre of its bounds at `center`: mirrored and turned by
/// `orientation`, then squeezed by `sx`.
fn oriented_pen(
    laid: &Layout,
    sx: f32,
    orientation: Orientation,
    scale: (f32, f32),
    center: (f32, f32),
) -> filmcraft_text::Xform {
    oriented_pen_of(laid.bounds, sx, orientation, scale, center)
}

/// [`oriented_pen`] for a block with `bounds` (a vertical text's pieces together).
fn oriented_pen_of(
    bounds: [f32; 4],
    sx: f32,
    orientation: Orientation,
    scale: (f32, f32),
    center: (f32, f32),
) -> filmcraft_text::Xform {
    let [x0, y0, x1, y1] = bounds;
    let (cx, cy) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
    let [a, b, c, d] = orientation.matrix();
    // Scaled (Encore's horizontal / vertical scale) before it is turned, squeezed after.
    let (a, b, c, d) = (a * scale.0, b * scale.0, c * scale.1, d * scale.1);
    let (a, c) = (a * sx, c * sx);
    [
        a,
        b,
        c,
        d,
        center.0 - (a * cx + c * cy),
        center.1 - (b * cx + d * cy),
    ]
}

/// A text's coverage mask with its edge treated as `mode` asks (Encore's anti-aliasing list). DiscCraft's
/// rasterizer gives one exact coverage; the modes reshape it: none keeps whole pixels, strong fills edge pixels
/// more, sharp narrows the edge, smooth spreads it over one more pixel.
fn antialiased(mut mask: Mask, mode: TextAntialias) -> Mask {
    match mode {
        TextAntialias::Crisp => {}
        TextAntialias::None => {
            for value in &mut mask.a {
                *value = if *value >= 0.5 { 1.0 } else { 0.0 };
            }
        }
        TextAntialias::Strong => {
            for value in &mut mask.a {
                *value = value.clamp(0.0, 1.0).powf(0.6);
            }
        }
        TextAntialias::Sharp => {
            for value in &mut mask.a {
                *value = ((*value - 0.5) * 1.8 + 0.5).clamp(0.0, 1.0);
            }
        }
        TextAntialias::Smooth => {
            let (w, h) = (mask.w, mask.h);
            let source = mask.a.clone();
            let at = |x: usize, y: usize| {
                if x >= w || y >= h {
                    return 0.0;
                }
                y.checked_mul(w)
                    .and_then(|row| row.checked_add(x))
                    .and_then(|i| source.get(i))
                    .copied()
                    .unwrap_or(0.0)
            };
            for y in 0..h {
                for x in 0..w {
                    // The pixel itself counts half; its four neighbours share the other half.
                    let around = at(x.wrapping_sub(1), y)
                        + at(x.saturating_add(1), y)
                        + at(x, y.wrapping_sub(1))
                        + at(x, y.saturating_add(1));
                    let smoothed = at(x, y) * 0.5 + around * 0.125;
                    if let Some(slot) = y
                        .checked_mul(w)
                        .and_then(|row| row.checked_add(x))
                        .and_then(|i| mask.a.get_mut(i))
                    {
                        *slot = smoothed.clamp(0.0, 1.0);
                    }
                }
            }
        }
    }
    mask
}

/// How much smaller a superscript or subscript is set (InDesign's and Photoshop's usual 58.3 %).
const SCRIPT_SIZE: f32 = 0.583;
/// How far a superscript is raised or a subscript lowered, as a share of the text size (33.3 %).
const SCRIPT_POSITION: f32 = 0.333;

/// A text's horizontal and vertical scale as factors.
fn text_scale(format: &TextFormat) -> (f32, f32) {
    let factor = |percent: u16| f32::from(percent.clamp(MIN_TEXT_SCALE, MAX_TEXT_SCALE)) / 100.0;
    (
        factor(format.horizontal_scale),
        factor(format.vertical_scale),
    )
}

/// The drawn size of `laid`: scaled by `scale`, turned by `orientation`, then squeezed by `sx`.
fn oriented_size(
    laid: &Layout,
    sx: f32,
    orientation: Orientation,
    scale: (f32, f32),
) -> (f32, f32) {
    oriented_size_of(laid.bounds, sx, orientation, scale)
}

/// [`oriented_size`] for a block with `bounds`.
fn oriented_size_of(
    bounds: [f32; 4],
    sx: f32,
    orientation: Orientation,
    scale: (f32, f32),
) -> (f32, f32) {
    let [x0, y0, x1, y1] = bounds;
    let (w, h) = ((x1 - x0) * scale.0, (y1 - y0) * scale.1);
    if orientation.swaps_sides() {
        (h * sx, w)
    } else {
        (w * sx, h)
    }
}

pub(crate) fn draw_texts(menu: &Menu, width: u16, height: u16, rgb: &mut [u8]) {
    if menu.texts.is_empty() {
        return;
    }
    let sx = squeeze(menu);
    for text in menu.texts.iter().filter(|text| !text.hidden) {
        let family = family_for(text.font.as_deref());
        let Some((pieces, _)) = text_layout(text, &family, sx) else {
            continue;
        };
        let (mask, mask_x, mask_y) = crate::text_block::rasterize_pieces(&pieces);
        let mask = antialiased(mask, text.format.antialias);
        if mask.w == 0 || mask.h == 0 {
            continue;
        }
        let color = text.color.unwrap_or(menu.text_color);
        blend_shadow(
            &mask,
            mask_x,
            mask_y,
            text.shadow.as_ref(),
            rgb,
            width,
            height,
        );
        blend_mask(&mask, mask_x, mask_y, color, rgb, width, height);
    }
}

/// Draws `shadow` (when there is one) of an object whose coverage is `mask` at `mask_x`, `mask_y`.
fn blend_shadow(
    mask: &Mask,
    mask_x: i32,
    mask_y: i32,
    shadow: Option<&DropShadow>,
    rgb: &mut [u8],
    width: u16,
    height: u16,
) {
    if let Some(shadow) = shadow
        && let Some((shade, x, y)) = crate::shadow::shadow_mask(mask, mask_x, mask_y, shadow)
    {
        blend_mask(&shade, x, y, shadow.color, rgb, width, height);
    }
}

/// The frame rectangle a text object covers (at least its size square, also for an empty text).
pub(crate) fn text_rect(menu: &Menu, text: &MenuText) -> Rect {
    let size = text.size.clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE);
    let family = family_for(text.font.as_deref());
    let sx = squeeze(menu);
    let (w, h) = match text_layout(text, &family, sx) {
        Some((_, (w, h))) => (to_side(w), to_side(h)),
        None => (size, size),
    };
    // A text standing up (turned on its side, or vertical) is at least its size wide.
    let (min_w, min_h) = if text.orientation.swaps_sides() != text.vertical {
        (size, size / 2)
    } else {
        (size / 2, size)
    };
    Rect {
        x: text.x,
        y: text.y,
        w: w.max(min_w),
        h: h.max(min_h),
    }
}

/// The frame rectangle a text object covers as it is drawn: its box ([`text_rect`]) turned by its free angle about
/// the box's centre (for selecting and outlining it).
pub(crate) fn text_bounds(menu: &Menu, text: &MenuText) -> Rect {
    disccraft_project::rotated_bounds(text_rect(menu, text), text.orientation.angle)
}

/// A measured length as a rectangle side (at least 1 pixel).
fn to_side(length: f32) -> u16 {
    if length.is_finite() && length > 0.0 {
        // Finite and positive; `as` saturates at u16::MAX.
        (length.ceil() as u16).max(1)
    } else {
        1
    }
}

/// Lays out a text object: the pieces to draw (each a layout and its transform into the frame; one for a horizontal
/// text) and the drawn size of its box, whose top-left corner is the text's x, y before a free turn (`sx`: the
/// horizontal squeeze the text is drawn with).
fn text_layout(
    text: &MenuText,
    family: &str,
    sx: f32,
) -> Option<(Vec<crate::text_block::Piece>, (f32, f32))> {
    if text.text.trim().is_empty() {
        return None;
    }
    let size = f32::from(text.size.clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE));
    let format = text.format;
    let face = text.font_style.as_deref();
    let (pieces, bounds) = if text.vertical {
        crate::text_block::vertical_block(&text.text, family, size, face, &format)
    } else {
        let laid = formatted_layout(&text.text, family, size, face, &format);
        let bounds = laid.bounds;
        (vec![(laid, crate::text_block::IDENTITY)], bounds)
    };
    // The drawn (scaled, turned, squeezed) box starts at the text's x, y.
    let scale = text_scale(&format);
    let (w, h) = oriented_size_of(bounds, sx, text.orientation, scale);
    let center = (f32::from(text.x) + w * 0.5, f32::from(text.y) + h * 0.5);
    let pen = oriented_pen_of(bounds, sx, text.orientation, scale, center);
    let pieces: Vec<crate::text_block::Piece> = pieces
        .into_iter()
        .map(|(laid, xf)| (laid, crate::text_block::compose(&pen, &xf)))
        .collect();
    let finite = pieces
        .iter()
        .all(|(_, xf)| xf.iter().all(|value| value.is_finite()));
    (finite && w.is_finite() && h.is_finite()).then_some((pieces, (w, h)))
}

/// Lays out `text` at `size` in `family` (and `face`, a style of it) with the Character panel's `format`: faux bold
/// / italic, lines, caps, tracking, kerning, baseline shift, superscript / subscript (smaller and moved; the line
/// box stays the full size's), alignment and leading. Scales and anti-aliasing are applied when it is drawn.
fn formatted_layout(
    text: &str,
    family: &str,
    size: f32,
    face: Option<&str>,
    format: &TextFormat,
) -> Arc<Layout> {
    formatted_layout_in(text, family, size, face, format, false)
}

/// [`formatted_layout`], or with `vertical` as a column (characters one under the other, centred on x 0).
pub(crate) fn formatted_layout_in(
    text: &str,
    family: &str,
    size: f32,
    face: Option<&str>,
    format: &TextFormat,
    vertical: bool,
) -> Arc<Layout> {
    let mut style = text_style(family, size);
    if let Some(face) = face.map(str::trim).filter(|face| !face.is_empty()) {
        style.style = face.to_owned();
    }
    style.faux_bold = format.bold;
    style.faux_italic = format.italic;
    style.underline = format.underline;
    style.strikethrough = format.strikethrough;
    style.caps = match format.caps {
        disccraft_project::TextCaps::Normal => Caps::Normal,
        disccraft_project::TextCaps::All => Caps::All,
        disccraft_project::TextCaps::Small => Caps::Small,
    };
    style.tracking = f32::from(format.tracking);
    style.kerning = format.kerning;
    style.baseline_shift = f32::from(format.baseline);
    let script_shift = match format.script {
        TextScript::Normal => None,
        TextScript::Superscript => Some(size * SCRIPT_POSITION),
        TextScript::Subscript => Some(-size * SCRIPT_POSITION),
    };
    let full_style = script_shift.map(|_| style.clone());
    if let Some(shift) = script_shift {
        style.size = size * SCRIPT_SIZE;
        style.baseline_shift += shift;
    }
    let paragraph = ParagraphStyle {
        align: match format.align {
            disccraft_project::TextAlign::Left => filmcraft_text::Align::Left,
            disccraft_project::TextAlign::Center => filmcraft_text::Align::Center,
            disccraft_project::TextAlign::Right => filmcraft_text::Align::Right,
        },
        leading: f32::from(format.leading),
        vertical,
        ..ParagraphStyle::default()
    };
    let mut laid = layout(text, &style, &paragraph);
    if let (Some(full_style), Some(shift)) = (full_style, script_shift) {
        let full = layout(text, &full_style, &paragraph);
        let mut bounds = laid.bounds;
        // The moved small text and the full-size line boxes together.
        bounds[1] = (bounds[1] - shift).min(full.bounds[1]);
        bounds[3] = (bounds[3] - shift).max(full.bounds[3]);
        let mut owned = (*laid).clone();
        owned.bounds = bounds;
        laid = Arc::new(owned);
    }
    laid
}

/// Stretches each picture over its button's video thumbnail area (Encore's `(%)` layer), clipped to the frame.
pub(crate) fn draw_thumbnails(
    menu: &Menu,
    width: u16,
    height: u16,
    rgb: &mut [u8],
    pictures: &[crate::ThumbnailPicture<'_>],
) {
    for picture in pictures {
        let Some(button) = menu
            .buttons
            .get(picture.button)
            .filter(|button| !button.hidden)
        else {
            continue;
        };
        let Some(area) = button.thumbnail_placed() else {
            continue;
        };
        if disccraft_project::normalized_angle(i32::from(button.orientation.angle)) != 0 {
            draw_turned_thumbnail(button, area, (width, height), rgb, picture);
            continue;
        }
        // Inside the frame only (a rectangle from a file may reach far outside it).
        let w = area.w.min(width.saturating_sub(area.x));
        let h = area.h.min(height.saturating_sub(area.y));
        if w == 0 || h == 0 {
            continue;
        }
        let mut scaled = vec![0u8; usize::from(w) * usize::from(h) * 3];
        if !scale_rgb_into(
            picture.rgb,
            picture.width,
            picture.height,
            w,
            h,
            &mut scaled,
        ) {
            continue;
        }
        let row_bytes = usize::from(w) * 3;
        for (row, line) in scaled.chunks_exact(row_bytes).enumerate() {
            let y = usize::from(area.y) + row;
            let start = (y * usize::from(width) + usize::from(area.x)) * 3;
            if let Some(target) = rgb.get_mut(start..start + row_bytes) {
                target.copy_from_slice(line);
            }
        }
    }
}

/// The largest side a turned video thumbnail is scaled to before it is turned.
const MAX_THUMBNAIL_SIDE: u16 = 4096;

/// A video thumbnail turned with its button's content: the picture scaled to the thumbnail's size, then each frame
/// pixel of the turned area's bounds takes the picture pixel it shows (nearest).
fn draw_turned_thumbnail(
    button: &Button,
    area: Rect,
    (width, height): (u16, u16),
    rgb: &mut [u8],
    picture: &crate::ThumbnailPicture<'_>,
) {
    let (w, h) = (
        area.w.min(MAX_THUMBNAIL_SIDE),
        area.h.min(MAX_THUMBNAIL_SIDE),
    );
    if w == 0 || h == 0 {
        return;
    }
    let mut scaled = vec![0u8; usize::from(w) * usize::from(h) * 3];
    if !scale_rgb_into(
        picture.rgb,
        picture.width,
        picture.height,
        w,
        h,
        &mut scaled,
    ) {
        return;
    }
    let pivot = button.content_centre();
    let angle = button.orientation.angle;
    let bounds = disccraft_project::rotated_bounds_about(area, pivot, angle);
    let back = -i32::from(angle);
    for y in bounds.y..bounds.y.saturating_add(bounds.h).min(height) {
        for x in bounds.x..bounds.x.saturating_add(bounds.w).min(width) {
            // The pixel centre turned back about the pivot, into the upright area.
            let (ux, uy) = turned_about(
                (f32::from(x) + 0.5, f32::from(y) + 0.5),
                pivot,
                disccraft_project::normalized_angle(back),
            );
            let (lx, ly) = (ux - f32::from(area.x), uy - f32::from(area.y));
            if lx < 0.0 || ly < 0.0 || lx >= f32::from(area.w) || ly >= f32::from(area.h) {
                continue;
            }
            // Inside the area; `as` truncates the non-negative position.
            let sx = ((lx / f32::from(area.w)) * f32::from(w)) as usize;
            let sy = ((ly / f32::from(area.h)) * f32::from(h)) as usize;
            let from = (sy * usize::from(w) + sx) * 3;
            let to = (usize::from(y) * usize::from(width) + usize::from(x)) * 3;
            if let (Some(source), Some(target)) = (
                scaled.get(from..from + 3).map(<[u8]>::to_vec),
                rgb.get_mut(to..to + 3),
            ) {
                target.copy_from_slice(&source);
            }
        }
    }
}

pub(crate) fn draw_labels(menu: &Menu, width: u16, height: u16, rgb: &mut [u8]) {
    let family = family_for(menu.label_font.as_deref());
    let color = menu.text_color;
    let sx = squeeze(menu);

    for button in menu.buttons.iter().filter(|button| !button.hidden) {
        draw_label(button, &family, sx, color, width, height, rgb);
    }
}

/// Loads a background image. A vector drawing (SVG) is rendered at the frame size, so its edges stay sharp;
/// PNG is read directly; other images (JPEG, PSD, TIFF, ...) go through PhotoCraft.
pub(crate) fn load_image(
    project: &Project,
    asset: AssetId,
    width: u16,
    height: u16,
) -> Result<SourceImage, MenuError> {
    let asset = project
        .assets
        .get(&asset)
        .ok_or_else(|| MenuError::Invalid("background asset does not exist".to_string()))?;

    let path = Path::new(&asset.path);
    if is_png(path) {
        return decode_png(path);
    }
    let image = if disccraft_photo::is_svg(path) {
        disccraft_photo::render_svg_fill(path, u32::from(width), u32::from(height))
    } else {
        disccraft_photo::load_menu_background(path)
    }
    .map_err(|error| MenuError::Invalid(error.to_string()))?;
    rgba_source(image)
}

/// Wraps a decoded RGBA image, checking its size.
fn rgba_source(image: disccraft_photo::Image8) -> Result<SourceImage, MenuError> {
    let width = usize::try_from(image.width)
        .map_err(|_| MenuError::Invalid("image width too large".to_string()))?;
    let height = usize::try_from(image.height)
        .map_err(|_| MenuError::Invalid("image height too large".to_string()))?;
    if width == 0 || height == 0 || width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err(MenuError::Invalid(
            "image dimensions out of range".to_string(),
        ));
    }
    let expected = width.checked_mul(height).and_then(|n| n.checked_mul(4));
    if expected != Some(image.rgba.len()) {
        return Err(MenuError::Invalid("image buffer size mismatch".to_string()));
    }
    Ok(SourceImage {
        data: image.rgba,
        width,
        height,
        channels: 4,
    })
}

fn is_png(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
}

/// A button's subpicture from a PNG, as a PSD's `(=1)` / `(=2)` / `(=3)` layers give it: its size and each pixel's
/// value, 0 (none) for a transparent pixel (alpha under half; with no alpha, black), else 1, 2 or 3 by how light it
/// is (dark, mid, light thirds), which the menu's colour set colours.
///
/// # Errors
///
/// When the PNG cannot be read or is not 8-bit RGB / RGBA.
pub fn subpicture_from_png(path: &Path) -> Result<(u32, u32, Vec<u8>), MenuError> {
    let image = decode_png(path)?;
    let values = image
        .data
        .chunks_exact(image.channels)
        .map(|pixel| {
            let channel = |index: usize| u32::from(pixel.get(index).copied().unwrap_or(0));
            let luma = (299 * channel(0) + 587 * channel(1) + 114 * channel(2)) / 1000;
            let transparent = if image.channels == 4 {
                channel(3) < 128
            } else {
                luma < 43
            };
            match (transparent, luma) {
                (true, _) => 0,
                (false, 0..=84) => 1,
                (false, 85..=169) => 2,
                (false, _) => 3,
            }
        })
        .collect();
    let size = |value: usize| {
        u32::try_from(value).map_err(|_| MenuError::Invalid("png too large".to_string()))
    };
    Ok((size(image.width)?, size(image.height)?, values))
}

fn decode_png(path: &Path) -> Result<SourceImage, MenuError> {
    let png_err =
        |e: &dyn std::fmt::Display| MenuError::Invalid(format!("{}: {e}", path.display()));
    let file = File::open(path).map_err(|e| png_err(&e))?;
    let mut decoder = Decoder::new(file);
    decoder.set_transformations(Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(|e| png_err(&e))?;

    let (color_type, bit_depth) = reader.output_color_type();
    if bit_depth != BitDepth::Eight {
        return Err(MenuError::Invalid("png must be 8-bit".to_string()));
    }

    let channels = match color_type {
        ColorType::Rgb => 3,
        ColorType::Rgba => 4,
        _ => {
            return Err(MenuError::Invalid("unsupported png color type".to_string()));
        }
    };

    let info = reader.info();
    let width = usize::try_from(info.width)
        .map_err(|_| MenuError::Invalid("png width too large".to_string()))?;
    let height = usize::try_from(info.height)
        .map_err(|_| MenuError::Invalid("png height too large".to_string()))?;

    if width == 0 || height == 0 || width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err(MenuError::Invalid(
            "png dimensions out of range".to_string(),
        ));
    }

    let len = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(channels))
        .ok_or_else(|| MenuError::Invalid("png buffer too large".to_string()))?;

    let mut data = vec![0u8; len];
    reader.next_frame(&mut data).map_err(|e| png_err(&e))?;

    Ok(SourceImage {
        data,
        width,
        height,
        channels,
    })
}

fn scale_bilinear(src: &SourceImage, dst_w: u16, dst_h: u16, rgb: &mut [u8]) {
    let dst_w = usize::from(dst_w);
    let dst_h = usize::from(dst_h);

    for dy in 0..dst_h {
        for dx in 0..dst_w {
            let color = bilinear_color(src, dx, dy, dst_w, dst_h);
            set_pixel(rgb, dst_w, dx, dy, color);
        }
    }
}

fn bilinear_color(src: &SourceImage, dx: usize, dy: usize, dst_w: usize, dst_h: usize) -> [u8; 3] {
    let sx = dest_to_source(dx, dst_w, src.width);
    let sy = dest_to_source(dy, dst_h, src.height);

    // sx / sy are clamped to [0, len - 1], so the casts cannot wrap.
    let x0 = sx.floor() as usize;
    let y0 = sy.floor() as usize;
    let x1 = x0.saturating_add(1).min(src.width.saturating_sub(1));
    let y1 = y0.saturating_add(1).min(src.height.saturating_sub(1));

    let fx = sx - x0 as f32;
    let fy = sy - y0 as f32;

    let p00 = sample_pixel(src, x0, y0);
    let p10 = sample_pixel(src, x1, y0);
    let p01 = sample_pixel(src, x0, y1);
    let p11 = sample_pixel(src, x1, y1);

    let top = mix_pixel(p00, p10, fx);
    let bottom = mix_pixel(p01, p11, fx);
    mix_pixel(top, bottom, fy)
}

fn dest_to_source(dest: usize, dst_len: usize, src_len: usize) -> f32 {
    if dst_len == 0 || src_len == 0 {
        return 0.0;
    }

    let dest_f = dest as f32;
    let dst_f = dst_len as f32;
    let src_f = src_len as f32;
    let max = src_len.saturating_sub(1) as f32;

    let coord = (dest_f + 0.5) * src_f / dst_f - 0.5;
    coord.clamp(0.0, max)
}

fn sample_pixel(src: &SourceImage, x: usize, y: usize) -> [u8; 3] {
    if x >= src.width || y >= src.height {
        return [0, 0, 0];
    }

    let Some(row) = y.checked_mul(src.width) else {
        return [0, 0, 0];
    };
    let Some(pixel) = row.checked_add(x) else {
        return [0, 0, 0];
    };
    let Some(idx) = pixel.checked_mul(src.channels) else {
        return [0, 0, 0];
    };
    let Some(end) = idx.checked_add(src.channels) else {
        return [0, 0, 0];
    };
    let Some(data) = src.data.get(idx..end) else {
        return [0, 0, 0];
    };

    let r = data.first().copied().unwrap_or(0);
    let g = data.get(1).copied().unwrap_or(0);
    let b = data.get(2).copied().unwrap_or(0);
    let a = data.get(3).copied().unwrap_or(255);

    let alpha = f32::from(a) / 255.0;

    [
        to_u8(f32::from(r) * alpha),
        to_u8(f32::from(g) * alpha),
        to_u8(f32::from(b) * alpha),
    ]
}

fn mix_pixel(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    let [ar, ag, ab] = a;
    let [br, bg, bb] = b;

    [
        to_u8(f32::from(ar) + (f32::from(br) - f32::from(ar)) * t),
        to_u8(f32::from(ag) + (f32::from(bg) - f32::from(ag)) * t),
        to_u8(f32::from(ab) + (f32::from(bb) - f32::from(ab)) * t),
    ]
}

fn to_u8(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

fn set_pixel(rgb: &mut [u8], width: usize, x: usize, y: usize, color: [u8; 3]) {
    let Some(x_base) = x.checked_mul(3) else {
        return;
    };
    let Some(row) = y.checked_mul(width).and_then(|n| n.checked_mul(3)) else {
        return;
    };
    let Some(base) = row.checked_add(x_base) else {
        return;
    };
    let Some(end) = base.checked_add(3) else {
        return;
    };
    let Some([r, g, b]) = rgb.get_mut(base..end) else {
        return;
    };

    let [cr, cg, cb] = color;
    *r = cr;
    *g = cg;
    *b = cb;
}

/// Scales an RGB24 picture of any size into the frame (`out` is `width * height * 3` bytes). Returns false and
/// leaves `out` alone when the picture's size does not match its bytes or is too large.
pub(crate) fn scale_rgb_into(
    rgb: &[u8],
    picture_width: usize,
    picture_height: usize,
    width: u16,
    height: u16,
    out: &mut [u8],
) -> bool {
    let fits = picture_width > 0
        && picture_height > 0
        && picture_width <= MAX_IMAGE_DIMENSION
        && picture_height <= MAX_IMAGE_DIMENSION
        && picture_width
            .checked_mul(picture_height)
            .and_then(|n| n.checked_mul(3))
            == Some(rgb.len())
        && out.len() == usize::from(width) * usize::from(height) * 3;
    if !fits {
        return false;
    }
    let source = SourceImage {
        data: rgb.to_vec(),
        width: picture_width,
        height: picture_height,
        channels: 3,
    };
    // The picture is a whole menu screen already (a video frame letterboxed to 4:3 with its own pixel shape, as
    // `asset.frame` and the build give it), so it fills the frame.
    scale_bilinear(&source, width, height, out);
    true
}

fn fill_color(rgb: &mut [u8], color: [u8; 3]) {
    let [cr, cg, cb] = color;

    for chunk in rgb.as_chunks_mut::<3>().0 {
        *chunk = [cr, cg, cb];
    }
}

fn draw_label(
    button: &Button,
    family: &str,
    sx: f32,
    color: [u8; 3],
    width: u16,
    height: u16,
    rgb: &mut [u8],
) {
    let Some((laid, pen)) = label_layout(button, family, sx) else {
        return;
    };
    let (mask, mask_x, mask_y) = render::rasterize(&laid, &pen);
    let mask = antialiased(mask, button.label_format.antialias);
    if mask.w == 0 || mask.h == 0 {
        return;
    }

    blend_shadow(
        &mask,
        mask_x,
        mask_y,
        button.shadow.as_ref(),
        rgb,
        width,
        height,
    );
    blend_mask(&mask, mask_x, mask_y, color, rgb, width, height);
}

/// Lays out a button's text, centred in the button (in the part its video thumbnail leaves; `sx`: the horizontal
/// squeeze). `None` for an empty text.
fn label_layout(
    button: &Button,
    family: &str,
    sx: f32,
) -> Option<(Arc<Layout>, filmcraft_text::Xform)> {
    if button.label.is_empty() {
        return None;
    }
    let rect = button.label_rect();
    // The text is sized for the button standing upright, then turned with it.
    let upright = if button.orientation.swaps_sides() {
        Rect {
            w: rect.h,
            h: rect.w,
            ..rect
        }
    } else {
        rect
    };
    // The button's text with its own Character formatting (sized by the button).
    let laid = formatted_layout(
        &button.label,
        family,
        font_size(upright),
        button.label_font_style.as_deref(),
        &button.label_format,
    );
    let pen = oriented_pen(
        &laid,
        sx,
        button.orientation,
        text_scale(&button.label_format),
        label_center(button),
    );
    pen.iter()
        .all(|value| value.is_finite())
        .then_some((laid, pen))
}

/// Where the middle of a button's text is drawn: the middle of its part of the content box, moved by its offset,
/// then turned with the content about the content box's centre.
fn label_center(button: &Button) -> (f32, f32) {
    let rect = button.label_rect();
    let upright = (
        f32::from(rect.x) + f32::from(rect.w) * 0.5 + f32::from(button.label_offset[0]),
        f32::from(rect.y) + f32::from(rect.h) * 0.5 + f32::from(button.label_offset[1]),
    );
    turned_about(upright, button.content_centre(), button.orientation.angle)
}

/// `point` turned by `angle` (tenths of a degree, clockwise on screen) about `pivot`.
pub(crate) fn turned_about(point: (f32, f32), pivot: (f32, f32), angle: i16) -> (f32, f32) {
    if disccraft_project::normalized_angle(i32::from(angle)) == 0 {
        return point;
    }
    let (sin, cos) = (f32::from(angle) / 10.0).to_radians().sin_cos();
    let (dx, dy) = (point.0 - pivot.0, point.1 - pivot.1);
    (pivot.0 + dx * cos - dy * sin, pivot.1 + dx * sin + dy * cos)
}

/// The middle and the drawn size of a button's text (as [`label_layout`] draws it); `None` for no text.
fn label_extent(menu: &Menu, button: &Button) -> Option<((f32, f32), (f32, f32))> {
    if button.label.is_empty() {
        return None;
    }
    let family = family_for(menu.label_font.as_deref());
    let rect = button.label_rect();
    let upright = if button.orientation.swaps_sides() {
        Rect {
            w: rect.h,
            h: rect.w,
            ..rect
        }
    } else {
        rect
    };
    let laid = formatted_layout(
        &button.label,
        &family,
        font_size(upright),
        button.label_font_style.as_deref(),
        &button.label_format,
    );
    let size = oriented_size(
        &laid,
        squeeze(menu),
        button.orientation,
        text_scale(&button.label_format),
    );
    (size.0.is_finite() && size.1.is_finite()).then_some((label_center(button), size))
}

/// The frame rectangle a button's text covers where it is drawn (with its offset, turned with the content);
/// `None` for no text.
pub(crate) fn label_bounds(menu: &Menu, button: &Button) -> Option<Rect> {
    let ((cx, cy), (w, h)) = label_extent(menu, button)?;
    let x0 = (cx - w * 0.5).floor().max(0.0);
    let y0 = (cy - h * 0.5).floor().max(0.0);
    // Finite and clamped at 0; `as` saturates at u16::MAX.
    let upright = Rect {
        x: x0 as u16,
        y: y0 as u16,
        w: to_side(w),
        h: to_side(h),
    };
    Some(disccraft_project::rotated_bounds(
        upright,
        button.orientation.angle,
    ))
}

/// The frame rectangle a button's content covers as it is drawn: its content box, its text and its video
/// thumbnail, each where it is (moved, and turned with the content about the content box's centre), kept inside
/// the frame. Encore's bounds of a button set; the button's rectangle when the content was moved out or turned.
pub(crate) fn button_extent(menu: &Menu, button: &Button, frame: (u16, u16)) -> Rect {
    let pivot = button.content_centre();
    let angle = button.orientation.angle;
    let mut parts = vec![disccraft_project::rotated_bounds(
        button.content_box(),
        angle,
    )];
    parts.extend(label_bounds(menu, button));
    parts.extend(
        button
            .thumbnail_placed()
            .map(|area| disccraft_project::rotated_bounds_about(area, pivot, angle)),
    );
    let x0 = parts.iter().map(|r| u32::from(r.x)).min().unwrap_or(0);
    let y0 = parts.iter().map(|r| u32::from(r.y)).min().unwrap_or(0);
    let x1 = parts
        .iter()
        .map(|r| u32::from(r.x) + u32::from(r.w))
        .max()
        .unwrap_or(1)
        .min(u32::from(frame.0));
    let y1 = parts
        .iter()
        .map(|r| u32::from(r.y) + u32::from(r.h))
        .max()
        .unwrap_or(1)
        .min(u32::from(frame.1));
    let side = |from: u32, to: u32| u16::try_from(to.saturating_sub(from).max(1)).unwrap_or(1);
    Rect {
        x: u16::try_from(x0).unwrap_or(0),
        y: u16::try_from(y0).unwrap_or(0),
        w: side(x0, x1),
        h: side(y0, y1),
    }
}

/// A button's text as a coverage mask in frame pixels (for a text subpicture), with where its corner lands.
pub(crate) fn label_mask(menu: &Menu, button: &Button) -> Option<(Mask, i32, i32)> {
    let family = family_for(menu.label_font.as_deref());
    let sx = squeeze(menu);
    let (laid, pen) = label_layout(button, &family, sx)?;
    let (mask, x, y) = render::rasterize(&laid, &pen);
    let mask = antialiased(mask, button.label_format.antialias);
    (mask.w > 0 && mask.h > 0).then_some((mask, x, y))
}

/// A transparent RGBA canvas with one menu text object drawn in its colour.
pub(crate) fn text_rgba(menu: &Menu, text: &MenuText, width: u16, height: u16) -> Vec<u8> {
    let mut rgba = vec![0u8; usize::from(width) * usize::from(height) * 4];
    let family = family_for(text.font.as_deref());
    let sx = squeeze(menu);
    if let Some((pieces, _)) = text_layout(text, &family, sx) {
        let (mask, mask_x, mask_y) = crate::text_block::rasterize_pieces(&pieces);
        let mask = antialiased(mask, text.format.antialias);
        let color = text.color.unwrap_or(menu.text_color);
        if let Some(shadow) = &text.shadow
            && let Some((shade, sx2, sy2)) =
                crate::shadow::shadow_mask(&mask, mask_x, mask_y, shadow)
        {
            mask_into_rgba(&shade, sx2, sy2, shadow.color, width, height, &mut rgba);
        }
        mask_into_rgba(&mask, mask_x, mask_y, color, width, height, &mut rgba);
    }
    rgba
}

/// A transparent RGBA canvas with a button's text drawn in the menu's text colour.
pub(crate) fn label_rgba(menu: &Menu, button: &Button, width: u16, height: u16) -> Vec<u8> {
    let mut rgba = vec![0u8; usize::from(width) * usize::from(height) * 4];
    let family = family_for(menu.label_font.as_deref());
    let sx = squeeze(menu);
    if let Some((laid, pen)) = label_layout(button, &family, sx) {
        let (mask, mask_x, mask_y) = render::rasterize(&laid, &pen);
        let mask = antialiased(mask, button.label_format.antialias);
        mask_into_rgba(
            &mask,
            mask_x,
            mask_y,
            menu.text_color,
            width,
            height,
            &mut rgba,
        );
    }
    rgba
}

/// Writes a coverage mask into an RGBA canvas as `color` with the coverage as alpha (the larger alpha wins).
fn mask_into_rgba(
    mask: &Mask,
    mask_x: i32,
    mask_y: i32,
    color: [u8; 3],
    width: u16,
    height: u16,
    rgba: &mut [u8],
) {
    let width = i64::from(width);
    let height = i64::from(height);
    for row in 0..mask.h {
        for col in 0..mask.w {
            let Some(coverage) = row
                .checked_mul(mask.w)
                .and_then(|n| n.checked_add(col))
                .and_then(|index| mask.a.get(index))
            else {
                continue;
            };
            let alpha = to_u8(coverage.clamp(0.0, 1.0) * 255.0);
            if alpha == 0 {
                continue;
            }
            let (Ok(col), Ok(row)) = (i64::try_from(col), i64::try_from(row)) else {
                continue;
            };
            let x = i64::from(mask_x).saturating_add(col);
            let y = i64::from(mask_y).saturating_add(row);
            if x < 0 || y < 0 || x >= width || y >= height {
                continue;
            }
            let Some(index) = y
                .checked_mul(width)
                .and_then(|n| n.checked_add(x))
                .and_then(|n| usize::try_from(n).ok())
                .and_then(|n| n.checked_mul(4))
            else {
                continue;
            };
            if let Some(pixel) = rgba.get_mut(index..index + 4)
                && pixel.get(3).is_some_and(|old| *old < alpha)
            {
                pixel.copy_from_slice(&[color[0], color[1], color[2], alpha]);
            }
        }
    }
}

fn font_size(rect: Rect) -> f32 {
    let size = f32::from(rect.h) * 0.5;
    if size.is_finite() && size >= 12.0 {
        size
    } else {
        12.0
    }
}

fn text_style(family: &str, size: f32) -> TextStyle {
    TextStyle {
        family: family.to_string(),
        style: String::from("Regular"),
        size,
        tracking: 0.0,
        kerning: true,
        ligatures: true,
        baseline_shift: 0.0,
        faux_bold: false,
        faux_italic: false,
        caps: Caps::Normal,
        underline: false,
        strikethrough: false,
    }
}

/// Registers the platform's font folders with filmcraft-text once per process
/// (Windows Fonts, macOS Library/Fonts, Linux fontconfig folders).
fn ensure_system_fonts() {
    static ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        filmcraft_text::fonts::add_source(&filmcraft_text::fonts::DirectorySource::system());
    });
}

/// The family to draw with: `requested` when this computer has it, otherwise the default Japanese font.
fn family_for(requested: Option<&str>) -> String {
    if let Some(requested) = requested.map(str::trim).filter(|name| !name.is_empty()) {
        ensure_system_fonts();
        if families().iter().any(|(family, _)| family == requested) {
            return requested.to_owned();
        }
    }
    choose_family()
}

/// The font families on this computer, by name (for a font list in an editor).
pub(crate) fn font_families() -> Vec<String> {
    ensure_system_fonts();
    families().into_iter().map(|(family, _)| family).collect()
}

/// The styles of a family (the default font's when `requested` is `None` or not on this computer), lightest first.
pub(crate) fn font_styles(requested: Option<&str>) -> Vec<String> {
    let family = family_for(requested);
    families()
        .into_iter()
        .find(|(name, _)| *name == family)
        .map(|(_, styles)| styles)
        .unwrap_or_default()
}

fn choose_family() -> String {
    ensure_system_fonts();
    let available = families();

    for candidate in FONT_FAMILIES.iter() {
        if available.iter().any(|(family, _)| family == candidate) {
            return candidate.to_string();
        }
    }

    "Inter".to_string()
}

fn blend_mask(
    mask: &Mask,
    mask_x: i32,
    mask_y: i32,
    color: [u8; 3],
    rgb: &mut [u8],
    width: u16,
    height: u16,
) {
    let width = usize::from(width);
    let height = usize::from(height);
    let width_i = i64::try_from(width).unwrap_or(i64::MAX);
    let height_i = i64::try_from(height).unwrap_or(i64::MAX);

    for row in 0..mask.h {
        for col in 0..mask.w {
            let Some(idx) = row.checked_mul(mask.w).and_then(|n| n.checked_add(col)) else {
                continue;
            };
            let Some(coverage) = mask.a.get(idx) else {
                continue;
            };

            let alpha = coverage.clamp(0.0, 1.0);
            if alpha <= 0.0 {
                continue;
            }

            let Some(col_i) = i64::try_from(col).ok() else {
                continue;
            };
            let Some(row_i) = i64::try_from(row).ok() else {
                continue;
            };

            let dx = i64::from(mask_x).saturating_add(col_i);
            let dy = i64::from(mask_y).saturating_add(row_i);

            if dx < 0 || dx >= width_i || dy < 0 || dy >= height_i {
                continue;
            }

            let Some(x) = usize::try_from(dx).ok() else {
                continue;
            };
            let Some(y) = usize::try_from(dy).ok() else {
                continue;
            };

            blend_pixel(rgb, width, x, y, color, alpha);
        }
    }
}

pub(crate) fn blend_pixel(
    rgb: &mut [u8],
    width: usize,
    x: usize,
    y: usize,
    color: [u8; 3],
    alpha: f32,
) {
    let Some(x_base) = x.checked_mul(3) else {
        return;
    };
    let Some(row) = y.checked_mul(width).and_then(|n| n.checked_mul(3)) else {
        return;
    };
    let Some(base) = row.checked_add(x_base) else {
        return;
    };
    let Some(end) = base.checked_add(3) else {
        return;
    };
    let Some([r, g, b]) = rgb.get_mut(base..end) else {
        return;
    };

    let [cr, cg, cb] = color;
    *r = blend_channel(*r, cr, alpha);
    *g = blend_channel(*g, cg, alpha);
    *b = blend_channel(*b, cb, alpha);
}

fn blend_channel(old: u8, color: u8, alpha: f32) -> u8 {
    if alpha <= 0.0 {
        return old;
    }
    if alpha >= 1.0 {
        return color;
    }

    let old_f = f32::from(old);
    let color_f = f32::from(color);
    let value = old_f + (color_f - old_f) * alpha;

    {
        let _ = old;
        value.round().clamp(0.0, 255.0) as u8
    }
}
