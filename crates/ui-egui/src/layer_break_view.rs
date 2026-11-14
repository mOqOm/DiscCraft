//! The layer break of a dual-layer DVD in the Build tab (Encore's ブレークポイント): automatic, or manual at a frame
//! of a timeline (typed as a timecode, a frame number or milliseconds), and where the last built image breaks, read
//! back from the image, with whether it is where it was set.

use crate::app::DiscCraftApp;
use crate::i18n::{tl, tlf};
use disccraft_build::{LayerBreak, OutputKind, frame_90k, millis};
use disccraft_project::{Capacity, DiscKind, DiscSettings, Tick, TimelineId, VideoStandard};
use egui::{Color32, Ui};
use serde_json::json;

/// Colour of a break that is where it was set.
const MATCH: Color32 = Color32::from_rgb(0x3c, 0xb3, 0x71);

/// Ticks of one frame at the disc's rate.
fn frame_ticks(standard: VideoStandard) -> i64 {
    match standard {
        VideoStandard::Ntsc => 8_475_667_200,
        VideoStandard::Pal => 10_160_640_000,
    }
}

/// The frame nearest to `time` (the one the build takes).
pub(crate) fn frame_of(time: Tick, standard: VideoStandard) -> u64 {
    let per_frame = frame_ticks(standard);
    u64::try_from(time.0.saturating_add(per_frame / 2) / per_frame).unwrap_or(0)
}

/// A typed break time: a timecode (`00;00;01;15`), a frame number (`45f`) or milliseconds (`1501.5ms`). `None` for
/// anything else or a negative time.
pub(crate) fn parse_break_time(text: &str, settings: &DiscSettings) -> Option<Tick> {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    if let Some(ms) = lower.strip_suffix("ms") {
        let ms: f64 = ms.trim().parse().ok()?;
        if !ms.is_finite() || ms < 0.0 {
            return None;
        }
        return Some(Tick(crate::timeline_view::seconds_to_ticks(ms / 1000.0)));
    }
    if let Some(frames) = lower.strip_suffix('f') {
        let frames: i64 = frames.trim().parse().ok()?;
        return (frames >= 0)
            .then(|| frames.checked_mul(frame_ticks(settings.standard)))
            .flatten()
            .map(Tick);
    }
    disccraft_project::parse_time(text, settings).ok()
}

/// `frame 45 · 1501.50 ms` for a time.
fn frame_text(time: Tick, standard: VideoStandard) -> String {
    let frame = frame_of(time, standard);
    tlf!(
        "frame {frame} · {ms} ms",
        frame = frame,
        ms = format!("{:.2}", millis(frame.saturating_mul(frame_90k(standard))))
    )
}

impl DiscCraftApp {
    /// The Layer Break group of a DVD project: usable for a dual-layer disc (8.5 GB).
    pub(crate) fn layer_break_section(&mut self, ui: &mut Ui) {
        crate::theme::group_box(ui, "build_layer_break", tl!("Layer Break"), |ui| {
            let settings = &self.session.project.settings;
            let dual = settings.kind == DiscKind::Dvd && settings.capacity == Capacity::Dvd9;
            if !dual {
                ui.label(tl!("Used for a dual-layer disc (8.5 GB)."));
            }
            ui.add_enabled_ui(dual, |ui| self.layer_break_controls(ui));
            self.layer_break_result(ui);
        });
    }

    fn layer_break_controls(&mut self, ui: &mut Ui) {
        let current = self.session.project.settings.layer_break;
        ui.horizontal(|ui| {
            if ui.radio(current.is_none(), tl!("Automatic")).clicked() && current.is_some() {
                let _ = self.run_command("disc.setLayerBreak", json!({ "timeline": null }));
            }
            if ui.radio(current.is_some(), tl!("Manual")).clicked() && current.is_none() {
                // The timeline in the viewer (or the first) at its playhead.
                let timeline = self
                    .viewer_timeline()
                    .or_else(|| self.session.project.timelines.keys().next().copied());
                if let Some(timeline) = timeline {
                    self.set_layer_break_at_playhead(ui.ctx(), timeline);
                }
            }
        });
        let Some(point) = current else {
            ui.weak(tl!(
                "The build breaks at the first cell boundary past the middle of the image."
            ));
            return;
        };
        let settings = self.session.project.settings.clone();
        let mut chosen = None;
        let mut typed = None;
        let mut playhead = false;
        egui::Grid::new("build_layer_break_grid")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label(tl!("Timeline:"));
                let name = self
                    .session
                    .project
                    .timeline(point.timeline)
                    .map_or_else(|| tl!("(missing)").to_string(), |t| t.name.clone());
                egui::ComboBox::from_id_salt("build_layer_break_timeline")
                    .selected_text(name)
                    .show_ui(ui, |ui| {
                        for (id, timeline) in &self.session.project.timelines {
                            if ui
                                .selectable_label(*id == point.timeline, &timeline.name)
                                .clicked()
                            {
                                chosen = Some(*id);
                            }
                        }
                    });
                ui.end_row();

                ui.label(tl!("Time:"));
                ui.horizontal(|ui| {
                    // The field is as wide as a timecode, so the frame and milliseconds fit beside it.
                    ui.scope(|ui| {
                        ui.set_max_width(140.0);
                        let shown = disccraft_project::format_time(point.time, &settings);
                        let (response, committed) = crate::properties::committed_text(
                            ui,
                            "build_layer_break_time",
                            &shown,
                            false,
                        );
                        response.on_hover_text(tl!(
                            "A timecode, a frame number (45f) or milliseconds (1501.5ms)"
                        ));
                        typed = committed;
                    });
                    ui.label(frame_text(point.time, settings.standard));
                    playhead = ui
                        .button(tl!("Playhead"))
                        .on_hover_text(tl!("Set the break at the timeline's playhead"))
                        .clicked();
                });
                ui.end_row();
            });
        ui.weak(tl!(
            "The build starts a cell (not a chapter) at this frame. Layer 0 must be at least as large as layer 1."
        ));
        if let Some(timeline) = chosen {
            let _ = self.run_command(
                "disc.setLayerBreak",
                json!({ "timeline": timeline.0, "time": point.time.0 }),
            );
        }
        if let Some(text) = typed {
            match parse_break_time(&text, &settings) {
                Some(time) => {
                    let _ = self.run_command(
                        "disc.setLayerBreak",
                        json!({ "timeline": point.timeline.0, "time": time.0 }),
                    );
                }
                None => {
                    self.status =
                        tl!("Enter a timecode, a frame number (45f) or milliseconds (1501.5ms)")
                            .to_string();
                }
            }
        }
        if playhead {
            self.set_layer_break_at_playhead(ui.ctx(), point.timeline);
        }
    }

    fn set_layer_break_at_playhead(&mut self, ctx: &egui::Context, timeline: TimelineId) {
        let seconds = crate::timeline_view::playhead_seconds(ctx, timeline);
        let time = crate::timeline_view::seconds_to_ticks(seconds).max(0);
        let _ = self.run_command(
            "disc.setLayerBreak",
            json!({ "timeline": timeline.0, "time": time }),
        );
    }

    /// Where the last built image breaks, read back from it: the sectors of the layers, the title, cell and frame,
    /// and for a manual break whether it is at the frame set.
    fn layer_break_result(&self, ui: &mut Ui) {
        let Some(report) = &self.last_build else {
            return;
        };
        let Some(found) = &report.layer_break else {
            if self.session.project.settings.layer_break.is_some()
                && self.last_output_kind == Some(OutputKind::Folder)
            {
                ui.weak(tl!(
                    "A folder has no layer break: build an image to check it."
                ));
            }
            return;
        };
        ui.separator();
        for line in result_lines(found, self.session.project.settings.standard) {
            match line {
                ResultLine::Plain(text) => {
                    ui.label(text);
                }
                ResultLine::Good(text) => {
                    ui.colored_label(MATCH, text);
                }
                ResultLine::Bad(text) => {
                    ui.colored_label(ui.visuals().error_fg_color, text);
                }
            }
        }
    }
}

/// A line of the layer break result.
#[derive(Debug, PartialEq)]
pub(crate) enum ResultLine {
    Plain(String),
    Good(String),
    Bad(String),
}

/// The lines that describe a built layer break.
pub(crate) fn result_lines(found: &LayerBreak, standard: VideoStandard) -> Vec<ResultLine> {
    let mut lines = vec![ResultLine::Plain(tlf!(
        "Layer break at sector {sector} ({mode}): layer 0 {layer0} / layer 1 {layer1} sectors",
        sector = found.sector,
        mode = if found.manual {
            tl!("manual")
        } else {
            tl!("automatic")
        },
        layer0 = found.sector,
        layer1 = found.layer1_sectors()
    ))];
    let frame_of_90k =
        |time: u64| time.saturating_add(frame_90k(standard) / 2) / frame_90k(standard);
    match &found.place {
        Some(place) => lines.push(ResultLine::Plain(tlf!(
            "Title {title}, cell {cell}: frame {frame} ({ms} ms)",
            title = place.title,
            cell = place.cell,
            frame = frame_of_90k(place.time_90k),
            ms = format!("{:.2}", millis(place.time_90k))
        ))),
        None => lines.push(ResultLine::Bad(
            tl!("The break is not at the start of a cell.").to_string(),
        )),
    }
    match (found.as_requested(), found.requested_90k) {
        (Some(true), _) => lines.push(ResultLine::Good(tl!("As set.").to_string())),
        (Some(false), Some(requested)) => lines.push(ResultLine::Bad(tlf!(
            "Not where it was set (frame {frame}).",
            frame = frame_of_90k(requested)
        ))),
        _ => {}
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{ResultLine, frame_of, parse_break_time, result_lines};
    use disccraft_build::{LayerBreak, LayerBreakPlace};
    use disccraft_project::{DiscSettings, Tick, VideoStandard};

    const FRAME: i64 = 8_475_667_200;

    /// A break time is typed as a timecode, frames or milliseconds; the build takes the nearest frame.
    #[test]
    fn break_times_parse_in_three_forms() {
        let settings = DiscSettings::default();
        assert_eq!(
            parse_break_time("00;00;01;15", &settings),
            Some(Tick(45 * FRAME))
        );
        assert_eq!(parse_break_time(" 45f ", &settings), Some(Tick(45 * FRAME)));
        let ms = parse_break_time("1501.5ms", &settings).map(|t| frame_of(t, VideoStandard::Ntsc));
        assert_eq!(ms, Some(45));
        for bad in ["", "-3f", "-1ms", "abc", "xf", "NaNms"] {
            assert_eq!(parse_break_time(bad, &settings), None, "{bad}");
        }
        let pal = DiscSettings {
            standard: VideoStandard::Pal,
            ..DiscSettings::default()
        };
        assert_eq!(
            parse_break_time("25f", &pal),
            Some(Tick(25 * 10_160_640_000))
        );
    }

    /// The result says where the image breaks and whether that is where it was set.
    #[test]
    fn the_result_says_whether_the_break_is_where_set() {
        let mut found = LayerBreak {
            sector: 600,
            total_sectors: 1000,
            manual: true,
            place: Some(LayerBreakPlace {
                timeline: 5,
                title: 1,
                vts: 1,
                cell: 3,
                time_90k: 45 * 3003,
            }),
            requested_90k: Some(45 * 3003),
        };
        let lines = result_lines(&found, VideoStandard::Ntsc);
        assert_eq!(lines.len(), 3);
        assert!(matches!(lines.last(), Some(ResultLine::Good(_))));
        found.requested_90k = Some(46 * 3003);
        let lines = result_lines(&found, VideoStandard::Ntsc);
        assert!(matches!(lines.last(), Some(ResultLine::Bad(text)) if text.contains("46")));
        found.manual = false;
        found.requested_90k = None;
        assert_eq!(result_lines(&found, VideoStandard::Ntsc).len(), 2);
    }
}
