//! `cargo xtask icons`: makes the application icon's files from its original, `assets/app-icon/disccraft.png`
//! (1024 x 1024), laid out as the Craft apps lay theirs out (see `assets/app-icon/README.md`):
//!
//! - `hicolor/<size>x<size>/apps/io.github.mOqOm.DiscCraft.png`, 16 to 512 px: the Linux icon theme, the window;
//! - `disccraft.ico`, 16 to 256 px: Windows (the program file, the installer, the shortcuts);
//! - `disccraft-1024.png` and `disccraft.icns` (16 to 1024 px): macOS, the tile padded onto Apple's icon grid
//!   (824 of 1024).
//!
//! Each size is reduced from the original with a Lanczos 3 filter, the alpha premultiplied meanwhile so that the
//! see-through corners do not darken the edges.

use std::path::Path;

use image::{ImageEncoder, Rgba32FImage, RgbaImage};

type Result<T> = std::result::Result<T, String>;

const APP_ID: &str = "io.github.mOqOm.DiscCraft";
const HICOLOR: [u32; 8] = [16, 24, 32, 48, 64, 128, 256, 512];
const ICO: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
const ICNS: [(&str, u32); 7] = [
    ("icp4", 16),
    ("icp5", 32),
    ("icp6", 64),
    ("ic07", 128),
    ("ic08", 256),
    ("ic09", 512),
    ("ic10", 1024),
];
/// Apple's icon grid: a tile of 824 in a 1024 square.
const MAC_TILE: u32 = 824;
const MAC_SIZE: u32 = 1024;

pub fn run(root: &Path) -> Result<()> {
    let dir = root.join("assets").join("app-icon");
    let original = image::open(dir.join("disccraft.png"))
        .map_err(|error| format!("assets/app-icon/disccraft.png: {error}"))?
        .to_rgba32f();
    let tile = premultiplied(&original);

    for size in HICOLOR {
        let folder = dir
            .join("hicolor")
            .join(format!("{size}x{size}"))
            .join("apps");
        std::fs::create_dir_all(&folder)
            .map_err(|error| format!("{}: {error}", folder.display()))?;
        write(
            &folder.join(format!("{APP_ID}.png")),
            &png(&reduced(&tile, size))?,
        )?;
    }

    let ico = ICO
        .iter()
        .map(|size| png(&reduced(&tile, *size)).map(|bytes| (*size, bytes)))
        .collect::<Result<Vec<_>>>()?;
    write(&dir.join("disccraft.ico"), &ico_file(&ico)?)?;

    // macOS: the tile reduced to Apple's grid, centred in a see-through square.
    let small = image::imageops::resize(
        &tile,
        MAC_TILE,
        MAC_TILE,
        image::imageops::FilterType::Lanczos3,
    );
    let mut mac = Rgba32FImage::new(MAC_SIZE, MAC_SIZE);
    let margin = i64::from((MAC_SIZE - MAC_TILE) / 2);
    image::imageops::overlay(&mut mac, &small, margin, margin);
    write(
        &dir.join("disccraft-1024.png"),
        &png(&reduced(&mac, MAC_SIZE))?,
    )?;
    let icns = ICNS
        .iter()
        .map(|(kind, size)| png(&reduced(&mac, *size)).map(|bytes| (*kind, bytes)))
        .collect::<Result<Vec<_>>>()?;
    write(&dir.join("disccraft.icns"), &icns_file(&icns)?)?;

    println!("icons: assets/app-icon made from disccraft.png");
    Ok(())
}

/// The picture with its colours multiplied by its alpha.
fn premultiplied(picture: &Rgba32FImage) -> Rgba32FImage {
    let mut out = picture.clone();
    for pixel in out.pixels_mut() {
        let alpha = pixel[3];
        for channel in 0..3 {
            pixel[channel] *= alpha;
        }
    }
    out
}

/// A premultiplied picture reduced to `size` square (as it is at its own size), as 8-bit straight alpha.
fn reduced(premultiplied: &Rgba32FImage, size: u32) -> RgbaImage {
    let small = if premultiplied.width() == size {
        premultiplied.clone()
    } else {
        image::imageops::resize(
            premultiplied,
            size,
            size,
            image::imageops::FilterType::Lanczos3,
        )
    };
    let mut out = RgbaImage::new(size, size);
    for (to, from) in out.pixels_mut().zip(small.pixels()) {
        let alpha = from[3].clamp(0.0, 1.0);
        let straight = |value: f32| {
            if alpha > 0.0 {
                (value / alpha).clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        // Inside 0 to 255 after the clamp; `as` rounds it to a byte.
        let byte = |value: f32| (value * 255.0).round() as u8;
        *to = image::Rgba([
            byte(straight(from[0])),
            byte(straight(from[1])),
            byte(straight(from[2])),
            byte(alpha),
        ]);
    }
    out
}

fn png(picture: &RgbaImage) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            picture.as_raw(),
            picture.width(),
            picture.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| format!("PNG: {error}"))?;
    Ok(bytes)
}

/// An ICO of PNG images (Windows Vista on reads them): the header, an entry for each, then the PNGs.
fn ico_file(images: &[(u32, Vec<u8>)]) -> Result<Vec<u8>> {
    let count = u16::try_from(images.len()).map_err(|_| "too many ICO images".to_string())?;
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    let mut offset = 6 + 16 * images.len();
    for (size, bytes) in images {
        // 256 is written as 0.
        let side = u8::try_from(*size).unwrap_or(0);
        out.extend_from_slice(&[side, side, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&u32_of(bytes.len())?.to_le_bytes());
        out.extend_from_slice(&u32_of(offset)?.to_le_bytes());
        offset += bytes.len();
    }
    for (_, bytes) in images {
        out.extend_from_slice(bytes);
    }
    Ok(out)
}

/// An ICNS of PNG elements: `icns` and the whole length, then each element's type, length and PNG.
fn icns_file(elements: &[(&str, Vec<u8>)]) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    for (kind, bytes) in elements {
        body.extend_from_slice(kind.as_bytes());
        body.extend_from_slice(&u32_of(bytes.len() + 8)?.to_be_bytes());
        body.extend_from_slice(bytes);
    }
    let mut out = b"icns".to_vec();
    out.extend_from_slice(&u32_of(body.len() + 8)?.to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

fn u32_of(value: usize) -> Result<u32> {
    u32::try_from(value).map_err(|_| "icon file too large".to_string())
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).map_err(|error| format!("{}: {error}", path.display()))
}
