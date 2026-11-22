//! Encode a file to HD H.264 (Blu-ray settings), or HEVC (M12.43), and print the access-unit index summary.
//!
//! `cargo run --release -p disccraft-transcode --example hd -- <input> <out.h264> <out.idx> [h264|hevc] [width height]`

use std::error::Error;
use std::path::Path;

use disccraft_transcode::hd::{HdOptions, read_h264_index, transcode_h264};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (Some(input), Some(es), Some(index)) = (args.get(1), args.get(2), args.get(3)) else {
        return Err("usage: hd <input> <out.h264> <out.idx>".into());
    };
    let hevc = args.get(4).is_some_and(|codec| codec == "hevc");
    let size =
        |at: usize, default: u32| args.get(at).and_then(|n| n.parse().ok()).unwrap_or(default);
    let options = HdOptions {
        width: size(5, 1920),
        height: size(6, 1080),
        fps_num: 24000,
        fps_den: 1001,
        target_kbps: 12_000,
        max_kbps: 30_000,
        codec: if hevc {
            disccraft_transcode::hd::HdCodec::Hevc
        } else {
            disccraft_transcode::hd::HdCodec::H264
        },
        quality: disccraft_transcode::BEST_QUALITY,
        rendering: disccraft_transcode::Rendering::default(),
        mode: disccraft_transcode::RateMode::Vbr,
        interlaced: false,
        wide: false,
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
