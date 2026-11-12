//! Encode a file to HD H.264 (Blu-ray settings) and print the access-unit index summary.
//!
//! `cargo run --release -p disccraft-transcode --example hd -- <input> <out.h264> <out.idx>`

use std::error::Error;
use std::path::Path;

use disccraft_transcode::hd::{HdOptions, read_h264_index, transcode_h264};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (Some(input), Some(es), Some(index)) = (args.get(1), args.get(2), args.get(3)) else {
        return Err("usage: hd <input> <out.h264> <out.idx>".into());
    };
    let options = HdOptions {
        width: 1920,
        height: 1080,
        fps_num: 24000,
        fps_den: 1001,
        target_kbps: 12_000,
        max_kbps: 30_000,
    };
    let started = std::time::Instant::now();
    let frames = transcode_h264(
        Path::new(input),
        Path::new(es),
        Path::new(index),
        &options,
        &mut |_| {},
    )?;
    let units = read_h264_index(Path::new(index))?;
    let keyframes = units.iter().filter(|u| u.keyframe).count();
    println!(
        "frames {frames}, units {}, keyframes {keyframes}, first pts/dts {:?}, {:.1} s",
        units.len(),
        units.first().map(|u| (u.pts, u.dts)),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
