//! M12.20 check: a raw Dolby Digital file is heard (the monitor's sound, the waveform) though the media reader
//! opens no bare AC-3 stream: it is decoded once to a WAV and that is opened in its place.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_test_support::fixtures;
use disccraft_transcode::{AudioReader, probe_audio_seconds};

#[test]
fn a_raw_ac3_file_is_heard() {
    let fixture = fixtures().join("ntsc.ac3");
    if !fixture.is_file() {
        eprintln!("ntsc.ac3 fixture not found; skipping");
        return;
    }
    // A copy of its own, so its decode is this test's.
    let dir = std::env::temp_dir().join(format!("disccraft-rawac3-it-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sound = dir.join("heard.ac3");
    std::fs::copy(&fixture, &sound).unwrap();

    // Its length comes from its first frame and its size.
    let seconds = probe_audio_seconds(&sound).unwrap();
    assert!((seconds - 20.0).abs() < 0.1, "{seconds}");

    let reader = AudioReader::open(&sound).unwrap();
    assert!((reader.duration_seconds() - seconds).abs() < 0.1);
    // A second from the middle, at an output rate that is not the file's: stereo, and not silent.
    let samples = reader.read_stereo(44_100 * 10, 44_100, 44_100).unwrap();
    assert_eq!(samples.len(), 44_100 * 2);
    let peak = samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    assert!(peak > 0.01 && peak <= 1.0, "{peak}");
    let _ = std::fs::remove_dir_all(&dir);
}
