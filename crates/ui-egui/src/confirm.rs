//! 未保存の変更の確認ダイアログ（Encore と同じ はい / いいえ / キャンセル。[docs/ui-parity.md](../../../docs/ui-parity.md) §3.5）。
//!
//! プロジェクトを捨てる操作（閉じる、終了、別のプロジェクトを開く、新規作成）の前に、未保存の変更が
//! あれば「変更を保存しますか？」を出す。はい = 保存してから続行（保存をやめたら中止）、いいえ = 保存せずに
//! 続行、キャンセル = 中止。ウィンドウの × も同じ確認を通す。

use crate::DiscCraftApp;
use crate::i18n::{tl, tlf};
use crate::new_project::NewProjectForm;
use crate::theme;

use egui::{Align, Context, Id, Layout, Modal};
use std::path::PathBuf;

/// 確認のあとに実行する操作。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PendingAction {
    /// プロジェクトを閉じて、ようこそ画面に戻る。
    CloseProject,
    /// アプリケーションを終了する。
    Exit,
    /// 「プロジェクトを開く…」のファイルダイアログを出す。
    OpenProject,
    /// 最近使ったプロジェクトを開く。
    OpenRecent(PathBuf),
    /// 新規プロジェクトダイアログの結果で作り直す。
    NewProject(Box<NewProjectForm>),
}

impl DiscCraftApp {
    /// 未保存の変更があれば確認を出してから、なければすぐ `action` を実行する。
    pub(crate) fn confirm_or_run(&mut self, ctx: &Context, action: PendingAction) {
        if self.session.dirty {
            self.pending_action = Some(action);
        } else {
            self.run_pending(ctx, action);
        }
    }

    /// 確認ダイアログを描く（保留中の操作があるときだけ）。
    pub(crate) fn show_confirm(&mut self, ctx: &Context) {
        if self.pending_action.is_none() {
            return;
        }

        let name = self
            .session
            .path
            .as_deref()
            .and_then(|path| path.file_name())
            .map_or_else(
                || tl!("Untitled").to_owned(),
                |name| name.to_string_lossy().into_owned(),
            );

        let mut choice = None;
        let response = Modal::new(Id::unique("confirm_save")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(tlf!("Save changes to {name}?", name = name));
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // 右から左に並ぶので、キャンセルが右端になる。M6.510: ボタンはほかの Craft アプリのダイアログと同じく
                // 両端が丸く、既定の「はい」は青。
                if ui.add(theme::dialog_button(tl!("Cancel"))).clicked() {
                    choice = Some(Choice::Cancel);
                }
                if ui.add(theme::dialog_button(tl!("No"))).clicked() {
                    choice = Some(Choice::Discard);
                }
                if ui.add(theme::default_button(tl!("Yes"))).clicked() {
                    choice = Some(Choice::Save);
                }
            });
        });

        if response.should_close() {
            self.pending_action = None;
            return;
        }

        let Some(choice) = choice else {
            return;
        };

        match choice {
            Choice::Cancel => self.pending_action = None,
            Choice::Discard => {
                if let Some(action) = self.pending_action.take() {
                    self.run_pending(ctx, action);
                }
            }
            Choice::Save if self.session.path.is_none() => {
                // The Save dialog answers on a later frame; the close or exit goes on once it is saved (M9.64).
                let after = self.pending_action.take();
                self.save_project_then(false, after);
            }
            Choice::Save => {
                self.save_project(false);
                if self.session.dirty {
                    // 保存できなかった（名前を付けて保存をやめた など）。操作も中止する。
                    self.pending_action = None;
                } else if let Some(action) = self.pending_action.take() {
                    self.run_pending(ctx, action);
                }
            }
        }
    }

    /// 保留していた操作を実行する。
    pub(crate) fn run_pending(&mut self, ctx: &Context, action: PendingAction) {
        match action {
            PendingAction::CloseProject => self.close_project(),
            PendingAction::Exit => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            PendingAction::OpenProject => self.open_project(),
            PendingAction::OpenRecent(path) => {
                if let Err(error) = self.open_path(&path) {
                    self.status = error;
                }
            }
            PendingAction::NewProject(form) => self.apply_new_project(&form),
        }
    }

    /// ウィンドウの ×（Alt+F4）で閉じてよいか（M9.54: UI のランタイムの閉じる道筋から、`logic` で聞かれる）。確認済み
    /// なら閉じる。ディスクの書き込み中は閉じない（M9.35: 書きかけのディスクは使えない）。保存していない変更があれば
    /// 閉じずに確認ダイアログに回す。
    pub fn close_decision(&mut self) -> craft_ui::shell::embed::CloseDecision {
        use craft_ui::shell::embed::CloseDecision;
        if self.allow_close {
            return CloseDecision::Close;
        }
        if self.disc.is_writing() {
            self.status = crate::i18n::tl!(
                "A disc is being written. Stop it or wait for it to finish first."
            )
            .to_owned();
            return CloseDecision::Hold;
        }
        if self.session.dirty {
            self.pending_action = Some(PendingAction::Exit);
            return CloseDecision::Hold;
        }
        CloseDecision::Close
    }
}

/// ダイアログのボタン。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Choice {
    /// 保存してから続行。
    Save,
    /// 保存せずに続行。
    Discard,
    /// 中止。
    Cancel,
}
