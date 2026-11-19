//! A button's transition (Encore's button Transition: a video played between the menu and where the button goes)
//! keeps its sound on the disc: the hidden title the build makes of it has the video's audio, as the title it leads
//! to has its own.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_project::{DiscKind, Link, Project};
use std::path::{Path, PathBuf};
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

/// `seconds` of a test picture with a tone of `hertz`, as H.264 and AAC.
fn video_with_sound(ffmpeg: &Path, name: &str, seconds: u32, hertz: u32) -> Option<PathBuf> {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
    let video = fixtures.join(name);
    if video.is_file() {
        return Some(video);
    }
    std::fs::create_dir_all(&fixtures).unwrap();
    let made = Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=720x480:rate=30000/1001")
        .args(["-f", "lavfi", "-i"])
        .arg(format!("sine=frequency={hertz}:sample_rate=48000"))
        .args([
            "-t",
            &seconds.to_string(),
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
        ])
        .args(["-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
        .arg(&video)
        .status();
    if !made.is_ok_and(|status| status.success()) {
        let _ = std::fs::remove_file(&video);
        return None;
    }
    Some(video)
}

/// The codecs of the audio streams of `file`, as ffprobe reads them.
fn audio_codecs(ffprobe: &Path, file: &Path) -> Vec<String> {
    let output = Command::new(ffprobe)
        .args(["-v", "error", "-select_streams", "a", "-show_entries"])
        .args(["stream=codec_name", "-of", "csv=p=0"])
        .arg(file)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().trim_end_matches(',').to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

/// The tone of the first audio stream of `file`, in hertz: its upward zero crossings a second, decoded by ffmpeg to
/// 8 kHz mono.
fn tone(ffmpeg: &Path, file: &Path) -> f64 {
    let output = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(file)
        .args([
            "-map", "0:a:0", "-ac", "1", "-ar", "8000", "-f", "s16le", "-",
        ])
        .output()
        .unwrap();
    let samples: Vec<i16> = output
        .stdout
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair))
        .collect();
    let crossings = samples
        .windows(2)
        .filter(|pair| pair[0] < 0 && pair[1] >= 0)
        .count();
    crossings as f64 / (samples.len().max(1) as f64 / 8000.0)
}

/// Whether `hertz` is near `want`.
fn near(hertz: f64, want: f64) -> bool {
    (hertz - want).abs() < want * 0.05
}

#[test]
fn a_button_transition_keeps_its_sound() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let (Some(main), Some(transition)) = (
        video_with_sound(&ffmpeg, "main_sound.mp4", 4, 440),
        video_with_sound(&ffmpeg, "transition_sound.mp4", 2, 880),
    ) else {
        eprintln!("ffmpeg cannot make the fixtures (no libx264?); skipping");
        return;
    };
    let dir =
        std::env::temp_dir().join(format!("disccraft-transition-sound-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut project = Project::default();
    let main = project.add_asset(&main).unwrap();
    let transition = project.add_asset(&transition).unwrap();
    let title = project
        .add_timeline(main, Some("Main".to_string()))
        .unwrap();
    let menu = project.add_menu(Some("Menu".to_string())).unwrap();
    project
        .add_button(
            menu,
            "Play".to_string(),
            None,
            Link::Timeline {
                id: title,
                chapter: 0,
            },
        )
        .unwrap();
    if let Some(button) = project
        .menu_mut(menu)
        .and_then(|found| found.buttons.first_mut())
    {
        button.transition = Some(transition);
    }
    project.settings.first_play = Link::Menu {
        id: menu,
        button: None,
    };
    let built = |project: &Project, name: &str| {
        let out = dir.join(name);
        let report = build(
            project,
            &BuildOptions {
                output: OutputKind::Folder,
                path: out.clone(),
                cache: Some(dir.join("cache")),
                audio_language: None,
            },
        )
        .unwrap();
        (out, report.titles)
    };

    // A DVD: the title and the transition's hidden title, each with its sound.
    let (out, titles) = built(&project, "dvd");
    assert_eq!(titles, 2);
    let mut vobs: Vec<PathBuf> = std::fs::read_dir(out.join("VIDEO_TS"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("VTS_") && name.ends_with("_1.VOB"))
        })
        .collect();
    vobs.sort();
    assert_eq!(vobs.len(), 2, "{vobs:?}");
    for vob in &vobs {
        assert_eq!(audio_codecs(&ffprobe, vob), ["ac3"], "{vob:?}");
    }
    // Each its own: the title's 440 Hz, the transition's 880 Hz.
    let mut tones: Vec<f64> = vobs.iter().map(|vob| tone(&ffmpeg, vob)).collect();
    tones.sort_by(f64::total_cmp);
    assert!(
        tones.len() == 2 && near(tones[0], 440.0) && near(tones[1], 880.0),
        "{tones:?}"
    );

    // A Blu-ray: the same.
    project.settings.kind = DiscKind::BluRay;
    project.settings.capacity = disccraft_project::Capacity::Bd25;
    let (out, titles) = built(&project, "bd");
    assert_eq!(titles, 2);
    let streams: Vec<PathBuf> = std::fs::read_dir(out.join("BDMV/STREAM"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    let tones: Vec<f64> = streams
        .iter()
        .filter(|stream| !audio_codecs(&ffprobe, stream).is_empty())
        .map(|stream| tone(&ffmpeg, stream))
        .collect();
    assert!(
        tones.iter().any(|hertz| near(*hertz, 440.0))
            && tones.iter().any(|hertz| near(*hertz, 880.0)),
        "{streams:?} {tones:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
