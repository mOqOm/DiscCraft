//! B pictures (M12.32): a sequence coded I B B P … with closed GOPs decodes without errors in craft-video's MPEG-2
//! decoder, in display order and close to the source, and at the same bit rate it looks better than I and P
//! pictures alone.

use craft_video::mpeg2::{Decoder, Picture};
use disccraft_mpeg2venc::encoder::{CodedPicture, Encoder, EncoderConfig, Format, pulldown_flags};
use disccraft_mpeg2venc::{Frame420, Standard};

/// A smooth gradient with a textured 64×64 square moving by (+3, +2) px per frame and light deterministic noise (as
/// in `sequence.rs`).
fn synthetic(w: usize, h: usize, t: usize) -> Frame420 {
    let mut y = vec![0u8; w * h];
    let (sx, sy) = (100 + 3 * t, 80 + 2 * t);
    let mut seed = 0x1234_5678u32 ^ (t as u32).wrapping_mul(0x9E37_79B9);
    for row in 0..h {
        for col in 0..w {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 29) as i32 - 4;
            let mut v = 40 + (col * 120 / w) as i32 + (row * 60 / h) as i32;
            if (sx..sx + 64).contains(&col) && (sy..sy + 64).contains(&row) {
                v = if ((col - sx) / 8 + (row - sy) / 8) % 2 == 0 {
                    210
                } else {
                    60
                };
            }
            y[row * w + col] = (v + noise).clamp(16, 235) as u8;
        }
    }
    let (cw, ch) = (w / 2, h / 2);
    let cb = (0..cw * ch)
        .map(|i| (100 + (i % cw) * 50 / cw) as u8)
        .collect();
    let cr = (0..cw * ch)
        .map(|i| (150 - (i / cw) * 40 / ch) as u8)
        .collect();
    Frame420 {
        width: w as u16,
        height: h as u16,
        y,
        cb,
        cr,
    }
}

/// A textured scene panning by (2.5, 1.25) px a frame, its grain moving with it: closer to filmed footage than
/// `synthetic`, whose noise is new in every frame.
fn panning(w: usize, h: usize, t: usize) -> Frame420 {
    let grain = |x: i64, y: i64| {
        let mut seed = (x as u32).wrapping_mul(0x9E37_79B9) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
        seed ^= seed >> 15;
        seed = seed.wrapping_mul(0x2C1B_3C6D);
        f64::from((seed >> 28) as u8) - 7.5
    };
    let mut y = vec![0u8; w * h];
    for row in 0..h {
        for col in 0..w {
            let fx = col as f64 + 2.5 * t as f64;
            let fy = row as f64 + 1.25 * t as f64;
            let v = 128.0
                + 50.0 * (fx / 13.0).sin() * (fy / 17.0).cos()
                + 30.0 * ((fx + fy) / 7.0).sin()
                + grain(fx.floor() as i64, fy.floor() as i64);
            y[row * w + col] = v.round().clamp(16.0, 235.0) as u8;
        }
    }
    let (cw, ch) = (w / 2, h / 2);
    let cb = (0..cw * ch)
        .map(|i| (110 + ((i % cw + t) * 40 / cw) % 40) as u8)
        .collect();
    let cr = (0..cw * ch)
        .map(|i| (140 - ((i / cw + t) * 30 / ch) % 30) as u8)
        .collect();
    Frame420 {
        width: w as u16,
        height: h as u16,
        y,
        cb,
        cr,
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

/// Codes `sources` with `b_frames` B pictures between references, a GOP every 15 frames, at `bitrate`: the
/// elementary stream and the pictures in decoding order.
fn encode(b_frames: u8, sources: &[Frame420], bitrate: u32) -> (Vec<u8>, Vec<CodedPicture>) {
    let mut encoder = Encoder::new(EncoderConfig {
        standard: Standard::Ntsc,
        gop_size: 15,
        bitrate,
        max_bitrate: 9_800_000,
        wide: false,
        format: Format::Sd,
        pulldown: false,
    })
    .unwrap();
    encoder.set_b_frames(b_frames);
    let mut pictures = Vec::new();
    for (index, frame) in sources.iter().enumerate() {
        pictures.extend(encoder.push_frame(frame, index % 15 == 0).unwrap());
    }
    pictures.extend(encoder.flush().unwrap());
    let mut es: Vec<u8> = pictures
        .iter()
        .flat_map(|picture| picture.data.clone())
        .collect();
    es.extend_from_slice(&encoder.finish());
    (es, pictures)
}

/// Decodes `es` with craft-video's decoder: the pictures in display order (no decoder errors).
fn decode(es: &[u8]) -> Vec<Picture> {
    let mut decoder = Decoder::new();
    let mut pictures = Vec::new();
    for (index, range) in craft_video::mpeg2::access_units(es).into_iter().enumerate() {
        pictures.extend(decoder.decode(&es[range], index as i64).unwrap());
    }
    pictures.extend(decoder.flush());
    assert_eq!(
        decoder.errors(),
        0,
        "decoder errors: {:?}",
        decoder.last_error()
    );
    pictures
}

#[test]
fn b_pictures_decode_in_display_order() {
    let sources: Vec<Frame420> = (0..45).map(|t| synthetic(720, 480, t)).collect();
    let (es, pictures) = encode(2, &sources, 5_000_000);
    // Decoding order: each reference picture before the B pictures shown before it; every frame once.
    let order: Vec<u64> = pictures.iter().map(|picture| picture.display).collect();
    assert_eq!(&order[..7], &[0, 3, 1, 2, 6, 4, 5]);
    let mut shown = order.clone();
    shown.sort_unstable();
    assert_eq!(shown, (0..45).collect::<Vec<u64>>());
    // Closed GOPs at frames 0, 15 and 30: the B pictures of a GOP refer to none of the next, so the frames before
    // a GOP's I picture end with a P picture (frame 14 here: 12 B 13 B 14 P would need frame 15).
    let starts: Vec<u64> = pictures
        .iter()
        .filter(|picture| picture.gop_start)
        .map(|picture| picture.display)
        .collect();
    assert_eq!(starts, [0, 15, 30]);
    assert!(pictures.iter().any(|picture| picture.stats.bidirectional));
    let before_15 = pictures
        .iter()
        .find(|picture| picture.display == 14)
        .unwrap();
    assert!(before_15.reference && !before_15.stats.bidirectional);

    let decoded = decode(&es);
    assert_eq!(decoded.len(), sources.len());
    for (index, (picture, source)) in decoded.iter().zip(&sources).enumerate() {
        let quality = psnr(&picture.y, &source.y);
        assert!(quality > 30.0, "frame {index}: PSNR {quality:.1} dB");
    }
}

#[test]
fn b_pictures_look_better_at_the_same_rate() {
    let sources: Vec<Frame420> = (0..60).map(|t| panning(720, 480, t)).collect();
    let measure = |b_frames: u8| {
        let (es, _) = encode(b_frames, &sources, 3_000_000);
        let decoded = decode(&es);
        let mean = decoded
            .iter()
            .zip(&sources)
            .map(|(picture, source)| psnr(&picture.y, &source.y))
            .sum::<f64>()
            / sources.len() as f64;
        (es.len(), mean)
    };
    let (bytes_ip, psnr_ip) = measure(0);
    let (bytes_b, psnr_b) = measure(2);
    eprintln!("I/P: {bytes_ip} bytes, {psnr_ip:.2} dB; with B: {bytes_b} bytes, {psnr_b:.2} dB");
    // The same rate (within 15 %) and a better picture.
    assert!(
        (bytes_b as f64) < bytes_ip as f64 * 1.15,
        "{bytes_b} vs {bytes_ip}"
    );
    assert!(psnr_b > psnr_ip, "{psnr_b:.2} vs {psnr_ip:.2} dB");
}

/// The (top_field_first, repeat_first_field) of a picture's coding extension.
fn shown_fields(picture: &[u8]) -> Option<(bool, bool)> {
    let at = picture
        .windows(5)
        .position(|bytes| bytes[..4] == [0, 0, 1, 0xB5] && bytes[4] >> 4 == 8)?;
    let flags = *picture.get(at + 7)?;
    Some((flags >> 7 == 1, (flags >> 1) & 1 == 1))
}

/// M12.34: film coded with 3:2 pulldown and B pictures: each picture repeats a field by its place in display order
/// (the cadence of I and P pictures alone), GOPs of at most 14 pictures, and it decodes without errors.
#[test]
fn b_pictures_with_3_2_pulldown() {
    let sources: Vec<Frame420> = (0..40).map(|t| synthetic(720, 480, t)).collect();
    let mut encoder = Encoder::new(EncoderConfig {
        standard: Standard::Ntsc,
        gop_size: 12,
        bitrate: 4_000_000,
        max_bitrate: 9_800_000,
        wide: false,
        format: Format::Sd,
        pulldown: true,
    })
    .unwrap();
    encoder.set_b_frames(2);
    let mut pictures = Vec::new();
    for (index, frame) in sources.iter().enumerate() {
        pictures.extend(encoder.push_frame(frame, index % 12 == 0).unwrap());
    }
    pictures.extend(encoder.flush().unwrap());
    assert!(pictures.iter().any(|picture| picture.stats.bidirectional));
    for picture in &pictures {
        assert_eq!(
            shown_fields(&picture.data),
            Some(pulldown_flags(picture.display)),
            "picture {}",
            picture.display
        );
    }
    let mut es: Vec<u8> = pictures
        .iter()
        .flat_map(|picture| picture.data.clone())
        .collect();
    es.extend_from_slice(&encoder.finish());
    let decoded = decode(&es);
    assert_eq!(decoded.len(), sources.len());
    for (index, (picture, source)) in decoded.iter().zip(&sources).enumerate() {
        let quality = psnr(&picture.y, &source.y);
        assert!(quality > 30.0, "frame {index}: PSNR {quality:.1} dB");
    }
}
