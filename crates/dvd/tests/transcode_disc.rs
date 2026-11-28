//! M4 completion check: an H.264 + AAC MP4 becomes a menu DVD that fits DVD-5, navigates correctly, and decodes cleanly
//! (ffmpeg's DVD navigation and craft-video's MPEG-2 decoder). Skipped when ffmpeg is not available.

use disccraft_dvd::build_video_ts_with;
use disccraft_project::{Link, Project, Tick};
use disccraft_test_support::{ffmpeg, ffprobe, fixtures};
use disccraft_vm::{explore, load_video_ts};
use std::path::{Path, PathBuf};
use std::process::Command;

const TICKS_PER_SECOND: i64 = 254_016_000_000;

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
    let (Some(ffmpeg), Some(ffprobe)) = (ffmpeg(), ffprobe()) else {
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
                // libdvdnav tries lowercase `video_ts/` first; on case-sensitive
                // filesystems that miss is logged before it finds `VIDEO_TS/`.
                && !l.contains("findDirFile")
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

/// M12.25: interlaced H.264 (a camcorder's 1080i, made by x264 as MBAFF, in a transport stream with AC-3) becomes a
/// DVD: craft-video decodes it now (storytold/craft-libs PRs #1992 and #2017, through the mOqOm fork), so it is
/// converted like any other video, and the title decodes without errors with all its frames.
#[test]
fn interlaced_h264_becomes_a_dvd() {
    let (Some(ffmpeg), Some(ffprobe)) = (ffmpeg(), ffprobe()) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let video = fixtures().join("h264_1080i_short.m2ts");
    if !video.is_file() {
        std::fs::create_dir_all(fixtures()).unwrap();
        let made = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc2=size=1920x1080:rate=30000/1001")
            .args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
            .args(["-t", "2", "-c:v", "libx264", "-preset", "ultrafast"])
            .args(["-flags", "+ildct+ilme", "-x264-params", "tff=1:bframes=2"])
            .args(["-c:a", "ac3", "-b:a", "192k", "-f", "mpegts"])
            .arg(&video)
            .status();
        if !made.is_ok_and(|status| status.success()) {
            let _ = std::fs::remove_file(&video);
            eprintln!("ffmpeg cannot make interlaced H.264 (no libx264?); skipping");
            return;
        }
    }
    let mut project = Project::default();
    let v = project.add_asset(&video).unwrap();
    let t = project.add_timeline(v, Some("Camera".to_string())).unwrap();
    project.settings.first_play = Link::Timeline { id: t, chapter: 0 };
    let out = fixtures().join("dvd_interlaced_h264_out");
    let _ = std::fs::remove_dir_all(&out);
    let cache = fixtures().join("dvd_interlaced_h264_cache");
    let _ = std::fs::remove_dir_all(&cache);
    let _ = build_video_ts_with(&project, &out, &cache, &mut |_, _| {}).unwrap();
    let vob = out.join("VIDEO_TS/VTS_01_1.VOB");
    let probe = Command::new(&ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-count_frames"])
        .args([
            "-show_entries",
            "stream=codec_name,width,height,nb_read_frames",
        ])
        .args(["-of", "csv=p=0"])
        .arg(&vob)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    let line = text
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .trim_end_matches(',');
    // Two seconds at 29.97: 60 frames (a frame either way for where the sound and the picture end).
    let frames: u32 = line
        .rsplit(',')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    assert!(line.starts_with("mpeg2video,720,480,"), "{text}");
    assert!((59..=61).contains(&frames), "{text}");
    let decode = Command::new(&ffmpeg)
        .args(["-v", "error", "-xerror", "-i"])
        .arg(&vob)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decode.status.success() && decode.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&decode.stderr)
    );
    let _ = std::fs::remove_dir_all(&out);
    let _ = std::fs::remove_dir_all(&cache);
}

/// M12.26: an interlaced source's fields stay apart on the DVD. A narrow white bar moves 48 pixels a field (59.94
/// fields a second, interlaced by x264 as 1080i, the top field first); on the DVD each field is scaled on its own,
/// so the even lines (top field) show one sharp bar and the odd lines (bottom field) one sharp bar 18 pixels
/// further right (48 × 720 / 1920), with no ghost of the other field, and the pictures say the top field comes
/// first. Scaled as whole frames, every line was a mix of the two bars.
#[test]
fn an_interlaced_source_keeps_its_fields_on_a_dvd() {
    let (Some(ffmpeg), Some(ffprobe)) = (ffmpeg(), ffprobe()) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let video = fixtures().join("h264_1080i_bar.m2ts");
    if !video.is_file() {
        std::fs::create_dir_all(fixtures()).unwrap();
        let made = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("color=black:size=1920x1080:rate=60000/1001[bg];color=white:size=16x1080:rate=60000/1001[bar];[bg][bar]overlay=x='mod(n*48,1600)+100':y=0:shortest=1,tinterlace=mode=interleave_top,format=yuv420p")
            .args(["-t", "1", "-c:v", "libx264", "-preset", "ultrafast", "-qp", "10"])
            .args(["-flags", "+ildct+ilme", "-x264-params", "tff=1:bframes=0"])
            .args(["-aspect", "16:9", "-f", "mpegts"])
            .arg(&video)
            .status();
        if !made.is_ok_and(|status| status.success()) {
            let _ = std::fs::remove_file(&video);
            eprintln!("ffmpeg cannot make interlaced H.264 (no libx264?); skipping");
            return;
        }
    }
    let mut project = Project::default();
    project.settings.widescreen = true;
    let v = project.add_asset(&video).unwrap();
    let t = project.add_timeline(v, Some("Bar".to_string())).unwrap();
    project.settings.first_play = Link::Timeline { id: t, chapter: 0 };
    let out = fixtures().join("dvd_interlaced_bar_out");
    let cache = fixtures().join("dvd_interlaced_bar_cache");
    let _ = std::fs::remove_dir_all(&out);
    let _ = std::fs::remove_dir_all(&cache);
    let _ = build_video_ts_with(&project, &out, &cache, &mut |_, _| {}).unwrap();
    let vob = out.join("VIDEO_TS/VTS_01_1.VOB");
    let probe = Command::new(&ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries"])
        .args(["stream=field_order", "-of", "csv=p=0"])
        .arg(&vob)
        .output()
        .unwrap();
    let order = String::from_utf8_lossy(&probe.stdout);
    assert_eq!(order.trim().trim_end_matches(','), "tt", "{order}");
    // The luma of the tenth picture.
    let decoded = Command::new(&ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(&vob)
        .args([
            "-map",
            "0:v:0",
            "-vf",
            r"select=eq(n\,10)",
            "-frames:v",
            "1",
        ])
        .args(["-f", "rawvideo", "-pix_fmt", "gray", "-"])
        .output()
        .unwrap();
    let luma = decoded.stdout;
    assert_eq!(
        luma.len(),
        720 * 480,
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    // The bright columns of a line, and where their middle is.
    let bright =
        |row: usize| -> Vec<usize> { (0..720).filter(|&x| luma[row * 720 + x] > 160).collect() };
    let middle =
        |columns: &[usize]| columns.iter().sum::<usize>() as f64 / columns.len().max(1) as f64;
    for (top, bottom) in [(200_usize, 201_usize), (240, 241), (300, 301)] {
        let (a, b) = (bright(top), bright(bottom));
        assert!(
            !a.is_empty() && !b.is_empty(),
            "rows {top}/{bottom}: no bar"
        );
        // One bar each: a few columns, not two bars' worth.
        assert!(
            a.len() <= 10 && b.len() <= 10,
            "rows {top}/{bottom}: {a:?} {b:?}"
        );
        let shift = middle(&b) - middle(&a);
        assert!(
            (shift - 18.0).abs() < 4.0,
            "rows {top}/{bottom}: shift {shift}"
        );
        // No ghost of the other field's bar.
        let other = middle(&b).round() as usize;
        assert!(
            luma[top * 720 + other] < 90,
            "row {top}: ghost {}",
            luma[top * 720 + other]
        );
    }
    // M12.36: the P pictures are coded with the field tools (frame_pred_frame_dct 0): each field of the bar is
    // predicted from a field of the reference.
    let bytes = std::fs::read(&vob).unwrap();
    let mut picture_type = None;
    let mut p_pictures = Vec::new();
    for at in 0..bytes.len().saturating_sub(8) {
        if bytes[at..at + 3] != [0, 0, 1] {
            continue;
        }
        match bytes[at + 3] {
            0x00 => picture_type = Some((bytes[at + 5] >> 3) & 7),
            0xB5 if bytes[at + 4] >> 4 == 8 && picture_type.take() == Some(2) => {
                p_pictures.push((bytes[at + 7] >> 6) & 1 == 0);
            }
            _ => {}
        }
    }
    assert!(!p_pictures.is_empty());
    assert!(p_pictures.iter().all(|&tools| tools), "{p_pictures:?}");
    let _ = std::fs::remove_dir_all(&out);
    let _ = std::fs::remove_dir_all(&cache);
}
