//! Times how long a media file takes to open, probe and give its first frames (for large files).
//!
//! ```text
//! cargo run --release -p disccraft-transcode --example open_time -- movie.mkv
//! ```

use std::path::Path;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: open_time <file>")?;
    let path = Path::new(&path);
    let timed = |name: &str, f: &mut dyn FnMut() -> String| {
        let start = Instant::now();
        let result = f();
        println!("{name}: {:.2} s  {result}", start.elapsed().as_secs_f64());
    };
    timed("probe_seconds", &mut || {
        format!("{:?}", disccraft_transcode::probe_seconds(path))
    });
    timed("probe_video", &mut || {
        format!("{:?}", disccraft_transcode::probe_video(path).is_ok())
    });
    timed("first_frame", &mut || {
        format!(
            "{:?}",
            disccraft_transcode::first_frame(path).map(|(w, h, _)| (w, h))
        )
    });
    timed("frame_at 3600 s", &mut || {
        format!(
            "{:?}",
            disccraft_transcode::frame_at(path, 3600.0).map(|(w, h, _)| (w, h))
        )
    });
    timed("probe_audio_seconds", &mut || {
        format!("{:?}", disccraft_transcode::probe_audio_seconds(path))
    });
    Ok(())
}
