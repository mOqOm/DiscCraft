//! ビルド結果のプレビューでのタイトルの再生（ROADMAP M6.177）。Encore のプレビューと同じく、VM がタイトルにいる
//! 間は、そのタイトルの素材（タイムラインの映像と、VM が選んでいるオーディオトラック）をそのチャプターの頭から
//! 映像と音つきで再生し、VM が表示にしている字幕を重ねる。終わったら VM のタイトルの終わり（エンドアクション）へ
//! 進む。
//!
//! M6.253: a chapter playlist's title plays its entries (chapters of its master timeline) one after another, and
//! a slideshow's title shows its slides for their durations with the show's audio clips. As an entry or a slide
//! ends, the VM is told the next chapter (`preview.chapter`), so the remote's chapter keys go on from what is on
//! screen. A slide moves with its pan and zoom as the build moves it (M6.285, the build's own motion), Scale and
//! Crop fills the frame, the transitions run across the cuts as the build mixes them (M6.286), and with the
//! subtitle stream on a slide's Create Subtitle shows (M6.287).

use crate::DiscCraftApp;
use crate::i18n::tlf;
use crate::monitor::RemoteKey;
use disccraft_project::TransitionKind;

use disccraft_engine::{Player, PlayerAudio, PlayerStatus};
use disccraft_project::{AssetId, Project, Timeline, TimelineId};
use std::path::PathBuf;
use std::time::Instant;

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
    /// A chapter playlist's entries from the one playing on: the stretch of the master timeline each plays
    /// (start and end, seconds). Empty for a timeline's own title, which plays to its end.
    segments: Vec<(f64, f64)>,
    /// The entry number (from 1) of each of `segments`, in the order they play.
    entries: Vec<u16>,
    /// A slideshow's slides, when the title is one.
    slides: Option<SlidePlay>,
    /// How the slide on screen is shown now: its pan and zoom ([`disccraft_build::SlideMotion::at`]) and
    /// whether it fills the frame (Scale and Crop) rather than fitting in it.
    view: SlideView,
    /// The neighbouring slide while a transition runs at a cut.
    blend: Option<SlideBlend>,
    /// The video clip of the timeline that is playing (M6.271): what is added to a time in its file to get the
    /// time on the timeline, and where on the timeline the clip ends (`None`: at its file's end).
    clip: (f64, Option<f64>),
    /// M13.19: the chapter (from 1) of the timeline whose parental alternative plays in its place, its own video
    /// and sound from its start; then the title goes on at the next chapter.
    alternative: Option<(u16, TimelineId)>,
}

/// A slideshow being shown in the preview.
struct SlidePlay {
    /// Each slide: its asset, when it starts and ends (seconds of the show), and whether it stays up until the
    /// viewer moves on (manual advance).
    slides: Vec<(AssetId, f64, f64, bool)>,
    /// Each slide's pan and zoom (its own, or a random one the show asks for) and whether it fills the frame.
    looks: Vec<(Option<disccraft_build::SlideMotion>, bool, Option<f32>)>,
    /// The transition at the cut before each slide (its own, or the show's) and its length, seconds.
    cuts: Vec<(TransitionKind, f64)>,
    /// The slideshow, and the slide on screen.
    show: u64,
    index: usize,
    /// The position when the clock was started (seconds of the show), and the clock (`None` while paused).
    offset: f64,
    began: Option<Instant>,
    /// The show's audio clips and their lengths (seconds), where known.
    clips: Vec<(PathBuf, Option<f64>)>,
    /// The clips start over when the last has ended.
    loop_audio: bool,
    /// The clip that sounds: its number, its decoder, and the output (dropping it stops the sound).
    sound: Option<(usize, Player, Option<crate::audio_out::AudioOut>)>,
    /// No clip is left to play (or none can be played).
    silent: bool,
}

/// What a title of the disc plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TitleKind {
    /// A timeline.
    Timeline(TimelineId),
    /// A slideshow (its id).
    Slideshow(u64),
    /// A chapter playlist (its id).
    ChapterPlaylist(u64),
}

/// What title `title` (from 1) plays: the titles are the timelines, then the slideshows, then the chapter
/// playlists, each in id order (as a build numbers them).
pub(crate) fn title_kind(project: &Project, title: u8) -> Option<TitleKind> {
    if let Some(id) = title_timeline(project, title) {
        return Some(TitleKind::Timeline(id));
    }
    let index = usize::from(title)
        .checked_sub(1)?
        .checked_sub(project.timelines.len())?;
    let slideshows = slideshow_ids(project);
    if let Some(id) = slideshows.get(index) {
        return Some(TitleKind::Slideshow(*id));
    }
    project
        .chapter_playlists
        .keys()
        .nth(index.checked_sub(slideshows.len())?)
        .map(|id| TitleKind::ChapterPlaylist(*id))
}

/// The most entries a Random playlist plays in the preview before its title ends (the disc plays on until stopped).
const RANDOM_PLAYS: usize = 99;

/// The order the entries `numbers` of a Random or Shuffle playlist play in the preview, from entry `first`:
/// Shuffle plays every entry once (the rest in a random order), Random a random entry each time (up to
/// [`RANDOM_PLAYS`]). `seed` makes the order.
fn play_order(
    order: disccraft_project::PlaybackOrder,
    numbers: &[u16],
    first: u16,
    seed: u64,
) -> Vec<u16> {
    let mut state = seed | 1;
    let mut next = |below: usize| -> usize {
        // xorshift64: enough to vary the order between previews.
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        usize::try_from(state % u64::try_from(below.max(1)).unwrap_or(1)).unwrap_or(0)
    };
    let mut played = vec![first];
    match order {
        disccraft_project::PlaybackOrder::Random => {
            while played.len() < RANDOM_PLAYS && !numbers.is_empty() {
                if let Some(number) = numbers.get(next(numbers.len())) {
                    played.push(*number);
                }
            }
        }
        _ => {
            let mut rest: Vec<u16> = numbers
                .iter()
                .copied()
                .filter(|number| *number != first)
                .collect();
            // Fisher-Yates.
            for index in (1..rest.len()).rev() {
                rest.swap(index, next(index + 1));
            }
            played.extend(rest);
        }
    }
    played
}

/// A seed for [`play_order`] from the clock.
fn order_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |since| {
            u64::try_from(since.as_nanos() % u128::from(u64::MAX)).unwrap_or(1)
        })
}

/// The stretch of `timeline` each entry of a chapter playlist plays (`chapters`: chapter numbers from 0): from
/// the chapter's point to the next chapter's (the last chapter: to the end, as infinity). Entries that name no
/// chapter are left out.
pub(crate) fn playlist_segments(timeline: &Timeline, chapters: &[u32]) -> Vec<(f64, f64)> {
    let point = |index: usize| {
        timeline
            .chapters
            .get(index)
            .map(|chapter| crate::timeline_view::ticks_to_seconds(chapter.time.0).max(0.0))
    };
    chapters
        .iter()
        .filter_map(|chapter| {
            let index = usize::try_from(*chapter).ok()?;
            let start = point(index)?;
            let end = index
                .checked_add(1)
                .and_then(point)
                .unwrap_or(f64::INFINITY);
            Some((start, end))
        })
        .collect()
}

/// When each slide of durations `seconds` starts and ends in the show.
pub(crate) fn slide_times(seconds: &[f64]) -> Vec<(f64, f64)> {
    let mut at = 0.0;
    seconds
        .iter()
        .map(|seconds| {
            let start = at;
            at += seconds.max(0.0);
            (start, at)
        })
        .collect()
}

/// The slide on screen at `position`: the last that has started (the first before the show, the last after).
pub(crate) fn slide_at(times: &[(f64, f64)], position: f64) -> usize {
    times
        .iter()
        .rposition(|(start, _)| *start <= position)
        .unwrap_or(0)
}

/// Where a slideshow stands: the slide shown, the time of the show, and whether it waits on a slide.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ShowPlace {
    /// The slide shown (0 the first).
    pub(crate) index: usize,
    /// The time of the show, in seconds.
    pub(crate) position: f64,
    /// The clock was held back to the end of the slide the VM is on (it stays up until Next).
    pub(crate) held_back: bool,
    /// The slide shown stays up past its end (Manual Advance) and has reached it: the show waits, silent, as the
    /// disc's still cell does once its pictures and sound have played. Next then goes on from where the sound
    /// stopped, not from a part already heard.
    pub(crate) waiting: bool,
}

/// Where a show whose slides span `times` (and stay up, `held`) stands at `position` seconds, with the VM on
/// slide `on` (0 the first): a held slide holds the clock at its end.
pub(crate) fn show_place(
    times: &[(f64, f64)],
    held: &[bool],
    on: usize,
    position: f64,
) -> ShowPlace {
    let is_held = |index: usize| held.get(index).copied().unwrap_or(false);
    let mut place = ShowPlace {
        index: slide_at(times, position),
        position,
        held_back: false,
        waiting: false,
    };
    if place.index > on
        && is_held(on)
        && let Some((_, end)) = times.get(on)
    {
        place = ShowPlace {
            index: on,
            position: *end,
            held_back: true,
            waiting: false,
        };
    }
    place.waiting = is_held(place.index)
        && times
            .get(place.index)
            .is_some_and(|(_, end)| place.position >= *end);
    place
}

/// The audio clip that sounds at `position` of a show whose clips last `lengths` seconds, and how far into it;
/// `None` after the last clip (unless they loop). A clip of unknown length is taken to last to the end.
pub(crate) fn sound_at(
    lengths: &[Option<f64>],
    position: f64,
    looped: bool,
) -> Option<(usize, f64)> {
    let total: Option<f64> = lengths.iter().copied().sum();
    let mut position = position.max(0.0);
    if let Some(total) = total.filter(|total| looped && *total > 0.0) {
        position %= total;
    }
    for (index, length) in lengths.iter().enumerate() {
        match length {
            Some(length) if position >= *length => position -= length,
            _ => return Some((index, position)),
        }
    }
    None
}

/// モニターに映すプレビューの映像: フレーム、16:9 か、重ねる字幕。
pub(crate) struct PreviewPicture {
    pub(crate) texture: egui::TextureHandle,
    pub(crate) wide: bool,
    /// The picture is fitted into the frame in its own shape (a slide), not spread over it.
    pub(crate) fitted: bool,
    /// A slide's pan and zoom and fill at this moment.
    pub(crate) view: SlideView,
    /// The neighbouring slide while a transition runs.
    pub(crate) blend: Option<SlideBlend>,
    /// The subtitle on screen and the colour group it is drawn in.
    pub(crate) subtitle: Option<crate::subtitles::ShownSubtitle>,
}

/// How a slide is shown at one moment of the preview: its pan and zoom (scale, and the centre of the view in
/// frame widths and heights from the frame's centre), whether it fills the frame (Scale and Crop), and the shape of
/// its pixels when they are not square (width over height: Interpret Footage, or a DVD-sized still's SD pixels).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SlideView {
    pub(crate) motion: Option<(f32, f32, f32)>,
    pub(crate) crop: bool,
    pub(crate) pixel: Option<f32>,
}

/// A transition between the slide on screen and its neighbour at one moment: which, how far through (0 to 1), the
/// neighbour's picture and how it is shown, and whether the slide on screen is the one coming in.
#[derive(Clone)]
pub(crate) struct SlideBlend {
    pub(crate) kind: TransitionKind,
    pub(crate) alpha: f32,
    pub(crate) texture: egui::TextureHandle,
    pub(crate) view: SlideView,
    pub(crate) current_is_to: bool,
}

/// Where a transition is at one moment of the show, as the build makes it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CutAt {
    kind: TransitionKind,
    /// How far through (0 to 1).
    alpha: f32,
    /// The neighbouring slide, and the point of it shown (0 its first frame, 1 its last).
    other: usize,
    other_t: f32,
    /// Whether the slide on screen is the one coming in.
    current_is_to: bool,
}

/// The transition at `position` (seconds of the show) on slide `index`, as the build makes it: centred on the
/// cut, half before and half after, never more than half of either slide nor 10 seconds in all (`times`: each
/// slide's start and end; `cuts`: the transition before each slide and its length). The slide going out shows
/// its last moment after the cut, the one coming in its first before it.
fn transition_at(
    times: &[(f64, f64)],
    cuts: &[(TransitionKind, f64)],
    index: usize,
    position: f64,
) -> Option<CutAt> {
    let length = |slide: usize| {
        times
            .get(slide)
            .map_or(0.0, |(start, end)| (end - start).max(0.0))
    };
    // The kind and half-length of the cut before slide `slide`.
    let half = |slide: usize| -> Option<(TransitionKind, f64)> {
        let previous = slide.checked_sub(1)?;
        let (kind, seconds) = cuts.get(slide).copied()?;
        let half = (seconds.min(10.0) / 2.0)
            .min(length(previous) / 2.0)
            .min(length(slide) / 2.0);
        (kind != TransitionKind::None && half > 0.0).then_some((kind, half))
    };
    let (start, end) = times.get(index).copied()?;
    // `as` is exact enough for a share of a transition.
    if let Some((kind, half)) = half(index)
        && position < start + half
    {
        return Some(CutAt {
            kind,
            alpha: ((position - (start - half)) / (2.0 * half)).clamp(0.0, 1.0) as f32,
            other: index.checked_sub(1)?,
            other_t: 1.0,
            current_is_to: true,
        });
    }
    let next = index.checked_add(1)?;
    let (kind, half) = half(next)?;
    (position >= end - half).then(|| CutAt {
        kind,
        alpha: ((position - (end - half)) / (2.0 * half)).clamp(0.0, 1.0) as f32,
        other: next,
        other_t: 0.0,
        current_is_to: false,
    })
}

/// Draws a slideshow's picture for one moment: the slide (see [`draw_slide`]), or while a transition runs the two
/// slides as the build mixes them (a cross dissolve, a dip to black, or a wipe in from the left).
pub(crate) fn draw_slides(
    painter: &egui::Painter,
    frame: egui::Rect,
    (texture, view): (&egui::TextureHandle, SlideView),
    blend: Option<&SlideBlend>,
) {
    let whole = (egui::Color32::WHITE, frame);
    let Some(blend) = blend else {
        draw_slide(painter, frame, texture, view, whole);
        return;
    };
    let (from, to) = if blend.current_is_to {
        ((&blend.texture, blend.view), (texture, view))
    } else {
        ((texture, view), (&blend.texture, blend.view))
    };
    let alpha = blend.alpha.clamp(0.0, 1.0);
    // `as` saturates; the shades are bytes.
    let byte = |amount: f32| (amount.clamp(0.0, 1.0) * 255.0).round() as u8;
    match blend.kind {
        TransitionKind::None => draw_slide(painter, frame, to.0, to.1, whole),
        TransitionKind::CrossDissolve => {
            draw_slide(painter, frame, from.0, from.1, whole);
            let tint = egui::Color32::from_white_alpha(byte(alpha));
            draw_slide(painter, frame, to.0, to.1, (tint, frame));
        }
        TransitionKind::DipToBlack => {
            if alpha < 0.5 {
                let shade = egui::Color32::from_gray(byte(1.0 - alpha * 2.0));
                draw_slide(painter, frame, from.0, from.1, (shade, frame));
            } else {
                let shade = egui::Color32::from_gray(byte(alpha * 2.0 - 1.0));
                draw_slide(painter, frame, to.0, to.1, (shade, frame));
            }
        }
        TransitionKind::Wipe => {
            draw_slide(painter, frame, from.0, from.1, whole);
            let edge = frame.min.x + frame.width() * alpha;
            let wiped = egui::Rect::from_min_max(frame.min, egui::pos2(edge, frame.max.y));
            draw_slide(painter, frame, to.0, to.1, (egui::Color32::WHITE, wiped));
        }
    }
}

/// Where a slide's picture of `size` is drawn for `frame` (see [`draw_slide`]); `None` for an empty picture.
fn slide_rect(frame: egui::Rect, size: egui::Vec2, view: SlideView) -> Option<egui::Rect> {
    if size.x <= 0.0 || size.y <= 0.0 {
        return None;
    }
    // Its pixels as wide as they are shown on the disc.
    let pixel = view
        .pixel
        .filter(|pixel| pixel.is_finite() && *pixel > 0.0)
        .unwrap_or(1.0);
    let size = egui::vec2(size.x * pixel, size.y);
    let (across, down) = (frame.width() / size.x, frame.height() / size.y);
    let fit = if view.crop {
        across.max(down)
    } else {
        across.min(down)
    };
    let mut shown = egui::Rect::from_center_size(frame.center(), size * fit);
    if let Some((scale, x, y)) = view.motion.filter(|(scale, _, _)| *scale > 0.0) {
        let centre = frame.center() + egui::vec2(x * frame.width(), y * frame.height());
        let moved = |point: egui::Pos2| frame.center() + (point - centre) * scale;
        shown = egui::Rect::from_min_max(moved(shown.min), moved(shown.max));
    }
    Some(shown)
}

/// Draws a slide's picture into `frame` as the disc shows it: fitted in the frame with its own shape (or filling
/// it, Scale and Crop), then moved by its pan and zoom (the frame shows `1 / scale` of it around the view's
/// centre), cut to the frame.
pub(crate) fn draw_slide(
    painter: &egui::Painter,
    frame: egui::Rect,
    texture: &egui::TextureHandle,
    view: SlideView,
    (tint, clip): (egui::Color32, egui::Rect),
) {
    let Some(shown) = slide_rect(frame, texture.size_vec2(), view) else {
        return;
    };
    painter
        .with_clip_rect(frame.intersect(clip).intersect(painter.clip_rect()))
        .image(
            texture.id(),
            shown,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            tint,
        );
}

/// スライドショーの id（題番号の順: ビルドと同じく id の順）。
/// The longest edge of a slide's picture in the preview, pixels.
const SLIDE_EDGE: u32 = 720;

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

/// M13.19: the parental alternative that plays in place of chapter `chapter` (from 1) of `timeline` in its version
/// `variant` ([`Timeline::parental_variants`], the one the player plays: the preview state's), when that version
/// replaces the chapter.
pub(crate) fn parental_alternative(
    timeline: &Timeline,
    variant: Option<usize>,
    chapter: u16,
) -> Option<AssetId> {
    let variants = timeline.parental_variants();
    let version = variants.get(variant?)?;
    let index = usize::from(chapter).checked_sub(1)?;
    timeline
        .parental_chapters
        .iter()
        .zip(&version.replaced)
        .find(|(parental, replaced)| **replaced && parental.chapter == index)
        .map(|(parental, _)| parental.alternative)
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
        .map(|asset| asset.media_file().into_owned())
}

/// Where the sound of [`audio_source`] starts on the timeline, in frames of the disc's standard rate (an audio
/// clip moved along its track; 0 for the video file's own sound).
pub(crate) fn audio_start(timeline: &Timeline, stream: Option<u8>) -> u32 {
    stream
        .and_then(|stream| usize::from(stream).checked_sub(1))
        .and_then(|index| timeline.audio.get(index))
        .or_else(|| timeline.audio.first())
        .map_or(0, |track| track.start)
}

/// The part of its file the sound of [`audio_source`] plays, in frames of the disc's standard rate: what its in
/// point leaves out, and where its out point is (`None`: the file's end). An untrimmed clip, and the video
/// file's own sound, are `(0, None)`.
pub(crate) fn audio_trim(timeline: &Timeline, stream: Option<u8>) -> (u32, Option<u32>) {
    stream
        .and_then(|stream| usize::from(stream).checked_sub(1))
        .and_then(|index| timeline.audio.get(index))
        .or_else(|| timeline.audio.first())
        .map_or((0, None), |track| (track.trim_start, track.trim_end))
}

/// The audio stream (from 1) a player that prefers `language` takes of `timeline`: its first track in that
/// language. `None` without a language, a track in it, or more than one track to choose from.
pub(crate) fn preferred_track(timeline: &Timeline, language: &str) -> Option<u8> {
    if language.is_empty() || timeline.audio.len() < 2 {
        return None;
    }
    timeline
        .audio
        .iter()
        .position(|track| track.language.eq_ignore_ascii_case(language))
        .and_then(|index| u8::try_from(index.saturating_add(1)).ok())
}

/// The subtitle stream (from 1) a player whose subtitle language is `language` shows on `timeline`: its first
/// subtitle track in that language; `None` when there is none or no language is set.
pub(crate) fn preferred_subtitle(timeline: &Timeline, language: &str) -> Option<u8> {
    if language.is_empty() {
        return None;
    }
    timeline
        .subtitles
        .iter()
        .position(|track| track.language.eq_ignore_ascii_case(language))
        .and_then(|index| u8::try_from(index.saturating_add(1)).ok())
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

        if self
            .preview_playback
            .as_ref()
            .is_some_and(|playback| playback.slides.is_some())
        {
            self.poll_preview_slides(ctx);
            return;
        }
        let paused = self.preview_paused;
        // The length of the timeline that plays: the entry of its last chapter ends there.
        let total = self
            .preview_playback
            .as_ref()
            .and_then(|playback| playback.timeline)
            .map(|id| self.timeline_seconds(id));
        let half_frame = 0.5 / self.preview_fps().max(1.0);
        // A timeline's title that has played into its next chapter: the VM is told, so the chapter shown and
        // what Next and Previous go to follow the picture (M6.272).
        let passed = self.preview_playback.as_ref().and_then(|playback| {
            let timeline = self.session.project.timeline(playback.timeline?)?;
            let position = playback.frame.as_ref()?.1;
            if !playback.segments.is_empty() || playback.slides.is_some() {
                return None;
            }
            let chapters = u16::try_from(timeline.chapters.len()).unwrap_or(u16::MAX);
            let now = (playback.at.1..=chapters)
                .rev()
                .find(|chapter| chapter_start(timeline, *chapter) <= position + half_frame)?;
            (now > playback.at.1).then_some((playback.at.0, now))
        });
        // A chapter the player's version replaces with its parental alternative (M13.19): the playback starts
        // again there, with the alternative.
        let into_alternative = passed.is_some_and(|(_, chapter)| {
            self.preview_playback
                .as_ref()
                .and_then(|playback| self.session.project.timeline(playback.timeline?))
                .and_then(|timeline| parental_alternative(timeline, self.preview.variant, chapter))
                .is_some()
        });
        if let Some((title, chapter)) = passed
            && !paused
        {
            self.preview_chapter(chapter);
            if self.preview.title == Some((title, chapter))
                && !into_alternative
                && let Some(playback) = self.preview_playback.as_mut()
            {
                playback.at = (title, chapter);
            }
        }
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
            // The time on the timeline: the clip's file plays from the clip's in point.
            playback.frame = Some((texture, frame.seconds + playback.clip.0));
        }
        // A parental alternative that has played to its end (M13.19): the title goes on at the next chapter (the
        // VM is told, and the playback starts there), or after the last chapter ends.
        if let Some((chapter, id)) = playback.alternative
            && matches!(player.status(), PlayerStatus::Ended)
        {
            let chapters = self.session.project.timeline(id).map_or(0, |timeline| {
                u16::try_from(timeline.chapters.len()).unwrap_or(u16::MAX)
            });
            self.preview_playback = None;
            if chapter < chapters {
                self.preview_chapter(chapter.saturating_add(1));
            } else {
                self.send_remote(RemoteKey::Finish);
            }
            ctx.request_repaint();
            return;
        }
        // A chapter playlist: the entry that was playing has reached its end — its next chapter, or for the
        // last chapter (when more entries follow, as in a Random or Shuffle playlist) the timeline's end.
        let more = playback.segments.len() > 1;
        let file_ended = matches!(player.status(), PlayerStatus::Ended);
        let entry_ended = match (playback.segments.first(), &playback.frame) {
            (Some((_, end)), Some((_, seconds))) => {
                *seconds + half_frame >= *end
                    || (more
                        && end.is_infinite()
                        && (file_ended
                            || total.is_some_and(|total| *seconds + 3.0 * half_frame >= total)))
            }
            _ => false,
        };
        if entry_ended {
            let (_, ended) = playback.segments.remove(0);
            if !playback.entries.is_empty() {
                let _ = playback.entries.remove(0);
            }
            // The entry that plays next (a Random / Shuffle playlist's from its order).
            let next_entry = playback.entries.first().copied();
            let at = playback.at;
            if let Some((title, order)) = self.preview_order.as_mut()
                && *title == at.0
                && !order.is_empty()
            {
                let _ = order.remove(0);
            }
            match playback.segments.first().copied() {
                // The last entry: the title's end.
                None => {
                    self.preview_playback = None;
                    self.send_remote(RemoteKey::Finish);
                }
                Some((start, _)) => {
                    // The VM goes to the next entry. When that is the chapter that follows on the timeline,
                    // the playback runs on; else it starts again from the entry's chapter.
                    let runs_on = (start - ended).abs() <= 2.0 * half_frame;
                    let next = next_entry.unwrap_or_else(|| at.1.saturating_add(1));
                    self.preview_chapter(next);
                    if runs_on
                        && self.preview.title == Some((at.0, next))
                        && let Some(playback) = self.preview_playback.as_mut()
                    {
                        playback.at = (at.0, next);
                    }
                }
            }
            ctx.request_repaint();
            return;
        }
        // The video clip that was playing has reached its out point (its file may go on), or its file's end.
        let position = playback.frame.as_ref().map(|(_, seconds)| *seconds);
        let clip_end = playback.clip.1;
        let clip_ended = match (clip_end, position) {
            (Some(end), Some(position)) => position + half_frame >= end,
            _ => false,
        };
        let status = player.status();
        if clip_ended || matches!(status, PlayerStatus::Ended) {
            // Another clip follows on the timeline: the playback goes on from there (a gap is skipped, as
            // in the Monitor).
            let id = playback.timeline;
            let next = clip_end.or(position);
            let total = id.map_or(0.0, |id| self.timeline_seconds(id));
            match next {
                Some(next) if next < total - 2.0 * half_frame => {
                    self.preview_playback = None;
                    // A little past the join, so the clip found there is the next one.
                    self.preview_resume = Some(next + 0.1 * half_frame);
                }
                // タイトルの終わり: VM がエンドアクションへ進む（同じタイトルに戻れば、また頭から再生する）。
                _ => {
                    self.preview_playback = None;
                    self.send_remote(RemoteKey::Finish);
                }
            }
            ctx.request_repaint();
            return;
        }
        match status {
            PlayerStatus::Playing => ctx.request_repaint(),
            PlayerStatus::Ended => {}
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
            segments: Vec::new(),
            entries: Vec::new(),
            slides: None,
            view: SlideView::default(),
            blend: None,
            clip: (0.0, None),
            alternative: None,
        };
        let (id, segments) = match title_kind(&self.session.project, title) {
            Some(TitleKind::Timeline(id)) => {
                // M13.19: the player's version of the title plays this chapter's parental alternative.
                if let Some(asset) = self.session.project.timeline(id).and_then(|timeline| {
                    parental_alternative(timeline, self.preview.variant, chapter)
                }) {
                    return self.start_preview_alternative(playback, (id, asset), resume);
                }
                (id, Vec::new())
            }
            Some(TitleKind::ChapterPlaylist(list)) => {
                let project = &self.session.project;
                let Some(list) = project.chapter_playlists.get(&list) else {
                    return playback;
                };
                let Some(timeline) = project.timeline(list.timeline) else {
                    return playback;
                };
                // Each entry's number and the stretch it plays (entries that name no chapter are left out).
                let all: Vec<(u16, (f64, f64))> = list
                    .chapters
                    .iter()
                    .enumerate()
                    .filter_map(|(index, chapter)| {
                        let number = u16::try_from(index.checked_add(1)?).ok()?;
                        Some((number, *playlist_segments(timeline, &[*chapter]).first()?))
                    })
                    .collect();
                let numbers: Vec<u16> = all.iter().map(|(number, _)| *number).collect();
                // From the entry the VM is on: in the listed order, or (Random / Shuffle) in an order made when
                // the title starts and kept while it plays, so a jump between entries goes on with it.
                let order = match list.order {
                    disccraft_project::PlaybackOrder::Sequential => numbers
                        .iter()
                        .copied()
                        .filter(|number| *number >= chapter)
                        .collect(),
                    other => self
                        .preview_order
                        .as_ref()
                        .filter(|(kept, order)| *kept == title && order.first() == Some(&chapter))
                        .map(|(_, order)| order.clone())
                        .unwrap_or_else(|| play_order(other, &numbers, chapter, order_seed())),
                };
                self.preview_order = (list.order != disccraft_project::PlaybackOrder::Sequential)
                    .then(|| (title, order.clone()));
                let segments: Vec<(f64, f64)> = order
                    .iter()
                    .filter_map(|number| {
                        all.iter()
                            .find(|(entry, _)| entry == number)
                            .map(|(_, stretch)| *stretch)
                    })
                    .collect();
                if segments.is_empty() {
                    return playback;
                }
                playback.entries = order;
                (list.timeline, segments)
            }
            Some(TitleKind::Slideshow(show)) => {
                return self.start_preview_slides(playback, show, chapter, resume);
            }
            None => return playback,
        };
        // A player with a language of its own takes the first track in it, when the disc chose none.
        if self.preview.audio.is_none()
            && let Some(track) =
                self.session.project.timeline(id).and_then(|timeline| {
                    preferred_track(timeline, &self.preview_prefs.audio_language)
                })
            && let Some(state) =
                self.run_query("preview.streams", serde_json::json!({ "audio": track }))
        {
            self.preview = crate::monitor::parse_preview_state(&state);
            playback.streams = (self.preview.audio, self.preview.subtitle);
        }
        // A player with a subtitle language of its own shows the first track in it, when no subtitles are shown.
        if self.preview.subtitle.is_none()
            && let Some(track) = self.session.project.timeline(id).and_then(|timeline| {
                preferred_subtitle(timeline, &self.preview_prefs.subtitle_language)
            })
            && let Some(state) =
                self.run_query("preview.streams", serde_json::json!({ "subtitle": track }))
        {
            self.preview = crate::monitor::parse_preview_state(&state);
            playback.streams = (self.preview.audio, self.preview.subtitle);
        }
        // The timeline's video clips (several, trimmed or moved: each plays its stretch of its own file).
        let clips = self.timeline_clips(id);
        let project = &self.session.project;
        let Some(timeline) = project.timeline(id) else {
            return playback;
        };
        // A chapter playlist's entry starts at its chapter of the master timeline.
        let from = resume.unwrap_or_else(|| {
            segments
                .first()
                .map_or_else(|| chapter_start(timeline, chapter), |(start, _)| *start)
        });
        // The clip that plays at that time, and the position in its file (in a gap: the clip after it).
        let clip = crate::playback::clip_at(&clips, from);
        let Some(video) = project
            .assets
            .get(&clip.map_or(timeline.video, |(clip, _)| clip.asset))
            .map(|asset| asset.media_file().into_owned())
        else {
            return playback;
        };
        // Where the player starts in the file, and how a time in the file becomes a time on the timeline.
        let (file_from, offset, clip_end) = match clip {
            Some((clip, local)) => (
                local,
                clip.start - clip.source,
                (clip.duration > 0.0).then_some(clip.start + clip.duration),
            ),
            None => (from, 0.0, None),
        };
        playback.clip = (offset, clip_end);
        // Past the clip's start when the time asked for was in a gap in front of it.
        let from = file_from + offset;
        let has_track = !timeline.audio.is_empty();
        playback.segments = segments;
        let audio_path = audio_source(project, timeline, self.preview.audio);
        // An audio clip moved along its track starts later: the sound is that far behind the picture.
        let (rate_num, rate_den) = crate::timeline_view::frame_rate(project.settings.standard);
        let frame_seconds = f64::from(rate_den) / f64::from(rate_num.max(1));
        let audio_start = f64::from(audio_start(timeline, self.preview.audio)) * frame_seconds;
        // A trimmed clip plays a part of its file.
        let (trim_start, trim_end) = audio_trim(timeline, self.preview.audio);
        let trim_start = f64::from(trim_start) * frame_seconds;
        let trim_end = trim_end.map(|end| f64::from(end) * frame_seconds);
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
        // A track of several clips plays as parts, from where the playback starts.
        let chosen = self
            .preview
            .audio
            .and_then(|stream| usize::from(stream).checked_sub(1))
            .and_then(|index| timeline.audio.get(index))
            .or_else(|| timeline.audio.first());
        let parts = chosen
            .map(|track| disccraft_engine::audio_track_parts(project, track, frame_seconds, from))
            .unwrap_or_default();
        // The track's sound stream of its file (M6.418).
        let stream = chosen.map_or(0, |track| track.stream as usize);
        let player_audio = match (&device, audio_path) {
            (Some(device), Some(path)) => Some(PlayerAudio {
                stream,
                parts,
                path,
                rate: device.rate(),
                // An audio track runs along the whole timeline; the video file's own sound is at the picture's
                // place in the clip's file.
                from: has_track.then_some(from - audio_start + trim_start),
                until: trim_end,
            }),
            _ => None,
        };
        // The picture as the disc shows it, in square pixels: the HD frame, or the SD frame at its display aspect
        // (16:9 or 4:3), so the screen does not stretch it.
        let size = hd.map_or_else(
            || sd_size(playback.wide, project.settings.standard),
            |(width, height, _, _)| (width, height),
        );
        // Preferences ▸ Preview ▸ Preview playback quality (M6.341).
        let size = self.preview_prefs.quality().size(size);
        let player = Player::start_sized(video, player_audio, file_from, fps, Some(size));
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

    /// M13.19: plays the parental alternative `asset` of the chapter `playback` is at, in timeline `id`'s title, as
    /// the disc's version for the player's level plays it: the alternative's own video and sound, from its start (or
    /// `resume`, seconds into it), at the title's picture shape. The title's subtitles are not shown over it (the
    /// build gives the alternative none).
    fn start_preview_alternative(
        &mut self,
        mut playback: PreviewPlayback,
        (id, asset): (TimelineId, AssetId),
        resume: Option<f64>,
    ) -> PreviewPlayback {
        playback.alternative = Some((playback.at.1, id));
        let project = &self.session.project;
        let (Some(timeline), Some(video)) = (
            project.timeline(id),
            project
                .assets
                .get(&asset)
                .map(|asset| asset.media_file().into_owned()),
        ) else {
            return playback;
        };
        playback.wide = timeline.wide(&project.settings);
        let hd = disc_hd(&project.settings);
        if hd.is_some() {
            playback.wide = true;
        }
        let fps = hd.map_or_else(
            || self.preview_fps(),
            |(_, _, num, den)| f64::from(num) / f64::from(den.max(1)),
        );
        let size = hd.map_or_else(
            || sd_size(playback.wide, project.settings.standard),
            |(width, height, _, _)| (width, height),
        );
        let size = self.preview_prefs.quality().size(size);
        let device = match crate::audio_out::AudioDevice::open_default() {
            Ok(device) => Some(device),
            Err(message) => {
                self.status = tlf!("Playing without sound: {message}", message = message);
                None
            }
        };
        // Its own sound, at the picture's place in its file.
        let player_audio = device.as_ref().map(|device| PlayerAudio {
            stream: 0,
            parts: Vec::new(),
            path: video.clone(),
            rate: device.rate(),
            from: None,
            until: None,
        });
        let player = Player::start_sized(
            video,
            player_audio,
            resume.unwrap_or(0.0).max(0.0),
            fps,
            Some(size),
        );
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
        let fitted = playback.slides.is_some();
        let view = playback.view;
        let blend = playback.blend.clone();
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
                crate::subtitles::cue_at(&cues, seconds).map(|cue| {
                    crate::subtitles::ShownSubtitle {
                        text: cue.text.clone(),
                        colors: self.cue_colors(id, track, cue.look),
                        look: cue.look,
                        image: cue.image.clone(),
                    }
                })
            }
            // A slideshow's subtitle (Create Subtitle on the slide): its name and description, in the show's colour
            // group, stroke and place, while the preview shows subtitles.
            _ => self.slide_subtitle(),
        };
        // The title a preview plays goes to the video output too (Preferences ▸ Audio/Video Output).
        self.video_out_frame = Some((
            texture.clone(),
            if wide { 16.0 / 9.0 } else { 4.0 / 3.0 },
            fitted,
        ));
        Some(PreviewPicture {
            texture,
            wide,
            fitted,
            view,
            blend,
            subtitle,
        })
    }

    /// The subtitle of the slide on screen, when the preview shows subtitles and the slide makes one (Encore's
    /// Create Subtitle): as the build draws it.
    fn slide_subtitle(&self) -> Option<crate::subtitles::ShownSubtitle> {
        self.preview.subtitle?;
        let play = self.preview_playback.as_ref()?.slides.as_ref()?;
        let project = &self.session.project;
        let show = project.slideshow(play.show)?;
        let slide = show.slides.get(play.index)?;
        let text = project.slide_subtitle_text(slide);
        if text.is_empty() {
            return None;
        }
        Some(crate::subtitles::ShownSubtitle {
            text,
            colors: show
                .subtitle_colors
                .unwrap_or_default()
                .group(slide.subtitle_group),
            look: disccraft_project::CueLook {
                group: None,
                stroke: Some(slide.subtitle_stroke),
                align: Some(slide.subtitle_align),
                justify: None,
            },
            image: None,
        })
    }

    /// Tells the VM the title is at `chapter` now (an entry or a slide ended and the next began).
    fn preview_chapter(&mut self, chapter: u16) {
        if let Some(state) =
            self.run_query("preview.chapter", serde_json::json!({ "chapter": chapter }))
        {
            self.preview = crate::monitor::parse_preview_state(&state);
        }
    }

    /// Starts showing slideshow `show` from slide `chapter` (from 1), or from `resume` seconds of the show.
    fn start_preview_slides(
        &mut self,
        mut playback: PreviewPlayback,
        show: u64,
        chapter: u16,
        resume: Option<f64>,
    ) -> PreviewPlayback {
        let Some(found) = self.session.project.slideshow(show).cloned() else {
            return playback;
        };
        if found.slides.is_empty() {
            return playback;
        }
        // The audio clips, with their lengths where they are known already.
        let clips: Vec<(AssetId, PathBuf)> = found
            .audio_list()
            .into_iter()
            .filter_map(|id| {
                let asset = self.session.project.assets.get(&id)?;
                Some((id, asset.media_file().into_owned()))
            })
            .collect();
        let clips: Vec<(PathBuf, Option<f64>)> = clips
            .into_iter()
            .map(|(id, path)| {
                let seconds = self.asset_seconds(id).filter(|seconds| *seconds > 0.0);
                (path, seconds)
            })
            .collect();
        // The slides' own durations, or (Fit Slideshow to Audio Duration) stretched to the sound's length.
        let mut seconds: Vec<f64> = found
            .slides
            .iter()
            .map(|slide| crate::timeline_view::ticks_to_seconds(slide.duration.0).max(0.0))
            .collect();
        let own: f64 = seconds.iter().sum();
        let sound: Option<f64> = clips.iter().map(|(_, length)| *length).sum();
        if let Some(sound) = sound.filter(|sound| found.fit_to_audio && *sound > 0.0 && own > 0.0) {
            for slide in &mut seconds {
                *slide *= sound / own;
            }
        }
        let held = found.held_slides();
        let slides: Vec<(AssetId, f64, f64, bool)> = found
            .slides
            .iter()
            .zip(slide_times(&seconds))
            .enumerate()
            .map(|(index, (slide, (start, end)))| {
                (
                    slide.asset,
                    start,
                    end,
                    held.get(index).copied().unwrap_or(false),
                )
            })
            .collect();
        let first = usize::from(chapter)
            .saturating_sub(1)
            .min(slides.len().saturating_sub(1));
        let from = resume
            .or_else(|| slides.get(first).map(|(_, start, _, _)| *start))
            .unwrap_or(0.0);
        let settings = &self.session.project.settings;
        playback.wide = settings.widescreen || disc_hd(settings).is_some();
        // As the build moves and places each slide.
        let looks = (0..found.slides.len())
            .map(|index| {
                let crop = found.slides.get(index).is_some_and(|slide| {
                    slide.scale == disccraft_project::SlideScale::ScaleAndCrop
                });
                let pixel = found
                    .slides
                    .get(index)
                    .and_then(|slide| self.slide_pixel(slide.asset));
                (
                    found.slide_effect(index).map(disccraft_build::slide_motion),
                    crop,
                    pixel,
                )
            })
            .collect();
        // The transition at each cut, as the build makes it.
        let cuts = found
            .slides
            .iter()
            .map(|slide| {
                let (kind, length) = slide
                    .transition
                    .map_or((found.transition, found.transition_duration), |own| {
                        (own.kind, own.duration)
                    });
                (
                    kind,
                    crate::timeline_view::ticks_to_seconds(length.0).max(0.0),
                )
            })
            .collect();
        playback.slides = Some(SlidePlay {
            slides,
            looks,
            cuts,
            show,
            index: first,
            offset: from,
            began: Some(Instant::now()),
            silent: clips.is_empty(),
            clips,
            loop_audio: found.loop_audio,
            sound: None,
        });
        playback
    }

    /// The shape of slide asset `asset`'s pixels as the build draws them (width over height), when not square: the
    /// one chosen for it, else the picture's own (a DVD-sized still has SD pixels).
    fn slide_pixel(&mut self, asset: AssetId) -> Option<f32> {
        let found =
            self.run_query_quiet("asset.pixelAspect", serde_json::json!({ "asset": asset.0 }))?;
        let pair = |value: &serde_json::Value| {
            let width = value.get(0)?.as_u64()?;
            let height = value.get(1)?.as_u64()?;
            // `as` is exact enough for a pixel's shape.
            (width > 0 && height > 0 && width != height).then(|| width as f32 / height as f32)
        };
        let chosen = found.get("chosen").filter(|value| !value.is_null());
        match chosen {
            Some(value) => pair(value),
            None => found.get("file").and_then(pair),
        }
    }

    /// One audio clip of a slideshow, playing from `from` seconds into it.
    fn start_slide_sound(
        &mut self,
        path: PathBuf,
        from: f64,
    ) -> Option<(Player, Option<crate::audio_out::AudioOut>)> {
        let device = match crate::audio_out::AudioDevice::open_default() {
            Ok(device) => device,
            Err(message) => {
                self.status = tlf!("Playing without sound: {message}", message = message);
                return None;
            }
        };
        let audio = PlayerAudio {
            stream: 0,
            parts: Vec::new(),
            path: path.clone(),
            rate: device.rate(),
            from: None,
            until: None,
        };
        // An audio file has no picture: the player plays its sound alone, to its end.
        let player = Player::start_sized(path, Some(audio), from, self.preview_fps(), None);
        let out = player.audio().and_then(|feed| match device.play(feed) {
            Ok(out) => Some(out),
            Err(message) => {
                self.status = tlf!("Playing without sound: {message}", message = message);
                None
            }
        });
        Some((player, out))
    }

    /// The picture of slide asset `asset` for the preview: larger than the project's thumbnails, made once.
    fn slide_picture(
        &mut self,
        ctx: &egui::Context,
        asset: AssetId,
    ) -> Option<egui::TextureHandle> {
        if let Some(known) = self.preview_slides.get(&asset.0) {
            return known.clone();
        }
        let value = self.run_query_quiet(
            "asset.thumbnail",
            serde_json::json!({ "asset": asset.0, "edge": SLIDE_EDGE, "wait": false }),
        );
        if crate::project_panel::is_pending(value.as_ref()) {
            self.media_pending = true;
            return None;
        }
        let texture = value.as_ref().and_then(|value| {
            crate::project_panel::decode_rgb_texture(
                ctx,
                &format!("preview_slide_{}", asset.0),
                value,
            )
        });
        let _ = self.preview_slides.insert(asset.0, texture.clone());
        texture
    }

    /// Called as the window is drawn while a slideshow's title is shown: the slide for the time, its sound, the
    /// next slide's chapter to the VM, and the title's end.
    fn poll_preview_slides(&mut self, ctx: &egui::Context) {
        let Some(mut playback) = self.preview_playback.take() else {
            return;
        };
        let Some(mut play) = playback.slides.take() else {
            self.preview_playback = Some(playback);
            return;
        };
        // Paused: the clock stops where it is and the sound with it.
        if self.preview_paused {
            if let Some(began) = play.began.take() {
                play.offset += began.elapsed().as_secs_f64();
            }
            play.sound = None;
            playback.slides = Some(play);
            self.preview_playback = Some(playback);
            return;
        }
        let began = *play.began.get_or_insert_with(Instant::now);
        let times: Vec<(f64, f64)> = play
            .slides
            .iter()
            .map(|(_, start, end, _)| (*start, *end))
            .collect();
        let held: Vec<bool> = play.slides.iter().map(|(.., held)| *held).collect();
        // The slide the VM is on; one that stays up holds the clock at its end.
        let on = usize::from(playback.at.1).saturating_sub(1);
        let place = show_place(
            &times,
            &held,
            on,
            play.offset + began.elapsed().as_secs_f64(),
        );
        let (index, position) = (place.index, place.position);
        if place.held_back {
            play.offset = position;
            play.began = Some(Instant::now());
        }
        // Waiting on a slide for Next: silent, as on the disc (the sound starts again from here with Next).
        if place.waiting {
            play.sound = None;
        }
        let last = play.slides.len().saturating_sub(1);
        let finished = index >= last
            && play
                .slides
                .get(last)
                .is_some_and(|(_, _, end, held)| !held && position >= *end);
        play.index = index;
        if let Some((asset, start, end, _)) = play.slides.get(index).copied() {
            let texture = self
                .slide_picture(ctx, asset)
                .or_else(|| playback.frame.as_ref().map(|(texture, _)| texture.clone()));
            playback.frame = texture.map(|texture| (texture, position));
            // Its pan and zoom at this point of the slide, as the build moves it.
            let (motion, crop, pixel) = play
                .looks
                .get(index)
                .copied()
                .unwrap_or((None, false, None));
            let t = if end > start {
                ((position - start) / (end - start)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            // `as` is exact enough for a share of a slide's time.
            playback.view = SlideView {
                motion: motion.map(|motion| motion.at(t as f32)),
                crop,
                pixel,
            };
            // A transition at the cut before or after it: the neighbour as the build shows it there.
            playback.blend = transition_at(&times, &play.cuts, index, position).and_then(|cut| {
                let (asset, ..) = play.slides.get(cut.other).copied()?;
                let texture = self.slide_picture(ctx, asset)?;
                let (motion, crop, pixel) = play
                    .looks
                    .get(cut.other)
                    .copied()
                    .unwrap_or((None, false, None));
                Some(SlideBlend {
                    kind: cut.kind,
                    alpha: cut.alpha,
                    texture,
                    view: SlideView {
                        motion: motion.map(|motion| motion.at(cut.other_t)),
                        crop,
                        pixel,
                    },
                    current_is_to: cut.current_is_to,
                })
            });
        }
        // The sound: the clip for the time when none plays yet, the next one when a clip has ended.
        let ended = play
            .sound
            .as_ref()
            .is_some_and(|(_, player, _)| !matches!(player.status(), PlayerStatus::Playing));
        if !play.silent && !finished && !place.waiting && (play.sound.is_none() || ended) {
            let next = match play.sound.take() {
                Some((clip, ..)) => {
                    let next = clip.saturating_add(1);
                    if next < play.clips.len() {
                        Some((next, 0.0))
                    } else if play.loop_audio {
                        Some((0, 0.0))
                    } else {
                        None
                    }
                }
                None => {
                    let lengths: Vec<Option<f64>> =
                        play.clips.iter().map(|(_, length)| *length).collect();
                    sound_at(&lengths, position, play.loop_audio)
                }
            };
            play.sound = next
                .and_then(|(clip, from)| Some((clip, play.clips.get(clip)?.0.clone(), from)))
                .and_then(|(clip, path, from)| {
                    let (player, out) = self.start_slide_sound(path, from)?;
                    Some((clip, player, out))
                });
            play.silent = play.sound.is_none();
        }
        let at = playback.at;
        playback.slides = Some(play);
        self.preview_playback = Some(playback);
        if finished {
            self.preview_playback = None;
            self.send_remote(RemoteKey::Finish);
        } else if index > on {
            // The next slide came up by itself: the VM follows, and the show runs on.
            let chapter = u16::try_from(index.saturating_add(1)).unwrap_or(u16::MAX);
            self.preview_chapter(chapter);
            if self.preview.title == Some((at.0, chapter))
                && let Some(playback) = self.preview_playback.as_mut()
            {
                playback.at = (at.0, chapter);
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }

    /// テレビ方式のフレームレート（フレーム / 秒）。
    pub(crate) fn preview_fps(&self) -> f64 {
        let (num, den) = crate::timeline_view::frame_rate(self.session.project.settings.standard);
        f64::from(num) / f64::from(den.max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ShowPlace, TitleKind, audio_source, chapter_start, parental_alternative, playlist_segments,
        preferred_track, show_place, slide_at, slide_times, sound_at, subtitle_track, title_kind,
        title_timeline,
    };
    use disccraft_project::{
        Asset, AssetId, AssetKind, AudioTrack, Chapter, Project, SubtitleTrack, Tick, Timeline,
        TimelineId,
    };
    use std::path::PathBuf;

    fn timeline(id: u64, video: u64) -> Timeline {
        Timeline {
            parental_level: None,
            parental_alternative: disccraft_project::Link::NotSet,
            parental_chapters: Vec::new(),
            id: TimelineId(id),
            name: format!("T{id}"),
            video: AssetId(video),
            more_video: Vec::new(),
            video_gaps: Vec::new(),
            subtitle_colors: None,
            video_trims: Vec::new(),
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

    /// M13.19: the version the player plays replaces the chapters whose level is above its range: with chapter 2
    /// for level 5 (alternative 8) and chapter 3 for level 3 (alternative 9), the versions are levels 5–8 (none
    /// replaced), 3–4 (chapter 2) and 1–2 (chapters 2 and 3).
    #[test]
    fn a_version_plays_the_alternatives_of_the_chapters_it_replaces() {
        let mut timeline = timeline(1, 1);
        timeline.chapters = vec![chapter(0), chapter(10), chapter(20)];
        timeline.parental_chapters = vec![
            disccraft_project::ParentalChapter {
                chapter: 1,
                level: 5,
                alternative: AssetId(8),
            },
            disccraft_project::ParentalChapter {
                chapter: 2,
                level: 3,
                alternative: AssetId(9),
            },
        ];
        let played = |variant: Option<usize>| -> Vec<Option<AssetId>> {
            (1..=3)
                .map(|chapter| parental_alternative(&timeline, variant, chapter))
                .collect()
        };
        assert_eq!(played(None), [None, None, None]);
        assert_eq!(played(Some(0)), [None, None, None]);
        assert_eq!(played(Some(1)), [None, Some(AssetId(8)), None]);
        assert_eq!(played(Some(2)), [None, Some(AssetId(8)), Some(AssetId(9))]);
        assert_eq!(played(Some(3)), [None, None, None]);
        assert_eq!(parental_alternative(&timeline, Some(1), 0), None);
    }

    fn asset(id: u64, path: &str) -> Asset {
        Asset {
            id: AssetId(id),
            name: path.to_owned(),
            path: path.to_owned(),
            kind: AssetKind::Video,
            offline: false,
            cues: None,
            composition: None,
            description: String::new(),
            transcode_kbps: None,
            pixel_aspect: None,
            located_dvd: None,
            located_bluray: None,
            ignore_alpha: false,
            transcode_mode: Default::default(),
            transcode_audio: Default::default(),
            transcode_preset: None,
            bluray_transcode: None,
            transcode_film: false,
            bluray_video: None,
            bluray_codec: None,
            transcode_quality: disccraft_project::BEST_QUALITY,
            max_render_quality: false,
            frame_blend: false,
            menu_file: false,
        }
    }

    fn chapter(ticks: i64) -> Chapter {
        Chapter {
            time: Tick(ticks),
            name: String::new(),
            poster: None,
        }
    }

    /// M6.253: the titles after the timelines are the slideshows, then the chapter playlists.
    #[test]
    fn a_title_is_a_timeline_a_slideshow_or_a_chapter_playlist()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let video = project.add_asset(std::path::Path::new("movie.m2v"))?;
        let timeline = project.add_timeline(video, None)?;
        let show = project.add_slideshow(None)?;
        let list = project.add_chapter_playlist(timeline, None)?;
        assert_eq!(title_kind(&project, 1), Some(TitleKind::Timeline(timeline)));
        assert_eq!(title_kind(&project, 2), Some(TitleKind::Slideshow(show)));
        assert_eq!(
            title_kind(&project, 3),
            Some(TitleKind::ChapterPlaylist(list))
        );
        assert_eq!(title_kind(&project, 4), None);
        assert_eq!(title_kind(&project, 0), None);
        Ok(())
    }

    /// A chapter playlist's entries are stretches of its master timeline: each chapter to the next one's point,
    /// the last chapter to the end.
    #[test]
    fn a_chapter_playlist_plays_its_chapters_stretches() {
        let second = 254_016_000_000_i64;
        let mut master = timeline(1, 1);
        master.chapters = vec![chapter(0), chapter(10 * second), chapter(25 * second)];
        // Chapters 3, 1, 2, and one the timeline does not have.
        let segments = playlist_segments(&master, &[2, 0, 1, 7]);
        assert_eq!(
            segments,
            vec![(25.0, f64::INFINITY), (0.0, 10.0), (10.0, 25.0)]
        );
        assert!(playlist_segments(&master, &[]).is_empty());
    }

    /// The slide for a time of the show, and the audio clip for it.
    #[test]
    fn a_slideshow_shows_the_slide_and_plays_the_clip_for_the_time() {
        let times = slide_times(&[5.0, 3.0, 4.0]);
        assert_eq!(times, vec![(0.0, 5.0), (5.0, 8.0), (8.0, 12.0)]);
        assert_eq!(slide_at(&times, 0.0), 0);
        assert_eq!(slide_at(&times, 4.99), 0);
        assert_eq!(slide_at(&times, 5.0), 1);
        assert_eq!(slide_at(&times, 11.0), 2);
        assert_eq!(slide_at(&times, 99.0), 2);
        assert_eq!(slide_at(&times, -1.0), 0);
        assert_eq!(slide_at(&[], 1.0), 0);

        // Two clips of 4 and 6 seconds.
        let clips = [Some(4.0), Some(6.0)];
        assert_eq!(sound_at(&clips, 0.0, false), Some((0, 0.0)));
        assert_eq!(sound_at(&clips, 5.5, false), Some((1, 1.5)));
        assert_eq!(sound_at(&clips, 10.0, false), None);
        // Looping: they start over.
        assert_eq!(sound_at(&clips, 10.0, true), Some((0, 0.0)));
        assert_eq!(sound_at(&clips, 25.0, true), Some((1, 1.0)));
        // A clip of unknown length is taken to last.
        assert_eq!(sound_at(&[Some(4.0), None], 30.0, true), Some((1, 26.0)));
        assert_eq!(sound_at(&[], 1.0, true), None);
    }

    /// M6.312: a slide that stays up (Manual Advance) holds the show at its end and waits there silent, as the
    /// disc's still cell does; Next goes on from there, so the sound picks up where it stopped instead of playing
    /// the slide's part again.
    #[test]
    fn a_held_slide_waits_silent_and_next_goes_on_from_its_end() {
        let times = slide_times(&[5.0, 3.0, 4.0]);
        let held = [false, true, false];
        // Slide 2 plays as any other.
        let place = show_place(&times, &held, 1, 6.0);
        assert_eq!(
            (place.index, place.held_back, place.waiting),
            (1, false, false)
        );
        // Its end reached (the clock ran on): held back to it, waiting.
        let place = show_place(&times, &held, 1, 9.5);
        assert_eq!(
            place,
            ShowPlace {
                index: 1,
                position: 8.0,
                held_back: true,
                waiting: true
            }
        );
        // Next: the VM is on slide 3, which starts at that end, and the sound goes on from there.
        let place = show_place(&times, &held, 2, 8.0);
        assert_eq!((place.index, place.waiting), (2, false));
        assert_eq!(
            sound_at(&[Some(4.0), Some(6.0)], place.position, false),
            Some((1, 4.0))
        );
        // A slide that does not stay up runs on into the next.
        let place = show_place(&times, &[false; 3], 0, 5.5);
        assert_eq!(
            (place.index, place.held_back, place.waiting),
            (1, false, false)
        );
        // The last slide, held: it waits past its end.
        let place = show_place(&times, &[false, false, true], 2, 30.0);
        assert_eq!((place.index, place.waiting), (2, true));
        assert!(!show_place(&times, &[false, false, true], 2, 11.0).waiting);
        // No slides at all.
        assert!(!show_place(&[], &[], 0, 1.0).waiting);
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
                more: Vec::new(),
                asset: AssetId(2),
                stream: 0,
                language: "ja".to_owned(),
                start: 0,
                trim_start: 0,
                trim_end: None,
            },
            AudioTrack {
                more: Vec::new(),
                asset: AssetId(3),
                stream: 0,
                language: "en".to_owned(),
                start: 0,
                trim_start: 0,
                trim_end: None,
            },
        ];
        // M6.257: a player that prefers a language takes the first track in it; without one (or a track in
        // it), the disc's first.
        assert_eq!(preferred_track(&movie, "en"), Some(2));
        assert_eq!(preferred_track(&movie, "EN"), Some(2));
        assert_eq!(preferred_track(&movie, "ja"), Some(1));
        assert_eq!(preferred_track(&movie, "fr"), None);
        assert_eq!(preferred_track(&movie, ""), None);
        // M6.293: a player's subtitle language shows its first subtitle track in it.
        movie.subtitles = ["en", "ja"]
            .iter()
            .map(|language| disccraft_project::SubtitleTrack {
                asset: AssetId(9),
                language: (*language).to_owned(),
                group: 0,
            })
            .collect();
        assert_eq!(super::preferred_subtitle(&movie, "JA"), Some(2));
        assert_eq!(super::preferred_subtitle(&movie, "en"), Some(1));
        assert_eq!(super::preferred_subtitle(&movie, "fr"), None);
        assert_eq!(super::preferred_subtitle(&movie, ""), None);
        assert_eq!(
            audio_source(&project, &movie, Some(2)),
            Some(PathBuf::from("en.ac3"))
        );
        // A moved clip's start goes with the track that is heard.
        if let Some(track) = movie.audio.get_mut(1) {
            track.start = 45;
        }
        assert_eq!(super::audio_start(&movie, Some(2)), 45);
        assert_eq!(super::audio_start(&movie, Some(1)), 0);
        assert_eq!(super::audio_start(&movie, None), 0);
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
                group: 0,
            },
            SubtitleTrack {
                asset: AssetId(6),
                language: "en".to_owned(),
                group: 0,
            },
        ];
        assert_eq!(subtitle_track(&movie, Some(2)), Some(1));
        assert_eq!(subtitle_track(&movie, None), None);
        assert_eq!(subtitle_track(&movie, Some(3)), None);
    }
}

#[cfg(test)]
mod slide_view_tests {
    use super::{SlideView, slide_rect};
    use disccraft_project::{SlideEffect, SlidePan, SlideZoom};

    fn close(a: egui::Rect, b: [f32; 4]) -> bool {
        [a.min.x, a.min.y, a.max.x, a.max.y]
            .iter()
            .zip(b)
            .all(|(got, want)| (got - want).abs() < 0.5)
    }

    /// The preview places a slide as the build moves it: fitted (or filling the frame), then grown about the
    /// view's centre; a pan to the right starts with the picture's right edge on the frame's and ends with its
    /// left edge on the frame's left.
    #[test]
    fn slides_are_placed_as_the_build_moves_them() {
        let frame = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 300.0));
        let wide = egui::vec2(800.0, 400.0);
        let still = SlideView::default();
        assert!(close(
            slide_rect(frame, wide, still).unwrap_or(frame),
            [0.0, 50.0, 400.0, 250.0]
        ));
        let crop = SlideView {
            crop: true,
            ..still
        };
        assert!(close(
            slide_rect(frame, wide, crop).unwrap_or(frame),
            [-100.0, 0.0, 500.0, 300.0]
        ));
        let pan = disccraft_build::slide_motion(SlideEffect {
            pan: SlidePan::Right,
            zoom: SlideZoom::None,
        });
        let full = egui::vec2(400.0, 300.0);
        let at = |t: f32| {
            slide_rect(
                frame,
                full,
                SlideView {
                    motion: Some(pan.at(t)),
                    crop: false,
                    pixel: None,
                },
            )
            .unwrap_or(frame)
        };
        assert!((at(0.0).max.x - 400.0).abs() < 0.5, "{:?}", at(0.0));
        assert!((at(1.0).min.x - 0.0).abs() < 0.5, "{:?}", at(1.0));
        // Grown all the way through, centred up and down.
        assert!((at(0.5).height() - 300.0 * 1.12).abs() < 0.5);
        assert!((at(0.5).center().y - 150.0).abs() < 0.5);
        assert!(slide_rect(frame, egui::Vec2::ZERO, still).is_none());
        // M6.321: a 720 x 480 still of SD pixels (8:9) fills a 4:3 frame, as on the disc; of square ones it is
        // letterboxed.
        let sd = egui::vec2(720.0, 480.0);
        let shaped = SlideView {
            pixel: Some(8.0 / 9.0),
            ..still
        };
        assert!(close(
            slide_rect(frame, sd, shaped).unwrap_or(frame),
            [0.0, 0.0, 400.0, 300.0]
        ));
        let square = slide_rect(frame, sd, still).unwrap_or(frame);
        assert!((square.height() - 400.0 / 1.5).abs() < 0.5, "{square:?}");
    }
}

#[cfg(test)]
mod transition_tests {
    use super::transition_at;
    use disccraft_project::TransitionKind;

    /// As the build: a 2-second cross dissolve between 6-second slides runs from 5 to 7 seconds, half on each
    /// side of the cut; it is cut to half the shorter slide; a hard cut and the ends of the show have none.
    #[test]
    fn transitions_run_across_the_cut() {
        let times = [(0.0, 6.0), (6.0, 12.0), (12.0, 13.0)];
        let cuts = [
            (TransitionKind::None, 0.0),
            (TransitionKind::CrossDissolve, 2.0),
            (TransitionKind::Wipe, 4.0),
        ];
        assert_eq!(transition_at(&times, &cuts, 0, 4.9), None);
        let going = transition_at(&times, &cuts, 0, 5.5).unwrap_or_else(|| panic!("no cut"));
        assert_eq!((going.other, going.current_is_to), (1, false));
        assert!((going.alpha - 0.25).abs() < 1e-4);
        assert!((going.other_t - 0.0).abs() < 1e-6);
        let coming = transition_at(&times, &cuts, 1, 6.5).unwrap_or_else(|| panic!("no cut"));
        assert_eq!((coming.other, coming.current_is_to), (0, true));
        assert!((coming.alpha - 0.75).abs() < 1e-4);
        assert!((coming.other_t - 1.0).abs() < 1e-6);
        assert_eq!(transition_at(&times, &cuts, 1, 7.0), None);
        // The 4-second wipe into a 1-second slide is cut to half a second each side.
        assert_eq!(transition_at(&times, &cuts, 1, 11.4), None);
        let wipe = transition_at(&times, &cuts, 1, 11.75).unwrap_or_else(|| panic!("no cut"));
        assert_eq!(wipe.kind, TransitionKind::Wipe);
        assert!((wipe.alpha - 0.25).abs() < 1e-4);
        let none = [(TransitionKind::None, 0.0), (TransitionKind::None, 2.0)];
        assert_eq!(transition_at(&times[..2], &none, 0, 5.9), None);
    }
}

#[cfg(test)]
mod order_tests {
    use super::{RANDOM_PLAYS, play_order};
    use disccraft_project::PlaybackOrder;

    /// Shuffle plays every entry once from the one asked for; Random keeps picking entries of the list; the
    /// seed changes the order.
    #[test]
    fn random_and_shuffle_orders() {
        let numbers = [1, 2, 3, 4, 5];
        let shuffled = play_order(PlaybackOrder::Shuffle, &numbers, 3, 42);
        assert_eq!(shuffled.first(), Some(&3));
        let mut sorted = shuffled.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, numbers.to_vec());
        let random = play_order(PlaybackOrder::Random, &numbers, 2, 7);
        assert_eq!(random.len(), RANDOM_PLAYS);
        assert_eq!(random.first(), Some(&2));
        assert!(random.iter().all(|number| numbers.contains(number)));
        let orders: std::collections::BTreeSet<Vec<u16>> = (1..20)
            .map(|seed| play_order(PlaybackOrder::Shuffle, &numbers, 1, seed))
            .collect();
        assert!(orders.len() > 1, "{orders:?}");
    }
}
