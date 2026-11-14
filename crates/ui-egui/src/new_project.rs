//! New Project dialog (docs/ui-parity.md §2.1).
//!
//! The layout follows the classic authoring tools: a Basic and an Advanced tab; on the Basic tab a "Name and
//! Location" group and a "Project Settings" group with the authoring mode, the TV standard and a summary of the
//! transcode settings DiscCraft will use. Only text and standard widgets are used (no images or icons).

use std::path::{Path, PathBuf};

use crate::i18n::{tl, tlf};
use disccraft_project::{Capacity, DiscKind, VideoStandard};
use egui::text::{CCursor, CCursorRange};
use egui::{Align, Button, Layout, Ui};

/// Tabs of the New Project dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum NewProjectTab {
    /// Name, location, authoring mode, TV standard.
    #[default]
    Basic,
    /// Aspect ratio and disc capacity.
    Advanced,
}

/// Fields of the New Project dialog.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NewProjectForm {
    /// Project name.
    pub(crate) name: String,
    /// Folder where the project file is saved (empty: choose later with Save As).
    pub(crate) location: String,
    /// Folders of recent projects, offered in the location drop-down.
    pub(crate) history: Vec<String>,
    /// Authoring mode.
    pub(crate) kind: DiscKind,
    /// TV standard.
    pub(crate) standard: VideoStandard,
    /// 16:9 instead of 4:3.
    pub(crate) wide: bool,
    /// Disc capacity.
    pub(crate) capacity: Capacity,
    /// The tab in front.
    pub(crate) tab: NewProjectTab,
    /// Focus the name field and select its text on the next frame.
    pub(crate) focus_name: bool,
    /// Project Settings mode: the dialog edits the open project, so the name, location and TV standard are locked.
    pub(crate) settings: bool,
}

impl Default for NewProjectForm {
    fn default() -> Self {
        Self {
            name: tl!("Untitled").to_owned(),
            location: String::new(),
            history: Vec::new(),
            kind: DiscKind::Dvd,
            standard: VideoStandard::Ntsc,
            wide: false,
            capacity: Capacity::Dvd5,
            tab: NewProjectTab::Basic,
            focus_name: true,
            settings: false,
        }
    }
}

impl NewProjectForm {
    /// A form whose location is the folder of the most recent project (or the Documents folder), with the folders
    /// of recent projects as the location history.
    pub(crate) fn new(recent: &[PathBuf]) -> Self {
        let history = location_history(recent);
        let location = history
            .first()
            .cloned()
            .or_else(documents_dir)
            .unwrap_or_default();
        Self {
            location,
            history,
            standard: crate::workspace::load_default_standard(),
            ..Self::default()
        }
    }

    /// A Project Settings form for the open project (`path` is its file, if saved).
    pub(crate) fn for_project(
        path: Option<&Path>,
        settings: &disccraft_project::DiscSettings,
    ) -> Self {
        let name = path
            .and_then(Path::file_stem)
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| tl!("Untitled").to_owned());
        let location = path
            .and_then(Path::parent)
            .map(|folder| folder.display().to_string())
            .unwrap_or_default();
        Self {
            name,
            location,
            history: Vec::new(),
            kind: settings.kind,
            standard: settings.standard,
            wide: settings.widescreen,
            capacity: settings.capacity,
            tab: NewProjectTab::Basic,
            focus_name: false,
            settings: true,
        }
    }

    /// The project file path, `location/name.dcproj`, or `None` when the location is empty. The name is trimmed;
    /// characters not allowed in Windows file names (`<>:"/\|?*`) are replaced with `_`.
    pub(crate) fn project_path(&self) -> Option<PathBuf> {
        let location = self.location.trim();
        if location.is_empty() {
            return None;
        }

        let name = sanitize_file_name(self.name.trim());
        Some(PathBuf::from(location).join(format!("{name}.dcproj")))
    }

    /// Whether OK may be pressed: the trimmed name is not empty.
    pub(crate) fn is_valid(&self) -> bool {
        !self.name.trim().is_empty()
    }

    /// Keep the capacity consistent with the authoring mode (DVD5/DVD9 for DVD, BD25 to BDXL for Blu-ray).
    pub(crate) fn fit_capacity(&mut self) {
        let fits = Capacity::choices(self.kind).contains(&self.capacity);
        if !fits {
            self.capacity = match self.kind {
                DiscKind::Dvd => Capacity::Dvd5,
                DiscKind::BluRay => Capacity::Bd25,
            };
        }
    }
}

/// What the dialog produced this frame.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FormAction {
    /// OK was pressed.
    Ok,
    /// Cancel was pressed.
    Cancel,
}

/// Height of the tab area (fits the Basic tab).
const TAB_HEIGHT: f32 = 212.0;

/// Draw the dialog contents. Returns OK or Cancel when pressed.
pub(crate) fn draw_new_project(ui: &mut Ui, form: &mut NewProjectForm) -> Option<FormAction> {
    ui.heading(if form.settings {
        tl!("Project Settings")
    } else {
        tl!("New Project")
    });
    ui.horizontal(|ui| {
        for (tab, label) in [
            (NewProjectTab::Basic, tl!("Basic")),
            (NewProjectTab::Advanced, tl!("Advanced")),
        ] {
            if ui.selectable_label(form.tab == tab, label).clicked() {
                form.tab = tab;
            }
        }
    });
    ui.separator();

    // Both tabs take the same height so the dialog does not change size when switching tabs.
    let size = egui::vec2(ui.available_width(), TAB_HEIGHT);
    ui.allocate_ui_with_layout(size, Layout::top_down(Align::Min), |ui| {
        ui.set_min_size(size);
        match form.tab {
            NewProjectTab::Basic => {
                draw_name_group(ui, form);
                ui.add_space(4.0);
                draw_settings_group(ui, form);
            }
            NewProjectTab::Advanced => draw_advanced_group(ui, form),
        }
    });

    ui.add_space(6.0);
    ui.label(tl!(
        "You can change the authoring mode and TV standard later in the disc properties."
    ));
    // Always take the line, so the buttons stay in place while the location is typed.
    let note = if form.location.trim().is_empty() && !form.settings {
        tl!("Leave the location empty to choose where to save later.")
    } else {
        " "
    };
    ui.small(note);
    ui.add_space(6.0);

    draw_buttons(ui, form)
}

/// The "Name and Location" group.
fn draw_name_group(ui: &mut Ui, form: &mut NewProjectForm) {
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        ui.label(tl!("Name and Location"));
        egui::Grid::new("new_project_name")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label(tl!("Name:"));
                ui.add_enabled_ui(!form.settings, |ui| draw_name_field(ui, form));
                ui.end_row();

                ui.label(tl!("Location:"));
                ui.add_enabled_ui(!form.settings, |ui| draw_location_field(ui, form));
                ui.end_row();
            });
    });
}

/// The name field; on the first frame it takes the focus with the whole name selected, ready to be typed over.
fn draw_name_field(ui: &mut Ui, form: &mut NewProjectForm) {
    let output = egui::TextEdit::singleline(&mut form.name)
        .desired_width(240.0)
        .show(ui);

    if form.focus_name {
        form.focus_name = false;
        output.response.request_focus();
        let mut state = output.state;
        let end = form.name.chars().count();
        state
            .cursor
            .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(end))));
        state.store(ui.ctx(), output.response.id);
    }
}

/// The location field with a drop-down of recent folders and a Browse button.
fn draw_location_field(ui: &mut Ui, form: &mut NewProjectForm) {
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut form.location).desired_width(216.0));

        let mut picked = None;
        egui::ComboBox::from_id_salt("new_project_history")
            .selected_text("")
            .width(16.0)
            .show_ui(ui, |ui| {
                if form.history.is_empty() {
                    ui.label(tl!("No recent folders"));
                }
                for folder in &form.history {
                    if ui
                        .selectable_label(*folder == form.location, folder.as_str())
                        .clicked()
                    {
                        picked = Some(folder.clone());
                    }
                }
            });
        if let Some(folder) = picked {
            form.location = folder;
        }

        if ui.button(tl!("Browse…")).clicked() {
            browse_location(form);
        }
    });
}

/// The "Project Settings" group: authoring mode, TV standard and the transcode summary.
fn draw_settings_group(ui: &mut Ui, form: &mut NewProjectForm) {
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        ui.label(tl!("Project Settings"));
        egui::Grid::new("new_project_settings")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label(tl!("Authoring mode:"));
                ui.horizontal(|ui| {
                    let before = form.kind;
                    ui.radio_value(&mut form.kind, DiscKind::BluRay, "Blu-ray");
                    ui.radio_value(&mut form.kind, DiscKind::Dvd, "DVD");
                    if form.kind != before {
                        form.fit_capacity();
                    }
                });
                ui.end_row();

                ui.label(tl!("TV standard:"));
                ui.add_enabled_ui(!form.settings, |ui| draw_standard_combo(ui, form));
                ui.end_row();

                ui.label(tl!("Transcode settings:"));
                ui.vertical(|ui| {
                    for line in transcode_summary(form.kind, form.standard) {
                        ui.small(line);
                    }
                });
                ui.end_row();
            });
    });
}

fn draw_standard_combo(ui: &mut Ui, form: &mut NewProjectForm) {
    egui::ComboBox::from_id_salt("new_project_standard")
        .selected_text(standard_label(form.standard))
        .show_ui(ui, |ui| {
            for standard in [VideoStandard::Ntsc, VideoStandard::Pal] {
                ui.selectable_value(&mut form.standard, standard, standard_label(standard));
            }
        });
}

/// The Advanced tab: aspect ratio and disc capacity.
fn draw_advanced_group(ui: &mut Ui, form: &mut NewProjectForm) {
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        ui.label(tl!("Disc"));
        egui::Grid::new("new_project_advanced")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label(tl!("Aspect ratio:"));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut form.wide, false, "4:3");
                    ui.radio_value(&mut form.wide, true, "16:9");
                });
                ui.end_row();

                ui.label(tl!("Disc capacity:"));
                let choices = Capacity::choices(form.kind);
                // Blu-ray has four sizes (BDXL included), so they wrap.
                ui.horizontal_wrapped(|ui| {
                    for capacity in choices.iter().copied() {
                        let label = capacity_label(&capacity);
                        ui.radio_value(&mut form.capacity, capacity, label);
                    }
                });
                ui.end_row();
            });
    });
}

/// OK and Cancel, right-aligned in that order.
fn draw_buttons(ui: &mut Ui, form: &NewProjectForm) -> Option<FormAction> {
    let mut action = None;
    // A one-row strip: a right-to-left layout on its own would take (and centre within) all the remaining height.
    let size = egui::vec2(ui.available_width(), 24.0);
    ui.allocate_ui_with_layout(size, Layout::right_to_left(Align::Center), |ui| {
        // Right-to-left: Cancel is added first so that OK ends up on its left.
        if ui
            .add_sized([80.0, 22.0], Button::new(tl!("Cancel")))
            .clicked()
        {
            action = Some(FormAction::Cancel);
        }
        if ui
            .add_enabled(
                form.is_valid(),
                Button::new(tl!("OK")).min_size([80.0, 22.0].into()),
            )
            .clicked()
        {
            action = Some(FormAction::Ok);
        }
    });
    action
}

/// Lines describing the transcode settings DiscCraft uses for this mode and standard. They follow the actual
/// pipeline: MPEG-2 progressive frames (`mpeg2venc`), AC-3 192 kbps stereo for transcoded audio and the bitrate budget
/// that fills the disc with video up to 9.8 Mbps (`dvd::prepare`). Blu-ray currently reuses the same SD video.
pub(crate) fn transcode_summary(kind: DiscKind, standard: VideoStandard) -> Vec<String> {
    let (width, height, rate) = match standard {
        VideoStandard::Ntsc => (720, 480, "29.97"),
        VideoStandard::Pal => (720, 576, "25"),
    };
    let mut lines = vec![
        tlf!(
            "Video: MPEG-2, {width} x {height}, {rate} fps, progressive",
            width = width,
            height = height,
            rate = rate
        ),
        tl!("Audio: Dolby Digital (AC-3) 192 kbps stereo (AC-3 and WAV sources are kept)")
            .to_owned(),
        tl!("Bitrate: automatic, to fit the disc (video up to 9.8 Mbps)").to_owned(),
    ];
    if kind == DiscKind::BluRay {
        lines.push(tl!("Blu-ray currently uses the same standard-definition video.").to_owned());
    }
    lines
}

/// Folders of recent projects, newest first, without duplicates.
pub(crate) fn location_history(recent: &[PathBuf]) -> Vec<String> {
    let mut folders: Vec<String> = Vec::new();
    for path in recent {
        let Some(parent) = path.parent() else {
            continue;
        };
        let folder = parent.display().to_string();
        if !folder.is_empty() && !folders.contains(&folder) {
            folders.push(folder);
        }
    }
    folders
}

/// The user's Documents folder, if it can be found.
fn documents_dir() -> Option<String> {
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE");
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME");

    let home = PathBuf::from(home?);
    let documents = home.join("Documents");
    let folder = if documents.is_dir() { documents } else { home };
    Some(folder.display().to_string())
}

/// Open a folder picker and store the chosen folder in the form.
fn browse_location(form: &mut NewProjectForm) {
    let mut dialog = rfd::FileDialog::new();
    let location = form.location.trim().to_owned();

    if !location.is_empty() {
        dialog = dialog.set_directory(Path::new(&location));
    }

    if let Some(folder) = dialog.pick_folder() {
        form.location = folder.display().to_string();
    }
}

/// Replace characters not allowed in Windows file names with underscores.
fn sanitize_file_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => c,
        })
        .collect()
}

fn standard_label(standard: VideoStandard) -> &'static str {
    match standard {
        VideoStandard::Ntsc => "NTSC",
        VideoStandard::Pal => "PAL",
    }
}

fn capacity_label(capacity: &Capacity) -> &'static str {
    match capacity {
        Capacity::Dvd5 => "DVD5 (4.7 GB)",
        Capacity::Dvd9 => "DVD9 (8.5 GB)",
        Capacity::Bd25 => "BD25 (25 GB)",
        Capacity::Bd50 => "BD50 (50 GB)",
        Capacity::Bd100 => tl!("BD100 (100 GB, BDXL 3 layers)"),
        Capacity::Bd128 => tl!("BD128 (128 GB, BDXL 4 layers)"),
        Capacity::Custom { .. } => "",
    }
}

#[cfg(test)]
mod tests {
    use super::{NewProjectForm, location_history, transcode_summary};
    use disccraft_project::{Capacity, DiscKind, VideoStandard};
    use std::path::PathBuf;

    fn file_name(form: &NewProjectForm) -> Option<String> {
        form.project_path().and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
    }

    #[test]
    fn project_path_uses_the_trimmed_safe_name() {
        let form = NewProjectForm {
            name: "  a/b:c  ".to_owned(),
            location: "C:/work".to_owned(),
            ..NewProjectForm::default()
        };
        assert_eq!(file_name(&form).as_deref(), Some("a_b_c.dcproj"));
        assert_eq!(
            form.project_path()
                .and_then(|path| path.parent().map(PathBuf::from)),
            Some(PathBuf::from("C:/work"))
        );
    }

    #[test]
    fn empty_location_means_save_later() {
        let form = NewProjectForm::default();
        assert!(form.project_path().is_none());
    }

    #[test]
    fn name_must_not_be_blank() {
        let mut form = NewProjectForm::default();
        assert!(form.is_valid());
        form.name = "   ".to_owned();
        assert!(!form.is_valid());
    }

    #[test]
    fn new_form_starts_in_the_latest_project_folder() {
        let recent = vec![
            PathBuf::from("C:/discs/b/b.dcproj"),
            PathBuf::from("C:/discs/a/a.dcproj"),
            PathBuf::from("C:/discs/b/other.dcproj"),
        ];
        let history = location_history(&recent);
        assert_eq!(history.len(), 2);
        let form = NewProjectForm::new(&recent);
        assert_eq!(Some(&form.location), history.first());
        assert!(form.focus_name);
    }

    #[test]
    fn capacity_follows_the_authoring_mode() {
        let mut form = NewProjectForm {
            kind: DiscKind::BluRay,
            ..NewProjectForm::default()
        };
        form.fit_capacity();
        assert_eq!(form.capacity, Capacity::Bd25);

        form.capacity = Capacity::Bd50;
        form.fit_capacity();
        assert_eq!(form.capacity, Capacity::Bd50);

        form.kind = DiscKind::Dvd;
        form.fit_capacity();
        assert_eq!(form.capacity, Capacity::Dvd5);
    }

    #[test]
    fn summary_matches_the_standard() {
        crate::i18n::set_current(crate::i18n::Language::En);
        let ntsc = transcode_summary(DiscKind::Dvd, VideoStandard::Ntsc);
        let pal = transcode_summary(DiscKind::BluRay, VideoStandard::Pal);
        crate::i18n::set_current(crate::i18n::Language::Ja);

        assert!(ntsc.first().is_some_and(|line| line.contains("720 x 480")));
        assert!(pal.first().is_some_and(|line| line.contains("720 x 576")));
        assert_eq!(ntsc.len(), 3);
        assert_eq!(pal.len(), 4);
    }
}
