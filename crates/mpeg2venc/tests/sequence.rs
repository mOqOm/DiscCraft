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
