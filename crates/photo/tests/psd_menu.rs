//! A menu PSD written with the Craft PSD writer (craft-psd, which PhotoCraft uses) is read back with Encore-compatible layer naming: `(+)` groups become
//! buttons, `(=1)` layers their highlights, and the background leaves the highlights out.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use craft_psd::{GroupSpec, LayerSpec, PixelData, PsdBuilder};
use disccraft_photo::{Image8, load_image, load_psd_menu};

const W: u32 = 64;
const H: u32 = 48;

fn solid(w: u32, h: u32, rgba: [u8; 4]) -> PixelData {
    PixelData::Rgba8(rgba.repeat(usize::try_from(w * h).unwrap()))
}

fn pixel(image: &Image8, x: u32, y: u32) -> [u8; 4] {
    let i = usize::try_from((y * image.width + x) * 4).unwrap();
    [
        image.rgba[i],
        image.rgba[i + 1],
        image.rgba[i + 2],
        image.rgba[i + 3],
    ]
}

fn write_psd(tag: &str) -> std::path::PathBuf {
    let mut b = PsdBuilder::new(W, H);
    b.push_layer(LayerSpec::new(
        "Background",
        0,
        0,
        W,
        H,
        solid(W, H, [0, 0, 80, 255]),
    ));
    b.begin_group(GroupSpec::new("(+) Play"));
    b.push_layer(LayerSpec::new(
        "label",
        8,
        8,
        20,
        10,
        solid(20, 10, [255, 0, 0, 255]),
    ));
    b.push_layer(LayerSpec::new(
        "(=1)",
        6,
        6,
        24,
        14,
        solid(24, 14, [255, 255, 0, 255]),
    ));
    b.end_group().unwrap();
    b.begin_group(GroupSpec::new("(+)Back"));
    b.push_layer(LayerSpec::new(
        "label",
        36,
        30,
        16,
        8,
        solid(16, 8, [0, 255, 0, 255]),
    ));
    b.end_group().unwrap();
    b.composite(solid(W, H, [0, 0, 80, 255]));
    let path =
        std::env::temp_dir().join(format!("disccraft-photo-{tag}-{}.psd", std::process::id()));
    std::fs::write(&path, b.to_bytes().unwrap()).unwrap();
    path
}

#[test]
fn reads_buttons_highlights_and_background() {
    let path = write_psd("menu");
    let menu = load_psd_menu(&path).unwrap();
    let image = load_image(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert_eq!((menu.width, menu.height), (W, H));
    // Encore numbers the buttons from the bottom of the layer stack: Play was added first (lowest), Back last.
    let names: Vec<&str> = menu.buttons.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["Play", "Back"]);

    // The button covers its whole layer set (label and highlight), as in Encore.
    let play = &menu.buttons[0];
    assert_eq!((play.x, play.y, play.width, play.height), (6, 6, 24, 14));
    let highlight = play.highlight.as_ref().expect("Play has a highlight");
    assert_eq!((highlight.width, highlight.height), (24, 14));
    assert_eq!(pixel(highlight, 0, 0), [255, 255, 0, 255]);

    let back = &menu.buttons[1];
    assert_eq!((back.x, back.y, back.width, back.height), (36, 30, 16, 8));
    assert!(back.highlight.is_none());

    // The menu background hides the highlight layer; the plain image shows every visible layer.
    assert_eq!(pixel(&menu.background, 10, 10), [255, 0, 0, 255]);
    assert_eq!(pixel(&menu.background, 7, 7), [0, 0, 80, 255]);
    assert_eq!(pixel(&image, 10, 10), [255, 255, 0, 255]);
    assert_eq!(pixel(&image, 40, 32), [0, 255, 0, 255]);
}

/// M6.335: a button's subpicture layers are kept in their own colours, half-clear pixels as they are, one layer
/// per number (for a Blu-ray menu's rich-colour highlights).
#[test]
fn subpicture_layers_keep_their_own_colours() {
    let mut b = PsdBuilder::new(W, H);
    b.push_layer(LayerSpec::new(
        "Background",
        0,
        0,
        W,
        H,
        solid(W, H, [0, 0, 80, 255]),
    ));
    b.begin_group(GroupSpec::new("(+) Play"));
    // (=1): a left half of soft orange and a right half of red; (=3): a blue square on top.
    let mut first = Vec::new();
    for _ in 0..10 {
        for x in 0..20 {
            first.extend_from_slice(if x < 10 {
                &[250, 140, 0, 128]
            } else {
                &[255, 0, 0, 255]
            });
        }
    }
    b.push_layer(LayerSpec::new(
        "(=1) frame",
        10,
        10,
        20,
        10,
        PixelData::Rgba8(first),
    ));
    b.push_layer(LayerSpec::new(
        "(=3) dot",
        14,
        12,
        4,
        4,
        solid(4, 4, [0, 0, 255, 255]),
    ));
    b.end_group().unwrap();
    b.composite(solid(W, H, [0, 0, 80, 255]));
    let path = std::env::temp_dir().join(format!("disccraft-photo-own-{}.psd", std::process::id()));
    std::fs::write(&path, b.to_bytes().unwrap()).unwrap();
    let menu = load_psd_menu(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let play = &menu.buttons[0];
    assert_eq!((play.x, play.y, play.width, play.height), (10, 10, 20, 10));
    let sub = play.subpicture.as_ref().expect("subpicture");
    let at = |rgba: &[u8], x: usize, y: usize| -> [u8; 4] {
        let i = (y * 20 + x) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    };
    let first = sub.layers[0].as_deref().expect("(=1) layer");
    assert_eq!(first.len(), 20 * 10 * 4);
    let soft = at(first, 2, 2);
    assert!(
        soft[0].abs_diff(250) <= 2 && soft[1].abs_diff(140) <= 2 && soft[3].abs_diff(128) <= 1,
        "straight colour with its alpha: {soft:?}"
    );
    assert_eq!(at(first, 15, 0), [255, 0, 0, 255]);
    assert_eq!(sub.colors[2], Some([0, 0, 255]));
    // The (=3) layer alone, not laid over the (=1) layer.
    assert!(sub.layers[1].is_none());
    let third = sub.layers[2].as_deref().expect("(=3) layer");
    assert_eq!(at(third, 5, 3), [0, 0, 255, 255]);
    assert_eq!(at(third, 15, 0)[3], 0);
}

/// M6.336: a subpicture layer's colour (for Encore's automatic colour set) is its last opaque pixel, as Encore takes
/// it: green then red gives red.
#[test]
fn a_layers_colour_is_its_last_opaque_pixel() {
    let mut b = PsdBuilder::new(W, H);
    b.push_layer(LayerSpec::new(
        "Background",
        0,
        0,
        W,
        H,
        solid(W, H, [0, 0, 80, 255]),
    ));
    b.begin_group(GroupSpec::new("(+) Play"));
    let mut two = Vec::new();
    for _ in 0..4 {
        for x in 0..8 {
            two.extend_from_slice(if x < 4 {
                &[0, 200, 0, 255]
            } else {
                &[230, 20, 20, 255]
            });
        }
    }
    b.push_layer(LayerSpec::new("(=1)", 4, 4, 8, 4, PixelData::Rgba8(two)));
    b.end_group().unwrap();
    b.composite(solid(W, H, [0, 0, 80, 255]));
    let path =
        std::env::temp_dir().join(format!("disccraft-photo-last-{}.psd", std::process::id()));
    std::fs::write(&path, b.to_bytes().unwrap()).unwrap();
    let menu = load_psd_menu(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let sub = menu.buttons[0].subpicture.as_ref().expect("subpicture");
    assert_eq!(sub.colors, [Some([230, 20, 20]), None, None]);
}

/// A PhotoCraft document (`.pcraft`, PhotoCraft's own format) reads like the PSD it was made from: the same
/// buttons, highlight and background. The document is written by PhotoCraft's own writer.
#[test]
fn a_photocraft_document_reads_like_its_psd() {
    let psd = write_psd("pcraft");
    let from_psd = load_psd_menu(&psd).unwrap();
    let bytes = std::fs::read(&psd).unwrap();
    let _ = std::fs::remove_file(&psd);

    let document = photocraft_io::import("menu.psd", &bytes).unwrap().document;
    let saved = photocraft_io::export(
        &document,
        "menu.pcraft",
        &photocraft_io::ExportOptions::default(),
    )
    .unwrap();
    let path = std::env::temp_dir().join(format!("disccraft-photo-{}.pcraft", std::process::id()));
    std::fs::write(&path, saved.bytes).unwrap();
    assert!(disccraft_photo::is_pcraft(&path));
    assert!(disccraft_photo::is_document(&path));
    let menu = load_psd_menu(&path).unwrap();
    let image = load_image(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert_eq!((menu.width, menu.height), (W, H));
    let names: Vec<&str> = menu.buttons.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["Play", "Back"]);
    for (read, expected) in menu.buttons.iter().zip(&from_psd.buttons) {
        assert_eq!(
            (read.x, read.y, read.width, read.height),
            (expected.x, expected.y, expected.width, expected.height)
        );
        assert_eq!(read.highlight.is_some(), expected.highlight.is_some());
    }
    assert_eq!(pixel(&menu.background, 10, 10), [255, 0, 0, 255]);
    assert_eq!(pixel(&menu.background, 7, 7), [0, 0, 80, 255]);
    assert_eq!(pixel(&image, 10, 10), [255, 255, 0, 255]);
}

/// M10.27: a menu PSD with Encore's `(#)` page groups. Its top level's `(+)` groups are the first page's buttons,
/// each `(#)` group's its own page's (the lowest group first, whatever its name). The shared picture leaves the
/// buttons and the pages out; page 0's picture is the top level's buttons, each later page's its group, over
/// transparency, highlights left out.
#[test]
fn page_groups_are_the_menus_pages() {
    let mut b = PsdBuilder::new(W, H);
    b.push_layer(LayerSpec::new(
        "背景",
        0,
        0,
        W,
        H,
        solid(W, H, [0, 0, 80, 255]),
    ));
    b.push_layer(LayerSpec::new(
        "shared art",
        0,
        0,
        8,
        8,
        solid(8, 8, [0, 200, 0, 255]),
    ));
    b.begin_group(GroupSpec::new("(+) Play"));
    b.push_layer(LayerSpec::new(
        "label",
        10,
        10,
        10,
        6,
        solid(10, 6, [255, 0, 0, 255]),
    ));
    b.push_layer(LayerSpec::new(
        "(=1)",
        9,
        9,
        12,
        8,
        solid(12, 8, [255, 255, 0, 255]),
    ));
    b.end_group().unwrap();
    // The lower page group is the second page, though its name sorts after the other's.
    b.begin_group(GroupSpec::new("(#) Z first"));
    b.push_layer(LayerSpec::new(
        "z art",
        40,
        0,
        8,
        8,
        solid(8, 8, [200, 200, 0, 255]),
    ));
    b.begin_group(GroupSpec::new("(+) Back"));
    b.push_layer(LayerSpec::new(
        "label",
        30,
        30,
        10,
        6,
        solid(10, 6, [0, 255, 255, 255]),
    ));
    b.end_group().unwrap();
    b.end_group().unwrap();
    b.begin_group(GroupSpec::new("(#) A second"));
    b.push_layer(LayerSpec::new(
        "a art",
        50,
        40,
        8,
        8,
        solid(8, 8, [255, 0, 255, 255]),
    ));
    b.begin_group(GroupSpec::new("(+) One"));
    b.push_layer(LayerSpec::new(
        "label",
        2,
        30,
        10,
        6,
        solid(10, 6, [255, 255, 255, 255]),
    ));
    b.end_group().unwrap();
    b.begin_group(GroupSpec::new("(+) Two"));
    b.push_layer(LayerSpec::new(
        "label",
        20,
        30,
        10,
        6,
        solid(10, 6, [255, 255, 255, 255]),
    ));
    b.end_group().unwrap();
    b.end_group().unwrap();
    b.composite(solid(W, H, [0, 0, 80, 255]));
    let path =
        std::env::temp_dir().join(format!("disccraft-photo-pages-{}.psd", std::process::id()));
    std::fs::write(&path, b.to_bytes().unwrap()).unwrap();

    let menu = load_psd_menu(&path).unwrap();
    let names = |buttons: &[disccraft_photo::PsdButton]| -> Vec<String> {
        buttons.iter().map(|button| button.name.clone()).collect()
    };
    assert_eq!(names(&menu.buttons), ["Play"]);
    assert_eq!(menu.pages.len(), 2);
    assert_eq!(menu.pages[0].0, "Z first");
    assert_eq!(names(&menu.pages[0].1), ["Back"]);
    assert_eq!(menu.pages[1].0, "A second");
    assert_eq!(names(&menu.pages[1].1), ["One", "Two"]);
    assert_eq!(disccraft_photo::psd_page_count(&path).unwrap(), 3);

    let pages = disccraft_photo::load_menu_pages(&path).unwrap();
    assert_eq!(pages.pages.len(), 3);
    // Shared: the background and its art, no button and no page.
    assert_eq!(pixel(&pages.shared, 2, 2), [0, 200, 0, 255]);
    assert_eq!(pixel(&pages.shared, 12, 12), [0, 0, 80, 255]);
    assert_eq!(pixel(&pages.shared, 42, 2), [0, 0, 80, 255]);
    // Page 0: the Play button's label, its highlight left out, nothing else.
    assert_eq!(pixel(&pages.pages[0], 12, 12), [255, 0, 0, 255]);
    assert_eq!(pixel(&pages.pages[0], 9, 9)[3], 0);
    assert_eq!(pixel(&pages.pages[0], 2, 2)[3], 0);
    assert_eq!(pixel(&pages.pages[0], 32, 32)[3], 0);
    // Page 1 ("Z first"): its art and its button.
    assert_eq!(pixel(&pages.pages[1], 42, 2), [200, 200, 0, 255]);
    assert_eq!(pixel(&pages.pages[1], 32, 32), [0, 255, 255, 255]);
    assert_eq!(pixel(&pages.pages[1], 12, 12)[3], 0);
    assert_eq!(pixel(&pages.pages[1], 52, 42)[3], 0);
    // Page 2 ("A second").
    assert_eq!(pixel(&pages.pages[2], 52, 42), [255, 0, 255, 255]);
    assert_eq!(pixel(&pages.pages[2], 42, 2)[3], 0);
    let _ = std::fs::remove_file(&path);
}
