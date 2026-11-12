//! Vector images: SVG drawings are rendered (with resvg, used unmodified) at the size they are shown at, so their
//! lines stay sharp on a menu background, a slide or a thumbnail instead of being scaled up from a small bitmap.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{self, fontdb};

use crate::{Image8, PhotoError};

/// Largest SVG file read (SVG is text; anything bigger is not a drawing).
const MAX_SVG_BYTES: u64 = 64 * 1024 * 1024;
/// Largest side an SVG is rendered at.
const MAX_SIDE: u32 = 8192;

/// Whether the path names an SVG drawing (`.svg`, case-insensitive).
pub fn is_svg(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
}

/// Renders an SVG stretched to exactly `width` x `height` (a menu background fills the frame the same way a
/// bitmap background does). Transparent parts stay transparent.
pub fn render_svg_fill(path: &Path, width: u32, height: u32) -> Result<Image8, PhotoError> {
    let tree = load(path)?;
    let size = tree.size();
    let (width, height) = checked_size(path, width, height)?;
    let scale_x = width as f32 / size.width();
    let scale_y = height as f32 / size.height();
    render(
        path,
        &tree,
        width,
        height,
        Transform::from_scale(scale_x, scale_y),
    )
}

/// Renders an SVG as large as fits in `max_width` x `max_height`, keeping its proportions (slides, thumbnails).
pub fn render_svg_fit(path: &Path, max_width: u32, max_height: u32) -> Result<Image8, PhotoError> {
    let tree = load(path)?;
    let size = tree.size();
    let (max_width, max_height) = checked_size(path, max_width, max_height)?;
    let scale = (max_width as f32 / size.width()).min(max_height as f32 / size.height());
    if !scale.is_finite() || scale <= 0.0 {
        return Err(invalid(path, "the drawing has no size"));
    }
    // The products are at most the limits above, so the casts cannot overflow.
    let width = ((size.width() * scale).round() as u32).clamp(1, max_width);
    let height = ((size.height() * scale).round() as u32).clamp(1, max_height);
    render(
        path,
        &tree,
        width,
        height,
        Transform::from_scale(scale, scale),
    )
}

fn checked_size(path: &Path, width: u32, height: u32) -> Result<(u32, u32), PhotoError> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(invalid(path, "render size out of range"));
    }
    Ok((width, height))
}

/// Parses an SVG file. Relative image links resolve next to the file; text uses the system fonts.
fn load(path: &Path) -> Result<usvg::Tree, PhotoError> {
    let display = path.display().to_string();
    let metadata = std::fs::metadata(path).map_err(|error| PhotoError::Io {
        path: display.clone(),
        message: error.to_string(),
    })?;
    if metadata.len() > MAX_SVG_BYTES {
        return Err(invalid(path, "file too large"));
    }
    let bytes = std::fs::read(path).map_err(|error| PhotoError::Io {
        path: display,
        message: error.to_string(),
    })?;

    let options = usvg::Options {
        resources_dir: path.parent().map(Path::to_path_buf),
        fontdb: system_fonts(),
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_data(&bytes, &options)
        .map_err(|error| invalid(path, &error.to_string()))?;
    let size = tree.size();
    if !(size.width() > 0.0 && size.height() > 0.0) {
        return Err(invalid(path, "the drawing has no size"));
    }
    Ok(tree)
}

/// The system fonts, loaded once per process (loading them takes a moment).
fn system_fonts() -> Arc<fontdb::Database> {
    static FONTS: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut fonts = fontdb::Database::new();
            fonts.load_system_fonts();
            Arc::new(fonts)
        })
        .clone()
}

fn render(
    path: &Path,
    tree: &usvg::Tree,
    width: u32,
    height: u32,
    transform: Transform,
) -> Result<Image8, PhotoError> {
    let mut pixmap =
        Pixmap::new(width, height).ok_or_else(|| invalid(path, "render size out of range"))?;
    resvg::render(tree, transform, &mut pixmap.as_mut());
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect();
    Ok(Image8 {
        width,
        height,
        rgba,
    })
}

fn invalid(path: &Path, message: &str) -> PhotoError {
    PhotoError::Invalid(format!("{}: {message}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{is_svg, render_svg_fill, render_svg_fit};
    use std::error::Error;
    use std::path::{Path, PathBuf};

    fn write_svg(name: &str, body: &str) -> Result<PathBuf, Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-photo-svg-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(name);
        std::fs::write(&path, body)?;
        Ok(path)
    }

    fn pixel(image: &crate::Image8, x: u32, y: u32) -> Option<[u8; 4]> {
        let index = usize::try_from((y * image.width + x) * 4).ok()?;
        let bytes = image.rgba.get(index..index + 4)?;
        <[u8; 4]>::try_from(bytes).ok()
    }

    // A 40 x 30 drawing: the left half red, the right half blue.
    const HALVES: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30" viewBox="0 0 40 30">
        <rect x="0" y="0" width="20" height="30" fill="#ff0000"/>
        <rect x="20" y="0" width="20" height="30" fill="#0000ff"/>
    </svg>"##;

    #[test]
    fn a_drawing_fills_the_frame_sharply() -> Result<(), Box<dyn Error>> {
        let path = write_svg("halves.svg", HALVES)?;
        let image = render_svg_fill(&path, 720, 480)?;
        assert_eq!((image.width, image.height), (720, 480));
        assert_eq!(image.rgba.len(), 720 * 480 * 4);
        // Sharp edge: just left of the middle is red, just right is blue.
        assert_eq!(pixel(&image, 358, 240), Some([255, 0, 0, 255]));
        assert_eq!(pixel(&image, 361, 240), Some([0, 0, 255, 255]));
        Ok(())
    }

    #[test]
    fn fit_keeps_the_proportions() -> Result<(), Box<dyn Error>> {
        let path = write_svg("halves-fit.svg", HALVES)?;
        let image = render_svg_fit(&path, 400, 400)?;
        assert_eq!((image.width, image.height), (400, 300));
        Ok(())
    }

    #[test]
    fn broken_files_are_errors() -> Result<(), Box<dyn Error>> {
        let path = write_svg("broken.svg", "<svg")?;
        assert!(render_svg_fill(&path, 100, 100).is_err());
        assert!(render_svg_fill(Path::new("no-such.svg"), 100, 100).is_err());
        let ok = write_svg("ok.svg", HALVES)?;
        assert!(render_svg_fill(&ok, 0, 100).is_err());
        assert!(render_svg_fit(&ok, 100_000, 100).is_err());
        assert!(is_svg(Path::new("logo.SVG")));
        assert!(!is_svg(Path::new("logo.png")));
        Ok(())
    }
}
