//! Writing the built disc to a recordable disc (M9.35): Encore's Build panel output "DVD Disc" / "Blu-ray Disc".
//! The project is built into a disc image beside it, the image is written to the disc in the chosen recorder
//! (`disccraft_platform::burn`, IMAPI2 on Windows), once for each copy, and the image is removed when all copies are
//! written or the writing is given up. With the Source "Disc image" (M6.504, Encore's 使用するソース ディスクイメージ)
//! an image made before is written as it is instead: nothing is built, and the image stays.

use crate::app::DiscCraftApp;
use crate::i18n::{tl, tlf};

use disccraft_platform::burn::{self, BurnError, BurnRequest, BurnStep, Media, Recorder};
use egui::{self, Ui};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// The most copies written in one go (Encore's Copies box).
pub(crate) const MAX_COPIES: u32 = 99;

/// The sectors of a DVD of one layer (DVD±R): an image past them goes on two layers, which meet at its layer break.
const DVD_LAYER_SECTORS: u64 = 2_295_104;

/// The bytes of a disc sector.
const SECTOR_BYTES: u64 = 2048;

/// The Build panel's disc output and the writing it starts.
pub(crate) struct DiscWriter {
    /// The Output list's "Disc" is chosen (the build makes an image to write).
    pub(crate) chosen: bool,
    /// The Source list's "Disc image" is chosen (M6.504): Build writes [`Self::source_image`] as it is.
    pub(crate) from_image: bool,
    /// The disc image the Source names (its path as typed or browsed to).
    pub(crate) source_image: String,
    /// The session writes the Source's own image, which stays when the session ends (the project's is removed).
    pub(crate) keep_image: bool,
    /// The recorders found, once looked for.
    pub(crate) recorders: Option<Vec<Recorder>>,
    /// The recorder chosen (its id).
    pub(crate) recorder: Option<String>,
    /// The disc in the chosen recorder, as last looked at (`None` before that).
    pub(crate) media: Option<Result<Option<Media>, BurnError>>,
    /// The write speed chosen (sectors a second), or the fastest.
    pub(crate) speed: Option<u32>,
    /// How many discs to write.
    pub(crate) copies: u32,
    /// Encore's "Auto-erase rewritable discs".
    pub(crate) erase_rewritable: bool,
    /// DiscCraft's "Verify the disc after writing" (M9.38): read back and compared with the image before it comes out.
    pub(crate) verify: bool,
    /// The image being written, while a writing session lasts (removed at its end).
    pub(crate) image: Option<PathBuf>,
    /// The build running now makes the image of a writing session.
    pub(crate) building: bool,
    /// The copy being written (or to be written next), from 1.
    pub(crate) copy: u32,
    /// The writing running now.
    job: Option<BurnJob>,
    /// What the writing asks of the user between copies or after a failure.
    pub(crate) prompt: Option<WriterPrompt>,
}

impl Default for DiscWriter {
    fn default() -> Self {
        Self {
            chosen: false,
            from_image: false,
            source_image: String::new(),
            keep_image: false,
            recorders: None,
            recorder: None,
            media: None,
            speed: None,
            copies: 1,
            erase_rewritable: false,
            verify: true,
            image: None,
            building: false,
            copy: 1,
            job: None,
            prompt: None,
        }
    }
}

/// A writing on its own thread.
struct BurnJob {
    rx: Receiver<Result<(), BurnError>>,
    progress: Arc<Mutex<(BurnStep, f32)>>,
    cancel: Arc<AtomicBool>,
    started: Instant,
}

/// What the writing waits for the user to answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WriterPrompt {
    /// A copy is written and more are to come: the next blank disc goes in.
    NextCopy,
    /// The writing failed, with why: try again with the same image, or give up.
    Failed(String),
    /// The Source's disc image cannot be written as it is, with why (M6.504): nothing was written.
    Refused(String),
}

/// Why the Source's disc image is not written (M6.504).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceImageProblem {
    /// No file is named.
    NotChosen,
    /// The file is not there, or cannot be read.
    Missing,
    /// It is empty or not whole 2048-byte sectors: not a disc image.
    NotAnImage,
    /// A DVD image larger than one layer, for a dual-layer disc, without the layer break note DiscCraft's build
    /// writes beside it (`<name>.layerbreak.txt`): the drive would put the layer change anywhere.
    NoLayerBreak,
}

/// Whether an image of `bytes` (`None`: not readable) with the layer break note `layer_break` can be written to a disc
/// that is a dual-layer DVD or not (M6.504).
fn source_image_problem(
    bytes: Option<u64>,
    layer_break: Option<u32>,
    dual_layer_dvd: bool,
) -> Option<SourceImageProblem> {
    let bytes = match bytes {
        None => return Some(SourceImageProblem::Missing),
        Some(bytes) => bytes,
    };
    if bytes == 0 || bytes % SECTOR_BYTES != 0 {
        return Some(SourceImageProblem::NotAnImage);
    }
    if dual_layer_dvd && bytes / SECTOR_BYTES > DVD_LAYER_SECTORS && layer_break.is_none() {
        return Some(SourceImageProblem::NoLayerBreak);
    }
    None
}

/// The Source's disc image `typed` as a file to write to the disc `media` (the one in the recorder, when known), or
/// why it cannot be (M6.504).
pub(crate) fn check_source_image(
    typed: &str,
    media: Option<&Media>,
) -> Result<PathBuf, SourceImageProblem> {
    let typed = typed.trim().trim_matches('"');
    if typed.is_empty() {
        return Err(SourceImageProblem::NotChosen);
    }
    let path = Path::new(typed);
    let bytes = std::fs::metadata(path)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map(|metadata| metadata.len());
    let dual_layer_dvd = media.is_some_and(|media| media.kind.dual_layer());
    match source_image_problem(
        bytes,
        disccraft_build::image_layer_break(path),
        dual_layer_dvd,
    ) {
        Some(problem) => Err(problem),
        None => Ok(path.to_path_buf()),
    }
}

/// The user's words for why the Source's disc image is not written.
pub(crate) fn source_image_text(problem: SourceImageProblem) -> String {
    match problem {
        SourceImageProblem::NotChosen => tl!("Choose the disc image to write.").to_owned(),
        SourceImageProblem::Missing => tl!("The disc image cannot be found.").to_owned(),
        SourceImageProblem::NotAnImage => {
            tl!("The file is not a disc image (it is not made of whole 2,048-byte sectors).").to_owned()
        }
        SourceImageProblem::NoLayerBreak => tl!(
            "The image is larger than one layer, but its layer break is not known. DiscCraft writes it beside the images it builds (name.layerbreak.txt): build the disc image again with DiscCraft."
        )
        .to_owned(),
    }
}

impl DiscWriter {
    /// Whether a disc is being written.
    pub(crate) fn is_writing(&self) -> bool {
        self.job.is_some()
    }

    /// Whether a writing session (its build, a writing, or a question between them) is going on.
    pub(crate) fn is_busy(&self) -> bool {
        self.building || self.job.is_some() || self.prompt.is_some()
    }

    /// The writing's step and how far it is (0 to 1), and how long it has taken in whole seconds.
    pub(crate) fn progress(&self) -> Option<(BurnStep, f32, u64)> {
        self.job.as_ref().map(|job| {
            let (step, fraction) = job
                .progress
                .lock()
                .map_or((BurnStep::Writing, 0.0), |guard| *guard);
            (step, fraction, job.started.elapsed().as_secs())
        })
    }

    /// Stops the writing at its next read of the image.
    pub(crate) fn stop(&self) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// The chosen recorder, as found.
    pub(crate) fn chosen_recorder(&self) -> Option<&Recorder> {
        let id = self.recorder.as_deref()?;
        self.recorders
            .as_deref()?
            .iter()
            .find(|recorder| recorder.id == id)
    }
}

/// The user's words for why a disc could not be written.
pub(crate) fn burn_error_text(error: &BurnError) -> String {
    match error {
        BurnError::Unsupported => {
            tl!("Writing discs is not available on this system yet.").to_owned()
        }
        BurnError::NoRecorder => tl!("The recorder is not available.").to_owned(),
        BurnError::NoMedia => tl!("There is no disc in the recorder.").to_owned(),
        BurnError::MediaNotSupported(kind) => tlf!(
            "The disc in the recorder ({kind}) cannot be written.",
            kind = kind.name()
        ),
        BurnError::NotBlank => tl!(
            "The disc is not blank. Insert a blank disc (or turn on Auto-erase rewritable discs for a rewritable one)."
        )
        .to_owned(),
        BurnError::TooLarge { needed, free } => tlf!(
            "The disc is too small: the project needs {needed}, the disc has {free}.",
            needed = crate::build_panel::format_bytes(*needed),
            free = crate::build_panel::format_bytes(*free)
        ),
        BurnError::WriteProtected => tl!("The disc is write-protected.").to_owned(),
        BurnError::Busy => tl!("The recorder is busy. Wait a moment and try again.").to_owned(),
        BurnError::Cancelled => tl!("Writing stopped").to_owned(),
        BurnError::Image(message) => {
            tlf!("The disc image cannot be read: {message}", message = message)
        }
        BurnError::VerifyMismatch { sector } => tlf!(
            "Verify failed: the disc differs from the image from sector {sector}. Write it again on another disc.",
            sector = sector
        ),
        BurnError::VerifyRead(message) => tlf!(
            "Verify failed: the disc cannot be read back ({message}).",
            message = message
        ),
        BurnError::LayerBreak {
            wanted,
            disc: 0,
            ..
        } => tlf!(
            "The drive does not tell where the dual-layer disc's first layer ends, so the layer break (sector {wanted}) cannot be placed.",
            wanted = wanted
        ),
        BurnError::LayerBreak {
            wanted,
            disc,
            fixed: true,
        } => tlf!(
            "The disc's first layer is fixed at {disc} sectors, but the image's layer break is at sector {wanted}. Use a blank DVD+R DL.",
            disc = disc,
            wanted = wanted
        ),
        BurnError::LayerBreak { wanted, disc, .. } => tlf!(
            "The image's first layer ({wanted} sectors) is larger than the disc's ({disc} sectors).",
            wanted = wanted,
            disc = disc
        ),
        BurnError::LayerBreakMoved { wanted, written } => tlf!(
            "The written disc's first layer ends after {written} sectors, not at the image's layer break ({wanted}).",
            written = written,
            wanted = wanted
        ),
        BurnError::Drive(message) => {
            tlf!("The recorder failed: {message}", message = message)
        }
        BurnError::StartedBefore { layer0 } => tlf!(
            "A writing stopped on this DVD+R DL before: the drive calls it blank, but its first layer is fixed at {layer0} sectors, so part of it may be written. Use a blank DVD+R DL.",
            layer0 = layer0
        ),
        BurnError::System { .. } => error.to_string(),
    }
}

/// A line about the disc in the recorder.
fn media_text(media: &Result<Option<Media>, BurnError>) -> String {
    match media {
        Ok(None) => tl!("No disc").to_owned(),
        Ok(Some(media)) if media.kind.read_only() => tlf!(
            "{kind}: cannot be written (a disc already written and closed, or a pressed one)",
            kind = media.kind.name()
        ),
        Ok(Some(media)) if !media.supported => {
            tlf!("{kind}: cannot be written", kind = media.kind.name())
        }
        // M9.47: it calls itself blank, but a writing stopped on it.
        Ok(Some(media)) if media.started_before() => tlf!(
            "{kind}: a writing stopped on it before (its first layer is fixed), so it cannot be written",
            kind = media.kind.name()
        ),
        Ok(Some(media)) if media.blank => tlf!(
            "{kind}, blank, {free} free",
            kind = media.kind.name(),
            free = crate::build_panel::format_bytes(media.free_bytes)
        ),
        Ok(Some(media)) if media.kind.rewritable() => tlf!(
            "{kind}, rewritable, with data on it",
            kind = media.kind.name()
        ),
        Ok(Some(media)) => tlf!("{kind}, not blank", kind = media.kind.name()),
        Err(error) => burn_error_text(error),
    }
}

impl DiscCraftApp {
    /// Looks for the recorders again (Encore's Refresh), keeps the one chosen when it is still there, else takes the
    /// first, and looks at its disc.
    pub(crate) fn refresh_recorders(&mut self) {
        let found = burn::recorders().unwrap_or_default();
        let keep = self
            .disc
            .recorder
            .as_ref()
            .is_some_and(|id| found.iter().any(|recorder| &recorder.id == id));
        if !keep {
            self.disc.recorder = found.first().map(|recorder| recorder.id.clone());
        }
        self.disc.recorders = Some(found);
        self.refresh_media();
    }

    /// Looks at the disc in the chosen recorder (and drops a write speed it does not offer).
    pub(crate) fn refresh_media(&mut self) {
        self.disc.media = self.disc.recorder.as_deref().map(burn::media);
        let speeds: Vec<u32> = match &self.disc.media {
            Some(Ok(Some(media))) => media.write_speeds.clone(),
            _ => Vec::new(),
        };
        if self
            .disc
            .speed
            .is_some_and(|speed| !speeds.contains(&speed))
        {
            self.disc.speed = None;
        }
    }

    /// The Destination group for a disc (Encore's: Recorder [Refresh], Write Speed, Copies, Run test before
    /// writing, Auto-erase rewritable discs).
    pub(crate) fn disc_destination_section(&mut self, ui: &mut Ui) {
        if self.disc.recorders.is_none() {
            self.refresh_recorders();
        }
        let recorders = self.disc.recorders.clone().unwrap_or_default();
        let mut chosen = None;
        let mut refresh = false;
        crate::theme::group_box(ui, "build_destination", tl!("Destination"), |ui| {
            egui::Grid::new("build_disc_grid")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label(tl!("Recorder:"));
                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 80.0).max(160.0);
                        let current = self
                            .disc
                            .chosen_recorder()
                            .map_or_else(|| tl!("Not found").to_owned(), Recorder::label);
                        egui::ComboBox::from_id_salt("build_recorder")
                            .width(width)
                            .selected_text(current)
                            .show_ui(ui, |ui| {
                                for recorder in &recorders {
                                    let on = self.disc.recorder.as_deref() == Some(recorder.id.as_str());
                                    if ui.selectable_label(on, recorder.label()).clicked() {
                                        chosen = Some(recorder.id.clone());
                                    }
                                }
                            });
                        if ui.button(tl!("Refresh")).clicked() {
                            refresh = true;
                        }
                    });
                    ui.end_row();

                    ui.label(tl!("Write Speed:"));
                    ui.horizontal(|ui| {
                        self.write_speed_combo(ui);
                        ui.add_space(16.0);
                        ui.label(tl!("Copies:"));
                        ui.add(egui::DragValue::new(&mut self.disc.copies).range(1..=MAX_COPIES));
                    });
                    ui.end_row();

                    ui.label("");
                    ui.vertical(|ui| {
                        let mut test = false;
                        ui.add_enabled(false, egui::Checkbox::new(&mut test, tl!("Run test before writing")))
                            .on_disabled_hover_text(tl!(
                                "Windows' disc writing (IMAPI2) has no test mode: the disc is written at once"
                            ));
                        ui.checkbox(&mut self.disc.erase_rewritable, tl!("Auto-erase rewritable discs"));
                        // M9.38: DiscCraft's own (Encore has no verify).
                        ui.checkbox(&mut self.disc.verify, tl!("Verify the disc after writing"))
                            .on_hover_text(tl!(
                                "Reads the disc back and compares it with the image, sector by sector, before it comes out"
                            ));
                    });
                    ui.end_row();

                    ui.label(tl!("Disc:"));
                    let line = match &self.disc.media {
                        // M6.505: the disc as it was looked at before the writing is not what it is now.
                        Some(Ok(Some(media))) if self.disc.is_writing() => tlf!(
                            "{kind}: being written",
                            kind = media.kind.name()
                        ),
                        Some(media) => media_text(media),
                        None => tl!("No recorder").to_owned(),
                    };
                    ui.label(line);
                    ui.end_row();
                });
        });
        if let Some(id) = chosen {
            self.disc.recorder = Some(id);
            self.refresh_media();
        }
        if refresh {
            self.refresh_recorders();
        }
    }

    /// The write speeds the disc in the recorder offers, fastest first, after "Maximum".
    fn write_speed_combo(&mut self, ui: &mut Ui) {
        let media = match &self.disc.media {
            Some(Ok(Some(media))) => Some(media.clone()),
            _ => None,
        };
        let speeds = media
            .as_ref()
            .map(|media| media.write_speeds.clone())
            .unwrap_or_default();
        let text = |speed: Option<u32>| match (speed, &media) {
            (Some(speed), Some(media)) => format!("{}x", media.kind.speed_factor(speed)),
            _ => tl!("Maximum").to_owned(),
        };
        let mut chosen = None;
        ui.add_enabled_ui(!speeds.is_empty(), |ui| {
            egui::ComboBox::from_id_salt("build_write_speed")
                .selected_text(text(self.disc.speed))
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.disc.speed.is_none(), text(None))
                        .clicked()
                    {
                        chosen = Some(None);
                    }
                    for speed in &speeds {
                        if ui
                            .selectable_label(self.disc.speed == Some(*speed), text(Some(*speed)))
                            .clicked()
                        {
                            chosen = Some(Some(*speed));
                        }
                    }
                });
        });
        if let Some(speed) = chosen {
            self.disc.speed = speed;
        }
    }

    /// Where the image of a writing session is built: beside the project, removed when the writing ends.
    pub(crate) fn disc_image_path(&self) -> PathBuf {
        let image = self.default_build_path_for(disccraft_build::OutputKind::Iso);
        let stem = image
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "DiscCraft".to_owned());
        image.with_file_name(format!("{stem}.disccraft-disc.iso"))
    }

    /// Starts writing the session's image to the disc in the chosen recorder: copy `self.disc.copy` of
    /// `self.disc.copies`.
    pub(crate) fn start_writing(&mut self) {
        let (Some(image), Some(recorder)) = (self.disc.image.clone(), self.disc.recorder.clone())
        else {
            self.end_writing(tl!("No recorder").to_owned());
            return;
        };
        let image_path = image.clone();
        let request = BurnRequest {
            image,
            recorder,
            speed: self.disc.speed,
            erase_rewritable: self.disc.erase_rewritable,
            eject: true,
            verify: self.disc.verify,
            // M9.42: a DVD-9 image's layer break, from the note the build writes beside it.
            layer_break: disccraft_build::image_layer_break(&image_path),
        };
        let progress = Arc::new(Mutex::new((BurnStep::Writing, 0.0_f32)));
        let shared = Arc::clone(&progress);
        let report: Arc<dyn Fn(BurnStep, f32) + Send + Sync> = Arc::new(move |step, fraction| {
            if let Ok(mut guard) = shared.lock() {
                *guard = (step, fraction.clamp(0.0, 1.0));
            }
        });
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancel);
        let (tx, rx) = channel();
        let _ = std::thread::Builder::new()
            .name("disccraft-burn".to_owned())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    burn::burn(&request, report, stop)
                }))
                .unwrap_or_else(|_| {
                    Err(BurnError::System {
                        code: 0,
                        message: "the disc writing stopped unexpectedly".to_owned(),
                    })
                });
                let _ = tx.send(result);
            });
        self.disc.job = Some(BurnJob {
            rx,
            progress,
            cancel,
            started: Instant::now(),
        });
        self.status = tlf!(
            "Writing the disc (copy {copy} of {copies})",
            copy = self.disc.copy,
            copies = self.disc.copies
        );
    }

    /// Looks at the writing: when a copy is done, asks for the next disc or ends; when it failed, asks whether to
    /// try again.
    pub(crate) fn poll_writing(&mut self, ctx: &egui::Context) {
        let outcome = match &self.disc.job {
            Some(job) => match job.rx.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err(BurnError::System {
                    code: 0,
                    message: "the disc writing stopped unexpectedly".to_owned(),
                })),
            },
            None => return,
        };
        let Some(result) = outcome else {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            return;
        };
        // M6.505: a stop while the disc was read back left it written (and closed), only not verified.
        let verifying = matches!(self.disc.progress(), Some((BurnStep::Verifying, _, _)));
        self.disc.job = None;
        match result {
            Ok(()) if self.disc.copy < self.disc.copies => {
                self.disc.copy += 1;
                self.disc.prompt = Some(WriterPrompt::NextCopy);
                self.status = tl!("Disc written. Insert the next blank disc.").to_owned();
            }
            Ok(()) => {
                let copies = self.disc.copies;
                self.end_writing(match (copies > 1, self.disc.verify) {
                    (true, true) => tlf!("{copies} discs written and verified", copies = copies),
                    (true, false) => tlf!("{copies} discs written", copies = copies),
                    (false, true) => tl!("Disc written and verified").to_owned(),
                    (false, false) => tl!("Disc written").to_owned(),
                });
            }
            Err(BurnError::Cancelled) if verifying => self.end_writing(
                tl!("Verify stopped: the disc is written and closed, but not verified").to_owned(),
            ),
            Err(BurnError::Cancelled) => self.end_writing(tl!("Writing stopped").to_owned()),
            Err(error) => {
                let text = burn_error_text(&error);
                self.status = text.clone();
                self.disc.prompt = Some(WriterPrompt::Failed(text));
            }
        }
        self.refresh_media();
    }

    /// Writes the Source's disc image to the disc as it is (M6.504): nothing is built, and the image stays when the
    /// writing ends. When it cannot be written (none named, not found, not an image, or a dual-layer DVD image
    /// without its layer break), says why in the writer's dialog and writes nothing.
    pub(crate) fn start_writing_image(&mut self) {
        if self.disc.recorder.is_none() {
            self.status = tl!("No recorder").to_owned();
            return;
        }
        // The disc may have been changed since it was last looked at.
        self.refresh_media();
        let media = match &self.disc.media {
            Some(Ok(Some(media))) => Some(media.clone()),
            _ => None,
        };
        match check_source_image(&self.disc.source_image, media.as_ref()) {
            Ok(image) => {
                self.disc.image = Some(image);
                self.disc.keep_image = true;
                self.disc.copy = 1;
                self.start_writing();
            }
            Err(problem) => {
                let text = source_image_text(problem);
                self.status = text.clone();
                self.disc.prompt = Some(WriterPrompt::Refused(text));
            }
        }
    }

    /// Ends the writing session: removes the image it built (not the Source's own image) and says `status`.
    pub(crate) fn end_writing(&mut self, status: String) {
        if let Some(image) = self.disc.image.take()
            && !self.disc.keep_image
        {
            let _ = std::fs::remove_file(&image);
            let _ = std::fs::remove_dir_all(image.with_extension("files"));
        }
        self.disc.keep_image = false;
        self.disc.building = false;
        self.disc.prompt = None;
        self.disc.job = None;
        self.disc.copy = 1;
        self.status = status;
    }

    /// The question between copies or after a failure.
    pub(crate) fn show_writer_prompt(&mut self, ctx: &egui::Context) {
        let Some(prompt) = self.disc.prompt.clone() else {
            return;
        };
        let mut choice = None;
        let _ = egui::Modal::new(egui::Id::unique("disc_writer_prompt")).show(ctx, |ui| {
            ui.set_width(440.0);
            match &prompt {
                WriterPrompt::NextCopy => {
                    ui.strong(tl!("Next Disc"));
                    ui.add_space(6.0);
                    ui.label(tlf!(
                        "Insert a blank disc for copy {copy} of {copies}.",
                        copy = self.disc.copy,
                        copies = self.disc.copies
                    ));
                }
                WriterPrompt::Failed(text) | WriterPrompt::Refused(text) => {
                    ui.strong(tl!("The disc could not be written"));
                    ui.add_space(6.0);
                    ui.label(text);
                }
            }
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Nothing was written for a refused image: only OK (the session ends as Cancel ends it).
                if let WriterPrompt::Refused(_) = prompt {
                    if ui.add(crate::theme::default_button(tl!("OK"))).clicked() {
                        choice = Some(false);
                    }
                    return;
                }
                if ui.add(crate::theme::dialog_button(tl!("Cancel"))).clicked() {
                    choice = Some(false);
                }
                let go = match prompt {
                    WriterPrompt::NextCopy => tl!("Write"),
                    WriterPrompt::Failed(_) | WriterPrompt::Refused(_) => tl!("Try Again"),
                };
                if ui.add(crate::theme::default_button(go)).clicked() {
                    choice = Some(true);
                }
            });
        });
        match choice {
            Some(true) => {
                self.disc.prompt = None;
                self.refresh_media();
                self.start_writing();
            }
            Some(false) => self.end_writing(match prompt {
                // The reason stays in the status bar.
                WriterPrompt::Refused(text) => text,
                _ => tl!("Writing stopped").to_owned(),
            }),
            None => {}
        }
    }

    /// The writing's line for the Build panel and the status bar: what it does and how far.
    pub(crate) fn writing_step(&self) -> Option<(String, f32, u64)> {
        let (step, fraction, seconds) = self.disc.progress()?;
        let what = match step {
            BurnStep::Erasing => tl!("Erasing the disc").to_owned(),
            BurnStep::Writing => tlf!(
                "Writing the disc (copy {copy} of {copies})",
                copy = self.disc.copy,
                copies = self.disc.copies
            ),
            BurnStep::Closing => tl!("Closing the disc").to_owned(),
            BurnStep::Verifying => tl!("Verifying the disc").to_owned(),
        };
        Some((what, fraction, seconds))
    }

    /// Chooses the Source's disc image (Encore's 使用するソース ▸ 参照…).
    pub(crate) fn pick_source_image(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(tl!("Disc image"), &["iso", "img"])
            .pick_file()
        {
            self.disc.source_image = path.to_string_lossy().into_owned();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SourceImageProblem::{Missing, NoLayerBreak, NotAnImage, NotChosen};
    use super::{DVD_LAYER_SECTORS, check_source_image, source_image_problem};
    use crate::app::DiscCraftApp;
    use crate::i18n::tl;
    use std::error::Error;
    use std::path::PathBuf;

    /// A folder of its own for one test.
    fn scratch(name: &str) -> Result<PathBuf, Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!(
            "disccraft-disc-writer-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// M6.504: the Source's image is whole sectors; a DVD image past one layer needs its layer break for a
    /// dual-layer disc (else the drive puts the layer change anywhere), and is too large for any other.
    #[test]
    fn a_source_image_is_checked_before_it_is_written() {
        let layer = DVD_LAYER_SECTORS * 2048;
        assert_eq!(source_image_problem(None, None, false), Some(Missing));
        assert_eq!(source_image_problem(Some(0), None, false), Some(NotAnImage));
        assert_eq!(
            source_image_problem(Some(4097), None, false),
            Some(NotAnImage)
        );
        assert_eq!(source_image_problem(Some(layer), None, true), None);
        assert_eq!(
            source_image_problem(Some(layer + 2048), None, true),
            Some(NoLayerBreak)
        );
        assert_eq!(
            source_image_problem(Some(layer + 2048), Some(2_038_416), true),
            None
        );
        // Not a dual-layer disc: the writing says it is too small.
        assert_eq!(source_image_problem(Some(layer + 2048), None, false), None);
    }

    /// M6.504: the Source's box may hold a path in quotes (as Windows copies it); a folder or a missing file is not
    /// an image.
    #[test]
    fn the_source_image_is_found_from_its_box() -> Result<(), Box<dyn Error>> {
        let dir = scratch("source")?;
        let image = dir.join("disc.iso");
        std::fs::write(&image, vec![0u8; 4096])?;
        assert_eq!(check_source_image("  ", None), Err(NotChosen));
        assert_eq!(
            check_source_image(&format!("\"{}\"", image.display()), None),
            Ok(image.clone())
        );
        assert_eq!(
            check_source_image(&dir.join("none.iso").to_string_lossy(), None),
            Err(Missing)
        );
        assert_eq!(
            check_source_image(&dir.to_string_lossy(), None),
            Err(Missing)
        );
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    /// M6.504: the end of a writing removes the image the session built, never the Source's own image.
    #[test]
    fn the_source_image_stays_when_the_writing_ends() -> Result<(), Box<dyn Error>> {
        let dir = scratch("keep")?;
        let kept = dir.join("kept.iso");
        let built = dir.join("built.iso");
        std::fs::write(&kept, vec![0u8; 2048])?;
        std::fs::write(&built, vec![0u8; 2048])?;
        let mut app = DiscCraftApp::new(&egui::Context::default());
        app.disc.image = Some(kept.clone());
        app.disc.keep_image = true;
        app.end_writing("done".to_owned());
        assert!(kept.exists());
        assert!(!app.disc.keep_image);
        app.disc.image = Some(built.clone());
        app.end_writing("done".to_owned());
        assert!(!built.exists());
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    /// M6.504: with the Source "Disc image", Build writes that image and does not build the project (no recorder:
    /// nothing starts).
    #[test]
    fn build_with_a_source_image_does_not_build_the_project() {
        let mut app = DiscCraftApp::new(&egui::Context::default());
        app.disc.chosen = true;
        app.disc.from_image = true;
        app.disc.recorder = None;
        app.request_build();
        assert_eq!(app.status, tl!("No recorder"));
        assert!(!app.build_job.is_running());
        assert!(!app.build_prompt);
    }
}
