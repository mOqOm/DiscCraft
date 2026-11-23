#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use disccraft_psmux::es::parse_video;
use disccraft_transcode::{Standard, TranscodeOptions, transcode};

static FIXTURE_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn transcodes_h264_aac_fixture() -> Result<(), Box<dyn Error>> {
    let ffmpeg = ffmpeg_path();
    if !tool_available(&ffmpeg) {
        println!("skipped: ffmpeg not found");
        return Ok(());
    }

    let _guard = FIXTURE_LOCK.lock().map_err(|_| "fixture lock poisoned")?;

    let fixtures = fixtures_dir();
    fs::create_dir_all(&fixtures)?;

    let input = fixtures.join("h264_aac.mp4");
    if !input.is_file() {
        create_fixture(&ffmpeg, &input)?;
    }

    let dir = temp_dir("h264_aac");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;

    // The file has sound: a timeline made from it lays the sound out as its own track.
    let probe = disccraft_transcode::probe_video(&input)?;
    assert!(probe.has_audio);
    assert!(probe.wide);

    // M10.12: a frame at a Blu-ray HD size. The 16:9 picture fills a 16:9 frame (its first row is picture); a 4:3
    // frame letterboxes it (its first row is a black bar); sizes are bounded.
    let reader = disccraft_transcode::FrameReader::open(&input)?;
    let (width, height, rgb) = reader.frame_sized(1.0, 1920, 1080)?;
    assert_eq!((width, height, rgb.len()), (1920, 1080, 1920 * 1080 * 3));
    assert!(rgb[..1920 * 3].iter().any(|value| *value > 16));
    let (_, _, boxed) = reader.frame_sized(1.0, 960, 720)?;
    assert!(boxed[..960 * 3].iter().all(|value| *value == 0));
    let (width, height, _) = reader.frame_sized(0.0, 100_000, 1)?;
    assert_eq!((width, height), (disccraft_transcode::MAX_FRAME_WIDTH, 16));

    // M6.406: a motion menu's frames for a widescreen menu fill the anamorphic 720 x 480 frame (its first row is
    // picture); for a 4:3 menu they are letterboxed (a black first row).
    for (wide, filled) in [(true, true), (false, false)] {
        let mut first = Vec::new();
        disccraft_transcode::for_each_frame(&input, Standard::Ntsc, 1, wide, &mut |_, rgb| {
            first = rgb.to_vec();
            Ok(())
        })?;
        assert_eq!(first.len(), 720 * 480 * 3);
        assert_eq!(first[..720 * 3].iter().any(|value| *value > 16), filled);
    }

    let result = run_transcode(&input, &dir);
    let _ = fs::remove_dir_all(&dir);
    result
}

fn run_transcode(input: &Path, dir: &Path) -> Result<(), Box<dyn Error>> {
    let video_out = dir.join("out.m2v");
    let audio_out = dir.join("out.wav");

    let options = TranscodeOptions {
        mode: Default::default(),
        standard: Standard::Ntsc,
        video_bitrate: 6_000_000,
        max_bitrate: 9_800_000,
        wide: false,
        gop_starts: Vec::new(),
        film: false,
        quality: disccraft_transcode::BEST_QUALITY,
        rendering: disccraft_transcode::Rendering::default(),
    };

    let mut progress = |_fraction| {};
    let output = transcode(input, &video_out, &audio_out, &options, &mut progress)?;

    if output.frames < 178 || output.frames > 182 {
        return Err(format!("unexpected frame count {}", output.frames).into());
    }

    let video_data = fs::read(&output.video)?;
    let info = parse_video(&video_data, 0).map_err(|error| format!("parse video: {error}"))?;

    if info.sequence.width != 720
        || info.sequence.height != 480
        || info.sequence.frame_rate_code != 4
    {
        return Err(format!(
            "unexpected video info {}x{} rate {}",
            info.sequence.width, info.sequence.height, info.sequence.frame_rate_code
        )
        .into());
    }

    let audio_path = output
        .audio
        .ok_or_else(|| "missing audio output".to_string())?;
    let wav = fs::read(&audio_path)?;
    check_wav(&wav)?;

    // M6.314: film (Encore's NTSC DV 23.976 presets): the source's pictures at 23.976 a second, 6 s in 144 of them,
    // in a 29.97 sequence, each showing as 3:2 pulldown says (a player, here ffprobe, reads repeat_pict 1, 0, 1, 0,
    // …); the sound as long; and it decodes without an error.
    let film = TranscodeOptions {
        film: true,
        ..options.clone()
    };
    let made = transcode(
        input,
        &dir.join("film.m2v"),
        &dir.join("film.wav"),
        &film,
        &mut |_| {},
    )?;
    assert_eq!(made.frames, 144);
    let data = fs::read(&made.video)?;
    let info = parse_video(&data, 0).map_err(|error| format!("parse video: {error}"))?;
    assert_eq!(info.sequence.frame_rate_code, 4);
    check_wav(&fs::read(made.audio.as_ref().ok_or("missing film audio")?)?)?;
    let ffprobe = ffprobe_path();
    if tool_available(&ffprobe) {
        let probed = Command::new(&ffprobe)
            .args([
                "-v",
                "error",
                "-show_entries",
                "frame=repeat_pict",
                "-of",
                "csv=p=0",
            ])
            .arg(&made.video)
            .output()?;
        let text = String::from_utf8_lossy(&probed.stdout);
        let repeats: Vec<&str> = text
            .lines()
            .map(|line| line.trim().trim_end_matches(','))
            .collect();
        assert_eq!(repeats.len(), 144, "{text}");
        assert!(
            repeats.chunks(4).all(|four| four == ["1", "0", "1", "0"]),
            "{repeats:?}"
        );
    }
    let decoded = Command::new(ffmpeg_path())
        .args(["-v", "error", "-xerror", "-i"])
        .arg(&made.video)
        .args(["-f", "null", "-"])
        .output()?;
    assert!(
        decoded.status.success() && decoded.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );

    // M6.300: VBR 2 pass goes over the video twice (the first pass writes nothing) and makes the same frames; the
    // progress runs once from 0 to 1 across both passes.
    let two = TranscodeOptions {
        mode: disccraft_transcode::RateMode::Vbr2Pass,
        ..options.clone()
    };
    let mut seen = Vec::new();
    let mut progress = |fraction: f32| seen.push(fraction);
    let second = transcode(
        input,
        &dir.join("two.m2v"),
        &dir.join("two.wav"),
        &two,
        &mut progress,
    )?;
    assert_eq!(second.frames, output.frames);
    let data = fs::read(&second.video)?;
    let info = parse_video(&data, 0).map_err(|error| format!("parse video: {error}"))?;
    assert_eq!((info.sequence.width, info.sequence.height), (720, 480));
    assert!(seen.windows(2).all(|pair| pair[0] <= pair[1]), "{seen:?}");
    assert!(seen.iter().any(|fraction| (0.45..=0.55).contains(fraction)));
    assert_eq!(seen.last().copied(), Some(1.0));

    // M6.326: Encore's "Low quality" (quality 2) searches less for motion: another stream of the same frames, which
    // decodes without an error.
    let low = TranscodeOptions {
        quality: 2,
        ..options
    };
    let made = transcode(
        input,
        &dir.join("low.m2v"),
        &dir.join("low.wav"),
        &low,
        &mut |_| {},
    )?;
    assert_eq!(made.frames, output.frames);
    assert_ne!(fs::read(&made.video)?, video_data);
    let decoded = Command::new(ffmpeg_path())
        .args(["-v", "error", "-xerror", "-i"])
        .arg(&made.video)
        .args(["-f", "null", "-"])
        .output()?;
    assert!(
        decoded.status.success() && decoded.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );

    Ok(())
}

fn check_wav(wav: &[u8]) -> Result<(), Box<dyn Error>> {
    if wav.len() < 44 {
        return Err("wav too short".into());
    }
    if &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return Err("bad wav header".into());
    }

    let channels = u16::from_le_bytes([wav[22], wav[23]]);
    let sample_rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]);
    let data_size = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]) as u64;

    if channels != 2 {
        return Err(format!("expected stereo, got {channels}").into());
    }
    if sample_rate != 48_000 {
        return Err(format!("expected 48 kHz, got {sample_rate}").into());
    }

    let frames = data_size / 4;
    let expected = 6 * 48_000;
    if frames.abs_diff(expected) > 48_000 {
        return Err(format!("audio length {frames} not near 6 s").into());
    }

    Ok(())
}

fn create_fixture(ffmpeg: &Path, path: &Path) -> Result<(), Box<dyn Error>> {
    let output = Command::new(ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=1280x720:rate=24",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=660:sample_rate=44100",
            "-t",
            "6",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(path)
        .output()?;

    if !output.status.success() {
        return Err(format!("ffmpeg failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }

    Ok(())
}

fn tool_available(cmd: &Path) -> bool {
    Command::new(cmd)
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn ffprobe_path() -> PathBuf {
    env::var_os("DISCCRAFT_FFPROBE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffprobe"))
}

fn ffmpeg_path() -> PathBuf {
    env::var_os("DISCCRAFT_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|parent| parent.parent())
        .map(PathBuf::from)
        .unwrap_or(manifest)
}

fn fixtures_dir() -> PathBuf {
    workspace_root().join("target").join("fixtures")
}

fn temp_dir(name: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "disccraft-transcode-{}-{}",
        std::process::id(),
        name
    ))
}
