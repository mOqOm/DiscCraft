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
    DiscCraftApp, apply_theme, enable_media_cache, framework, install_crash_log, install_fonts,
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
        // The UI runtime the panels' docking and the menu bar are built on (M9.54), before the fonts, which it notes.
        framework::install(&cc.egui_ctx);
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
    fn logic(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        // The window's close goes through the UI runtime, which asks whether it may close (unsaved work, a disc being
        // written).
        framework::close_path(ctx, || self.app.close_decision());
    }

    fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) {
        framework::before_ui(ui.ctx(), frame);
        self.app.ui(ui);
        // M9.64: the file dialog asked for this frame shows now, over the window.
        self.app.show_dialogs(frame);
    }

    fn raw_input_hook(
        &mut self,
        ctx: &eframe::egui::Context,
        raw_input: &mut eframe::egui::RawInput,
    ) {
        self.app.native_menu_input(ctx);
        self.app.capture_input(raw_input);
        self.app.framework_input(ctx);
    }
}

/// M9.65: a ready update starts its helper once DiscCraft is going (craft-update; dropping its updater would too, this
/// makes it independent of when eframe drops the app).
impl Drop for AppWrapper {
    fn drop(&mut self) {
        self.app.exiting();
    }
}

fn main() -> ExitCode {
    // M9.65: on Windows the update helper is a copy of this program started with `--finish-update`: it waits for
    // DiscCraft to exit, installs the update and starts the new DiscCraft.
    #[cfg(windows)]
    {
        let arguments: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
        if let Some(result) =
            craft_update::self_update::finish(&disccraft_ui_egui::UPDATE_APP, &arguments)
        {
            return if result.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            };
        }
    }
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
