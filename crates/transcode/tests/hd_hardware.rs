//! M12.40: a registered hardware H.264 encoder makes an HD transcode's progressive one-pass H.264: it gets every
//! picture as NV12 with its time and the Blu-ray settings (4 slices, a one-second GOP, 90 % of the target as the
//! average), and its access units are the stream and its index. M12.47: a two-pass encode is its one pass, and when
//! the size came out off the target a second at a corrected average, which replaces the first. A video whose encoder
//! cannot start or fails on the way is encoded again on the CPU. M12.44: the same encoder makes
//! HEVC Main 10 from P010 pictures, also when two passes were asked for (HEVC is made in one), and every access unit
//! gets a delimiter and every IDR picture the parameter sets. M12.51: an interlaced format's H.264 goes to the
//! hardware too, its frames to be coded as fields in the source's order. A fake encoder stands for the hardware;
//! ffmpeg makes the source (skipped without it).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_test_support::ffmpeg;
use std::process::Command;
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicUsize, Ordering};

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
    // Progressive formats: frames, not fields.
    assert_eq!(config.fields, None);
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

/// The averages the sized fake was opened at, the last.
static SIZED_MEAN: AtomicU32 = AtomicU32::new(0);

/// A fake whose pictures make a stream 10 % over a 6 Mbit/s target at the first average (filled with 0xAA), and the
/// target's size at any other (0xBB).
struct Sized {
    first: bool,
}

/// The bytes of one picture at 6 Mbit/s and 23.976 frames a second.
const PICTURE_AT_TARGET: usize = 6_000_000 / 8 * 1001 / 24_000;

impl HardwareVideo for Sized {
    fn encode(&mut self, _picture: &[u8], pts: i64) -> Result<Vec<HardwarePacket>, String> {
        let (fill, len) = if self.first {
            (0xAA, PICTURE_AT_TARGET * 11 / 10)
        } else {
            (0xBB, PICTURE_AT_TARGET)
        };
        let mut data = UNIT[..6].to_vec();
        data.resize(len, fill);
        Ok(vec![HardwarePacket {
            data,
            pts,
            key: pts == 0,
        }])
    }

    fn finish(&mut self) -> Result<Vec<HardwarePacket>, String> {
        Ok(Vec::new())
    }
}

fn sized(config: &HardwareConfig) -> Result<Box<dyn HardwareVideo>, String> {
    SIZED_MEAN.store(config.bitrate, Ordering::SeqCst);
    Ok(Box::new(Sized {
        first: config.bitrate == 5_400_000,
    }))
}

/// M12.51: the fields the last fielded fake was asked for: -1 none (frames), 0 the bottom field first, 1 the top.
static FIELDS: AtomicI64 = AtomicI64::new(-2);

/// A fake that takes any format and gives a unit a picture, the first a key frame.
struct Fielded;

impl HardwareVideo for Fielded {
    fn encode(&mut self, _picture: &[u8], pts: i64) -> Result<Vec<HardwarePacket>, String> {
        Ok(vec![HardwarePacket {
            data: UNIT.to_vec(),
            pts,
            key: pts == 0,
        }])
    }

    fn finish(&mut self) -> Result<Vec<HardwarePacket>, String> {
        Ok(Vec::new())
    }
}

fn fielded(config: &HardwareConfig) -> Result<Box<dyn HardwareVideo>, String> {
    FIELDS.store(config.fields.map_or(-1, i64::from), Ordering::SeqCst);
    Ok(Box::new(Fielded))
}

fn refusing(_config: &HardwareConfig) -> Result<Box<dyn HardwareVideo>, String> {
    Err("no hardware encoder".to_string())
}

/// The output of one transcode: its pictures, its index and its stream.
type Made = (u64, Vec<disccraft_transcode::hd::AccessUnit>, Vec<u8>);

/// M12.47: two passes on the hardware, run on `options` with `run` (the fake registered, `pictures` pictures counted
/// so far). The fake's units are far under the target (an easy video): its one pass is kept, no second. A first pass
/// 10 % over the target: a second pass at an average 1000 / 1100 as high takes its place, nothing left beside it.
fn two_passes_on_the_hardware(
    options: &HdOptions,
    pictures: usize,
    dir: &std::path::Path,
    run: &dyn Fn(&HdOptions) -> Made,
) {
    let two_pass = HdOptions {
        mode: RateMode::Vbr2Pass,
        ..options.clone()
    };
    let (_, units, bytes) = run(&two_pass);
    assert_eq!(PICTURES.load(Ordering::SeqCst), 2 * pictures);
    assert_eq!(units.len(), pictures);
    assert_eq!(bytes, UNIT.repeat(pictures));
    set_hardware_encoder(Some(sized));
    let (_, units, bytes) = run(&two_pass);
    assert_eq!(units.len(), pictures);
    assert!(bytes.contains(&0xBB) && !bytes.contains(&0xAA));
    let mean = SIZED_MEAN.load(Ordering::SeqCst);
    assert!((4_850_000..4_950_000).contains(&mean), "{mean}");
    assert!(!dir.join("out.h264.second").exists() && !dir.join("out.idx.second").exists());
    set_hardware_encoder(Some(fake));
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

    two_passes_on_the_hardware(&options, pictures, &dir, &run);

    // An encoder that does not start, and one that fails on picture 5: the CPU encodes the video.
    set_hardware_encoder(Some(refusing));
    let (_, units, bytes) = run(&options);
    assert_eq!(units.len(), pictures);
    assert_ne!(bytes, UNIT.repeat(pictures));
    set_hardware_encoder(Some(fake));
    let before = PICTURES.load(Ordering::SeqCst);
    FAIL_AT.store(i64::try_from(before + 5).unwrap(), Ordering::SeqCst);
    let (_, units, bytes) = run(&options);
    assert_eq!(PICTURES.load(Ordering::SeqCst), before + 6);
    assert_eq!(units.len(), pictures);
    assert_ne!(bytes, UNIT.repeat(pictures));

    // M12.51: 1080i from an interlaced source at its rate: the hardware makes it, as fields, the top one first from a
    // source whose top field comes first and the bottom one from one whose bottom field does.
    FAIL_AT.store(-1, Ordering::SeqCst);
    for (order, top_first) in [("tff", true), ("bff", false)] {
        let interlaced = dir.join(format!("{order}.ts"));
        let made = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc2=size=1920x1080:rate=60000/1001")
            .args(["-t", "0.5", "-vf"])
            .arg(format!("tinterlace=mode=interleave_top,setfield={order}"))
            .args(["-c:v", "mpeg2video", "-flags", "+ildct+ilme"])
            .arg(&interlaced)
            .status()
            .unwrap();
        assert!(made.success());
        set_hardware_encoder(Some(fielded));
        let options = HdOptions {
            width: 1920,
            height: 1080,
            fps_num: 30_000,
            fps_den: 1001,
            interlaced: true,
            ..options.clone()
        };
        let frames = transcode_h264(&interlaced, &es, &index, &options, &mut |_| {}).unwrap();
        assert_eq!(
            FIELDS.load(Ordering::SeqCst),
            i64::from(top_first),
            "{order}"
        );
        assert_eq!(
            std::fs::read(&es).unwrap(),
            UNIT.repeat(usize::try_from(frames).unwrap())
        );
    }

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
    if !cfg!(feature = "hevc") {
        // M12.45: without the `hevc` feature HEVC is refused, the hardware encoder not asked.
        assert!(transcode_h264(&source, &es, &index, &hevc, &mut |_| {}).is_err());
        assert_eq!(PICTURES.load(Ordering::SeqCst), before);
        set_hardware_encoder(None);
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
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
