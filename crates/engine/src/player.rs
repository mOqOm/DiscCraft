//! Plays a video asset in the monitor: a worker thread decodes the frame for the current time, in real time, and
//! keeps only the latest one; the UI takes it on every repaint. When decoding falls behind, frames are skipped,
//! not delayed.
//!
//! With sound (ROADMAP M6.175), a second worker decodes the audio ahead into a queue that the audio output's
//! callback takes from ([`AudioFeed::fill`]). Both follow one clock, started when the first picture is ready:
//! when the sound falls behind it (a late start, a slow decode), the late samples are skipped.

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use disccraft_transcode::{AudioReader, FrameReader};

/// The longest the worker sleeps before looking at the clock again.
const MAX_SLEEP: Duration = Duration::from_millis(20);
/// How far ahead of the clock the audio worker decodes, in seconds.
const AUDIO_AHEAD: f64 = 0.5;
/// How much sound one decode adds, in seconds.
const AUDIO_CHUNK: f64 = 0.1;
/// How far the sound may lag the clock before the late samples are skipped, in seconds.
const AUDIO_LATE: f64 = 0.08;
/// The output rates a player decodes for (Hz).
const AUDIO_RATES: std::ops::RangeInclusive<u32> = 8_000..=384_000;

/// The sound of a playback: the file to take it from, and the output's rate.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerAudio {
    /// An audio file, or a video file with sound.
    pub path: PathBuf,
    /// The output device's sample rate (Hz).
    pub rate: u32,
    /// Where in the audio file to start (seconds), when that is not the picture's start position: a timeline's
    /// audio track runs along the whole timeline while the picture is one of its clips.
    pub from: Option<f64>,
}

#[derive(Debug, Default)]
struct AudioQueue {
    /// Decoded sound not yet played: interleaved stereo.
    samples: VecDeque<f32>,
    /// Frames taken from the start position on (played, or skipped to catch up with the clock).
    taken: u64,
    /// Frames the decoder should skip before decoding more (taken beyond what was queued).
    skip: u64,
    /// The decoder has stopped: the end of the sound, or an error.
    done: bool,
    /// Why there is no sound, when the decoder failed.
    error: Option<String>,
}

#[derive(Debug)]
struct AudioShared {
    rate: u32,
    /// When the picture started (the playback clock's zero), set once the first frame is decoded.
    clock: Arc<OnceLock<Instant>>,
    queue: Mutex<AudioQueue>,
}

/// The sound of a playing [`Player`] for the audio output's callback: interleaved stereo at the output's rate,
/// kept on the picture's clock.
#[derive(Clone, Debug)]
pub struct AudioFeed {
    shared: Arc<AudioShared>,
}

impl AudioFeed {
    /// Fills `out` (interleaved, `channels` per frame) with the next sound: left and right on the first two
    /// channels (their mix on a single one), silence on the rest and while nothing is decoded yet.
    pub fn fill(&self, out: &mut [f32], channels: usize) {
        out.fill(0.0);
        if channels == 0 {
            return;
        }
        let Some(started) = self.shared.clock.get() else {
            return;
        };
        let Ok(mut queue) = self.shared.queue.lock() else {
            return;
        };
        // Catch up with the clock: skip what should already have been heard.
        let rate = f64::from(self.shared.rate);
        // `as` saturates; the clock is never negative.
        let due = (started.elapsed().as_secs_f64() * rate) as u64;
        let late = due.saturating_sub(queue.taken);
        if late as f64 > AUDIO_LATE * rate {
            let queued = u64::try_from(queue.samples.len() / 2).unwrap_or(u64::MAX);
            let dropped = late.min(queued);
            let drop_samples = usize::try_from(dropped.saturating_mul(2)).unwrap_or(usize::MAX);
            let drop_samples = drop_samples.min(queue.samples.len());
            queue.samples.drain(..drop_samples);
            queue.skip = queue.skip.saturating_add(late.saturating_sub(dropped));
            queue.taken = queue.taken.saturating_add(late);
        }
        let mut taken = 0u64;
        for frame in out.chunks_mut(channels) {
            if queue.samples.len() < 2 {
                break;
            }
            let left = queue.samples.pop_front().unwrap_or(0.0);
            let right = queue.samples.pop_front().unwrap_or(0.0);
            match frame {
                [mono] => *mono = (left + right) * 0.5,
                [first, second, ..] => {
                    *first = left;
                    *second = right;
                }
                [] => {}
            }
            taken = taken.saturating_add(1);
        }
        queue.taken = queue.taken.saturating_add(taken);
    }

    /// The output's sample rate this sound is decoded at.
    pub fn rate(&self) -> u32 {
        self.shared.rate
    }

    /// Why there is no sound, when the audio could not be decoded.
    pub fn error(&self) -> Option<String> {
        self.shared
            .queue
            .lock()
            .ok()
            .and_then(|queue| queue.error.clone())
    }
}

/// One decoded frame.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerFrame {
    /// Position of the frame in the video, in seconds.
    pub seconds: f64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// RGB24 pixels, row by row.
    pub rgb: Vec<u8>,
}

/// A video file held open for the monitor's still frames: scrubbing asks for frame after frame and reuses the
/// decoder. Frames come at a size (square pixels, the picture fitted in with bars), as the player gives them.
pub struct FrameSource {
    reader: FrameReader,
    path: PathBuf,
}

impl std::fmt::Debug for FrameSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameSource")
            .field("path", &self.path)
            .finish()
    }
}

impl FrameSource {
    /// Opens a video file.
    ///
    /// # Errors
    ///
    /// When the file cannot be read as a video.
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        Ok(Self {
            reader: FrameReader::open(path).map_err(|error| error.to_string())?,
            path: path.to_path_buf(),
        })
    }

    /// Whether `path` opens without a wait: it is open already (or was found not to be a video). When it is not,
    /// it is opened on a thread of its own from here on, and this is asked again later.
    #[must_use]
    pub fn ready(path: &std::path::Path) -> bool {
        if disccraft_transcode::media_open(path) {
            return true;
        }
        disccraft_transcode::warm(path);
        false
    }

    /// The file it reads.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// The frame at `seconds` (clamped to the file) at `size` (width, height).
    ///
    /// # Errors
    ///
    /// When the frame cannot be decoded.
    pub fn frame(&self, seconds: f64, (width, height): (u32, u32)) -> Result<PlayerFrame, String> {
        let (width, height, rgb) = self
            .reader
            .frame_sized(seconds, width, height)
            .map_err(|error| error.to_string())?;
        Ok(PlayerFrame {
            seconds,
            width,
            height,
            rgb,
        })
    }
}

/// One still frame asked of a [`FrameLoader`].
#[derive(Clone, Debug, PartialEq)]
pub struct FrameRequest {
    /// The video file.
    pub path: PathBuf,
    /// The frame's time in the file, seconds.
    pub seconds: f64,
    /// The size it is wanted at (width, height).
    pub size: (u32, u32),
    /// The caller's name for it (an asset and a frame number, say), given back with the frame.
    pub key: (u64, i64),
}

#[derive(Default)]
struct LoaderState {
    /// The newest frame asked for and not yet started on.
    request: Option<FrameRequest>,
    /// The last frame decoded and not yet taken.
    result: Option<(FrameRequest, Result<PlayerFrame, String>)>,
    /// A frame is being decoded.
    working: bool,
    /// The loader was dropped.
    closed: bool,
}

/// Decodes the monitor's still frames on a thread of its own, so the interface never waits for a decoder (or for
/// a long file to open). Only the newest request is kept: while a frame is being decoded, a later request
/// replaces the one waiting, as scrubbing asks for frame after frame.
pub struct FrameLoader {
    state: Arc<(Mutex<LoaderState>, std::sync::Condvar)>,
}

impl std::fmt::Debug for FrameLoader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameLoader").finish_non_exhaustive()
    }
}

impl Default for FrameLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameLoader {
    /// Starts the loader's thread (when it cannot be started, requests are answered with an error).
    #[must_use]
    pub fn new() -> Self {
        let state = Arc::new((
            Mutex::new(LoaderState::default()),
            std::sync::Condvar::new(),
        ));
        let worker = Arc::clone(&state);
        let spawned = thread::Builder::new()
            .name("disccraft-frames".to_string())
            .spawn(move || load_frames(&worker));
        if spawned.is_err()
            && let Ok(mut state) = state.0.lock()
        {
            state.closed = true;
        }
        Self { state }
    }

    /// Asks for a frame, in place of a request that is still waiting.
    pub fn request(&self, request: FrameRequest) {
        let Ok(mut state) = self.state.0.lock() else {
            return;
        };
        if state.closed {
            let error = Err("the frame loader is not running".to_string());
            state.result = Some((request, error));
            return;
        }
        state.request = Some(request);
        self.state.1.notify_one();
    }

    /// The newest decoded frame (or why it could not be decoded) since the last call, with its request.
    pub fn take(&self) -> Option<(FrameRequest, Result<PlayerFrame, String>)> {
        self.state.0.lock().ok()?.result.take()
    }

    /// Whether a frame is waiting or being decoded.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.state
            .0
            .lock()
            .is_ok_and(|state| state.working || state.request.is_some())
    }
}

impl Drop for FrameLoader {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.0.lock() {
            state.closed = true;
            state.request = None;
        }
        self.state.1.notify_one();
    }
}

/// The loader's thread: decode the newest request, keeping the file open for the next.
fn load_frames(shared: &(Mutex<LoaderState>, std::sync::Condvar)) {
    let mut source: Option<FrameSource> = None;
    loop {
        let request = {
            let Ok(mut state) = shared.0.lock() else {
                return;
            };
            loop {
                if state.closed {
                    return;
                }
                if let Some(request) = state.request.take() {
                    state.working = true;
                    break request;
                }
                state = match shared.1.wait(state) {
                    Ok(state) => state,
                    Err(_) => return,
                };
            }
        };
        let decoded = catch_unwind(AssertUnwindSafe(|| {
            if source
                .as_ref()
                .is_none_or(|open| open.path() != request.path.as_path())
            {
                source = None;
                source = Some(FrameSource::open(&request.path)?);
            }
            match &source {
                Some(open) => open.frame(request.seconds, request.size),
                None => Err("the video is not open".to_string()),
            }
        }))
        .unwrap_or_else(|_| Err("the frame decoder stopped unexpectedly".to_string()));
        let Ok(mut state) = shared.0.lock() else {
            return;
        };
        state.working = false;
        state.result = Some((request, decoded));
    }
}

/// Where playback is.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayerStatus {
    /// Frames are coming.
    Playing,
    /// The end of the video was reached.
    Ended,
    /// The video could not be decoded (or the worker failed).
    Failed(String),
}

#[derive(Debug)]
struct Shared {
    frame: Option<PlayerFrame>,
    status: PlayerStatus,
    /// Why only the sound plays, when the picture cannot be decoded.
    no_picture: Option<String>,
}

/// Real-time playback of one video file.
#[derive(Debug)]
pub struct Player {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    audio: Option<(AudioFeed, Option<JoinHandle<()>>)>,
}

impl Player {
    /// Starts playing `path` from `from_seconds` at `fps` frames per second, without sound.
    pub fn start(path: PathBuf, from_seconds: f64, fps: f64) -> Self {
        Self::start_with_audio(path, None, from_seconds, fps)
    }

    /// Starts playing `path` from `from_seconds` at `fps` frames per second, with the sound of `audio` from the
    /// same position for the output to take ([`Player::audio`]).
    pub fn start_with_audio(
        path: PathBuf,
        audio: Option<PlayerAudio>,
        from_seconds: f64,
        fps: f64,
    ) -> Self {
        Self::start_sized(path, audio, from_seconds, fps, None)
    }

    /// [`Player::start_with_audio`], with the frames at `size` (width, height; square pixels, the picture fitted in
    /// with bars, as a Blu-ray HD title shows) instead of the 720 x 480 letterboxed frame.
    pub fn start_sized(
        path: PathBuf,
        audio: Option<PlayerAudio>,
        from_seconds: f64,
        fps: f64,
        size: Option<(u32, u32)>,
    ) -> Self {
        let shared = Arc::new(Mutex::new(Shared {
            frame: None,
            status: PlayerStatus::Playing,
            no_picture: None,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let clock = Arc::new(OnceLock::new());
        let worker_shared = Arc::clone(&shared);
        let worker_stop = Arc::clone(&stop);
        let worker_clock = Arc::clone(&clock);
        let from = if from_seconds.is_finite() {
            from_seconds.max(0.0)
        } else {
            0.0
        };
        let fps = if fps.is_finite() && fps > 0.0 {
            fps
        } else {
            30.0
        };
        // With sound to play, a picture that cannot be decoded does not stop the playback.
        let sound = audio.is_some();

        let worker = thread::Builder::new()
            .name("disccraft-player".to_string())
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    play(
                        &path,
                        (from, fps, size, sound),
                        &worker_shared,
                        &worker_stop,
                        &worker_clock,
                    )
                }));
                let status = match result {
                    Ok(Ok(status)) => status,
                    Ok(Err(message)) => PlayerStatus::Failed(message),
                    Err(_) => PlayerStatus::Failed("the player stopped unexpectedly".to_string()),
                };
                set_status(&worker_shared, status);
            });
        let worker = match worker {
            Ok(handle) => Some(handle),
            Err(error) => {
                set_status(&shared, PlayerStatus::Failed(error.to_string()));
                None
            }
        };

        let audio = audio
            .filter(|audio| AUDIO_RATES.contains(&audio.rate))
            .map(|audio| {
                let audio_from = audio
                    .from
                    .filter(|at| at.is_finite())
                    .map_or(from, |at| at.max(0.0));
                start_audio(audio, audio_from, clock, &stop)
            });

        Self {
            shared,
            stop,
            worker,
            audio,
        }
    }

    /// The sound for the audio output, when the player was started with some.
    pub fn audio(&self) -> Option<AudioFeed> {
        self.audio.as_ref().map(|(feed, _)| feed.clone())
    }

    /// Why only the sound is playing, when the picture cannot be decoded (the frames are black then).
    pub fn no_picture(&self) -> Option<String> {
        self.shared
            .lock()
            .ok()
            .and_then(|shared| shared.no_picture.clone())
    }

    /// The newest frame since the last call, if any.
    pub fn take_frame(&self) -> Option<PlayerFrame> {
        self.shared
            .lock()
            .ok()
            .and_then(|mut shared| shared.frame.take())
    }

    /// Where playback is.
    pub fn status(&self) -> PlayerStatus {
        self.shared.lock().map_or_else(
            |_| PlayerStatus::Failed("the player state is unavailable".to_string()),
            |shared| shared.status.clone(),
        )
    }

    /// Stops playback and waits for the workers.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Some(worker) = self.audio.as_mut().and_then(|(_, worker)| worker.take()) {
            let _ = worker.join();
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}

fn set_status(shared: &Mutex<Shared>, status: PlayerStatus) {
    if let Ok(mut shared) = shared.lock() {
        // A failure or the end is final; keep the first one.
        if shared.status == PlayerStatus::Playing {
            shared.status = status;
        }
    }
}

/// Starts the audio worker: it decodes `audio` from `from` seconds into the feed's queue, ahead of the clock.
fn start_audio(
    audio: PlayerAudio,
    from: f64,
    clock: Arc<OnceLock<Instant>>,
    stop: &Arc<AtomicBool>,
) -> (AudioFeed, Option<JoinHandle<()>>) {
    let feed = AudioFeed {
        shared: Arc::new(AudioShared {
            rate: audio.rate,
            clock,
            queue: Mutex::new(AudioQueue::default()),
        }),
    };
    let worker_feed = feed.clone();
    let worker_stop = Arc::clone(stop);
    let worker = thread::Builder::new()
        .name("disccraft-player-audio".to_string())
        .spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| {
                decode_audio(&audio, from, &worker_feed.shared, &worker_stop)
            }));
            let error = match result {
                Ok(Ok(())) => None,
                Ok(Err(message)) => Some(message),
                Err(_) => Some("the audio decoder stopped unexpectedly".to_string()),
            };
            if let Ok(mut queue) = worker_feed.shared.queue.lock() {
                queue.done = true;
                queue.error = error;
            }
        });
    match worker {
        Ok(handle) => (feed, Some(handle)),
        Err(error) => {
            if let Ok(mut queue) = feed.shared.queue.lock() {
                queue.done = true;
                queue.error = Some(error.to_string());
            }
            (feed, None)
        }
    }
}

/// The audio worker: keep [`AUDIO_AHEAD`] seconds decoded past what was taken, until the end or a stop.
fn decode_audio(
    audio: &PlayerAudio,
    from: f64,
    shared: &AudioShared,
    stop: &AtomicBool,
) -> Result<(), String> {
    let reader = AudioReader::open(&audio.path).map_err(|error| error.to_string())?;
    let rate = f64::from(audio.rate);
    // `as` saturates; `from` is a position in a file of at most hours.
    let mut position = (from * rate) as u64;
    let ahead = (AUDIO_AHEAD * rate) as usize;
    let chunk = ((AUDIO_CHUNK * rate) as usize).max(1);
    while !stop.load(Ordering::Relaxed) {
        let queued = {
            let mut queue = shared.queue.lock().map_err(|error| error.to_string())?;
            position = position.saturating_add(std::mem::take(&mut queue.skip));
            queue.samples.len() / 2
        };
        if queued >= ahead {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        let samples = reader
            .read_stereo(position, chunk, audio.rate)
            .map_err(|error| error.to_string())?;
        if samples.len() < 2 {
            return Ok(());
        }
        let frames = u64::try_from(samples.len() / 2).unwrap_or(0);
        position = position.saturating_add(frames);
        let mut queue = shared.queue.lock().map_err(|error| error.to_string())?;
        queue.samples.extend(samples);
    }
    Ok(())
}

/// The size of the black frames of a playback without a picture: small, in the shape of `size`.
fn black_size(size: Option<(u32, u32)>) -> (u32, u32) {
    const WIDTH: u32 = 64;
    let (width, height) = size.filter(|(w, h)| *w > 0 && *h > 0).unwrap_or((720, 480));
    let rows = u64::from(WIDTH) * u64::from(height) / u64::from(width);
    (
        WIDTH,
        u32::try_from(rows).unwrap_or(WIDTH).clamp(1, 4 * WIDTH),
    )
}

/// The worker of a video whose picture cannot be decoded (`message` says why) but whose sound plays: the clock
/// runs to the end of the file, and a black frame tells the time of each frame.
fn play_sound_only(
    path: &std::path::Path,
    (from, fps, size): (f64, f64, Option<(u32, u32)>),
    message: &str,
    shared: &Mutex<Shared>,
    stop: &AtomicBool,
    clock: &OnceLock<Instant>,
) -> Result<PlayerStatus, String> {
    // How long it plays: the file's length, as its header or its sound tells it.
    let end = disccraft_transcode::probe_video(path)
        .map(|probe| probe.seconds)
        .or_else(|_| disccraft_transcode::probe_audio_seconds(path))
        .map_err(|_| message.to_string())?;
    if !end.is_finite() || from >= end {
        return Ok(PlayerStatus::Ended);
    }
    let (width, height) = black_size(size);
    let black = || vec![0u8; width as usize * height as usize * 3];
    if let Ok(mut shared) = shared.lock() {
        shared.no_picture = Some(message.to_string());
    }
    let started = *clock.get_or_init(Instant::now);
    let mut shown: Option<i64> = None;
    while !stop.load(Ordering::Relaxed) {
        let now = from + started.elapsed().as_secs_f64();
        if now >= end {
            return Ok(PlayerStatus::Ended);
        }
        // `as` saturates; the index only orders the frames.
        let index = (now * fps).floor() as i64;
        if shown == Some(index) {
            let next = (index.saturating_add(1)) as f64 / fps;
            thread::sleep(Duration::from_secs_f64(
                (next - now).clamp(0.0, MAX_SLEEP.as_secs_f64()),
            ));
            continue;
        }
        if let Ok(mut shared) = shared.lock() {
            shared.frame = Some(PlayerFrame {
                seconds: index as f64 / fps,
                width,
                height,
                rgb: black(),
            });
        }
        shown = Some(index);
    }
    Ok(PlayerStatus::Playing)
}

/// The worker: decode the frame for the clock until the end or a stop. The clock starts with the first frame.
fn play(
    path: &std::path::Path,
    (from, fps, size, sound): (f64, f64, Option<(u32, u32)>, bool),
    shared: &Mutex<Shared>,
    stop: &AtomicBool,
    clock: &OnceLock<Instant>,
) -> Result<PlayerStatus, String> {
    let reader = match FrameReader::open(path) {
        Ok(reader) => reader,
        Err(error) if sound => {
            return play_sound_only(
                path,
                (from, fps, size),
                &error.to_string(),
                shared,
                stop,
                clock,
            );
        }
        Err(error) => return Err(error.to_string()),
    };
    let frame_at = |seconds: f64| {
        match size {
            Some((width, height)) => reader.frame_sized(seconds, width, height),
            None => reader.frame(seconds),
        }
        .map_err(|error| error.to_string())
    };
    let end = reader.duration_seconds();
    if from >= end {
        return Ok(PlayerStatus::Ended);
    }
    // The first frame is decoded before the clock starts, so the sound waits for the picture.
    let first = (from * fps).floor();
    let picture = match frame_at(first / fps) {
        Ok(picture) => picture,
        Err(message) if sound => {
            return play_sound_only(path, (from, fps, size), &message, shared, stop, clock);
        }
        Err(message) => return Err(message),
    };
    let started = *clock.get_or_init(Instant::now);
    let mut shown: Option<i64> = None;
    if let Ok(mut shared) = shared.lock() {
        let (width, height, rgb) = picture;
        shared.frame = Some(PlayerFrame {
            seconds: first / fps,
            width,
            height,
            rgb,
        });
        // `as` saturates; the index only orders the frames.
        shown = Some(first as i64);
    }

    while !stop.load(Ordering::Relaxed) {
        let now = from + started.elapsed().as_secs_f64();
        if now >= end {
            return Ok(PlayerStatus::Ended);
        }
        // `as` saturates; the index only orders the frames.
        let index = (now * fps).floor() as i64;
        if shown == Some(index) {
            let next = (index.saturating_add(1)) as f64 / fps;
            let wait = Duration::from_secs_f64((next - now).clamp(0.0, MAX_SLEEP.as_secs_f64()));
            thread::sleep(wait);
            continue;
        }
        let seconds = index as f64 / fps;
        let (width, height, rgb) = frame_at(seconds)?;
        if let Ok(mut shared) = shared.lock() {
            shared.frame = Some(PlayerFrame {
                seconds,
                width,
                height,
                rgb,
            });
        }
        shown = Some(index);
    }
    Ok(PlayerStatus::Playing)
}

#[cfg(test)]
mod tests {
    use super::{AudioFeed, AudioQueue, AudioShared, Player, PlayerAudio, PlayerStatus};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    /// A feed at `rate` whose clock started `ago` and whose queue holds `frames` stereo frames of (0.5, -0.5).
    fn queued_feed(rate: u32, ago: Duration, frames: usize) -> AudioFeed {
        let clock = OnceLock::new();
        let _ = clock.set(Instant::now().checked_sub(ago).unwrap_or_else(Instant::now));
        let mut queue = AudioQueue::default();
        for _ in 0..frames {
            queue.samples.extend([0.5, -0.5]);
        }
        AudioFeed {
            shared: Arc::new(AudioShared {
                rate,
                clock: Arc::new(clock),
                queue: Mutex::new(queue),
            }),
        }
    }

    fn taken(feed: &AudioFeed) -> (u64, u64, usize) {
        feed.shared.queue.lock().map_or((0, 0, 0), |queue| {
            (queue.taken, queue.skip, queue.samples.len() / 2)
        })
    }

    /// M12.18: a video whose picture the decoder does not read (interlaced H.264) plays its sound, with black
    /// frames that tell the time; without sound to play, it fails as before.
    #[test]
    fn a_video_without_a_readable_picture_plays_its_sound() {
        let video =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures/h264_1080i.mkv");
        if !video.is_file() {
            eprintln!("h264_1080i.mkv fixture not found; skipping");
            return;
        }
        let wait = |what: &str, done: &dyn Fn() -> bool| {
            let start = Instant::now();
            while !done() {
                assert!(start.elapsed() < Duration::from_secs(30), "{what}");
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        let silent = Player::start_sized(video.clone(), None, 0.5, 30.0, Some((1920, 1080)));
        wait("never failed", &|| silent.status() != PlayerStatus::Playing);
        assert!(matches!(silent.status(), PlayerStatus::Failed(_)));
        assert!(silent.no_picture().is_none());

        let audio = PlayerAudio {
            path: video.clone(),
            rate: 48_000,
            from: None,
        };
        let player = Player::start_sized(video, Some(audio), 0.5, 30.0, Some((1920, 1080)));
        let frame = std::cell::RefCell::new(None);
        wait("no frame", &|| {
            *frame.borrow_mut() = player.take_frame();
            frame.borrow().is_some() || player.status() != PlayerStatus::Playing
        });
        let frame = frame.into_inner().expect("a frame");
        assert_eq!((frame.width, frame.height), (64, 36));
        assert!(frame.seconds >= 0.5 && frame.rgb.iter().all(|byte| *byte == 0));
        assert!(player.no_picture().is_some_and(|why| why.contains("H.264")));
        assert_eq!(player.status(), PlayerStatus::Playing);
        // The sound comes: the fixture's tone.
        let feed = player.audio().expect("a feed");
        let heard = std::cell::Cell::new(false);
        wait("no sound", &|| {
            let mut out = vec![0.0_f32; 960];
            feed.fill(&mut out, 2);
            heard.set(out.iter().any(|sample| sample.abs() > 0.01));
            heard.get() || feed.error().is_some()
        });
        assert!(heard.get(), "{:?}", feed.error());
        // It ends at the end of the file (3 seconds).
        wait("never ended", &|| player.status() != PlayerStatus::Playing);
        assert_eq!(player.status(), PlayerStatus::Ended);
    }

    /// M12.19: still frames are decoded off the caller's thread; a request replaces the one still waiting.
    #[test]
    fn the_loader_decodes_the_newest_request_on_its_own_thread() {
        use super::{FrameLoader, FrameRequest};
        let video =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures/ntsc.m2v");
        if !video.is_file() {
            eprintln!("ntsc.m2v fixture not found; skipping");
            return;
        }
        let loader = FrameLoader::new();
        assert!(!loader.busy() && loader.take().is_none());
        let request = |path: &PathBuf, frame: i64| FrameRequest {
            path: path.clone(),
            seconds: frame as f64 / 30.0,
            size: (640, 480),
            key: (7, frame),
        };
        // Asked in a burst: the last is answered, and nothing after it.
        for frame in 0..5 {
            loader.request(request(&video, frame));
        }
        let mut answered = Vec::new();
        let start = Instant::now();
        while loader.busy() || answered.last().is_none_or(|(key, _)| *key != (7, 4)) {
            assert!(start.elapsed() < Duration::from_secs(60), "{answered:?}");
            if let Some((asked, frame)) = loader.take() {
                let frame = frame.expect("a frame");
                assert_eq!((frame.width, frame.height), (640, 480));
                answered.push((asked.key, frame.rgb.len()));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(answered.len() <= 5, "{answered:?}");
        assert!(answered.iter().all(|(_, len)| *len == 640 * 480 * 3));
        // A file that is not a video is an error, not a hang; the loader goes on after it.
        loader.request(request(&PathBuf::from("no such file.mp4"), 0));
        let start = Instant::now();
        let failed = loop {
            assert!(start.elapsed() < Duration::from_secs(60));
            if let Some((_, result)) = loader.take() {
                break result;
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        assert!(failed.is_err());
        loader.request(request(&video, 1));
        let start = Instant::now();
        while loader.take().is_none_or(|(_, frame)| frame.is_err()) {
            assert!(start.elapsed() < Duration::from_secs(60));
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn the_feed_plays_left_and_right_on_the_first_channels() {
        let feed = queued_feed(1000, Duration::ZERO, 4);
        // Four channels: left, right, then silence.
        let mut out = vec![9.0_f32; 12];
        feed.fill(&mut out, 4);
        assert_eq!(
            out,
            vec![
                0.5, -0.5, 0.0, 0.0, 0.5, -0.5, 0.0, 0.0, 0.5, -0.5, 0.0, 0.0
            ]
        );
        // One channel: their mix. Past the queue: silence, not counted as played.
        let mut out = vec![9.0_f32; 3];
        feed.fill(&mut out, 1);
        assert_eq!(out, vec![0.0, 0.0, 0.0]);
        assert_eq!(taken(&feed), (4, 0, 0));
    }

    #[test]
    fn late_sound_is_skipped_to_the_clock() {
        // The clock is at 1 s (1000 frames) and nothing was taken: 1000 frames are late. 300 are queued: those
        // are dropped and the decoder is told to skip the other 700.
        let feed = queued_feed(1000, Duration::from_secs(1), 300);
        let mut out = vec![0.0_f32; 4];
        feed.fill(&mut out, 2);
        let (taken_frames, skip, queued) = taken(&feed);
        assert!((1000..1100).contains(&taken_frames), "{taken_frames}");
        assert!((700..800).contains(&skip), "{skip}");
        assert_eq!(queued, 0);
        assert_eq!(out, vec![0.0; 4]);
        // A little late (under the limit) is played, not skipped.
        let feed = queued_feed(1000, Duration::from_millis(20), 300);
        let mut out = vec![0.0_f32; 2];
        feed.fill(&mut out, 2);
        assert_eq!(out, vec![0.5, -0.5]);
        assert_eq!(taken(&feed), (1, 0, 299));
    }

    #[test]
    fn the_feed_is_silent_until_the_picture_starts() {
        let feed = queued_feed(1000, Duration::ZERO, 4);
        let unstarted = AudioFeed {
            shared: Arc::new(AudioShared {
                rate: 1000,
                clock: Arc::new(OnceLock::new()),
                queue: Mutex::new(AudioQueue::default()),
            }),
        };
        let mut out = vec![1.0_f32; 4];
        unstarted.fill(&mut out, 2);
        assert_eq!(out, vec![0.0; 4]);
        // No channels: nothing to fill, nothing taken.
        feed.fill(&mut [], 0);
        assert_eq!(taken(&feed), (0, 0, 4));
    }

    #[test]
    fn sound_comes_with_the_picture() {
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("fixtures");
        let (video, audio) = (fixtures.join("ntsc.m2v"), fixtures.join("ntsc.wav"));
        if !video.is_file() || !audio.is_file() {
            println!("skipped: no fixture");
            return;
        }
        let mut player = Player::start_with_audio(
            video,
            Some(PlayerAudio {
                path: audio,
                rate: 44_100,
                from: None,
            }),
            0.5,
            29.97,
        );
        let Some(feed) = player.audio() else {
            panic!("no feed");
        };
        assert_eq!(feed.rate(), 44_100);
        // Take the sound as an output would, 10 ms at a time, until some of it is not silent.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut heard = false;
        let mut out = vec![0.0_f32; 441 * 2];
        while !heard && Instant::now() < deadline {
            feed.fill(&mut out, 2);
            heard = out.iter().any(|sample| sample.abs() > 1e-4);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(heard, "no sound: {:?}", feed.error());
        assert_eq!(feed.error(), None);
        player.stop();
    }

    #[test]
    fn a_file_without_sound_plays_silently() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("fixtures")
            .join("ntsc.m2v");
        if !fixture.is_file() {
            println!("skipped: no fixture");
            return;
        }
        let mut player = Player::start_with_audio(
            fixture.clone(),
            Some(PlayerAudio {
                path: fixture,
                rate: 48_000,
                from: None,
            }),
            0.0,
            29.97,
        );
        let Some(feed) = player.audio() else {
            panic!("no feed");
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while feed.error().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(feed.error().is_some());
        let mut out = vec![1.0_f32; 8];
        feed.fill(&mut out, 2);
        assert_eq!(out, vec![0.0; 8]);
        // The picture plays on.
        assert_eq!(player.status(), PlayerStatus::Playing);
        // An output rate out of range: no sound at all.
        let silent = Player::start_with_audio(
            PathBuf::from("no-such-video.m2v"),
            Some(PlayerAudio {
                path: PathBuf::from("no-such-audio.wav"),
                rate: 0,
                from: None,
            }),
            0.0,
            29.97,
        );
        assert!(silent.audio().is_none());
        player.stop();
    }

    #[test]
    fn a_missing_file_fails_without_panicking() {
        let mut player = Player::start(PathBuf::from("no-such-video.m2v"), 0.0, 29.97);
        let deadline = Instant::now() + Duration::from_secs(5);
        while player.status() == PlayerStatus::Playing && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(player.status(), PlayerStatus::Failed(_)));
        assert_eq!(player.take_frame(), None);
        player.stop();
    }

    #[test]
    fn frames_come_and_stop_ends_the_worker() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("fixtures")
            .join("ntsc.m2v");
        if !fixture.is_file() {
            println!("skipped: no fixture");
            return;
        }
        let mut player = Player::start(fixture, 1.0, 29.97);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut frame = None;
        while frame.is_none() && Instant::now() < deadline {
            frame = player.take_frame();
            std::thread::sleep(Duration::from_millis(10));
        }
        let frame = frame.unwrap_or_else(|| super::PlayerFrame {
            seconds: -1.0,
            width: 0,
            height: 0,
            rgb: Vec::new(),
        });
        // The first frame is the one that covers the start (it may begin a little before it).
        assert!(
            frame.seconds >= 1.0 - 1.0 / 29.97,
            "first frame at {}",
            frame.seconds
        );
        assert_eq!((frame.width, frame.height), (720, 480));
        assert_eq!(frame.rgb.len(), 720 * 480 * 3);
        player.stop();
        assert_eq!(player.status(), PlayerStatus::Playing);
    }
}
