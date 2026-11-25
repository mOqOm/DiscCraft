//! The Interpret Footage dialog (Encore: Interpret Footage… of an asset): whether a video asset's pictures have
//! the pixel aspect ratio the file says, or one chosen here, and whether a picture's alpha channel is ignored.
//!
//! The dialog is Encore's: "Use the asset's pixel aspect ratio: <the file's>" / "Specify pixel aspect ratio:
//! [list]" / "Ignore alpha channel" (for pictures: video assets carry none) / OK and Cancel. A picture the size of a
//! DVD frame has SD pixels of its own (Encore: "SD NTSC (0.9091)").

use disccraft_project::{AssetId, AssetKind};
use serde_json::{Value, json};

use crate::app::DiscCraftApp;
use crate::i18n::tl;

/// The pixel aspect ratios offered, as Encore names them: width over height of a pixel.
const CHOICES: [(&str, (u32, u32)); 8] = [
    ("Square Pixels (1.0)", (1, 1)),
    ("D1/DV NTSC (0.9091)", (10, 11)),
    ("D1/DV NTSC Widescreen 16:9 (1.2121)", (40, 33)),
    ("D1/DV PAL (1.0940)", (128, 117)),
    ("D1/DV PAL Widescreen 16:9 (1.4587)", (512, 351)),
    ("Anamorphic 2:1 (2.0)", (2, 1)),
    ("HD Anamorphic 1080 (1.333)", (4, 3)),
    ("DVCPRO HD (1.5)", (3, 2)),
];

/// The dialog's state while it is open.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InterpretDraft {
    /// The asset being interpreted.
    asset: AssetId,
    /// The file's own pixel aspect ratio; `None` when the file cannot be opened as video.
    file: Option<(u32, u32)>,
    /// "Specify pixel aspect ratio" is chosen.
    specify: bool,
    /// The ratio specified.
    chosen: (u32, u32),
    /// The asset is a picture: its alpha channel can be interpreted too.
    picture: bool,
    /// "Ignore alpha channel" is ticked (a picture's pixels are all opaque).
    ignore_alpha: bool,
}

/// The offered ratio a ratio is (within a thousandth), if any.
fn choice_of(aspect: (u32, u32)) -> Option<usize> {
    let value = f64::from(aspect.0) / f64::from(aspect.1.max(1));
    CHOICES.iter().position(|(_, (width, height))| {
        (f64::from(*width) / f64::from(*height) - value).abs() < 0.001 * value.max(1.0)
    })
}

/// A ratio in words: the offered one it is, else its numbers ("8:9 (0.8889)").
pub(crate) fn aspect_text(aspect: (u32, u32)) -> String {
    match choice_of(aspect).and_then(|index| CHOICES.get(index)) {
        Some((name, _)) => crate::i18n::t(name).to_string(),
        None => format!(
            "{}:{} ({:.4})",
            aspect.0,
            aspect.1,
            f64::from(aspect.0) / f64::from(aspect.1.max(1))
        ),
    }
}

/// `[width, height]` of a command's result.
fn pair(value: Option<&Value>) -> Option<(u32, u32)> {
    let part = |index: usize| {
        value?
            .get(index)?
            .as_u64()
            .and_then(|part| u32::try_from(part).ok())
    };
    part(0).zip(part(1))
}

impl DiscCraftApp {
    /// Whether `asset` can be interpreted: a video asset (its pixel aspect ratio) or a picture (its alpha channel).
    pub(crate) fn can_interpret_footage(&self, asset: AssetId) -> bool {
        self.session
            .project
            .assets
            .get(&asset)
            .is_some_and(|asset| matches!(asset.kind, AssetKind::Video | AssetKind::Image))
    }

    /// Opens the dialog for `asset`.
    pub(crate) fn open_interpret_footage(&mut self, asset: AssetId) {
        let Some(found) = self.run_command("asset.pixelAspect", json!({ "asset": asset.0 })) else {
            return;
        };
        let file = pair(found.get("file"));
        let chosen = pair(found.get("chosen"));
        let picture = self
            .session
            .project
            .assets
            .get(&asset)
            .is_some_and(|asset| asset.kind == AssetKind::Image);
        self.interpret_footage_dialog = Some(InterpretDraft {
            asset,
            file,
            specify: chosen.is_some(),
            // Encore's list starts at Square Pixels when nothing is chosen.
            chosen: chosen.unwrap_or((1, 1)),
            picture,
            ignore_alpha: found
                .get("ignoreAlpha")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }

    /// Draws the dialog while it is open.
    pub(crate) fn interpret_footage_window(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.interpret_footage_dialog.clone() else {
            return;
        };
        let mut done = None;
        let response =
            egui::Modal::new(egui::Id::unique("interpret_footage_dialog")).show(ctx, |ui| {
                ui.set_width(420.0);
                ui.heading(tl!("Interpret Footage"));
                ui.add_space(4.0);
                ui.label(tl!("Pixel Aspect Ratio"));
                // Encore names a DVD-sized picture's own shape "SD NTSC (0.9091)" (a 720 x 480 PNG; the PAL one by
                // the same pattern).
                let own = match draft.file {
                    Some((8, 9) | (9, 10)) if draft.picture => tl!("SD NTSC (0.9091)").to_string(),
                    Some((16, 15)) if draft.picture => tl!("SD PAL (1.0940)").to_string(),
                    Some(aspect) => aspect_text(aspect),
                    None => tl!("Unknown").to_string(),
                };
                ui.scope(|ui| {
                    ui.radio_value(
                        &mut draft.specify,
                        false,
                        format!("{} {own}", tl!("Use the asset's pixel aspect ratio:")),
                    );
                    ui.horizontal(|ui| {
                        ui.radio_value(
                            &mut draft.specify,
                            true,
                            tl!("Specify pixel aspect ratio:"),
                        );
                        egui::ComboBox::from_id_salt("interpret_footage_choice")
                            .width(230.0)
                            .selected_text(aspect_text(draft.chosen))
                            .show_ui(ui, |ui| {
                                for (name, aspect) in CHOICES {
                                    if ui
                                        .selectable_label(
                                            choice_of(draft.chosen) == choice_of(aspect),
                                            crate::i18n::t(name),
                                        )
                                        .clicked()
                                    {
                                        draft.chosen = aspect;
                                        draft.specify = true;
                                    }
                                }
                            });
                    });
                });
                // A video asset carries no alpha channel.
                let _ = ui.add_enabled(
                    draft.picture,
                    egui::Checkbox::new(&mut draft.ignore_alpha, tl!("Ignore alpha channel")),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.add(crate::theme::default_button(tl!("OK"))).clicked() {
                        done = Some(true);
                    }
                    if ui.add(crate::theme::dialog_button(tl!("Cancel"))).clicked() {
                        done = Some(false);
                    }
                });
            });
        if response.should_close() {
            done = Some(false);
        }
        match done {
            Some(true) if draft.picture => {
                self.interpret_footage_dialog = None;
                let aspect = draft
                    .specify
                    .then_some(json!([draft.chosen.0, draft.chosen.1]));
                let shaped = self.run_command(
                    "asset.setPixelAspect",
                    json!({ "asset": draft.asset.0, "aspect": aspect }),
                );
                let opaque = self.run_command(
                    "asset.setIgnoreAlpha",
                    json!({ "asset": draft.asset.0, "ignore": draft.ignore_alpha }),
                );
                if shaped.is_some() || opaque.is_some() {
                    // The picture is drawn otherwise now, wherever it is shown.
                    let _ = self.thumbnails.remove(&draft.asset.0);
                    self.menu_thumbnails.clear();
                    self.preview_slides.clear();
                    self.monitor_frame = None;
                }
            }
            Some(true) => {
                self.interpret_footage_dialog = None;
                let aspect = draft
                    .specify
                    .then_some(json!([draft.chosen.0, draft.chosen.1]));
                if self
                    .run_command(
                        "asset.setPixelAspect",
                        json!({ "asset": draft.asset.0, "aspect": aspect }),
                    )
                    .is_some()
                {
                    // The asset's pictures are another shape now, wherever they are shown.
                    let _ = self.thumbnails.remove(&draft.asset.0);
                    self.menu_thumbnails.clear();
                    self.monitor_frame = None;
                    self.transcode_dirty = true;
                }
            }
            Some(false) => self.interpret_footage_dialog = None,
            None => self.interpret_footage_dialog = Some(draft),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CHOICES, choice_of};

    #[test]
    fn a_ratio_is_named_when_it_is_one_of_the_offered() {
        for (index, (_, aspect)) in CHOICES.iter().enumerate() {
            assert_eq!(choice_of(*aspect), Some(index));
        }
        // The same ratio in other numbers.
        assert_eq!(choice_of((20, 22)), Some(1));
        assert_eq!(choice_of((4000, 3000)), Some(6));
        // A DVD's own 8:9 pixel is none of Encore's.
        assert_eq!(choice_of((8, 9)), None);
    }
}
