use crate::app::DiscCraftApp;
use crate::i18n::{tl, tlf};

use disccraft_build::{BuildOptions, BuildReport, OutputKind, build};
use disccraft_project::Project;
use disccraft_vm::{Report, explore, load_video_ts};

use egui::{self, Ui};
use rfd::FileDialog;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::thread;

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
}

impl BuildJob {
    pub(crate) fn begin(&mut self) {
        self.state = BuildState::Running;
        self.result = None;
        self.rx = None;
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
        F: FnOnce(&Project, &BuildOptions) -> Result<BuildReport, String> + Send + 'static,
    {
        self.begin();
        self.output_kind = Some(options.output);

        if background {
            let (tx, rx) = channel();
            self.rx = Some(rx);

            let _ = thread::spawn(move || {
                let result = runner(&project, &options);
                let _ = tx.send(result);
            });
        } else {
            self.finish(runner(&project, &options));
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

    pub(crate) fn start_build(&mut self) {
        let path = self.build_path.trim();

        if path.is_empty() {
            self.status = tl!("Enter an output path").to_string();
            return;
        }

        let project = self.session.project.clone();
        let options = BuildOptions {
            output: self.build_output,
            path: PathBuf::from(path),
        };

        self.build_job.start(project, options, build, true);

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
