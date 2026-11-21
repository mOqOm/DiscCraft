//! M12.40: a registered hardware H.264 encoder makes an HD transcode's progressive one-pass H.264: it gets every
//! picture as NV12 with its time and the Blu-ray settings (4 slices, a one-second GOP, 90 % of the target as the
//! average), and its access units are the stream and its index. A two-pass encode stays on the CPU, and so does a
//! video whose encoder cannot start or fails on the way (encoded again on the CPU). A fake encoder stands for the
//! hardware; ffmpeg makes the source (skipped without it).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

use disccraft_transcode::RateMode;
use disccraft_transcode::hd::{
    HardwareH264, HardwareH264Config, HardwarePacket, HdCodec, HdOptions, read_h264_index,
    set_hardware_h264, transcode_h264,
};

static PICTURES: AtomicUsize = AtomicUsize::new(0);
/// The picture the fake fails on (-1: none).
static FAIL_AT: AtomicI64 = AtomicI64::new(-1);

/// What a fake access unit holds: a delimiter and a byte of its own.
const UNIT: [u8; 7] = [0, 0, 0, 1, 9, 0xF0, 0xAA];

struct Fake {
    size: usize,
}

impl HardwareH264 for Fake {
    fn encode(&mut self, nv12: &[u8], pts: i64) -> Result<Vec<HardwarePacket>, String> {
        assert_eq!(nv12.len(), self.size);
        let number = PICTURES.fetch_add(1, Ordering::SeqCst);
        if i64::try_from(number).unwrap() == FAIL_AT.load(Ordering::SeqCst) {
            return Err("the fake encoder failed".to_string());
        }
        Ok(vec![HardwarePacket {
            data: UNIT.to_vec(),
            pts,
            key: number.is_multiple_of(24),
        }])
    }

    fn finish(&mut self) -> Result<Vec<HardwarePacket>, String> {
        Ok(Vec::new())
    }
}

fn fake(config: &HardwareH264Config) -> Result<Box<dyn HardwareH264>, String> {
    assert_eq!((config.slices, config.gop, config.sar), (4, 24, (1, 1)));
    assert_eq!(
        (config.bitrate, config.max_bitrate, config.cbr),
        (5_400_000, 15_000_000, false)
    );
    let size = usize::try_from(config.width * config.height * 3 / 2).unwrap();
    Ok(Box::new(Fake { size }))
}

fn refusing(_config: &HardwareH264Config) -> Result<Box<dyn HardwareH264>, String> {
    Err("no hardware encoder".to_string())
}

fn ffmpeg() -> Option<PathBuf> {
    let path =
        std::env::var_os("DISCCRAFT_FFMPEG").map_or_else(|| PathBuf::from("ffmpeg"), PathBuf::from);
    Command::new(&path)
        .arg("-version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|_| path)
}

#[test]
fn a_registered_hardware_encoder_makes_progressive_hd_h264() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-hd-hardware-{}", std::process::id()));
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
            "testsrc2=size=1280x720:rate=24000/1001",
        ])
        .args([
            "-t",
            "1",
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
    let options = HdOptions {
        width: 1280,
        height: 720,
        fps_num: 24_000,
        fps_den: 1001,
        target_kbps: 6000,
        max_kbps: 15_000,
        codec: HdCodec::H264,
        quality: 5,
        rendering: disccraft_transcode::Rendering::default(),
        mode: RateMode::Vbr,
        interlaced: false,
        wide: true,
    };
    let (es, index) = (dir.join("out.h264"), dir.join("out.idx"));
    let run = |options: &HdOptions| {
        let frames = transcode_h264(&source, &es, &index, options, &mut |_| {}).unwrap();
        (
            frames,
            read_h264_index(&index).unwrap(),
            std::fs::read(&es).unwrap(),
        )
    };

    // The fake makes the stream: a unit a picture, the index pointing at each, key frames as it says.
    set_hardware_h264(Some(fake));
    let (frames, units, bytes) = run(&options);
    let pictures = PICTURES.load(Ordering::SeqCst);
    assert_eq!(usize::try_from(frames).unwrap(), pictures);
    assert_eq!(units.len(), pictures);
    assert_eq!(bytes, UNIT.repeat(pictures));
    assert_eq!(
        units.iter().filter(|unit| unit.keyframe).count(),
        pictures.div_ceil(24)
    );

    // Two passes: the CPU's encoder (real H.264, with its sequence parameter set).
    let two_pass = HdOptions {
        mode: RateMode::Vbr2Pass,
        ..options.clone()
    };
    let (_, units, bytes) = run(&two_pass);
    assert_eq!(PICTURES.load(Ordering::SeqCst), pictures);
    assert_eq!(units.len(), pictures);
    assert!(
        bytes
            .windows(5)
            .any(|window| window == [0, 0, 0, 1, 0x67] || window[1..] == [0, 0, 1, 0x67])
    );

    // An encoder that does not start, and one that fails on picture 5: the CPU encodes the video.
    set_hardware_h264(Some(refusing));
    let (_, units, bytes) = run(&options);
    assert_eq!(units.len(), pictures);
    assert_ne!(bytes, UNIT.repeat(pictures));
    set_hardware_h264(Some(fake));
    FAIL_AT.store(i64::try_from(pictures + 5).unwrap(), Ordering::SeqCst);
    let (_, units, bytes) = run(&options);
    assert_eq!(PICTURES.load(Ordering::SeqCst), pictures + 6);
    assert_eq!(units.len(), pictures);
    assert_ne!(bytes, UNIT.repeat(pictures));

    set_hardware_h264(None);
    let _ = std::fs::remove_dir_all(&dir);
}
