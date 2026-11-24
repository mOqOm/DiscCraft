//! The pictures along a video clip in the Timeline viewer (M6.483). Encore shows, under a clip's name, the frame at
//! the clip's head and at every chapter point in it, each from its place to the next. The frames are decoded on a
//! thread of their own (the newest asked first, so what is on screen comes before what was scrolled past) and kept
//! as textures for the session, and on disk with the file's other facts for the next run. The thread decodes with a
//! decoder of its own and waits while the monitor plays (or the disc preview is open): decoding the pictures with
//! the monitor's shared decoder, beside playback, made the monitor stutter for the first minute after a project was
//! opened (M6.486).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

/// A picture: the asset and the frame number in its file.
pub(crate) type ThumbKey = (u64, i64);

/// The size the pictures are decoded at (16:9; a 4:3 picture comes with bars at the sides).
const THUMB_SIZE: (u32, u32) = (128, 72);
/// How many files the thread keeps open (the decoders are kept for the next picture of the same file).
const OPEN_FILES: usize = 3;
/// How many pictures are kept before the oldest are forgotten.
const KEPT: usize = 800;

/// A picture asked for.
struct Job {
    key: ThumbKey,
    path: PathBuf,
    seconds: f64,
    /// Where in the file the picture's stretch ends: a black frame is passed over for the first lit one before it.
    until: f64,
}

/// The decoded picture (RGB) or `None` when the frame could not be read.
type Decoded = Option<(u32, u32, Vec<u8>)>;

#[derive(Default)]
struct Shared {
    /// Pictures waiting to be decoded (the newest at the back, taken first).
    jobs: VecDeque<Job>,
    /// Pictures decoded and not yet made textures.
    done: Vec<(ThumbKey, Decoded)>,
    /// The thread was stopped.
    closed: bool,
    /// The thread waits (playback needs the computer).
    paused: bool,
}

/// The pictures decoded so far and the thread that decodes them.
pub(crate) struct ClipThumbs {
    shared: Arc<(Mutex<Shared>, Condvar)>,
    /// Textures by picture; `None` for a frame that could not be read.
    textures: HashMap<ThumbKey, Option<egui::TextureHandle>>,
    /// Pictures asked for and not yet back.
    asked: HashSet<ThumbKey>,
    /// The thread has been started.
    started: bool,
    /// What [`ClipThumbs::set_paused`] last said.
    paused: bool,
}

impl Default for ClipThumbs {
    fn default() -> Self {
        Self {
            shared: Arc::new((Mutex::new(Shared::default()), Condvar::new())),
            textures: HashMap::new(),
            asked: HashSet::new(),
            started: false,
            paused: false,
        }
    }
}

impl Drop for ClipThumbs {
    fn drop(&mut self) {
        if let Ok(mut shared) = self.shared.0.lock() {
            shared.closed = true;
            shared.jobs.clear();
        }
        self.shared.1.notify_all();
    }
}

impl ClipThumbs {
    /// The picture `key` (the frame at `seconds` in the file `path`, or the first lit one after it before `until`
    /// when it is black, as Encore finds bright pictures for the timeline) when it is ready; otherwise asks for it
    /// (once) and the viewer is drawn again when it comes.
    pub(crate) fn get(
        &mut self,
        ctx: &egui::Context,
        key: ThumbKey,
        path: &std::path::Path,
        (seconds, until): (f64, f64),
    ) -> Option<egui::TextureHandle> {
        self.collect(ctx);
        if let Some(texture) = self.textures.get(&key) {
            return texture.clone();
        }
        if self.asked.insert(key) {
            self.start(ctx);
            if let Ok(mut shared) = self.shared.0.lock() {
                shared.jobs.push_back(Job {
                    key,
                    path: path.to_path_buf(),
                    seconds: if seconds.is_finite() {
                        seconds.max(0.0)
                    } else {
                        0.0
                    },
                    until: if until.is_finite() { until } else { 0.0 },
                });
            }
            self.shared.1.notify_one();
        }
        None
    }

    /// Holds the thread while `paused` (the monitor is playing or the disc preview is open); a picture half
    /// looked for is put back and done afterwards.
    pub(crate) fn set_paused(&mut self, paused: bool) {
        if paused == self.paused {
            return;
        }
        self.paused = paused;
        if let Ok(mut shared) = self.shared.0.lock() {
            shared.paused = paused;
        }
        self.shared.1.notify_all();
    }

    /// Forgets every picture (a project closed, an asset replaced).
    pub(crate) fn clear(&mut self) {
        if let Ok(mut shared) = self.shared.0.lock() {
            shared.jobs.clear();
            shared.done.clear();
        }
        self.textures.clear();
        self.asked.clear();
    }

    /// Makes textures of the pictures that came back.
    fn collect(&mut self, ctx: &egui::Context) {
        let done = match self.shared.0.lock() {
            Ok(mut shared) => std::mem::take(&mut shared.done),
            Err(_) => return,
        };
        if self.textures.len() + done.len() > KEPT {
            self.textures.clear();
        }
        for (key, picture) in done {
            let _ = self.asked.remove(&key);
            let texture = picture.and_then(|(width, height, rgb)| {
                let size = [usize::try_from(width).ok()?, usize::try_from(height).ok()?];
                (rgb.len() == size[0].checked_mul(size[1])?.checked_mul(3)?).then(|| {
                    ctx.load_texture(
                        format!("clip_thumb_{}_{}", key.0, key.1),
                        egui::ColorImage::from_rgb(size, &rgb),
                        egui::TextureOptions::LINEAR,
                    )
                })
            });
            let _ = self.textures.insert(key, texture);
        }
    }

    /// Starts the thread the first time a picture is asked for.
    fn start(&mut self, ctx: &egui::Context) {
        if self.started {
            return;
        }
        self.started = true;
        let (shared, ctx) = (Arc::clone(&self.shared), ctx.clone());
        let spawned = std::thread::Builder::new()
            .name("disccraft-clip-thumbs".to_owned())
            .spawn(move || decode_thumbs(&shared, &ctx));
        if spawned.is_err()
            && let Ok(mut shared) = self.shared.0.lock()
        {
            shared.closed = true;
        }
    }
}

/// The thread: decodes the newest picture asked for, keeping a few files open.
fn decode_thumbs(shared: &(Mutex<Shared>, Condvar), ctx: &egui::Context) {
    let mut open: Vec<disccraft_engine::FrameSource> = Vec::new();
    loop {
        let job = {
            let Ok(mut state) = shared.0.lock() else {
                return;
            };
            loop {
                if state.closed {
                    return;
                }
                if !state.paused
                    && let Some(job) = state.jobs.pop_back()
                {
                    break job;
                }
                state = match shared.1.wait(state) {
                    Ok(state) => state,
                    Err(_) => return,
                };
            }
        };
        // Asked before each frame is decoded: playback has started, or the app is closing.
        let go_on = || {
            shared
                .0
                .lock()
                .map(|state| !state.paused && !state.closed)
                .unwrap_or(false)
        };
        // A panic in a decoder must not take the app down: the picture is just missing.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            decode_one(&mut open, &job, &go_on)
        }))
        .unwrap_or(Outcome::Done(None));
        if let Ok(mut state) = shared.0.lock() {
            match outcome {
                Outcome::Done(picture) => state.done.push((job.key, picture)),
                // Back at the front of the line, for when playback stops.
                Outcome::Stopped => state.jobs.push_back(job),
            }
        }
        ctx.request_repaint();
    }
}

/// How a picture came out.
enum Outcome {
    /// Decoded (or not readable: `None`).
    Done(Decoded),
    /// Left for later: the thread was paused while it looked.
    Stopped,
}

/// One picture: remembered from an earlier run, or decoded with a decoder of the thread's own (not the one the
/// monitor plays with: a picture far away in the same file moved it, a stutter each time), kept open for the next
/// picture of the file, and then remembered (M6.486).
fn decode_one(
    open: &mut Vec<disccraft_engine::FrameSource>,
    job: &Job,
    go_on: &dyn Fn() -> bool,
) -> Outcome {
    if let Some(frame) =
        disccraft_engine::remembered_timeline_picture(&job.path, job.seconds, job.until, THUMB_SIZE)
    {
        return Outcome::Done(Some((frame.width, frame.height, frame.rgb)));
    }
    if !go_on() {
        return Outcome::Stopped;
    }
    let index = match open.iter().position(|source| source.path() == job.path) {
        Some(index) => index,
        None => {
            let Ok(source) = disccraft_engine::FrameSource::open_own(&job.path) else {
                return Outcome::Done(None);
            };
            if open.len() >= OPEN_FILES {
                let _ = open.remove(0);
            }
            open.push(source);
            open.len() - 1
        }
    };
    let Some(source) = open.get(index) else {
        return Outcome::Done(None);
    };
    match source.lit_frame(job.seconds, THUMB_SIZE, job.until, go_on) {
        Ok(Some(frame)) => {
            disccraft_engine::remember_timeline_picture(
                &job.path,
                (job.seconds, job.until),
                THUMB_SIZE,
                &frame,
            );
            Outcome::Done(Some((frame.width, frame.height, frame.rgb)))
        }
        Ok(None) => Outcome::Stopped,
        Err(_) => Outcome::Done(None),
    }
}
