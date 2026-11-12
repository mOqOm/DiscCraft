//! タイムラインの再生（タイムライン ▸ 再生、Space）。engine の [`Player`] が映像を実時間でデコードし、
//! 画面は描くたびにいちばん新しいフレームをモニターに映して再生ヘッドを進める。音はまだ出さない。

use crate::DiscCraftApp;
use crate::i18n::{tl, tlf};
use crate::workspace::CenterTab;

use disccraft_engine::{Player, PlayerFrame, PlayerStatus};
use disccraft_project::TimelineId;
use std::path::PathBuf;

/// 再生ヘッドを利用者が動かしたとみなすずれ（秒）。これより動いていたら再生を止める。
const MOVED_BY_USER: f64 = 0.25;

/// 再生中のタイムライン。
pub(crate) struct Playback {
    /// 再生しているタイムライン。
    timeline: TimelineId,
    /// 映像のアセット（モニターのフレームのキャッシュのキー）。
    asset: u64,
    /// デコードしているスレッド。
    player: Player,
    /// 最後に再生ヘッドを置いた位置（秒）。
    last: f64,
}

impl DiscCraftApp {
    /// 再生中か。
    pub(crate) fn is_playing(&self) -> bool {
        self.playback.is_some()
    }

    /// 再生 / 停止を切り替える。タイムラインビューアのタイムラインを、再生ヘッドの位置から（終わりにいれば
    /// 頭から）再生する。
    pub(crate) fn toggle_playback(&mut self, ctx: &egui::Context) {
        if self.playback.take().is_some() {
            return;
        }
        let Some(id) = self.viewer_timeline() else {
            self.status = tl!("Select a timeline first").to_string();
            return;
        };
        let Some(video) = self
            .session
            .project
            .timeline(id)
            .map(|timeline| timeline.video)
        else {
            return;
        };
        let Some(path) = self
            .session
            .project
            .assets
            .get(&video)
            .map(|asset| PathBuf::from(&asset.path))
        else {
            return;
        };
        let duration = self.asset_seconds(video).unwrap_or(0.0);
        let fps = self.playback_fps();
        let mut from = crate::timeline_view::playhead_seconds(ctx, id);
        if duration > 0.0 && from >= duration - 1.0 / fps {
            from = 0.0;
        }

        // Encore と同じくモニターで見せる（ビルド結果のプレビュー中なら閉じる）。
        if self.preview.open {
            self.close_preview();
        }
        self.tab = CenterTab::Monitor;
        crate::timeline_view::set_playhead_seconds(ctx, id, from);
        self.playback = Some(Playback {
            timeline: id,
            asset: video.0,
            player: Player::start(path, from, fps),
            last: from,
        });
        ctx.request_repaint();
    }

    /// 描くたびに呼ぶ: 新しいフレームをモニターに映して再生ヘッドを進め、終わりや失敗で止める。
    pub(crate) fn poll_playback(&mut self, ctx: &egui::Context) {
        let Some(playback) = self.playback.as_ref() else {
            return;
        };
        let id = playback.timeline;
        let here = crate::timeline_view::playhead_seconds(ctx, id);
        // ほかのタイムラインに移ったり、再生ヘッドを動かしたりしたら止める。
        if self.viewer_timeline() != Some(id) || (here - playback.last).abs() > MOVED_BY_USER {
            self.playback = None;
            return;
        }

        let frame = playback.player.take_frame();
        let status = playback.player.status();
        if let Some(frame) = frame {
            self.show_played_frame(ctx, id, &frame);
        }
        match status {
            PlayerStatus::Playing => ctx.request_repaint(),
            PlayerStatus::Ended => self.playback = None,
            PlayerStatus::Failed(message) => {
                self.status = tlf!("Cannot play: {message}", message = message);
                self.playback = None;
            }
        }
    }

    /// 再生したフレームをモニターのフレームにし、再生ヘッドをその位置に置く。
    fn show_played_frame(&mut self, ctx: &egui::Context, id: TimelineId, frame: &PlayerFrame) {
        let fps = self.playback_fps();
        let Some(playback) = self.playback.as_mut() else {
            return;
        };
        let (Ok(width), Ok(height)) = (usize::try_from(frame.width), usize::try_from(frame.height))
        else {
            return;
        };
        if width.checked_mul(height).and_then(|n| n.checked_mul(3)) != Some(frame.rgb.len()) {
            return;
        }
        let image = egui::ColorImage::from_rgb([width, height], &frame.rgb);
        let texture = ctx.load_texture("monitor_frame", image, egui::TextureOptions::LINEAR);
        // モニターはフレーム番号をキャッシュのキーにする（`monitor.rs` の `timeline_frame` と同じ数え方）。
        // `as` saturates; the number is only a key.
        let key = (frame.seconds * fps).round() as i64;
        self.monitor_frame = Some((playback.asset, key, texture));
        playback.last = frame.seconds;
        crate::timeline_view::set_playhead_seconds(ctx, id, frame.seconds);
    }

    /// テレビ方式のフレームレート（フレーム / 秒）。
    fn playback_fps(&self) -> f64 {
        let (num, den) = crate::timeline_view::frame_rate(self.session.project.settings.standard);
        f64::from(num) / f64::from(den.max(1))
    }
}
