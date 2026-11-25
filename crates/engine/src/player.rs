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

use disccraft_transcode::{AudioReader, FRAME_SNAP_SECONDS, FrameReader};

use crate::time_stretch::TimeStretch;

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
/// The playback speeds a player takes (M6.427): slower and faster ones are brought into this range.
const SPEEDS: std::ops::RangeInclusive<f64> = 0.25..=16.0;
/// The fastest playback whose sound is heard (M6.427); faster playback is silent.
pub const MAX_HEARD_SPEED: f64 = 2.0;

/// `speed` as a player uses it: within [`SPEEDS`], 1 when it is not a number.
fn usable_speed(speed: f64) -> f64 {
    if speed.is_finite() && speed > 0.0 {
        speed.clamp(*SPEEDS.start(), *SPEEDS.end())
    } else {
        1.0
    }
}

/// The sound of a playback: the file to take it from, and the output's rate.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerAudio {
    /// An audio file, or a video file with sound.
    pub path: PathBuf,
    /// The output device's sample rate (Hz).
    pub rate: u32,
    /// Where in the audio file to start (seconds), when that is not the picture's start position: a timeline's
    /// audio track runs along the whole timeline while the picture is one of its clips. Negative when the sound
    /// starts later than the playback does (an audio clip moved along its track): that long is silent first.
    pub from: Option<f64>,
    /// Where in the audio file the sound ends (seconds), when that is before the file's end: an audio clip whose
    /// out point is trimmed.
    pub until: Option<f64>,
    /// The sound as parts played one after the other (an audio track of several clips, from where the playback
    /// starts). When there are any, they are what plays: `path`, `from` and `until` are not looked at.
    pub parts: Vec<PlayerAudioPart>,
    /// Which sound stream of `path` plays (M6.418): 0 the first.
    pub stream: usize,
}

/// One part of a playback's sound: a stretch of a file, with silence in front.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerAudioPart {
    /// An audio file, or a video file with sound.
    pub path: PathBuf,
    /// Seconds of silence before the part (after the part before it has ended).
    pub silence: f64,
    /// Where in the file the part starts, seconds.
    pub from: f64,
    /// Where in the file the part ends, seconds (`None`: the file's end).
    pub until: Option<f64>,
    /// Which sound stream of the file plays (M6.418): 0 the first.
    pub stream: usize,
}

/// How many sound streams the file of asset `asset` has (M6.418): what an audio track of it can play.
#[must_use]
pub fn audio_stream_count(
    project: &disccraft_project::Project,
    asset: disccraft_project::AssetId,
) -> usize {
    project.assets.get(&asset).map_or(0, |asset| {
        disccraft_transcode::audio_stream_count(&asset.media_file())
    })
}

/// The sound of an audio track of several clips from `from` seconds of the timeline on, as the parts a player
/// plays one after the other (M6.269): each clip's stretch of its file, with the silence up to it in front.
///
/// Empty for a track of one clip (which plays as before), and when a file is missing.
#[must_use]
pub fn audio_track_parts(
    project: &disccraft_project::Project,
    track: &disccraft_project::AudioTrack,
    frame_seconds: f64,
    from: f64,
) -> Vec<PlayerAudioPart> {
    if track.more.is_empty() {
        return Vec::new();
    }
    let mut parts = Vec::new();
    // The time on the timeline up to which the parts so far have sounded.
    let mut cursor = from.max(0.0);
    for (_, clip) in track.clips() {
        let Some(path) = project
            .assets
            .get(&clip.asset)
            .map(|asset| asset.media_file().into_owned())
        else {
            return Vec::new();
        };
        let start = f64::from(clip.start) * frame_seconds;
        let trim = f64::from(clip.trim_start) * frame_seconds;
        let until = clip.trim_end.map(|end| f64::from(end) * frame_seconds);
        // Where the clip ends on the timeline: its out point, or its file's end.
        let file_end = until.or_else(|| disccraft_transcode::probe_audio_seconds(&path).ok());
        let end = file_end.map(|end| start + (end - trim).max(0.0));
        if end.is_some_and(|end| end <= cursor) {
            continue;
        }
        let (silence, file_from) = if start >= cursor {
            (start - cursor, trim)
        } else {
            (0.0, trim + (cursor - start))
        };
        // The track's stream of each clip's file (M6.429); a file with fewer streams plays its first.
        let stream = if track.stream == 0
            || clip.asset == track.asset
            || disccraft_transcode::audio_stream_count(&path) > track.stream as usize
        {
            track.stream as usize
        } else {
            0
        };
        parts.push(PlayerAudioPart {
            path,
            silence,
            from: file_from,
            until,
            stream,
        });
        match end {
            Some(end) => cursor = end,
            // A clip whose length is not known: what follows it cannot be placed.
            None => break,
        }
    }
    parts
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
    /// The playback speed (M6.427): the queue holds the sound already played at it (pitch kept).
    speed: f64,
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

    /// [`FrameSource::open`] with a decoder of its own instead of the one the monitor shares with every other
    /// reader of the file (M6.486): for the Timeline viewer's pictures, taken here and there in a file, which moved
    /// the shared decoder away from where the monitor was playing (a stutter each time).
    ///
    /// # Errors
    ///
    /// When the file cannot be read as a video.
    pub fn open_own(path: &std::path::Path) -> Result<Self, String> {
        Ok(Self {
            reader: FrameReader::open_own(path).map_err(|error| error.to_string())?,
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

    /// The first frame from `seconds` on that shows something, looked for every half second before `until`
    /// (Encore's "find bright poster frames for the timeline", M6.483: a clip that fades in from black shows its
    /// first lit picture, by the rule of a new timeline's poster frame); the frame at `seconds` when none does.
    /// `go_on` is asked before each frame is decoded: when it says no, the search stops with `None` (the timeline's
    /// pictures give way to playback, which needs the computer more).
    ///
    /// # Errors
    ///
    /// When the frame at `seconds` cannot be decoded.
    pub fn lit_frame(
        &self,
        seconds: f64,
        size: (u32, u32),
        until: f64,
        go_on: &dyn Fn() -> bool,
    ) -> Result<Option<PlayerFrame>, String> {
        /// How many half seconds are looked at, at most (ten seconds).
        const STEPS: u32 = 20;
        if !go_on() {
            return Ok(None);
        }
        let first = self.frame(seconds, size)?;
        if disccraft_transcode::is_poster_lit(&first.rgb) {
            return Ok(Some(first));
        }
        for step in 1..=STEPS {
            let at = seconds + f64::from(step) * disccraft_transcode::POSTER_STEP_SECONDS;
            // Also stops for an `until` that is not a number.
            if at.partial_cmp(&until) != Some(std::cmp::Ordering::Less) {
                break;
            }
            if !go_on() {
                return Ok(None);
            }
            match self.frame(at, size) {
                Ok(frame) if disccraft_transcode::is_poster_lit(&frame.rgb) => {
                    return Ok(Some(frame));
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        Ok(Some(first))
    }
}

/// The name a Timeline viewer picture is remembered under (letters and digits, as the cache asks): its size and
/// where it was looked for, in milliseconds.
fn timeline_picture_kind(seconds: f64, until: f64, (width, height): (u32, u32)) -> String {
    // `as` saturates; the times are a file's seconds.
    let ms = |value: f64| {
        if value.is_finite() {
            (value.max(0.0) * 1000.0).round() as u64
        } else {
            0
        }
    };
    format!("tlpic{width}x{height}a{}b{}", ms(seconds), ms(until))
}

/// A picture of the Timeline viewer ([`FrameSource::lit_frame`] at `seconds` before `until`, at `size`) remembered
/// from an earlier run (M6.486): a project opens with its pictures at once instead of decoding them again.
#[must_use]
pub fn remembered_timeline_picture(
    path: &std::path::Path,
    seconds: f64,
    until: f64,
    size: (u32, u32),
) -> Option<PlayerFrame> {
    let blob =
        disccraft_transcode::cached_blob(&timeline_picture_kind(seconds, until, size), path)?;
    picture_from_blob(&blob, seconds, size)
}

/// Remembers a picture of the Timeline viewer for [`remembered_timeline_picture`] (with the file's other facts,
/// when the application keeps them).
pub fn remember_timeline_picture(
    path: &std::path::Path,
    (seconds, until): (f64, f64),
    size: (u32, u32),
    frame: &PlayerFrame,
) {
    disccraft_transcode::store_blob(
        &timeline_picture_kind(seconds, until, size),
        path,
        &picture_blob(frame),
    );
}

/// A picture as it is kept on disk: its width and height (little-endian) and its RGB24 pixels.
fn picture_blob(frame: &PlayerFrame) -> Vec<u8> {
    let mut blob = Vec::with_capacity(frame.rgb.len().saturating_add(8));
    blob.extend_from_slice(&frame.width.to_le_bytes());
    blob.extend_from_slice(&frame.height.to_le_bytes());
    blob.extend_from_slice(&frame.rgb);
    blob
}

/// The picture kept in `blob` ([`picture_blob`]) when it is whole and no larger than `size`: a file changed or cut
/// short on disk gives nothing (and the picture is decoded again).
fn picture_from_blob(blob: &[u8], seconds: f64, size: (u32, u32)) -> Option<PlayerFrame> {
    let width = u32::from_le_bytes(blob.get(0..4)?.try_into().ok()?);
    let height = u32::from_le_bytes(blob.get(4..8)?.try_into().ok()?);
    let rgb = blob.get(8..)?;
    let expected = usize::try_from(
        u64::from(width)
            .checked_mul(u64::from(height))?
            .checked_mul(3)?,
    )
    .ok()?;
    if width == 0 || height == 0 || width > size.0 || height > size.1 || rgb.len() != expected {
        return None;
    }
    Some(PlayerFrame {
        seconds,
        width,
        height,
        rgb: rgb.to_vec(),
    })
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
        Self::start_at_speed(path, audio, from_seconds, fps, size, 1.0)
    }

    /// [`Player::start_sized`] at `speed` times real time (M6.427; 0.5 half speed, 2 twice as fast, within
    /// 0.25 to 16). The sound keeps its pitch, and is not played above [`MAX_HEARD_SPEED`].
    pub fn start_at_speed(
        path: PathBuf,
        audio: Option<PlayerAudio>,
        from_seconds: f64,
        fps: f64,
        size: Option<(u32, u32)>,
        speed: f64,
    ) -> Self {
        let speed = usable_speed(speed);
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
                        (from, fps, size, sound, speed),
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
            .filter(|audio| AUDIO_RATES.contains(&audio.rate) && speed <= MAX_HEARD_SPEED)
            .map(|audio| {
                let audio_from = audio.from.filter(|at| at.is_finite()).unwrap_or(from);
                start_audio(audio, audio_from, (clock, speed), &stop)
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
    (clock, speed): (Arc<OnceLock<Instant>>, f64),
    stop: &Arc<AtomicBool>,
) -> (AudioFeed, Option<JoinHandle<()>>) {
    let feed = AudioFeed {
        shared: Arc::new(AudioShared {
            rate: audio.rate,
            speed,
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
    // One part: the file from `from` (a negative `from` is silence first), up to the trimmed out point.
    let whole;
    let parts: &[PlayerAudioPart] = if audio.parts.is_empty() {
        whole = [PlayerAudioPart {
            path: audio.path.clone(),
            silence: (-from).max(0.0),
            from: from.max(0.0),
            until: audio.until,
            stream: audio.stream,
        }];
        &whole
    } else {
        &audio.parts
    };
    // At a speed other than 1, the sound goes into the queue already played at that speed (M6.427).
    let mut stretch = TimeStretch::new(shared.speed, audio.rate);
    for part in parts {
        if !decode_audio_part(part, audio.rate, shared, stop, &mut stretch)? {
            return Ok(());
        }
    }
    if let Some(stretch) = stretch.as_mut() {
        let rest = stretch.finish();
        let mut queue = shared.queue.lock().map_err(|error| error.to_string())?;
        queue.samples.extend(rest);
    }
    Ok(())
}

/// Decodes one part into the queue (through `stretch` when the speed is not 1); `false` when the playback was
/// stopped (no further part is wanted).
fn decode_audio_part(
    part: &PlayerAudioPart,
    output_rate: u32,
    shared: &AudioShared,
    stop: &AtomicBool,
    stretch: &mut Option<TimeStretch>,
) -> Result<bool, String> {
    // What goes into the queue: the sound as it is, or played at the speed.
    let played = |stretch: &mut Option<TimeStretch>, samples: Vec<f32>| match stretch {
        Some(stretch) => stretch.push(&samples),
        None => samples,
    };
    let reader =
        AudioReader::open_stream(&part.path, part.stream).map_err(|error| error.to_string())?;
    let rate = f64::from(output_rate);
    let seconds = |value: f64| {
        if value.is_finite() {
            value.max(0.0)
        } else {
            0.0
        }
    };
    // `as` saturates; the positions are in files of at most hours.
    let mut position = (seconds(part.from) * rate) as u64;
    // Frames of silence before the sound starts.
    let mut silence = (seconds(part.silence) * rate) as u64;
    let ahead = (AUDIO_AHEAD * rate) as usize;
    let chunk = ((AUDIO_CHUNK * rate) as usize).max(1);
    // The frame the sound ends in front of, when the clip's out point is trimmed.
    let end = part
        .until
        .filter(|until| until.is_finite())
        .map(|until| (until.max(0.0) * rate) as u64);
    while !stop.load(Ordering::Relaxed) {
        let queued = {
            let mut queue = shared.queue.lock().map_err(|error| error.to_string())?;
            // What the output skipped to catch up comes off the silence first, then off the sound. The output
            // counts in played frames: at a speed, that many times the speed of the file's.
            let mut skip = std::mem::take(&mut queue.skip);
            if skip > 0 && stretch.is_some() {
                // `as` saturates; the skips are of seconds at most.
                skip = (skip as f64 * shared.speed).round() as u64;
                // The sound jumps: the stretch starts again from there.
                *stretch = TimeStretch::new(shared.speed, output_rate);
            }
            let from_silence = skip.min(silence);
            silence -= from_silence;
            position = position.saturating_add(skip - from_silence);
            queue.samples.len() / 2
        };
        if queued >= ahead {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        if silence > 0 {
            let frames = silence.min(chunk as u64);
            silence -= frames;
            let samples = played(stretch, vec![0.0; frames as usize * 2]);
            let mut queue = shared.queue.lock().map_err(|error| error.to_string())?;
            queue.samples.extend(samples);
            continue;
        }
        // Up to the trimmed out point.
        let chunk = match end {
            Some(end) if position >= end => return Ok(true),
            Some(end) => chunk.min(usize::try_from(end - position).unwrap_or(chunk)),
            None => chunk,
        };
        let samples = reader
            .read_stereo(position, chunk, output_rate)
            .map_err(|error| error.to_string())?;
        if samples.len() < 2 {
            return Ok(true);
        }
        let frames = u64::try_from(samples.len() / 2).unwrap_or(0);
        position = position.saturating_add(frames);
        let samples = played(stretch, samples);
        let mut queue = shared.queue.lock().map_err(|error| error.to_string())?;
        queue.samples.extend(samples);
    }
    Ok(false)
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
    (from, fps, size, speed): (f64, f64, Option<(u32, u32)>, f64),
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
        let now = from + started.elapsed().as_secs_f64() * speed;
        if now >= end {
            return Ok(PlayerStatus::Ended);
        }
        // `as` saturates; the index only orders the frames.
        let index = frame_number(now, fps) as i64;
        if shown == Some(index) {
            let next = (index.saturating_add(1)) as f64 / fps;
            thread::sleep(Duration::from_secs_f64(
                ((next - now) / speed).clamp(0.0, MAX_SLEEP.as_secs_f64()),
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

/// The number of the frame showing at `seconds` (a float, to order the frames), taken as the picture is: a hair
/// later ([`FRAME_SNAP_SECONDS`]), so a play from a chapter (its start in seconds is a hair early about 1 time in 10
/// at 29.97) starts on the chapter's frame, not the one before it (M6.449).
fn frame_number(seconds: f64, fps: f64) -> f64 {
    ((seconds + FRAME_SNAP_SECONDS) * fps).floor()
}

/// The worker: decode the frame for the clock until the end or a stop. The clock starts with the first frame.
fn play(
    path: &std::path::Path,
    (from, fps, size, sound, speed): (f64, f64, Option<(u32, u32)>, bool, f64),
    shared: &Mutex<Shared>,
    stop: &AtomicBool,
    clock: &OnceLock<Instant>,
) -> Result<PlayerStatus, String> {
    let reader = match FrameReader::open(path) {
        Ok(reader) => reader,
        Err(error) if sound => {
            return play_sound_only(
                path,
                (from, fps, size, speed),
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
    let first = frame_number(from, fps);
    let picture = match frame_at(first / fps) {
        Ok(picture) => picture,
        Err(message) if sound => {
            return play_sound_only(
                path,
                (from, fps, size, speed),
                &message,
                shared,
                stop,
                clock,
            );
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
        // At a speed, the picture's time runs that many times as fast as the clock (M6.427).
        let now = from + started.elapsed().as_secs_f64() * speed;
        if now >= end {
            return Ok(PlayerStatus::Ended);
        }
        // `as` saturates; the index only orders the frames.
        let index = frame_number(now, fps) as i64;
        if shown == Some(index) {
            let next = (index.saturating_add(1)) as f64 / fps;
            let wait =
                Duration::from_secs_f64(((next - now) / speed).clamp(0.0, MAX_SLEEP.as_secs_f64()));
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
    use super::{
        AudioFeed, AudioQueue, AudioShared, Player, PlayerAudio, PlayerFrame, PlayerStatus,
        frame_number, picture_blob, picture_from_blob, timeline_picture_kind,
    };
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    /// The Timeline viewer's pictures kept on disk (M6.486): named with letters and digits only (what the cache
    /// accepts), read back as they were written, and a file cut short, padded or claiming a larger picture is not
    /// taken (it is decoded again).
    #[test]
    fn timeline_pictures_are_kept_and_checked_when_read_back() {
        for (seconds, until) in [
            (0.0, 1.5),
            (12.345, 99.9),
            (-3.0, f64::NAN),
            (f64::INFINITY, 1e300),
        ] {
            let kind = timeline_picture_kind(seconds, until, (128, 72));
            assert!(kind.chars().all(|c| c.is_ascii_alphanumeric()), "{kind}");
        }
        assert_eq!(
            timeline_picture_kind(1.5, 3.0, (128, 72)),
            "tlpic128x72a1500b3000"
        );
        let frame = PlayerFrame {
            seconds: 2.0,
            width: 4,
            height: 2,
            rgb: (0..24).collect(),
        };
        let blob = picture_blob(&frame);
        assert_eq!(picture_from_blob(&blob, 2.0, (128, 72)), Some(frame));
        // Cut short, one byte too many, bigger than asked for, empty or a zero size.
        assert_eq!(
            picture_from_blob(&blob[..blob.len() - 1], 2.0, (128, 72)),
            None
        );
        let mut longer = blob.clone();
        longer.push(0);
        assert_eq!(picture_from_blob(&longer, 2.0, (128, 72)), None);
        assert_eq!(picture_from_blob(&blob, 2.0, (2, 2)), None);
        assert_eq!(picture_from_blob(&[], 2.0, (128, 72)), None);
        assert_eq!(picture_from_blob(&[0; 8], 2.0, (128, 72)), None);
        // A size whose pixels overflow is refused, not allocated.
        let huge: Vec<u8> = [u32::MAX.to_le_bytes(), u32::MAX.to_le_bytes()].concat();
        assert_eq!(picture_from_blob(&huge, 2.0, (u32::MAX, u32::MAX)), None);
    }

    #[test]
    fn a_play_from_a_frame_start_starts_on_that_frame() {
        // Every frame start of three hours at 29.97 and 23.976, carried as seconds as a chapter's is (a test disc's
        // chapter 7, frame 31 238, started on 31 237).
        const TICKS_PER_SECOND: i64 = 254_016_000_000;
        for (numerator, denominator) in [(30_000_i64, 1001_i64), (24_000, 1001)] {
            let fps = numerator as f64 / denominator as f64;
            let frame_ticks = TICKS_PER_SECOND / numerator * denominator;
            for frame in 0..(3 * 3600 * numerator / denominator) {
                let seconds = (frame * frame_ticks) as f64 / TICKS_PER_SECOND as f64;
                assert_eq!(
                    frame_number(seconds, fps),
                    frame as f64,
                    "frame {frame} at {fps}"
                );
            }
        }
    }

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
                speed: 1.0,
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

    /// An MKV of MPEG-4 Part 2 video (DivX / Xvid, which craft-video does not decode) and AC-3, made with ffmpeg
    /// into the fixtures; `None` without ffmpeg.
    fn mpeg4_part2_fixture() -> Option<PathBuf> {
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
        let video = fixtures.join("mpeg4_part2_ac3.mkv");
        if video.is_file() {
            return Some(video);
        }
        let ffmpeg = std::env::var_os("DISCCRAFT_FFMPEG").unwrap_or_else(|| "ffmpeg".into());
        std::fs::create_dir_all(&fixtures).ok()?;
        let part = fixtures.join(format!("mpeg4_part2_ac3.{}.mkv", std::process::id()));
        let made = std::process::Command::new(ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc2=size=640x480:rate=25")
            .args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
            .args([
                "-t", "2", "-c:v", "mpeg4", "-q:v", "4", "-c:a", "ac3", "-b:a", "192k",
            ])
            .arg(&part)
            .status()
            .ok()?;
        if !made.success() || std::fs::rename(&part, &video).is_err() {
            let _ = std::fs::remove_file(&part);
            return None;
        }
        Some(video)
    }

    /// M12.18: a video whose picture the decoder does not read (MPEG-4 Part 2: interlaced H.264 was the example
    /// until craft-video decoded it, M12.25) plays its sound, with black frames that tell the time; without sound
    /// to play, it fails as before.
    #[test]
    fn a_video_without_a_readable_picture_plays_its_sound() {
        let Some(video) = mpeg4_part2_fixture() else {
            eprintln!("no ffmpeg for the MPEG-4 Part 2 fixture; skipping");
            return;
        };
        let wait = |what: &str, done: &dyn Fn() -> bool| {
            let start = Instant::now();
            while !done() {
                assert!(start.elapsed() < Duration::from_secs(30), "{what}");
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        let silent = Player::start_sized(video.clone(), None, 0.5, 30.0, Some((640, 480)));
        wait("never failed", &|| silent.status() != PlayerStatus::Playing);
        assert!(matches!(silent.status(), PlayerStatus::Failed(_)));
        assert!(silent.no_picture().is_none());

        let audio = PlayerAudio {
            stream: 0,
            parts: Vec::new(),
            path: video.clone(),
            rate: 48_000,
            from: None,
            until: None,
        };
        let player = Player::start_sized(video, Some(audio), 0.5, 30.0, Some((640, 480)));
        let frame = std::cell::RefCell::new(None);
        wait("no frame", &|| {
            *frame.borrow_mut() = player.take_frame();
            frame.borrow().is_some() || player.status() != PlayerStatus::Playing
        });
        let frame = frame.into_inner().expect("a frame");
        assert_eq!((frame.width, frame.height), (64, 48));
        assert!(frame.seconds >= 0.5 && frame.rgb.iter().all(|byte| *byte == 0));
        assert!(player.no_picture().is_some());
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
        // It ends at the end of the file (2 seconds).
        wait("never ended", &|| player.status() != PlayerStatus::Playing);
        assert_eq!(player.status(), PlayerStatus::Ended);
    }

    /// M12.25: interlaced H.264 (a camcorder's 1080i, made by x264 as MBAFF) plays its picture: craft-video decodes
    /// it now (storytold/craft-libs PRs #1992 and #2017, through the mOqOm fork).
    #[test]
    fn interlaced_h264_plays_its_picture() {
        let video =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures/h264_1080i.mkv");
        if !video.is_file() {
            eprintln!("h264_1080i.mkv fixture not found; skipping");
            return;
        }
        let player = Player::start_sized(video, None, 0.5, 30.0, Some((480, 270)));
        let start = Instant::now();
        let frame = loop {
            if let Some(frame) = player.take_frame() {
                break frame;
            }
            assert!(
                player.status() == PlayerStatus::Playing,
                "{:?}",
                player.status()
            );
            assert!(start.elapsed() < Duration::from_secs(30), "no frame");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!((frame.width, frame.height), (480, 270));
        assert!(player.no_picture().is_none());
        // A picture, not black.
        assert!(frame.rgb.iter().filter(|byte| **byte > 40).count() > frame.rgb.len() / 4);
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

    /// A WAV of `seconds` of one steady value (16-bit mono, 48 kHz).
    fn steady_wav(path: &std::path::Path, seconds: f64, value: i16) {
        let samples = (seconds * 48_000.0) as u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + samples * 2).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&48_000_u32.to_le_bytes());
        wav.extend_from_slice(&96_000_u32.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(samples * 2).to_le_bytes());
        for _ in 0..samples {
            wav.extend_from_slice(&value.to_le_bytes());
        }
        std::fs::write(path, wav).unwrap();
    }

    /// M6.269: the sound of a track of several clips plays as parts: each a stretch of its file, with its
    /// silence in front; and the parts of a track are worked out from where the playback starts.
    #[test]
    fn the_parts_of_a_sound_play_one_after_the_other() {
        use super::{PlayerAudioPart, audio_track_parts, decode_audio};
        use disccraft_project::{AudioClip, AudioTrack, Project};
        let dir =
            std::env::temp_dir().join(format!("disccraft-player-parts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (loud, soft) = (dir.join("loud.wav"), dir.join("soft.wav"));
        steady_wav(&loud, 0.5, 16_384);
        steady_wav(&soft, 0.5, 4_096);

        // 0.1 s of the loud file from its 0.1 s on, 0.05 s of silence, then 0.05 s of the soft file.
        let audio = PlayerAudio {
            stream: 0,
            path: loud.clone(),
            rate: 48_000,
            from: None,
            until: None,
            parts: vec![
                PlayerAudioPart {
                    stream: 0,
                    path: loud.clone(),
                    silence: 0.0,
                    from: 0.1,
                    until: Some(0.2),
                },
                PlayerAudioPart {
                    stream: 0,
                    path: soft.clone(),
                    silence: 0.05,
                    from: 0.0,
                    until: Some(0.05),
                },
            ],
        };
        let shared = AudioShared {
            rate: 48_000,
            speed: 1.0,
            clock: Arc::new(OnceLock::new()),
            queue: Mutex::new(AudioQueue::default()),
        };
        decode_audio(
            &audio,
            0.0,
            &shared,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
        let queued: Vec<f32> = shared
            .queue
            .lock()
            .unwrap()
            .samples
            .iter()
            .copied()
            .collect();
        let left: Vec<f32> = queued.iter().step_by(2).copied().collect();
        assert_eq!(left.len(), 4_800 + 2_400 + 2_400);
        let level = |from: usize, to: usize| {
            let part = &left[from..to];
            part.iter().map(|value| value.abs()).sum::<f32>() / part.len() as f32
        };
        assert!(
            (level(100, 4_700) - 0.5).abs() < 0.02,
            "{}",
            level(100, 4_700)
        );
        assert!(level(4_900, 7_100) < 0.001, "{}", level(4_900, 7_100));
        assert!(
            (level(7_300, 9_500) - 0.125).abs() < 0.01,
            "{}",
            level(7_300, 9_500)
        );

        // A track: the loud file's first 0.3 s at the start, and the soft file from its 0.1 s on at 0.5 s of the
        // timeline (frames of a tenth of a second).
        let mut project = Project::default();
        let first = project.add_asset(&loud).unwrap();
        let second = project.add_asset(&soft).unwrap();
        let track = AudioTrack {
            trim_end: Some(3),
            more: vec![AudioClip {
                asset: second,
                start: 5,
                trim_start: 1,
                trim_end: None,
            }],
            ..AudioTrack::of(first)
        };
        let parts = |from: f64| -> Vec<(bool, f64, f64, Option<f64>)> {
            audio_track_parts(&project, &track, 0.1, from)
                .into_iter()
                .map(|part| {
                    let round = |value: f64| (value * 1000.0).round() / 1000.0;
                    (
                        part.path.ends_with("soft.wav"),
                        round(part.silence),
                        round(part.from),
                        part.until.map(round),
                    )
                })
                .collect()
        };
        assert_eq!(
            parts(0.0),
            [(false, 0.0, 0.0, Some(0.3)), (true, 0.2, 0.1, None)]
        );
        // From inside the first clip; from the gap; from inside the second clip; from behind everything.
        assert_eq!(
            parts(0.2),
            [(false, 0.0, 0.2, Some(0.3)), (true, 0.2, 0.1, None)]
        );
        assert_eq!(parts(0.4), [(true, 0.1, 0.1, None)]);
        assert_eq!(parts(0.7), [(true, 0.0, 0.3, None)]);
        assert_eq!(parts(5.0), []);
        // A track of one clip has no parts: it plays as it always did.
        assert!(audio_track_parts(&project, &AudioTrack::of(first), 0.1, 0.0).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// M6.427: at a speed, the sound goes into the queue already played at it: half as long at twice the speed,
    /// twice as long at half, at the same level (the pitch is the time stretch's own test).
    #[test]
    fn the_sound_is_queued_at_the_playback_speed() {
        use super::decode_audio;
        let dir = std::env::temp_dir().join(format!("disccraft-speed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("steady.wav");
        steady_wav(&wav, 0.8, 16_384);
        for (speed, seconds) in [(2.0, 0.4), (0.5, 1.6)] {
            let audio = PlayerAudio {
                stream: 0,
                path: wav.clone(),
                rate: 48_000,
                from: Some(0.0),
                // A fifth of a second of the file at half speed (the queue holds no more than its lead). The
                // file's end is given too: the sound decoder does not stop there by itself.
                until: Some(if speed < 1.0 { 0.2 } else { 0.8 }),
                parts: Vec::new(),
            };
            let shared = AudioShared {
                rate: 48_000,
                speed,
                clock: Arc::new(OnceLock::new()),
                queue: Mutex::new(AudioQueue::default()),
            };
            decode_audio(
                &audio,
                0.0,
                &shared,
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
            let left: Vec<f32> = shared
                .queue
                .lock()
                .unwrap()
                .samples
                .iter()
                .step_by(2)
                .copied()
                .collect();
            let expected = if speed < 1.0 { 0.4 } else { seconds } * 48_000.0;
            assert!(
                (left.len() as f64 - expected).abs() < 0.05 * 48_000.0,
                "{speed}: {} frames for {expected}",
                left.len()
            );
            let middle = &left[left.len() / 4..left.len() * 3 / 4];
            assert!(
                middle.iter().all(|value| (value - 0.5).abs() < 0.01),
                "{speed}: level"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_feed_is_silent_until_the_picture_starts() {
        let feed = queued_feed(1000, Duration::ZERO, 4);
        let unstarted = AudioFeed {
            shared: Arc::new(AudioShared {
                rate: 1000,
                speed: 1.0,
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
                stream: 0,
                parts: Vec::new(),
                path: audio,
                rate: 44_100,
                from: None,
                until: None,
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
                stream: 0,
                parts: Vec::new(),
                path: fixture,
                rate: 48_000,
                from: None,
                until: None,
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
                stream: 0,
                parts: Vec::new(),
                path: PathBuf::from("no-such-audio.wav"),
                rate: 0,
                from: None,
                until: None,
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
