//! Help ▸ Check for Updates… and the check at start (M9.65), on craft-libs' updater (craft-update) with the update
//! dialogs the other Craft apps show. Follows FilmCraft's `crates/ui-egui/src/updates_ui.rs` (NOTICE).
//!
//! DiscCraft asks GitHub only when the user checks, or at every start once Preferences ▸ General turns that on (off
//! until then). A newer release is offered in craft-update's prompt: a copy that can replace itself (Linux AppImage,
//! .deb, .rpm and Flatpak; a signed Windows copy) installs it and starts again once DiscCraft has closed; any other
//! copy gets the Download button, which opens the release on GitHub.

use crate::app::DiscCraftApp;
use craft_ui::dialog::{DialogHost, StackDialog};
use craft_update::stack_dialogs::{CheckChoice, UpdateCheckDialog};
use craft_update::updater::{Action, Status, Updater};
use egui::Ui;

/// DiscCraft as craft-update knows it: the repository its releases come from, the stem of their file names
/// (`disccraft-0.1.0-alpha.17-windows-x64.msi`), and the name, icon and package identity its dialogs and installs use.
pub const UPDATE_APP: craft_update::App = craft_update::App {
    icon_png: Some(include_bytes!(
        "../../../assets/app-icon/hicolor/128x128/apps/io.github.mOqOm.DiscCraft.png"
    )),
    developer: "mOqOm",
    bundle_id_override: Some("io.github.mOqOm.DiscCraft"),
    ..craft_update::App::new(
        "disccraft",
        "DiscCraft",
        "DVD and Blu-ray authoring",
        "mOqOm/DiscCraft",
    )
};

/// The version releases are compared with: the release the published executables were built for (`v0.1.0-alpha.17`
/// is 0.1.0-alpha.17), else the package version.
pub(crate) fn running_version() -> String {
    match crate::help::release_tag() {
        Some(tag) => tag.trim().trim_start_matches(['v', 'V']).to_owned(),
        None => env!("CARGO_PKG_VERSION").to_owned(),
    }
}

/// The updater, its dialogs, and what was asked of them.
pub(crate) struct Updates {
    updater: Updater,
    /// The update prompt and the check's result.
    dialogs: DialogHost,
    /// Preferences ▸ General ▸ Check for updates at start.
    startup_check: bool,
    /// Help ▸ Check for Updates… was chosen; the next pass starts the check.
    check_requested: bool,
    /// The check's result dialog answers Help ▸ Check for Updates… until it is closed.
    window_open: bool,
}

impl Updates {
    /// The updater; it asks GitHub at start when `startup_check`, else only when the user checks.
    pub(crate) fn new(startup_check: bool) -> Self {
        let mut updater = Updater::new(&UPDATE_APP, running_version()).with_translator(translate);
        if !startup_check {
            // What the last update came to (installed, or why not) shows at the first start after it even when this
            // start asks GitHub nothing.
            updater.read_last_update();
        }
        Self {
            updater,
            dialogs: DialogHost::new(),
            startup_check,
            check_requested: false,
            window_open: false,
        }
    }

    /// Turns the check at start on or off (Preferences ▸ General).
    pub(crate) fn set_startup_check(&mut self, on: bool) {
        self.startup_check = on;
    }
}

impl DiscCraftApp {
    /// Help ▸ Check for Updates…: asks GitHub for the newest release and shows what it found.
    pub(crate) fn check_for_updates(&mut self, ctx: &egui::Context) {
        self.updates.check_requested = true;
        self.updates.window_open = true;
        ctx.request_repaint();
    }

    /// Starts the checks that are due, takes their answers, and shows the update dialogs. Every pass.
    pub(crate) fn updates_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        // An update installs only over nothing unsaved and no disc being written: it closes DiscCraft.
        let clean = !self.session.dirty && !self.disc.is_writing();
        let updates = &mut self.updates;
        if std::mem::take(&mut updates.check_requested) {
            updates.updater.check_now();
        } else if updates.startup_check {
            updates.updater.check_at_start();
        }
        updates.updater.tick(&ctx);
        let action = updates.updater.run(&mut updates.dialogs, clean);
        updates.window_open = result_window_open(
            updates.window_open,
            updates.updater.status(),
            updates.check_requested,
        );
        sync_check_dialog(updates);
        updates.dialogs.run(ui);
        if let Some(result) = updates.dialogs.take::<UpdateCheckDialog>() {
            match result.ok() {
                Some(CheckChoice::CheckAgain | CheckChoice::CheckNow) => {
                    updates.check_requested = true;
                    ctx.request_repaint();
                }
                // The next pass's `run` asks the window to close once nothing is unsaved.
                Some(CheckChoice::RestartToUpdate) => {
                    updates.updater.restart_to_update();
                    ctx.request_repaint();
                }
                // OK, Esc and the close box.
                _ => {
                    updates.window_open = false;
                    updates.updater.dismiss_status();
                }
            }
        }
        match action {
            Action::OpenUrl(url) => self.open_link(&ctx, &url),
            // Through the window's own close, which asks about unsaved work and a disc being written.
            Action::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            _ => {}
        }
    }

    /// DiscCraft is exiting: a ready update starts its helper, which installs it once DiscCraft has gone.
    pub fn exiting(&mut self) {
        self.updates.updater.start_pending_update();
    }
}

/// The updater's own messages (under the prompt: why an update cannot start or failed) in the interface's language.
fn translate(text: &str, args: &[(&str, &str)]) -> String {
    crate::i18n::fmt(crate::i18n::t(text), args)
}

/// Whether the check's result dialog shows this pass. It answers Help ▸ Check for Updates… until the requested check
/// ends, and it shows whenever the status is not idle (at the first start after an update: `Updated` or
/// `InstallFailed`). A check that finds a release leaves the status idle and the update prompt takes over, so the
/// dialog closes then.
fn result_window_open(open: bool, status: &Status, check_requested: bool) -> bool {
    !matches!(status, Status::Idle) || (open && check_requested)
}

/// Opens the result dialog while it should show and gives it the updater's status, or closes it once it should not.
fn sync_check_dialog(updates: &mut Updates) {
    if !updates.window_open {
        if let Some(id) = updates
            .dialogs
            .get::<UpdateCheckDialog>()
            .map(StackDialog::id)
        {
            updates.dialogs.close_by_id(id);
        }
        return;
    }
    let status = updates.updater.status().clone();
    if let Some(dialog) = updates.dialogs.get_mut::<UpdateCheckDialog>() {
        dialog.set_status(status);
    } else {
        let mut dialog = UpdateCheckDialog::new(running_version());
        dialog.set_status(status);
        let _ = updates.dialogs.open(dialog);
    }
}

#[cfg(test)]
mod tests {
    use super::{UPDATE_APP, Updates, result_window_open, running_version};
    use craft_update::updater::Status;

    /// The releases' file names are found for every platform DiscCraft is published for, and nothing else.
    #[test]
    fn the_release_files_are_found_for_each_platform() {
        use craft_update::{Arch, Asset, Os, PackageKind, Platform, Release, pick_asset};
        let names = [
            "disccraft-0.1.0-alpha.17-linux-x86_64.AppImage",
            "disccraft-0.1.0-alpha.17-linux-x86_64.deb",
            "disccraft-0.1.0-alpha.17-linux-x86_64.flatpak",
            "disccraft-0.1.0-alpha.17-macos-universal.dmg",
            "disccraft-0.1.0-alpha.17-manual-en.pdf",
            "disccraft-0.1.0-alpha.17-windows-arm64.msi",
            "disccraft-0.1.0-alpha.17-windows-x64-portable.zip",
            "disccraft-0.1.0-alpha.17-windows-x64.msi",
            "disccraft-cli-0.1.0-alpha.17-macos-universal.zip",
        ];
        let release = Release {
            assets: names
                .iter()
                .map(|name| Asset {
                    name: (*name).to_owned(),
                    size: 1,
                    url: format!("https://example.invalid/{name}"),
                    sha256: None,
                })
                .collect(),
            ..Release::default()
        };
        let stems: Vec<&str> = UPDATE_APP.stems().collect();
        let picked = |os, arch, kind| {
            pick_asset(&release, &stems, Platform { os, arch }, &[kind])
                .map(|(asset, _)| asset.name.as_str())
        };
        assert_eq!(
            picked(Os::Windows, Arch::X64, PackageKind::Msi),
            Some("disccraft-0.1.0-alpha.17-windows-x64.msi")
        );
        assert_eq!(
            picked(Os::Windows, Arch::Arm64, PackageKind::Msi),
            Some("disccraft-0.1.0-alpha.17-windows-arm64.msi")
        );
        assert_eq!(
            picked(Os::Linux, Arch::X64, PackageKind::AppImage),
            Some("disccraft-0.1.0-alpha.17-linux-x86_64.AppImage")
        );
        assert_eq!(
            picked(Os::MacOs, Arch::Arm64, PackageKind::Dmg),
            Some("disccraft-0.1.0-alpha.17-macos-universal.dmg")
        );
        assert_eq!(UPDATE_APP.bundle_id(), "io.github.mOqOm.DiscCraft");
    }

    /// A release tag is the version releases are compared with; a newer alpha is newer, the same one is not.
    #[test]
    fn versions_compare_as_the_releases_are_numbered() {
        use craft_update::{is_newer, parse_version};
        let running = parse_version(&running_version());
        assert!(running.is_some());
        let alpha17 = parse_version("v0.1.0-alpha.17");
        let alpha18 = parse_version("v0.1.0-alpha.18");
        assert!(alpha17.is_some() && alpha18.is_some());
        if let (Some(alpha17), Some(alpha18)) = (alpha17, alpha18) {
            assert!(is_newer(&alpha18, &alpha17));
            assert!(!is_newer(&alpha17, &alpha17));
        }
    }

    /// Nothing is asked at start unless Preferences ▸ General says so, and nothing shows.
    #[test]
    fn a_new_updater_is_idle() {
        let updates = Updates::new(false);
        assert!(!updates.startup_check && !updates.check_requested && !updates.window_open);
        assert!(!updates.updater.prompt_open());
        assert_eq!(updates.updater.status(), &Status::Idle);
    }

    /// The texts the check's result dialog draws in `language`.
    fn check_dialog_texts(language: crate::i18n::Language) -> Vec<String> {
        fn texts(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| texts(shape, out)),
                egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        crate::framework::install_headless(&ctx);
        let mut updates = Updates::new(false);
        updates.window_open = true;
        let mut out = Vec::new();
        for _ in 0..3 {
            let _ = crate::framework::before_input(&ctx, language);
            super::sync_check_dialog(&mut updates);
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 700.0),
                    )),
                    ..Default::default()
                },
                |ui| updates.dialogs.run(ui),
            );
            output.textures_delta.clear();
            out.clear();
            for shape in &output.shapes {
                texts(&shape.shape, &mut out);
            }
        }
        out
    }

    /// The update dialogs read in the interface's language (their words come from DiscCraft's catalogs).
    #[test]
    fn the_check_dialog_reads_in_japanese() {
        let english = check_dialog_texts(crate::i18n::Language::En);
        assert!(
            english.iter().any(|text| text == "Check for Updates"),
            "{english:?}"
        );
        let japanese = check_dialog_texts(crate::i18n::Language::Ja);
        assert!(
            japanese.iter().any(|text| text == "更新を確認"),
            "{japanese:?}"
        );
        assert!(
            japanese
                .iter()
                .any(|text| text == "まだ更新を確認していません。"),
            "{japanese:?}"
        );
        assert!(
            japanese.iter().any(|text| text == "今すぐ確認"),
            "{japanese:?}"
        );
    }

    #[test]
    fn the_result_window_shows_every_status_but_idle() {
        for status in [
            Status::Checking,
            Status::UpToDate,
            Status::Failed("offline".into()),
            Status::RateLimited,
            Status::Installing,
            Status::Ready,
            Status::Updated("1.2.3".into()),
            Status::InstallFailed("access denied".into()),
        ] {
            assert!(
                result_window_open(false, &status, false),
                "{status:?} opens the window"
            );
        }
        assert!(!result_window_open(false, &Status::Idle, false));
        assert!(
            !result_window_open(true, &Status::Idle, false),
            "a check that found a release closes it"
        );
        assert!(
            result_window_open(true, &Status::Idle, true),
            "a queued check keeps it open"
        );
    }
}
