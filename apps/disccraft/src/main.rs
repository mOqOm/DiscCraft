#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use disccraft_ui_egui::{DiscCraftApp, apply_theme, install_japanese_font};
use std::path::PathBuf;
use std::process::ExitCode;

struct AppWrapper {
    app: DiscCraftApp,
}

impl AppWrapper {
    fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        install_japanese_font(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx);

        let mut app = DiscCraftApp::new(&cc.egui_ctx);

        if let Some(path) = path {
            let _ = app.open_path(&path);
        }

        Self { app }
    }
}

impl eframe::App for AppWrapper {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
    }
}

fn main() -> ExitCode {
    let path = std::env::args().nth(1).map(PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("DiscCraft")
            // Like Encore, start maximized; the panels take their share of the screen.
            .with_inner_size([1280.0, 800.0])
            .with_maximized(true),
        ..Default::default()
    };

    match eframe::run_native(
        "DiscCraft",
        options,
        Box::new(move |cc| Ok(Box::new(AppWrapper::new(cc, path)))),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            use std::io::Write;
            // A closed error stream is not a reason to panic.
            let _ = writeln!(
                std::io::stderr(),
                "DiscCraft を起動できませんでした: {error}"
            );
            ExitCode::FAILURE
        }
    }
}
