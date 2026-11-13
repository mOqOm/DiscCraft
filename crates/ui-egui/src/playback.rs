//! タイムラインの再生（タイムライン ▸ 再生、Space）。engine の [`Player`] が映像を実時間でデコードし、
//! 画面は描くたびにいちばん新しいフレームをモニターに映して再生ヘッドを進める。音（M6.175）はタイムラインの
//! 1 つ目のオーディオトラック（なければ映像のファイルの音）を、OS の既定の出力デバイスで鳴らす
//! （[`crate::audio_out`]）。

use crate::DiscCraftApp;
use crate::i18n::{tl, tlf};
use crate::workspace::CenterTab;

use disccraft_engine::{Player, PlayerAudio, PlayerFrame, PlayerStatus};
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
    /// 鳴っている音（出力デバイスがないときは `None`）。落とすと止まるので、持っておくだけ。
    _audio: Option<crate::audio_out::AudioOut>,
    /// オーディオトラックの音か（映像のファイルの音なら、音がなくても知らせない）。
    track_audio: bool,
    /// 音を再生できないことをもう知らせたか。
    audio_error_shown: bool,
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
        let Some((video, track)) = self.session.project.timeline(id).map(|timeline| {
            (
                timeline.video,
                timeline.audio.first().map(|track| track.asset),
            )
        }) else {
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

        // 音: 1 つ目のオーディオトラック、なければ映像のファイルの音を、出力デバイスの周波数で。
        let audio_path = match track {
            Some(asset) => self
                .session
                .project
                .assets
                .get(&asset)
                .map(|asset| PathBuf::from(&asset.path)),
            None => Some(path.clone()),
        };
        let device = match crate::audio_out::AudioDevice::open_default() {
            Ok(device) => Some(device),
            Err(message) => {
                self.status = tlf!("Playing without sound: {message}", message = message);
                None
            }
        };
        let player_audio = match (&device, audio_path) {
            (Some(device), Some(audio)) => Some(PlayerAudio {
                path: audio,
                rate: device.rate(),
            }),
            _ => None,
        };
        let player = Player::start_with_audio(path, player_audio, from, fps);
        let audio = match (device, player.audio()) {
            (Some(device), Some(feed)) => match device.play(feed) {
                Ok(out) => Some(out),
                Err(message) => {
                    self.status = tlf!("Playing without sound: {message}", message = message);
                    None
                }
            },
            _ => None,
        };
        self.playback = Some(Playback {
            timeline: id,
            asset: video.0,
            _audio: audio,
            track_audio: track.is_some(),
            audio_error_shown: false,
            player,
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
        // オーディオトラックの音をデコードできなければ、一度だけ知らせる（映像はそのまま再生する）。
        let audio_error = if playback.track_audio && !playback.audio_error_shown {
            playback.player.audio().and_then(|feed| feed.error())
        } else {
            None
        };
        if let Some(message) = audio_error {
            self.status = tlf!("Cannot play the sound: {message}", message = message);
            if let Some(playback) = self.playback.as_mut() {
                playback.audio_error_shown = true;
            }
        }
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
