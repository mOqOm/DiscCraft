//! Times an HD Blu-ray transcode (1080p 23.976 H.264 at 15 Mbit/s) of a file, on the hardware encoder or the CPU:
//! `cargo run --release -p disccraft-platform --example hd_build_time -- <input> <out dir> hw|cpu [vbr|vbr2] [kbps]`.
//! With `DISCCRAFT_HD_TIMING=1` the transcode also prints where the time went.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (Some(input), Some(out), Some(engine)) = (args.get(1), args.get(2), args.get(3)) else {
        println!("usage: hd_build_time <input> <out dir> hw|cpu [vbr|vbr2] [kbps]");
        return Ok(());
    };
    disccraft_platform::use_for_transcodes(engine == "hw");
    let mode = if args.get(4).is_some_and(|mode| mode == "vbr2") {
        disccraft_transcode::RateMode::Vbr2Pass
    } else {
        disccraft_transcode::RateMode::Vbr
    };
    let options = disccraft_transcode::hd::HdOptions {
        width: 1920,
        height: 1080,
        fps_num: 24000,
        fps_den: 1001,
        target_kbps: args
            .get(5)
            .and_then(|kbps| kbps.parse().ok())
            .unwrap_or(15_000),
        max_kbps: 40_000,
        codec: disccraft_transcode::hd::HdCodec::H264,
        quality: disccraft_transcode::BEST_QUALITY,
        rendering: disccraft_transcode::Rendering::default(),
        mode,
        interlaced: false,
        wide: false,
    };
    std::fs::create_dir_all(out)?;
    let es = Path::new(out).join("out.h264");
    let index = Path::new(out).join("out.idx");
    let started = std::time::Instant::now();
    let frames = disccraft_transcode::hd::transcode_h264(
        Path::new(input),
        &es,
        &index,
        &options,
        &mut |_| {},
    )?;
    let elapsed = started.elapsed().as_secs_f64();
    println!(
        "{engine} {:?}: {frames} frames in {elapsed:.2} s = {:.1} fps, {} bytes",
        mode,
        frames as f64 / elapsed,
        std::fs::metadata(&es)?.len()
    );
    Ok(())
}
