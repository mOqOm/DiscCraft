//! M6.349: an HD encode that stops part way (the Progress panel's Stop, or an encode error) returns at once
//! instead of hanging: the decoding thread, a few pictures ahead, is let go. Before the fix it waited forever on
//! its full channel. ffmpeg makes the source (skipped without it).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::env;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use disccraft_transcode::hd::{HdCodec, HdOptions, transcode_h264};
use disccraft_transcode::{RateMode, TranscodeError};

fn ffmpeg() -> Option<PathBuf> {
    let path =
        env::var_os("DISCCRAFT_FFMPEG").map_or_else(|| PathBuf::from("ffmpeg"), PathBuf::from);
    Command::new(&path)
        .arg("-version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|_| path)
}

#[test]
fn a_stopped_hd_encode_returns_instead_of_hanging() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = env::temp_dir().join(format!("disccraft-hd-stop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.mp4");
    let made = Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=s=1280x720:r=30000/1001")
        .args(["-t", "4", "-c:v", "libx264", "-preset", "ultrafast"])
        .args(["-pix_fmt", "yuv420p"])
        .arg(&source)
        .status()
        .unwrap();
    assert!(made.success());

    let (done, finished) = mpsc::channel();
    let es = dir.join("stopped.es");
    let index = dir.join("stopped.idx");
    let input = source.clone();
    // On a thread of its own, so a hang fails the test instead of holding it up.
    std::thread::spawn(move || {
        let options = HdOptions {
            width: 1280,
            height: 720,
            fps_num: 60_000,
            fps_den: 1001,
            target_kbps: 6_000,
            max_kbps: 20_000,
            codec: HdCodec::H264,
            quality: disccraft_transcode::BEST_QUALITY,
            rendering: disccraft_transcode::Rendering::default(),
            mode: RateMode::Vbr,
        };
        // Stop once the first picture is encoded: the decoder is then ahead, its channel full.
        let mut asked = false;
        let result = transcode_h264(&input, &es, &index, &options, &mut |_| {
            if !asked {
                asked = true;
                std::thread::sleep(Duration::from_millis(500));
                disccraft_transcode::request_stop();
            }
        });
        let _ = done.send(result);
    });
    let result = finished.recv_timeout(Duration::from_secs(120));
    disccraft_transcode::clear_stop();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        matches!(result, Ok(Err(TranscodeError::Stopped))),
        "{result:?}"
    );
}
