//! M6.366 check: the sound of an AVCHD-like `.m2ts` (interlaced H.264, which the media reader refuses as a whole)
//! is heard: its AC-3, and the same sound as Blu-ray LPCM (made with ffmpeg, the oracle), are taken out and read.
//! The fixture's tone is 440 Hz in its first second and 880 Hz in its second.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_transcode::AudioReader;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Crossings of zero in half a second of the left channel from `from` seconds: the tone's frequency.
fn crossings(reader: &AudioReader, from: f64) -> usize {
    let rate = 48_000u32;
    let start = (from * f64::from(rate)) as u64;
    let samples = reader.read_stereo(start, rate as usize / 2, rate).unwrap();
    let left: Vec<f32> = samples.iter().step_by(2).copied().collect();
    left.windows(2)
        .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
        .count()
}

fn heard(path: &Path) {
    let reader = AudioReader::open(path).unwrap();
    assert!(
        (reader.duration_seconds() - 3.0).abs() < 0.2,
        "{}",
        reader.duration_seconds()
    );
    let first = crossings(&reader, 0.25);
    let second = crossings(&reader, 1.25);
    assert!((430..=450).contains(&first), "{first}");
    assert!((870..=890).contains(&second), "{second}");
}

#[test]
fn the_sound_of_an_interlaced_m2ts_is_heard() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/fixtures/h264_1080i_tones.m2ts");
    if !fixture.is_file() {
        eprintln!("h264_1080i_tones.m2ts fixture not found; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("disccraft-ts-audio-it-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ac3 = dir.join("camera.m2ts");
    std::fs::copy(&fixture, &ac3).unwrap();
    heard(&ac3);

    // The same with Blu-ray LPCM, when ffmpeg is there to make it.
    let ffmpeg =
        std::env::var_os("DISCCRAFT_FFMPEG").map_or_else(|| PathBuf::from("ffmpeg"), PathBuf::from);
    let lpcm = dir.join("camera_lpcm.m2ts");
    let made = Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-i"])
        .arg(&ac3)
        .args([
            "-c:v",
            "copy",
            "-c:a",
            "pcm_bluray",
            "-f",
            "mpegts",
            "-mpegts_m2ts_mode",
            "1",
        ])
        .arg(&lpcm)
        .status()
        .is_ok_and(|status| status.success());
    if made {
        heard(&lpcm);
    } else {
        eprintln!("ffmpeg not found; the LPCM half is skipped");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
