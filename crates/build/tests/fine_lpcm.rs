//! M13.13 check: a 96 kHz 24-bit WAV goes on a DVD as LPCM as it is, and a decoder (ffmpeg, used as an outside
//! oracle) reads back the very samples of the file.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, TranscodeStatus, build, transcode_status};
use disccraft_project::{AudioTrack, Link, Project};
use std::path::PathBuf;
use std::process::Command;

fn tool(env: &str, name: &str) -> Option<PathBuf> {
    let path = std::env::var_os(env).map_or_else(|| PathBuf::from(name), PathBuf::from);
    Command::new(&path)
        .arg("-version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| path)
}

#[test]
fn a_96_khz_24_bit_wav_is_lpcm_on_a_dvd() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
    let video = fixtures.join("sound_track.mp4");
    if !video.is_file() {
        eprintln!("sound_track.mp4 fixture not found; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("disccraft-fine-lpcm-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Three seconds of a tone on the left and noise on the right, so every byte of a sample matters.
    let wav = dir.join("fine.wav");
    let made = Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("sine=frequency=440:sample_rate=96000:duration=3")
        .args(["-f", "lavfi", "-i"])
        .arg("anoisesrc=sample_rate=96000:duration=3:amplitude=0.3:seed=7")
        .args(["-filter_complex", "[0][1]amerge=inputs=2[a]", "-map", "[a]"])
        .args(["-c:a", "pcm_s24le"])
        .arg(&wav)
        .status()
        .unwrap();
    assert!(made.success());

    let mut project = Project::default();
    let v = project.add_asset(&video).unwrap();
    let a = project.add_asset(&wav).unwrap();
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
    // Nothing to convert of the WAV.
    assert_eq!(
        transcode_status(&project, None)[&a],
        TranscodeStatus::Unneeded
    );

    let out = dir.join("disc");
    let report = build(
        &project,
        &BuildOptions {
            output: OutputKind::Folder,
            path: out.clone(),
            cache: None,
            audio_language: None,
        },
    )
    .unwrap();
    assert_eq!(report.titles, 1);
    let vob = out.join("VIDEO_TS/VTS_01_1.VOB");
    let probe = Command::new(&ffprobe)
        .args(["-v", "error", "-select_streams", "a:0", "-show_entries"])
        .args([
            "stream=codec_name,sample_rate,channels,bits_per_raw_sample",
            "-of",
            "csv=p=0",
        ])
        .arg(&vob)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    assert_eq!(
        text.lines().next().unwrap_or_default().trim(),
        "pcm_dvd,96000,2,24",
        "{text}"
    );

    // The decoded samples are the file's (the title is as long as its two seconds of video).
    let raw = |input: &PathBuf| {
        let output = Command::new(&ffmpeg)
            .args(["-v", "error", "-xerror", "-i"])
            .arg(input)
            .args(["-map", "0:a:0", "-f", "s24le", "-"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    let (decoded, source) = (raw(&vob), raw(&wav));
    assert!(decoded.len() > 96_000 * 2 * 3, "{}", decoded.len());
    assert!(decoded.len() < source.len());
    assert!(decoded == source[..decoded.len()], "the samples differ");
    let _ = std::fs::remove_dir_all(&dir);
}

/// M13.16: the same on a Blu-ray, whose LPCM was 16-bit 48 kHz only: a 96 kHz 24-bit WAV, and a 48 kHz 24-bit
/// mono one, go on the disc as they are.
#[test]
fn a_24_bit_wav_is_lpcm_on_a_blu_ray() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
    let video = fixtures.join("sound_track.mp4");
    if !video.is_file() {
        eprintln!("sound_track.mp4 fixture not found; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("disccraft-fine-lpcm-bd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, rate, channels) in [("stereo96", 96_000, 2), ("mono48", 48_000, 1)] {
        let wav = dir.join(format!("{name}.wav"));
        let made = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!(
                "anoisesrc=sample_rate={rate}:duration=3:amplitude=0.3:seed=11"
            ))
            .args(["-ac", &channels.to_string(), "-c:a", "pcm_s24le"])
            .arg(&wav)
            .status()
            .unwrap();
        assert!(made.success());

        let mut project = Project::default();
        project.settings.kind = disccraft_project::DiscKind::BluRay;
        project.settings.capacity = disccraft_project::Capacity::Bd25;
        let v = project.add_asset(&video).unwrap();
        let a = project.add_asset(&wav).unwrap();
        let t = project.add_timeline(v, Some("Main".to_string())).unwrap();
        project
            .timeline_mut(t)
            .unwrap()
            .audio
            .push(AudioTrack::of(a));
        project.settings.first_play = Link::Timeline { id: t, chapter: 0 };
        assert_eq!(
            transcode_status(&project, None)[&a],
            TranscodeStatus::Unneeded
        );
        let out = dir.join(name);
        let report = build(
            &project,
            &BuildOptions {
                output: OutputKind::Folder,
                path: out.clone(),
                cache: None,
                audio_language: None,
            },
        )
        .unwrap();
        assert_eq!(report.titles, 1);
        let stream = out.join("BDMV/STREAM/00001.m2ts");
        let probe = Command::new(&ffprobe)
            .args(["-v", "error", "-select_streams", "a:0", "-show_entries"])
            .args([
                "stream=codec_name,sample_rate,channels,bits_per_raw_sample",
                "-of",
                "csv=p=0",
            ])
            .arg(&stream)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&probe.stdout);
        // (A mono stream is stored as two channels; the decoder gives the one.)
        assert_eq!(
            text.lines().next().unwrap_or_default().trim(),
            format!("pcm_bluray,{rate},{channels},24"),
            "{text}"
        );
        // The decoded samples are the file's (the title is as long as its two seconds of video).
        let raw = |input: &PathBuf| {
            let output = Command::new(&ffmpeg)
                .args(["-v", "error", "-xerror", "-i"])
                .arg(input)
                .args(["-map", "0:a:0", "-f", "s24le", "-"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        };
        let (decoded, source) = (raw(&stream), raw(&wav));
        assert!(decoded.len() > rate as usize * channels * 3, "{name}");
        // The last unit is filled up with silence past the title's end; up to the file's samples, they agree.
        let same = decoded
            .len()
            .min(source.len())
            .min(rate as usize * channels * 3 * 2);
        assert!(
            decoded[..same] == source[..same],
            "{name}: the samples differ"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
