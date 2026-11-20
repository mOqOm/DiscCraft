//! I/P sequences from the encoder must decode without errors in craft-video's MPEG-2 decoder and match the source closely.

use craft_video::mpeg2::Decoder;
use disccraft_mpeg2venc::encoder::{Encoder, EncoderConfig};
use disccraft_mpeg2venc::{Frame420, Standard};

/// A smooth gradient with a textured 64×64 square moving by (+3, +2) px per frame and light deterministic noise.
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

fn run(standard: Standard, w: usize, h: usize, frames: usize, gop: u16, check_sizes: bool) {
    let mut enc = Encoder::new(EncoderConfig {
        standard,
        gop_size: gop,
        bitrate: 6_000_000,
        max_bitrate: 9_800_000,
        wide: false,
        format: disccraft_mpeg2venc::encoder::Format::Sd,
        pulldown: false,
    })
    .unwrap();
    let sources: Vec<Frame420> = (0..frames).map(|t| synthetic(w, h, t)).collect();
    let mut es = Vec::new();
    let mut sizes = Vec::new();
    for f in &sources {
        let au = enc.encode_frame(f).unwrap();
        sizes.push(au.len());
        es.extend_from_slice(&au);
    }
    let last_recon = enc.reconstruction().unwrap().y.clone();
    es.extend_from_slice(&enc.finish());
    std::fs::create_dir_all(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/fixtures"
    ))
    .unwrap();
    std::fs::write(
        format!(
            "{}/../../target/fixtures/ip_{w}x{h}.m2v",
            env!("CARGO_MANIFEST_DIR")
        ),
        &es,
    )
    .unwrap();

    let mut dec = Decoder::new();
    let mut pics = Vec::new();
    for (i, range) in craft_video::mpeg2::access_units(&es)
        .into_iter()
        .enumerate()
    {
        pics.extend(dec.decode(&es[range], i as i64).unwrap());
    }
    pics.extend(dec.flush());
    assert_eq!(dec.errors(), 0, "decoder errors: {:?}", dec.last_error());
    assert_eq!(pics.len(), frames);
    for (i, (p, s)) in pics.iter().zip(&sources).enumerate() {
        assert_eq!((p.width as usize, p.height as usize), (w, h));
        let q = psnr(&p.y, &s.y);
        assert!(q > 30.0, "frame {i}: PSNR {q:.1} dB");
    }
    let last = pics.last().unwrap();
    let mad = last
        .y
        .iter()
        .zip(&last_recon)
        .map(|(&a, &b)| (i32::from(a) - i32::from(b)).abs())
        .sum::<i32>() as f64
        / last_recon.len() as f64;
    assert!(mad < 0.25, "decoder vs encoder reconstruction drift {mad}");
    let gop = usize::from(gop);
    let i_avg =
        sizes.iter().step_by(gop).sum::<usize>() as f64 / sizes.iter().step_by(gop).count() as f64;
    let p: Vec<usize> = sizes
        .iter()
        .enumerate()
        .filter(|(i, _)| i % gop != 0)
        .map(|(_, s)| *s)
        .collect();
    let p_avg = p.iter().sum::<usize>() as f64 / p.len().max(1) as f64;
    // Only meaningful once rate control has seen a few GOPs.
    assert!(
        !check_sizes || p_avg < i_avg,
        "P frames ({p_avg:.0} B) should be smaller than I frames ({i_avg:.0} B)"
    );
}

#[test]
fn ntsc_ip_sequence_decodes() {
    run(Standard::Ntsc, 720, 480, 45, 15, true);
}

#[test]
fn pal_ip_sequence_decodes() {
    run(Standard::Pal, 720, 576, 12, 12, false);
}

/// The GOP header must announce a closed GOP without a broken link (players rely on it), with the marker bit inside time_code.
#[test]
fn gop_header_is_closed() {
    let mut enc = Encoder::new(EncoderConfig {
        standard: Standard::Ntsc,
        gop_size: 15,
        bitrate: 6_000_000,
        max_bitrate: 9_800_000,
        wide: false,
        format: disccraft_mpeg2venc::encoder::Format::Sd,
        pulldown: false,
    })
    .unwrap();
    let au = enc.encode_frame(&synthetic(720, 480, 0)).unwrap();
    let pos = au.windows(4).position(|w| w == [0, 0, 1, 0xB8]).unwrap();
    let b = &au[pos + 4..pos + 8];
    // bits: drop(1) hours(5) minutes(6) | marker(1) at bit 12 | seconds(6) pictures(6) | closed_gop at bit 25, broken_link at bit 26
    let bits = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    assert_eq!((bits >> (31 - 12)) & 1, 1, "marker bit");
    assert_eq!((bits >> (31 - 25)) & 1, 1, "closed_gop");
    assert_eq!((bits >> (31 - 26)) & 1, 0, "broken_link");
}

/// M14.1: a GOP starts at the frame the caller asks for (a chapter or a layer break, where a DVD cell must begin
/// exactly), the GOPs after it count from there, and no GOP grows past what a DVD allows (18 frames NTSC) even when
/// the caller asks for none.
#[test]
fn a_gop_starts_where_asked() {
    let config = EncoderConfig {
        standard: Standard::Ntsc,
        gop_size: 15,
        bitrate: 6_000_000,
        max_bitrate: 9_800_000,
        wide: false,
        format: disccraft_mpeg2venc::encoder::Format::Sd,
        pulldown: false,
    };
    let starts_of = |forced: &[usize], frames: usize, regular: bool| -> Vec<usize> {
        let mut enc = Encoder::new(config.clone()).unwrap();
        (0..frames)
            .filter(|&index| {
                let frame = synthetic(720, 480, index);
                let au = if regular {
                    enc.encode_frame(&frame).unwrap()
                } else {
                    enc.encode_frame_gop(&frame, forced.contains(&index))
                        .unwrap()
                };
                au.windows(4).any(|w| w == [0, 0, 1, 0xB8])
            })
            .collect()
    };
    // The regular cadence, then a forced start at 7 that the next GOPs count from.
    assert_eq!(starts_of(&[], 40, true), vec![0, 15, 30]);
    assert_eq!(starts_of(&[7, 22], 40, false), vec![0, 7, 22]);
    // Asked for nothing: still a GOP every 18 frames at the most.
    assert_eq!(starts_of(&[], 40, false), vec![0, 18, 36]);
}

/// An HD sequence (1920 x 1080 as 59.94i or 50i, MP@HL): the frames are given at the coded 1920 x 1088, decode
/// without errors to the lines shown, and say what they are in their headers.
fn run_hd(standard: Standard, frame_rate_code: u8) {
    use disccraft_mpeg2venc::encoder::Format;
    let (w, h) = (1920usize, 1088usize);
    assert_eq!(Format::Hd1080.coded_size(standard), (1920, 1088));
    let mut enc = Encoder::new(EncoderConfig {
        standard,
        gop_size: 15,
        bitrate: 25_000_000,
        max_bitrate: 35_000_000,
        wide: true,
        format: Format::Hd1080,
        pulldown: false,
    })
    .unwrap();
    let frames = 4;
    // The square moves 24 pixels a frame: further than an SD sequence's vectors reach.
    let sources: Vec<Frame420> = (0..frames).map(|t| synthetic(w, h, t * 8)).collect();
    let mut es = Vec::new();
    for f in &sources {
        es.extend_from_slice(&enc.encode_frame(f).unwrap());
    }
    es.extend_from_slice(&enc.finish());
    std::fs::create_dir_all(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/fixtures"
    ))
    .unwrap();
    std::fs::write(
        format!(
            "{}/../../target/fixtures/ip_hd_{frame_rate_code}.m2v",
            env!("CARGO_MANIFEST_DIR")
        ),
        &es,
    )
    .unwrap();

    // Sequence header: 1920 x 1080, 16:9, the frame rate, a 597 x 16,384-bit VBV buffer.
    assert_eq!(&es[..4], &[0, 0, 1, 0xB3]);
    assert_eq!(&es[4..7], &[0x78, 0x04, 0x38]);
    assert_eq!(es[7], 0x30 | frame_rate_code);
    let vbv = (u16::from(es[10] & 0x1F) << 5) | u16::from(es[11] >> 3);
    assert_eq!(vbv, 597);
    // Sequence extension: MP@HL, not a progressive sequence, 4:2:0, the marker bit; then the display extension
    // with BT.709.
    assert_eq!(&es[12..16], &[0, 0, 1, 0xB5]);
    assert_eq!(&es[16..22], &[0x14, 0x42, 0x00, 0x01, 0x00, 0x00]);
    assert_eq!(&es[22..26], &[0, 0, 1, 0xB5]);
    assert_eq!(&es[26..30], &[0x21, 0x01, 0x01, 0x01]);

    let mut dec = Decoder::new();
    let mut pics = Vec::new();
    for (i, range) in craft_video::mpeg2::access_units(&es)
        .into_iter()
        .enumerate()
    {
        pics.extend(dec.decode(&es[range], i as i64).unwrap());
    }
    pics.extend(dec.flush());
    assert_eq!(dec.errors(), 0, "decoder errors: {:?}", dec.last_error());
    assert_eq!(pics.len(), frames);
    for (i, (p, s)) in pics.iter().zip(&sources).enumerate() {
        assert_eq!(p.width as usize, w);
        let shown = w * (p.height as usize).min(1080);
        let q = psnr(&p.y[..shown], &s.y[..shown]);
        assert!(q > 30.0, "frame {i}: PSNR {q:.1} dB");
    }
}

/// M6.393: an HDV-sized sequence (1440 x 1080, MP@H-14): the frames given at the coded 1440 x 1088 decode without
/// errors, and the headers say 1440 x 1080, 16:9, a 448 x 16,384-bit VBV buffer and High-1440.
#[test]
fn hd_1440_sequence_decodes() {
    use disccraft_mpeg2venc::encoder::Format;
    let (w, h) = (1440usize, 1088usize);
    assert_eq!(Format::Hd1440.coded_size(Standard::Ntsc), (1440, 1088));
    let mut enc = Encoder::new(EncoderConfig {
        standard: Standard::Ntsc,
        gop_size: 15,
        bitrate: 18_000_000,
        max_bitrate: 25_000_000,
        wide: true,
        format: Format::Hd1440,
        pulldown: false,
    })
    .unwrap();
    let frames = 4;
    let sources: Vec<Frame420> = (0..frames).map(|t| synthetic(w, h, t * 8)).collect();
    let mut es = Vec::new();
    for f in &sources {
        es.extend_from_slice(&enc.encode_frame(f).unwrap());
    }
    es.extend_from_slice(&enc.finish());
    assert_eq!(&es[..4], &[0, 0, 1, 0xB3]);
    assert_eq!(&es[4..7], &[0x5A, 0x04, 0x38]);
    assert_eq!(es[7], 0x30 | 4);
    let vbv = (u16::from(es[10] & 0x1F) << 5) | u16::from(es[11] >> 3);
    assert_eq!(vbv, 448);
    assert_eq!(&es[16..18], &[0x14, 0x62]);
    assert_eq!(&es[22..26], &[0, 0, 1, 0xB5]);
    let mut dec = Decoder::new();
    let mut pics = Vec::new();
    for (i, range) in craft_video::mpeg2::access_units(&es)
        .into_iter()
        .enumerate()
    {
        pics.extend(dec.decode(&es[range], i as i64).unwrap());
    }
    pics.extend(dec.flush());
    assert_eq!(dec.errors(), 0, "decoder errors: {:?}", dec.last_error());
    assert_eq!(pics.len(), frames);
    for (i, (p, s)) in pics.iter().zip(&sources).enumerate() {
        assert_eq!(p.width as usize, w);
        let shown = w * (p.height as usize).min(1080);
        let q = psnr(&p.y[..shown], &s.y[..shown]);
        assert!(q > 30.0, "frame {i}: PSNR {q:.1} dB");
    }
}

#[test]
fn hd_1080i_59_94_sequence_decodes() {
    run_hd(Standard::Ntsc, 4);
}

#[test]
fn hd_1080i_50_sequence_decodes() {
    run_hd(Standard::Pal, 3);
}

/// An SD sequence's extension is the 48 bits of 13818-2 6.2.2.3 (MP@ML, the marker bit set).
#[test]
fn sd_sequence_extension_is_well_formed() {
    let mut enc = Encoder::new(EncoderConfig {
        standard: Standard::Ntsc,
        gop_size: 15,
        bitrate: 6_000_000,
        max_bitrate: 9_800_000,
        wide: false,
        format: disccraft_mpeg2venc::encoder::Format::Sd,
        pulldown: false,
    })
    .unwrap();
    let es = enc.encode_frame(&synthetic(720, 480, 0)).unwrap();
    assert_eq!(&es[12..16], &[0, 0, 1, 0xB5]);
    assert_eq!(&es[16..22], &[0x14, 0x82, 0x00, 0x01, 0x00, 0x00]);
    // The GOP header follows at once.
    assert_eq!(&es[22..26], &[0, 0, 1, 0xB8]);
}

/// An HD still: one I picture of a sequence, ended by a sequence end code, that fits the VBV buffer.
#[test]
fn hd_still_decodes() {
    use disccraft_mpeg2venc::encoder::Format;
    let enc = Encoder::new(EncoderConfig {
        standard: Standard::Ntsc,
        gop_size: 15,
        bitrate: 25_000_000,
        max_bitrate: 35_000_000,
        wide: true,
        format: Format::Hd1080,
        pulldown: false,
    })
    .unwrap();
    let source = synthetic(1920, 1088, 0);
    let es = enc.encode_still(&source, 2).unwrap();
    assert!(es.ends_with(&[0, 0, 1, 0xB7]));
    assert!((es.len() as u64) * 8 <= Format::Hd1080.vbv_bits());
    let mut dec = Decoder::new();
    let mut pics = Vec::new();
    for (i, range) in craft_video::mpeg2::access_units(&es)
        .into_iter()
        .enumerate()
    {
        pics.extend(dec.decode(&es[range], i as i64).unwrap());
    }
    pics.extend(dec.flush());
    assert_eq!(dec.errors(), 0, "decoder errors: {:?}", dec.last_error());
    assert_eq!(pics.len(), 1);
    let shown = 1920 * 1080;
    assert!(psnr(&pics[0].y[..shown], &source.y[..shown]) > 35.0);
}

/// M6.314: film (Encore's NTSC DV 23.976 presets). Each picture says how 3:2 pulldown shows it (top_field_first,
/// repeat_first_field: T B T | B T | B T B | T B) and that it is progressive; the sequence stays 29.97 (frame_rate_code
/// 4) as a DVD needs; no GOP holds more than 14 pictures (35 fields of the 36 a DVD allows); a GOP's time code counts
/// the frames shown (two and a half a picture); and the stream decodes, every picture once. PAL or HD film is refused.
#[test]
fn film_is_shown_by_pulldown() {
    use disccraft_mpeg2venc::encoder::{Format, MAX_FILM_GOP_FRAMES, pulldown_flags};
    let config = EncoderConfig {
        standard: Standard::Ntsc,
        gop_size: 12,
        bitrate: 6_000_000,
        max_bitrate: 9_800_000,
        wide: false,
        format: Format::Sd,
        pulldown: true,
    };
    let mut enc = Encoder::new(config.clone()).unwrap();
    let frames = 40usize;
    let mut es = Vec::new();
    let mut shown = Vec::new();
    let mut gops = Vec::new();
    for index in 0..frames {
        let au = enc
            .encode_frame_gop(&synthetic(720, 480, index), false)
            .unwrap();
        // The picture coding extension (identifier 8): top_field_first is bit 24 of it, repeat_first_field bit 30,
        // progressive_frame bit 32.
        let at = au
            .windows(5)
            .position(|w| w[..4] == [0, 0, 1, 0xB5] && w[4] >> 4 == 8)
            .unwrap();
        let ext = &au[at + 4..at + 9];
        let bits = u32::from_be_bytes([ext[0], ext[1], ext[2], ext[3]]);
        shown.push(((bits >> 7) & 1 == 1, (bits >> 1) & 1 == 1, ext[4] >> 7 == 1));
        if let Some(at) = au.windows(4).position(|w| w == [0, 0, 1, 0xB8]) {
            let b = &au[at + 4..at + 8];
            let bits = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
            gops.push((index, (bits >> 13) & 0x3F, (bits >> 7) & 0x3F));
        }
        if index == 0 {
            let at = au.windows(4).position(|w| w == [0, 0, 1, 0xB3]).unwrap();
            assert_eq!(au[at + 7] & 0x0F, 4, "frame_rate_code 29.97");
        }
        es.extend_from_slice(&au);
    }
    es.extend_from_slice(&enc.finish());
    let expected: Vec<(bool, bool, bool)> = (0..frames as u64)
        .map(|index| {
            let (top, repeat) = pulldown_flags(index);
            (top, repeat, true)
        })
        .collect();
    assert_eq!(shown, expected);
    assert_eq!(
        shown
            .iter()
            .take(4)
            .map(|(top, repeat, _)| (*top, *repeat))
            .collect::<Vec<_>>(),
        vec![(true, true), (false, false), (false, true), (true, false)]
    );
    // GOPs of 14 pictures at the most; their time codes (seconds, pictures at 30) count the frames shown: 35 fields
    // (17 frames) before picture 14, 70 fields (35 frames: 1 s 5) before picture 28.
    assert_eq!(MAX_FILM_GOP_FRAMES, 14);
    assert_eq!(gops, vec![(0, 0, 0), (14, 0, 17), (28, 1, 5)]);

    let mut dec = Decoder::new();
    let mut pics = Vec::new();
    for (i, range) in craft_video::mpeg2::access_units(&es)
        .into_iter()
        .enumerate()
    {
        pics.extend(dec.decode(&es[range], i as i64).unwrap());
    }
    pics.extend(dec.flush());
    assert_eq!(dec.errors(), 0, "decoder errors: {:?}", dec.last_error());
    assert_eq!(pics.len(), frames);

    for (standard, format) in [
        (Standard::Pal, Format::Sd),
        (Standard::Ntsc, Format::Hd1080),
    ] {
        let refused = Encoder::new(EncoderConfig {
            standard,
            format,
            max_bitrate: 9_800_000,
            ..config.clone()
        });
        assert!(refused.is_err(), "{standard:?} {format:?}");
    }
}

/// Interlaced material: a checkered picture that moves 6 px sideways a field, its top field taken half a frame
/// before its bottom one (combing on every vertical edge).
fn interlaced(w: usize, h: usize, t: usize) -> Frame420 {
    let picture = |field_time: usize| -> Vec<u8> {
        let shift = 6 * field_time;
        (0..w * h)
            .map(|i| {
                let (row, col) = (i / w, i % w);
                if ((col + shift) / 12 + row / 12).is_multiple_of(2) {
                    190
                } else {
                    70
                }
            })
            .collect()
    };
    let (top, bottom) = (picture(2 * t), picture(2 * t + 1));
    let mut y = top;
    for row in (1..h).step_by(2) {
        y[row * w..(row + 1) * w].copy_from_slice(&bottom[row * w..(row + 1) * w]);
    }
    let base = synthetic(w, h, t);
    Frame420 { y, ..base }
}

/// Encodes `frames` of `interlaced` at 3 Mbit/s, as interlaced pictures or not; the decoded pictures, their mean
/// luma PSNR and the last picture's distance from the encoder's reconstruction.
fn interlaced_run(as_fields: bool, frames: usize) -> (f64, f64) {
    let (w, h) = (720, 480);
    let mut enc = Encoder::new(EncoderConfig {
        standard: Standard::Ntsc,
        gop_size: 15,
        bitrate: 3_000_000,
        max_bitrate: 6_000_000,
        wide: false,
        format: disccraft_mpeg2venc::encoder::Format::Sd,
        pulldown: false,
    })
    .unwrap();
    if as_fields {
        enc.set_interlaced(true);
    }
    let sources: Vec<Frame420> = (0..frames).map(|t| interlaced(w, h, t)).collect();
    let mut es = Vec::new();
    for f in &sources {
        es.extend_from_slice(&enc.encode_frame(f).unwrap());
    }
    let last_recon = enc.reconstruction().unwrap().y.clone();
    es.extend_from_slice(&enc.finish());
    let mut dec = Decoder::new();
    let mut pics = Vec::new();
    for (i, range) in craft_video::mpeg2::access_units(&es)
        .into_iter()
        .enumerate()
    {
        pics.extend(dec.decode(&es[range], i as i64).unwrap());
    }
    pics.extend(dec.flush());
    assert_eq!(dec.errors(), 0, "decoder errors: {:?}", dec.last_error());
    assert_eq!(pics.len(), frames);
    let mean = pics
        .iter()
        .zip(&sources)
        .map(|(p, s)| psnr(&p.y, &s.y))
        .sum::<f64>()
        / frames as f64;
    let last = pics.last().unwrap();
    let mad = last
        .y
        .iter()
        .zip(&last_recon)
        .map(|(&a, &b)| (i32::from(a) - i32::from(b)).abs())
        .sum::<i32>() as f64
        / last_recon.len() as f64;
    (mean, mad)
}

/// M6.371: interlaced pictures (frame_pred_frame_dct 0, frame_motion_type and a dct_type a macroblock) decode
/// without errors, as the encoder reconstructed them. M6.394: on this material they now code better than
/// progressive frames (about 33 dB against 30 at 3 Mbit/s; 25 before, when the motion search fell into vectors that
/// take one field for the other).
#[test]
fn interlaced_pictures_decode_as_reconstructed() {
    let (fields, drift) = interlaced_run(true, 30);
    assert!(
        drift < 0.25,
        "decoder vs encoder reconstruction drift {drift}"
    );
    let (frames, _) = interlaced_run(false, 30);
    assert!(
        fields > frames + 1.5,
        "{fields:.2} dB, frames {frames:.2} dB"
    );
}

/// M12.26: pictures marked interlaced with frame coding (`set_field_order`): each picture coding extension says
/// the field order given (here the bottom field first), frame_pred_frame_dct 1, chroma_420_type and
/// progressive_frame 0, and the pictures decode without errors as the encoder reconstructed them.
#[test]
fn pictures_marked_interlaced_keep_frame_coding() {
    let (w, h) = (720, 480);
    let mut enc = Encoder::new(EncoderConfig {
        standard: Standard::Ntsc,
        gop_size: 15,
        bitrate: 4_000_000,
        max_bitrate: 8_000_000,
        wide: false,
        format: disccraft_mpeg2venc::encoder::Format::Sd,
        pulldown: false,
    })
    .unwrap();
    enc.set_field_order(false);
    let mut es = Vec::new();
    for t in 0..4 {
        es.extend_from_slice(&enc.encode_frame(&interlaced(w, h, t)).unwrap());
    }
    let last_recon = enc.reconstruction().unwrap().y.clone();
    es.extend_from_slice(&enc.finish());
    let extensions: Vec<usize> = es
        .windows(5)
        .enumerate()
        .filter(|(_, bytes)| bytes[..4] == [0, 0, 1, 0xB5] && bytes[4] >> 4 == 8)
        .map(|(at, _)| at + 4)
        .collect();
    assert_eq!(extensions.len(), 4);
    for at in extensions {
        let flags = es[at + 3];
        assert_eq!(flags >> 7, 0, "top_field_first");
        assert_eq!((flags >> 6) & 1, 1, "frame_pred_frame_dct");
        assert_eq!(flags & 1, 0, "chroma_420_type");
        assert_eq!(es[at + 4] >> 7, 0, "progressive_frame");
    }
    let mut dec = Decoder::new();
    let mut pics = Vec::new();
    for (i, range) in craft_video::mpeg2::access_units(&es)
        .into_iter()
        .enumerate()
    {
        pics.extend(dec.decode(&es[range], i as i64).unwrap());
    }
    pics.extend(dec.flush());
    assert_eq!(dec.errors(), 0, "decoder errors: {:?}", dec.last_error());
    assert_eq!(pics.len(), 4);
    assert!(pics.last().unwrap().y == last_recon);
}
