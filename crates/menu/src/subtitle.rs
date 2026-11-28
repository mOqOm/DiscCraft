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
    /// The picture the cue shows instead of its text (an image script's subtitle).
    pub image: Option<disccraft_project::CueImage>,
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

/// The most a channel of a picture subtitle's pixel may differ from a mapped colour and still be taken as that
/// colour (pictures saved with a little noise, as JPEG pictures are).
const IMAGE_COLOR_TOLERANCE: u8 = 24;

/// A cue as a 2-bit subpicture: its picture when it has one ([`render_image_cue`]), else its text drawn in
/// `style` ([`render_cue_styled`]).
///
/// # Errors
///
/// As those two.
pub fn render_any_cue(
    cue: &Cue,
    frame_width: u16,
    frame_height: u16,
    style: CueStyle,
) -> Result<CueBitmap, MenuError> {
    match &cue.image {
        Some(image) => render_image_cue(image, frame_width, frame_height),
        None => render_cue_styled(&cue.text, frame_width, frame_height, style),
    }
}

/// Whether a cue has something to show: a picture, or text that is not blank (a subtitle added in the Monitor and
/// not typed yet has none).
#[must_use]
pub fn cue_shows(cue: &Cue) -> bool {
    cue.image.is_some() || !cue.text.trim().is_empty()
}

/// Draws a picture subtitle (Encore's image script and FAB image script) as a 2-bit subpicture. The picture's
/// top left corner stands at the left top of its area, and what runs past the area or the frame is cut. As in
/// Encore's Map Colors, pixels of the background colour are transparent, those of the fill and the outline colour
/// are drawn in the colour group's fill and stroke, and any other colour is transparent (so are pixels less than
/// half opaque).
///
/// # Errors
///
/// When the frame size is zero, the picture cannot be read, or none of its pixels in the frame is the fill or the
/// outline colour.
pub fn render_image_cue(
    image: &disccraft_project::CueImage,
    frame_width: u16,
    frame_height: u16,
) -> Result<CueBitmap, MenuError> {
    if frame_width == 0 || frame_height == 0 {
        return Err(MenuError::Invalid(
            "frame dimensions must be non-zero".to_string(),
        ));
    }
    let picture = disccraft_photo::load_image(std::path::Path::new(&image.path))
        .map_err(|error| MenuError::Invalid(format!("subtitle picture {}: {error}", image.path)))?;
    let [left, top, right, bottom] = image.area;
    let width = u32::from(right.min(frame_width).saturating_sub(left)).min(picture.width);
    let height = u32::from(bottom.min(frame_height).saturating_sub(top)).min(picture.height);
    // Each pixel of the part in the frame, and the box around the ones drawn.
    let mut values = Vec::new();
    let mut drawn: Option<(u32, u32, u32, u32)> = None;
    for y in 0..height {
        for x in 0..width {
            let value = usize::try_from(u64::from(y) * u64::from(picture.width) + u64::from(x))
                .ok()
                .and_then(|index| index.checked_mul(4))
                .and_then(|index| picture.rgba.get(index..index.saturating_add(4)))
                .map_or(TRANSPARENT, |pixel| image_pixel_value(pixel, &image.colors));
            values.push(value);
            if value != TRANSPARENT {
                drawn = Some(match drawn {
                    None => (x, y, x, y),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                });
            }
        }
    }
    let Some((x0, y0, x1, y1)) = drawn else {
        return Err(MenuError::Invalid(format!(
            "subtitle picture {}: none of its pixels in the frame is the fill or the outline colour",
            image.path
        )));
    };
    let too_big = || MenuError::Invalid(format!("subtitle picture {} is too large", image.path));
    let box_width = u16::try_from(x1 - x0 + 1).map_err(|_| too_big())?;
    let box_height = u16::try_from(y1 - y0 + 1).map_err(|_| too_big())?;
    let x = left.saturating_add(u16::try_from(x0).map_err(|_| too_big())?);
    let first_row = top.saturating_add(u16::try_from(y0).map_err(|_| too_big())?);
    let (y, rows) = make_even_height(first_row, box_height, frame_height);
    let mut pixels = Vec::with_capacity(usize::from(box_width) * usize::from(rows));
    for row in 0..rows {
        // The rows of the box in the picture; a row added to make the height even is transparent.
        let source = i64::from(y) + i64::from(row) - i64::from(top);
        let start = u64::try_from(source)
            .ok()
            .filter(|source| *source < u64::from(height))
            .and_then(|source| usize::try_from(source * u64::from(width) + u64::from(x0)).ok());
        match start
            .and_then(|start| values.get(start..start.saturating_add(usize::from(box_width))))
        {
            Some(line) => pixels.extend_from_slice(line),
            None => pixels.resize(
                pixels.len().saturating_add(usize::from(box_width)),
                TRANSPARENT,
            ),
        }
    }
    Ok(CueBitmap {
        x,
        y,
        width: box_width,
        height: rows,
        pixels,
    })
}

/// A picture subtitle's file as RGBA pixels (width, height, row-major RGBA), for Map Colors to show and sample.
///
/// # Errors
///
/// When the picture cannot be read.
pub fn load_picture(path: &std::path::Path) -> Result<(u32, u32, Vec<u8>), MenuError> {
    let picture = disccraft_photo::load_image(path)
        .map_err(|error| MenuError::Invalid(format!("{}: {error}", path.display())))?;
    Ok((picture.width, picture.height, picture.rgba))
}

/// The subpicture value of one RGBA pixel of a picture subtitle: the nearest of the three mapped colours within
/// [`IMAGE_COLOR_TOLERANCE`] (the background first on a tie), transparent for any other colour: 0 transparent,
/// 1 the fill, 2 the outline.
#[must_use]
pub fn image_pixel_value(pixel: &[u8], colors: &disccraft_project::ImageCueColors) -> u8 {
    let (Some(&red), Some(&green), Some(&blue), Some(&alpha)) =
        (pixel.first(), pixel.get(1), pixel.get(2), pixel.get(3))
    else {
        return TRANSPARENT;
    };
    if alpha < 128 {
        return TRANSPARENT;
    }
    let distance = |color: [u8; 3]| {
        red.abs_diff(color[0])
            .max(green.abs_diff(color[1]))
            .max(blue.abs_diff(color[2]))
    };
    [
        (colors.background, TRANSPARENT),
        (colors.fill, FILL),
        (colors.outline, OUTLINE),
    ]
    .into_iter()
    .map(|(color, value)| (distance(color), value))
    .filter(|(distance, _)| *distance <= IMAGE_COLOR_TOLERANCE)
    .min_by_key(|(distance, _)| *distance)
    .map_or(TRANSPARENT, |(_, value)| value)
}

/// Encore's Map Colors before anything is picked: the picture's commonest colour is the background, the next the
/// fill and the next the outline (colours closer than the tolerance count as one). In a picture with transparent
/// pixels the commonest opaque colour is the fill and the next the outline, and the background is a colour the
/// picture does not use. A missing colour is the fill's.
///
/// # Errors
///
/// When the picture cannot be read.
pub fn guess_image_colors(
    path: &std::path::Path,
) -> Result<disccraft_project::ImageCueColors, MenuError> {
    let picture = disccraft_photo::load_image(path)
        .map_err(|error| MenuError::Invalid(format!("{}: {error}", path.display())))?;
    let mut counts: std::collections::HashMap<[u8; 3], u64> = std::collections::HashMap::new();
    let mut transparent = false;
    for &[red, green, blue, alpha] in picture.rgba.as_chunks::<4>().0 {
        if alpha < 128 {
            transparent = true;
        } else {
            let count = counts.entry([red, green, blue]).or_insert(0);
            *count = count.saturating_add(1);
        }
    }
    let mut colors: Vec<([u8; 3], u64)> = counts.into_iter().collect();
    colors.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let near = |a: [u8; 3], b: [u8; 3]| {
        a.iter()
            .zip(b)
            .all(|(x, y)| x.abs_diff(y) <= IMAGE_COLOR_TOLERANCE)
    };
    let mut picked: Vec<[u8; 3]> = Vec::new();
    for (color, _) in colors {
        if picked.len() == 3 {
            break;
        }
        if !picked.iter().any(|chosen| near(*chosen, color)) {
            picked.push(color);
        }
    }
    let at = |index: usize| picked.get(index).copied();
    let (background, fill, outline) = if transparent {
        let fill = at(0).unwrap_or([255, 255, 255]);
        let outline = at(1).unwrap_or(fill);
        // A colour far from both, so that no opaque pixel is taken for the background.
        let unused = [
            [0, 255, 0],
            [255, 0, 255],
            [0, 0, 255],
            [255, 0, 0],
            [0, 255, 255],
            [255, 255, 0],
        ]
        .into_iter()
        .find(|color| !near(*color, fill) && !near(*color, outline))
        .unwrap_or([0, 255, 0]);
        (unused, fill, outline)
    } else {
        let background = at(0).unwrap_or([0, 0, 0]);
        let fill = at(1).unwrap_or(background);
        (background, fill, at(2).unwrap_or(fill))
    };
    Ok(disccraft_project::ImageCueColors {
        background,
        fill,
        outline,
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
        image: None,
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

pub(crate) fn strip_tags_text(text: &str) -> String {
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

#[cfg(test)]
mod image_tests {
    use super::{FILL, OUTLINE, TRANSPARENT, guess_image_colors, render_image_cue};
    use disccraft_project::{CueImage, ImageCueColors};
    use std::error::Error;
    use std::path::{Path, PathBuf};

    const BLUE: [u8; 3] = [0, 0, 255];
    const WHITE: [u8; 3] = [255, 255, 255];
    const BLACK: [u8; 3] = [0, 0, 0];

    /// Writes a 40 x 20 picture subtitle: a blue background, a black box (the outline) at columns 9..=30 and rows
    /// 3..=14 with a white box (the fill) inside it, and one red pixel (a colour not mapped) at (35, 17).
    fn write_picture(path: &Path, alpha: u8) -> Result<(), Box<dyn Error>> {
        let mut rgba = Vec::new();
        for y in 0..20_u32 {
            for x in 0..40_u32 {
                let outline = (9..=30).contains(&x) && (3..=14).contains(&y);
                let fill = (10..=29).contains(&x) && (4..=13).contains(&y);
                let (color, opacity) = if fill {
                    (WHITE, 255)
                } else if outline {
                    (BLACK, 255)
                } else if (x, y) == (35, 17) {
                    ([255, 0, 0], 255)
                } else {
                    (BLUE, alpha)
                };
                rgba.extend_from_slice(&color);
                rgba.push(opacity);
            }
        }
        let file = std::fs::File::create(path)?;
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 40, 20);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&rgba)?;
        Ok(())
    }

    fn scratch(name: &str) -> Result<PathBuf, Box<dyn Error>> {
        let dir =
            std::env::temp_dir().join(format!("disccraft-image-cue-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// The mapped colours become the fill and the outline, the background and any other colour transparent; the
    /// bitmap is the box around what is drawn, at the picture's place in the frame, with an even height.
    #[test]
    fn a_picture_subtitle_is_drawn_in_its_mapped_colours() -> Result<(), Box<dyn Error>> {
        let dir = scratch("draw")?;
        let path = dir.join("sub.png");
        write_picture(&path, 255)?;
        let colors = guess_image_colors(&path)?;
        // The commonest colour is the background, then the fill, then the outline.
        assert_eq!(
            colors,
            ImageCueColors {
                background: BLUE,
                fill: WHITE,
                outline: BLACK
            }
        );
        let image = CueImage {
            path: path.to_string_lossy().into_owned(),
            area: [100, 400, 700, 470],
            colors,
        };
        let bitmap = render_image_cue(&image, 720, 480)?;
        assert_eq!((bitmap.x, bitmap.y), (109, 403));
        // 12 rows drawn: already even.
        assert_eq!((bitmap.width, bitmap.height), (22, 12));
        let at = |x: usize, y: usize| bitmap.pixels.get(y * 22 + x).copied();
        assert_eq!(at(0, 0), Some(OUTLINE));
        assert_eq!(at(1, 1), Some(FILL));
        assert_eq!(at(21, 11), Some(OUTLINE));
        assert!(!bitmap.pixels.contains(&TRANSPARENT));

        // Cut by the area: only the first 15 columns and 10 rows of the picture are shown; an odd height gets a
        // transparent row.
        let cut = render_image_cue(
            &CueImage {
                area: [0, 0, 15, 10],
                ..image.clone()
            },
            720,
            480,
        )?;
        assert_eq!((cut.x, cut.y, cut.width, cut.height), (9, 3, 6, 8));
        assert_eq!(cut.pixels.get(7 * 6..), Some(&[TRANSPARENT; 6][..]));

        // The red pixel is drawn only when it is mapped; a picture with nothing mapped is refused.
        let red = render_image_cue(
            &CueImage {
                colors: ImageCueColors {
                    outline: [255, 0, 0],
                    ..colors
                },
                ..image.clone()
            },
            720,
            480,
        )?;
        // The black border is not mapped then: the white box and the red pixel make the box.
        assert_eq!((red.x, red.y, red.width, red.height), (110, 404, 26, 14));
        let nothing = CueImage {
            colors: ImageCueColors {
                background: BLUE,
                fill: [0, 255, 0],
                outline: [0, 255, 0],
            },
            ..image.clone()
        };
        assert!(render_image_cue(&nothing, 720, 480).is_err());
        assert!(render_image_cue(&image, 0, 480).is_err());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// A picture with a transparent background: the commonest opaque colour is the fill, and the background is
    /// one the picture does not use.
    #[test]
    fn a_transparent_picture_takes_its_opaque_colours() -> Result<(), Box<dyn Error>> {
        let dir = scratch("alpha")?;
        let path = dir.join("sub.png");
        write_picture(&path, 0)?;
        let colors = guess_image_colors(&path)?;
        assert_eq!((colors.fill, colors.outline), (WHITE, BLACK));
        assert!(![WHITE, BLACK, [255, 0, 0]].contains(&colors.background));
        let bitmap = render_image_cue(
            &CueImage {
                path: path.to_string_lossy().into_owned(),
                area: [0, 0, 720, 480],
                colors,
            },
            720,
            480,
        )?;
        assert_eq!(
            (bitmap.x, bitmap.y, bitmap.width, bitmap.height),
            (9, 3, 22, 12)
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
