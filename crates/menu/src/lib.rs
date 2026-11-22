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
mod effects;
mod graphic;
mod highlight;
mod palette;
mod route;
mod shadow;
pub mod subtitle;
pub mod subtitle_file;
mod text_block;
mod wrap;

use disccraft_project::frame_size;
use thiserror::Error;

pub use disccraft_photo::{PsdTreeNode, psd_layer_tree};
pub use disccraft_project::{Menu, Project, Rect};
pub use draw::subpicture_from_png;
pub use graphic::graphic_coverage;
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

/// A picture for a button's video thumbnail (Encore's `(%)` layer): RGB24 of `width` x `height`, stretched over the
/// thumbnail's whole area as Encore does (no bars, nothing cut off).
#[derive(Clone, Copy, Debug)]
pub struct ThumbnailPicture<'a> {
    /// The button's index.
    pub button: usize,
    /// The pixels.
    pub rgb: &'a [u8],
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
}

/// What a menu picture is drawn with besides the menu itself.
#[derive(Clone, Copy, Debug, Default)]
pub struct MenuPictures<'a> {
    /// A picture of the whole screen under the menu (a motion menu's video frame, already 4:3 as `asset.frame` and
    /// the build give it), stretched to the frame instead of the menu's background.
    pub background: Option<(&'a [u8], usize, usize)>,
    /// Pictures for the buttons' video thumbnails.
    pub thumbnails: &'a [ThumbnailPicture<'a>],
}

/// A still picture (an image asset, such as a slideshow's slide shown in a video thumbnail) as RGB24 of its own
/// size, any transparency over black; `None` when it cannot be read.
pub fn image_rgb(
    project: &Project,
    asset: disccraft_project::AssetId,
) -> Option<(Vec<u8>, usize, usize)> {
    let (width, height) = frame_size(&project.settings);
    let image = draw::load_image(project, asset, width, height).ok()?;
    let pixels = image.width.checked_mul(image.height)?;
    let rgb: Vec<u8> = match image.channels {
        3 => image.data,
        4 => image
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|[r, g, b, a]| {
                // At most 255 x 255 / 255: fits u8.
                let over = |value: u8| {
                    u8::try_from(u16::from(value) * u16::from(*a) / 255).unwrap_or(value)
                };
                [over(*r), over(*g), over(*b)]
            })
            .collect(),
        _ => return None,
    };
    (rgb.len() == pixels.checked_mul(3)?).then_some((rgb, image.width, image.height))
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
    /// The palette entry of each highlight pixel value (0 to 3) in the normal state (Encore's 標準グループ colours 1
    /// to 3; colour 1 is [`NORMAL_PALETTE_ENTRY`]).
    pub normal_entries: [u8; 4],
    /// The contrast (0 to 15) of each highlight pixel value in the normal state; 0 hides it.
    pub normal_contrasts: [u8; 4],
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
    render_disc_menu(project, menu, MenuPictures::default())
}

/// The highlight of the button `index` of `menu` alone (as the menu's own highlight draws it, without the other
/// buttons'): the states of a disc's button show only its own (GitHub issue #13: a picture laid out as a button
/// over the others took their highlights into its own).
///
/// # Errors
///
/// When the menu has no such button.
pub fn button_highlight(
    project: &Project,
    menu: &Menu,
    index: usize,
) -> Result<HighlightLayer, MenuError> {
    let button = menu
        .buttons
        .get(index)
        .ok_or_else(|| MenuError::Invalid("the menu has no such button".to_string()))?;
    let mut alone = menu.clone();
    alone.buttons = vec![button.clone()];
    highlight::build(&alone, draw::is_document_menu(project, menu))
}

/// Renders a menu for a disc like [`render_menu`], with `pictures` (a motion menu's frame, the buttons' video
/// thumbnails).
///
/// # Errors
///
/// As [`render_menu`]; also when the background picture does not match its size.
pub fn render_disc_menu(
    project: &Project,
    menu: &Menu,
    pictures: MenuPictures<'_>,
) -> Result<RenderedMenu, MenuError> {
    if menu.buttons.is_empty() {
        return Err(MenuError::Invalid("menu has no buttons".to_string()));
    }
    render(project, menu, pictures)
}

/// Renders a menu for an editor like [`render_menu_preview`], with `pictures`.
///
/// # Errors
///
/// As [`render_menu_preview`]; also when the background picture does not match its size.
pub fn render_menu_with(
    project: &Project,
    menu: &Menu,
    pictures: MenuPictures<'_>,
) -> Result<RenderedMenu, MenuError> {
    render(project, menu, pictures)
}

/// Renders a menu for an editor: like [`render_menu`], but a menu without buttons (a new, blank menu) is shown too,
/// with an empty highlight layer.
///
/// # Errors
///
/// As [`render_menu`], except for a menu without buttons.
pub fn render_menu_preview(project: &Project, menu: &Menu) -> Result<RenderedMenu, MenuError> {
    render(project, menu, MenuPictures::default())
}

/// Draws the menu: the background (or the picture under it), the pictures and shapes, the texts, the buttons'
/// video thumbnails and the button texts, bottom to top.
fn render(
    project: &Project,
    menu: &Menu,
    pictures: MenuPictures<'_>,
) -> Result<RenderedMenu, MenuError> {
    let (width, height) = frame_size(&project.settings);
    let mut rgb = draw::new_frame(width, height)?;

    match pictures.background {
        // A frame of the menu's own size is used as it is (every frame of a motion menu).
        Some((pixels, picture_width, picture_height))
            if (picture_width, picture_height) == (usize::from(width), usize::from(height))
                && pixels.len() == rgb.len() =>
        {
            rgb.copy_from_slice(pixels);
        }
        Some((pixels, picture_width, picture_height)) => {
            if !draw::scale_rgb_into(
                pixels,
                picture_width,
                picture_height,
                width,
                height,
                &mut rgb,
            ) {
                return Err(MenuError::Invalid(
                    "the background picture does not match its size".to_string(),
                ));
            }
        }
        None => draw::draw_background(project, menu, width, height, &mut rgb)?,
    }
    // A PSD shown by page draws the page's own layers over the background (M10.27).
    draw::draw_psd_page(project, menu, width, height, &mut rgb)?;
    draw_graphics_rgb(project, menu, width, height, &mut rgb);
    draw::draw_texts(menu, width, height, &mut rgb);
    draw::draw_thumbnails(menu, width, height, &mut rgb, pictures.thumbnails);
    draw::draw_labels(menu, width, height, &mut rgb);

    let highlight = highlight::build(menu, draw::is_document_menu(project, menu))?;
    let buttons = button_geometry(menu, &project.settings.routing);
    let colors = palette::dvd_colors(menu, &highlight);

    Ok(RenderedMenu {
        width,
        height,
        rgb,
        highlight,
        palette: colors.palette,
        color_sets: colors.color_sets,
        normal_entries: colors.normal_entries,
        normal_contrasts: colors.normal_contrasts,
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
    render(
        project,
        menu,
        MenuPictures {
            background: Some((rgb, width, height)),
            thumbnails: &[],
        },
    )
}

/// The layers of `menu` for editing it in an image editor (Encore's "Edit Menu in Photoshop"), bottom to top: the
/// background ("背景"), one layer per text object, and one `(+)` group per button with its text and its highlight as
/// hidden `(=1)` / `(=2)` / `(=3)` layers (the frame, the text or the button's subpicture, each colour in its group's
/// selected colour). Texts and buttons' texts are editable type layers (M6.398) showing what DiscCraft draws, a
/// text's drop shadow a layer style; a text with other layer effects is pixels. The first highlight layer (else the
/// pixel text layer) marks the button's corners faintly (alpha 1), so the group keeps the button's
/// rectangle. Button 1 is the lowest group, as Encore numbers a PSD menu's
/// buttons from the bottom of the layer stack (and [`disccraft_photo::load_psd_menu`] reads them back in that
/// order). `picture` replaces the
/// background (a motion menu's first frame).
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
    let artwork = draw::is_document_menu(project, menu);
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
            // A replacement layer keeps Encore's `(!)` in front of its name.
            name: if graphic.replacement {
                format!("(!) {}", graphic.kind_name(project))
            } else {
                graphic.kind_name(project)
            },
            rgba: graphic_rgba(project, graphic, width, height),
            visible: true,
        });
    }
    for text in &menu.texts {
        layers.push(text_psd_layer(menu, text, width, height));
    }
    for (index, button) in menu.buttons.iter().enumerate() {
        let group = menu.highlight_group(button.highlight_group);
        // The button's own name (a PSD button's `(+)` group's, M10.46), else its text, else its number.
        let title = if !button.name.trim().is_empty() {
            button.name.clone()
        } else if button.label.trim().is_empty() {
            format!("ボタン {}", index.saturating_add(1))
        } else {
            button.label.clone()
        };
        let rect = button.rect;
        let values = highlight::button_values(menu, index, artwork).unwrap_or_default();
        let mut highlights = Vec::new();
        // A PSD button's layers in their own colours (M6.335) go back as they are.
        let own = highlight::color_layers(button);
        for (n, layer) in (1_u8..).zip(own.iter().flatten()) {
            let Some(layer) = layer else {
                continue;
            };
            let mut rgba = vec![0u8; usize::from(width) * usize::from(height) * 4];
            for (row, line) in layer
                .chunks(usize::from(rect.w).max(1).saturating_mul(4))
                .enumerate()
            {
                let y = usize::from(rect.y) + row;
                let x = usize::from(rect.x);
                if y >= usize::from(height) || x >= usize::from(width) {
                    continue;
                }
                let take = line.len().min((usize::from(width) - x) * 4);
                let start = (y * usize::from(width) + x) * 4;
                if let (Some(target), Some(source)) =
                    (rgba.get_mut(start..start + take), line.get(..take))
                {
                    target.copy_from_slice(source);
                }
            }
            highlights.push(PsdLayer::Raster {
                name: format!("(={n}) ハイライト"),
                rgba,
                visible: false,
            });
        }
        for n in 1..=3_u8 {
            if own.is_some() || !values.contains(&n) {
                continue;
            }
            let [r, g, b] = group.selected_color(n).0;
            let mut rgba = vec![0u8; usize::from(width) * usize::from(height) * 4];
            for (row, line) in values.chunks(usize::from(rect.w).max(1)).enumerate() {
                for (col, value) in line.iter().enumerate() {
                    let x = usize::from(rect.x) + col;
                    let y = usize::from(rect.y) + row;
                    if *value != n || x >= usize::from(width) || y >= usize::from(height) {
                        continue;
                    }
                    let i = (y * usize::from(width) + x) * 4;
                    if let Some(pixel) = rgba.get_mut(i..i + 4) {
                        pixel.copy_from_slice(&[r, g, b, 255]);
                    }
                }
            }
            highlights.push(PsdLayer::Raster {
                name: format!("(={n}) ハイライト"),
                rgba,
                visible: false,
            });
        }
        let mut label = draw::label_rgba(menu, button, width, height);
        // The text as an editable type layer (M6.398) when the group has a highlight layer to keep the button's
        // corners (a type layer's pixels are set anew when its text is edited).
        let typed = if highlights.is_empty() {
            None
        } else {
            draw::label_type(menu, button)
        };
        // The button's corners, faintly, so the group covers the whole button.
        let marked = match (&typed, highlights.first_mut()) {
            (Some(_), Some(PsdLayer::Raster { rgba, .. })) => rgba,
            _ => &mut label,
        };
        mark_corners(marked, rect, (width, height));
        let mut children = Vec::new();
        if let Some(area) = button.thumbnail_placed().map(|area| {
            disccraft_project::rotated_bounds_about(
                area,
                button.content_centre(),
                button.orientation.angle,
            )
        }) {
            // Encore's video thumbnail layer: a grey placeholder where the video goes.
            let mut placeholder = vec![0u8; usize::from(width) * usize::from(height) * 4];
            for y in area.y..area.y.saturating_add(area.h).min(height) {
                for x in area.x..area.x.saturating_add(area.w).min(width) {
                    let i = (usize::from(y) * usize::from(width) + usize::from(x)) * 4;
                    if let Some(pixel) = placeholder.get_mut(i..i + 4) {
                        pixel.copy_from_slice(&THUMBNAIL_PLACEHOLDER);
                    }
                }
            }
            children.push(PsdLayer::Raster {
                name: "(%) ビデオ".to_owned(),
                rgba: placeholder,
                visible: true,
            });
        }
        layers.push(PsdLayer::Group {
            name: format!("(+) {title}"),
            // M10.49: the set made from a PSD's keeps that layer's id, so the edited PSD finds this button again.
            id: button.psd_layer,
            children: [
                children,
                vec![match typed {
                    Some(setting) => PsdLayer::Text {
                        name: title,
                        rgba: label,
                        visible: true,
                        text: Box::new(setting),
                    },
                    None => PsdLayer::Raster {
                        name: title,
                        rgba: label,
                        visible: true,
                    },
                }],
                highlights,
            ]
            .concat(),
        });
    }
    Ok((width, height, layers))
}

/// A menu text as a layer of its PSD: an editable type layer (M6.398) showing what DiscCraft draws, its drop
/// shadow a layer style; a text with other layer effects stays pixels (they are drawn into it).
fn text_psd_layer(
    menu: &Menu,
    text: &disccraft_project::MenuText,
    width: u16,
    height: u16,
) -> disccraft_photo::PsdLayer {
    use disccraft_photo::PsdLayer;
    let typed = if text.effects.is_empty() {
        draw::text_type(menu, text)
    } else {
        None
    };
    match typed {
        Some(setting) => PsdLayer::Text {
            // Photoshop names a type layer after its text.
            name: text
                .text
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or("テキスト")
                .to_owned(),
            rgba: draw::text_rgba(
                menu,
                &disccraft_project::MenuText {
                    shadow: None,
                    ..text.clone()
                },
                width,
                height,
            ),
            visible: !text.hidden,
            text: Box::new(setting),
        },
        None => PsdLayer::Raster {
            name: format!("テキスト: {}", text.text.lines().next().unwrap_or_default()),
            rgba: draw::text_rgba(menu, text, width, height),
            visible: !text.hidden,
        },
    }
}

/// Marks the corners of `rect` in an RGBA layer of the frame faintly (alpha 1 where nothing is drawn), so the layer
/// covers the whole rectangle.
fn mark_corners(rgba: &mut [u8], rect: Rect, (width, height): (u16, u16)) {
    let right = rect
        .x
        .saturating_add(rect.w)
        .saturating_sub(1)
        .min(width.saturating_sub(1));
    let bottom = rect
        .y
        .saturating_add(rect.h)
        .saturating_sub(1)
        .min(height.saturating_sub(1));
    for (x, y) in [
        (rect.x, rect.y),
        (right, rect.y),
        (rect.x, bottom),
        (right, bottom),
    ] {
        let i = (usize::from(y) * usize::from(width) + usize::from(x)) * 4;
        if let Some(alpha) = rgba.get_mut(i + 3)
            && *alpha == 0
        {
            *alpha = 1;
        }
    }
}

/// The grey of a video thumbnail's placeholder layer in a written PSD.
const THUMBNAIL_PLACEHOLDER: [u8; 4] = [128, 128, 128, 255];

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

/// Whether a paragraph text has lines its box's height leaves out (Encore's overset text: they are not drawn).
pub fn text_overset(menu: &Menu, text: &disccraft_project::MenuText) -> bool {
    draw::text_overset(menu, text)
}

/// The frame rectangle a menu text object covers as it is drawn, turned by its free angle (Encore's Rotate tool):
/// for selecting and outlining it. Its box before the turn is [`text_rect`].
pub fn text_bounds(menu: &Menu, text: &disccraft_project::MenuText) -> Rect {
    draw::text_bounds(menu, text)
}

/// The frame rectangle a graphic covers as it is drawn: its box turned by its free angle.
pub fn graphic_bounds(graphic: &disccraft_project::MenuGraphic) -> Rect {
    graphic::drawn_rect(graphic)
}

/// Where the menu's page of its PSD background draws over the shared layers (M10.27), in the frame; `None` when it
/// draws nothing (or the menu shows no page of a PSD).
pub fn psd_page_bounds(project: &Project, menu: &Menu) -> Option<Rect> {
    let (width, height) = frame_size(&project.settings);
    draw::psd_page_rect(project, menu, width, height)
}

/// The frame rectangle a button's text covers where it is drawn (Encore's text layer of a button set, for
/// selecting it with the Direct Select tool); `None` for a button without text.
pub fn label_bounds(menu: &Menu, button: &disccraft_project::Button) -> Option<Rect> {
    draw::label_bounds(menu, button)
}

/// The frame rectangle a button's content covers as it is drawn (its content box, text and video thumbnail, moved
/// and turned), inside the frame of `frame` size: Encore's bounds of the button set, which the button's rectangle
/// becomes when its content is moved out of it or turned.
pub fn button_extent(menu: &Menu, button: &disccraft_project::Button, frame: (u16, u16)) -> Rect {
    draw::button_extent(menu, button, frame)
}

/// Encore's Create Subpicture for a button without artwork in a PSD (M6.330): the shape of its text and video
/// thumbnail as they are drawn before the button is turned, over the frame rectangle returned (its content box, or
/// more when the text was moved out of it). `None` when the button shows neither.
pub fn own_subpicture(menu: &Menu, button: &disccraft_project::Button) -> Option<(Rect, Vec<u8>)> {
    draw::own_subpicture(menu, button)
}

/// A PSD button's subpicture layers in their own colours as a rich-colour Blu-ray highlight shows them, as Encore
/// does (M6.335): the normal, selected and activated pictures over the button's rectangle, straight-alpha RGBA
/// `rect.w * rect.h * 4` bytes each (`None` for a state that shows nothing). A subpicture with only a `(=1)` layer
/// shows it when selected and activated; with more layers each state shows its own. `None` for a button without
/// colour layers.
pub fn button_color_states(button: &disccraft_project::Button) -> Option<[Option<Vec<u8>>; 3]> {
    highlight::color_states(button)
}

/// Render `menu` over a caller-supplied background frame (RGB24, frame size of the project) — one frame of a motion menu.
pub fn render_menu_over(
    project: &Project,
    menu: &Menu,
    background: &[u8],
) -> Result<RenderedMenu, MenuError> {
    let (width, height) = frame_size(&project.settings);
    let (width, height) = (usize::from(width), usize::from(height));
    if Some(background.len()) != width.checked_mul(height).and_then(|n| n.checked_mul(3)) {
        return Err(MenuError::Invalid(
            "background frame size does not match the menu".to_string(),
        ));
    }
    render_disc_menu(
        project,
        menu,
        MenuPictures {
            background: Some((background, width, height)),
            thumbnails: &[],
        },
    )
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
    use super::{auto_route, button_highlight, render_menu, ycrcb_entry};
    use disccraft_project::{Link, MenuBackground, Project, Rect};
    use std::error::Error;

    /// GitHub issue #13: a button's own highlight leaves the other buttons' out, and in a menu made from a PSD a button
    /// without subpicture layers (a picture laid out as a button) has none, as in Encore, not DiscCraft's frame.
    #[test]
    fn a_buttons_own_highlight_and_a_psd_buttons_without_layers() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        project.menu_mut(menu_id).ok_or("menu")?.buttons.clear();
        let wide = Rect {
            x: 0,
            y: 40,
            w: 200,
            h: 10,
        };
        let small = Rect {
            x: 50,
            y: 20,
            w: 40,
            h: 40,
        };
        project.add_button(menu_id, String::new(), Some(wide), Link::NotSet)?;
        project.add_button(menu_id, String::new(), Some(small), Link::NotSet)?;
        let menu = project.menu(menu_id).ok_or("menu")?.clone();
        let value = |layer: &super::HighlightLayer, x: u16, y: u16| -> u8 {
            if x < layer.x
                || y < layer.y
                || x >= layer.x + layer.width
                || y >= layer.y + layer.height
            {
                return 0;
            }
            let at = usize::from(y - layer.y) * usize::from(layer.width) + usize::from(x - layer.x);
            layer.pixels.get(at).copied().unwrap_or(0)
        };
        // The whole menu's highlight has the wide button's frame across the small one; its own has only its frame.
        let all = render_menu(&project, &menu)?.highlight;
        assert_ne!(value(&all, 60, 40), 0);
        let own = button_highlight(&project, &menu, 1)?;
        assert_eq!(value(&own, 60, 40), 0);
        assert_ne!(value(&own, 60, 20), 0);
        // Made from a PSD: no frame for either.
        let psd = project.add_asset(std::path::Path::new("missing-menu.psd"))?;
        let mut from_psd = menu.clone();
        from_psd.background = MenuBackground::Image { asset: psd };
        for index in 0..2 {
            let layer = button_highlight(&project, &from_psd, index)?;
            assert!(
                layer.pixels.iter().all(|value| *value == 0),
                "button {index}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_buttons_subpicture_is_its_highlight_and_goes_through_a_psd() -> Result<(), Box<dyn Error>>
    {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let rect = Rect {
            x: 100,
            y: 100,
            w: 8,
            h: 4,
        };
        let _ = project.add_button(menu_id, String::new(), Some(rect), Link::NotSet)?;
        // A 4 x 2 subpicture (colours 1, 2, 3 and none) stretched over the 8 x 4 button.
        let sub = disccraft_project::Subpicture::from_values(4, 2, &[1, 1, 2, 3, 0, 0, 2, 3])
            .ok_or("subpicture")?;
        let menu = project.menu_mut(menu_id).ok_or("menu")?;
        menu.buttons.first_mut().ok_or("button")?.subpicture = Some(sub);
        let menu = menu.clone();
        let expected = [
            [1, 1, 1, 1, 2, 2, 3, 3],
            [1, 1, 1, 1, 2, 2, 3, 3],
            [0, 0, 0, 0, 2, 2, 3, 3],
            [0, 0, 0, 0, 2, 2, 3, 3],
        ]
        .concat();
        let highlight = render_menu(&project, &menu)?.highlight;
        assert_eq!(
            (highlight.x, highlight.y, highlight.width, highlight.height),
            (100, 100, 8, 4)
        );
        assert_eq!(highlight.pixels, expected);
        assert!(
            highlight
                .coverage
                .iter()
                .zip(&expected)
                .all(|(cover, value)| (*cover == 255) == (*value != 0))
        );

        // Written to a PSD as (=1) to (=3) layers, it reads back the same, over the same rectangle.
        let dir =
            std::env::temp_dir().join(format!("disccraft-psd-sub-menu-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("menu.psd");
        super::export_menu_psd(&project, &menu, None, &path)?;
        let read = disccraft_photo::load_psd_menu(&path)?;
        let button = read.buttons.first().ok_or("button")?;
        assert_eq!(
            (button.x, button.y, button.width, button.height),
            (100, 100, 8, 4)
        );
        let values = button.subpicture.as_ref().map(|sub| sub.values.clone());
        assert_eq!(values, Some(expected));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// M6.342: a text object's and a button's text's layer effects are drawn: a red stroke around white text puts red
    /// pixels next to it, which are not there without it.
    #[test]
    fn texts_and_button_texts_draw_their_layer_effects() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let rect = Rect {
            x: 300,
            y: 300,
            w: 200,
            h: 60,
        };
        let _ = project.add_button(menu_id, "Go".to_owned(), Some(rect), Link::NotSet)?;
        let menu = project.menu_mut(menu_id).ok_or("menu")?;
        menu.background = MenuBackground::Color { color: [0, 0, 0] };
        menu.texts.push(disccraft_project::MenuText {
            effects: Default::default(),
            box_width: None,
            box_height: None,
            text: "Title".to_owned(),
            x: 100,
            y: 100,
            size: 60,
            color: Some([255, 255, 255]),
            font: None,
            font_style: None,
            format: Default::default(),
            shadow: None,
            orientation: Default::default(),
            vertical: false,
            hidden: false,
            locked: false,
            spans: Vec::new(),
        });
        let plain = menu.clone();
        let stroke = disccraft_project::LayerEffects {
            stroke: Some(disccraft_project::StrokeEffect {
                size: 3,
                color: [255, 0, 0],
                opacity: 100,
            }),
            ..Default::default()
        };
        if let Some(text) = menu.texts.first_mut() {
            text.effects = stroke;
        }
        if let Some(button) = menu.buttons.first_mut() {
            button.effects = stroke;
        }
        let stroked = menu.clone();
        // Red pixels above y 260 (the text) and below it (the button's text).
        let reds = |menu: &disccraft_project::Menu| -> Result<(usize, usize), Box<dyn Error>> {
            let rendered = render_menu(&project, menu)?;
            let width = usize::from(rendered.width);
            let mut counts = (0, 0);
            for (index, pixel) in rendered.rgb.as_chunks::<3>().0.iter().enumerate() {
                if pixel[0] > 200 && pixel[1] < 60 && pixel[2] < 60 {
                    if index / width < 260 {
                        counts.0 += 1;
                    } else {
                        counts.1 += 1;
                    }
                }
            }
            Ok(counts)
        };
        assert_eq!(reds(&plain)?, (0, 0));
        let (text, button) = reds(&stroked)?;
        assert!(text > 50 && button > 20, "{text} {button}");
        Ok(())
    }

    /// M6.335: a PSD button's subpicture layers in their own colours are laid over the button as its subpicture is
    /// (stretched), each state showing its layer, and go to a PSD and back as they are.
    /// M6.398 (GitHub issue #13: a menu made in DiscCraft came out of Save Menu as File flattened): its texts and
    /// its buttons' texts go into the PSD as editable type set where DiscCraft draws them (point text on its first
    /// baseline, a paragraph text in its box, a drop shadow as a layer style) and show what DiscCraft draws; a text
    /// with other layer effects stays pixels; the button's set still covers the whole button.
    #[test]
    fn texts_and_button_texts_go_into_a_psd_as_editable_type() -> Result<(), Box<dyn Error>> {
        use disccraft_photo::PsdLayer;
        use disccraft_project::{
            DropShadow, LayerEffects, MenuText, Orientation, StrokeEffect, TextFormat,
        };
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let rect = Rect {
            x: 100,
            y: 300,
            w: 180,
            h: 40,
        };
        let _ = project.add_button(menu_id, "Play".to_owned(), Some(rect), Link::NotSet)?;
        let menu = project.menu_mut(menu_id).ok_or("menu")?;
        menu.background = MenuBackground::Color { color: [0, 0, 0] };
        menu.text_color = [255, 255, 255];
        let text = |content: &str, x: u16, y: u16| MenuText {
            effects: Default::default(),
            box_width: None,
            box_height: None,
            text: content.to_owned(),
            x,
            y,
            size: 32,
            color: None,
            font: None,
            font_style: None,
            format: TextFormat::default(),
            shadow: None,
            orientation: Orientation::default(),
            hidden: false,
            locked: false,
            vertical: false,
            spans: Vec::new(),
        };
        menu.texts.push(MenuText {
            shadow: Some(DropShadow::default()),
            ..text("Title", 60, 40)
        });
        menu.texts.push(MenuText {
            box_width: Some(200),
            ..text("A paragraph text set in its box", 300, 40)
        });
        menu.texts.push(MenuText {
            effects: LayerEffects {
                stroke: Some(StrokeEffect {
                    size: 2,
                    color: [255, 0, 0],
                    opacity: 100,
                }),
                ..LayerEffects::default()
            },
            ..text("Stroked", 60, 220)
        });
        let menu = menu.clone();

        let (_, _, layers) = super::menu_psd_layers(&project, &menu, None)?;
        let typed = |index: usize| match layers.get(index) {
            Some(PsdLayer::Text { name, text, .. }) => Some((name.clone(), text.clone())),
            _ => None,
        };
        let (name, title) = typed(1).ok_or("the title is no type layer")?;
        assert_eq!((name.as_str(), title.text.as_str()), ("Title", "Title"));
        assert!(title.area.is_none());
        assert!(title.shadow.is_some());
        // Point text: its origin is on the first baseline at the left end of the line.
        let [a, _, _, d, x, y] = title.transform;
        assert!((a - 1.0).abs() < 1e-6 && (d - 1.0).abs() < 1e-6);
        assert!((x - 60.0).abs() < 4.0 && y > 50.0 && y < 80.0, "{x} {y}");
        let (_, paragraph) = typed(2).ok_or("the paragraph is no type layer")?;
        let [_, _, width, _] = paragraph.area.ok_or("no box")?;
        assert!((width - 200.0).abs() < 1.0, "{width}");
        // Its box starts at the text's left and the font's typographic ascent above DiscCraft's first baseline
        // (where Photoshop sets the first line): a little below the top of the lines.
        let [_, _, _, _, x, y] = paragraph.transform;
        assert!((x - 300.0).abs() < 2.0 && y > 39.0 && y < 56.0, "{x} {y}");
        assert!(
            matches!(layers.get(3), Some(PsdLayer::Raster { name, .. }) if name == "テキスト: Stroked")
        );
        let Some(PsdLayer::Group { children, .. }) = layers.get(4) else {
            return Err("no button set".into());
        };
        assert!(
            children
                .iter()
                .any(|layer| matches!(layer, PsdLayer::Text { text, .. } if text.text == "Play"))
        );

        // Read back: the button covers its whole rectangle, and what the type layers show is what DiscCraft draws.
        let dir = std::env::temp_dir().join(format!("disccraft-psd-type-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("typed.psd");
        super::export_menu_psd(&project, &menu, None, &path)?;
        let read = disccraft_photo::load_psd_menu(&path)?;
        let _ = std::fs::remove_dir_all(&dir);
        let button = read.buttons.first().ok_or("no button")?;
        assert_eq!(
            (button.x, button.y, button.width, button.height),
            (100, 300, 180, 40)
        );
        let drawn = render_menu(&project, &menu)?;
        // The paragraph and the button's text (no shadow there) are where DiscCraft draws them: its solid pixels
        // and the background around them come back as they were. (Photoshop, and PhotoCraft as it, lays type over
        // what is below with a text gamma, so the half-covered edge pixels come back a little brighter: alpha 62
        // shows as 100 in Photoshop CS6.)
        let mut differ = 0_usize;
        let mut solid = 0_usize;
        for (top, bottom, left, right) in [(40, 200, 300, 520), (300, 340, 100, 280)] {
            for y in top..bottom {
                for x in left..right {
                    let ours = drawn.rgb.get((y * 720 + x) * 3..(y * 720 + x) * 3 + 3);
                    let theirs = read
                        .background
                        .rgba
                        .get((y * 720 + x) * 4..(y * 720 + x) * 4 + 3);
                    let (Some(ours), Some(theirs)) = (ours, theirs) else {
                        continue;
                    };
                    let plain = ours.iter().all(|v| *v == 255) || ours.iter().all(|v| *v == 0);
                    solid += usize::from(ours.iter().all(|v| *v == 255));
                    if plain && ours.iter().zip(theirs).any(|(a, b)| a.abs_diff(*b) > 2) {
                        differ += 1;
                    }
                }
            }
        }
        assert!(solid > 500, "{solid}");
        assert_eq!(differ, 0);
        Ok(())
    }

    #[test]
    fn colour_layers_are_the_states_and_go_through_a_psd() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let rect = Rect {
            x: 100,
            y: 100,
            w: 4,
            h: 2,
        };
        let _ = project.add_button(menu_id, String::new(), Some(rect), Link::NotSet)?;
        // 2 x 1: (=1) red then soft orange; (=2) only its right pixel, green.
        let mut sub =
            disccraft_project::Subpicture::from_values(2, 1, &[1, 2]).ok_or("subpicture")?;
        let red = [255, 0, 0, 255];
        let soft = [250, 140, 0, 128];
        let green = [0, 200, 0, 255];
        sub.color_layers = [
            disccraft_project::ColorLayer::from_rgba(2, 1, &[red, soft].concat()),
            disccraft_project::ColorLayer::from_rgba(2, 1, &[[0; 4], green].concat()),
            None,
        ];
        let menu = project.menu_mut(menu_id).ok_or("menu")?;
        menu.buttons.first_mut().ok_or("button")?.subpicture = Some(sub);
        let menu = menu.clone();
        let button = menu.buttons.first().ok_or("button")?;
        let states = super::button_color_states(button).ok_or("states")?;
        // Stretched over 4 x 2: each source pixel two wide, two high.
        let row = |left: [u8; 4], right: [u8; 4]| [left, left, right, right].concat();
        let normal = [row(red, soft), row(red, soft)].concat();
        let selected = [row([0; 4], green), row([0; 4], green)].concat();
        assert_eq!(states[0].as_deref(), Some(&normal[..]));
        assert_eq!(states[1].as_deref(), Some(&selected[..]));
        assert_eq!(states[2], None);

        // To a PSD and back: the same layers, in their own colours.
        let dir =
            std::env::temp_dir().join(format!("disccraft-psd-colour-menu-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("menu.psd");
        super::export_menu_psd(&project, &menu, None, &path)?;
        let read = disccraft_photo::load_psd_menu(&path)?;
        let _ = std::fs::remove_dir_all(&dir);
        let read = read
            .buttons
            .first()
            .and_then(|button| button.subpicture.as_ref())
            .ok_or("read subpicture")?;
        assert_eq!(read.layers[0].as_deref(), Some(&normal[..]));
        assert_eq!(read.layers[1].as_deref(), Some(&selected[..]));
        assert_eq!(read.layers[2], None);
        Ok(())
    }

    /// M6.329: a subpicture with an area of its own (a pasted layer that reaches past the button) is drawn over
    /// that area, and the button's extent takes it in; turned, it turns with the content about the content box's
    /// centre.
    #[test]
    fn a_subpicture_reaching_past_the_button_is_drawn_there() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let rect = Rect {
            x: 100,
            y: 100,
            w: 8,
            h: 4,
        };
        let _ = project.add_button(menu_id, String::new(), Some(rect), Link::NotSet)?;
        // 12 x 4: the button's 8 columns and 4 more to its right, colour 1 in the last 4.
        let mut sub = disccraft_project::Subpicture::from_values(
            12,
            4,
            &[[0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1]; 4].concat(),
        )
        .ok_or("subpicture")?;
        sub.area = Some(disccraft_project::ContentBox {
            x: 0,
            y: 0,
            w: 12,
            h: 4,
        });
        let menu = project.menu_mut(menu_id).ok_or("menu")?;
        menu.buttons.first_mut().ok_or("button")?.subpicture = Some(sub);
        let mut menu = menu.clone();
        let button = menu.buttons.first().ok_or("button")?;
        let extent = super::button_extent(&menu, button, (720, 480));
        assert_eq!(
            extent,
            Rect {
                x: 100,
                y: 100,
                w: 12,
                h: 4
            }
        );
        // As a paste leaves it: the rectangle the extent, the content box where it was.
        if let Some(button) = menu.buttons.first_mut() {
            button.rect = extent;
            button.body = Some(disccraft_project::ContentBox {
                x: 0,
                y: 0,
                w: 8,
                h: 4,
            });
        }
        let highlight = render_menu(&project, &menu)?.highlight;
        let lit: Vec<(u16, u16)> = (0..highlight.height)
            .flat_map(|y| (0..highlight.width).map(move |x| (x, y)))
            .filter(|(x, y)| {
                highlight
                    .pixels
                    .get(usize::from(*y) * usize::from(highlight.width) + usize::from(*x))
                    .is_some_and(|value| *value == 1)
            })
            .map(|(x, y)| (highlight.x + x, highlight.y + y))
            .collect();
        assert_eq!(lit.len(), 16);
        assert!(
            lit.iter()
                .all(|(x, y)| (108..112).contains(x) && (100..104).contains(y))
        );

        // Turned half a turn: about the content box's centre (104, 102), so the lit part goes to its left.
        let mut turned = menu.clone();
        let content = menu.buttons.first().ok_or("button")?.content_box();
        if let Some(button) = turned.buttons.first_mut() {
            button.orientation.angle = 1800;
        }
        // Refit as turning a button does: the rectangle the turned extent, the content box kept.
        let extent =
            super::button_extent(&turned, turned.buttons.first().ok_or("button")?, (720, 480));
        assert_eq!((extent.x, extent.w), (96, 12));
        if let Some(button) = turned.buttons.first_mut() {
            button.rect = extent;
            button.body = Some(disccraft_project::ContentBox {
                x: i16::try_from(i32::from(content.x) - i32::from(extent.x))?,
                y: i16::try_from(i32::from(content.y) - i32::from(extent.y))?,
                w: content.w,
                h: content.h,
            });
        }
        let highlight = render_menu(&project, &turned)?.highlight;
        let lit: Vec<u16> = (0..highlight.height)
            .flat_map(|y| (0..highlight.width).map(move |x| (x, y)))
            .filter(|(x, y)| {
                highlight
                    .pixels
                    .get(usize::from(*y) * usize::from(highlight.width) + usize::from(*x))
                    .is_some_and(|value| *value == 1)
            })
            .map(|(x, _)| highlight.x + x)
            .collect();
        assert_eq!(lit.len(), 16);
        assert!(lit.iter().all(|x| (96..100).contains(x)), "{lit:?}");
        Ok(())
    }

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

    /// M6.197: a hidden text (the Layers panel's eye) is not drawn on the menu.
    #[test]
    fn a_hidden_text_is_not_drawn() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let _ = project.add_button(menu_id, String::new(), None, Link::NotSet)?;
        let menu = project.menu_mut(menu_id).ok_or("menu")?;
        menu.background = MenuBackground::Color { color: [0, 0, 0] };
        menu.texts.push(disccraft_project::MenuText {
            effects: Default::default(),
            box_width: None,
            box_height: None,
            text: "Shown".to_owned(),
            x: 300,
            y: 300,
            size: 40,
            color: Some([255, 255, 255]),
            font: None,
            font_style: None,
            format: Default::default(),
            shadow: None,
            orientation: Default::default(),
            vertical: false,
            hidden: false,
            locked: false,
            spans: Vec::new(),
        });
        let lit = |project: &Project| -> Result<usize, Box<dyn Error>> {
            let menu = project.menu(menu_id).ok_or("menu")?;
            let picture = render_menu(project, menu)?;
            Ok(picture
                .rgb
                .as_chunks::<3>()
                .0
                .iter()
                .filter(|px| px.iter().all(|v| *v > 128))
                .count())
        };
        assert!(lit(&project)? > 50);
        if let Some(text) = project.menu_mut(menu_id).and_then(|m| m.texts.first_mut()) {
            text.hidden = true;
        }
        assert_eq!(lit(&project)?, 0);
        Ok(())
    }

    /// M6.389: characters with formatting of their own: a larger size makes the line (and the box) taller and the
    /// text wider, a colour draws those characters in it and the others in the text's, and in a paragraph text the
    /// larger characters are measured as they are set, so the text wraps into more lines.
    #[test]
    fn characters_are_drawn_in_formatting_of_their_own() -> Result<(), Box<dyn Error>> {
        use disccraft_project::{MenuBackground, MenuText, Orientation, SpanStyle, TextFormat};
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let mut menu = project.menu(menu_id).ok_or("menu")?.clone();
        menu.background = MenuBackground::Color { color: [0, 0, 0] };
        menu.text_color = [255, 255, 255];
        let plain = MenuText {
            effects: Default::default(),
            box_width: None,
            box_height: None,
            text: "AAAA BBBB".to_owned(),
            x: 40,
            y: 60,
            size: 32,
            color: None,
            font: None,
            font_style: None,
            format: TextFormat::default(),
            shadow: None,
            orientation: Orientation::default(),
            hidden: false,
            locked: false,
            vertical: false,
            spans: Vec::new(),
        };
        let base = super::text_rect(&menu, &plain);
        let mut big = plain.clone();
        big.style_chars(5..9, &|style: &mut SpanStyle| style.size = Some(96));
        let grown = super::text_rect(&menu, &big);
        assert!(grown.h > base.h * 2, "{grown:?} {base:?}");
        assert!(grown.w > base.w, "{grown:?} {base:?}");

        // "BBBB" in red: red pixels right of the middle, white ones left of it, no red on the left.
        let mut red = plain.clone();
        red.style_chars(5..9, &|style: &mut SpanStyle| {
            style.color = Some([255, 0, 0])
        });
        menu.texts = vec![red.clone()];
        let frame = crate::render_menu_preview(&project, &menu)?;
        let width = usize::from(frame.width);
        let middle = usize::from(base.x) + usize::from(base.w) / 2;
        let (mut red_left, mut red_right, mut white_left) = (0, 0, 0);
        for (index, pixel) in frame.rgb.chunks(3).enumerate() {
            let x = index % width;
            let is_red = pixel[0] > 200 && pixel[1] < 60 && pixel[2] < 60;
            let is_white = pixel.iter().all(|value| *value > 200);
            if is_red && x < middle {
                red_left += 1;
            }
            if is_red && x >= middle {
                red_right += 1;
            }
            if is_white && x < middle {
                white_left += 1;
            }
        }
        assert!(
            red_right > 50 && white_left > 50,
            "{red_right} {white_left}"
        );
        assert_eq!(red_left, 0);
        // The layer exported for Photoshop is drawn in the same colours.
        let layer = crate::draw::text_rgba(&menu, &red, frame.width, frame.height);
        assert!(
            layer
                .chunks(4)
                .any(|px| px[0] > 200 && px[1] < 60 && px[3] > 200)
        );

        // A paragraph text as wide as the plain text: the larger characters go to a line of their own.
        let mut boxed = plain.clone();
        boxed.box_width = Some(base.w + 8);
        let one = super::text_rect(&menu, &boxed);
        let mut boxed_big = boxed.clone();
        boxed_big.style_chars(5..9, &|style: &mut SpanStyle| style.size = Some(64));
        let wrapped = super::text_rect(&menu, &boxed_big);
        assert!(wrapped.h > one.h * 2, "{wrapped:?} {one:?}");
        assert_eq!(wrapped.w, one.w);
        Ok(())
    }

    /// M6.390: the same in vertical text: a larger size makes the column longer, and a colour draws those characters
    /// (lower in the column) in it and the others in the text's.
    #[test]
    fn vertical_characters_are_drawn_in_formatting_of_their_own() -> Result<(), Box<dyn Error>> {
        use disccraft_project::{MenuBackground, MenuText, Orientation, SpanStyle, TextFormat};
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let mut menu = project.menu(menu_id).ok_or("menu")?.clone();
        menu.background = MenuBackground::Color { color: [0, 0, 0] };
        menu.text_color = [255, 255, 255];
        let plain = MenuText {
            effects: Default::default(),
            box_width: None,
            box_height: None,
            text: "あいうえおかき".to_owned(),
            x: 200,
            y: 40,
            size: 32,
            color: None,
            font: None,
            font_style: None,
            format: TextFormat::default(),
            shadow: None,
            orientation: Orientation::default(),
            hidden: false,
            locked: false,
            vertical: true,
            spans: Vec::new(),
        };
        let base = super::text_rect(&menu, &plain);
        let mut big = plain.clone();
        big.style_chars(4..7, &|style: &mut SpanStyle| style.size = Some(64));
        let grown = super::text_rect(&menu, &big);
        assert!(grown.h > base.h + 50, "{grown:?} {base:?}");

        let mut red = plain.clone();
        red.style_chars(4..7, &|style: &mut SpanStyle| {
            style.color = Some([255, 0, 0])
        });
        menu.texts = vec![red];
        let frame = crate::render_menu_preview(&project, &menu)?;
        let width = usize::from(frame.width);
        let middle = usize::from(base.y) + usize::from(base.h) / 2;
        let (mut red_top, mut red_bottom, mut white_top) = (0, 0, 0);
        for (index, pixel) in frame.rgb.chunks(3).enumerate() {
            let y = index / width;
            let is_red = pixel[0] > 200 && pixel[1] < 60 && pixel[2] < 60;
            let is_white = pixel.iter().all(|value| *value > 200);
            if is_red && y < middle {
                red_top += 1;
            }
            if is_red && y >= middle {
                red_bottom += 1;
            }
            if is_white && y < middle {
                white_top += 1;
            }
        }
        assert!(
            red_bottom > 50 && white_top > 50,
            "{red_bottom} {white_top}"
        );
        assert_eq!(red_top, 0);
        Ok(())
    }

    /// M6.298: a paragraph text's box height: the box is that tall whatever is in it, and the lines past it are
    /// left out (overset); a text whose lines fit is not overset, and point text has no height.
    #[test]
    fn a_paragraph_text_is_cut_at_its_height() -> Result<(), Box<dyn Error>> {
        use disccraft_project::{MenuText, Orientation, TextFormat};
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let menu = project.menu(menu_id).ok_or("menu")?.clone();
        let point = MenuText {
            effects: Default::default(),
            box_width: None,
            box_height: None,
            text: "あいうえおかきくけこさしすせそ".to_owned(),
            x: 40,
            y: 60,
            size: 40,
            color: None,
            font: None,
            font_style: None,
            format: TextFormat::default(),
            shadow: None,
            orientation: Orientation::default(),
            hidden: false,
            locked: false,
            vertical: false,
            spans: Vec::new(),
        };
        let one_line = super::text_rect(&menu, &point);
        // A third as wide: three lines.
        let width = one_line.w / 3 + 4;
        let three = MenuText {
            box_width: Some(width),
            ..point.clone()
        };
        let all = super::text_rect(&menu, &three);
        assert!(!super::text_overset(&menu, &three));
        // As tall as one and a half lines: one line is drawn, the box keeps its height.
        let tall = one_line.h * 3 / 2;
        let cut = MenuText {
            box_height: Some(tall),
            ..three.clone()
        };
        let rect = super::text_rect(&menu, &cut);
        assert!(super::text_overset(&menu, &cut));
        assert!(rect.h.abs_diff(tall) <= 1, "{rect:?} {tall}");
        assert!(rect.h < all.h, "{rect:?} {all:?}");
        // The pixels a text changes in the menu's frame.
        let blank = {
            let mut menu = menu.clone();
            menu.texts.clear();
            crate::render_menu_preview(&project, &menu)?.rgb
        };
        let drawn = |text: &MenuText| -> Result<usize, Box<dyn Error>> {
            let mut menu = menu.clone();
            menu.texts = vec![text.clone()];
            let frame = crate::render_menu_preview(&project, &menu)?;
            Ok(frame
                .rgb
                .chunks(3)
                .zip(blank.chunks(3))
                .filter(|(pixel, under)| pixel != under)
                .count())
        };
        let lit_all = drawn(&three)?;
        let lit_cut = drawn(&cut)?;
        assert!(lit_cut > 0 && lit_cut * 2 < lit_all, "{lit_cut} {lit_all}");
        // Taller than the lines: nothing left out, the box that tall.
        let roomy = MenuText {
            box_height: Some(all.h + 50),
            ..three.clone()
        };
        assert!(!super::text_overset(&menu, &roomy));
        assert!(super::text_rect(&menu, &roomy).h.abs_diff(all.h + 50) <= 1);
        // Point text ignores a height.
        let free = MenuText {
            box_height: Some(10),
            ..point.clone()
        };
        assert!(!super::text_overset(&menu, &free));
        assert_eq!(super::text_rect(&menu, &free), one_line);
        Ok(())
    }

    /// M6.266: a paragraph text wraps at its box's width: the box is that wide whatever is in it, the text is
    /// as many lines tall as it takes, kinsoku keeps a full stop off the head of a line, and a vertical one wraps
    /// its columns at that height.
    #[test]
    fn a_paragraph_text_wraps_at_its_width() -> Result<(), Box<dyn Error>> {
        use disccraft_project::{MenuText, Orientation, TextFormat, TextKinsoku};
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let menu = project.menu(menu_id).ok_or("menu")?.clone();
        let point = MenuText {
            effects: Default::default(),
            box_width: None,
            box_height: None,
            text: "あいうえおかきくけこ".to_owned(),
            x: 40,
            y: 60,
            size: 40,
            color: None,
            font: None,
            font_style: None,
            format: TextFormat::default(),
            shadow: None,
            orientation: Orientation::default(),
            hidden: false,
            locked: false,
            vertical: false,
            spans: Vec::new(),
        };
        let one_line = super::text_rect(&menu, &point);
        // Half as wide: two lines, in a box of exactly that width.
        let half = one_line.w / 2 + 4;
        let boxed = MenuText {
            box_width: Some(half),
            box_height: None,
            ..point.clone()
        };
        let two_lines = super::text_rect(&menu, &boxed);
        assert_eq!((two_lines.x, two_lines.y), (40, 60));
        assert!(
            two_lines.w.abs_diff(half) <= 1,
            "{one_line:?} {two_lines:?}"
        );
        assert!(
            two_lines.h > one_line.h * 3 / 2 && two_lines.h < one_line.h * 3,
            "{one_line:?} {two_lines:?}"
        );
        // A box wider than the text changes nothing but the box.
        let roomy = MenuText {
            box_width: Some(one_line.w + 200),
            box_height: None,
            ..point.clone()
        };
        let still_one = super::text_rect(&menu, &roomy);
        assert_eq!(still_one.h, one_line.h);
        assert!(still_one.w.abs_diff(one_line.w + 200) <= 1, "{still_one:?}");
        // Kinsoku: with a full stop as the sixth character and room for five, the strong set takes one more
        // character down (three lines of 4 + 4 + 3 instead of 5 + 5 + 1: the same height here, so the lines
        // are compared through the wrapping itself).
        let stops = "あいうえお。かきくけこ";
        let measure = |line: &str| {
            let rect = super::text_rect(
                &menu,
                &MenuText {
                    text: line.to_owned(),
                    ..point.clone()
                },
            );
            f32::from(rect.w)
        };
        let five = measure("あいうえお") + 2.0;
        assert_eq!(
            crate::wrap::wrap(stops, five, TextKinsoku::Strong, &measure),
            "あいうえ\nお。かきく\nけこ"
        );
        assert_eq!(
            crate::wrap::wrap(stops, five, TextKinsoku::None, &measure),
            "あいうえお\n。かきくけ\nこ"
        );
        // Vertical: the columns wrap at that height, so there are more of them.
        let standing = MenuText {
            vertical: true,
            ..point.clone()
        };
        let column = super::text_rect(&menu, &standing);
        let wrapped = super::text_rect(
            &menu,
            &MenuText {
                box_width: Some(column.h / 2 + 4),
                box_height: None,
                ..standing.clone()
            },
        );
        assert!(
            wrapped.w > column.w * 3 / 2 && wrapped.h < column.h * 3 / 4,
            "{column:?} {wrapped:?}"
        );
        // M6.317: the box keeps its size whatever the columns in it: as long as drawn, and as wide as its reach
        // (the columns from its right edge).
        let roomy = MenuText {
            box_width: Some(column.h + 100),
            box_height: Some(column.w * 4),
            ..standing.clone()
        };
        let rect = super::text_rect(&menu, &roomy);
        assert!(
            rect.h.abs_diff(column.h + 100) <= 1 && rect.w.abs_diff(column.w * 4) <= 1,
            "{column:?} {rect:?}"
        );
        assert!(!super::text_overset(&menu, &roomy));
        // Narrower than its columns: the columns past it are left out and the box is as wide as it.
        let narrow = MenuText {
            box_height: Some(wrapped.w * 2 / 3),
            ..MenuText {
                box_width: Some(column.h / 2 + 4),
                box_height: None,
                ..standing.clone()
            }
        };
        assert!(super::text_overset(&menu, &narrow));
        assert!(super::text_rect(&menu, &narrow).w <= wrapped.w * 2 / 3 + 1);
        Ok(())
    }

    /// M6.192: a vertical text stands in a column (taller than wide, as wide as the horizontal text is tall), each
    /// line is a column to the left of the one before, its box starts at its x, y, and the drawn pixels lie in it.
    #[test]
    fn a_vertical_text_stands_in_columns() -> Result<(), Box<dyn Error>> {
        use disccraft_project::{MenuText, Orientation};
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        project.menu_mut(menu_id).ok_or("menu")?.background =
            MenuBackground::Color { color: [0, 0, 0] };
        let text = MenuText {
            effects: Default::default(),
            box_width: None,
            box_height: None,
            text: "縦書きDVD".to_owned(),
            x: 300,
            y: 60,
            size: 40,
            color: Some([255, 255, 255]),
            font: None,
            font_style: None,
            format: Default::default(),
            shadow: None,
            orientation: Orientation::default(),
            hidden: false,
            locked: false,
            vertical: true,
            spans: Vec::new(),
        };
        let horizontal = MenuText {
            box_width: None,
            box_height: None,
            vertical: false,
            ..text.clone()
        };
        let menu = project.menu(menu_id).ok_or("menu")?.clone();
        let flat = super::text_rect(&menu, &horizontal);
        let standing = super::text_rect(&menu, &text);
        assert_eq!((standing.x, standing.y), (300, 60));
        assert!(standing.h > standing.w * 3, "{standing:?}");
        assert!(
            standing.w < flat.h * 2 && standing.h > flat.w / 2,
            "{flat:?} {standing:?}"
        );
        // Two lines: two columns, about twice as wide.
        let two = MenuText {
            box_width: None,
            box_height: None,
            text: "縦書き
二行目"
                .to_owned(),
            ..text.clone()
        };
        let wide = super::text_rect(&menu, &two);
        assert!(wide.w > standing.w * 3 / 2, "{standing:?} {wide:?}");
        // The pixels lie in the box: render it and find the lit columns and rows.
        let mut with_text = menu.clone();
        with_text.texts.push(text.clone());
        let picture = render_menu(&project, &with_text)
            .or_else(|_| super::render_menu_preview(&project, &with_text))?;
        let width = usize::from(picture.width);
        let lit: Vec<(usize, usize)> = picture
            .rgb
            .as_chunks::<3>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, px)| px.iter().all(|v| *v > 128))
            .map(|(i, _)| (i % width, i / width))
            .collect();
        assert!(!lit.is_empty());
        let (min_x, max_x) = (
            lit.iter().map(|p| p.0).min().unwrap_or(0),
            lit.iter().map(|p| p.0).max().unwrap_or(0),
        );
        let (min_y, max_y) = (
            lit.iter().map(|p| p.1).min().unwrap_or(0),
            lit.iter().map(|p| p.1).max().unwrap_or(0),
        );
        assert!(
            min_x >= 299 && max_x <= usize::from(standing.x + standing.w) + 1,
            "{min_x}..{max_x} {standing:?}"
        );
        assert!(
            min_y >= 59 && max_y <= usize::from(standing.y + standing.h) + 1,
            "{min_y}..{max_y} {standing:?}"
        );
        assert!(max_y - min_y > (max_x - min_x) * 3, "a tall column");
        Ok(())
    }

    /// M6.191: a graphic and a text turned by a free angle are drawn turned about their box's centre: a square
    /// turned 45° covers its diagonal tips but no longer its corners, and a turned text covers taller bounds.
    #[test]
    fn a_free_turn_turns_graphics_and_texts_about_their_centre() -> Result<(), Box<dyn Error>> {
        use disccraft_project::{GraphicKind, MenuGraphic, MenuText, Orientation, ShapeKind};
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        project.menu_mut(menu_id).ok_or("menu")?.background =
            MenuBackground::Color { color: [0, 0, 0] };
        let square = Rect {
            x: 200,
            y: 150,
            w: 100,
            h: 100,
        };
        let _ = project.add_menu_graphic(
            menu_id,
            MenuGraphic {
                rect: square,
                kind: GraphicKind::Shape {
                    shape: ShapeKind::Rectangle,
                    color: [255, 255, 255],
                },
                opacity: 100,
                shadow: None,
                orientation: Orientation::default().with_angle(450),
                hidden: false,
                locked: false,
                replacement: false,
                effects: Default::default(),
            },
        )?;
        let menu = project.menu(menu_id).ok_or("menu")?.clone();
        let picture = super::render_menu_preview(&project, &menu)?;
        let lit = |x: usize, y: usize| {
            picture
                .rgb
                .get((y * usize::from(picture.width) + x) * 3)
                .is_some_and(|value| *value > 128)
        };
        // Centre (250, 200): the corner of the upright square is dark, the diagonal tip (half-diagonal ≈ 70.7 px
        // straight up) lit.
        assert!(lit(250, 200));
        assert!(!lit(203, 153), "the corner is turned away");
        assert!(lit(250, 134), "the tip points up");
        assert!(!lit(250, 125));
        let bounds = super::graphic_bounds(menu.graphics.first().ok_or("graphic")?);
        assert!((140..=143).contains(&bounds.w), "{bounds:?}");

        let text = MenuText {
            effects: Default::default(),
            box_width: None,
            box_height: None,
            text: "Rotate".to_owned(),
            x: 100,
            y: 300,
            size: 40,
            color: None,
            font: None,
            font_style: None,
            format: Default::default(),
            shadow: None,
            orientation: Orientation::default(),
            hidden: false,
            locked: false,
            vertical: false,
            spans: Vec::new(),
        };
        let flat = super::text_bounds(&menu, &text);
        let turned = MenuText {
            box_width: None,
            box_height: None,
            orientation: Orientation::default().with_angle(900),
            ..text.clone()
        };
        let standing = super::text_bounds(&menu, &turned);
        // The box stays; its bounds stand up about the same centre.
        assert_eq!(
            super::text_rect(&menu, &turned),
            super::text_rect(&menu, &text)
        );
        assert!(
            standing.h > flat.h && standing.w < flat.w,
            "{flat:?} -> {standing:?}"
        );
        let centre = |r: Rect| {
            (
                i32::from(r.x) * 2 + i32::from(r.w),
                i32::from(r.y) * 2 + i32::from(r.h),
            )
        };
        let (a, b) = (centre(flat), centre(standing));
        assert!(
            (a.0 - b.0).abs() <= 2 && (a.1 - b.1).abs() <= 2,
            "{a:?} {b:?}"
        );
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
                hidden: false,
                locked: false,
                replacement: false,
                effects: Default::default(),
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
                hidden: false,
                locked: false,
                replacement: false,
                effects: Default::default(),
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

    /// M6.190: a button's text is drawn moved by its offset (the Direct Select tool), its bounds follow, and an
    /// offset is limited so the text stays inside the button.
    #[test]
    fn a_buttons_text_moves_by_its_offset_inside_the_button() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let rect = Rect {
            x: 100,
            y: 100,
            w: 300,
            h: 60,
        };
        let _ = project.add_button(menu_id, "Go".to_owned(), Some(rect), Link::NotSet)?;
        if let Some(menu) = project.menu_mut(menu_id) {
            menu.background = MenuBackground::Color { color: [0, 0, 0] };
            menu.text_color = [255, 255, 255];
        }
        let menu = project.menu(menu_id).ok_or("menu")?.clone();
        let button = menu.buttons.first().ok_or("button")?.clone();
        let centred = super::label_bounds(&menu, &button).ok_or("bounds")?;
        // Centred in the button.
        let middle = |r: Rect| (i32::from(r.x) * 2 + i32::from(r.w)) / 2;
        assert!((middle(centred) - 250).abs() <= 1, "{centred:?}");
        // Moved 80 px to the right: the bounds follow, and the drawn text with them.
        let offset = [80, 0];
        let moved = disccraft_project::Button {
            label_offset: offset,
            ..button.clone()
        };
        let bounds = super::label_bounds(&menu, &moved).ok_or("bounds")?;
        assert_eq!(i32::from(bounds.x), i32::from(centred.x) + 80);
        let lit_columns =
            |menu: &disccraft_project::Menu| -> Result<(usize, usize), Box<dyn Error>> {
                let mut project = project.clone();
                if let Some(slot) = project.menu_mut(menu_id) {
                    *slot = menu.clone();
                }
                let picture = render_menu(&project, menu)?;
                let width = usize::from(picture.width);
                let lit: Vec<usize> = picture
                    .rgb
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .enumerate()
                    .filter(|(_, px)| px.iter().any(|v| *v > 128))
                    .map(|(i, _)| i % width)
                    .collect();
                Ok((
                    lit.iter().copied().min().unwrap_or(0),
                    lit.iter().copied().max().unwrap_or(0),
                ))
            };
        let mut moved_menu = menu.clone();
        if let Some(slot) = moved_menu.buttons.first_mut() {
            slot.label_offset = offset;
        }
        let (left, _) = lit_columns(&menu)?;
        let (moved_left, _) = lit_columns(&moved_menu)?;
        assert!(
            (moved_left as i64 - left as i64 - 80).abs() <= 1,
            "{left} -> {moved_left}"
        );
        // M6.193: moved out of the button (Encore), the button's extent grows to take the text in; inside, the
        // extent is the button itself.
        assert_eq!(super::button_extent(&menu, &button, (720, 480)), rect);
        let out = disccraft_project::Button {
            label_offset: [250, 0],
            ..button.clone()
        };
        let text = super::label_bounds(&menu, &out).ok_or("bounds")?;
        let extent = super::button_extent(&menu, &out, (720, 480));
        assert_eq!((extent.x, extent.y), (rect.x, rect.y));
        assert_eq!(extent.x + extent.w, text.x + text.w, "{extent:?} {text:?}");
        assert!(text.x + text.w > rect.x + rect.w);
        Ok(())
    }

    /// M6.194: a button's content turned by a free angle turns about its content box's centre: its text is drawn
    /// turned (its bounds stand up at a quarter turn) and the extent is the turned box's bounds.
    #[test]
    fn a_buttons_content_turns_about_its_box() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let rect = Rect {
            x: 200,
            y: 200,
            w: 200,
            h: 60,
        };
        let _ = project.add_button(menu_id, "Play".to_owned(), Some(rect), Link::NotSet)?;
        let menu = project.menu(menu_id).ok_or("menu")?.clone();
        let button = menu.buttons.first().ok_or("button")?.clone();
        let flat = super::label_bounds(&menu, &button).ok_or("bounds")?;
        let turned = disccraft_project::Button {
            orientation: button.orientation.with_angle(900),
            ..button.clone()
        };
        let standing = super::label_bounds(&menu, &turned).ok_or("bounds")?;
        assert!(
            standing.h > standing.w && flat.w > flat.h,
            "{flat:?} {standing:?}"
        );
        // On its side about (300, 230): 60 wide, 200 high.
        let extent = super::button_extent(&menu, &turned, (720, 480));
        assert_eq!(
            extent,
            Rect {
                x: 270,
                y: 130,
                w: 60,
                h: 200
            }
        );
        // The disc's highlight (a text subpicture) is the turned text: standing up inside the button's area.
        let mut on_disc = menu.clone();
        if let Some(slot) = on_disc.buttons.first_mut() {
            *slot = disccraft_project::Button {
                rect: extent,
                body: Some(disccraft_project::ContentBox {
                    x: -70,
                    y: 70,
                    w: 200,
                    h: 60,
                }),
                text_subpicture: true,
                ..turned.clone()
            };
        }
        let rendered = render_menu(&project, &on_disc)?;
        let layer = &rendered.highlight;
        let width = usize::from(layer.width);
        let lit: Vec<(usize, usize)> = layer
            .pixels
            .iter()
            .enumerate()
            .filter(|(_, value)| **value != 0)
            .map(|(i, _)| {
                (
                    usize::from(layer.x) + i % width,
                    usize::from(layer.y) + i / width,
                )
            })
            .collect();
        assert!(!lit.is_empty());
        let span = |pick: fn(&(usize, usize)) -> usize| {
            let values: Vec<usize> = lit.iter().map(pick).collect();
            (
                values.iter().copied().min().unwrap_or(0),
                values.iter().copied().max().unwrap_or(0),
            )
        };
        let ((x0, x1), (y0, y1)) = (span(|p| p.0), span(|p| p.1));
        assert!(y1 - y0 > x1 - x0, "standing: {x0}..{x1} x {y0}..{y1}");
        assert!(
            x0 >= 270 && x1 < 330 && y0 >= 130 && y1 < 330,
            "{x0}..{x1} x {y0}..{y1}"
        );
        Ok(())
    }

    #[test]
    fn a_thumbnail_picture_fills_the_area_and_the_text_goes_below() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        let rect = Rect {
            x: 100,
            y: 100,
            w: 200,
            h: 160,
        };
        let _ = project.add_button(
            menu_id,
            "Scene".to_owned(),
            Some(rect),
            disccraft_project::Link::NotSet,
        )?;
        if let Some(menu) = project.menu_mut(menu_id) {
            menu.background = disccraft_project::MenuBackground::Color { color: [0, 0, 0] };
            menu.text_color = [255, 255, 255];
            if let Some(button) = menu.buttons.first_mut() {
                button.thumbnail = Some(disccraft_project::ThumbnailArea {
                    x: 0,
                    y: 0,
                    w: 10_000,
                    h: 7_500,
                });
            }
        }
        let menu = project.menu(menu_id).ok_or("menu")?.clone();
        let button = menu.buttons.first().ok_or("button")?;
        assert_eq!(
            button.thumbnail_rect(),
            Some(Rect {
                x: 100,
                y: 100,
                w: 200,
                h: 120
            })
        );
        assert_eq!(
            button.label_rect(),
            Rect {
                x: 100,
                y: 220,
                w: 200,
                h: 40
            }
        );

        // A 4 x 2 red picture covers the whole area (stretched, no bars).
        let red = [200u8, 0, 0].repeat(8);
        let pictures = [super::ThumbnailPicture {
            button: 0,
            rgb: &red,
            width: 4,
            height: 2,
        }];
        let rendered = super::render_disc_menu(
            &project,
            &menu,
            super::MenuPictures {
                background: None,
                thumbnails: &pictures,
            },
        )?;
        let at = |x: usize, y: usize| {
            let i = (y * usize::from(rendered.width) + x) * 3;
            rendered.rgb.get(i..i + 3).map(<[u8]>::to_vec)
        };
        for (x, y) in [(101, 101), (298, 101), (101, 218), (298, 218), (200, 160)] {
            assert_eq!(at(x, y), Some(vec![200, 0, 0]), "({x}, {y})");
        }
        // Outside the area the background stays, and the text is drawn in the strip below.
        assert_eq!(at(99, 99), Some(vec![0, 0, 0]));
        let strip_has_text =
            (100..300).any(|x| (222..258).any(|y| at(x, y) != Some(vec![0, 0, 0])));
        assert!(strip_has_text);
        // Without a picture the area keeps what is under it.
        let plain = super::render_menu(&project, &menu)?;
        let i = (160 * usize::from(plain.width) + 200) * 3;
        assert_eq!(plain.rgb.get(i..i + 3), Some(&[0u8, 0, 0][..]));

        // Written to a PSD, the area comes back as a (%) layer.
        let dir =
            std::env::temp_dir().join(format!("disccraft-psd-thumb-menu-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("menu.psd");
        super::export_menu_psd(&project, &menu, None, &path)?;
        let read = disccraft_photo::load_psd_menu(&path)?;
        let thumbnail = read.buttons.first().and_then(|button| button.thumbnail);
        assert_eq!(thumbnail, Some([100, 100, 200, 120]));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn a_saved_psd_reads_back_with_the_same_button_numbers() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        for (index, label) in ["One", "Two", "Three"].into_iter().enumerate() {
            let y = 100 + u16::try_from(index)? * 80;
            let _ = project.add_button(
                menu_id,
                label.to_owned(),
                Some(Rect {
                    x: 100,
                    y,
                    w: 200,
                    h: 50,
                }),
                disccraft_project::Link::NotSet,
            )?;
        }
        let menu = project.menu(menu_id).ok_or("menu")?.clone();
        let dir = std::env::temp_dir().join(format!("disccraft-psd-order-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("menu.psd");
        super::export_menu_psd(&project, &menu, None, &path)?;
        let read = disccraft_photo::load_psd_menu(&path)?;
        let names: Vec<&str> = read
            .buttons
            .iter()
            .map(|button| button.name.as_str())
            .collect();
        assert_eq!(names, ["One", "Two", "Three"]);
        let tops: Vec<u32> = read.buttons.iter().map(|button| button.y).collect();
        assert!(tops.windows(2).all(|pair| pair[0] < pair[1]), "{tops:?}");
        let _ = std::fs::remove_dir_all(&dir);
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
                hidden: false,
                locked: false,
                replacement: false,
                effects: Default::default(),
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
                hidden: false,
                locked: false,
                replacement: false,
                effects: Default::default(),
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
                angle: 0,
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
                angle: 0,
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
            effects: Default::default(),
            box_width: None,
            box_height: None,
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
            hidden: false,
            locked: false,
            vertical: false,
            spans: Vec::new(),
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
    fn a_button_text_takes_its_own_formatting() -> Result<(), Box<dyn Error>> {
        if filmcraft_text::families().is_empty() {
            return Ok(());
        }
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        project.add_button(
            menu_id,
            "Play".to_string(),
            Some(Rect {
                x: 100,
                y: 100,
                w: 400,
                h: 80,
            }),
            Link::Stop,
        )?;
        let mut menu = project.menu(menu_id).ok_or("menu")?.clone();
        menu.background = MenuBackground::Color { color: [0, 0, 0] };
        let ink = |menu: &disccraft_project::Menu| -> Result<(f64, usize), Box<dyn Error>> {
            let rendered = super::render_menu_preview(&project, menu)?;
            let light = rendered.rgb.iter().map(|v| f64::from(*v)).sum();
            let partial = rendered
                .rgb
                .chunks(3)
                .filter(|p| p[0] > 10 && p[0] < 245)
                .count();
            Ok((light, partial))
        };
        let (plain, _) = ink(&menu)?;
        if let Some(button) = menu.buttons.first_mut() {
            button.label_format.bold = true;
            button.label_format.horizontal_scale = 150;
        }
        let (heavy, _) = ink(&menu)?;
        assert!(heavy > plain * 1.2, "{plain} {heavy}");
        if let Some(button) = menu.buttons.first_mut() {
            button.label_format.antialias = disccraft_project::TextAntialias::None;
        }
        let (_, partial) = ink(&menu)?;
        assert_eq!(partial, 0, "no anti-aliasing on the button text either");
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
            effects: Default::default(),
            box_width: None,
            box_height: None,
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
            hidden: false,
            locked: false,
            vertical: false,
            spans: Vec::new(),
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
            effects: Default::default(),
            box_width: None,
            box_height: None,
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
            hidden: false,
            locked: false,
            vertical: false,
            spans: Vec::new(),
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
            effects: Default::default(),
            box_width: None,
            box_height: None,
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
            hidden: false,
            locked: false,
            vertical: false,
            spans: Vec::new(),
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
    fn a_text_subpicture_highlights_the_text_not_a_frame() -> Result<(), Box<dyn Error>> {
        if filmcraft_text::families().is_empty() {
            return Ok(());
        }
        let mut project = Project::default();
        let menu_id = project.add_menu(None)?;
        project.add_button(
            menu_id,
            "PLAY".to_string(),
            Some(Rect {
                x: 100,
                y: 100,
                w: 300,
                h: 80,
            }),
            Link::Stop,
        )?;
        let mut menu = project.menu(menu_id).ok_or("menu")?.clone();
        assert!(
            menu.buttons
                .first()
                .is_some_and(|button| button.text_subpicture)
        );
        let lit = |menu: &disccraft_project::Menu| -> Result<(bool, usize), Box<dyn Error>> {
            let layer = super::render_menu(&project, menu)?.highlight;
            let at = |x: u16, y: u16| {
                let i =
                    usize::from(y - layer.y) * usize::from(layer.width) + usize::from(x - layer.x);
                layer.pixels.get(i).copied().unwrap_or(0)
            };
            // The top-left corner of the button: on the frame, away from the text.
            let corner = at(101, 101) == 1;
            let count = layer.pixels.iter().filter(|pixel| **pixel == 1).count();
            Ok((corner, count))
        };
        let (corner, text_pixels) = lit(&menu)?;
        assert!(!corner && text_pixels > 50, "{corner} {text_pixels}");
        if let Some(button) = menu.buttons.first_mut() {
            button.text_subpicture = false;
        }
        let (corner, _) = lit(&menu)?;
        assert!(
            corner,
            "without a text subpicture the frame is the highlight"
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
            effects: Default::default(),
            box_width: None,
            box_height: None,
            format: disccraft_project::TextFormat::default(),
            shadow: None,
            orientation: Default::default(),
            hidden: false,
            locked: false,
            vertical: false,
            text: "Welcome".to_owned(),
            x: 60,
            y: 40,
            size: 48,
            color: Some([255, 255, 0]),
            font: None,
            font_style: None,
            spans: Vec::new(),
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
        // This test is about the frame highlight (buttons without a text subpicture).
        for button in &mut project.menu_mut(menu_id).ok_or("missing menu")?.buttons {
            button.text_subpicture = false;
        }

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
