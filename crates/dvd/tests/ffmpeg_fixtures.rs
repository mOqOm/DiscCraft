use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use disccraft_dvd::build_video_ts;
use disccraft_ifo::{read_vmgi, read_vtsi};
use disccraft_project::{AudioTrack, DiscKind, Project, Tick, VideoStandard};

const TICKS_PER_SECOND: i64 = 254_016_000_000;
static FIXTURE_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn builds_ntsc_ac3_dvd() -> Result<(), Box<dyn Error>> {
    let ffmpeg = ffmpeg_path();
    let ffprobe = ffprobe_path();
    if !tool_available(&ffmpeg) || !tool_available(&ffprobe) {
        println!("skipped: ffmpeg not found");
        return Ok(());
    }

    let _guard = FIXTURE_LOCK.lock().map_err(|_| "fixture lock poisoned")?;

    let fixtures = fixtures_dir();
    fs::create_dir_all(&fixtures)?;

    let m2v = fixtures.join("ntsc.m2v");
    let ac3 = fixtures.join("ntsc.ac3");

    if !m2v.is_file() {
        create_m2v(&ffmpeg, &m2v)?;
    }
    if !ac3.is_file() {
        create_ac3(&ffmpeg, &ac3)?;
    }

    let project = build_project(&m2v, &ac3)?;
    let dir = temp_dir("ntsc_ac3");
    let _ = fs::remove_dir_all(&dir);

    let result = run_dvd_test(&project, &dir, &ffprobe, &["mpeg2video", "ac3"]);
    let _ = fs::remove_dir_all(&dir);
    result
}

#[test]
fn builds_ntsc_lpcm_dvd() -> Result<(), Box<dyn Error>> {
    let ffmpeg = ffmpeg_path();
    let ffprobe = ffprobe_path();
    if !tool_available(&ffmpeg) || !tool_available(&ffprobe) {
        println!("skipped: ffmpeg not found");
        return Ok(());
    }

    let _guard = FIXTURE_LOCK.lock().map_err(|_| "fixture lock poisoned")?;

    let fixtures = fixtures_dir();
    fs::create_dir_all(&fixtures)?;

    let m2v = fixtures.join("ntsc.m2v");
    let wav = fixtures.join("ntsc.wav");

    if !m2v.is_file() {
        create_m2v(&ffmpeg, &m2v)?;
    }
    if !wav.is_file() {
        create_wav(&ffmpeg, &wav)?;
    }

    let project = build_project(&m2v, &wav)?;
    let dir = temp_dir("ntsc_lpcm");
    let _ = fs::remove_dir_all(&dir);

    let result = run_dvd_test(&project, &dir, &ffprobe, &["mpeg2video", "pcm_dvd"]);
    let _ = fs::remove_dir_all(&dir);
    result
}

fn run_dvd_test(
    project: &Project,
    dir: &Path,
    ffprobe: &Path,
    expected_codecs: &[&str],
) -> Result<(), Box<dyn Error>> {
    let report = build_video_ts(project, dir)?;
    let video_ts = dir.join("VIDEO_TS");

    for name in [
        "VIDEO_TS.IFO",
        "VIDEO_TS.BUP",
        "VTS_01_0.IFO",
        "VTS_01_0.BUP",
        "VTS_01_1.VOB",
    ] {
        check_file(&video_ts.join(name))?;
    }

    let vmgi_data = fs::read(video_ts.join("VIDEO_TS.IFO"))?;
    let vmgi = read_vmgi(&vmgi_data)?;
    if vmgi.titles.len() != 1 {
        return Err(format!("expected one VMG title, got {}", vmgi.titles.len()).into());
    }
    let title = vmgi.titles.first().ok_or("missing VMG title")?;
    if title.chapters != 3 {
        return Err(format!("expected 3 chapters, got {}", title.chapters).into());
    }

    let vtsi_data = fs::read(video_ts.join("VTS_01_0.IFO"))?;
    let vtsi = read_vtsi(&vtsi_data)?;
    let cells = vtsi.pgcs.first().map_or(0, |pgc| pgc.cells.len());
    if cells != 3 {
        return Err(format!("expected 3 cells, got {cells}").into());
    }

    if report.titles.len() != 1 {
        return Err(format!("expected one report title, got {}", report.titles.len()).into());
    }
    let report_title = report.titles.first().ok_or("missing report title")?;
    if report_title.chapters != 3 {
        return Err(format!("expected report chapters 3, got {}", report_title.chapters).into());
    }

    let expected = i64::from(20) * 90_000;
    let got = i64::from(report_title.duration_90k);
    let diff = got
        .checked_sub(expected)
        .and_then(|x| x.checked_abs())
        .ok_or("duration overflow")?;
    if diff > 45_000 {
        return Err(format!("duration {got} is not within 0.5 s of {expected}").into());
    }

    let stdout = probe_codecs(ffprobe, &video_ts.join("VTS_01_1.VOB"))?;
    for codec in expected_codecs {
        expect_contains(&stdout, codec)?;
    }

    Ok(())
}

fn build_project(video: &Path, audio: &Path) -> Result<Project, Box<dyn Error>> {
    let mut project = Project::default();
    project.settings.kind = DiscKind::Dvd;
    project.settings.standard = VideoStandard::Ntsc;

    let video_asset = project.add_asset(video)?;
    let audio_asset = project.add_asset(audio)?;
    let timeline = project.add_timeline(video_asset, Some("NTSC".to_string()))?;

    project.add_chapter(
        timeline,
        Tick(5 * TICKS_PER_SECOND),
        Some("Chapter 2".to_string()),
    )?;
    project.add_chapter(
        timeline,
        Tick(10 * TICKS_PER_SECOND),
        Some("Chapter 3".to_string()),
    )?;

    let timeline = project.timeline_mut(timeline).ok_or("missing timeline")?;
    timeline.audio.push(AudioTrack {
        asset: audio_asset,
        language: "ja".to_string(),
    });

    Ok(project)
}

fn probe_codecs(ffprobe: &Path, vob: &Path) -> Result<String, Box<dyn Error>> {
    let output = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "csv=p=0",
        ])
        .arg(vob)
        .output()?;

    if !output.stderr.is_empty() {
        return Err(format!(
            "ffprobe stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    if !output.status.success() {
        return Err("ffprobe failed".into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn create_m2v(ffmpeg: &Path, path: &Path) -> Result<(), Box<dyn Error>> {
    run_success(
        ffmpeg,
        &[
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=720x480:rate=30000/1001",
            "-t",
            "20",
            "-vf",
            "setfield=tff",
            "-c:v",
            "mpeg2video",
            "-pix_fmt",
            "yuv420p",
            "-b:v",
            "6000k",
            "-maxrate",
            "8000k",
            "-bufsize",
            "1835k",
            "-g",
            "15",
            "-bf",
            "2",
            "-flags",
            "+cgop+ilme+ildct",
            "-sc_threshold",
            "1000000000",
            "-aspect",
            "4:3",
            "-f",
            "mpeg2video",
        ],
        path,
    )
}

fn create_ac3(ffmpeg: &Path, path: &Path) -> Result<(), Box<dyn Error>> {
    run_success(
        ffmpeg,
        &[
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000:sample_rate=48000",
            "-t",
            "20",
            "-ac",
            "2",
            "-c:a",
            "ac3",
            "-b:a",
            "192k",
        ],
        path,
    )
}

fn create_wav(ffmpeg: &Path, path: &Path) -> Result<(), Box<dyn Error>> {
    run_success(
        ffmpeg,
        &[
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            "20",
            "-ac",
            "2",
            "-c:a",
            "pcm_s16le",
        ],
        path,
    )
}

fn run_success(cmd: &Path, args: &[&str], output_path: &Path) -> Result<(), Box<dyn Error>> {
    let output = Command::new(cmd).args(args).arg(output_path).output()?;
    if !output.status.success() {
        return Err(format!(
            "{} failed: {}",
            cmd.display(),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
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

fn ffmpeg_path() -> PathBuf {
    env::var_os("DISCCRAFT_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

fn ffprobe_path() -> PathBuf {
    env::var_os("DISCCRAFT_FFPROBE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffprobe"))
}

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .unwrap_or(manifest)
}

fn fixtures_dir() -> PathBuf {
    workspace_root().join("target").join("fixtures")
}

fn temp_dir(name: &str) -> PathBuf {
    env::temp_dir().join(format!("disccraft-dvd-{}-{}", std::process::id(), name))
}

fn check_file(path: &Path) -> Result<(), Box<dyn Error>> {
    if path.is_file() {
        Ok(())
    } else {
        Err(format!("missing file {}", path.display()).into())
    }
}

fn expect_contains(haystack: &str, needle: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(format!("expected {needle:?} in ffprobe output {haystack:?}").into())
    }
}
