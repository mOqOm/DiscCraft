//! VBR 2 pass (M6.300): a second pass shares the sequence's bits by what the first pass measured, so a hard scene
//! gets more of them than one pass gives it, and the average stays near the target.

use disccraft_mpeg2venc::encoder::{Encoder, EncoderConfig, PictureStats};
use disccraft_mpeg2venc::{Frame420, Standard};

const WIDTH: usize = 720;
const HEIGHT: usize = 480;

/// A hard picture (a gradient under fresh noise of ±`amplitude` every picture) for `t` below `hard`, after that an
/// easy one (a still gradient).
fn picture(t: usize, hard: usize, amplitude: i32) -> Frame420 {
    let mut y = vec![0u8; WIDTH * HEIGHT];
    let mut seed = 0x2468_ace0u32 ^ (t as u32).wrapping_mul(0x9E37_79B9);
    for row in 0..HEIGHT {
        for col in 0..WIDTH {
            let base = 60 + (col * 100 / WIDTH) as i32 + (row * 40 / HEIGHT) as i32;
            let noise = if t >= hard {
                0
            } else {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 24) as i32 % (2 * amplitude + 1) - amplitude
            };
            if let Some(pixel) = y.get_mut(row * WIDTH + col) {
                *pixel = (base + noise).clamp(16, 235) as u8;
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

fn config(bitrate: u32) -> EncoderConfig {
    EncoderConfig {
        standard: Standard::Ntsc,
        gop_size: 15,
        bitrate,
        max_bitrate: 9_800_000,
        wide: false,
        format: Default::default(),
        pulldown: false,
    }
}

/// Encodes `frames`; each picture's stats.
fn encode(encoder: &mut Encoder, frames: &[Frame420]) -> Result<Vec<PictureStats>, String> {
    frames
        .iter()
        .map(|frame| {
            encoder
                .encode_frame(frame)
                .map_err(|error| error.to_string())?;
            encoder.last_picture().ok_or_else(|| "no stats".to_string())
        })
        .collect()
}

fn mean_code(stats: &[PictureStats]) -> f64 {
    stats
        .iter()
        .map(|picture| f64::from(picture.code))
        .sum::<f64>()
        / stats.len().max(1) as f64
}

#[test]
fn a_second_pass_gives_the_hard_scene_more_bits() -> Result<(), String> {
    // The hard scene first: one pass cannot know that an easy one follows and leaves it bits it does not need.
    let (hard, total) = (30, 60);
    let frames: Vec<Frame420> = (0..total).map(|t| picture(t, hard, 12)).collect();
    let bitrate = 2_500_000;

    let mut one = Encoder::new(config(bitrate)).map_err(|error| error.to_string())?;
    let first = encode(&mut one, &frames)?;

    let mut two = Encoder::new(config(bitrate)).map_err(|error| error.to_string())?;
    two.plan_second_pass(&first);
    let second = encode(&mut two, &frames)?;

    let bits =
        |stats: &[PictureStats]| stats.iter().map(|picture| picture.bits).sum::<u64>() as f64;
    let target = f64::from(bitrate) * 1001.0 / 30000.0 * total as f64;
    let (one_hard, two_hard) = (
        first.get(..hard).unwrap_or_default(),
        second.get(..hard).unwrap_or_default(),
    );
    eprintln!(
        "one pass: {:.0} bits ({:.2} of target), hard mean code {:.2}; two pass: {:.0} bits ({:.2}), hard mean code {:.2}",
        bits(&first),
        bits(&first) / target,
        mean_code(one_hard),
        bits(&second),
        bits(&second) / target,
        mean_code(two_hard),
    );
    // The average stays near the target.
    let ratio = bits(&second) / target;
    assert!((0.75..=1.25).contains(&ratio), "{ratio}");
    // The hard scene is quantised finer than one pass managed, with the easy scene's bits.
    assert!(
        mean_code(two_hard) + 1.0 < mean_code(one_hard),
        "{} {}",
        mean_code(two_hard),
        mean_code(one_hard)
    );
    assert!(bits(two_hard) > bits(one_hard) * 1.2);
    Ok(())
}
