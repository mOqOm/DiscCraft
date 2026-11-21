//! Drawing helpers for menu frames.

use crate::MenuError;
use disccraft_project::{
    AssetId, Button, DropShadow, MAX_TEXT_SCALE, MAX_TEXT_SIZE, MIN_TEXT_SCALE, MIN_TEXT_SIZE,
    Menu, MenuBackground, MenuText, Orientation, Project, Rect, SpanStyle, TextAntialias,
    TextFormat, TextScript,
};
use filmcraft_text::{
    Caps, Layout, Mask, ParagraphStyle, StyleRun, TextStyle, families, layout, layout_rich, render,
};
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
            // A PSD shown by page (M10.27): the layers every page shows.
            let source = match psd_pages(project, menu)? {
                Some(pages) => rgba_source(pages.shared.clone())?,
                None => load_image(project, *asset, width, height)?,
            };
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

/// Draws over the frame what the menu's page of its PSD background draws (M10.27, Encore's `(#)` groups: page 0
/// the top level's buttons, page n the n-th group), stretched as the background is. Nothing for a menu that names
/// no page or whose background is no PSD.
pub(crate) fn draw_psd_page(
    project: &Project,
    menu: &Menu,
    width: u16,
    height: u16,
    rgb: &mut [u8],
) -> Result<(), MenuError> {
    let Some(pages) = psd_pages(project, menu)? else {
        return Ok(());
    };
    let Some(page) = menu
        .psd_page
        .and_then(|page| pages.pages.get(usize::from(page)))
    else {
        return Ok(());
    };
    blend_scaled(&rgba_source(page.clone())?, width, height, rgb);
    Ok(())
}

/// Where the menu's page of its PSD background draws (M10.27), in the frame: the bounds of what the page's own
/// layers cover, as [`draw_psd_page`] stretches them; `None` when it draws nothing.
pub(crate) fn psd_page_rect(
    project: &Project,
    menu: &Menu,
    width: u16,
    height: u16,
) -> Option<Rect> {
    let pages = psd_pages(project, menu).ok().flatten()?;
    let page = pages.pages.get(usize::from(menu.psd_page?))?;
    let (w, h) = (
        usize::try_from(page.width).ok()?,
        usize::try_from(page.height).ok()?,
    );
    let mut bounds: Option<(usize, usize, usize, usize)> = None;
    for (index, pixel) in page.rgba.as_chunks::<4>().0.iter().enumerate() {
        if pixel[3] == 0 {
            continue;
        }
        let (x, y) = (index % w.max(1), index / w.max(1));
        bounds = Some(match bounds {
            None => (x, y, x, y),
            Some((x1, y1, x2, y2)) => (x1.min(x), y1.min(y), x2.max(x), y2.max(y)),
        });
    }
    let (x1, y1, x2, y2) = bounds?;
    // Stretched to the frame, rounded outwards.
    let scale = |value: usize, from: usize, to: u16| -> u64 {
        u64::try_from(value).unwrap_or(u64::MAX) * u64::from(to)
            / u64::try_from(from.max(1)).unwrap_or(1)
    };
    let left = scale(x1, w, width);
    let top = scale(y1, h, height);
    let right = scale(x2 + 1, w, width)
        .saturating_add(1)
        .min(u64::from(width));
    let bottom = scale(y2 + 1, h, height)
        .saturating_add(1)
        .min(u64::from(height));
    Some(Rect {
        x: u16::try_from(left).ok()?,
        y: u16::try_from(top).ok()?,
        w: u16::try_from(right.saturating_sub(left)).ok()?,
        h: u16::try_from(bottom.saturating_sub(top)).ok()?,
    })
}

/// The pictures by page of the menu's PSD background (M10.27), when the menu names a page and its background is a
/// Whether `menu` is made from a layered document (its background a PSD or PhotoCraft document): its buttons are
/// pictures of that document, highlighted only by their own subpicture layers.
pub(crate) fn is_document_menu(project: &Project, menu: &Menu) -> bool {
    let MenuBackground::Image { asset } = &menu.background else {
        return false;
    };
    project
        .assets
        .get(asset)
        .is_some_and(|asset| disccraft_photo::is_document(Path::new(&asset.path)))
}

/// layered document; kept for the last few files (a menu is drawn again and again as it is edited).
fn psd_pages(
    project: &Project,
    menu: &Menu,
) -> Result<Option<Arc<disccraft_photo::PsdPages>>, MenuError> {
    type Kept = Vec<(
        (std::path::PathBuf, u64, Option<std::time::SystemTime>),
        Arc<disccraft_photo::PsdPages>,
    )>;
    static KEPT: std::sync::Mutex<Kept> = std::sync::Mutex::new(Vec::new());
    if menu.psd_page.is_none() {
        return Ok(None);
    }
    let MenuBackground::Image { asset } = &menu.background else {
        return Ok(None);
    };
    let Some(asset) = project.assets.get(asset) else {
        return Ok(None);
    };
    let path = Path::new(&asset.path);
    if !disccraft_photo::is_document(path) {
        return Ok(None);
    }
    let metadata = std::fs::metadata(path).ok();
    let key = (
        path.to_path_buf(),
        metadata.as_ref().map_or(0, std::fs::Metadata::len),
        metadata.and_then(|metadata| metadata.modified().ok()),
    );
    if let Ok(kept) = KEPT.lock()
        && let Some((_, pages)) = kept.iter().find(|(known, _)| *known == key)
    {
        return Ok(Some(Arc::clone(pages)));
    }
    let pages = Arc::new(
        disccraft_photo::load_menu_pages(path)
            .map_err(|error| MenuError::Invalid(error.to_string()))?,
    );
    if let Ok(mut kept) = KEPT.lock() {
        kept.retain(|(known, _)| known.0 != key.0);
        kept.push((key, Arc::clone(&pages)));
        if kept.len() > 4 {
            let _ = kept.remove(0);
        }
    }
    Ok(Some(pages))
}

/// Draws the RGBA picture `src` stretched over the frame onto `rgb` by its opacity (bilinear, on premultiplied
/// colours so transparent pixels do not darken the edges).
fn blend_scaled(src: &SourceImage, dst_w: u16, dst_h: u16, rgb: &mut [u8]) {
    if src.channels != 4 || src.width == 0 || src.height == 0 {
        return;
    }
    let (dst_w, dst_h) = (usize::from(dst_w), usize::from(dst_h));
    let pixel = |x: usize, y: usize| -> [f32; 4] {
        let at = y
            .saturating_mul(src.width)
            .saturating_add(x)
            .saturating_mul(4);
        match src.data.get(at..at + 4) {
            Some(&[r, g, b, a]) => {
                let a = f32::from(a) / 255.0;
                [f32::from(r) * a, f32::from(g) * a, f32::from(b) * a, a]
            }
            _ => [0.0; 4],
        }
    };
    for dy in 0..dst_h {
        let sy = dest_to_source(dy, dst_h, src.height);
        // sy is clamped to [0, height - 1], so the cast cannot wrap.
        let y0 = sy.floor() as usize;
        let y1 = y0.saturating_add(1).min(src.height - 1);
        let fy = sy - y0 as f32;
        for dx in 0..dst_w {
            let sx = dest_to_source(dx, dst_w, src.width);
            let x0 = sx.floor() as usize;
            let x1 = x0.saturating_add(1).min(src.width - 1);
            let fx = sx - x0 as f32;
            let (p00, p10, p01, p11) = (pixel(x0, y0), pixel(x1, y0), pixel(x0, y1), pixel(x1, y1));
            let mut mixed = [0.0_f32; 4];
            for (channel, value) in mixed.iter_mut().enumerate() {
                let top = p00[channel] + (p10[channel] - p00[channel]) * fx;
                let bottom = p01[channel] + (p11[channel] - p01[channel]) * fx;
                *value = top + (bottom - top) * fy;
            }
            let alpha = mixed[3];
            if alpha <= 0.0 {
                continue;
            }
            let at = (dy * dst_w + dx) * 3;
            if let Some(out) = rgb.get_mut(at..at + 3) {
                for (channel, value) in out.iter_mut().enumerate() {
                    let blended = mixed[channel] + f32::from(*value) * (1.0 - alpha);
                    // Clamped to a byte.
                    *value = blended.round().clamp(0.0, 255.0) as u8;
                }
            }
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
        let Some((pieces, _, _, paints)) = text_layout(text, &family, sx) else {
            continue;
        };
        let (mask, mask_x, mask_y) = crate::text_block::rasterize_pieces(&pieces);
        let mask = antialiased(mask, text.format.antialias);
        if mask.w == 0 || mask.h == 0 {
            continue;
        }
        let color = text.color.unwrap_or(menu.text_color);
        let fills = paints.fills(
            &pieces,
            (&mask, mask_x, mask_y),
            color,
            text.format.antialias,
        );
        blend_shadow(
            &mask,
            mask_x,
            mask_y,
            text.shadow.as_ref(),
            rgb,
            width,
            height,
        );
        // Layer effects (M6.342), as an image's: the outer glow under the text, the rest over it.
        blend_effects(
            crate::effects::under(&mask, mask_x, mask_y, &text.effects),
            rgb,
            (width, height),
        );
        for (fill, x, y, color) in &fills {
            blend_mask(fill, *x, *y, *color, rgb, width, height);
        }
        blend_effects(
            crate::effects::over(&mask, mask_x, mask_y, &text.effects),
            rgb,
            (width, height),
        );
    }
}

/// Draws layer effects (each a coverage mask in its colour) over the frame.
fn blend_effects(
    painted: Vec<crate::effects::Painted>,
    rgb: &mut [u8],
    (width, height): (u16, u16),
) {
    for (effect, x, y, color) in painted {
        blend_mask(&effect, x, y, color, rgb, width, height);
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
        Some((_, (w, h), _, _)) => (to_side(w), to_side(h)),
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

/// The parts of a text drawn in colours of their own (M6.389: characters given a colour in the Character panel):
/// byte ranges of its (single) laid-out piece's string with their colours, and the colour of each of its
/// decorations that has one (parallel to the layout's underlines).
#[derive(Default)]
struct Paints {
    /// Byte ranges of the laid-out string and their colours.
    glyphs: Vec<(std::ops::Range<usize>, [u8; 3])>,
    /// The colour of each underline or strikethrough, `None`: the text's.
    decorations: Vec<Option<[u8; 3]>>,
    /// A vertical text's: the colour of each of its pieces, `None`: the text's.
    pieces: Vec<Option<[u8; 3]>>,
}

impl Paints {
    /// The colour of the character at byte `at` of the laid-out string (`None`: the text's).
    fn color_at(&self, at: usize) -> Option<[u8; 3]> {
        self.glyphs
            .iter()
            .find(|(range, _)| range.contains(&at))
            .map(|(_, color)| *color)
    }

    /// What is drawn in each colour: the whole mask in `color` when no character has a colour of its own, else
    /// the characters (and decorations) in the text's colour, then those in each other one, each rasterized from
    /// the single piece.
    fn fills(
        &self,
        pieces: &[crate::text_block::Piece],
        (mask, x, y): (&Mask, i32, i32),
        color: [u8; 3],
        antialias: TextAntialias,
    ) -> Vec<(Mask, i32, i32, [u8; 3])> {
        // A vertical text: its pieces by colour.
        if self.pieces.iter().any(Option::is_some) {
            let mut colors: Vec<Option<[u8; 3]>> = vec![None];
            for own in self.pieces.iter().flatten() {
                if !colors.contains(&Some(*own)) {
                    colors.push(Some(*own));
                }
            }
            return colors
                .into_iter()
                .filter_map(|wanted| {
                    let part: Vec<crate::text_block::Piece> = pieces
                        .iter()
                        .zip(self.pieces.iter().chain(std::iter::repeat(&None)))
                        .filter(|(_, own)| **own == wanted)
                        .map(|(piece, _)| piece.clone())
                        .collect();
                    let (fill, fx, fy) = crate::text_block::rasterize_pieces(&part);
                    let fill = antialiased(fill, antialias);
                    (fill.w > 0 && fill.h > 0).then_some((fill, fx, fy, wanted.unwrap_or(color)))
                })
                .collect();
        }
        let [(laid, xf)] = pieces else {
            return vec![(mask.clone(), x, y, color)];
        };
        if self.glyphs.is_empty() {
            return vec![(mask.clone(), x, y, color)];
        }
        let mut colors: Vec<Option<[u8; 3]>> = vec![None];
        for (_, own) in &self.glyphs {
            if !colors.contains(&Some(*own)) {
                colors.push(Some(*own));
            }
        }
        colors
            .into_iter()
            .filter_map(|wanted| {
                let mut part = (**laid).clone();
                part.glyphs
                    .retain(|glyph| self.color_at(glyph.cluster) == wanted);
                let kept: Vec<bool> = (0..part.underlines.len())
                    .map(|index| self.decorations.get(index).copied().flatten() == wanted)
                    .collect();
                let mut keep = kept.iter();
                part.underlines
                    .retain(|_| keep.next().copied().unwrap_or(wanted.is_none()));
                let mut keep = kept.iter();
                part.underline_runs
                    .retain(|_| keep.next().copied().unwrap_or(wanted.is_none()));
                if part.glyphs.is_empty() && part.underlines.is_empty() {
                    return None;
                }
                let (fill, fx, fy) = render::rasterize(&part, xf);
                let fill = antialiased(fill, antialias);
                (fill.w > 0 && fill.h > 0).then_some((fill, fx, fy, wanted.unwrap_or(color)))
            })
            .collect()
    }
}

/// The family, size, face and format of characters in `span` of a text set in `family`, `size`, `face` and
/// `format` (M6.390: a vertical text's parts are laid out in them).
pub(crate) fn span_setting(
    family: &str,
    (size, face, format): (f32, Option<&str>, &TextFormat),
    span: &SpanStyle,
) -> (String, f32, Option<String>, TextFormat) {
    let family = span
        .font
        .as_deref()
        .map_or_else(|| family.to_owned(), |font| family_for(Some(font)));
    let size = span.size.map_or(size, |own| {
        f32::from(own.clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE))
    });
    let face = span.font_style.clone().or_else(|| face.map(str::to_owned));
    let mut format = *format;
    format.bold = span.bold.unwrap_or(format.bold);
    format.italic = span.italic.unwrap_or(format.italic);
    format.underline = span.underline.unwrap_or(format.underline);
    format.strikethrough = span.strikethrough.unwrap_or(format.strikethrough);
    format.caps = span.caps.unwrap_or(format.caps);
    format.tracking = span.tracking.unwrap_or(format.tracking);
    format.baseline = span.baseline.unwrap_or(format.baseline);
    (family, size, face, format)
}

/// A text's spans as byte ranges of its text (M6.389).
fn byte_spans(text: &MenuText) -> Vec<(std::ops::Range<usize>, SpanStyle)> {
    let mut offsets: Vec<usize> = text.text.char_indices().map(|(at, _)| at).collect();
    offsets.push(text.text.len());
    let last = offsets.len().saturating_sub(1);
    text.spans
        .iter()
        .filter_map(|span| {
            let start = *offsets.get((span.start as usize).min(last))?;
            let end = *offsets.get((span.end as usize).min(last))?;
            (start < end).then(|| (start..end, span.style.clone()))
        })
        .collect()
}

/// The parts of `spans` inside `range`, counted from its start.
fn spans_in(
    spans: &[(std::ops::Range<usize>, SpanStyle)],
    range: &std::ops::Range<usize>,
) -> Vec<(std::ops::Range<usize>, SpanStyle)> {
    spans
        .iter()
        .filter_map(|(span, style)| {
            let start = span.start.max(range.start);
            let end = span.end.min(range.end);
            (start < end).then(|| (start - range.start..end - range.start, style.clone()))
        })
        .collect()
}

/// `spans` (byte ranges of a text) moved onto the text's `lines` (byte ranges of it) laid one after another, each
/// after a line break.
fn spans_on_lines(
    spans: &[(std::ops::Range<usize>, SpanStyle)],
    lines: &[std::ops::Range<usize>],
) -> Vec<(std::ops::Range<usize>, SpanStyle)> {
    let mut moved = Vec::new();
    let mut at = 0_usize;
    for line in lines {
        moved.extend(
            spans_in(spans, line)
                .into_iter()
                .map(|(range, style)| (range.start + at..range.end + at, style)),
        );
        at = at.saturating_add(line.len()).saturating_add(1);
    }
    moved
}

/// The layout of a horizontal text whose characters have formatting of their own (M6.389): its lines wrapped with
/// each line measured as it is set, those past the box's height left out, and the colours of its characters.
/// (laid, overset, paints).
fn rich_layout(
    text: &MenuText,
    family: &str,
    (size, face, format): (f32, Option<&str>, &TextFormat),
    (limit, tall): (Option<f32>, Option<f32>),
) -> (Arc<Layout>, bool, Paints) {
    let spans = byte_spans(text);
    let source = text.text.as_str();
    let measure = |range: std::ops::Range<usize>| {
        let line = source.get(range.clone()).unwrap_or("");
        let laid = formatted_layout_spans(
            line,
            family,
            (size, face, format),
            (false, &[]),
            &spans_in(&spans, &range),
        )
        .0;
        let [x0, _, x1, _] = laid.bounds;
        x1 - x0
    };
    let rules = crate::wrap::Rules::of(format);
    let em = room_em(size, format);
    let (mut lines, mut room, ranges) = match (limit, format.align) {
        (Some(limit), disccraft_project::TextAlign::Justify) => {
            crate::wrap::justify_lines(source, limit, rules, &measure, em)
        }
        (limit, _) => {
            let ranges =
                crate::wrap::wrap_lines(source, limit.unwrap_or(f32::NAN), rules, &measure);
            (crate::wrap::joined(source, &ranges), Vec::new(), ranges)
        }
    };
    let laid_spans = spans_on_lines(&spans, &ranges);
    let mut overset = false;
    if let Some(tall) = tall {
        let high = |lines: &str| {
            let [_, y0, _, y1] = formatted_layout_spans(
                lines,
                family,
                (size, face, format),
                (false, &room),
                &laid_spans,
            )
            .0
            .bounds;
            y1 - y0
        };
        let kept = fitting_lines(&lines, tall, &high);
        if kept < lines.len() {
            overset = true;
            lines.truncate(kept);
            room.retain(|(range, _)| range.end <= kept);
        }
    }
    let (laid, runs) = formatted_layout_spans(
        &lines,
        family,
        (size, face, format),
        (false, &room),
        &laid_spans,
    );
    let glyphs: Vec<(std::ops::Range<usize>, [u8; 3])> = laid_spans
        .iter()
        .filter_map(|(range, style)| Some((range.clone(), style.color?)))
        .collect();
    let paints = Paints {
        decorations: laid
            .underline_runs
            .iter()
            .map(|run| {
                let start = runs.get(usize::from(*run).checked_sub(1)?)?.start;
                glyphs
                    .iter()
                    .find(|(range, _)| range.contains(&start))
                    .map(|(_, color)| *color)
            })
            .collect(),
        glyphs,
        pieces: Vec::new(),
    };
    (laid, overset, paints)
}

/// Lays out a text object: the pieces to draw (each a layout and its transform into the frame; one for a horizontal
/// text), the drawn size of its box, whose top-left corner is the text's x, y before a free turn (`sx`: the
/// horizontal squeeze the text is drawn with), whether lines are left out of its box, and the colours of its
/// characters that have their own.
#[allow(clippy::type_complexity)]
fn text_layout(
    text: &MenuText,
    family: &str,
    sx: f32,
) -> Option<(Vec<crate::text_block::Piece>, (f32, f32), bool, Paints)> {
    if text.text.trim().is_empty() {
        return None;
    }
    let size = f32::from(text.size.clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE));
    let format = text.format;
    let face = text.font_style.as_deref();
    let scale = text_scale(&format);
    // Paragraph text: the box's width (a vertical text's height) in the layout's own units, before the text is
    // scaled and squeezed.
    let limit = |along: f32| {
        text.box_width
            .map(|side| {
                f32::from(side.clamp(
                    disccraft_project::MIN_TEXT_BOX,
                    disccraft_project::MAX_TEXT_BOX,
                )) / along
            })
            .filter(|limit| limit.is_finite() && *limit > 0.0)
    };
    // The box's height (a vertical text's reach across its columns), in the layout's own units.
    let tall = |across: f32| {
        text.box_width
            .and(text.box_height)
            .map(|side| {
                f32::from(side.clamp(
                    disccraft_project::MIN_TEXT_BOX,
                    disccraft_project::MAX_TEXT_BOX,
                )) / across
            })
            .filter(|tall| tall.is_finite() && *tall > 0.0)
    };
    let mut overset = false;
    let mut paints = Paints::default();
    let em = room_em(size, &format);
    let (pieces, bounds) = if text.vertical {
        let rules = crate::wrap::Rules::of(&format);
        // Characters with formatting of their own (M6.389) are measured and set as they are.
        let spans = byte_spans(text);
        let source = text.text.as_str();
        let column_length = |range: std::ops::Range<usize>| {
            let line = source.get(range.clone()).unwrap_or("");
            let [_, y0, _, y1] = crate::text_block::vertical_block_spans(
                line,
                family,
                (size, face, &format),
                &[],
                &spans_in(&spans, &range),
            )
            .1;
            y1 - y0
        };
        let (mut columns, mut room, ranges) = match (limit(scale.1), format.align) {
            // Justified: each column but a paragraph's last spread down to the box.
            (Some(limit), disccraft_project::TextAlign::Justify) => {
                crate::wrap::justify_lines(source, limit, rules, &column_length, em)
            }
            (limit, _) => {
                let ranges = crate::wrap::wrap_lines(
                    source,
                    limit.unwrap_or(f32::NAN),
                    rules,
                    &column_length,
                );
                (crate::wrap::joined(source, &ranges), Vec::new(), ranges)
            }
        };
        let laid_spans = spans_on_lines(&spans, &ranges);
        // Columns past the box's reach are left out.
        if let Some(reach) = tall(scale.0 * sx) {
            let wide = |columns: &str| {
                let [x0, _, x1, _] =
                    crate::text_block::vertical_block(columns, family, size, face, &format).1;
                x1 - x0
            };
            let kept = fitting_lines(&columns, reach, &wide);
            if kept < columns.len() {
                overset = true;
                columns.truncate(kept);
                room.retain(|(range, _)| range.end <= kept);
            }
        }
        let (pieces, [x0, y0, x1, y1], colors) = crate::text_block::vertical_block_spans(
            &columns,
            family,
            (size, face, &format),
            &room,
            &laid_spans,
        );
        paints.pieces = colors;
        // The box keeps the size it was drawn, whatever the columns in it (as a horizontal box does): as long as
        // its columns may be, the columns placed in it by the alignment (top, middle, bottom), and as wide as its
        // reach, the columns from its right edge.
        let (top, bottom) = match limit(scale.1) {
            Some(limit) => {
                let spare = (limit - (y1 - y0)).max(0.0);
                let before = match format.align {
                    disccraft_project::TextAlign::Left | disccraft_project::TextAlign::Justify => {
                        0.0
                    }
                    disccraft_project::TextAlign::Center => spare * 0.5,
                    disccraft_project::TextAlign::Right => spare,
                };
                (y0 - before, y0 - before + limit.max(y1 - y0))
            }
            None => (y0, y1),
        };
        let left = tall(scale.0 * sx).map_or(x0, |reach| x1 - reach.max(x1 - x0));
        (pieces, [left, top, x1, bottom])
    } else {
        let limit = limit(scale.0 * sx);
        let tall = tall(scale.1);
        let measure = |line: &str| {
            let [x0, _, x1, _] = formatted_layout(line, family, size, face, &format).bounds;
            x1 - x0
        };
        let rules = crate::wrap::Rules::of(&format);
        // Characters with formatting of their own (M6.389) are measured and set as they are.
        let rich = (!text.spans.is_empty())
            .then(|| rich_layout(text, family, (size, face, &format), (limit, tall)));
        let mut laid = match rich {
            Some((laid, cut, own)) => {
                overset = cut;
                paints = own;
                laid
            }
            None => {
                let (mut lines, mut room) = match (limit, format.align) {
                    // Justified: each line but a paragraph's last spread to the box.
                    (Some(limit), disccraft_project::TextAlign::Justify) => {
                        crate::wrap::justify(&text.text, limit, rules, &measure, em)
                    }
                    _ => (
                        limit.map_or_else(
                            || text.text.clone(),
                            |limit| crate::wrap::wrap(&text.text, limit, rules, &measure),
                        ),
                        Vec::new(),
                    ),
                };
                // Lines below the box's height are left out (Encore's overset text).
                if let Some(tall) = tall {
                    let high = |lines: &str| {
                        let [_, y0, _, y1] = formatted_layout_with(
                            lines,
                            family,
                            size,
                            face,
                            &format,
                            (false, &room),
                        )
                        .bounds;
                        y1 - y0
                    };
                    let kept = fitting_lines(&lines, tall, &high);
                    if kept < lines.len() {
                        overset = true;
                        lines.truncate(kept);
                        room.retain(|(range, _)| range.end <= kept);
                    }
                }
                formatted_layout_with(&lines, family, size, face, &format, (false, &room))
            }
        };
        // The box is as wide as it was drawn, whatever the lines in it: they are aligned inside it.
        if let Some(limit) = limit {
            let [x0, y0, x1, y1] = laid.bounds;
            let spare = (limit - (x1 - x0)).max(0.0);
            let before = match format.align {
                disccraft_project::TextAlign::Left | disccraft_project::TextAlign::Justify => 0.0,
                disccraft_project::TextAlign::Center => spare * 0.5,
                disccraft_project::TextAlign::Right => spare,
            };
            // As tall as its box when it has a height, whatever the lines in it.
            let bottom = tall.map_or(y1, |tall| y0 + tall);
            let mut owned = (*laid).clone();
            owned.bounds = [x0 - before, y0, x0 - before + limit.max(x1 - x0), bottom];
            laid = Arc::new(owned);
        }
        let bounds = laid.bounds;
        (vec![(laid, crate::text_block::IDENTITY)], bounds)
    };
    // The drawn (scaled, turned, squeezed) box starts at the text's x, y.
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
    (finite && w.is_finite() && h.is_finite()).then_some((pieces, (w, h), overset, paints))
}

/// How much of `lines` (whole lines, a byte length) fits in `room`, by `extent` (the size of some lines): all of
/// it when it fits, else the most lines that do (none at all when even the first does not).
fn fitting_lines(lines: &str, room: f32, extent: &dyn Fn(&str) -> f32) -> usize {
    if extent(lines) <= room {
        return lines.len();
    }
    // The ends of the first 1, 2, … lines (not the last: all of them do not fit).
    let ends: Vec<usize> = lines.match_indices('\n').map(|(at, _)| at).collect();
    let (mut fits, mut over) = (0_usize, ends.len().saturating_add(1));
    while fits + 1 < over {
        let middle = fits + (over - fits) / 2;
        let end = ends.get(middle - 1).copied().unwrap_or(lines.len());
        if lines.get(..end).is_some_and(|head| extent(head) <= room) {
            fits = middle;
        } else {
            over = middle;
        }
    }
    match fits.checked_sub(1) {
        Some(last) => ends.get(last).copied().unwrap_or(0),
        None => 0,
    }
}

/// The em a justified line's room is counted in (as tracking is): the size the text is set in (smaller for
/// superscript and subscript).
pub(crate) fn room_em(size: f32, format: &TextFormat) -> f32 {
    if format.script == TextScript::Normal {
        size
    } else {
        size * SCRIPT_SIZE
    }
}

/// Whether a paragraph text has lines (columns) its box's height leaves out (Encore's overset text).
pub(crate) fn text_overset(menu: &Menu, text: &MenuText) -> bool {
    let family = family_for(text.font.as_deref());
    text_layout(text, &family, squeeze(menu)).is_some_and(|(_, _, overset, _)| overset)
}

/// How much of an em (in 1/1000) tsume at 100 per cent takes off the room after a kana or a kanji: about the
/// blank a Japanese font leaves at the two sides of such a character.
const TSUME_BODY: f32 = 100.0;
/// The blank half of a full-width punctuation mark, 1/1000 em.
const PUNCTUATION_BLANK: f32 = 500.0;

/// A full-width punctuation mark whose blank half is at its right (it closes, or ends a clause).
fn closes(c: char) -> bool {
    matches!(
        c,
        '、' | '。' | '，' | '．' | '」' | '』' | '）' | '】' | '〕' | '｝' | '〉' | '》'
    )
}

/// A full-width bracket whose blank half is at its left (it opens).
fn opens(c: char) -> bool {
    matches!(c, '「' | '『' | '（' | '【' | '〔' | '｛' | '〈' | '《')
}

/// A kana, a kanji or a full-width form: a character set in a full square.
fn full_width(c: char) -> bool {
    matches!(
        u32::from(c),
        0x3041..=0x309F | 0x30A0..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xFF01..=0xFF60
    )
}

/// The blanks the font of `style` leaves at the left and the right of `c` (1/1000 em), measured from its glyph
/// (M6.345): from the origin to where its outline starts, and from where it ends to the advance. `None` when the
/// font has no glyph with an outline for it. Kept per font and character.
pub(crate) fn side_blanks(style: &TextStyle, c: char) -> Option<(f32, f32)> {
    type Key = (String, String, bool, char);
    static MEASURED: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<Key, Option<(f32, f32)>>>,
    > = std::sync::OnceLock::new();
    let cache = MEASURED.get_or_init(Default::default);
    let key = (
        style.family.clone(),
        style.style.clone(),
        style.faux_bold,
        c,
    );
    if let Some(found) = cache.lock().ok().and_then(|cache| cache.get(&key).copied()) {
        return found;
    }
    // At 1000 pixels an em, a pixel is 1/1000 em.
    let probe = TextStyle {
        size: 1000.0,
        tracking: 0.0,
        kerning: false,
        baseline_shift: 0.0,
        faux_italic: false,
        underline: false,
        strikethrough: false,
        caps: Caps::Normal,
        ..style.clone()
    };
    let laid = layout(&c.to_string(), &probe, &ParagraphStyle::default());
    let measured = (|| {
        let glyph = laid.glyphs.first()?;
        let line = laid.lines.first()?;
        let advance = line.carets.last()?.1 - line.carets.first()?.1;
        let (x0, _, x1, _) =
            filmcraft_text::render::glyph_path(glyph, &[1.0, 0.0, 0.0, 1.0, 0.0, 0.0]).bounds()?;
        (advance > 0.0).then(|| (x0.clamp(0.0, advance), (advance - x1).clamp(0.0, advance)))
    })();
    if let Ok(mut cache) = cache.lock()
        && cache.len() < MAX_MEASURED_GLYPHS
    {
        let _ = cache.insert(key, measured);
    }
    measured
}

/// The most glyphs [`side_blanks`] keeps measured.
const MAX_MEASURED_GLYPHS: usize = 20_000;

/// The characters of `text` that are set closer to what follows them, and by how much (1/1000 em), for
/// `format`'s tsume and mojikumi. Empty when neither asks for anything.
///
/// Mojikumi: a punctuation mark at half width gives up its blank half ([`PUNCTUATION_BLANK`]): a closing one the
/// room after it, an opening one the room after the character before it (none at the head of a line). Tsume:
/// its percentage of the blank of a full-width character: for a kana or kanji the blank after it and the blank
/// before the kana or kanji that follows; for a punctuation mark that mojikumi left at full width its blank side.
/// `blanks` measures them glyph by glyph (M6.345, [`side_blanks`]); without a measure (`None`) a kana or kanji
/// has [`TSUME_BODY`] and a punctuation mark [`PUNCTUATION_BLANK`], a Japanese font's usual ones.
pub(crate) fn japanese_spacing(
    text: &str,
    format: &TextFormat,
    blanks: &dyn Fn(char) -> Option<(f32, f32)>,
) -> Vec<(std::ops::Range<usize>, f32)> {
    use disccraft_project::TextMojikumi;
    let tsume = f32::from(format.tsume.min(disccraft_project::MAX_TSUME)) / 100.0;
    let (every, line_end) = match format.mojikumi {
        TextMojikumi::PunctuationHalf => (true, true),
        TextMojikumi::LineEndHalf => (false, true),
        TextMojikumi::None | TextMojikumi::LineEndFull | TextMojikumi::PunctuationFull => {
            (false, false)
        }
    };
    if tsume <= 0.0 && !line_end {
        return Vec::new();
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    // What each character gives up of the room after it.
    let mut less = vec![0.0_f32; chars.len()];
    for (at, (_, c)) in chars.iter().enumerate() {
        let next = chars.get(at + 1).map(|(_, next)| *next);
        let ends_line = next.is_none_or(|next| next == '\n');
        if closes(*c) {
            let half = every || (line_end && ends_line);
            if let Some(slot) = less.get_mut(at) {
                *slot = if half {
                    PUNCTUATION_BLANK
                } else {
                    blanks(*c).map_or(PUNCTUATION_BLANK, |(_, right)| right) * tsume
                };
            }
        } else if opens(*c) {
            // Its blank is before it: the character before gives the room up.
            let before = at
                .checked_sub(1)
                .filter(|before| chars.get(*before).is_some_and(|(_, c)| *c != '\n'));
            if let Some(slot) = before.and_then(|before| less.get_mut(before)) {
                *slot += if every {
                    PUNCTUATION_BLANK
                } else {
                    blanks(*c).map_or(PUNCTUATION_BLANK, |(left, _)| left) * tsume
                };
            }
        } else if full_width(*c)
            && let Some(slot) = less.get_mut(at)
        {
            // The blank after it, and the one before a kana or kanji that follows it on the line.
            let measured = blanks(*c).map(|(_, right)| {
                let following = next
                    .filter(|next| full_width(*next) && !closes(*next) && !opens(*next))
                    .and_then(blanks)
                    .map_or(0.0, |(left, _)| left);
                right + following
            });
            *slot += measured.unwrap_or(TSUME_BODY) * tsume;
        }
    }
    chars
        .iter()
        .zip(less)
        .filter(|(_, less)| *less > 0.0)
        .map(|((offset, c), less)| (*offset..offset + c.len_utf8(), less))
        .collect()
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
    formatted_layout_with(text, family, size, face, format, (vertical, &[]))
}

/// [`formatted_layout_in`] with `room` put after some characters (byte ranges of `text`, 1/1000 em, as tracking
/// is): a justified paragraph text's lines.
pub(crate) fn formatted_layout_with(
    text: &str,
    family: &str,
    size: f32,
    face: Option<&str>,
    format: &TextFormat,
    (vertical, room): (bool, &[(std::ops::Range<usize>, f32)]),
) -> Arc<Layout> {
    formatted_layout_spans(text, family, (size, face, format), (vertical, room), &[]).0
}

/// [`formatted_layout_with`] with characters set in formatting of their own (`spans`: byte ranges of `text` and
/// what they set over `format`, M6.389). Also the byte range of each style run the layout was given (a glyph's or
/// a decoration's run `i + 1` is the `i`th).
#[allow(clippy::type_complexity)]
fn formatted_layout_spans(
    text: &str,
    family: &str,
    (size, face, format): (f32, Option<&str>, &TextFormat),
    (vertical, room): (bool, &[(std::ops::Range<usize>, f32)]),
    spans: &[(std::ops::Range<usize>, SpanStyle)],
) -> (Arc<Layout>, Vec<std::ops::Range<usize>>) {
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
    // Encore's カーニング as a number (M6.344): with metric kerning off, the same change between every two
    // characters, on top of the tracking.
    let manual = if format.kerning {
        0.0
    } else {
        f32::from(format.manual_kerning)
    };
    style.tracking = f32::from(format.tracking) + manual;
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
            disccraft_project::TextAlign::Left | disccraft_project::TextAlign::Justify => {
                filmcraft_text::Align::Left
            }
            disccraft_project::TextAlign::Center => filmcraft_text::Align::Center,
            disccraft_project::TextAlign::Right => filmcraft_text::Align::Right,
        },
        vertical,
        ..ParagraphStyle::default()
    };
    // Encore's 行送り: the baselines `pitch` apart, whatever the font's own line height (set per style below).
    let pitch = format.line_pitch_at(size);
    // Tsume and mojikumi: Japanese characters set closer than the font sets them (tsume by each glyph's own
    // blanks across a line, M6.345); a justified line's room spreads them again.
    let measure = |c: char| side_blanks(&style, c);
    let fixed = |_: char| None;
    let blanks: &dyn Fn(char) -> Option<(f32, f32)> = if vertical { &fixed } else { &measure };
    let mut spacing = japanese_spacing(text, format, blanks);
    for (range, more) in room {
        match spacing.iter_mut().find(|(at, _)| at.start == range.start) {
            Some((_, less)) => *less -= *more,
            None => spacing.push((range.clone(), -*more)),
        }
    }
    spacing.retain(|(_, less)| *less != 0.0);
    // A span's characters in the text's style with the span's fields over it.
    let own = |base: &TextStyle, span: &SpanStyle| {
        let mut set = base.clone();
        if let Some(font) = span.font.as_deref() {
            set.family = family_for(Some(font));
        }
        if let Some(face) = span
            .font_style
            .as_deref()
            .map(str::trim)
            .filter(|face| !face.is_empty())
        {
            set.style = face.to_owned();
        }
        if let Some(bold) = span.bold {
            set.faux_bold = bold;
        }
        if let Some(italic) = span.italic {
            set.faux_italic = italic;
        }
        if let Some(underline) = span.underline {
            set.underline = underline;
        }
        if let Some(strikethrough) = span.strikethrough {
            set.strikethrough = strikethrough;
        }
        if let Some(caps) = span.caps {
            set.caps = match caps {
                disccraft_project::TextCaps::Normal => Caps::Normal,
                disccraft_project::TextCaps::All => Caps::All,
                disccraft_project::TextCaps::Small => Caps::Small,
            };
        }
        if let Some(tracking) = span.tracking {
            set.tracking = f32::from(tracking) + manual;
        }
        // Its own size and baseline shift, made smaller and moved as the text's are for superscript / subscript.
        let own_size = span.size.map_or(size, |own| {
            f32::from(own.clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE))
        });
        let own_shift = f32::from(span.baseline.unwrap_or(format.baseline));
        let scale = base.size / size.max(1.0);
        set.size = own_size * scale;
        set.baseline_shift = match format.script {
            TextScript::Normal => own_shift,
            TextScript::Superscript => own_shift + own_size * SCRIPT_POSITION,
            TextScript::Subscript => own_shift - own_size * SCRIPT_POSITION,
        };
        set
    };
    let lay = |style: &TextStyle| {
        let mut paragraph = paragraph.clone();
        // (A column of a vertical text is one line: its columns are placed by the caller.)
        if !vertical {
            paragraph.leading = pitch - own_line_height(style);
        }
        if spacing.is_empty() && spans.is_empty() {
            return (layout(text, style, &paragraph), Vec::new());
        }
        // The spans' characters in their own style, then each character set closer or spread (in its span's
        // style), which wins where they meet.
        let mut runs: Vec<StyleRun> = spans
            .iter()
            .map(|(range, span)| StyleRun {
                range: range.clone(),
                style: own(style, span),
            })
            .collect();
        runs.extend(spacing.iter().map(|(range, less)| {
            let mut tighter = spans
                .iter()
                .find(|(span, _)| span.contains(&range.start))
                .map_or_else(|| style.clone(), |(_, span)| own(style, span));
            tighter.tracking -= *less;
            StyleRun {
                range: range.clone(),
                style: tighter,
            }
        }));
        let ranges = runs.iter().map(|run| run.range.clone()).collect();
        (layout_rich(text, style, &runs, &paragraph), ranges)
    };
    let (mut laid, runs) = lay(&style);
    if let (Some(full_style), Some(shift)) = (full_style, script_shift) {
        let full = lay(&full_style).0;
        let mut bounds = laid.bounds;
        // The moved small text and the full-size line boxes together.
        bounds[1] = (bounds[1] - shift).min(full.bounds[1]);
        bounds[3] = (bounds[3] - shift).max(full.bounds[3]);
        let mut owned = (*laid).clone();
        owned.bounds = bounds;
        laid = Arc::new(owned);
    }
    (laid, runs)
}

/// How far apart the layout puts two lines of `style` with no leading: the font's own line height.
fn own_line_height(style: &TextStyle) -> f32 {
    let probe = layout("X\nX", style, &ParagraphStyle::default());
    match (probe.lines.first(), probe.lines.get(1)) {
        (Some(first), Some(second)) => second.baseline - first.baseline,
        _ => style.size * disccraft_project::AUTO_LEADING,
    }
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
    let mut image = if is_png(path) {
        decode_png(path)?
    } else {
        rgba_source(
            if disccraft_photo::is_svg(path) {
                disccraft_photo::render_svg_fill(path, u32::from(width), u32::from(height))
            } else {
                disccraft_photo::load_menu_background(path)
            }
            .map_err(|error| MenuError::Invalid(error.to_string()))?,
        )?
    };
    // Interpret Footage ▸ Ignore Alpha Channel: every pixel opaque.
    if asset.ignore_alpha && image.channels == 4 {
        for pixel in image.data.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
    }
    Ok(image)
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
    let mut decoder = Decoder::new(std::io::BufReader::new(file));
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
    // Layer effects of the button's text (M6.342).
    blend_effects(
        crate::effects::under(&mask, mask_x, mask_y, &button.effects),
        rgb,
        (width, height),
    );
    blend_mask(&mask, mask_x, mask_y, color, rgb, width, height);
    blend_effects(
        crate::effects::over(&mask, mask_x, mask_y, &button.effects),
        rgb,
        (width, height),
    );
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
    // A subpicture that reaches past the content box (a pasted layer, M6.329) widens the button.
    parts.extend(
        button
            .subpicture
            .as_ref()
            .filter(|sub| sub.area.is_some())
            .map(|sub| {
                disccraft_project::rotated_bounds_about(
                    sub.rect_in(button.content_box()),
                    pivot,
                    angle,
                )
            }),
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

/// What Encore's Create Subpicture makes of a button that has no artwork in a PSD (M6.330): its content as it is
/// drawn before it is turned — the text where it covers a pixel by half or more, and the video thumbnail — over the
/// smallest frame rectangle holding them and its content box (the text may be moved out of the box). `None` when
/// the button shows neither.
pub(crate) fn own_subpicture(menu: &Menu, button: &Button) -> Option<(Rect, Vec<u8>)> {
    // Upright: the subpicture turns with the content when it is drawn (a mirror stays, as the text has it).
    let mut upright = button.clone();
    upright.orientation.angle = 0;
    let content = upright.content_box();
    let text = label_mask(menu, &upright);
    let thumbnail = upright.thumbnail_placed();
    if text.is_none() && thumbnail.is_none() {
        return None;
    }
    // The area: the content box, the text's mask and the thumbnail, inside the frame's coordinates.
    let mut x0 = i64::from(content.x);
    let mut y0 = i64::from(content.y);
    let mut x1 = x0 + i64::from(content.w);
    let mut y1 = y0 + i64::from(content.h);
    if let Some((mask, mx, my)) = &text {
        x0 = x0.min(i64::from(*mx));
        y0 = y0.min(i64::from(*my));
        x1 = x1.max(i64::from(*mx) + i64::try_from(mask.w).ok()?);
        y1 = y1.max(i64::from(*my) + i64::try_from(mask.h).ok()?);
    }
    if let Some(area) = thumbnail {
        x0 = x0.min(i64::from(area.x));
        y0 = y0.min(i64::from(area.y));
        x1 = x1.max(i64::from(area.x) + i64::from(area.w));
        y1 = y1.max(i64::from(area.y) + i64::from(area.h));
    }
    let (x0, y0) = (x0.max(0), y0.max(0));
    let rect = Rect {
        x: u16::try_from(x0).ok()?,
        y: u16::try_from(y0).ok()?,
        w: u16::try_from(x1 - x0).ok()?,
        h: u16::try_from(y1 - y0).ok()?,
    };
    let (w, h) = (usize::from(rect.w), usize::from(rect.h));
    let mut values = vec![0_u8; w.checked_mul(h)?];
    let mut set = |fx: i64, fy: i64| {
        let (Ok(col), Ok(row)) = (usize::try_from(fx - x0), usize::try_from(fy - y0)) else {
            return;
        };
        if col < w
            && let Some(value) = values.get_mut(row * w + col)
        {
            *value = 1;
        }
    };
    if let Some((mask, mx, my)) = &text {
        for row in 0..mask.h {
            for col in 0..mask.w {
                if mask
                    .a
                    .get(row * mask.w + col)
                    .is_some_and(|cover| *cover >= 0.5)
                {
                    set(
                        i64::from(*mx) + i64::try_from(col).ok()?,
                        i64::from(*my) + i64::try_from(row).ok()?,
                    );
                }
            }
        }
    }
    if let Some(area) = thumbnail {
        for fy in i64::from(area.y)..i64::from(area.y) + i64::from(area.h) {
            for fx in i64::from(area.x)..i64::from(area.x) + i64::from(area.w) {
                set(fx, fy);
            }
        }
    }
    values
        .iter()
        .any(|value| *value != 0)
        .then_some((rect, values))
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
    if let Some((pieces, _, _, paints)) = text_layout(text, &family, sx) {
        let (mask, mask_x, mask_y) = crate::text_block::rasterize_pieces(&pieces);
        let mask = antialiased(mask, text.format.antialias);
        let color = text.color.unwrap_or(menu.text_color);
        let fills = paints.fills(
            &pieces,
            (&mask, mask_x, mask_y),
            color,
            text.format.antialias,
        );
        if let Some(shadow) = &text.shadow
            && let Some((shade, sx2, sy2)) =
                crate::shadow::shadow_mask(&mask, mask_x, mask_y, shadow)
        {
            mask_into_rgba(&shade, sx2, sy2, shadow.color, width, height, &mut rgba);
        }
        let under = crate::effects::under(&mask, mask_x, mask_y, &text.effects);
        let over = crate::effects::over(&mask, mask_x, mask_y, &text.effects);
        if under.is_empty() && over.is_empty() && fills.len() == 1 {
            mask_into_rgba(&mask, mask_x, mask_y, color, width, height, &mut rgba);
        } else {
            // With layer effects (M6.342), or characters in colours of their own (M6.389), each is laid over what
            // is below it.
            for (effect, ex, ey, effect_color) in under {
                mask_over_rgba(&effect, (ex, ey), effect_color, (width, height), &mut rgba);
            }
            for (fill, x, y, color) in &fills {
                mask_over_rgba(fill, (*x, *y), *color, (width, height), &mut rgba);
            }
            for (effect, ex, ey, effect_color) in over {
                mask_over_rgba(&effect, (ex, ey), effect_color, (width, height), &mut rgba);
            }
        }
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

/// Lays a coverage mask over an RGBA canvas as `color` (straight alpha, source over), as one layer of a text over
/// another (its layer effects, M6.342).
fn mask_over_rgba(
    mask: &Mask,
    (mask_x, mask_y): (i32, i32),
    color: [u8; 3],
    (width, height): (u16, u16),
    rgba: &mut [u8],
) {
    let (width, height) = (i64::from(width), i64::from(height));
    for (index, coverage) in mask.a.iter().enumerate() {
        let top = u32::from(to_u8(coverage.clamp(0.0, 1.0) * 255.0));
        if top == 0 || mask.w == 0 {
            continue;
        }
        let (Ok(col), Ok(row)) = (i64::try_from(index % mask.w), i64::try_from(index / mask.w))
        else {
            continue;
        };
        let x = i64::from(mask_x).saturating_add(col);
        let y = i64::from(mask_y).saturating_add(row);
        if x < 0 || y < 0 || x >= width || y >= height {
            continue;
        }
        let Some(at) = y
            .checked_mul(width)
            .and_then(|n| n.checked_add(x))
            .and_then(|n| usize::try_from(n).ok())
            .and_then(|n| n.checked_mul(4))
        else {
            continue;
        };
        let Some(pixel) = rgba.get_mut(at..at + 4) else {
            continue;
        };
        let under = u32::from(pixel[3]);
        // Both alphas times 255.
        let alpha = top * 255 + under * (255 - top);
        for (channel, value) in pixel.iter_mut().take(3).zip(color) {
            let mixed = u32::from(value) * top * 255 + u32::from(*channel) * under * (255 - top);
            *channel = u8::try_from((mixed + alpha / 2) / alpha.max(1)).unwrap_or(u8::MAX);
        }
        pixel[3] = u8::try_from((alpha + 127) / 255).unwrap_or(u8::MAX);
    }
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

#[cfg(test)]
mod spacing_tests {
    use super::{formatted_layout, japanese_spacing, side_blanks, text_style};
    use disccraft_project::{TextFormat, TextMojikumi, TextScript};

    fn format(tsume: u8, mojikumi: TextMojikumi) -> TextFormat {
        TextFormat {
            tsume,
            mojikumi,
            ..TextFormat::default()
        }
    }

    /// The characters that give room up, by their place in the text (in characters), and how much.
    fn less(text: &str, format: &TextFormat) -> Vec<(usize, f32)> {
        japanese_spacing(text, format, &|_| None)
            .into_iter()
            .map(|(range, less)| (text[..range.start].chars().count(), less))
            .collect()
    }

    #[test]
    fn tsume_and_mojikumi_close_up_japanese_characters() {
        // Nothing asked: nothing changes, whatever the text.
        assert!(japanese_spacing("あ。「い」ABC", &TextFormat::default(), &|_| None).is_empty());
        assert!(
            japanese_spacing("あ。", &format(0, TextMojikumi::PunctuationFull), &|_| {
                None
            })
            .is_empty()
        );
        // Tsume: its share of a kana's or kanji's blank, and of a punctuation mark's blank half; Latin letters
        // are left alone.
        assert_eq!(
            less("あA漢。", &format(50, TextMojikumi::None)),
            [(0, 50.0), (2, 50.0), (3, 250.0)]
        );
        // Half-width punctuation: a closing mark gives up the room after it, an opening one the room after the
        // character before it; at the head of a line there is none before it.
        assert_eq!(
            less(
                "「あ」い（う）。",
                &format(0, TextMojikumi::PunctuationHalf)
            ),
            [(2, 500.0), (3, 500.0), (6, 500.0), (7, 500.0)]
        );
        // At line ends only: the mark that ends a line, not one inside it.
        assert_eq!(
            less(
                "あ、い。
う。え",
                &format(0, TextMojikumi::LineEndHalf)
            ),
            [(3, 500.0)]
        );
        assert_eq!(
            less("あ。", &format(0, TextMojikumi::LineEndHalf)),
            [(1, 500.0)]
        );
    }

    /// M6.344: Encore's カーニング as a number changes the room between the characters: 200 (1/1000 em) makes a line
    /// wider, -100 narrower; with metric kerning on the number is not used.
    #[test]
    fn a_kerning_number_spaces_the_characters() {
        let width = |kerning: bool, manual_kerning: i16| {
            let format = TextFormat {
                kerning,
                manual_kerning,
                ..TextFormat::default()
            };
            let [x0, _, x1, _] = formatted_layout("ABCD", "", 40.0, None, &format).bounds;
            x1 - x0
        };
        let none = width(false, 0);
        assert!(
            width(false, 200) > none + 20.0,
            "{} {none}",
            width(false, 200)
        );
        assert!(
            width(false, -100) < none - 10.0,
            "{} {none}",
            width(false, -100)
        );
        assert!((width(true, 200) - width(true, 0)).abs() < 0.5);
    }

    /// M6.297: a justified line laid out with the room it was given reaches the box (a full line, so the
    /// characters are spread by tracking alone).
    #[test]
    fn a_justified_line_reaches_the_box() {
        let format = TextFormat {
            align: disccraft_project::TextAlign::Justify,
            ..TextFormat::default()
        };
        let measure = |line: &str| {
            let [x0, _, x1, _] = formatted_layout(line, "", 40.0, None, &format).bounds;
            x1 - x0
        };
        // Room for five characters and most of a sixth.
        let five = measure("あいうえお");
        let limit = five + (measure("あいうえおか") - five) * 0.6;
        let rules = crate::wrap::Rules::of(&format);
        let (lines, room) =
            crate::wrap::justify("あいうえおかきくけこさ", limit, rules, &measure, 40.0);
        let first = lines.split('\n').next().unwrap_or_default();
        assert_eq!(first, "あいうえお");
        let mine: Vec<_> = room
            .into_iter()
            .filter(|(range, _)| range.end <= first.len())
            .collect();
        assert_eq!(mine.len(), 4);
        let [x0, _, x1, _] =
            super::formatted_layout_with(first, "", 40.0, None, &format, (false, &mine)).bounds;
        assert!(((x1 - x0) - limit).abs() < 0.5, "{} {limit}", x1 - x0);
    }

    /// M6.302: a justified column of vertical text laid out with its room reaches the box's height.
    #[test]
    fn a_justified_column_reaches_the_box() {
        let format = TextFormat {
            align: disccraft_project::TextAlign::Justify,
            ..TextFormat::default()
        };
        let tall = |line: &str| {
            let [_, y0, _, y1] = crate::text_block::vertical_block(line, "", 40.0, None, &format).1;
            y1 - y0
        };
        let five = tall("あいうえお");
        let limit = five + (tall("あいうえおか") - five) * 0.6;
        let rules = crate::wrap::Rules::of(&format);
        let (lines, room) =
            crate::wrap::justify("あいうえおかきくけこさ", limit, rules, &tall, 40.0);
        let first = lines.split('\n').next().unwrap_or_default();
        assert_eq!(first, "あいうえお");
        let mine: Vec<_> = room
            .into_iter()
            .filter(|(range, _)| range.end <= first.len())
            .collect();
        let [_, y0, _, y1] =
            crate::text_block::vertical_block_with(first, "", 40.0, None, &format, &mine).1;
        assert!(((y1 - y0) - limit).abs() < 0.5, "{} {limit}", y1 - y0);
    }

    #[test]
    fn a_line_set_closer_is_narrower() {
        let width = |format: &TextFormat| {
            let laid = formatted_layout("あいう。", "", 40.0, None, format);
            laid.bounds[2] - laid.bounds[0]
        };
        let plain = width(&TextFormat::default());
        let tsume = width(&format(100, TextMojikumi::None));
        let half = width(&format(0, TextMojikumi::LineEndHalf));
        // M6.345: tsume takes each glyph's own blanks out: after あ and い theirs and the next one's, after う its
        // own (a punctuation mark follows), and the full stop's right blank (1/1000 em; 40 px an em).
        let style = text_style("", 40.0);
        let blank = |c: char| side_blanks(&style, c).unwrap_or((0.0, 0.0));
        let blanks = blank('あ').1
            + blank('い').0
            + blank('い').1
            + blank('う').0
            + blank('う').1
            + blank('。').1;
        let expected = blanks * 40.0 / 1000.0;
        assert!(expected > 10.0, "{expected}");
        assert!(
            (plain - tsume - expected).abs() < 1.0,
            "{plain} {tsume} {expected}"
        );
        // The half-width full stop gives up half an em, 20 px.
        assert!((plain - half - 20.0).abs() < 1.0, "{plain} {half}");
    }

    /// M6.345: a glyph's blanks are measured from its outline (each inside its advance; which side a punctuation
    /// mark's blank is on depends on the font, so it is not asked here); a space has no outline.
    #[test]
    fn blanks_are_measured_glyph_by_glyph() {
        let style = text_style("", 40.0);
        for c in ['あ', '漢', '。', '「'] {
            let (left, right) = side_blanks(&style, c).unwrap_or((-1.0, -1.0));
            assert!(
                left >= 0.0 && right >= 0.0 && left + right < 1000.0,
                "{c} {left} {right}"
            );
        }
        assert_eq!(side_blanks(&style, ' '), None);
    }

    /// M6.313: Encore's 行送り. 自動 sets the lines 120 % of the size apart whatever the font's own line height
    /// (Encore CS6: 100 px text, baselines 120 px apart); a pitch puts the baselines that far apart (150 px: 150)
    /// and leaves the first line where it is. The extra leading of older projects adds to it. A vertical text's
    /// columns are as far apart.
    #[test]
    fn lines_are_the_line_pitch_apart() {
        let baselines = |format: &TextFormat| -> Vec<f32> {
            formatted_layout("Hg\nHg\nHg", "", 100.0, None, format)
                .lines
                .iter()
                .map(|line| line.baseline)
                .collect()
        };
        let gaps = |format: &TextFormat| -> Vec<f32> {
            baselines(format)
                .windows(2)
                .filter_map(|pair| Some(pair.get(1)? - pair.first()?))
                .collect()
        };
        let near = |gaps: Vec<f32>, want: f32| {
            gaps.len() == 2 && gaps.iter().all(|gap| (gap - want).abs() < 0.01)
        };
        let auto = TextFormat::default();
        assert!(near(gaps(&auto), 120.0), "{:?}", gaps(&auto));
        let fixed = TextFormat {
            line_pitch: Some(150),
            ..TextFormat::default()
        };
        assert!(near(gaps(&fixed), 150.0), "{:?}", gaps(&fixed));
        assert_eq!(
            baselines(&fixed).first().copied(),
            baselines(&auto).first().copied()
        );
        let older = TextFormat {
            leading: 10,
            ..TextFormat::default()
        };
        assert!(near(gaps(&older), 130.0), "{:?}", gaps(&older));
        // Superscript: the full size's pitch.
        let small = TextFormat {
            script: TextScript::Superscript,
            ..TextFormat::default()
        };
        assert!(near(gaps(&small), 120.0), "{:?}", gaps(&small));

        let width = |format: &TextFormat| {
            let [x0, _, x1, _] =
                crate::text_block::vertical_block("あ\nい", "", 40.0, None, format).1;
            x1 - x0
        };
        let tall = TextFormat {
            line_pitch: Some(80),
            ..TextFormat::default()
        };
        // Each column as wide as the pitch: 自動 2 x 48 px, then 2 x 80.
        let (auto, tall) = (width(&TextFormat::default()), width(&tall));
        assert!((auto - 96.0).abs() < 0.5, "{auto}");
        assert!((tall - 160.0).abs() < 0.5, "{tall}");
    }
}

#[cfg(test)]
mod alpha_tests {
    use super::load_image;
    use disccraft_project::Project;

    /// M6.294: a picture whose alpha channel is ignored (Interpret Footage) is drawn opaque, its colours as they
    /// are; otherwise its alpha is kept.
    #[test]
    fn an_ignored_alpha_channel_is_opaque() -> Result<(), Box<dyn std::error::Error>> {
        let dir =
            std::env::temp_dir().join(format!("disccraft-ignore-alpha-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("half.png");
        let file = std::io::BufWriter::new(std::fs::File::create(&path)?);
        let mut encoder = png::Encoder::new(file, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()?
            .write_image_data(&[200, 100, 50, 128, 10, 20, 30, 0])?;
        let mut project = Project::default();
        let id = project.add_asset(&path)?;
        let alpha = |project: &Project| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
            let image = load_image(project, id, 720, 480)?;
            assert_eq!(image.channels, 4);
            Ok(image
                .data
                .chunks(4)
                .filter_map(|pixel| pixel.get(3).copied())
                .collect())
        };
        assert_eq!(alpha(&project)?, vec![128, 0]);
        if let Some(asset) = project.assets.get_mut(&id) {
            asset.ignore_alpha = true;
        }
        assert_eq!(alpha(&project)?, vec![255, 255]);
        assert_eq!(
            load_image(&project, id, 720, 480)?.data,
            vec![200, 100, 50, 255, 10, 20, 30, 255]
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
