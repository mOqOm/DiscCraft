//! M6.422 check: the subtitle tracks inside an MKV (SubRip and ASS here) become a new timeline's subtitle tracks, each
//! taken out as a `.srt` beside the project, in the language the file names, with the file's times and text (ASS
//! override tags taken out). M6.542: an MP4's text track (`mov_text`, 3GPP timed text) the same way, and an MKV's
//! VobSub as a `.idx` + `.sub` with the same subpicture units at the same times (only on a DVD). The files are made
//! with ffmpeg (the outside oracle); without it the tests are skipped.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_engine::Session;
use disccraft_test_support::ffmpeg;
use serde_json::{Value, json};
use std::process::Command;

const JAPANESE: &str = "1\n00:00:00,500 --> 00:00:01,500\nこんにちは\n\n2\n00:00:02,000 --> 00:00:03,250\n二行目の\n字幕です\n\n";
const ENGLISH: &str = "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 480\n\n\
[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, \
Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, \
MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,\
1,2,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:00.75,0:00:01.75,Default,,0,0,0,,{\\i1}Hello{\\i0}, world\n\
Dialogue: 0,0:00:02.50,0:00:03.50,Default,,0,0,0,,Line one\\NLine two\n";

fn cues(session: &mut Session, asset: u64) -> Vec<(i64, i64, String)> {
    let value = session
        .execute("subtitle.cues", &json!({ "asset": asset }))
        .unwrap();
    value["cues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cue| {
            // `as` is exact enough for milliseconds.
            let ms = |key: &str| (cue[key].as_f64().unwrap() * 1000.0).round() as i64;
            let text = match &cue["text"] {
                Value::Array(lines) => lines
                    .iter()
                    .map(|line| line.as_str().unwrap().to_owned())
                    .collect::<Vec<_>>()
                    .join("|"),
                other => other.as_str().unwrap_or_default().replace('\n', "|"),
            };
            (ms("start"), ms("end"), text)
        })
        .collect()
}

#[test]
fn an_mkvs_subtitle_tracks_become_the_timelines() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-embedded-subs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ja.srt"), JAPANESE).unwrap();
    std::fs::write(dir.join("en.ass"), ENGLISH).unwrap();
    let video = dir.join("subtitled.mkv");
    let made = Command::new(&ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=30000/1001",
        ])
        .args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
        .arg("-i")
        .arg(dir.join("ja.srt"))
        .arg("-i")
        .arg(dir.join("en.ass"))
        .args([
            "-map", "0:v", "-map", "1:a", "-map", "2:s", "-map", "3:s", "-t", "4",
        ])
        .args([
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-c:s", "copy",
        ])
        .args([
            "-metadata:s:s:0",
            "language=jpn",
            "-metadata:s:s:1",
            "language=eng",
        ])
        .arg(&video)
        .status()
        .unwrap();
    assert!(made.success());

    let mut session = Session::new();
    session.path = Some(dir.join("project.dcproj"));
    let asset = session
        .execute("asset.import", &json!({"path": video.to_string_lossy()}))
        .unwrap()["asset"]
        .as_u64()
        .unwrap();
    let timeline = session
        .execute("timeline.create", &json!({"asset": asset}))
        .unwrap()["timeline"]
        .as_u64()
        .unwrap();
    let tracks = session
        .project
        .timeline(disccraft_project::TimelineId(timeline))
        .unwrap()
        .subtitles
        .clone();
    let languages: Vec<&str> = tracks.iter().map(|track| track.language.as_str()).collect();
    assert_eq!(languages, ["ja", "en"]);
    // Each taken out beside the project.
    for track in &tracks {
        let asset = &session.project.assets[&track.asset];
        assert_eq!(asset.kind, disccraft_project::AssetKind::Subtitle);
        assert!(
            std::path::Path::new(&asset.path).starts_with(dir.join("project.subtitles")),
            "{}",
            asset.path
        );
    }
    assert_eq!(
        cues(&mut session, tracks[0].asset.0),
        [
            (500, 1500, "こんにちは".to_owned()),
            (2000, 3250, "二行目の|字幕です".to_owned()),
        ]
    );
    assert_eq!(
        cues(&mut session, tracks[1].asset.0),
        [
            (750, 1750, "Hello, world".to_owned()),
            (2500, 3500, "Line one|Line two".to_owned()),
        ]
    );
    // The probe lists them; the command adds them again beside the tracks there are.
    let probe = session
        .execute("asset.probe", &json!({"asset": asset}))
        .unwrap();
    assert_eq!(
        probe["subtitleStreams"],
        json!([
            {"format": "text", "language": "ja", "name": "", "forced": false},
            {"format": "text", "language": "en", "name": "", "forced": false},
        ]),
        "{probe}"
    );
    let imported = session
        .execute(
            "timeline.importSourceSubtitles",
            &json!({"timeline": timeline}),
        )
        .unwrap();
    assert_eq!(imported, json!({"added": 2, "skipped": 0}));
    let count = session
        .project
        .timeline(disccraft_project::TimelineId(timeline))
        .unwrap()
        .subtitles
        .len();
    assert_eq!(count, 4);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Runs ffmpeg with `args`, quietly; whether it made its output.
fn run(ffmpeg: &std::path::Path, args: &[&std::ffi::OsStr]) -> bool {
    Command::new(ffmpeg)
        .args(["-v", "error", "-y"])
        .args(args)
        .status()
        .is_ok_and(|status| status.success())
}

/// The timeline made of `video` in a new project beside `dir` (a Blu-ray when `bluray`): its subtitle tracks.
fn timeline_of(dir: &std::path::Path, video: &std::path::Path, bluray: bool) -> (Session, u64) {
    let mut session = Session::new();
    session.path = Some(dir.join("project.dcproj"));
    if bluray {
        session.project.settings.kind = disccraft_project::DiscKind::BluRay;
    }
    let asset = session
        .execute("asset.import", &json!({"path": video.to_string_lossy()}))
        .unwrap()["asset"]
        .as_u64()
        .unwrap();
    let timeline = session
        .execute("timeline.create", &json!({"asset": asset}))
        .unwrap()["timeline"]
        .as_u64()
        .unwrap();
    (session, timeline)
}

/// M6.542: an MP4's text track (as ffmpeg's `mov_text` writes 3GPP timed text) becomes a subtitle track, taken out
/// as a `.srt` with its times and text, in the language the file names.
#[test]
fn an_mp4s_text_track_becomes_the_timelines() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-mp4-text-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ja.srt"), JAPANESE).unwrap();
    let video = dir.join("subtitled.mp4");
    let srt = dir.join("ja.srt");
    let args: Vec<&std::ffi::OsStr> = [
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=320x240:rate=30000/1001",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000",
        "-i",
    ]
    .iter()
    .map(std::ffi::OsStr::new)
    .chain([srt.as_os_str()])
    .chain(
        [
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:s",
            "-t",
            "4",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-c:s",
            "mov_text",
            "-metadata:s:s:0",
            "language=jpn",
        ]
        .iter()
        .map(std::ffi::OsStr::new),
    )
    .chain([video.as_os_str()])
    .collect();
    assert!(run(&ffmpeg, &args));
    let (mut session, timeline) = timeline_of(&dir, &video, false);
    let tracks = session
        .project
        .timeline(disccraft_project::TimelineId(timeline))
        .unwrap()
        .subtitles
        .clone();
    let languages: Vec<&str> = tracks.iter().map(|track| track.language.as_str()).collect();
    assert_eq!(languages, ["ja"]);
    assert!(
        session.project.assets[&tracks[0].asset]
            .path
            .ends_with(".srt")
    );
    assert_eq!(
        cues(&mut session, tracks[0].asset.0),
        [
            (500, 1500, "こんにちは".to_owned()),
            (2000, 3250, "二行目の|字幕です".to_owned()),
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// M6.542: an MKV's VobSub (here one DiscCraft wrote and ffmpeg put into an MKV) is taken out as a `.idx` + `.sub`
/// holding the same subpicture units at the same times, for a DVD; M6.546: for a Blu-ray too.
#[test]
fn an_mkvs_vobsub_becomes_a_dvds() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-mkv-vobsub-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Two subpictures: a white bar, and a larger one spanning packs.
    let unit = |width: u16, height: u16, seconds: u32| {
        let bitmap = disccraft_spu::Bitmap {
            width,
            height,
            pixels: (0..usize::from(width) * usize::from(height))
                .map(|at| u8::try_from(at % 3 + 1).unwrap())
                .collect(),
        };
        let params = disccraft_spu::SpuParams {
            x: 100,
            y: 400,
            colors: [0, 1, 2, 3],
            alpha: [0, 15, 15, 15],
            forced: false,
            stop_delay: Some(disccraft_spu::spu_delay_from_90k(seconds * 90_000)),
        };
        disccraft_spu::encode_spu(&bitmap, &params).unwrap()
    };
    let units = vec![(1.0, unit(40, 10, 1)), (2.5, unit(500, 60, 1))];
    let header = "# VobSub index file, v7 (do not modify this line!)\nsize: 720x480\norg: 0, 0\n\
                  scale: 100%, 100%\nalpha: 100%\nsmooth: OFF\nfadein/out: 0, 0\nalign: OFF at LEFT TOP\n\
                  time offset: 0\nforced subs: OFF\npalette: 000000, ffffff, 808080, 000000, 000000, 000000, \
                  000000, 000000, 000000, 000000, 000000, 000000, 000000, 000000, 000000, 000000\n\
                  custom colors: OFF, tridx: 0000, colors: 000000, 000000, 000000, 000000\n";
    let (idx, sub) = disccraft_transcode::vobsub_files(header, Some("ja"), &units);
    std::fs::write(dir.join("ja.idx"), idx).unwrap();
    std::fs::write(dir.join("ja.sub"), &sub).unwrap();
    let video = dir.join("vobsub.mkv");
    let idx_path = dir.join("ja.idx");
    let args: Vec<&std::ffi::OsStr> = [
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=720x480:rate=30000/1001",
        "-i",
    ]
    .iter()
    .map(std::ffi::OsStr::new)
    .chain([idx_path.as_os_str()])
    .chain(
        [
            "-map",
            "0:v",
            "-map",
            "1:s",
            "-t",
            "4",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:s",
            "copy",
            "-metadata:s:s:0",
            "language=jpn",
        ]
        .iter()
        .map(std::ffi::OsStr::new),
    )
    .chain([video.as_os_str()])
    .collect();
    assert!(run(&ffmpeg, &args));
    let found = disccraft_transcode::embedded_subtitles(&video);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].format, disccraft_transcode::EmbeddedFormat::VobSub);

    let (session, timeline) = timeline_of(&dir, &video, false);
    let tracks = session
        .project
        .timeline(disccraft_project::TimelineId(timeline))
        .unwrap()
        .subtitles
        .clone();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].language, "ja");
    let taken = std::path::PathBuf::from(&session.project.assets[&tracks[0].asset].path);
    assert_eq!(taken.extension().and_then(|ext| ext.to_str()), Some("idx"));
    // The same units at the same times: the `.sub` as DiscCraft writes those units, the times in the `.idx`.
    assert_eq!(std::fs::read(taken.with_extension("sub")).unwrap(), sub);
    let text = std::fs::read_to_string(&taken).unwrap();
    let times: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("timestamp: "))
        .collect();
    assert_eq!(
        times,
        [
            "00:00:01:000, filepos: 000000000",
            "00:00:02:500, filepos: 000000800"
        ]
    );
    assert!(text.contains("palette: 000000, ffffff, 808080"), "{text}");

    // M6.546: a Blu-ray takes it too (its build makes each subpicture PGS).
    let (session, timeline) = timeline_of(&dir.join("bd"), &video, true);
    let tracks = &session
        .project
        .timeline(disccraft_project::TimelineId(timeline))
        .unwrap()
        .subtitles;
    assert_eq!(tracks.len(), 1);
    assert!(
        session.project.assets[&tracks[0].asset]
            .path
            .ends_with(".idx")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// M6.547: an MP4's VobSub (`mp4s`; here one DiscCraft wrote and ffmpeg put into an MP4) is taken out as a `.idx` +
/// `.sub` with the same subpicture units at the same times, and the colours the MP4 keeps as `00 Y Cr Cb`.
#[test]
fn an_mp4s_vobsub_becomes_the_timelines() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-mp4-vobsub-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let unit = |width: u16, x: u16| {
        let bitmap = disccraft_spu::Bitmap {
            width,
            height: 10,
            pixels: (0..usize::from(width) * 10)
                .map(|at| u8::try_from(at % 3 + 1).unwrap())
                .collect(),
        };
        let params = disccraft_spu::SpuParams {
            x,
            y: 400,
            colors: [0, 1, 2, 3],
            alpha: [0, 15, 15, 15],
            forced: false,
            stop_delay: Some(disccraft_spu::spu_delay_from_90k(90_000)),
        };
        disccraft_spu::encode_spu(&bitmap, &params).unwrap()
    };
    let units = vec![(1.0, unit(40, 100)), (2.5, unit(600, 20))];
    let header = "# VobSub index file, v7 (do not modify this line!)\nsize: 720x480\npalette: 000000, ffffff, \
                  ff0000, 0000ff, 000000, 000000, 000000, 000000, 000000, 000000, 000000, 000000, 000000, 000000, \
                  000000, 00ff00\n";
    let (idx, sub) = disccraft_transcode::vobsub_files(header, Some("en"), &units);
    std::fs::write(dir.join("en.idx"), idx).unwrap();
    std::fs::write(dir.join("en.sub"), &sub).unwrap();
    let video = dir.join("vobsub.mp4");
    let idx_path = dir.join("en.idx");
    let args: Vec<&std::ffi::OsStr> = [
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=720x480:rate=30000/1001",
        "-i",
    ]
    .iter()
    .map(std::ffi::OsStr::new)
    .chain([idx_path.as_os_str()])
    .chain(
        [
            "-map", "0:v", "-map", "1:s", "-t", "4", "-c:v", "libx264", "-pix_fmt", "yuv420p",
            "-c:s", "copy",
        ]
        .iter()
        .map(std::ffi::OsStr::new),
    )
    .chain([video.as_os_str()])
    .collect();
    assert!(run(&ffmpeg, &args));
    let found = disccraft_transcode::embedded_subtitles(&video);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].format, disccraft_transcode::EmbeddedFormat::VobSub);

    let (session, timeline) = timeline_of(&dir, &video, false);
    let tracks = session
        .project
        .timeline(disccraft_project::TimelineId(timeline))
        .unwrap()
        .subtitles
        .clone();
    assert_eq!(tracks.len(), 1);
    let taken = std::path::PathBuf::from(&session.project.assets[&tracks[0].asset].path);
    assert_eq!(taken.extension().and_then(|ext| ext.to_str()), Some("idx"));
    assert_eq!(std::fs::read(taken.with_extension("sub")).unwrap(), sub);
    let text = std::fs::read_to_string(&taken).unwrap();
    let times: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("timestamp: "))
        .collect();
    assert_eq!(
        times,
        [
            "00:00:01:000, filepos: 000000000",
            "00:00:02:500, filepos: 000000800"
        ]
    );
    // The colours come back within a step or two of rounding.
    let palette: Vec<[i32; 3]> = text
        .lines()
        .find_map(|line| line.strip_prefix("palette: "))
        .unwrap()
        .split(", ")
        .map(|colour| {
            let value = i32::from_str_radix(colour, 16).unwrap();
            [value >> 16, (value >> 8) & 255, value & 255]
        })
        .collect();
    let near =
        |got: [i32; 3], want: [i32; 3]| got.iter().zip(want).all(|(a, b)| (a - b).abs() <= 3);
    assert!(near(palette[1], [255, 255, 255]), "{palette:?}");
    assert!(near(palette[2], [255, 0, 0]), "{palette:?}");
    assert!(near(palette[3], [0, 0, 255]), "{palette:?}");
    assert!(near(palette[15], [0, 255, 0]), "{palette:?}");
    assert!(text.contains("size: 720x480"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}
