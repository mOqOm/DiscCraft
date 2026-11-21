//! M12.35: progressive HD sequences (a Blu-ray's 1080p 23.976 / 24, 1440 x 1080p and 720p as MPEG-2): the sequence
//! says its frame rate and that it is progressive, every picture is a progressive frame shown once, a GOP's time
//! code counts the rate's frames, B pictures go between the references, and craft-video's decoder decodes every
//! frame close to the source. A rate the format's level does not allow, or a DVD's format, is refused.

use craft_video::mpeg2::Decoder;
use disccraft_mpeg2venc::encoder::{Encoder, EncoderConfig, Format, FrameRate};
use disccraft_mpeg2venc::{Frame420, Standard};

/// A gradient with a textured square moving by (+6, +3) px per frame.
fn moving(w: usize, h: usize, t: usize) -> Frame420 {
    let (sx, sy) = (100 + 6 * t, 80 + 3 * t);
    let y = (0..w * h)
        .map(|i| {
            let (row, col) = (i / w, i % w);
            if (sx..sx + 96).contains(&col) && (sy..sy + 96).contains(&row) {
                if ((col - sx) / 12 + (row - sy) / 12) % 2 == 0 {
                    200
                } else {
                    60
                }
            } else {
                (40 + col * 120 / w + row * 60 / h) as u8
            }
        })
        .collect();
    let (cw, ch) = (w / 2, h / 2);
    Frame420 {
        width: w as u16,
        height: h as u16,
        y,
        cb: (0..cw * ch)
            .map(|i| (100 + (i % cw) * 50 / cw) as u8)
            .collect(),
        cr: (0..cw * ch)
            .map(|i| (150 - (i / cw) * 40 / ch) as u8)
            .collect(),
    }
}

fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let mse = a
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

fn config(format: Format) -> EncoderConfig {
    EncoderConfig {
        standard: Standard::Ntsc,
        gop_size: 12,
        bitrate: 20_000_000,
        max_bitrate: 30_000_000,
        wide: true,
        format,
        pulldown: false,
    }
}

/// Codes `frames` frames of `format` at `rate` with two B pictures and a GOP every `gop` frames, and checks the
/// headers (`profile_level`, frame_rate_code `code`), every picture's flags, the GOPs' time codes and the decoded
/// pictures.
fn run(format: Format, rate: FrameRate, (profile_level, code): (u8, u8), gop: usize) {
    let (w, h) = format.coded_size(Standard::Ntsc);
    let (w, h) = (usize::from(w), usize::from(h));
    let mut encoder = Encoder::new(config(format)).unwrap();
    encoder.set_progressive(rate).unwrap();
    encoder.set_b_frames(2);
    let frames = 2 * gop + 5;
    let sources: Vec<Frame420> = (0..frames).map(|t| moving(w, h, t)).collect();
    let mut pictures = Vec::new();
    for (index, frame) in sources.iter().enumerate() {
        pictures.extend(encoder.push_frame(frame, index % gop == 0).unwrap());
    }
    pictures.extend(encoder.flush().unwrap());
    assert!(pictures.iter().any(|picture| picture.stats.bidirectional));
    let mut es: Vec<u8> = pictures.iter().flat_map(|p| p.data.clone()).collect();
    es.extend_from_slice(&encoder.finish());
    std::fs::create_dir_all(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/fixtures"
    ))
    .unwrap();
    std::fs::write(
        format!(
            "{}/../../target/fixtures/progressive_{w}_{}.m2v",
            env!("CARGO_MANIFEST_DIR"),
            rate.nominal()
        ),
        &es,
    )
    .unwrap();

    // Sequence header: the frame rate. Sequence extension: the profile and level, progressive_sequence 1, 4:2:0.
    assert_eq!(&es[..4], &[0, 0, 1, 0xB3]);
    assert_eq!(es[7] & 0x0F, code, "frame_rate_code");
    assert_eq!(&es[12..16], &[0, 0, 1, 0xB5]);
    assert_eq!(es[16], 0x10 | (profile_level >> 4));
    assert_eq!(es[17], ((profile_level & 0x0F) << 4) | 0x08 | 0x02);

    // Every picture: top_field_first 0, frame_pred_frame_dct 1, repeat_first_field 0, progressive_frame 1.
    for picture in &pictures {
        let at = picture
            .data
            .windows(5)
            .position(|b| b[..4] == [0, 0, 1, 0xB5] && b[4] >> 4 == 8)
            .unwrap();
        let ext = &picture.data[at + 4..at + 9];
        let bits = u32::from_be_bytes([ext[0], ext[1], ext[2], ext[3]]);
        assert_eq!((bits >> 7) & 1, 0, "top_field_first");
        assert_eq!((bits >> 6) & 1, 1, "frame_pred_frame_dct");
        assert_eq!((bits >> 1) & 1, 0, "repeat_first_field");
        assert_eq!(ext[4] >> 7, 1, "progressive_frame");
    }

    // The GOPs' time codes count the rate's frames: the second GOP starts `gop` frames in.
    let codes: Vec<(u64, u32, u32)> = pictures
        .iter()
        .filter(|picture| picture.gop_start)
        .map(|picture| {
            let at = picture
                .data
                .windows(4)
                .position(|b| b == [0, 0, 1, 0xB8])
                .unwrap();
            let b = &picture.data[at + 4..at + 8];
            let bits = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
            (picture.display, (bits >> 13) & 0x3F, (bits >> 7) & 0x3F)
        })
        .collect();
    let nominal = u64::from(rate.nominal());
    let expected: Vec<(u64, u32, u32)> = (0..frames as u64)
        .step_by(gop)
        .map(|start| (start, (start / nominal) as u32, (start % nominal) as u32))
        .collect();
    assert_eq!(codes, expected);

    let mut decoder = Decoder::new();
    let mut decoded = Vec::new();
    for (index, range) in craft_video::mpeg2::access_units(&es)
        .into_iter()
        .enumerate()
    {
        decoded.extend(decoder.decode(&es[range], index as i64).unwrap());
    }
    decoded.extend(decoder.flush());
    assert_eq!(decoder.errors(), 0, "{:?}", decoder.last_error());
    assert_eq!(decoded.len(), frames);
    let shown = w * (h.min(1080));
    for (index, (picture, source)) in decoded.iter().zip(&sources).enumerate() {
        let quality = psnr(&picture.y[..shown], &source.y[..shown]);
        assert!(quality > 30.0, "frame {index}: {quality:.1} dB");
    }
}

#[test]
fn a_720p_59_94_sequence_decodes() {
    assert_eq!(Format::Hd720.coded_size(Standard::Ntsc), (1280, 720));
    run(Format::Hd720, FrameRate::Fps59_94, (0x44, 7), 30);
}

#[test]
fn a_720p_50_sequence_decodes() {
    run(Format::Hd720, FrameRate::Fps50, (0x44, 6), 24);
}

#[test]
fn a_1080p_23_976_sequence_decodes() {
    run(Format::Hd1080, FrameRate::Fps23_976, (0x44, 1), 12);
}

#[test]
fn a_1440_by_1080p_24_sequence_decodes() {
    run(Format::Hd1440, FrameRate::Fps24, (0x46, 2), 12);
}

#[test]
fn rates_beyond_the_level_and_sd_are_refused() {
    // 1080 lines at 59.94 or 50 frames: more samples a second than High (or High-1440) allows.
    for (format, rate) in [
        (Format::Hd1080, FrameRate::Fps59_94),
        (Format::Hd1080, FrameRate::Fps50),
        (Format::Hd1440, FrameRate::Fps50),
    ] {
        let mut encoder = Encoder::new(config(format)).unwrap();
        assert!(
            encoder.set_progressive(rate).is_err(),
            "{format:?} {rate:?}"
        );
    }
    let mut sd = Encoder::new(EncoderConfig {
        max_bitrate: 9_800_000,
        ..config(Format::Sd)
    })
    .unwrap();
    assert!(sd.set_progressive(FrameRate::Fps23_976).is_err());
}
