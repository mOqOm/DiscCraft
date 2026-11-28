//! The user manual's screenshots (M15.1): with `DISCCRAFT_CAPTURE` naming a script, the application plays it and
//! saves pictures of itself, so that the manual's pictures can be taken again the same way (in another language, after
//! the screen changed). Nothing of this runs without the variable.
//!
//! The script is JSON Lines (a `#` line is a comment); each line is one step, done in order, one a frame:
//!
//! - `{"size": [1600, 1000]}`: the window's inner size, in points (not maximized).
//! - `{"language": "ja"}`: the display language.
//! - `{"open": "path.dcproj"}`: open a project.
//! - `{"action": "Preferences"}`: a menu command, by its name in the code (`Action`).
//! - `{"select": "timeline", "id": 1}`: select a project item (asset, timeline, menu, slideshow, playlist,
//!   chapterPlaylist; `none`).
//! - `{"command": "disc.setName", "params": {...}}`: an engine command, as the panels send them.
//! - `{"show": "Flowchart"}`: bring a panel to the front (`Panel`; Window ▸ … would close one already in front).
//! - `{"playhead": 3.5}`: put the selected timeline's playhead at that second.
//! - `{"build": "iso", "path": "S:/out/disc.iso"}`: start a build (folder, iso or ddp) to that place, as the Build
//!   panel's Build button does.
//! - `{"prefs": "Media"}`: the page of Preferences.
//! - `{"preview": {"menu": 1, "button": 1}}`: open the Project Preview there (`preview.open`'s start: a menu's PGCN
//!   and button, or a title and chapter; `{}` from First Play).
//! - `{"remote": "Enter"}`: a key of the Project Preview's remote (Up, Down, Left, Right, Enter, Title, Menu; Next and
//!   Previous are its chapter buttons, M6.558).
//! - `{"key": "Escape"}`: a key press, by egui's name for the key, with modifiers before it (`Alt+F` opens the
//!   File menu, `ArrowDown`, `Enter`).
//! - `{"press": "Escape"}`: a key press as `key`, but given to the next frame's input as a real key is (so that egui
//!   itself sees it: a pick whip taken back with Escape, arrow keys moving through an open list).
//! - `{"text": "abc"}`: typed text, in the next frame's input (a field with the keyboard takes it).
//! - `{"pointer": [x, y]}`: the mouse pointer moves there (points), `"button": "press"` / `"release"` also presses or
//!   lets go of the left button there; moves in between a press and a release drag (the pick whip, M6.518).
//!   `"rightPress"` / `"rightRelease"` do the same with the right button (a tab's menu, M9.54).
//!   `{"pointer": null}` takes the pointer off the window (no hover in the pictures that follow).
//! - `{"wait": 30}`: frames to wait; `{"sleep": 1500}`: milliseconds to wait (media loading, playback);
//!   `{"built": 600000}`: wait until the build a `build` step started has ended, at most that many milliseconds
//!   (M6.560; a build still going then is a failure in the log).
//! - `{"shot": "name.png", ...}`: a picture saved in the output folder (`DISCCRAFT_CAPTURE_OUT`, else the script's
//!   folder), of the whole window, or cut to `"panel": "Timeline"` (a panel's contents), `"area": "prefs_window"`
//!   (a dialog, by its id), or `"rect": [x, y, w, h]` (points); `"pad": 8` widens the cut. `"viewport": "preview"`
//!   cuts out the Project Preview's window: while a script plays, other windows are drawn inside the main one
//!   (egui's embedded viewports), so that one picture holds them.
//! - `{"close": "preview"}`: close the Project Preview (as its window's close box does); `"check"` closes the Check
//!   Project window.
//! - `{"location": "S:/Samples"}`: the New Project dialog's location (its default is the user's own Documents folder,
//!   which a picture for others should not show).
//! - `{"marks": [[x, y, "1"], ...]}`: numbered marks drawn over the window (points) for the pictures that follow, to
//!   name its parts in the text; `{"marks": []}` takes them away.
//! - `{"expect": "/menus/14/endAction", "is": "notSet"}`: a check (M6.534): the value at that place in the project
//!   as it is saved (`/`-separated keys and array indices; a place that is not there is `null`) must be `is`.
//!   `{"expect": "pickWhip", "is": false}` checks whether a pick whip is being dragged; `"selection"` what is
//!   selected (`{"menu": 14}` as `select` names it, `"none"`: the disc), and `"viewer/menu"` (`timeline`,
//!   `slideshow`, `chapterPlaylist`) the id the viewer shows, `null` when it is empty (M6.540); `"preview/title"` the
//!   Project Preview's title and chapter (`[3, 1]`, `null` when no title plays; M6.558), and `"preview/variant"` the
//!   parental version it plays (M6.560: `0` the first, `null` for a title without parental chapters). A check that
//!   does not hold is a failure in the log, so a check script (`scripts/manual/checks/`) says by itself whether what
//!   it does works.
//! - `{"quit": true}`: leave without asking to save.
//!
//! What was done and what failed goes to `capture.log` beside the pictures.

use crate::app::{DiscCraftApp, Selection};
use egui::{Context, Ui};
use serde_json::Value;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The longest script read.
const MAX_SCRIPT_BYTES: u64 = 4 << 20;
/// The most frames one `wait` asks for.
const MAX_WAIT_FRAMES: u64 = 3_600;
/// The longest `built` wait, in milliseconds (M6.560).
const MAX_BUILD_WAIT_MS: u64 = 3_600_000;
/// The longest `sleep`, in milliseconds.
const MAX_SLEEP_MS: u64 = 120_000;
/// How long a screenshot may take to come back before the script goes on without it.
const SHOT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// A screenshot asked for and not yet saved.
struct PendingShot {
    /// Where the picture goes.
    path: PathBuf,
    /// Set by the screenshot's callback: the saving's outcome.
    done: Arc<Mutex<Option<Result<(), String>>>>,
    /// When it was asked for.
    asked: std::time::Instant,
}

/// A script being played.
pub(crate) struct DocCapture {
    steps: Vec<Value>,
    next: usize,
    /// Frames still to wait before the next step.
    wait_frames: u64,
    /// The time to wait until before the next step.
    wait_until: Option<std::time::Instant>,
    /// M6.560: waiting for the build to end, until this time at the most.
    wait_build: Option<std::time::Instant>,
    pending: Option<PendingShot>,
    /// Whether other windows are drawn inside the main one yet.
    embedded: bool,
    /// Numbered marks drawn over the window: where (points) and what.
    marks: Vec<(egui::Pos2, String)>,
    /// Pointer events of a `pointer` step, for the next frame's input (egui reads the pointer before a frame).
    input: Vec<egui::Event>,
    out: PathBuf,
    log: Option<std::fs::File>,
}

/// The script named by `DISCCRAFT_CAPTURE`, read; `None` without the variable (or with a script that cannot be read,
/// said on standard error).
pub(crate) fn from_env() -> Option<DocCapture> {
    let script = PathBuf::from(std::env::var_os("DISCCRAFT_CAPTURE")?);
    match load(&script) {
        Ok(capture) => Some(capture),
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "DISCCRAFT_CAPTURE: {error}");
            None
        }
    }
}

/// Reads the script `script`; the pictures go to `DISCCRAFT_CAPTURE_OUT`, else beside it.
fn load(script: &Path) -> Result<DocCapture, String> {
    let size = std::fs::metadata(script)
        .map_err(|error| format!("{}: {error}", script.display()))?
        .len();
    if size > MAX_SCRIPT_BYTES {
        return Err(format!("{}: the script is too large", script.display()));
    }
    let text = std::fs::read_to_string(script)
        .map_err(|error| format!("{}: {error}", script.display()))?;
    let steps = parse(&text)?;
    let out = std::env::var_os("DISCCRAFT_CAPTURE_OUT")
        .map(PathBuf::from)
        .or_else(|| script.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    std::fs::create_dir_all(&out).map_err(|error| format!("{}: {error}", out.display()))?;
    let log = std::fs::File::create(out.join("capture.log")).ok();
    Ok(DocCapture {
        steps,
        next: 0,
        wait_frames: 0,
        wait_until: None,
        wait_build: None,
        pending: None,
        embedded: false,
        marks: Vec::new(),
        input: Vec::new(),
        out,
        log,
    })
}

/// The steps of a script: one JSON object a line, blank lines and `#` lines left out.
fn parse(text: &str) -> Result<Vec<Value>, String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with('#')
        })
        .map(|(number, line)| {
            let value: Value = serde_json::from_str(line.trim())
                .map_err(|error| format!("line {}: {error}", number.saturating_add(1)))?;
            if value.is_object() {
                Ok(value)
            } else {
                Err(format!("line {}: not an object", number.saturating_add(1)))
            }
        })
        .collect()
}

/// A project item named by a script: its kind and id.
fn selection(kind: &str, id: u64) -> Option<Selection> {
    Some(match kind {
        "none" => Selection::None,
        "asset" => Selection::Asset(disccraft_project::AssetId(id)),
        "timeline" => Selection::Timeline(disccraft_project::TimelineId(id)),
        "menu" => Selection::Menu(disccraft_project::MenuId(id)),
        "slideshow" => Selection::Slideshow(id),
        "playlist" => Selection::Playlist(id),
        "chapterPlaylist" => Selection::ChapterPlaylist(id),
        _ => return None,
    })
}

/// `selection` as a check names it (M6.540): `{"menu": 14}`, as the `select` step names it, or `"none"` (the disc).
fn selection_value(selection: Selection) -> Value {
    let (kind, id) = match selection {
        Selection::None => return Value::from("none"),
        Selection::Asset(id) => ("asset", id.0),
        Selection::Timeline(id) => ("timeline", id.0),
        Selection::Menu(id) => ("menu", id.0),
        Selection::Slideshow(id) => ("slideshow", id),
        Selection::Playlist(id) => ("playlist", id),
        Selection::ChapterPlaylist(id) => ("chapterPlaylist", id),
    };
    serde_json::json!({ kind: id })
}

impl DiscCraftApp {
    /// The id the viewer of `kind` shows (`null` when it is empty), for a check (M6.540); `None` for no such viewer.
    fn viewer_value(&self, kind: &str) -> Option<Value> {
        let id = match kind {
            "menu" => self.viewer_menu().map(|id| id.0),
            "timeline" => self.viewer_timeline().map(|id| id.0),
            "slideshow" => self.viewer_slideshow(),
            "chapterPlaylist" => self.viewer_chapter_playlist(),
            _ => return None,
        };
        Some(id.map_or(Value::Null, Value::from))
    }
}

/// `rect` (points) in an image of `size` pixels at `scale` pixels a point, widened by `pad` points and kept inside
/// the image: the pixels' left, top, width and height, `None` when nothing is left.
fn crop_pixels(rect: egui::Rect, pad: f32, scale: f32, size: [usize; 2]) -> Option<[usize; 4]> {
    let rect = rect.expand(pad);
    let to_px = |value: f32, most: usize| -> usize {
        // `as` saturates: a negative or not-a-number value is 0, a huge one is the most.
        ((value * scale).round().max(0.0) as usize).min(most)
    };
    let left = to_px(rect.min.x, size[0]);
    let top = to_px(rect.min.y, size[1]);
    let right = to_px(rect.max.x, size[0]);
    let bottom = to_px(rect.max.y, size[1]);
    let width = right.checked_sub(left).filter(|width| *width > 0)?;
    let height = bottom.checked_sub(top).filter(|height| *height > 0)?;
    Some([left, top, width, height])
}

/// Saves the part `crop` (pixels: left, top, width, height; `None`: all) of `image` as an RGB PNG at `path`.
fn save_png(image: &egui::ColorImage, crop: Option<[usize; 4]>, path: &Path) -> Result<(), String> {
    let [width, height] = image.size;
    let [left, top, w, h] = crop.unwrap_or([0, 0, width, height]);
    let mut rgb = Vec::with_capacity(w.saturating_mul(h).saturating_mul(3));
    for y in top..top.saturating_add(h).min(height) {
        for x in left..left.saturating_add(w).min(width) {
            let pixel = image
                .pixels
                .get(y.saturating_mul(width).saturating_add(x))
                .copied()
                .unwrap_or_default();
            rgb.extend_from_slice(&[pixel.r(), pixel.g(), pixel.b()]);
        }
    }
    let w = u32::try_from(w.min(width.saturating_sub(left))).map_err(|error| error.to_string())?;
    let h = u32::try_from(h.min(height.saturating_sub(top))).map_err(|error| error.to_string())?;
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("{}: {error}", folder.display()))?;
    }
    let file =
        std::fs::File::create(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&rgb))
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// The marks of a picture: a blue disc with a white edge and its number, over everything else.
fn draw_marks(ctx: &Context, marks: &[(egui::Pos2, String)]) {
    if marks.is_empty() {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::unique("doc_capture_marks"),
    ));
    for (at, text) in marks {
        painter.circle(
            *at,
            13.0,
            egui::Color32::from_rgb(55, 142, 240),
            egui::Stroke::new(2.0, egui::Color32::WHITE),
        );
        painter.text(
            *at,
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(15.0),
            egui::Color32::WHITE,
        );
    }
}

impl DocCapture {
    fn note(&mut self, text: &str) {
        if let Some(log) = self.log.as_mut() {
            let _ = writeln!(log, "{text}");
        }
    }
}

impl DiscCraftApp {
    /// Plays the next step of the capture script, if one is running (called once a frame, before anything is drawn).
    /// The pointer events of the script's last `pointer` step, into the next frame's input (eframe's raw input hook).
    pub fn capture_input(&mut self, raw: &mut egui::RawInput) {
        if let Some(capture) = self.doc_capture.as_mut() {
            raw.events.append(&mut capture.input);
        }
    }

    pub(crate) fn play_capture(&mut self, ui: &Ui) {
        let Some(mut capture) = self.doc_capture.take() else {
            return;
        };
        // The script runs on its own: every frame is drawn, and other windows inside this one.
        ui.ctx().request_repaint();
        if !capture.embedded {
            ui.ctx().set_embed_viewports(true);
            capture.embedded = true;
        }
        let keep = self.capture_step(ui, &mut capture);
        draw_marks(ui.ctx(), &capture.marks);
        if keep {
            self.doc_capture = Some(capture);
        }
    }

    /// One frame of the script; `false` when it has ended.
    fn capture_step(&mut self, ui: &Ui, capture: &mut DocCapture) -> bool {
        if let Some(pending) = capture.pending.as_ref() {
            let outcome = pending.done.lock().ok().and_then(|mut done| done.take());
            match outcome {
                Some(Ok(())) => {
                    let text = format!("saved {}", pending.path.display());
                    capture.pending = None;
                    capture.note(&text);
                }
                Some(Err(error)) => {
                    capture.pending = None;
                    capture.note(&format!("shot failed: {error}"));
                }
                None if pending.asked.elapsed() > SHOT_TIMEOUT => {
                    let text = format!("shot timed out: {}", pending.path.display());
                    capture.pending = None;
                    capture.note(&text);
                }
                None => return true,
            }
        }
        if capture.wait_frames > 0 {
            capture.wait_frames -= 1;
            return true;
        }
        if let Some(until) = capture.wait_until {
            if std::time::Instant::now() < until {
                return true;
            }
            capture.wait_until = None;
        }
        if self.capture_waits_for_build(capture) {
            return true;
        }
        let Some(step) = capture.steps.get(capture.next).cloned() else {
            capture.note("end of script");
            return false;
        };
        capture.next = capture.next.saturating_add(1);
        capture.note(&step.to_string());
        // A step that changes the screen leaves it a few frames to settle before the next one.
        capture.wait_frames = 3;
        match self.capture_do(ui, capture, &step) {
            Ok(true) => true,
            Ok(false) => false,
            Err(error) => {
                capture.note(&format!("  failed: {error}"));
                true
            }
        }
    }

    /// Does `step`; `Ok(false)` to end the script.
    fn capture_do(
        &mut self,
        ui: &Ui,
        capture: &mut DocCapture,
        step: &Value,
    ) -> Result<bool, String> {
        let ctx = ui.ctx();
        let text = |key: &str| step.get(key).and_then(Value::as_str);
        if let Some(size) = step.get("size").and_then(Value::as_array) {
            let number = |index: usize| {
                size.get(index)
                    .and_then(Value::as_f64)
                    // `as`: a window's size in points, well within f32.
                    .map(|value| value.clamp(320.0, 8192.0) as f32)
            };
            let (Some(width), Some(height)) = (number(0), number(1)) else {
                return Err("size: [width, height]".to_owned());
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(width, height)));
            capture.wait_frames = 20;
        } else if let Some(code) = text("language") {
            let language = crate::i18n::Language::ALL
                .into_iter()
                .find(|language| language.code() == code)
                .ok_or_else(|| format!("no language {code}"))?;
            self.set_language(language);
        } else if let Some(path) = text("open") {
            self.open_path(Path::new(path))?;
            capture.wait_frames = 30;
        } else if let Some(name) = text("action") {
            let action = crate::actions::Action::ALL
                .into_iter()
                .find(|action| format!("{action:?}") == name)
                .ok_or_else(|| format!("no action {name}"))?;
            self.run_action(ui, action);
            capture.wait_frames = 10;
        } else if let Some(kind) = text("select") {
            let id = step.get("id").and_then(Value::as_u64).unwrap_or(0);
            let chosen = selection(kind, id).ok_or_else(|| format!("no item kind {kind}"))?;
            self.select(chosen);
            capture.wait_frames = 10;
        } else if let Some(id) = text("command") {
            let params = step.get("params").cloned().unwrap_or(Value::Null);
            if self.run_command(id, params).is_none() {
                return Err(format!("{id}: {}", self.status));
            }
        } else if let Some(name) = text("show") {
            let panel = crate::workspace::Panel::ALL
                .into_iter()
                .find(|panel| format!("{panel:?}") == name)
                .ok_or_else(|| format!("no panel {name}"))?;
            self.show_panel(panel);
            capture.wait_frames = 10;
        } else if let Some(seconds) = step.get("playhead").and_then(Value::as_f64) {
            let Selection::Timeline(id) = self.selection else {
                return Err("playhead: select a timeline first".to_owned());
            };
            crate::timeline_view::set_playhead_seconds(ctx, id, seconds.max(0.0));
            capture.wait_frames = 20;
        } else if let Some(kind) = text("build") {
            self.build_output = match kind {
                "folder" => disccraft_build::OutputKind::Folder,
                "iso" => disccraft_build::OutputKind::Iso,
                "ddp" => disccraft_build::OutputKind::Ddp,
                _ => return Err(format!("no build output {kind}")),
            };
            self.build_path = text("path").unwrap_or_default().to_owned();
            self.start_build();
            capture.wait_frames = 10;
        } else if let Some(page) = text("prefs") {
            let category = crate::prefs::PrefsCategory::ALL
                .into_iter()
                .find(|category| format!("{category:?}") == page)
                .ok_or_else(|| format!("no Preferences page {page}"))?;
            self.prefs.open = true;
            self.prefs.category = category;
        } else if let Some(start) = step.get("preview") {
            self.open_project_preview(start.clone());
            capture.wait_frames = 20;
        } else if let Some(name @ ("Next" | "Previous")) = text("remote") {
            self.capture_chapter_key(name == "Next");
            capture.wait_frames = 10;
        } else if let Some(name) = text("remote") {
            use crate::monitor::RemoteKey;
            let key = match name {
                "Up" => RemoteKey::Up,
                "Down" => RemoteKey::Down,
                "Left" => RemoteKey::Left,
                "Right" => RemoteKey::Right,
                "Enter" => RemoteKey::Enter,
                "Title" => RemoteKey::Title,
                "Menu" => RemoteKey::Menu,
                _ => return Err(format!("no remote key {name}")),
            };
            self.send_remote(key);
            capture.wait_frames = 10;
        } else if let Some(name) = text("key") {
            let (key, modifiers) = key_press(name).ok_or_else(|| format!("no key {name}"))?;
            for pressed in [true, false] {
                ctx.input_mut(|input| {
                    input.events.push(egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers,
                    });
                });
            }
        } else if let Some(name) = text("press") {
            let (key, modifiers) = key_press(name).ok_or_else(|| format!("no key {name}"))?;
            capture.input = [true, false]
                .into_iter()
                .map(|pressed| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers,
                })
                .collect();
            capture.wait_frames = 2;
        } else if let Some(typed) = text("text") {
            capture.input = vec![egui::Event::Text(typed.to_owned())];
            capture.wait_frames = 2;
        } else if let Some(at) = step.get("pointer") {
            capture.input = pointer_events(at, text("button"))?;
            capture.wait_frames = 2;
        } else if let Some(frames) = step.get("wait").and_then(Value::as_u64) {
            capture.wait_frames = frames.min(MAX_WAIT_FRAMES);
        } else if let Some(ms) = step.get("built").and_then(Value::as_u64) {
            capture.wait_build = Some(
                std::time::Instant::now()
                    + std::time::Duration::from_millis(ms.min(MAX_BUILD_WAIT_MS)),
            );
            capture.wait_frames = 10;
        } else if let Some(ms) = step.get("sleep").and_then(Value::as_u64) {
            capture.wait_until = Some(
                std::time::Instant::now() + std::time::Duration::from_millis(ms.min(MAX_SLEEP_MS)),
            );
        } else if let Some(name) = text("shot") {
            self.capture_shot(ctx, capture, step, name)?;
        } else if let Some(name) = text("close") {
            match name {
                "preview" => self.close_project_preview_window(ctx),
                "check" => self.check_window.open = false,
                _ => return Err(format!("cannot close {name}")),
            }
            capture.wait_frames = 10;
        } else if let Some(folder) = text("location") {
            let form = self
                .welcome
                .new_project
                .as_mut()
                .ok_or_else(|| "the New Project dialog is not open".to_owned())?;
            form.location = folder.to_owned();
        } else if let Some(marks) = step.get("marks").and_then(Value::as_array) {
            capture.marks = marks
                .iter()
                .filter_map(|mark| {
                    let mark = mark.as_array()?;
                    // `as`: points on a screen.
                    let x = mark.first()?.as_f64()? as f32;
                    let y = mark.get(1)?.as_f64()? as f32;
                    let text = mark.get(2)?.as_str()?.chars().take(4).collect();
                    Some((egui::pos2(x, y), text))
                })
                .collect();
            capture.wait_frames = 5;
        } else if let Some(place) = text("expect") {
            let wanted = step.get("is").cloned().unwrap_or(Value::Null);
            let found = self.expected_place(ctx, place)?;
            if found != wanted {
                return Err(format!("expected {place} to be {wanted}, it is {found}"));
            }
            capture.note("  ok");
            capture.wait_frames = 0;
        } else if step.get("quit").is_some() {
            self.session.dirty = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return Ok(false);
        } else {
            return Err("unknown step".to_owned());
        }
        Ok(true)
    }

    /// M6.560: whether a `built` step still waits for the build (it goes on when the build has ended, or at its
    /// deadline with a failure in the log).
    fn capture_waits_for_build(&self, capture: &mut DocCapture) -> bool {
        let Some(deadline) = capture.wait_build else {
            return false;
        };
        let running = self.build_job.is_running();
        if running && std::time::Instant::now() < deadline {
            return true;
        }
        capture.wait_build = None;
        if running {
            capture.note("  failed: the build did not end in time");
        }
        false
    }

    /// M6.558: the Project Preview's chapter button, Next (`next`) or Previous, as its window's button presses it.
    fn capture_chapter_key(&mut self, next: bool) {
        self.preview_paused = false;
        let key = if next { "next" } else { "previous" };
        if let Some(state) = self.run_query("preview.key", serde_json::json!({ "key": key })) {
            self.preview = crate::monitor::parse_preview_state(&state);
        }
    }

    /// What an `expect` step's `place` is now (see the module's notes).
    fn expected_place(&self, ctx: &Context, place: &str) -> Result<Value, String> {
        Ok(if place == "pickWhip" {
            Value::Bool(crate::pickwhip::any_dragged(ctx))
        } else if place == "selection" {
            selection_value(self.selection)
        } else if place == "preview/variant" {
            self.preview
                .variant
                .map_or(Value::Null, |variant| serde_json::json!(variant))
        } else if place == "preview/title" {
            // M6.558: the title and chapter the Project Preview plays.
            self.preview.title.map_or(Value::Null, |(title, chapter)| {
                serde_json::json!([title, chapter])
            })
        } else if let Some(kind) = place.strip_prefix("viewer/") {
            self.viewer_value(kind)
                .ok_or_else(|| format!("no viewer {kind}"))?
        } else {
            let project =
                serde_json::to_value(&self.session.project).map_err(|error| error.to_string())?;
            value_at(&project, place).cloned().unwrap_or(Value::Null)
        })
    }

    /// Asks for the picture of `step` (see the module's notes), saved as `name` in the output folder.
    fn capture_shot(
        &mut self,
        ctx: &Context,
        capture: &mut DocCapture,
        step: &Value,
        name: &str,
    ) -> Result<(), String> {
        if name.contains("..") || Path::new(name).is_absolute() {
            return Err(format!("{name}: a name inside the output folder"));
        }
        let pad = step.get("pad").and_then(Value::as_f64).unwrap_or(0.0);
        // `as`: a few points.
        let pad = pad.clamp(0.0, 200.0) as f32;
        let rect = if let Some(panel) = step.get("panel").and_then(Value::as_str) {
            let found = self
                .panel_bodies
                .iter()
                .find(|(shown, _)| format!("{shown:?}") == panel)
                .map(|(_, rect)| *rect)
                .ok_or_else(|| format!("panel {panel} is not shown"))?;
            Some(found)
        } else if step.get("viewport").and_then(Value::as_str) == Some("preview") {
            let id = egui::Id::unique(egui::ViewportId::from_hash_of("disccraft_project_preview"));
            let found = ctx
                .memory(|memory| memory.area_rect(id))
                .ok_or_else(|| "the Project Preview is not open".to_owned())?;
            Some(found)
        } else if let Some(area) = step.get("area").and_then(Value::as_str) {
            let id = egui::Id::unique(area);
            let found = ctx
                .memory(|memory| memory.area_rect(id))
                .ok_or_else(|| format!("no area {area}"))?;
            Some(found)
        } else if let Some(rect) = step.get("rect").and_then(Value::as_array) {
            let number = |index: usize| {
                rect.get(index)
                    .and_then(Value::as_f64)
                    // `as`: points on a screen.
                    .map(|value| value as f32)
                    .unwrap_or(0.0)
            };
            Some(egui::Rect::from_min_size(
                egui::pos2(number(0), number(1)),
                egui::vec2(number(2), number(3)),
            ))
        } else {
            None
        };
        let path = capture.out.join(name);
        let done = Arc::new(Mutex::new(None));
        let reply = Arc::clone(&done);
        let scale = ctx.pixels_per_point();
        let target = path.clone();
        let callback = egui::ScreenshotCallback::new(move |image: Arc<egui::ColorImage>| {
            let crop = rect.map(|rect| crop_pixels(rect, pad, scale, image.size));
            let outcome = match crop {
                Some(None) => Err("the cut is outside the window".to_owned()),
                Some(Some(crop)) => save_png(&image, Some(crop), &target),
                None => save_png(&image, None, &target),
            };
            if let Ok(mut slot) = reply.lock() {
                *slot = Some(outcome);
            }
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(callback));
        capture.pending = Some(PendingShot {
            path,
            done,
            asked: std::time::Instant::now(),
        });
        Ok(())
    }
}

/// A pointer step's events: a move to `at` (`[x, y]` in points), then a press or a release of the left button there.
/// The value at `place` in `root`: `/`-separated keys of objects and indices of arrays (`/menus/14/buttons/0/link`);
/// `None` when it is not there.
fn value_at<'a>(root: &'a Value, place: &str) -> Option<&'a Value> {
    place
        .split('/')
        .filter(|key| !key.is_empty())
        .try_fold(root, |value, key| match value {
            Value::Object(map) => map.get(key),
            Value::Array(items) => items.get(key.parse::<usize>().ok()?),
            _ => None,
        })
}

fn pointer_events(at: &Value, button: Option<&str>) -> Result<Vec<egui::Event>, String> {
    if at.is_null() {
        return Ok(vec![egui::Event::PointerGone]);
    }
    let point = at
        .as_array()
        .and_then(|xy| Some((xy.first()?.as_f64()?, xy.get(1)?.as_f64()?)))
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .ok_or_else(|| format!("pointer {at}: not [x, y]"))?;
    let pos = egui::pos2(point.0 as f32, point.1 as f32);
    let mut events = vec![egui::Event::PointerMoved(pos)];
    let (button, pressed) = match button {
        None => return Ok(events),
        Some("press") => (egui::PointerButton::Primary, true),
        Some("release") => (egui::PointerButton::Primary, false),
        Some("rightPress") => (egui::PointerButton::Secondary, true),
        Some("rightRelease") => (egui::PointerButton::Secondary, false),
        Some(other) => return Err(format!("no button {other}")),
    };
    events.push(egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    });
    Ok(events)
}

/// A key step's key and modifiers: `Escape`, `Alt+F`, `Ctrl+Shift+S` (egui's key names).
fn key_press(name: &str) -> Option<(egui::Key, egui::Modifiers)> {
    let mut modifiers = egui::Modifiers::NONE;
    let mut parts: Vec<&str> = name.split('+').collect();
    let key = egui::Key::from_name(parts.pop()?)?;
    for part in parts {
        match part {
            "Alt" => modifiers.alt = true,
            "Ctrl" => {
                modifiers.ctrl = true;
                modifiers.command = true;
            }
            "Shift" => modifiers.shift = true,
            _ => return None,
        }
    }
    Some((key, modifiers))
}

#[cfg(test)]
mod tests {
    use super::{Value, crop_pixels, parse, pointer_events, selection, selection_value, value_at};

    #[test]
    fn scripts_are_json_lines_with_comments() {
        let steps = parse("# a comment\n\n{\"wait\": 3}\n  {\"shot\": \"a.png\"}\n");
        assert_eq!(steps.map(|steps| steps.len()), Ok(2));
        assert!(parse("{\"wait\": 3}\n[1, 2]\n").is_err());
        assert!(parse("{nope}\n").is_err());
        assert!(selection("menu", 2).is_some());
        assert!(selection("button", 2).is_none());
    }

    /// M6.534: a check finds its place in the project as it is saved; M6.540: a selection is named as `select`
    /// names it.
    #[test]
    fn a_check_finds_its_place_in_the_project() {
        use crate::app::Selection;
        assert_eq!(
            selection_value(Selection::Menu(disccraft_project::MenuId(14))),
            serde_json::json!({ "menu": 14 })
        );
        assert_eq!(selection_value(Selection::None), Value::from("none"));
        assert_eq!(
            selection_value(Selection::ChapterPlaylist(3)),
            serde_json::json!({ "chapterPlaylist": 3 })
        );
        let project = serde_json::json!({
            "menus": { "14": { "endAction": "notSet", "buttons": [{ "link": { "timeline": 11 } }] } }
        });
        assert_eq!(
            value_at(&project, "/menus/14/endAction"),
            Some(&Value::from("notSet"))
        );
        assert_eq!(
            value_at(&project, "/menus/14/buttons/0/link"),
            Some(&serde_json::json!({ "timeline": 11 }))
        );
        assert_eq!(value_at(&project, "/menus/14/buttons/1/link"), None);
        assert_eq!(value_at(&project, "/menus/14/endAction/x"), None);
        assert_eq!(value_at(&project, "/"), Some(&project));
    }

    /// M6.518: a pointer step moves the pointer, and presses or lets go of the left button there.
    #[test]
    fn pointer_steps_move_press_and_release() {
        let at = serde_json::json!([120, 48.5]);
        let moved = egui::Event::PointerMoved(egui::pos2(120.0, 48.5));
        assert_eq!(pointer_events(&at, None), Ok(vec![moved.clone()]));
        let pressed = pointer_events(&at, Some("press")).unwrap_or_default();
        assert!(matches!(
            pressed.as_slice(),
            [_, egui::Event::PointerButton { pressed: true, .. }]
        ));
        let released = pointer_events(&at, Some("release")).unwrap_or_default();
        assert!(matches!(
            released.as_slice(),
            [_, egui::Event::PointerButton { pressed: false, .. }]
        ));
        assert!(pointer_events(&at, Some("double")).is_err());
        assert!(pointer_events(&serde_json::json!([1]), None).is_err());
        assert_eq!(
            pointer_events(&serde_json::Value::Null, None),
            Ok(vec![egui::Event::PointerGone])
        );
        assert!(pointer_events(&serde_json::json!("1, 2"), None).is_err());
    }

    #[test]
    fn cuts_stay_inside_the_picture() {
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(100.0, 50.0));
        assert_eq!(
            crop_pixels(rect, 0.0, 1.0, [1000, 800]),
            Some([10, 20, 100, 50])
        );
        assert_eq!(
            crop_pixels(rect, 5.0, 2.0, [1000, 800]),
            Some([10, 30, 220, 120])
        );
        // Past the edges: cut to the picture; wholly outside: nothing.
        assert_eq!(
            crop_pixels(rect, 0.0, 1.0, [60, 40]),
            Some([10, 20, 50, 20])
        );
        let outside = egui::Rect::from_min_size(egui::pos2(900.0, 900.0), egui::vec2(10.0, 10.0));
        assert_eq!(crop_pixels(outside, 0.0, 1.0, [100, 100]), None);
    }
}
