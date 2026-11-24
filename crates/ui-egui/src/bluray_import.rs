//! File ▸ Import ▸ Blu-ray… (M6.429): a timeline from a playlist of a Blu-ray (a disc in a drive, a disc's
//! folder, or an image mounted as a drive), to make a DVD of it or a disc with new menus.
//!
//! The drive or folder is chosen first; the dialog then lists the disc's playlists (length, chapters, clips, sound)
//! with its main one, the longest, chosen, and the engine's `timeline.importBluray` makes the timeline.

use disccraft_project::TimelineId;
use rfd::FileDialog;
use serde_json::{Value, json};

use crate::app::{DiscCraftApp, Selection};
use crate::i18n::{tl, tlf};

/// Playlists shorter than this are hidden unless asked for (menus' backgrounds, logos, warnings).
const SHORT_SECONDS: f64 = 60.0;

/// One playlist of the disc as the dialog lists it.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    playlist: u32,
    seconds: f64,
    chapters: u64,
    clips: usize,
    /// The sound streams: "jpn LPCM", with the ones DiscCraft does not read marked.
    audio: String,
    /// Why it cannot be imported (unreadable, clip files missing).
    problem: Option<String>,
}

/// The dialog's state while it is open.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BlurayDraft {
    /// The drive or folder chosen.
    path: String,
    /// The disc's title, when its metadata names one.
    title: Option<String>,
    rows: Vec<Row>,
    /// The playlist chosen.
    chosen: Option<u32>,
    /// Short playlists are listed too.
    show_short: bool,
    /// The disc's menu is made a DVD menu too (M6.430).
    with_menu: bool,
}

/// `seconds` as `H:MM:SS`.
fn length_text(seconds: f64) -> String {
    // `as` saturates; a playlist is hours long at most.
    let total = seconds.max(0.0).round() as u64;
    format!("{}:{:02}:{:02}", total / 3600, total / 60 % 60, total % 60)
}

/// The rows of a `bluray.playlists` result.
fn rows(found: &Value) -> Vec<Row> {
    found
        .get("playlists")
        .and_then(Value::as_array)
        .map(|playlists| {
            playlists
                .iter()
                .filter_map(|playlist| {
                    let number = u32::try_from(playlist.get("playlist")?.as_u64()?).ok()?;
                    let error = playlist.get("error").and_then(Value::as_str);
                    let missing = playlist
                        .get("missing")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let audio = playlist
                        .get("audio")
                        .and_then(Value::as_array)
                        .map(|streams| {
                            streams
                                .iter()
                                .map(|stream| {
                                    let text = |key: &str| {
                                        stream.get(key).and_then(Value::as_str).unwrap_or("")
                                    };
                                    let readable = stream
                                        .get("readable")
                                        .and_then(Value::as_bool)
                                        .unwrap_or(false);
                                    let name = format!("{} {}", text("language"), text("format"));
                                    if readable {
                                        name
                                    } else {
                                        tlf!("{stream} (not read)", stream = name)
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    Some(Row {
                        playlist: number,
                        seconds: playlist
                            .get("seconds")
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0),
                        chapters: playlist
                            .get("chapters")
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                        clips: playlist
                            .get("clips")
                            .and_then(Value::as_array)
                            .map_or(0, Vec::len),
                        audio,
                        problem: match (error, missing) {
                            (Some(error), _) => Some(error.to_owned()),
                            (None, true) => Some(tl!("Clip files are missing").to_owned()),
                            (None, false) => None,
                        },
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

impl DiscCraftApp {
    /// File ▸ Import ▸ Blu-ray…: asks for the disc's drive or folder, then opens the dialog of its playlists.
    pub(crate) fn import_bluray(&mut self) {
        let Some(folder) = FileDialog::new()
            .set_title(tl!(
                "Import from Blu-ray: choose the disc's drive or folder"
            ))
            .pick_folder()
        else {
            return;
        };
        let path = folder.to_string_lossy().into_owned();
        let Some(found) = self.run_query("bluray.playlists", json!({ "path": path })) else {
            return;
        };
        let rows = rows(&found);
        if rows.is_empty() {
            self.status = tl!("The Blu-ray has no playlists").to_owned();
            return;
        }
        self.bluray_dialog = Some(BlurayDraft {
            path,
            title: found
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_owned),
            chosen: found
                .get("main")
                .and_then(Value::as_u64)
                .and_then(|main| u32::try_from(main).ok()),
            rows,
            show_short: false,
            with_menu: true,
        });
    }

    /// Draws the dialog while it is open.
    pub(crate) fn bluray_import_window(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.bluray_dialog.clone() else {
            return;
        };
        let mut done = None;
        let response = egui::Modal::new(egui::Id::unique("bluray_import_dialog")).show(ctx, |ui| {
            ui.set_width(620.0);
            ui.heading(tl!("Import from Blu-ray"));
            if let Some(title) = &draft.title {
                ui.label(title);
            }
            ui.small(&draft.path);
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    egui::Grid::new("bluray_playlists")
                        .striped(true)
                        .num_columns(5)
                        .spacing([14.0, 4.0])
                        .show(ui, |ui| {
                            ui.strong(tl!("Blu-ray playlist"));
                            ui.strong(tl!("Length"));
                            ui.strong(tl!("Chapters"));
                            ui.strong(tl!("Clips"));
                            ui.strong(tl!("Sound"));
                            ui.end_row();
                            for row in &draft.rows {
                                let short = row.seconds < SHORT_SECONDS;
                                if short && !draft.show_short && draft.chosen != Some(row.playlist) {
                                    continue;
                                }
                                let usable = row.problem.is_none();
                                let picked = ui
                                    .add_enabled(
                                        usable,
                                        egui::RadioButton::new(
                                            draft.chosen == Some(row.playlist),
                                            format!("{:05}", row.playlist),
                                        ),
                                    )
                                    .clicked();
                                if picked {
                                    draft.chosen = Some(row.playlist);
                                }
                                ui.label(length_text(row.seconds));
                                ui.label(row.chapters.to_string());
                                ui.label(row.clips.to_string());
                                match &row.problem {
                                    Some(problem) => crate::theme::status_label(
                                        ui,
                                        crate::theme::Status::Warning,
                                        problem,
                                    ),
                                    None => ui.label(&row.audio),
                                };
                                ui.end_row();
                            }
                        });
                });
            let _ = ui.checkbox(
                &mut draft.show_short,
                tl!("Show playlists shorter than a minute"),
            );
            let _ = ui
                .checkbox(&mut draft.with_menu, tl!("Make the disc's menu a menu too"))
                .on_hover_text(tl!(
                    "The first menu the disc shows becomes a motion menu with a button for each of its buttons that plays something, at the same place, linked to the timelines of what it plays. A draft to arrange further."
                ));
            ui.add_space(4.0);
            ui.small(tl!(
                "The playlist's clips are laid out in order with its chapters and the sound DiscCraft reads (LPCM, AC-3). Subtitles are not imported. Encrypted discs cannot be read; a disc image is read once it is mounted as a drive."
            ));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(draft.chosen.is_some(), egui::Button::new(tl!("Import")))
                    .clicked()
                {
                    done = Some(true);
                }
                if ui.button(tl!("Cancel")).clicked() {
                    done = Some(false);
                }
            });
        });
        if response.should_close() {
            done = Some(false);
        }
        match (done, draft.chosen) {
            (Some(true), Some(playlist)) => {
                self.bluray_dialog = None;
                self.finish_bluray_import(&draft.path, playlist);
                if draft.with_menu {
                    self.import_bluray_menu(&draft.path);
                }
            }
            (Some(_), _) => self.bluray_dialog = None,
            (None, _) => self.bluray_dialog = Some(draft),
        }
    }

    /// Makes the first menu the disc at `path` shows a menu of the project (M6.430), and opens it.
    fn import_bluray_menu(&mut self, path: &str) {
        let before = self.status.clone();
        let Some(made) = self.run_command("menu.importBluray", json!({ "path": path })) else {
            self.status = format!(
                "{before} {}",
                tlf!(
                    "The disc's menu was not made: {reason}",
                    reason = self.status
                )
            );
            return;
        };
        let ids = |key: &str| -> Vec<u64> {
            made.get(key)
                .and_then(Value::as_array)
                .map(|ids| ids.iter().filter_map(Value::as_u64).collect())
                .unwrap_or_default()
        };
        for id in ids("assets").into_iter().chain(ids("timelines")) {
            self.place_new_item(id);
        }
        let Some(menu) = made.get("menu").and_then(Value::as_u64) else {
            return;
        };
        self.place_new_item(menu);
        self.select(Selection::Menu(disccraft_project::MenuId(menu)));
        self.status = format!(
            "{before} {}",
            tlf!(
                "Its menu: {buttons} buttons.",
                buttons = made.get("buttons").and_then(Value::as_u64).unwrap_or(0)
            )
        );
    }

    /// Makes the timeline of `playlist` of the disc at `path`, and selects it.
    fn finish_bluray_import(&mut self, path: &str, playlist: u32) {
        let Some(imported) = self.run_command(
            "timeline.importBluray",
            json!({ "path": path, "playlist": playlist }),
        ) else {
            return;
        };
        let assets: Vec<u64> = imported
            .get("assets")
            .and_then(Value::as_array)
            .map(|ids| ids.iter().filter_map(Value::as_u64).collect())
            .unwrap_or_default();
        for id in &assets {
            self.place_new_item(*id);
        }
        let Some(timeline) = imported.get("timeline").and_then(Value::as_u64) else {
            return;
        };
        self.place_new_item(timeline);
        self.select(Selection::Timeline(TimelineId(timeline)));
        let count = |key: &str| imported.get(key).and_then(Value::as_u64).unwrap_or(0);
        let left_out: Vec<String> = imported
            .get("audioLeftOut")
            .and_then(Value::as_array)
            .map(|streams| {
                streams
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        self.status = if left_out.is_empty() {
            tlf!(
                "Imported playlist {playlist} of the Blu-ray: {clips} clips, {chapters} chapters, {tracks} audio tracks",
                playlist = format!("{playlist:05}"),
                clips = count("clips"),
                chapters = count("chapters"),
                tracks = count("audioTracks")
            )
        } else {
            tlf!(
                "Imported playlist {playlist} of the Blu-ray: {clips} clips, {chapters} chapters, {tracks} audio tracks (left out: {streams})",
                playlist = format!("{playlist:05}"),
                clips = count("clips"),
                chapters = count("chapters"),
                tracks = count("audioTracks"),
                streams = left_out.join(", ")
            )
        };
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::{length_text, rows};
    use serde_json::json;

    #[test]
    fn the_playlists_are_listed_from_the_engines_answer() {
        assert_eq!(length_text(11_276.499), "3:07:56");
        assert_eq!(length_text(59.6), "0:01:00");
        let listed = rows(&json!({
            "playlists": [
                {"playlist": 1, "seconds": 11276.5, "chapters": 48, "clips": ["00001", "00002"],
                 "audio": [{"language": "jpn", "format": "LPCM", "readable": true},
                           {"language": "jpn", "format": "DTS-HD MA", "readable": false}],
                 "subtitles": ["jpn"], "missing": false},
                {"playlist": 2, "seconds": 10.0, "chapters": 1, "clips": ["00009"], "audio": [], "missing": true},
                {"playlist": 3, "error": "truncated at byte 12"},
            ]
        }));
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].clips, 2);
        assert!(listed[0].audio.starts_with("jpn LPCM, jpn DTS-HD MA"));
        assert!(listed[0].problem.is_none());
        assert!(listed[1].problem.is_some());
        assert_eq!(listed[2].problem.as_deref(), Some("truncated at byte 12"));
    }
}
