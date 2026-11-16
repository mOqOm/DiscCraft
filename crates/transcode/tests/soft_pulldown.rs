//! Soft pulldown: 24 progressive frames, each shown for 2 or 3 fields by repeat_first_field (2:3), last one second at
//! 29.97, not 24 / 29.97 s, and the frame shown at a time is the one the fields put there. Most film and anime NTSC
//! DVDs are coded so.

use disccraft_mpeg2venc::encoder::{Encoder, EncoderConfig};
use disccraft_mpeg2venc::{Frame420, Standard};
use std::path::PathBuf;

const FRAMES: usize = 24;

/// Frame `index` is a flat grey of its own: luma 40 + 6·index.
fn luma(index: usize) -> u8 {
    u8::try_from(40 + 6 * index).unwrap()
}

/// An NTSC elementary stream of [`FRAMES`] flat frames with repeat_first_field set on every other picture.
fn soft_pulldown_stream() -> Vec<u8> {
    let mut encoder = Encoder::new(EncoderConfig {
        standard: Standard::Ntsc,
        gop_size: 12,
        bitrate: 6_000_000,
        max_bitrate: 9_000_000,
        wide: false,
    })
    .unwrap();
    let mut stream = Vec::new();
    for index in 0..FRAMES {
        let frame = Frame420 {
            width: 720,
            height: 480,
            y: vec![luma(index); 720 * 480],
            cb: vec![128; 360 * 240],
            cr: vec![128; 360 * 240],
        };
        stream.extend(encoder.encode_frame(&frame).unwrap());
    }
    stream.extend(encoder.finish());

    // The picture coding extensions (00 00 01 B5, identifier 8), one per picture in order: repeat_first_field is bit 1
    // of their fourth byte. Odd pictures get it: 2, 3, 2, 3, … fields.
    let mut picture = 0;
    let mut position = 0;
    while position + 8 <= stream.len() {
        if stream[position..position + 4] == [0, 0, 1, 0xB5] && stream[position + 4] >> 4 == 8 {
            if picture % 2 == 1 {
                stream[position + 7] |= 0x02;
            }
            picture += 1;
        }
        position += 1;
    }
    assert_eq!(picture, FRAMES);
    stream
}

fn fixture() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("disccraft-soft-pulldown-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pulldown.m2v");
    std::fs::write(&path, soft_pulldown_stream()).unwrap();
    path
}

/// Which frame a picture is, from the grey in its middle.
fn frame_index(rgb: &[u8]) -> usize {
    let centre = (240 * 720 + 360) * 3;
    let grey = f64::from(rgb[centre]);
    (0..FRAMES)
        .min_by(|&a, &b| {
            let level = |index: usize| (f64::from(luma(index)) - 16.0) * 255.0 / 219.0;
            (level(a) - grey).abs().total_cmp(&(level(b) - grey).abs())
        })
        .unwrap()
}

#[test]
fn soft_pulldown_lasts_its_fields_and_shows_each_frame_when_its_fields_do() {
    let path = fixture();

    // 24 frames of 2.5 fields on average: 60 fields, 30 frame periods.
    let seconds = disccraft_transcode::probe_seconds(&path).unwrap();
    assert!(
        (seconds - 30.0 * 1001.0 / 30000.0).abs() < 1e-6,
        "{seconds} s"
    );

    // Frame k (k even) starts after 5k/2 fields; sample the middle of each frame's showing.
    let field = 1001.0 / 60000.0;
    let mut start = 0.0;
    for index in 0..FRAMES {
        let fields = if index % 2 == 1 { 3.0 } else { 2.0 };
        let middle = start + fields * field / 2.0;
        let (_, _, rgb) = disccraft_transcode::frame_at(&path, middle).unwrap();
        assert_eq!(frame_index(&rgb), index, "at {middle:.4} s");
        start += fields * field;
    }

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
