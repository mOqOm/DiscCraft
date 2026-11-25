//! New Project dialog (docs/ui-parity.md §2.1).
//!
//! The layout follows the classic authoring tools: a Basic and an Advanced tab; on the Basic tab a "Name and
//! Location" group and a "Project Settings" group with the authoring mode, the TV standard and a summary of the
//! transcode settings DiscCraft will use. Only text and standard widgets are used (no images or icons).

use std::path::{Path, PathBuf};

use crate::i18n::{tl, tlf};
use disccraft_project::{BdCodec, BdVideo, Capacity, DiscKind, VideoStandard};
use egui::text::{CCursor, CCursorRange};
use egui::{Align, Layout, Ui};

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
    /// A Blu-ray's video format (M6.357: HD unless the user picks SD).
    pub(crate) bd_video: BdVideo,
    /// A Blu-ray's codec: the open project's in Project Settings; `None` for a new project, whose is
    /// [`default_bd_codec`].
    pub(crate) bd_codec: Option<BdCodec>,
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
            bd_video: default_bd_video(VideoStandard::Ntsc),
            bd_codec: None,
            tab: NewProjectTab::Basic,
            focus_name: true,
            settings: false,
        }
    }
}

/// The Blu-ray video format a new project starts with (M6.357): HD, as Encore's New Project dialog does. On an NTSC
/// disc 1080p 23.976 (H.264), which most films and their files are; on a PAL disc 1080i 50 (MPEG-2), the
/// format that plays 25 fps sources at their own speed. A Blu-ray had been SD until it was changed, so an HD file
/// was converted down to SD unless the user found the setting.
pub(crate) fn default_bd_video(standard: VideoStandard) -> BdVideo {
    match standard {
        VideoStandard::Ntsc => BdVideo::Hd1080p23,
        VideoStandard::Pal => BdVideo::Hd1080i50,
    }
}

/// The Blu-ray codec a new project starts with: Encore's New Project dialog starts a PAL Blu-ray in H.264, 1920 x
/// 1080 at 25 with the odd field first (seen 2026-11-21, M10.43), so the PAL default format is H.264 (coded as
/// interlaced frames); anything else is automatic (an NTSC disc's 1080p 23.976 is H.264 automatically).
pub(crate) fn default_bd_codec(standard: VideoStandard, video: BdVideo) -> BdCodec {
    if standard == VideoStandard::Pal && video == default_bd_video(standard) {
        BdCodec::H264
    } else {
        BdCodec::Auto
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
        let standard = crate::workspace::load_default_standard();
        Self {
            location,
            history,
            standard,
            bd_video: default_bd_video(standard),
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
            bd_video: settings.bd_video,
            bd_codec: Some(settings.bd_codec),
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

/// Height of the tab area (fits the Basic tab, with a Blu-ray's video row).
const TAB_HEIGHT: f32 = 252.0;

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

        if ui
            .add(crate::theme::dialog_button(tl!("Browse…")))
            .clicked()
        {
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

                // A Blu-ray's video: HD from the start, SD only when picked (M6.357).
                if form.kind == DiscKind::BluRay {
                    ui.label(tl!("Blu-ray video:"));
                    egui::ComboBox::from_id_salt("new_project_bd_video")
                        .selected_text(crate::properties::bd_video_name(form.bd_video))
                        .show_ui(ui, |ui| {
                            for video in BdVideo::ALL {
                                ui.selectable_value(
                                    &mut form.bd_video,
                                    video,
                                    crate::properties::bd_video_name(video),
                                );
                            }
                        });
                    ui.end_row();
                }

                ui.label(tl!("Transcode settings:"));
                ui.vertical(|ui| {
                    for line in transcode_summary(
                        form.kind,
                        form.standard,
                        (form.bd_video, form.bd_codec()),
                    ) {
                        ui.small(line);
                    }
                });
                ui.end_row();
            });
    });
}

fn draw_standard_combo(ui: &mut Ui, form: &mut NewProjectForm) {
    let before = form.standard;
    egui::ComboBox::from_id_salt("new_project_standard")
        .selected_text(standard_label(form.standard))
        .show_ui(ui, |ui| {
            for standard in [VideoStandard::Ntsc, VideoStandard::Pal] {
                ui.selectable_value(&mut form.standard, standard, standard_label(standard));
            }
        });
    form.follow_standard(before);
}

impl NewProjectForm {
    /// After the TV standard changed from `before`: a Blu-ray video format still at the old standard's default
    /// becomes the new one's (one the user picked stays).
    /// The Blu-ray codec: the open project's, or a new one's ([`default_bd_codec`]).
    pub(crate) fn bd_codec(&self) -> BdCodec {
        self.bd_codec
            .unwrap_or_else(|| default_bd_codec(self.standard, self.bd_video))
    }

    pub(crate) fn follow_standard(&mut self, before: VideoStandard) {
        if self.standard != before && self.bd_video == default_bd_video(before) {
            self.bd_video = default_bd_video(self.standard);
        }
    }
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
            .add_sized([80.0, 22.0], crate::theme::dialog_button(tl!("Cancel")))
            .clicked()
        {
            action = Some(FormAction::Cancel);
        }
        if ui
            .add_enabled(
                form.is_valid(),
                crate::theme::default_button(tl!("OK")).min_size([80.0, 22.0].into()),
            )
            .clicked()
        {
            action = Some(FormAction::Ok);
        }
    });
    action
}

/// Lines describing the transcode settings DiscCraft uses for this mode, standard and (on a Blu-ray) video format.
/// They follow the actual pipeline: SD MPEG-2 progressive frames (`mpeg2venc`) in the bitrate budget that fills the
/// disc with video up to 9.8 Mbps (`dvd::prepare`); HD H.264 or 1080i MPEG-2 at the format's usual rates, lower when
/// the titles do not fit (`dvd::hd_budget`, through `disccraft_build::hd_default_kbps`), and a file whose video already fits the format kept as it is; AC-3
/// 192 kbps stereo for transcoded audio.
pub(crate) fn transcode_summary(
    kind: DiscKind,
    standard: VideoStandard,
    (bd_video, codec): (BdVideo, BdCodec),
) -> Vec<String> {
    let audio = tl!("Audio: Dolby Digital (AC-3) 192 kbps stereo (AC-3 and WAV sources are kept)")
        .to_owned();
    if let (DiscKind::BluRay, Some((_, height, num, den)), Some(width)) =
        (kind, bd_video.hd(), bd_video.video_width())
    {
        let (codec, (target, peak)) = if codec.mpeg2(bd_video) {
            ("MPEG-2", disccraft_build::hd_default_kbps(true))
        } else {
            ("H.264", disccraft_build::hd_default_kbps(false))
        };
        let rate = format!("{:.3}", f64::from(num) / f64::from(den.max(1)))
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned();
        return vec![
            tlf!(
                "Video: {codec}, {width} x {height}, {rate} fps (files that fit go on as they are)",
                codec = codec,
                width = width,
                height = height,
                rate = rate
            ),
            audio,
            tlf!(
                "Bitrate: {target} Mbps on average, up to {peak} (more when the disc has room, less when it is short)",
                target = target / 1000,
                peak = peak / 1000
            ),
        ];
    }
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
        audio,
        tl!("Bitrate: automatic, to fit the disc (video up to 9.8 Mbps)").to_owned(),
    ];
    if kind == DiscKind::BluRay {
        lines.push(tl!("Standard definition: HD sources are converted down to SD.").to_owned());
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
    use super::{NewProjectForm, default_bd_video, location_history, transcode_summary};
    use disccraft_project::{BdVideo, Capacity, DiscKind, VideoStandard};
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
        let ntsc = transcode_summary(
            DiscKind::Dvd,
            VideoStandard::Ntsc,
            (
                BdVideo::Sd,
                super::default_bd_codec(VideoStandard::Ntsc, BdVideo::Sd),
            ),
        );
        let pal = transcode_summary(
            DiscKind::BluRay,
            VideoStandard::Pal,
            (
                BdVideo::Sd,
                super::default_bd_codec(VideoStandard::Pal, BdVideo::Sd),
            ),
        );
        let hd = transcode_summary(
            DiscKind::BluRay,
            VideoStandard::Ntsc,
            (
                BdVideo::Hd1080p23,
                super::default_bd_codec(VideoStandard::Ntsc, BdVideo::Hd1080p23),
            ),
        );
        let interlaced = transcode_summary(
            DiscKind::BluRay,
            VideoStandard::Pal,
            (
                BdVideo::Hd1080i50,
                super::default_bd_codec(VideoStandard::Pal, BdVideo::Hd1080i50),
            ),
        );
        crate::i18n::set_current(crate::i18n::Language::Ja);

        assert!(ntsc.first().is_some_and(|line| line.contains("720 x 480")));
        assert!(pal.first().is_some_and(|line| line.contains("720 x 576")));
        assert_eq!(ntsc.len(), 3);
        assert_eq!(pal.len(), 4);
        // M6.357: an HD Blu-ray says its own video and rates.
        assert!(
            hd.first()
                .is_some_and(|line| line.contains("H.264, 1920 x 1080, 23.976 fps")),
            "{hd:?}"
        );
        assert!(
            hd.get(2).is_some_and(|line| line.contains("12 Mbps")),
            "{hd:?}"
        );
        assert!(
            interlaced
                .first()
                .is_some_and(|line| line.contains("H.264, 1920 x 1080, 25 fps")),
            "{interlaced:?}"
        );
    }

    /// M6.357: a new Blu-ray project is HD (by the TV standard) and follows a change of the standard until the user
    /// picks a format; a project's settings show its own format.
    #[test]
    fn a_new_blu_ray_is_hd() {
        let mut form = NewProjectForm::default();
        assert_eq!(form.bd_video, BdVideo::Hd1080p23);
        form.standard = VideoStandard::Pal;
        form.follow_standard(VideoStandard::Ntsc);
        assert_eq!(form.bd_video, BdVideo::Hd1080i50);
        assert_eq!(default_bd_video(VideoStandard::Pal), BdVideo::Hd1080i50);
        // M10.43: as Encore's, a PAL disc's 1080i 25 is H.264; the rest automatic.
        assert_eq!(form.bd_codec(), disccraft_project::BdCodec::H264);
        assert_eq!(
            super::default_bd_codec(VideoStandard::Ntsc, BdVideo::Hd1080p23),
            disccraft_project::BdCodec::Auto
        );
        assert_eq!(
            super::default_bd_codec(VideoStandard::Pal, BdVideo::Sd),
            disccraft_project::BdCodec::Auto
        );
        // A format the user picked stays.
        form.bd_video = BdVideo::Hd720p50;
        form.standard = VideoStandard::Ntsc;
        form.follow_standard(VideoStandard::Pal);
        assert_eq!(form.bd_video, BdVideo::Hd720p50);
        let settings = disccraft_project::DiscSettings {
            bd_video: BdVideo::Sd,
            ..Default::default()
        };
        assert_eq!(
            NewProjectForm::for_project(None, &settings).bd_video,
            BdVideo::Sd
        );
    }
}
