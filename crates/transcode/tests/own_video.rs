//! M6.561: video formats DiscCraft decodes itself, in the containers the media reader opens but cannot decode them
//! in: each frame read from a MOV, an AVI and a Matroska file comes out as ffmpeg decodes it. ffmpeg makes the files
//! and decodes them for the comparison (skipped without it).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_test_support::ffmpeg;
use std::path::Path;
use std::process::Command;

/// A second of a 25-frames-a-second test picture, `width` x `height`, coded with `codec` (its options `extra`) into
/// `path`.
fn make(ffmpeg: &Path, path: &Path, (width, height): (u32, u32), codec: &[&str]) {
    let made = Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("testsrc2=size={width}x{height}:rate=25"))
        .args(["-t", "1"])
        .args(codec)
        .arg(path)
        .status()
        .unwrap();
    assert!(made.success(), "{}", path.display());
}

/// Frame `index` of `path` as ffmpeg decodes it, RGB24.
fn ffmpeg_frame(ffmpeg: &Path, path: &Path, index: u32) -> Vec<u8> {
    let output = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(path)
        .arg("-vf")
        .arg(format!("select=eq(n\\,{index})"))
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .unwrap();
    assert!(output.status.success());
    output.stdout
}

/// The largest difference of one sample between two pictures of the same size.
fn largest_difference(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

#[test]
fn png_frames_decode_in_mov_avi_and_matroska() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-own-video-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let size = (352, 288);
    for name in ["png.mov", "png.avi", "png.mkv"] {
        let path = dir.join(name);
        make(&ffmpeg, &path, size, &["-c:v", "png", "-pix_fmt", "rgb24"]);
        let reader = disccraft_transcode::FrameReader::open(&path).unwrap();
        for (index, seconds) in [(0, 0.0), (12, 12.0 / 25.0), (24, 24.0 / 25.0)] {
            let (width, height, rgb) = reader.frame_sized(seconds, size.0, size.1).unwrap();
            assert_eq!((width, height), size, "{name}");
            let expected = ffmpeg_frame(&ffmpeg, &path, index);
            assert_eq!(
                largest_difference(&rgb, &expected),
                0,
                "{name} frame {index}: as ffmpeg decodes it"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The PSNR of `a` against `b` (8-bit samples), dB.
fn psnr(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let error: f64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| (f64::from(*x) - f64::from(*y)).powi(2))
        .sum::<f64>()
        / a.len() as f64;
    if error == 0.0 {
        return f64::INFINITY;
    }
    10.0 * (255.0 * 255.0 / error).log10()
}

/// M6.562: FFV1 (version 3 with slices and CRCs, and version 1) in a MOV, an AVI and a Matroska file, with frames that
/// carry the key frame's states on (a key frame every second frame): each frame shows as ffmpeg shows it. The planes
/// are exactly ffmpeg's (disccraft-ffv1's tests); here the pictures go through DiscCraft's colour conversion to RGB,
/// so they are compared by PSNR.
#[test]
fn ffv1_decodes_in_mov_avi_and_matroska() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-own-ffv1-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let size = (352, 288);
    let codecs: [(&str, &[&str]); 2] = [
        (
            "v3",
            &[
                "-c:v",
                "ffv1",
                "-level",
                "3",
                "-slices",
                "4",
                "-slicecrc",
                "1",
                "-g",
                "2",
                "-pix_fmt",
                "yuv420p",
            ],
        ),
        (
            "v1",
            &[
                "-c:v", "ffv1", "-level", "1", "-coder", "rice", "-g", "2", "-pix_fmt", "yuv422p",
            ],
        ),
    ];
    for (version, codec) in codecs {
        for container in ["mov", "avi", "mkv"] {
            let name = format!("ffv1-{version}.{container}");
            let path = dir.join(&name);
            make(&ffmpeg, &path, size, codec);
            let reader = disccraft_transcode::FrameReader::open(&path).unwrap();
            for (index, seconds) in [(0, 0.0), (3, 3.0 / 25.0), (13, 13.0 / 25.0)] {
                let (width, height, rgb) = reader.frame_sized(seconds, size.0, size.1).unwrap();
                assert_eq!((width, height), size, "{name}");
                let expected = ffmpeg_frame(&ffmpeg, &path, index);
                let quality = psnr(&rgb, &expected);
                assert!(
                    quality > 30.0,
                    "{name} frame {index}: {quality:.1} dB against ffmpeg"
                );
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Frame `index` of `path` as ffmpeg decodes it, scaled to `width` x `height` (square pixels), RGB24.
fn ffmpeg_frame_scaled(
    ffmpeg: &Path,
    path: &Path,
    index: u32,
    (width, height): (u32, u32),
) -> Vec<u8> {
    let output = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(path)
        .arg("-vf")
        .arg(format!("select=eq(n\\,{index}),scale={width}:{height}"))
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .unwrap();
    assert!(output.status.success());
    output.stdout
}

/// M6.563: H.263 (CIF, 12:11 pixels) in a 3GP, a MOV, an AVI and a Matroska file, P-pictures after each I-picture:
/// each frame shows as ffmpeg shows it, at 4:3 with square pixels (384 x 288). The two scale the picture each their
/// own way, so a frame is checked to be close to ffmpeg's and much closer than the frame twelve before or after it.
#[test]
fn h263_decodes_in_3gp_mov_avi_and_matroska() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-own-h263-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let shown = (384, 288);
    for container in ["3gp", "mov", "avi", "mkv"] {
        let name = format!("h263.{container}");
        let path = dir.join(&name);
        make(
            &ffmpeg,
            &path,
            (352, 288),
            &[
                "-c:v", "h263", "-q:v", "4", "-g", "10", "-pix_fmt", "yuv420p",
            ],
        );
        let reader = disccraft_transcode::FrameReader::open(&path).unwrap();
        for (index, other) in [(0u32, 12u32), (7, 19), (13, 1)] {
            let (width, height, rgb) = reader
                .frame_sized(f64::from(index) / 25.0, shown.0, shown.1)
                .unwrap();
            assert_eq!((width, height), shown, "{name}");
            let right = psnr(&rgb, &ffmpeg_frame_scaled(&ffmpeg, &path, index, shown));
            let wrong = psnr(&rgb, &ffmpeg_frame_scaled(&ffmpeg, &path, other, shown));
            assert!(
                right > 22.0 && right > wrong + 8.0,
                "{name} frame {index}: {right:.1} dB, against frame {other} {wrong:.1} dB"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// M6.567: Hap (DXT1, Hap Q), uncompressed 10-bit RGB (r210, R10k) and Theora (in a Matroska file, and in Ogg files
/// with and without sound, one of a size that is not a multiple of 16, whose picture is a region of its frames), read
/// with craft-video's decoders: each frame shows as ffmpeg shows it.
#[test]
fn hap_rgb10_and_theora_decode() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-own-craftvideo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cases: [(&str, (u32, u32), &[&str], f64); 7] = [
        ("hap.mov", (320, 240), &["-c:v", "hap"], 45.0),
        (
            "hapq.mov",
            (320, 240),
            &["-c:v", "hap", "-format", "hap_q"],
            35.0,
        ),
        ("r210.mov", (320, 240), &["-c:v", "r210"], 45.0),
        ("r10k.mov", (320, 240), &["-c:v", "r10k"], 45.0),
        (
            "theora.mkv",
            (320, 240),
            &["-c:v", "libtheora", "-q:v", "7"],
            35.0,
        ),
        (
            "theora.ogv",
            (330, 250),
            &["-c:v", "libtheora", "-q:v", "7"],
            35.0,
        ),
        (
            "sound.ogv",
            (320, 240),
            // `make` puts `-t 1` before this input, where it limits the sine; the output is cut to a second too.
            &[
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-t",
                "1",
                "-c:v",
                "libtheora",
                "-q:v",
                "7",
                "-c:a",
                "libvorbis",
            ],
            35.0,
        ),
    ];
    for (name, size, codec, least) in cases {
        let path = dir.join(name);
        make(&ffmpeg, &path, size, codec);
        let reader = disccraft_transcode::FrameReader::open(&path).unwrap();
        for index in [0u32, 12, 24] {
            let (width, height, rgb) = reader
                .frame_sized(f64::from(index) / 25.0, size.0, size.1)
                .unwrap();
            assert_eq!((width, height), size, "{name}");
            let quality = psnr(&rgb, &ffmpeg_frame(&ffmpeg, &path, index));
            eprintln!("{name} frame {index}: {quality:.1} dB");
            assert!(
                quality > least,
                "{name} frame {index}: {quality:.1} dB against ffmpeg"
            );
        }
    }
    // The Ogg file's sound is still heard.
    let sound = disccraft_transcode::AudioReader::open(&dir.join("sound.ogv")).unwrap();
    assert!((sound.duration_seconds() - 1.0).abs() < 0.1);
    let _ = std::fs::remove_dir_all(&dir);
}
