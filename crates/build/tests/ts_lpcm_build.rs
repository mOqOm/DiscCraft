//! M6.366 check: an AVCHD-like `.m2ts` whose sound is Blu-ray LPCM (interlaced H.264, which the media reader
//! refuses as a whole) builds a 1080i Blu-ray with its sound: the video goes on as it is and the title has an audio
//! stream that ffprobe (the oracle) reads. The LPCM file is made from the AC-3 fixture with ffmpeg.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_project::{BdVideo, DiscKind, Link, Project};
use disccraft_test_support::{ffmpeg, ffprobe, fixtures};
use std::process::Command;

#[test]
fn an_lpcm_m2ts_builds_with_its_sound() {
    let fixture = fixtures().join("h264_1080i_tones.m2ts");
    if !fixture.is_file() {
        eprintln!("h264_1080i_tones.m2ts fixture not found; skipping");
        return;
    }
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-ts-lpcm-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let video = dir.join("camera.m2ts");
    let made = Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-i"])
        .arg(&fixture)
        .args(["-c:v", "copy", "-c:a", "pcm_bluray", "-f", "mpegts"])
        .args(["-mpegts_m2ts_mode", "1"])
        .arg(&video)
        .status()
        .is_ok_and(|status| status.success());
    if !made {
        eprintln!("ffmpeg not found; skipping");
        return;
    }

    let mut project = Project::default();
    project.settings.kind = DiscKind::BluRay;
    project.settings.bd_video = BdVideo::Hd1080i59;
    let asset = project.add_asset(&video).unwrap();
    let timeline = project
        .add_timeline(asset, Some("Main".to_string()))
        .unwrap();
    project.settings.first_play = Link::Timeline {
        id: timeline,
        chapter: 0,
    };
    let options = BuildOptions {
        output: OutputKind::Folder,
        path: dir.join("disc"),
        cache: Some(dir.join("cache")),
        audio_language: None,
    };
    let report = build(&project, &options).unwrap();
    assert_eq!(report.titles, 1);

    let stream = std::fs::read_dir(dir.join("disc").join("BDMV").join("STREAM"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension().is_some_and(|ext| ext == "m2ts")
                && std::fs::metadata(path).is_ok_and(|meta| meta.len() > 100_000)
        })
        .expect("no title stream");
    let probed = ffprobe().map(|ffprobe| {
        Command::new(ffprobe)
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type,codec_name",
            ])
            .args(["-of", "csv=p=0"])
            .arg(&stream)
            .output()
    });
    let Some(Ok(probed)) = probed else {
        eprintln!("ffprobe not found; the stream is not checked");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    };
    let text = String::from_utf8_lossy(&probed.stdout).to_string();
    assert!(text.contains("h264,video"), "{text}");
    assert!(text.contains(",audio"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}
