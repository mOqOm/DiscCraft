//! M12.43: an HD transcode to HEVC (Ultra HD Blu-ray's codec): Main 10, 4:2:0 at 10 bits, the size and rate asked
//! for, an IDR picture every second (24 pictures at 23.976), an access unit delimiter first in every access unit and
//! the parameter sets before every IDR picture; it decodes without errors and looks like the source. ffmpeg is the
//! oracle (skipped without it).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::PathBuf;
use std::process::Command;

use disccraft_transcode::RateMode;
use disccraft_transcode::hd::{HdCodec, HdOptions, read_h264_index, transcode_h264};

fn tool(env: &str, name: &str) -> Option<PathBuf> {
    let path = std::env::var_os(env).map_or_else(|| PathBuf::from(name), PathBuf::from);
    Command::new(&path)
        .arg("-version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|_| path)
}

#[test]
fn an_hd_transcode_makes_hevc_main_10() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-hevc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.mp4");
    let made = Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=1280x720:rate=24000/1001")
        .args([
            "-t",
            "1.3",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-qp",
            "12",
        ])
        .args(["-pix_fmt", "yuv420p"])
        .arg(&source)
        .status()
        .unwrap();
    assert!(made.success());
    let options = HdOptions {
        width: 1280,
        height: 720,
        fps_num: 24_000,
        fps_den: 1001,
        target_kbps: 8000,
        max_kbps: 15_000,
        codec: HdCodec::Hevc,
        quality: 5,
        rendering: disccraft_transcode::Rendering::default(),
        // A two-pass encode is made in one.
        mode: RateMode::Vbr2Pass,
        interlaced: false,
        wide: true,
    };
    let (es, index) = (dir.join("out.hevc"), dir.join("out.idx"));
    let frames = transcode_h264(&source, &es, &index, &options, &mut |_| {}).unwrap();
    assert_eq!(frames, 31);
    let units = read_h264_index(&index).unwrap();
    assert_eq!(units.len(), 31);
    let keys: Vec<usize> = units
        .iter()
        .enumerate()
        .filter(|(_, unit)| unit.keyframe)
        .map(|(at, _)| at)
        .collect();
    assert_eq!(keys, [0, 24]);
    let bytes = std::fs::read(&es).unwrap();
    for unit in &units {
        let start = usize::try_from(unit.offset).unwrap();
        // An access unit delimiter first; a VPS (type 32) right after it on an IDR picture.
        assert_eq!(&bytes[start..start + 7], &[0, 0, 0, 1, 0x46, 0x01, 0x50]);
        assert_eq!(
            bytes[start + 11] >> 1 == 32,
            unit.keyframe,
            "unit at {start}"
        );
    }

    let probed = Command::new(&ffprobe)
        .args(["-v", "error", "-f", "hevc", "-select_streams", "v:0"])
        .args([
            "-show_entries",
            "stream=codec_name,profile,pix_fmt,width,height",
        ])
        .args(["-of", "csv=p=0"])
        .arg(&es)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probed.stdout);
    assert_eq!(text.trim(), "hevc,Main 10,1280,720,yuv420p10le", "{text}");
    let decoded = Command::new(&ffmpeg)
        .args(["-v", "error", "-xerror", "-f", "hevc", "-i"])
        .arg(&es)
        .args(["-f", "rawvideo", "-pix_fmt", "yuv420p", "-"])
        .output()
        .unwrap();
    assert!(
        decoded.status.success(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    let reference = Command::new(&ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(&source)
        .args([
            "-frames:v",
            "31",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "yuv420p",
            "-",
        ])
        .output()
        .unwrap();
    // The Y planes of yuv420p (a gray output would be range-scaled).
    let (luma, frame) = (1280 * 720, 1280 * 720 * 3 / 2);
    assert_eq!(decoded.stdout.len(), 31 * frame);
    assert_eq!(reference.stdout.len(), 31 * frame);
    // PSNR of the luma over all pictures.
    let squared: f64 = decoded
        .stdout
        .chunks(frame)
        .zip(reference.stdout.chunks(frame))
        .flat_map(|(a, b)| a[..luma].iter().zip(&b[..luma]))
        .map(|(a, b)| (f64::from(*a) - f64::from(*b)).powi(2))
        .sum();
    let mse = squared / (31 * luma) as f64;
    let psnr = 10.0 * (255.0 * 255.0 / mse.max(1e-9)).log10();
    assert!(psnr > 30.0, "PSNR {psnr:.1} dB");
    let _ = std::fs::remove_dir_all(&dir);
}
