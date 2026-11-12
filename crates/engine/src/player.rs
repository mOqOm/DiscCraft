//! Plays a video asset in the monitor: a worker thread decodes the frame for the current time, in real time, and
//! keeps only the latest one; the UI takes it on every repaint. When decoding falls behind, frames are skipped,
//! not delayed. There is no sound yet.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use disccraft_transcode::FrameReader;

/// The longest the worker sleeps before looking at the clock again.
const MAX_SLEEP: Duration = Duration::from_millis(20);

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
}

/// Real-time playback of one video file.
#[derive(Debug)]
pub struct Player {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Player {
    /// Starts playing `path` from `from_seconds` at `fps` frames per second.
    pub fn start(path: PathBuf, from_seconds: f64, fps: f64) -> Self {
        let shared = Arc::new(Mutex::new(Shared {
            frame: None,
            status: PlayerStatus::Playing,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_shared = Arc::clone(&shared);
        let worker_stop = Arc::clone(&stop);
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

        let worker = thread::Builder::new()
            .name("disccraft-player".to_string())
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    play(&path, from, fps, &worker_shared, &worker_stop)
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

        Self {
            shared,
            stop,
            worker,
        }
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

    /// Stops playback and waits for the worker.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
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

/// The worker: decode the frame for the clock until the end or a stop.
fn play(
    path: &std::path::Path,
    from: f64,
    fps: f64,
    shared: &Mutex<Shared>,
    stop: &AtomicBool,
) -> Result<PlayerStatus, String> {
    let reader = FrameReader::open(path).map_err(|error| error.to_string())?;
    let end = reader.duration_seconds();
    let started = Instant::now();
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
            let wait = Duration::from_secs_f64((next - now).clamp(0.0, MAX_SLEEP.as_secs_f64()));
            thread::sleep(wait);
            continue;
        }
        let seconds = index as f64 / fps;
        let (width, height, rgb) = reader.frame(seconds).map_err(|error| error.to_string())?;
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
    use super::{Player, PlayerStatus};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

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
