//! Builds a 1080i Blu-ray of one title for each video given (each encoded to MPEG-2 HD) into a fresh folder and
//! cache, and prints how long it took: for timing the HD titles made side by side (M6.368).
//!
//! `cargo run --release -p disccraft-build --example hd_titles_time -- <out dir> <video>...`

use std::error::Error;
use std::path::PathBuf;

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_project::{BdVideo, DiscKind, Link, Project};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (Some(out), Some(_)) = (args.get(1), args.get(2)) else {
        return Err("usage: hd_titles_time <out dir> <video>...".into());
    };
    let out = PathBuf::from(out);
    let _ = std::fs::remove_dir_all(&out);
    let mut project = Project::default();
    project.settings.kind = DiscKind::BluRay;
    project.settings.bd_video = BdVideo::Hd1080i59;
    for (number, video) in args.iter().skip(2).enumerate() {
        let asset = project.add_asset(std::path::Path::new(video))?;
        let timeline = project.add_timeline(asset, Some(format!("Title {}", number + 1)))?;
        if number == 0 {
            project.settings.first_play = Link::Timeline {
                id: timeline,
                chapter: 0,
            };
        }
    }
    let options = BuildOptions {
        output: OutputKind::Folder,
        path: out.join("disc"),
        cache: Some(out.join("cache")),
        audio_language: None,
    };
    let started = std::time::Instant::now();
    let report = build(&project, &options)?;
    println!(
        "{} titles in {:.1} s",
        report.titles,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
