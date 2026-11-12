//! Renders a three-button sample menu to `target/fixtures/menu_render.png` (and its highlight layer to `menu_highlight.png`).

use disccraft_menu::render_menu;
use disccraft_project::{Link, Project};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

fn write_png(
    path: &Path,
    w: u32,
    h: u32,
    color: png::ColorType,
    data: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut enc = png::Encoder::new(BufWriter::new(File::create(path)?), w, h);
    enc.set_color(color);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(data)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut project = Project::default();
    let menu = project.add_menu(Some("メインメニュー".to_string()))?;
    for label in ["最初から再生", "第2章 チャプター", "Chapter 3"] {
        project.add_button(menu, label.to_string(), None, Link::NotSet)?;
    }
    let m = project.menu(menu).ok_or("menu")?;
    let r = render_menu(&project, m)?;
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
    write_png(
        &dir.join("menu_render.png"),
        r.width.into(),
        r.height.into(),
        png::ColorType::Rgb,
        &r.rgb,
    )?;
    let hl: Vec<u8> = r.highlight.pixels.iter().map(|&p| p * 85).collect();
    write_png(
        &dir.join("menu_highlight.png"),
        r.highlight.width.into(),
        r.highlight.height.into(),
        png::ColorType::Grayscale,
        &hl,
    )?;
    println!(
        "buttons: {:?}",
        r.buttons
            .iter()
            .map(|b| (b.y1, b.up, b.down))
            .collect::<Vec<_>>()
    );
    Ok(())
}
