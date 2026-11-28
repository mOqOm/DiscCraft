//! H.263 streams ffmpeg makes decode as ffmpeg decodes them: I- and P-pictures of sub-QCIF, QCIF, CIF and 4CIF
//! (two macroblock rows a GOB), a fixed quantizer and rate control (quantizer changes in the macroblocks), GOB
//! headers. H.263 leaves the inverse transform's arithmetic to the decoder (within Annex A's tolerance), so the
//! pictures are compared by PSNR, every frame, every plane. ffmpeg makes the streams (in an AVI, whose chunks are read
//! here) and decodes them; skipped without it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use disccraft_test_support::ffmpeg;
use std::path::Path;
use std::process::Command;

use disccraft_h263::{Decoder, is_intra};

fn le32(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

/// An AVI's video chunks.
fn avi_frames(bytes: &[u8]) -> Vec<Vec<u8>> {
    fn walk(bytes: &[u8], mut at: usize, end: usize, out: &mut Vec<Vec<u8>>) {
        while at + 8 <= end {
            let id = &bytes[at..at + 4];
            let size = le32(bytes, at + 4);
            let body = at + 8;
            if id == b"LIST" {
                walk(bytes, body + 4, (body + size).min(end), out);
            } else if &id[2..4] == b"dc" || &id[2..4] == b"db" {
                out.push(bytes[body..body + size].to_vec());
            }
            at = body + size + (size & 1);
        }
    }
    let mut frames = Vec::new();
    walk(bytes, 12, bytes.len(), &mut frames);
    frames
}

fn psnr(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let error = a
        .iter()
        .zip(b)
        .map(|(x, y)| (f64::from(*x) - f64::from(*y)).powi(2))
        .sum::<f64>()
        / a.len() as f64;
    if error == 0.0 {
        99.0
    } else {
        10.0 * (255.0 * 255.0 / error).log10()
    }
}

struct Case {
    name: &'static str,
    size: &'static str,
    options: &'static [&'static str],
}

const CASES: &[Case] = &[
    Case {
        name: "cif-fixed",
        size: "352x288",
        options: &["-q:v", "4", "-g", "12"],
    },
    Case {
        name: "qcif-rate",
        size: "176x144",
        options: &["-b:v", "96k", "-g", "25"],
    },
    Case {
        name: "cif-gob-headers",
        size: "352x288",
        options: &["-q:v", "6", "-g", "12", "-ps", "300"],
    },
    Case {
        name: "subqcif",
        size: "128x96",
        options: &["-q:v", "3", "-g", "10"],
    },
    Case {
        name: "4cif",
        size: "704x576",
        options: &["-q:v", "5", "-g", "15"],
    },
    Case {
        name: "cif-coarse",
        size: "352x288",
        options: &["-q:v", "31", "-g", "30"],
    },
];

fn ffmpeg_yuv(ffmpeg: &Path, path: &Path) -> Vec<u8> {
    let output = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-f", "rawvideo", "-pix_fmt", "yuv420p", "-"])
        .output()
        .unwrap();
    assert!(output.status.success());
    output.stdout
}

#[test]
fn ffmpeg_streams_decode_as_ffmpeg_decodes_them() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-h263-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let only = std::env::var("H263_CASE").ok();
    let mut failed = Vec::new();
    for case in CASES {
        if only.as_deref().is_some_and(|only| only != case.name) {
            continue;
        }
        let path = dir.join(format!("{}.avi", case.name));
        let made = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!("testsrc2=size={}:rate=25", case.size))
            .args(["-t", "2", "-pix_fmt", "yuv420p", "-c:v", "h263"])
            .args(case.options)
            .arg(&path)
            .status()
            .unwrap();
        assert!(made.success(), "{}", case.name);
        let frames = avi_frames(&std::fs::read(&path).unwrap());
        let expected = ffmpeg_yuv(&ffmpeg, &path);
        let frame_size = expected.len() / frames.len().max(1);
        let mut decoder = Decoder::new();
        let mut worst = f64::INFINITY;
        let mut intra = 0;
        for (index, frame) in frames.iter().enumerate() {
            intra += usize::from(is_intra(frame));
            let picture = match decoder.decode(frame) {
                Ok(picture) => picture,
                Err(error) => {
                    failed.push(format!("{} frame {index}: {error}", case.name));
                    break;
                }
            };
            let got: Vec<u8> = [picture.y, picture.cb, picture.cr].concat();
            let want = &expected[index * frame_size..(index + 1) * frame_size];
            if got.len() != want.len() {
                failed.push(format!(
                    "{} frame {index}: {} bytes vs {}",
                    case.name,
                    got.len(),
                    want.len()
                ));
                break;
            }
            worst = worst.min(psnr(&got, want));
        }
        eprintln!(
            "{}: {} frames ({intra} I), worst PSNR {worst:.2} dB",
            case.name,
            frames.len()
        );
        if worst < 45.0 {
            failed.push(format!("{}: worst PSNR {worst:.2} dB", case.name));
        }
        // The same pictures cut short or with a bit flipped: errors or pictures, never a panic.
        let mut broken = Decoder::new();
        for (index, frame) in frames.iter().enumerate() {
            let _ = broken.decode(&frame[..frame.len() / 2]);
            let mut flipped = frame.clone();
            let at = (index * 7919 + 9) % flipped.len().max(1);
            if let Some(byte) = flipped.get_mut(at) {
                *byte ^= 0x08;
            }
            let _ = broken.decode(&flipped);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(failed.is_empty(), "{failed:#?}");
}
