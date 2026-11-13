//! ビルドパネル（Encore と同じ形。docs/ui-parity.md §3.4）。

use crate::app::DiscCraftApp;
use crate::build_panel::{format_bytes, problem_color, usage_fraction};
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
    }
}

/// Region checkbox label.
pub(crate) fn region_label(region: u8) -> &'static str {
    match region {
        1 => tl!("1 - United States, Canada"),
        2 => tl!("2 - Japan, Europe, South Africa, Middle East"),
        3 => tl!("3 - Southeast Asia, Korea, Hong Kong"),
        4 => tl!("4 - Australia, Central and South America"),
        5 => tl!("5 - Russia, South Asia, Africa"),
        6 => tl!("6 - China"),
        7 => tl!("7 - Reserved"),
        8 => tl!("8 - International venues (aircraft, ships)"),
        _ => "",
    }
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
    }
}

/// The Build and Check Project buttons at the right (Encore's are wide).
const BUTTON_SIZE: egui::Vec2 = egui::Vec2::new(120.0, 0.0);

impl DiscCraftApp {
    /// The Build tab.
    pub(crate) fn build_view(&mut self, ui: &mut Ui) {
        self.build_top_row(ui);
        self.build_description_row(ui);

        ui.strong(tlf!(
            "{kind} Settings",
            kind = kind_label(self.session.project.settings.kind)
        ));

        self.source_section(ui);
        self.destination_section(ui);
        self.disc_info_section(ui);

        if self.session.project.settings.kind == DiscKind::Dvd {
            self.region_section(ui);
        }

        self.project_check_section(ui);
        self.result_section(ui);
    }

    /// Draws the format and output row with the Build button at the right (Encore's layout).
    fn build_top_row(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(tl!("Format:"));
            self.format_combo(ui);
            ui.add_space(12.0);
            ui.label(tl!("Output:"));
            self.output_combo(ui);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can_build = !self.build_job.is_running() && !self.build_path.trim().is_empty();
                if ui
                    .add_enabled(
                        can_build,
                        egui::Button::new(tl!("Build")).min_size(BUTTON_SIZE),
                    )
                    .clicked()
                {
                    self.start_build();
                }
            });
        });
    }

    /// Draws the output description with the Check Project button at the right (Encore's layout).
    fn build_description_row(&mut self, ui: &mut Ui) {
        let description = output_description(self.build_output);
        ui.horizontal(|ui| {
            ui.label(description);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(tl!("Check Project")).min_size(BUTTON_SIZE))
                    .clicked()
                {
                    self.check_window.open = true;
                    self.check_project();
                }
            });
        });
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
        let current = self.build_output;
        let mut selected = None;

        egui::ComboBox::from_id_salt("build_output")
            .selected_text(output_label(kind, current))
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(
                        current == OutputKind::Folder,
                        output_label(kind, OutputKind::Folder),
                    )
                    .clicked()
                {
                    selected = Some(OutputKind::Folder);
                }

                if ui
                    .selectable_label(
                        current == OutputKind::Iso,
                        output_label(kind, OutputKind::Iso),
                    )
                    .clicked()
                {
                    selected = Some(OutputKind::Iso);
                }
            });

        if let Some(output) = selected {
            self.build_output = output;
        }
    }

    /// Draws the source section (Encore: 使用するソース のコンボ。DiscCraft は今のプロジェクトだけなので、
    /// フォルダーの欄と 参照 は使えない).
    fn source_section(&self, ui: &mut Ui) {
        crate::theme::group_box(ui, "build_source", tl!("Source"), |ui| {
            egui::Grid::new("build_source_grid")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label(tl!("Source:"));
                    let _ = egui::ComboBox::from_id_salt("build_source")
                        .selected_text(tl!("Current project"))
                        .show_ui(ui, |ui| {
                            let _ = ui.selectable_label(true, tl!("Current project"));
                        });
                    ui.end_row();

                    ui.label("");
                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 80.0).max(120.0);
                        let mut folder = String::new();
                        let _ = ui.add_enabled(
                            false,
                            egui::TextEdit::singleline(&mut folder).desired_width(width),
                        );
                        let _ = ui.add_enabled(false, egui::Button::new(tl!("Browse…")));
                    });
                });
        });
    }

    /// Draws the destination section.
    fn destination_section(&mut self, ui: &mut Ui) {
        crate::theme::group_box(ui, "build_destination", tl!("Destination"), |ui| {
            egui::Grid::new("build_destination_grid")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label(tl!("Location:"));

                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 80.0).max(120.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.build_path).desired_width(width),
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
                    ui.label(tl!("Name:"));
                    ui.horizontal(|ui| {
                        ui.set_max_width(240.0);
                        changed_name =
                            crate::properties::committed_text(ui, "build_disc_name", &name, false)
                                .1;
                    });
                    ui.end_row();

                    ui.label(tl!("Size:"));
                    ui.horizontal(|ui| {
                        self.capacity_combo(ui);

                        if ui.button(tl!("Refresh Usage")).clicked() {
                            self.update_usage();
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

            ui.add(egui::ProgressBar::new(fraction));
            ui.horizontal(|ui| {
                let (swatch, _) =
                    ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(swatch, 1.0, ui.visuals().selection.bg_fill);
                ui.label(tlf!("{used} used. {free} free.", used = used, free = free));
            });
        } else {
            ui.label(tl!("No usage information"));
        }
    }

    /// Draws the capacity combo.
    fn capacity_combo(&mut self, ui: &mut Ui) {
        let kind = self.session.project.settings.kind;
        let current = self.session.project.settings.capacity;
        let mut selected = None;

        egui::ComboBox::from_id_salt("build_capacity")
            .selected_text(capacity_text(&current))
            .show_ui(ui, |ui| {
                for choice in capacity_choices(kind).iter().copied() {
                    if ui
                        .selectable_label(current == choice, capacity_text(&choice))
                        .clicked()
                    {
                        selected = Some(choice);
                    }
                }
            });

        if let Some(capacity) = selected {
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
                    ui.label(tl!("Valid:"));

                    ui.horizontal(|ui| {
                        self.region_mode_controls(ui);
                    });
                });

            self.region_checkbox_grid(ui);
        });
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
                    let mut checked = self.session.project.settings.regions.contains(&region);

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
                        for problem in &self.problems {
                            let color = problem_color(&problem.severity);
                            let text = format!("[{}] {}", problem.severity, problem.message);

                            ui.colored_label(color, text);
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
                ui.colored_label(egui::Color32::RED, error);
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
    use super::{capacity_choices, output_label, toggle_region};
    use crate::i18n::{Language, set_current};
    use disccraft_build::OutputKind;
    use disccraft_project::{Capacity, DiscKind};

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

    #[test]
    /// Output labels are English when the current language is English.
    fn output_label_uses_english_kind() {
        set_current(Language::En);

        assert_eq!(
            output_label(DiscKind::Dvd, OutputKind::Folder),
            "DVD Folder"
        );
        assert_eq!(output_label(DiscKind::Dvd, OutputKind::Iso), "DVD Image");
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
