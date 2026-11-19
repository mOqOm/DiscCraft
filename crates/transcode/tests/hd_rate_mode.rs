//! How an HD Blu-ray encode keeps its rate (M6.328), checked with ffmpeg as the oracle (skipped without it): VBR 2
//! pass goes over the video twice (the progress runs once from 0 to 1, half way after the first pass) and makes
//! the same pictures; CBR keeps the peak at the target. Both codecs, each stream decoding without an error.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use disccraft_transcode::RateMode;
use disccraft_transcode::hd::{HdCodec, HdOptions, read_h264_index, transcode_h264};

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

fn decodes(ffmpeg: &Path, stream: &Path, format: &str) {
    let output = Command::new(ffmpeg)
        .args(["-v", "error", "-xerror", "-f", format, "-i"])
        .arg(stream)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        output.status.success() && output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_blu_ray_encode_keeps_its_rate_its_own_way() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = env::temp_dir().join(format!("disccraft-hd-rate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.mp4");
    let made = Command::new(&ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=1280x720:r=30000/1001",
        ])
        .args([
            "-t",
            "2",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&source)
        .status()
        .unwrap();
    assert!(made.success());

    for (codec, (width, height, fps_num, fps_den), format) in [
        (HdCodec::H264, (1280, 720, 60_000, 1001), "h264"),
        (HdCodec::Mpeg2, (1920, 1080, 30_000, 1001), "mpegvideo"),
    ] {
        let options = |mode: RateMode| HdOptions {
            width,
            height,
            fps_num,
            fps_den,
            target_kbps: 6_000,
            max_kbps: 20_000,
            codec,
            quality: disccraft_transcode::BEST_QUALITY,
            rendering: disccraft_transcode::Rendering::default(),
            mode,
        };
        let encode = |mode: RateMode, name: &str, progress: &mut dyn FnMut(f32)| {
            let es = dir.join(format!("{name}.es"));
            let index = dir.join(format!("{name}.idx"));
            let frames = transcode_h264(&source, &es, &index, &options(mode), progress).unwrap();
            decodes(&ffmpeg, &es, format);
            (
                frames,
                std::fs::read(&es).unwrap(),
                read_h264_index(&index).unwrap(),
            )
        };
        let (one_frames, one, _) = encode(RateMode::Vbr, &format!("{format}-vbr"), &mut |_| {});
        let mut seen = Vec::new();
        let (two_frames, two, two_units) = encode(
            RateMode::Vbr2Pass,
            &format!("{format}-two"),
            &mut |fraction| seen.push(fraction),
        );
        assert_eq!(two_frames, one_frames, "{format}");
        assert_eq!(two_units.len() as u64, two_frames, "{format}");
        assert_ne!(two, one, "{format}");
        assert!(
            seen.windows(2).all(|pair| pair[0] <= pair[1]),
            "{format} {seen:?}"
        );
        assert!(
            seen.iter().any(|fraction| (0.45..=0.55).contains(fraction)),
            "{format}"
        );
        assert_eq!(seen.last().copied(), Some(1.0));
        let (cbr_frames, cbr, _) = encode(RateMode::Cbr, &format!("{format}-cbr"), &mut |_| {});
        assert_eq!(cbr_frames, one_frames, "{format}");
        assert_ne!(cbr, one, "{format}");
        // Around the target, 6 Mbit/s, either way.
        let seconds = one_frames as f64 * f64::from(fps_den) / f64::from(fps_num);
        for (name, stream) in [("two-pass", &two), ("CBR", &cbr)] {
            let kbps = stream.len() as f64 * 8.0 / seconds / 1000.0;
            eprintln!("{format} {name}: {kbps:.0} kbit/s");
            assert!(
                (4_500.0..=7_500.0).contains(&kbps),
                "{format} {name} {kbps}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
