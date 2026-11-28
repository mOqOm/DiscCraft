//! M10.39: the progressive Blu-ray formats as MPEG-2. On a 720p 59.94 disc whose codec is automatic (H.264), a video
//! whose own codec is MPEG-2 makes its title MPEG-2 of 1280 x 720 at 59.94 progressive frames, and a video whose
//! own format is 1080p 23.976 in MPEG-2 makes its title MPEG-2 of 1920 x 1080 at 23.976; the menu stays the
//! disc's H.264. Each clip says so in its clip information, and ffmpeg decodes every clip without an error. Needs the
//! `ntsc.m2v` fixture and ffmpeg; skipped without.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_bd::build_bdmv;
use disccraft_project::{BdCodec, BdVideo, ClipTrim, DiscKind, Link, Project};
use disccraft_test_support::{ffmpeg, ffprobe, fixtures};
use std::path::PathBuf;
use std::process::Command;

/// What ffprobe says of a clip's video: codec, size, field order, frame rate and packets.
fn probe(ffprobe: &PathBuf, stream: &PathBuf) -> String {
    let out = Command::new(ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-count_packets"])
        .args(["-show_entries"])
        .args(["stream=codec_name,width,height,field_order,r_frame_rate,nb_read_packets"])
        .args(["-of", "csv=p=0"])
        .arg(stream)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .trim_end_matches(',')
        .to_string()
}

/// ffmpeg decodes the clip's video without an error.
fn decodes(ffmpeg: &PathBuf, stream: &PathBuf) {
    let out = Command::new(ffmpeg)
        .args(["-v", "error", "-xerror", "-err_detect", "explode", "-i"])
        .arg(stream)
        .args(["-map", "0:v:0", "-f", "null", "-"])
        .output()
        .unwrap();
    // (Probing a menu clip, ffmpeg tries its short graphics stream as MP3 audio and says so.)
    let errors = String::from_utf8_lossy(&out.stderr);
    let errors: Vec<&str> = errors
        .lines()
        .filter(|line| !line.contains("mp3float") && !line.contains("Last message repeated"))
        .collect();
    assert!(
        out.status.success() && errors.is_empty(),
        "{stream:?}: {errors:?}"
    );
}

#[test]
fn progressive_formats_can_be_mpeg2() {
    let (Some(ffmpeg), Some(ffprobe)) = (ffmpeg(), ffprobe()) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let video = fixtures().join("ntsc.m2v");
    if !video.is_file() {
        eprintln!("ntsc.m2v fixture not found; skipping");
        return;
    }
    // A second file of the same video: an asset of its own.
    let film = fixtures().join("mpeg2_progressive_film.m2v");
    std::fs::copy(&video, &film).unwrap();
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    p.settings.bd_video = BdVideo::Hd720p59;
    let fast = p.add_asset(&video).unwrap();
    let slow = p.add_asset(&film).unwrap();
    p.assets.get_mut(&fast).unwrap().bluray_codec = Some(BdCodec::Mpeg2);
    {
        let asset = p.assets.get_mut(&slow).unwrap();
        asset.bluray_video = Some(BdVideo::Hd1080p23);
        asset.bluray_codec = Some(BdCodec::Mpeg2);
    }
    let first = p.add_timeline(fast, Some("Fast".to_string())).unwrap();
    let second = p.add_timeline(slow, Some("Film".to_string())).unwrap();
    // One second of each (30 frames at 29.97).
    for timeline in [first, second] {
        p.timeline_mut(timeline).unwrap().video_trims = vec![ClipTrim {
            start: 0,
            end: Some(30),
        }];
    }
    let menu = p.add_menu(Some("Main menu".to_string())).unwrap();
    p.add_button(
        menu,
        "Play".to_string(),
        None,
        Link::Timeline {
            id: first,
            chapter: 0,
        },
    )
    .unwrap();
    p.settings.first_play = Link::Menu {
        id: menu,
        button: None,
    };

    let out = fixtures().join("bd_mpeg2_progressive_out");
    let _ = std::fs::remove_dir_all(&out);
    let report = build_bdmv(&p, &out).unwrap();
    assert_eq!(report.titles.len(), 2);

    let mut clips: Vec<PathBuf> = std::fs::read_dir(out.join("BDMV/STREAM"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    clips.sort();
    let mut found: Vec<String> = clips
        .iter()
        .map(|stream| {
            decodes(&ffmpeg, stream);
            probe(&ffprobe, stream)
        })
        .collect();
    found.sort();
    // The menu's still: the disc's H.264, one picture (ffprobe guesses a single picture's rate).
    assert!(
        found[0].starts_with("h264,1280,720,progressive,") && found[0].ends_with(",1"),
        "{found:?}"
    );
    // One second at 59.94 and at 23.976.
    assert_eq!(
        found[1..],
        [
            "mpeg2video,1280,720,progressive,60000/1001,60",
            "mpeg2video,1920,1080,progressive,24000/1001,24",
        ],
        "{clips:?}"
    );

    let mut codings: Vec<String> = std::fs::read_dir(out.join("BDMV/CLIPINF"))
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let info = disccraft_bdmv::clpi::read_clpi(&std::fs::read(&path).unwrap()).unwrap();
            let text = format!("{info:?}");
            ["H264Video", "Mpeg2Video"]
                .iter()
                .find_map(|kind| {
                    let at = text.find(kind)?;
                    let end = text[at..].find('}')? + at + 1;
                    Some(text[at..end].to_string())
                })
                .unwrap_or(text)
        })
        .collect();
    codings.sort();
    assert_eq!(
        codings,
        [
            "H264Video { video_format: 5, frame_rate: 7, aspect: 3 }",
            "Mpeg2Video { video_format: 5, frame_rate: 7, aspect: 3 }",
            "Mpeg2Video { video_format: 6, frame_rate: 1, aspect: 3 }",
        ]
    );
    if std::env::var_os("DISCCRAFT_KEEP_OUTPUT").is_none() {
        let _ = std::fs::remove_dir_all(&out);
    }
}
