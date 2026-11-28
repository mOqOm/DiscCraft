//! ビルドパネル（Encore と同じ形。docs/ui-parity.md §3.4）。

use crate::app::DiscCraftApp;
use crate::build_panel::{format_bytes, problem_label, usage_fraction};
use crate::i18n::{tl, tlf};

use disccraft_build::OutputKind;
use disccraft_project::{Capacity, DiscKind};
use egui::{self, Ui};

/// Plain disc kind label used in interpolated strings.
fn kind_label(kind: DiscKind) -> &'static str {
    match kind {
        DiscKind::Dvd => "DVD",
        DiscKind::BluRay => "Blu-ray",
    }
}

/// Output combo label, e.g. "DVD Folder", "Blu-ray Image".
pub(crate) fn output_label(kind: DiscKind, output: OutputKind) -> String {
    match output {
        OutputKind::Folder => tlf!("{kind} Folder", kind = kind_label(kind)),
        OutputKind::Iso => tlf!("{kind} Image", kind = kind_label(kind)),
        // Encore's DVD output list says "DDP Image" (a DVD's only).
        OutputKind::Ddp => tl!("DDP Image").to_owned(),
    }
}

/// The Output list's disc, e.g. "Blu-ray Disc" (M9.35, Encore's first output).
pub(crate) fn disc_label(kind: DiscKind) -> String {
    tlf!("{kind} Disc", kind = kind_label(kind))
}

/// Region checkbox label.
pub(crate) fn region_label(region: u8) -> &'static str {
    match region {
        1 => tl!("1 - United States, Canada"),
        2 => tl!("2 - Japan, Europe, South Africa, Middle East"),
        3 => tl!("3 - Southeast Asia, Korea, Hong Kong"),
        4 => tl!("4 - Australia, Mexico, Central and South America"),
        5 => tl!("5 - Russia, South Asia, Africa, North Korea"),
        6 => tl!("6 - China"),
        7 => tl!("7 - Reserved (undefined)"),
        8 => tl!("8 - Special venues such as aircraft"),
        _ => "",
    }
}

/// Whether `region`'s box shows checked for the disc's `regions` (empty: all regions). All regions shows every region
/// checked but 7, greyed (Encore, M6.463: 7 is reserved and left out, as in the custom list's start).
pub(crate) fn shown_checked(regions: &[u8], region: u8) -> bool {
    if regions.is_empty() {
        region != 7
    } else {
        regions.contains(&region)
    }
}

/// The width of a build group's label column: its labels end there, right-aligned (Encore, M6.470).
const LABEL_COLUMN: f32 = 130.0;
/// The Region Code group's (Encore's 有効 : ends nearer the edge).
const REGION_LABEL_COLUMN: f32 = 95.0;
/// The disc usage bar (Encore: 24 high with round ends in a dark edge, the used part shaded from (77, 121, 151) down
/// to (107, 158, 202)), its empty part, and the swatches under it (the used part's and the DVD-ROM content's).
const USAGE_HEIGHT: f32 = 24.0;
const USAGE_EDGE: crate::theme::Shade =
    crate::theme::Shade::new(egui::Color32::from_gray(6), egui::Color32::from_gray(140));
const USAGE_EMPTY: crate::theme::Shade =
    crate::theme::Shade::new(egui::Color32::from_gray(40), egui::Color32::from_gray(210));
const USAGE_TOP: egui::Color32 = egui::Color32::from_rgb(77, 121, 151);
const USAGE_BOTTOM: egui::Color32 = egui::Color32::from_rgb(107, 158, 202);
const USED_COLOR: egui::Color32 = egui::Color32::from_rgb(103, 161, 198);
const ROM_COLOR: egui::Color32 = egui::Color32::from_rgb(69, 234, 142);
/// M6.501: what multiplexing adds (orange: told apart from the blue and the green whatever the colour vision).
const MUX_COLOR: egui::Color32 = egui::Color32::from_rgb(230, 159, 0);

/// A label right-aligned in a column `width` wide (a build group's labels line up on the right, as in Encore).
fn group_label(ui: &mut Ui, text: &str, width: f32) {
    // The cell takes the whole width (a grid sizes its column by what its cells take) and the text sits at its end.
    let color = ui.visuals().text_color();
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::TextStyle::Body.resolve(ui.style()),
        color,
    );
    let size = egui::vec2(width.max(galley.size().x), ui.spacing().interact_size.y);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let at = egui::pos2(
        rect.max.x - galley.size().x,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(at, galley, color);
}

/// A 12 by 12 swatch of `color` in a black edge (Encore's, before each part of the usage sentence).
fn swatch(ui: &mut Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);
    ui.painter().rect_filled(rect.shrink(1.0), 0.0, color);
}

/// Encore's disc usage bar across the group: a round-ended bar, its used part (`fraction` of it) shaded blue, the
/// DVD-ROM content's share (`rom_fraction`) green at the end of the used part, and (M6.501) what multiplexing adds
/// (`mux_fraction`) orange before it.
fn draw_usage_bar(ui: &mut Ui, fraction: f32, (mux_fraction, rom_fraction): (f32, f32)) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), USAGE_HEIGHT),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    painter.rect_filled(rect, USAGE_HEIGHT / 2.0, USAGE_EDGE.get());
    let inner = rect.shrink(1.0);
    let radius = inner.height() / 2.0;
    let used_x = inner.min.x + inner.width() * fraction.clamp(0.0, 1.0);
    let rom_x = (used_x - inner.width() * rom_fraction.clamp(0.0, 1.0)).max(inner.min.x);
    let mux_x = (rom_x - inner.width() * mux_fraction.clamp(0.0, 1.0)).max(inner.min.x);
    // Row by row, each as wide as the round ends let it be.
    let rows = inner.height().max(0.0) as usize;
    for row in 0..rows {
        let y = inner.min.y + row as f32 + 0.5;
        let dy = (y - inner.center().y).abs();
        let chord = (radius * radius - dy * dy).max(0.0).sqrt();
        let left = inner.min.x + radius - chord;
        let right = inner.max.x - radius + chord;
        if right <= left {
            continue;
        }
        let shade = row as f32 / rows.max(1) as f32;
        let used = lerp_color(USAGE_TOP, USAGE_BOTTOM, shade);
        for (from, to, color) in [
            (left, right, USAGE_EMPTY.get()),
            (left, used_x.min(right), used),
            (mux_x.max(left), rom_x.min(right), MUX_COLOR),
            (rom_x.max(left), used_x.min(right), ROM_COLOR),
        ] {
            if to > from {
                painter.hline(from..=to, y, egui::Stroke::new(1.0, color));
            }
        }
    }
}

/// `a` to `b` by `t` (0 to 1).
fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let mix = |x: u8, y: u8| {
        let value = f32::from(x) + (f32::from(y) - f32::from(x)) * t.clamp(0.0, 1.0);
        value.round().clamp(0.0, 255.0) as u8
    };
    egui::Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// The region list after toggling `region` on or off.
pub(crate) fn toggle_region(regions: &[u8], region: u8, on: bool) -> Vec<u8> {
    let mut result = regions.to_vec();

    if on {
        if !result.contains(&region) {
            result.push(region);
        }
    } else {
        result.retain(|&current| current != region);
    }

    result.sort_unstable();
    result
}

/// The capacities offered for a disc kind.
pub(crate) fn capacity_choices(kind: DiscKind) -> &'static [Capacity] {
    Capacity::choices(kind)
}

/// The capacities offered for a disc kind with Custom last (Encore's Size list): the custom one is `current` when
/// it is custom, else as large as `current` (within what the kind holds), to be changed in the box beside.
pub(crate) fn capacity_list(kind: DiscKind, current: Capacity) -> Vec<Capacity> {
    let mut list = capacity_choices(kind).to_vec();
    list.push(match current {
        Capacity::Custom { .. } => current,
        other => {
            let range = Capacity::custom_range(kind);
            Capacity::Custom {
                bytes: other.bytes().clamp(*range.start(), *range.end()),
            }
        }
    });
    list
}

/// A custom capacity's size in gigabytes (10⁹ bytes, as the sizes in the list), edited in place: the new size in
/// bytes when it was changed (on release, not while dragged).
pub(crate) fn custom_capacity_box(
    ui: &mut Ui,
    salt: &str,
    kind: DiscKind,
    bytes: u64,
) -> Option<u64> {
    let id = ui.make_persistent_id(salt);
    let range = Capacity::custom_range(kind);
    // In GiB, as every size is shown (M6.494).
    const GIB: f64 = 1_073_741_824.0;
    let gigabytes = |bytes: u64| bytes as f64 / GIB;
    let mut size = ui
        .data(|data| data.get_temp::<f64>(id))
        .unwrap_or_else(|| gigabytes(bytes));
    let response = ui
        .add(
            egui::DragValue::new(&mut size)
                .speed(0.01)
                .range(gigabytes(*range.start())..=gigabytes(*range.end()))
                .fixed_decimals(2)
                .suffix(" GiB"),
        )
        .on_hover_text(tl!(
            "The disc's size, in gibibytes (1 GiB = 1,073,741,824 bytes)"
        ));
    if response.dragged() {
        ui.data_mut(|data| data.insert_temp(id, size));
        return None;
    }
    ui.data_mut(|data| data.remove::<f64>(id));
    let wanted = ((size * GIB).round() as u64).clamp(*range.start(), *range.end());
    ((response.changed() || response.drag_stopped()) && wanted != bytes).then_some(wanted)
}

/// Text for a capacity choice.
fn capacity_text(capacity: &Capacity) -> String {
    match capacity {
        Capacity::Dvd5 => tl!("4.7 GB (DVD5)").to_string(),
        Capacity::Dvd9 => tl!("8.5 GB (DVD9)").to_string(),
        Capacity::Bd25 => tl!("25 GB (BD25)").to_string(),
        Capacity::Bd50 => tl!("50 GB (BD50)").to_string(),
        Capacity::Bd100 => tl!("100 GB (BDXL, 3 layers)").to_string(),
        Capacity::Bd128 => tl!("128 GB (BDXL, 4 layers)").to_string(),
        Capacity::Custom { bytes } => tlf!("Custom ({size})", size = format_bytes(*bytes)),
    }
}

/// Description for the selected output kind.
fn output_description(output: OutputKind) -> &'static str {
    match output {
        OutputKind::Folder => tl!("Creates a disc folder (VIDEO_TS or BDMV) on the hard drive."),
        OutputKind::Iso => tl!("Creates an ISO disc image on the hard drive."),
        OutputKind::Ddp => tl!("Creates a DDP image on the hard drive for replication."),
    }
}

/// The Build and Check Project buttons at the right (Encore's are wide).
const BUTTON_SIZE: egui::Vec2 = egui::Vec2::new(120.0, 0.0);

impl DiscCraftApp {
    /// The Build tab.
    pub(crate) fn build_view(&mut self, ui: &mut Ui) {
        // A DDP image is a DVD's only (as in Encore): a Blu-ray project builds a folder instead.
        if self.session.project.settings.kind == DiscKind::BluRay
            && self.build_output == OutputKind::Ddp
        {
            self.build_output = OutputKind::Folder;
        }
        self.build_top_row(ui);
        self.build_description_row(ui);
        // M6.505: what a running build or writing does is in the Build Progress dialog over the window.

        ui.strong(tlf!(
            "{kind} Settings",
            kind = kind_label(self.session.project.settings.kind)
        ));

        self.source_section(ui);
        self.destination_section(ui);
        self.disc_info_section(ui);

        if self.session.project.settings.kind == DiscKind::Dvd {
            self.layer_break_section(ui);
            self.region_section(ui);
        } else {
            // M6.543: a Blu-ray's parental level, asked of the player's age setting.
            crate::theme::group_box(ui, "build_bd_parental", tl!("Parental Control"), |ui| {
                self.parental_level_row(ui);
            });
        }

        self.project_check_section(ui);
        self.result_section(ui);
    }

    /// Draws the format and output row with the Build button at the right (Encore's layout). In a narrow panel the
    /// Build button gets a row of its own above them, so it is never cut off.
    fn build_top_row(&mut self, ui: &mut Ui) {
        // M9.37: the format and output lists with their labels take about 400 points, the Build button 120 more.
        let narrow = ui.available_width() < 540.0;
        if narrow {
            self.build_button(ui);
        }
        ui.horizontal(|ui| {
            ui.label(tl!("Format:"));
            self.format_combo(ui);
            ui.add_space(12.0);
            ui.label(tl!("Output:"));
            self.output_combo(ui);

            if !narrow {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.build_button(ui);
                });
            }
        });
    }

    /// The Build button: usable whenever no build runs (an empty Location builds next to the project).
    fn build_button(&mut self, ui: &mut Ui) {
        // A disc needs a recorder (Encore's Build is unavailable with "Not found").
        let ready = !self.disc.chosen || self.disc.recorder.is_some();
        if ui
            .add_enabled(
                !self.build_job.is_running() && !self.disc.is_busy() && ready,
                egui::Button::new(tl!("Build")).min_size(BUTTON_SIZE),
            )
            .clicked()
        {
            self.request_build();
        }
    }

    /// Draws the output description with the Check Project button at the right (Encore's layout).
    fn build_description_row(&mut self, ui: &mut Ui) {
        let description = if self.disc.chosen {
            tlf!(
                "Creates a disc to play in a {kind} player, a computer or a game console.",
                kind = kind_label(self.session.project.settings.kind)
            )
        } else {
            output_description(self.build_output).to_owned()
        };
        // M9.37: when the description and the button do not fit side by side, the description goes on its own
        // (wrapped) lines and the button under it, instead of under the button.
        let text_width = ui
            .painter()
            .layout_no_wrap(
                description.clone(),
                egui::TextStyle::Body.resolve(ui.style()),
                egui::Color32::WHITE,
            )
            .size()
            .x;
        let fits = text_width + BUTTON_SIZE.x + 24.0 <= ui.available_width();
        let mut check = false;
        if fits {
            ui.horizontal(|ui| {
                ui.label(description);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    check = ui
                        .add(egui::Button::new(tl!("Check Project")).min_size(BUTTON_SIZE))
                        .clicked();
                });
            });
        } else {
            ui.add(egui::Label::new(description).wrap());
            // A row of its own (a right-to-left layout straight in the panel would take all its height).
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    check = ui
                        .add(egui::Button::new(tl!("Check Project")).min_size(BUTTON_SIZE))
                        .clicked();
                });
            });
        }
        if check {
            self.check_window.open = true;
            self.check_project();
        }
    }

    /// Draws the disc format combo.
    fn format_combo(&mut self, ui: &mut Ui) {
        let current = self.session.project.settings.kind;
        let mut selected = None;

        egui::ComboBox::from_id_salt("build_format")
            .selected_text(kind_label(current))
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(current == DiscKind::Dvd, "DVD")
                    .clicked()
                {
                    selected = Some(DiscKind::Dvd);
                }

                if ui
                    .selectable_label(current == DiscKind::BluRay, "Blu-ray")
                    .clicked()
                {
                    selected = Some(DiscKind::BluRay);
                }
            });

        if let Some(kind) = selected {
            self.set_disc_kind(kind);
        }
    }

    /// Draws the output kind combo.
    fn output_combo(&mut self, ui: &mut Ui) {
        let kind = self.session.project.settings.kind;
        let disc = self.disc.chosen;
        let current = self.build_output;
        let mut selected = None;
        let mut disc_chosen = false;

        egui::ComboBox::from_id_salt("build_output")
            .selected_text(if disc {
                disc_label(kind)
            } else {
                output_label(kind, current)
            })
            .show_ui(ui, |ui| {
                // Encore's list starts with the disc (M9.35).
                if ui.selectable_label(disc, disc_label(kind)).clicked() {
                    disc_chosen = true;
                }
                let current = if disc { None } else { Some(current) };
                if ui
                    .selectable_label(
                        current == Some(OutputKind::Folder),
                        output_label(kind, OutputKind::Folder),
                    )
                    .clicked()
                {
                    selected = Some(OutputKind::Folder);
                }

                if ui
                    .selectable_label(
                        current == Some(OutputKind::Iso),
                        output_label(kind, OutputKind::Iso),
                    )
                    .clicked()
                {
                    selected = Some(OutputKind::Iso);
                }

                if kind == DiscKind::Dvd
                    && ui
                        .selectable_label(
                            current == Some(OutputKind::Ddp),
                            output_label(kind, OutputKind::Ddp),
                        )
                        .clicked()
                {
                    selected = Some(OutputKind::Ddp);
                }
            });

        if disc_chosen && !self.disc.is_busy() {
            self.disc.chosen = true;
            self.build_output = OutputKind::Iso;
        } else if let Some(output) = selected.filter(|_| !self.disc.is_busy()) {
            self.disc.chosen = false;
            self.build_output = output;
        }
    }

    /// Draws the source section (Encore: 使用するソース のコンボ). With the disc output the list also has Disc image
    /// (M6.504): an image made before, written to the disc as it is, named in the box under the list. Otherwise the
    /// source is the current project, and the box and Browse are unavailable.
    fn source_section(&mut self, ui: &mut Ui) {
        let disc = self.disc.chosen;
        let from_image = disc && self.disc.from_image;
        let idle = !self.disc.is_busy() && !self.build_job.is_running();
        let mut chosen = None;
        let mut browse = false;
        crate::theme::group_box(ui, "build_source", tl!("Source"), |ui| {
            egui::Grid::new("build_source_grid")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    group_label(ui, tl!("Source:"), LABEL_COLUMN);
                    ui.add_enabled_ui(idle, |ui| {
                        egui::ComboBox::from_id_salt("build_source")
                            .selected_text(if from_image {
                                tl!("Disc image")
                            } else {
                                tl!("Current project")
                            })
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(!from_image, tl!("Current project"))
                                    .clicked()
                                {
                                    chosen = Some(false);
                                }
                                if disc
                                    && ui.selectable_label(from_image, tl!("Disc image")).clicked()
                                {
                                    chosen = Some(true);
                                }
                            });
                    });
                    ui.end_row();

                    group_label(ui, "", LABEL_COLUMN);
                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 80.0).max(120.0);
                        if from_image {
                            ui.add_enabled(
                                idle,
                                egui::TextEdit::singleline(&mut self.disc.source_image)
                                    .desired_width(width)
                                    .hint_text(tl!("The disc image (.iso) to write")),
                            );
                            browse = ui
                                .add_enabled(idle, egui::Button::new(tl!("Browse…")))
                                .clicked();
                        } else {
                            let mut folder = String::new();
                            let _ = ui.add_enabled(
                                false,
                                egui::TextEdit::singleline(&mut folder).desired_width(width),
                            );
                            let _ = ui.add_enabled(false, egui::Button::new(tl!("Browse…")));
                        }
                    });
                });
        });
        if let Some(image) = chosen {
            self.disc.from_image = image;
        }
        if browse {
            self.pick_source_image();
        }
    }

    /// Draws the destination section.
    fn destination_section(&mut self, ui: &mut Ui) {
        if self.disc.chosen {
            self.disc_destination_section(ui);
            return;
        }
        crate::theme::group_box(ui, "build_destination", tl!("Destination"), |ui| {
            egui::Grid::new("build_destination_grid")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    group_label(ui, tl!("Location:"), LABEL_COLUMN);

                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 80.0).max(120.0);
                        // Empty: the build goes next to the project; the box shows where.
                        let hint = self.default_build_path().to_string_lossy().into_owned();
                        ui.add(
                            egui::TextEdit::singleline(&mut self.build_path)
                                .desired_width(width)
                                .hint_text(hint),
                        );

                        if ui.button(tl!("Browse…")).clicked() {
                            self.pick_build_path();
                        }
                    });
                });
        });
    }

    /// Draws the disc information section (Encore: the usage bar, the usage sentence under it, 名前, サイズ).
    fn disc_info_section(&mut self, ui: &mut Ui) {
        crate::theme::group_box(ui, "build_disc_info", tl!("Disc Info"), |ui| {
            self.usage_bar(ui);

            let name = self.session.project.settings.name.clone();
            let mut changed_name = None;
            egui::Grid::new("build_disc_info_grid")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    group_label(ui, tl!("Name:"), LABEL_COLUMN);
                    ui.horizontal(|ui| {
                        ui.set_max_width(240.0);
                        changed_name =
                            crate::properties::committed_text(ui, "build_disc_name", &name, false)
                                .1;
                    });
                    ui.end_row();

                    group_label(ui, tl!("Size:"), LABEL_COLUMN);
                    ui.horizontal(|ui| {
                        self.capacity_combo(ui);

                        if ui.button(tl!("Refresh Usage")).clicked() {
                            self.update_usage();
                        }
                    });
                    ui.end_row();

                    // Encore: DVD-ROM コンテンツ / BD-ROM コンテンツ [参照…]. A folder of files for a computer,
                    // put in the disc's root beside the video.
                    let dvd = self.session.project.settings.kind == DiscKind::Dvd;
                    let label = if dvd {
                        tl!("DVD-ROM content:")
                    } else {
                        tl!("BD-ROM content:")
                    };
                    group_label(ui, label, LABEL_COLUMN);
                    ui.horizontal(|ui| {
                        let folder = self.session.project.settings.rom_content.clone();
                        let width = (ui.available_width() - 150.0).max(120.0);
                        let mut shown = folder.clone().unwrap_or_default();
                        ui.add_enabled(
                            false,
                            egui::TextEdit::singleline(&mut shown).desired_width(width),
                        );
                        if ui.button(tl!("Browse…")).clicked() {
                            self.pick_rom_content();
                        }
                        if ui
                            .add_enabled(folder.is_some(), egui::Button::new(tl!("Clear")))
                            .clicked()
                        {
                            self.clear_rom_content();
                        }
                    });
                    ui.end_row();
                });
            if let Some(name) = changed_name {
                let _ = self.run_command("disc.setName", serde_json::json!({ "name": name }));
            }
        });
    }

    /// Draws the usage progress bar and, like Encore, the sentence under it with a swatch of the bar's colour.
    fn usage_bar(&self, ui: &mut Ui) {
        if let Some(usage) = &self.usage {
            let fraction = usage_fraction(usage);
            let used = format_bytes(usage.estimated_bytes);
            let free = format_bytes(usage.capacity_bytes.saturating_sub(usage.estimated_bytes));

            let share = |bytes: u64| {
                if usage.capacity_bytes > 0 {
                    (bytes as f64 / usage.capacity_bytes as f64).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                }
            };
            draw_usage_bar(
                ui,
                fraction,
                (share(usage.mux_bytes), share(usage.rom_bytes)),
            );
            // Encore: one line under the bar, each part after a swatch of its colour in the bar.
            let rom = format_bytes(usage.rom_bytes);
            // M6.498: what the used part is made of (the streams and what multiplexing adds).
            let made_of = tlf!(
                "Video and sound {streams}, multiplexing {mux}, ROM content {rom}",
                streams = format_bytes(
                    usage
                        .estimated_bytes
                        .saturating_sub(usage.mux_bytes)
                        .saturating_sub(usage.rom_bytes)
                ),
                mux = format_bytes(usage.mux_bytes),
                rom = format_bytes(usage.rom_bytes)
            );
            ui.horizontal(|ui| {
                swatch(ui, USED_COLOR);
                ui.label(tlf!("{used} used. {free} free.", used = used, free = free))
                    .on_hover_text(made_of);
                // M6.501: what multiplexing adds, as a part of its own (it took room the bar showed free before).
                if usage.mux_bytes > 0 {
                    swatch(ui, MUX_COLOR);
                    ui.label(tlf!(
                        "{size} used for multiplexing.",
                        size = format_bytes(usage.mux_bytes)
                    ))
                    .on_hover_text(tl!(
                        "What the disc adds to the video and sound: the headers of its packs and a navigation pack every half second"
                    ));
                }
                swatch(ui, ROM_COLOR);
                ui.label(if self.session.project.settings.kind == DiscKind::Dvd {
                    tlf!("{size} used for DVD-ROM content.", size = rom)
                } else {
                    tlf!("{size} used for BD-ROM content.", size = rom)
                });
            });
            // M6.498: a DVD9 with a manual layer break: each layer, and where the free space can go (layer 1 holds no
            // more than layer 0).
            if let Some((layer0, most, layer1)) = usage.layers {
                let status = if layer0 > most || layer1 > layer0 {
                    crate::theme::Status::Warning
                } else {
                    crate::theme::Status::Good
                };
                crate::theme::status_label(
                    ui,
                    status,
                    tlf!(
                        "Layer 0 (up to the layer break): about {layer0} of {most}. Layer 1: about {layer1}, at most as much as layer 0.",
                        layer0 = format_bytes(layer0),
                        most = format_bytes(most),
                        layer1 = format_bytes(layer1)
                    ),
                );
                ui.label(tlf!(
                    "Room left: {room0} on layer 0, {room1} on layer 1.",
                    room0 = format_bytes(most.saturating_sub(layer0)),
                    room1 = format_bytes(layer0.min(most).saturating_sub(layer1))
                ));
            }
        } else {
            ui.label(tl!("No usage information"));
        }
    }

    /// Draws the capacity combo (Encore: the sizes, then Custom with the size beside it).
    fn capacity_combo(&mut self, ui: &mut Ui) {
        let kind = self.session.project.settings.kind;
        let current = self.session.project.settings.capacity;
        let mut selected = None;

        egui::ComboBox::from_id_salt("build_capacity")
            .selected_text(capacity_text(&current))
            .show_ui(ui, |ui| {
                for choice in capacity_list(kind, current) {
                    let text = match choice {
                        Capacity::Custom { .. } => tl!("Custom").to_string(),
                        other => capacity_text(&other),
                    };
                    if ui.selectable_label(current == choice, text).clicked() {
                        selected = Some(choice);
                    }
                }
            });
        if let Capacity::Custom { bytes } = current
            && let Some(bytes) = custom_capacity_box(ui, "build_custom_capacity", kind, bytes)
        {
            selected = Some(Capacity::Custom { bytes });
        }

        if let Some(capacity) = selected.filter(|capacity| *capacity != current) {
            self.set_capacity(capacity);
        }
    }

    /// Draws the DVD region section.
    fn region_section(&mut self, ui: &mut Ui) {
        crate::theme::group_box(ui, "build_region", tl!("Region Code"), |ui| {
            egui::Grid::new("build_region_grid")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    group_label(ui, tl!("Valid:"), REGION_LABEL_COLUMN);

                    ui.horizontal(|ui| {
                        self.region_mode_controls(ui);
                    });
                });

            self.region_checkbox_grid(ui);
            self.parental_level_row(ui);
        });
    }

    /// Draws the disc's parental level: none, or 1 to 8.
    fn parental_level_row(&mut self, ui: &mut Ui) {
        let current = self.session.project.settings.parental_level;
        let text = |level: u8| match level {
            0 => tl!("None").to_string(),
            level => level.to_string(),
        };
        let mut selected = None;
        ui.horizontal(|ui| {
            ui.label(tl!("Parental level:"));
            egui::ComboBox::from_id_salt("build_parental_level")
                .selected_text(text(current))
                .show_ui(ui, |ui| {
                    for level in 0..=8 {
                        if ui.selectable_label(current == level, text(level)).clicked() {
                            selected = Some(level);
                        }
                    }
                });
        })
        .response
        .on_hover_text(if self.session.project.settings.kind == DiscKind::BluRay {
            tl!(
                "A Blu-ray player whose parental age is below the level's does not play the disc (levels 1 to 8: all ages, 6, 10, 13, 15, 17, 18 and 21)"
            )
        } else {
            tl!("A player set to a lower level asks for its password before the disc plays")
        });
        if let Some(level) = selected {
            let _ = self.run_command(
                "disc.setParentalLevel",
                serde_json::json!({ "level": level }),
            );
        }
    }

    /// Draws the all/custom region mode controls.
    fn region_mode_controls(&mut self, ui: &mut Ui) {
        let is_all = self.session.project.settings.regions.is_empty();

        if ui.radio(is_all, tl!("All regions")).clicked() && !is_all {
            self.set_regions(Vec::new());
        }

        if ui.radio(!is_all, tl!("Custom")).clicked() && is_all {
            self.set_regions(vec![1, 2, 3, 4, 5, 6, 8]);
        }
    }

    /// Draws the region checkboxes.
    fn region_checkbox_grid(&mut self, ui: &mut Ui) {
        let is_custom = !self.session.project.settings.regions.is_empty();

        egui::Grid::new("build_region_checkboxes")
            .num_columns(1)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                for region in 1..=8 {
                    let mut checked = shown_checked(&self.session.project.settings.regions, region);

                    let response = ui.add_enabled(
                        is_custom,
                        egui::Checkbox::new(&mut checked, region_label(region)),
                    );

                    if response.changed() {
                        let regions =
                            toggle_region(&self.session.project.settings.regions, region, checked);
                        self.set_regions(regions);
                    }
                    ui.end_row();
                }
            });
    }

    /// Sends a region list to the command layer.
    fn set_regions(&mut self, regions: Vec<u8>) {
        let params = serde_json::json!({ "regions": regions });
        let _ = self.run_command("disc.setRegions", params);
    }

    /// Draws the project check section.
    fn project_check_section(&self, ui: &mut Ui) {
        crate::theme::group_box(ui, "build_project_check", tl!("Project Check"), |ui| {
            if self.problems.is_empty() {
                ui.label(tl!("No problems"));
            } else {
                egui::ScrollArea::vertical()
                    .max_height(140.0)
                    .show(ui, |ui| {
                        // The same lines as the Check Project window: the item's name and what is wrong.
                        let all: Vec<&crate::build_panel::ProblemView> =
                            self.problems.iter().collect();
                        for row in crate::check_window::check_rows(&self.session.project, &all) {
                            for (severity, text) in &row.problems {
                                problem_label(ui, severity, format!("{}: {text}", row.name));
                            }
                        }
                    });
            }
        });
    }

    /// Draws the build result section.
    fn result_section(&mut self, ui: &mut Ui) {
        crate::theme::group_box(ui, "build_result", tl!("Result"), |ui| {
            let error = self
                .build_job
                .result
                .as_ref()
                .and_then(|result| result.as_ref().err())
                .cloned();

            if let Some(error) = error {
                crate::theme::status_label(ui, crate::theme::Status::Error, error);
            }

            let summary = self.last_build.as_ref().map(|report| {
                tlf!(
                    "Titles {titles} / chapters {chapters} / {size}",
                    titles = report.titles,
                    chapters = report.chapters,
                    size = format_bytes(report.total_bytes)
                )
            });

            if let Some(summary) = summary {
                ui.label(summary);
            }

            let can_simulate =
                self.last_build.is_some() && self.last_output_kind == Some(OutputKind::Folder);

            if ui
                .add_enabled(can_simulate, egui::Button::new(tl!("Simulate")))
                .clicked()
            {
                self.simulate_last_build();
            }

            if let Some(summary) = self.simulate.clone() {
                self.simulate_summary(ui, &summary);
            }
        });
    }
}

/// Unit tests for pure build view helpers.
#[cfg(test)]
mod tests {
    use super::{capacity_choices, capacity_list, output_label, shown_checked, toggle_region};
    use crate::i18n::{Language, set_current};
    use disccraft_build::OutputKind;
    use disccraft_project::{Capacity, DiscKind};

    #[test]
    /// All regions shows 1–6 and 8 checked, 7 not (Encore); a custom list shows its own.
    fn all_regions_show_checked_but_the_reserved_one() {
        let all: Vec<u8> = (1..=8)
            .filter(|region| shown_checked(&[], *region))
            .collect();
        assert_eq!(all, vec![1, 2, 3, 4, 5, 6, 8]);
        let custom: Vec<u8> = (1..=8)
            .filter(|region| shown_checked(&[2, 7], *region))
            .collect();
        assert_eq!(custom, vec![2, 7]);
    }

    #[test]
    /// Adding a region keeps the list sorted.
    fn toggle_region_adds_sorted() {
        let result = toggle_region(&[3, 1], 2, true);
        assert_eq!(result, vec![1, 2, 3]);
    }

    #[test]
    /// Removing a region keeps the remaining order sorted.
    fn toggle_region_removes() {
        let result = toggle_region(&[1, 2, 3], 2, false);
        assert_eq!(result, vec![1, 3]);
    }

    #[test]
    /// Adding an existing region is a no-op.
    fn toggle_region_duplicate_add_is_noop() {
        let result = toggle_region(&[1, 2], 2, true);
        assert_eq!(result, vec![1, 2]);
    }

    #[test]
    /// Capacity choices match the disc kind.
    fn capacity_choices_match_kind() {
        assert_eq!(
            capacity_choices(DiscKind::Dvd),
            [Capacity::Dvd5, Capacity::Dvd9]
        );
        assert_eq!(
            capacity_choices(DiscKind::BluRay),
            [
                Capacity::Bd25,
                Capacity::Bd50,
                Capacity::Bd100,
                Capacity::Bd128
            ]
        );
    }

    /// Encore's Size list ends with Custom: as large as the size chosen, or the custom size itself; never more
    /// than a single-layer DVD on a DVD.
    #[test]
    fn the_capacity_list_ends_with_custom() {
        assert_eq!(
            capacity_list(DiscKind::Dvd, Capacity::Dvd5),
            [
                Capacity::Dvd5,
                Capacity::Dvd9,
                Capacity::Custom {
                    bytes: 4_700_000_000
                }
            ]
        );
        assert_eq!(
            capacity_list(DiscKind::Dvd, Capacity::Dvd9).last(),
            Some(&Capacity::Custom {
                bytes: 4_700_000_000
            })
        );
        let mini = Capacity::Custom {
            bytes: 1_400_000_000,
        };
        assert_eq!(capacity_list(DiscKind::Dvd, mini).last(), Some(&mini));
        assert_eq!(
            capacity_list(DiscKind::BluRay, Capacity::Bd50).last(),
            Some(&Capacity::Custom {
                bytes: Capacity::Bd50.bytes()
            })
        );
    }

    #[test]
    /// Output labels are English when the current language is English.
    fn output_label_uses_english_kind() {
        set_current(Language::En);

        assert_eq!(
            output_label(DiscKind::Dvd, OutputKind::Folder),
            "DVD Folder"
        );
        assert_eq!(output_label(DiscKind::Dvd, OutputKind::Iso), "DVD Image");
        assert_eq!(output_label(DiscKind::Dvd, OutputKind::Ddp), "DDP Image");
        assert_eq!(
            output_label(DiscKind::BluRay, OutputKind::Folder),
            "Blu-ray Folder"
        );
        assert_eq!(
            output_label(DiscKind::BluRay, OutputKind::Iso),
            "Blu-ray Image"
        );

        set_current(Language::Ja);
    }
}
