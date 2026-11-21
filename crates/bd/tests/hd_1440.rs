//! M10.40: the 1440 x 1080 Blu-ray formats (HDV's and AVCHD's size, shown 16:9). On a 1440 x 1080i 59.94 disc whose
//! codec is automatic (MPEG-2), the project's video is MPEG-2 of 1440 x 1080, interlaced; a video whose own codec is
//! H.264 is H.264 of 1440 x 1080 coded as interlaced frames (both are read as interlaced, the top field first:
//! since M12.37 the interlaced source's fields stay apart, as interlaced MPEG-2 pictures); videos whose own format
//! is 1440 x 1080p 23.976 / 24 are
//! progressive MPEG-2 / H.264 of that size. Every one says its pixels are 4:3 wide (shown 16:9) and decodes without
//! an error in ffmpeg; the clips say 1080i or 1080p, 16:9. The disc's graphics stay 1920 x 1080: the menu's picture,
//! and the subtitles drawn over the 1440 video. Needs the `ntsc.m2v` fixture and ffmpeg; skipped without.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_bd::build_bdmv;
use disccraft_project::{BdCodec, BdVideo, ClipTrim, DiscKind, Link, Project, SubtitleTrack};
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

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures")
}

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

/// A Blu-ray project of four one-second titles of `video` (a copy each, so each is an asset of its own) in the
/// formats and codecs above, with a subtitle on the first and a menu.
fn project(video: &PathBuf) -> Project {
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    p.settings.bd_video = BdVideo::Hd1440i59;
    let own = [
        ("plain", None, None),
        ("h264", None, Some(BdCodec::H264)),
        ("film", Some(BdVideo::Hd1440p23), Some(BdCodec::Mpeg2)),
        ("cinema", Some(BdVideo::Hd1440p24), Some(BdCodec::H264)),
    ];
    let mut timelines = Vec::new();
    for (name, format, codec) in own {
        let copy = fixtures().join(format!("hd_1440_{name}.m2v"));
        std::fs::copy(video, &copy).unwrap();
        let asset = p.add_asset(&copy).unwrap();
        {
            let found = p.assets.get_mut(&asset).unwrap();
            found.bluray_video = format;
            found.bluray_codec = codec;
        }
        let timeline = p.add_timeline(asset, Some(name.to_string())).unwrap();
        p.timeline_mut(timeline).unwrap().video_trims = vec![ClipTrim {
            start: 0,
            end: Some(30),
        }];
        timelines.push(timeline);
    }
    let srt = fixtures().join("hd_1440_sub.srt");
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
fn the_1440_formats_are_coded_1440_wide() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let video = fixtures().join("ntsc.m2v");
    if !video.is_file() {
        eprintln!("ntsc.m2v fixture not found; skipping");
        return;
    }
    let p = project(&video);
    let out = fixtures().join("bd_1440_out");
    let _ = std::fs::remove_dir_all(&out);
    let report = build_bdmv(&p, &out).unwrap();
    assert_eq!(report.titles.len(), 4);

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
    // The menu's still: 1920 x 1080, square pixels, one picture.
    assert!(
        found
            .iter()
            .any(|line| line.starts_with("mpeg2video,1920,1080,1:1,") && line.ends_with(",1")),
        "{found:?}"
    );
    // The four titles, 1440 wide, pixels 4:3 wide (ffprobe lists the pixels' shape before the scan).
    for (codec, scan, rate, frames) in [
        ("mpeg2video", "tt", "30000/1001", 30),
        ("h264", "tt", "30000/1001", 30),
        ("mpeg2video", "progressive", "24000/1001", 24),
        ("h264", "progressive", "24/1", 24),
    ] {
        let wanted = format!("{codec},1440,1080,4:3,{scan},{rate},{frames}");
        assert!(found.contains(&wanted), "{wanted} in {found:?}");
    }

    // The subtitles over the 1440 video are drawn for the 1920 x 1080 frame.
    let subtitles: Vec<String> = clips
        .iter()
        .map(|stream| probe(&ffprobe, stream, "s:0", "codec_name,width,height"))
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(subtitles, ["hdmv_pgs_subtitle,1920,1080"]);

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
            "H264Video { video_format: 4, frame_rate: 4, aspect: 3 }",
            "H264Video { video_format: 6, frame_rate: 2, aspect: 3 }",
            "Mpeg2Video { video_format: 4, frame_rate: 4, aspect: 3 }",
            "Mpeg2Video { video_format: 4, frame_rate: 4, aspect: 3 }",
            "Mpeg2Video { video_format: 6, frame_rate: 1, aspect: 3 }",
        ]
    );
    if std::env::var_os("DISCCRAFT_KEEP_OUTPUT").is_none() {
        let _ = std::fs::remove_dir_all(&out);
    }
}
