//! M10.41: an SD Blu-ray in H.264 (Encore's NTSC SD H.264 presets). With the disc's codec set to H.264, each title is
//! H.264 of 720 x 480 coded as interlaced frames (top field first) at 29.97, its pixels shaped for 4:3 or 16:9 as the
//! title is (8:9 or 32:27 across the whole width, as a DVD's); the clips say 480i, 29.97 and the title's aspect; the
//! menu stays the SD disc's MPEG-2; ffmpeg decodes every clip without an error and sees the subtitles at 720 x 480.
//! Needs the `ntsc.m2v` fixture and ffmpeg; skipped without.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_bd::build_bdmv;
use disccraft_project::{BdCodec, ClipTrim, DiscKind, Link, Project, SubtitleTrack};
use disccraft_test_support::{ffmpeg, ffprobe, fixtures};
use std::path::PathBuf;
use std::process::Command;

/// What ffprobe says of a clip's stream `select`: its `entries`.
fn probe(ffprobe: &PathBuf, stream: &PathBuf, select: &str, entries: &str) -> String {
    let out = Command::new(ffprobe)
        .args(["-v", "error", "-select_streams", select])
        .args(["-count_packets"])
        .args(["-show_entries", &format!("stream={entries}")])
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

/// An SD H.264 Blu-ray of two one-second titles of `video` (a copy each), the first 4:3 with a subtitle, the
/// second 16:9, and a menu.
fn project(video: &PathBuf) -> Project {
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    p.settings.bd_codec = BdCodec::H264;
    let mut timelines = Vec::new();
    for (name, wide) in [("narrow", false), ("wide", true)] {
        let copy = fixtures().join(format!("sd_h264_{name}.m2v"));
        std::fs::copy(video, &copy).unwrap();
        let asset = p.add_asset(&copy).unwrap();
        let timeline = p.add_timeline(asset, Some(name.to_string())).unwrap();
        let found = p.timeline_mut(timeline).unwrap();
        found.video_trims = vec![ClipTrim {
            start: 0,
            end: Some(30),
        }];
        found.widescreen = Some(wide);
        timelines.push(timeline);
    }
    let srt = fixtures().join("sd_h264_sub.srt");
    std::fs::write(&srt, "1\n00:00:00,100 --> 00:00:00,900\nWWWWWWWW\n\n").unwrap();
    let sub = p.add_asset(&srt).unwrap();
    p.timeline_mut(timelines[0])
        .unwrap()
        .subtitles
        .push(SubtitleTrack {
            asset: sub,
            language: "en".to_string(),
            group: 1,
        });
    let menu = p.add_menu(Some("Main menu".to_string())).unwrap();
    p.add_button(
        menu,
        "Play".to_string(),
        None,
        Link::Timeline {
            id: timelines[0],
            chapter: 0,
        },
    )
    .unwrap();
    p.settings.first_play = Link::Menu {
        id: menu,
        button: None,
    };
    p
}

#[test]
fn an_sd_blu_ray_can_be_h264() {
    let (Some(ffmpeg), Some(ffprobe)) = (ffmpeg(), ffprobe()) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let video = fixtures().join("ntsc.m2v");
    if !video.is_file() {
        eprintln!("ntsc.m2v fixture not found; skipping");
        return;
    }
    let p = project(&video);
    assert!(p.settings.bd_sd_h264());
    let out = fixtures().join("bd_sd_h264_out");
    let _ = std::fs::remove_dir_all(&out);
    let report = build_bdmv(&p, &out).unwrap();
    assert_eq!(report.titles.len(), 2);

    let mut clips: Vec<PathBuf> = std::fs::read_dir(out.join("BDMV/STREAM"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    clips.sort();
    let entries =
        "codec_name,width,height,field_order,r_frame_rate,sample_aspect_ratio,nb_read_packets";
    let mut found: Vec<String> = clips
        .iter()
        .map(|stream| {
            decodes(&ffmpeg, stream);
            probe(&ffprobe, stream, "v:0", entries)
        })
        .collect();
    found.sort();
    // The menu: the SD disc's MPEG-2.
    assert!(
        found
            .iter()
            .any(|line| line.starts_with("mpeg2video,720,480,")),
        "{found:?}"
    );
    // The titles: H.264 of 720 x 480, interlaced, 29.97, 4:3 and 16:9 across the whole width.
    for shape in ["8:9", "32:27"] {
        let wanted = format!("h264,720,480,{shape},tt,30000/1001,30");
        assert!(found.contains(&wanted), "{wanted} in {found:?}");
    }
    let subtitles: Vec<String> = clips
        .iter()
        .map(|stream| probe(&ffprobe, stream, "s:0", "codec_name,width,height"))
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(subtitles, ["hdmv_pgs_subtitle,720,480"]);

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
            "H264Video { video_format: 1, frame_rate: 4, aspect: 2 }",
            "H264Video { video_format: 1, frame_rate: 4, aspect: 3 }",
            "Mpeg2Video { video_format: 1, frame_rate: 4, aspect: 2 }",
        ]
    );
    if std::env::var_os("DISCCRAFT_KEEP_OUTPUT").is_none() {
        let _ = std::fs::remove_dir_all(&out);
    }
}
