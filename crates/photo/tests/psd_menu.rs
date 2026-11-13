//! A menu PSD written with PhotoCraft's PSD writer is read back with Encore-compatible layer naming: `(+)` groups become
//! buttons, `(=1)` layers their highlights, and the background leaves the highlights out.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use disccraft_photo::{Image8, load_image, load_psd_menu};
use photocraft_psd::{GroupSpec, LayerSpec, PixelData, PsdBuilder};

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

fn write_psd() -> std::path::PathBuf {
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
    let path = std::env::temp_dir().join(format!("disccraft-photo-{}.psd", std::process::id()));
    std::fs::write(&path, b.to_bytes().unwrap()).unwrap();
    path
}

#[test]
fn reads_buttons_highlights_and_background() {
    let path = write_psd();
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
