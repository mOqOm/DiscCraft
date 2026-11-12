//! Start screen (docs/ui-parity.md §2.1). The New Project dialog it opens lives in [`crate::new_project`].

use std::path::{Path, PathBuf};

use crate::i18n::tl;
use crate::new_project::{FormAction, NewProjectForm, draw_new_project};
use egui::{Align, Button, Context, Id, Layout, Modal, ScrollArea, Ui, vec2};

/// State of the start screen and the New Project dialog.
#[derive(Clone, Debug, Default)]
pub(crate) struct Welcome {
    /// The start dialog is shown.
    pub(crate) open: bool,
    /// The What's New list is expanded.
    pub(crate) whats_new: bool,
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
            ui.set_width(460.0);
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

            if self.whats_new {
                draw_whats_new(ui);
            }

            ui.separator();
            draw_exit(ui)
        });

        if response.should_close() {
            self.open = false;
            return None;
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

/// Draw the recent projects list and return a choice when one is clicked.
fn draw_recent(ui: &mut Ui, recent: &[PathBuf]) -> Option<WelcomeChoice> {
    if recent.is_empty() {
        ui.label(tl!("No recent projects"));
        return None;
    }

    let mut choice = None;

    ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
        for path in recent {
            if choice.is_some() {
                break;
            }

            let label = recent_label(path);
            let response = ui
                .selectable_label(false, label)
                .on_hover_text(path.display().to_string());

            if response.clicked() {
                choice = Some(WelcomeChoice::OpenRecent(path.clone()));
            }
        }
    });

    choice
}

/// Draw the three main start-screen buttons.
fn draw_start_actions(
    ui: &mut Ui,
    welcome: &mut Welcome,
    recent: &[PathBuf],
) -> Option<WelcomeChoice> {
    let mut choice = None;

    ui.horizontal(|ui| {
        if ui
            .add_sized(vec2(140.0, 48.0), Button::new(tl!("New Project")))
            .clicked()
        {
            welcome.start_new_project(recent);
        }

        if ui
            .add_sized(vec2(140.0, 48.0), Button::new(tl!("Open Project…")))
            .clicked()
        {
            choice = Some(WelcomeChoice::Open);
        }

        if ui
            .add_sized(vec2(140.0, 48.0), Button::new(tl!("What's New")))
            .clicked()
        {
            welcome.whats_new = !welcome.whats_new;
        }
    });

    choice
}

/// Draw the expanded What's New list.
fn draw_whats_new(ui: &mut Ui) {
    ui.add_space(8.0);
    ui.label(tl!(
        "Panel layout and start screen that follow the classic authoring tools"
    ));
    ui.label(tl!(
        "Menu bar and keyboard shortcuts that follow the classic authoring tools"
    ));
    ui.label(tl!("Title Button in the disc properties and the flowchart"));
    ui.label(tl!("Flowchart with drag-to-link"));
    ui.label(tl!("Timeline panel with chapter points"));
    ui.label(tl!("Pick whip linking"));
    ui.label(tl!("Blu-ray authoring (folder and ISO)"));
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
