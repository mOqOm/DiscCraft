//! Writing a layered PSD for editing a menu in PhotoCraft (or any PSD editor), like Encore's "Edit Menu in
//! Photoshop". The document is built with PhotoCraft's document model and saved by PhotoCraft's PSD writer.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use photocraft_doc::text::{
    AntiAlias, Caps, CharStyle, FontBaseline, Orientation, ParagraphRun, ParagraphStyle, TextAlign,
    TextRun, TextShape,
};
use photocraft_doc::{
    BlendMode, Color, ColorMode, Document, Effect, FxCommon, Layer, LayerContent, PixelFormat,
    Rect, SampleType, Size, Surface, TextLayer,
};
use photocraft_geom::Affine;

use crate::{MAX_DIMENSION, PhotoError};

/// One layer of a PSD written by [`write_layered_psd`].
#[derive(Debug, Clone, PartialEq)]
pub enum PsdLayer {
    /// Pixels covering the whole canvas: straight-alpha RGBA, `width * height * 4` bytes.
    Raster {
        /// Layer name.
        name: String,
        /// The pixels.
        rgba: Vec<u8>,
        /// Whether the layer is shown.
        visible: bool,
    },
    /// An editable type layer (M6.398, as Encore writes a menu's texts): Photoshop shows `rgba` (straight-alpha RGBA
    /// of the whole canvas, `width * height * 4` bytes: the text as DiscCraft draws it) until the text is edited,
    /// then sets [`PsdText`] itself.
    Text {
        /// Layer name.
        name: String,
        /// The pixels the layer shows.
        rgba: Vec<u8>,
        /// Whether the layer is shown.
        visible: bool,
        /// The text and how it is set.
        text: Box<PsdText>,
    },
    /// A group of layers, bottom to top.
    Group {
        /// Group name.
        name: String,
        /// Its PSD layer id (`lyid`), which Photoshop keeps while the group is edited or renamed and gives a copy
        /// anew (M10.49: a button set's, so the menu knows the set again); `None` lets the writer choose.
        id: Option<u32>,
        /// The layers in the group, bottom to top.
        children: Vec<PsdLayer>,
    },
}

/// The text of a type layer ([`PsdLayer::Text`]) and how it is set (Photoshop's Character and Paragraph panels).
#[derive(Debug, Clone, PartialEq)]
pub struct PsdText {
    /// The text; a line break starts a new paragraph.
    pub text: String,
    /// Its character formatting.
    pub style: PsdCharStyle,
    /// Parts of the text set otherwise: byte ranges of `text` (in order, not overlapping) and their formatting.
    pub runs: Vec<(std::ops::Range<usize>, PsdCharStyle)>,
    /// How its lines line up.
    pub align: PsdTextAlign,
    /// Vertical type: columns from the top, the first on the right.
    pub vertical: bool,
    /// A paragraph text's box in text space (left, top, width, height); `None`: point text.
    pub area: Option<[f32; 4]>,
    /// From text space to the canvas (`a, b, c, d, e, f`: x' = a x + c y + e, y' = b x + d y + f). Text space has
    /// its origin on the first baseline at the alignment's anchor (point text: the left end, the middle or the
    /// right end of the line; a vertical text's: the top, middle or bottom of its first column, on its centre).
    pub transform: [f64; 6],
    /// How its edges are smoothed.
    pub antialias: PsdAntialias,
    /// A drop shadow (Photoshop's layer style).
    pub shadow: Option<PsdShadow>,
}

/// Character formatting of a [`PsdText`].
#[derive(Debug, Clone, PartialEq)]
pub struct PsdCharStyle {
    /// Font family.
    pub family: String,
    /// The family's style ("Bold"); `None`: the regular one.
    pub style: Option<String>,
    /// The face's PostScript name ("Arial-BoldMT"), by which Photoshop finds it; `None`: from the family.
    pub postscript: Option<String>,
    /// Size in pixels (points: the PSD is 72 dpi).
    pub size: f32,
    /// Colour.
    pub color: [u8; 3],
    /// Faux bold.
    pub faux_bold: bool,
    /// Faux italic.
    pub faux_italic: bool,
    /// Underline.
    pub underline: bool,
    /// Strikethrough.
    pub strikethrough: bool,
    /// All caps / small caps.
    pub caps: PsdCaps,
    /// Superscript / subscript.
    pub script: PsdScript,
    /// Tracking in 1/1000 em.
    pub tracking: f32,
    /// Baseline shift in pixels, positive up.
    pub baseline_shift: f32,
}

/// Capitals of a [`PsdCharStyle`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PsdCaps {
    /// As typed.
    #[default]
    Normal,
    /// All capitals.
    All,
    /// Small capitals.
    Small,
}

/// Superscript / subscript of a [`PsdCharStyle`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PsdScript {
    /// On the baseline.
    #[default]
    Normal,
    /// Smaller and raised.
    Superscript,
    /// Smaller and lowered.
    Subscript,
}

/// Line alignment of a [`PsdText`] (a vertical text's: top, middle, bottom).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PsdTextAlign {
    /// Left (top).
    #[default]
    Left,
    /// Centred.
    Center,
    /// Right (bottom).
    Right,
    /// Justified, the last line left.
    Justify,
}

/// Anti-aliasing of a [`PsdText`] (Photoshop's).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PsdAntialias {
    /// None.
    None,
    /// Sharp.
    Sharp,
    /// Crisp.
    #[default]
    Crisp,
    /// Strong.
    Strong,
    /// Smooth.
    Smooth,
}

/// A drop shadow (Photoshop's layer style).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PsdShadow {
    /// Colour.
    pub color: [u8; 3],
    /// Opacity, 0 to 1.
    pub opacity: f32,
    /// Where the light comes from, degrees counter-clockwise from the right.
    pub angle: f32,
    /// Distance in pixels.
    pub distance: f32,
    /// Size (blur) in pixels.
    pub size: f32,
    /// Spread, 0 to 1.
    pub spread: f32,
}

/// Write `layers` (bottom to top) as an 8-bit RGB PSD of `width` x `height`.
///
/// # Errors
///
/// Returns an error for a size out of range, a raster layer whose bytes do not match the canvas, or a failure to
/// encode or write the file.
pub fn write_layered_psd(
    path: &Path,
    width: u32,
    height: u32,
    layers: &[PsdLayer],
) -> Result<(), PhotoError> {
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(PhotoError::Invalid(format!(
            "canvas size {width}x{height} out of range"
        )));
    }
    let name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("menu");
    let mut doc = Document::new(name, Size { width, height }, ColorMode::Rgb, SampleType::U8);
    let format = doc.pixel_format();
    let canvas = doc.bounds();
    let expected = usize::try_from(width)
        .ok()
        .zip(usize::try_from(height).ok())
        .and_then(|(w, h)| w.checked_mul(h))
        .and_then(|area| area.checked_mul(4))
        .ok_or_else(|| PhotoError::Invalid("canvas too large".to_string()))?;
    doc.layers = layers
        .iter()
        .map(|layer| build_layer(layer, format, canvas, expected))
        .collect::<Result<_, _>>()?;
    save(&doc, path)
}

/// What is laid over a menu's own PSD when the menu is written out again (M6.399): the PSD keeps its layers, and
/// what was added to the menu since goes over them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PsdAdditions {
    /// The `(#)` page group the additions go into (1 for the lowest); `None`: the document's top level.
    pub page: Option<usize>,
    /// Leave the page groups out (a page added to a menu shown by page shows only the layers its pages share).
    pub shared_only: bool,
    /// Layers laid over the page, bottom to top.
    pub layers: Vec<PsdLayer>,
    /// The menu's buttons, bottom to top.
    pub buttons: Vec<PsdButtonAddition>,
}

/// A button of [`PsdAdditions`]: when the PSD has its button set (by its layer id, else its name), `into` goes into
/// that set; else the whole `set` is laid over the page.
#[derive(Debug, Clone, PartialEq)]
pub struct PsdButtonAddition {
    /// The PSD layer id of its set.
    pub id: Option<u32>,
    /// Its set's name after the `(+)` (empty: found by its id only).
    pub name: String,
    /// Layers laid at the top of its set when the PSD has the set (its text added since).
    pub into: Vec<PsdLayer>,
    /// The whole set, for a button the PSD has no set for.
    pub set: PsdLayer,
}

/// Write the PSD (or PhotoCraft document) at `source` to `path` as a PSD with all its own layers, and `additions`
/// (drawn on a `width` x `height` canvas) laid over them (M6.399: a menu made from a PSD, or edited in an image
/// editor, is no longer flattened into one picture when it is written out). `Ok(false)`, writing nothing, when the
/// document's canvas is another size.
///
/// # Errors
///
/// When the document cannot be read, an added layer's bytes do not match the canvas, or the file cannot be written.
pub fn write_psd_over(
    source: &Path,
    path: &Path,
    (width, height): (u32, u32),
    additions: &PsdAdditions,
) -> Result<bool, PhotoError> {
    let (mut doc, canvas) = crate::load_document(source)?;
    if crate::rect_width_u32(canvas) != Some(width)
        || crate::rect_height_u32(canvas) != Some(height)
    {
        return Ok(false);
    }
    let format = doc.pixel_format();
    let expected = usize::try_from(width)
        .ok()
        .zip(usize::try_from(height).ok())
        .and_then(|(w, h)| w.checked_mul(h))
        .and_then(|area| area.checked_mul(4))
        .ok_or_else(|| PhotoError::Invalid("canvas too large".to_string()))?;
    let build = |layer: &PsdLayer| build_layer(layer, format, canvas, expected);
    let mut top: Vec<Layer> = additions
        .layers
        .iter()
        .map(build)
        .collect::<Result<_, _>>()?;
    let mut buttons = Vec::with_capacity(additions.buttons.len());
    for button in &additions.buttons {
        buttons.push((
            button.id,
            button.name.trim().to_owned(),
            button
                .into
                .iter()
                .map(build)
                .collect::<Result<Vec<_>, _>>()?,
            build(&button.set)?,
        ));
    }

    if additions.shared_only {
        doc.layers.retain(|layer| !crate::is_page_group(layer));
    }
    // The page group the additions go into, by its place among the top level's layers.
    let page = additions
        .page
        .filter(|page| *page >= 1 && !additions.shared_only)
        .and_then(|page| {
            doc.layers
                .iter()
                .enumerate()
                .filter(|(_, layer)| crate::is_page_group(layer))
                .nth(page - 1)
                .map(|(index, _)| index)
        });
    let target: &mut Vec<Layer> = &mut doc.layers;
    match page.and_then(|index| target.get_mut(index)) {
        Some(Layer {
            content: LayerContent::Group(group),
            ..
        }) => lay_over(&mut group.children, &mut top, buttons),
        _ => lay_over(target, &mut top, buttons),
    }
    save(&doc, path)?;
    Ok(true)
}

/// Lays `buttons` (id, name, layers for its set, the whole set) into their sets in `layers` (a page's), the sets
/// the page does not have over the page after `top`.
#[allow(clippy::type_complexity)]
fn lay_over(
    layers: &mut Vec<Layer>,
    top: &mut Vec<Layer>,
    buttons: Vec<(Option<u32>, String, Vec<Layer>, Layer)>,
) {
    let mut sets = Vec::new();
    for (id, name, into, set) in buttons {
        let path = id
            .and_then(|id| set_path(layers, &|layer| layer.psd_id == Some(id)))
            .or_else(|| {
                (!name.is_empty())
                    .then(|| {
                        set_path(layers, &|layer| {
                            crate::button_name(&layer.name).as_deref() == Some(name.as_str())
                        })
                    })
                    .flatten()
            });
        match path.and_then(|path| children_at(layers, &path)) {
            Some(children) => children.extend(into),
            None => sets.push(set),
        }
    }
    layers.append(top);
    layers.extend(sets);
}

/// Where the first button set `wanted` takes is among `layers` (indexes down the groups), not looking into page
/// groups (another page's sets).
fn set_path(layers: &[Layer], wanted: &dyn Fn(&Layer) -> bool) -> Option<Vec<usize>> {
    for (index, layer) in layers.iter().enumerate() {
        let LayerContent::Group(group) = &layer.content else {
            continue;
        };
        if crate::is_button_name(&layer.name) {
            if wanted(layer) {
                return Some(vec![index]);
            }
        } else if !crate::is_page_group(layer)
            && let Some(mut rest) = set_path(&group.children, wanted)
        {
            rest.insert(0, index);
            return Some(rest);
        }
    }
    None
}

/// The layers of the group at `path` (from [`set_path`]).
fn children_at<'a>(layers: &'a mut [Layer], path: &[usize]) -> Option<&'a mut Vec<Layer>> {
    let (first, rest) = path.split_first()?;
    let LayerContent::Group(group) = &mut layers.get_mut(*first)?.content else {
        return None;
    };
    if rest.is_empty() {
        Some(&mut group.children)
    } else {
        children_at(&mut group.children, rest)
    }
}

/// Exports `doc` as a PSD to `path`.
fn save(doc: &Document, path: &Path) -> Result<(), PhotoError> {
    let display = path.display().to_string();
    let exported = catch_unwind(AssertUnwindSafe(|| {
        photocraft_io::export(doc, "psd", &photocraft_io::ExportOptions::default())
    }))
    .map_err(|_| PhotoError::Invalid("internal error while writing the PSD".to_string()))?
    .map_err(|error| PhotoError::Io {
        path: display.clone(),
        message: error.to_string(),
    })?;
    std::fs::write(path, exported.bytes).map_err(|error| PhotoError::Io {
        path: display,
        message: error.to_string(),
    })
}

fn build_layer(
    layer: &PsdLayer,
    format: PixelFormat,
    canvas: Rect,
    expected: usize,
) -> Result<Layer, PhotoError> {
    match layer {
        PsdLayer::Raster {
            name,
            rgba,
            visible,
        } => {
            if rgba.len() != expected {
                return Err(PhotoError::Invalid(format!(
                    "layer {name}: {} bytes for a {expected}-byte canvas",
                    rgba.len()
                )));
            }
            let mut out = Layer::raster(name.clone(), format);
            out.visible = *visible;
            if let Some(surface) = out.surface_mut() {
                surface
                    .try_write_interleaved(canvas, rgba)
                    .map_err(|error| PhotoError::Invalid(format!("layer {name}: {error:?}")))?;
            }
            Ok(out)
        }
        PsdLayer::Text {
            name,
            rgba,
            visible,
            text,
        } => {
            if rgba.len() != expected {
                return Err(PhotoError::Invalid(format!(
                    "layer {name}: {} bytes for a {expected}-byte canvas",
                    rgba.len()
                )));
            }
            let mut look = Layer::raster(name.clone(), format);
            if let Some(surface) = look.surface_mut() {
                surface
                    .try_write_interleaved(canvas, rgba)
                    .map_err(|error| PhotoError::Invalid(format!("layer {name}: {error:?}")))?;
            }
            let LayerContent::Raster(cache) = look.content else {
                return Err(PhotoError::Invalid(format!("layer {name}: no pixels")));
            };
            let mut out = Layer::new(name.clone(), LayerContent::Text(type_layer(text, cache)));
            out.visible = *visible;
            if let Some(shadow) = &text.shadow {
                out.effects.enabled = true;
                out.effects.items.push(drop_shadow(shadow));
            }
            Ok(out)
        }
        PsdLayer::Group { name, id, children } => {
            let children = children
                .iter()
                .map(|child| build_layer(child, format, canvas, expected))
                .collect::<Result<Vec<_>, _>>()?;
            let mut group = Layer::group(name.clone(), children);
            group.psd_id = *id;
            Ok(group)
        }
    }
}

/// PhotoCraft's type layer for `text`, showing `cache` until it is edited.
fn type_layer(text: &PsdText, cache: Surface) -> TextLayer {
    let len = text.text.len();
    // The runs cover the whole text: the parts set otherwise, the base style between them.
    let mut runs: Vec<TextRun> = Vec::new();
    let mut at = 0_usize;
    let push = |runs: &mut Vec<TextRun>, len: usize, style: &PsdCharStyle| {
        if len > 0 {
            runs.push(TextRun {
                len,
                style: char_style(style),
            });
        }
    };
    for (range, style) in &text.runs {
        let (start, end) = (range.start.min(len), range.end.min(len));
        if start < at || start >= end {
            continue;
        }
        push(&mut runs, start - at, &text.style);
        push(&mut runs, end - start, style);
        at = end;
    }
    push(&mut runs, len - at, &text.style);
    let align = match (text.align, text.area.is_some()) {
        (PsdTextAlign::Left, _) | (PsdTextAlign::Justify, false) => TextAlign::Left,
        (PsdTextAlign::Center, _) => TextAlign::Center,
        (PsdTextAlign::Right, _) => TextAlign::Right,
        (PsdTextAlign::Justify, true) => TextAlign::JustifyLeft,
    };
    TextLayer {
        text: text.text.clone(),
        font_family: text.style.family.clone(),
        size_pt: text.style.size,
        color: color_of(text.style.color),
        transform: Affine { m: text.transform },
        cache: Some(cache),
        psd_raw: None,
        runs,
        paragraphs: vec![ParagraphRun {
            len,
            style: ParagraphStyle {
                align,
                ..ParagraphStyle::default()
            },
        }],
        shape: match text.area {
            Some([x, y, width, height]) => TextShape::Box {
                x,
                y,
                width,
                height,
            },
            None => TextShape::Point,
        },
        orientation: if text.vertical {
            Orientation::Vertical
        } else {
            Orientation::Horizontal
        },
        antialias: match text.antialias {
            PsdAntialias::None => AntiAlias::None,
            PsdAntialias::Sharp => AntiAlias::Sharp,
            PsdAntialias::Crisp => AntiAlias::Crisp,
            PsdAntialias::Strong => AntiAlias::Strong,
            PsdAntialias::Smooth => AntiAlias::Smooth,
        },
        warp: None,
    }
}

fn char_style(style: &PsdCharStyle) -> CharStyle {
    let mut out = CharStyle {
        font_family: style.family.clone(),
        size_pt: style.size,
        color: color_of(style.color),
        tracking: style.tracking,
        baseline_shift_pt: style.baseline_shift,
        underline: style.underline,
        strikethrough: style.strikethrough,
        faux_bold: style.faux_bold,
        faux_italic: style.faux_italic,
        caps: match style.caps {
            PsdCaps::Normal => Caps::Normal,
            PsdCaps::All => Caps::AllCaps,
            PsdCaps::Small => Caps::SmallCaps,
        },
        baseline: match style.script {
            PsdScript::Normal => FontBaseline::Normal,
            PsdScript::Superscript => FontBaseline::Superscript,
            PsdScript::Subscript => FontBaseline::Subscript,
        },
        ..CharStyle::default()
    };
    if let Some(face) = &style.style {
        out.font_style.clone_from(face);
    }
    out.postscript_name.clone_from(&style.postscript);
    out
}

fn color_of([r, g, b]: [u8; 3]) -> Color {
    Color::rgb(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
    )
}

fn drop_shadow(shadow: &PsdShadow) -> Effect {
    let mut effect = Effect::default_drop_shadow();
    if let Effect::DropShadow(fx) = &mut effect {
        fx.common = FxCommon::new(BlendMode::Multiply, shadow.opacity.clamp(0.0, 1.0));
        fx.color = color_of(shadow.color);
        fx.angle = shadow.angle;
        // The shadow keeps its own angle, not the document's light.
        fx.use_global_light = false;
        fx.distance = shadow.distance.max(0.0);
        fx.size = shadow.size.max(0.0);
        fx.spread = shadow.spread.clamp(0.0, 1.0);
    }
    effect
}

#[cfg(test)]
mod tests {
    use super::{PsdLayer, write_layered_psd};
    use crate::{load_image, load_menu_background, load_psd_menu};
    use std::error::Error;

    fn solid(width: usize, height: usize, rgba: [u8; 4]) -> Vec<u8> {
        rgba.repeat(width * height)
    }

    /// M6.343: a PSD layer's layer effects (Photoshop layer styles) are in the menu read from it: a red stroke
    /// round a white square in the PSD is red round it in the menu's background.
    #[test]
    fn a_psd_layers_effects_are_in_the_menu() -> Result<(), Box<dyn Error>> {
        use photocraft_doc::{
            BlendMode, Color, Effect, FxCommon, FxPaint, StrokeFx, StrokePosition,
        };
        let dir = std::env::temp_dir().join(format!("disccraft-psd-fx-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let plain = dir.join("plain.psd");
        let (w, h) = (40_usize, 40_usize);
        let mut square = vec![0u8; w * h * 4];
        for y in 10..30 {
            for x in 10..30 {
                let i = (y * w + x) * 4;
                if let Some(px) = square.get_mut(i..i + 4) {
                    px.copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
        write_layered_psd(
            &plain,
            40,
            40,
            &[
                PsdLayer::Raster {
                    name: "背景".to_owned(),
                    rgba: solid(w, h, [0, 0, 0, 255]),
                    visible: true,
                },
                PsdLayer::Raster {
                    name: "square".to_owned(),
                    rgba: square,
                    visible: true,
                },
            ],
        )?;
        // The square gets a stroke in PhotoCraft (as a PSD made in Photoshop with a layer style has it).
        let mut doc = photocraft_io::import("plain.psd", &std::fs::read(&plain)?)?.document;
        let layer = doc.layers.get_mut(1).ok_or("square layer")?;
        layer.effects.enabled = true;
        layer.effects.items.push(Effect::Stroke(StrokeFx {
            common: FxCommon::new(BlendMode::Normal, 1.0),
            size: 4.0,
            position: StrokePosition::Outside,
            paint: FxPaint::Color(Color::rgb(1.0, 0.0, 0.0)),
            overprint: false,
        }));
        let styled = dir.join("styled.psd");
        let exported =
            photocraft_io::export(&doc, "psd", &photocraft_io::ExportOptions::default())?;
        std::fs::write(&styled, exported.bytes)?;
        let menu = load_psd_menu(&styled)?;
        let _ = std::fs::remove_dir_all(&dir);
        let at = |x: usize, y: usize| -> Option<[u8; 4]> {
            let i = (y * menu.background.width as usize + x) * 4;
            let px = menu.background.rgba.get(i..i + 4)?;
            Some([px[0], px[1], px[2], px[3]])
        };
        // Inside the square white, just outside it red, further out the black background.
        assert_eq!(at(20, 20), Some([255, 255, 255, 255]));
        let edge = at(8, 20).ok_or("pixel")?;
        assert!(edge[0] > 200 && edge[1] < 60 && edge[2] < 60, "{edge:?}");
        assert_eq!(at(2, 20), Some([0, 0, 0, 255]));
        Ok(())
    }

    #[test]
    fn a_thumbnail_layer_marks_where_the_video_goes() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-psd-thumb-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("menu.psd");
        let (w, h) = (16_usize, 8_usize);
        let area = |x0: usize, y0: usize, x1: usize, y1: usize| {
            let mut rgba = vec![0u8; w * h * 4];
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = (y * w + x) * 4;
                    if let Some(px) = rgba.get_mut(i..i + 4) {
                        px.copy_from_slice(&[128, 128, 128, 255]);
                    }
                }
            }
            rgba
        };
        let layers = vec![
            PsdLayer::Raster {
                name: "背景".to_owned(),
                rgba: solid(w, h, [0, 0, 0, 255]),
                visible: true,
            },
            PsdLayer::Group {
                name: "(+) Scene".to_owned(),
                id: None,
                children: vec![
                    PsdLayer::Raster {
                        name: "frame".to_owned(),
                        rgba: area(1, 1, 15, 7),
                        visible: true,
                    },
                    // Hidden still counts: it marks the place.
                    PsdLayer::Raster {
                        name: "(%) video".to_owned(),
                        rgba: area(2, 2, 14, 6),
                        visible: false,
                    },
                ],
            },
        ];
        write_layered_psd(&path, 16, 8, &layers)?;
        let menu = load_psd_menu(&path)?;
        let button = menu.buttons.first().ok_or("no button")?;
        assert_eq!(button.thumbnail, Some([2, 2, 12, 4]));
        assert_eq!(
            (button.x, button.y, button.width, button.height),
            (1, 1, 14, 6)
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn a_written_menu_reads_back_with_its_buttons() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-psd-write-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("menu.psd");
        let (w, h) = (16_usize, 8_usize);
        // A button covering x 4..8, y 2..6, with a hidden highlight.
        let mut label = vec![0u8; w * h * 4];
        for y in 2..6 {
            for x in 4..8 {
                let i = (y * w + x) * 4;
                if let Some(px) = label.get_mut(i..i + 4) {
                    px.copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
        // The highlight covers x 2..10, y 1..7, a bit larger than the label.
        let mut highlight = vec![0u8; w * h * 4];
        for y in 1..7 {
            for x in 2..10 {
                let i = (y * w + x) * 4;
                if let Some(px) = highlight.get_mut(i..i + 4) {
                    px.copy_from_slice(&[255, 255, 0, 255]);
                }
            }
        }
        let layers = vec![
            PsdLayer::Raster {
                name: "背景".to_owned(),
                rgba: solid(w, h, [0, 0, 128, 255]),
                visible: true,
            },
            PsdLayer::Group {
                name: "(+) Play".to_owned(),
                id: None,
                children: vec![
                    PsdLayer::Raster {
                        name: "Play".to_owned(),
                        rgba: label,
                        visible: true,
                    },
                    PsdLayer::Raster {
                        name: "(=1) ハイライト".to_owned(),
                        rgba: highlight,
                        visible: false,
                    },
                ],
            },
        ];
        write_layered_psd(&path, 16, 8, &layers)?;

        let menu = load_psd_menu(&path)?;
        assert_eq!((menu.width, menu.height), (16, 8));
        assert_eq!(menu.buttons.len(), 1);
        assert_eq!(
            menu.buttons.first().and_then(|button| button.thumbnail),
            None
        );
        let button = menu.buttons.first().ok_or("no button")?;
        assert_eq!(
            (
                button.name.as_str(),
                button.x,
                button.y,
                button.width,
                button.height
            ),
            ("Play", 2, 1, 8, 6)
        );
        // The background has the label but not the highlight.
        let background = load_menu_background(&path)?;
        assert_eq!(background.rgba.get(0..4), Some(&[0, 0, 128, 255][..]));
        let label_pixel = (2 * w + 4) * 4;
        assert_eq!(
            background.rgba.get(label_pixel..label_pixel + 4),
            Some(&[255, 255, 255, 255][..])
        );
        assert_eq!(load_image(&path)?.width, 16);

        // Wrong sizes are refused.
        let bad = vec![PsdLayer::Raster {
            name: "x".to_owned(),
            rgba: vec![0; 3],
            visible: true,
        }];
        assert!(write_layered_psd(&path, 16, 8, &bad).is_err());
        assert!(write_layered_psd(&path, 0, 8, &[]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn each_subpicture_layer_is_its_colour_and_the_top_one_wins() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-psd-sub-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("menu.psd");
        let (w, h) = (8_usize, 4_usize);
        // A layer of `colour` at `alpha` over x0..x1 of rows 1..3.
        let strip = |x0: usize, x1: usize, colour: [u8; 3], alpha: u8| {
            let mut rgba = vec![0u8; w * h * 4];
            for y in 1..3 {
                for x in x0..x1 {
                    let i = (y * w + x) * 4;
                    if let Some(px) = rgba.get_mut(i..i + 4) {
                        px.copy_from_slice(&[colour[0], colour[1], colour[2], alpha]);
                    }
                }
            }
            rgba
        };
        let layers = vec![
            PsdLayer::Raster {
                name: "背景".to_owned(),
                rgba: solid(w, h, [0, 0, 0, 255]),
                visible: true,
            },
            PsdLayer::Group {
                name: "(+) Play".to_owned(),
                id: None,
                children: vec![
                    // Bottom to top: colour 1 over x 1..7, colour 3 over x 4..6 (any colour of its own), colour 2
                    // over x 5..7, and a faint colour 2 over x 1..2 that does not count.
                    PsdLayer::Raster {
                        name: "(=1) 縁".to_owned(),
                        rgba: strip(1, 7, [255, 0, 0], 255),
                        visible: false,
                    },
                    PsdLayer::Raster {
                        name: "(=3)".to_owned(),
                        rgba: strip(4, 6, [0, 0, 0], 255),
                        visible: true,
                    },
                    PsdLayer::Raster {
                        name: "(=2) 中".to_owned(),
                        rgba: strip(5, 7, [9, 9, 9], 255),
                        visible: false,
                    },
                    PsdLayer::Raster {
                        name: "(=2) かすか".to_owned(),
                        rgba: strip(1, 2, [9, 9, 9], 60),
                        visible: false,
                    },
                ],
            },
        ];
        write_layered_psd(&path, 8, 4, &layers)?;
        let menu = load_psd_menu(&path)?;
        let button = menu.buttons.first().ok_or("no button")?;
        assert_eq!(
            (button.x, button.y, button.width, button.height),
            (1, 1, 6, 2)
        );
        let sub = button.subpicture.as_ref().ok_or("no subpicture")?;
        assert_eq!((sub.width, sub.height), (6, 2));
        assert_eq!(sub.values, [1, 1, 1, 3, 2, 2].repeat(2));
        // Each layer's own colour (the first one of a number).
        assert_eq!(
            sub.colors,
            [Some([255, 0, 0]), Some([9, 9, 9]), Some([0, 0, 0])]
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// M10.49: a button set's PSD layer id is written and read back, so a menu knows the set again after the PSD is
    /// edited (Photoshop keeps the id while the set is moved or renamed).
    #[test]
    fn a_button_sets_layer_id_is_kept() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-psd-lyid-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("menu.psd");
        let (w, h) = (8_usize, 4_usize);
        let mut block = vec![0u8; w * h * 4];
        for px in block.chunks_mut(4).take(8) {
            px.copy_from_slice(&[255, 255, 255, 255]);
        }
        let set = |name: &str, id: Option<u32>| PsdLayer::Group {
            name: name.to_owned(),
            id,
            children: vec![PsdLayer::Raster {
                name: "art".to_owned(),
                rgba: block.clone(),
                visible: true,
            }],
        };
        write_layered_psd(
            &path,
            8,
            4,
            &[set("(+) Play", Some(41)), set("(+) Scenes", Some(7))],
        )?;
        let menu = load_psd_menu(&path)?;
        let ids: Vec<(&str, Option<u32>)> = menu
            .buttons
            .iter()
            .map(|button| (button.name.as_str(), button.layer_id))
            .collect();
        assert_eq!(ids, [("Play", Some(41)), ("Scenes", Some(7))]);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// M6.398: a type layer is written as Photoshop's editable type (its text, family, PostScript name, size,
    /// colour, alignment, box and transform, its drop shadow as a layer style) and shows the pixels it was given
    /// until it is edited.
    #[test]
    fn a_type_layer_is_editable_text_showing_the_given_pixels() -> Result<(), Box<dyn Error>> {
        use super::{PsdCharStyle, PsdShadow, PsdText, PsdTextAlign};
        use photocraft_doc::text::{TextAlign, TextShape};
        use photocraft_doc::{Effect, LayerContent};
        let dir = std::env::temp_dir().join(format!("disccraft-psd-type-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("type.psd");
        let (w, h) = (64_usize, 48_usize);
        // The look: a white bar where the text is drawn.
        let mut look = vec![0u8; w * h * 4];
        for y in 10..20 {
            for x in 8..40 {
                let i = (y * w + x) * 4;
                look[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        let style = PsdCharStyle {
            family: "Arial".to_owned(),
            style: Some("Bold".to_owned()),
            postscript: Some("Arial-BoldMT".to_owned()),
            size: 12.0,
            color: [255, 255, 255],
            faux_bold: false,
            faux_italic: false,
            underline: false,
            strikethrough: false,
            caps: Default::default(),
            script: Default::default(),
            tracking: 0.0,
            baseline_shift: 0.0,
        };
        let red = PsdCharStyle {
            color: [255, 0, 0],
            ..style.clone()
        };
        let text = PsdText {
            text: "Play all".to_owned(),
            style,
            runs: vec![(5..8, red)],
            align: PsdTextAlign::Center,
            vertical: false,
            area: Some([0.0, 0.0, 40.0, 20.0]),
            transform: [1.0, 0.0, 0.0, 1.0, 8.0, 10.0],
            antialias: Default::default(),
            shadow: Some(PsdShadow {
                color: [0, 0, 0],
                opacity: 0.5,
                angle: 120.0,
                distance: 3.0,
                size: 2.0,
                spread: 0.0,
            }),
        };
        write_layered_psd(
            &path,
            64,
            48,
            &[PsdLayer::Text {
                name: "Play all".to_owned(),
                rgba: look,
                visible: true,
                text: Box::new(text),
            }],
        )?;
        let doc = photocraft_io::import("type.psd", &std::fs::read(&path)?)?.document;
        let layer = doc.layers.first().ok_or("layer")?;
        let LayerContent::Text(typed) = &layer.content else {
            return Err("not a type layer".into());
        };
        assert_eq!(typed.text.trim_end(), "Play all");
        assert_eq!(typed.font_family, "Arial");
        assert!((typed.size_pt - 12.0).abs() < 0.01);
        let runs = typed.char_runs();
        assert_eq!(runs.iter().map(|run| run.len).collect::<Vec<_>>(), [5, 3]);
        let last = runs.last().ok_or("run")?;
        assert_eq!(last.style.postscript_name.as_deref(), Some("Arial-BoldMT"));
        assert_eq!(last.style.color.to_rgba8(), [255, 0, 0, 255]);
        assert_eq!(
            typed.paragraph_runs().first().map(|run| run.style.align),
            Some(TextAlign::Center)
        );
        assert!(
            matches!(typed.shape, TextShape::Box { width, height, .. } if (width - 40.0).abs() < 0.01 && (height - 20.0).abs() < 0.01),
            "{:?}",
            typed.shape
        );
        assert!(
            (typed.transform.m[4] - 8.0).abs() < 0.01 && (typed.transform.m[5] - 10.0).abs() < 0.01
        );
        assert!(layer.effects.enabled);
        assert!(matches!(
            layer.effects.items.first(),
            Some(Effect::DropShadow(shadow)) if (shadow.distance - 3.0).abs() < 0.01 && !shadow.use_global_light
        ));
        // The given pixels are what the layer shows (the shadow is drawn beside them).
        let image = load_image(&path)?;
        let at = |x: usize, y: usize| {
            let i = (y * w + x) * 4;
            image
                .rgba
                .get(i..i + 4)
                .map(|px| [px[0], px[1], px[2], px[3]])
        };
        assert_eq!(at(20, 15), Some([255, 255, 255, 255]));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// M6.399: written over a PSD with pages, the additions go into the page's `(#)` group (a button's new text
    /// into its set there, a button without one as a new set) and every layer of the PSD stays; a page that shows
    /// only the shared layers leaves the page groups out; a canvas of another size writes nothing.
    #[test]
    fn additions_go_over_their_page_of_the_psd() -> Result<(), Box<dyn Error>> {
        use super::{PsdAdditions, PsdButtonAddition, write_psd_over};
        use crate::psd_layer_tree;
        let dir = std::env::temp_dir().join(format!("disccraft-psd-pages-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let (w, h) = (32_usize, 24_usize);
        let raster = |name: &str| PsdLayer::Raster {
            name: name.to_owned(),
            rgba: solid(w, h, [255, 255, 255, 255]),
            visible: true,
        };
        let set = |name: &str| PsdLayer::Group {
            name: format!("(+) {name}"),
            id: None,
            children: vec![raster(name)],
        };
        let source = dir.join("pages.psd");
        write_layered_psd(
            &source,
            32,
            24,
            &[
                raster("背景"),
                set("Play"),
                PsdLayer::Group {
                    name: "(#) One".to_owned(),
                    id: None,
                    children: vec![set("Next")],
                },
                PsdLayer::Group {
                    name: "(#) Two".to_owned(),
                    id: None,
                    children: vec![set("Back"), raster("art")],
                },
            ],
        )?;
        let additions = PsdAdditions {
            page: Some(2),
            shared_only: false,
            layers: vec![raster("added text")],
            buttons: vec![
                PsdButtonAddition {
                    id: None,
                    name: "Back".to_owned(),
                    into: vec![raster("Back again")],
                    set: set("unused"),
                },
                PsdButtonAddition {
                    id: None,
                    name: "Extra".to_owned(),
                    into: vec![raster("unused")],
                    set: set("Extra"),
                },
            ],
        };
        let out = dir.join("out.psd");
        assert!(write_psd_over(&source, &out, (32, 24), &additions)?);
        let tree = psd_layer_tree(&out)?;
        let names = |nodes: &[crate::PsdTreeNode]| -> Vec<String> {
            nodes.iter().map(|node| node.name.clone()).collect()
        };
        assert_eq!(names(&tree), ["(#) Two", "(#) One", "(+) Play", "背景"]);
        let two = tree.first().ok_or("page two")?;
        assert_eq!(
            names(&two.children),
            ["(+) Extra", "added text", "art", "(+) Back"]
        );
        let back = two.children.last().ok_or("Back set")?;
        assert_eq!(names(&back.children), ["Back again", "Back"]);

        // A page added later: the shared layers only, the additions at the top level.
        let shared = PsdAdditions {
            page: None,
            shared_only: true,
            layers: vec![raster("added text")],
            buttons: Vec::new(),
        };
        assert!(write_psd_over(&source, &out, (32, 24), &shared)?);
        assert_eq!(
            names(&psd_layer_tree(&out)?),
            ["added text", "(+) Play", "背景"]
        );
        // Another canvas size: nothing written.
        let other = dir.join("other.psd");
        assert!(!write_psd_over(&source, &other, (64, 48), &shared)?);
        assert!(!other.exists());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
