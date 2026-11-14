//! The encoded AC-3 decodes in ffmpeg (the external oracle) in step with its source: a click at 1.000 s is heard at
//! 1.000 s, so a soundtrack stays in sync with its picture. Skipped when ffmpeg is not available.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

fn ffmpeg() -> Option<PathBuf> {
    let path =
        std::env::var_os("DISCCRAFT_FFMPEG").map_or_else(|| PathBuf::from("ffmpeg"), PathBuf::from);
    Command::new(&path)
        .arg("-version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| path)
}

fn wav(channels: u16, samples: &[i16]) -> Vec<u8> {
    let data_len = u32::try_from(samples.len() * 2).unwrap();
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&48_000u32.to_le_bytes());
    out.extend_from_slice(&(48_000 * 2 * u32::from(channels)).to_le_bytes());
    out.extend_from_slice(&(2 * channels).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

#[test]
fn a_click_is_heard_when_it_was_made() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-ac3-oracle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    // Two seconds of silence, stereo, with a click (a short burst) at exactly 1.000 s.
    let mut samples = vec![0i16; 2 * 96_000];
    for n in 48_000..48_008 {
        samples[2 * n] = 24_000;
        samples[2 * n + 1] = 24_000;
    }
    let input = dir.join("click.wav");
    let ac3 = dir.join("click.ac3");
    let decoded = dir.join("click.raw");
    std::fs::write(&input, wav(2, &samples)).unwrap();
    let encoded = disccraft_ac3enc::encode_wav(&input, &ac3).unwrap();
    assert_eq!(encoded.kbps, 192);

    let status = Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-f", "ac3", "-i"])
        .arg(&ac3)
        .args(["-f", "s16le", "-ac", "1"])
        .arg(&decoded)
        .status()
        .unwrap();
    assert!(status.success());
    let raw = std::fs::read(&decoded).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    let pcm: Vec<i16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect();
    // As long as the source (to the frame), and the loudest sample within half a millisecond of 1.000 s.
    assert!(
        pcm.len() >= 96_000 && pcm.len() < 96_000 + 1536,
        "{} samples",
        pcm.len()
    );
    let peak = pcm
        .iter()
        .enumerate()
        .max_by_key(|(_, s)| s.unsigned_abs())
        .map(|(n, _)| n)
        .unwrap();
    assert!(peak.abs_diff(48_000) < 24, "the click is at sample {peak}");
}
