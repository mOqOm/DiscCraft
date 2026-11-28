//! M6.254 check: an audio clip trimmed on its timeline is that part of its file on the disc, where the clip was
//! put (ffmpeg as an outside oracle). The file is a tone of 440 Hz in its first second, 880 Hz in its second and
//! 1320 Hz in its third; a WAV is decoded and cut, Dolby Digital is cut by its frames.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_project::{AudioClip, AudioTrack, Link, Project};
use disccraft_test_support::{ffmpeg, ffprobe};
use std::path::Path;
use std::process::Command;

/// The first sound of `file` as 16-bit mono samples at 48 kHz.
fn samples(ffmpeg: &Path, file: &Path) -> Vec<i16> {
    let output = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(file)
        .args([
            "-map", "0:a:0", "-ac", "1", "-ar", "48000", "-f", "s16le", "-",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{file:?}");
    output
        .stdout
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair))
        .collect()
}

/// The pitch of the sound from `from` to `to` seconds (how often it crosses zero upwards), and how loud it is.
fn pitch(samples: &[i16], from: f64, to: f64) -> (f64, i32) {
    let part = samples
        .get((from * 48_000.0) as usize..(to * 48_000.0) as usize)
        .unwrap_or_default();
    let crossings = part
        .array_windows()
        .filter(|[a, b]| *a < 0 && *b >= 0)
        .count();
    let loud = part.iter().map(|sample| i32::from(*sample).abs()).max();
    (
        crossings as f64 * 48_000.0 / part.len().max(1) as f64,
        loud.unwrap_or(0),
    )
}

#[test]
fn a_trimmed_audio_clip_is_that_part_of_its_file() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-audio-trim-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Four seconds of grey video (an MPEG-2 stream a DVD takes as it is), and the tones as WAV and as AC-3.
    let video = dir.join("grey.m2v");
    let made = Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("color=c=gray:size=720x480:rate=30000/1001")
        .args(["-t", "4", "-c:v", "mpeg2video", "-b:v", "3M", "-g", "15"])
        .args(["-aspect", "4:3", "-f", "mpeg2video"])
        .arg(&video)
        .status()
        .unwrap();
    assert!(made.success());
    let tones = |name: &str, codec: &[&str]| {
        let file = dir.join(name);
        let made = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("aevalsrc=0.5*sin(2*PI*440*(1+floor(t))*t):sample_rate=48000:duration=3")
            .args(["-ac", "2"])
            .args(codec)
            .arg(&file)
            .status()
            .unwrap();
        assert!(made.success());
        file
    };
    let wav = tones("tones.wav", &["-c:a", "pcm_s16le"]);
    let ac3 = tones("tones.ac3", &["-c:a", "ac3", "-b:a", "192k"]);

    let build_with = |name: &str, audio: &Path, track: (u32, u32, Option<u32>)| {
        let mut project = Project::default();
        let v = project.add_asset(&video).unwrap();
        let a = project.add_asset(audio).unwrap();
        let t = project.add_timeline(v, Some("Main".to_string())).unwrap();
        project.timeline_mut(t).unwrap().audio.push(AudioTrack {
            more: Vec::new(),
            start: track.0,
            trim_start: track.1,
            trim_end: track.2,
            ..AudioTrack::of(a)
        });
        project.settings.first_play = Link::Timeline { id: t, chapter: 0 };
        let out = dir.join(name);
        let report = build(
            &project,
            &BuildOptions {
                output: OutputKind::Folder,
                path: out.clone(),
                cache: Some(dir.join("cache")),
                audio_language: None,
            },
        )
        .unwrap();
        assert_eq!(report.titles, 1);
        samples(&ffmpeg, &out.join("VIDEO_TS/VTS_01_1.VOB"))
    };
    let near = |(pitch, loud): (f64, i32), wanted: f64| {
        assert!(loud > 4_000, "silent ({loud})");
        assert!((pitch - wanted).abs() < 20.0, "{pitch} Hz, not {wanted}");
    };
    let silent = |(_, loud): (f64, i32)| assert!(loud < 400, "not silent ({loud})");

    for (kind, audio) in [("wav", &wav), ("ac3", &ac3)] {
        // The whole file, for comparison: the three tones.
        let whole = build_with(&format!("{kind}-whole"), audio, (0, 0, None));
        near(pitch(&whole, 0.2, 0.8), 440.0);
        near(pitch(&whole, 2.2, 2.8), 1320.0);

        // Its second second alone (frames 30 to 60 of the file): 880 Hz for a second, then nothing.
        let middle = build_with(&format!("{kind}-middle"), audio, (0, 30, Some(60)));
        // M6.308: a trimmed WAV stays PCM; Dolby Digital stays Dolby Digital.
        if let Some(ffprobe) = ffprobe() {
            let codec = Command::new(&ffprobe)
                .args(["-v", "error", "-select_streams", "a:0", "-show_entries"])
                .args(["stream=codec_name", "-of", "csv=p=0"])
                .arg(
                    dir.join(format!("{kind}-middle"))
                        .join("VIDEO_TS/VTS_01_1.VOB"),
                )
                .output()
                .unwrap();
            let codec = String::from_utf8_lossy(&codec.stdout).trim().to_owned();
            assert_eq!(codec, if kind == "wav" { "pcm_dvd" } else { "ac3" });
        }
        near(pitch(&middle, 0.2, 0.8), 880.0);
        let seconds = middle.len() as f64 / 48_000.0;
        assert!((seconds - 1.0).abs() < 0.1, "{kind}: {seconds} s");

        // From its third second on, put a second into the timeline: silence, then 1320 Hz.
        let late = build_with(&format!("{kind}-late"), audio, (30, 60, None));
        silent(pitch(&late, 0.1, 0.9));
        near(pitch(&late, 1.2, 1.8), 1320.0);
        let seconds = late.len() as f64 / 48_000.0;
        assert!((seconds - 2.0).abs() < 0.1, "{kind}: {seconds} s");
    }

    // M6.269: two clips on one track: the WAV's second second at the start, and the AC-3's third second two
    // seconds in. 880 Hz, a second of silence, then 1320 Hz.
    let mut project = Project::default();
    let v = project.add_asset(&video).unwrap();
    let first = project.add_asset(&wav).unwrap();
    let second = project.add_asset(&ac3).unwrap();
    let t = project.add_timeline(v, Some("Main".to_string())).unwrap();
    project.timeline_mut(t).unwrap().audio.push(AudioTrack {
        trim_start: 30,
        trim_end: Some(60),
        more: vec![AudioClip {
            asset: second,
            start: 60,
            trim_start: 60,
            trim_end: None,
        }],
        ..AudioTrack::of(first)
    });
    project.settings.first_play = Link::Timeline { id: t, chapter: 0 };
    let out = dir.join("two-clips");
    let report = build(
        &project,
        &BuildOptions {
            output: OutputKind::Folder,
            path: out.clone(),
            cache: Some(dir.join("cache")),
            audio_language: None,
        },
    )
    .unwrap();
    assert_eq!(report.titles, 1);
    let two = samples(&ffmpeg, &out.join("VIDEO_TS/VTS_01_1.VOB"));
    near(pitch(&two, 0.2, 0.8), 880.0);
    silent(pitch(&two, 1.15, 1.85));
    near(pitch(&two, 2.2, 2.8), 1320.0);
    let seconds = two.len() as f64 / 48_000.0;
    assert!((seconds - 3.0).abs() < 0.1, "two clips: {seconds} s");

    // M6.308: clips of WAV and Dolby Digital are joined as Dolby Digital; two WAV clips stay PCM.
    let codec = |out: &Path| -> Option<String> {
        let ffprobe = ffprobe()?;
        let output = Command::new(&ffprobe)
            .args(["-v", "error", "-select_streams", "a:0", "-show_entries"])
            .args(["stream=codec_name", "-of", "csv=p=0"])
            .arg(out.join("VIDEO_TS/VTS_01_1.VOB"))
            .output()
            .ok()?;
        Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    if let Some(mixed) = codec(&out) {
        assert_eq!(mixed, "ac3");
        let mut project = Project::default();
        let v = project.add_asset(&video).unwrap();
        let first = project.add_asset(&wav).unwrap();
        let t = project.add_timeline(v, Some("Main".to_string())).unwrap();
        project.timeline_mut(t).unwrap().audio.push(AudioTrack {
            trim_start: 30,
            trim_end: Some(60),
            more: vec![AudioClip {
                asset: first,
                start: 60,
                trim_start: 60,
                trim_end: None,
            }],
            ..AudioTrack::of(first)
        });
        project.settings.first_play = Link::Timeline { id: t, chapter: 0 };
        let out = dir.join("two-wavs");
        let _ = build(
            &project,
            &BuildOptions {
                output: OutputKind::Folder,
                path: out.clone(),
                cache: Some(dir.join("cache")),
                audio_language: None,
            },
        )
        .unwrap();
        assert_eq!(codec(&out).as_deref(), Some("pcm_dvd"));
        let two = samples(&ffmpeg, &out.join("VIDEO_TS/VTS_01_1.VOB"));
        near(pitch(&two, 0.2, 0.8), 880.0);
        silent(pitch(&two, 1.15, 1.85));
        near(pitch(&two, 2.2, 2.8), 1320.0);
    }

    // M6.309: two Dolby Digital clips are joined by their frames (not decoded): 880 Hz, silence, 1320 Hz.
    if codec(&out).is_some() {
        let mut project = Project::default();
        let v = project.add_asset(&video).unwrap();
        let sound = project.add_asset(&ac3).unwrap();
        let t = project.add_timeline(v, Some("Main".to_string())).unwrap();
        project.timeline_mut(t).unwrap().audio.push(AudioTrack {
            trim_start: 30,
            trim_end: Some(60),
            more: vec![AudioClip {
                asset: sound,
                start: 60,
                trim_start: 60,
                trim_end: None,
            }],
            ..AudioTrack::of(sound)
        });
        project.settings.first_play = Link::Timeline { id: t, chapter: 0 };
        let out = dir.join("two-ac3");
        let _ = build(
            &project,
            &BuildOptions {
                output: OutputKind::Folder,
                path: out.clone(),
                cache: Some(dir.join("cache")),
                audio_language: None,
            },
        )
        .unwrap();
        assert_eq!(codec(&out).as_deref(), Some("ac3"));
        let two = samples(&ffmpeg, &out.join("VIDEO_TS/VTS_01_1.VOB"));
        near(pitch(&two, 0.2, 0.8), 880.0);
        silent(pitch(&two, 1.15, 1.85));
        near(pitch(&two, 2.2, 2.8), 1320.0);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
