use crate::app::DiscCraftApp;
use crate::i18n::{tl, tlf};

use disccraft_build::{BuildOptions, BuildReport, OutputKind, STEP_IMAGE, build_with_progress};
use disccraft_project::Project;
use disccraft_vm::{Report, explore, load_video_ts};

use egui::{self, Ui};
use rfd::FileDialog;
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
    /// When the running build started.
    started: Option<Instant>,
}

impl BuildJob {
    pub(crate) fn begin(&mut self) {
        self.state = BuildState::Running;
        self.result = None;
        self.rx = None;
        self.started = Some(Instant::now());
        self.progress = Arc::new(Mutex::new(None));
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
        let mut report = move |step: &str, fraction: f32| {
            if let Ok(mut guard) = shared.lock() {
                *guard = Some((step.to_owned(), fraction.clamp(0.0, 1.0)));
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

#[derive(Clone, Debug)]
pub(crate) struct Usage {
    pub(crate) estimated_bytes: u64,
    pub(crate) capacity_bytes: u64,
    /// タイムラインごとのエンコード後のサイズの見積もり（バイト）。
    pub(crate) timelines: std::collections::BTreeMap<u64, u64>,
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

    Some(Usage {
        estimated_bytes,
        capacity_bytes,
        timelines,
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

pub(crate) fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
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

pub(crate) fn problem_color(severity: &str) -> egui::Color32 {
    let lower = severity.to_lowercase();

    if lower.contains("error") {
        egui::Color32::RED
    } else if lower.contains("warn") {
        egui::Color32::YELLOW
    } else {
        egui::Color32::GRAY
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
                    ui.colored_label(egui::Color32::RED, tl!("Dead ends"));

                    for line in &summary.dead_ends {
                        ui.label(line);
                    }
                }

                if !summary.loops.is_empty() {
                    ui.colored_label(egui::Color32::YELLOW, tl!("Loops"));

                    for line in &summary.loops {
                        ui.label(line);
                    }
                }
            });
    }

    pub(crate) fn pick_build_path(&mut self) {
        let path = match self.build_output {
            OutputKind::Folder => FileDialog::new().pick_folder(),
            OutputKind::Iso => FileDialog::new().add_filter("ISO", &["iso"]).save_file(),
        };

        if let Some(path) = path {
            self.build_path = path.to_string_lossy().to_string();
        }
    }

    /// Where a build goes when the Location box is empty: next to the saved project (else in the home folder),
    /// named after the project — a `<name>_DVD` / `<name>_BD` folder, or `<name>.iso`.
    pub(crate) fn default_build_path(&self) -> PathBuf {
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
        match self.build_output {
            OutputKind::Folder => folder.join(format!("{stem}_{kind}")),
            OutputKind::Iso => folder.join(format!("{stem}.iso")),
        }
    }

    /// What the running build is doing, for the Build panel and the status bar: the step and how far (0 to 1).
    pub(crate) fn build_step(&self) -> (String, f32) {
        match self.build_job.current_progress() {
            None => (tl!("Preparing…").to_owned(), 0.0),
            Some((step, fraction)) if step == STEP_IMAGE => {
                (tl!("Writing the disc image").to_owned(), fraction)
            }
            Some((step, fraction)) => (tlf!("Transcoding {name}", name = step), fraction),
        }
    }

    /// The status bar while a build runs: the step, how far and how long so far.
    pub(crate) fn build_status_text(&self) -> Option<String> {
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

    pub(crate) fn start_build(&mut self) {
        // An empty Location builds next to the project (Encore always has a location; a build never silently
        // does nothing).
        if self.build_path.trim().is_empty() {
            self.build_path = self.default_build_path().to_string_lossy().into_owned();
        }
        let path = self.build_path.trim().to_owned();

        let project = self.session.project.clone();
        let options = BuildOptions {
            output: self.build_output,
            path: PathBuf::from(path),
            cache: None,
            // Audio without a language of its own is in DiscCraft's display language (unless the disc says).
            audio_language: Some(crate::i18n::current().code().to_owned()),
        };

        self.build_job
            .start(project, options, build_with_progress, true);

        self.status = tl!("Build started").to_string();
        self.simulate = None;
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
