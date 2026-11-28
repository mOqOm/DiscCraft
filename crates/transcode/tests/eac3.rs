//! M6.565: Dolby Digital Plus (E-AC-3) is heard. A raw `.eac3` / `.ec3` stream (and one named `.ac3`) is decoded
//! with craft-audio's E-AC-3 decoder, through the path raw AC-3 files take, and its length is known without
//! decoding it. ffmpeg makes the streams and decodes them for the comparison; skipped without it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use disccraft_test_support::ffmpeg;
use std::path::Path;
use std::process::Command;

use disccraft_transcode::{AudioReader, probe_audio_seconds};

/// Three seconds of 440 Hz on the left and 660 Hz on the right, as E-AC-3 at `layout` into `out`.
fn make(ffmpeg: &Path, out: &Path, layout: &str) {
    let mut command = Command::new(ffmpeg);
    command.args(["-v", "error", "-y"]);
    command.args([
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:duration=3:sample_rate=48000",
    ]);
    command.args([
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=660:duration=3:sample_rate=48000",
    ]);
    if layout == "stereo" {
        command.args(["-filter_complex", "[0][1]amerge=inputs=2[a]"]);
    } else {
        // 5.1 with the tones on the front left and right only.
        command.args([
            "-filter_complex",
            "[0][1]amerge=inputs=2,pan=5.1|FL=c0|FR=c1|FC=0*c0|LFE=0*c0|BL=0*c0|BR=0*c0[a]",
        ]);
    }
    command.args(["-map", "[a]", "-c:a", "eac3", "-b:a", "256k", "-f", "eac3"]);
    let status = command.arg(out).status().unwrap();
    assert!(status.success(), "{}", out.display());
}

/// `path` decoded by ffmpeg to interleaved stereo f32 at 48 kHz (5.1 folded down by ffmpeg).
fn ffmpeg_stereo(ffmpeg: &Path, path: &Path) -> Vec<f32> {
    let output = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-f", "f32le", "-ac", "2", "-ar", "48000", "-"])
        .output()
        .unwrap();
    assert!(output.status.success());
    output
        .stdout
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
        .collect()
}

/// The power of `frequency` (Hz) in one channel (`side` 0 left, 1 right) of interleaved stereo at 48 kHz.
fn tone(samples: &[f32], side: usize, frequency: f64) -> f64 {
    let step = std::f64::consts::TAU * frequency / 48_000.0;
    let (mut re, mut im) = (0.0, 0.0);
    for (index, frame) in samples.as_chunks::<2>().0.iter().enumerate() {
        let value = f64::from(frame[side]);
        re += value * (step * index as f64).cos();
        im += value * (step * index as f64).sin();
    }
    (re * re + im * im) / (samples.len() / 2).max(1) as f64
}

/// The signal-to-noise ratio (dB) of `got` against `want`, at the offset (within a frame) where they match best.
fn best_snr(got: &[f32], want: &[f32]) -> f64 {
    let mut best = f64::NEG_INFINITY;
    for shift in -1536i64..=1536 {
        let (mut signal, mut noise) = (0.0f64, 0.0f64);
        for (index, value) in got.iter().enumerate().step_by(7) {
            let Some(other) = usize::try_from(index as i64 + shift * 2)
                .ok()
                .and_then(|at| want.get(at))
            else {
                continue;
            };
            signal += f64::from(*other).powi(2);
            noise += f64::from(*value - *other).powi(2);
        }
        if signal > 0.0 {
            best = best.max(10.0 * (signal / noise.max(1e-12)).log10());
        }
    }
    best
}

#[test]
fn eac3_streams_are_heard_as_ffmpeg_hears_them() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-eac3-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, layout) in [
        ("stereo.eac3", "stereo"),
        ("surround.ec3", "5.1"),
        ("named.ac3", "stereo"),
    ] {
        let path = dir.join(name);
        make(&ffmpeg, &path, layout);
        let seconds = probe_audio_seconds(&path).unwrap();
        assert!((seconds - 3.0).abs() < 0.05, "{name}: {seconds} s");
        let reader = AudioReader::open(&path).unwrap();
        // A second from the middle.
        let got = reader.read_stereo(48_000, 48_000, 48_000).unwrap();
        assert_eq!(got.len(), 96_000, "{name}");
        // The tones stay on their sides.
        let (left, right) = (tone(&got, 0, 440.0), tone(&got, 1, 660.0));
        assert!(left > 1e-3 && right > 1e-3, "{name}: {left} {right}");
        assert!(
            tone(&got, 0, 660.0) < left / 1000.0,
            "{name}: right tone on the left"
        );
        assert!(
            tone(&got, 1, 440.0) < right / 1000.0,
            "{name}: left tone on the right"
        );
        if layout == "stereo" {
            // A stereo stream is heard as ffmpeg hears it (the decoders' dither differs, so not bit for bit).
            let want = ffmpeg_stereo(&ffmpeg, &path);
            let snr = best_snr(&got, want.get(96_000..).unwrap());
            eprintln!("{name}: {snr:.1} dB against ffmpeg");
            assert!(snr > 30.0, "{name}: {snr:.1} dB");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
