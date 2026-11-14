//! ビルド結果のプレビューでのタイトルの再生（ROADMAP M6.177）。Encore のプレビューと同じく、VM がタイトルにいる
//! 間は、そのタイトルの素材（タイムラインの映像と、VM が選んでいるオーディオトラック）をそのチャプターの頭から
//! 映像と音つきで再生し、VM が表示にしている字幕を重ねる。終わったら VM のタイトルの終わり（エンドアクション）へ
//! 進む。スライドショーとチャプター再生リストのタイトルは、今までどおり場所の表示だけ。

use crate::DiscCraftApp;
use crate::i18n::tlf;
use crate::monitor::RemoteKey;

use disccraft_engine::{Player, PlayerAudio, PlayerStatus};
use disccraft_project::{Project, Timeline, TimelineId};
use std::path::PathBuf;

/// プレビューで再生しているタイトル。
pub(crate) struct PreviewPlayback {
    /// 再生を始めた VM の場所（タイトル、チャプター）。
    at: (u8, u16),
    /// 再生を始めたときに VM が選んでいたオーディオと字幕（変われば今の位置から再生し直す）。
    streams: (Option<u8>, Option<u8>),
    /// 再生しているタイムライン（再生できないタイトルなら `None`）。
    timeline: Option<TimelineId>,
    /// 鳴っている音。落とすと止まるので、持っておくだけ。
    _audio: Option<crate::audio_out::AudioOut>,
    /// 映像と音のデコード（終わったか、再生できないなら `None`）。
    player: Option<Player>,
    /// 最後に映したフレームとその位置（秒）。
    frame: Option<(egui::TextureHandle, f64)>,
    /// 16:9 か。
    wide: bool,
    /// 重ねる字幕トラック（タイムラインの字幕トラックの番号、0 から）。
    subtitle: Option<usize>,
}

/// モニターに映すプレビューの映像: フレーム、16:9 か、重ねる字幕。
pub(crate) struct PreviewPicture {
    pub(crate) texture: egui::TextureHandle,
    pub(crate) wide: bool,
    pub(crate) subtitle: Option<String>,
}

/// スライドショーの id（題番号の順: ビルドと同じく id の順）。
pub(crate) fn slideshow_ids(project: &Project) -> Vec<u64> {
    project.slideshows.keys().copied().collect()
}

/// DVD のタイトル `title`（1 から）が再生するタイムライン。ビルドと同じく、題番号はタイムライン、スライドショー、
/// チャプター再生リストの順なので、タイムラインの数より後ろはタイムラインではない。
/// An SD title's frame in square pixels: 480 (NTSC) or 576 (PAL) lines at its display aspect, 16:9 or 4:3.
pub(crate) fn sd_size(wide: bool, standard: disccraft_project::VideoStandard) -> (u32, u32) {
    let height = match standard {
        disccraft_project::VideoStandard::Ntsc => 480,
        disccraft_project::VideoStandard::Pal => 576,
    };
    let width = if wide {
        height * 16 / 9
    } else {
        height * 4 / 3
    };
    (width, height)
}

/// The disc's HD frame (width, height, rate numerator, denominator) when it is a Blu-ray with HD titles.
pub(crate) fn disc_hd(settings: &disccraft_project::DiscSettings) -> Option<(u32, u32, u32, u32)> {
    (settings.kind == disccraft_project::DiscKind::BluRay)
        .then(|| settings.bd_video.hd())
        .flatten()
}

pub(crate) fn title_timeline(project: &Project, title: u8) -> Option<TimelineId> {
    let index = usize::from(title).checked_sub(1)?;
    project.timelines.keys().nth(index).copied()
}

/// チャプター `chapter`（1 から）の頭（秒）。ないチャプターは頭から。
pub(crate) fn chapter_start(timeline: &Timeline, chapter: u16) -> f64 {
    usize::from(chapter)
        .checked_sub(1)
        .and_then(|index| timeline.chapters.get(index))
        .map_or(0.0, |chapter| {
            crate::timeline_view::ticks_to_seconds(chapter.time.0).max(0.0)
        })
}

/// VM が選んでいるオーディオストリーム（1 から）の素材。ディスクの音声はタイムラインのオーディオトラックの順
/// （トラックがなければ映像のファイルの音）なので、その番号のトラック、なければ 1 つ目、トラックがなければ映像。
pub(crate) fn audio_source(
    project: &Project,
    timeline: &Timeline,
    stream: Option<u8>,
) -> Option<PathBuf> {
    let track = stream
        .and_then(|stream| usize::from(stream).checked_sub(1))
        .and_then(|index| timeline.audio.get(index))
        .or_else(|| timeline.audio.first());
    let asset = track.map_or(timeline.video, |track| track.asset);
    project
        .assets
        .get(&asset)
        .map(|asset| PathBuf::from(&asset.path))
}

/// VM が表示にしている字幕ストリーム（1 から）のタイムラインの字幕トラック（0 から）。
pub(crate) fn subtitle_track(timeline: &Timeline, stream: Option<u8>) -> Option<usize> {
    let index = usize::from(stream?).checked_sub(1)?;
    (index < timeline.subtitles.len()).then_some(index)
}

impl DiscCraftApp {
    /// 描くたびに呼ぶ: VM の場所に合わせてタイトルの再生を始め / 止め、新しいフレームを取り、終わったらタイトルを
    /// 終える。
    pub(crate) fn poll_preview_playback(&mut self, ctx: &egui::Context) {
        let wanted = if self.preview.open {
            self.preview.title
        } else {
            None
        };
        let Some(at) = wanted else {
            self.preview_playback = None;
            return;
        };
        let streams = (self.preview.audio, self.preview.subtitle);
        let changed = self
            .preview_playback
            .as_ref()
            .is_none_or(|playback| playback.at != at || playback.streams != streams);
        if changed {
            // オーディオか字幕だけが変わったら、今の位置から再生し直す（同じタイトルとチャプターのまま）。
            let same_place = self
                .preview_playback
                .as_ref()
                .is_some_and(|playback| playback.at == at);
            if same_place && self.preview_resume.is_none() {
                self.preview_resume = self.preview_position();
            }
            // 前の再生を止めてから始める（出力デバイスを 2 つ開かない）。
            self.preview_playback = None;
            let resume = self.preview_resume.take();
            self.preview_playback = Some(self.start_preview_title(at, resume));
        }

        let paused = self.preview_paused;
        let Some(playback) = self.preview_playback.as_mut() else {
            return;
        };
        // 一時停止: 映像は最後のフレームのまま、音も止める（再生で、その位置から再生し直す）。
        if paused {
            if let Some(mut player) = playback.player.take() {
                player.stop();
            }
            playback._audio = None;
            return;
        }
        let Some(player) = playback.player.as_ref() else {
            return;
        };
        if let Some(frame) = player.take_frame()
            && let (Ok(width), Ok(height)) =
                (usize::try_from(frame.width), usize::try_from(frame.height))
            && width.checked_mul(height).and_then(|n| n.checked_mul(3)) == Some(frame.rgb.len())
        {
            let image = egui::ColorImage::from_rgb([width, height], &frame.rgb);
            let texture = ctx.load_texture("preview_frame", image, egui::TextureOptions::LINEAR);
            playback.frame = Some((texture, frame.seconds));
        }
        match player.status() {
            PlayerStatus::Playing => ctx.request_repaint(),
            PlayerStatus::Ended => {
                // タイトルの終わり: VM がエンドアクションへ進む（同じタイトルに戻れば、また頭から再生する）。
                self.preview_playback = None;
                self.send_remote(RemoteKey::Finish);
                ctx.request_repaint();
            }
            PlayerStatus::Failed(message) => {
                self.status = tlf!("Cannot play: {message}", message = message);
                playback.player = None;
            }
        }
    }

    /// タイトル `at` の再生を始める（`resume` があればその秒から、なければチャプターの頭から）。タイムラインで
    /// なければ、再生しない印だけを返す（毎フレーム試さない）。
    fn start_preview_title(
        &mut self,
        (title, chapter): (u8, u16),
        resume: Option<f64>,
    ) -> PreviewPlayback {
        let mut playback = PreviewPlayback {
            at: (title, chapter),
            streams: (self.preview.audio, self.preview.subtitle),
            timeline: None,
            _audio: None,
            player: None,
            frame: None,
            wide: false,
            subtitle: None,
        };
        let project = &self.session.project;
        let Some(id) = title_timeline(project, title) else {
            return playback;
        };
        let Some(timeline) = project.timeline(id) else {
            return playback;
        };
        let Some(video) = project
            .assets
            .get(&timeline.video)
            .map(|asset| PathBuf::from(&asset.path))
        else {
            return playback;
        };
        let from = resume.unwrap_or_else(|| chapter_start(timeline, chapter));
        let audio_path = audio_source(project, timeline, self.preview.audio);
        playback.timeline = Some(id);
        playback.wide = timeline.wide(&project.settings);
        // A Blu-ray HD title shows at the disc's HD size and rate (always 16:9), not as the 720 x 480 SD frame.
        let hd = disc_hd(&project.settings);
        if hd.is_some() {
            playback.wide = true;
        }
        let fps = hd.map_or_else(
            || self.preview_fps(),
            |(_, _, num, den)| f64::from(num) / f64::from(den.max(1)),
        );
        playback.subtitle = subtitle_track(timeline, self.preview.subtitle);

        let device = match crate::audio_out::AudioDevice::open_default() {
            Ok(device) => Some(device),
            Err(message) => {
                self.status = tlf!("Playing without sound: {message}", message = message);
                None
            }
        };
        let player_audio = match (&device, audio_path) {
            (Some(device), Some(path)) => Some(PlayerAudio {
                path,
                rate: device.rate(),
            }),
            _ => None,
        };
        // The picture as the disc shows it, in square pixels: the HD frame, or the SD frame at its display aspect
        // (16:9 or 4:3), so the screen does not stretch it.
        let size = hd.map_or_else(
            || sd_size(playback.wide, project.settings.standard),
            |(width, height, _, _)| (width, height),
        );
        let player = Player::start_sized(video, player_audio, from, fps, Some(size));
        playback._audio = match (device, player.audio()) {
            (Some(device), Some(feed)) => match device.play(feed) {
                Ok(out) => Some(out),
                Err(message) => {
                    self.status = tlf!("Playing without sound: {message}", message = message);
                    None
                }
            },
            _ => None,
        };
        playback.player = Some(player);
        playback
    }

    /// プレビューで再生しているタイトルの今の位置（秒。映したフレームの時刻）。
    pub(crate) fn preview_position(&self) -> Option<f64> {
        self.preview_playback
            .as_ref()
            .and_then(|playback| playback.frame.as_ref())
            .map(|(_, seconds)| *seconds)
    }

    /// モニターに映すプレビューの映像（タイトルを再生していて、フレームがあるとき）。
    pub(crate) fn preview_picture(&mut self) -> Option<PreviewPicture> {
        let playback = self.preview_playback.as_ref()?;
        let (texture, seconds) = playback.frame.clone()?;
        let wide = playback.wide;
        let subtitle = match (playback.timeline, playback.subtitle) {
            (Some(id), Some(track)) => {
                let asset = self
                    .session
                    .project
                    .timeline(id)?
                    .subtitles
                    .get(track)?
                    .asset;
                let cues = self.subtitle_cues(asset);
                crate::subtitles::cue_at(&cues, seconds).map(|cue| cue.text.clone())
            }
            _ => None,
        };
        Some(PreviewPicture {
            texture,
            wide,
            subtitle,
        })
    }

    /// テレビ方式のフレームレート（フレーム / 秒）。
    fn preview_fps(&self) -> f64 {
        let (num, den) = crate::timeline_view::frame_rate(self.session.project.settings.standard);
        f64::from(num) / f64::from(den.max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::{audio_source, chapter_start, subtitle_track, title_timeline};
    use disccraft_project::{
        Asset, AssetId, AssetKind, AudioTrack, Chapter, Project, SubtitleTrack, Tick, Timeline,
        TimelineId,
    };
    use std::path::PathBuf;

    fn timeline(id: u64, video: u64) -> Timeline {
        Timeline {
            id: TimelineId(id),
            name: format!("T{id}"),
            video: AssetId(video),
            audio: Vec::new(),
            subtitles: Vec::new(),
            chapters: vec![chapter(0)],
            end_action: disccraft_project::Link::NotSet,
            menu_remote: disccraft_project::Link::NotSet,
            override_link: None,
            description: String::new(),
            prohibited_ops: Vec::new(),
            widescreen: None,
        }
    }

    fn asset(id: u64, path: &str) -> Asset {
        Asset {
            id: AssetId(id),
            name: path.to_owned(),
            path: path.to_owned(),
            kind: AssetKind::Video,
            offline: false,
            description: String::new(),
        }
    }

    fn chapter(ticks: i64) -> Chapter {
        Chapter {
            time: Tick(ticks),
            name: String::new(),
            poster: None,
        }
    }

    #[test]
    fn a_bluray_hd_disc_previews_at_its_hd_size() {
        use disccraft_project::{BdVideo, DiscKind, DiscSettings};
        let mut settings = DiscSettings::default();
        assert_eq!(super::disc_hd(&settings), None);
        settings.bd_video = BdVideo::Hd1080p23;
        // A DVD has no HD titles, whatever the Blu-ray setting says.
        assert_eq!(super::disc_hd(&settings), None);
        settings.kind = DiscKind::BluRay;
        assert_eq!(super::disc_hd(&settings), Some((1920, 1080, 24_000, 1001)));
        settings.bd_video = BdVideo::Sd;
        assert_eq!(super::disc_hd(&settings), None);
        // An SD title at its display aspect, in square pixels.
        use disccraft_project::VideoStandard;
        assert_eq!(super::sd_size(true, VideoStandard::Ntsc), (853, 480));
        assert_eq!(super::sd_size(false, VideoStandard::Ntsc), (640, 480));
        assert_eq!(super::sd_size(true, VideoStandard::Pal), (1024, 576));
        assert_eq!(super::sd_size(false, VideoStandard::Pal), (768, 576));
    }

    #[test]
    fn titles_are_the_timelines_in_order() {
        let mut project = Project::default();
        for id in [3, 7] {
            let _ = project.timelines.insert(TimelineId(id), timeline(id, 1));
        }
        assert_eq!(title_timeline(&project, 1), Some(TimelineId(3)));
        assert_eq!(title_timeline(&project, 2), Some(TimelineId(7)));
        // After the timelines come slideshows and chapter playlists: not played here.
        assert_eq!(title_timeline(&project, 3), None);
        assert_eq!(title_timeline(&project, 0), None);
    }

    #[test]
    fn a_chapter_plays_from_its_point() {
        let mut movie = timeline(1, 1);
        movie.chapters.push(chapter(254_016_000_000 * 5 / 2));
        assert_eq!(chapter_start(&movie, 1), 0.0);
        assert!((chapter_start(&movie, 2) - 2.5).abs() < 1e-9);
        // A chapter the timeline does not have: from the start.
        assert_eq!(chapter_start(&movie, 9), 0.0);
        assert_eq!(chapter_start(&movie, 0), 0.0);
    }

    #[test]
    fn the_sound_is_the_track_the_disc_chose() {
        let mut project = Project::default();
        for (id, path) in [(1, "movie.mp4"), (2, "ja.wav"), (3, "en.ac3")] {
            let _ = project.assets.insert(AssetId(id), asset(id, path));
        }
        let mut movie = timeline(4, 1);
        // No audio tracks: the video file's own sound.
        assert_eq!(
            audio_source(&project, &movie, Some(1)),
            Some(PathBuf::from("movie.mp4"))
        );
        movie.audio = vec![
            AudioTrack {
                asset: AssetId(2),
                language: "ja".to_owned(),
            },
            AudioTrack {
                asset: AssetId(3),
                language: "en".to_owned(),
            },
        ];
        assert_eq!(
            audio_source(&project, &movie, Some(2)),
            Some(PathBuf::from("en.ac3"))
        );
        // None chosen, or one the timeline does not have: the first track.
        assert_eq!(
            audio_source(&project, &movie, None),
            Some(PathBuf::from("ja.wav"))
        );
        assert_eq!(
            audio_source(&project, &movie, Some(8)),
            Some(PathBuf::from("ja.wav"))
        );
    }

    #[test]
    fn the_subtitle_is_the_stream_shown() {
        let mut movie = timeline(1, 1);
        movie.subtitles = vec![
            SubtitleTrack {
                asset: AssetId(5),
                language: "ja".to_owned(),
            },
            SubtitleTrack {
                asset: AssetId(6),
                language: "en".to_owned(),
            },
        ];
        assert_eq!(subtitle_track(&movie, Some(2)), Some(1));
        assert_eq!(subtitle_track(&movie, None), None);
        assert_eq!(subtitle_track(&movie, Some(3)), None);
    }
}
