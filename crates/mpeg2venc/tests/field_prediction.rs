//! Field prediction (M12.36): interlaced material whose two fields were taken at different moments — a 59.94p pan
//! woven into 29.97 frames — coded as interlaced pictures predicts each field of a macroblock from either field of a
//! reference. It decodes without errors in craft-video's MPEG-2 decoder as the encoder reconstructed it, keeps the
//! field tools on its P pictures, and looks better than the same material coded as frames at the same rate.

use craft_video::mpeg2::{Decoder, Picture};
use disccraft_mpeg2venc::encoder::{CodedPicture, Encoder, EncoderConfig, Format};
use disccraft_mpeg2venc::{Frame420, Standard};

/// A textured scene panning by (2, 1.5) px a field: field time `field` (59.94 a second).
fn scene(w: usize, h: usize, field: usize) -> Vec<u8> {
    let grain = |x: i64, y: i64| {
        let mut seed = (x as u32).wrapping_mul(0x9E37_79B9) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
        seed ^= seed >> 15;
        seed = seed.wrapping_mul(0x2C1B_3C6D);
        f64::from((seed >> 28) as u8) - 7.5
    };
    let mut y = vec![0u8; w * h];
    for row in 0..h {
        for col in 0..w {
            let fx = col as f64 + 2.0 * field as f64;
            let fy = row as f64 + 1.5 * field as f64;
            let v = 128.0
                + 50.0 * (fx / 13.0).sin() * (fy / 17.0).cos()
                + 30.0 * ((fx + fy) / 7.0).sin()
                + grain(fx.floor() as i64, fy.floor() as i64);
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

/// Codes `sources` with two B pictures between references and a GOP every 15 frames at 3 Mbit/s, as interlaced
/// pictures with the field tools or as frames: the pictures in decoding order, the elementary stream, and the
/// last coded picture's reconstruction.
fn encode(sources: &[Frame420], interlaced: bool) -> (Vec<CodedPicture>, Vec<u8>, Vec<u8>) {
    let mut encoder = Encoder::new(EncoderConfig {
        standard: Standard::Ntsc,
        gop_size: 15,
        bitrate: 3_000_000,
        max_bitrate: 9_800_000,
        wide: false,
        format: Format::Sd,
        pulldown: false,
    })
    .unwrap();
    encoder.set_b_frames(2);
    if interlaced {
        encoder.set_interlaced(true);
    }
    let mut pictures = Vec::new();
    for (index, frame) in sources.iter().enumerate() {
        pictures.extend(encoder.push_frame(frame, index % 15 == 0).unwrap());
    }
    pictures.extend(encoder.flush().unwrap());
    let last = encoder.reconstruction().unwrap().y.clone();
    let mut es: Vec<u8> = pictures
        .iter()
        .flat_map(|picture| picture.data.clone())
        .collect();
    es.extend_from_slice(&encoder.finish());
    (pictures, es, last)
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

/// Whether a picture's coding extension has frame_pred_frame_dct 0: its macroblocks carry the field tools.
fn has_field_tools(picture: &[u8]) -> bool {
    picture
        .windows(5)
        .position(|bytes| bytes[..4] == [0, 0, 1, 0xB5] && bytes[4] >> 4 == 8)
        .and_then(|at| picture.get(at + 7))
        .is_some_and(|flags| (flags >> 6) & 1 == 0)
}

fn mean_psnr(decoded: &[Picture], sources: &[Frame420]) -> f64 {
    decoded
        .iter()
        .zip(sources)
        .map(|(picture, source)| psnr(&picture.y, &source.y))
        .sum::<f64>()
        / sources.len() as f64
}

#[test]
fn fields_taken_at_different_moments_are_predicted_as_fields() {
    let sources: Vec<Frame420> = (0..45).map(|t| woven(720, 480, t)).collect();
    let (pictures, es, last) = encode(&sources, true);
    let decoded = decode(&es);
    assert_eq!(decoded.len(), sources.len());

    // The P pictures keep the field tools: enough of their macroblocks take them. The B pictures measure them on
    // the first of each GOP.
    let predicted: Vec<&CodedPicture> = pictures
        .iter()
        .filter(|picture| picture.reference && !picture.stats.intra)
        .collect();
    assert!(
        predicted
            .iter()
            .all(|picture| has_field_tools(&picture.data)),
        "a P picture without the field tools"
    );
    let first_b = pictures
        .iter()
        .find(|picture| picture.stats.bidirectional)
        .unwrap();
    assert!(has_field_tools(&first_b.data));

    // The decoder reconstructs the last coded picture (a B picture) as the encoder did.
    let last_coded = pictures.last().unwrap();
    assert!(last_coded.stats.bidirectional);
    let shown = &decoded[usize::try_from(last_coded.display).unwrap()];
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

    // Better than the same material coded as frames, at the same rate.
    let (_, frame_es, _) = encode(&sources, false);
    let (fields, frames) = (
        mean_psnr(&decoded, &sources),
        mean_psnr(&decode(&frame_es), &sources),
    );
    eprintln!(
        "fields {fields:.2} dB ({} bytes), frames {frames:.2} dB ({} bytes)",
        es.len(),
        frame_es.len()
    );
    assert!(
        (es.len() as f64) < frame_es.len() as f64 * 1.05,
        "{} vs {} bytes",
        es.len(),
        frame_es.len()
    );
    assert!(fields > frames + 0.3, "{fields:.2} vs {frames:.2} dB");
}
