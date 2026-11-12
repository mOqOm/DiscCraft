//! Writing a layered PSD for editing a menu in PhotoCraft (or any PSD editor), like Encore's "Edit Menu in
//! Photoshop". The document is built with PhotoCraft's document model and saved by PhotoCraft's PSD writer.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use photocraft_doc::{ColorMode, Document, Layer, PixelFormat, Rect, SampleType, Size};

use crate::{MAX_DIMENSION, PhotoError};

/// One layer of a PSD written by [`write_layered_psd`].
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// A group of layers, bottom to top.
    Group {
        /// Group name.
        name: String,
        /// The layers in the group, bottom to top.
        children: Vec<PsdLayer>,
    },
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

    let display = path.display().to_string();
    let exported = catch_unwind(AssertUnwindSafe(|| {
        photocraft_io::export(&doc, "psd", &photocraft_io::ExportOptions::default())
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
        PsdLayer::Group { name, children } => {
            let children = children
                .iter()
                .map(|child| build_layer(child, format, canvas, expected))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Layer::group(name.clone(), children))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PsdLayer, write_layered_psd};
    use crate::{load_image, load_menu_background, load_psd_menu};
    use std::error::Error;

    fn solid(width: usize, height: usize, rgba: [u8; 4]) -> Vec<u8> {
        rgba.repeat(width * height)
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
}
