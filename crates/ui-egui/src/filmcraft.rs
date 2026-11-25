//! FilmCraft 連携: タイムライン ▸ FilmCraft のマーカーを読み込み…。FilmCraft のプロジェクト（.fcproj）のシーケンスを
//! 選び、そのチャプターマーカー（またはすべてのマーカー）をタイムラインのチャプターポイントにする。Encore が
//! Premiere Pro のチャプターマーカーをチャプターにするのと同じ役目。

use crate::DiscCraftApp;
use crate::i18n::{tl, tlf};
use disccraft_project::TimelineId;
use rfd::FileDialog;
use serde_json::{Value, json};

/// One sequence offered in the dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SequenceChoice {
    /// The sequence's item id in the FilmCraft project.
    pub(crate) id: u64,
    /// The sequence's name.
    pub(crate) name: String,
    /// How many markers it has, and how many of them are chapter markers.
    pub(crate) markers: u64,
    pub(crate) chapters: u64,
}

/// The dialog's state while it is open.
#[derive(Clone, Debug)]
pub(crate) struct FilmMarkerDialog {
    /// The timeline that takes the markers; `None` makes a new timeline of the sequence.
    timeline: Option<TimelineId>,
    path: String,
    sequences: Vec<SequenceChoice>,
    chosen: usize,
    /// Every marker, not only the chapter markers.
    all: bool,
}

/// The sequences in a `filmcraft.sequences` result.
pub(crate) fn parse_sequences(value: &Value) -> Vec<SequenceChoice> {
    value
        .get("sequences")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|sequence| {
                    Some(SequenceChoice {
                        id: sequence.get("id").and_then(Value::as_u64).unwrap_or(0),
                        name: sequence.get("name")?.as_str()?.to_owned(),
                        markers: sequence.get("markers").and_then(Value::as_u64).unwrap_or(0),
                        chapters: sequence
                            .get("chapterMarkers")
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The sequence the dialog starts on: the one called like the timeline or its video, else the first with chapter
/// markers, else the first.
pub(crate) fn first_choice(sequences: &[SequenceChoice], hints: &[&str]) -> usize {
    sequences
        .iter()
        .position(|sequence| {
            hints
                .iter()
                .any(|hint| sequence.name.eq_ignore_ascii_case(hint.trim()))
        })
        .or_else(|| sequences.iter().position(|sequence| sequence.chapters > 0))
        .unwrap_or(0)
}

impl DiscCraftApp {
    /// タイムライン ▸ FilmCraft のマーカーを読み込み…: プロジェクトを選んでダイアログを開く。
    pub(crate) fn import_filmcraft_markers(&mut self) {
        let Some(timeline) = self.selected_timeline() else {
            self.status = tl!("Select a timeline first").to_string();
            return;
        };
        let Some(path) = FileDialog::new()
            .add_filter(tl!("FilmCraft Project"), &["fcproj"])
            .pick_file()
        else {
            return;
        };
        let path = path.to_string_lossy().into_owned();
        let Some(listed) = self.run_command("filmcraft.sequences", json!({ "path": path })) else {
            return;
        };
        let sequences = parse_sequences(&listed);
        if sequences.is_empty() {
            self.status = tl!("The FilmCraft project has no sequences.").to_owned();
            return;
        }
        let (timeline_name, video_name) = self
            .session
            .project
            .timeline(timeline)
            .map(|found| {
                let video = self
                    .session
                    .project
                    .assets
                    .get(&found.video)
                    .map(|asset| asset.name.clone())
                    .unwrap_or_default();
                (found.name.clone(), video)
            })
            .unwrap_or_default();
        let chosen = first_choice(&sequences, &[&timeline_name, &video_name]);
        self.film_marker_dialog = Some(FilmMarkerDialog {
            timeline: Some(timeline),
            path,
            sequences,
            chosen,
            all: false,
        });
    }

    /// The dialog: the sequence to take the markers from, and which markers.
    pub(crate) fn show_film_marker_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.film_marker_dialog.take() else {
            return;
        };
        let mut choice = None;
        let response =
            egui::Modal::new(egui::Id::unique("filmcraft_marker_dialog")).show(ctx, |ui| {
                ui.set_width(460.0);
                ui.strong(if dialog.timeline.is_some() {
                    tl!("Import FilmCraft Markers")
                } else {
                    tl!("New Timeline from FilmCraft Sequence")
                });
                ui.add_space(6.0);
                ui.label(tl!("Sequence:"));
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for (index, sequence) in dialog.sequences.iter().enumerate() {
                            let text = tlf!(
                                "{name} ({chapters} chapter markers, {markers} markers)",
                                name = sequence.name,
                                chapters = sequence.chapters,
                                markers = sequence.markers
                            );
                            let _ = ui.radio_value(&mut dialog.chosen, index, text);
                        }
                    });
                ui.add_space(6.0);
                let _ = ui.radio_value(&mut dialog.all, false, tl!("Chapter markers only"));
                let _ = ui.radio_value(&mut dialog.all, true, tl!("All markers"));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(crate::theme::dialog_button(tl!("Cancel"))).clicked() {
                        choice = Some(false);
                    }
                    if ui.add(crate::theme::default_button(tl!("OK"))).clicked() {
                        choice = Some(true);
                    }
                });
            });
        match choice {
            Some(true) => {
                let Some(sequence) = dialog.sequences.get(dialog.chosen) else {
                    return;
                };
                let Some(timeline) = dialog.timeline else {
                    let sequence = sequence.clone();
                    self.start_film_export(&dialog.path, &sequence, dialog.all);
                    return;
                };
                let result = self.run_command(
                    "timeline.importFilmCraftMarkers",
                    json!({
                        "timeline": timeline.0,
                        "path": dialog.path,
                        "sequence": sequence.name,
                        "all": dialog.all,
                    }),
                );
                if let Some(result) = result {
                    let count = |key: &str| result.get(key).and_then(Value::as_u64).unwrap_or(0);
                    self.status = tlf!(
                        "Added {added} chapters from FilmCraft markers ({skipped} left out)",
                        added = count("added"),
                        skipped = count("skipped")
                    );
                }
            }
            Some(false) => {}
            None if response.should_close() => {}
            None => self.film_marker_dialog = Some(dialog),
        }
    }
}

/// A FilmCraft export running in the background (New Timeline from FilmCraft Sequence).
pub(crate) struct FilmExport {
    /// The export's outcome, sent once.
    receiver: std::sync::mpsc::Receiver<Result<(), String>>,
    /// The movie being written.
    output: std::path::PathBuf,
    /// The FilmCraft project and the sequence exported.
    project: String,
    sequence: String,
    /// Every marker becomes a chapter, not only the chapter markers.
    all: bool,
}

/// A file name made of a sequence's name: the characters a file name cannot hold become `_`.
pub(crate) fn file_stem_of(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .take(80)
        .collect();
    let cleaned = cleaned
        .trim_matches(|c: char| c == '.' || c == ' ')
        .to_owned();
    if cleaned.is_empty() {
        "Sequence".to_owned()
    } else {
        cleaned
    }
}

/// The arguments of `filmcraft-cli` that export sequence `sequence` of `project` to `output` as H.264 and wait
/// for the file (the disc's own format is made when the disc is built).
pub(crate) fn export_args(project: &str, sequence: u64, output: &std::path::Path) -> Vec<String> {
    vec![
        "--project".to_owned(),
        project.to_owned(),
        "--compact".to_owned(),
        "exec".to_owned(),
        "file.exportMedia".to_owned(),
        json!({
            "path": output.to_string_lossy(),
            "sequence": sequence,
            "format": "h264",
            "bitrateMode": "crf",
            "crf": 16,
            "wait": true,
        })
        .to_string(),
    ]
}

/// Runs the export and reports how it went.
fn run_export(
    cli: &std::path::Path,
    args: &[String],
    output: &std::path::Path,
) -> Result<(), String> {
    let mut command = std::process::Command::new(cli);
    let _ = command.args(args).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console window flashes up beside the GUI.
        let _ = command.creation_flags(0x0800_0000);
    }
    let result = command
        .output()
        .map_err(|error| format!("{}: {error}", cli.display()))?;
    if result.status.success() && output.is_file() {
        Ok(())
    } else {
        let message = String::from_utf8_lossy(&result.stderr);
        let message = message.trim();
        Err(if message.is_empty() {
            "FilmCraft: the export failed".to_owned()
        } else {
            format!("FilmCraft: {message}")
        })
    }
}

impl DiscCraftApp {
    /// タイムライン ▸ FilmCraft のシーケンスからタイムラインを作成…: プロジェクトを選んでダイアログを開く。
    pub(crate) fn new_timeline_from_filmcraft(&mut self) {
        if self.film_export.is_some() {
            self.status = tl!("FilmCraft is still exporting.").to_owned();
            return;
        }
        if crate::photocraft::find_cli(crate::photocraft::CraftApp::Film).is_none() {
            self.status = tl!("FilmCraft (filmcraft-cli) was not found.").to_owned();
            return;
        }
        let Some(path) = FileDialog::new()
            .add_filter(tl!("FilmCraft Project"), &["fcproj"])
            .pick_file()
        else {
            return;
        };
        let path = path.to_string_lossy().into_owned();
        let Some(listed) = self.run_command("filmcraft.sequences", json!({ "path": path })) else {
            return;
        };
        let sequences = parse_sequences(&listed);
        if sequences.is_empty() {
            self.status = tl!("The FilmCraft project has no sequences.").to_owned();
            return;
        }
        let chosen = first_choice(&sequences, &[]);
        self.film_marker_dialog = Some(FilmMarkerDialog {
            timeline: None,
            path,
            sequences,
            chosen,
            all: false,
        });
    }

    /// Starts exporting `sequence` of the FilmCraft project with `filmcraft-cli`, in the background.
    fn start_film_export(&mut self, project: &str, sequence: &SequenceChoice, all: bool) {
        let Some(cli) = crate::photocraft::find_cli(crate::photocraft::CraftApp::Film) else {
            self.status = tl!("FilmCraft (filmcraft-cli) was not found.").to_owned();
            return;
        };
        // Beside the DiscCraft project when it is saved, else beside the FilmCraft project.
        let folder = self
            .session
            .path
            .as_deref()
            .and_then(std::path::Path::parent)
            .or_else(|| std::path::Path::new(project).parent())
            .map(std::path::Path::to_path_buf)
            .unwrap_or_default();
        let output = crate::photocraft::unique_path(&folder, &file_stem_of(&sequence.name), "mp4");
        let args = export_args(project, sequence.id, &output);
        let (sender, receiver) = std::sync::mpsc::channel();
        let target = output.clone();
        let _ = std::thread::spawn(move || {
            let _ = sender.send(run_export(&cli, &args, &target));
        });
        self.status = tlf!("FilmCraft is exporting {name}…", name = sequence.name);
        self.film_export = Some(FilmExport {
            receiver,
            output,
            project: project.to_owned(),
            sequence: sequence.name.clone(),
            all,
        });
    }

    /// Takes a finished FilmCraft export: the movie becomes an asset and a timeline named after the sequence,
    /// with the sequence's markers as chapters.
    pub(crate) fn poll_film_export(&mut self, ctx: &egui::Context) {
        let Some(export) = self.film_export.take() else {
            return;
        };
        let outcome = match export.receiver.try_recv() {
            Ok(outcome) => outcome,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
                self.film_export = Some(export);
                return;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("FilmCraft: the export stopped".to_owned())
            }
        };
        if let Err(message) = outcome {
            self.status = message;
            return;
        }
        let imported = self.run_command(
            "asset.import",
            crate::gui_params::import_asset(&export.output),
        );
        let Some(asset) = imported.and_then(|value| value.get("asset").and_then(Value::as_u64))
        else {
            return;
        };
        let created = self.run_command(
            "timeline.create",
            json!({ "asset": asset, "name": export.sequence }),
        );
        let Some(timeline) =
            created.and_then(|value| value.get("timeline").and_then(Value::as_u64))
        else {
            return;
        };
        self.place_new_item(timeline);
        let markers = self.run_command(
            "timeline.importFilmCraftMarkers",
            json!({
                "timeline": timeline,
                "path": export.project,
                "sequence": export.sequence,
                "all": export.all,
            }),
        );
        let added = markers
            .and_then(|value| value.get("added").and_then(Value::as_u64))
            .unwrap_or(0);
        self.status = tlf!(
            "Made timeline {name} from FilmCraft ({added} chapters)",
            name = export.sequence,
            added = added
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{SequenceChoice, export_args, file_stem_of, first_choice, parse_sequences};

    fn sequence(name: &str, chapters: u64) -> SequenceChoice {
        SequenceChoice {
            id: 1,
            name: name.to_owned(),
            markers: chapters + 1,
            chapters,
        }
    }

    #[test]
    fn the_dialog_starts_on_the_likely_sequence() {
        let list = [
            sequence("Rough", 0),
            sequence("Final", 3),
            sequence("movie", 0),
        ];
        assert_eq!(first_choice(&list, &["Timeline 1", "Movie"]), 2);
        assert_eq!(first_choice(&list, &["Timeline 1", "other"]), 1);
        assert_eq!(first_choice(&[sequence("A", 0), sequence("B", 0)], &[]), 0);
    }

    #[test]
    fn sequences_are_read_from_the_commands_result() {
        let value = serde_json::json!({"sequences": [
            {"id": 21, "name": "Main", "duration": 10, "markers": 4, "chapterMarkers": 3},
            {"duration": 1},
            {"name": "Extras"},
        ]});
        assert_eq!(
            parse_sequences(&value),
            [
                SequenceChoice {
                    id: 21,
                    name: "Main".to_owned(),
                    markers: 4,
                    chapters: 3
                },
                SequenceChoice {
                    id: 0,
                    name: "Extras".to_owned(),
                    markers: 0,
                    chapters: 0
                },
            ]
        );
        assert!(parse_sequences(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn a_sequences_name_becomes_a_file_name() {
        assert_eq!(file_stem_of(" Main: cut 1/2? "), "Main_ cut 1_2_");
        assert_eq!(file_stem_of("本編"), "本編");
        assert_eq!(file_stem_of(" .. "), "Sequence");
        assert_eq!(file_stem_of(&"a".repeat(200)).chars().count(), 80);
    }

    /// What `filmcraft-cli` is asked: the project, then one `file.exportMedia` of the sequence that waits.
    #[test]
    fn the_export_names_the_sequence_and_waits() -> Result<(), Box<dyn std::error::Error>> {
        let args = export_args(
            "C:/films/a.fcproj",
            21,
            std::path::Path::new("C:/out/Main.mp4"),
        );
        let head: Vec<&str> = args.iter().take(5).map(String::as_str).collect();
        assert_eq!(
            head,
            [
                "--project",
                "C:/films/a.fcproj",
                "--compact",
                "exec",
                "file.exportMedia"
            ]
        );
        let params: serde_json::Value = serde_json::from_str(args.get(5).ok_or("no params")?)?;
        assert_eq!(params.get("sequence"), Some(&serde_json::json!(21)));
        assert_eq!(
            params.get("path"),
            Some(&serde_json::json!("C:/out/Main.mp4"))
        );
        assert_eq!(params.get("format"), Some(&serde_json::json!("h264")));
        assert_eq!(params.get("wait"), Some(&serde_json::json!(true)));
        Ok(())
    }
}
