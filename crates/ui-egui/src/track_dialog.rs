//! タイムライン ▸ オーディオトラックを追加… / 字幕トラックを追加… / オーディオトラックを削除 / 字幕トラックを削除
//! （Encore のタイムラインメニューと同じ項目。[docs/ui-parity.md](../../../docs/ui-parity.md) §3.1）。
//!
//! DiscCraft のトラックは必ずアセットを持つので、追加のときにアセットと言語をダイアログで選ぶ。

use crate::app::{DiscCraftApp, Selection};
use crate::gui_params;
use crate::i18n::tl;

use disccraft_project::{AssetId, AssetKind, TimelineId};
use egui::{Context, Id, Modal};
use serde_json::Value;

/// トラックの種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrackKind {
    /// オーディオトラック。
    Audio,
    /// 字幕トラック。
    Subtitle,
}

impl TrackKind {
    /// このトラックに置けるアセットの種類。
    fn asset_kind(self) -> AssetKind {
        match self {
            Self::Audio => AssetKind::Audio,
            Self::Subtitle => AssetKind::Subtitle,
        }
    }

    /// ダイアログの見出し。
    fn title(self) -> &'static str {
        match self {
            Self::Audio => tl!("Add Audio Track"),
            Self::Subtitle => tl!("Add Subtitle Track"),
        }
    }

    /// 追加するコマンドとパラメータ。
    fn add_command(self, timeline: u64, asset: u64, language: &str) -> (&'static str, Value) {
        match self {
            Self::Audio => (
                "timeline.addAudioTrack",
                gui_params::add_audio_track(timeline, asset, language),
            ),
            Self::Subtitle => (
                "timeline.addSubtitleTrack",
                gui_params::add_subtitle_track(timeline, asset, language),
            ),
        }
    }

    /// 削除するコマンド。
    fn remove_command(self) -> &'static str {
        match self {
            Self::Audio => "timeline.removeAudioTrack",
            Self::Subtitle => "timeline.removeSubtitleTrack",
        }
    }
}

/// 「トラックを追加」ダイアログの状態。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TrackDialog {
    /// 追加先のタイムライン。
    timeline: TimelineId,
    /// トラックの種類。
    kind: TrackKind,
    /// 選んだアセット。
    asset: Option<AssetId>,
    /// 言語コード（ja、en など）。
    language: String,
}

impl DiscCraftApp {
    /// 選択中のタイムライン（タイムラインメニューの対象）。
    pub(crate) fn selected_timeline(&self) -> Option<TimelineId> {
        match self.selection {
            Selection::Timeline(id) => Some(id),
            _ => None,
        }
    }

    /// 選択中のタイムラインにある、`kind` のトラックの数。
    pub(crate) fn track_count(&self, kind: TrackKind) -> usize {
        let Some(timeline) = self
            .selected_timeline()
            .and_then(|id| self.session.project.timeline(id))
        else {
            return 0;
        };
        match kind {
            TrackKind::Audio => timeline.audio.len(),
            TrackKind::Subtitle => timeline.subtitles.len(),
        }
    }

    /// 「トラックを追加」ダイアログを開く。最初のアセットを選んでおく。
    pub(crate) fn open_track_dialog(&mut self, kind: TrackKind) {
        let Some(timeline) = self.selected_timeline() else {
            return;
        };
        let asset = self.track_assets(kind).first().map(|(id, _)| *id);
        self.track_dialog = Some(TrackDialog {
            timeline,
            kind,
            asset,
            language: "ja".to_owned(),
        });
    }

    /// `asset` を選んだ状態でトラックの追加ダイアログを開く（字幕を読み込み… の続き）。
    pub(crate) fn open_track_dialog_with(
        &mut self,
        timeline: TimelineId,
        kind: TrackKind,
        asset: AssetId,
        language: String,
    ) {
        self.track_dialog = Some(TrackDialog {
            timeline,
            kind,
            asset: Some(asset),
            language,
        });
    }

    /// 選択中のタイムラインの、いちばん下の `kind` のトラックを削除する。
    pub(crate) fn remove_last_track(&mut self, kind: TrackKind) {
        let Some(timeline) = self.selected_timeline() else {
            return;
        };
        let Some(track) = self.track_count(kind).checked_sub(1) else {
            return;
        };
        let _ = self.run_command(
            kind.remove_command(),
            gui_params::remove_track(timeline.0, track),
        );
    }

    /// `kind` のトラックに置けるアセット（id と名前、プロジェクトの順）。
    fn track_assets(&self, kind: TrackKind) -> Vec<(AssetId, String)> {
        self.session
            .project
            .assets
            .values()
            .filter(|asset| asset.kind == kind.asset_kind())
            .map(|asset| (asset.id, asset.name.clone()))
            .collect()
    }

    /// 「トラックを追加」ダイアログ（開いているときだけ描く）。
    pub(crate) fn track_dialog_window(&mut self, ctx: &Context) {
        let Some(mut state) = self.track_dialog.clone() else {
            return;
        };
        let assets = self.track_assets(state.kind);

        let mut done = None;
        let response = Modal::new(Id::unique("track_dialog")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.heading(state.kind.title());
            ui.add_space(6.0);
            egui::Grid::new("track_dialog_grid")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label(tl!("Asset:"));
                    let current = assets
                        .iter()
                        .find(|(id, _)| Some(*id) == state.asset)
                        .map_or_else(|| tl!("None").to_owned(), |(_, name)| name.clone());
                    egui::ComboBox::from_id_salt("track_dialog_asset")
                        .width(220.0)
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for (id, name) in &assets {
                                if ui
                                    .selectable_label(state.asset == Some(*id), name)
                                    .clicked()
                                {
                                    state.asset = Some(*id);
                                }
                            }
                        });
                    ui.end_row();

                    ui.label(tl!("Language:"));
                    ui.add(egui::TextEdit::singleline(&mut state.language).desired_width(80.0));
                    ui.end_row();
                });
            if assets.is_empty() {
                ui.small(match state.kind {
                    TrackKind::Audio => tl!("Import an audio file first."),
                    TrackKind::Subtitle => tl!("Import a subtitle file (.srt) first."),
                });
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(state.asset.is_some(), egui::Button::new(tl!("OK")))
                    .clicked()
                {
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
                self.track_dialog = None;
                if let Some(asset) = state.asset {
                    let (command, params) =
                        state
                            .kind
                            .add_command(state.timeline.0, asset.0, state.language.trim());
                    let added = self.run_command(command, params);
                    // Encore と同じく、足した字幕トラックをモニターに出す。
                    if state.kind == TrackKind::Subtitle
                        && let Some(track) = added
                            .and_then(|value| value.get("track").and_then(Value::as_u64))
                            .and_then(|track| usize::try_from(track).ok())
                    {
                        let _ = self.shown_subtitles.insert(state.timeline, Some(track));
                    }
                }
            }
            Some(false) => self.track_dialog = None,
            None => self.track_dialog = Some(state),
        }
    }
}
