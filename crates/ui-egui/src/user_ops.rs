//! ユーザー操作（Encore のプロパティの「ユーザー操作: すべて許可 [設定…]」）。タイムラインとメニューごとに、
//! リモコンで禁止する操作を選ぶ。DVD の PGC の禁止操作（UOP）になる。

use crate::app::DiscCraftApp;
use crate::i18n::{tl, tlf};
use disccraft_project::{MenuId, TimelineId, UserOp};
use egui::{Context, Id, Modal, Ui};
use serde_json::json;

/// ユーザー操作を設定する相手。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UserOpsTarget {
    Timeline(TimelineId),
    Menu(MenuId),
    ChapterPlaylist(u64),
    /// The whole disc (File ▸ Set Disc User Operations…).
    Disc,
}

/// 開いている「ユーザー操作」ダイアログ。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UserOpsDialog {
    pub(crate) target: UserOpsTarget,
    /// 禁止する操作。
    pub(crate) prohibited: Vec<UserOp>,
}

/// 操作の表示名。
pub(crate) fn op_label(op: UserOp) -> &'static str {
    match op {
        UserOp::TimePlay => tl!("Play or search by time"),
        UserOp::ChapterPlay => tl!("Play or search by chapter"),
        UserOp::TitlePlay => tl!("Play a title"),
        UserOp::Stop => tl!("Stop"),
        UserOp::GoUp => tl!("Go up"),
        UserOp::TimeSearch => tl!("Time or chapter search in the title"),
        UserOp::PreviousChapter => tl!("Previous Chapter"),
        UserOp::NextChapter => tl!("Next Chapter"),
        UserOp::FastForward => tl!("Fast forward"),
        UserOp::Rewind => tl!("Rewind"),
        UserOp::TitleMenu => tl!("Title button"),
        UserOp::RootMenu => tl!("Menu button"),
        UserOp::SubtitleMenu => tl!("Subtitle menu"),
        UserOp::AudioMenu => tl!("Audio menu"),
        UserOp::AngleMenu => tl!("Angle menu"),
        UserOp::ChapterMenu => tl!("Chapter menu"),
        UserOp::Resume => tl!("Resume"),
        UserOp::ButtonSelect => tl!("Select and activate buttons"),
        UserOp::StillOff => tl!("Leave a still"),
        UserOp::Pause => tl!("Pause"),
        UserOp::AudioChange => tl!("Change audio"),
        UserOp::SubtitleChange => tl!("Change subtitles"),
        UserOp::AngleChange => tl!("Change angle"),
        UserOp::KaraokeChange => tl!("Change karaoke mix"),
        UserOp::VideoModeChange => tl!("Change display mode"),
    }
}

/// 行に出す要約（すべて許可 / n 個を禁止）。
pub(crate) fn summary(prohibited: &[UserOp]) -> String {
    if prohibited.is_empty() {
        tl!("All allowed").to_owned()
    } else {
        tlf!("{n} prohibited", n = prohibited.len())
    }
}

impl DiscCraftApp {
    /// ファイル ▸ ディスクのユーザー操作を設定…: the dialog for the whole disc.
    pub(crate) fn open_disc_user_ops(&mut self) {
        self.user_ops_dialog = Some(UserOpsDialog {
            target: UserOpsTarget::Disc,
            prohibited: self.session.project.settings.prohibited_ops.clone(),
        });
    }

    /// プロパティの「ユーザー操作:」の行（要約と 設定… ボタン）。
    pub(crate) fn user_ops_row(&mut self, ui: &mut Ui, target: UserOpsTarget, current: &[UserOp]) {
        ui.horizontal(|ui| {
            crate::properties::prop_label(ui, tl!("User Operations:"));
            let _ = ui.label(summary(current)).on_hover_text(
                current
                    .iter()
                    .map(|op| op_label(*op))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            if ui.button(tl!("Set…")).clicked() {
                self.user_ops_dialog = Some(UserOpsDialog {
                    target,
                    prohibited: current.to_vec(),
                });
            }
        });
    }

    /// 「ユーザー操作」ダイアログ。チェックした操作を許可し、外した操作を禁止する。
    pub(crate) fn user_ops_window(&mut self, ctx: &Context) {
        let Some(mut state) = self.user_ops_dialog.clone() else {
            return;
        };
        let mut done = None;
        let response = Modal::new(Id::unique("user_ops_dialog")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.heading(tl!("User Operations"));
            ui.label(tl!(
                "Checked operations are allowed. Uncheck the ones the viewer may not use (DVD)."
            ));
            ui.add_space(6.0);
            egui::Grid::new("user_ops_grid")
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    for (index, op) in UserOp::ALL.into_iter().enumerate() {
                        let mut allowed = !state.prohibited.contains(&op);
                        if ui.checkbox(&mut allowed, op_label(op)).changed() {
                            if allowed {
                                state.prohibited.retain(|other| *other != op);
                            } else {
                                state.prohibited.push(op);
                            }
                        }
                        if index % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button(tl!("Allow All")).clicked() {
                    state.prohibited.clear();
                }
                ui.separator();
                if ui.button(tl!("OK")).clicked() {
                    done = Some(true);
                }
                if ui.button(tl!("Cancel")).clicked() {
                    done = Some(false);
                }
            });
        });
        if response.should_close() {
            done = Some(false);
        }
        match done {
            Some(true) => {
                self.user_ops_dialog = None;
                let names: Vec<String> = disccraft_project::normalized_ops(&state.prohibited)
                    .into_iter()
                    .map(UserOp::name)
                    .collect();
                let (command, params) = match state.target {
                    UserOpsTarget::Timeline(id) => (
                        "timeline.setUserOps",
                        json!({"timeline": id.0, "prohibited": names}),
                    ),
                    UserOpsTarget::Menu(id) => (
                        "menu.setUserOps",
                        json!({"menu": id.0, "prohibited": names}),
                    ),
                    UserOpsTarget::ChapterPlaylist(id) => (
                        "chapterplaylist.setUserOps",
                        json!({"chapterPlaylist": id, "prohibited": names}),
                    ),
                    UserOpsTarget::Disc => ("disc.setUserOps", json!({"prohibited": names})),
                };
                let _ = self.run_command(command, params);
            }
            Some(false) => self.user_ops_dialog = None,
            None => self.user_ops_dialog = Some(state),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{op_label, summary};
    use disccraft_project::UserOp;

    #[test]
    fn every_operation_has_a_label_and_the_summary_counts() {
        for op in UserOp::ALL {
            assert!(!op_label(op).is_empty());
        }
        assert!(!summary(&[]).is_empty());
        assert!(summary(&[UserOp::Stop, UserOp::Pause]).contains('2'));
    }
}
