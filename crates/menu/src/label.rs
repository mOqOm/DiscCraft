//! A disc label (M6.506): a PSD for PhotoCraft to print on the face of a printable DVD or Blu-ray, at its real size.
//!
//! The document is the whole disc, 120 mm across, at the resolution chosen (its print size is set by it, so that it
//! prints at 100 % on the printer's disc tray). Its artwork is in a group under a layer mask that shows only the
//! printable ring (from the inner to the outer diameter the disc's maker gives: a "wide printable" disc from about
//! 22 mm, others from about 43 mm, both to about 118 mm), so nothing is printed on the hole, on the clear hub or past
//! the edge, whatever is added to the group later. In the group: the picture (a menu's background, cut to the disc),
//! the disc's title across the upper half and its chapters in the lower half, as editable type in points. Over the
//! group, a hidden layer of guides: the disc's edge, the printable ring and the centre hole (15 mm).
//!
//! The sizes are those of the disc itself (ECMA-267 for a DVD: 120 mm across, a 15 mm centre hole); the printable
//! ring is the disc's maker's, so it is asked for.

pub mod designcraft;

use crate::{MenuError, draw};
use disccraft_photo::{MaskedGroup, PsdLayer};
use disccraft_project::{
    Link, Menu, MenuId, MenuText, Project, TextAlign, TextFormat, default_chapter_number,
};

/// A disc's diameter, mm.
pub const DISC_MM: f32 = 120.0;
/// Its centre hole's diameter, mm.
pub const HOLE_MM: f32 = 15.0;
/// The narrowest and widest printable ring a label may ask for, mm across.
pub const PRINTABLE_MM: std::ops::RangeInclusive<f32> = HOLE_MM..=DISC_MM;
/// The resolutions a label may be made at, pixels an inch.
pub const DPI_RANGE: std::ops::RangeInclusive<f32> = 150.0..=600.0;

/// The title's size, points.
const TITLE_POINTS: f32 = 16.0;
/// The largest and smallest size of the chapters, points: the largest that fits.
const LINES_POINTS: (f32, f32) = (8.0, 4.5);
/// The guides' colour.
const GUIDE_COLOR: [u8; 3] = [0, 174, 239];
/// Millimetres an inch.
const MM_PER_INCH: f32 = 25.4;

/// What a disc label shows, and its disc's printable ring.
#[derive(Clone, Debug, PartialEq)]
pub struct DiscLabel {
    /// The printable ring's inner diameter, mm.
    pub inner_mm: f32,
    /// The printable ring's outer diameter, mm.
    pub outer_mm: f32,
    /// Pixels an inch.
    pub dpi: f32,
    /// The title, across the upper half.
    pub title: String,
    /// Lines (the chapters), in the lower half.
    pub lines: Vec<String>,
    /// The picture under them: RGB24, its width and height, and the width over the height it is shown at.
    pub picture: Option<(Vec<u8>, usize, usize, f32)>,
    /// The layers' names, in the display language.
    pub names: LabelNames,
}

/// The names of a label's layers.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct LabelNames {
    /// The masked group of the artwork.
    pub face: String,
    /// The picture.
    pub picture: String,
    /// The guides.
    pub guides: String,
    /// The chapters (the title's layer is named after its text).
    pub lines: String,
}

/// The chapters with names of their own of the timeline that has the most, as "n. name" (n the chapter's number).
pub fn named_chapters(project: &Project) -> Vec<String> {
    project
        .timelines
        .values()
        .map(|timeline| {
            timeline
                .chapters
                .iter()
                .enumerate()
                .filter(|(_, chapter)| {
                    !chapter.name.trim().is_empty()
                        && default_chapter_number(&chapter.name).is_none()
                })
                .map(|(index, chapter)| format!("{}. {}", index + 1, chapter.name.trim()))
                .collect::<Vec<_>>()
        })
        .fold(Vec::new(), |most, these| {
            if these.len() > most.len() {
                these
            } else {
                most
            }
        })
}

/// The menu whose background a label starts with: the one the disc plays first, else the first menu.
pub fn first_menu(project: &Project) -> Option<MenuId> {
    match project.settings.first_play {
        Link::Menu { id, .. } if project.menu(id).is_some() => Some(project.menu_root(id)),
        _ => project.menus.keys().next().copied(),
    }
}

/// The width over the height `menu` is shown at, in a frame of `frame` pixels: an HD frame as it is, an SD menu 16:9
/// or 4:3.
pub fn display_aspect(menu: &Menu, frame: (u16, u16)) -> f32 {
    if frame.0 > 720 {
        f32::from(frame.0) / f32::from(frame.1.max(1))
    } else if menu.widescreen {
        16.0 / 9.0
    } else {
        4.0 / 3.0
    }
}

/// The label's size in pixels a side, and the resolution that makes that side exactly [`DISC_MM`].
fn canvas(dpi: f32) -> (u16, f32) {
    let dpi = if dpi.is_finite() {
        dpi.clamp(*DPI_RANGE.start(), *DPI_RANGE.end())
    } else {
        300.0
    };
    // `as` saturates; the value is at most 120 / 25.4 * 600, about 2835.
    let side = (DISC_MM / MM_PER_INCH * dpi).round().max(1.0) as u16;
    (side, f32::from(side) / (DISC_MM / MM_PER_INCH))
}

/// The printable ring's diameters in mm (inner, outer): within the disc, the inner one at least the hole's and less
/// than the outer one.
fn ring_mm(label: &DiscLabel) -> (f32, f32) {
    let clamp = |mm: f32, fallback: f32| {
        if mm.is_finite() {
            mm.clamp(*PRINTABLE_MM.start(), *PRINTABLE_MM.end())
        } else {
            fallback
        }
    };
    let outer = clamp(label.outer_mm, 118.0).max(HOLE_MM + 1.0);
    let inner = clamp(label.inner_mm, 22.0).min(outer - 1.0).max(HOLE_MM);
    (inner, outer)
}

/// The printable ring's radii in pixels (inner, outer), for a canvas of `side` pixels ([`ring_mm`]).
fn ring(label: &DiscLabel, side: u16) -> (f32, f32) {
    let per_mm = f32::from(side) / DISC_MM;
    let (inner, outer) = ring_mm(label);
    (inner / 2.0 * per_mm, outer / 2.0 * per_mm)
}

/// One byte a pixel: 255 on the ring from `inner` to `outer` pixels from the centre, 0 off it, smoothed over a pixel.
fn ring_mask(side: u16, (inner, outer): (f32, f32)) -> Vec<u8> {
    let n = usize::from(side);
    let centre = f32::from(side) / 2.0;
    let mut mask = vec![0u8; n * n];
    for (index, value) in mask.iter_mut().enumerate() {
        let (x, y) = ((index % n) as f32 + 0.5, (index / n) as f32 + 0.5);
        let d = (x - centre).hypot(y - centre);
        let cover = (outer - d + 0.5).clamp(0.0, 1.0) * (d - inner + 0.5).clamp(0.0, 1.0);
        // `as` saturates; the value is within 0..=255.
        *value = (cover * 255.0).round() as u8;
    }
    mask
}

/// The picture cut to the disc: scaled, as it is shown (its aspect), to cover the square of `side` pixels, centred;
/// white without one.
fn picture_rgba(picture: Option<&(Vec<u8>, usize, usize, f32)>, side: u16) -> Vec<u8> {
    let n = usize::from(side);
    let mut rgba = [255u8, 255, 255, 255].repeat(n * n);
    let Some((rgb, width, height, aspect)) = picture else {
        return rgba;
    };
    let (width, height) = (*width, *height);
    if width == 0 || height == 0 || rgb.len() < width * height * 3 {
        return rgba;
    }
    let aspect = if aspect.is_finite() && *aspect > 0.0 {
        *aspect
    } else {
        width as f32 / height as f32
    };
    // Shown `height * aspect` wide: the shorter side covers the square.
    let shown = (height as f32 * aspect, height as f32);
    let scale = f32::from(side) / shown.0.min(shown.1);
    let x_per_shown = width as f32 / shown.0;
    let sample = |x: f32, y: f32| -> [f32; 3] {
        let x = (x - 0.5).clamp(0.0, (width - 1) as f32);
        let y = (y - 0.5).clamp(0.0, (height - 1) as f32);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |x: usize, y: usize, c: usize| {
            f32::from(rgb.get((y * width + x) * 3 + c).copied().unwrap_or(0))
        };
        [0, 1, 2].map(|c| {
            let top = at(x0, y0, c) * (1.0 - fx) + at(x1, y0, c) * fx;
            let bottom = at(x0, y1, c) * (1.0 - fx) + at(x1, y1, c) * fx;
            top * (1.0 - fy) + bottom * fy
        })
    };
    let half = f32::from(side) / 2.0;
    for (index, pixel) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = ((index % n) as f32 + 0.5, (index / n) as f32 + 0.5);
        let shown_x = (x - half) / scale + shown.0 / 2.0;
        let shown_y = (y - half) / scale + shown.1 / 2.0;
        let [r, g, b] = sample(shown_x * x_per_shown, shown_y);
        // `as` saturates; the values are within 0..=255.
        *pixel = [r.round() as u8, g.round() as u8, b.round() as u8, 255];
    }
    rgba
}

/// The picture in square pixels as it is shown (`height * aspect` wide, `height` high), as a PNG (M6.507: placed in
/// DesignCraft, which fills the ring with it).
fn picture_png(
    (rgb, width, height, aspect): &(Vec<u8>, usize, usize, f32),
) -> Result<Vec<u8>, MenuError> {
    let (width, height) = (*width, *height);
    if width == 0 || height == 0 || rgb.len() < width * height * 3 {
        return Err(MenuError::Invalid("the picture is empty".to_owned()));
    }
    let shown = if aspect.is_finite() && *aspect > 0.0 {
        // `as` saturates; kept within the size a picture may be.
        ((height as f32 * aspect).round() as usize).clamp(1, 16_384)
    } else {
        width
    };
    let at = |x: usize, y: usize, c: usize| {
        f32::from(rgb.get((y * width + x) * 3 + c).copied().unwrap_or(0))
    };
    let mut out = Vec::with_capacity(shown * height * 3);
    for y in 0..height {
        for x in 0..shown {
            let from = ((x as f32 + 0.5) * width as f32 / shown as f32 - 0.5)
                .clamp(0.0, (width - 1) as f32);
            let (x0, f) = (from.floor() as usize, from.fract());
            let x1 = (x0 + 1).min(width - 1);
            for c in 0..3 {
                // `as` saturates; the value is within 0..=255.
                out.push((at(x0, y, c) * (1.0 - f) + at(x1, y, c) * f).round() as u8);
            }
        }
    }
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(
            &mut png,
            u32::try_from(shown).unwrap_or(1),
            u32::try_from(height).unwrap_or(1),
        );
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .and_then(|mut writer| writer.write_image_data(&out))
            .map_err(|error| MenuError::Invalid(format!("the picture: {error}")))?;
    }
    Ok(png)
}

/// Circles of the disc's edge, the printable ring and the hole, and a cross at the centre, on a clear layer.
fn guides_rgba(side: u16, (inner, outer): (f32, f32)) -> Vec<u8> {
    let n = usize::from(side);
    let centre = f32::from(side) / 2.0;
    let per_mm = f32::from(side) / DISC_MM;
    let circles = [
        DISC_MM / 2.0 * per_mm - 1.0,
        outer,
        inner,
        HOLE_MM / 2.0 * per_mm,
    ];
    let width = (per_mm * 0.25).max(1.0);
    let cross = 3.0 * per_mm;
    let [r, g, b] = GUIDE_COLOR;
    let mut rgba = vec![0u8; n * n * 4];
    for (index, pixel) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = ((index % n) as f32 + 0.5, (index / n) as f32 + 0.5);
        let d = (x - centre).hypot(y - centre);
        let on_circle = circles
            .iter()
            .map(|radius| (width / 2.0 + 0.5 - (d - radius).abs()).clamp(0.0, 1.0))
            .fold(0.0_f32, f32::max);
        let on_cross = if (x - centre).abs() <= cross && (y - centre).abs() <= cross {
            (width / 2.0 + 0.5 - (x - centre).abs().min((y - centre).abs())).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let cover = on_circle.max(on_cross);
        if cover > 0.0 {
            // `as` saturates; the value is within 0..=255.
            *pixel = [r, g, b, (cover * 255.0).round() as u8];
        }
    }
    rgba
}

/// A text of `points` at `dpi` as a menu text: a paragraph box `width` pixels wide (and `height` high, when given) at
/// `(x, y)`, centred.
fn label_text(
    text: &str,
    points: f32,
    dpi: f32,
    (x, y): (f32, f32),
    (width, height): (f32, Option<f32>),
    color: [u8; 3],
) -> MenuText {
    let pixels = |value: f32| -> u16 {
        // `as` saturates; the values are within the canvas.
        value.round().clamp(0.0, f32::from(u16::MAX)) as u16
    };
    MenuText {
        text: text.to_owned(),
        x: pixels(x),
        y: pixels(y),
        size: pixels(points * dpi / 72.0).clamp(
            disccraft_project::MIN_TEXT_SIZE,
            disccraft_project::MAX_TEXT_SIZE,
        ),
        color: Some(color),
        font: None,
        font_style: None,
        format: TextFormat {
            align: TextAlign::Center,
            ..TextFormat::default()
        },
        shadow: None,
        effects: Default::default(),
        orientation: Default::default(),
        vertical: false,
        box_width: Some(pixels(width).clamp(
            disccraft_project::MIN_TEXT_BOX,
            disccraft_project::MAX_TEXT_BOX,
        )),
        box_height: height.map(|height| {
            pixels(height).clamp(
                disccraft_project::MIN_TEXT_BOX,
                disccraft_project::MAX_TEXT_BOX,
            )
        }),
        hidden: false,
        locked: false,
        spans: Vec::new(),
    }
}

/// The room between two entries on a line.
const ENTRY_GAP: &str = "   ";

/// `entries` on lines no wider than `width` pixels at `points` (at `dpi`), each line as many whole entries as fit
/// (an entry is never cut between two lines; one wider than the line has a line of its own).
fn pack_lines(menu: &Menu, entries: &[&str], (points, dpi): (f32, f32), width: f32) -> Vec<String> {
    let measure = |line: &str| -> f32 {
        let mut text = label_text(line, points, dpi, (0.0, 0.0), (width, None), [0, 0, 0]);
        // Point text: as wide as the line is.
        text.box_width = None;
        f32::from(draw::text_rect(menu, &text).w)
    };
    let mut lines: Vec<String> = Vec::new();
    for entry in entries {
        let joined = lines.last().map(|line| format!("{line}{ENTRY_GAP}{entry}"));
        match (joined, lines.last_mut()) {
            (Some(joined), Some(line)) if measure(&joined) <= width => *line = joined,
            _ => lines.push((*entry).to_owned()),
        }
    }
    lines
}

/// `text` (of a menu of `side` pixels) as an editable type layer named `name`, its sizes in points at `dpi`; `None`
/// for an empty text.
fn type_layer(menu: &Menu, text: &MenuText, side: u16, dpi: f32, name: &str) -> Option<PsdLayer> {
    let mut setting = draw::text_type(menu, text)?;
    let to_points = 72.0 / dpi;
    setting.style.size *= to_points;
    setting.style.baseline_shift *= to_points;
    for (_, style) in &mut setting.runs {
        style.size *= to_points;
        style.baseline_shift *= to_points;
    }
    Some(PsdLayer::Text {
        name: name.to_owned(),
        rgba: draw::text_rgba(menu, text, side, side),
        visible: true,
        text: Box::new(setting),
    })
}

/// Writes `label` as a PSD at `path` (see the module).
///
/// # Errors
///
/// When the file cannot be written.
pub fn export_disc_label_psd(label: &DiscLabel, path: &std::path::Path) -> Result<(), MenuError> {
    let (side, dpi) = canvas(label.dpi);
    let (inner, outer) = ring(label, side);
    let centre = f32::from(side) / 2.0;
    let per_mm = f32::from(side) / DISC_MM;
    // Light text over a picture, dark on the white disc.
    let color = if label.picture.is_some() {
        [255, 255, 255]
    } else {
        [32, 32, 32]
    };
    let menu = Menu {
        text_color: color,
        ..Menu::default()
    };
    let mut children = vec![PsdLayer::Raster {
        name: label.names.picture.clone(),
        rgba: picture_rgba(label.picture.as_ref(), side),
        visible: true,
    }];
    // The title: across the middle of the ring's upper half, as wide as the ring allows there.
    let title = label.title.trim();
    if !title.is_empty() {
        let width = outer * 1.4;
        let mut text = label_text(
            title,
            TITLE_POINTS,
            dpi,
            (centre - width / 2.0, 0.0),
            (width, None),
            color,
        );
        let height = f32::from(draw::text_rect(&menu, &text).h);
        let middle = centre - (inner + outer) / 2.0;
        text.y = (middle - height / 2.0).round().max(0.0) as u16;
        if let Some(layer) = type_layer(&menu, &text, side, dpi, title) {
            children.push(layer);
        }
    }
    // The chapters: a box in the lower half, from just under the hole's ring to where the ring is still 1.3 times
    // the outer radius across, at the largest size that fits.
    let lines: Vec<&str> = label
        .lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();
    if !lines.is_empty() {
        let top = centre + inner + 2.0 * per_mm;
        let bottom = centre + outer * 0.75;
        let width = outer * 1.3;
        let place = (centre - width / 2.0, top);
        let mut points = LINES_POINTS.0;
        let fitted = loop {
            let text = label_text(
                &pack_lines(&menu, &lines, (points, dpi), width).join("\n"),
                points,
                dpi,
                place,
                (width, Some(bottom - top)),
                color,
            );
            if points <= LINES_POINTS.1 || !draw::text_overset(&menu, &text) {
                break text;
            }
            points -= 0.5;
        };
        if let Some(layer) = type_layer(&menu, &fitted, side, dpi, &label.names.lines) {
            children.push(layer);
        }
    }
    let mask = ring_mask(side, (inner, outer));
    let guides = PsdLayer::Raster {
        name: label.names.guides.clone(),
        rgba: guides_rgba(side, (inner, outer)),
        visible: false,
    };
    disccraft_photo::write_masked_psd(
        path,
        (u32::from(side), u32::from(side)),
        dpi,
        MaskedGroup {
            name: &label.names.face,
            mask: &mask,
            children: &children,
        },
        std::slice::from_ref(&guides),
    )
    .map_err(|error| MenuError::Invalid(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{
        DiscLabel, LabelNames, canvas, export_disc_label_psd, first_menu, named_chapters, ring,
        ring_mask,
    };
    use disccraft_project::{Chapter, Link, Project, Tick};
    use std::error::Error;

    fn label() -> DiscLabel {
        DiscLabel {
            inner_mm: 22.0,
            outer_mm: 118.0,
            dpi: 300.0,
            title: "Summer Trip 2026".to_owned(),
            lines: vec!["1. Arrival".to_owned(), "2. The Sea".to_owned()],
            picture: None,
            names: LabelNames {
                face: "Disc face".to_owned(),
                picture: "Picture".to_owned(),
                guides: "Guides".to_owned(),
                lines: "Chapters".to_owned(),
            },
        }
    }

    /// M6.506: 120 mm at 300 dpi is 1,417 pixels, and the resolution written makes that exactly 120 mm.
    #[test]
    fn the_label_is_the_disc_at_its_real_size() {
        let (side, dpi) = canvas(300.0);
        assert_eq!(side, 1417);
        assert!((f32::from(side) / dpi * 25.4 - 120.0).abs() < 1e-3);
        assert_eq!(canvas(600.0).0, 2835);
        // Out of range: kept within 150 to 600 dpi.
        assert_eq!(canvas(5000.0).0, 2835);
        assert_eq!(canvas(f32::NAN).0, 1417);
    }

    /// M6.506: the mask shows the printable ring only: not the hole, the clear hub or past the outer diameter.
    #[test]
    fn only_the_printable_ring_is_shown() {
        let (side, _) = canvas(300.0);
        let (inner, outer) = ring(&label(), side);
        let mask = ring_mask(side, (inner, outer));
        let n = usize::from(side);
        let at = |mm_from_centre: f32| {
            let x = (f32::from(side) / 2.0 + mm_from_centre * f32::from(side) / 120.0) as usize;
            mask.get(n / 2 * n + x).copied()
        };
        assert_eq!(at(0.0), Some(0));
        assert_eq!(at(10.0), Some(0), "the hub inside 22 mm");
        assert_eq!(at(12.0), Some(255));
        assert_eq!(at(58.5), Some(255));
        assert_eq!(at(59.5), Some(0), "past 118 mm");
        assert_eq!(mask.first(), Some(&0), "the corner");
        // A ring asked inside out or past the disc is kept within it.
        let odd = DiscLabel {
            inner_mm: 200.0,
            outer_mm: 5.0,
            ..label()
        };
        let (inner, outer) = ring(&odd, side);
        assert!(inner >= 15.0 / 2.0 * f32::from(side) / 120.0 - 0.01 && inner < outer);
    }

    /// M6.506: the PSD reads back at its resolution, with the artwork in a masked group, the texts as type in
    /// points, and hidden guides; flattened, the hole and the corners are clear.
    #[test]
    fn the_label_psd_has_the_disc_in_a_masked_group() -> Result<(), Box<dyn Error>> {
        use photocraft_doc::LayerContent;
        let dir = std::env::temp_dir().join(format!("disccraft-label-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("label.psd");
        export_disc_label_psd(&label(), &path)?;
        let doc = photocraft_io::import("label.psd", &std::fs::read(&path)?)?.document;
        assert!(
            (doc.resolution_dpi - 1417.0 / (120.0 / 25.4)).abs() < 0.05,
            "{}",
            doc.resolution_dpi
        );
        let layers: Vec<&photocraft_doc::Layer> = doc.layers.iter().collect();
        assert_eq!(layers.len(), 2);
        let face = layers.first().ok_or("face")?;
        assert_eq!(face.name, "Disc face");
        assert!(face.mask.is_some());
        let children = face.children().ok_or("group")?;
        let names: Vec<&str> = children.iter().map(|layer| layer.name.as_str()).collect();
        assert_eq!(names, ["Picture", "Summer Trip 2026", "Chapters"]);
        let LayerContent::Text(title) = &children.get(1).ok_or("title")?.content else {
            return Err("the title is not type".into());
        };
        assert!((title.size_pt - 16.0).abs() < 0.2, "{}", title.size_pt);
        let guides = layers.get(1).ok_or("guides")?;
        assert_eq!(guides.name, "Guides");
        assert!(!guides.visible);
        let image = disccraft_photo::load_image(&path)?;
        let n = image.width as usize;
        let alpha = |x: usize, y: usize| image.rgba.get((y * n + x) * 4 + 3).copied();
        assert_eq!(alpha(n / 2, n / 2), Some(0), "the hole");
        assert_eq!(alpha(0, 0), Some(0), "the corner");
        assert_eq!(alpha(n / 2, n / 2 + n / 4), Some(255), "the ring");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// M6.506: the chapters are set as whole entries on their lines, never cut between two, in their order; at a
    /// width narrower than any entry, one entry a line.
    #[test]
    fn the_chapters_are_whole_on_their_lines() {
        use super::{ENTRY_GAP, pack_lines};
        let menu = disccraft_project::Menu::default();
        let entries = ["1. Opening", "2. A rather long name of a chapter", "3. End"];
        let lines = pack_lines(&menu, &entries, (8.0, 300.0), 600.0);
        assert!(lines.len() >= 2, "{lines:?}");
        assert_eq!(lines.join(ENTRY_GAP), entries.join(ENTRY_GAP));
        for entry in entries {
            assert!(lines.iter().any(|line| line.contains(entry)), "{entry}");
        }
        assert_eq!(pack_lines(&menu, &entries, (8.0, 300.0), 10.0), entries);
    }

    /// M6.506: a label lists the chapters with names of their own of the timeline that has the most, numbered as
    /// they are on the disc; a chapter named by default ("Chapter 2") is left out. Its picture starts as the menu
    /// the disc plays first.
    #[test]
    fn the_label_lists_the_named_chapters() -> Result<(), String> {
        let mut project = Project::default();
        let other = project
            .add_menu(Some("Other".to_owned()))
            .map_err(|error| error.to_string())?;
        let top = project
            .add_menu(Some("Top".to_owned()))
            .map_err(|error| error.to_string())?;
        assert_eq!(first_menu(&project), Some(other));
        project.settings.first_play = Link::Menu {
            id: top,
            button: None,
        };
        assert_eq!(first_menu(&project), Some(top));
        assert!(named_chapters(&project).is_empty());
        let video = project
            .add_asset(std::path::Path::new("a.m2ts"))
            .map_err(|error| error.to_string())?;
        let id = project
            .add_timeline(video, None)
            .map_err(|error| error.to_string())?;
        let chapter = |seconds: i64, name: &str| Chapter {
            time: Tick(seconds * 254_016_000_000),
            name: name.to_owned(),
            poster: None,
        };
        project.timeline_mut(id).ok_or("timeline")?.chapters = vec![
            chapter(0, "Opening"),
            chapter(60, "Chapter 2"),
            chapter(120, " Finale "),
        ];
        assert_eq!(named_chapters(&project), ["1. Opening", "3. Finale"]);
        Ok(())
    }
}
