//! M12.40: a registered hardware H.264 encoder makes an HD transcode's progressive one-pass H.264: it gets every
//! picture as NV12 with its time and the Blu-ray settings (4 slices, a one-second GOP, 90 % of the target as the
//! average), and its access units are the stream and its index. A two-pass encode stays on the CPU, and so does a
//! video whose encoder cannot start or fails on the way (encoded again on the CPU). M12.44: the same encoder makes
//! HEVC Main 10 from P010 pictures, also when two passes were asked for (HEVC is made in one), and every access unit
//! gets a delimiter and every IDR picture the parameter sets. A fake encoder stands for the hardware; ffmpeg makes
//! the source (skipped without it).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

use disccraft_transcode::RateMode;
use disccraft_transcode::hd::{
    HardwareCodec, HardwareConfig, HardwarePacket, HardwareVideo, HdCodec, HdOptions,
    read_h264_index, set_hardware_encoder, transcode_h264,
};

static PICTURES: AtomicUsize = AtomicUsize::new(0);
/// The picture the fake fails on (-1: none).
static FAIL_AT: AtomicI64 = AtomicI64::new(-1);

/// What a fake access unit holds: a delimiter and a byte of its own.
const UNIT: [u8; 7] = [0, 0, 0, 1, 9, 0xF0, 0xAA];

/// The fake HEVC encoder's parameter sets (VPS, SPS, the PPS after a three-byte start code), given with its first
/// IDR picture only, and its pictures: an IDR slice (type 19) and a trailing one (type 1), without delimiters.
const HEVC_SETS: [u8; 20] = [
    0, 0, 0, 1, 0x40, 0x01, 0xA1, 0, 0, 0, 1, 0x42, 0x01, 0xA2, 0, 0, 1, 0x44, 0x01, 0xA3,
];
const HEVC_IDR: [u8; 7] = [0, 0, 0, 1, 0x26, 0x01, 0xB1];
const HEVC_TRAIL: [u8; 7] = [0, 0, 0, 1, 0x02, 0x01, 0xB2];
/// The delimiter and the four-byte-start-code sets the transcode puts first.
const HEVC_AUD: [u8; 7] = [0, 0, 0, 1, 0x46, 0x01, 0x50];
const HEVC_SETS_OUT: [u8; 21] = [
    0, 0, 0, 1, 0x40, 0x01, 0xA1, 0, 0, 0, 1, 0x42, 0x01, 0xA2, 0, 0, 0, 1, 0x44, 0x01, 0xA3,
];

struct Fake {
    codec: HardwareCodec,
    size: usize,
    first: usize,
}

impl HardwareVideo for Fake {
    fn encode(&mut self, picture: &[u8], pts: i64) -> Result<Vec<HardwarePacket>, String> {
        assert_eq!(picture.len(), self.size);
        let number = PICTURES.fetch_add(1, Ordering::SeqCst);
        if i64::try_from(number).unwrap() == FAIL_AT.load(Ordering::SeqCst) {
            return Err("the fake encoder failed".to_string());
        }
        let key = (number - self.first).is_multiple_of(24);
        let data = match self.codec {
            HardwareCodec::H264 => UNIT.to_vec(),
            HardwareCodec::HevcMain10 => {
                // P010: the low byte of every sample is 0.
                assert!(picture.iter().step_by(2).all(|low| *low == 0));
                match (key, number == self.first) {
                    (true, true) => [HEVC_SETS.as_slice(), &HEVC_IDR].concat(),
                    (true, false) => HEVC_IDR.to_vec(),
                    (false, _) => HEVC_TRAIL.to_vec(),
                }
            }
        };
        Ok(vec![HardwarePacket { data, pts, key }])
    }

    fn finish(&mut self) -> Result<Vec<HardwarePacket>, String> {
        Ok(Vec::new())
    }
}

fn fake(config: &HardwareConfig) -> Result<Box<dyn HardwareVideo>, String> {
    let (slices, bytes) = match config.codec {
        HardwareCodec::H264 => (4, 1),
        HardwareCodec::HevcMain10 => (1, 2),
    };
    assert_eq!(
        (config.slices, config.gop, config.sar),
        (slices, 24, (1, 1))
    );
    assert_eq!(
        (config.bitrate, config.max_bitrate, config.cbr),
        (5_400_000, 15_000_000, false)
    );
    let size = usize::try_from(config.width * config.height * 3 / 2 * bytes).unwrap();
    Ok(Box::new(Fake {
        codec: config.codec,
        size,
        first: PICTURES.load(Ordering::SeqCst),
    }))
}

fn refusing(_config: &HardwareConfig) -> Result<Box<dyn HardwareVideo>, String> {
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
    set_hardware_encoder(Some(fake));
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
    set_hardware_encoder(Some(refusing));
    let (_, units, bytes) = run(&options);
    assert_eq!(units.len(), pictures);
    assert_ne!(bytes, UNIT.repeat(pictures));
    set_hardware_encoder(Some(fake));
    FAIL_AT.store(i64::try_from(pictures + 5).unwrap(), Ordering::SeqCst);
    let (_, units, bytes) = run(&options);
    assert_eq!(PICTURES.load(Ordering::SeqCst), pictures + 6);
    assert_eq!(units.len(), pictures);
    assert_ne!(bytes, UNIT.repeat(pictures));

    // HEVC Main 10 (M12.44), asked for in two passes: the fake makes it in one, from P010 pictures; every unit
    // gets a delimiter, every IDR picture the parameter sets of the first.
    set_hardware_encoder(Some(fake));
    FAIL_AT.store(-1, Ordering::SeqCst);
    let before = PICTURES.load(Ordering::SeqCst);
    let hevc = HdOptions {
        codec: HdCodec::Hevc,
        mode: RateMode::Vbr2Pass,
        ..options.clone()
    };
    let (frames, units, bytes) = run(&hevc);
    assert_eq!(PICTURES.load(Ordering::SeqCst) - before, pictures);
    assert_eq!(usize::try_from(frames).unwrap(), pictures);
    let mut expected = Vec::new();
    for (number, unit) in units.iter().enumerate() {
        assert_eq!(unit.keyframe, number.is_multiple_of(24));
        expected.extend_from_slice(&HEVC_AUD);
        if unit.keyframe {
            expected.extend_from_slice(&HEVC_SETS_OUT);
            expected.extend_from_slice(&HEVC_IDR);
        } else {
            expected.extend_from_slice(&HEVC_TRAIL);
        }
    }
    assert_eq!(bytes, expected);

    set_hardware_encoder(None);
    let _ = std::fs::remove_dir_all(&dir);
}
