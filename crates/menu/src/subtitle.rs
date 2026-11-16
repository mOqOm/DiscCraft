//! SRT subtitle parsing and 2-bit subpicture rendering.

use crate::MenuError;
use disccraft_project::Tick;
use filmcraft_text::{Align, Caps, Mask, ParagraphStyle, TextStyle, families, layout, render};
use std::sync::OnceLock;

pub(crate) const MAX_SRT_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_CUES: usize = 20_000;
pub(crate) const TICKS_PER_MS: u64 = 254_016_000;

const FONT_DIVISOR: f32 = 18.0;
const MIN_FONT_SIZE: f32 = 16.0;
const BOTTOM_MARGIN: f32 = 0.08;
/// The title-safe inset on each side, as a part of the frame's width.
const SIDE_MARGIN: f32 = 0.10;

const FILL_THRESHOLD: f32 = 0.5;
const DILATE_THRESHOLD: f32 = 0.5;
/// The widest outline around a cue's letters, in pixels.
const MAX_OUTLINE: u8 = 4;

const TRANSPARENT: u8 = 0;
const FILL: u8 = 1;
const OUTLINE: u8 = 2;
/// A pixel the letters cover only in part: drawn in the anti-alias colour, which softens their edges.
pub const ANTIALIAS: u8 = 3;
/// The least coverage that makes an anti-alias pixel.
const ANTIALIAS_THRESHOLD: f32 = 0.2;

const FONT_FAMILIES: [&str; 7] = [
    "Yu Gothic UI",
    "Yu Gothic",
    "Meiryo",
    "Noto Sans CJK JP",
    "Noto Sans JP",
    "Hiragino Sans",
    "Inter",
];

/// One parsed subtitle cue.
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    /// First tick at which the cue is visible.
    pub start: Tick,
    /// First tick at which the cue is no longer visible.
    pub end: Tick,
    /// Cue text, with lines joined by `\n`.
    pub text: String,
    /// The cue's own colour group, stroke and place (the default for cues read from a file).
    pub look: disccraft_project::CueLook,
}

/// A rendered 2-bit subpicture bitmap.
#[derive(Clone, Debug, PartialEq)]
pub struct CueBitmap {
    /// Left edge in the frame.
    pub x: u16,
    /// Top edge in the frame.
    pub y: u16,
    /// Bitmap width.
    pub width: u16,
    /// Bitmap height, always even.
    pub height: u16,
    /// Row-major pixels: 0 transparent, 1 fill, 2 outline.
    pub pixels: Vec<u8>,
}

/// Parses SRT text into cues.
///
/// # Errors
///
/// Returns [`MenuError::Invalid`] when the input is too large, contains too
/// many cues, or contains a malformed timing line.
pub fn parse_srt(text: &str) -> Result<Vec<Cue>, MenuError> {
    if text.len() > MAX_SRT_BYTES {
        return Err(MenuError::Invalid("srt input is too large".to_string()));
    }

    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut cues = Vec::new();
    let mut block: Vec<&str> = Vec::new();
    let mut block_no = 1usize;

    for raw_line in body.split('\n') {
        let line = raw_line.trim_end_matches('\r');

        if line.trim().is_empty() {
            if !block.is_empty() {
                process_block(&block, block_no, &mut cues)?;
                block_no = block_no
                    .checked_add(1)
                    .ok_or_else(|| MenuError::Invalid("too many srt blocks".to_string()))?;
                block.clear();
            }
            continue;
        }

        block.push(line);
    }

    if !block.is_empty() {
        process_block(&block, block_no, &mut cues)?;
    }

    if cues.len() > MAX_CUES {
        return Err(MenuError::Invalid("too many srt cues".to_string()));
    }

    cues.sort_by_key(|a| a.start);
    Ok(cues)
}

/// Renders cue text into a 2-bit subpicture bitmap.
///
/// # Errors
///
/// Returns [`MenuError::Invalid`] when the frame size is zero, the text is
/// empty, or the rendered cue has no visible pixels inside the frame.
pub fn render_cue(text: &str, frame_width: u16, frame_height: u16) -> Result<CueBitmap, MenuError> {
    render_cue_styled(text, frame_width, frame_height, CueStyle::default())
}

/// How a cue is drawn (Encore's subtitle Stroke and Alignment).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CueStyle {
    /// The outline around the letters, in pixels (0: none).
    pub outline: u8,
    /// Where the text sits between the top (0) and the bottom (100) of the subtitle area, in percent.
    pub vertical: u8,
    /// Which side the lines are set to: centred in the frame, or from an edge of the title-safe area.
    pub horizontal: disccraft_project::SubtitleJustify,
}

impl Default for CueStyle {
    /// A medium outline at the bottom.
    fn default() -> Self {
        Self {
            outline: 2,
            vertical: 100,
            horizontal: disccraft_project::SubtitleJustify::Center,
        }
    }
}

/// Whether a subtitle's text does not fit the subtitle area of a frame: wider than the title-safe width (a line
/// that cannot be broken) or, with its line breaks, taller than the title-safe height. Such
/// text reaches into the edges a television may hide, or off the frame. Text that cannot be laid out fits.
#[must_use]
pub fn cue_overflows(text: &str, frame_width: u16, frame_height: u16) -> bool {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let text = strip_tags_text(text);
    if text.trim().is_empty() || frame_width == 0 || frame_height == 0 {
        return false;
    }
    let family = choose_family();
    let style = text_style(&family, font_size(frame_height));
    let safe_width = f32::from(frame_width) * (1.0 - 2.0 * SIDE_MARGIN);
    // Laid out in the title-safe width, as the text frame of Encore's subtitles.
    let para = ParagraphStyle {
        align: Align::Center,
        width: Some(safe_width),
        ..ParagraphStyle::default()
    };
    let [x0, y0, x1, y1] = layout(&text, &style, &para).bounds;
    let (width, height) = (x1 - x0, y1 - y0);
    if !width.is_finite() || !height.is_finite() {
        return false;
    }
    // The title-safe height as well: Encore's subtitle text frame is the title-safe box (80 % of the frame each
    // way; ten lines of the default size fit an NTSC frame), and its check reports text that runs out of it.
    let room = f32::from(frame_height) * (1.0 - 2.0 * SIDE_MARGIN);
    width > safe_width + 1.0 || height > room
}

/// [`render_cue`] with a chosen outline and vertical place.
///
/// # Errors
///
/// As [`render_cue`].
pub fn render_cue_styled(
    text: &str,
    frame_width: u16,
    frame_height: u16,
    cue_style: CueStyle,
) -> Result<CueBitmap, MenuError> {
    let radius = i64::from(cue_style.outline.min(MAX_OUTLINE));
    if frame_width == 0 || frame_height == 0 {
        return Err(MenuError::Invalid(
            "frame dimensions must be non-zero".to_string(),
        ));
    }

    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let text = strip_tags_text(text);

    if text.is_empty() {
        return Err(MenuError::Invalid("cue text is empty".to_string()));
    }

    let family = choose_family();
    let style = text_style(&family, font_size(frame_height));
    let para = ParagraphStyle {
        align: match cue_style.horizontal {
            disccraft_project::SubtitleJustify::Left => Align::Left,
            disccraft_project::SubtitleJustify::Center => Align::Center,
            disccraft_project::SubtitleJustify::Right => Align::Right,
        },
        width: Some(f32::from(frame_width)),
        ..ParagraphStyle::default()
    };

    let layout = layout(&text, &style, &para);
    if layout.glyphs.is_empty() {
        return Err(MenuError::Invalid(
            "cue text produced no glyphs".to_string(),
        ));
    }

    let [x0, y0, x1, y1] = layout.bounds;
    let frame_w_f = f32::from(frame_width);
    let frame_h_f = f32::from(frame_height);
    let text_w = x1 - x0;

    if !text_w.is_finite() {
        return Err(MenuError::Invalid("cue bounds are not finite".to_string()));
    }

    // Left and right stand on the title-safe edges (a tenth of the frame in from each side, as Encore's subtitle
    // text frame).
    let x = match cue_style.horizontal {
        disccraft_project::SubtitleJustify::Left => frame_w_f * SIDE_MARGIN - x0,
        disccraft_project::SubtitleJustify::Center => (frame_w_f - text_w) * 0.5 - x0,
        disccraft_project::SubtitleJustify::Right => frame_w_f * (1.0 - SIDE_MARGIN) - x1,
    };
    // The subtitle area leaves the same margin at the top and the bottom; at 100 % the text stands on its bottom.
    let top = frame_h_f * BOTTOM_MARGIN;
    let bottom = frame_h_f * (1.0 - BOTTOM_MARGIN);
    let room = bottom - top - (y1 - y0);
    let y = top + room * f32::from(cue_style.vertical.min(100)) / 100.0 - y0;

    if !x.is_finite() || !y.is_finite() {
        return Err(MenuError::Invalid(
            "cue placement is not finite".to_string(),
        ));
    }

    let (mask, mask_x, mask_y) = render::rasterize(&layout, &render::at(x, y));
    if mask.w == 0 || mask.h == 0 {
        return Err(MenuError::Invalid("cue produced no pixels".to_string()));
    }

    let bbox = bounding_box(&mask, mask_x, mask_y, (frame_width, frame_height), radius)?;
    let pixels = rasterize_pixels(&mask, mask_x, mask_y, &bbox, radius)?;

    Ok(CueBitmap {
        x: bbox.x,
        y: bbox.y,
        width: bbox.width,
        height: bbox.height,
        pixels,
    })
}

pub(crate) fn process_block(
    lines: &[&str],
    block_no: usize,
    cues: &mut Vec<Cue>,
) -> Result<(), MenuError> {
    if lines.is_empty() {
        return Ok(());
    }

    let timing_index = find_timing_index(lines)
        .ok_or_else(|| MenuError::Invalid(format!("block {block_no}: missing timing line")))?;

    if timing_index > 1 {
        return Err(MenuError::Invalid(format!(
            "block {block_no}: unexpected lines before timing line"
        )));
    }

    let timing_line = lines.get(timing_index).copied().unwrap_or_default();
    let (start, end) = parse_timing(timing_line, block_no)?;

    if end <= start {
        return Ok(());
    }

    let text = collect_text(lines, timing_index);
    if text.is_empty() {
        return Ok(());
    }

    cues.push(Cue {
        start,
        end,
        text,
        look: disccraft_project::CueLook::default(),
    });

    if cues.len() > MAX_CUES {
        return Err(MenuError::Invalid("too many srt cues".to_string()));
    }

    Ok(())
}

fn find_timing_index(lines: &[&str]) -> Option<usize> {
    lines.iter().position(|line| line.contains("-->"))
}

fn collect_text(lines: &[&str], timing_index: usize) -> String {
    let mut out = String::new();

    for line in lines.iter().skip(timing_index.saturating_add(1)) {
        let stripped = strip_tags_line(line).trim().to_string();
        if stripped.is_empty() {
            continue;
        }

        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&stripped);
    }

    out
}

fn parse_timing(line: &str, block_no: usize) -> Result<(Tick, Tick), MenuError> {
    let malformed = || MenuError::Invalid(format!("block {block_no}: malformed timing line"));

    let (start_part, end_part) = line.split_once("-->").ok_or_else(malformed)?;
    let start_ms = parse_time(start_part.trim()).ok_or_else(malformed)?;
    let end_str = end_part.split_whitespace().next().unwrap_or_default();
    let end_ms = parse_time(end_str).ok_or_else(malformed)?;

    let start = tick_from_ms(start_ms)
        .ok_or_else(|| MenuError::Invalid(format!("block {block_no}: timing overflow")))?;
    let end = tick_from_ms(end_ms)
        .ok_or_else(|| MenuError::Invalid(format!("block {block_no}: timing overflow")))?;

    Ok((start, end))
}

/// `HH:MM:SS,mmm` (SRT) or `HH:MM:SS.mmm` / `MM:SS.mmm` (WebVTT) as a tick count.
pub(crate) fn parse_srt_time_ticks(s: &str) -> Option<Tick> {
    tick_from_ms(parse_time(s)?)
}

fn parse_time(s: &str) -> Option<u64> {
    let parts: Vec<&str> = s.split(':').collect();
    // WebVTT may leave the hours out.
    let (h, m, sec) = match parts.as_slice() {
        [h, m, sec] => (*h, *m, *sec),
        [m, sec] => ("0", *m, *sec),
        _ => return None,
    };

    let (sec_str, ms_str) = sec.split_once(',').or_else(|| sec.split_once('.'))?;

    let h = parse_u64(h)?;
    let m = parse_u64(m)?;
    let sec_num = parse_u64(sec_str)?;
    let ms = parse_u64(ms_str)?;

    if m >= 60 || sec_num >= 60 || ms >= 1000 {
        return None;
    }

    h.checked_mul(60)?
        .checked_add(m)?
        .checked_mul(60)?
        .checked_add(sec_num)?
        .checked_mul(1000)?
        .checked_add(ms)
}

fn parse_u64(s: &str) -> Option<u64> {
    s.parse::<u64>().ok()
}

fn tick_from_ms(ms: u64) -> Option<Tick> {
    let ticks = ms.checked_mul(TICKS_PER_MS)?;
    Some(Tick(i64::try_from(ticks).ok()?))
}

fn font_size(frame_height: u16) -> f32 {
    let size = f32::from(frame_height) / FONT_DIVISOR;
    if size.is_finite() && size >= MIN_FONT_SIZE {
        size
    } else {
        MIN_FONT_SIZE
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

/// Registers the platform's font folders with filmcraft-text once per process.
fn ensure_system_fonts() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        filmcraft_text::fonts::add_source(&filmcraft_text::fonts::DirectorySource::system());
    });
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

fn strip_tags_text(text: &str) -> String {
    let mut out = String::new();

    for line in text.split('\n') {
        let stripped = strip_tags_line(line.trim_end_matches('\r'))
            .trim()
            .to_string();
        if stripped.is_empty() {
            continue;
        }

        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&stripped);
    }

    out
}

fn strip_tags_line(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;

    while let Some(pos) = rest.find('<') {
        if let Some(before) = rest.get(..pos) {
            out.push_str(before);
        }

        let Some(after) = rest.get(pos..) else {
            break;
        };

        let Some(tag_end) = after.find('>') else {
            out.push_str(after);
            break;
        };

        let tag = after.get(1..tag_end).unwrap_or_default();
        if !is_known_tag(tag) {
            out.push('<');
            out.push_str(tag);
            out.push('>');
        }

        let Some(next_start) = tag_end.checked_add(1) else {
            break;
        };

        rest = after.get(next_start..).unwrap_or_default();
    }

    out.push_str(rest);
    out
}

fn is_known_tag(tag: &str) -> bool {
    let name = tag.split_whitespace().next().unwrap_or_default();

    name.eq_ignore_ascii_case("i")
        || name.eq_ignore_ascii_case("/i")
        || name.eq_ignore_ascii_case("b")
        || name.eq_ignore_ascii_case("/b")
        || name.eq_ignore_ascii_case("u")
        || name.eq_ignore_ascii_case("/u")
        || name.eq_ignore_ascii_case("font")
        || name.eq_ignore_ascii_case("/font")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BBox {
    x: u16,
    y: u16,
    width: u16,
    height: u16,
}

#[derive(Default)]
struct BBoxBuilder {
    min_x: Option<i64>,
    min_y: Option<i64>,
    max_x: Option<i64>,
    max_y: Option<i64>,
}

impl BBoxBuilder {
    fn add_rect(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, frame_w: i64, frame_h: i64) {
        if frame_w <= 0 || frame_h <= 0 {
            return;
        }

        if x1 < 0 || y1 < 0 || x0 >= frame_w || y0 >= frame_h {
            return;
        }

        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = x1.min(frame_w - 1);
        let y1 = y1.min(frame_h - 1);

        if x0 > x1 || y0 > y1 {
            return;
        }

        self.min_x = Some(self.min_x.map_or(x0, |v| v.min(x0)));
        self.min_y = Some(self.min_y.map_or(y0, |v| v.min(y0)));
        self.max_x = Some(self.max_x.map_or(x1, |v| v.max(x1)));
        self.max_y = Some(self.max_y.map_or(y1, |v| v.max(y1)));
    }

    fn finish(self, frame_height: u16) -> Result<BBox, MenuError> {
        let (min_x, min_y, max_x, max_y) = match (self.min_x, self.min_y, self.max_x, self.max_y) {
            (Some(min_x), Some(min_y), Some(max_x), Some(max_y)) => (min_x, min_y, max_x, max_y),
            _ => return Err(MenuError::Invalid("cue is outside the frame".to_string())),
        };

        let Some(width) = max_x.checked_sub(min_x).and_then(|n| n.checked_add(1)) else {
            return Err(MenuError::Invalid(
                "cue bounding box is invalid".to_string(),
            ));
        };

        let Some(height) = max_y.checked_sub(min_y).and_then(|n| n.checked_add(1)) else {
            return Err(MenuError::Invalid(
                "cue bounding box is invalid".to_string(),
            ));
        };

        let x = u16::try_from(min_x)
            .map_err(|_| MenuError::Invalid("cue bounding box is too wide".to_string()))?;
        let y = u16::try_from(min_y)
            .map_err(|_| MenuError::Invalid("cue bounding box is too tall".to_string()))?;
        let width = u16::try_from(width)
            .map_err(|_| MenuError::Invalid("cue bounding box is too wide".to_string()))?;
        let height = u16::try_from(height)
            .map_err(|_| MenuError::Invalid("cue bounding box is too tall".to_string()))?;

        let (mut y, mut height) = make_even_height(y, height, frame_height);
        height = height.min(frame_height);
        y = y.min(frame_height.saturating_sub(height));

        if height % 2 == 1 {
            if height > 1 {
                height -= 1;
            } else if frame_height >= 2 {
                height = 2;
                y = 0;
            } else {
                height = 0;
            }
        }

        y = y.min(frame_height.saturating_sub(height));

        if width == 0 || height == 0 {
            return Err(MenuError::Invalid("cue bounding box is empty".to_string()));
        }

        Ok(BBox {
            x,
            y,
            width,
            height,
        })
    }
}

fn make_even_height(y: u16, height: u16, frame_height: u16) -> (u16, u16) {
    if height.is_multiple_of(2) {
        return (y, height);
    }

    let bottom = y.saturating_add(height);

    if bottom < frame_height {
        return (y, height.saturating_add(1));
    }

    if y > 0 {
        return (y.saturating_sub(1), height.saturating_add(1));
    }

    if height > 1 {
        return (y, height.saturating_sub(1));
    }

    if frame_height >= 2 {
        return (0, 2);
    }

    (y, height)
}

fn bounding_box(
    mask: &Mask,
    mask_x: i32,
    mask_y: i32,
    (frame_width, frame_height): (u16, u16),
    radius: i64,
) -> Result<BBox, MenuError> {
    if frame_width == 0 || frame_height == 0 {
        return Err(MenuError::Invalid(
            "frame dimensions must be non-zero".to_string(),
        ));
    }

    if mask.w == 0 || mask.h == 0 {
        return Err(MenuError::Invalid("cue produced no pixels".to_string()));
    }

    let frame_w_i = i64::from(frame_width);
    let frame_h_i = i64::from(frame_height);
    let mut builder = BBoxBuilder::default();

    for y in 0..mask.h {
        for x in 0..mask.w {
            let Some(alpha) = alpha_at(mask, x, y) else {
                continue;
            };

            let fill = alpha >= FILL_THRESHOLD;
            let strong = alpha > DILATE_THRESHOLD;

            if !fill && !strong {
                continue;
            }

            let Some(x_i) = i64::try_from(x).ok() else {
                continue;
            };
            let Some(y_i) = i64::try_from(y).ok() else {
                continue;
            };

            let Some(dx) = i64::from(mask_x).checked_add(x_i) else {
                continue;
            };
            let Some(dy) = i64::from(mask_y).checked_add(y_i) else {
                continue;
            };

            if fill {
                builder.add_rect(dx, dy, dx, dy, frame_w_i, frame_h_i);
            }

            if strong {
                let Some(x0) = dx.checked_sub(radius) else {
                    continue;
                };
                let Some(x1) = dx.checked_add(radius) else {
                    continue;
                };
                let Some(y0) = dy.checked_sub(radius) else {
                    continue;
                };
                let Some(y1) = dy.checked_add(radius) else {
                    continue;
                };

                builder.add_rect(x0, y0, x1, y1, frame_w_i, frame_h_i);
            }
        }
    }

    builder.finish(frame_height)
}

fn rasterize_pixels(
    mask: &Mask,
    mask_x: i32,
    mask_y: i32,
    bbox: &BBox,
    radius: i64,
) -> Result<Vec<u8>, MenuError> {
    let width = usize::from(bbox.width);
    let height = usize::from(bbox.height);

    let len = width
        .checked_mul(height)
        .ok_or_else(|| MenuError::Invalid("cue bitmap is too large".to_string()))?;

    let mut pixels = vec![TRANSPARENT; len];

    for py in 0..height {
        for px in 0..width {
            let Some(out_idx) = py.checked_mul(width).and_then(|n| n.checked_add(px)) else {
                continue;
            };

            let Some(px_i) = i64::try_from(px).ok() else {
                continue;
            };
            let Some(py_i) = i64::try_from(py).ok() else {
                continue;
            };

            let Some(dx) = px_i.checked_add(i64::from(bbox.x)) else {
                continue;
            };
            let Some(dy) = py_i.checked_add(i64::from(bbox.y)) else {
                continue;
            };

            let value = if fill_at_device(mask, mask_x, mask_y, dx, dy) {
                FILL
            } else if partly_covered(mask, (mask_x, mask_y), dx, dy) {
                ANTIALIAS
            } else if radius > 0 && dilated_at_device(mask, (mask_x, mask_y), dx, dy, radius) {
                OUTLINE
            } else {
                TRANSPARENT
            };

            if value != TRANSPARENT
                && let Some(slot) = pixels.get_mut(out_idx)
            {
                *slot = value;
            }
        }
    }

    Ok(pixels)
}

/// Whether the letters cover the device pixel in part: less than a fill pixel, enough to show.
fn partly_covered(mask: &Mask, (mask_x, mask_y): (i32, i32), dx: i64, dy: i64) -> bool {
    let at = |device: i64, origin: i32| {
        device
            .checked_sub(i64::from(origin))
            .and_then(|value| usize::try_from(value).ok())
    };
    let (Some(mx), Some(my)) = (at(dx, mask_x), at(dy, mask_y)) else {
        return false;
    };
    alpha_at(mask, mx, my).is_some_and(|a| (ANTIALIAS_THRESHOLD..FILL_THRESHOLD).contains(&a))
}

fn fill_at_device(mask: &Mask, mask_x: i32, mask_y: i32, dx: i64, dy: i64) -> bool {
    let Some(mx) = dx.checked_sub(i64::from(mask_x)) else {
        return false;
    };
    let Some(my) = dy.checked_sub(i64::from(mask_y)) else {
        return false;
    };

    if mx < 0 || my < 0 {
        return false;
    }

    let Some(mx) = usize::try_from(mx).ok() else {
        return false;
    };
    let Some(my) = usize::try_from(my).ok() else {
        return false;
    };

    alpha_at(mask, mx, my).is_some_and(|a| a >= FILL_THRESHOLD)
}

fn dilated_at_device(
    mask: &Mask,
    (mask_x, mask_y): (i32, i32),
    dx: i64,
    dy: i64,
    radius: i64,
) -> bool {
    let mask_x_i = i64::from(mask_x);
    let mask_y_i = i64::from(mask_y);

    for off_y in -radius..=radius {
        let Some(oy) = dy.checked_add(off_y) else {
            continue;
        };
        let Some(my) = oy.checked_sub(mask_y_i) else {
            continue;
        };

        if my < 0 {
            continue;
        }

        let Some(my) = usize::try_from(my).ok() else {
            continue;
        };

        if my >= mask.h {
            continue;
        }

        for off_x in -radius..=radius {
            let Some(ox) = dx.checked_add(off_x) else {
                continue;
            };
            let Some(mx) = ox.checked_sub(mask_x_i) else {
                continue;
            };

            if mx < 0 {
                continue;
            }

            let Some(mx) = usize::try_from(mx).ok() else {
                continue;
            };

            if mx >= mask.w {
                continue;
            }

            if alpha_at(mask, mx, my).is_some_and(|a| a > DILATE_THRESHOLD) {
                return true;
            }
        }
    }

    false
}

fn alpha_at(mask: &Mask, x: usize, y: usize) -> Option<f32> {
    let idx = y.checked_mul(mask.w)?.checked_add(x)?;
    mask.a.get(idx).copied()
}

#[cfg(test)]
mod tests {
    use super::{parse_srt, render_cue, tick_from_ms};
    use std::error::Error;

    #[test]
    fn parse_srt_accepts_common_input() -> Result<(), Box<dyn Error>> {
        let srt = "\u{feff}1\r\n00:01:02,345 --> 00:01:03,000\r\n<i>Hello</i>\r\n\r\n00:01:04,000 --> 00:01:05,000\r\n<b>世界</b>\r\n\r\n3\r\n00:01:06,000 --> 00:01:07,000 extra\r\n<font color=\"red\">Third</font>\r\n";
        let cues = parse_srt(srt)?;

        assert_eq!(cues.len(), 3);

        let first = cues.first().ok_or("missing first cue")?;
        let expected = tick_from_ms(62_345).ok_or("tick overflow")?;
        assert_eq!(first.start, expected);
        assert_eq!(first.text, "Hello");

        let second = cues.get(1).ok_or("missing second cue")?;
        assert_eq!(second.text, "世界");

        let third = cues.get(2).ok_or("missing third cue")?;
        assert_eq!(third.text, "Third");

        Ok(())
    }

    #[test]
    fn parse_srt_rejects_malformed_timing() -> Result<(), Box<dyn Error>> {
        let srt = "1\r\n00:01:02,345 --> 00:01:03,000\r\nHello\r\n\r\n2\r\nbad timing\r\nText\r\n";
        let err = parse_srt(srt).err().ok_or("expected error")?;
        assert!(err.to_string().contains("block 2"));

        Ok(())
    }

    #[test]
    fn render_cue_basic() -> Result<(), Box<dyn Error>> {
        let rendered = render_cue("Hello\n世界", 720, 480)?;

        assert!(rendered.width > 0);
        assert!(rendered.height > 0);
        assert_eq!(rendered.height % 2, 0);

        assert!(rendered.pixels.contains(&1));
        assert!(rendered.pixels.contains(&2));

        let x = usize::from(rendered.x);
        let y = usize::from(rendered.y);
        let w = usize::from(rendered.width);
        let h = usize::from(rendered.height);

        assert!(x + w <= 720);
        assert!(y + h <= 480);
        assert!(y >= 240);

        let expected_len = w.checked_mul(h).ok_or("overflow")?;
        assert_eq!(rendered.pixels.len(), expected_len);

        Ok(())
    }

    #[test]
    fn render_cue_rejects_empty_text() -> Result<(), Box<dyn Error>> {
        assert!(render_cue("", 720, 480).is_err());
        Ok(())
    }
}

#[cfg(test)]
mod style_tests {
    use super::{ANTIALIAS, CueStyle, OUTLINE, cue_overflows, render_cue, render_cue_styled};

    /// Encore's Stroke and Alignment: the outline's width, and the place from the top (100 % is the bottom).
    /// A line or two fits; a text of many lines is taller than the subtitle area.
    #[test]
    fn many_lines_overflow_the_subtitle_area() {
        assert!(!cue_overflows("Hello", 720, 480));
        assert!(!cue_overflows("Hello\nworld", 720, 480));
        assert!(!cue_overflows("", 720, 480));
        let tall = vec!["line"; 30].join("\n");
        assert!(cue_overflows(&tall, 720, 480));
        // The same words on one line wrap in the title-safe width, and are then too tall as well.
        let long = vec!["word"; 400].join(" ");
        assert!(cue_overflows(&long, 720, 480));
    }

    #[test]
    fn a_cue_takes_its_stroke_and_its_place() -> Result<(), Box<dyn std::error::Error>> {
        let usual = render_cue("Hello", 720, 480)?;
        let same = render_cue_styled("Hello", 720, 480, CueStyle::default())?;
        assert_eq!(usual, same);
        assert!(usual.pixels.contains(&OUTLINE));
        // The letters' half-covered edge pixels are anti-alias pixels: some, far fewer than the fill.
        let count = |value: u8| usual.pixels.iter().filter(|pixel| **pixel == value).count();
        assert!(
            count(ANTIALIAS) > 0 && count(ANTIALIAS) < count(1),
            "{}",
            count(ANTIALIAS)
        );

        let bare = render_cue_styled(
            "Hello",
            720,
            480,
            CueStyle {
                outline: 0,
                vertical: 100,
                horizontal: disccraft_project::SubtitleJustify::Center,
            },
        )?;
        assert!(!bare.pixels.contains(&OUTLINE));
        assert_eq!(bare.width + 4, usual.width);

        let thick = render_cue_styled(
            "Hello",
            720,
            480,
            CueStyle {
                outline: 3,
                vertical: 100,
                horizontal: disccraft_project::SubtitleJustify::Center,
            },
        )?;
        assert_eq!(thick.width, usual.width + 2);

        let top = render_cue_styled(
            "Hello",
            720,
            480,
            CueStyle {
                outline: 2,
                vertical: 0,
                horizontal: disccraft_project::SubtitleJustify::Center,
            },
        )?;
        // The area leaves 8 % at the top and the bottom: the line starts 38 lines down, its ink a little lower.
        assert!((34..=60).contains(&top.y), "{}", top.y);
        assert!(usual.y > 380, "{}", usual.y);
        let middle = render_cue_styled(
            "Hello",
            720,
            480,
            CueStyle {
                outline: 2,
                vertical: 50,
                horizontal: disccraft_project::SubtitleJustify::Center,
            },
        )?;
        assert!(top.y < middle.y && middle.y < usual.y);

        // Left and right: on the title-safe edges (72 and 648 of 720), give or take the outline.
        let side = |horizontal| {
            render_cue_styled(
                "Hello",
                720,
                480,
                CueStyle {
                    outline: 2,
                    vertical: 100,
                    horizontal,
                },
            )
        };
        let left = side(disccraft_project::SubtitleJustify::Left)?;
        let right = side(disccraft_project::SubtitleJustify::Right)?;
        assert!((66..=76).contains(&left.x), "{}", left.x);
        let right_edge = right.x + right.width;
        assert!((644..=654).contains(&right_edge), "{right_edge}");
        assert!(left.x < usual.x && usual.x < right.x);
        Ok(())
    }
}
