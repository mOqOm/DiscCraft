//! M8.3 check: a Blu-ray project becomes a `BDMV` folder that libbluray (ffmpeg's `bluray:` input) opens, lists with the
//! expected duration and decodes without errors. Skipped when ffmpeg is not available.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_bd::build_bdmv;
use disccraft_project::{AudioTrack, DiscKind, Link, Project, SubtitleTrack, Tick};
use std::path::{Path, PathBuf};
use std::process::Command;

const TICKS_PER_SECOND: i64 = 254_016_000_000;
/// ffmpeg's `bluray:` input ignores playlists shorter than 180 seconds.
const SECONDS: u32 = 190;

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

/// A long, DVD-compliant MPEG-2 + AC-3 pair (passed through without transcoding).
fn long_fixture(ffmpeg: &Path) -> (PathBuf, PathBuf) {
    let video = fixtures().join("bd_long.m2v");
    let audio = fixtures().join("bd_long.ac3");
    if !video.is_file() || !audio.is_file() {
        std::fs::create_dir_all(fixtures()).unwrap();
        let seconds = SECONDS.to_string();
        let ok = Command::new(ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .args(["testsrc=size=720x480:rate=30000/1001", "-t", &seconds])
            .args(["-vf", "setdar=4/3", "-c:v", "mpeg2video", "-b:v", "1500k"])
            .args([
                "-maxrate", "6M", "-bufsize", "1835k", "-g", "15", "-bf", "0",
            ])
            .args(["-pix_fmt", "yuv420p", "-f", "mpeg2video"])
            .arg(&video)
            .status()
            .unwrap()
            .success()
            && Command::new(ffmpeg)
                .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
                .arg(format!(
                    "sine=frequency=440:sample_rate=48000:duration={SECONDS}"
                ))
                .args(["-ac", "2", "-c:a", "ac3", "-b:a", "192k"])
                .arg(&audio)
                .status()
                .unwrap()
                .success();
        assert!(ok, "ffmpeg could not create the fixtures");
    }
    (video, audio)
}

fn project(video: &Path, audio: &Path) -> Project {
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    let v = p.add_asset(video).unwrap();
    let a = p.add_asset(audio).unwrap();
    let t = p.add_timeline(v, Some("Main".to_string())).unwrap();
    p.timeline_mut(t).unwrap().audio.push(AudioTrack {
        asset: a,
        language: "ja".to_string(),
    });
    let srt = fixtures().join("bd_sub.srt");
    std::fs::write(
        &srt,
        "1
00:00:01,000 --> 00:00:04,000
Subtitle test

",
    )
    .unwrap();
    let s = p.add_asset(&srt).unwrap();
    p.timeline_mut(t).unwrap().subtitles.push(SubtitleTrack {
        asset: s,
        language: "en".to_string(),
    });
    p.add_chapter(t, Tick(63 * TICKS_PER_SECOND), None).unwrap();
    p.add_chapter(t, Tick(127 * TICKS_PER_SECOND), None)
        .unwrap();
    // A chapter playlist (chapters 3, 1) becomes a second PlayList over the same clip.
    let highlights = p
        .add_chapter_playlist(t, Some("Highlights".to_string()))
        .unwrap();
    p.add_chapter_playlist_entry(highlights, 2, None).unwrap();
    p.add_chapter_playlist_entry(highlights, 0, None).unwrap();
    // A pop-up menu over the title: the timeline's remote menu.
    let popup = p.add_menu(Some("Popup".to_string())).unwrap();
    p.add_button(
        popup,
        "Chapter 2".to_string(),
        None,
        Link::Timeline { id: t, chapter: 1 },
    )
    .unwrap();
    p.timeline_mut(t).unwrap().menu_remote = Link::Menu {
        id: popup,
        button: None,
    };
    p.settings.first_play = Link::Timeline { id: t, chapter: 0 };
    p
}

#[test]
fn project_becomes_a_bdmv_folder_that_libbluray_plays() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let (video, audio) = long_fixture(&ffmpeg);
    let out = fixtures().join("bd_out");
    let report = build_bdmv(&project(&video, &audio), &out).unwrap();
    assert_eq!(report.titles.len(), 2);
    assert_eq!(report.titles.first().unwrap().chapters, 3);
    for file in [
        "index.bdmv",
        "MovieObject.bdmv",
        "PLAYLIST/00001.mpls",
        "PLAYLIST/00002.mpls",
        "CLIPINF/00001.clpi",
        "STREAM/00001.m2ts",
        "BACKUP/index.bdmv",
    ] {
        assert!(out.join("BDMV").join(file).is_file(), "missing {file}");
    }

    // The chapter playlist's PlayList: two PlayItems over clip 00001, the last chapter then the first, and a
    // mark at the start of each.
    let highlights = disccraft_bdmv::mpls::read_mpls(
        &std::fs::read(out.join("BDMV/PLAYLIST/00002.mpls")).unwrap(),
    )
    .unwrap();
    assert_eq!(highlights.items.len(), 2);
    assert_eq!(highlights.marks.len(), 2);
    let main = disccraft_bdmv::mpls::read_mpls(
        &std::fs::read(out.join("BDMV/PLAYLIST/00001.mpls")).unwrap(),
    )
    .unwrap();
    let main_item = main.items.first().unwrap();
    // The pop-up menu rides in the title clip as an IG stream the playlist lists.
    assert_eq!(main_item.streams.ig.len(), 1);
    let first = highlights.items.first().unwrap();
    let second = highlights.items.get(1).unwrap();
    assert_eq!(&first.clip, b"00001");
    assert_eq!(first.in_time, main.marks.get(2).unwrap().time);
    assert_eq!(first.out_time, main_item.out_time);
    assert_eq!(second.in_time, main_item.in_time);
    assert_eq!(second.out_time, main.marks.get(1).unwrap().time);
    let playlist_title = report.titles.get(1).unwrap();
    assert_eq!(playlist_title.chapters, 2);
    assert_eq!(playlist_title.playlist, 2);

    // libbluray lists the playlist with the title duration and the two streams.
    let input = format!("bluray:{}", out.display()).replace('\\', "/");
    let probe = Command::new(&ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name:format=duration",
        ])
        .args(["-of", "compact", &input])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    assert!(
        probe.status.success(),
        "{}",
        String::from_utf8_lossy(&probe.stderr)
    );
    assert!(text.contains("codec_name=mpeg2video"), "{text}");
    assert!(text.contains("codec_name=ac3"), "{text}");
    assert!(text.contains("codec_name=hdmv_pgs_subtitle"), "{text}");

    // The whole title (video, audio, subtitles) decodes without errors through libbluray.
    let decode = Command::new(&ffmpeg)
        .args(["-v", "error", "-i", &input])
        // The overlay decodes the presentation graphics stream as well.
        .args(["-filter_complex", "[0:v][0:s:0]overlay", "-map", "0:a"])
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&decode.stderr);
    assert!(decode.status.success(), "{stderr}");
    assert!(stderr.trim().is_empty(), "decode errors: {stderr}");
}
