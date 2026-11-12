#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use disccraft_psmux::es::parse_video;
use disccraft_transcode::{Standard, TranscodeOptions, transcode};

static FIXTURE_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn transcodes_h264_aac_fixture() -> Result<(), Box<dyn Error>> {
    let ffmpeg = ffmpeg_path();
    if !tool_available(&ffmpeg) {
        println!("skipped: ffmpeg not found");
        return Ok(());
    }

    let _guard = FIXTURE_LOCK.lock().map_err(|_| "fixture lock poisoned")?;

    let fixtures = fixtures_dir();
    fs::create_dir_all(&fixtures)?;

    let input = fixtures.join("h264_aac.mp4");
    if !input.is_file() {
        create_fixture(&ffmpeg, &input)?;
    }

    let dir = temp_dir("h264_aac");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;

    let result = run_transcode(&input, &dir);
    let _ = fs::remove_dir_all(&dir);
    result
}

fn run_transcode(input: &Path, dir: &Path) -> Result<(), Box<dyn Error>> {
    let video_out = dir.join("out.m2v");
    let audio_out = dir.join("out.wav");

    let options = TranscodeOptions {
        standard: Standard::Ntsc,
        video_bitrate: 6_000_000,
        max_bitrate: 9_800_000,
        wide: false,
    };

    let mut progress = |_fraction| {};
    let output = transcode(input, &video_out, &audio_out, &options, &mut progress)?;

    if output.frames < 178 || output.frames > 182 {
        return Err(format!("unexpected frame count {}", output.frames).into());
    }

    let video_data = fs::read(&output.video)?;
    let info = parse_video(&video_data, 0).map_err(|error| format!("parse video: {error}"))?;

    if info.sequence.width != 720
        || info.sequence.height != 480
        || info.sequence.frame_rate_code != 4
    {
        return Err(format!(
            "unexpected video info {}x{} rate {}",
            info.sequence.width, info.sequence.height, info.sequence.frame_rate_code
        )
        .into());
    }

    let audio_path = output
        .audio
        .ok_or_else(|| "missing audio output".to_string())?;
    let wav = fs::read(&audio_path)?;
    check_wav(&wav)?;

    Ok(())
}

fn check_wav(wav: &[u8]) -> Result<(), Box<dyn Error>> {
    if wav.len() < 44 {
        return Err("wav too short".into());
    }
    if &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return Err("bad wav header".into());
    }

    let channels = u16::from_le_bytes([wav[22], wav[23]]);
    let sample_rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]);
    let data_size = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]) as u64;

    if channels != 2 {
        return Err(format!("expected stereo, got {channels}").into());
    }
    if sample_rate != 48_000 {
        return Err(format!("expected 48 kHz, got {sample_rate}").into());
    }

    let frames = data_size / 4;
    let expected = 6 * 48_000;
    if frames.abs_diff(expected) > 48_000 {
        return Err(format!("audio length {frames} not near 6 s").into());
    }

    Ok(())
}

fn create_fixture(ffmpeg: &Path, path: &Path) -> Result<(), Box<dyn Error>> {
    let output = Command::new(ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=1280x720:rate=24",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=660:sample_rate=44100",
            "-t",
            "6",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(path)
        .output()?;

    if !output.status.success() {
        return Err(format!("ffmpeg failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }

    Ok(())
}

fn tool_available(cmd: &Path) -> bool {
    Command::new(cmd)
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn ffmpeg_path() -> PathBuf {
    env::var_os("DISCCRAFT_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|parent| parent.parent())
        .map(PathBuf::from)
        .unwrap_or(manifest)
}

fn fixtures_dir() -> PathBuf {
    workspace_root().join("target").join("fixtures")
}

fn temp_dir(name: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "disccraft-transcode-{}-{}",
        std::process::id(),
        name
    ))
}
