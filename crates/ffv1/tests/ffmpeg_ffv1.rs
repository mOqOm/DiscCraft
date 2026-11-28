//! FFV1 streams ffmpeg makes decode to exactly the samples ffmpeg decodes from them (FFV1 is lossless, so any
//! difference is a decoding error): versions 0, 1 and 3; the Golomb-Rice coder and the range coder with the default
//! and a custom state table; slices with CRCs; Y'CbCr 4:2:0, 4:2:2 and 4:4:4, grey, transparency, RGB; 8, 10 and 16
//! bits; frames that carry the states of the key frame before them. ffmpeg makes the streams (in an AVI, whose chunks
//! and format are read here) and decodes them for the comparison; skipped without it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use disccraft_test_support::ffmpeg;
use std::path::Path;
use std::process::Command;

use disccraft_ffv1::{Decoder, Layout, is_keyframe};

fn le32(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

/// An AVI's first video stream: its width, height, the codec data after its BITMAPINFOHEADER, and its chunks.
fn read_avi(bytes: &[u8]) -> (u32, u32, Vec<u8>, Vec<Vec<u8>>) {
    let mut setup = Vec::new();
    let mut size = (0, 0);
    let mut frames = Vec::new();
    let mut at = 12;
    fn walk(
        bytes: &[u8],
        mut at: usize,
        end: usize,
        out: &mut (Vec<u8>, (u32, u32), Vec<Vec<u8>>),
    ) {
        while at + 8 <= end {
            let id = &bytes[at..at + 4];
            let size = le32(bytes, at + 4);
            let body = at + 8;
            if id == b"LIST" {
                walk(bytes, body + 4, (body + size).min(end), out);
            } else if id == b"strf" && out.0.is_empty() && out.1 == (0, 0) {
                // biSize counts the codec data too when an AVI writer puts it there (ffmpeg does), and the chunk
                // may end in a padding byte: the data is from byte 40 to biSize (RFC 9043 §4.3.3.1).
                let bi_size = le32(bytes, body).min(size);
                out.1 = (le32(bytes, body + 4) as u32, le32(bytes, body + 8) as u32);
                out.0 = bytes[body + 40..body + bi_size.max(40)].to_vec();
            } else if &id[2..4] == b"dc" || &id[2..4] == b"db" {
                out.2.push(bytes[body..body + size].to_vec());
            }
            at = body + size + (size & 1);
        }
    }
    let mut out = (setup, size, frames);
    walk(bytes, at, bytes.len(), &mut out);
    (setup, size, frames) = out;
    at = 0;
    let _ = at;
    (size.0, size.1, setup, frames)
}

/// Frame bytes of `pix_fmt` as ffmpeg decodes `path`, all frames.
fn ffmpeg_raw(ffmpeg: &Path, path: &Path, pix_fmt: &str) -> Vec<u8> {
    let output = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-f", "rawvideo", "-pix_fmt", pix_fmt, "-"])
        .output()
        .unwrap();
    assert!(output.status.success());
    output.stdout
}

/// A decoded picture as the raw bytes of ffmpeg's planar layout of the same format (little-endian above 8 bits).
fn raw_of(picture: &disccraft_ffv1::Picture) -> Vec<u8> {
    let wide = picture.bits > 8;
    let mut out = Vec::new();
    for plane in &picture.planes {
        for sample in plane {
            if wide {
                out.extend_from_slice(&sample.to_le_bytes());
            } else {
                out.push(u8::try_from(*sample).unwrap());
            }
        }
    }
    out
}

struct Case {
    name: &'static str,
    /// The pixel format given to the encoder.
    input: &'static str,
    /// The planar pixel format the pictures are compared in.
    compare: &'static str,
    options: &'static [&'static str],
}

const CASES: &[Case] = &[
    Case {
        name: "v1-golomb-420",
        input: "yuv420p",
        compare: "yuv420p",
        options: &["-level", "1", "-coder", "rice"],
    },
    Case {
        name: "v1-range-420",
        input: "yuv420p",
        compare: "yuv420p",
        options: &["-level", "1", "-coder", "range_def"],
    },
    Case {
        name: "v0-golomb-420",
        input: "yuv420p",
        compare: "yuv420p",
        options: &["-level", "0", "-coder", "rice"],
    },
    Case {
        name: "v3-range-slices-crc-420",
        input: "yuv420p",
        compare: "yuv420p",
        options: &[
            "-level",
            "3",
            "-coder",
            "range_def",
            "-slices",
            "4",
            "-slicecrc",
            "1",
        ],
    },
    Case {
        name: "v3-range-table-422",
        input: "yuv422p",
        compare: "yuv422p",
        options: &[
            "-level",
            "3",
            "-coder",
            "range_tab",
            "-slices",
            "4",
            "-slicecrc",
            "1",
        ],
    },
    Case {
        name: "v3-golomb-444",
        input: "yuv444p",
        compare: "yuv444p",
        options: &[
            "-level",
            "3",
            "-coder",
            "rice",
            "-slices",
            "4",
            "-slicecrc",
            "1",
        ],
    },
    Case {
        name: "v3-range-large-context-420",
        input: "yuv420p",
        compare: "yuv420p",
        options: &[
            "-level",
            "3",
            "-coder",
            "range_tab",
            "-context",
            "1",
            "-slices",
            "6",
        ],
    },
    Case {
        name: "v3-gray",
        input: "gray",
        compare: "gray",
        options: &["-level", "3", "-coder", "range_def"],
    },
    Case {
        name: "v3-alpha",
        input: "yuva420p",
        compare: "yuva420p",
        options: &["-level", "3", "-coder", "range_def"],
    },
    Case {
        name: "v3-rgb",
        input: "bgr0",
        compare: "gbrp",
        options: &["-level", "3", "-coder", "range_def"],
    },
    Case {
        name: "v3-rgb-golomb",
        input: "bgr0",
        compare: "gbrp",
        options: &["-level", "3", "-coder", "rice"],
    },
    Case {
        name: "v3-10bit",
        input: "yuv420p10le",
        compare: "yuv420p10le",
        options: &["-level", "3", "-coder", "range_def"],
    },
    Case {
        name: "v3-rgb-10bit",
        input: "gbrp10le",
        compare: "gbrp10le",
        options: &["-level", "3", "-coder", "range_def"],
    },
    Case {
        name: "v3-16bit",
        input: "yuv420p16le",
        compare: "yuv420p16le",
        options: &["-level", "3", "-coder", "range_def"],
    },
    Case {
        name: "v3-alpha-golomb",
        input: "yuva420p",
        compare: "yuva420p",
        options: &["-level", "3", "-coder", "rice"],
    },
    Case {
        name: "v3-rgb-alpha",
        input: "bgra",
        compare: "gbrap",
        options: &["-level", "3", "-coder", "range_def"],
    },
    Case {
        name: "v3-rgb-alpha-golomb",
        input: "bgra",
        compare: "gbrap",
        options: &["-level", "3", "-coder", "rice"],
    },
    Case {
        name: "v3-gray-golomb",
        input: "gray",
        compare: "gray",
        options: &["-level", "3", "-coder", "rice"],
    },
    Case {
        name: "v1-rgb",
        input: "bgr0",
        compare: "gbrp",
        options: &["-level", "1", "-coder", "range_def"],
    },
    Case {
        name: "v1-rgb-golomb",
        input: "bgr0",
        compare: "gbrp",
        options: &["-level", "1", "-coder", "rice"],
    },
    Case {
        name: "v3-411",
        input: "yuv411p",
        compare: "yuv411p",
        options: &["-level", "3", "-coder", "range_def"],
    },
    Case {
        name: "v3-410",
        input: "yuv410p",
        compare: "yuv410p",
        options: &["-level", "3", "-coder", "range_def"],
    },
    Case {
        name: "v3-12bit-444",
        input: "yuv444p12le",
        compare: "yuv444p12le",
        options: &["-level", "3", "-coder", "range_tab"],
    },
    Case {
        name: "v3-gray16",
        input: "gray16le",
        compare: "gray16le",
        options: &["-level", "3", "-coder", "range_def"],
    },
    Case {
        name: "v3-many-slices-no-crc",
        input: "yuv420p",
        compare: "yuv420p",
        options: &[
            "-level",
            "3",
            "-coder",
            "range_tab",
            "-slices",
            "24",
            "-slicecrc",
            "0",
        ],
    },
];

/// The picture size of a case: an odd one for some (slices and chroma that do not divide evenly).
fn size_of(case: &Case) -> &'static str {
    match case.name {
        "v3-range-slices-crc-420" | "v3-410" | "v3-rgb-golomb" => "351x287",
        _ => "352x288",
    }
}

#[test]
fn ffmpeg_streams_decode_exactly() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-ffv1-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let only = std::env::var("FFV1_CASE").ok();
    let mut failed = Vec::new();
    for case in CASES {
        if only.as_deref().is_some_and(|only| only != case.name) {
            continue;
        }
        let path = dir.join(format!("{}.avi", case.name));
        let made = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!("testsrc2=size={}:rate=25", size_of(case)))
            .args(["-t", "0.2"])
            .args(["-pix_fmt", case.input, "-c:v", "ffv1", "-g", "2"])
            .args(case.options)
            .arg(&path)
            .status()
            .unwrap();
        assert!(made.success(), "{}", case.name);
        let (width, height, setup, frames) = read_avi(&std::fs::read(&path).unwrap());
        let expected = ffmpeg_raw(&ffmpeg, &path, case.compare);
        let mut decoder = match Decoder::new(width, height, Some(&setup)) {
            Ok(decoder) => decoder,
            Err(error) => {
                failed.push(format!("{}: setup: {error}", case.name));
                continue;
            }
        };
        let frame_bytes = expected.len() / frames.len().max(1);
        let mut keys = 0;
        for (index, frame) in frames.iter().enumerate() {
            keys += usize::from(is_keyframe(frame));
            let picture = match decoder.decode(frame) {
                Ok(picture) => picture,
                Err(error) => {
                    failed.push(format!("{} frame {index}: {error}", case.name));
                    break;
                }
            };
            if case.compare.starts_with("gbr") {
                assert_eq!(picture.layout, Layout::Gbr);
            }
            let got = raw_of(&picture);
            let want = &expected[index * frame_bytes..(index + 1) * frame_bytes];
            if got != want {
                let first = got.iter().zip(want).position(|(a, b)| a != b);
                failed.push(format!(
                    "{} frame {index}: {} bytes vs {}; first difference at {first:?}",
                    case.name,
                    got.len(),
                    want.len()
                ));
                break;
            }
        }
        eprintln!("{}: {} frames, {keys} key frames", case.name, frames.len());
        // The same frames cut short or with a bit flipped: errors or pictures, never a panic.
        if let Ok(mut broken) = Decoder::new(width, height, Some(&setup)) {
            for (index, frame) in frames.iter().enumerate() {
                let _ = broken.decode(&frame[..frame.len() * 2 / 3]);
                let mut flipped = frame.clone();
                let at = (index * 7919 + 13) % flipped.len().max(1);
                if let Some(byte) = flipped.get_mut(at) {
                    *byte ^= 0x10;
                }
                let _ = broken.decode(&flipped);
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(failed.is_empty(), "{failed:#?}");
}
