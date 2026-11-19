//! Two of Encore's smaller panels (seen in Encore CS6 on 2026-11-18, docs/encore-behavior.md 10):
//!
//! - **Progress** (Window ▸ Progress, F10): a "Transcode" group (a progress bar, what is being transcoded, the
//!   list of what is still to transcode, and Stop) and a "Media" group (a progress bar for the media being read).
//! - **Character** (Window ▸ Character, F3): the font and character formatting of the text or the button
//!   selected in the Menu Viewer. The same rows are in the text's and the button's property sheets.

use crate::app::DiscCraftApp;
use crate::i18n::{tl, tlf};

use egui::Ui;

impl DiscCraftApp {
    /// The Progress panel.
    pub(crate) fn progress_panel(&mut self, ui: &mut Ui) {
        // What runs: a build, or Transcode Now.
        let running = if self.build_job.is_running() {
            let (step, fraction) = self.build_step();
            Some((step, fraction, self.build_job.elapsed_seconds()))
        } else if self.transcode_job.is_running() {
            let (step, fraction) = match self.transcode_job.current_progress() {
                Some((step, fraction)) => (tlf!("Transcoding {name}", name = step), fraction),
                None => (tl!("Preparing…").to_owned(), 0.0),
            };
            Some((step, fraction, self.transcode_job.elapsed_seconds()))
        } else {
            None
        };
        crate::theme::group_box(ui, "progress_transcode", tl!("Transcode"), |ui| {
            let (step, fraction) = running.as_ref().map_or(
                (tl!("Nothing is being transcoded").to_owned(), 0.0),
                |run| (run.0.clone(), run.1),
            );
            let mut bar = egui::ProgressBar::new(fraction.clamp(0.0, 1.0));
            if running.is_some() {
                bar = bar.show_percentage();
            }
            let _ = ui.add(bar);
            ui.horizontal(|ui| {
                ui.label(step);
                if let Some((_, _, seconds)) = &running {
                    ui.weak(crate::build_panel::elapsed_text(*seconds));
                }
            });
            // Encore's list of the transcodes: what the job still has to transcode (M6.340).
            let waiting = self.transcode_list(running.is_some());
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_height(72.0);
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .id_salt("progress_transcode_list")
                    .max_height(110.0)
                    .show(ui, |ui| {
                        for name in &waiting {
                            ui.label(name.as_str());
                        }
                    });
            });
            // Encore's Stop: the transcodes give up at their next picture.
            if ui
                .add_enabled(running.is_some(), egui::Button::new(tl!("Stop")))
                .on_hover_text(tl!(
                    "Stops the transcodes of the build or of Transcode Now; what is already transcoded is kept"
                ))
                .clicked()
            {
                disccraft_build::request_stop();
                self.status = tl!("Stopping…").to_owned();
            }
        });
        ui.add_space(6.0);
        // The media files being opened in the background (their length, size and first picture).
        let opening: Vec<String> = self
            .run_query_quiet("asset.pending", serde_json::json!({}))
            .and_then(|value| {
                let ids = value.get("opening")?.as_array()?.clone();
                Some(
                    ids.iter()
                        .filter_map(serde_json::Value::as_u64)
                        .filter_map(|id| {
                            self.session
                                .project
                                .assets
                                .get(&disccraft_project::AssetId(id))
                                .map(|asset| asset.name.clone())
                        })
                        .collect(),
                )
            })
            .unwrap_or_default();
        crate::theme::group_box(ui, "progress_media", tl!("Media"), |ui| {
            let mut bar = egui::ProgressBar::new(if opening.is_empty() { 0.0 } else { 1.0 });
            if !opening.is_empty() {
                bar = bar.animate(true);
            }
            let _ = ui.add(bar);
            if opening.is_empty() {
                ui.label(tl!("No media is being read"));
            } else {
                ui.label(tlf!("Reading {n} media files", n = opening.len()));
                for name in opening.iter().take(8) {
                    ui.weak(name);
                }
            }
        });
        if running.is_some() || !opening.is_empty() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    /// The names of the assets the running job still has to transcode; the transcode status is looked at again
    /// every two seconds while it runs. Nothing when no job runs.
    fn transcode_list(&mut self, running: bool) -> Vec<String> {
        if !running {
            self.transcode_queue.clear();
            self.transcode_queue_checked = None;
            return Vec::new();
        }
        let due = self
            .transcode_queue_checked
            .is_none_or(|checked| checked.elapsed() >= std::time::Duration::from_secs(2));
        if due {
            self.transcode_dirty = true;
            self.transcode_queue_checked = Some(std::time::Instant::now());
        }
        self.transcode_queue
            .iter()
            .filter(|id| {
                self.transcode.get(id) == Some(&crate::project_panel::TranscodeState::Untranscoded)
            })
            .filter_map(|id| {
                self.session
                    .project
                    .assets
                    .get(&disccraft_project::AssetId(*id))
                    .map(|asset| {
                        crate::project_panel::name_with_extension(&asset.name, &asset.path)
                    })
            })
            .collect()
    }

    /// The Character panel: the character rows of the text, or of the one button, selected in the menu shown in
    /// the Menu Viewer; with nothing selected, those of the next text typed (Encore's defaults).
    pub(crate) fn character_panel(&mut self, ui: &mut Ui) {
        let Some(menu) = self.viewer_menu() else {
            self.next_text_rows(ui);
            return;
        };
        if let Some(index) = self.selected_menu_text(menu) {
            self.text_character_rows(ui, menu, index);
            return;
        }
        let button = match self.selected_buttons.as_slice() {
            [index] if self.selection == crate::app::Selection::Menu(menu) => Some(*index),
            _ => None,
        };
        let found = button.and_then(|index| {
            let button = self.session.project.menu(menu)?.buttons.get(index)?.clone();
            Some((index, button))
        });
        match found {
            Some((index, button)) => self.button_character_rows(ui, menu, index, &button),
            None => self.next_text_rows(ui),
        }
    }

    /// The Metadata panel (Encore: Window ▸ Metadata): the XMP metadata of the selected asset's file (a
    /// timeline's: its video's), by schema, with a search field. Read only: DiscCraft writes nothing into media
    /// files.
    pub(crate) fn metadata_panel(&mut self, ui: &mut Ui) {
        let asset = match self.selection {
            crate::app::Selection::Asset(id) => Some(id),
            crate::app::Selection::Timeline(id) => self
                .session
                .project
                .timeline(id)
                .map(|timeline| timeline.video),
            _ => None,
        };
        let found = asset.and_then(|id| {
            let asset = self.session.project.assets.get(&id)?;
            Some((id, asset.name.clone(), asset.media_file().into_owned()))
        });
        let _ = ui.add(
            egui::TextEdit::singleline(&mut self.metadata_search)
                .hint_text(tl!("Search"))
                .desired_width(f32::INFINITY),
        );
        let Some((id, name, path)) = found else {
            ui.weak(tl!("Select an asset to see its metadata"));
            return;
        };
        let metadata =
            std::sync::Arc::clone(self.metadata_cache.entry(id.0).or_insert_with(|| {
                std::sync::Arc::new(disccraft_engine::metadata::file_metadata(&path))
            }));
        ui.horizontal(|ui| {
            ui.strong(name);
            if ui
                .small_button(tl!("Reload"))
                .on_hover_text(tl!("Reads the file's metadata again"))
                .clicked()
            {
                let _ = self.metadata_cache.remove(&id.0);
            }
        });
        if let Some(sidecar) = &metadata.sidecar {
            ui.weak(tlf!(
                "From the sidecar file {name}",
                name = sidecar
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ));
        }
        if metadata.groups.is_empty() {
            ui.weak(tl!("This file has no XMP metadata"));
            return;
        }
        let wanted = self.metadata_search.trim().to_lowercase();
        for group in &metadata.groups {
            let items: Vec<&(String, String)> = group
                .items
                .iter()
                .filter(|(name, value)| {
                    wanted.is_empty()
                        || name.to_lowercase().contains(&wanted)
                        || value.to_lowercase().contains(&wanted)
                })
                .collect();
            if items.is_empty() {
                continue;
            }
            egui::CollapsingHeader::new(crate::i18n::t(&group.name).to_owned())
                .id_salt(("metadata_group", &group.name))
                // A search opens what it finds.
                .open((!wanted.is_empty()).then_some(true))
                .show(ui, |ui| {
                    egui::Grid::new(("metadata_grid", &group.name))
                        .num_columns(2)
                        .striped(true)
                        .show(ui, |ui| {
                            for (name, value) in items {
                                ui.label(name);
                                ui.add(egui::Label::new(value).wrap().selectable(true));
                                ui.end_row();
                            }
                        });
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::DiscCraftApp;
    use crate::project_panel::TranscodeState;

    /// M6.340: the Progress panel lists what the running job still has to transcode, asks for the status again now
    /// and then, and forgets the list when nothing runs.
    #[test]
    fn the_progress_list_is_what_is_left_to_transcode() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = DiscCraftApp::new(&egui::Context::default());
        let first = app
            .session
            .project
            .add_asset(std::path::Path::new("first.mp4"))?;
        let second = app
            .session
            .project
            .add_asset(std::path::Path::new("second.mp4"))?;
        app.transcode.insert(first.0, TranscodeState::Untranscoded);
        app.transcode.insert(second.0, TranscodeState::Transcoded);
        app.transcode_queue = vec![first.0, second.0];
        app.transcode_dirty = false;
        assert_eq!(app.transcode_list(true), vec!["first.mp4".to_owned()]);
        assert!(app.transcode_dirty, "the status is looked at again");
        app.transcode.insert(first.0, TranscodeState::Transcoded);
        assert!(app.transcode_list(true).is_empty());
        assert!(app.transcode_list(false).is_empty());
        assert!(app.transcode_queue.is_empty());
        Ok(())
    }
}
