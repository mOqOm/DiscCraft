//! File ▸ Create Disc Label… (M6.506, DiscCraft's own: Encore makes no labels): the disc's face at its real size, to
//! print on a printable DVD or Blu-ray ([`disccraft_menu::label`]): a menu's background filling the printable ring
//! (nothing printed on the hole or past the edge), the disc's name and its chapters as editable type, and guides that
//! do not print. M6.507: a DesignCraft document (made by DesignCraft's command line), opened in DesignCraft; without
//! DesignCraft, a PSD for PhotoCraft (M6.506) as before. It is written beside the project.
//!
//! M6.509: the label made is linked to the project (`disc.setLabel`, kept when the project is saved, with its path
//! relative to the project file). File ▸ Open Disc Label opens it again; the dialog shows it, opens it, links
//! another file (or the label found beside the project, made before labels were linked) and unlinks it.

use crate::app::DiscCraftApp;
use crate::i18n::{tl, tlf};
use crate::photocraft::{CraftApp, find_app, open_with_default_app, unique_path};
use disccraft_menu::label::{DiscLabel, LabelNames, first_menu, named_chapters};
use disccraft_project::{MenuBackground, MenuId};
use egui::{Context, Id, Modal};
use std::path::{Path, PathBuf};

/// The most entries of the project's folder looked at for a label made before labels were linked.
const MAX_FOLDER_ENTRIES: usize = 10_000;

/// What the dialog's row for the project's label asks for.
#[derive(Clone, Debug, PartialEq)]
enum LabelChoice {
    /// Open the linked label.
    Open,
    /// Choose a file and link it.
    Choose,
    /// Link this file (the one found beside the project).
    Link(PathBuf),
    /// Unlink the label (its file stays).
    Unlink,
}

/// The app that opens a label file: DesignCraft for its documents, PhotoCraft for a PSD, else none (the system's
/// default app).
fn label_app(path: &Path) -> Option<CraftApp> {
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase());
    match extension.as_deref() {
        Some("designcraft") => Some(CraftApp::Design),
        Some("psd" | "psb") => Some(CraftApp::Photo),
        _ => None,
    }
}

/// Opens a label file in the app that edits it (its program when found, else the system's default app for it);
/// the app's name, or the error.
fn open_label_file(path: &Path) -> Result<&'static str, String> {
    let app = label_app(path);
    let opened = match app.and_then(find_app) {
        Some(exe) => std::process::Command::new(exe)
            .arg(path)
            .spawn()
            .map(|_| ()),
        None => open_with_default_app(path),
    };
    opened
        .map(|()| app.map_or_else(|| tl!("the default app"), CraftApp::name))
        .map_err(|error| error.to_string())
}

/// The newest label beside the project file `project` that File ▸ Create Disc Label… named for it
/// (`<project> Disc Label.designcraft`, `<project> Disc Label 2.psd`, …, in either language): a label made
/// before labels were linked to the project.
fn label_beside(project: &Path) -> Option<PathBuf> {
    let folder = project.parent()?;
    let stem = project.file_stem()?.to_string_lossy().into_owned();
    let prefixes: Vec<String> = [tl!("Disc Label"), "Disc Label", "ディスクのレーベル"]
        .iter()
        .map(|words| crate::photocraft::file_name_for(&format!("{stem} {words}")))
        .collect();
    std::fs::read_dir(folder)
        .ok()?
        .take(MAX_FOLDER_ENTRIES)
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| label_app(path).is_some() && path.is_file())
        .filter(|path| {
            path.file_stem()
                .map(|name| name.to_string_lossy())
                .is_some_and(|name| {
                    prefixes.iter().any(|prefix| {
                        name.strip_prefix(prefix.as_str()).is_some_and(|rest| {
                            rest.is_empty()
                                || rest
                                    .strip_prefix(' ')
                                    .is_some_and(|number| number.parse::<u32>().is_ok())
                        })
                    })
                })
        })
        .max_by_key(|path| {
            std::fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .ok()
        })
}

/// A printable disc's face: its printable ring, as the disc's maker gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Printable {
    /// "Wide printable": from about 22 mm across.
    Wide,
    /// From about 43 mm across.
    Standard,
    /// Diameters typed in.
    Custom,
}

impl Printable {
    /// The ring's inner and outer diameters, mm (`None` for typed ones).
    fn ring(self) -> Option<(f32, f32)> {
        match self {
            Self::Wide => Some((22.0, 118.0)),
            Self::Standard => Some((43.0, 118.0)),
            Self::Custom => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Wide => tl!("Wide printable (22 to 118 mm)"),
            Self::Standard => tl!("Standard printable (43 to 118 mm)"),
            Self::Custom => tl!("Custom"),
        }
    }
}

/// The Create Disc Label dialog while it is open.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DiscLabelDialog {
    printable: Printable,
    inner_mm: f32,
    outer_mm: f32,
    dpi: f32,
    background: Option<MenuId>,
    title: String,
    chapters: bool,
    /// DesignCraft's command line, when it was found as the dialog opened (else the label is a PSD).
    designcraft: Option<std::path::PathBuf>,
    /// M6.509: a label found beside the project as the dialog opened, when none is linked (one made before labels
    /// were linked).
    found: Option<PathBuf>,
}

impl DiscCraftApp {
    /// File ▸ Create Disc Label…: opens its dialog.
    pub(crate) fn open_disc_label(&mut self) {
        let project = &self.session.project;
        // M6.508: the disc's own name only; the project file's name is no title for a disc (it was printed on it).
        let title = project.settings.name.trim().to_owned();
        self.disc_label = Some(DiscLabelDialog {
            printable: Printable::Wide,
            inner_mm: 22.0,
            outer_mm: 118.0,
            dpi: 300.0,
            background: first_menu(project),
            title,
            chapters: !named_chapters(project).is_empty(),
            designcraft: disccraft_menu::label::designcraft::find_cli(),
            found: if project.settings.disc_label.is_none() {
                self.session.path.as_deref().and_then(label_beside)
            } else {
                None
            },
        });
    }

    /// File ▸ Open Disc Label (M6.509): opens the project's label in the app that edits it.
    pub(crate) fn open_disc_label_file(&mut self) {
        let Some(path) = self.session.project.settings.disc_label.clone() else {
            self.status = tl!(
                "The project has no disc label: make one with File ▸ Create Disc Label…, or link a file there."
            )
            .to_owned();
            return;
        };
        let path = PathBuf::from(path);
        if !path.is_file() {
            self.status = tlf!(
                "The disc label {path} is missing: link it again in File ▸ Create Disc Label…",
                path = path.display()
            );
            return;
        }
        self.status = match open_label_file(&path) {
            Ok(app) => tlf!(
                "Opened the disc label {path} in {app}.",
                path = path.display(),
                app = app
            ),
            Err(error) => tlf!(
                "Cannot open the disc label {path}: {message}",
                path = path.display(),
                message = error
            ),
        };
    }

    /// Links `path` to the project as its disc label (M6.509); whether it was.
    pub(crate) fn link_disc_label(&mut self, path: &Path) -> bool {
        let linked = self
            .run_command(
                "disc.setLabel",
                serde_json::json!({ "path": path.to_string_lossy() }),
            )
            .is_some();
        if linked {
            self.status = tlf!(
                "The disc label {path} is linked to the project: save the project to keep it.",
                path = path.display()
            );
        }
        linked
    }

    /// What the dialog's row for the project's label asked for.
    fn label_choice(&mut self, choice: LabelChoice) {
        match choice {
            LabelChoice::Open => self.open_disc_label_file(),
            LabelChoice::Choose => {
                let mut picker = craft_dialogs::FileDialog::open()
                    .title(tl!("Link a Disc Label"))
                    .filter(tl!("Disc label"), &["designcraft", "psd", "psb"])
                    .filter(tl!("All files"), &["*"]);
                let start = self
                    .session
                    .project
                    .settings
                    .disc_label
                    .as_deref()
                    .map(Path::new)
                    .and_then(Path::parent)
                    .or_else(|| self.session.path.as_deref().and_then(Path::parent))
                    .map(Path::to_path_buf);
                if let Some(start) = start {
                    picker = picker.directory(start);
                }
                crate::file_dialogs::ask(picker, crate::file_dialogs::Then::LinkDiscLabel);
            }
            LabelChoice::Link(path) => {
                let _ = self.link_disc_label(&path);
            }
            LabelChoice::Unlink => {
                if self
                    .run_command("disc.setLabel", serde_json::json!({ "path": null }))
                    .is_some()
                {
                    self.status = tl!(
                        "The disc label is no longer linked to the project (its file stays where it is)."
                    )
                    .to_owned();
                }
            }
        }
    }

    /// The Create Disc Label dialog (drawn while it is open).
    pub(crate) fn disc_label_window(&mut self, ctx: &Context) {
        let Some(mut dialog) = self.disc_label.clone() else {
            return;
        };
        let chapters = named_chapters(&self.session.project);
        let menus: Vec<(MenuId, String)> = self
            .session
            .project
            .menus
            .values()
            .filter(|menu| menu.page_of.is_none())
            .map(|menu| (menu.id, menu.name.clone()))
            .collect();
        let linked = self
            .session
            .project
            .settings
            .disc_label
            .as_deref()
            .map(PathBuf::from);
        let mut done = None;
        let mut choice = None;
        let response = Modal::new(Id::unique("disc_label")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.strong(tl!("Create Disc Label"));
            ui.add_space(6.0);
            // M6.509: the label linked to the project.
            ui.horizontal_wrapped(|ui| {
                ui.label(tl!("This project's label:"));
                match &linked {
                    Some(path) => {
                        let name = path.file_name().map_or_else(
                            || path.to_string_lossy(),
                            |name| name.to_string_lossy(),
                        );
                        if path.is_file() {
                            ui.strong(name.as_ref())
                                .on_hover_text(path.display().to_string());
                        } else {
                            ui.colored_label(
                                ui.visuals().warn_fg_color,
                                tlf!("{name} (missing)", name = name),
                            )
                            .on_hover_text(path.display().to_string());
                        }
                    }
                    None => {
                        ui.weak(tl!("None"));
                    }
                }
            });
            ui.horizontal(|ui| {
                if let Some(path) = &linked
                    && ui
                        .add_enabled(path.is_file(), crate::theme::dialog_button(tl!("Open")))
                        .clicked()
                {
                    choice = Some(LabelChoice::Open);
                }
                if ui.add(crate::theme::dialog_button(tl!("Link a File…"))).clicked() {
                    choice = Some(LabelChoice::Choose);
                }
                if linked.is_some() && ui.add(crate::theme::dialog_button(tl!("Unlink"))).clicked() {
                    choice = Some(LabelChoice::Unlink);
                }
            });
            if linked.is_none()
                && let Some(found) = &dialog.found
            {
                ui.horizontal_wrapped(|ui| {
                    let name = found.file_name().map_or_else(
                        || found.to_string_lossy(),
                        |name| name.to_string_lossy(),
                    );
                    ui.label(tlf!("Found beside the project: {name}", name = name));
                    if ui.add(crate::theme::dialog_button(tl!("Link This File"))).clicked() {
                        choice = Some(LabelChoice::Link(found.clone()));
                    }
                });
            }
            ui.weak(tl!(
                "The label made is linked to the project and kept when the project is saved (an earlier label's file stays where it is)."
            ));
            ui.separator();
            ui.label(if dialog.designcraft.is_some() {
                tl!(
                    "Makes the disc's face at its real size in DesignCraft, to print on a printable disc on the printer's disc tray."
                )
            } else {
                tl!(
                    "DesignCraft was not found: makes the disc's face at its real size for PhotoCraft instead, to print on a printable disc on the printer's disc tray."
                )
            });
            ui.add_space(8.0);
            egui::Grid::new("disc_label_grid")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label(tl!("Printable area:"));
                    ui.vertical(|ui| {
                        egui::ComboBox::from_id_salt("disc_label_printable")
                            .selected_text(dialog.printable.label())
                            .show_ui(ui, |ui| {
                                for kind in [Printable::Wide, Printable::Standard, Printable::Custom] {
                                    if ui.selectable_label(dialog.printable == kind, kind.label()).clicked() {
                                        dialog.printable = kind;
                                        if let Some((inner, outer)) = kind.ring() {
                                            dialog.inner_mm = inner;
                                            dialog.outer_mm = outer;
                                        }
                                    }
                                }
                            });
                        ui.add_enabled_ui(dialog.printable == Printable::Custom, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(tl!("Inner"));
                                ui.add(
                                    egui::DragValue::new(&mut dialog.inner_mm)
                                        .range(15.0..=60.0)
                                        .speed(0.5)
                                        .suffix(" mm"),
                                );
                                ui.label(tl!("Outer"));
                                ui.add(
                                    egui::DragValue::new(&mut dialog.outer_mm)
                                        .range(80.0..=120.0)
                                        .speed(0.5)
                                        .suffix(" mm"),
                                );
                            });
                        });
                        ui.weak(tl!(
                            "As the disc's package says; set the printer's disc settings the same."
                        ));
                    });
                    ui.end_row();

                    // A PSD's pixels only: DesignCraft sets the text as type and places the picture as it is.
                    if dialog.designcraft.is_none() {
                        ui.label(tl!("Resolution:"));
                        ui.horizontal(|ui| {
                            for dpi in [300.0_f32, 600.0] {
                                ui.radio_value(&mut dialog.dpi, dpi, format!("{dpi:.0} dpi"));
                            }
                        });
                        ui.end_row();
                    }

                    ui.label(tl!("Background:"));
                    let current = dialog
                        .background
                        .and_then(|id| menus.iter().find(|(menu, _)| *menu == id))
                        .map_or_else(|| tl!("None (white)").to_owned(), |(_, name)| name.clone());
                    egui::ComboBox::from_id_salt("disc_label_background")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(dialog.background.is_none(), tl!("None (white)"))
                                .clicked()
                            {
                                dialog.background = None;
                            }
                            for (id, name) in &menus {
                                if ui
                                    .selectable_label(dialog.background == Some(*id), name)
                                    .clicked()
                                {
                                    dialog.background = Some(*id);
                                }
                            }
                        });
                    ui.end_row();

                    ui.label(tl!("Disc name:"));
                    ui.add(egui::TextEdit::singleline(&mut dialog.title).desired_width(300.0));
                    ui.end_row();

                    ui.label(tl!("Chapters:"));
                    ui.add_enabled(
                        !chapters.is_empty(),
                        egui::Checkbox::new(
                            &mut dialog.chapters,
                            tlf!("The chapter names ({count})", count = chapters.len()),
                        ),
                    )
                    .on_disabled_hover_text(tl!("No chapter has a name of its own"));
                    ui.end_row();
                });
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(crate::theme::dialog_button(tl!("Cancel"))).clicked() {
                    done = Some(false);
                }
                if ui.add(crate::theme::default_button(tl!("Create"))).clicked() {
                    done = Some(true);
                }
            });
        });
        if response.should_close() {
            done = Some(false);
        }
        if let Some(choice) = choice {
            // Opening the label is what the dialog was opened for: it closes.
            if choice == LabelChoice::Open {
                done = Some(false);
            }
            if let LabelChoice::Link(_) = &choice {
                dialog.found = None;
            }
            self.label_choice(choice);
        }
        match done {
            Some(true) => {
                self.disc_label = None;
                self.create_disc_label(&dialog, chapters);
            }
            Some(false) => self.disc_label = None,
            None => self.disc_label = Some(dialog),
        }
    }

    /// Writes the label of `dialog` beside the project and opens it in PhotoCraft.
    fn create_disc_label(&mut self, dialog: &DiscLabelDialog, chapters: Vec<String>) {
        let picture = dialog
            .background
            .and_then(|id| self.session.project.menu(id).cloned())
            .and_then(|menu| {
                let frame = disccraft_project::frame_size(&self.session.project.settings);
                let aspect = crate::menu_editor::menu_aspect(&menu, frame);
                let video = match &menu.background {
                    MenuBackground::Video { asset, .. } => {
                        self.first_video_frame_pub(*asset, aspect > 1.5)
                    }
                    _ => None,
                };
                disccraft_menu::menu_background_rgb(
                    &self.session.project,
                    &menu,
                    video
                        .as_ref()
                        .map(|(rgb, width, height)| (rgb.as_slice(), *width, *height)),
                )
                .ok()
                .map(|(rgb, width, height)| (rgb, usize::from(width), usize::from(height), aspect))
            });
        let label = DiscLabel {
            inner_mm: dialog.inner_mm,
            outer_mm: dialog.outer_mm,
            dpi: dialog.dpi,
            title: dialog.title.clone(),
            lines: if dialog.chapters {
                chapters
            } else {
                Vec::new()
            },
            picture,
            names: LabelNames {
                face: tl!("Disc Face").to_owned(),
                picture: tl!("Picture").to_owned(),
                guides: tl!("Guides (hidden, not printed)").to_owned(),
                lines: tl!("Chapters").to_owned(),
            },
        };
        let folder = match self
            .session
            .path
            .as_deref()
            .and_then(std::path::Path::parent)
        {
            Some(folder) => folder.to_path_buf(),
            None => match self.craft_folder() {
                Ok(folder) => folder,
                Err(error) => {
                    self.status = error;
                    return;
                }
            },
        };
        let stem = self
            .session
            .path
            .as_deref()
            .and_then(|path| path.file_stem())
            .map_or_else(
                || "DiscCraft".to_owned(),
                |stem| stem.to_string_lossy().into_owned(),
            );
        let base = crate::photocraft::file_name_for(&format!("{stem} {}", tl!("Disc Label")));
        let mut overset = false;
        let (path, app) = match &dialog.designcraft {
            Some(cli) => {
                let path = unique_path(&folder, &base, "designcraft");
                match disccraft_menu::label::designcraft::make(&label, cli, &path) {
                    Ok(made) => overset = made.chapters_overset,
                    Err(error) => {
                        self.status =
                            tlf!("Cannot make the disc label: {message}", message = error);
                        return;
                    }
                }
                (path, CraftApp::Design)
            }
            None => {
                let path = unique_path(&folder, &base, "psd");
                if let Err(error) = disccraft_menu::label::export_disc_label_psd(&label, &path) {
                    self.status = tlf!("Cannot make the disc label: {message}", message = error);
                    return;
                }
                (path, CraftApp::Photo)
            }
        };
        // M6.509: linked to the project (kept when it is saved).
        let linked = self.link_disc_label(&path);
        let opened = match find_app(app) {
            Some(exe) => std::process::Command::new(exe)
                .arg(&path)
                .spawn()
                .map(|_| ()),
            None => open_with_default_app(&path),
        };
        let note = match opened {
            Ok(()) => tlf!(
                "Disc label made: {path}. In {app}, print it at 100 % (not scaled to fit) on the printer's CD/DVD paper.",
                path = path.display(),
                app = app.name()
            ),
            Err(error) => tlf!(
                "Disc label made: {path} (it could not be opened: {message})",
                path = path.display(),
                message = error
            ),
        };
        let note = if linked {
            tlf!(
                "{note} It is linked to the project: save the project to keep it.",
                note = note
            )
        } else {
            note
        };
        self.status = if overset {
            tlf!(
                "{note} Some chapters did not fit even at the smallest size: shorten them there.",
                note = note
            )
        } else {
            note
        };
    }
}

#[cfg(test)]
mod tests {
    use super::{LabelChoice, label_app, label_beside};
    use crate::app::DiscCraftApp;
    use crate::photocraft::CraftApp;
    use std::error::Error;
    use std::path::Path;

    /// A fresh folder in the temp folder.
    fn temp_folder(name: &str) -> Result<std::path::PathBuf, Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!(
            "disccraft-label-ui-{}-{name}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    #[test]
    fn label_files_open_in_the_app_that_edits_them() {
        assert_eq!(
            label_app(Path::new("a.designcraft")),
            Some(CraftApp::Design)
        );
        assert_eq!(
            label_app(Path::new("a.DesignCraft")),
            Some(CraftApp::Design)
        );
        assert_eq!(label_app(Path::new("a.psd")), Some(CraftApp::Photo));
        assert_eq!(label_app(Path::new("a.PSB")), Some(CraftApp::Photo));
        assert_eq!(label_app(Path::new("a.pdf")), None);
        assert_eq!(label_app(Path::new("label")), None);
    }

    /// M6.509: a label made before labels were linked is found beside the project by the name Create Disc Label…
    /// gives it (in either language, numbered when there were several; the newest), and nothing else is.
    #[test]
    fn the_label_made_beside_the_project_is_found() -> Result<(), Box<dyn Error>> {
        let dir = temp_folder("beside")?;
        let project = dir.join("disc.dcproj");
        std::fs::write(&project, b"{}")?;
        assert_eq!(label_beside(&project), None);
        for other in [
            "disc Disc Labels.designcraft",
            "disc Disc Label x.psd",
            "other Disc Label.designcraft",
            "disc Disc Label.txt",
            "discDisc Label.designcraft",
        ] {
            std::fs::write(dir.join(other), b"not it")?;
        }
        std::fs::create_dir_all(dir.join("disc Disc Label 3.designcraft"))?;
        assert_eq!(label_beside(&project), None);
        let first = dir.join("disc Disc Label.designcraft");
        std::fs::write(&first, b"a label")?;
        assert_eq!(label_beside(&project), Some(first.clone()));
        // A newer one, numbered, in Japanese.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let newer = dir.join("disc ディスクのレーベル 2.psd");
        std::fs::write(&newer, b"a newer label")?;
        assert_eq!(label_beside(&project), Some(newer));
        // No project folder to look in.
        assert_eq!(label_beside(Path::new("")), None);
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    /// M6.509: the dialog offers the label found beside the project; linking it is a change to save (and saved
    /// with the project); unlinking keeps the file; a missing label is reported, not opened.
    #[test]
    fn the_dialog_links_and_unlinks_the_label() -> Result<(), Box<dyn Error>> {
        let dir = temp_folder("dialog")?;
        let project = dir.join("disc.dcproj");
        let label = dir.join("disc Disc Label.designcraft");
        std::fs::write(&label, b"a label")?;
        let mut app = DiscCraftApp::new(&egui::Context::default());
        app.session.save(Some(&project))?;
        app.open_disc_label();
        assert_eq!(
            app.disc_label
                .as_ref()
                .and_then(|dialog| dialog.found.clone()),
            Some(label.clone())
        );
        app.label_choice(LabelChoice::Link(label.clone()));
        assert_eq!(
            app.session
                .project
                .settings
                .disc_label
                .as_deref()
                .map(Path::new),
            Some(label.as_path())
        );
        assert!(app.session.dirty);
        app.session.save(None)?;
        let saved = std::fs::read_to_string(&project)?;
        assert!(
            saved.contains("\"relativeDiscLabel\": \"disc Disc Label.designcraft\""),
            "{saved}"
        );
        // Linked: nothing more is offered.
        app.open_disc_label();
        assert_eq!(
            app.disc_label
                .as_ref()
                .and_then(|dialog| dialog.found.clone()),
            None
        );

        // Gone: File ▸ Open Disc Label says so.
        std::fs::rename(&label, dir.join("moved away.designcraft"))?;
        app.open_disc_label_file();
        assert!(
            app.status.contains("disc Disc Label.designcraft"),
            "{}",
            app.status
        );

        app.label_choice(LabelChoice::Unlink);
        assert_eq!(app.session.project.settings.disc_label, None);
        assert!(dir.join("moved away.designcraft").is_file());
        app.open_disc_label_file();
        assert!(
            app.status.contains(crate::i18n::tl!("Create Disc Label")),
            "{}",
            app.status
        );
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
