//! M13.14 check: an MPEG-1 Layer II file goes on a PAL DVD as it is (an MPEG audio stream holding the very bytes
//! of the file, read back by ffmpeg as an outside oracle), and on an NTSC DVD it is converted to AC-3.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, TranscodeStatus, build, transcode_status};
use disccraft_project::{AudioTrack, Link, Project, VideoStandard};
use disccraft_test_support::{ffmpeg, ffprobe, fixtures};
use std::path::Path;
use std::process::Command;

/// The first audio stream of `vob`: codec, sampling frequency, channels.
fn audio_of(ffprobe: &Path, vob: &Path) -> String {
    let probe = Command::new(ffprobe)
        .args(["-v", "error", "-select_streams", "a:0", "-show_entries"])
        .args(["stream=codec_name,sample_rate,channels", "-of", "csv=p=0"])
        .arg(vob)
        .output()
        .unwrap();
    String::from_utf8_lossy(&probe.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

#[test]
fn an_mp2_file_is_passed_through_on_a_pal_dvd() {
    let (Some(ffmpeg), Some(ffprobe)) = (ffmpeg(), ffprobe()) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let fixtures = fixtures();
    let video = fixtures.join("sound_track.mp4");
    if !video.is_file() {
        eprintln!("sound_track.mp4 fixture not found; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("disccraft-mp2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mp2 = dir.join("sound.mp2");
    let made = Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("sine=frequency=440:sample_rate=48000:duration=3")
        .args(["-ac", "2", "-c:a", "mp2", "-b:a", "224k"])
        .arg(&mp2)
        .status()
        .unwrap();
    assert!(made.success());

    let mut project = Project::default();
    project.settings.standard = VideoStandard::Pal;
    let v = project.add_asset(&video).unwrap();
    let a = project.add_asset(&mp2).unwrap();
    let t = project.add_timeline(v, Some("Main".to_string())).unwrap();
    project.timeline_mut(t).unwrap().audio.push(AudioTrack {
        more: Vec::new(),
        asset: a,
        stream: 0,
        language: String::new(),
        start: 0,
        trim_start: 0,
        trim_end: None,
    });
    project.settings.first_play = Link::Timeline { id: t, chapter: 0 };
    // Nothing to convert of the sound on a PAL disc.
    assert_eq!(
        transcode_status(&project, None)[&a],
        TranscodeStatus::Unneeded
    );

    let build_to = |project: &Project, name: &str| {
        let out = dir.join(name);
        let report = build(
            project,
            &BuildOptions {
                output: OutputKind::Folder,
                path: out.clone(),
                cache: None,
                audio_language: None,
            },
        )
        .unwrap();
        assert_eq!(report.titles, 1);
        out.join("VIDEO_TS/VTS_01_1.VOB")
    };
    let vob = build_to(&project, "pal");
    assert_eq!(audio_of(&ffprobe, &vob), "mp2,48000,2");
    // The stream decodes without an error.
    let decoded = Command::new(&ffmpeg)
        .args(["-v", "error", "-xerror", "-i"])
        .arg(&vob)
        .args(["-map", "0:a:0", "-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decoded.status.success() && decoded.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    // The frames on the disc are the file's (the title is as long as its two seconds of video).
    let copied = Command::new(&ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(&vob)
        .args(["-map", "0:a:0", "-c:a", "copy", "-f", "mp2", "-"])
        .output()
        .unwrap();
    assert!(copied.status.success());
    let source = std::fs::read(&mp2).unwrap();
    // 224 kbit/s: 672 bytes a frame of 24 ms.
    assert!(copied.stdout.len() >= 672 * 80, "{}", copied.stdout.len());
    assert_eq!(copied.stdout.len() % 672, 0);
    assert!(copied.stdout.len() < source.len());
    assert!(
        copied.stdout == source[..copied.stdout.len()],
        "the frames differ"
    );

    // An NTSC player need not decode MPEG audio: there the file is converted.
    project.settings.standard = VideoStandard::Ntsc;
    assert_ne!(
        transcode_status(&project, None)[&a],
        TranscodeStatus::Unneeded
    );
    let vob = build_to(&project, "ntsc");
    assert_eq!(audio_of(&ffprobe, &vob), "ac3,48000,2");
    let _ = std::fs::remove_dir_all(&dir);
}
