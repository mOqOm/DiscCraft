use crate::app::DiscCraftApp;
use crate::craft_timeline;
use crate::i18n::{tl, tlf};
use crate::menubar::ViewRequest;
use disccraft_project::{FrameRate, TimelineId, VideoStandard, format_frames};
use egui::{Color32, Id, Rect, Response, Stroke, Ui};
use serde_json::{Value, json};

/// Pixels per second at zoom 1.0.
const BASE_SCALE: f32 = 8.0;
/// Ticks per second used by the project timeline.
const TICKS_PER_SECOND: i64 = 254_016_000_000;
/// Height of the timeline canvas in points.
const CANVAS_HEIGHT: f32 = 150.0;
/// Marker hit tolerance in points.
const HIT_TOLERANCE: f32 = 6.0;

/// Converts a time in seconds to an x coordinate on the timeline canvas.
pub(crate) fn time_to_x(seconds: f64, origin_x: f32, scale: f32) -> f32 {
    let seconds = if seconds.is_finite() && seconds > 0.0 {
        seconds
    } else {
        0.0
    };
    let px = f64::from(origin_x) + seconds * f64::from(BASE_SCALE) * f64::from(scale);

    if !px.is_finite() {
        return origin_x;
    }
    if px > f64::from(f32::MAX) {
        return f32::MAX;
    }
    if px < f64::from(f32::MIN) {
        return f32::MIN;
    }
    px as f32
}

/// Converts an x coordinate on the timeline canvas to a non-negative time in seconds.
pub(crate) fn x_to_time(x: f32, origin_x: f32, scale: f32) -> f64 {
    let px_per_second = f64::from(BASE_SCALE) * f64::from(scale);
    if !px_per_second.is_finite() || px_per_second <= 0.0 {
        return 0.0;
    }

    let seconds = (f64::from(x) - f64::from(origin_x)) / px_per_second;
    if seconds.is_finite() && seconds > 0.0 {
        seconds
    } else {
        0.0
    }
}

/// Returns the frame rate as frames per second, or zero when invalid.
fn frames_per_second(fps_num: u32, fps_den: u32) -> f64 {
    if fps_num == 0 || fps_den == 0 {
        return 0.0;
    }

    let fps = f64::from(fps_num) / f64::from(fps_den);
    if fps.is_finite() && fps > 0.0 {
        fps
    } else {
        0.0
    }
}

/// Returns the index of the frame shown at `seconds` (0 for a bad time or rate).
fn frame_index(seconds: f64, fps: f64) -> i64 {
    if fps <= 0.0 || !seconds.is_finite() || seconds <= 0.0 {
        return 0;
    }

    // The nearest frame, like the monitor (a 6.0 s clip at 29.97 lasts 180 frames, "00;00;06;00"). `as` saturates.
    (seconds * fps).round() as i64
}

/// The project's frame rate as a FilmCraft rate.
fn rate_of(fps_num: u32, fps_den: u32) -> FrameRate {
    FrameRate::new(i64::from(fps_num), i64::from(fps_den)).sane()
}

/// Formats seconds as a timecode like Encore: NTSC as drop-frame "HH;MM;SS;FF", PAL as "HH:MM:SS:FF".
pub(crate) fn timecode(seconds: f64, fps_num: u32, fps_den: u32) -> String {
    let fps = frames_per_second(fps_num, fps_den);
    format_frames(frame_index(seconds, fps), rate_of(fps_num, fps_den))
}

/// Returns the index of the first marker within tolerance of x.
pub(crate) fn hit_marker(markers_x: &[f32], x: f32, tolerance: f32) -> Option<usize> {
    let tolerance = if tolerance.is_finite() && tolerance > 0.0 {
        tolerance
    } else {
        0.0
    };

    markers_x
        .iter()
        .enumerate()
        .find(|(_, marker_x)| (*marker_x - x).abs() <= tolerance)
        .map(|(index, _)| index)
}

/// Converts project ticks to seconds.
pub(crate) fn ticks_to_seconds(ticks: i64) -> f64 {
    ticks as f64 / TICKS_PER_SECOND as f64
}

/// Converts seconds to project ticks with rounding.
pub(crate) fn seconds_to_ticks(seconds: f64) -> i64 {
    if !seconds.is_finite() {
        return 0;
    }

    // `as` from f64 saturates at the i64 range.
    (seconds * TICKS_PER_SECOND as f64).round() as i64
}

/// Returns the project frame rate as numerator and denominator.
pub(crate) fn frame_rate(standard: VideoStandard) -> (u32, u32) {
    match standard {
        VideoStandard::Ntsc => (30000, 1001),
        VideoStandard::Pal => (25, 1),
    }
}

/// Builds an egui temp-memory key for timeline state.
fn timeline_key(name: &str, id: u64) -> Id {
    Id::unique(name).with(id)
}

/// Returns a fallback duration when a probe is unavailable.
fn fallback_duration(chapters: &[ChapterSnapshot]) -> f64 {
    let last = chapters
        .iter()
        .map(|chapter| chapter.time)
        .fold(0.0_f64, f64::max);

    if !last.is_finite() {
        return 60.0;
    }
    (last + 30.0).max(60.0)
}

/// Returns the time to draw for a marker, using drag state when active.
fn marker_time(drag: Option<MarkerDrag>, index: usize, chapter_time: f64) -> f64 {
    match drag {
        Some(drag) if drag.marker == index => drag.time,
        _ => chapter_time,
    }
}

/// Cached result of probing a timeline's video asset.
#[derive(Clone)]
struct DurationProbe {
    /// The video clips that were probed (the first clip and the ones after it).
    videos: Vec<u64>,
    /// Whether the probe has already been attempted.
    done: bool,
    /// Probed duration in seconds, or zero when unknown.
    seconds: f64,
    /// Where each clip after the first starts, in seconds.
    joins: Vec<f64>,
    /// Where every clip plays, as (start, end) in seconds (empty: one clip over the whole duration).
    spans: Vec<(f64, f64)>,
    /// Where each of those clips starts in its file, in seconds (its trimmed in point).
    sources: Vec<f64>,
}

/// Drag state for a chapter marker.
#[derive(Clone, Copy)]
struct MarkerDrag {
    /// Index of the dragged marker.
    marker: usize,
    /// Current dragged time in seconds.
    time: f64,
}

/// Owned chapter data used by the timeline view.
struct ChapterSnapshot {
    /// Chapter time in seconds.
    time: f64,
    /// Chapter name.
    name: String,
    /// Poster frame in seconds, when one is set.
    poster: Option<f64>,
}

/// One audio or subtitle track row under the video.
struct TrackRow {
    /// Track header, e.g. "Audio 1:".
    label: String,
    /// Index among the tracks of its kind.
    index: usize,
    /// ISO 639-1 language ("" when not set).
    language: String,
    /// Clip name drawn in the bar.
    clip: String,
    /// Subtitle track (drawn in a different colour).
    subtitle: bool,
    /// The cues of a subtitle track, drawn as blocks at their times.
    cues: Option<std::sync::Arc<Vec<crate::subtitles::SubtitleCue>>>,
    /// The track's asset (its waveform is looked up by it).
    asset: disccraft_project::AssetId,
    /// An audio track's waveform peaks, once worked out.
    peaks: Option<crate::waveforms::Peaks>,
    /// Where an audio track's clip starts on the timeline, seconds.
    start: f64,
    /// How long an audio track's clip is, seconds (`None` until its file has been read: drawn to the end).
    length: Option<f64>,
    /// How much of its file's head an audio track's clip leaves out, seconds (its in point trimmed).
    trim_start: f64,
    /// The clips of an audio track after its first (M6.269); the fields above are the first clip's.
    more: Vec<MoreClip>,
}

/// A clip of an audio track after its first.
struct MoreClip {
    /// Its number on the track (1 the second clip).
    number: usize,
    /// The name drawn in it.
    name: String,
    /// Its sound file.
    asset: disccraft_project::AssetId,
    /// Where it starts on the timeline, seconds.
    start: f64,
    /// How long it is, seconds (`None` until its file has been read).
    length: Option<f64>,
    /// How much of its file's head it leaves out, seconds.
    trim_start: f64,
    /// Its file's waveform peaks, once worked out.
    peaks: Option<crate::waveforms::Peaks>,
}

/// One clip of an audio track as it lies on the track: its number, where it starts and ends (seconds), how much
/// of its file's head it leaves out, its name and its file.
struct ClipBox<'a> {
    number: usize,
    start: f64,
    end: f64,
    trim_start: f64,
    name: &'a str,
    /// Its file's waveform peaks, when worked out.
    peaks: Option<&'a crate::waveforms::Peaks>,
}

/// The clips of an audio track's row, the first clip first (a clip whose length is not known runs to the end of
/// the timeline).
fn clip_boxes(track: &TrackRow, duration: f64) -> Vec<ClipBox<'_>> {
    let span = |start: f64, length: Option<f64>| {
        let end = length.map_or(duration.max(start), |length| start + length);
        (start, end.max(start))
    };
    let (start, end) = span(track.start, track.length);
    std::iter::once(ClipBox {
        number: 0,
        start,
        end,
        trim_start: track.trim_start,
        name: &track.clip,
        peaks: track.peaks.as_ref(),
    })
    .chain(track.more.iter().map(|clip| {
        let (start, end) = span(clip.start, clip.length);
        ClipBox {
            number: clip.number,
            start,
            end,
            trim_start: clip.trim_start,
            name: &clip.name,
            peaks: clip.peaks.as_ref(),
        }
    }))
    .collect()
}

/// Owned snapshot of timeline data used for drawing and interaction.
struct TimelineSnapshot {
    /// Timeline id as u64.
    id: u64,
    /// Video asset id.
    video: u64,
    /// Video asset display name.
    asset_name: String,
    /// The video clips after the first (Encore: more videos butted on to the end): asset id and name.
    more_video: Vec<(u64, String)>,
    /// Where each of those clips starts, in seconds (filled in with the duration).
    joins: Vec<f64>,
    /// Where every clip plays, as (start, end) in seconds (filled in with the duration).
    spans: Vec<(f64, f64)>,
    /// Where each clip starts in its file, in seconds (filled in with the duration; empty for one whole clip).
    sources: Vec<f64>,
    /// The empty stretches in front of the clips, in frames (so a moved clip is measured again).
    gaps: Vec<u64>,
    /// The clips' trims as numbers (start, end + 1 or 0), so a changed trim is measured again.
    trims: Vec<u64>,
    /// Chapter snapshots.
    chapters: Vec<ChapterSnapshot>,
    /// Audio and subtitle tracks, in that order.
    tracks: Vec<TrackRow>,
    /// Numerator of the project frame rate.
    fps_num: u32,
    /// Denominator of the project frame rate.
    fps_den: u32,
}

/// Mutable interaction state stored in egui temp memory.
#[derive(Clone, Copy)]
struct TimelineInteraction {
    /// Playhead time in seconds.
    playhead: f64,
    /// Selected chapter index, if any.
    selected: Option<usize>,
    /// Active marker drag, if any.
    drag: Option<MarkerDrag>,
}

/// Data needed to draw one timeline frame.
struct TimelineFrame<'a> {
    /// Timeline snapshot.
    snapshot: &'a TimelineSnapshot,
    /// The visible canvas (ruler and rows).
    canvas: Rect,
    /// Where time 0 is (left of the canvas when the view is scrolled).
    origin_x: f32,
    /// Timeline duration in seconds.
    duration: f64,
    /// Zoom factor: points a second over [`BASE_SCALE`].
    zoom: f32,
    /// Playhead time in seconds.
    playhead: f64,
    /// Selected chapter index.
    selected: Option<usize>,
    /// Active marker drag.
    drag: Option<MarkerDrag>,
    /// The video clip's last frame (drawn at the end of the video bar).
    last: Option<egui::TextureHandle>,
    /// The pictures along each video clip (Encore): per clip, where each starts in the timeline (the clip's head and
    /// its chapter points) and its picture when it is ready.
    pictures: Vec<Vec<(f64, Option<egui::TextureHandle>)>>,
    /// Whether chapter names are drawn next to the chapter points (View ▸ Show Chapter Names).
    show_names: bool,
    /// A video clip being dragged along the track: the clip and how far, in seconds.
    clip_move: Option<(usize, f64)>,
    /// An audio track's clip being dragged along its track: the track and how far, in seconds.
    audio_move: Option<(usize, usize, f64)>,
    /// A subtitle clip being dragged (moved or trimmed): its subtitle track, its number, and where it would be
    /// (start, end in seconds).
    cue_move: Option<(usize, usize, f64, f64)>,
    /// The clip chosen in this timeline (outlined; Edit ▸ Clear and Ripple Delete act on it).
    picked: Option<ClipPick>,
}

/// A clip chosen in the timeline viewer by clicking it (Encore: a selected clip, which Edit ▸ Clear and Ripple
/// Delete act on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClipPick {
    /// A video clip (0 the first).
    Video(usize),
    /// A clip of an audio track: the track and the clip's number on it (0 the first).
    Audio(usize, usize),
}

/// Where a subtitle clip is held to be dragged in the timeline: its body (moved along the track) or its in or out
/// edge (trimmed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CueGrip {
    /// Moved along the track.
    Body,
    /// Its in point.
    In,
    /// Its out point.
    Out,
}

impl CueGrip {
    fn code(self) -> u8 {
        match self {
            CueGrip::Body => 0,
            CueGrip::In => 1,
            CueGrip::Out => 2,
        }
    }

    fn of(code: u8) -> Self {
        match code {
            1 => CueGrip::In,
            2 => CueGrip::Out,
            _ => CueGrip::Body,
        }
    }
}

/// Where subtitle clip `index` of `cues` (each `(start, end)` in seconds, in order) goes when held by `grip` and
/// dragged `shift` seconds: by whole `frame`s, snapped to the nearest of `snap`'s times within its reach (seconds)
/// when snapping is on, kept off its neighbours and at least `shortest` seconds long. `None` for no such clip.
pub(crate) fn dragged_cue(
    cues: &[(f64, f64)],
    index: usize,
    grip: CueGrip,
    shift: f64,
    (frame, shortest): (f64, f64),
    snap: Option<(&[f64], f64)>,
) -> Option<(f64, f64)> {
    let (start, end) = *cues.get(index)?;
    let before = index
        .checked_sub(1)
        .and_then(|previous| cues.get(previous))
        .map_or(0.0, |previous| previous.1)
        .max(0.0);
    let after = cues
        .get(index.saturating_add(1))
        .map_or(f64::INFINITY, |next| next.0);
    let steps = if frame > 0.0 && frame.is_finite() {
        (shift / frame).round() * frame
    } else {
        shift
    };
    // The nearest snap point to any of `edges`, as how far to go to it.
    let pull = |edges: &[f64]| -> f64 {
        snap.and_then(|(times, reach)| {
            times
                .iter()
                .flat_map(|time| edges.iter().map(move |edge| time - edge))
                .filter(|by| by.abs() <= reach)
                .min_by(|a, b| a.abs().total_cmp(&b.abs()))
        })
        .unwrap_or(0.0)
    };
    let length = end - start;
    match grip {
        CueGrip::Body => {
            let moved = start + steps;
            let moved = moved + pull(&[moved, moved + length]);
            let latest = (after - length).max(before);
            Some((
                moved.max(before).min(latest),
                moved.max(before).min(latest) + length,
            ))
        }
        CueGrip::In => {
            let moved = start + steps;
            let moved = moved + pull(&[moved]);
            // A clip already shorter than the shortest is not shortened further.
            let latest = (end - shortest).max(start).max(before);
            Some((moved.max(before).min(latest), end))
        }
        CueGrip::Out => {
            let moved = end + steps;
            let moved = moved + pull(&[moved]);
            let earliest = (start + shortest).min(end);
            Some((start, moved.max(earliest).min(after.max(earliest))))
        }
    }
}

/// Timeline view methods for the desktop application.
impl DiscCraftApp {
    /// Draws the timeline viewer for one title timeline: a header column (the big current time, the toolbar, the track
    /// names and languages) beside the ruler and the tracks, and under them the timeline switcher and the Craft
    /// apps' zoom scroll bar (ROADMAP M9.8).
    /// A Project panel row dragged over the timeline viewer: outlines the viewer, and on release adds the item as a
    /// track ([`DiscCraftApp::drop_on_timeline`]).
    fn accept_timeline_drop(&mut self, ui: &Ui, id: TimelineId, area: Rect) {
        let Some(payload) = egui::DragAndDrop::payload::<crate::project_panel::DragRow>(ui.ctx())
        else {
            return;
        };
        if !ui
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|at| area.contains(at))
        {
            return;
        }
        let _ = ui.painter().rect_stroke(
            area.shrink(2.0),
            4.0,
            egui::Stroke::new(2.0, egui::Color32::from_rgb(0x4a, 0x90, 0xff)),
            egui::StrokeKind::Inside,
        );
        if !ui.input(|i| i.pointer.any_released()) {
            return;
        }
        let _ = egui::DragAndDrop::take_payload::<crate::project_panel::DragRow>(ui.ctx());
        // A sound file dropped on an audio track's row becomes another clip of that track at the time under the
        // pointer (Encore); anywhere else the item is added as a track, as before.
        let target = ui.data_mut(|data| data.remove_temp::<(usize, f64)>(drop_target_key(id.0)));
        let sound = self
            .session
            .project
            .assets
            .get(&disccraft_project::AssetId(payload.0))
            .is_some_and(|asset| asset.kind == disccraft_project::AssetKind::Audio);
        if let (Some((track, time)), true) = (target, sound) {
            if self
                .run_command(
                    "timeline.addAudioClip",
                    json!({
                        "timeline": id.0,
                        "track": track,
                        "asset": payload.0,
                        "time": seconds_to_ticks(time),
                    }),
                )
                .is_some()
            {
                self.status = tl!("Added an audio clip").to_string();
            }
            return;
        }
        self.drop_on_timeline(id, payload.0);
    }

    pub(crate) fn timeline_view(&mut self, ui: &mut Ui, id: TimelineId) {
        let area = ui.max_rect();
        // 字幕トラックの字幕を読んでおく（スナップショットに入れて、トラックの上に並べる）。
        self.load_timeline_cues(id);
        let Some(mut snapshot) = self.timeline_snapshot(id) else {
            ui.label(tl!("No timeline selected"));
            return;
        };
        // The audio clips' waveforms (worked out once per asset, on a thread of their own): every clip's file,
        // the second and later clips' too.
        for track in snapshot.tracks.iter_mut().filter(|track| !track.subtitle) {
            let file = |app: &Self, asset: disccraft_project::AssetId| {
                app.session
                    .project
                    .assets
                    .get(&asset)
                    .map(|found| found.media_file().into_owned())
            };
            if let Some(path) = file(self, track.asset) {
                track.peaks = self.waveforms.get(track.asset.0, path, ui.ctx());
            }
            for clip in &mut track.more {
                if let Some(path) = file(self, clip.asset) {
                    clip.peaks = self.waveforms.get(clip.asset.0, path, ui.ctx());
                }
            }
        }

        let duration = self.timeline_duration(ui, &mut snapshot);
        let view_key = timeline_key("timeline_view", id.0);
        let mut view = ui
            .data(|data| data.get_temp::<craft_timeline::View>(view_key))
            .unwrap_or_default();
        let mut interaction = TimelineInteraction {
            playhead: timeline_playhead(ui, id),
            selected: timeline_selected(ui, id),
            drag: timeline_drag(ui, id),
        };
        self.timeline_requests(id, &mut interaction, &mut view, duration);

        let _ = egui::Panel::bottom(Id::unique("timeline_bar").with(id.0))
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(0, 3)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // （タイムラインの切り替えは、Encore と同じくタブの ▼。）
                    let start = ui.cursor().min.x;
                    // The zoom scroll bar spans the tracks, under them.
                    let top = ui.cursor().min.y;
                    let left = start + HEADER_WIDTH;
                    let right = (ui.max_rect().max.x - 4.0).max(left + 60.0);
                    let bar = Rect::from_min_max(
                        egui::pos2(left, top + 5.0),
                        egui::pos2(right, top + 15.0),
                    );
                    let _ = ui.allocate_rect(bar, egui::Sense::hover());
                    craft_timeline::zoom_scrollbar(
                        ui,
                        bar,
                        &mut view,
                        duration,
                        Id::unique("timeline_zoombar").with(id.0),
                    );
                });
            });

        let _ = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                // Timeline ▸ Move Track to View asked for a place to scroll to.
                let wanted = ui.data_mut(|data| data.remove_temp::<f32>(track_view_key(id.0)));
                let mut rows = egui::ScrollArea::vertical()
                    .id_salt(("timeline_rows", id.0))
                    .auto_shrink([false, false]);
                if let Some(offset) = wanted {
                    rows = rows.vertical_scroll_offset(offset.max(0.0));
                }
                rows.show(ui, |ui| {
                    let height = canvas_height(&snapshot.tracks);
                    let _ = ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        self.track_headers(ui, &snapshot, id, height, &mut interaction, duration);
                        self.timeline_scroll(
                            ui,
                            &snapshot,
                            id,
                            duration,
                            &mut view,
                            &mut interaction,
                        );
                    });
                });
            });
        ui.data_mut(|data| data.insert_temp(view_key, view));
        timeline_store_state(ui, id, &interaction);
        self.accept_timeline_drop(ui, id, area);
    }

    /// The canvas with the ruler, the video and track bars, the chapter points and the playhead: the visible part of
    /// the timeline, at the view's zoom and scroll (eased, following the playhead while playing; the wheel zooms
    /// with Alt and scrolls sideways with Ctrl or Shift).
    fn timeline_scroll(
        &mut self,
        ui: &mut Ui,
        snapshot: &TimelineSnapshot,
        id: TimelineId,
        duration: f64,
        view: &mut craft_timeline::View,
        interaction: &mut TimelineInteraction,
    ) {
        let height = canvas_height(&snapshot.tracks);
        let video = disccraft_project::AssetId(snapshot.video);
        let last = self.clip_last_frame(ui.ctx(), video, duration);
        let (canvas, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(60.0), height),
            egui::Sense::click_and_drag(),
        );
        let dt = f64::from(ui.input(|input| input.stable_dt));
        if view.animate(
            dt,
            canvas,
            duration,
            interaction.playhead,
            self.is_playing(),
        ) {
            ui.ctx().request_repaint();
        }
        view.left = canvas.min.x;
        // M6.525: a pick whip held near the left or right edge scrolls the timeline, to reach a chapter marker out of
        // sight.
        let edge = crate::pickwhip::edge_delta(ui.ctx(), canvas, false);
        if edge.x != 0.0 && view.pps > 0.0 {
            view.scroll = (view.scroll - f64::from(edge.x) / view.pps).max(0.0);
            view.target_scroll = view.scroll;
            ui.ctx().request_repaint();
        }
        let origin_of =
            |view: &craft_timeline::View| canvas.min.x - (view.scroll * view.pps) as f32;
        let zoom_of = |view: &craft_timeline::View| (view.pps / f64::from(BASE_SCALE)) as f32;
        let (origin_x, zoom) = (origin_of(view), zoom_of(view));
        if craft_timeline::wheel_scroll(ui, canvas, view, duration, |x| {
            x_to_time(x, origin_x, zoom)
        }) {
            // The wheel was the timeline's: the rows do not scroll with it.
            ui.input_mut(|input| input.smooth_scroll_delta = egui::Vec2::ZERO);
            ui.ctx().request_repaint();
        }
        let in_view = (
            x_to_time(canvas.min.x, origin_x, zoom),
            x_to_time(canvas.max.x, origin_x, zoom),
        );
        let pictures = self.clip_pictures(ui.ctx(), snapshot, duration, in_view);
        let frame = TimelineFrame {
            snapshot,
            canvas,
            origin_x,
            duration,
            zoom,
            playhead: interaction.playhead,
            selected: interaction.selected,
            drag: interaction.drag,
            last,
            pictures,
            show_names: self.show_chapter_names,
            audio_move: ui
                .data(|data| {
                    data.get_temp::<(usize, usize, f32, f32)>(timeline_key("audio_move_drag", id.0))
                })
                .map(|(track, clip, pressed_x, now_x)| {
                    (
                        track,
                        clip,
                        f64::from(now_x - pressed_x) / (f64::from(BASE_SCALE) * f64::from(zoom)),
                    )
                }),
            clip_move: ui
                .data(|data| {
                    data.get_temp::<(usize, f32, f32)>(timeline_key("clip_move_drag", id.0))
                })
                .map(|(clip, pressed_x, now_x)| {
                    (
                        clip,
                        f64::from(now_x - pressed_x) / (f64::from(BASE_SCALE) * f64::from(zoom)),
                    )
                }),
            cue_move: None,
            picked: self
                .clip_selection
                .filter(|(timeline, _)| *timeline == id)
                .map(|(_, pick)| pick),
        };
        // A subtitle clip being dragged: where it would go, drawn as an outline.
        let cue_move = ui
            .data(|data| data.get_temp::<CueDragState>(timeline_key("cue_drag", id.0)))
            .and_then(|state| self.cue_drag_result(&frame, state));
        let frame = TimelineFrame { cue_move, ..frame };

        timeline_draw(&ui.painter().with_clip_rect(canvas), &frame);
        let before = interaction.selected;
        self.timeline_handle_input(ui, &frame, &response, interaction);
        if interaction.selected != before {
            self.focus_chapter(id, interaction.selected);
            // A chapter point chosen lets go of a chosen clip.
            if interaction.selected.is_some() {
                self.clip_selection = None;
            }
        }
        // A click on a subtitle clip selects it: the Properties panel shows the subtitle clip (as in Encore). A
        // click anywhere else in the viewer lets go of it.
        if response.clicked()
            && let Some(point) = response.interact_pointer_pos()
        {
            // A click on no clip lets go of the chosen clip (a click on one chose it as it was let go).
            if clip_body_at(&frame, point).is_none()
                && audio_clip_at(&frame, point).is_none()
                && self
                    .clip_selection
                    .is_some_and(|(timeline, _)| timeline == id)
            {
                self.clip_selection = None;
            }
            match cue_at_point(&frame, point) {
                Some((track, cue)) => {
                    self.select(crate::app::Selection::Timeline(id));
                    self.cue_focus = Some((id, track, cue));
                }
                None => {
                    if self.cue_focus.is_some_and(|(focused, _, _)| focused == id) {
                        self.cue_focus = None;
                    }
                }
            }
        }
        self.note_drop_target(ui, &frame, id);
        self.timeline_pick_whip(ui, &frame, &response, id);
        self.timeline_context_menu(ui, &frame, &response, id, interaction);
    }

    /// A pick whip dropped on the timeline (Encore): on a chapter marker it links to that chapter, elsewhere to the
    /// timeline's start. The marker under it is outlined while it hovers.
    fn timeline_pick_whip(
        &mut self,
        ui: &Ui,
        frame: &TimelineFrame<'_>,
        response: &Response,
        id: TimelineId,
    ) {
        use crate::pickwhip::{PickWhip, dragged, exact_link_for_drop, released};
        let hovering = response.dnd_hover_payload::<PickWhip>().is_some();
        let Some(point) = ui
            .ctx()
            .pointer_hover_pos()
            .filter(|point| frame.canvas.contains(*point))
        else {
            return;
        };
        let markers_x: Vec<f32> = frame
            .snapshot
            .chapters
            .iter()
            .map(|chapter| time_to_x(chapter.time, frame.origin_x, frame.zoom))
            .collect();
        let hit = hit_marker(&markers_x, point.x, HIT_TOLERANCE);
        let chapter = hit.and_then(|index| u32::try_from(index).ok()).unwrap_or(0);
        let link = disccraft_project::Link::Timeline { id, chapter };
        let node = crate::flowchart::NodeId::Timeline(id);
        // Outlined when a drop links here; the tether says whether it does (M6.518). An asset field's pick whip is
        // refused (M6.520).
        crate::pickwhip::refuse_asset_whip(response);
        let hovering = hovering
            && dragged(ui.ctx()).is_some_and(|port| {
                let links = exact_link_for_drop(port, node, link.clone()).is_some();
                crate::pickwhip::aim(ui.ctx(), links);
                links
            });
        if hovering {
            // M6.524: the gold of every pick whip target (a chapter marker, else the whole timeline).
            match hit.and_then(|index| markers_x.get(index)) {
                Some(x) => {
                    let _ = ui.painter().vline(
                        *x,
                        frame.canvas.y_range(),
                        Stroke::new(3.0, crate::flowchart::DROP_EDGE),
                    );
                }
                None => crate::pickwhip::mark_target(ui.painter(), frame.canvas),
            }
        }
        if let Some(payload) = released::<PickWhip>(response)
            && let Some((command, base, link)) = exact_link_for_drop(payload.0, node, link)
        {
            self.apply_link(command, base, &link);
        }
    }

    /// Chooses a clip of timeline `timeline` (outlined; Edit ▸ Clear and Ripple Delete act on it), and the timeline.
    fn pick_clip(&mut self, timeline: u64, pick: ClipPick) {
        let id = TimelineId(timeline);
        self.clip_selection = Some((id, pick));
        self.cue_focus = None;
        self.select(crate::app::Selection::Timeline(id));
    }

    /// Lets go of the clip and the subtitle chosen in timeline `id` (Encore's Next Clip past the last clip).
    fn let_go_of_clips(&mut self, id: TimelineId) {
        if self
            .clip_selection
            .is_some_and(|(timeline, _)| timeline == id)
        {
            self.clip_selection = None;
        }
        if self
            .cue_focus
            .is_some_and(|(timeline, _, _)| timeline == id)
        {
            self.cue_focus = None;
        }
    }

    /// Encore's ◂ / ▸ of an audio or subtitle track's header: the playhead goes to the start of the clip before it /
    /// after it on that track (past the last clip, to its end), and that clip is chosen.
    fn step_track_clip(
        &mut self,
        id: TimelineId,
        track: &TrackRow,
        interaction: &mut TimelineInteraction,
        duration: f64,
        forward: bool,
    ) {
        if track.subtitle {
            let cues = track_step_spans(track, duration);
            if let Some((time, cue)) = clip_step(&cues, interaction.playhead, forward) {
                interaction.playhead = time;
                self.let_go_of_clips(id);
                if let Some(cue) = cue {
                    self.select(crate::app::Selection::Timeline(id));
                    self.cue_focus = Some((id, track.index, cue));
                }
            }
            return;
        }
        let boxes = sorted_audio_boxes(track, duration);
        let spans: Vec<(f64, f64)> = boxes.iter().map(|(start, end, _)| (*start, *end)).collect();
        if let Some((time, clip)) = clip_step(&spans, interaction.playhead, forward) {
            interaction.playhead = time;
            match clip.and_then(|index| boxes.get(index)) {
                Some((_, _, number)) => {
                    self.pick_clip(id.0, ClipPick::Audio(track.index, *number));
                }
                None => self.let_go_of_clips(id),
            }
        }
    }

    /// Edit ▸ Clear on a chosen clip: a video clip leaves its place empty (a timeline's only clip stays); an audio
    /// clip is taken off its track, the track itself when it was its only clip.
    pub(crate) fn clear_clip(&mut self, id: TimelineId, pick: ClipPick) {
        match pick {
            ClipPick::Video(clip) => {
                if self.timeline_clips(id).len() < 2 {
                    self.status = tl!("A timeline's only clip cannot be removed").to_owned();
                    return;
                }
                let _ = self.run_command(
                    "timeline.removeVideo",
                    json!({"timeline": id.0, "clip": clip, "ripple": false}),
                );
            }
            ClipPick::Audio(track, clip) => {
                let several = self
                    .session
                    .project
                    .timeline(id)
                    .and_then(|timeline| timeline.audio.get(track))
                    .is_some_and(|found| !found.more.is_empty());
                if several {
                    let _ = self.run_command(
                        "timeline.removeAudioClip",
                        json!({"timeline": id.0, "track": track, "clip": clip}),
                    );
                } else {
                    let _ = self.run_command(
                        "timeline.removeAudioTrack",
                        json!({"timeline": id.0, "track": track}),
                    );
                }
            }
        }
    }

    /// Where a dragged subtitle clip would go ([`dragged_cue`]): its track, its number and its (start, end) in
    /// seconds; snapped (Preferences ▸ Timelines ▸ Snap subtitle clips) to the other clips' edges, the chapter
    /// points, the playhead and the start.
    fn cue_drag_result(
        &self,
        frame: &TimelineFrame<'_>,
        (track, cue, grip, pressed_x, now_x): CueDragState,
    ) -> Option<(usize, usize, f64, f64)> {
        let row = frame
            .snapshot
            .tracks
            .iter()
            .find(|row| row.subtitle && row.index == track)?;
        let cues: Vec<(f64, f64)> = row
            .cues
            .as_ref()?
            .iter()
            .map(|found| (found.start, found.end))
            .collect();
        let pps = f64::from(BASE_SCALE) * f64::from(frame.zoom);
        if !(pps.is_finite() && pps > 0.0) {
            return None;
        }
        let (num, den) = (frame.snapshot.fps_num, frame.snapshot.fps_den);
        let step = f64::from(den.max(1)) / f64::from(num.max(1));
        // `as` is exact for a count of frames.
        let shortest = crate::subtitle_edit::min_subtitle_frames(num, den) as f64 * step;
        let snaps: Vec<f64> = if self.timeline_prefs.snap_subtitles {
            let mut times = vec![0.0, frame.playhead];
            times.extend(frame.snapshot.chapters.iter().map(|chapter| chapter.time));
            for other in frame.snapshot.tracks.iter().filter(|other| other.subtitle) {
                for (number, found) in other.cues.iter().flat_map(|cues| cues.iter()).enumerate() {
                    if !(other.index == track && number == cue) {
                        times.extend([found.start, found.end]);
                    }
                }
            }
            times
        } else {
            Vec::new()
        };
        let snap = (!snaps.is_empty()).then_some((snaps.as_slice(), f64::from(CUE_SNAP) / pps));
        let shift = f64::from(now_x - pressed_x) / pps;
        let (start, end) =
            dragged_cue(&cues, cue, CueGrip::of(grip), shift, (step, shortest), snap)?;
        Some((track, cue, start, end))
    }

    /// A Project panel row dragged over an audio track's row: where it would go (the track and the time under the
    /// pointer), marked on the row and kept for the drop ([`DiscCraftApp::accept_timeline_drop`]).
    fn note_drop_target(&self, ui: &Ui, frame: &TimelineFrame<'_>, id: TimelineId) {
        let key = drop_target_key(id.0);
        let dragging =
            egui::DragAndDrop::has_payload_of_type::<crate::project_panel::DragRow>(ui.ctx());
        let target = ui
            .input(|i| i.pointer.interact_pos())
            .filter(|point| dragging && point.x >= frame.origin_x && frame.canvas.contains(*point))
            .and_then(|point| {
                let row = audio_row_at(frame, point)?;
                let time =
                    x_to_time(point.x, frame.origin_x, frame.zoom).clamp(0.0, frame.duration);
                Some((row, time))
            });
        match target {
            Some((row, time)) => {
                // Where the clip would start, across the row under the pointer.
                let top = frame
                    .snapshot
                    .tracks
                    .iter()
                    .zip(track_rows_geometry(&frame.snapshot.tracks))
                    .find(|(track, _)| !track.subtitle && track.index == row)
                    .map(|(_, (top, height))| (frame.canvas.min.y + top, height));
                if let Some((y, height)) = top {
                    let x = time_to_x(time, frame.origin_x, frame.zoom);
                    let _ = ui.painter().line_segment(
                        [egui::pos2(x, y), egui::pos2(x, y + height)],
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(0x4a, 0x90, 0xff)),
                    );
                }
                ui.data_mut(|data| data.insert_temp(key, (row, time)));
            }
            None => ui.data_mut(|data| data.remove::<(usize, f64)>(key)),
        }
    }

    /// The pictures along each video clip (M6.483, as Encore shows them): the frame at the clip's head and at each
    /// chapter point in it. Only the pictures of the stretch `in_view` (seconds) are asked for; they are decoded on
    /// a thread of their own ([`crate::clip_thumbs`]).
    fn clip_pictures(
        &mut self,
        ctx: &egui::Context,
        snapshot: &TimelineSnapshot,
        duration: f64,
        in_view: (f64, f64),
    ) -> Vec<Vec<(f64, Option<egui::TextureHandle>)>> {
        let spans = if snapshot.spans.is_empty() {
            vec![(0.0, duration)]
        } else {
            snapshot.spans.clone()
        };
        let assets: Vec<u64> = std::iter::once(snapshot.video)
            .chain(snapshot.more_video.iter().map(|(asset, _)| *asset))
            .collect();
        let fps = frames_per_second(snapshot.fps_num, snapshot.fps_den).max(1.0);
        let chapters: Vec<f64> = snapshot
            .chapters
            .iter()
            .map(|chapter| chapter.time)
            .collect();
        let mut out = Vec::with_capacity(spans.len());
        for (index, (start, end)) in spans.iter().enumerate() {
            let path = assets.get(index).and_then(|asset| {
                self.session
                    .project
                    .assets
                    .get(&disccraft_project::AssetId(*asset))
                    .map(|found| (*asset, std::path::PathBuf::from(&found.path)))
            });
            let Some((asset, path)) = path else {
                out.push(Vec::new());
                continue;
            };
            let source = snapshot.sources.get(index).copied().unwrap_or(0.0);
            let starts = picture_starts(*start, *end, &chapters, 0.5 / fps);
            let mut pictures = Vec::with_capacity(starts.len());
            for (n, at) in starts.iter().enumerate() {
                let until = starts.get(n + 1).copied().unwrap_or(*end);
                let texture = if until >= in_view.0 && *at <= in_view.1 {
                    let seconds = at - start + source;
                    // `as` saturates; a frame number in a file is small.
                    let key = (asset, (seconds * fps).round() as i64);
                    self.clip_thumbs
                        .get(ctx, key, &path, (seconds, until - start + source))
                } else {
                    None
                };
                pictures.push((*at, texture));
            }
            out.push(pictures);
        }
        out
    }

    /// The last frame of a video asset (cached per asset; `asset.frame` at one frame before the end).
    fn clip_last_frame(
        &mut self,
        ctx: &egui::Context,
        video: disccraft_project::AssetId,
        duration: f64,
    ) -> Option<egui::TextureHandle> {
        if let Some(cached) = self.last_frames.get(&video.0) {
            return cached.clone();
        }
        let (num, den) = frame_rate(self.session.project.settings.standard);
        let step = f64::from(den.max(1)) / f64::from(num.max(1));
        let seconds = (duration - step).max(0.0);
        // Not waited for: asked again once the file is open.
        let value = self.run_query_quiet(
            "asset.frame",
            json!({ "asset": video.0, "seconds": seconds, "wait": false }),
        );
        if crate::project_panel::is_pending(value.as_ref()) {
            self.media_pending = true;
            return None;
        }
        let texture = value.and_then(|value| {
            crate::project_panel::decode_rgb_texture(
                ctx,
                &format!("clip_last_frame_{}", video.0),
                &value,
            )
        });
        let _ = self.last_frames.insert(video.0, texture.clone());
        texture
    }

    /// Builds an owned snapshot of the timeline data needed by the view.
    fn timeline_snapshot(&mut self, id: TimelineId) -> Option<TimelineSnapshot> {
        let timeline = &self.session.project.timeline(id)?.clone();
        let (fps_num, fps_den) = frame_rate(self.session.project.settings.standard);

        // Encore shows a clip by its file's name with the extension (00001.m2ts), as the Project panel does
        // (M6.482).
        let asset_name = match self.session.project.assets.get(&timeline.video) {
            Some(asset) => crate::project_panel::name_with_extension(&asset.name, &asset.path),
            None => String::new(),
        };

        let chapters = timeline
            .chapters
            .iter()
            .map(|chapter| ChapterSnapshot {
                time: ticks_to_seconds(chapter.time.0),
                name: chapter.name.clone(),
                poster: chapter.poster.map(|poster| ticks_to_seconds(poster.0)),
            })
            .collect();

        let tracks = self.track_rows(timeline);

        let more_video = timeline
            .more_video
            .iter()
            .map(|asset| {
                let name = self
                    .session
                    .project
                    .assets
                    .get(asset)
                    .map_or_else(String::new, |found| {
                        crate::project_panel::name_with_extension(&found.name, &found.path)
                    });
                (asset.0, name)
            })
            .collect();

        Some(TimelineSnapshot {
            id: id.0,
            video: timeline.video.0,
            asset_name,
            more_video,
            joins: Vec::new(),
            spans: Vec::new(),
            sources: Vec::new(),
            gaps: timeline
                .video_gaps
                .iter()
                .map(|gap| u64::from(*gap))
                .collect(),
            trims: timeline
                .video_trims
                .iter()
                .flat_map(|trim| {
                    [
                        u64::from(trim.start),
                        trim.end.map_or(0, |end| u64::from(end).saturating_add(1)),
                    ]
                })
                .collect(),
            chapters,
            tracks,
            fps_num,
            fps_den,
        })
    }

    /// Track rows for the audio and subtitle tracks of `timeline`.
    fn track_rows(&mut self, timeline: &disccraft_project::Timeline) -> Vec<TrackRow> {
        let (num, den) = frame_rate(self.session.project.settings.standard);
        let frame = f64::from(den.max(1)) / f64::from(num.max(1));
        let lengths: Vec<Option<f64>> = timeline
            .audio
            .iter()
            .map(|track| self.asset_probe(track.asset).seconds)
            .collect();
        // The lengths of the files of each track's further clips.
        let more_lengths: Vec<Vec<Option<f64>>> = timeline
            .audio
            .iter()
            .map(|track| {
                track
                    .more
                    .iter()
                    .map(|clip| self.asset_probe(clip.asset).seconds)
                    .collect()
            })
            .collect();
        let name = |asset: &disccraft_project::AssetId| {
            self.session
                .project
                .assets
                .get(asset)
                .map(|asset| crate::project_panel::name_with_extension(&asset.name, &asset.path))
                .unwrap_or_default()
        };
        let audio = timeline
            .audio
            .iter()
            .enumerate()
            .map(|(index, track)| TrackRow {
                label: tlf!("Audio {n}:", n = index.saturating_add(1)),
                index,
                language: track.language.clone(),
                // M6.424: a track playing another sound stream of its file than the first says which.
                clip: if track.stream > 0 {
                    tlf!(
                        "{clip} (Stream {n})",
                        clip = name(&track.asset),
                        n = track.stream.saturating_add(1)
                    )
                } else {
                    name(&track.asset)
                },
                subtitle: false,
                cues: None,
                asset: track.asset,
                peaks: None,
                start: f64::from(track.start) * frame,
                length: audio_clip_length(
                    lengths.get(index).copied().flatten(),
                    f64::from(track.trim_start) * frame,
                    track.trim_end.map(|end| f64::from(end) * frame),
                ),
                trim_start: f64::from(track.trim_start) * frame,
                more: track
                    .more
                    .iter()
                    .enumerate()
                    .map(|(at, clip)| MoreClip {
                        number: at.saturating_add(1),
                        name: name(&clip.asset),
                        asset: clip.asset,
                        start: f64::from(clip.start) * frame,
                        length: audio_clip_length(
                            more_lengths
                                .get(index)
                                .and_then(|lengths| lengths.get(at))
                                .copied()
                                .flatten(),
                            f64::from(clip.trim_start) * frame,
                            clip.trim_end.map(|end| f64::from(end) * frame),
                        ),
                        trim_start: f64::from(clip.trim_start) * frame,
                        peaks: None,
                    })
                    .collect(),
            });
        let subtitles = timeline
            .subtitles
            .iter()
            .enumerate()
            .map(|(index, track)| TrackRow {
                label: tlf!("Subtitle {n}:", n = index.saturating_add(1)),
                index,
                language: track.language.clone(),
                clip: name(&track.asset),
                subtitle: true,
                cues: self.cached_cues(track.asset),
                asset: track.asset,
                peaks: None,
                start: 0.0,
                length: None,
                trim_start: 0.0,
                more: Vec::new(),
            });
        audio.chain(subtitles).collect()
    }

    /// Returns the timeline duration, probing the video asset once per timeline.
    fn timeline_duration(&mut self, ui: &mut Ui, snapshot: &mut TimelineSnapshot) -> f64 {
        let key = timeline_key("timeline_duration", snapshot.id);
        let videos: Vec<u64> = std::iter::once(snapshot.video)
            .chain(snapshot.more_video.iter().map(|(asset, _)| *asset))
            // A marker no asset id takes, then the trims: a different trim is a different measurement.
            .chain(std::iter::once(u64::MAX))
            .chain(snapshot.trims.iter().copied())
            // And the gaps: a moved clip is another measurement too.
            .chain(std::iter::once(u64::MAX))
            .chain(snapshot.gaps.iter().copied())
            .collect();
        let mut probe = match ui.data(|data| data.get_temp::<DurationProbe>(key)) {
            Some(probe) if probe.videos == videos => probe,
            _ => DurationProbe {
                videos,
                done: false,
                seconds: 0.0,
                joins: Vec::new(),
                spans: Vec::new(),
                sources: Vec::new(),
            },
        };

        if !probe.done {
            probe.done = true;
            if snapshot.more_video.is_empty()
                && snapshot.trims.is_empty()
                && snapshot.gaps.iter().all(|gap| *gap == 0)
            {
                if let Some(value) = self.run_query("asset.probe", json!({"asset": snapshot.video}))
                    && let Some(seconds) = value.get("seconds").and_then(Value::as_f64)
                    && seconds.is_finite()
                    && seconds > 0.0
                {
                    probe.seconds = seconds;
                }
            } else if let Some(value) =
                self.run_query("timeline.clips", json!({"timeline": snapshot.id}))
            {
                // Several clips: the engine adds their lengths up (frame-exact for MPEG-2 streams).
                let seconds =
                    |field: Option<&Value>| field.and_then(Value::as_i64).map(ticks_to_seconds);
                probe.seconds = seconds(value.get("duration")).unwrap_or(0.0);
                probe.spans = value
                    .get("clips")
                    .and_then(Value::as_array)
                    .map(|clips| {
                        clips
                            .iter()
                            .filter_map(|clip| {
                                let start = seconds(clip.get("start"))?;
                                Some((start, start + seconds(clip.get("duration"))?))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                probe.sources = value
                    .get("clips")
                    .and_then(Value::as_array)
                    .map(|clips| {
                        clips
                            .iter()
                            .map(|clip| seconds(clip.get("sourceStart")).unwrap_or(0.0))
                            .collect()
                    })
                    .unwrap_or_default();
                probe.joins = probe
                    .spans
                    .iter()
                    .skip(1)
                    .map(|(start, _)| *start)
                    .collect();
            }
            ui.data_mut(|data| data.insert_temp(key, probe.clone()));
        }
        snapshot.joins.clone_from(&probe.joins);
        snapshot.spans.clone_from(&probe.spans);
        snapshot.sources.clone_from(&probe.sources);

        if probe.seconds > 0.0 {
            probe.seconds
        } else {
            fallback_duration(&snapshot.chapters)
        }
    }

    /// Requests from the menus and shortcuts: View ▸ Zoom In / Zoom Out (about the playhead), Fit in Window, add a
    /// chapter point at the playhead, and delete the selected chapter point.
    fn timeline_requests(
        &mut self,
        id: TimelineId,
        interaction: &mut TimelineInteraction,
        view: &mut craft_timeline::View,
        duration: f64,
    ) {
        let width = (view.width).max(1.0);
        let playhead_x = view.left + ((interaction.playhead - view.scroll) * view.pps) as f32;
        match self.view_request.take() {
            Some(ViewRequest::ZoomIn) => {
                view.zoom_about(1.25, interaction.playhead, playhead_x, width, duration);
            }
            Some(ViewRequest::ZoomOut) => {
                view.zoom_about(
                    1.0 / 1.25,
                    interaction.playhead,
                    playhead_x,
                    width,
                    duration,
                );
            }
            Some(ViewRequest::Fit) => view.fit_pending = true,
            Some(ViewRequest::AddChapterPoint) => {
                self.add_chapter_at_playhead(id, interaction.playhead, duration);
            }
            Some(ViewRequest::Clear) => {
                // A chosen clip is cleared (its place left empty); else the chosen chapter point (the first cannot
                // be removed).
                if let Some((_, pick)) = self.clip_selection.filter(|(timeline, _)| *timeline == id)
                {
                    self.clip_selection = None;
                    self.clear_clip(id, pick);
                } else if let Some(index) = interaction.selected.filter(|index| *index > 0) {
                    let _ = self.run_command(
                        "timeline.removeChapter",
                        json!({"timeline": id.0, "chapter": index}),
                    );
                    interaction.selected = None;
                    interaction.drag = None;
                }
            }
            other => self.view_request = other,
        }
    }

    /// Adds a chapter point at the playhead (kept inside the clip).
    fn add_chapter_at_playhead(&mut self, id: TimelineId, playhead: f64, duration: f64) {
        let time = seconds_to_ticks(playhead.clamp(0.0, duration));
        let _ = self.run_command(
            "timeline.addChapter",
            json!({"timeline": id.0, "time": time}),
        );
    }

    /// Encore's timeline and source times in the timeline header (M6.483): hot text that a drag moves a frame a point
    /// and a click lets be typed. The source time is where the playhead is in the video clip under it (none in a
    /// gap); a typed source time goes to that time in the clip's file, within the clip.
    fn header_times(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        snapshot: &TimelineSnapshot,
        id: TimelineId,
        interaction: &mut TimelineInteraction,
        duration: f64,
    ) {
        let painter = ui.painter().clone();
        let at = |x: f32, y: f32| egui::pos2(rect.min.x + x, rect.min.y + y);
        let time_font = egui::FontId::proportional(12.0);
        let fps = frames_per_second(snapshot.fps_num, snapshot.fps_den).max(1.0);
        let frame_seconds = 1.0 / fps;
        let playhead = interaction.playhead;
        let mut moved_to: Option<f64> = None;
        match crate::hot_text::hot_timecode(
            ui,
            Id::unique("timeline_time").with(id.0),
            (at(HEADER_TIME_X, 22.0), egui::Align2::LEFT_TOP),
            &timecode(playhead, snapshot.fps_num, snapshot.fps_den),
            time_font.clone(),
        ) {
            crate::hot_text::HotChange::Frames(frames) => {
                // `as` is exact for a drag's frames.
                moved_to = Some(playhead + frames as f64 * frame_seconds);
            }
            crate::hot_text::HotChange::Typed(text) => {
                match disccraft_project::parse_time(&text, &self.session.project.settings) {
                    Ok(time) => moved_to = Some(ticks_to_seconds(time.0)),
                    Err(error) => self.status = error.to_string(),
                }
            }
            crate::hot_text::HotChange::None => {}
        }
        // The source time: where the playhead is in the video clip under it (Encore); none in a gap.
        let spans = if snapshot.spans.is_empty() {
            vec![(0.0, duration)]
        } else {
            snapshot.spans.clone()
        };
        let under = spans.iter().enumerate().find(|(index, (start, end))| {
            playhead >= *start
                && (playhead < *end || (*index + 1 == spans.len() && playhead <= *end))
        });
        match under {
            Some((index, (start, end))) => {
                let source = snapshot.sources.get(index).copied().unwrap_or(0.0);
                match crate::hot_text::hot_timecode(
                    ui,
                    Id::unique("timeline_source_time").with(id.0),
                    (at(HEADER_SOURCE_X, 22.0), egui::Align2::LEFT_TOP),
                    &timecode(
                        playhead - start + source,
                        snapshot.fps_num,
                        snapshot.fps_den,
                    ),
                    time_font,
                ) {
                    crate::hot_text::HotChange::Frames(frames) => {
                        // `as` is exact for a drag's frames.
                        moved_to = Some(playhead + frames as f64 * frame_seconds);
                    }
                    crate::hot_text::HotChange::Typed(text) => {
                        match disccraft_project::parse_time(&text, &self.session.project.settings) {
                            // A time in the clip's file: there in the timeline, within the clip.
                            Ok(time) => {
                                moved_to = Some(
                                    (start + ticks_to_seconds(time.0) - source).clamp(*start, *end),
                                );
                            }
                            Err(error) => self.status = error.to_string(),
                        }
                    }
                    crate::hot_text::HotChange::None => {}
                }
            }
            None => {
                painter.text(
                    at(HEADER_SOURCE_X, 22.0),
                    egui::Align2::LEFT_TOP,
                    "--;--;--;--",
                    time_font,
                    craft_timeline::style::TEXT_DIM.get(),
                );
            }
        }
        if let Some(seconds) = moved_to {
            // Moving the time stops playback, as the other ways of moving the playhead do.
            self.playback = None;
            interaction.playhead = seconds.clamp(0.0, duration.max(0.0));
        }
    }

    /// The header column: the current time ("Timeline:" and, inside the clip, "Source:") with the add-chapter-point
    /// button, then a header per row: "Video", and each audio and subtitle track with its language as a list.
    fn track_headers(
        &mut self,
        ui: &mut Ui,
        snapshot: &TimelineSnapshot,
        id: TimelineId,
        height: f32,
        interaction: &mut TimelineInteraction,
        duration: f64,
    ) {
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(HEADER_WIDTH, height), egui::Sense::hover());
        let painter = ui.painter().clone();
        // Encore's timeline header (M6.483): "Timeline:" and "Source:" with the times under them as hot text (orange,
        // dotted under; drag sideways a frame a point, click to type), and the tools at the bottom right (Encore's
        // add-chapter mark there; DiscCraft's display settings beside it).
        let at = |x: f32, y: f32| egui::pos2(rect.min.x + x, rect.min.y + y);
        painter.rect_filled(
            Rect::from_min_max(rect.min, at(HEADER_WIDTH, RULER_HEIGHT)),
            0.0,
            craft_timeline::style::TL_BG,
        );
        let label_font = egui::FontId::proportional(11.0);
        for (x, label) in [
            (HEADER_TIME_X, tl!("Timeline:")),
            (HEADER_SOURCE_X, tl!("Source timecode:")),
        ] {
            painter.text(
                at(x, 6.0),
                egui::Align2::LEFT_TOP,
                label,
                label_font.clone(),
                craft_timeline::style::TEXT.get(),
            );
        }
        self.header_times(ui, rect, snapshot, id, interaction, duration);

        // The tools at the bottom right: the display settings, then adding a chapter point at the playhead.
        let tools = [
            (
                crate::icons::Icon::Wrench,
                "settings",
                tl!("Timeline Display Settings"),
            ),
            (
                crate::icons::Icon::Marker,
                "marker",
                tl!("Add Chapter at Playhead"),
            ),
        ];
        let mut x = HEADER_WIDTH - 2.0 * HEADER_TOOL - 6.0;
        for (icon, key, tip) in tools {
            let button = Rect::from_min_size(
                at(x, RULER_HEIGHT - HEADER_TOOL - 3.0),
                egui::vec2(HEADER_TOOL, HEADER_TOOL),
            );
            let response = ui
                .interact(
                    button,
                    Id::unique(("timeline_tool", key)).with(id.0),
                    egui::Sense::click(),
                )
                .on_hover_text(tip);
            if response.hovered() {
                crate::theme::hover_box(&painter, button);
            }
            crate::icons::paint(
                &painter,
                button.shrink(4.0),
                icon,
                if response.hovered() {
                    crate::theme::HOVER_ICON.get()
                } else {
                    craft_timeline::style::TEXT_DIM.get()
                },
            );
            if key == "marker" && response.clicked() {
                self.add_chapter_at_playhead(id, interaction.playhead, duration);
            }
            if key == "settings" {
                let _ = egui::Popup::menu(&response)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        let _ =
                            ui.checkbox(&mut self.show_chapter_names, tl!("Show Chapter Names"));
                    });
            }
            x += HEADER_TOOL + 2.0;
        }

        // Track headers as Encore draws them (M6.474): a small box at the left with the track's mark (the video's
        // eye, an audio track's speaker, a subtitle track's eye), the name after it, and the language list.
        let video = Rect::from_min_max(
            at(0.0, VIDEO_TOP),
            at(HEADER_WIDTH, VIDEO_TOP + VIDEO_HEIGHT),
        );
        header_row(&painter, video, tl!("Video"));
        // Encore's video eye shows the track is seen; clicking it changes nothing (measured), so it is only drawn.
        let eye = mark_box(video);
        paint_mark_box(&painter, eye, false);
        crate::icons::paint(
            &painter,
            mark_icon(eye),
            crate::icons::Icon::Eye,
            craft_timeline::style::ICON_ACTIVE.get(),
        );
        // Encore's ◂ ▸ (Previous Clip / Next Clip) at the right of every track header.
        let spans: Vec<(f64, f64)> = if snapshot.spans.is_empty() {
            vec![(0.0, duration)]
        } else {
            snapshot.spans.clone()
        };
        let reach = step_reach(&spans, interaction.playhead);
        if let Some(forward) = step_arrows(ui, &painter, video, (id.0, 0, 0), reach)
            && let Some((time, clip)) = clip_step(&spans, interaction.playhead, forward)
        {
            interaction.playhead = time;
            match clip {
                Some(clip) => self.pick_clip(id.0, ClipPick::Video(clip)),
                None => self.let_go_of_clips(id),
            }
        }

        for (track, (top, row_h)) in snapshot
            .tracks
            .iter()
            .zip(track_rows_geometry(&snapshot.tracks))
        {
            let row = Rect::from_min_max(at(0.0, top), at(HEADER_WIDTH, top + row_h));
            header_row(&painter, row, &track.label);
            let mark = mark_box(row);
            if !track.subtitle {
                // The speaker marks the audio track heard in the monitor (Encore: the box at the left of an audio
                // track's header; a new track takes it).
                let response = ui
                    .interact(
                        mark,
                        Id::unique("audio_heard").with((id.0, track.index)),
                        egui::Sense::click(),
                    )
                    .on_hover_text(tl!("Hear This Track in Monitor"));
                let heard = self.heard_audio(id) == Some(track.index);
                paint_mark_box(&painter, mark, response.hovered());
                paint_speaker(
                    &painter,
                    mark_icon(mark).shrink(1.0),
                    if heard {
                        craft_timeline::style::ICON_ACTIVE.get()
                    } else {
                        craft_timeline::style::TEXT_DIM.get()
                    },
                    heard,
                );
                if response.clicked() {
                    self.set_heard_audio(id, track.index);
                }
            }
            if track.subtitle {
                // The eye shows the track's subtitles in the monitor (Encore: one track or none).
                let response = ui
                    .interact(
                        mark,
                        Id::unique("subtitle_shown").with((id.0, track.index)),
                        egui::Sense::click(),
                    )
                    .on_hover_text(tl!("Show Subtitles in Monitor"));
                let shown = self.shown_subtitle(id) == Some(track.index);
                paint_mark_box(&painter, mark, response.hovered());
                crate::icons::paint(
                    &painter,
                    mark_icon(mark),
                    if shown {
                        crate::icons::Icon::Eye
                    } else {
                        crate::icons::Icon::EyeOff
                    },
                    if shown {
                        craft_timeline::style::ICON_ACTIVE.get()
                    } else {
                        craft_timeline::style::TEXT_DIM.get()
                    },
                );
                if response.clicked() {
                    self.toggle_shown_subtitle(id, track.index);
                }
            }
            let reach = step_reach(&track_step_spans(track, duration), interaction.playhead);
            if let Some(forward) = step_arrows(
                ui,
                &painter,
                row,
                (id.0, 1 + u8::from(track.subtitle), track.index),
                reach,
            ) {
                self.step_track_clip(id, track, interaction, duration, forward);
            }
            // The language list, left of the ◂ ▸ (Encore's place).
            let combo = Rect::from_min_size(
                egui::pos2(row.max.x - 66.0 - STEP_ARROWS_WIDTH, row.center().y - 10.0),
                egui::vec2(60.0, 20.0),
            );
            // An audio track without a language of its own gets the disc's, else the display language.
            let fallback = if track.subtitle {
                String::new()
            } else {
                self.session
                    .project
                    .settings
                    .audio_language
                    .clone()
                    .unwrap_or_else(|| crate::i18n::current().code().to_owned())
            };
            if let Some(language) = language_list(ui, combo, id, track, &fallback) {
                let kind = if track.subtitle { "subtitle" } else { "audio" };
                let _ = self.run_command(
                    "timeline.setTrackLanguage",
                    json!({"timeline": id.0, "kind": kind, "track": track.index, "language": language}),
                );
            }
        }
    }

    /// Right-click menu of the timeline canvas: add a chapter point at the clicked time, delete the chapter point
    /// under the pointer, or add chapter points at an interval.
    fn timeline_context_menu(
        &mut self,
        ui: &Ui,
        frame: &TimelineFrame<'_>,
        response: &Response,
        id: TimelineId,
        interaction: &mut TimelineInteraction,
    ) {
        let key = timeline_key("timeline_context", id.0);
        if response.secondary_clicked()
            && let Some(point) = response.interact_pointer_pos()
        {
            let time = x_to_time(point.x, frame.origin_x, frame.zoom).clamp(0.0, frame.duration);
            let markers_x: Vec<f32> = frame
                .snapshot
                .chapters
                .iter()
                .map(|chapter| time_to_x(chapter.time, frame.origin_x, frame.zoom))
                .collect();
            let hit = hit_marker(&markers_x, point.x, HIT_TOLERANCE);
            ui.data_mut(|data| data.insert_temp(key, (time, hit)));
            // The audio track's row and the clip that were clicked, for the audio clip items.
            let row = audio_row_at(frame, point);
            let clip = audio_clip_at(frame, point).map(|(_, clip)| clip);
            ui.data_mut(|data| data.insert_temp(audio_context_key(id.0), (row, clip)));
        }

        let Some((time, hit)) = ui.data(|data| data.get_temp::<(f64, Option<usize>)>(key)) else {
            return;
        };
        let duration = frame.duration;
        // The clip under the pointer (Encore: Clear on a clip); a timeline's only clip stays.
        let clips = frame.snapshot.joins.len().saturating_add(1);
        let clip = frame
            .snapshot
            .joins
            .iter()
            .filter(|join| **join <= time)
            .count();
        let (audio_row, audio_clip) = ui
            .data(|data| data.get_temp::<(Option<usize>, Option<usize>)>(audio_context_key(id.0)))
            .unwrap_or((None, None));
        let _ = response.context_menu(|ui| {
            // On an audio track's row (M6.269): another clip of a sound file at this time, and Clear on a clip
            // of a track that has several.
            if let Some(track) = audio_row {
                let sounds: Vec<(u64, String)> = self
                    .session
                    .project
                    .assets
                    .values()
                    .filter(|asset| asset.kind == disccraft_project::AssetKind::Audio)
                    .map(|asset| (asset.id.0, asset.name.clone()))
                    .collect();
                ui.menu_button(tl!("Add Audio Clip Here"), |ui| {
                    if sounds.is_empty() {
                        let _ = ui.add_enabled(false, egui::Button::new(tl!("(no audio assets)")));
                    }
                    for (asset, name) in &sounds {
                        if ui.button(name).clicked() {
                            let _ = self.run_command(
                                "timeline.addAudioClip",
                                json!({
                                    "timeline": id.0,
                                    "track": track,
                                    "asset": asset,
                                    "time": seconds_to_ticks(time),
                                }),
                            );
                            ui.close();
                        }
                    }
                });
                let several = frame
                    .snapshot
                    .tracks
                    .iter()
                    .any(|row| !row.subtitle && row.index == track && !row.more.is_empty());
                if let Some(clip) = audio_clip.filter(|_| several)
                    && ui.button(tl!("Clear Audio Clip")).clicked()
                {
                    let _ = self.run_command(
                        "timeline.removeAudioClip",
                        json!({"timeline": id.0, "track": track, "clip": clip}),
                    );
                    ui.close();
                }
                // Which sound stream of its file the track plays (M6.418), when the file has several (an MKV's
                // languages, a camcorder's second stereo pair).
                let played = self
                    .session
                    .project
                    .timeline(id)
                    .and_then(|timeline| timeline.audio.get(track))
                    .map(|track| (track.asset, track.stream as usize));
                if let Some((asset, current)) = played {
                    let streams =
                        disccraft_engine::audio_stream_count(&self.session.project, asset);
                    if streams > 1 {
                        ui.menu_button(tl!("Sound Stream"), |ui| {
                            for stream in 0..streams {
                                let label = tlf!("Stream {n}", n = stream + 1);
                                if ui.selectable_label(stream == current, label).clicked() {
                                    let _ = self.run_command(
                                        "timeline.setAudioStream",
                                        json!({"timeline": id.0, "track": track, "stream": stream}),
                                    );
                                    ui.close();
                                }
                            }
                        });
                    }
                }
                let _ = ui.separator();
            }
            if clips > 1 {
                // Encore: Clear leaves the clip's place empty; Ripple Delete closes it up.
                if ui.button(tl!("Clear Video Clip")).clicked() {
                    let _ = self.run_command(
                        "timeline.removeVideo",
                        json!({"timeline": id.0, "clip": clip, "ripple": false}),
                    );
                    ui.close();
                }
                if ui
                    .button(tl!("Ripple Delete"))
                    .on_hover_text(tl!(
                        "Removes the clip and moves the clips after it up into its place"
                    ))
                    .clicked()
                {
                    let _ = self.run_command(
                        "timeline.removeVideo",
                        json!({"timeline": id.0, "clip": clip, "ripple": true}),
                    );
                    ui.close();
                }
                let _ = ui.separator();
            }
            if ui.button(tl!("Add Chapter Point Here")).clicked() {
                let _ = self.run_command(
                    "timeline.addChapter",
                    json!({"timeline": id.0, "time": seconds_to_ticks(time)}),
                );
                ui.close();
            }
            // The first chapter point cannot be removed.
            let removable = hit.filter(|index| *index > 0);
            if ui
                .add_enabled(
                    removable.is_some(),
                    egui::Button::new(tl!("Delete Chapter Point")),
                )
                .clicked()
            {
                if let Some(index) = removable {
                    let _ = self.run_command(
                        "timeline.removeChapter",
                        json!({"timeline": id.0, "chapter": index}),
                    );
                }
                ui.close();
            }
            // Encore: その時刻を含むチャプターのポスターフレームにする。
            let containing = frame
                .snapshot
                .chapters
                .iter()
                .rposition(|chapter| chapter.time <= time);
            if ui
                .add_enabled(
                    containing.is_some(),
                    egui::Button::new(tl!("Set Poster Frame")),
                )
                .clicked()
            {
                if let Some(index) = containing {
                    let _ = self.run_command(
                        "timeline.setPosterFrame",
                        json!({"timeline": id.0, "chapter": index, "time": seconds_to_ticks(time)}),
                    );
                }
                ui.close();
            }
            if ui.button(tl!("Add Chapter Points at Interval…")).clicked() {
                self.chapter_interval = Some(crate::context_menus::ChapterInterval {
                    timeline: id,
                    minutes: 5.0,
                    duration,
                });
                ui.close();
            }
            ui.separator();
            if crate::actions::menu_check(
                ui,
                crate::actions::Action::ShowChapterNames,
                self.show_chapter_names,
            ) {
                self.show_chapter_names = !self.show_chapter_names;
                ui.close();
            }
            ui.separator();
            if ui.button(tl!("Preview from Here")).clicked() {
                // 再生ヘッドはこのフレームの終わりに `interaction` から書き戻されるので、そちらも動かす。
                interaction.playhead = time;
                set_playhead_seconds(ui.ctx(), id, time);
                if !self.is_playing() {
                    self.toggle_playback(ui.ctx());
                }
                ui.close();
            }
        });
    }

    /// A subtitle clip pressed, dragged or let go of in the timeline (Encore: moving a subtitle clip along its
    /// track, or trimming it by an edge): whether the pointer was the subtitle clip's (nothing else then takes it).
    fn handle_cue_drag(
        &mut self,
        ui: &Ui,
        frame: &TimelineFrame<'_>,
        response: &Response,
        marker_drag: bool,
    ) -> bool {
        let pointer = ui.input(|input| input.pointer.interact_pos());
        // A subtitle clip being dragged (Encore: moving a subtitle clip along its track, or trimming it by an edge).
        // On release it goes where the outline is, through `subtitle.setCue` (one undo step).
        let cue_key = timeline_key("cue_drag", frame.snapshot.id);
        if let Some(state) = ui.data(|data| data.get_temp::<CueDragState>(cue_key)) {
            let (track, cue, grip, pressed_x, _) = state;
            let now_x = pointer.map_or(pressed_x, |point| point.x);
            let state = (track, cue, grip, pressed_x, now_x);
            if ui.input(|input| input.pointer.any_released()) {
                ui.data_mut(|data| data.remove::<CueDragState>(cue_key));
                // A press and release without moving is a click (it selects the clip), not a move.
                if (now_x - pressed_x).abs() >= 3.0
                    && let Some((_, _, start, end)) = self.cue_drag_result(frame, state)
                    && let Some(asset) = frame
                        .snapshot
                        .tracks
                        .iter()
                        .find(|row| row.subtitle && row.index == track)
                        .map(|row| row.asset)
                {
                    let _ = self.run_command(
                        "subtitle.setCue",
                        json!({
                            "asset": asset.0,
                            "cue": cue,
                            "start": seconds_to_ticks(start),
                            "end": seconds_to_ticks(end),
                        }),
                    );
                }
            } else {
                ui.ctx()
                    .set_cursor_icon(if CueGrip::of(grip) == CueGrip::Body {
                        egui::CursorIcon::Grabbing
                    } else {
                        egui::CursorIcon::ResizeHorizontal
                    });
                ui.data_mut(|data| data.insert_temp(cue_key, state));
                ui.ctx().request_repaint();
            }
            return true;
        }
        if let Some(point) = pointer
            && frame.canvas.contains(point)
            && response.contains_pointer()
            && !marker_drag
            && let Some((track, cue, grip)) = cue_grip_at(frame, point)
            // A chapter point's line runs through the clip: a press on it drags the chapter, as before.
            && (grip != CueGrip::Body
                || hit_marker(
                    &frame
                        .snapshot
                        .chapters
                        .iter()
                        .map(|chapter| time_to_x(chapter.time, frame.origin_x, frame.zoom))
                        .collect::<Vec<f32>>(),
                    point.x,
                    HIT_TOLERANCE,
                )
                .is_none())
        {
            if grip != CueGrip::Body {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if ui.input(|input| input.pointer.button_pressed(egui::PointerButton::Primary))
                && !ui.input(|input| input.pointer.any_released())
            {
                ui.data_mut(|data| {
                    data.insert_temp(cue_key, (track, cue, grip.code(), point.x, point.x));
                });
                return true;
            }
        }
        false
    }

    /// Handles pointer interaction with the timeline canvas.
    fn timeline_handle_input(
        &mut self,
        ui: &mut Ui,
        frame: &TimelineFrame<'_>,
        response: &Response,
        interaction: &mut TimelineInteraction,
    ) {
        let pointer = ui.input(|input| input.pointer.interact_pos());

        // A video clip's edge being dragged (Encore: trimming a clip by its edge). On release the engine moves
        // the in or out point to the nearest GOP start.
        let edge_key = timeline_key("clip_edge_drag", frame.snapshot.id);
        if let Some((clip, out_edge)) = ui.data(|data| data.get_temp::<(usize, bool)>(edge_key)) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            if ui.input(|input| input.pointer.any_released()) {
                ui.data_mut(|data| data.remove::<(usize, bool)>(edge_key));
                if let Some(point) = pointer {
                    let time = x_to_time(point.x, frame.origin_x, frame.zoom).max(0.0);
                    let _ = self.run_command(
                        "timeline.trimVideo",
                        json!({
                            "timeline": frame.snapshot.id,
                            "clip": clip,
                            "edge": if out_edge { "out" } else { "in" },
                            "time": seconds_to_ticks(time),
                        }),
                    );
                }
            }
            return;
        }
        // A video clip's body being dragged (Encore: moving a clip along the track). On release the engine puts
        // the clip there, by frames, between its neighbours.
        let move_key = timeline_key("clip_move_drag", frame.snapshot.id);
        if let Some((clip, pressed_x, _)) =
            ui.data(|data| data.get_temp::<(usize, f32, f32)>(move_key))
        {
            let now_x = pointer.map_or(pressed_x, |point| point.x);
            if ui.input(|input| input.pointer.any_released()) {
                ui.data_mut(|data| data.remove::<(usize, f32, f32)>(move_key));
                // A press and release without moving is a click, not a move: it chooses the clip.
                if (now_x - pressed_x).abs() < 3.0 {
                    self.pick_clip(frame.snapshot.id, ClipPick::Video(clip));
                }
                if (now_x - pressed_x).abs() >= 3.0
                    && let Some((start, _)) = clip_spans_of(frame).get(clip).copied()
                {
                    let shift = f64::from(now_x - pressed_x)
                        / (f64::from(BASE_SCALE) * f64::from(frame.zoom));
                    let _ = self.run_command(
                        "timeline.moveVideo",
                        json!({
                            "timeline": frame.snapshot.id,
                            "clip": clip,
                            "time": seconds_to_ticks((start + shift).max(0.0)),
                        }),
                    );
                }
            } else {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                ui.data_mut(|data| data.insert_temp(move_key, (clip, pressed_x, now_x)));
                ui.ctx().request_repaint();
            }
            return;
        }
        if self.handle_cue_drag(ui, frame, response, interaction.drag.is_some()) {
            return;
        }
        // An audio clip's edge being dragged (M6.254: trimming its in or out point). On release the engine trims
        // it to the nearest frame.
        let audio_edge_key = timeline_key("audio_edge_drag", frame.snapshot.id);
        if let Some((track, clip, out_edge)) =
            ui.data(|data| data.get_temp::<(usize, usize, bool)>(audio_edge_key))
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            if ui.input(|input| input.pointer.any_released()) {
                ui.data_mut(|data| data.remove::<(usize, usize, bool)>(audio_edge_key));
                if let Some(point) = pointer {
                    let time = x_to_time(point.x, frame.origin_x, frame.zoom).max(0.0);
                    let _ = self.run_command(
                        "timeline.trimAudio",
                        json!({
                            "timeline": frame.snapshot.id,
                            "track": track,
                            "clip": clip,
                            "edge": if out_edge { "out" } else { "in" },
                            "time": seconds_to_ticks(time),
                        }),
                    );
                }
            }
            return;
        }
        if let Some(point) = pointer
            && frame.canvas.contains(point)
            && response.contains_pointer()
            && interaction.drag.is_none()
            && let Some(edge) = audio_edge_at(frame, point)
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            if ui.input(|input| input.pointer.button_pressed(egui::PointerButton::Primary)) {
                ui.data_mut(|data| data.insert_temp(audio_edge_key, edge));
                return;
            }
        }
        // An audio track's clip being dragged (Encore: moving an audio clip along its track). On release the engine
        // starts its sound at the nearest frame.
        let audio_key = timeline_key("audio_move_drag", frame.snapshot.id);
        if let Some((track, clip, pressed_x, _)) =
            ui.data(|data| data.get_temp::<(usize, usize, f32, f32)>(audio_key))
        {
            let now_x = pointer.map_or(pressed_x, |point| point.x);
            if ui.input(|input| input.pointer.any_released()) {
                ui.data_mut(|data| data.remove::<(usize, usize, f32, f32)>(audio_key));
                let start = frame
                    .snapshot
                    .tracks
                    .iter()
                    .find(|row| !row.subtitle && row.index == track)
                    .and_then(|row| {
                        clip_boxes(row, frame.duration)
                            .into_iter()
                            .find(|found| found.number == clip)
                            .map(|found| found.start)
                    });
                // A press and release without moving is a click, not a move: it chooses the clip.
                if (now_x - pressed_x).abs() < 3.0 {
                    self.pick_clip(frame.snapshot.id, ClipPick::Audio(track, clip));
                }
                if (now_x - pressed_x).abs() >= 3.0
                    && let Some(start) = start
                {
                    let shift = f64::from(now_x - pressed_x)
                        / (f64::from(BASE_SCALE) * f64::from(frame.zoom));
                    let _ = self.run_command(
                        "timeline.moveAudio",
                        json!({
                            "timeline": frame.snapshot.id,
                            "track": track,
                            "clip": clip,
                            "time": seconds_to_ticks((start + shift).max(0.0)),
                        }),
                    );
                }
            } else {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                ui.data_mut(|data| data.insert_temp(audio_key, (track, clip, pressed_x, now_x)));
                ui.ctx().request_repaint();
            }
            return;
        }
        if let Some(point) = pointer
            && frame.canvas.contains(point)
            && response.contains_pointer()
            && interaction.drag.is_none()
            && let Some((track, clip)) = audio_clip_at(frame, point)
            && ui.input(|input| input.pointer.button_pressed(egui::PointerButton::Primary))
            && !ui.input(|input| input.pointer.any_released())
        {
            ui.data_mut(|data| data.insert_temp(audio_key, (track, clip, point.x, point.x)));
            return;
        }
        if let Some(point) = pointer
            && frame.canvas.contains(point)
            && response.contains_pointer()
            && interaction.drag.is_none()
            && let Some(clip) = clip_body_at(frame, point)
            && ui.input(|input| input.pointer.button_pressed(egui::PointerButton::Primary))
            && !ui.input(|input| input.pointer.any_released())
            // A chapter point's line runs through the clip: a press on it drags the chapter, as before.
            && hit_marker(
                &frame
                    .snapshot
                    .chapters
                    .iter()
                    .map(|chapter| time_to_x(chapter.time, frame.origin_x, frame.zoom))
                    .collect::<Vec<f32>>(),
                point.x,
                HIT_TOLERANCE,
            )
            .is_none()
        {
            ui.data_mut(|data| data.insert_temp(move_key, (clip, point.x, point.x)));
            return;
        }
        if let Some(point) = pointer
            && frame.canvas.contains(point)
            && response.contains_pointer()
            && let Some(edge) = clip_edge_at(frame, point)
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            if ui.input(|input| input.pointer.button_pressed(egui::PointerButton::Primary)) {
                ui.data_mut(|data| data.insert_temp(edge_key, edge));
                return;
            }
        }

        if let Some(drag) = interaction.drag {
            if ui.input(|input| input.pointer.any_released()) {
                self.finish_marker_drag(frame, interaction, drag);
                return;
            }

            if let Some(point) = pointer {
                let time =
                    x_to_time(point.x, frame.origin_x, frame.zoom).clamp(0.0, frame.duration);
                interaction.drag = Some(MarkerDrag {
                    marker: drag.marker,
                    time,
                });
            }
            return;
        }

        let Some(point) = pointer else {
            return;
        };
        // 手前にポップアップ（右クリックメニューなど）があるときは、キャンバスの押下として扱わない。
        if !frame.canvas.contains(point) || !response.contains_pointer() {
            return;
        }

        let markers_x: Vec<f32> = frame
            .snapshot
            .chapters
            .iter()
            .map(|chapter| time_to_x(chapter.time, frame.origin_x, frame.zoom))
            .collect();
        let hit = hit_marker(&markers_x, point.x, HIT_TOLERANCE);

        if response.double_clicked() {
            interaction.selected = hit;
            return;
        }

        // 何もないところをドラッグすると、再生ヘッドがついてくる（スクラブ）。
        if response.dragged_by(egui::PointerButton::Primary)
            && !ui.input(|input| input.pointer.button_pressed(egui::PointerButton::Primary))
        {
            interaction.playhead =
                x_to_time(point.x, frame.origin_x, frame.zoom).clamp(0.0, frame.duration);
            return;
        }

        if ui.input(|input| input.pointer.button_pressed(egui::PointerButton::Primary)) {
            if let Some(index) = hit {
                interaction.selected = Some(index);
                // A press and release in the same frame is a click, not the start of a drag.
                let released = ui.input(|input| input.pointer.any_released());
                if index != 0
                    && !released
                    && let Some(chapter) = frame.snapshot.chapters.get(index)
                {
                    interaction.drag = Some(MarkerDrag {
                        marker: index,
                        time: chapter.time,
                    });
                }
                return;
            }

            interaction.playhead =
                x_to_time(point.x, frame.origin_x, frame.zoom).clamp(0.0, frame.duration);
            interaction.selected = None;
        }
    }

    /// Completes a marker drag and runs the move command when needed.
    fn finish_marker_drag(
        &mut self,
        frame: &TimelineFrame<'_>,
        interaction: &mut TimelineInteraction,
        drag: MarkerDrag,
    ) {
        interaction.drag = None;

        let Some(chapter) = frame.snapshot.chapters.get(drag.marker) else {
            return;
        };
        if (drag.time - chapter.time).abs() <= 0.001 {
            return;
        }

        let ticks = seconds_to_ticks(drag.time);
        let _ = self.run_command(
            "timeline.moveChapter",
            json!({
                "timeline": frame.snapshot.id,
                "chapter": drag.marker,
                "time": ticks
            }),
        );
    }
}

/// Reads the timeline playhead from egui temp memory.
fn timeline_playhead(ui: &Ui, id: TimelineId) -> f64 {
    let key = timeline_key("timeline_playhead", id.0);
    match ui.data(|data| data.get_temp::<f64>(key)) {
        Some(time) if time.is_finite() && time >= 0.0 => time,
        _ => 0.0,
    }
}

/// The playhead of timeline `id` in seconds (0 when it has not been moved yet). Used by the monitor.
pub(crate) fn playhead_seconds(ctx: &egui::Context, id: TimelineId) -> f64 {
    let key = timeline_key("timeline_playhead", id.0);
    match ctx.data(|data| data.get_temp::<f64>(key)) {
        Some(time) if time.is_finite() && time >= 0.0 => time,
        _ => 0.0,
    }
}

/// Move the playhead of timeline `id` (the monitor's transport buttons).
pub(crate) fn set_playhead_seconds(ctx: &egui::Context, id: TimelineId, seconds: f64) {
    let key = timeline_key("timeline_playhead", id.0);
    let seconds = if seconds.is_finite() {
        seconds.max(0.0)
    } else {
        0.0
    };
    ctx.data_mut(|data| data.insert_temp(key, seconds));
}

/// The name shown for a chapter: its own name, with the default "Chapter N" names (and empty names) in the display
/// language, like Encore's "チャプター N".
pub(crate) fn chapter_label(name: &str, index: usize) -> String {
    let number = if name.trim().is_empty() {
        Some(index.saturating_add(1).to_string())
    } else {
        name.strip_prefix("Chapter ")
            .filter(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
            .map(str::to_owned)
    };
    match number {
        Some(n) => tlf!("Chapter {n}", n = n),
        None => name.to_owned(),
    }
}

/// Selects a chapter point in the timeline viewer (for example after its In point moved it to another index).
pub(crate) fn set_selected_chapter(ctx: &egui::Context, id: TimelineId, index: Option<usize>) {
    ctx.data_mut(|data| data.insert_temp(timeline_key("timeline_selected", id.0), index));
}

/// Reads the selected chapter index from egui temp memory.
fn timeline_selected(ui: &Ui, id: TimelineId) -> Option<usize> {
    let key = timeline_key("timeline_selected", id.0);
    ui.data(|data| data.get_temp::<Option<usize>>(key))
        .unwrap_or_default()
}

/// Reads the active marker drag from egui temp memory.
fn timeline_drag(ui: &Ui, id: TimelineId) -> Option<MarkerDrag> {
    let key = timeline_key("timeline_drag", id.0);
    ui.data(|data| data.get_temp::<Option<MarkerDrag>>(key))
        .unwrap_or_default()
}

/// Stores timeline interaction state in egui temp memory.
fn timeline_store_state(ui: &mut Ui, id: TimelineId, interaction: &TimelineInteraction) {
    ui.data_mut(|data| {
        data.insert_temp(
            timeline_key("timeline_playhead", id.0),
            interaction.playhead,
        );
        data.insert_temp(
            timeline_key("timeline_selected", id.0),
            interaction.selected,
        );
        data.insert_temp(timeline_key("timeline_drag", id.0), interaction.drag);
    });
}

/// Draws the complete timeline canvas.
fn timeline_draw(painter: &egui::Painter, frame: &TimelineFrame<'_>) {
    draw_rows(painter, frame);
    draw_ruler(painter, frame);
    draw_video_bar(painter, frame);
    draw_track_bars(painter, frame);
    draw_markers(painter, frame);
    draw_playhead(painter, frame);
}

/// Draws the lanes behind each row, across the whole canvas (the Craft timeline's track background, a subtitle
/// lane in the caption colour), with a separator under each.
fn draw_rows(painter: &egui::Painter, frame: &TimelineFrame<'_>) {
    let canvas = frame.canvas;
    let band = |top: f32, bottom: f32| {
        Rect::from_min_max(
            egui::pos2(canvas.min.x, canvas.min.y + top),
            egui::pos2(canvas.max.x, canvas.min.y + bottom),
        )
    };
    painter.rect_filled(
        band(RULER_HEIGHT, canvas.height()),
        0.0,
        craft_timeline::style::TL_BG,
    );
    let separator = |rect: Rect| {
        painter.hline(
            rect.x_range(),
            rect.max.y - 0.5,
            Stroke::new(1.0, craft_timeline::style::SEPARATOR),
        );
    };
    separator(band(VIDEO_TOP, VIDEO_TOP + VIDEO_HEIGHT));
    for (track, (top, height)) in frame
        .snapshot
        .tracks
        .iter()
        .zip(track_rows_geometry(&frame.snapshot.tracks))
    {
        let lane = band(top, top + height);
        if track.subtitle {
            painter.rect_filled(lane, 0.0, craft_timeline::style::CAPTION_LANE);
        }
        separator(lane);
    }
}

/// Draws the time ruler as Encore lays it out (M6.475; the strides are the Craft apps', FilmCraft's): timecodes far
/// enough apart (every label on a tick), the ticks hanging over a light line, and under it a dark band for the
/// chapter flags, the ticks reaching into it as far again.
fn draw_ruler(painter: &egui::Painter, frame: &TimelineFrame<'_>) {
    let fps = frames_per_second(frame.snapshot.fps_num, frame.snapshot.fps_den);
    if fps <= 0.0 {
        return;
    }
    let rate = rate_of(frame.snapshot.fps_num, frame.snapshot.fps_den);
    let ruler = Rect::from_min_max(
        frame.canvas.min,
        egui::pos2(frame.canvas.max.x, frame.canvas.min.y + RULER_HEIGHT),
    );
    painter.rect_filled(ruler, 0.0, craft_timeline::style::TL_BG);
    let band = flag_band(ruler);
    painter.rect_filled(band, 0.0, FLAG_BAND_FILL);
    // The light line the ticks hang from, and a dimmer one under it (Encore: 179 and 148).
    let line = band.min.y - 2.0;
    painter.hline(ruler.x_range(), line + 0.5, Stroke::new(1.0, RULER_LINE));
    painter.hline(
        ruler.x_range(),
        line + 1.5,
        Stroke::new(1.0, RULER_LINE_DIM),
    );
    let pps = f64::from(BASE_SCALE) * f64::from(frame.zoom);
    let (minor, per_label) = encore_ruler_steps(pps / fps);
    let label_y = line - 18.0;
    let at_frame = |f: i64| frame.origin_x + (f as f64 / fps * pps) as f32;
    // `as` saturates; the frames only bound the loop.
    let first = (((f64::from(ruler.min.x - frame.origin_x)) / pps * fps).floor() as i64).max(0);
    let last = ((f64::from(ruler.max.x - frame.origin_x)) / pps * fps).ceil() as i64 + 1;
    let mut f = (first / minor) * minor;
    let mut count = 0_u32;
    while f <= last && count < 10_000 {
        let x = at_frame(f);
        let tick = f / minor;
        let major = tick % per_label == 0;
        let h = tick_height(tick);
        painter.line_segment(
            [egui::pos2(x, line - h), egui::pos2(x, line)],
            Stroke::new(1.0, craft_timeline::style::RULER_TICK),
        );
        painter.line_segment(
            [egui::pos2(x, band.min.y), egui::pos2(x, band.min.y + h)],
            Stroke::new(1.0, RULER_TICK_IN_BAND),
        );
        if major {
            painter.text(
                egui::pos2(x + 1.0, label_y),
                egui::Align2::CENTER_CENTER,
                format_frames(f, rate),
                egui::FontId::proportional(11.0),
                craft_timeline::style::RULER_TEXT.get(),
            );
        }
        f = f.saturating_add(minor);
        count += 1;
    }
}

/// The ruler's ticks as Encore spaces them (M6.479, measured: a tick every 8 px, a timecode every 32 ticks, 256 px):
/// `(frames between ticks, ticks between timecodes)` for `frame_px` pixels a frame. The ticks stay on whole frames;
/// zoomed in past 8 px a frame, there is a tick on every frame and the timecodes stay about 256 px apart (on a long
/// tick).
fn encore_ruler_steps(frame_px: f64) -> (i64, i64) {
    const TICK_PX: f64 = 8.0;
    const LABEL_PX: f64 = 256.0;
    if !(frame_px.is_finite() && frame_px > 0.0) {
        return (1, 32);
    }
    // `as` saturates; a frame count on a ruler is small.
    let minor = ((TICK_PX / frame_px).round() as i64).max(1);
    // Minor ticks are frame_px * minor apart; a timecode every few long ticks (every 4 ticks), 256 px apart.
    let tick_px = frame_px * minor as f64;
    let long_ticks = ((LABEL_PX / (tick_px * 4.0)).round() as i64).clamp(1, 8);
    (minor, long_ticks * 4)
}

/// A tick's height from its number (Encore: 8 every fourth tick, 6 every other one, 4 for the rest).
fn tick_height(tick: i64) -> f32 {
    if tick % 4 == 0 {
        8.0
    } else if tick % 2 == 0 {
        6.0
    } else {
        4.0
    }
}

/// Where a video clip from `start` to `end` shows a picture (M6.483, Encore): its head, then each chapter point in
/// it (sorted; a point within `near` seconds of the one before, or of the clip's ends, adds none).
fn picture_starts(start: f64, end: f64, chapters: &[f64], near: f64) -> Vec<f64> {
    let mut inside: Vec<f64> = chapters
        .iter()
        .copied()
        .filter(|time| time.is_finite() && *time > start + near && *time < end - near)
        .collect();
    inside.sort_by(f64::total_cmp);
    let mut starts = vec![start];
    for time in inside {
        if starts.last().is_none_or(|last| time - last > near) {
            starts.push(time);
        }
    }
    starts
}

/// A small speaker (DiscCraft's own drawing): the box and cone, and two sound waves when `on`.
fn paint_speaker(painter: &egui::Painter, rect: Rect, color: egui::Color32, on: bool) {
    let (w, h) = (rect.width(), rect.height());
    let at = |x: f32, y: f32| egui::pos2(rect.min.x + w * x, rect.min.y + h * y);
    let _ = painter.add(egui::Shape::convex_polygon(
        vec![
            at(0.0, 0.35),
            at(0.25, 0.35),
            at(0.55, 0.1),
            at(0.55, 0.9),
            at(0.25, 0.65),
            at(0.0, 0.65),
        ],
        color,
        egui::Stroke::NONE,
    ));
    if on {
        let stroke = egui::Stroke::new(1.2, color);
        let _ = painter.line_segment([at(0.7, 0.35), at(0.7, 0.65)], stroke);
        let _ = painter.line_segment([at(0.9, 0.2), at(0.9, 0.8)], stroke);
    }
}

/// Where every video clip plays, as (start, end) in seconds: the measured spans, or one clip over the whole
/// duration.
fn clip_spans_of(frame: &TimelineFrame<'_>) -> Vec<(f64, f64)> {
    if frame.snapshot.spans.is_empty() {
        vec![(0.0, frame.duration)]
    } else {
        frame.snapshot.spans.clone()
    }
}

/// The video clip whose body is under `point` (not its edges, which trim it).
fn clip_body_at(frame: &TimelineFrame<'_>, point: egui::Pos2) -> Option<usize> {
    let top = frame.canvas.min.y + VIDEO_TOP;
    if point.y < top || point.y > top + VIDEO_HEIGHT || clip_edge_at(frame, point).is_some() {
        return None;
    }
    let seconds = x_to_time(point.x, frame.origin_x, frame.zoom);
    if point.x < frame.origin_x {
        return None;
    }
    clip_spans_of(frame)
        .iter()
        .position(|(start, end)| seconds >= *start && seconds < *end)
}

/// The video clip edge under `point`, as (clip, whether it is the clip's out edge): within a few points of a
/// clip's end from inside it, or of its start from inside it, on the video bar.
fn clip_edge_at(frame: &TimelineFrame<'_>, point: egui::Pos2) -> Option<(usize, bool)> {
    const REACH: f32 = 5.0;
    let top = frame.canvas.min.y + VIDEO_TOP;
    if point.y < top || point.y > top + VIDEO_HEIGHT {
        return None;
    }
    for (clip, (start, end)) in clip_spans_of(frame).iter().enumerate() {
        let (left, right) = (
            time_to_x(*start, frame.origin_x, frame.zoom),
            time_to_x(*end, frame.origin_x, frame.zoom),
        );
        if point.x <= right && right - point.x <= REACH {
            return Some((clip, true));
        }
        if point.x >= left && point.x - left <= REACH {
            return Some((clip, false));
        }
    }
    None
}

/// A video clip's ground and the name on it, its lower edge, and the line between its pictures (Encore, measured:
/// (161,179,192), black, (135,144,153) and 206; the same in both looks).
const VIDEO_CLIP: Color32 = Color32::from_rgb(161, 179, 192);
const VIDEO_CLIP_NAME: Color32 = Color32::BLACK;
const VIDEO_CLIP_EDGE: Color32 = Color32::from_rgb(135, 144, 153);
const PICTURE_LINE: Color32 = Color32::from_gray(206);

/// Draws the video clips as Encore does (M6.483): a light blue-grey clip with its file's name in black, and under
/// the name the frame at the clip's head and at each chapter point, each from its place to the next (cut off where
/// the next begins; the clip's ground shows where a stretch is wider than its picture), a light line between them,
/// and the last clip's last frame at its end. The corner marks show where the clip reaches the media's first and
/// last frame.
fn draw_video_bar(painter: &egui::Painter, frame: &TimelineFrame<'_>) {
    let origin_x = frame.origin_x;
    let spans = clip_spans_of(frame);
    let top = frame.canvas.min.y + VIDEO_TOP + 1.0;
    let bottom = frame.canvas.min.y + VIDEO_TOP + VIDEO_HEIGHT - 1.0;
    let names = std::iter::once(frame.snapshot.asset_name.as_str()).chain(
        frame
            .snapshot
            .more_video
            .iter()
            .map(|(_, name)| name.as_str()),
    );
    let last = spans.len().saturating_sub(1);
    // One body per clip (Encore: the clips on the video track, with the gaps between them left empty).
    for (index, ((start, end), name)) in spans.iter().zip(names).enumerate() {
        let left = time_to_x(*start, origin_x, frame.zoom);
        let right = time_to_x(*end, origin_x, frame.zoom);
        if right <= left {
            continue;
        }
        let body = Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom));
        painter.rect_filled(body, 0.0, VIDEO_CLIP);
        painter.hline(
            body.x_range(),
            body.max.y - 0.5,
            Stroke::new(1.0, VIDEO_CLIP_EDGE),
        );
        let strip = Rect::from_min_max(
            egui::pos2(body.min.x + 1.0, body.min.y + craft_timeline::NAME_BAND),
            egui::pos2(body.max.x - 1.0, body.max.y - 1.0),
        );
        let pictures = frame.pictures.get(index).map_or(&[][..], Vec::as_slice);
        let mut covered = strip.min.x;
        for (n, (at, texture)) in pictures.iter().enumerate() {
            let x = time_to_x(*at, origin_x, frame.zoom)
                .max(strip.min.x)
                .round();
            let next = pictures.get(n + 1).map_or(strip.max.x, |(time, _)| {
                time_to_x(*time, origin_x, frame.zoom)
            });
            if next < frame.canvas.min.x || x > frame.canvas.max.x {
                continue;
            }
            if n > 0 {
                painter.vline(x - 0.5, strip.y_range(), Stroke::new(1.0, PICTURE_LINE));
            }
            if let Some(texture) = texture {
                let size = texture.size_vec2();
                let width = strip.height() * size.x / size.y.max(1.0);
                let place = Rect::from_min_size(
                    egui::pos2(x, strip.min.y),
                    egui::vec2(width, strip.height()),
                );
                let room = Rect::from_min_max(
                    egui::pos2(x, strip.min.y),
                    egui::pos2((next - 1.0).min(strip.max.x), strip.max.y),
                );
                painter
                    .with_clip_rect(room.intersect(painter.clip_rect()))
                    .image(
                        texture.id(),
                        place,
                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                covered = covered.max(place.max.x.min(room.max.x));
            }
        }
        // The last clip's last frame at its end, when the stretch before it leaves room (Encore: the clip's head,
        // its chapter points and its end).
        if index == last
            && let Some(texture) = &frame.last
        {
            let size = texture.size_vec2();
            let width = strip.height() * size.x / size.y.max(1.0);
            if strip.max.x - width > covered + 4.0 {
                let at = Rect::from_min_size(
                    egui::pos2(strip.max.x - width, strip.min.y),
                    egui::vec2(width, strip.height()),
                );
                crate::project_panel::draw_fitted(painter, at, texture);
            }
        }
        craft_timeline::clip_name_and_edges(painter, body, name, (true, true), VIDEO_CLIP_NAME);
        // The chosen clip: outlined.
        if frame.picked == Some(ClipPick::Video(index)) {
            let _ = painter.rect_stroke(
                body,
                2.0,
                egui::Stroke::new(2.0, Color32::WHITE),
                egui::StrokeKind::Inside,
            );
        }
    }
    // A clip being dragged along the track (Encore: moving a clip): an outline where it would start.
    if let Some((clip, shift)) = frame.clip_move
        && let Some((start, end)) = spans.get(clip)
    {
        let ghost = Rect::from_min_max(
            egui::pos2(
                time_to_x((start + shift).max(0.0), origin_x, frame.zoom),
                top,
            ),
            egui::pos2(
                time_to_x((end + shift).max(0.0), origin_x, frame.zoom),
                bottom,
            ),
        );
        let _ = painter.rect_stroke(
            ghost,
            2.0,
            egui::Stroke::new(1.5, craft_timeline::style::RULER_TEXT),
            egui::StrokeKind::Inside,
        );
    }
}

/// Height of the ruler band at the top of the canvas (the Craft ruler: markers row, timecodes, ticks), as tall as
/// the header column's current time and toolbar.
const RULER_HEIGHT: f32 = 56.0;
/// Top of the video bar, relative to the canvas.
const VIDEO_TOP: f32 = RULER_HEIGHT;
/// Height of the video bar (taller than the audio and subtitle bars, to show the clip's frames).
const VIDEO_HEIGHT: f32 = 56.0;
/// Top of the first track row (below the chapter labels), relative to the canvas.
const TRACKS_TOP: f32 = VIDEO_TOP + VIDEO_HEIGHT + 18.0;
/// Height of an audio track row (room for its waveform) and of a subtitle track row (the Craft caption row).
const AUDIO_ROW: f32 = 40.0;
const SUBTITLE_ROW: f32 = 28.0;

/// The top (relative to the canvas) and height of each audio and subtitle track row, in order.
fn track_rows_geometry(tracks: &[TrackRow]) -> Vec<(f32, f32)> {
    let mut top = TRACKS_TOP;
    tracks
        .iter()
        .map(|track| {
            let height = if track.subtitle {
                SUBTITLE_ROW
            } else {
                AUDIO_ROW
            };
            let row = (top, height);
            top += height;
            row
        })
        .collect()
}

/// A track header row as Encore draws it (M6.478, measured): a lighter grey than the timeline (102), a dark line
/// along the top with a blue-grey one under it and a dark line along the bottom, and the name in light grey after
/// the mark box (the Craft badge, V1 / A1 / S1, is gone since M6.474).
fn header_row(painter: &egui::Painter, row: Rect, name: &str) {
    painter.rect_filled(row, 0.0, HEADER_ROW);
    painter.hline(
        row.x_range(),
        row.min.y + 0.5,
        Stroke::new(1.0, HEADER_ROW_EDGE),
    );
    painter.hline(
        row.x_range(),
        row.min.y + 1.5,
        Stroke::new(1.0, HEADER_ROW_TOP),
    );
    painter.hline(
        row.x_range(),
        row.max.y - 0.5,
        Stroke::new(1.0, HEADER_ROW_EDGE),
    );
    painter.with_clip_rect(row.shrink(1.0)).text(
        egui::pos2(row.min.x + MARK_NAME_X, row.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(11.0),
        HEADER_ROW_TEXT.get(),
    );
}

/// A track header row's ground, its dark top and bottom lines, the blue-grey line under the top, and the name
/// (Encore: 102, 54, (85,110,110) and 217).
const HEADER_ROW: crate::theme::Shade = crate::theme::Shade::gray(102, 209);
const HEADER_ROW_EDGE: crate::theme::Shade = crate::theme::Shade::gray(54, 179);
const HEADER_ROW_TOP: crate::theme::Shade = crate::theme::Shade::new(
    Color32::from_rgb(85, 110, 110),
    Color32::from_rgb(174, 199, 199),
);
const HEADER_ROW_TEXT: crate::theme::Shade = crate::theme::Shade::gray(217, 40);

/// Centre of the mark box from a header's left edge, the box's size, and where the track's name starts (Encore:
/// the box at 15–30, the name from 37).
const MARK_CENTER_X: f32 = 22.0;
const MARK_SIZE: egui::Vec2 = egui::vec2(18.0, 16.0);
const MARK_NAME_X: f32 = 36.0;
/// The mark box, etched into the header (Encore: a light edge outside, a dark one inside, the ground a little
/// darker than the header; M6.478). Lighter under the pointer when the box does something.
const MARK_FILL: crate::theme::Shade = crate::theme::Shade::gray(99, 205);
const MARK_FILL_HOVER: crate::theme::Shade = crate::theme::Shade::gray(116, 222);
const MARK_OUTER: crate::theme::Shade = crate::theme::Shade::gray(122, 240);
const MARK_INNER: crate::theme::Shade = crate::theme::Shade::gray(84, 165);

/// The box at the left of a track header that holds its mark, centred on the row.
fn mark_box(row: Rect) -> Rect {
    Rect::from_center_size(
        egui::pos2(row.min.x + MARK_CENTER_X, row.center().y),
        MARK_SIZE,
    )
}

/// Where the mark is drawn inside its box (an icon of the Craft set is drawn on a square).
fn mark_icon(mark: Rect) -> Rect {
    Rect::from_center_size(mark.center(), egui::vec2(14.0, 14.0))
}

/// Width the ◂ ▸ take at the right of a track header.
const STEP_ARROWS_WIDTH: f32 = 26.0;
/// The ◂ ▸ when they go somewhere and when there is nowhere to go (Encore: 202 and 132; M6.478).
const STEP_ARROW: crate::theme::Shade = crate::theme::Shade::gray(202, 70);
const STEP_ARROW_DIM: crate::theme::Shade = crate::theme::Shade::gray(132, 168);

/// Encore's ◂ ▸ at the right of a track header (Previous Clip, Next Clip): `Some(false)` when ◂ was clicked,
/// `Some(true)` for ▸. `can` says whether each (◂, ▸) has somewhere to go; one that has not is dim and does
/// nothing, as in Encore (◂ at the head of the timeline). Under the pointer, Encore's raised box with the
/// triangle gold. The triangles are plain shapes (DiscCraft's own drawing).
fn step_arrows(
    ui: &mut Ui,
    painter: &egui::Painter,
    row: Rect,
    salt: (u64, u8, usize),
    can: (bool, bool),
) -> Option<bool> {
    let mut clicked = None;
    for (forward, enabled, from_right, tip) in [
        (false, can.0, 19.0, tl!("Previous Clip")),
        (true, can.1, 8.0, tl!("Next Clip")),
    ] {
        let hit = Rect::from_center_size(
            egui::pos2(row.max.x - from_right, row.center().y),
            egui::vec2(11.0, 16.0),
        );
        let response = ui
            .interact(
                hit,
                Id::unique(("clip_step", forward)).with(salt),
                if enabled {
                    egui::Sense::click()
                } else {
                    egui::Sense::hover()
                },
            )
            .on_hover_text(tip);
        let hovered = enabled && response.hovered();
        if hovered {
            crate::theme::hover_box(painter, hit);
        }
        let color = if hovered {
            crate::theme::HOVER_ICON.get()
        } else if enabled {
            STEP_ARROW.get()
        } else {
            STEP_ARROW_DIM.get()
        };
        let (x, y) = (hit.center().x, hit.center().y);
        let (back, front) = if forward { (-2.5, 3.0) } else { (2.5, -3.0) };
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(x + back, y - 5.5),
                egui::pos2(x + front, y),
                egui::pos2(x + back, y + 5.5),
            ],
            color,
            Stroke::NONE,
        ));
        if enabled && response.clicked() {
            clicked = Some(forward);
        }
    }
    clicked
}

/// Whether ◂ and ▸ have somewhere to go from `playhead` on a track whose clips are `spans`.
fn step_reach(spans: &[(f64, f64)], playhead: f64) -> (bool, bool) {
    (
        clip_step(spans, playhead, false).is_some(),
        clip_step(spans, playhead, true).is_some(),
    )
}

/// An audio track's clips as (start, end, number), in order of their start.
fn sorted_audio_boxes(track: &TrackRow, duration: f64) -> Vec<(f64, f64, usize)> {
    let mut boxes: Vec<(f64, f64, usize)> = clip_boxes(track, duration)
        .iter()
        .map(|clip| (clip.start, clip.end, clip.number))
        .collect();
    boxes.sort_by(|a, b| a.0.total_cmp(&b.0));
    boxes
}

/// What ◂ ▸ step through on a track row: a subtitle track's subtitles, an audio track's clips (in order).
fn track_step_spans(track: &TrackRow, duration: f64) -> Vec<(f64, f64)> {
    if track.subtitle {
        track
            .cues
            .as_deref()
            .map(|cues| cues.iter().map(|cue| (cue.start, cue.end)).collect())
            .unwrap_or_default()
    } else {
        sorted_audio_boxes(track, duration)
            .iter()
            .map(|(start, end, _)| (*start, *end))
            .collect()
    }
}

/// Encore's Previous Clip / Next Clip on a track whose clips are `spans` ((start, end) in seconds): where the
/// playhead goes from `playhead`, and the index of the clip chosen there. Next: the first clip starting after the
/// playhead; past the last one, the end of the last clip with no clip chosen. Previous: the last clip starting
/// before the playhead (from inside a clip, that clip's start). `None` when there is nowhere to go. Measured in
/// Encore CS6 (encore-behavior.md 9.28).
fn clip_step(spans: &[(f64, f64)], playhead: f64, forward: bool) -> Option<(f64, Option<usize>)> {
    // Within this much of a clip's start, the playhead is at it (a frame is far longer).
    const NEAR: f64 = 1e-4;
    let clips = spans
        .iter()
        .enumerate()
        .filter(|(_, (start, end))| start.is_finite() && end.is_finite());
    if forward {
        if let Some((index, (start, _))) = clips
            .clone()
            .filter(|(_, (start, _))| *start > playhead + NEAR)
            .min_by(|a, b| a.1.0.total_cmp(&b.1.0))
        {
            return Some((*start, Some(index)));
        }
        let end = clips.map(|(_, (_, end))| *end).max_by(f64::total_cmp)?;
        (end > playhead + NEAR).then_some((end, None))
    } else {
        clips
            .filter(|(_, (start, _))| *start < playhead - NEAR)
            .max_by(|a, b| a.1.0.total_cmp(&b.1.0))
            .map(|(index, (start, _))| (*start, Some(index)))
    }
}

/// The mark box: etched into the header, lighter while the pointer is over a box that does something.
fn paint_mark_box(painter: &egui::Painter, mark: Rect, hovered: bool) {
    use egui::emath::GuiRounding as _;
    let mark = mark.round_to_pixels(painter.pixels_per_point());
    let fill = if hovered { MARK_FILL_HOVER } else { MARK_FILL };
    painter.rect_filled(mark, 0.0, fill);
    painter.rect_stroke(
        mark,
        0.0,
        Stroke::new(1.0, MARK_OUTER),
        egui::StrokeKind::Inside,
    );
    painter.rect_stroke(
        mark.shrink(1.0),
        0.0,
        Stroke::new(1.0, MARK_INNER),
        egui::StrokeKind::Inside,
    );
}
/// Width of the track header column.
const HEADER_WIDTH: f32 = 200.0;
/// Where the timeline's and the source's times start in the header (Encore: 18 and 96 of its 216), and the size of
/// the tools at its bottom right.
const HEADER_TIME_X: f32 = 10.0;
const HEADER_SOURCE_X: f32 = 92.0;
const HEADER_TOOL: f32 = 22.0;
/// The languages offered in a track header (ISO 639-1), like the list in Encore. A track's own language is kept
/// in the list even when it is not one of these.
pub(crate) const LANGUAGES: &[&str] = &[
    "ja", "en", "zh", "ko", "fr", "de", "es", "it", "pt", "ru", "nl", "sv", "no", "da", "fi", "pl",
    "th",
];

/// Canvas height for the audio and subtitle rows `tracks`.
fn canvas_height(tracks: &[TrackRow]) -> f32 {
    let rows: f32 = track_rows_geometry(tracks)
        .iter()
        .map(|(_, height)| height)
        .sum();
    (TRACKS_TOP + rows + 8.0).max(CANVAS_HEIGHT)
}

/// The language list of a track header. Returns the language chosen this frame.
fn language_list(
    ui: &mut Ui,
    rect: Rect,
    id: TimelineId,
    track: &TrackRow,
    fallback: &str,
) -> Option<String> {
    let mut chosen = None;
    let salt = (id.0, track.subtitle, track.index);
    // Without a language of its own the track shows the one it gets (`fallback`), or "--".
    let shown = match (track.language.is_empty(), fallback.is_empty()) {
        (false, _) => track.language.clone(),
        (true, false) => fallback.to_owned(),
        (true, true) => "--".to_owned(),
    };
    // 親の行の並びを動かさない子の Ui に置く（行のカーソルが戻って、キャンバスが見出しに重なるのを防ぐ）。
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    let _ = egui::ComboBox::from_id_salt(("track_language", salt))
        .width(rect.width() - 8.0)
        .selected_text(shown)
        .show_ui(&mut child, |ui| {
            let own = (!track.language.is_empty() && !LANGUAGES.contains(&track.language.as_str()))
                .then_some(track.language.as_str());
            for code in own.into_iter().chain(LANGUAGES.iter().copied()) {
                if ui.selectable_label(track.language == code, code).clicked()
                    && track.language != code
                {
                    chosen = Some(code.to_owned());
                }
            }
        });
    chosen
}

/// How long an audio clip is, seconds: its file's length (`None` while unknown) less what its in point leaves
/// out, or up to its trimmed out point.
fn audio_clip_length(file: Option<f64>, trim_start: f64, trim_end: Option<f64>) -> Option<f64> {
    let end = match (trim_end, file) {
        (Some(end), Some(file)) => end.min(file),
        (Some(end), None) => end,
        (None, Some(file)) => file,
        (None, None) => return None,
    };
    Some((end - trim_start).max(0.0))
}

/// Where Timeline ▸ Move Track to View leaves the vertical scroll it asks the timeline's rows for.
fn track_view_key(timeline: u64) -> egui::Id {
    timeline_key("timeline_track_view", timeline)
}

/// How far down the timeline's rows audio track `track` starts (the rows above it: the ruler, the video, and the
/// audio tracks before it), points.
pub(crate) fn audio_track_top(track: usize) -> f32 {
    // `as` is exact for the track counts a timeline holds.
    TRACKS_TOP + AUDIO_ROW * track as f32
}

impl DiscCraftApp {
    /// Timeline ▸ Move Track to View: the timeline in the viewer scrolls down (or up) to the audio track that is
    /// heard (the speaker in its header), so its row is in sight.
    pub(crate) fn move_track_to_view(&mut self, ctx: &egui::Context) {
        let Some(id) = self.viewer_timeline() else {
            return;
        };
        let track = self.heard_audio(id).unwrap_or(0);
        // A little of the row above stays in sight.
        let offset = (audio_track_top(track) - 8.0).max(0.0);
        ctx.data_mut(|data| data.insert_temp(track_view_key(id.0), offset));
        self.show_panel(crate::workspace::Panel::TimelineViewer);
    }
}

/// Where the context menu keeps the audio row and clip it was opened on.
/// Where a dragged Project panel row would go on an audio track (see `note_drop_target`).
fn drop_target_key(timeline: u64) -> egui::Id {
    timeline_key("timeline_drop_target", timeline)
}

fn audio_context_key(timeline: u64) -> egui::Id {
    timeline_key("timeline_context_audio", timeline)
}

/// The edge of an audio clip under `point` (within reach of it): the audio track's index, the clip's number on
/// it, and whether it is the clip's end (its out point) rather than its start.
fn audio_edge_at(frame: &TimelineFrame<'_>, point: egui::Pos2) -> Option<(usize, usize, bool)> {
    const REACH: f32 = 5.0;
    frame
        .snapshot
        .tracks
        .iter()
        .zip(track_rows_geometry(&frame.snapshot.tracks))
        .find_map(|(track, (top, height))| {
            let rows = (frame.canvas.min.y + top)..=(frame.canvas.min.y + top + height);
            if track.subtitle || !rows.contains(&point.y) {
                return None;
            }
            clip_boxes(track, frame.duration)
                .into_iter()
                .find_map(|clip| {
                    let (left, right) = (
                        time_to_x(clip.start, frame.origin_x, frame.zoom),
                        time_to_x(clip.end, frame.origin_x, frame.zoom),
                    );
                    if point.x <= right && right - point.x <= REACH {
                        Some((track.index, clip.number, true))
                    } else if point.x >= left && point.x - left <= REACH && point.x <= right {
                        Some((track.index, clip.number, false))
                    } else {
                        None
                    }
                })
        })
}

/// The audio track whose row is under `point` (its index among the audio tracks).
fn audio_row_at(frame: &TimelineFrame<'_>, point: egui::Pos2) -> Option<usize> {
    frame
        .snapshot
        .tracks
        .iter()
        .zip(track_rows_geometry(&frame.snapshot.tracks))
        .find_map(|(track, (top, height))| {
            let rows = (frame.canvas.min.y + top)..=(frame.canvas.min.y + top + height);
            (!track.subtitle && rows.contains(&point.y)).then_some(track.index)
        })
}

/// The audio clip under `point`: its track (the index among the audio tracks) and its number on the track.
fn audio_clip_at(frame: &TimelineFrame<'_>, point: egui::Pos2) -> Option<(usize, usize)> {
    if point.x < frame.origin_x {
        return None;
    }
    let seconds = x_to_time(point.x, frame.origin_x, frame.zoom);
    frame
        .snapshot
        .tracks
        .iter()
        .zip(track_rows_geometry(&frame.snapshot.tracks))
        .find_map(|(track, (top, height))| {
            let rows = (frame.canvas.min.y + top)..=(frame.canvas.min.y + top + height);
            if track.subtitle || !rows.contains(&point.y) {
                return None;
            }
            clip_boxes(track, frame.duration)
                .into_iter()
                .find(|clip| seconds >= clip.start && seconds < clip.end)
                .map(|clip| (track.index, clip.number))
        })
}

/// Draws one clip per audio track (the Craft audio clip colour, its name band) and each subtitle track's cues.
fn draw_track_bars(painter: &egui::Painter, frame: &TimelineFrame<'_>) {
    let origin_x = frame.origin_x;
    let right = time_to_x(frame.duration, origin_x, frame.zoom);
    if right <= origin_x {
        return;
    }

    for (track, (top, height)) in frame
        .snapshot
        .tracks
        .iter()
        .zip(track_rows_geometry(&frame.snapshot.tracks))
    {
        let lane = Rect::from_min_max(
            egui::pos2(origin_x, frame.canvas.min.y + top + 1.0),
            egui::pos2(right, frame.canvas.min.y + top + height - 1.0),
        );
        if let Some(cues) = &track.cues {
            draw_cues(painter, frame, lane, cues);
            // Being dragged: an outline where it would go.
            if let Some((moved, _, start, end)) = frame.cue_move
                && moved == track.index
            {
                let block = Rect::from_min_max(
                    egui::pos2(time_to_x(start, origin_x, frame.zoom), lane.min.y + 1.0),
                    egui::pos2(time_to_x(end, origin_x, frame.zoom), lane.max.y - 1.0),
                );
                let _ = painter.rect_stroke(
                    block,
                    3.0,
                    egui::Stroke::new(1.5, craft_timeline::style::RULER_TEXT),
                    egui::StrokeKind::Inside,
                );
            }
            continue;
        }
        if track.subtitle {
            continue;
        }
        // The audio clips: each where its sound starts (it may have been moved along the track), for its length.
        for clip in clip_boxes(track, frame.duration) {
            let lane = Rect::from_min_max(
                egui::pos2(time_to_x(clip.start, origin_x, frame.zoom), lane.min.y),
                egui::pos2(time_to_x(clip.end, origin_x, frame.zoom), lane.max.y),
            );
            if lane.width() <= 0.0 {
                continue;
            }
            craft_timeline::clip_body(painter, lane, craft_timeline::style::CLIP_AUDIO.get());
            // Each clip's own file's waveform.
            if let Some(peaks) = clip.peaks {
                let pps = f64::from(BASE_SCALE) * f64::from(frame.zoom);
                let (start, trim_start) = (clip.start, clip.trim_start);
                craft_timeline::draw_waveform(
                    painter,
                    lane,
                    peaks,
                    // The time in the file: the clip's head is its trimmed in point.
                    |x| f64::from(x - origin_x) / pps - start + trim_start,
                    craft_timeline::darker(craft_timeline::style::CLIP_AUDIO.get()),
                );
            }
            craft_timeline::clip_name_and_edges(
                painter,
                lane,
                clip.name,
                (true, true),
                craft_timeline::style::CLIP_NAME.get(),
            );
            // The chosen clip: outlined.
            if frame.picked == Some(ClipPick::Audio(track.index, clip.number)) {
                let _ = painter.rect_stroke(
                    lane,
                    2.0,
                    egui::Stroke::new(2.0, Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            }
            // Being dragged: an outline where it would start.
            if let Some((moved, number, shift)) = frame.audio_move
                && moved == track.index
                && number == clip.number
            {
                let by =
                    time_to_x((clip.start + shift).max(0.0), origin_x, frame.zoom) - lane.min.x;
                let _ = painter.rect_stroke(
                    lane.translate(egui::vec2(by, 0.0)),
                    2.0,
                    egui::Stroke::new(1.5, craft_timeline::style::RULER_TEXT),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }
}

/// A subtitle clip being dragged: its subtitle track, its number, the grip ([`CueGrip`] code), and the pointer's x
/// when pressed and now.
type CueDragState = (usize, usize, u8, f32, f32);

/// How near (points) an edge of a subtitle clip a press trims it instead of moving it.
const CUE_EDGE: f32 = 4.0;

/// How near (points) a dragged subtitle clip's edge snaps to a snap point.
const CUE_SNAP: f32 = 8.0;

/// The subtitle clip under `point` and where it is held: its subtitle track, its number and the grip (an edge
/// within [`CUE_EDGE`], else its body).
fn cue_grip_at(frame: &TimelineFrame<'_>, point: egui::Pos2) -> Option<(usize, usize, CueGrip)> {
    if point.x < frame.origin_x - CUE_EDGE {
        return None;
    }
    frame
        .snapshot
        .tracks
        .iter()
        .zip(track_rows_geometry(&frame.snapshot.tracks))
        .find_map(|(track, (top, height))| {
            let rows = (frame.canvas.min.y + top)..=(frame.canvas.min.y + top + height);
            let cues = track.cues.as_ref().filter(|_| rows.contains(&point.y))?;
            cues.iter().enumerate().find_map(|(number, cue)| {
                let left = time_to_x(cue.start, frame.origin_x, frame.zoom);
                let right = time_to_x(cue.end, frame.origin_x, frame.zoom);
                let grip = if (point.x - left).abs() <= CUE_EDGE && right - left > 3.0 * CUE_EDGE {
                    CueGrip::In
                } else if (point.x - right).abs() <= CUE_EDGE {
                    CueGrip::Out
                } else if point.x > left && point.x < right {
                    CueGrip::Body
                } else {
                    return None;
                };
                Some((track.index, number, grip))
            })
        })
}

/// The subtitle clip under `point`: its subtitle track (counted among the subtitle tracks) and its number.
fn cue_at_point(frame: &TimelineFrame<'_>, point: egui::Pos2) -> Option<(usize, usize)> {
    let seconds = x_to_time(point.x, frame.origin_x, frame.zoom);
    if point.x < frame.origin_x {
        return None;
    }
    frame
        .snapshot
        .tracks
        .iter()
        .zip(track_rows_geometry(&frame.snapshot.tracks))
        .find_map(|(track, (top, height))| {
            let rows = (frame.canvas.min.y + top)..=(frame.canvas.min.y + top + height);
            let cues = track.cues.as_ref().filter(|_| rows.contains(&point.y))?;
            let cue = cues
                .iter()
                .position(|cue| cue.start <= seconds && seconds < cue.end)?;
            Some((track.index, cue))
        })
}

/// Draws a subtitle track's cues as the Craft apps draw captions: a rounded block in the caption colour per cue, with
/// its first line.
fn draw_cues(
    painter: &egui::Painter,
    frame: &TimelineFrame<'_>,
    lane: Rect,
    cues: &[crate::subtitles::SubtitleCue],
) {
    let origin_x = frame.origin_x;
    let visible = frame.canvas.x_range();
    for cue in cues {
        let left = time_to_x(cue.start, origin_x, frame.zoom);
        let right = time_to_x(cue.end, origin_x, frame.zoom).max(left + 1.0);
        if right < visible.min || left > visible.max {
            continue;
        }
        let block = Rect::from_min_max(
            egui::pos2(left, lane.min.y + 1.0),
            egui::pos2(right, lane.max.y - 1.0),
        );
        painter.rect_filled(block, 3.0, craft_timeline::style::CAPTION);
        if block.width() > 14.0 {
            let text = crate::subtitles::cue_label(cue);
            painter
                .with_clip_rect(block.shrink2(egui::vec2(4.0, 0.0)))
                .text(
                    egui::pos2(block.min.x + 5.0, block.center().y),
                    egui::Align2::LEFT_CENTER,
                    text,
                    egui::FontId::proportional(11.0),
                    Color32::WHITE,
                );
        }
    }
}

/// Draws chapter markers.
fn draw_markers(painter: &egui::Painter, frame: &TimelineFrame<'_>) {
    let origin_x = frame.origin_x;
    // Encore's numbered flags in the ruler's bottom band (M6.475), a faint line through the video row.
    let ruler = Rect::from_min_max(
        frame.canvas.min,
        egui::pos2(frame.canvas.max.x, frame.canvas.min.y + RULER_HEIGHT),
    );
    let band = flag_band(ruler);
    let top = frame.canvas.min.y + RULER_HEIGHT;
    let bottom = frame.canvas.min.y + VIDEO_TOP + VIDEO_HEIGHT;

    for (index, chapter) in frame.snapshot.chapters.iter().enumerate() {
        let time = marker_time(frame.drag, index, chapter.time);
        let x = time_to_x(time, origin_x, frame.zoom);
        if x < frame.canvas.min.x - 20.0 || x > frame.canvas.max.x + 20.0 {
            continue;
        }

        let selected = frame.selected == Some(index);
        let dragging = match frame.drag {
            Some(drag) => drag.marker == index,
            None => false,
        };

        let color = if index == 0 {
            Color32::GRAY
        } else if selected {
            craft_timeline::style::SELECTED_BORDER.get()
        } else {
            craft_timeline::style::MARKER.get()
        };

        // Encore draws no line through the video row (its pictures change at each chapter point, M6.483); one shows
        // where a chapter point being dragged would go.
        if dragging {
            painter.line_segment(
                [egui::pos2(x, top), egui::pos2(x, bottom)],
                Stroke::new(1.5, color.gamma_multiply(0.9)),
            );
        }
        chapter_flag(painter, band, x, index, selected);

        // ポスターフレームは小さな絵の枠で（チャプターと同じ色）。ルーラーの上の端に。
        if let Some(poster) = chapter.poster {
            let px = time_to_x(poster, origin_x, frame.zoom);
            if px >= frame.canvas.min.x - 20.0 && px <= frame.canvas.max.x + 20.0 {
                let picture = egui::Rect::from_center_size(
                    egui::pos2(px, ruler.min.y + 6.0),
                    egui::vec2(8.0, 6.0),
                );
                let _ = painter.rect_stroke(
                    picture,
                    0.0,
                    Stroke::new(1.0, color),
                    egui::StrokeKind::Inside,
                );
            }
        }

        let label = if dragging {
            timecode(time, frame.snapshot.fps_num, frame.snapshot.fps_den)
        } else if frame.show_names {
            chapter_label(&chapter.name, index)
        } else {
            continue;
        };

        painter.text(
            egui::pos2(x + 4.0, bottom + 2.0),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::proportional(10.0),
            Color32::LIGHT_GRAY,
        );
    }
}

/// Draws the playhead as Encore does (M6.484): a yellow head on the ruler's tick row (above the flag band), an
/// orange-red line through the flag band and a red one down through the rows.
fn draw_playhead(painter: &egui::Painter, frame: &TimelineFrame<'_>) {
    let x = time_to_x(frame.playhead, frame.origin_x, frame.zoom);
    if x < frame.canvas.min.x - 1.0 || x > frame.canvas.max.x + 1.0 {
        return;
    }
    let ruler = Rect::from_min_max(
        frame.canvas.min,
        egui::pos2(frame.canvas.max.x, frame.canvas.min.y + RULER_HEIGHT),
    );
    let ticks = Rect::from_min_max(ruler.min, egui::pos2(ruler.max.x, flag_band(ruler).min.y));
    craft_timeline::draw_playhead(painter, x, ticks, ruler.max.y, frame.canvas.max.y);
}

/// Height of the ruler's bottom band, where the chapter flags hang (Encore: 16).
const FLAG_BAND: f32 = 16.0;
/// The band's fill, the light line over it and the dimmer line under that, the ticks reaching into the band
/// (Encore's greys 41, 179, 148 and 148, set against DiscCraft's darker timeline).
const FLAG_BAND_FILL: crate::theme::Shade = crate::theme::Shade::gray(34, 153);
const RULER_LINE: crate::theme::Shade = crate::theme::Shade::gray(170, 250);
const RULER_LINE_DIM: crate::theme::Shade = crate::theme::Shade::gray(120, 76);
const RULER_TICK_IN_BAND: crate::theme::Shade = crate::theme::Shade::gray(105, 110);
/// A chapter flag: its face and edge, a chosen flag's face, and the number on it.
const FLAG_FACE: Color32 = Color32::from_gray(232);
const FLAG_EDGE: Color32 = Color32::from_gray(120);
const FLAG_CHOSEN: Color32 = Color32::from_rgb(0, 114, 178);
/// The number on a chosen flag.
const FLAG_CHOSEN_NUMBER: Color32 = Color32::WHITE;
const FLAG_NUMBER: Color32 = Color32::from_gray(40);

/// The ruler's bottom band, where the chapter flags hang.
fn flag_band(ruler: Rect) -> Rect {
    Rect::from_min_max(
        egui::pos2(ruler.min.x, (ruler.max.y - FLAG_BAND).max(ruler.min.y)),
        ruler.max,
    )
}

/// A chapter flag as Encore shows one (M6.475): a light label with the chapter's number (1 the first), hanging
/// under the tick line from a short point at the chapter's time. The shape is DiscCraft's own (a rounded label and
/// a narrow point), not Encore's. A later flag covers an earlier one where they meet, as in Encore.
fn chapter_flag(painter: &egui::Painter, band: Rect, x: f32, index: usize, chosen: bool) {
    let number = index.saturating_add(1).to_string();
    // M6.481: the chosen flag is a strong blue with a white number, far from the other flags' white in lightness
    // as well as colour (it was a light blue, near white for those who see blue and yellow alike).
    let ink = if chosen {
        FLAG_CHOSEN_NUMBER
    } else {
        FLAG_NUMBER
    };
    let galley = painter.layout_no_wrap(number, egui::FontId::proportional(9.0), ink);
    let width = (galley.size().x + 4.0).max(9.0).round();
    let face = Rect::from_min_size(
        egui::pos2((x - width / 2.0).round(), band.min.y + 4.0),
        egui::vec2(width, 10.0),
    );
    let fill = if chosen { FLAG_CHOSEN } else { FLAG_FACE };
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(x - 2.5, face.min.y + 0.5),
            egui::pos2(x, band.min.y),
            egui::pos2(x + 2.5, face.min.y + 0.5),
        ],
        fill,
        Stroke::NONE,
    ));
    painter.rect(
        face,
        2.0,
        fill,
        Stroke::new(1.0, FLAG_EDGE),
        egui::StrokeKind::Inside,
    );
    painter.galley(face.center() - galley.size() / 2.0, galley, ink);
}

/// Unit tests for pure timeline helpers.
#[cfg(test)]
mod tests {
    use super::{
        chapter_label, clip_step, encore_ruler_steps, hit_marker, picture_starts, step_reach,
        tick_height, time_to_x, timecode, x_to_time,
    };

    /// Encore's pictures along a video clip (M6.483): the clip's head and each chapter point inside it, once each.
    #[test]
    fn a_clip_shows_a_picture_at_its_head_and_each_chapter_point() {
        let chapters = [0.0, 10.0, 10.01, 50.0, 99.99, 120.0, 150.0];
        assert_eq!(
            picture_starts(0.0, 100.0, &chapters, 0.02),
            vec![0.0, 10.0, 50.0]
        );
        // The second clip: its head (a chapter point is there too) and the point inside it.
        assert_eq!(
            picture_starts(100.0, 200.0, &[100.0, 120.0, f64::NAN], 0.02),
            vec![100.0, 120.0]
        );
        assert_eq!(picture_starts(5.0, 6.0, &[], 0.02), vec![5.0]);
    }

    /// Encore's ruler (M6.479): a tick every 8 px on whole frames, taller every second and fourth one, a timecode
    /// every 32 ticks (256 px); zoomed in, a tick on every frame with the timecodes still about 256 px apart.
    #[test]
    fn the_ruler_ticks_are_as_fine_as_encores() {
        assert_eq!(encore_ruler_steps(8.0 / 256.0), (256, 32));
        assert_eq!(encore_ruler_steps(0.5), (16, 32));
        // 9 px ticks: seven long ticks (252 px) between timecodes.
        assert_eq!(encore_ruler_steps(3.0), (3, 28));
        // A frame wider than a tick: every frame, a timecode every two long ticks (320 px).
        assert_eq!(encore_ruler_steps(40.0), (1, 8));
        assert_eq!(encore_ruler_steps(1000.0), (1, 4));
        // Broken input does not stop the ruler.
        assert_eq!(encore_ruler_steps(0.0), (1, 32));
        assert_eq!(encore_ruler_steps(f64::NAN), (1, 32));
        assert_eq!(
            (0..5).map(tick_height).collect::<Vec<_>>(),
            [8.0, 4.0, 6.0, 4.0, 8.0]
        );
    }

    /// Encore dims ◂ where there is no clip to go back to (the head of the timeline) and ▸ past the end of the last
    /// clip (M6.478).
    #[test]
    fn the_clip_arrows_dim_where_there_is_nowhere_to_go() {
        let spans = [(0.0, 100.0), (100.0, 170.0)];
        assert_eq!(step_reach(&spans, 0.0), (false, true));
        assert_eq!(step_reach(&spans, 50.0), (true, true));
        assert_eq!(step_reach(&spans, 170.0), (true, false));
        // A track with nothing on it goes nowhere.
        assert_eq!(step_reach(&[], 10.0), (false, false));
    }

    /// Encore's Previous Clip / Next Clip (measured, encore-behavior.md 9.28): next goes to the next clip's start and
    /// chooses it, past the last clip to its end with none chosen; previous goes to the start of the clip the
    /// playhead is in (or, at a start, the clip before).
    #[test]
    fn previous_and_next_clip_go_where_encore_goes() {
        let spans = [(0.0, 100.0), (100.0, 170.0), (170.0, 250.0)];
        assert_eq!(clip_step(&spans, 0.0, true), Some((100.0, Some(1))));
        assert_eq!(clip_step(&spans, 100.0, true), Some((170.0, Some(2))));
        assert_eq!(clip_step(&spans, 170.0, true), Some((250.0, None)));
        assert_eq!(clip_step(&spans, 250.0, true), None);
        assert_eq!(clip_step(&spans, 135.0, false), Some((100.0, Some(1))));
        assert_eq!(clip_step(&spans, 100.0, false), Some((0.0, Some(0))));
        assert_eq!(clip_step(&spans, 250.0, false), Some((170.0, Some(2))));
        assert_eq!(clip_step(&spans, 0.0, false), None);
        // Clips out of order, with gaps; a broken time goes nowhere.
        let spans = [(50.0, 60.0), (0.0, 10.0)];
        assert_eq!(clip_step(&spans, 20.0, true), Some((50.0, Some(0))));
        assert_eq!(clip_step(&spans, 20.0, false), Some((0.0, Some(1))));
        assert_eq!(clip_step(&spans, f64::NAN, true), None);
        assert_eq!(clip_step(&[], 0.0, true), None);
    }

    /// M6.299: a dragged subtitle clip moves by whole frames, stays off its neighbours and the start, keeps at least
    /// the shortest length when trimmed, and snaps to the nearest snap point within reach.
    #[test]
    fn a_subtitle_clip_is_dragged_trimmed_and_snapped() {
        use super::{CueGrip, dragged_cue};
        let cues = [(1.0, 2.0), (3.0, 4.0), (6.0, 7.0)];
        let grid = (0.5, 0.5);
        // Moved by 0.6 s: a whole frame of 0.5.
        assert_eq!(
            dragged_cue(&cues, 1, CueGrip::Body, 0.6, grid, None),
            Some((3.5, 4.5))
        );
        // Not into the next clip, nor the one before.
        assert_eq!(
            dragged_cue(&cues, 1, CueGrip::Body, 5.0, grid, None),
            Some((5.0, 6.0))
        );
        assert_eq!(
            dragged_cue(&cues, 1, CueGrip::Body, -5.0, grid, None),
            Some((2.0, 3.0))
        );
        assert_eq!(
            dragged_cue(&cues, 0, CueGrip::Body, -5.0, grid, None),
            Some((0.0, 1.0))
        );
        // Trimmed: never shorter than the shortest, nor past a neighbour.
        assert_eq!(
            dragged_cue(&cues, 1, CueGrip::In, 2.0, grid, None),
            Some((3.5, 4.0))
        );
        assert_eq!(
            dragged_cue(&cues, 1, CueGrip::In, -3.0, grid, None),
            Some((2.0, 4.0))
        );
        assert_eq!(
            dragged_cue(&cues, 1, CueGrip::Out, -2.0, grid, None),
            Some((3.0, 3.5))
        );
        assert_eq!(
            dragged_cue(&cues, 1, CueGrip::Out, 9.0, grid, None),
            Some((3.0, 6.0))
        );
        // Snapped: the nearer edge goes to the snap point within reach.
        let snaps = [5.2];
        assert_eq!(
            dragged_cue(&cues, 1, CueGrip::Body, 1.0, grid, Some((&snaps, 0.3))),
            Some((4.2, 5.2))
        );
        assert_eq!(
            dragged_cue(&cues, 1, CueGrip::Out, 1.0, grid, Some((&snaps, 0.3))),
            Some((3.0, 5.2))
        );
        // Out of reach: by frames only.
        assert_eq!(
            dragged_cue(&cues, 1, CueGrip::Out, 0.5, grid, Some((&snaps, 0.3))),
            Some((3.0, 4.5))
        );
        assert_eq!(dragged_cue(&cues, 3, CueGrip::Body, 1.0, grid, None), None);
    }

    /// Checks that time and x coordinates round trip.
    #[test]
    fn time_round_trip() {
        let x = time_to_x(12.5, 20.0, 2.0);
        let seconds = x_to_time(x, 20.0, 2.0);
        assert!((seconds - 12.5).abs() < 1e-3);
    }

    /// Checks that x_to_time never returns a negative time.
    #[test]
    fn x_to_time_never_negative() {
        assert_eq!(x_to_time(0.0, 100.0, 1.0), 0.0);
    }

    /// NTSC shows drop-frame timecode with semicolons, as in Encore.
    #[test]
    fn timecode_ntsc() {
        assert_eq!(timecode(3723.5, 30000, 1001), "01;02;03;15");
        assert_eq!(timecode(6.0 * 1.001, 30000, 1001), "00;00;06;00");
        assert_eq!(timecode(-1.0, 30000, 1001), "00;00;00;00");
        assert_eq!(timecode(f64::NAN, 30000, 1001), "00;00;00;00");
    }

    /// PAL shows non-drop timecode with colons.
    #[test]
    fn timecode_pal() {
        assert_eq!(timecode(61.0, 25, 1), "00:01:01:00");
        assert_eq!(timecode(1.0, 0, 0), "00:00:00:00");
    }

    /// Checks marker hit tolerance.
    #[test]
    fn chapter_labels_translate_default_names_only() {
        assert!(chapter_label("Chapter 3", 0).contains('3'));
        assert!(chapter_label("", 4).contains('5'));
        assert_eq!(chapter_label("Opening", 1), "Opening");
        assert_eq!(chapter_label("Chapter One", 1), "Chapter One");
    }

    #[test]
    fn hit_marker_tolerance() {
        let markers = [10.0_f32, 20.0_f32, 30.0_f32];
        assert_eq!(hit_marker(&markers, 21.0, 2.0), Some(1));
        assert_eq!(hit_marker(&markers, 15.0, 2.0), None);
    }
}
