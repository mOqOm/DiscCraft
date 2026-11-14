//! M4 completion check: an H.264 + AAC MP4 becomes a menu DVD that fits DVD-5, navigates correctly, and decodes cleanly
//! (ffmpeg's DVD navigation and craft-video's MPEG-2 decoder). Skipped when ffmpeg is not available.

use disccraft_dvd::build_video_ts_with;
use disccraft_project::{Link, Project, Tick};
use disccraft_vm::{explore, load_video_ts};
use std::path::{Path, PathBuf};
use std::process::Command;

const TICKS_PER_SECOND: i64 = 254_016_000_000;

fn tool(env: &str, name: &str) -> Option<PathBuf> {
    let path = std::env::var_os(env).map_or_else(|| PathBuf::from(name), PathBuf::from);
    Command::new(&path)
        .arg("-version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| path)
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures")
}

fn mp4_fixture(ffmpeg: &Path) -> PathBuf {
    let path = fixtures().join("h264_aac.mp4");
    if !path.is_file() {
        std::fs::create_dir_all(fixtures()).unwrap();
        let status = Command::new(ffmpeg)
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
            ])
            .args([
                "sine=frequency=660:sample_rate=44100",
                "-t",
                "6",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .args(["-c:a", "aac", "-shortest"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
    }
    path
}

fn project(mp4: &Path) -> Project {
    let mut p = Project::default();
    let v = p.add_asset(mp4).unwrap();
    let t = p.add_timeline(v, Some("Movie".to_string())).unwrap();
    p.add_chapter(t, Tick(2 * TICKS_PER_SECOND), None).unwrap();
    p.add_chapter(t, Tick(4 * TICKS_PER_SECOND), None).unwrap();
    p.timeline_mut(t).unwrap().end_action = Link::LastMenu;
    let m = p.add_menu(Some("Menu".to_string())).unwrap();
    for (chapter, label) in ["Play", "Chapter 2", "Chapter 3"].into_iter().enumerate() {
        p.add_button(
            m,
            label.to_string(),
            None,
            Link::Timeline {
                id: t,
                chapter: chapter as u32,
            },
        )
        .unwrap();
    }
    p.settings.first_play = Link::Menu {
        id: m,
        button: None,
    };
    // The language of audio without its own (the build fills it with the display language when unset).
    p.settings.audio_language = Some("ja".to_string());
    p
}

#[test]
fn mp4_becomes_a_clean_menu_dvd() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("skipped: ffmpeg not found");
        return;
    };
    let mp4 = mp4_fixture(&ffmpeg);
    let dir = std::env::temp_dir().join(format!("disccraft-transcode-disc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cache = dir.join("cache");
    let project = project(&mp4);

    let mut last = 0.0f32;
    let report =
        build_video_ts_with(&project, &dir.join("out"), &cache, &mut |_, f| last = f).unwrap();
    assert!(last > 0.9, "progress reached {last}");
    assert!(report.total_bytes < 4_700_000_000);
    let video_ts = dir.join("out").join("VIDEO_TS");

    // The video's own soundtrack has no language of its own: it takes the disc's (Japanese), as Encore writes it.
    let vtsi =
        disccraft_ifo::read_vtsi(&std::fs::read(video_ts.join("VTS_01_0.IFO")).unwrap()).unwrap();
    assert_eq!(vtsi.audio.len(), 1);
    assert_eq!(vtsi.audio[0].language, Some(*b"ja"));

    // Navigation: all three chapters reachable from the menu, no dead ends or loops.
    let model = load_video_ts(&video_ts).unwrap();
    // The project preview's model (made without building) is the disc's.
    assert_eq!(disccraft_dvd::preview_model(&project).unwrap(), model);
    let r = explore(&model);
    assert_eq!(r.chapters_reached.len(), 3, "{:?}", r.chapters_reached);
    assert!(
        r.dead_ends.is_empty() && r.loops.is_empty(),
        "{:?} {:?}",
        r.dead_ends,
        r.loops
    );

    // Streams in the title VOB.
    let out = Command::new(&ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "csv=p=0",
        ])
        .arg(video_ts.join("VTS_01_1.VOB"))
        .output()
        .unwrap();
    let codecs = String::from_utf8_lossy(&out.stdout);
    // The soundtrack is transcoded to Dolby Digital (AC-3), as Encore makes it.
    assert!(
        codecs.contains("mpeg2video") && codecs.contains("ac3"),
        "{codecs}"
    );

    // At 192 kbit/s, starting with the picture (the encoder's priming is taken off the front).
    let out = Command::new(&ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name,start_time,bit_rate",
        ])
        .args(["-of", "csv=p=0"])
        .arg(video_ts.join("VTS_01_1.VOB"))
        .output()
        .unwrap();
    let streams = String::from_utf8_lossy(&out.stdout);
    let field = |codec: &str, index: usize| -> String {
        streams
            .lines()
            .find(|line| line.starts_with(codec))
            .and_then(|line| line.split(',').nth(index))
            .unwrap_or_default()
            .to_string()
    };
    assert_eq!(field("ac3", 2), "192000", "{streams}");
    let start = |codec: &str| field(codec, 1).parse::<f64>().unwrap();
    assert!(
        (start("ac3") - start("mpeg2video")).abs() < 0.034,
        "{streams}"
    );

    // Full decode through ffmpeg's DVD navigation (libdvdnav) reports no errors.
    let out = Command::new(&ffmpeg)
        .args(["-v", "error", "-f", "dvdvideo", "-title", "1", "-i"])
        .arg(dir.join("out"))
        .args(["-map", "0", "-f", "null", "-"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    let real_errors: Vec<&str> = stderr
        .lines()
        .filter(|l| {
            !l.contains("libdvdcss")
                && !l.contains("for reading")
                && !l.contains("CSS auth")
                && !l.contains("device")
        })
        .collect();
    assert!(
        out.status.success() && real_errors.is_empty(),
        "{real_errors:?}"
    );

    // The cached MPEG-2 decodes cleanly in craft-video's MPEG-2 decoder, and a second build reuses the cache.
    let m2v = std::fs::read_dir(&cache)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "m2v"))
        .unwrap();
    let es = std::fs::read(&m2v).unwrap();
    let mut dec = craft_video::mpeg2::Decoder::new();
    let mut pictures = 0usize;
    for (i, range) in craft_video::mpeg2::access_units(&es)
        .into_iter()
        .enumerate()
    {
        pictures += dec.decode(&es[range], i as i64).unwrap().len();
    }
    pictures += dec.flush().len();
    assert_eq!(dec.errors(), 0, "{:?}", dec.last_error());
    assert!((178..=182).contains(&pictures), "{pictures} pictures");
    let started = std::time::Instant::now();
    build_video_ts_with(&project, &dir.join("out2"), &cache, &mut |_, _| {}).unwrap();
    assert!(
        started.elapsed().as_secs_f64() < 2.0,
        "second build should reuse the cache"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
