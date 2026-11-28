//! The Library's own files (M6.403, Encore's Library ▸ Add Item…): pictures and layered documents copied into the
//! library's folder, each a menu, a button, a picture, a layer set or a replacement layer by its contents (as Encore
//! tells them, docs/encore-behavior.md 9.18); and DiscCraft's Import Folder…, which makes a set of each folder of
//! such files (a folder of menus the user has: DiscCraft ships no files of its own here).

use crate::i18n::{tl, tlf};
use crate::user_library::{FileItem, FileKind};
use disccraft_project::{AssetId, AssetKind, MenuId};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The files the Library takes (Encore's "library files", PhotoCraft's documents beside them; Encore's own `.em`
/// is not read).
pub(crate) const LIBRARY_EXTENSIONS: [&str; 11] = [
    "psd", "psb", "pcraft", "png", "jpg", "jpeg", "jpe", "bmp", "gif", "tif", "tiff",
];

/// The most files one import takes.
pub(crate) const MAX_IMPORT_FILES: usize = 5000;

/// The largest file taken (a bigger one is skipped).
const MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024;

/// How deep Import Folder… looks into the folders under the one chosen.
const MAX_DEPTH: usize = 8;

/// The folder, inside the library's, that holds its files.
pub(crate) const ITEMS_FOLDER: &str = "items";

/// One file taken into the library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Taken {
    /// The set it goes into (empty: the set shown when it was added).
    pub(crate) set: String,
    /// Its name in the Library: the file's name, without its extension unless it is a picture (as Encore lists
    /// them).
    pub(crate) name: String,
    /// The copy, relative to the library's folder.
    pub(crate) file: String,
    /// What it is.
    pub(crate) kind: FileKind,
}

/// What an import took, and how many files it left (unreadable, too big, or over the limit).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Imported {
    pub(crate) items: Vec<Taken>,
    pub(crate) skipped: usize,
}

/// Whether the Library takes the file at `path` (by its extension).
pub(crate) fn is_library_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            LIBRARY_EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

/// The files of `folder` by set for Import Folder…: the folder's own files as a set named after it, and each folder
/// under it (to [`MAX_DEPTH`]) that has files as a set named after that folder; the sets in the order of their paths,
/// the files in each by name, [`MAX_IMPORT_FILES`] at most in all.
pub(crate) fn folder_sets(folder: &Path) -> Vec<(String, Vec<PathBuf>)> {
    let mut sets = Vec::new();
    let mut count = 0usize;
    let mut pending = vec![(folder.to_path_buf(), 0usize)];
    let mut folders = Vec::new();
    while let Some((dir, depth)) = pending.pop() {
        folders.push(dir.clone());
        if depth >= MAX_DEPTH {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&dir) {
            let mut inner: Vec<PathBuf> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect();
            inner.sort();
            pending.extend(inner.into_iter().rev().map(|path| (path, depth + 1)));
        }
    }
    folders.sort();
    for dir in folders {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && is_library_file(path))
            .collect();
        files.sort();
        files.truncate(MAX_IMPORT_FILES.saturating_sub(count));
        if files.is_empty() {
            continue;
        }
        count += files.len();
        let name = dir.file_name().map_or_else(
            || dir.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        sets.push((name, files));
    }
    sets
}

/// Copies the files of `sources` (set name, files) into `library`'s items folder (a name already there gets a
/// number) and tells what each one is; `done` counts the files gone through. A file that cannot be read or copied,
/// or is over 1 GB, is skipped.
///
/// # Errors
///
/// When the items folder cannot be made.
pub(crate) fn take_files(
    library: &Path,
    sources: &[(String, Vec<PathBuf>)],
    done: &AtomicUsize,
) -> Result<Imported, String> {
    let items = library.join(ITEMS_FOLDER);
    std::fs::create_dir_all(&items).map_err(|error| format!("{}: {error}", items.display()))?;
    let mut imported = Imported::default();
    for (set, files) in sources {
        for source in files {
            let _ = done.fetch_add(1, Ordering::Relaxed);
            match take_file(&items, source) {
                Some((file, kind)) => imported.items.push(Taken {
                    set: set.clone(),
                    name: item_name(source, kind),
                    file: format!("{ITEMS_FOLDER}/{file}"),
                    kind,
                }),
                None => imported.skipped += 1,
            }
        }
    }
    Ok(imported)
}

/// Tells what `source` is and copies it into `items`: the copy's file name and the kind; `None` when it is skipped.
fn take_file(items: &Path, source: &Path) -> Option<(String, FileKind)> {
    let size = std::fs::metadata(source).ok()?.len();
    if size > MAX_FILE_BYTES {
        return None;
    }
    let kind = FileKind::from(disccraft_photo::library_file_kind(source).ok()?);
    let stem = source.file_stem()?.to_string_lossy().into_owned();
    let ext = source.extension()?.to_string_lossy().into_owned();
    let target = crate::photocraft::unique_path(items, &stem, &ext);
    std::fs::copy(source, &target).ok()?;
    Some((target.file_name()?.to_string_lossy().into_owned(), kind))
}

/// A file's name in the Library: a picture keeps its extension, the others go without (as Encore lists them).
fn item_name(source: &Path, kind: FileKind) -> String {
    let name = if kind == FileKind::Image {
        source.file_name()
    } else {
        source.file_stem()
    };
    name.map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

/// An import running on a thread of its own: how far it is, and its result when it is over.
pub(crate) struct LibraryImport {
    pub(crate) done: Arc<AtomicUsize>,
    pub(crate) total: usize,
    pub(crate) result: std::sync::mpsc::Receiver<Result<Imported, String>>,
}

/// Starts copying `sources` into the library's folder `library` on a thread of its own.
pub(crate) fn start_import(
    library: PathBuf,
    sources: Vec<(String, Vec<PathBuf>)>,
) -> LibraryImport {
    let done = Arc::new(AtomicUsize::new(0));
    let total = sources.iter().map(|(_, files)| files.len()).sum();
    let (send, result) = std::sync::mpsc::channel();
    let counter = Arc::clone(&done);
    let _ = std::thread::Builder::new()
        .name("library-import".to_owned())
        .spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                take_files(&library, &sources, &counter)
            }))
            .unwrap_or_else(|_| Err("internal error while importing".to_owned()));
            let _ = send.send(outcome);
        });
    LibraryImport {
        done,
        total,
        result,
    }
}

impl crate::DiscCraftApp {
    /// The Library's Add Items from Files… (Encore's Library ▸ Add Item…): the files chosen are copied into the
    /// library and go into the set shown (a set of one's own, else User).
    pub(crate) fn library_add_files(&mut self) {
        if self.library_busy() {
            return;
        }
        crate::file_dialogs::ask(
            craft_dialogs::FileDialog::open_many()
                .filter(tl!("Library Files"), &LIBRARY_EXTENSIONS),
            crate::file_dialogs::Then::LibraryFiles,
        );
    }

    /// The files chosen for the Library's Add Items from Files….
    pub(crate) fn library_add_paths(&mut self, files: Vec<PathBuf>) {
        if self.library_busy() {
            return;
        }
        let files: Vec<PathBuf> = files
            .into_iter()
            .filter(|path| is_library_file(path))
            .take(MAX_IMPORT_FILES)
            .collect();
        let set = self.own_set_name(self.library.set).unwrap_or_default();
        self.start_library_import(vec![(String::new(), files)], set);
    }

    /// The Library's Import Folder… (DiscCraft's own): a set of the folder chosen and of each folder under it that
    /// has PSDs or pictures, the files copied into the library.
    pub(crate) fn library_import_folder(&mut self) {
        if self.library_busy() {
            return;
        }
        crate::file_dialogs::ask(
            craft_dialogs::FileDialog::folder(),
            crate::file_dialogs::Then::LibraryFolder,
        );
    }

    /// The folder chosen for the Library's Import Folder….
    pub(crate) fn library_import_from(&mut self, folder: &Path) {
        if self.library_busy() {
            return;
        }
        let sets = folder_sets(folder);
        if sets.is_empty() {
            self.status = tl!("No library files (PSDs or pictures) in that folder").to_owned();
            return;
        }
        self.start_library_import(sets, String::new());
    }

    fn library_busy(&mut self) -> bool {
        if self.library_import.is_some() {
            self.status = tl!("The library is still adding files").to_owned();
        }
        self.library_import.is_some()
    }

    fn start_library_import(&mut self, sources: Vec<(String, Vec<PathBuf>)>, set: String) {
        let Some(library) = crate::workspace::library_dir() else {
            self.status = tl!("The library has no folder (Preferences ▸ Media)").to_owned();
            return;
        };
        self.library_import = Some((start_import(library, sources), set));
    }

    /// Takes in what an import took once it is over; meanwhile shows how far it is.
    pub(crate) fn poll_library_import(&mut self, ctx: &egui::Context) {
        let Some((import, _)) = &self.library_import else {
            return;
        };
        match import.result.try_recv() {
            Ok(result) => {
                let set = self
                    .library_import
                    .take()
                    .map(|(_, set)| set)
                    .unwrap_or_default();
                self.finish_library_import(result, &set);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                self.status = tlf!(
                    "Adding to the library: {done} of {total}",
                    done = import.done.load(Ordering::Relaxed),
                    total = import.total
                );
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.library_import = None;
                self.status = tlf!(
                    "Could not add to the library: {error}",
                    error = "internal error"
                );
            }
        }
    }

    /// Adds what an import took to the library (files chosen one by one into `set`, a folder's into a set of its
    /// name) and shows the last set.
    fn finish_library_import(&mut self, result: Result<Imported, String>, set: &str) {
        let imported = match result {
            Ok(imported) => imported,
            Err(error) => {
                self.status = tlf!("Could not add to the library: {error}", error = error);
                return;
            }
        };
        let mut added = 0usize;
        let mut skipped = imported.skipped;
        let mut shown = None;
        for taken in imported.items {
            let set = if taken.set.is_empty() {
                set.to_owned()
            } else {
                self.user_library.set_for(&taken.set)
            };
            let item = crate::user_library::UserItem {
                name: taken.name,
                kind: crate::user_library::UserItemKind::File(crate::user_library::FileItem {
                    file: taken.file.clone(),
                    kind: taken.kind,
                }),
                set: set.clone(),
            };
            if self.user_library.add(item).is_ok() {
                added += 1;
                shown = Some(set);
            } else {
                // The library is full: the copy goes.
                if let Some(path) = crate::workspace::library_dir().map(|dir| dir.join(&taken.file))
                {
                    let _ = std::fs::remove_file(path);
                }
                skipped += 1;
            }
        }
        if let Err(error) = self.user_library.save() {
            self.status = tlf!("Could not save the library: {error}", error = error);
            return;
        }
        if let Some(set) = shown {
            self.library.set = self
                .user_library
                .sets
                .iter()
                .position(|own| *own == set)
                .map_or(crate::library_panel::LibrarySet::User, |index| {
                    crate::library_panel::LibrarySet::Own(index)
                });
            self.library.selected = None;
        }
        self.status = if skipped == 0 {
            tlf!("Added {n} items to the library", n = added)
        } else {
            tlf!(
                "Added {n} items to the library; {skipped} files could not be taken",
                n = added,
                skipped = skipped
            )
        };
    }

    /// The library's file `file` as a file of the project: copied into the project's folder of menu files (the
    /// copy made before is used again), so the menu or picture made of it stays when the library item goes.
    fn library_file_for_project(&mut self, file: &FileItem) -> Option<PathBuf> {
        let source = file.path().filter(|path| path.is_file());
        let Some(source) = source else {
            self.status = tlf!("The library's file is missing: {name}", name = file.file);
            return None;
        };
        let folder = match self.craft_folder() {
            Ok(folder) => folder,
            Err(error) => {
                self.status = error;
                return None;
            }
        };
        let name = source.file_name()?;
        let same = folder.join(name);
        let size = |path: &Path| std::fs::metadata(path).ok().map(|meta| meta.len());
        if same.is_file() && size(&same) == size(&source) {
            return Some(same);
        }
        let stem = source.file_stem()?.to_string_lossy().into_owned();
        let ext = source.extension()?.to_string_lossy().into_owned();
        let target = crate::photocraft::unique_path(&folder, &stem, &ext);
        match std::fs::copy(&source, &target) {
            Ok(_) => Some(target),
            Err(error) => {
                self.status = format!("{}: {error}", target.display());
                None
            }
        }
    }

    /// New Menu (`replace`: `None`) or Replace (the open menu) with a menu file of the library: as File ▸ Import ▸
    /// Menu… and Menu ▸ Replace Menu… do with the PSD.
    pub(crate) fn library_file_menu(&mut self, file: &FileItem, replace: Option<MenuId>) {
        let Some(path) = self.library_file_for_project(file) else {
            return;
        };
        let path = path.to_string_lossy().into_owned();
        match replace {
            Some(menu) => {
                if self
                    .run_command("menu.replace", json!({ "menu": menu.0, "path": path }))
                    .is_some()
                {
                    self.status = tl!("Replaced the menu").to_owned();
                }
            }
            None => {
                let made = self.run_command("menu.import", json!({ "path": path }));
                if let Some(id) = made.and_then(|value| value.get("menu").and_then(Value::as_u64)) {
                    self.select(crate::app::Selection::Menu(MenuId(id)));
                }
            }
        }
    }

    /// Places a file of the library in `menu` (its top left corner at `at`, frame pixels; else centred): a picture
    /// as an image layer fitting half the frame; a button as its picture with a button over its set, the set's
    /// highlight layers its subpicture (M6.405, `menu.placePsdButton`); a layer set as its layers, each an image layer
    /// where it is in the set, and a replacement layer as one (M6.405, as Encore places them). A document larger than
    /// the frame is stretched into it.
    pub(crate) fn place_library_file(
        &mut self,
        menu: MenuId,
        file: &FileItem,
        at: Option<(u16, u16)>,
    ) {
        let Some(path) = self.library_file_for_project(file) else {
            return;
        };
        let text = path.to_string_lossy().into_owned();
        match file.kind {
            FileKind::Button => {
                let mut params = json!({ "menu": menu.0, "path": text });
                if let (Some((x, y)), Some(map)) = (at, params.as_object_mut()) {
                    map.insert("x".to_owned(), json!(x));
                    map.insert("y".to_owned(), json!(y));
                }
                let placed = self.run_command("menu.placePsdButton", params);
                if let Some(button) = placed
                    .and_then(|value| value.get("button").and_then(Value::as_u64))
                    .and_then(|button| usize::try_from(button).ok())
                {
                    self.select_only_button(menu, button);
                }
                return;
            }
            FileKind::LayerSet | FileKind::Replacement => {
                self.place_library_layers(menu, &path, file.kind == FileKind::Replacement, at);
                return;
            }
            FileKind::Menu | FileKind::Image => {}
        }
        let known = self
            .session
            .project
            .assets
            .values()
            .find(|asset| asset.kind == AssetKind::Image && Path::new(&asset.path) == path)
            .map(|asset| asset.id);
        let asset = match known {
            Some(asset) => asset,
            None => {
                let imported = self.run_command("asset.import", json!({ "path": text }));
                match imported.and_then(|value| value.get("asset").and_then(Value::as_u64)) {
                    Some(id) => AssetId(id),
                    None => return,
                }
            }
        };
        // Its size from the file itself: a new asset's is not known yet (it is measured in the background).
        let size = disccraft_photo::picture_size(&path).ok();
        if let Some(index) = self.place_document(menu, asset, (size, true), at) {
            self.select_menu_graphic(menu, index);
        }
    }

    /// A layer set's layers (or a replacement layer) of the document at `path` placed in `menu` (M6.405): each layer
    /// written as a picture of its own beside the document (`<name> <layer>.psd`) and placed where it is in the set,
    /// the set's top left corner at `at`, else the set centred in the frame (as Encore places it); a document larger
    /// than the frame stretched into it.
    fn place_library_layers(
        &mut self,
        menu: MenuId,
        path: &Path,
        replacement: bool,
        at: Option<(u16, u16)>,
    ) {
        let ((canvas_w, canvas_h), pieces) = match disccraft_photo::psd_pieces(path) {
            Ok(found) => found,
            Err(error) => {
                self.status = error.to_string();
                return;
            }
        };
        if pieces.is_empty() {
            return;
        }
        let (frame_w, frame_h) = disccraft_project::frame_size(&self.session.project.settings);
        let fits = canvas_w <= u32::from(frame_w) && canvas_h <= u32::from(frame_h);
        let scale = |value: u32, canvas: u32, frame: u16| -> u16 {
            let scaled = if fits {
                u64::from(value)
            } else {
                u64::from(value) * u64::from(frame) / u64::from(canvas.max(1))
            };
            u16::try_from(scaled).unwrap_or(u16::MAX)
        };
        let folder = path.parent().unwrap_or(Path::new("."));
        let stem = path
            .file_stem()
            .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
        let mut graphics = Vec::new();
        let mut images = Vec::new();
        for piece in &pieces {
            let name: String = piece
                .name
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || " -_()".contains(c) {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            let target =
                crate::photocraft::unique_path(folder, &format!("{stem} {}", name.trim()), "psd");
            let layer = disccraft_photo::PsdLayer::Raster {
                name: piece.name.clone(),
                rgba: piece.image.rgba.clone(),
                visible: true,
            };
            if let Err(error) = disccraft_photo::write_layered_psd(
                &target,
                piece.image.width,
                piece.image.height,
                &[layer],
            ) {
                self.status = error.to_string();
                return;
            }
            graphics.push(disccraft_project::MenuGraphic {
                rect: disccraft_project::Rect {
                    x: scale(piece.x, canvas_w, frame_w),
                    y: scale(piece.y, canvas_h, frame_h),
                    w: scale(piece.image.width, canvas_w, frame_w).max(1),
                    h: scale(piece.image.height, canvas_h, frame_h).max(1),
                },
                // Taken from the project's asset of the file when it is placed.
                kind: disccraft_project::GraphicKind::Image { asset: AssetId(0) },
                opacity: disccraft_project::FULL_OPACITY,
                shadow: None,
                orientation: Default::default(),
                hidden: false,
                locked: false,
                replacement: replacement && pieces.len() == 1,
                effects: Default::default(),
            });
            images.push(Some(target.to_string_lossy().into_owned()));
        }
        // Centred in the frame unless dropped somewhere.
        let at = at.or_else(|| {
            let left = graphics.iter().map(|graphic| graphic.rect.x).min()?;
            let top = graphics.iter().map(|graphic| graphic.rect.y).min()?;
            let right = graphics
                .iter()
                .map(|graphic| graphic.rect.x.saturating_add(graphic.rect.w))
                .max()?;
            let bottom = graphics
                .iter()
                .map(|graphic| graphic.rect.y.saturating_add(graphic.rect.h))
                .max()?;
            Some((
                frame_w.saturating_sub(right.saturating_sub(left)) / 2,
                frame_h.saturating_sub(bottom.saturating_sub(top)) / 2,
            ))
        });
        let item = crate::user_library::LayerItem {
            graphics,
            images,
            replacement: replacement && pieces.len() == 1,
        };
        self.place_layers(menu, &item, at);
    }

    /// Places the image asset of a Library file of `size` (`None`: not known): a picture (`picture`) fitting half the
    /// frame, keeping its shape, as the Library's pictures are placed; a layered document at its own size, one as wide
    /// or as tall as the frame (made for a disc's menu) stretched over the whole frame as a menu's PSD of another size
    /// is. Its top left corner at `at` (moved in to stay in the frame), else centred: the new graphic's number.
    fn place_document(
        &mut self,
        menu: MenuId,
        asset: AssetId,
        (size, picture): (Option<(u32, u32)>, bool),
        at: Option<(u16, u16)>,
    ) -> Option<usize> {
        let (frame_w, frame_h) = disccraft_project::frame_size(&self.session.project.settings);
        let (image_w, image_h) = size
            .or(self.asset_probe(asset).dimensions)
            .filter(|(w, h)| *w > 0 && *h > 0)
            .unwrap_or((u32::from(frame_w) / 3, u32::from(frame_h) / 3));
        let (w, h) = if !picture && (image_w >= u32::from(frame_w) || image_h >= u32::from(frame_h))
        {
            (frame_w, frame_h)
        } else {
            let fit = if picture { 2.0 } else { 1.0 };
            let scale = (f64::from(frame_w) / fit / f64::from(image_w))
                .min(f64::from(frame_h) / fit / f64::from(image_h))
                .min(1.0);
            (
                ((f64::from(image_w) * scale).round() as u16).clamp(1, frame_w.max(1)),
                ((f64::from(image_h) * scale).round() as u16).clamp(1, frame_h.max(1)),
            )
        };
        let (x, y) = match at {
            Some((x, y)) => (
                x.min(frame_w.saturating_sub(w)),
                y.min(frame_h.saturating_sub(h)),
            ),
            None => (frame_w.saturating_sub(w) / 2, frame_h.saturating_sub(h) / 2),
        };
        self.run_command(
            "menu.addImage",
            json!({ "menu": menu.0, "asset": asset.0, "x": x, "y": y, "w": w, "h": h }),
        )
        .and_then(|value| value.get("index").and_then(Value::as_u64))
        .and_then(|index| usize::try_from(index).ok())
    }

    /// The preview of a file of the library: its picture (a layered document without its highlight layers), fitted
    /// in the preview's box.
    pub(crate) fn library_file_preview(&mut self, ui: &mut egui::Ui, file: &FileItem, height: f32) {
        let texture = match self.library_previews.get(&file.file) {
            Some(texture) => texture.clone(),
            None => {
                let texture = file
                    .path()
                    .and_then(|path| disccraft_photo::load_menu_background(&path).ok())
                    .map(|image| preview_texture(ui.ctx(), &file.file, &image));
                let _ = self
                    .library_previews
                    .insert(file.file.clone(), texture.clone());
                texture
            }
        };
        let (outer, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), height),
            egui::Sense::hover(),
        );
        let painter = ui.painter_at(outer);
        painter.rect_filled(outer, 2.0, ui.visuals().extreme_bg_color);
        if let Some(texture) = texture {
            let size = texture.size_vec2();
            let scale = (outer.width() / size.x.max(1.0))
                .min(outer.height() / size.y.max(1.0))
                .min(1.0);
            let rect = egui::Rect::from_center_size(outer.center(), size * scale);
            painter.image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }
}

/// The longest side of a Library file's preview, in pixels.
const PREVIEW_SIDE: u32 = 320;

/// `image` shrunk to [`PREVIEW_SIDE`] at most, as a texture.
fn preview_texture(
    ctx: &egui::Context,
    name: &str,
    image: &disccraft_photo::Image8,
) -> egui::TextureHandle {
    let (w, h) = (image.width.max(1), image.height.max(1));
    let step = w.max(h).div_ceil(PREVIEW_SIDE).max(1);
    let (tw, th) = (w.div_ceil(step), h.div_ceil(step));
    let mut rgba = Vec::with_capacity((tw * th * 4) as usize);
    for y in 0..th {
        for x in 0..tw {
            let i = (((y * step).min(h - 1) * w + (x * step).min(w - 1)) * 4) as usize;
            rgba.extend_from_slice(image.rgba.get(i..i + 4).unwrap_or(&[0, 0, 0, 0]));
        }
    }
    ctx.load_texture(
        format!("library-file:{name}"),
        egui::ColorImage::from_rgba_unmultiplied([tw as usize, th as usize], &rgba),
        egui::TextureOptions::LINEAR,
    )
}

#[cfg(test)]
mod tests {
    use super::{folder_sets, take_files};
    use crate::user_library::FileKind;
    use std::sync::atomic::AtomicUsize;

    /// M6.403: Import Folder… makes a set of the folder and of each folder under it that has library files; the
    /// files are copied into the library's items folder (a name taken gets a number), each with its kind, a picture
    /// named with its extension.
    #[test]
    fn a_folder_of_files_becomes_sets_of_items() -> Result<(), Box<dyn std::error::Error>> {
        let root =
            std::env::temp_dir().join(format!("disccraft-library-files-{}", std::process::id()));
        let source = root.join("Library");
        let menus = source.join("Menus");
        std::fs::create_dir_all(&menus)?;
        std::fs::create_dir_all(source.join("Empty"))?;
        std::fs::write(source.join("photo.png"), b"not read")?;
        std::fs::write(source.join("notes.txt"), b"not taken")?;
        std::fs::write(menus.join("photo.png"), b"another")?;
        let sets = folder_sets(&source);
        let names: Vec<(&str, usize)> = sets
            .iter()
            .map(|(name, files)| (name.as_str(), files.len()))
            .collect();
        assert_eq!(names, [("Library", 1), ("Menus", 1)]);

        let library = root.join("library");
        let done = AtomicUsize::new(0);
        let imported = take_files(&library, &sets, &done)?;
        assert_eq!(imported.skipped, 0);
        let items: Vec<(&str, &str, &str, FileKind)> = imported
            .items
            .iter()
            .map(|item| {
                (
                    item.set.as_str(),
                    item.name.as_str(),
                    item.file.as_str(),
                    item.kind,
                )
            })
            .collect();
        assert_eq!(
            items,
            [
                ("Library", "photo.png", "items/photo.png", FileKind::Image),
                ("Menus", "photo.png", "items/photo 2.png", FileKind::Image),
            ]
        );
        assert!(library.join("items").join("photo 2.png").is_file());
        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }
}
