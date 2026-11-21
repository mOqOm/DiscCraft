//! Field pictures (M12.38): with [`Encoder::set_field_pictures`] each frame is coded as two field pictures, the first
//! field first: an I frame as an I field and a P field, a P frame as two P fields, a B frame as two B fields, both
//! under the frame's temporal_reference. The stream decodes without errors as the encoder reconstructed it in
//! craft-video's decoder, close to the source, and (when ffmpeg is installed) to the same pictures in ffmpeg.

use craft_video::mpeg2::Decoder;
use disccraft_mpeg2venc::encoder::{CodedPicture, Encoder, EncoderConfig, Format};
use disccraft_mpeg2venc::{Frame420, Standard};
use std::path::PathBuf;
use std::process::Command;

/// A textured scene panning by (2, 1.5) px a field: field time `field`.
fn scene(w: usize, h: usize, field: usize) -> Vec<u8> {
    let mut y = vec![0u8; w * h];
    for row in 0..h {
        for col in 0..w {
            let fx = col as f64 + 2.0 * field as f64;
            let fy = row as f64 + 1.5 * field as f64;
            let v = 128.0
                + 50.0 * (fx / 13.0).sin() * (fy / 17.0).cos()
                + 30.0 * ((fx + fy) / 7.0).sin();
            y[row * w + col] = v.round().clamp(16.0, 235.0) as u8;
        }
    }
    y
}

/// Frame `t`: its top field from field time `2t`, its bottom field from `2t + 1`.
fn woven(w: usize, h: usize, t: usize) -> Frame420 {
    let (mut y, bottom) = (scene(w, h, 2 * t), scene(w, h, 2 * t + 1));
    for row in (1..h).step_by(2) {
        y[row * w..(row + 1) * w].copy_from_slice(&bottom[row * w..(row + 1) * w]);
    }
    let (cw, ch) = (w / 2, h / 2);
    Frame420 {
        width: w as u16,
        height: h as u16,
        y,
        cb: (0..cw * ch)
            .map(|i| (110 + ((i % cw + t) * 40 / cw) % 40) as u8)
            .collect(),
        cr: (0..cw * ch)
            .map(|i| (140 - ((i / cw + t) * 30 / ch) % 30) as u8)
            .collect(),
    }
}

fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let mse: f64 = a
        .iter()
        .zip(b)
        .map(|(&x, &y)| (f64::from(x) - f64::from(y)).powi(2))
        .sum::<f64>()
        / a.len() as f64;
    if mse == 0.0 {
        99.0
    } else {
        10.0 * (255.0 * 255.0 / mse).log10()
    }
}

/// Each picture of `access_unit`: (picture_coding_type, temporal_reference, picture_structure).
fn pictures(access_unit: &[u8]) -> Vec<(u8, u16, u8)> {
    let mut found = Vec::new();
    let mut header = None;
    for at in 0..access_unit.len().saturating_sub(8) {
        if access_unit[at..at + 3] != [0, 0, 1] {
            continue;
        }
        match access_unit[at + 3] {
            0x00 => {
                let reference =
                    (u16::from(access_unit[at + 4]) << 2) | u16::from(access_unit[at + 5] >> 6);
                header = Some(((access_unit[at + 5] >> 3) & 7, reference));
            }
            0xB5 if access_unit[at + 4] >> 4 == 8 => {
                if let Some((kind, reference)) = header.take() {
                    found.push((kind, reference, access_unit[at + 6] & 3));
                }
            }
            _ => {}
        }
    }
    found
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
fn frames_coded_as_field_pictures_decode_as_reconstructed() {
    let (w, h) = (720, 480);
    let sources: Vec<Frame420> = (0..30).map(|t| woven(w, h, t)).collect();
    let mut encoder = Encoder::new(EncoderConfig {
        standard: Standard::Ntsc,
        gop_size: 15,
        bitrate: 4_000_000,
        max_bitrate: 9_800_000,
        wide: false,
        format: Format::Sd,
        pulldown: false,
    })
    .unwrap();
    encoder.set_b_frames(2);
    encoder.set_field_pictures(true);
    let mut coded: Vec<CodedPicture> = Vec::new();
    for (index, frame) in sources.iter().enumerate() {
        coded.extend(encoder.push_frame(frame, index % 15 == 0).unwrap());
    }
    coded.extend(encoder.flush().unwrap());
    let last = encoder.reconstruction().unwrap().y.clone();
    let mut es: Vec<u8> = coded
        .iter()
        .flat_map(|picture| picture.data.clone())
        .collect();
    es.extend_from_slice(&encoder.finish());

    // Two field pictures an access unit, the top field first, under one temporal_reference: I + P, P + P, B + B.
    for picture in &coded {
        let fields = pictures(&picture.data);
        assert_eq!(fields.len(), 2, "{fields:?}");
        assert_eq!((fields[0].2, fields[1].2), (1, 2), "{fields:?}");
        assert_eq!(fields[0].1, fields[1].1, "{fields:?}");
        let kinds = (fields[0].0, fields[1].0);
        let expected = if picture.stats.intra {
            (1, 2)
        } else if picture.stats.bidirectional {
            (3, 3)
        } else {
            (2, 2)
        };
        assert_eq!(kinds, expected, "{fields:?}");
    }

    let mut decoder = Decoder::new();
    let mut decoded = Vec::new();
    for (index, range) in craft_video::mpeg2::access_units(&es)
        .into_iter()
        .enumerate()
    {
        decoded.extend(decoder.decode(&es[range], index as i64).unwrap());
    }
    decoded.extend(decoder.flush());
    assert_eq!(
        decoder.errors(),
        0,
        "decoder errors: {:?}",
        decoder.last_error()
    );
    assert_eq!(decoded.len(), sources.len());
    let mean = decoded
        .iter()
        .zip(&sources)
        .map(|(picture, source)| psnr(&picture.y, &source.y))
        .sum::<f64>()
        / sources.len() as f64;
    assert!(mean > 32.0, "{mean:.2} dB");
    let shown = &decoded[usize::try_from(coded.last().unwrap().display).unwrap()];
    let drift = shown
        .y
        .iter()
        .zip(&last)
        .map(|(&a, &b)| (i32::from(a) - i32::from(b)).abs())
        .sum::<i32>() as f64
        / last.len() as f64;
    assert!(
        drift < 0.25,
        "decoder vs encoder reconstruction drift {drift}"
    );

    // ffmpeg as a second decoder: the same pictures.
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping its check");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-field-pictures-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fields.m2v");
    std::fs::write(&path, &es).unwrap();
    let output = Command::new(&ffmpeg)
        .args(["-v", "error", "-xerror", "-f", "mpegvideo", "-i"])
        .arg(&path)
        .args(["-f", "rawvideo", "-pix_fmt", "yuv420p", "-"])
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The luma of each 4:2:0 picture.
    let frames: Vec<&[u8]> = output
        .stdout
        .chunks(w * h * 3 / 2)
        .map(|picture| &picture[..w * h])
        .collect();
    assert_eq!(frames.len(), sources.len());
    // The two decoders' inverse DCTs round a little differently (13818-2 annex A allows it): a sample or two in a
    // hundred one or two apart, never a picture of its own.
    for (index, (theirs, ours)) in frames.iter().zip(&decoded).enumerate() {
        let differ = theirs.iter().zip(&ours.y).filter(|(a, b)| a != b).count();
        let most = theirs
            .iter()
            .zip(&ours.y)
            .map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs())
            .max()
            .unwrap_or(0);
        assert!(
            differ * 20 < w * h && most <= 3,
            "picture {index}: {differ} samples differ, by up to {most}"
        );
    }
}
