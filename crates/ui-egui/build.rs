//! The craft-fonts faces under Encore's (M9.66), embedded as every Craft app embeds them (craft-fonts-build).
//!
//! The checkout comes from `DISCCRAFT_FONTS_DIR` (a storytold/craft-fonts checkout, as an absolute path; the release
//! builds set it), or else craft-fonts' own `CRAFT_FONTS_DIR`. DiscCraft has a variable of its own because
//! `CRAFT_FONTS_DIR` also makes FilmCraft's and PhotoCraft's text crates, which DiscCraft builds, embed every face of
//! the checkout (about 47 MB each). Every face of the manifest is embedded, the CJK interface faces cut to the
//! characters of the catalogs in `i18n/locales`. Unset, the table is empty and the interface draws with the OS's faces.
//! `DISCCRAFT_FONTS_REQUIRED=1` (or `CRAFT_FONTS_REQUIRED=1`) makes a missing or broken checkout an error. Only the
//! local directory is read: no network.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=DISCCRAFT_FONTS_DIR");
    println!("cargo::rerun-if-env-changed=DISCCRAFT_FONTS_REQUIRED");
    let mut embed = craft_fonts_build::Embed::new().subset_to(catalogs());
    if let Some(dir) = std::env::var_os("DISCCRAFT_FONTS_DIR").filter(|dir| !dir.is_empty()) {
        embed = embed.dir(dir);
    }
    if std::env::var("DISCCRAFT_FONTS_REQUIRED").is_ok_and(|value| value.trim() == "1") {
        embed = embed.required(true);
    }
    if let Err(error) = embed.write("craft_fonts.rs") {
        println!("cargo::error=craft-fonts: {error}");
    }
}

/// Every catalog the interface embeds (`i18n/locales/<language>/common.tsv` and `disccraft.tsv`): the characters the
/// cut interface faces keep.
fn catalogs() -> Vec<PathBuf> {
    let locales = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("i18n")
        .join("locales");
    println!("cargo::rerun-if-changed={}", locales.display());
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&locales)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|locale| ["common.tsv", "disccraft.tsv"].map(|file| locale.path().join(file)))
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    paths
}
