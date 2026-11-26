#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use disccraft_ui_egui::{
    DiscCraftApp, apply_theme, enable_media_cache, install_crash_log, install_fonts,
};
use std::path::PathBuf;
use std::process::ExitCode;

/// M6.500: mimalloc as the allocator. A transcode asks for pictures of megabytes many times a second; the system's
/// allocator gave each fresh pages (half a million page faults a second), on which several transcodes side by side
/// waited for one another. mimalloc reuses its own pages: five clips at once went 1.8 times as fast.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

struct AppWrapper {
    app: DiscCraftApp,
}

impl AppWrapper {
    fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        install_fonts(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx);

        enable_media_cache();
        let mut app = DiscCraftApp::new(&cc.egui_ctx);
        // On macOS the menus go to the system's menu bar (nothing happens elsewhere).
        app.install_native_menu(&cc.egui_ctx);

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

    fn raw_input_hook(
        &mut self,
        _ctx: &eframe::egui::Context,
        raw_input: &mut eframe::egui::RawInput,
    ) {
        self.app.native_menu_input(raw_input);
    }
}

fn main() -> ExitCode {
    // A crash leaves the session log behind (Help ▸ Reveal Log Files… shows the folder).
    install_crash_log();
    let path = std::env::args().nth(1).map(PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("DiscCraft")
            // Like Encore, start maximized; the panels take their share of the screen.
            .with_inner_size([1280.0, 800.0])
            .with_maximized(true)
            // Files dragged in from Explorer are imported into the project (as in Encore).
            .with_drag_and_drop(true)
            // The application icon (assets/app-icon) on the window and the taskbar, and the ID the Linux desktop
            // file names (Wayland app ID, X11 window class).
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!(
                    "../../../assets/app-icon/hicolor/256x256/apps/io.github.mOqOm.DiscCraft.png"
                ))
                .unwrap_or_default(),
            )
            .with_app_id("io.github.mOqOm.DiscCraft"),
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
            let _ = writeln!(std::io::stderr(), "DiscCraft could not start: {error}");
            ExitCode::FAILURE
        }
    }
}
