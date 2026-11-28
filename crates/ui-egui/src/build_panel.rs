use crate::app::DiscCraftApp;
use crate::i18n::{tl, tlf};

use disccraft_build::{BuildOptions, BuildReport, OutputKind, STEP_IMAGE, build_with_progress};
use disccraft_project::Project;
use disccraft_vm::{Report, explore, load_video_ts};

use crate::file_dialogs::{Then, ask};
use craft_dialogs::FileDialog;
use egui::{self, Ui};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum BuildState {
    #[default]
    Idle,
    Running,
    Finished,
}

#[derive(Default)]
pub(crate) struct BuildJob {
    pub(crate) state: BuildState,
    pub(crate) result: Option<Result<BuildReport, String>>,
    rx: Option<Receiver<Result<BuildReport, String>>>,
    pub(crate) output_kind: Option<OutputKind>,
    /// What the running build last said it was doing: the step (a media name, or `STEP_IMAGE`) and how far (0 to 1).
    progress: Arc<Mutex<Option<(String, f32)>>>,
    /// M6.497: every step that is running (transcodes run side by side), in the order they began, each with how far
    /// it is and when it last said so; a step goes when it reaches 1.
    steps: Arc<Mutex<Vec<(String, f32, Instant)>>>,
    /// When the running build started.
    started: Option<Instant>,
    /// M6.497: Cancel was pressed; the build stops at its next picture or chunk.
    pub(crate) cancelling: bool,
}

/// A step that has said nothing for this long is not shown as running (one that never reported its end).
const STEP_SILENT_SECONDS: u64 = 60;

impl BuildJob {
    pub(crate) fn begin(&mut self) {
        self.state = BuildState::Running;
        self.result = None;
        self.rx = None;
        self.started = Some(Instant::now());
        self.progress = Arc::new(Mutex::new(None));
        self.steps = Arc::new(Mutex::new(Vec::new()));
        self.cancelling = false;
    }

    /// M6.497: the steps that are running, each with how far it is (0 to 1), in the order they began.
    pub(crate) fn running_steps(&self) -> Vec<(String, f32)> {
        self.steps
            .lock()
            .map(|steps| {
                steps
                    .iter()
                    .filter(|(_, _, heard)| heard.elapsed().as_secs() < STEP_SILENT_SECONDS)
                    .map(|(step, fraction, _)| (step.clone(), *fraction))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The running build's step and how far it is, if it has said.
    pub(crate) fn current_progress(&self) -> Option<(String, f32)> {
        self.progress.lock().ok().and_then(|guard| guard.clone())
    }

    /// How long the running build has taken, in whole seconds.
    pub(crate) fn elapsed_seconds(&self) -> u64 {
        self.started
            .map_or(0, |started| started.elapsed().as_secs())
    }

    pub(crate) fn finish(&mut self, result: Result<BuildReport, String>) {
        self.state = BuildState::Finished;
        self.result = Some(result);
        self.rx = None;
    }

    pub(crate) fn is_running(&self) -> bool {
        self.state == BuildState::Running
    }

    pub(crate) fn start<F>(
        &mut self,
        project: Project,
        options: BuildOptions,
        runner: F,
        background: bool,
    ) where
        F: FnOnce(
                &Project,
                &BuildOptions,
                &mut dyn FnMut(&str, f32),
            ) -> Result<BuildReport, String>
            + Send
            + 'static,
    {
        self.begin();
        self.output_kind = Some(options.output);
        let shared = Arc::clone(&self.progress);
        let steps = Arc::clone(&self.steps);
        let mut report = move |step: &str, fraction: f32| {
            let fraction = fraction.clamp(0.0, 1.0);
            if let Ok(mut guard) = shared.lock() {
                *guard = Some((step.to_owned(), fraction));
            }
            if let Ok(mut steps) = steps.lock() {
                note_step(&mut steps, step, fraction, Instant::now());
            }
        };

        if background {
            let (tx, rx) = channel();
            self.rx = Some(rx);

            let _ = thread::spawn(move || {
                let result = runner(&project, &options, &mut report);
                let _ = tx.send(result);
            });
        } else {
            self.finish(runner(&project, &options, &mut report));
        }
    }

    pub(crate) fn poll(&mut self) -> Option<Result<BuildReport, String>> {
        let outcome = if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    Some(Err(tl!("The build thread ended").to_string()))
                }
            }
        } else {
            None
        };

        if let Some(result) = &outcome {
            self.finish(result.clone());
        }

        outcome
    }
}

/// Notes in `steps` that `step` is `fraction` of the way at `now`: a new step is added at the end, a known one
/// updated, and one that reached 1 taken off.
fn note_step(steps: &mut Vec<(String, f32, Instant)>, step: &str, fraction: f32, now: Instant) {
    let found = steps.iter().position(|(known, _, _)| known == step);
    match (found, fraction >= 1.0) {
        (Some(index), true) => {
            let _ = steps.remove(index);
        }
        (Some(index), false) => {
            if let Some(entry) = steps.get_mut(index) {
                entry.1 = fraction;
                entry.2 = now;
            }
        }
        (None, true) => {}
        (None, false) => steps.push((step.to_owned(), fraction, now)),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Usage {
    pub(crate) estimated_bytes: u64,
    pub(crate) capacity_bytes: u64,
    /// ROM コンテンツ（映像のそばに置くファイル）のバイト数。
    pub(crate) rom_bytes: u64,
    /// タイムラインごとのエンコード後のサイズの見積もり（バイト）。
    pub(crate) timelines: std::collections::BTreeMap<u64, u64>,
    /// M6.498: what multiplexing adds to the streams (part of `estimated_bytes`).
    pub(crate) mux_bytes: u64,
    /// M6.498: a DVD9's layers at a manual layer break: (layer 0, the most layer 0 holds, layer 1), in bytes.
    pub(crate) layers: Option<(u64, u64, u64)>,
}

#[derive(Clone, Debug)]
pub(crate) struct ProblemView {
    pub(crate) severity: String,
    /// Stable problem code from `project.check` (e.g. "endActionNotSet").
    pub(crate) code: String,
    pub(crate) message: String,
}

#[derive(Clone, Debug)]
pub(crate) struct SimulateSummary {
    pub(crate) chapters: Vec<String>,
    pub(crate) dead_ends: Vec<String>,
    pub(crate) loops: Vec<String>,
}

impl SimulateSummary {
    pub(crate) fn from_report(report: &Report) -> Self {
        Self {
            chapters: report
                .chapters_reached
                .iter()
                .map(|(title, chapter)| {
                    tlf!(
                        "Title {title} chapter {chapter}",
                        title = title,
                        chapter = chapter
                    )
                })
                .collect(),
            dead_ends: report.dead_ends.clone(),
            loops: report.loops.clone(),
        }
    }
}

pub(crate) fn parse_usage(value: &Value) -> Option<Usage> {
    let object = value.as_object()?;

    let estimated_bytes = get_u64(
        object,
        &[
            "estimatedBytes",
            "estimated_bytes",
            "usageBytes",
            "usage_bytes",
        ],
    )?;

    let capacity_bytes = get_u64(object, &["capacityBytes", "capacity_bytes", "capacity"])?;

    let timelines = object
        .get("timelines")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let id = entry.get("timeline").and_then(Value::as_u64)?;
                    let bytes = entry.get("bytes").and_then(Value::as_u64)?;
                    Some((id, bytes))
                })
                .collect()
        })
        .unwrap_or_default();

    let layers = object.get("layers").and_then(|layers| {
        Some((
            layers.get("layer0Bytes")?.as_u64()?,
            layers.get("layer0MaxBytes")?.as_u64()?,
            layers.get("layer1Bytes")?.as_u64()?,
        ))
    });
    Some(Usage {
        estimated_bytes,
        capacity_bytes,
        rom_bytes: get_u64(object, &["romBytes"]).unwrap_or(0),
        timelines,
        mux_bytes: get_u64(object, &["muxBytes"]).unwrap_or(0),
        layers,
    })
}

fn get_u64(object: &Map<String, Value>, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_u64))
}

pub(crate) fn parse_problems(value: &Value) -> Vec<ProblemView> {
    let array = value
        .as_array()
        .or_else(|| value.get("problems").and_then(Value::as_array));

    let Some(array) = array else {
        return Vec::new();
    };

    array
        .iter()
        .filter_map(|item| {
            let severity = item
                .get("severity")
                .and_then(Value::as_str)
                .unwrap_or("info")
                .to_string();

            let message = item
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| item.as_str())
                .unwrap_or("")
                .to_string();

            let code = item
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();

            if message.is_empty() {
                None
            } else {
                Some(ProblemView {
                    severity,
                    code,
                    message,
                })
            }
        })
        .collect()
}

/// M6.497: a label and a bar for each of `steps`, one under another; Preparing… and a moving bar when there are none.
pub(crate) fn step_bars(ui: &mut Ui, steps: &[(String, f32)]) {
    if steps.is_empty() {
        ui.label(tl!("Preparing…"));
        ui.add(egui::ProgressBar::new(0.0).animate(true));
        return;
    }
    for (step, fraction) in steps {
        ui.label(step.as_str());
        ui.add(egui::ProgressBar::new(*fraction).show_percentage());
    }
}

/// The words for a step a build reports: writing the image, or transcoding a medium (its name).
fn step_text(step: &str) -> String {
    if step == STEP_IMAGE {
        tl!("Writing the disc image").to_owned()
    } else {
        tlf!("Transcoding {name}", name = step)
    }
}

/// A size in binary units, named as they are (M6.494: 1 GiB = 1024 MiB = 1,073,741,824 bytes; it said "GB" for them).
pub(crate) fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;

    if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.2} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.2} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

pub(crate) fn usage_fraction(usage: &Usage) -> f32 {
    if usage.capacity_bytes == 0 {
        return 0.0;
    }

    let ratio = usage.estimated_bytes as f64 / usage.capacity_bytes as f64;
    ratio.clamp(0.0, 1.0) as f32
}

/// A problem of the project check: its mark and text in the colour of its severity (an error, a warning), or grey
/// for a note (M6.481: the mark's shape tells them apart without the colour).
pub(crate) fn problem_label(
    ui: &mut Ui,
    severity: &str,
    text: impl Into<egui::RichText>,
) -> egui::Response {
    match crate::theme::Status::of_severity(severity) {
        Some(status) => crate::theme::status_label(ui, status, text),
        None => ui.colored_label(egui::Color32::GRAY, text),
    }
}

pub(crate) fn simulate_folder(path: &Path) -> Result<SimulateSummary, String> {
    let model = load_video_ts(path).map_err(|error| error.to_string())?;
    let report = explore(&model);
    Ok(SimulateSummary::from_report(&report))
}

impl DiscCraftApp {
    pub(crate) fn simulate_summary(&self, ui: &mut Ui, summary: &SimulateSummary) {
        ui.label(tlf!(
            "Chapters reached: {count}",
            count = summary.chapters.len()
        ));

        egui::ScrollArea::vertical()
            .max_height(120.0)
            .show(ui, |ui| {
                for line in &summary.chapters {
                    ui.label(line);
                }

                if !summary.dead_ends.is_empty() {
                    crate::theme::status_label(ui, crate::theme::Status::Error, tl!("Dead ends"));

                    for line in &summary.dead_ends {
                        ui.label(line);
                    }
                }

                if !summary.loops.is_empty() {
                    crate::theme::status_label(ui, crate::theme::Status::Warning, tl!("Loops"));

                    for line in &summary.loops {
                        ui.label(line);
                    }
                }
            });
    }

    pub(crate) fn pick_build_path(&mut self) {
        let dialog = match self.build_output {
            OutputKind::Folder | OutputKind::Ddp => FileDialog::folder(),
            OutputKind::Iso => FileDialog::save().filter("ISO", &["iso"]),
        };
        ask(dialog, Then::BuildPath);
    }

    /// Chooses the ROM content folder (Encore's DVD-ROM content / BD-ROM content ▸ Browse…).
    pub(crate) fn pick_rom_content(&mut self) {
        ask(FileDialog::folder(), Then::RomContent);
    }

    /// The ROM content folder chosen.
    pub(crate) fn rom_content_chosen(&mut self, path: &Path) {
        let path = path.to_string_lossy().into_owned();
        if self
            .run_command("disc.setRomContent", serde_json::json!({ "path": path }))
            .is_some()
        {
            self.refresh_usage();
        }
    }

    /// Takes the ROM content off the disc.
    pub(crate) fn clear_rom_content(&mut self) {
        if self
            .run_command("disc.setRomContent", serde_json::json!({ "path": null }))
            .is_some()
        {
            self.refresh_usage();
        }
    }

    /// Where a build goes when the Location box is empty: next to the saved project (else in the home folder),
    /// named after the project — a `<name>_DVD` / `<name>_BD` folder, `<name>.iso`, or a `<name>_DDP` folder.
    /// The options a render into the build's cache would use now: the Location as the build panel has it (its
    /// default when empty). `None` when that is not a valid path.
    pub(crate) fn cache_options(&self) -> Option<BuildOptions> {
        let path = if self.build_path.trim().is_empty() {
            self.default_build_path().to_string_lossy().into_owned()
        } else {
            self.build_path.trim().to_owned()
        };
        let target = build_target(
            self.build_output,
            Path::new(&path),
            &self.session.project.settings.name,
        )?;
        Some(BuildOptions {
            output: self.build_output,
            path: target,
            cache: None,
            audio_language: Some(crate::i18n::current().code().to_owned()),
        })
    }

    pub(crate) fn default_build_path(&self) -> PathBuf {
        self.default_build_path_for(self.build_output)
    }

    /// Where a build of `output` goes when Location is empty: beside the saved project (else in the home folder),
    /// named after it.
    pub(crate) fn default_build_path_for(&self, output: OutputKind) -> PathBuf {
        let saved = self.session.path.as_deref();
        let stem = saved
            .and_then(Path::file_stem)
            .and_then(|stem| stem.to_str())
            .map(str::to_owned)
            .or_else(|| Some(self.session.project.settings.name.trim().to_owned()))
            .filter(|stem| !stem.trim().is_empty())
            .map_or_else(|| "DiscCraft".to_owned(), |stem| file_safe(&stem));
        let folder = saved
            .and_then(Path::parent)
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .or_else(|| {
                std::env::var_os("USERPROFILE")
                    .or_else(|| std::env::var_os("HOME"))
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| PathBuf::from("."));
        let kind = match self.session.project.settings.kind {
            disccraft_project::DiscKind::Dvd => "DVD",
            disccraft_project::DiscKind::BluRay => "BD",
        };
        match output {
            OutputKind::Folder => folder.join(format!("{stem}_{kind}")),
            OutputKind::Iso => folder.join(format!("{stem}.iso")),
            OutputKind::Ddp => folder.join(format!("{stem}_DDP")),
        }
    }

    /// What the running build is doing, for the Build panel and the status bar: the step and how far (0 to 1).
    pub(crate) fn build_step(&self) -> (String, f32) {
        match self.build_job.current_progress() {
            None => (tl!("Preparing…").to_owned(), 0.0),
            Some((step, fraction)) => (step_text(&step), fraction),
        }
    }

    /// M6.497: every step the running build is doing (the transcodes run side by side), each named and with how far
    /// it is; empty before the first says anything.
    pub(crate) fn build_steps(&self) -> Vec<(String, f32)> {
        self.build_job
            .running_steps()
            .into_iter()
            .map(|(step, fraction)| (step_text(&step), fraction))
            .collect()
    }

    /// M6.497: the bars of the steps the running build is doing, one under another, or Preparing… before any.
    pub(crate) fn build_step_bars(&self, ui: &mut Ui) {
        step_bars(ui, &self.build_steps());
    }

    /// M6.497: every step Transcode Now is doing, named, with how far it is.
    pub(crate) fn transcode_steps(&self) -> Vec<(String, f32)> {
        self.transcode_job
            .running_steps()
            .into_iter()
            .map(|(step, fraction)| (step_text(&step), fraction))
            .collect()
    }

    /// The status bar while a build runs: the step, how far and how long so far.
    pub(crate) fn build_status_text(&self) -> Option<String> {
        if let Some((what, fraction, seconds)) = self.writing_step() {
            return Some(tlf!(
                "{step} {percent}% ({time})",
                step = what,
                // `as` saturates; the fraction is within 0..=1.
                percent = (fraction * 100.0).round() as u32,
                time = elapsed_text(seconds)
            ));
        }
        if !self.build_job.is_running() {
            return None;
        }
        let (step, fraction) = self.build_step();
        Some(tlf!(
            "Building: {step} {percent}% ({time})",
            step = step,
            // `as` saturates; the fraction is within 0..=1.
            percent = (fraction * 100.0).round() as u32,
            time = elapsed_text(self.build_job.elapsed_seconds())
        ))
    }

    /// The Build button, as in Encore CS6: the project is checked first, and when the check finds something a
    /// dialog offers to show it, to go on anyway, or to cancel. A project without problems builds at once.
    pub(crate) fn request_build(&mut self) {
        // M6.504: an image made before is written as it is; the project is not built, so it is not checked.
        if self.disc.chosen && self.disc.from_image {
            self.start_writing_image();
            return;
        }
        self.refresh_check();
        if self.problems.is_empty() {
            self.start_build();
        } else {
            self.build_prompt = true;
        }
    }

    /// The dialog of [`Self::request_build`]: Show opens the Check Project window (and does not build), Ignore and
    /// Continue builds, Cancel does nothing.
    pub(crate) fn show_build_prompt(&mut self, ctx: &egui::Context) {
        if !self.build_prompt {
            return;
        }
        let mut choice = None;
        let response = egui::Modal::new(egui::Id::unique("build_check_prompt")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.strong(tl!("Check Project"));
            ui.add_space(6.0);
            ui.label(tl!(
                "The project has problems, so the disc may not play correctly. Do you want to look at them?"
            ));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Right to left, so Cancel ends up on the right as in Encore.
                if ui.add(crate::theme::dialog_button(tl!("Cancel"))).clicked() {
                    choice = Some(0);
                }
                if ui.add(crate::theme::dialog_button(tl!("Ignore and Continue"))).clicked() {
                    choice = Some(1);
                }
                if ui.add(crate::theme::default_button(tl!("Show"))).clicked() {
                    choice = Some(2);
                }
            });
        });
        if response.should_close() {
            self.build_prompt = false;
            return;
        }
        match choice {
            Some(1) => {
                self.build_prompt = false;
                self.start_build();
            }
            Some(2) => {
                self.build_prompt = false;
                self.check_window.open = true;
            }
            Some(_) => self.build_prompt = false,
            None => {}
        }
    }

    /// The dialog of a build that failed (M6.503): why, until **OK**; **Copy** puts the error on the clipboard. A
    /// build that was stopped does not show it.
    pub(crate) fn show_build_failure(&mut self, ctx: &egui::Context) {
        let Some(error) = self.build_failure.clone() else {
            return;
        };
        let mut close = false;
        let response = egui::Modal::new(egui::Id::unique("build_failure")).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.strong(tl!("Build Failed"));
            ui.add_space(6.0);
            ui.label(tl!(
                "The build stopped with this error. What was already transcoded stays for the next build."
            ));
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .max_height(160.0)
                .show(ui, |ui| {
                    crate::theme::status_label(ui, crate::theme::Status::Error, error.as_str());
                });
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(crate::theme::default_button(tl!("OK"))).clicked() {
                    close = true;
                }
                if ui.add(crate::theme::dialog_button(tl!("Copy"))).clicked() {
                    ui.ctx().copy_text(error.clone());
                }
            });
        });
        if close || response.should_close() {
            self.build_failure = None;
        }
    }

    pub(crate) fn start_build(&mut self) {
        // A disc (M9.35): the image is built beside the project, then written to the disc in the recorder.
        let target = if self.disc.chosen {
            if self.disc.recorder.is_none() {
                self.status = tl!("No recorder").to_owned();
                return;
            }
            self.build_output = OutputKind::Iso;
            let image = self.disc_image_path();
            self.disc.image = Some(image.clone());
            self.disc.building = true;
            image
        } else {
            // An empty Location builds next to the project (Encore always has a location; a build never silently
            // does nothing).
            if self.build_path.trim().is_empty() {
                self.build_path = self.default_build_path().to_string_lossy().into_owned();
            }
            let path = self.build_path.trim().to_owned();
            let Some(target) = build_target(
                self.build_output,
                Path::new(&path),
                &self.session.project.settings.name,
            ) else {
                // Encore: an image needs a file to write, not a folder.
                self.status = tl!("The destination path is not valid.").to_string();
                return;
            };
            target
        };
        // Encore saves the project when a build starts.
        if self.session.path.is_some() && self.session.dirty {
            self.save_project(false);
        }

        let project = self.session.project.clone();
        let options = BuildOptions {
            output: self.build_output,
            path: target,
            cache: None,
            // Audio without a language of its own is in DiscCraft's display language (unless the disc says).
            audio_language: Some(crate::i18n::current().code().to_owned()),
        };

        self.build_job
            .start(project, options, build_with_progress, true);
        self.transcode_queue = self.untranscoded_assets(None);

        self.status = tl!("Build started").to_string();
        self.simulate = None;
    }

    /// Encore's Transcode Now: makes the transcodes a build would make, on a thread of its own, into the build's
    /// cache (the build's Location decides where, as for a build). `only` is one asset (the timelines that play
    /// it); `None` is the whole project.
    pub(crate) fn start_transcode_now(&mut self, only: Option<disccraft_project::AssetId>) {
        let was_running = self.transcode_job.is_running();
        self.start_cache_job(move |project, options, progress| {
            disccraft_build::transcode_now(project, options, only, progress)
        });
        if !was_running && self.transcode_job.is_running() {
            self.transcode_queue = self.untranscoded_assets(only);
        }
    }

    /// The assets a build would still transcode (`only`: just that one), for the Progress panel's list (M6.340).
    fn untranscoded_assets(&self, only: Option<disccraft_project::AssetId>) -> Vec<u64> {
        use crate::project_panel::TranscodeState;
        self.transcode
            .iter()
            .filter(|(id, state)| {
                **state == TranscodeState::Untranscoded && only.is_none_or(|only| only.0 == **id)
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// File ▸ Render ▸ Project / Motion Menu / Slideshow: makes `what` into the build's cache in the background.
    pub(crate) fn start_render(&mut self, what: disccraft_build::Render) {
        let was_running = self.transcode_job.is_running();
        self.start_cache_job(move |project, options, progress| {
            disccraft_build::render(project, options, what, progress).map(|_| ())
        });
        if !was_running
            && self.transcode_job.is_running()
            && what == disccraft_build::Render::Project
        {
            self.transcode_queue = self.untranscoded_assets(None);
        }
    }

    /// Runs `work` (a transcode or a render into the build's cache) as the background transcode job.
    fn start_cache_job(
        &mut self,
        work: impl Fn(&Project, &BuildOptions, &mut dyn FnMut(&str, f32)) -> Result<(), String>
        + Send
        + 'static,
    ) {
        if self.build_job.is_running() || self.transcode_job.is_running() {
            self.status = tl!("A build or a transcode is already running").to_string();
            return;
        }
        if self.build_path.trim().is_empty() {
            self.build_path = self.default_build_path().to_string_lossy().into_owned();
        }
        let path = self.build_path.trim().to_owned();
        let Some(target) = build_target(
            self.build_output,
            Path::new(&path),
            &self.session.project.settings.name,
        ) else {
            self.status = tl!("The destination path is not valid.").to_string();
            return;
        };
        let project = self.session.project.clone();
        let options = BuildOptions {
            output: self.build_output,
            path: target,
            cache: None,
            audio_language: Some(crate::i18n::current().code().to_owned()),
        };
        let runner = move |project: &Project,
                           options: &BuildOptions,
                           progress: &mut dyn FnMut(&str, f32)| {
            let started = Instant::now();
            work(project, options, progress)?;
            Ok(BuildReport {
                output: disccraft_build::cache_dir(options),
                titles: 0,
                chapters: 0,
                total_bytes: 0,
                seconds: started.elapsed().as_secs_f64(),
                layer_break: None,
            })
        };
        self.transcode_job.start(project, options, runner, true);
        self.status = tl!("Transcode started").to_string();
    }

    /// Encore's Revert to Original: takes `asset`'s transcodes out of the build's cache (the one Transcode Now
    /// and the build fill), so that it is "Untranscoded" again.
    pub(crate) fn revert_to_original(&mut self, asset: disccraft_project::AssetId) {
        // Transcoded files located for this kind of disc are forgotten: the build goes back to the source.
        let kind = self.session.project.settings.kind;
        let located = self
            .session
            .project
            .assets
            .get(&asset)
            .is_some_and(|found| found.located(kind).is_some());
        if located {
            let disc = match kind {
                disccraft_project::DiscKind::Dvd => "dvd",
                disccraft_project::DiscKind::BluRay => "bluray",
            };
            let params = serde_json::json!({ "asset": asset.0, "disc": disc, "video": null });
            if self.run_command("asset.locateTranscoded", params).is_some() {
                self.status = tl!("Reverted to the original").to_string();
                self.transcode_dirty = true;
            }
            return;
        }
        let path = self.build_path.trim().to_owned();
        let Some(target) = build_target(
            self.build_output,
            Path::new(&path),
            &self.session.project.settings.name,
        ) else {
            self.status = tl!("The destination path is not valid.").to_string();
            return;
        };
        let options = BuildOptions {
            output: self.build_output,
            path: target,
            cache: None,
            audio_language: None,
        };
        self.status =
            match disccraft_build::revert_to_original(&self.session.project, &options, asset) {
                Ok(_) => tl!("Reverted to the original").to_string(),
                Err(error) => error,
            };
        self.transcode_dirty = true;
    }

    /// Whether `asset` is a video (what Locate Transcoded File applies to).
    pub(crate) fn is_video_asset(&self, asset: disccraft_project::AssetId) -> bool {
        self.session
            .project
            .assets
            .get(&asset)
            .is_some_and(|found| found.kind == disccraft_project::AssetKind::Video)
    }

    /// File ▸ Locate Transcoded File ▸ DVD… / Blu-ray… (and the asset's right-click): choose the video the asset was
    /// transcoded to for that kind of disc, then its sound (cancel: none), which a build of that kind then takes in
    /// place of the asset's own file.
    pub(crate) fn locate_transcoded(
        &mut self,
        asset: disccraft_project::AssetId,
        kind: disccraft_project::DiscKind,
    ) {
        let (title, disc, extensions): (&str, &str, &[&str]) = match kind {
            disccraft_project::DiscKind::Dvd => (
                tl!("Transcoded Video for DVD"),
                "dvd",
                &["m2v", "mpg", "mpeg", "vob"],
            ),
            disccraft_project::DiscKind::BluRay => (
                tl!("Transcoded Video for Blu-ray"),
                "bluray",
                &["m2ts", "mts", "m2t", "ts", "m2v", "mp4", "m4v", "mkv"],
            ),
        };
        let start = self
            .session
            .project
            .assets
            .get(&asset)
            .and_then(|found| Path::new(&found.path).parent().map(Path::to_path_buf));
        let mut dialog = FileDialog::open()
            .title(title)
            .filter(tl!("Video"), extensions);
        if let Some(start) = start.filter(|folder| folder.is_dir()) {
            dialog = dialog.directory(start);
        }
        ask(dialog, Then::LocateVideo { asset, disc });
    }

    /// Locate Transcoded File: the video is chosen; asks for its sound (Cancel: none).
    pub(crate) fn locate_transcoded_sound(
        &mut self,
        asset: disccraft_project::AssetId,
        disc: &'static str,
        video: PathBuf,
    ) {
        let mut sound = FileDialog::open()
            .title(tl!("Its Sound (Cancel: None)"))
            .filter(tl!("Sound"), &["ac3", "wav", "mpa", "mp2", "dts"]);
        if let Some(folder) = video.parent() {
            sound = sound.directory(folder);
        }
        ask(sound, Then::LocateSound { asset, disc, video });
    }

    /// Locate Transcoded File: links the chosen video (and sound) to `asset` for a `disc` build.
    pub(crate) fn locate_transcoded_files(
        &mut self,
        asset: disccraft_project::AssetId,
        disc: &'static str,
        video: &Path,
        sound: Option<&Path>,
    ) {
        let mut params = Map::new();
        params.insert("asset".to_owned(), Value::from(asset.0));
        params.insert("disc".to_owned(), Value::from(disc));
        params.insert(
            "video".to_owned(),
            Value::from(video.to_string_lossy().into_owned()),
        );
        if let Some(sound) = &sound {
            params.insert(
                "audio".to_owned(),
                Value::from(sound.to_string_lossy().into_owned()),
            );
        }
        if self
            .run_command("asset.locateTranscoded", Value::Object(params))
            .is_some()
        {
            let name = video
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.status = tlf!("Located the transcoded file: {file}", file = name);
            self.transcode_dirty = true;
        }
    }

    /// Called as the window is drawn: says in the status bar how far Transcode Now is, and what came of it.
    pub(crate) fn poll_transcode_now(&mut self, ctx: &egui::Context) {
        if let Some(result) = self.transcode_job.poll() {
            self.status = match result {
                Ok(report) => tlf!(
                    "Transcode finished ({time})",
                    time = elapsed_text(report.seconds as u64)
                ),
                Err(error) if disccraft_build::was_stopped(&error) => {
                    tl!("Transcode stopped").to_owned()
                }
                Err(error) => tlf!("Cannot transcode: {message}", message = error),
            };
            // The statuses in the Project panel are asked for again.
            self.transcode_dirty = true;
        }
        if !self.transcode_job.is_running() {
            return;
        }
        let time = elapsed_text(self.transcode_job.elapsed_seconds());
        self.status = match self.transcode_job.current_progress() {
            Some((step, fraction)) => tlf!(
                "Transcoding {step}: {percent}% ({time})",
                step = step,
                percent = (fraction * 100.0).round() as u32,
                time = time
            ),
            None => tlf!("Transcoding… ({time})", time = time),
        };
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }

    pub(crate) fn simulate_last_build(&mut self) {
        if self.last_output_kind != Some(OutputKind::Folder) {
            self.status = tl!("Simulate is available after a folder build").to_string();
            return;
        }

        let Some(path) = self.last_build.as_ref().map(|report| report.output.clone()) else {
            self.status = tl!("There is no build result").to_string();
            return;
        };

        match simulate_folder(&path) {
            Ok(summary) => {
                self.simulate = Some(summary);
                self.status = tl!("Simulation finished").to_string();
            }
            Err(error) => self.status = error,
        }
    }
}

/// Where a build to `location` goes, as in Encore CS6: an image is the file itself (`None` when the location is
/// a folder: there is nothing to name the image); a folder build, and a DDP image (Encore's is a folder too), goes
/// into a folder named after the disc inside an existing location, unless the location already is such a folder
/// (a rebuild goes to the same place) or the disc has no name.
pub(crate) fn build_target(
    output: OutputKind,
    location: &Path,
    disc_name: &str,
) -> Option<PathBuf> {
    match output {
        OutputKind::Iso => (!location.is_dir()).then(|| location.to_path_buf()),
        OutputKind::Folder | OutputKind::Ddp => {
            let name = file_safe(disc_name.trim());
            let built = match output {
                OutputKind::Ddp => location.join("DDPID").is_file(),
                _ => location.join("VIDEO_TS").is_dir() || location.join("BDMV").is_dir(),
            };
            if location.is_dir() && !built && !name.trim().is_empty() {
                Some(location.join(name))
            } else {
                Some(location.to_path_buf())
            }
        }
    }
}

/// `name` with the characters a file name cannot hold replaced by "_".
fn file_safe(name: &str) -> String {
    name.trim()
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect()
}

/// `seconds` as "m:ss" (or "h:mm:ss").
pub(crate) fn elapsed_text(seconds: u64) -> String {
    let (hours, minutes, secs) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

#[cfg(test)]
mod step_tests {
    use super::note_step;
    use std::time::Instant;

    /// M6.497: the steps that run side by side are kept in the order they began, updated as they go, and let go
    /// when they reach 1 (a step that reports its end after it is gone adds nothing).
    #[test]
    fn running_steps_come_and_go() {
        let now = Instant::now();
        let mut steps = Vec::new();
        note_step(&mut steps, "00003", 0.1, now);
        note_step(&mut steps, "00001", 0.2, now);
        note_step(&mut steps, "00003", 0.4, now);
        let names = |steps: &Vec<(String, f32, Instant)>| -> Vec<(String, f32)> {
            steps
                .iter()
                .map(|(name, fraction, _)| (name.clone(), *fraction))
                .collect()
        };
        assert_eq!(
            names(&steps),
            [("00003".to_owned(), 0.4), ("00001".to_owned(), 0.2)]
        );
        note_step(&mut steps, "00003", 1.0, now);
        note_step(&mut steps, "00007", 1.0, now);
        assert_eq!(names(&steps), [("00001".to_owned(), 0.2)]);
    }
}

#[cfg(test)]
mod progress_tests {
    use super::{elapsed_text, file_safe};

    #[test]
    fn elapsed_time_and_file_names() {
        assert_eq!(elapsed_text(0), "0:00");
        assert_eq!(elapsed_text(75), "1:15");
        assert_eq!(elapsed_text(3_725), "1:02:05");
        assert_eq!(file_safe(" a/b:c? "), "a_b_c_");
        assert_eq!(file_safe("映画"), "映画");
    }
}

#[cfg(test)]
mod build_target_tests {
    use super::build_target;
    use disccraft_build::OutputKind;

    /// As in Encore CS6: a folder build goes into `<location>/<disc name>`, an image needs a file name.
    #[test]
    fn a_build_goes_where_encore_puts_it() -> Result<(), Box<dyn std::error::Error>> {
        let dir =
            std::env::temp_dir().join(format!("disccraft-build-target-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        assert_eq!(
            build_target(OutputKind::Folder, &dir, "Untitled Project"),
            Some(dir.join("Untitled Project"))
        );
        assert_eq!(
            build_target(OutputKind::Folder, &dir, " "),
            Some(dir.clone())
        );
        // A folder that does not exist yet is the disc folder itself.
        let fresh = dir.join("movie_DVD");
        assert_eq!(
            build_target(OutputKind::Folder, &fresh, "Movie"),
            Some(fresh.clone())
        );
        // A rebuild into a disc folder stays there.
        std::fs::create_dir_all(fresh.join("VIDEO_TS"))?;
        assert_eq!(
            build_target(OutputKind::Folder, &fresh, "Movie"),
            Some(fresh)
        );
        // A DDP image goes into a folder named after the disc too, and a rebuild into it stays there.
        assert_eq!(
            build_target(OutputKind::Ddp, &dir, "Untitled Project"),
            Some(dir.join("Untitled Project"))
        );
        let ddp = dir.join("Movie");
        std::fs::create_dir_all(&ddp)?;
        std::fs::write(ddp.join("DDPID"), b"")?;
        assert_eq!(build_target(OutputKind::Ddp, &ddp, "Movie"), Some(ddp));
        // An image: a folder is refused, a file is taken.
        assert_eq!(build_target(OutputKind::Iso, &dir, "Movie"), None);
        assert_eq!(
            build_target(OutputKind::Iso, &dir.join("movie.iso"), "Movie"),
            Some(dir.join("movie.iso"))
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
