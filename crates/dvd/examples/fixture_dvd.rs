//! Builds `target/fixtures/dvd_out/VIDEO_TS` from the ffmpeg fixtures (ntsc.m2v + ntsc.ac3, chapters at 0 / 5 / 10 s)
//! for manual checks with ffprobe or VLC. Run the dvd crate's tests first so the fixtures exist.

use disccraft_dvd::build_video_ts;
use disccraft_project::{AudioTrack, Project, Tick};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
    let mut project = Project::default();
    let video = project.add_asset(&fixtures.join("ntsc.m2v"))?;
    let audio = project.add_asset(&fixtures.join("ntsc.ac3"))?;
    let timeline = project.add_timeline(video, Some("Fixture".to_string()))?;
    project.add_chapter(timeline, Tick(5 * 254_016_000_000), None)?;
    project.add_chapter(timeline, Tick(10 * 254_016_000_000), None)?;
    if let Some(tl) = project.timeline_mut(timeline) {
        tl.audio.push(AudioTrack {
            asset: audio,
            language: "ja".to_string(),
        });
    }
    let report = build_video_ts(&project, &fixtures.join("dvd_out"))?;
    println!(
        "{} ({} bytes)",
        report.video_ts.display(),
        report.total_bytes
    );
    for t in &report.titles {
        println!(
            "title vts={} chapters={} sectors={} duration={:.2}s",
            t.vts,
            t.chapters,
            t.sectors,
            f64::from(t.duration_90k) / 90_000.0
        );
    }
    Ok(())
}
