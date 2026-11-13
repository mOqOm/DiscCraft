use crate::app::DiscCraftApp;
use crate::i18n::{tl, tlf};
use crate::menubar::ViewRequest;
use disccraft_project::{FrameRate, TimelineId, VideoStandard, format_frames};
use egui::{Color32, Id, Rect, Response, Shape, Stroke, Ui};
use serde_json::{Value, json};

/// Pixels per second at zoom 1.0.
const BASE_SCALE: f32 = 8.0;
/// Ticks per second used by the project timeline.
const TICKS_PER_SECOND: i64 = 254_016_000_000;
/// Height of the timeline canvas in points.
const CANVAS_HEIGHT: f32 = 120.0;
/// Horizontal padding around the timeline canvas.
const CANVAS_PADDING: f32 = 40.0;
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

/// Returns the seconds between labelled ruler ticks so labels are at least 60 px apart.
pub(crate) fn ruler_step(scale: f32) -> f64 {
    let px_per_second = f64::from(BASE_SCALE) * f64::from(scale);
    if !px_per_second.is_finite() || px_per_second <= 0.0 {
        return 600.0;
    }

    for step in [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0] {
        if step * px_per_second >= 60.0 {
            return step;
        }
    }
    600.0
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

/// Seconds between ruler ticks `step` timecode seconds apart (at 29.97 the timecode counts 30 frames a
/// second, so a timecode second is a little longer than a clock second).
fn ruler_tick_seconds(step: f64, fps_num: u32, fps_den: u32) -> f64 {
    let fps = frames_per_second(fps_num, fps_den);
    if fps <= 0.0 {
        return step;
    }
    // The base is small (30 or 25), so the conversion is exact.
    step * rate_of(fps_num, fps_den).timecode_base() as f64 / fps
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

/// Returns a zoom factor that fits the duration into the available width.
fn fit_zoom(available_width: f32, duration: f64) -> f32 {
    if !available_width.is_finite() || available_width <= 0.0 {
        return 1.0;
    }
    if !duration.is_finite() || duration <= 0.0 {
        return 1.0;
    }

    let px = duration * f64::from(BASE_SCALE);
    if !px.is_finite() || px <= 0.0 {
        return 1.0;
    }

    let zoom = f64::from(available_width) / px;
    if !zoom.is_finite() {
        return 1.0;
    }
    (zoom as f32).clamp(0.1, 100.0)
}

/// Returns the desired canvas width for a duration and zoom.
fn timeline_width(duration: f64, zoom: f32) -> f32 {
    if !duration.is_finite() || duration <= 0.0 || !zoom.is_finite() || zoom <= 0.0 {
        return 200.0;
    }

    let px = duration * f64::from(BASE_SCALE) * f64::from(zoom) + f64::from(CANVAS_PADDING);
    if !px.is_finite() {
        return 100000.0;
    }
    px.clamp(200.0, 100000.0) as f32
}

/// Returns the time to draw for a marker, using drag state when active.
fn marker_time(drag: Option<MarkerDrag>, index: usize, chapter_time: f64) -> f64 {
    match drag {
        Some(drag) if drag.marker == index => drag.time,
        _ => chapter_time,
    }
}

/// Cached result of probing a timeline's video asset.
#[derive(Clone, Copy)]
struct DurationProbe {
    /// Asset id that was probed.
    asset: u64,
    /// Whether the probe has already been attempted.
    done: bool,
    /// Probed duration in seconds, or zero when unknown.
    seconds: f64,
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
}

/// Owned snapshot of timeline data used for drawing and interaction.
struct TimelineSnapshot {
    /// Timeline id as u64.
    id: u64,
    /// Timeline name.
    name: String,
    /// Video asset id.
    video: u64,
    /// Video asset display name.
    asset_name: String,
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
    /// Canvas rectangle.
    canvas: Rect,
    /// Timeline duration in seconds.
    duration: f64,
    /// Current zoom factor.
    zoom: f32,
    /// Playhead time in seconds.
    playhead: f64,
    /// Selected chapter index.
    selected: Option<usize>,
    /// Active marker drag.
    drag: Option<MarkerDrag>,
    /// The video clip's first frame (drawn at the start of the video bar).
    first: Option<egui::TextureHandle>,
    /// The video clip's last frame (drawn at the end of the video bar).
    last: Option<egui::TextureHandle>,
    /// Whether chapter names are drawn next to the chapter points (View ▸ Show Chapter Names).
    show_names: bool,
}

/// Timeline view methods for the desktop application.
impl DiscCraftApp {
    /// Draws the timeline viewer for one title timeline, laid out like Encore's: a header column (the current
    /// time, the add-chapter-point button, the track names and languages) beside the ruler and the tracks, and the
    /// zoom in a bar at the bottom.
    pub(crate) fn timeline_view(&mut self, ui: &mut Ui, id: TimelineId) {
        // 字幕トラックの字幕を読んでおく（スナップショットに入れて、トラックの上に並べる）。
        self.load_timeline_cues(id);
        let Some(snapshot) = self.timeline_snapshot(id) else {
            ui.label(tl!("No timeline selected"));
            return;
        };

        let duration = self.timeline_duration(ui, &snapshot);
        // 初めて開いたときは、クリップ全体が横幅に収まる拡大率にする。
        let mut zoom = timeline_zoom(ui, id).unwrap_or_else(|| {
            fit_zoom(
                ui.available_width() - HEADER_WIDTH - CANVAS_PADDING,
                duration,
            )
        });
        let mut interaction = TimelineInteraction {
            playhead: timeline_playhead(ui, id),
            selected: timeline_selected(ui, id),
            drag: timeline_drag(ui, id),
        };

        let _ = egui::Panel::bottom(Id::unique("timeline_bar").with(id.0))
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(0, 3)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    self.timeline_bar(ui, &snapshot, id, &mut interaction, duration, &mut zoom);
                });
            });
        timeline_store_zoom(ui, id, zoom);

        let _ = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("timeline_rows", id.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let height = canvas_height(snapshot.tracks.len());
                        let _ = ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            self.track_headers(ui, &snapshot, id, height, &interaction, duration);
                            self.timeline_scroll(
                                ui,
                                &snapshot,
                                id,
                                duration,
                                zoom,
                                &mut interaction,
                            );
                        });
                    });
            });
        timeline_store_state(ui, id, &interaction);
    }

    /// The scrolling canvas with the ruler, the video and track bars, the chapter points and the playhead.
    fn timeline_scroll(
        &mut self,
        ui: &mut Ui,
        snapshot: &TimelineSnapshot,
        id: TimelineId,
        duration: f64,
        zoom: f32,
        interaction: &mut TimelineInteraction,
    ) {
        let height = canvas_height(snapshot.tracks.len());
        let video = disccraft_project::AssetId(snapshot.video);
        let first = self.asset_thumbnail(ui.ctx(), video);
        let last = self.clip_last_frame(ui.ctx(), video, duration);
        let _ = egui::ScrollArea::horizontal().show(ui, |ui| {
            let (canvas, response) = timeline_canvas(ui, duration, zoom, height);
            let frame = TimelineFrame {
                snapshot,
                canvas,
                duration,
                zoom,
                playhead: interaction.playhead,
                selected: interaction.selected,
                drag: interaction.drag,
                first,
                last,
                show_names: self.show_chapter_names,
            };

            timeline_draw(ui, &frame);
            let before = interaction.selected;
            self.timeline_handle_input(ui, &frame, &response, interaction);
            if interaction.selected != before {
                self.focus_chapter(id, interaction.selected);
            }
            self.timeline_context_menu(ui, &frame, &response, id, interaction);
        });
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
        let texture = self
            .run_query(
                "asset.frame",
                json!({ "asset": video.0, "seconds": seconds }),
            )
            .and_then(|value| {
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
    fn timeline_snapshot(&self, id: TimelineId) -> Option<TimelineSnapshot> {
        let timeline = self.session.project.timeline(id)?;
        let (fps_num, fps_den) = frame_rate(self.session.project.settings.standard);

        let asset_name = match self.session.project.assets.get(&timeline.video) {
            Some(asset) => asset.name.clone(),
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

        Some(TimelineSnapshot {
            id: id.0,
            name: timeline.name.clone(),
            video: timeline.video.0,
            asset_name,
            chapters,
            tracks,
            fps_num,
            fps_den,
        })
    }

    /// Track rows for the audio and subtitle tracks of `timeline`.
    fn track_rows(&self, timeline: &disccraft_project::Timeline) -> Vec<TrackRow> {
        let name = |asset: &disccraft_project::AssetId| {
            self.session
                .project
                .assets
                .get(asset)
                .map(|asset| asset.name.clone())
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
                clip: name(&track.asset),
                subtitle: false,
                cues: None,
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
            });
        audio.chain(subtitles).collect()
    }

    /// Returns the timeline duration, probing the video asset once per timeline.
    fn timeline_duration(&mut self, ui: &mut Ui, snapshot: &TimelineSnapshot) -> f64 {
        let key = timeline_key("timeline_duration", snapshot.id);
        let mut probe = match ui.data(|data| data.get_temp::<DurationProbe>(key)) {
            Some(probe) if probe.asset == snapshot.video => probe,
            _ => DurationProbe {
                asset: snapshot.video,
                done: false,
                seconds: 0.0,
            },
        };

        if !probe.done {
            probe.done = true;
            if let Some(value) = self.run_query("asset.probe", json!({"asset": snapshot.video}))
                && let Some(seconds) = value.get("seconds").and_then(Value::as_f64)
                && seconds.is_finite()
                && seconds > 0.0
            {
                probe.seconds = seconds;
            }
            ui.data_mut(|data| data.insert_temp(key, probe));
        }

        if probe.seconds > 0.0 {
            probe.seconds
        } else {
            fallback_duration(&snapshot.chapters)
        }
    }

    /// The bar under the tracks: the zoom (out, slider, in, fit), the duration, and the selected chapter point.
    /// Requests from the menus and shortcuts (zoom, add or delete a chapter point) are handled here too.
    fn timeline_bar(
        &mut self,
        ui: &mut Ui,
        snapshot: &TimelineSnapshot,
        id: TimelineId,
        interaction: &mut TimelineInteraction,
        duration: f64,
        zoom: &mut f32,
    ) {
        let fit = fit_zoom(
            ui.available_width() - HEADER_WIDTH - CANVAS_PADDING,
            duration,
        );
        // メニューやショートカットからの依頼は、ボタンのクリックと同じに扱う。
        let request = self.view_request.take();

        self.timeline_switcher(ui, id, &snapshot.name);
        ui.separator();
        if ui
            .small_button(tl!("−"))
            .on_hover_text(tl!("Zoom Out"))
            .clicked()
            || request == Some(ViewRequest::ZoomOut)
        {
            *zoom = (*zoom / 1.25).clamp(MIN_ZOOM, MAX_ZOOM);
        }
        ui.spacing_mut().slider_width = 110.0;
        let _ = ui
            .add(
                egui::Slider::new(&mut *zoom, MIN_ZOOM..=MAX_ZOOM)
                    .logarithmic(true)
                    .show_value(false),
            )
            .on_hover_text(tl!("Zoom"));
        if ui
            .small_button(tl!("+"))
            .on_hover_text(tl!("Zoom In"))
            .clicked()
            || request == Some(ViewRequest::ZoomIn)
        {
            *zoom = (*zoom * 1.25).clamp(MIN_ZOOM, MAX_ZOOM);
        }
        if ui.button(tl!("Fit")).clicked() || request == Some(ViewRequest::Fit) {
            *zoom = fit;
        }
        if request == Some(ViewRequest::AddChapterPoint) {
            self.add_chapter_at_playhead(id, interaction.playhead, duration);
        }

        ui.separator();
        ui.label(tlf!(
            "Duration: {time}",
            time = timecode(duration, snapshot.fps_num, snapshot.fps_den)
        ));

        let Some(index) = interaction.selected else {
            return;
        };
        let Some(chapter) = snapshot.chapters.get(index) else {
            interaction.selected = None;
            return;
        };
        ui.separator();
        // 名前の変更はプロパティパネル（チャプターのシート）で行う。
        ui.label(chapter_label(&chapter.name, index));
        if index == 0 {
            ui.label(tl!("First chapter is fixed"));
        } else if ui.button(tl!("Remove Chapter")).clicked() || request == Some(ViewRequest::Clear)
        {
            let _ = self.run_command(
                "timeline.removeChapter",
                json!({"timeline": id.0, "chapter": index}),
            );
            interaction.selected = None;
            interaction.drag = None;
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

    /// The header column: the current time ("Timeline:" and, inside the clip, "Source:") with the add-chapter-point
    /// button, then a header per row: "Video", and each audio and subtitle track with its language as a list.
    fn track_headers(
        &mut self,
        ui: &mut Ui,
        snapshot: &TimelineSnapshot,
        id: TimelineId,
        height: f32,
        interaction: &TimelineInteraction,
        duration: f64,
    ) {
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(HEADER_WIDTH, height), egui::Sense::hover());
        let painter = ui.painter().clone();
        let weak = ui.visuals().weak_text_color();
        let text = ui.visuals().text_color();
        let accent = ui.visuals().warn_fg_color;
        let small = egui::FontId::proportional(10.0);
        let mono = egui::FontId::monospace(11.0);

        // Encore と同じく、トラック名の上に今の時刻（タイムラインとソース）をアクセントの色で。
        let playhead = timecode(interaction.playhead, snapshot.fps_num, snapshot.fps_den);
        let at = |x: f32, y: f32| egui::pos2(rect.min.x + x, rect.min.y + y);
        painter.text(
            at(4.0, 1.0),
            egui::Align2::LEFT_TOP,
            tl!("Timeline:"),
            small.clone(),
            weak,
        );
        painter.text(
            at(4.0, 13.0),
            egui::Align2::LEFT_TOP,
            playhead.clone(),
            mono.clone(),
            accent,
        );
        painter.text(
            at(SOURCE_X, 1.0),
            egui::Align2::LEFT_TOP,
            tl!("Source timecode:"),
            small,
            weak,
        );
        if interaction.playhead <= duration {
            painter.text(
                at(SOURCE_X, 13.0),
                egui::Align2::LEFT_TOP,
                playhead,
                mono,
                accent,
            );
        }

        // チャプターポイントを追加（再生ヘッドの位置）。チャプターポイントの印に + を添えた絵のボタン。
        let button = Rect::from_min_size(at(HEADER_WIDTH - 24.0, 4.0), egui::vec2(20.0, 20.0));
        let response = ui
            .interact(
                button,
                Id::unique("timeline_add_chapter").with(id.0),
                egui::Sense::click(),
            )
            .on_hover_text(tl!("Add Chapter at Playhead"));
        let visuals = ui.style().interact(&response);
        painter.rect(
            button,
            2.0,
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        add_chapter_glyph(&painter, button, visuals.fg_stroke.color);
        if response.clicked() {
            self.add_chapter_at_playhead(id, interaction.playhead, duration);
        }

        // 行の見出し。ビデオの行と、トラックごとの行（名前と言語のリスト）。
        let video = Rect::from_min_max(
            at(0.0, VIDEO_TOP - ROW_GAP),
            at(HEADER_WIDTH, VIDEO_TOP + VIDEO_HEIGHT + ROW_GAP),
        );
        painter.rect_filled(video, 0.0, crate::theme::HEADER);
        painter.text(
            egui::pos2(video.min.x + 6.0, video.center().y),
            egui::Align2::LEFT_CENTER,
            tl!("Video"),
            egui::FontId::proportional(12.0),
            text,
        );

        let mut top = TRACKS_TOP;
        for track in &snapshot.tracks {
            let row = Rect::from_min_max(
                at(0.0, top - ROW_GAP),
                at(HEADER_WIDTH, top + BAR_HEIGHT + ROW_GAP),
            );
            painter.rect_filled(row, 0.0, crate::theme::HEADER);
            let mut label_x = row.min.x + 6.0;
            if track.subtitle {
                // Encore と同じく、見出しの左の四角でモニターに出す字幕トラックを切り替える（0 本か 1 本）。
                let toggle = Rect::from_center_size(
                    egui::pos2(row.min.x + 12.0, row.center().y),
                    egui::vec2(14.0, 14.0),
                );
                let response = ui
                    .interact(
                        toggle,
                        Id::unique("subtitle_shown").with((id.0, track.index)),
                        egui::Sense::click(),
                    )
                    .on_hover_text(tl!("Show Subtitles in Monitor"));
                let shown = self.shown_subtitle(id) == Some(track.index);
                let visuals = ui.style().interact(&response);
                painter.rect_stroke(
                    toggle,
                    1.0,
                    egui::Stroke::new(1.0, visuals.fg_stroke.color),
                    egui::StrokeKind::Inside,
                );
                if shown {
                    subtitle_glyph(&painter, toggle.shrink(3.0), text);
                }
                if response.clicked() {
                    self.toggle_shown_subtitle(id, track.index);
                }
                label_x = toggle.max.x + 6.0;
            }
            painter.text(
                egui::pos2(label_x, row.center().y),
                egui::Align2::LEFT_CENTER,
                track.label.clone(),
                egui::FontId::proportional(12.0),
                text,
            );
            let combo = Rect::from_min_size(
                egui::pos2(row.max.x - 66.0, row.center().y - 10.0),
                egui::vec2(60.0, 20.0),
            );
            if let Some(language) = language_list(ui, combo, id, track) {
                let kind = if track.subtitle { "subtitle" } else { "audio" };
                let _ = self.run_command(
                    "timeline.setTrackLanguage",
                    json!({"timeline": id.0, "kind": kind, "track": track.index, "language": language}),
                );
            }
            top += TRACK_PITCH;
        }
    }

    /// The timeline's name as a drop-down that switches the viewer to another timeline (like the tab menu in
    /// classic authoring tools).
    fn timeline_switcher(&mut self, ui: &mut Ui, id: TimelineId, name: &str) {
        let timelines: Vec<(TimelineId, String)> = self
            .session
            .project
            .timelines
            .iter()
            .map(|(id, timeline)| (*id, timeline.name.clone()))
            .collect();

        let mut chosen = None;
        egui::ComboBox::from_id_salt("timeline_switcher")
            .selected_text(name)
            .show_ui(ui, |ui| {
                for (other, other_name) in &timelines {
                    if ui.selectable_label(*other == id, other_name).clicked() {
                        chosen = Some(*other);
                    }
                }
            });
        if let Some(other) = chosen.filter(|other| *other != id) {
            self.select(crate::app::Selection::Timeline(other));
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
            let time =
                x_to_time(point.x, frame.canvas.min.x, frame.zoom).clamp(0.0, frame.duration);
            let markers_x: Vec<f32> = frame
                .snapshot
                .chapters
                .iter()
                .map(|chapter| time_to_x(chapter.time, frame.canvas.min.x, frame.zoom))
                .collect();
            let hit = hit_marker(&markers_x, point.x, HIT_TOLERANCE);
            ui.data_mut(|data| data.insert_temp(key, (time, hit)));
        }

        let Some((time, hit)) = ui.data(|data| data.get_temp::<(f64, Option<usize>)>(key)) else {
            return;
        };
        let duration = frame.duration;
        let _ = response.context_menu(|ui| {
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

    /// Handles pointer interaction with the timeline canvas.
    fn timeline_handle_input(
        &mut self,
        ui: &mut Ui,
        frame: &TimelineFrame<'_>,
        response: &Response,
        interaction: &mut TimelineInteraction,
    ) {
        let pointer = ui.input(|input| input.pointer.interact_pos());

        if let Some(drag) = interaction.drag {
            if ui.input(|input| input.pointer.any_released()) {
                self.finish_marker_drag(frame, interaction, drag);
                return;
            }

            if let Some(point) = pointer {
                let time =
                    x_to_time(point.x, frame.canvas.min.x, frame.zoom).clamp(0.0, frame.duration);
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
            .map(|chapter| time_to_x(chapter.time, frame.canvas.min.x, frame.zoom))
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
                x_to_time(point.x, frame.canvas.min.x, frame.zoom).clamp(0.0, frame.duration);
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
                x_to_time(point.x, frame.canvas.min.x, frame.zoom).clamp(0.0, frame.duration);
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

/// Reads the timeline zoom factor from egui temp memory (None before the viewer first showed the timeline).
fn timeline_zoom(ui: &Ui, id: TimelineId) -> Option<f32> {
    let key = timeline_key("timeline_zoom", id.0);
    ui.data(|data| data.get_temp::<f32>(key))
        .filter(|zoom| zoom.is_finite() && *zoom > 0.0)
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

/// Stores the timeline zoom factor in egui temp memory.
fn timeline_store_zoom(ui: &mut Ui, id: TimelineId, zoom: f32) {
    let key = timeline_key("timeline_zoom", id.0);
    ui.data_mut(|data| data.insert_temp(key, zoom));
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

/// Allocates the timeline canvas inside the horizontal scroll area.
fn timeline_canvas(ui: &mut Ui, duration: f64, zoom: f32, height: f32) -> (Rect, Response) {
    let width = timeline_width(duration, zoom).max(ui.available_width());
    let (response, painter) =
        ui.allocate_painter(egui::vec2(width, height), egui::Sense::click_and_drag());
    drop(painter);
    (response.rect, response)
}

/// Draws the complete timeline canvas.
fn timeline_draw(ui: &Ui, frame: &TimelineFrame<'_>) {
    let painter = ui.painter();
    draw_rows(painter, frame);
    draw_ruler(painter, frame);
    draw_video_bar(painter, frame);
    draw_track_bars(painter, frame);
    draw_markers(painter, frame);
    draw_playhead(painter, frame);
}

/// Draws the bands behind the ruler and each row, across the whole canvas.
fn draw_rows(painter: &egui::Painter, frame: &TimelineFrame<'_>) {
    let canvas = frame.canvas;
    let band = |top: f32, bottom: f32| {
        Rect::from_min_max(
            egui::pos2(canvas.min.x, canvas.min.y + top),
            egui::pos2(canvas.max.x, canvas.min.y + bottom),
        )
    };
    painter.rect_filled(band(0.0, RULER_HEIGHT), 0.0, crate::theme::TAB_STRIP);
    painter.rect_filled(
        band(VIDEO_TOP - ROW_GAP, VIDEO_TOP + VIDEO_HEIGHT + ROW_GAP),
        0.0,
        ROW_FILL,
    );
    let mut top = TRACKS_TOP;
    for _ in &frame.snapshot.tracks {
        painter.rect_filled(
            band(top - ROW_GAP, top + BAR_HEIGHT + ROW_GAP),
            0.0,
            ROW_FILL,
        );
        top += TRACK_PITCH;
    }
}

/// Draws the time ruler: a timecode every major tick, and minor ticks between, on to the end of the canvas.
fn draw_ruler(painter: &egui::Painter, frame: &TimelineFrame<'_>) {
    let step = ruler_tick_seconds(
        ruler_step(frame.zoom),
        frame.snapshot.fps_num,
        frame.snapshot.fps_den,
    );
    if step <= 0.0 {
        return;
    }

    let origin_x = frame.canvas.min.x;
    let bottom = frame.canvas.min.y + RULER_HEIGHT;
    let major_px = time_to_x(step, origin_x, frame.zoom) - origin_x;
    let minors: u8 = if major_px >= 100.0 { 10 } else { 5 };
    let minor = step / f64::from(minors);
    let mut count = 0_u32;

    // Each tick is placed from its own count, so the rounding does not add up along the ruler.
    while count < 100_000 {
        let time = minor * f64::from(count);
        let x = time_to_x(time, origin_x, frame.zoom);
        if x > frame.canvas.max.x {
            break;
        }
        let major = count.is_multiple_of(u32::from(minors));
        let length = if major { 10.0 } else { 4.0 };
        painter.vline(
            x,
            (bottom - length)..=bottom,
            Stroke::new(1.0, Color32::GRAY),
        );
        if major {
            painter.text(
                egui::pos2(x + 2.0, frame.canvas.min.y + 2.0),
                egui::Align2::LEFT_TOP,
                timecode(time, frame.snapshot.fps_num, frame.snapshot.fps_den),
                egui::FontId::proportional(10.0),
                Color32::LIGHT_GRAY,
            );
        }
        count += 1;
    }
}

/// Draws the video asset bar.
fn draw_video_bar(painter: &egui::Painter, frame: &TimelineFrame<'_>) {
    let origin_x = frame.canvas.min.x;
    let left = origin_x;
    let right = time_to_x(frame.duration, origin_x, frame.zoom);
    if right <= left {
        return;
    }

    let top = frame.canvas.min.y + VIDEO_TOP;
    let bottom = top + VIDEO_HEIGHT;
    let rect = Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom));

    painter.rect_filled(rect, 0.0, Color32::from_rgb(70, 110, 160));

    // Like Encore: the clip's first frame at its start and its last frame at its end, below the name strip.
    let strip = Rect::from_min_max(
        egui::pos2(rect.min.x, rect.min.y + 14.0),
        egui::pos2(rect.max.x, rect.max.y - 1.0),
    );
    let thumb_width = strip.height() * 4.0 / 3.0;
    if strip.width() > thumb_width * 2.0 + 4.0 {
        if let Some(texture) = &frame.first {
            let at = Rect::from_min_size(strip.min, egui::vec2(thumb_width, strip.height()));
            crate::project_panel::draw_fitted(painter, at, texture);
        }
        if let Some(texture) = &frame.last {
            let at = Rect::from_min_size(
                egui::pos2(strip.max.x - thumb_width, strip.min.y),
                egui::vec2(thumb_width, strip.height()),
            );
            crate::project_panel::draw_fitted(painter, at, texture);
        }
    }

    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, Color32::from_rgb(120, 170, 220)),
        egui::StrokeKind::Inside,
    );

    if rect.width() > 20.0 {
        painter.text(
            rect.min + egui::vec2(4.0, 1.0),
            egui::Align2::LEFT_TOP,
            frame.snapshot.asset_name.clone(),
            egui::FontId::proportional(11.0),
            Color32::WHITE,
        );
    }
}

/// Height of the ruler band at the top of the canvas.
const RULER_HEIGHT: f32 = 26.0;
/// Space above and below a bar inside its row band.
const ROW_GAP: f32 = 3.0;
/// Fill of the row bands in the canvas (a little darker than the headers).
const ROW_FILL: Color32 = Color32::from_gray(50);
/// Left edge of "Source:" in the header column.
const SOURCE_X: f32 = 92.0;
/// The smallest and largest zoom factors.
const MIN_ZOOM: f32 = 0.1;
/// The largest zoom factor.
const MAX_ZOOM: f32 = 100.0;
/// Top of the video bar, relative to the canvas.
const VIDEO_TOP: f32 = 30.0;
/// Height of the video bar and of each track bar.
const BAR_HEIGHT: f32 = 24.0;
/// Height of the video bar (taller than the audio and subtitle bars, to show the clip's frames).
const VIDEO_HEIGHT: f32 = 44.0;
/// Top of the first track row (below the chapter labels), relative to the canvas.
const TRACKS_TOP: f32 = 92.0;
/// Distance between track rows.
const TRACK_PITCH: f32 = 30.0;
/// Width of the track header column.
const HEADER_WIDTH: f32 = 200.0;
/// The languages offered in a track header (ISO 639-1), like the list in Encore. A track's own language is kept
/// in the list even when it is not one of these.
const LANGUAGES: &[&str] = &[
    "ja", "en", "zh", "ko", "fr", "de", "es", "it", "pt", "ru", "nl", "sv", "no", "da", "fi", "pl",
    "th",
];

/// Canvas height for `tracks` audio and subtitle rows.
fn canvas_height(tracks: usize) -> f32 {
    let rows = f32::from(u16::try_from(tracks).unwrap_or(u16::MAX));
    (TRACKS_TOP + rows * TRACK_PITCH + 8.0).max(CANVAS_HEIGHT)
}

/// The language list of a track header. Returns the language chosen this frame.
fn language_list(ui: &mut Ui, rect: Rect, id: TimelineId, track: &TrackRow) -> Option<String> {
    let mut chosen = None;
    let salt = (id.0, track.subtitle, track.index);
    let shown = if track.language.is_empty() {
        "--".to_owned()
    } else {
        track.language.clone()
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

/// The add-chapter-point picture: a chapter marker (a downward triangle) with a plus beside it.
fn add_chapter_glyph(painter: &egui::Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    painter.add(Shape::convex_polygon(
        vec![
            egui::pos2(c.x - 6.0, c.y - 4.0),
            egui::pos2(c.x + 2.0, c.y - 4.0),
            egui::pos2(c.x - 2.0, c.y + 2.0),
        ],
        MARKER,
        Stroke::NONE,
    ));
    painter.vline(
        c.x - 2.0,
        (c.y + 2.0)..=(c.y + 6.0),
        Stroke::new(1.0, MARKER),
    );
    let stroke = Stroke::new(1.5, color);
    painter.hline((c.x + 2.0)..=(c.x + 8.0), c.y + 3.0, stroke);
    painter.vline(c.x + 5.0, c.y..=(c.y + 6.0), stroke);
}

/// Draws one bar per audio and subtitle track, spanning the timeline.
fn draw_track_bars(painter: &egui::Painter, frame: &TimelineFrame<'_>) {
    let origin_x = frame.canvas.min.x;
    let right = time_to_x(frame.duration, origin_x, frame.zoom);
    if right <= origin_x {
        return;
    }

    let mut top = frame.canvas.min.y + TRACKS_TOP;
    for track in &frame.snapshot.tracks {
        let rect = Rect::from_min_max(
            egui::pos2(origin_x, top),
            egui::pos2(right, top + BAR_HEIGHT),
        );
        let (fill, edge) = if track.subtitle {
            (
                Color32::from_rgb(110, 80, 140),
                Color32::from_rgb(170, 130, 210),
            )
        } else {
            (
                Color32::from_rgb(70, 120, 80),
                Color32::from_rgb(120, 190, 130),
            )
        };
        if let Some(cues) = &track.cues {
            draw_cues(painter, frame, rect, cues, fill, edge);
            top += TRACK_PITCH;
            continue;
        }
        painter.rect_filled(rect, 0.0, fill);
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
        if rect.width() > 20.0 {
            painter.text(
                rect.min + egui::vec2(4.0, 4.0),
                egui::Align2::LEFT_TOP,
                track.clip.clone(),
                egui::FontId::proportional(12.0),
                Color32::WHITE,
            );
        }
        top += TRACK_PITCH;
    }
}

/// The mark of a subtitle track shown in the monitor: a frame with two lines of text at its foot.
fn subtitle_glyph(painter: &egui::Painter, area: Rect, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.0, color);
    painter.rect_stroke(area, 0.0, stroke, egui::StrokeKind::Inside);
    for (y, inset) in [(area.max.y - 5.0, 2.0), (area.max.y - 2.5, 3.0)] {
        painter.line_segment(
            [
                egui::pos2(area.min.x + inset, y),
                egui::pos2(area.max.x - inset, y),
            ],
            stroke,
        );
    }
}

/// Draws a subtitle track: a faint bar for the whole timeline and one block per cue with its text.
fn draw_cues(
    painter: &egui::Painter,
    frame: &TimelineFrame<'_>,
    bar: Rect,
    cues: &[crate::subtitles::SubtitleCue],
    fill: Color32,
    edge: Color32,
) {
    painter.rect_filled(bar, 0.0, fill.gamma_multiply(0.35));
    let origin_x = frame.canvas.min.x;
    let visible = frame.canvas.x_range();
    for cue in cues {
        let left = time_to_x(cue.start, origin_x, frame.zoom);
        let right = time_to_x(cue.end, origin_x, frame.zoom).max(left + 1.0);
        if right < visible.min || left > visible.max {
            continue;
        }
        let block = Rect::from_min_max(egui::pos2(left, bar.min.y), egui::pos2(right, bar.max.y));
        painter.rect_filled(block, 0.0, fill);
        painter.rect_stroke(block, 0.0, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
        if block.width() > 24.0 {
            let text = cue.text.lines().next().unwrap_or_default().to_owned();
            painter.with_clip_rect(block.shrink(2.0)).text(
                block.min + egui::vec2(3.0, 4.0),
                egui::Align2::LEFT_TOP,
                text,
                egui::FontId::proportional(11.0),
                Color32::WHITE,
            );
        }
    }
}

/// Draws chapter markers.
fn draw_markers(painter: &egui::Painter, frame: &TimelineFrame<'_>) {
    let origin_x = frame.canvas.min.x;
    // Encore と同じく、チャプターポイントの印は目盛りの下の端に。
    let top = frame.canvas.min.y + RULER_HEIGHT - 9.0;
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
            Color32::from_rgb(255, 220, 80)
        } else {
            MARKER
        };

        painter.line_segment(
            [egui::pos2(x, top), egui::pos2(x, bottom)],
            Stroke::new(if selected { 2.0 } else { 1.0 }, color),
        );

        let triangle = [
            egui::pos2(x - 5.0, top),
            egui::pos2(x + 5.0, top),
            egui::pos2(x, top + 8.0),
        ];
        painter.add(Shape::convex_polygon(
            triangle.to_vec(),
            color,
            Stroke::NONE,
        ));

        // ポスターフレームは小さな絵の枠で（チャプターと同じ色）。
        if let Some(poster) = chapter.poster {
            let px = time_to_x(poster, origin_x, frame.zoom);
            if px >= frame.canvas.min.x - 20.0 && px <= frame.canvas.max.x + 20.0 {
                let picture =
                    egui::Rect::from_center_size(egui::pos2(px, top + 4.0), egui::vec2(8.0, 6.0));
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

/// Draws the playhead: a handle on the ruler and a line down through the rows.
fn draw_playhead(painter: &egui::Painter, frame: &TimelineFrame<'_>) {
    let x = time_to_x(frame.playhead, frame.canvas.min.x, frame.zoom);
    if x < frame.canvas.min.x || x > frame.canvas.max.x {
        return;
    }

    let top = frame.canvas.min.y;
    let handle = top + RULER_HEIGHT - 12.0;
    painter.add(Shape::convex_polygon(
        vec![
            egui::pos2(x - 5.0, handle),
            egui::pos2(x + 5.0, handle),
            egui::pos2(x + 5.0, handle + 7.0),
            egui::pos2(x, handle + 12.0),
            egui::pos2(x - 5.0, handle + 7.0),
        ],
        PLAYHEAD,
        Stroke::NONE,
    ));
    painter.vline(
        x,
        (handle + 12.0)..=frame.canvas.max.y,
        Stroke::new(1.0, PLAYHEAD),
    );
}

/// Colour of a chapter point.
const MARKER: Color32 = Color32::from_rgb(220, 120, 80);
/// Colour of the playhead.
const PLAYHEAD: Color32 = Color32::from_rgb(255, 80, 80);

/// Unit tests for pure timeline helpers.
#[cfg(test)]
mod tests {
    use super::{
        BASE_SCALE, chapter_label, hit_marker, ruler_step, ruler_tick_seconds, time_to_x, timecode,
        x_to_time,
    };

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

    /// Checks that ruler steps keep labels at least 60 px apart.
    #[test]
    fn ruler_step_spacing() {
        for scale in [0.1_f32, 0.5_f32, 1.0_f32, 2.0_f32, 10.0_f32] {
            let step = ruler_step(scale);
            let px = step * f64::from(BASE_SCALE) * f64::from(scale);
            assert!(px >= 60.0 || step == 600.0);
        }
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

    /// Ruler ticks fall on whole timecode seconds.
    #[test]
    fn ruler_ticks_follow_the_timecode() {
        let tick = ruler_tick_seconds(10.0, 30000, 1001);
        assert_eq!(timecode(tick, 30000, 1001), "00;00;10;00");
        assert_eq!(timecode(tick * 6.0, 30000, 1001), "00;01;00;02");
        assert_eq!(ruler_tick_seconds(10.0, 25, 1), 10.0);
        assert_eq!(ruler_tick_seconds(10.0, 0, 0), 10.0);
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
