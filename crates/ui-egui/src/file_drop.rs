//! Files dragged in from the system (Explorer, Finder) and dropped on the window: imported into the project as
//! assets, as Encore imports files dropped on its Project panel. A dropped folder brings its media files (not its
//! subfolders). While files are dragged over the window, it says where they go.

use crate::app::{DiscCraftApp, Selection};
use crate::i18n::{tl, tlf};
use crate::workspace::Panel;
use disccraft_project::AssetId;
use std::path::{Path, PathBuf};

/// The file types DiscCraft imports as assets (the File ▸ Import ▸ Asset dialog's list).
pub(crate) const MEDIA_EXTENSIONS: [&str; 48] = [
    "mp4", "mov", "m4v", "avi", "mkv", "webm", "mxf", "m2v", "m1v", "mpg", "mpeg", "vob", "m2ts",
    "mts", "m2t", "ts", "dv", "dif", "wav", "ac3", "dts", "mp3", "aac", "m4a", "mka", "mp2", "ogg",
    "oga", "opus", "flac", "aif", "aiff", "png", "jpg", "jpeg", "bmp", "tif", "tiff", "psd", "psb",
    "pcraft", "svg", "srt", "vtt", "idx", "sup", "ecproj", "scraft",
];

/// The most files one drop imports (a folder of thousands of pictures would otherwise stall the window).
const MAX_DROPPED_FILES: usize = 500;

/// Whether `path` names a file type DiscCraft imports.
pub(crate) fn is_media(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            MEDIA_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

/// The media files of a drop: the dropped files themselves, and the media files directly inside dropped folders
/// (in name order); other files are left out. At most [`MAX_DROPPED_FILES`].
pub(crate) fn media_files(dropped: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for path in dropped {
        if path.is_dir() {
            let mut inside: Vec<PathBuf> = std::fs::read_dir(path)
                .map(|entries| {
                    entries
                        .filter_map(Result::ok)
                        .map(|entry| entry.path())
                        .filter(|entry| entry.is_file() && is_media(entry))
                        .collect()
                })
                .unwrap_or_default();
            inside.sort();
            files.extend(inside);
        } else if is_media(path) {
            files.push(path.clone());
        }
    }
    files.truncate(MAX_DROPPED_FILES);
    files
}

impl DiscCraftApp {
    /// Imports files dropped on the window (and while they are dragged over it, says that a drop imports them).
    pub(crate) fn accept_dropped_files(&mut self, ctx: &egui::Context) {
        let hovering = ctx.input(|i| i.raw.hovered_files.len());
        if hovering > 0 {
            paint_drop_hint(ctx, hovering);
        }
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        let files = media_files(&dropped);
        if files.is_empty() {
            self.status = tl!("No files DiscCraft can import were dropped").to_string();
            return;
        }
        self.show_panel(Panel::Project);
        let mut imported = 0usize;
        let mut last = None;
        for path in &files {
            if let Some(id) = self.import_asset_id(path) {
                imported += 1;
                last = Some(id);
            }
        }
        if let Some(id) = last {
            self.select(Selection::Asset(AssetId(id)));
            self.active_panel = Some(Panel::Project);
        }
        if imported > 0 {
            self.status = tlf!("Imported {count} files", count = imported);
        }
    }
}

/// A veil over the window while files are dragged over it, with what a drop does.
fn paint_drop_hint(ctx: &egui::Context, count: usize) {
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("file_drop_hint"),
    ));
    painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(140));
    let frame = screen.shrink(24.0);
    let _ = painter.rect_stroke(
        frame,
        8.0,
        egui::Stroke::new(2.0, egui::Color32::from_rgb(0x4a, 0x90, 0xff)),
        egui::StrokeKind::Inside,
    );
    painter.text(
        screen.center(),
        egui::Align2::CENTER_CENTER,
        tlf!(
            "Drop to import {count} items into the project",
            count = count
        ),
        egui::FontId::proportional(22.0),
        egui::Color32::WHITE,
    );
}

#[cfg(test)]
mod tests {
    use super::{is_media, media_files};
    use std::path::{Path, PathBuf};

    /// Media files are known by their extension (any case); a dropped folder gives its media files in name order,
    /// not its other files or subfolders.
    #[test]
    fn a_drop_keeps_the_media_files() -> Result<(), Box<dyn std::error::Error>> {
        assert!(is_media(Path::new("movie.MP4")) && is_media(Path::new("a/b/menu.psd")));
        assert!(!is_media(Path::new("notes.txt")) && !is_media(Path::new("noextension")));
        let dir = std::env::temp_dir().join(format!("disccraft-drop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub"))?;
        for name in ["b.png", "a.wav", "readme.txt", "sub/c.mp4"] {
            std::fs::write(dir.join(name), b"x")?;
        }
        let files = media_files(&[
            dir.clone(),
            PathBuf::from("clip.mov"),
            PathBuf::from("x.doc"),
        ]);
        assert_eq!(
            files,
            vec![
                dir.join("a.wav"),
                dir.join("b.png"),
                PathBuf::from("clip.mov")
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
