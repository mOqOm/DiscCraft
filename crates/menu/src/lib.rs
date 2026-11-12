//! DiscCraft menu renderer (L2).
//!
//! This crate renders a project menu into a still RGB frame and a highlight
//! layer suitable for DVD / Blu-ray authoring.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::cognitive_complexity
)]

mod draw;
mod graphic;
mod highlight;
mod palette;
mod route;
mod shadow;
pub mod subtitle;
pub mod subtitle_file;

use disccraft_project::frame_size;
use thiserror::Error;

pub use disccraft_project::{Menu, Project, Rect};
pub use highlight::HighlightLayer;
pub use palette::{NORMAL_ENTRY as NORMAL_PALETTE_ENTRY, ycrcb_entry};
pub use route::{auto_route, auto_route_with, button_routes};

/// Errors returned by menu rendering.
#[derive(Debug, Error)]
pub enum MenuError {
    /// The menu or one of its inputs is invalid.
    #[error("invalid menu: {0}")]
    Invalid(String),
    /// A filesystem error occurred.
    #[error("io error: {0}")]
    Io(#[from] Box<std::io::Error>),
    /// A PNG decoding error occurred.
    #[error("png error: {0}")]
    Png(#[from] Box<png::DecodingError>),
}

/// Navigation geometry for one rendered button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButtonGeometry {
    /// Left edge.
    pub x1: u16,
    /// Top edge.
    pub y1: u16,
    /// Inclusive right edge.
    pub x2: u16,
    /// Inclusive bottom edge.
    pub y2: u16,
    /// One-based up target.
    pub up: u8,
    /// One-based down target.
    pub down: u8,
    /// One-based left target.
    pub left: u8,
    /// One-based right target.
    pub right: u8,
}

/// A rendered menu frame and its metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedMenu {
    /// Frame width.
    pub width: u16,
    /// Frame height.
    pub height: u16,
    /// RGB24 frame pixels.
    pub rgb: Vec<u8>,
    /// Highlight layer covering all buttons.
    pub highlight: HighlightLayer,
    /// Palette entries packed as `0x00_YY_CR_CB`.
    pub palette: [u32; 16],
    /// The DVD color sets of highlight groups 1 to 3.
    pub color_sets: [[u8; 8]; 3],
    /// Contrast (0 to 15) of the highlight frame in the normal state, drawn with palette entry
    /// [`NORMAL_PALETTE_ENTRY`] (Encore's 標準グループ); 0 hides it.
    pub normal_alpha: u8,
    /// Button geometry and navigation.
    pub buttons: Vec<ButtonGeometry>,
}

/// Renders a menu into a frame and highlight layer.
///
/// # Errors
///
/// Returns [`MenuError::Invalid`] when the menu has no buttons or when an
/// image background cannot be used. Returns I/O or PNG errors when an image
/// background cannot be read.
pub fn render_menu(project: &Project, menu: &Menu) -> Result<RenderedMenu, MenuError> {
    if menu.buttons.is_empty() {
        return Err(MenuError::Invalid("menu has no buttons".to_string()));
    }
    render(project, menu)
}

/// Renders a menu for an editor: like [`render_menu`], but a menu without buttons (a new, blank menu) is shown too,
/// with an empty highlight layer.
///
/// # Errors
///
/// As [`render_menu`], except for a menu without buttons.
pub fn render_menu_preview(project: &Project, menu: &Menu) -> Result<RenderedMenu, MenuError> {
    render(project, menu)
}

fn render(project: &Project, menu: &Menu) -> Result<RenderedMenu, MenuError> {
    let (width, height) = frame_size(&project.settings);
    let mut rgb = draw::new_frame(width, height)?;

    draw::draw_background(project, menu, width, height, &mut rgb)?;
    draw_graphics_rgb(project, menu, width, height, &mut rgb);
    draw::draw_texts(menu, width, height, &mut rgb);
    draw::draw_labels(menu, width, height, &mut rgb);

    let highlight = highlight::build(menu, width, height)?;
    let buttons = button_geometry(menu, &project.settings.routing);
    let palette = palette::build(menu);
    let color_sets = palette::color_sets(menu);
    let normal_alpha = palette::normal_alpha(menu);

    Ok(RenderedMenu {
        width,
        height,
        rgb,
        highlight,
        palette,
        color_sets,
        normal_alpha,
        buttons,
    })
}

/// Render `menu` over a picture of the whole menu screen (a video frame as `asset.frame` gives it, already 4:3),
/// scaled to the frame — for an editor showing a motion menu with the first frame of its video.
///
/// # Errors
///
/// As [`render_menu`]; also when the picture's bytes do not match its size.
pub fn render_menu_over_picture(
    project: &Project,
    menu: &Menu,
    rgb: &[u8],
    width: usize,
    height: usize,
) -> Result<RenderedMenu, MenuError> {
    let mut rendered = render_menu_preview(project, menu)?;
    let mut frame = vec![0u8; rendered.rgb.len()];
    if !draw::scale_rgb_into(
        rgb,
        width,
        height,
        rendered.width,
        rendered.height,
        &mut frame,
    ) {
        return Err(MenuError::Invalid(
            "the background picture does not match its size".to_string(),
        ));
    }
    draw_graphics_rgb(project, menu, rendered.width, rendered.height, &mut frame);
    draw::draw_texts(menu, rendered.width, rendered.height, &mut frame);
    draw::draw_labels(menu, rendered.width, rendered.height, &mut frame);
    rendered.rgb = frame;
    Ok(rendered)
}

/// The layers of `menu` for editing it in an image editor (Encore's "Edit Menu in Photoshop"), bottom to top: the
/// background ("背景"), one layer per text object, and one `(+)` group per button with its text and a hidden
/// `(=1)` highlight filled in the highlight colour. `picture` replaces the background (a motion menu's first frame).
///
/// # Errors
///
/// When the background cannot be drawn.
pub fn menu_psd_layers(
    project: &Project,
    menu: &Menu,
    picture: Option<(&[u8], usize, usize)>,
) -> Result<(u16, u16, Vec<disccraft_photo::PsdLayer>), MenuError> {
    use disccraft_photo::PsdLayer;
    let (width, height) = frame_size(&project.settings);
    let mut rgb = draw::new_frame(width, height)?;
    let scaled = picture.is_some_and(|(pixels, picture_width, picture_height)| {
        draw::scale_rgb_into(
            pixels,
            picture_width,
            picture_height,
            width,
            height,
            &mut rgb,
        )
    });
    if !scaled {
        draw::draw_background(project, menu, width, height, &mut rgb)?;
    }
    let background: Vec<u8> = rgb
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|[r, g, b]| [*r, *g, *b, 255])
        .collect();
    let mut layers = vec![PsdLayer::Raster {
        name: "背景".to_owned(),
        rgba: background,
        visible: true,
    }];
    for graphic in &menu.graphics {
        layers.push(PsdLayer::Raster {
            name: graphic.kind_name(project),
            rgba: graphic_rgba(project, graphic, width, height),
            visible: true,
        });
    }
    for text in &menu.texts {
        let first = text.text.lines().next().unwrap_or_default();
        layers.push(PsdLayer::Raster {
            name: format!("テキスト: {first}"),
            rgba: draw::text_rgba(menu, text, width, height),
            visible: true,
        });
    }
    for (index, button) in menu.buttons.iter().enumerate() {
        let [r, g, b] = menu.highlight_group(button.highlight_group).selected;
        let title = if button.label.trim().is_empty() {
            format!("ボタン {}", index.saturating_add(1))
        } else {
            button.label.clone()
        };
        let mut highlight = vec![0u8; usize::from(width) * usize::from(height) * 4];
        let rect = button.rect;
        for y in rect.y..rect.y.saturating_add(rect.h).min(height) {
            for x in rect.x..rect.x.saturating_add(rect.w).min(width) {
                let i = (usize::from(y) * usize::from(width) + usize::from(x)) * 4;
                if let Some(pixel) = highlight.get_mut(i..i + 4) {
                    pixel.copy_from_slice(&[r, g, b, 255]);
                }
            }
        }
        layers.push(PsdLayer::Group {
            name: format!("(+) {title}"),
            children: vec![
                PsdLayer::Raster {
                    name: title,
                    rgba: draw::label_rgba(menu, button, width, height),
                    visible: true,
                },
                PsdLayer::Raster {
                    name: "(=1) ハイライト".to_owned(),
                    rgba: highlight,
                    visible: false,
                },
            ],
        });
    }
    Ok((width, height, layers))
}

/// Write `menu` as a layered PSD (see [`menu_psd_layers`]) for editing in PhotoCraft or another PSD editor.
///
/// # Errors
///
/// When the background cannot be drawn or the file cannot be written.
pub fn export_menu_psd(
    project: &Project,
    menu: &Menu,
    picture: Option<(&[u8], usize, usize)>,
    path: &std::path::Path,
) -> Result<(), MenuError> {
    let (width, height, layers) = menu_psd_layers(project, menu, picture)?;
    disccraft_photo::write_layered_psd(path, u32::from(width), u32::from(height), &layers)
        .map_err(|error| MenuError::Invalid(error.to_string()))
}

/// The font families on this computer, sorted by name (for a font list in an editor).
pub fn font_families() -> Vec<String> {
    draw::font_families()
}

/// The styles of a font family ("Regular", "Bold", ...), lightest first; `None` (or a family this computer does
/// not have) lists the default font's styles.
pub fn font_styles(family: Option<&str>) -> Vec<String> {
    draw::font_styles(family)
}

/// A button's text as an RGBA layer of the whole frame (for a Blu-ray pop-up menu's normal state).
pub fn label_rgba(
    menu: &Menu,
    button: &disccraft_project::Button,
    width: u16,
    height: u16,
) -> Vec<u8> {
    draw::label_rgba(menu, button, width, height)
}

/// The frame rectangle a menu text object covers (for selecting and outlining it in an editor).
pub fn text_rect(menu: &Menu, text: &disccraft_project::MenuText) -> Rect {
    draw::text_rect(menu, text)
}

/// Render `menu` over a caller-supplied background frame (RGB24, frame size of the project) — one frame of a motion menu.
pub fn render_menu_over(
    project: &Project,
    menu: &Menu,
    background: &[u8],
) -> Result<RenderedMenu, MenuError> {
    let mut rendered = render_menu(project, menu)?;
    if background.len() != rendered.rgb.len() {
        return Err(MenuError::Invalid(
            "background frame size does not match the menu".to_string(),
        ));
    }
    let mut rgb = background.to_vec();
    draw_graphics_rgb(project, menu, rendered.width, rendered.height, &mut rgb);
    draw::draw_texts(menu, rendered.width, rendered.height, &mut rgb);
    draw::draw_labels(menu, rendered.width, rendered.height, &mut rgb);
    rendered.rgb = rgb;
    Ok(rendered)
}

/// Blends the menu's graphic layers (images and shapes) into an RGB24 frame.
fn draw_graphics_rgb(project: &Project, menu: &Menu, width: u16, height: u16, rgb: &mut [u8]) {
    let w = usize::from(width);
    let mut sink = |x: usize, y: usize, color: [u8; 3], coverage: f32| {
        draw::blend_pixel(rgb, w, x, y, color, coverage);
    };
    graphic::paint_graphics(project, menu, width, height, &mut sink);
}

/// One graphic as a transparent RGBA layer of the frame (for a PSD export).
fn graphic_rgba(
    project: &Project,
    graphic: &disccraft_project::MenuGraphic,
    width: u16,
    height: u16,
) -> Vec<u8> {
    let w = usize::from(width);
    let mut rgba = vec![0u8; w * usize::from(height) * 4];
    let mut sink = |x: usize, y: usize, color: [u8; 3], coverage: f32| {
        let Some(i) = y
            .checked_mul(w)
            .and_then(|n| n.checked_add(x))
            .and_then(|n| n.checked_mul(4))
        else {
            return;
        };
        if let Some(pixel) = rgba.get_mut(i..i.saturating_add(4)) {
            let alpha = (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
            pixel.copy_from_slice(&[color[0], color[1], color[2], alpha]);
        }
    };
    graphic::paint_graphic(project, graphic, width, height, &mut sink);
    rgba
}

fn button_geometry(menu: &Menu, options: &disccraft_project::RouteOptions) -> Vec<ButtonGeometry> {
    let rects: Vec<Rect> = menu.buttons.iter().map(|button| button.rect).collect();
    let routes = button_routes(menu, options);

    rects
        .iter()
        .zip(routes)
        .map(|(rect, route)| ButtonGeometry {
            x1: rect.x,
            y1: rect.y,
            x2: rect.x.saturating_add(rect.w.saturating_sub(1)),
            y2: rect.y.saturating_add(rect.h.saturating_sub(1)),
            up: route.0,
            down: route.1,
            left: route.2,
            right: route.3,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{auto_route, render_menu, ycrcb_entry};
    use disccraft_project::{Link, MenuBackground, Project, Rect};
    use std::error::Error;

    #[test]
    fn a_menu_without_buttons_is_shown_in_an_editor_but_not_built() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let menu = project.menu(menu_id).ok_or("menu")?.clone();
        assert!(render_menu(&project, &menu).is_err());
        let preview = super::render_menu_preview(&project, &menu)?;
        assert_eq!(preview.rgb.len(), 720 * 480 * 3);
        assert!(preview.buttons.is_empty());
        assert!(preview.highlight.pixels.iter().all(|pixel| *pixel == 0));
        Ok(())
    }

    /// Graphic layers draw over the background and under the texts: an ellipse's middle shows its colour, its
    /// corner shows the background; a half-transparent image blends; a PSD export gets one layer per graphic.
    #[test]
    fn graphics_are_drawn_over_the_background() -> Result<(), Box<dyn Error>> {
        use disccraft_project::{GraphicKind, MenuGraphic, ShapeKind};
        let dir = std::env::temp_dir().join(format!(
            "disccraft-menu-graphic-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir)?;
        // A 2 x 2 PNG: blue, fully opaque.
        let logo = dir.join("logo.png");
        {
            let file = std::fs::File::create(&logo)?;
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header()?;
            writer.write_image_data(&[0, 0, 255, 255].repeat(4))?;
        }

        let mut project = Project::default();
        let asset = project.add_asset(&logo)?;
        let menu_id = project.add_menu(None)?;
        project.menu_mut(menu_id).ok_or("menu")?.background =
            MenuBackground::Color { color: [0, 0, 0] };
        let _ = project.add_menu_graphic(
            menu_id,
            MenuGraphic {
                rect: Rect {
                    x: 100,
                    y: 100,
                    w: 200,
                    h: 100,
                },
                kind: GraphicKind::Shape {
                    shape: ShapeKind::Ellipse,
                    color: [255, 0, 0],
                },
                opacity: 100,
                shadow: None,
                orientation: Default::default(),
            },
        )?;
        let _ = project.add_menu_graphic(
            menu_id,
            MenuGraphic {
                rect: Rect {
                    x: 400,
                    y: 300,
                    w: 100,
                    h: 100,
                },
                kind: GraphicKind::Image { asset },
                opacity: 50,
                shadow: None,
                orientation: Default::default(),
            },
        )?;
        let menu = project.menu(menu_id).ok_or("menu")?.clone();
        let rendered = super::render_menu_preview(&project, &menu)?;
        let pixel = |x: usize, y: usize| {
            let i = (y * 720 + x) * 3;
            rendered.rgb.get(i..i + 3).map(|p| [p[0], p[1], p[2]])
        };
        assert_eq!(pixel(200, 150), Some([255, 0, 0]));
        assert_eq!(pixel(101, 101), Some([0, 0, 0]));
        assert_eq!(pixel(450, 350), Some([0, 0, 128]));
        assert_eq!(pixel(10, 10), Some([0, 0, 0]));

        let (_, _, layers) = super::menu_psd_layers(&project, &menu, None)?;
        assert_eq!(layers.len(), 3);
        Ok(())
    }

    #[test]
    fn a_shape_casts_a_sharp_shadow_the_way_the_light_falls() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        project.menu_mut(menu_id).ok_or("menu")?.background = MenuBackground::Color {
            color: [255, 255, 255],
        };
        let _ = project.add_menu_graphic(
            menu_id,
            disccraft_project::MenuGraphic {
                rect: Rect {
                    x: 100,
                    y: 100,
                    w: 200,
                    h: 100,
                },
                kind: disccraft_project::GraphicKind::Shape {
                    shape: disccraft_project::ShapeKind::Rectangle,
                    color: [255, 0, 0],
                },
                opacity: 100,
                orientation: Default::default(),
                shadow: Some(disccraft_project::DropShadow {
                    color: [0, 0, 255],
                    opacity: 100,
                    angle: 90,
                    distance: 10,
                    size: 0,
                    spread: 0,
                }),
            },
        )?;
        let menu = project.menu(menu_id).ok_or("menu")?.clone();
        let rendered = super::render_menu_preview(&project, &menu)?;
        let pixel = |x: usize, y: usize| {
            let i = (y * 720 + x) * 3;
            rendered.rgb.get(i..i + 3).map(|p| [p[0], p[1], p[2]])
        };
        // Light from straight above: a sharp blue shadow 10 pixels below the rectangle, under the rectangle.
        assert_eq!(pixel(200, 150), Some([255, 0, 0]));
        assert_eq!(pixel(200, 205), Some([0, 0, 255]));
        assert_eq!(pixel(200, 215), Some([255, 255, 255]));
        assert_eq!(pixel(200, 95), Some([255, 255, 255]));
        assert_eq!(pixel(95, 150), Some([255, 255, 255]));

        // A soft, half-transparent shadow fades out instead of ending sharply.
        let mut soft = menu.clone();
        if let Some(graphic) = soft.graphics.first_mut() {
            graphic.shadow = Some(disccraft_project::DropShadow {
                color: [0, 0, 0],
                opacity: 50,
                angle: 90,
                distance: 10,
                size: 8,
                spread: 0,
            });
        }
        let rendered = super::render_menu_preview(&project, &soft)?;
        let pixel = |x: usize, y: usize| {
            let i = (y * 720 + x) * 3;
            rendered.rgb.get(i..i + 3).map(|p| [p[0], p[1], p[2]])
        };
        let near = pixel(200, 205).ok_or("pixel")?;
        let far = pixel(200, 213).ok_or("pixel")?;
        let gone = pixel(200, 230).ok_or("pixel")?;
        assert!(
            near[0] < far[0] && far[0] < gone[0],
            "{near:?} {far:?} {gone:?}"
        );
        assert_eq!(gone, [255, 255, 255]);
        Ok(())
    }

    #[test]
    fn a_flipped_or_turned_image_is_drawn_that_way() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!(
            "disccraft-menu-orient-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir)?;
        // A 2 x 1 PNG: red on the left, green on the right.
        let path = dir.join("pair.png");
        {
            let file = std::fs::File::create(&path)?;
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 2, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header()?;
            writer.write_image_data(&[255, 0, 0, 255, 0, 255, 0, 255])?;
        }
        let mut project = Project::default();
        let asset = project.add_asset(&path)?;
        let menu_id = project.add_menu(None)?;
        project.menu_mut(menu_id).ok_or("menu")?.background =
            MenuBackground::Color { color: [0, 0, 0] };
        let base = project.menu(menu_id).ok_or("menu")?.clone();
        let draw = |orientation: disccraft_project::Orientation, rect: Rect| {
            let mut menu = base.clone();
            menu.graphics = vec![disccraft_project::MenuGraphic {
                rect,
                kind: disccraft_project::GraphicKind::Image { asset },
                opacity: 100,
                shadow: None,
                orientation,
            }];
            super::render_menu_preview(&project, &menu)
        };
        let pixel = |rendered: &super::RenderedMenu, x: usize, y: usize| {
            let i = (y * 720 + x) * 3;
            rendered.rgb.get(i..i + 3).map(|p| [p[0], p[1], p[2]])
        };
        let wide = Rect {
            x: 100,
            y: 100,
            w: 200,
            h: 100,
        };
        let upright = draw(disccraft_project::Orientation::default(), wide)?;
        assert_eq!(pixel(&upright, 110, 150), Some([255, 0, 0]));
        assert_eq!(pixel(&upright, 290, 150), Some([0, 255, 0]));
        let flipped = draw(
            disccraft_project::Orientation {
                flip: true,
                turns: 0,
            },
            wide,
        )?;
        assert_eq!(pixel(&flipped, 110, 150), Some([0, 255, 0]));
        assert_eq!(pixel(&flipped, 290, 150), Some([255, 0, 0]));
        // A quarter turn clockwise: the left (red) side is now on top.
        let tall = Rect {
            x: 100,
            y: 100,
            w: 100,
            h: 200,
        };
        let turned = draw(
            disccraft_project::Orientation {
                flip: false,
                turns: 1,
            },
            tall,
        )?;
        assert_eq!(pixel(&turned, 150, 110), Some([255, 0, 0]));
        assert_eq!(pixel(&turned, 150, 290), Some([0, 255, 0]));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn anti_aliasing_modes_shape_the_edge() -> Result<(), Box<dyn Error>> {
        if filmcraft_text::families().is_empty() {
            return Ok(());
        }
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let mut menu = project.menu(menu_id).ok_or("menu")?.clone();
        menu.background = MenuBackground::Color { color: [0, 0, 0] };
        menu.texts.push(disccraft_project::MenuText {
            text: "Edge".to_owned(),
            x: 100,
            y: 100,
            size: 40,
            color: Some([255, 255, 255]),
            font: None,
            font_style: None,
            format: disccraft_project::TextFormat::default(),
            shadow: None,
            orientation: disccraft_project::Orientation::default(),
        });
        // (pixels partly lit, total light) of the picture with `mode`.
        let measure =
            |mode: disccraft_project::TextAntialias| -> Result<(usize, f64), Box<dyn Error>> {
                let mut menu = menu.clone();
                if let Some(text) = menu.texts.first_mut() {
                    text.format.antialias = mode;
                }
                let rendered = super::render_menu_preview(&project, &menu)?;
                let partial = rendered
                    .rgb
                    .chunks(3)
                    .filter(|p| p[0] > 10 && p[0] < 245)
                    .count();
                let light = rendered.rgb.iter().map(|v| f64::from(*v)).sum();
                Ok((partial, light))
            };
        use disccraft_project::TextAntialias as Aa;
        let (crisp_edge, crisp_light) = measure(Aa::Crisp)?;
        let (none_edge, _) = measure(Aa::None)?;
        let (_, strong_light) = measure(Aa::Strong)?;
        let (sharp_edge, _) = measure(Aa::Sharp)?;
        let (smooth_edge, _) = measure(Aa::Smooth)?;
        assert_eq!(none_edge, 0, "no anti-aliasing leaves no partly lit pixels");
        assert!(strong_light > crisp_light);
        assert!(sharp_edge < crisp_edge);
        assert!(smooth_edge > crisp_edge);
        Ok(())
    }

    #[test]
    fn a_bold_style_draws_heavier_text() -> Result<(), Box<dyn Error>> {
        let styles = super::font_styles(None);
        let Some(bold) = styles
            .iter()
            .find(|style| style.to_lowercase().contains("bold"))
            .cloned()
        else {
            return Ok(());
        };
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let mut menu = project.menu(menu_id).ok_or("menu")?.clone();
        menu.background = MenuBackground::Color { color: [0, 0, 0] };
        menu.texts.push(disccraft_project::MenuText {
            text: "Heavy".to_owned(),
            x: 100,
            y: 100,
            size: 60,
            color: Some([255, 255, 255]),
            font: None,
            font_style: None,
            format: disccraft_project::TextFormat::default(),
            shadow: None,
            orientation: disccraft_project::Orientation::default(),
        });
        let ink = |menu: &disccraft_project::Menu| -> Result<f64, Box<dyn Error>> {
            let rendered = super::render_menu_preview(&project, menu)?;
            Ok(rendered.rgb.iter().map(|v| f64::from(*v)).sum())
        };
        let regular = ink(&menu)?;
        if let Some(text) = menu.texts.first_mut() {
            text.font_style = Some(bold);
        }
        let heavy = ink(&menu)?;
        assert!(heavy > regular * 1.1, "{regular} {heavy}");
        Ok(())
    }

    #[test]
    fn scaled_and_superscript_text_is_measured_and_drawn() -> Result<(), Box<dyn Error>> {
        if filmcraft_text::families().is_empty() {
            return Ok(());
        }
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let mut menu = project.menu(menu_id).ok_or("menu")?.clone();
        menu.background = MenuBackground::Color { color: [0, 0, 0] };
        menu.texts.push(disccraft_project::MenuText {
            text: "Xy".to_owned(),
            x: 100,
            y: 100,
            size: 60,
            color: Some([255, 255, 255]),
            font: None,
            font_style: None,
            format: disccraft_project::TextFormat::default(),
            shadow: None,
            orientation: disccraft_project::Orientation::default(),
        });
        let with = |change: &dyn Fn(&mut disccraft_project::TextFormat)| {
            let mut changed = menu.clone();
            if let Some(text) = changed.texts.first_mut() {
                change(&mut text.format);
            }
            changed
        };
        let plain = super::text_rect(&menu, menu.texts.first().ok_or("text")?);
        let wide = with(&|f| f.horizontal_scale = 200);
        let wide_rect = super::text_rect(&wide, wide.texts.first().ok_or("text")?);
        assert!(
            wide_rect.w.abs_diff(plain.w * 2) <= 2,
            "{plain:?} {wide_rect:?}"
        );
        assert_eq!(wide_rect.h, plain.h);
        let tall = with(&|f| f.vertical_scale = 200);
        let tall_rect = super::text_rect(&tall, tall.texts.first().ok_or("text")?);
        assert!(
            tall_rect.h.abs_diff(plain.h * 2) <= 2,
            "{plain:?} {tall_rect:?}"
        );

        // Superscript: less ink, and its lowest ink is higher than the plain text's.
        let ink = |menu: &disccraft_project::Menu| -> Result<(usize, usize), Box<dyn Error>> {
            let rendered = super::render_menu_preview(&project, menu)?;
            let mut count = 0usize;
            let mut lowest = 0usize;
            for (i, pixel) in rendered.rgb.chunks(3).enumerate() {
                if pixel.iter().any(|v| *v > 128) {
                    count += 1;
                    lowest = lowest.max(i / 720);
                }
            }
            Ok((count, lowest))
        };
        let (plain_ink, plain_low) = ink(&menu)?;
        let sup = with(&|f| f.script = disccraft_project::TextScript::Superscript);
        let (sup_ink, sup_low) = ink(&sup)?;
        assert!(
            sup_ink < plain_ink && sup_low < plain_low,
            "{plain_ink} {plain_low} {sup_ink} {sup_low}"
        );
        let sub = with(&|f| f.script = disccraft_project::TextScript::Subscript);
        let (_, sub_low) = ink(&sub)?;
        assert!(sub_low > sup_low);
        // The box keeps the full line height.
        let sup_rect = super::text_rect(&sup, sup.texts.first().ok_or("text")?);
        assert!(sup_rect.h >= plain.h);
        Ok(())
    }

    #[test]
    fn a_turned_text_stands_on_its_side() -> Result<(), Box<dyn Error>> {
        if filmcraft_text::families().is_empty() {
            return Ok(());
        }
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let mut menu = project.menu(menu_id).ok_or("menu")?.clone();
        menu.texts.push(disccraft_project::MenuText {
            text: "Welcome home".to_owned(),
            x: 100,
            y: 100,
            size: 40,
            color: Some([255, 255, 255]),
            font: None,
            font_style: None,
            format: disccraft_project::TextFormat::default(),
            shadow: None,
            orientation: disccraft_project::Orientation::default(),
        });
        let flat = super::text_rect(&menu, menu.texts.first().ok_or("text")?);
        let mut turned = menu.clone();
        if let Some(text) = turned.texts.first_mut() {
            text.orientation = text.orientation.rotated(1);
        }
        let standing = super::text_rect(&turned, turned.texts.first().ok_or("text")?);
        assert_eq!((standing.w, standing.h), (flat.h, flat.w));
        // The ink lies inside the turned rectangle.
        let rendered = super::render_menu_preview(&project, &turned)?;
        let lit = |x: usize, y: usize| {
            let i = (y * 720 + x) * 3;
            rendered
                .rgb
                .get(i..i + 3)
                .is_some_and(|p| p.iter().any(|v| *v > 128))
        };
        let mut inside = 0usize;
        let mut outside = 0usize;
        for y in 0..480 {
            for x in 0..720 {
                if lit(x, y) {
                    let within = x >= usize::from(standing.x)
                        && x < usize::from(standing.x + standing.w) + 2
                        && y >= usize::from(standing.y)
                        && y < usize::from(standing.y + standing.h) + 2;
                    if within {
                        inside += 1;
                    } else {
                        outside += 1;
                    }
                }
            }
        }
        assert!(
            inside > 50 && outside == 0,
            "{inside} inside, {outside} outside"
        );
        Ok(())
    }

    #[test]
    fn a_picture_is_scaled_under_the_menu() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        project.add_button(menu_id, String::new(), None, Link::Stop)?;
        let menu = project.menu(menu_id).ok_or("menu")?.clone();
        // A 4 x 3 picture: the left half red, the right half green. It fills the 4:3 screen.
        let mut picture = Vec::new();
        for _ in 0..3 {
            picture.extend_from_slice(&[255, 0, 0, 255, 0, 0, 0, 255, 0, 0, 255, 0]);
        }
        let rendered = super::render_menu_over_picture(&project, &menu, &picture, 4, 3)?;
        let at = |rendered: &super::RenderedMenu, x: usize, y: usize| {
            let index = (y * usize::from(rendered.width) + x) * 3;
            rendered.rgb.get(index..index + 3).map(<[u8]>::to_vec)
        };
        assert_eq!(at(&rendered, 5, 5), Some(vec![255, 0, 0]));
        assert_eq!(at(&rendered, 714, 5), Some(vec![0, 255, 0]));

        // The picture is a whole menu screen (already letterboxed by `asset.frame`), so it fills the frame: a
        // 720 x 480 frame is not letterboxed again.
        let screen = vec![200u8; 720 * 480 * 3];
        let rendered = super::render_menu_over_picture(&project, &menu, &screen, 720, 480)?;
        assert_eq!(at(&rendered, 360, 5), Some(vec![200, 200, 200]));
        assert_eq!(at(&rendered, 360, 475), Some(vec![200, 200, 200]));
        // Bytes that do not match the size are refused.
        assert!(super::render_menu_over_picture(&project, &menu, &picture, 3, 1).is_err());
        assert!(super::render_menu_over_picture(&project, &menu, &picture, 0, 0).is_err());
        Ok(())
    }

    #[test]
    fn text_objects_are_drawn_under_the_buttons() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        project.add_button(menu_id, String::new(), None, Link::Stop)?;
        let plain = project.menu(menu_id).ok_or("menu")?.clone();
        let mut with_text = plain.clone();
        with_text.texts.push(disccraft_project::MenuText {
            format: disccraft_project::TextFormat::default(),
            shadow: None,
            orientation: Default::default(),
            text: "Welcome".to_owned(),
            x: 60,
            y: 40,
            size: 48,
            color: Some([255, 255, 0]),
            font: None,
            font_style: None,
        });

        let rect = super::text_rect(&with_text, with_text.texts.first().ok_or("text")?);
        assert_eq!((rect.x, rect.y), (60, 40));
        assert!(rect.h >= 48 && rect.w >= 24);
        // On a 16:9 menu the text is squeezed to three quarters of its width (its height stays).
        let mut wide = with_text.clone();
        wide.widescreen = true;
        let squeezed = super::text_rect(&wide, wide.texts.first().ok_or("text")?);
        if !filmcraft_text::families().is_empty() {
            assert!(squeezed.w < rect.w && squeezed.w >= rect.w * 7 / 10);
        }
        assert_eq!(squeezed.h, rect.h);

        let before = render_menu(&project, &plain)?;
        let after = render_menu(&project, &with_text)?;
        // With a font on the system, the text changes pixels inside its rectangle and nowhere above it.
        if !filmcraft_text::families().is_empty() {
            assert_ne!(before.rgb, after.rgb);
        }
        let row = |rgb: &[u8], y: usize| {
            let start = y * usize::from(after.width) * 3;
            rgb.get(start..start + usize::from(after.width) * 3)
                .map(<[u8]>::to_vec)
        };
        assert_eq!(row(&before.rgb, 10), row(&after.rgb, 10));
        Ok(())
    }

    #[test]
    fn an_svg_background_is_rendered_sharp_at_frame_size() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-menu-svg-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let svg = dir.join("halves.svg");
        // A tiny 4 x 3 drawing: left half red, right half blue. A bitmap that small would blur when stretched.
        std::fs::write(
            &svg,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="3"><rect width="2" height="3" fill="#f00"/><rect x="2" width="2" height="3" fill="#00f"/></svg>"##,
        )?;
        let mut project = Project::default();
        let asset = project.add_asset(&svg)?;
        let menu_id = project.add_menu(None)?;
        project.add_button(menu_id, "Play".to_string(), None, Link::Stop)?;
        project.menu_mut(menu_id).ok_or("menu")?.background = MenuBackground::Image { asset };
        let menu = project.menu(menu_id).ok_or("menu")?.clone();

        let rendered = render_menu(&project, &menu)?;
        let at = |x: usize, y: usize| {
            let index = (y * usize::from(rendered.width) + x) * 3;
            rendered.rgb.get(index..index + 3).map(<[u8]>::to_vec)
        };
        // Right next to the middle line the colours are still pure: the drawing was rendered, not scaled up.
        assert_eq!(at(358, 20), Some(vec![255, 0, 0]));
        assert_eq!(at(361, 20), Some(vec![0, 0, 255]));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn ycrcb_entries() -> Result<(), Box<dyn Error>> {
        assert_eq!(ycrcb_entry([255, 255, 255]), 0x00EB_8080);
        assert_eq!(ycrcb_entry([0, 0, 0]), 0x0010_8080);

        let red = ycrcb_entry([255, 0, 0]);
        let y = (red >> 16) & 0xFF;
        let cr = (red >> 8) & 0xFF;
        let cb = red & 0xFF;

        assert!((i32::try_from(y).unwrap_or(0) - 0x51).abs() <= 2);
        assert!((i32::try_from(cr).unwrap_or(0) - 0xF0).abs() <= 2);
        assert!((i32::try_from(cb).unwrap_or(0) - 0x5A).abs() <= 2);

        Ok(())
    }

    #[test]
    fn stacked_buttons_route() -> Result<(), Box<dyn Error>> {
        let rects = vec![
            Rect {
                x: 100,
                y: 100,
                w: 400,
                h: 48,
            },
            Rect {
                x: 100,
                y: 184,
                w: 400,
                h: 48,
            },
            Rect {
                x: 100,
                y: 268,
                w: 400,
                h: 48,
            },
        ];

        let routes = auto_route(&rects);

        let first = routes.first().ok_or("missing first route")?;
        assert_eq!(first.0, 3);
        assert_eq!(first.1, 2);
        assert_eq!(first.2, 1);
        assert_eq!(first.3, 1);

        let third = routes.get(2).ok_or("missing third route")?;
        assert_eq!(third.0, 2);
        assert_eq!(third.1, 1);
        assert_eq!(third.2, 3);
        assert_eq!(third.3, 3);

        Ok(())
    }

    #[test]
    fn render_color_menu() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;

        project.menu_mut(menu_id).ok_or("missing menu")?.background = MenuBackground::Color {
            color: [10, 20, 30],
        };

        project.add_button(menu_id, "Play".to_string(), None, Link::Stop)?;
        project.add_button(menu_id, "第2章".to_string(), None, Link::Stop)?;
        project.add_button(menu_id, "Chapter 3".to_string(), None, Link::Stop)?;

        let menu = project.menu(menu_id).ok_or("missing menu")?;
        let rendered = render_menu(&project, menu)?;

        assert_eq!(rendered.width, 720);
        assert_eq!(rendered.height, 480);

        let corner = (10 * 720 + 10) * 3;
        assert_eq!(&rendered.rgb[corner..corner + 3], &[10, 20, 30]);

        let first = menu.buttons.first().ok_or("missing button")?;
        let rect = first.rect;
        let x_start = usize::from(rect.x);
        let x_end = x_start + usize::from(rect.w);
        let y_start = usize::from(rect.y);
        let y_end = y_start + usize::from(rect.h);
        let frame_w = usize::from(rendered.width);

        let mut found_label = false;
        for (pixel_index, chunk) in rendered.rgb.as_chunks::<3>().0.iter().enumerate() {
            let x = pixel_index % frame_w;
            let y = pixel_index / frame_w;

            if x < x_start || x >= x_end || y < y_start || y >= y_end {
                continue;
            }

            if chunk != [10u8, 20, 30].as_slice() {
                found_label = true;
                break;
            }
        }
        assert!(found_label);

        let mut min_x = u32::MAX;
        let mut min_y = u32::MAX;
        let mut max_x = 0u32;
        let mut max_y = 0u32;

        for button in &menu.buttons {
            let rect = button.rect;
            let x = u32::from(rect.x);
            let y = u32::from(rect.y);
            let x1 = x + u32::from(rect.w) - 1;
            let y1 = y + u32::from(rect.h) - 1;

            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x1);
            max_y = max_y.max(y1);
        }

        assert_eq!(u32::from(rendered.highlight.x), min_x);
        assert_eq!(u32::from(rendered.highlight.y), min_y);
        assert_eq!(u32::from(rendered.highlight.width), max_x - min_x + 1);
        assert_eq!(u32::from(rendered.highlight.height), max_y - min_y + 1);

        let layer_x = u32::from(rendered.highlight.x);
        let layer_y = u32::from(rendered.highlight.y);
        let layer_w = usize::from(rendered.highlight.width);

        for button in &menu.buttons {
            let rect = button.rect;
            let x = u32::from(rect.x);
            let y = u32::from(rect.y);
            let x1 = x + u32::from(rect.w) - 1;
            let y1 = y + u32::from(rect.h) - 1;

            let top_left = ((y - layer_y) as usize) * layer_w + (x - layer_x) as usize;
            let top_right = ((y - layer_y) as usize) * layer_w + (x1 - layer_x) as usize;
            let bottom_left = ((y1 - layer_y) as usize) * layer_w + (x - layer_x) as usize;
            let bottom_right = ((y1 - layer_y) as usize) * layer_w + (x1 - layer_x) as usize;

            assert_eq!(rendered.highlight.pixels[top_left], 1);
            assert_eq!(rendered.highlight.pixels[top_right], 1);
            assert_eq!(rendered.highlight.pixels[bottom_left], 1);
            assert_eq!(rendered.highlight.pixels[bottom_right], 1);

            let cx = x + u32::from(rect.w) / 2;
            let cy = y + u32::from(rect.h) / 2;
            let centre = ((cy - layer_y) as usize) * layer_w + (cx - layer_x) as usize;
            assert_eq!(rendered.highlight.pixels[centre], 0);
        }

        Ok(())
    }

    #[test]
    fn render_requires_buttons() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let menu = project.menu(menu_id).ok_or("missing menu")?;

        assert!(render_menu(&project, menu).is_err());

        Ok(())
    }

    #[test]
    fn palette_and_color_sets() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        project.add_button(menu_id, "A".to_string(), None, Link::Stop)?;

        let menu = project.menu(menu_id).ok_or("missing menu")?;
        let rendered = render_menu(&project, menu)?;

        assert_eq!(rendered.palette[0], ycrcb_entry([0, 0, 0]));
        assert_eq!(rendered.palette[1], ycrcb_entry(menu.highlight_color));
        assert_eq!(rendered.palette[2], ycrcb_entry([255, 255, 255]));
        assert_eq!(rendered.palette[3], ycrcb_entry(menu.text_color));
        // Without a colour set every group is the highlight colour and white.
        assert_eq!(rendered.palette[4], ycrcb_entry(menu.highlight_color));
        assert_eq!(rendered.palette[7], ycrcb_entry([255, 255, 255]));

        assert_eq!(
            rendered.color_sets[0],
            [0x33, 0x10, 0x00, 0xF0, 0x33, 0x20, 0x00, 0xF0]
        );
        assert_eq!(
            rendered.color_sets[1],
            [0x33, 0x40, 0x00, 0xF0, 0x33, 0x50, 0x00, 0xF0]
        );
        assert_eq!(
            rendered.color_sets[2],
            [0x33, 0x60, 0x00, 0xF0, 0x33, 0x70, 0x00, 0xF0]
        );

        Ok(())
    }
}
