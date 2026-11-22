//! Encodes a raw yuv420p clip with the system's hardware H.264 (M12.39) or HEVC Main 10 (M12.44) encoder and says
//! how fast: `cargo run --release -p disccraft-platform --example hw_video -- h264 clip.yuv 1920 1080 30000 1001
//! out.h264 [kbps]` (`hevc` for HEVC).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{Read, Write};

use disccraft_platform::{HwCodec, HwVideoConfig, HwVideoEncoder, hardware_encoders};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = std::env::args().collect();
    println!(
        "hardware H.264 encoders: {:?}",
        hardware_encoders(HwCodec::H264)
    );
    println!(
        "hardware HEVC Main 10 encoders: {:?}",
        hardware_encoders(HwCodec::HevcMain10)
    );
    if args.len() < 8 {
        println!("usage: hw_video h264|hevc clip.yuv width height fps_num fps_den out [kbps]");
        return Ok(());
    }
    let codec = if args.remove(1) == "hevc" {
        HwCodec::HevcMain10
    } else {
        HwCodec::H264
    };
    let (width, height): (u32, u32) = (args[2].parse()?, args[3].parse()?);
    let (fps_num, fps_den): (u32, u32) = (args[4].parse()?, args[5].parse()?);
    let kbps: u32 = args.get(7).map_or(Ok(15_000), |value| value.parse())?;
    let config = HwVideoConfig {
        codec,
        width,
        height,
        fps_num,
        fps_den,
        bitrate: kbps * 1000,
        max_bitrate: 40_000_000,
        buffer_bits: 30_000_000,
        gop: 24,
        b_frames: 0,
        slices: 4,
        sar: (1, 1),
        cbr: false,
    };
    let mut encoder = HwVideoEncoder::open(&config)?;
    println!("using {}", encoder.name());
    let (w, h) = (width as usize, height as usize);
    let frame_len = w * h * 3 / 2;
    let mut input = std::io::BufReader::new(std::fs::File::open(&args[1])?);
    let mut output = std::io::BufWriter::new(std::fs::File::create(&args[6])?);
    let mut yuv = vec![0u8; frame_len];
    let mut nv12 = vec![0u8; frame_len];
    let duration = i64::from(fps_den) * 10_000_000 / i64::from(fps_num);
    let started = std::time::Instant::now();
    let mut frames = 0i64;
    let mut packets = 0usize;
    let mut keys = 0usize;
    while input.read_exact(&mut yuv).is_ok() {
        // NV12: the luma, then Cb and Cr interleaved.
        nv12[..w * h].copy_from_slice(&yuv[..w * h]);
        let (cb, cr) = yuv[w * h..].split_at(w * h / 4);
        for (pair, (u, v)) in nv12[w * h..].chunks_mut(2).zip(cb.iter().zip(cr)) {
            pair[0] = *u;
            pair[1] = *v;
        }
        let picture: Vec<u8> = match codec {
            HwCodec::H264 => nv12.clone(),
            // P010: each sample in the top bits of a little-endian 16-bit word.
            HwCodec::HevcMain10 => nv12.iter().flat_map(|sample| [0, *sample]).collect(),
        };
        for packet in encoder.encode(&picture, frames * duration, false)? {
            output.write_all(&packet.data)?;
            packets += 1;
            keys += usize::from(packet.key);
        }
        frames += 1;
    }
    for packet in encoder.finish()? {
        output.write_all(&packet.data)?;
        packets += 1;
        keys += usize::from(packet.key);
    }
    output.flush()?;
    let seconds = started.elapsed().as_secs_f64();
    println!(
        "{frames} frames, {packets} packets ({keys} key) in {seconds:.2} s: {:.1} fps",
        frames as f64 / seconds
    );
    Ok(())
}
