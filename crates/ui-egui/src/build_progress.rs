//! The Build Progress dialog (M6.505, Encore's ビルドの進行状況): while a build or a disc writing runs it sits in the
//! middle of the window over a dimmed backdrop, and the rest of the window cannot be used, as in Encore (its build
//! dialog is modal). It shows what the build is doing (a bar for each step: the transcodes run side by side) or what
//! the writing is doing (writing, closing, verifying the disc), the time so far, for a writing the time left at its
//! pace, and Cancel (a build) or Stop (a writing).
//!
//! The window can still be minimized: the taskbar button then shows how far the job is (Windows; a bar that moves
//! while a build runs, since its steps run side by side, and the share written or verified while a disc is written),
//! red when it failed, and it lights up when the job ends while the window is not in front.

use crate::app::DiscCraftApp;
use crate::disc_writer::WriterPrompt;
use crate::i18n::{tl, tlf};

use disccraft_platform::BurnStep;
use disccraft_platform::taskbar::{self, TaskbarProgress};
use egui::{self, Context};
use std::time::Instant;

/// The main window's title, by which the system's taskbar button is found.
const WINDOW_TITLE: &str = "DiscCraft";

/// What the dialog keeps from one frame to the next.
#[derive(Default)]
pub(crate) struct ProgressWatch {
    /// A build or a writing ran at the last frame (its end lights the taskbar button).
    busy: bool,
    /// What the taskbar button was last told.
    told: Option<Told>,
    /// The writing's step being timed: which, when it was first seen and how far it was then.
    clock: Option<(BurnStep, Instant, f32)>,
}

/// What the taskbar button shows, as compared from one frame to the next (a bar in thousandths).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Told {
    None,
    Indeterminate,
    Normal(u32),
    Error,
}

impl Told {
    /// What `progress` comes to on the button.
    fn of(progress: TaskbarProgress) -> Self {
        match progress {
            TaskbarProgress::None => Self::None,
            TaskbarProgress::Indeterminate => Self::Indeterminate,
            // `as` saturates; the fraction is within 0..=1.
            TaskbarProgress::Normal(fraction) => {
                Self::Normal((fraction.clamp(0.0, 1.0) * 1000.0).round() as u32)
            }
            TaskbarProgress::Error => Self::Error,
        }
    }
}

/// The seconds a step has left that went from `from` to `now` (fractions of it) in `seconds`, at that pace; none
/// before it has gone far enough to tell (10 seconds and 1 %).
pub(crate) fn seconds_left(seconds: f64, from: f32, now: f32) -> Option<u64> {
    let done = f64::from(now - from);
    if !(seconds >= 10.0 && done >= 0.01) {
        return None;
    }
    let left = seconds * f64::from((1.0 - now).max(0.0)) / done;
    // `as` saturates; the value is finite and not negative.
    left.is_finite().then(|| left.round() as u64)
}

impl DiscCraftApp {
    /// A build or a disc writing runs: the Build Progress dialog is up and the rest of the window waits.
    pub(crate) fn job_running(&self) -> bool {
        self.build_job.is_running() || self.disc.is_writing()
    }

    /// While the dialog is up, the keys go nowhere (the menus' shortcuts, the Monitor's, typing in a field below):
    /// called before anything reads them.
    pub(crate) fn hold_keys(&self, ctx: &Context) {
        if !self.job_running() {
            return;
        }
        ctx.input_mut(|input| {
            input.events.retain(|event| {
                !matches!(
                    event,
                    egui::Event::Key { .. }
                        | egui::Event::Text(_)
                        | egui::Event::Copy
                        | egui::Event::Cut
                        | egui::Event::Paste(_)
                        | egui::Event::Ime(_)
                )
            });
            input.keys_down.clear();
        });
    }

    /// The Build Progress dialog while a build or a writing runs, and the taskbar button after it.
    pub(crate) fn show_build_progress(&mut self, ctx: &Context) {
        let running = self.job_running();
        self.update_taskbar(ctx, running);
        if !running {
            self.progress_watch.clock = None;
            return;
        }
        let writing = self.writing_step();
        let mut stop = false;
        let mut cancel = false;
        let _ = egui::Modal::new(egui::Id::unique("build_progress")).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.strong(tl!("Build Progress"));
            ui.add_space(6.0);
            let left = match &writing {
                Some((what, fraction, _)) => {
                    ui.label(what.as_str());
                    ui.add(
                        egui::ProgressBar::new(*fraction)
                            .show_percentage()
                            .animate(true),
                    );
                    self.writing_seconds_left(*fraction)
                }
                None => {
                    self.build_step_bars(ui);
                    None
                }
            };
            let elapsed = writing.as_ref().map_or_else(
                || self.build_job.elapsed_seconds(),
                |(_, _, seconds)| *seconds,
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(tlf!(
                    "Elapsed: {time}",
                    time = crate::build_panel::elapsed_text(elapsed)
                ));
                if let Some(left) = left {
                    ui.add_space(12.0);
                    ui.label(tlf!(
                        "Time left: about {time}",
                        time = crate::build_panel::elapsed_text(left)
                    ));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if writing.is_some() {
                        stop = ui
                            .add(crate::theme::dialog_button(tl!("Stop")))
                            .on_hover_text(tl!(
                                "Stops writing; a write-once disc stopped part way cannot be used again"
                            ))
                            .clicked();
                    } else {
                        let label = if self.build_job.cancelling {
                            tl!("Cancelling…")
                        } else {
                            tl!("Cancel")
                        };
                        cancel = ui
                            .add_enabled(!self.build_job.cancelling, crate::theme::dialog_button(label))
                            .on_hover_text(tl!(
                                "Stops the build; what is already transcoded is kept for the next build"
                            ))
                            .clicked();
                    }
                });
            });
            if cfg!(windows) {
                ui.add_space(4.0);
                ui.weak(tl!(
                    "The window can be minimized: the taskbar button shows how far it is."
                ));
            }
        });
        if stop {
            self.disc.stop();
            self.status = tl!("Stopping…").to_owned();
        }
        if cancel {
            disccraft_build::request_stop();
            self.build_job.cancelling = true;
            self.status = tl!("Stopping…").to_owned();
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }

    /// The time the writing's step has left at its pace so far (none for closing the disc, which has no pace).
    fn writing_seconds_left(&mut self, fraction: f32) -> Option<u64> {
        let (step, _, _) = self.disc.progress()?;
        let now = Instant::now();
        match self.progress_watch.clock {
            Some((timed, _, _)) if timed == step => {}
            _ => self.progress_watch.clock = Some((step, now, fraction)),
        }
        let (_, since, from) = self.progress_watch.clock?;
        if step == BurnStep::Closing {
            return None;
        }
        seconds_left(now.duration_since(since).as_secs_f64(), from, fraction)
    }

    /// Tells the taskbar button how far the job is, when that changed; when a job has just ended, lights the button
    /// unless the window is in front.
    fn update_taskbar(&mut self, ctx: &Context, running: bool) {
        let failed = self.build_failure.is_some()
            || matches!(self.disc.prompt, Some(WriterPrompt::Failed(_)));
        let progress = if self.disc.is_writing() {
            match self.disc.progress() {
                Some((BurnStep::Closing | BurnStep::Erasing, _, _)) => {
                    TaskbarProgress::Indeterminate
                }
                Some((_, fraction, _)) => TaskbarProgress::Normal(fraction),
                None => TaskbarProgress::Indeterminate,
            }
        } else if running {
            TaskbarProgress::Indeterminate
        } else if failed {
            TaskbarProgress::Error
        } else {
            TaskbarProgress::None
        };
        let told = Told::of(progress);
        // Nothing to tell a button that never showed anything.
        let first_plain = self.progress_watch.told.is_none() && told == Told::None;
        if !first_plain && self.progress_watch.told != Some(told) {
            let _ = taskbar::set_progress(WINDOW_TITLE, progress);
            self.progress_watch.told = Some(told);
        }
        let ended = self.progress_watch.busy && !running;
        self.progress_watch.busy = running;
        let in_front = ctx.input(|input| input.viewport().focused) == Some(true);
        if ended && !in_front {
            let _ = taskbar::flash(WINDOW_TITLE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Told, seconds_left};
    use crate::app::DiscCraftApp;
    use disccraft_platform::taskbar::TaskbarProgress;

    /// The time left is the pace so far carried to the end; none too early to tell.
    #[test]
    fn the_time_left_follows_the_pace_so_far() {
        // 10 % in 60 seconds: 90 % in 540.
        assert_eq!(seconds_left(60.0, 0.0, 0.1), Some(540));
        // Timed from 50 %: 10 % more in 100 seconds, 40 % left.
        assert_eq!(seconds_left(100.0, 0.5, 0.6), Some(400));
        assert_eq!(seconds_left(5.0, 0.0, 0.5), None);
        assert_eq!(seconds_left(600.0, 0.2, 0.2), None);
        assert_eq!(seconds_left(600.0, 0.5, 0.4), None);
        assert_eq!(seconds_left(f64::NAN, 0.0, 0.5), None);
        assert_eq!(seconds_left(60.0, 0.0, 1.0), Some(0));
    }

    /// The button's bar is compared in thousandths, so that it is told again only when it moved.
    #[test]
    fn the_taskbar_bar_is_told_in_thousandths() {
        assert_eq!(Told::of(TaskbarProgress::Normal(0.5)), Told::Normal(500));
        assert_eq!(
            Told::of(TaskbarProgress::Normal(0.50001)),
            Told::Normal(500)
        );
        assert_eq!(Told::of(TaskbarProgress::Normal(2.0)), Told::Normal(1000));
        assert_eq!(Told::of(TaskbarProgress::None), Told::None);
    }

    /// Without a build or a writing the dialog is not up and the keys go through.
    #[test]
    fn no_job_holds_no_keys() {
        let ctx = egui::Context::default();
        let app = DiscCraftApp::new(&ctx);
        assert!(!app.job_running());
    }
}
