//! Native file dialogs that never hold up a frame (M9.64), on craft-libs' craft-dialogs: a dialog is asked for with
//! what comes after it ([`Then`]); it shows at the end of the frame, over the window ([`DiscCraftApp::show_dialogs`]),
//! and its answer comes back on a later frame to [`DiscCraftApp::dialog_answered`], which goes on from there.
//!
//! The UI's parts ask with [`ask`] wherever they are (a dialog's field, a panel's button): the request waits in a
//! queue of the UI thread until the app hands it to craft-dialogs. Tests answer the dialogs themselves
//! (`craft_dialogs::Dialogs::set_override`).

use std::cell::RefCell;
use std::path::PathBuf;

use craft_dialogs::{Answer, FileDialog};
use disccraft_project::{AssetId, MenuId, TimelineId};

use crate::app::DiscCraftApp;
use crate::i18n::{tl, tlf};

/// What the app does with a file dialog's answer.
#[derive(Debug, Clone)]
pub(crate) enum Then {
    /// File ▸ Open Project…
    OpenProject,
    /// File ▸ Save (a project with no file yet) and Save As…; `after` runs once it is saved (the close or exit the
    /// save was asked for).
    SaveProject {
        save_as: bool,
        after: Option<crate::confirm::PendingAction>,
    },
    /// File ▸ Import ▸ Asset…
    ImportAssets,
    /// File ▸ Import ▸ Timeline…
    ImportTimelines,
    /// File ▸ Import ▸ Slideshow…
    ImportSlideshow,
    /// Locate Asset… for this asset.
    RelinkAsset(AssetId),
    /// File ▸ Import ▸ Blu-ray…
    ImportBluray,
    /// The Build panel's output Browse…
    BuildPath,
    /// The disc's ROM content Browse…
    RomContent,
    /// Locate Transcoded File: the video, then [`Then::LocateSound`].
    LocateVideo { asset: AssetId, disc: &'static str },
    /// Locate Transcoded File: its sound (Cancel: none), after the video.
    LocateSound {
        asset: AssetId,
        disc: &'static str,
        video: PathBuf,
    },
    /// The menu colour set dialog's Import…
    MenuColorsImport,
    /// The menu colour set dialog's Export… of this set (as the command takes it).
    MenuColorsExport(Box<serde_json::Value>),
    /// The disc label row's Choose…
    LinkDiscLabel,
    /// The disc writer's source image Browse…
    SourceImage,
    /// Timeline ▸ Import FilmCraft Markers… for this timeline.
    FilmcraftMarkers(TimelineId),
    /// Timeline ▸ New Timeline from a FilmCraft Sequence…
    FilmcraftTimeline,
    /// Timeline ▸ Import Subtitles ▸ (FAB) Image Script… for this timeline; a FAB script then asks for its pictures'
    /// folder ([`Then::ImageScriptFolder`]).
    ImageScript { timeline: TimelineId, fab: bool },
    /// The folder of a FAB image script's pictures.
    ImageScriptFolder {
        timeline: TimelineId,
        script: PathBuf,
    },
    /// The Library's Add Items from Files…
    LibraryFiles,
    /// The Library's Import Folder…
    LibraryFolder,
    /// Menu ▸ Save Menu as File… of this menu.
    SaveMenuPsd(MenuId),
    /// File ▸ Import ▸ Menu… / Pop-up Menu… / Menu Page…
    ImportMenu(crate::menu_commands::MenuImport),
    /// Menu ▸ Replace Menu… of this menu.
    ReplaceMenu(MenuId),
    /// The New Project dialog's location Browse…
    NewProjectLocation,
    /// Timeline ▸ Save Frame as File…: this asset's frame at this time (seconds).
    SaveFrame { asset: AssetId, seconds: f64 },
    /// Preferences ▸ Media: the library content's folder.
    LibraryContentFolder,
    /// Preferences ▸ Media: the media cache's folder.
    MediaCacheFolder,
    /// A subtitle colour set's Export… (`export`) or Import… for the owner `owner` (`id`).
    SubtitleColors {
        owner: &'static str,
        id: u64,
        export: bool,
    },
    /// Timeline ▸ Import Subtitles ▸ Text Script… for this timeline.
    ImportSubtitles(TimelineId),
    /// Timeline ▸ Export Subtitles of this asset.
    ExportSubtitles(AssetId),
}

thread_local! {
    /// The dialogs asked for on the UI thread and not yet handed to the app's queue.
    static ASKED: RefCell<Vec<(FileDialog, Then)>> = const { RefCell::new(Vec::new()) };
}

/// Asks for `dialog`; its answer goes to `then` on a later frame.
pub(crate) fn ask(dialog: FileDialog, then: Then) {
    ASKED.with(|asked| asked.borrow_mut().push((dialog, then)));
}

/// The dialogs asked for since the last call.
fn take_asked() -> Vec<(FileDialog, Then)> {
    ASKED.with(|asked| std::mem::take(&mut *asked.borrow_mut()))
}

/// The paths an answer gives: the files or folder chosen, or the name a Save dialog was given. `None` for Cancel; a
/// dialog that could not be shown says so in `status`.
fn chosen(answer: Answer, status: &mut String) -> Option<Vec<PathBuf>> {
    match answer {
        Answer::Paths(paths) if !paths.is_empty() => Some(paths),
        Answer::Saved { path, .. } => Some(vec![path]),
        Answer::Unavailable(why) => {
            *status = tlf!("The file dialog could not be shown: {why}", why = why);
            None
        }
        _ => None,
    }
}

impl DiscCraftApp {
    /// Hands the dialogs asked for to craft-dialogs and the answers that came in to what comes after them. Called at
    /// the start of each frame.
    pub(crate) fn pump_dialogs(&mut self, ctx: &egui::Context) {
        for (dialog, then) in take_asked() {
            self.dialogs.ask(dialog, then);
        }
        for (then, answer) in self.dialogs.take_answers() {
            self.dialog_answered(ctx, then, answer);
        }
    }

    /// Shows the next dialog asked for, over the window (`parent`). Called at the end of each frame.
    pub fn show_dialogs(&mut self, parent: &eframe::Frame) {
        for (dialog, then) in take_asked() {
            self.dialogs.ask(dialog, then);
        }
        self.dialogs.show_pending(Some(parent));
    }

    /// Goes on from a dialog's answer.
    pub(crate) fn dialog_answered(&mut self, ctx: &egui::Context, then: Then, answer: Answer) {
        let Some(paths) = chosen(answer, &mut self.status) else {
            match then {
                Then::ImportAssets => self.status = tl!("Import cancelled").to_string(),
                Then::SaveProject { .. } => self.status = tl!("Save cancelled").to_string(),
                // The sound is optional: Cancel locates the video alone.
                Then::LocateSound { asset, disc, video } => {
                    self.locate_transcoded_files(asset, disc, &video, None);
                }
                _ => {}
            }
            return;
        };
        let Some(first) = paths.first().cloned() else {
            return;
        };
        match then {
            Then::OpenProject => {
                let _ = self.open_path(&first);
            }
            Then::SaveProject { save_as, after } => {
                self.save_project_to(ctx, &first, save_as, after);
            }
            Then::ImportAssets => {
                for path in paths {
                    let _ = self.import_asset_id(&path);
                }
            }
            Then::ImportTimelines => self.import_timeline_paths(&paths),
            Then::ImportSlideshow => self.import_slideshow_paths(&paths),
            Then::RelinkAsset(id) => self.relink_asset_to(id, &first),
            Then::ImportBluray => self.import_bluray_from(&first),
            Then::BuildPath => self.build_path = first.to_string_lossy().to_string(),
            Then::RomContent => self.rom_content_chosen(&first),
            Then::LocateVideo { asset, disc } => self.locate_transcoded_sound(asset, disc, first),
            Then::LocateSound { asset, disc, video } => {
                self.locate_transcoded_files(asset, disc, &video, Some(&first));
            }
            Then::MenuColorsImport => self.menu_colors_import(&first),
            Then::MenuColorsExport(set) => self.menu_colors_export(&set, &first),
            Then::LinkDiscLabel => {
                let _ = self.link_disc_label(&first);
            }
            Then::SourceImage => self.disc.source_image = first.to_string_lossy().into_owned(),
            Then::FilmcraftMarkers(timeline) => self.filmcraft_markers_from(timeline, &first),
            Then::FilmcraftTimeline => self.filmcraft_timeline_from(&first),
            Then::ImageScript { timeline, fab } => {
                self.image_script_chosen(ctx, timeline, fab, first)
            }
            Then::ImageScriptFolder { timeline, script } => {
                self.read_image_script(ctx, timeline, script, Some(first));
            }
            Then::LibraryFiles => self.library_add_paths(paths),
            Then::LibraryFolder => self.library_import_from(&first),
            Then::SaveMenuPsd(menu) => self.save_menu_psd_to(menu, &first),
            Then::ImportMenu(kind) => self.import_menu_path(kind, &first),
            Then::ReplaceMenu(menu) => self.replace_menu_with(menu, &first),
            Then::NewProjectLocation => self.welcome.set_location(first.display().to_string()),
            Then::SaveFrame { asset, seconds } => self.save_frame_to(asset, seconds, &first),
            Then::LibraryContentFolder => self.move_library(Some(&first)),
            Then::MediaCacheFolder => self.set_media_cache_dir(Some(&first)),
            Then::SubtitleColors { owner, id, export } => {
                self.subtitle_colors_file(owner, id, export, &first);
            }
            Then::ImportSubtitles(timeline) => self.import_subtitles_from(timeline, &first),
            Then::ExportSubtitles(asset) => self.export_subtitles_to(asset, &first),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;
    use serde_json::json;

    /// Answers every dialog asked for with `answer`, as the frames would: the request handed to craft-dialogs, shown
    /// (here: answered by the override) and its answer taken, until none is left.
    fn answer_all(app: &mut DiscCraftApp, ctx: &egui::Context, answer: Answer) {
        app.dialogs
            .set_override(Some(Box::new(move |_request| answer.clone())));
        for _ in 0..4 {
            app.pump_dialogs(ctx);
            app.dialogs.show_pending(None::<&eframe::Frame>);
        }
        app.pump_dialogs(ctx);
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "disccraft-file-dialogs-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// File ▸ Open Project…: the project chosen opens on the frame its answer comes back on.
    #[test]
    fn the_project_chosen_opens() {
        let dir = scratch("open");
        let path = dir.join("chosen.dcproj");
        let mut session = disccraft_engine::Session::new();
        let _ = session.save(Some(&path)).unwrap();
        let ctx = egui::Context::default();
        let mut app = DiscCraftApp::new(&ctx);
        app.open_project();
        // Nothing happens before the dialog has answered.
        assert!(app.session.path.is_none());
        answer_all(&mut app, &ctx, Answer::Paths(vec![path.clone()]));
        assert_eq!(app.session.path.as_deref(), Some(path.as_path()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Save on a project with no file, asked for by closing it: it is written where the Save dialog says, and the
    /// close it waited for happens then (before M9.64, a dialog answered later would have cancelled the close).
    #[test]
    fn a_save_that_a_close_waits_for_saves_then_closes() {
        let dir = scratch("save");
        let path = dir.join("saved.dcproj");
        let ctx = egui::Context::default();
        let mut app = DiscCraftApp::new(&ctx);
        let _ = app
            .session
            .execute("disc.setBlurayCodec", &json!({"codec": "h264"}))
            .unwrap();
        assert!(app.session.dirty);
        app.save_project_then(false, Some(crate::confirm::PendingAction::CloseProject));
        answer_all(
            &mut app,
            &ctx,
            Answer::Saved {
                path: path.clone(),
                filter: None,
            },
        );
        assert!(path.is_file(), "{}", app.status);
        // Closed: a new, empty project with no file.
        assert!(app.session.path.is_none() && !app.session.dirty);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Cancel says so where the old dialogs did; a dialog that could not be shown says why, never taken for Cancel.
    #[test]
    fn cancel_and_a_missing_dialog_are_reported() {
        let ctx = egui::Context::default();
        let mut app = DiscCraftApp::new(&ctx);
        app.import_assets();
        answer_all(&mut app, &ctx, Answer::Cancelled);
        assert_eq!(app.status, tl!("Import cancelled"));
        app.open_project();
        answer_all(
            &mut app,
            &ctx,
            Answer::Unavailable("no file chooser".to_owned()),
        );
        assert!(app.status.contains("no file chooser"), "{}", app.status);
        assert!(app.session.path.is_none());
    }
}
