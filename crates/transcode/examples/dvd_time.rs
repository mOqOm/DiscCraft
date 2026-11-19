//! Converts a file to DVD MPEG-2 (720 wide, NTSC, 6 Mbit/s) and prints how long it took, for timing the DVD
//! transcode (M12.24, M6.365).
//!
//! `cargo run --release -p disccraft-transcode --example dvd_time -- <input> <out.m2v> <out.wav>`

use std::error::Error;
use std::path::Path;

use disccraft_transcode::{TranscodeOptions, transcode};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (Some(input), Some(video), Some(audio)) = (args.get(1), args.get(2), args.get(3)) else {
        return Err("usage: dvd_time <input> <out.m2v> <out.wav>".into());
    };
    let options = TranscodeOptions {
        video_bitrate: 6_000_000,
        max_bitrate: 9_000_000,
        ..TranscodeOptions::default()
    };
    let started = std::time::Instant::now();
    let output = transcode(
        Path::new(input),
        Path::new(video),
        Path::new(audio),
        &options,
        &mut |_| {},
    )?;
    let seconds = started.elapsed().as_secs_f64();
    println!(
        "{} frames in {seconds:.2} s ({:.0} fps)",
        output.frames,
        output.frames as f64 / seconds
    );
    Ok(())
}
