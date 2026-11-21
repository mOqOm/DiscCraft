//! Encodes raw 8-bit 4:2:0 frames with the I/P sequence encoder and prints their sizes, for checks of rate control
//! and coding efficiency against real footage with ffmpeg / ffprobe.
//!
//! ```text
//! ffmpeg -i clip.vob -t 30 -s 720x480 -pix_fmt yuv420p -f rawvideo clip.yuv
//! cargo run --release -p disccraft-mpeg2venc --example encode_yuv -- clip.yuv clip.m2v [bitrate] [max_bitrate] [interlaced | fields]
//! ```
//!
//! The frames are 720×480 (NTSC); the bitrates are in bits per second (defaults: 6,000,000 and 9,000,000).

use disccraft_mpeg2venc::encoder::{Encoder, EncoderConfig};
use disccraft_mpeg2venc::{Frame420, Standard};
use std::io::{Read, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (Some(input), Some(output)) = (args.get(1), args.get(2)) else {
        return Err("usage: encode_yuv <in.yuv> <out.m2v> [bitrate] [max_bitrate]".into());
    };
    let bitrate = args.get(3).map_or(Ok(6_000_000), |value| value.parse())?;
    let max_bitrate = args.get(4).map_or(Ok(9_000_000), |value| value.parse())?;

    let (width, height) = (720usize, 480usize);
    let luma = width * height;
    let chroma = luma / 4;
    let mut reader = std::io::BufReader::new(std::fs::File::open(input)?);
    let mut writer = std::io::BufWriter::new(std::fs::File::create(output)?);
    let mut encoder = Encoder::new(EncoderConfig {
        standard: Standard::Ntsc,
        gop_size: 15,
        bitrate,
        max_bitrate,
        wide: true,
        format: disccraft_mpeg2venc::encoder::Format::Sd,
        pulldown: false,
    })?;
    // An `interlaced` fifth argument codes the frames as interlaced, the top field first (M6.371).
    if args.get(5).is_some_and(|flag| flag == "interlaced") {
        encoder.set_interlaced(true);
    }
    // `fields`: each frame as two field pictures, the top one first (M12.38).
    if args.get(5).is_some_and(|flag| flag == "fields") {
        encoder.set_field_pictures(true);
    }
    // DISCCRAFT_B_FRAMES=n puts n B pictures between the reference pictures (M12.32).
    let b_frames: u8 = std::env::var("DISCCRAFT_B_FRAMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    encoder.set_b_frames(b_frames);
    let mut given = 0usize;

    let mut buffer = vec![0u8; luma + 2 * chroma];
    let mut sizes = Vec::new();
    let mut codes = (0u64, 0u64);
    let started = std::time::Instant::now();
    while reader.read_exact(&mut buffer).is_ok() {
        let (y, rest) = buffer.split_at(luma);
        let (cb, cr) = rest.split_at(chroma);
        let frame = Frame420 {
            width: 720,
            height: 480,
            y: y.to_vec(),
            cb: cb.to_vec(),
            cr: cr.to_vec(),
        };
        for picture in encoder.push_frame(&frame, given.is_multiple_of(15))? {
            sizes.push(picture.data.len() * 8);
            writer.write_all(&picture.data)?;
        }
        given += 1;
        let (code_i, code_p) = encoder.quantiser_codes();
        codes = (codes.0 + u64::from(code_i), codes.1 + u64::from(code_p));
    }
    for picture in encoder.flush()? {
        sizes.push(picture.data.len() * 8);
        writer.write_all(&picture.data)?;
    }
    writer.write_all(&encoder.finish())?;
    writer.flush()?;

    let frames = sizes.len().max(1);
    let total: usize = sizes.iter().sum();
    let seconds = frames as f64 * 1001.0 / 30000.0;
    let i_frames: Vec<usize> = sizes.iter().copied().step_by(15).collect();
    let p_frames: Vec<usize> = sizes
        .iter()
        .enumerate()
        .filter(|(index, _)| index % 15 != 0)
        .map(|(_, bits)| *bits)
        .collect();
    let mean = |values: &[usize]| values.iter().sum::<usize>() / values.len().max(1);
    println!(
        "{frames} frames in {:.1} s: average {:.2} Mbit/s (target {:.2}), I {} kbit, P {} kbit, largest {} kbit, \
         mean quantiser I {:.1} P {:.1}",
        started.elapsed().as_secs_f64(),
        total as f64 / seconds / 1e6,
        f64::from(bitrate) / 1e6,
        mean(&i_frames) / 1000,
        mean(&p_frames) / 1000,
        sizes.iter().max().copied().unwrap_or(0) / 1000,
        codes.0 as f64 / frames as f64,
        codes.1 as f64 / frames as f64,
    );
    Ok(())
}
