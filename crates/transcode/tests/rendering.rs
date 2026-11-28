//! Encore's rendering checkboxes (M6.327), checked with ffmpeg as the oracle (skipped without it):
//! "Use Maximum Render Quality" turns a one-pixel checkerboard scaled to the DVD's size into an even grey where the
//! usual bilinear sampling shows a pattern of it; "Use Frame Blending" makes the 29.97 pictures that fall between
//! two frames of a 24 fps video that flickers black and white grey mixes of the two.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use disccraft_test_support::ffmpeg;
use std::env;
use std::path::Path;
use std::process::Command;

use disccraft_transcode::{Rendering, Standard, TranscodeOptions, transcode};

/// A near-lossless H.264 file of `seconds` made by the lavfi graph `graph` (the decoder takes no lossless one).
fn make(ffmpeg: &Path, graph: &str, seconds: &str, path: &Path) {
    let made = Command::new(ffmpeg)
        .args([
            "-v", "error", "-y", "-f", "lavfi", "-i", graph, "-t", seconds,
        ])
        .args(["-c:v", "libx264", "-qp", "4", "-pix_fmt", "yuv420p"])
        .arg(path)
        .status()
        .unwrap();
    assert!(made.success());
}

/// Each decoded picture's (mean, 10th percentile, 90th percentile) luma, as ffmpeg's signalstats reads them; the
/// decode must have no error.
fn luma(ffmpeg: &Path, video: &Path) -> Vec<(f64, f64, f64)> {
    let output = Command::new(ffmpeg)
        .args(["-v", "error", "-xerror", "-i"])
        .arg(video)
        .args([
            "-vf",
            "signalstats,metadata=print:file=-",
            "-f",
            "null",
            "-",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let value = |line: &str, key: &str| -> Option<f64> {
        line.strip_prefix(key)?
            .strip_prefix('=')?
            .trim()
            .parse()
            .ok()
    };
    let mut frames = Vec::new();
    let mut current = (0.0, 0.0, 0.0);
    for line in text.lines() {
        if let Some(mean) = value(line, "lavfi.signalstats.YAVG") {
            current.0 = mean;
        } else if let Some(low) = value(line, "lavfi.signalstats.YLOW") {
            current.1 = low;
        } else if let Some(high) = value(line, "lavfi.signalstats.YHIGH") {
            current.2 = high;
            frames.push(current);
        }
    }
    frames
}

fn options(rendering: Rendering) -> TranscodeOptions {
    TranscodeOptions {
        standard: Standard::Ntsc,
        video_bitrate: 8_000_000,
        max_bitrate: 9_800_000,
        wide: true,
        rendering,
        ..TranscodeOptions::default()
    }
}

#[test]
fn the_rendering_checkboxes_change_the_pictures() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = env::temp_dir().join(format!("disccraft-rendering-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // Maximum render quality: a one-pixel checkerboard, 1920 x 1080, to 720 x 480.
    let checker = dir.join("checker.mp4");
    make(
        &ffmpeg,
        "nullsrc=s=1920x1080:r=24,geq=lum='if(mod(X+Y\\,2)\\,235\\,16)':cb=128:cr=128",
        "0.5",
        &checker,
    );
    let spread = |rendering: Rendering, name: &str| -> f64 {
        let out = dir.join(name);
        let _ = transcode(
            &checker,
            &out,
            &dir.join(format!("{name}.wav")),
            &options(rendering),
            &mut |_| {},
        )
        .unwrap();
        let frames = luma(&ffmpeg, &out);
        assert!(!frames.is_empty());
        frames.iter().map(|(_, low, high)| high - low).sum::<f64>() / frames.len() as f64
    };
    let plain = spread(Rendering::default(), "plain.m2v");
    let fine = spread(
        Rendering {
            max_quality: true,
            ..Rendering::default()
        },
        "fine.m2v",
    );
    eprintln!(
        "checkerboard's 10-90% luma spread: bilinear {plain:.1}, maximum render quality {fine:.1}"
    );
    assert!(plain > 60.0, "{plain}");
    assert!(fine < 20.0, "{fine}");

    // Frame blending: black and white frames in turn at 24 fps, to 29.97.
    let flicker = dir.join("flicker.mp4");
    make(
        &ffmpeg,
        "nullsrc=s=640x360:r=24,geq=lum='if(mod(N\\,2)\\,235\\,16)':cb=128:cr=128",
        "1",
        &flicker,
    );
    let greys = |rendering: Rendering, name: &str| -> (usize, usize) {
        let out = dir.join(name);
        let _ = transcode(
            &flicker,
            &out,
            &dir.join(format!("{name}.wav")),
            &options(rendering),
            &mut |_| {},
        )
        .unwrap();
        let frames = luma(&ffmpeg, &out);
        let grey = frames
            .iter()
            .filter(|(mean, _, _)| (40.0..=215.0).contains(mean))
            .count();
        (grey, frames.len())
    };
    let (held, total) = greys(Rendering::default(), "held.m2v");
    let (mixed, _) = greys(
        Rendering {
            frame_blend: true,
            ..Rendering::default()
        },
        "mixed.m2v",
    );
    eprintln!("grey pictures of {total}: held {held}, blended {mixed}");
    assert!(total >= 29, "{total}");
    assert_eq!(held, 0);
    // Four of every five 29.97 pictures fall between two 24 fps frames.
    assert!(mixed >= total * 2 / 3, "{mixed} of {total}");
    let _ = std::fs::remove_dir_all(&dir);
}
