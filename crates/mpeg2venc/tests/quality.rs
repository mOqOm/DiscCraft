//! Encore's quality (M6.326): a lower one searches a smaller range for motion (and below 3 to whole pixels
//! only), so a picture that moves further than that range is predicted worse and, at the same bit rate, coded
//! with a coarser quantiser; the best is the encoder as it always was.

use disccraft_mpeg2venc::encoder::{BEST_QUALITY, Encoder, EncoderConfig, PictureStats};
use disccraft_mpeg2venc::{Frame420, Standard};

const WIDTH: usize = 720;
const HEIGHT: usize = 480;

/// A detailed picture (a fixed pattern of noise over a gradient) moved `t` times 10.5 pixels to the right: further
/// than a low quality searches, by half pixels.
fn picture(t: usize) -> Frame420 {
    let texture = |x: i64, y: i64| -> u8 {
        let mut seed = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)) as u32;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (seed >> 24) as i64 % 41 - 20;
        (100 + (x.rem_euclid(200)) / 3 + noise).clamp(16, 235) as u8
    };
    // In half pixels: 21 a picture.
    let shift = (t * 21) as i64;
    let mut y = vec![0u8; WIDTH * HEIGHT];
    for row in 0..HEIGHT {
        for col in 0..WIDTH {
            let half = col as i64 * 2 - shift;
            let (left, right) = (texture(half.div_euclid(2), row as i64), {
                texture((half + 1).div_euclid(2), row as i64)
            });
            if let Some(pixel) = y.get_mut(row * WIDTH + col) {
                *pixel = ((u16::from(left) + u16::from(right)).div_ceil(2)) as u8;
            }
        }
    }
    let chroma = (WIDTH / 2) * (HEIGHT / 2);
    Frame420 {
        width: WIDTH as u16,
        height: HEIGHT as u16,
        y,
        cb: vec![120; chroma],
        cr: vec![136; chroma],
    }
}

fn config() -> EncoderConfig {
    EncoderConfig {
        standard: Standard::Ntsc,
        gop_size: 15,
        bitrate: 3_000_000,
        max_bitrate: 9_800_000,
        wide: false,
        format: Default::default(),
        pulldown: false,
    }
}

/// Encodes `frames` at `quality` (`None`: left as the encoder starts): the stream and each picture's stats.
fn encode(
    frames: &[Frame420],
    quality: Option<u8>,
) -> Result<(Vec<u8>, Vec<PictureStats>), String> {
    let mut encoder = Encoder::new(config()).map_err(|error| error.to_string())?;
    if let Some(quality) = quality {
        encoder.set_quality(quality);
    }
    let mut stream = Vec::new();
    let mut stats = Vec::new();
    for frame in frames {
        stream.extend(
            encoder
                .encode_frame(frame)
                .map_err(|error| error.to_string())?,
        );
        stats.extend(encoder.last_picture());
    }
    stream.extend(encoder.finish());
    Ok((stream, stats))
}

/// The P pictures' mean quantiser_scale_code.
fn p_code(stats: &[PictureStats]) -> f64 {
    let p: Vec<f64> = stats
        .iter()
        .filter(|picture| !picture.intra)
        .map(|picture| f64::from(picture.code))
        .collect();
    p.iter().sum::<f64>() / p.len().max(1) as f64
}

#[test]
fn a_lower_quality_predicts_worse() -> Result<(), String> {
    let frames: Vec<Frame420> = (0..15).map(picture).collect();
    let (plain, _) = encode(&frames, None)?;
    let (best, high) = encode(&frames, Some(BEST_QUALITY))?;
    let (low_stream, low) = encode(&frames, Some(2))?;
    // The best is the encoder as it was.
    assert_eq!(plain, best);
    // A low quality finds neither the 10.5-pixel move (it searches 6) nor its half pixel: a coarser quantiser.
    assert_ne!(low_stream, best);
    let (high, low) = (p_code(&high), p_code(&low));
    eprintln!("P pictures' quantiser: quality 2 {low:.1}, quality 5 {high:.1}");
    assert!(low > high + 1.0, "low {low} high {high}");
    // Outside 1 to 5 it is clamped: 0 is 1, 9 is 5.
    let (zero, _) = encode(&frames, Some(0))?;
    let (one, _) = encode(&frames, Some(1))?;
    assert_eq!(zero, one);
    let (nine, _) = encode(&frames, Some(9))?;
    assert_eq!(nine, best);
    Ok(())
}
