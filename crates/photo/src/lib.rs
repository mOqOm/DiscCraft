//! PhotoCraft 連携: PSD の読み込みと合成、Encore 互換のレイヤー名によるメニューのボタン認識。
//!
//! 層: L0（docs/architecture.md §5）。
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::path::Path;

use photocraft_compose::{Buffer, flatten, render_layer};
use photocraft_doc::{Document, Layer, LayerContent};
use photocraft_geom::Rect;
use photocraft_raster::Rgba8Image;

/// Straight-alpha 8-bit RGBA image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image8 {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Row-major RGBA bytes.
    pub rgba: Vec<u8>,
}

mod svg;
mod write;

pub use svg::{is_svg, render_svg_fill, render_svg_fit};
pub use write::{PsdLayer, write_layered_psd};

/// Errors returned by this crate.
#[derive(Debug, thiserror::Error)]
pub enum PhotoError {
    /// File-system or PhotoCraft import failure.
    #[error("{path}: {message}")]
    Io {
        /// Path that failed.
        path: String,
        /// Error message.
        message: String,
    },
    /// Invalid PSD data or unsupported canvas size.
    #[error("PSD: {0}")]
    Invalid(String),
}

/// A menu button found in a PSD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PsdButton {
    /// Button name from the Encore-compatible group name.
    pub name: String,
    /// X coordinate relative to the canvas origin.
    pub x: u32,
    /// Y coordinate relative to the canvas origin.
    pub y: u32,
    /// Button width in pixels.
    pub width: u32,
    /// Button height in pixels.
    pub height: u32,
    /// Optional cropped highlight subpicture.
    pub highlight: Option<Image8>,
    /// The button's video thumbnail (Encore's `(%)` layer in the set): x, y, width and height on the canvas of
    /// what the layer covers.
    pub thumbnail: Option<[u32; 4]>,
    /// The button's subpicture (its `(=1)` / `(=2)` / `(=3)` layers) over the button's area.
    pub subpicture: Option<PsdSubpicture>,
}

/// A PSD button's subpicture (Encore's `(=1)` / `(=2)` / `(=3)` layers): which colour of the colour set each pixel
/// of the button's area is drawn in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PsdSubpicture {
    /// Width in pixels (the button's).
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Each pixel the number of the topmost subpicture layer drawn there (half opaque or more; the layer's own
    /// colour does not matter), 0 where none is.
    pub values: Vec<u8>,
    /// The colour of each subpicture layer (`(=1)` to `(=3)`): its first opaque pixel. Encore's automatic colour
    /// set uses them.
    pub colors: [Option<[u8; 3]>; 3],
}

/// A menu read from a PSD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PsdMenu {
    /// Canvas width in pixels.
    pub width: u32,
    /// Canvas height in pixels.
    pub height: u32,
    /// Background composite with highlight layers hidden.
    pub background: Image8,
    /// Buttons in Encore's numbering: from the bottom of the layer stack up (the lowest `(+)` group is button 1,
    /// wherever it lies on the screen).
    pub buttons: Vec<PsdButton>,
}

const MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024;
pub(crate) const MAX_DIMENSION: u32 = 8192;
const BUTTON_PREFIX: &str = "(+)";

/// Whether the path looks like a Photoshop document (`.psd` / `.psb`, case-insensitive).
pub fn is_psd(path: &Path) -> bool {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) => ext.eq_ignore_ascii_case("psd") || ext.eq_ignore_ascii_case("psb"),
        None => false,
    }
}

/// Read a PSD / PSB file and composite every visible layer (PhotoCraft's compositor: blend modes, opacity, masks, groups).
pub fn load_image(path: &Path) -> Result<Image8, PhotoError> {
    let (doc, canvas) = load_document(path)?;
    let image = image_from_rgba8(flatten(&doc).to_rgba8())?;
    validate_image_size(&image, canvas)?;
    Ok(image)
}

/// Read a PSD / PSB as a menu background: every visible layer except the highlight layers (`(=1)`, `(=2)`, `(=3)`),
/// which are subpictures, not part of the picture.
pub fn load_menu_background(path: &Path) -> Result<Image8, PhotoError> {
    let (doc, canvas) = load_document(path)?;
    render_background(&doc, canvas)
}

/// Read a menu PSD that follows the Encore-compatible layer naming.
pub fn load_psd_menu(path: &Path) -> Result<PsdMenu, PhotoError> {
    let (doc, canvas) = load_document(path)?;
    let background = render_background(&doc, canvas)?;
    let mut buttons = Vec::new();

    for layer in collect_button_layers(&doc.layers) {
        if let Some(button) = build_button(layer, canvas) {
            buttons.push(button);
        }
    }

    let width = rect_width_u32(canvas)
        .ok_or_else(|| PhotoError::Invalid("invalid canvas width".to_string()))?;
    let height = rect_height_u32(canvas)
        .ok_or_else(|| PhotoError::Invalid("invalid canvas height".to_string()))?;

    Ok(PsdMenu {
        width,
        height,
        background,
        buttons,
    })
}

fn load_document(path: &Path) -> Result<(Document, Rect), PhotoError> {
    let bytes = read_bytes(path)?;
    let name = file_name(path);
    let imported = photocraft_io::import(name, &bytes).map_err(|error| PhotoError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    })?;

    let canvas = canvas_rect(&imported.document)?;
    Ok((imported.document, canvas))
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, PhotoError> {
    let display = path.display().to_string();

    let metadata = std::fs::metadata(path).map_err(|error| PhotoError::Io {
        path: display.clone(),
        message: error.to_string(),
    })?;

    if metadata.len() > MAX_FILE_BYTES {
        return Err(PhotoError::Invalid(format!("{display}: file too large")));
    }

    std::fs::read(path).map_err(|error| PhotoError::Io {
        path: display,
        message: error.to_string(),
    })
}

fn file_name(path: &Path) -> &str {
    match path.file_name().and_then(|name| name.to_str()) {
        Some(name) => name,
        None => "document.psd",
    }
}

fn canvas_rect(doc: &Document) -> Result<Rect, PhotoError> {
    let rect = doc.bounds();

    let width = rect_width_u32(rect)
        .ok_or_else(|| PhotoError::Invalid("invalid canvas width".to_string()))?;
    let height = rect_height_u32(rect)
        .ok_or_else(|| PhotoError::Invalid("invalid canvas height".to_string()))?;

    if width == 0 || width > MAX_DIMENSION || height == 0 || height > MAX_DIMENSION {
        return Err(PhotoError::Invalid(format!(
            "canvas size {width}x{height} out of range"
        )));
    }

    Ok(rect)
}

fn image_from_rgba8(image: Rgba8Image) -> Result<Image8, PhotoError> {
    let width = usize::try_from(image.width)
        .map_err(|_| PhotoError::Invalid("bad image width".to_string()))?;
    let height = usize::try_from(image.height)
        .map_err(|_| PhotoError::Invalid("bad image height".to_string()))?;

    let expected = width
        .checked_mul(height)
        .and_then(|area| area.checked_mul(4))
        .ok_or_else(|| PhotoError::Invalid("image size overflow".to_string()))?;

    if image.pixels.len() != expected {
        return Err(PhotoError::Invalid("bad image pixel count".to_string()));
    }

    Ok(Image8 {
        width: image.width,
        height: image.height,
        rgba: image.pixels,
    })
}

fn validate_image_size(image: &Image8, canvas: Rect) -> Result<(), PhotoError> {
    let width = rect_width_u32(canvas)
        .ok_or_else(|| PhotoError::Invalid("invalid canvas width".to_string()))?;
    let height = rect_height_u32(canvas)
        .ok_or_else(|| PhotoError::Invalid("invalid canvas height".to_string()))?;

    if image.width != width || image.height != height {
        return Err(PhotoError::Invalid("rendered size mismatch".to_string()));
    }

    Ok(())
}

fn render_background(doc: &Document, canvas: Rect) -> Result<Image8, PhotoError> {
    let mut doc = doc.clone();
    hide_highlights_in_layers(&mut doc.layers);

    let image = image_from_rgba8(flatten(&doc).to_rgba8())?;
    validate_image_size(&image, canvas)?;
    Ok(image)
}

fn collect_button_layers(layers: &[Layer]) -> Vec<Layer> {
    let mut out = Vec::new();

    // Bottom to top (the document keeps its layers that way), as Encore numbers the buttons.
    for layer in layers {
        if let LayerContent::Group(group) = &layer.content {
            if is_button_name(&layer.name) {
                out.push(layer.clone());
            } else {
                out.extend(collect_button_layers(&group.children));
            }
        }
    }

    out
}

fn collect_highlight_layers(layer: &Layer) -> Vec<Layer> {
    let mut out = Vec::new();

    if let LayerContent::Group(group) = &layer.content {
        for child in &group.children {
            collect_highlight_from_layer(child, &mut out);
        }
    }

    out
}

fn collect_highlight_from_layer(layer: &Layer, out: &mut Vec<Layer>) {
    if is_highlight_name(&layer.name) {
        out.push(layer.clone());
        return;
    }

    if let LayerContent::Group(group) = &layer.content {
        for child in &group.children {
            collect_highlight_from_layer(child, out);
        }
    }
}

fn hide_highlights_in_layers(layers: &mut [Layer]) {
    for layer in layers {
        hide_highlight_layer(layer);
    }
}

fn hide_highlight_layer(layer: &mut Layer) {
    if is_highlight_name(&layer.name) {
        layer.visible = false;
    }

    if let LayerContent::Group(group) = &mut layer.content {
        hide_highlights_in_layers(&mut group.children);
    }
}

fn force_visible(layer: &mut Layer) {
    layer.visible = true;

    if let LayerContent::Group(group) = &mut layer.content {
        for child in &mut group.children {
            force_visible(child);
        }
    }
}

fn build_button(layer: Layer, canvas: Rect) -> Option<PsdButton> {
    let name = button_name(&layer.name)?;

    let highlights = collect_highlight_layers(&layer);
    let highlight_buffers = render_highlight_buffers(&highlights, canvas);
    let highlight_bbox = highlight_bbox(&highlight_buffers, canvas);
    let button_bbox = button_bbox(&layer, canvas);

    // Like Encore, the button covers its whole layer set: the visible layers and the highlight layers.
    let rect = match (button_bbox, highlight_bbox) {
        (Some(a), Some(b)) => union_bbox(&[a, b])?,
        (a, b) => a.or(b)?,
    };
    let highlight = build_highlight_image(&highlight_buffers, highlight_bbox, rect, canvas);
    let subpicture = build_subpicture(&highlights, rect, canvas);
    let thumbnail = thumbnail_bbox(&layer, canvas).and_then(|area| {
        Some([
            u32::try_from(area.x0).ok()?,
            u32::try_from(area.y0).ok()?,
            rect_width_u32(area)?,
            rect_height_u32(area)?,
        ])
    });

    Some(PsdButton {
        name,
        x: u32::try_from(rect.x0).ok()?,
        y: u32::try_from(rect.y0).ok()?,
        width: rect_width_u32(rect)?,
        height: rect_height_u32(rect)?,
        highlight,
        thumbnail,
        subpicture,
    })
}

/// The button's subpicture over `rect`: each `(=n)` layer, bottom to top, marks the pixels it draws half opaque
/// or more with `n` (the top layer wins where they overlap, as Encore flattens them). `None` when nothing is drawn.
fn build_subpicture(layers: &[Layer], rect: Rect, canvas: Rect) -> Option<PsdSubpicture> {
    let width = rect_width_u32(rect)?;
    let height = rect_height_u32(rect)?;
    let len = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?;
    let mut values = vec![0_u8; len];
    let mut colors = [None; 3];
    for layer in layers {
        let Some(value) = highlight_value(&layer.name) else {
            continue;
        };
        let mut clone = layer.clone();
        force_visible(&mut clone);
        let buffer = render_layer(&clone, canvas);
        let Some(image) = crop_buffer_to_image(&buffer, rect, canvas) else {
            continue;
        };
        let pixels = image.rgba.as_chunks::<4>().0;
        for (slot, pixel) in values.iter_mut().zip(pixels) {
            if pixel[3] >= 128 {
                *slot = value;
            }
        }
        if let Some(color) = colors.get_mut(usize::from(value - 1))
            && color.is_none()
        {
            *color = pixels
                .iter()
                .find(|pixel| pixel[3] == u8::MAX)
                .or_else(|| pixels.iter().find(|pixel| pixel[3] >= 128))
                .map(|pixel| [pixel[0], pixel[1], pixel[2]]);
        }
    }
    values
        .iter()
        .any(|value| *value != 0)
        .then_some(PsdSubpicture {
            width,
            height,
            values,
            colors,
        })
}

/// What the first `(%)` layer of a button set covers (shown or not: it is where the video goes).
fn thumbnail_bbox(layer: &Layer, canvas: Rect) -> Option<Rect> {
    let mut found = find_thumbnail_layer(layer)?.clone();
    force_visible(&mut found);
    let buffer = render_layer(&found, canvas);
    alpha_bbox(&buffer, canvas)
}

fn find_thumbnail_layer(layer: &Layer) -> Option<&Layer> {
    let LayerContent::Group(group) = &layer.content else {
        return None;
    };
    group.children.iter().find_map(|child| {
        if is_thumbnail_name(&child.name) {
            Some(child)
        } else {
            find_thumbnail_layer(child)
        }
    })
}

fn button_bbox(layer: &Layer, canvas: Rect) -> Option<Rect> {
    let mut hidden = layer.clone();
    hide_highlight_layer(&mut hidden);

    let buffer = render_layer(&hidden, canvas);
    alpha_bbox(&buffer, canvas)
}

fn render_highlight_buffers(layers: &[Layer], canvas: Rect) -> Vec<Buffer> {
    let mut out = Vec::new();

    for layer in layers {
        let mut clone = layer.clone();
        force_visible(&mut clone);

        let buffer = render_layer(&clone, canvas);
        if alpha_bbox(&buffer, canvas).is_some() {
            out.push(buffer);
        }
    }

    out
}

fn highlight_bbox(buffers: &[Buffer], canvas: Rect) -> Option<Rect> {
    let mut bboxes = Vec::new();

    for buffer in buffers {
        if let Some(bbox) = alpha_bbox(buffer, canvas) {
            bboxes.push(bbox);
        }
    }

    union_bbox(&bboxes)
}

fn build_highlight_image(
    buffers: &[Buffer],
    bbox: Option<Rect>,
    rect: Rect,
    canvas: Rect,
) -> Option<Image8> {
    let bbox = bbox?;

    let abs_bbox = relative_to_absolute(bbox, canvas)?;
    let abs_rect = relative_to_absolute(rect, canvas)?;
    intersect_rect(abs_bbox, abs_rect)?;

    let combined = combine_highlight_buffers(buffers, bbox, canvas)?;
    let image = crop_buffer_to_image(&combined, rect, canvas)?;

    if rgba_has_alpha(&image.rgba) {
        Some(image)
    } else {
        None
    }
}

fn combine_highlight_buffers(buffers: &[Buffer], union: Rect, canvas: Rect) -> Option<Buffer> {
    let rect = relative_to_absolute(union, canvas)?;
    let width = rect_width_usize(rect)?;
    let height = rect_height_usize(rect)?;
    let len = width.checked_mul(height)?;

    if len == 0 {
        return None;
    }

    let mut px = vec![[0.0_f32; 4]; len];

    for buffer in buffers {
        merge_buffer_into(&mut px, rect, buffer);
    }

    Some(Buffer { rect, px })
}

fn merge_buffer_into(dst: &mut [[f32; 4]], dst_rect: Rect, src: &Buffer) {
    let Some(width) = rect_width_usize(src.rect) else {
        return;
    };

    if width == 0 {
        return;
    }

    for (row_idx, row) in src.px.chunks_exact(width).enumerate() {
        merge_row_into(dst, dst_rect, src.rect, row_idx, row);
    }
}

fn merge_row_into(
    dst: &mut [[f32; 4]],
    dst_rect: Rect,
    src_rect: Rect,
    row_idx: usize,
    row: &[[f32; 4]],
) {
    for (col_idx, pixel) in row.iter().enumerate() {
        merge_pixel_into(dst, dst_rect, src_rect, row_idx, col_idx, *pixel);
    }
}

fn merge_pixel_into(
    dst: &mut [[f32; 4]],
    dst_rect: Rect,
    src_rect: Rect,
    row_idx: usize,
    col_idx: usize,
    pixel: [f32; 4],
) {
    let [r, g, b, a] = pixel;
    if !a.is_finite() || a <= 0.0 {
        return;
    }

    let Some(x) = col_x(src_rect, col_idx) else {
        return;
    };
    let Some(y) = row_y(src_rect, row_idx) else {
        return;
    };
    let Some(index) = target_index(dst_rect, x, y) else {
        return;
    };

    if let Some(dst_px) = dst.get_mut(index)
        && should_replace(*dst_px, a)
    {
        *dst_px = [r, g, b, a];
    }
}

fn target_index(rect: Rect, x: i64, y: i64) -> Option<usize> {
    if x < i64::from(rect.x0)
        || x >= i64::from(rect.x1)
        || y < i64::from(rect.y0)
        || y >= i64::from(rect.y1)
    {
        return None;
    }

    let dx = usize::try_from(x - i64::from(rect.x0)).ok()?;
    let dy = usize::try_from(y - i64::from(rect.y0)).ok()?;
    let width = rect_width_usize(rect)?;

    dy.checked_mul(width)?.checked_add(dx)
}

fn should_replace(current: [f32; 4], alpha: f32) -> bool {
    let [_, _, _, cur] = current;
    alpha >= cur
}

fn crop_buffer_to_image(buffer: &Buffer, target: Rect, canvas: Rect) -> Option<Image8> {
    let abs_target = relative_to_absolute(target, canvas)?;
    let width = rect_width_usize(abs_target)?;
    let height = rect_height_usize(abs_target)?;

    if width == 0 || height == 0 {
        return None;
    }

    let expected = width.checked_mul(height)?.checked_mul(4)?;
    let mut rgba = Vec::with_capacity(expected);

    let Some(inter) = intersect_rect(buffer.rect, abs_target) else {
        push_transparent(&mut rgba, expected);
        return image_from_rgba_vec(width, height, rgba);
    };

    let Some(src_w) = rect_width_usize(buffer.rect) else {
        push_transparent(&mut rgba, expected);
        return image_from_rgba_vec(width, height, rgba);
    };

    if src_w == 0 {
        push_transparent(&mut rgba, expected);
        return image_from_rgba_vec(width, height, rgba);
    }

    let top_pad = i32_diff_to_usize(inter.y0, abs_target.y0)?
        .checked_mul(width)?
        .checked_mul(4)?;
    push_transparent(&mut rgba, top_pad);

    let left_pad = i32_diff_to_usize(inter.x0, abs_target.x0)?;
    let right_pad = i32_diff_to_usize(abs_target.x1, inter.x1)?;
    let source_start = i32_diff_to_usize(inter.x0, buffer.rect.x0)?;
    let take = rect_width_usize(inter)?;

    for (row_idx, row) in buffer.px.chunks_exact(src_w).enumerate() {
        let Some(y) = row_y(buffer.rect, row_idx) else {
            continue;
        };

        if y < i64::from(inter.y0) {
            continue;
        }
        if y >= i64::from(inter.y1) {
            break;
        }

        append_cropped_row(&mut rgba, row, left_pad, source_start, take, right_pad);
    }

    let bottom_pad = i32_diff_to_usize(abs_target.y1, inter.y1)?
        .checked_mul(width)?
        .checked_mul(4)?;
    push_transparent(&mut rgba, bottom_pad);

    if rgba.len() != expected {
        return None;
    }

    image_from_rgba_vec(width, height, rgba)
}

fn append_cropped_row(
    rgba: &mut Vec<u8>,
    row: &[[f32; 4]],
    left_pad: usize,
    source_start: usize,
    take: usize,
    right_pad: usize,
) {
    push_transparent_pixels(rgba, left_pad);
    append_row(rgba, row, source_start, take);
    push_transparent_pixels(rgba, right_pad);
}

fn append_row(rgba: &mut Vec<u8>, row: &[[f32; 4]], x_start: usize, take: usize) {
    for pixel in row.iter().skip(x_start).take(take) {
        let [r, g, b, a] = *pixel;
        rgba.push(f32_to_u8(r));
        rgba.push(f32_to_u8(g));
        rgba.push(f32_to_u8(b));
        rgba.push(f32_to_u8(a));
    }
}

fn push_transparent_pixels(rgba: &mut Vec<u8>, pixels: usize) {
    if let Some(bytes) = pixels.checked_mul(4) {
        push_transparent(rgba, bytes);
    }
}

fn push_transparent(rgba: &mut Vec<u8>, bytes: usize) {
    rgba.extend(std::iter::repeat_n(0u8, bytes));
}

fn image_from_rgba_vec(width: usize, height: usize, rgba: Vec<u8>) -> Option<Image8> {
    Some(Image8 {
        width: u32::try_from(width).ok()?,
        height: u32::try_from(height).ok()?,
        rgba,
    })
}

fn rgba_has_alpha(rgba: &[u8]) -> bool {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .any(|px| matches!(px, [_, _, _, a] if *a > 0))
}

fn f32_to_u8(value: f32) -> u8 {
    if !value.is_finite() {
        return 0;
    }

    let clamped = value.clamp(0.0, 1.0);
    let scaled = clamped * 255.0 + 0.5;

    if scaled <= 0.0 {
        0
    } else if scaled >= 255.0 {
        255
    } else {
        scaled as u8
    }
}

fn alpha_bbox(buffer: &Buffer, canvas: Rect) -> Option<Rect> {
    let width = rect_width_usize(buffer.rect)?;
    if width == 0 {
        return None;
    }

    let mut bbox = None;

    for (row_idx, row) in buffer.px.chunks_exact(width).enumerate() {
        let Some(y) = row_y(buffer.rect, row_idx) else {
            continue;
        };

        for (col_idx, pixel) in row.iter().enumerate() {
            if pixel_alpha_positive(pixel)
                && let Some(x) = col_x(buffer.rect, col_idx)
            {
                add_point(&mut bbox, x, y);
            }
        }
    }

    bbox.and_then(|bbox| bbox_to_relative(bbox, canvas))
}

fn pixel_alpha_positive(pixel: &[f32; 4]) -> bool {
    let [_, _, _, alpha] = *pixel;
    alpha.is_finite() && alpha > 0.0
}

#[derive(Clone, Copy)]
struct BBox {
    min_x: i64,
    min_y: i64,
    max_x: i64,
    max_y: i64,
}

impl BBox {
    fn new(x: i64, y: i64) -> Self {
        Self {
            min_x: x,
            min_y: y,
            max_x: x.saturating_add(1),
            max_y: y.saturating_add(1),
        }
    }

    fn extend(&mut self, x: i64, y: i64) {
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x.saturating_add(1));
        self.max_y = self.max_y.max(y.saturating_add(1));
    }
}

fn add_point(bbox: &mut Option<BBox>, x: i64, y: i64) {
    if let Some(bbox) = bbox {
        bbox.extend(x, y);
    } else {
        *bbox = Some(BBox::new(x, y));
    }
}

fn bbox_to_relative(bbox: BBox, canvas: Rect) -> Option<Rect> {
    let width = i64::from(rect_width_u32(canvas)?);
    let height = i64::from(rect_height_u32(canvas)?);

    let x0 = clamp_i64(bbox.min_x.checked_sub(i64::from(canvas.x0))?, 0, width);
    let y0 = clamp_i64(bbox.min_y.checked_sub(i64::from(canvas.y0))?, 0, height);
    let x1 = clamp_i64(bbox.max_x.checked_sub(i64::from(canvas.x0))?, 0, width);
    let y1 = clamp_i64(bbox.max_y.checked_sub(i64::from(canvas.y0))?, 0, height);

    if x1 <= x0 || y1 <= y0 {
        return None;
    }

    Some(Rect {
        x0: i32::try_from(x0).ok()?,
        y0: i32::try_from(y0).ok()?,
        x1: i32::try_from(x1).ok()?,
        y1: i32::try_from(y1).ok()?,
    })
}

fn union_bbox(rects: &[Rect]) -> Option<Rect> {
    let mut x0: Option<i32> = None;
    let mut y0: Option<i32> = None;
    let mut x1: Option<i32> = None;
    let mut y1: Option<i32> = None;

    for rect in rects {
        x0 = Some(x0.map_or(rect.x0, |v| v.min(rect.x0)));
        y0 = Some(y0.map_or(rect.y0, |v| v.min(rect.y0)));
        x1 = Some(x1.map_or(rect.x1, |v| v.max(rect.x1)));
        y1 = Some(y1.map_or(rect.y1, |v| v.max(rect.y1)));
    }

    Some(Rect {
        x0: x0?,
        y0: y0?,
        x1: x1?,
        y1: y1?,
    })
}

fn relative_to_absolute(rect: Rect, canvas: Rect) -> Option<Rect> {
    let x0 = canvas.x0.checked_add(rect.x0)?;
    let y0 = canvas.y0.checked_add(rect.y0)?;
    let x1 = canvas.x0.checked_add(rect.x1)?;
    let y1 = canvas.y0.checked_add(rect.y1)?;

    if x1 <= x0 || y1 <= y0 {
        return None;
    }

    Some(Rect { x0, y0, x1, y1 })
}

fn intersect_rect(a: Rect, b: Rect) -> Option<Rect> {
    let x0 = a.x0.max(b.x0);
    let y0 = a.y0.max(b.y0);
    let x1 = a.x1.min(b.x1);
    let y1 = a.y1.min(b.y1);

    if x0 < x1 && y0 < y1 {
        Some(Rect { x0, y0, x1, y1 })
    } else {
        None
    }
}

fn rect_width_u32(rect: Rect) -> Option<u32> {
    let width = rect.x1.checked_sub(rect.x0)?;
    u32::try_from(width).ok()
}

fn rect_height_u32(rect: Rect) -> Option<u32> {
    let height = rect.y1.checked_sub(rect.y0)?;
    u32::try_from(height).ok()
}

fn rect_width_usize(rect: Rect) -> Option<usize> {
    usize::try_from(rect_width_u32(rect)?).ok()
}

fn rect_height_usize(rect: Rect) -> Option<usize> {
    usize::try_from(rect_height_u32(rect)?).ok()
}

fn row_y(rect: Rect, row_idx: usize) -> Option<i64> {
    let row = i64::try_from(row_idx).ok()?;
    i64::from(rect.y0).checked_add(row)
}

fn col_x(rect: Rect, col_idx: usize) -> Option<i64> {
    let col = i64::try_from(col_idx).ok()?;
    i64::from(rect.x0).checked_add(col)
}

fn i32_diff_to_usize(a: i32, b: i32) -> Option<usize> {
    let diff = i64::from(a).checked_sub(i64::from(b))?;
    usize::try_from(diff).ok()
}

fn clamp_i64(value: i64, min: i64, max: i64) -> i64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

fn is_button_name(name: &str) -> bool {
    name.starts_with(BUTTON_PREFIX)
}

fn button_name(name: &str) -> Option<String> {
    name.strip_prefix(BUTTON_PREFIX)
        .map(|rest| rest.trim().to_string())
}

fn is_thumbnail_name(name: &str) -> bool {
    name.trim_start().starts_with("(%)")
}

fn is_highlight_name(name: &str) -> bool {
    highlight_value(name).is_some()
}

/// The colour number of a subpicture layer's name (`(=1)` to `(=3)`).
fn highlight_value(name: &str) -> Option<u8> {
    [1_u8, 2, 3]
        .into_iter()
        .find(|value| name.starts_with(&format!("(={value})")))
}

#[cfg(test)]
mod tests {
    use super::{
        PhotoError, alpha_bbox, button_name, combine_highlight_buffers, crop_buffer_to_image,
        is_highlight_name, is_psd, load_image,
    };
    use photocraft_compose::Buffer;
    use photocraft_geom::Rect;
    use std::path::Path;

    #[test]
    fn parses_button_names() {
        assert_eq!(button_name("(+) Play").as_deref(), Some("Play"));
        assert_eq!(button_name("(+)").as_deref(), Some(""));
        assert_eq!(button_name("(+)  Play  ").as_deref(), Some("Play"));
        assert_eq!(button_name("Play"), None);
    }

    #[test]
    fn parses_highlight_names() {
        assert!(is_highlight_name("(=1)"));
        assert!(is_highlight_name("(=2) foo"));
        assert!(is_highlight_name("(=3)bar"));
        assert!(!is_highlight_name("(=4)"));
        assert!(!is_highlight_name("(=1"));
    }

    #[test]
    fn detects_psd_paths() {
        assert!(is_psd(Path::new("menu.PSD")));
        assert!(is_psd(Path::new("menu.psb")));
        assert!(!is_psd(Path::new("menu.png")));
        assert!(!is_psd(Path::new("menu")));
    }

    #[test]
    fn missing_file_is_io_error() {
        let result = load_image(Path::new("definitely-missing-photo.psd"));
        assert!(matches!(result, Err(PhotoError::Io { .. })));
    }

    #[test]
    fn alpha_bbox_is_canvas_relative() {
        let canvas = Rect {
            x0: 10,
            y0: 20,
            x1: 13,
            y1: 23,
        };

        let mut px = vec![[0.0_f32; 4]; 9];
        px[5] = [1.0, 0.0, 0.0, 1.0];

        let buffer = Buffer { rect: canvas, px };
        let bbox = alpha_bbox(&buffer, canvas);

        assert!(bbox.is_some());
        if let Some(bbox) = bbox {
            assert_eq!(bbox.x0, 2);
            assert_eq!(bbox.y0, 1);
            assert_eq!(bbox.x1, 3);
            assert_eq!(bbox.y1, 2);
        }
    }

    #[test]
    fn crops_buffer_to_image() {
        let canvas = Rect {
            x0: 0,
            y0: 0,
            x1: 2,
            y1: 2,
        };

        let px = vec![
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0, 1.0],
        ];

        let buffer = Buffer { rect: canvas, px };
        let target = Rect {
            x0: 1,
            y0: 0,
            x1: 2,
            y1: 2,
        };

        let image = crop_buffer_to_image(&buffer, target, canvas);
        assert!(image.is_some());

        if let Some(image) = image {
            assert_eq!(image.width, 1);
            assert_eq!(image.height, 2);
            assert_eq!(image.rgba.len(), 8);
            assert_eq!(image.rgba[0], 0);
            assert_eq!(image.rgba[1], 255);
            assert_eq!(image.rgba[2], 0);
            assert_eq!(image.rgba[3], 255);
            // Row-major buffer: column x = 1 of row 1 is the white pixel.
            assert_eq!(image.rgba[4], 255);
            assert_eq!(image.rgba[5], 255);
            assert_eq!(image.rgba[6], 255);
            assert_eq!(image.rgba[7], 255);
        }
    }

    #[test]
    fn crop_pads_outside_source() {
        let canvas = Rect {
            x0: 0,
            y0: 0,
            x1: 2,
            y1: 2,
        };

        let source_rect = Rect {
            x0: 0,
            y0: 0,
            x1: 1,
            y1: 1,
        };

        let buffer = Buffer {
            rect: source_rect,
            px: vec![[1.0, 0.0, 0.0, 1.0]],
        };

        let target = Rect {
            x0: 0,
            y0: 0,
            x1: 2,
            y1: 2,
        };

        let image = crop_buffer_to_image(&buffer, target, canvas);
        assert!(image.is_some());

        if let Some(image) = image {
            assert_eq!(image.width, 2);
            assert_eq!(image.height, 2);
            assert_eq!(image.rgba.len(), 16);
            assert_eq!(image.rgba[0], 255);
            assert_eq!(image.rgba[3], 255);
            assert_eq!(image.rgba[4], 0);
            assert_eq!(image.rgba[7], 0);
            assert_eq!(image.rgba[8], 0);
            assert_eq!(image.rgba[11], 0);
        }
    }

    #[test]
    fn combines_highlights_top_opaque_wins() {
        let canvas = Rect {
            x0: 0,
            y0: 0,
            x1: 1,
            y1: 1,
        };

        let bottom = Buffer {
            rect: canvas,
            px: vec![[1.0, 0.0, 0.0, 1.0]],
        };
        let top = Buffer {
            rect: canvas,
            px: vec![[0.0, 0.0, 1.0, 1.0]],
        };

        let union = Rect {
            x0: 0,
            y0: 0,
            x1: 1,
            y1: 1,
        };

        let combined = combine_highlight_buffers(&[bottom, top], union, canvas);
        assert!(combined.is_some());

        if let Some(combined) = combined {
            assert_eq!(combined.px[0], [0.0, 0.0, 1.0, 1.0]);
        }
    }

    #[test]
    fn combines_highlights_max_alpha_wins() {
        let canvas = Rect {
            x0: 0,
            y0: 0,
            x1: 1,
            y1: 1,
        };

        let bottom = Buffer {
            rect: canvas,
            px: vec![[1.0, 0.0, 0.0, 1.0]],
        };
        let top = Buffer {
            rect: canvas,
            px: vec![[0.0, 0.0, 1.0, 0.5]],
        };

        let union = Rect {
            x0: 0,
            y0: 0,
            x1: 1,
            y1: 1,
        };

        let combined = combine_highlight_buffers(&[bottom, top], union, canvas);
        assert!(combined.is_some());

        if let Some(combined) = combined {
            assert_eq!(combined.px[0], [1.0, 0.0, 0.0, 1.0]);
        }
    }
}
