//! Writes `target/fixtures/still.m2v`: an NTSC colour-bar still from the intra-only encoder, for checks with ffprobe / ffmpeg.

use disccraft_mpeg2venc::{Standard, StillOptions, encode_still, rgb_to_frame420};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (w, h) = (720usize, 480usize);
    let bars: [[u8; 3]; 8] = [
        [235, 235, 235],
        [235, 235, 16],
        [16, 235, 235],
        [16, 235, 16],
        [235, 16, 235],
        [235, 16, 16],
        [16, 16, 235],
        [16, 16, 16],
    ];
    let mut rgb = Vec::with_capacity(w * h * 3);
    for _y in 0..h {
        for x in 0..w {
            rgb.extend_from_slice(&bars[x * 8 / w]);
        }
    }
    let frame = rgb_to_frame420(720, 480, &rgb)?;
    let options = StillOptions {
        standard: Standard::Ntsc,
        quantiser_scale_code: 4,
        max_bytes: 224 * 1024,
        wide: false,
    };
    let data = encode_still(&frame, &options)?;
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures/still.m2v");
    std::fs::write(&out, &data)?;
    println!("{} ({} bytes)", out.display(), data.len());
    Ok(())
}
