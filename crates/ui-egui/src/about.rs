//! ヘルプ ▸ DiscCraft について…（Encore の「Adobe Encore について…」にあたる）。

use crate::app::DiscCraftApp;
use crate::i18n::{tl, tlf};

use egui::{Context, Id, Modal};

impl DiscCraftApp {
    /// 「DiscCraft について」ダイアログ（開いているときだけ描く）。
    pub(crate) fn about_window(&mut self, ctx: &Context) {
        if !self.about_open {
            return;
        }
        let mut close = false;
        let response = Modal::new(Id::unique("about_disccraft")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading("DiscCraft");
            ui.label(tlf!(
                "Version {version}",
                version = env!("CARGO_PKG_VERSION")
            ));
            ui.add_space(6.0);
            ui.label(tl!("DVD and Blu-ray authoring."));
            ui.label(tlf!(
                "License: {license}",
                license = env!("CARGO_PKG_LICENSE")
            ));
            ui.add_space(6.0);
            if ui.button(tl!("OK")).clicked() {
                close = true;
            }
        });
        if close || response.should_close() {
            self.about_open = false;
        }
    }
}
