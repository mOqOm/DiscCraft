//! Video output to another display (Encore: Preferences ▸ Audio/Video Output ▸ Video Devices, where a connected
//! monitor shows the preview full screen).
//!
//! What the Monitor shows of a title (the timeline's picture, or the title a preview plays) is also shown in a
//! window of its own: full screen on the display chosen in the preferences, or as an ordinary window to put
//! anywhere. Esc in that window turns the output off. Menus are not sent out, only titles' pictures.

use crate::app::DiscCraftApp;
use crate::i18n::tl;

use egui::{ViewportBuilder, ViewportId};

/// Where the video goes out, as kept in the settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum VideoOutput {
    /// Nowhere but DiscCraft's own panels.
    #[default]
    Off,
    /// A window of its own.
    Window,
    /// Full screen on display `n` (from 1, in the system's order).
    Display(u32),
}

impl VideoOutput {
    /// From the number kept in the settings: 0 a window, `n` display `n`; `None` off.
    pub(crate) fn from_setting(setting: Option<u32>) -> Self {
        match setting {
            None => Self::Off,
            Some(0) => Self::Window,
            Some(display) => Self::Display(display.min(64)),
        }
    }

    /// The number kept in the settings.
    pub(crate) fn setting(self) -> Option<u32> {
        match self {
            Self::Off => None,
            Self::Window => Some(0),
            Self::Display(display) => Some(display),
        }
    }
}

/// The picture that goes out: the texture, the screen's aspect ratio (width over height), and whether the
/// picture is fitted into that screen in its own shape (a slide) instead of filling it.
pub(crate) type VideoFrame = (egui::TextureHandle, f32, bool);

/// The largest rectangle of `aspect` inside `area`, centred.
fn fitted(area: egui::Rect, aspect: f32) -> egui::Rect {
    let aspect = if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        16.0 / 9.0
    };
    let (width, height) = (area.width(), area.height());
    let size = if width / height.max(1.0) > aspect {
        egui::vec2(height * aspect, height)
    } else {
        egui::vec2(width, width / aspect)
    };
    egui::Rect::from_center_size(area.center(), size)
}

impl DiscCraftApp {
    /// Draws the video output window while the output is on. Called once a frame, after the panels.
    pub(crate) fn video_output_window(&mut self, ctx: &egui::Context) {
        let output = self.video_output;
        if output == VideoOutput::Off {
            return;
        }
        // "Disable video output when in the background": no window while none of DiscCraft's is in front.
        let in_front = ctx.input(|input| {
            input
                .raw
                .viewports
                .values()
                .any(|viewport| viewport.focused == Some(true))
        });
        if !self.video_output_in_background && !in_front {
            return;
        }
        let mut builder = ViewportBuilder::default()
            .with_title(tl!("Video Output"))
            .with_inner_size([960.0, 540.0]);
        // A display was chosen and is there: a window without a frame that covers it exactly (in points). The
        // window system's own full screen is not used: asked for together with a position, it was not taken
        // (seen on Windows: the window stayed at its first size on the chosen display).
        let mut cover: Option<(egui::Pos2, egui::Vec2)> = None;
        if let VideoOutput::Display(number) = output {
            let displays = disccraft_platform::displays();
            let found = usize::try_from(number)
                .ok()
                .and_then(|number| number.checked_sub(1))
                .and_then(|index| displays.get(index).copied());
            if let Some(display) = found {
                let scale = ctx.pixels_per_point().max(0.1);
                let position = egui::pos2(display.x as f32 / scale, display.y as f32 / scale);
                let size = egui::vec2(display.width as f32 / scale, display.height as f32 / scale);
                builder = builder
                    .with_position(position)
                    .with_inner_size(size)
                    .with_decorations(false);
                cover = Some((position, size));
            }
        }
        let frame = self.video_out_frame.clone();
        let turn_off = ctx.show_viewport_immediate(
            ViewportId::from_hash_of("disccraft_video_output"),
            builder,
            |ui, _class| {
                let off = ui.ctx().input(|input| {
                    input.viewport().close_requested() || input.key_pressed(egui::Key::Escape)
                });
                // A display chosen while the window was up (or one whose size changed): moved and sized again.
                if let Some((position, size)) = cover {
                    let rect = ui.ctx().input(|input| input.viewport().inner_rect);
                    let placed = rect.is_some_and(|rect| {
                        (rect.min - position).length() < 2.0 && (rect.size() - size).length() < 2.0
                    });
                    if !placed {
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::OuterPosition(position));
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                    }
                }
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(egui::Color32::BLACK))
                    .show(ui, |ui| {
                        let Some((texture, aspect, own_shape)) = &frame else {
                            return;
                        };
                        let screen = fitted(ui.max_rect(), *aspect);
                        if *own_shape {
                            crate::project_panel::draw_fitted(ui.painter(), screen, texture);
                        } else {
                            ui.painter().image(
                                texture.id(),
                                screen,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                egui::Color32::WHITE,
                            );
                        }
                    });
                off
            },
        );
        if turn_off {
            self.set_video_output(VideoOutput::Off);
            self.status = tl!("Video output turned off").to_owned();
        }
    }

    /// Chooses where the video goes out, and keeps the choice.
    pub(crate) fn set_video_output(&mut self, output: VideoOutput) {
        self.video_output = output;
        if let Err(error) =
            crate::workspace::save_video_output(output.setting(), self.video_output_in_background)
        {
            self.status = error;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{VideoOutput, fitted};

    #[test]
    fn the_output_is_kept_as_a_number_and_the_picture_fits_the_screen() {
        for output in [
            VideoOutput::Off,
            VideoOutput::Window,
            VideoOutput::Display(1),
            VideoOutput::Display(3),
        ] {
            assert_eq!(VideoOutput::from_setting(output.setting()), output);
        }
        assert_eq!(
            VideoOutput::from_setting(Some(9999)),
            VideoOutput::Display(64)
        );
        // A 16:9 picture on a 4:3 screen is letterboxed; a 4:3 one on a 16:9 screen is pillarboxed.
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1600.0, 1200.0));
        let wide = fitted(screen, 16.0 / 9.0);
        assert!((wide.width() - 1600.0).abs() < 0.5 && (wide.height() - 900.0).abs() < 0.5);
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1920.0, 1080.0));
        let narrow = fitted(screen, 4.0 / 3.0);
        assert!((narrow.width() - 1440.0).abs() < 0.5 && (narrow.height() - 1080.0).abs() < 0.5);
        assert!(fitted(screen, f32::NAN).width() > 0.0);
    }
}
