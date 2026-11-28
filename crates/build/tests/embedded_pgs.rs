//! M6.422 check: a Blu-ray's PGS subtitles inside an MKV are taken out as a `.sup` that has the same segments at the
//! same times as the one ffmpeg (the outside oracle) takes out of the same file, and a Blu-ray built with it carries
//! them. The PGS comes from a Blu-ray DiscCraft builds with text subtitles, copied into an MKV by ffmpeg. Skipped
//! without ffmpeg / ffprobe.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_project::{DiscKind, Link, Project, SubtitleTrack};
use disccraft_test_support::{ffmpeg, ffprobe};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A Blu-ray of `video` with the subtitle asset file `subtitles` (language `ja`), built into `out`; its title's clip.
fn bluray_with(video: &Path, subtitles: &Path, out: &Path) -> PathBuf {
    let mut project = Project::default();
    project.settings.kind = DiscKind::BluRay;
    let asset = project.add_asset(video).unwrap();
    let timeline = project
        .add_timeline(asset, Some("Main".to_owned()))
        .unwrap();
    let sub = project.add_asset(subtitles).unwrap();
    project
        .timeline_mut(timeline)
        .unwrap()
        .subtitles
        .push(SubtitleTrack {
            asset: sub,
            language: "ja".to_owned(),
            group: 0,
        });
    project.settings.first_play = Link::Timeline {
        id: timeline,
        chapter: 0,
    };
    let _ = std::fs::remove_dir_all(out);
    build(
        &project,
        &BuildOptions {
            output: OutputKind::Folder,
            path: out.to_path_buf(),
            cache: Some(out.with_extension("cache")),
            audio_language: Some("ja".to_owned()),
        },
    )
    .unwrap();
    // The title's clip: the largest stream file.
    std::fs::read_dir(out.join("BDMV").join("STREAM"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .max_by_key(|path| std::fs::metadata(path).unwrap().len())
        .unwrap()
}

/// The packets' bytes in ffprobe's `-show_packets -show_data` output (hex dump lines `00000000: 0046 0028 ...  text`).
fn packet_data(dump: &str) -> Vec<Vec<u8>> {
    let mut packets = Vec::new();
    for line in dump.lines() {
        if line == "[PACKET]" {
            packets.push(Vec::new());
        }
        let Some((offset, rest)) = line.split_once(": ") else {
            continue;
        };
        if offset.len() != 8 || !offset.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let hex: String = rest
            .split("  ")
            .next()
            .unwrap()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let bytes =
            (0..hex.len() / 2).map(|at| u8::from_str_radix(&hex[2 * at..2 * at + 2], 16).unwrap());
        packets.last_mut().unwrap().extend(bytes);
    }
    packets
}

/// The (presentation time, segment) pairs of a `.sup`.
fn segments(sup: &[u8]) -> Vec<(u32, Vec<u8>)> {
    let mut found = Vec::new();
    let mut at = 0;
    while at + 13 <= sup.len() {
        assert_eq!(&sup[at..at + 2], b"PG");
        let pts = u32::from_be_bytes(sup[at + 2..at + 6].try_into().unwrap());
        let length = usize::from(u16::from_be_bytes([sup[at + 11], sup[at + 12]]));
        found.push((pts, sup[at + 10..at + 13 + length].to_vec()));
        at += 13 + length;
    }
    assert_eq!(at, sup.len());
    found
}

#[test]
fn an_mkvs_pgs_comes_out_as_the_sup_ffmpeg_takes_out() {
    let (Some(ffmpeg), Some(ffprobe)) = (ffmpeg(), ffprobe()) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-embedded-pgs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let video = dir.join("video.m2v");
    let ok = Command::new(&ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=720x480:rate=30000/1001",
        ])
        .args([
            "-t",
            "4",
            "-c:v",
            "mpeg2video",
            "-b:v",
            "4M",
            "-maxrate",
            "8M",
            "-bufsize",
            "1835k",
        ])
        .args([
            "-g",
            "15",
            "-bf",
            "2",
            "-pix_fmt",
            "yuv420p",
            "-aspect",
            "4:3",
            "-f",
            "mpeg2video",
        ])
        .arg(&video)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let srt = dir.join("ja.srt");
    std::fs::write(
        &srt,
        "1\n00:00:00,500 --> 00:00:01,500\nこんにちは\n\n2\n00:00:02,000 --> 00:00:03,000\n字幕です\n\n",
    )
    .unwrap();
    let clip = bluray_with(&video, &srt, &dir.join("first"));

    // ffmpeg copies the PGS into an MKV, and takes it out of the MKV as a .sup.
    let mkv = dir.join("pgs.mkv");
    let reference = dir.join("ffmpeg.sup");
    for (input, maps, output) in [
        (&clip, ["-map", "0:v", "-map", "0:s"].as_slice(), &mkv),
        (&mkv, ["-map", "0:s"].as_slice(), &reference),
    ] {
        let ok = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-i"])
            .arg(input)
            .args(maps)
            .args(["-c", "copy", "-metadata:s:s:0", "language=jpn"])
            .arg(output)
            .status()
            .unwrap()
            .success();
        assert!(ok, "ffmpeg could not copy {}", input.display());
    }

    let embedded = disccraft_transcode::embedded_subtitles(&mkv);
    assert_eq!(embedded.len(), 1, "{embedded:?}");
    assert_eq!(embedded[0].format, disccraft_transcode::EmbeddedFormat::Pgs);
    assert_eq!(embedded[0].language.as_deref(), Some("jpn"));
    let ours = disccraft_transcode::extract_embedded_subtitle(&mkv, 0, &dir.join("out")).unwrap();
    assert_eq!(ours.extension().unwrap(), "sup");
    let ours_segments = segments(&std::fs::read(&ours).unwrap());
    let reference_segments = segments(&std::fs::read(&reference).unwrap());
    assert!(!ours_segments.is_empty());
    assert_eq!(
        ours_segments, reference_segments,
        "the same segments at the same times"
    );

    // A Blu-ray built with the .sup taken out carries the subtitles.
    let again = bluray_with(&mkv, &ours, &dir.join("second"));
    let probe = Command::new(&ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "compact",
        ])
        .arg(&again)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    assert!(text.contains("hdmv_pgs_subtitle"), "{text}");
    // The picture decodes; the subtitles decode too (ffmpeg draws them to re-encode them as a DVD's).
    for maps in [
        ["-map", "0:v", "-f", "null"].as_slice(),
        ["-map", "0:s", "-c:s", "dvdsub", "-f", "null"].as_slice(),
    ] {
        let decode = Command::new(&ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(&again)
            .args(maps)
            .arg("-")
            .output()
            .unwrap();
        assert!(
            decode.status.success() && decode.stderr.is_empty(),
            "{maps:?}: {}",
            String::from_utf8_lossy(&decode.stderr)
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// M6.546: a DVD's subtitles (VobSub) on a Blu-ray: the clip carries them as a PGS stream (ffmpeg, the outside oracle,
/// finds it and decodes it without an error), M6.551: a forced unit as a forced one.
#[test]
fn a_vobsub_goes_on_a_blu_ray_as_pgs() {
    let (Some(ffmpeg), Some(ffprobe)) = (ffmpeg(), ffprobe()) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-vobsub-bd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let video = dir.join("video.m2v");
    let ok = Command::new(&ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=720x480:rate=30000/1001",
        ])
        .args([
            "-t",
            "4",
            "-c:v",
            "mpeg2video",
            "-b:v",
            "4M",
            "-maxrate",
            "8M",
            "-bufsize",
            "1835k",
        ])
        .args([
            "-g",
            "15",
            "-bf",
            "2",
            "-pix_fmt",
            "yuv420p",
            "-aspect",
            "4:3",
            "-f",
            "mpeg2video",
        ])
        .arg(&video)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    // Two subpictures, a second each; the first forced (M6.551).
    let unit = |x: u16| {
        let bitmap = disccraft_spu::Bitmap {
            width: 60,
            height: 12,
            pixels: (0..720).map(|at| if at % 60 < 4 { 2 } else { 1 }).collect(),
        };
        let params = disccraft_spu::SpuParams {
            x,
            y: 400,
            colors: [0, 1, 2, 3],
            alpha: [0, 15, 15, 0],
            forced: x == 100,
            stop_delay: Some(disccraft_spu::spu_delay_from_90k(90_000)),
        };
        disccraft_spu::encode_spu(&bitmap, &params).unwrap()
    };
    let header = "size: 720x480\npalette: 000000, ffffff, 202020, 000000, 000000, 000000, 000000, 000000, \
                  000000, 000000, 000000, 000000, 000000, 000000, 000000, 000000\n";
    let (idx, sub) = disccraft_transcode::vobsub_files(
        header,
        Some("ja"),
        &[(0.5, unit(100)), (2.0, unit(300))],
    );
    let idx_path = dir.join("subs.idx");
    std::fs::write(&idx_path, idx).unwrap();
    std::fs::write(idx_path.with_extension("sub"), sub).unwrap();
    let clip = bluray_with(&video, &idx_path, &dir.join("bd"));
    let probe = Command::new(&ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "compact",
        ])
        .arg(&clip)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    assert!(text.contains("hdmv_pgs_subtitle"), "{text}");
    let decode = Command::new(&ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(&clip)
        .args(["-map", "0:s", "-c:s", "dvdsub", "-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decode.status.success() && decode.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&decode.stderr)
    );
    // M6.551: ffmpeg reads the PGS's forced flag: made into DVD subpictures, the first starts with the forced start
    // command and the second with the ordinary one.
    let dvdsub = dir.join("dvdsub.mkv");
    let ok = Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-i"])
        .arg(&clip)
        .args(["-map", "0:s", "-c:s", "dvdsub"])
        .arg(&dvdsub)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let dump = Command::new(&ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "s",
            "-show_packets",
            "-show_data",
        ])
        .arg(&dvdsub)
        .output()
        .unwrap();
    let forced: Vec<bool> = packet_data(&String::from_utf8_lossy(&dump.stdout))
        .iter()
        .map(|unit| disccraft_spu::decode_spu(unit).unwrap().params.forced)
        .collect();
    assert_eq!(forced, [true, false]);
    let _ = std::fs::remove_dir_all(&dir);
}
