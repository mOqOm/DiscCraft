//! Start screen (docs/ui-parity.md §2.1). The New Project dialog it opens lives in [`crate::new_project`].

use std::path::{Path, PathBuf};

use crate::i18n::tl;
use crate::new_project::{FormAction, NewProjectForm, draw_new_project};
use egui::{
    Align, Align2, Button, Context, FontId, Id, Layout, Modal, ScrollArea, Sense, Ui, vec2,
};

/// State of the start screen and the New Project dialog.
#[derive(Clone, Debug, Default)]
pub(crate) struct Welcome {
    /// The start dialog is shown.
    pub(crate) open: bool,
    /// The New Project dialog is shown with this form.
    pub(crate) new_project: Option<NewProjectForm>,
}

/// What the user chose this frame.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum WelcomeChoice {
    /// OK in the New Project dialog.
    NewProject(Box<NewProjectForm>),
    /// OK in the Project Settings dialog.
    ProjectSettings(Box<NewProjectForm>),
    /// "Open Project…" (the app shows its file dialog).
    Open,
    /// A recent project was clicked.
    OpenRecent(PathBuf),
    /// "Remove from List" on a recent project (the start dialog stays open).
    Forget(PathBuf),
    /// "Exit".
    Exit,
}

impl Welcome {
    /// A welcome state that is open (used at startup).
    pub(crate) fn opened() -> Self {
        Self {
            open: true,
            ..Default::default()
        }
    }

    /// Open the Project Settings dialog for the open project.
    pub(crate) fn start_project_settings(&mut self, form: NewProjectForm) {
        self.new_project = Some(form);
    }

    /// Open the New Project dialog (also used by File ▸ New ▸ Project).
    pub(crate) fn start_new_project(&mut self, recent: &[PathBuf]) {
        self.new_project = Some(NewProjectForm::new(recent));
    }

    /// Draw whichever dialog is open and return the user's choice. The New Project dialog, when open, is drawn
    /// instead of the start dialog. Cancel or Esc in the New Project dialog goes back to the start dialog if it was
    /// open; choosing anything closes both dialogs.
    pub(crate) fn show(&mut self, ctx: &Context, recent: &[PathBuf]) -> Option<WelcomeChoice> {
        if self.new_project.is_some() {
            return self.show_new_project(ctx);
        }

        if self.open {
            return self.show_start(ctx, recent);
        }

        None
    }

    /// Draw the start dialog.
    fn show_start(&mut self, ctx: &Context, recent: &[PathBuf]) -> Option<WelcomeChoice> {
        let response = Modal::new(Id::unique("welcome")).show(ctx, |ui| {
            ui.set_width(DIALOG_WIDTH);
            draw_start_header(ui);
            ui.separator();
            ui.label(tl!("Recent Projects"));

            if let Some(choice) = draw_recent(ui, recent) {
                return Some(choice);
            }

            ui.add_space(8.0);

            if let Some(choice) = draw_start_actions(ui, self, recent) {
                return Some(choice);
            }

            ui.separator();
            draw_exit(ui)
        });

        if response.should_close() {
            self.open = false;
            return None;
        }

        // Taking a project off the list keeps the start dialog.
        if matches!(response.inner, Some(WelcomeChoice::Forget(_))) {
            return response.inner;
        }

        if response.inner.is_some() {
            self.open = false;
            self.new_project = None;
        }

        response.inner
    }

    /// Draw the New Project dialog.
    fn show_new_project(&mut self, ctx: &Context) -> Option<WelcomeChoice> {
        let response = Modal::new(Id::unique("new_project")).show(ctx, |ui| {
            ui.set_width(500.0);
            self.new_project
                .as_mut()
                .and_then(|form| draw_new_project(ui, form))
        });

        match response.inner {
            Some(FormAction::Ok) => {
                let form = self.new_project.take()?;
                if form.settings {
                    return Some(WelcomeChoice::ProjectSettings(Box::new(form)));
                }
                self.open = false;
                Some(WelcomeChoice::NewProject(Box::new(form)))
            }
            Some(FormAction::Cancel) => {
                self.new_project = None;
                None
            }
            None => {
                if response.should_close() {
                    self.new_project = None;
                }
                None
            }
        }
    }
}

/// Draw the product name and version header.
fn draw_start_header(ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.heading("DiscCraft");
        ui.small(env!("CARGO_PKG_VERSION"));
    });
}

/// Width of the start dialog.
const DIALOG_WIDTH: f32 = 560.0;
/// Height of one recent project row (name and date on the first line, folder on the second).
const ROW_HEIGHT: f32 = 42.0;
/// Rows shown before the list scrolls.
const VISIBLE_ROWS: f32 = 6.0;
/// Longest folder text in a row, in characters; longer folders keep their last folders after "…".
const FOLDER_CHARS: usize = 60;

/// A recent project as the list shows it.
#[derive(Clone, Debug, PartialEq)]
struct RecentEntry {
    /// The project's name (its file name without `.dcproj`).
    name: String,
    /// The folder it is in, shortened to fit.
    folder: String,
    /// When the file last changed, on this computer's clock; `None` when the file is not there.
    modified: Option<String>,
}

impl RecentEntry {
    fn new(path: &Path) -> Self {
        let modified = std::fs::metadata(path)
            .ok()
            .filter(std::fs::Metadata::is_file)
            .map(|meta| {
                meta.modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .and_then(|since| i64::try_from(since.as_secs()).ok())
                    .map(crate::project_panel::modified_text)
                    .unwrap_or_default()
            });
        Self {
            name: recent_label(path),
            folder: path
                .parent()
                .map(|folder| short_folder(folder, FOLDER_CHARS))
                .unwrap_or_default(),
            modified,
        }
    }
}

/// `folder` in at most `limit` characters: whole, or "…" and as many of its last folders as fit (at least the
/// last one), so projects of the same name are told apart by where they are.
fn short_folder(folder: &Path, limit: usize) -> String {
    let whole = folder.display().to_string();
    if whole.chars().count() <= limit {
        return whole;
    }
    let separator = std::path::MAIN_SEPARATOR.to_string();
    let parts: Vec<String> = folder
        .components()
        .filter_map(|part| match part {
            std::path::Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let mut kept: Vec<&str> = Vec::new();
    let mut length = 1usize; // "…"
    for part in parts.iter().rev() {
        let added = part.chars().count() + separator.chars().count();
        if !kept.is_empty() && length + added > limit {
            break;
        }
        kept.push(part);
        length += added;
    }
    kept.reverse();
    format!("…{separator}{}", kept.join(&separator))
}

/// Draw the recent projects list and return a choice when one is clicked, or removed with its context menu.
fn draw_recent(ui: &mut Ui, recent: &[PathBuf]) -> Option<WelcomeChoice> {
    if recent.is_empty() {
        ui.label(tl!("No recent projects"));
        return None;
    }

    let mut choice = None;
    let height = ROW_HEIGHT * VISIBLE_ROWS.min(recent.len() as f32);

    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .inner_margin(2)
        .show(ui, |ui| {
            ScrollArea::vertical()
                .max_height(height)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    // Rows touch, so the list shows whole rows until it scrolls.
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for path in recent {
                        if let Some(picked) = recent_row(ui, path) {
                            choice = Some(picked);
                        }
                    }
                });
        });

    choice
}

/// One row of the recent list: the name and when it last changed, the folder below. A project that is no longer
/// there is dimmed and cannot be opened; any can be taken off the list from its context menu.
fn recent_row(ui: &mut Ui, path: &Path) -> Option<WelcomeChoice> {
    let entry = RecentEntry::new(path);
    let exists = entry.modified.is_some();
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    let response = response.on_hover_text(path.display().to_string());

    let visuals = ui.visuals();
    if response.hovered() && exists {
        ui.painter()
            .rect_filled(rect, 3.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let strong = if exists {
        visuals.strong_text_color()
    } else {
        visuals.weak_text_color()
    };
    let weak = visuals.weak_text_color();
    let left = rect.left() + 10.0;
    let right = rect.right() - 10.0;
    let first = rect.top() + 12.0;
    let second = rect.top() + 29.0;
    let painter = ui.painter();
    painter.text(
        egui::pos2(left, first),
        Align2::LEFT_CENTER,
        &entry.name,
        FontId::proportional(13.5),
        strong,
    );
    let when = entry
        .modified
        .clone()
        .unwrap_or_else(|| tl!("Not found").to_owned());
    painter.text(
        egui::pos2(right, first),
        Align2::RIGHT_CENTER,
        when,
        FontId::proportional(11.5),
        weak,
    );
    painter.text(
        egui::pos2(left, second),
        Align2::LEFT_CENTER,
        &entry.folder,
        FontId::proportional(11.0),
        weak,
    );

    let mut choice = None;
    response.context_menu(|ui| {
        if ui.button(tl!("Remove from List")).clicked() {
            choice = Some(WelcomeChoice::Forget(path.to_path_buf()));
            ui.close();
        }
    });
    if response.clicked() && exists {
        choice = Some(WelcomeChoice::OpenRecent(path.to_path_buf()));
    }
    choice
}

/// Draw the two main start-screen buttons.
fn draw_start_actions(
    ui: &mut Ui,
    welcome: &mut Welcome,
    recent: &[PathBuf],
) -> Option<WelcomeChoice> {
    let mut choice = None;

    ui.horizontal(|ui| {
        if ui
            .add_sized(vec2(160.0, 48.0), Button::new(tl!("New Project")))
            .clicked()
        {
            welcome.start_new_project(recent);
        }

        if ui
            .add_sized(vec2(160.0, 48.0), Button::new(tl!("Open Project…")))
            .clicked()
        {
            choice = Some(WelcomeChoice::Open);
        }
    });

    choice
}

/// Draw the Exit button and return a choice when it is clicked.
fn draw_exit(ui: &mut Ui) -> Option<WelcomeChoice> {
    let mut choice = None;

    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ui.button(tl!("Exit")).clicked() {
            choice = Some(WelcomeChoice::Exit);
        }
    });

    choice
}

/// Return a short label for a recent project path.
fn recent_label(path: &Path) -> String {
    match path.file_stem() {
        Some(stem) => stem.to_string_lossy().into_owned(),
        None => path.display().to_string(),
    }
}

/// Put `path` at the front of `list`, removing an equal entry, and keep at most `limit` entries.
pub(crate) fn add_recent(list: &mut Vec<PathBuf>, path: &Path, limit: usize) {
    let path = path.to_path_buf();
    list.retain(|existing| existing != &path);
    list.insert(0, path);
    list.truncate(limit);
}

#[cfg(test)]
mod tests {
    use super::{Welcome, add_recent};
    use std::path::{Path, PathBuf};

    #[test]
    fn add_recent_moves_to_front_and_truncates() {
        let mut list = vec![PathBuf::from("b"), PathBuf::from("a")];
        add_recent(&mut list, Path::new("a"), 10);
        assert_eq!(list, vec![PathBuf::from("a"), PathBuf::from("b")]);

        let mut list = vec![PathBuf::from("a"), PathBuf::from("b")];
        add_recent(&mut list, Path::new("c"), 2);
        assert_eq!(list, vec![PathBuf::from("c"), PathBuf::from("a")]);
    }

    #[test]
    fn long_folders_keep_their_last_folders() {
        use super::short_folder;
        let sep = std::path::MAIN_SEPARATOR;
        let short = Path::new("C:/discs");
        assert_eq!(short_folder(short, 60), short.display().to_string());

        let long: PathBuf = ["C:", "work", "DiscCraft", "target", "demo", "bluray-wide"]
            .iter()
            .collect();
        // "…/target/demo/bluray-wide" is 25 characters; one fewer drops "target".
        let shown = short_folder(&long, 25);
        assert_eq!(shown, format!("…{sep}target{sep}demo{sep}bluray-wide"));
        assert_eq!(
            short_folder(&long, 24),
            format!("…{sep}demo{sep}bluray-wide")
        );
        // The last folder stays even when it alone is too long.
        let deep: PathBuf = ["C:", "a", "a-very-long-folder-name-indeed"]
            .iter()
            .collect();
        assert_eq!(
            short_folder(&deep, 10),
            format!("…{sep}a-very-long-folder-name-indeed")
        );
    }

    #[test]
    fn a_missing_project_has_no_date() {
        use super::RecentEntry;
        let entry = RecentEntry::new(Path::new("Z:/nowhere/名称未設定.dcproj"));
        assert_eq!(entry.name, "名称未設定");
        assert_eq!(entry.modified, None);
    }

    #[test]
    fn opened_is_open() {
        assert!(Welcome::opened().open);
    }

    #[test]
    fn start_new_project_opens_the_form() {
        let mut welcome = Welcome::default();
        welcome.start_new_project(&[PathBuf::from("C:/discs/a/a.dcproj")]);
        let location = welcome.new_project.map(|form| form.location);
        assert_eq!(
            location.map(PathBuf::from),
            Some(PathBuf::from("C:/discs/a"))
        );
    }
}
