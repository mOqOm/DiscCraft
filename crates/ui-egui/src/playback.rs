//! タイムラインの再生（タイムライン ▸ 再生、Space）。engine の [`Player`] が映像を実時間でデコードし、
//! 画面は描くたびにいちばん新しいフレームをモニターに映して再生ヘッドを進める。音（M6.175）はタイムラインの
//! 1 つ目のオーディオトラック（なければ映像のファイルの音）を、OS の既定の出力デバイスで鳴らす
//! （[`crate::audio_out`]）。

use crate::DiscCraftApp;
use crate::i18n::{tl, tlf};

use disccraft_engine::{Player, PlayerAudio, PlayerFrame, PlayerStatus};
use disccraft_project::{AssetId, TimelineId};

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
    /// 絵をデコードできず音だけ再生していることをもう知らせたか。
    no_picture_shown: bool,
    /// デコードしているスレッド。
    player: Player,
    /// 最後に再生ヘッドを置いた位置（秒）。
    last: f64,
    /// 再生しているクリップの、タイムライン上の位置からファイル上の位置を引いた値（秒）。フレームの時刻に
    /// 足すとタイムライン上の時刻になる。
    offset: f64,
    /// 再生しているクリップがタイムライン上で終わる位置（秒）。次のクリップがあれば、そこから続ける。
    clip_end: f64,
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
        let duration = self.timeline_seconds(id);
        let fps = self.playback_fps();
        let mut from = crate::timeline_view::playhead_seconds(ctx, id);
        if duration > 0.0 && from >= duration - 1.0 / fps {
            from = 0.0;
        }
        self.start_playback_at(ctx, id, from);
    }

    /// タイムライン `id` を `from`（秒）から再生する。再生するのは `from` にあるビデオクリップで、終わりに来て
    /// 次のクリップがあれば `poll_playback` がそこから続ける。
    fn start_playback_at(&mut self, ctx: &egui::Context, id: TimelineId, from: f64) {
        let fps = self.playback_fps();
        let clips = self.timeline_clips(id);
        let Some((clip, local)) = clip_at(&clips, from) else {
            return;
        };
        let video = clip.asset;
        let offset = clip.start - clip.source;
        let clip_end = clip.start + clip.duration;
        // 聞くのは、見出しのスピーカーで選んだオーディオトラック（Encore と同じく、初めは最後に足したトラック）。
        let heard = self.heard_audio(id).unwrap_or(0);
        let track = self
            .session
            .project
            .timeline(id)
            .and_then(|timeline| timeline.audio.get(heard).map(|track| track.asset));
        let Some(path) = self
            .session
            .project
            .assets
            .get(&video)
            .map(|asset| asset.media_file().into_owned())
        else {
            return;
        };

        // Encore と同じくモニターで見せる（ビルド結果のプレビュー中なら閉じる）。
        if self.preview.open {
            self.close_preview();
        }
        self.show_panel(crate::workspace::Panel::Monitor);
        crate::timeline_view::set_playhead_seconds(ctx, id, from);

        // 音: 1 つ目のオーディオトラック、なければ映像のファイルの音を、出力デバイスの周波数で。
        let audio_path = match track {
            Some(asset) => self
                .session
                .project
                .assets
                .get(&asset)
                .map(|asset| asset.media_file().into_owned()),
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
                // オーディオトラックはタイムライン全体に沿って流れる。映像のファイルの音はクリップと同じ位置。
                from: track.is_some().then_some(from),
            }),
            _ => None,
        };
        // The frames at the monitor's screen size, as its still frames.
        let size = self.monitor_frame_size(id);
        let player = Player::start_sized(path, player_audio, local, fps, Some(size));
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
            no_picture_shown: false,
            player,
            last: from,
            offset,
            clip_end,
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
        let no_picture = playback.player.no_picture();
        let announce = !playback.no_picture_shown;
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
        // 絵をデコードできない映像は、音だけを再生する（黒いフレームで再生ヘッドは進む）。一度だけ知らせる。
        if let Some(message) = &no_picture
            && announce
        {
            self.status = tlf!(
                "Playing the sound only (the picture cannot be decoded): {message}",
                message = message
            );
            if let Some(playback) = self.playback.as_mut() {
                playback.no_picture_shown = true;
            }
        }
        if let Some(frame) = frame {
            self.monitor_note = no_picture;
            self.show_played_frame(ctx, id, &frame);
        }
        match status {
            PlayerStatus::Playing => ctx.request_repaint(),
            PlayerStatus::Ended => {
                // 次のクリップがあれば、つなぎ目から続ける。
                let next = self.playback.as_ref().map(|playback| playback.clip_end);
                self.playback = None;
                let total = self.timeline_seconds(id);
                if let Some(next) = next
                    && next < total - 0.5 / self.playback_fps()
                {
                    crate::timeline_view::set_playhead_seconds(ctx, id, next);
                    self.start_playback_at(ctx, id, next);
                }
            }
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
        // クリップのファイル上の時刻を、タイムライン上の時刻に直す（クリップの終わりは越えない）。
        let seconds = (frame.seconds + playback.offset).min(playback.clip_end);
        let key = (seconds * fps).round() as i64;
        self.monitor_frame = Some((playback.asset, key, texture));
        playback.last = seconds;
        crate::timeline_view::set_playhead_seconds(ctx, id, seconds);
    }

    /// テレビ方式のフレームレート（フレーム / 秒）。
    fn playback_fps(&self) -> f64 {
        let (num, den) = crate::timeline_view::frame_rate(self.session.project.settings.standard);
        f64::from(num) / f64::from(den.max(1))
    }
}

/// One video clip of a timeline, in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ClipSpan {
    /// The clip's video asset.
    pub(crate) asset: AssetId,
    /// Where the clip starts on the timeline.
    pub(crate) start: f64,
    /// How long it plays.
    pub(crate) duration: f64,
    /// Where in its file it starts playing (its trimmed in point).
    pub(crate) source: f64,
}

/// The clip of `spans` that plays at `seconds` (the last clip at or past the end), with the position in its file.
/// In a gap between clips (Encore: a clip moved along the track) it is the clip after the gap, at its start:
/// playback goes on from there.
pub(crate) fn clip_at(spans: &[ClipSpan], seconds: f64) -> Option<(ClipSpan, f64)> {
    if in_gap(spans, seconds) {
        let next = spans.iter().find(|clip| clip.start > seconds)?;
        return Some((*next, next.source));
    }
    let clip = spans
        .iter()
        .rev()
        .find(|clip| seconds >= clip.start)
        .or_else(|| spans.first())?;
    let inside = (seconds - clip.start).clamp(0.0, clip.duration.max(0.0));
    Some((*clip, clip.source + inside))
}

/// Whether `seconds` falls in a gap: in front of a clip and not inside the clip before it. A gap plays black.
pub(crate) fn in_gap(spans: &[ClipSpan], seconds: f64) -> bool {
    let inside = spans
        .iter()
        .any(|clip| seconds >= clip.start && seconds < clip.start + clip.duration);
    !inside && spans.iter().any(|clip| clip.start > seconds)
}

impl DiscCraftApp {
    /// The video clips of timeline `id` (Encore: the clips butted together on the video track, each trimmed).
    /// A timeline of one whole clip is its video, measured by the asset's own length; others are measured by the
    /// engine once and kept until the clips or their trims change.
    pub(crate) fn timeline_clips(&mut self, id: TimelineId) -> Vec<ClipSpan> {
        let Some(timeline) = self.session.project.timeline(id) else {
            return Vec::new();
        };
        let video = timeline.video;
        if timeline.more_video.is_empty()
            && timeline.video_trims.is_empty()
            && !timeline.has_video_gaps()
        {
            let duration = self.asset_seconds(video).unwrap_or(0.0);
            return vec![ClipSpan {
                asset: video,
                start: 0.0,
                duration,
                source: 0.0,
            }];
        }
        let mut key: Vec<u64> = timeline.videos().iter().map(|asset| asset.0).collect();
        key.push(u64::MAX);
        for trim in &timeline.video_trims {
            key.push(u64::from(trim.start));
            key.push(trim.end.map_or(0, |end| u64::from(end).saturating_add(1)));
        }
        key.push(u64::MAX);
        key.extend(timeline.video_gaps.iter().map(|gap| u64::from(*gap)));
        if let Some((cached_id, cached_key, spans)) = &self.clip_cache
            && *cached_id == id
            && *cached_key == key
        {
            return spans.clone();
        }
        let seconds = |value: Option<&serde_json::Value>| {
            value
                .and_then(serde_json::Value::as_i64)
                .map(crate::timeline_view::ticks_to_seconds)
        };
        let spans: Vec<ClipSpan> = self
            .run_query("timeline.clips", serde_json::json!({ "timeline": id.0 }))
            .and_then(|value| {
                value
                    .get("clips")
                    .and_then(|clips| clips.as_array().cloned())
            })
            .unwrap_or_default()
            .iter()
            .filter_map(|clip| {
                Some(ClipSpan {
                    asset: AssetId(clip.get("asset")?.as_u64()?),
                    start: seconds(clip.get("start"))?,
                    duration: seconds(clip.get("duration"))?,
                    source: seconds(clip.get("sourceStart")).unwrap_or(0.0),
                })
            })
            .collect();
        self.clip_cache = Some((id, key, spans.clone()));
        spans
    }

    /// How long timeline `id` plays: its clips added up.
    pub(crate) fn timeline_seconds(&mut self, id: TimelineId) -> f64 {
        self.timeline_clips(id)
            .last()
            .map_or(0.0, |clip| clip.start + clip.duration)
    }
}

#[cfg(test)]
mod clip_tests {
    use super::{ClipSpan, clip_at};
    use disccraft_project::AssetId;

    /// The playhead maps to the clip under it and the place in that clip's file (its trim counted in).
    #[test]
    fn the_playhead_finds_its_clip() {
        let spans = [
            ClipSpan {
                asset: AssetId(1),
                start: 0.0,
                duration: 10.0,
                source: 0.0,
            },
            ClipSpan {
                asset: AssetId(2),
                start: 10.0,
                duration: 5.0,
                source: 2.0,
            },
        ];
        let at = |seconds| clip_at(&spans, seconds).map(|(clip, local)| (clip.asset.0, local));
        assert_eq!(at(0.0), Some((1, 0.0)));
        assert_eq!(at(9.5), Some((1, 9.5)));
        // The second clip starts two seconds into its file.
        assert_eq!(at(10.0), Some((2, 2.0)));
        assert_eq!(at(12.5), Some((2, 4.5)));
        // Past the end: the last clip's last moment.
        assert_eq!(at(99.0), Some((2, 7.0)));
        assert_eq!(clip_at(&[], 1.0), None);
    }

    /// A gap between clips: nothing plays there (black), and playback takes up the clip after it.
    #[test]
    fn a_gap_is_no_clip() {
        let span = |asset, start, duration| ClipSpan {
            asset: AssetId(asset),
            start,
            duration,
            source: 0.0,
        };
        let spans = [span(1, 2.0, 3.0), span(2, 8.0, 4.0)];
        assert!(super::in_gap(&spans, 0.5));
        assert!(!super::in_gap(&spans, 2.0));
        assert!(!super::in_gap(&spans, 4.9));
        assert!(super::in_gap(&spans, 5.0));
        assert!(!super::in_gap(&spans, 8.0));
        assert!(!super::in_gap(&spans, 99.0));
        let at = |seconds| clip_at(&spans, seconds).map(|(clip, local)| (clip.asset.0, local));
        assert_eq!(at(0.5), Some((1, 0.0)));
        assert_eq!(at(6.0), Some((2, 0.0)));
        assert_eq!(at(9.0), Some((2, 1.0)));
    }
}

/// The audio track heard in the monitor, as in Encore CS6: the last track (a track just added is the one heard)
/// until another track's speaker is clicked; a choice made for a different number of tracks no longer counts.
pub(crate) fn heard_track(choice: Option<(usize, usize)>, count: usize) -> Option<usize> {
    let last = count.checked_sub(1)?;
    Some(match choice {
        Some((tracks, track)) if tracks == count && track < count => track,
        _ => last,
    })
}

impl DiscCraftApp {
    /// The audio track of `timeline` that plays in the monitor (`None` without audio tracks).
    pub(crate) fn heard_audio(&self, timeline: TimelineId) -> Option<usize> {
        let count = self
            .session
            .project
            .timeline(timeline)
            .map_or(0, |timeline| timeline.audio.len());
        heard_track(self.heard_audio.get(&timeline).copied(), count)
    }

    /// The speaker on an audio track's header: that track is the one heard.
    pub(crate) fn set_heard_audio(&mut self, timeline: TimelineId, track: usize) {
        let count = self
            .session
            .project
            .timeline(timeline)
            .map_or(0, |timeline| timeline.audio.len());
        let _ = self.heard_audio.insert(timeline, (count, track));
    }
}

#[cfg(test)]
mod heard_tests {
    use super::heard_track;

    /// Encore CS6: a new audio track becomes the one heard; a click on another track's header moves it.
    #[test]
    fn the_newest_audio_track_is_heard_until_another_is_chosen() {
        assert_eq!(heard_track(None, 0), None);
        assert_eq!(heard_track(None, 1), Some(0));
        assert_eq!(heard_track(None, 2), Some(1));
        // Track 1 chosen among two tracks.
        assert_eq!(heard_track(Some((2, 0)), 2), Some(0));
        // A third track was added since: it is heard.
        assert_eq!(heard_track(Some((2, 0)), 3), Some(2));
        // The chosen track is gone.
        assert_eq!(heard_track(Some((2, 1)), 1), Some(0));
    }
}
