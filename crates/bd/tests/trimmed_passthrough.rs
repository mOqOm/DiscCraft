//! M10.21 check: a clip of H.264 that goes on an HD Blu-ray as it is can be trimmed at its key pictures. The
//! trimmed title holds the very pictures of that part of the file, and the part of the file's sound that goes
//! with them (ffmpeg and ffprobe as outside oracles). Skipped when ffmpeg is not available.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_bd::build_bdmv;
use disccraft_project::{BdVideo, ClipTrim, DiscKind, Link, Project};
use disccraft_test_support::{ffmpeg, ffprobe, fixtures};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Three seconds of interlaced 1080i H.264 with a key picture every second (closed GOPs, B pictures), and a tone
/// that is 440 Hz in the first second, 880 Hz in the second and 1320 Hz in the third.
fn fixture(ffmpeg: &Path, container: &str) -> Option<PathBuf> {
    let video = fixtures().join(format!("h264_1080i_tones.{container}"));
    if video.is_file() {
        return Some(video);
    }
    std::fs::create_dir_all(fixtures()).unwrap();
    let mut command = Command::new(ffmpeg);
    let _ = command
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=1920x1080:rate=30000/1001")
        .args(["-f", "lavfi", "-i"])
        .arg("aevalsrc=sin(2*PI*440*(1+floor(t))*t):sample_rate=48000")
        .args(["-t", "3", "-c:v", "libx264", "-preset", "ultrafast"])
        .args(["-profile:v", "high", "-level", "4.1", "-pix_fmt", "yuv420p"])
        .args(["-b:v", "6M", "-flags", "+ildct+ilme", "-x264-params"])
        .args(["tff=1:bframes=2:keyint=30:min-keyint=30:scenecut=0:aud=1:8x8dct=1:vbv-maxrate=20000:vbv-bufsize=20000"])
        .args(["-aspect", "16:9"]);
    let _ = if container == "m2ts" {
        command.args(["-c:a", "ac3", "-b:a", "192k", "-f", "mpegts"]);
        // 192-byte packets, as on a Blu-ray.
        command.args(["-mpegts_m2ts_mode", "1"])
    } else {
        command.args(["-c:a", "aac", "-b:a", "128k"])
    };
    let made = command.arg(&video).status();
    if !made.is_ok_and(|status| status.success()) {
        let _ = std::fs::remove_file(&video);
        return None;
    }
    Some(video)
}

/// The decoded pictures of `file`'s video as one MD5, through the video filter `filter` when given.
fn video_md5(ffmpeg: &Path, file: &Path, filter: Option<&str>) -> String {
    let mut command = Command::new(ffmpeg);
    let _ = command
        .args(["-v", "error", "-i"])
        .arg(file)
        .args(["-map", "0:v:0"]);
    if let Some(filter) = filter {
        let _ = command.args(["-vf", filter, "-fps_mode", "passthrough"]);
    }
    let output = command.args(["-f", "md5", "-"]).output().unwrap();
    assert!(output.status.success(), "{file:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// The first sound of `file` as 16-bit mono samples at 48 kHz.
fn samples(ffmpeg: &Path, file: &Path) -> Vec<i16> {
    let output = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(file)
        .args([
            "-map", "0:a:0", "-ac", "1", "-ar", "48000", "-f", "s16le", "-",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{file:?}");
    output
        .stdout
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair))
        .collect()
}

/// The pitch of a tone, from how often it crosses zero upwards.
fn pitch(samples: &[i16]) -> f64 {
    let crossings = samples
        .array_windows()
        .filter(|[a, b]| *a < 0 && *b >= 0)
        .count();
    crossings as f64 * 48_000.0 / samples.len().max(1) as f64
}

fn check(container: &str) {
    let (Some(ffmpeg), Some(ffprobe)) = (ffmpeg(), ffprobe()) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let Some(video) = fixture(&ffmpeg, container) else {
        eprintln!("ffmpeg cannot make interlaced H.264 (no libx264?); skipping");
        return;
    };
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    p.settings.bd_video = BdVideo::Hd1080i59;
    let v = p.add_asset(&video).unwrap();
    let t = p.add_timeline(v, Some("Main".to_string())).unwrap();
    p.settings.first_play = Link::Timeline { id: t, chapter: 0 };
    // The second of the three seconds: from the key picture on frame 30 up to the one on frame 60.
    p.timeline_mut(t).unwrap().video_trims = vec![ClipTrim {
        start: 30,
        end: Some(60),
    }];

    // The estimate keeps a third of the file and encodes nothing (a transport stream's length is not read from
    // its header, so an interlaced one has no estimate).
    if container != "m2ts" {
        let estimate = disccraft_dvd::estimate(&p).unwrap();
        let size = std::fs::metadata(&video).unwrap().len();
        assert!(
            estimate.passthrough_bytes > size / 4 && estimate.passthrough_bytes < size / 2,
            "{estimate:?}"
        );
        assert!(estimate.transcode_seconds.abs() < 1e-9, "{estimate:?}");
    }

    let out = fixtures().join(format!("bd_trimmed_{container}_out"));
    let _ = std::fs::remove_dir_all(&out);
    let report = build_bdmv(&p, &out).unwrap();
    assert_eq!(report.titles.len(), 1);
    // Nothing was encoded: no HD stream of the clip in the cache.
    let encoded: Vec<String> = std::fs::read_dir(out.join("disccraft-cache"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("hd-"))
        .collect();
    assert!(encoded.is_empty(), "{encoded:?}");

    let stream = out.join("BDMV/STREAM/00001.m2ts");
    let probe = Command::new(&ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-count_packets"])
        .args(["-show_entries"])
        .args(["stream=codec_name,width,height,nb_read_packets"])
        .args(["-of", "csv=p=0"])
        .arg(&stream)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    assert_eq!(
        text.lines().next().unwrap_or_default().trim(),
        "h264,1920,1080,30",
        "{text}"
    );
    // Its frames are interlaced, with the field order of the file's.
    let fields = |file: &Path| -> Vec<String> {
        let output = Command::new(&ffprobe)
            .args(["-v", "error", "-select_streams", "v:0", "-show_entries"])
            .args(["frame=interlaced_frame,top_field_first", "-of", "csv=p=0"])
            .arg(file)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| line.trim().trim_end_matches(',').to_string())
            .collect()
    };
    let (ours, theirs) = (fields(&stream), fields(&video));
    assert_eq!(ours.len(), 30);
    assert_eq!(ours.as_slice(), theirs.get(30..60).unwrap());
    assert!(ours.iter().all(|frame| frame.starts_with('1')), "{ours:?}");
    // Decodes without an error, to the pictures of the file's second second.
    let decode = Command::new(&ffmpeg)
        .args(["-v", "error", "-xerror", "-err_detect", "explode", "-i"])
        .arg(&stream)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decode.status.success() && decode.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&decode.stderr)
    );
    assert_eq!(
        video_md5(&ffmpeg, &stream, None),
        video_md5(&ffmpeg, &video, Some("select=between(n\\,30\\,59)"))
    );
    assert_ne!(
        video_md5(&ffmpeg, &stream, None),
        video_md5(&ffmpeg, &video, Some("select=between(n\\,0\\,29)"))
    );

    // The sound is the second second's too: a second of the 880 Hz tone.
    let sound = samples(&ffmpeg, &stream);
    let seconds = sound.len() as f64 / 48_000.0;
    assert!((seconds - 1.0).abs() < 0.08, "{seconds}");
    // The middle of it (clear of the encoder's delay at either end).
    let middle = sound.get(9_600..38_400).unwrap();
    let pitch = pitch(middle);
    assert!((pitch - 880.0).abs() < 15.0, "{pitch}");

    // An in point inside a GOP cannot be cut there: such a clip is encoded. Since craft-video decodes interlaced
    // H.264 (field pictures and MBAFF, M12.25), that works: the title is the disc's own 1080i video (MPEG-2) of
    // the frames from 45 on, and it decodes without errors.
    p.timeline_mut(t).unwrap().video_trims = vec![ClipTrim {
        start: 45,
        end: None,
    }];
    let _ = build_bdmv(&p, &out).unwrap();
    let probe = Command::new(&ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-count_packets"])
        .args([
            "-show_entries",
            "stream=codec_name,width,height,nb_read_packets",
        ])
        .args(["-of", "csv=p=0"])
        .arg(&stream)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    // 45 pictures (46 from an MKV, whose millisecond times round its length up by a frame).
    let line = text
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .trim_end_matches(',');
    assert!(
        ["mpeg2video,1920,1080,45", "mpeg2video,1920,1080,46"].contains(&line),
        "{text}"
    );
    let decode = Command::new(&ffmpeg)
        .args(["-v", "error", "-xerror", "-i"])
        .arg(&stream)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decode.status.success() && decode.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&decode.stderr)
    );
    if std::env::var_os("DISCCRAFT_KEEP_OUTPUT").is_none() {
        let _ = std::fs::remove_dir_all(&out);
    }
}

#[test]
fn a_trimmed_clip_in_an_mkv_goes_on_the_disc_as_it_is() {
    check("mkv");
}

#[test]
fn a_trimmed_clip_in_an_mp4_goes_on_the_disc_as_it_is() {
    check("mp4");
}

/// M12.22: a transport stream's interlaced H.264 (which the media reader refuses) is told by its index and its
/// sequence parameter set, so it is imported, goes on the disc as it is, and is trimmed like the others.
#[test]
fn a_trimmed_clip_in_a_transport_stream_goes_on_the_disc_as_it_is() {
    check("m2ts");
}

/// M12.22: the same file used whole. Its length, size, rate and sound are told without decoding it, and the
/// title holds all its pictures and its AC-3 as it is.
#[test]
fn an_interlaced_transport_stream_is_imported_and_goes_on_the_disc_as_it_is() {
    let (Some(ffmpeg), Some(ffprobe)) = (ffmpeg(), ffprobe()) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let Some(video) = fixture(&ffmpeg, "m2ts") else {
        eprintln!("ffmpeg cannot make interlaced H.264 (no libx264?); skipping");
        return;
    };
    let probe = disccraft_transcode::probe_video(&video).unwrap();
    assert_eq!((probe.width, probe.height), (1920, 1080));
    assert!((probe.fps - 30_000.0 / 1001.0).abs() < 1e-9, "{probe:?}");
    assert!((probe.seconds - 3.0).abs() < 0.05, "{probe:?}");
    assert!(probe.wide && probe.has_audio, "{probe:?}");
    let sound = disccraft_transcode::probe_audio_seconds(&video).unwrap();
    assert!((sound - 3.0).abs() < 0.1, "{sound}");

    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    p.settings.bd_video = BdVideo::Hd1080i59;
    let v = p.add_asset(&video).unwrap();
    let t = p.add_timeline(v, Some("Main".to_string())).unwrap();
    p.settings.first_play = Link::Timeline { id: t, chapter: 0 };
    let estimate = disccraft_dvd::estimate(&p).unwrap();
    assert!(estimate.passthrough_bytes > 0, "{estimate:?}");
    assert!(estimate.transcode_seconds.abs() < 1e-9, "{estimate:?}");

    let out = fixtures().join("bd_whole_m2ts_out");
    let _ = std::fs::remove_dir_all(&out);
    let report = build_bdmv(&p, &out).unwrap();
    assert_eq!(report.titles.len(), 1);
    let stream = out.join("BDMV/STREAM/00001.m2ts");
    let probe = Command::new(&ffprobe)
        .args(["-v", "error", "-count_packets", "-show_entries"])
        .args(["stream=codec_name,nb_read_packets", "-of", "csv=p=0"])
        .arg(&stream)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    let streams: Vec<&str> = text.lines().map(str::trim).collect();
    assert!(streams.contains(&"h264,90"), "{text}");
    assert!(
        streams.iter().any(|line| line.starts_with("ac3,")),
        "{text}"
    );
    let decode = Command::new(&ffmpeg)
        .args(["-v", "error", "-xerror", "-err_detect", "explode", "-i"])
        .arg(&stream)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decode.status.success() && decode.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&decode.stderr)
    );
    assert_eq!(
        video_md5(&ffmpeg, &stream, None),
        video_md5(&ffmpeg, &video, None)
    );
    // The file's AC-3 frames themselves.
    let copy = |file: &Path| {
        Command::new(&ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(file)
            .args(["-map", "0:a:0", "-c:a", "copy", "-f", "ac3", "-"])
            .output()
            .unwrap()
            .stdout
    };
    let (ours, theirs) = (copy(&stream), copy(&video));
    assert!(ours.len() > 60_000, "{}", ours.len());
    assert!(
        theirs
            .windows(ours.len().min(4096))
            .any(|part| Some(part) == ours.get(..part.len()))
    );
    if std::env::var_os("DISCCRAFT_KEEP_OUTPUT").is_none() {
        let _ = std::fs::remove_dir_all(&out);
    }
}

/// M10.22: HD MPEG-2 in a transport stream (an HDV camcorder's 1440 x 1080i, or 1920 x 1080i as broadcast) goes
/// on a 1080i disc as it is: the same pictures, declared MPEG-2, with the file's AC-3.
#[test]
fn hd_mpeg2_in_a_transport_stream_goes_on_the_disc_as_it_is() {
    let (Some(ffmpeg), Some(ffprobe)) = (ffmpeg(), ffprobe()) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    for (width, level) in [(1440_u32, "6"), (1920, "4")] {
        let video = fixtures().join(format!("mpeg2_{width}x1080i.m2ts"));
        if !video.is_file() {
            std::fs::create_dir_all(fixtures()).unwrap();
            let made = Command::new(&ffmpeg)
                .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
                .arg(format!("testsrc2=size={width}x1080:rate=30000/1001"))
                .args(["-f", "lavfi", "-i"])
                .arg("sine=frequency=440:sample_rate=48000")
                .args(["-t", "3", "-c:v", "mpeg2video", "-pix_fmt", "yuv420p"])
                // Main profile at High-1440 (6) or High (4) level, interlaced, two B pictures, a GOP of 15.
                .args(["-profile:v", "4", "-level:v", level])
                .args(["-b:v", "18M", "-maxrate", "25M", "-bufsize", "9M"])
                .args(["-vf", "setfield=tff", "-flags", "+ildct+ilme"])
                .args(["-bf", "2", "-g", "15"])
                .args(["-aspect", "16:9", "-ac", "2", "-c:a", "ac3", "-b:a", "192k"])
                .args(["-f", "mpegts", "-mpegts_m2ts_mode", "1"])
                .arg(&video)
                .status();
            if !made.is_ok_and(|status| status.success()) {
                let _ = std::fs::remove_file(&video);
                eprintln!("ffmpeg cannot make the fixture; skipping");
                return;
            }
        }
        let mut p = Project::default();
        p.settings.kind = DiscKind::BluRay;
        p.settings.bd_video = BdVideo::Hd1080i59;
        let v = p.add_asset(&video).unwrap();
        let t = p.add_timeline(v, Some("Main".to_string())).unwrap();
        p.settings.first_play = Link::Timeline { id: t, chapter: 0 };
        // Nothing to encode: the estimate counts the file as it is.
        let estimate = disccraft_dvd::estimate(&p).unwrap();
        assert!(estimate.passthrough_bytes > 0, "{width}: {estimate:?}");
        assert!(
            estimate.transcode_seconds.abs() < 1e-9,
            "{width}: {estimate:?}"
        );

        let out = fixtures().join(format!("bd_mpeg2_{width}_out"));
        let _ = std::fs::remove_dir_all(&out);
        let report = build_bdmv(&p, &out).unwrap();
        assert_eq!(report.titles.len(), 1);
        let encoded: Vec<String> = std::fs::read_dir(out.join("disccraft-cache"))
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("hd-") || name.ends_with(".m2v"))
            .collect();
        // Nothing encoded, and (M6.380) the video not copied into the cache: read from the file itself.
        assert!(encoded.is_empty(), "{encoded:?}");

        let stream = out.join("BDMV/STREAM/00001.m2ts");
        let probe = Command::new(&ffprobe)
            .args(["-v", "error", "-count_packets", "-show_entries"])
            .args([
                "stream=codec_name,width,height,nb_read_packets",
                "-of",
                "csv=p=0",
            ])
            .arg(&stream)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&probe.stdout);
        let streams: Vec<&str> = text
            .lines()
            .map(|line| line.trim().trim_end_matches(','))
            .collect();
        assert!(
            streams.contains(&format!("mpeg2video,{width},1080,90").as_str()),
            "{text}"
        );
        assert!(
            streams.iter().any(|line| line.starts_with("ac3,")),
            "{text}"
        );
        let decode = Command::new(&ffmpeg)
            .args(["-v", "error", "-xerror", "-err_detect", "explode", "-i"])
            .arg(&stream)
            .args(["-f", "null", "-"])
            .output()
            .unwrap();
        assert!(
            decode.status.success() && decode.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&decode.stderr)
        );
        assert_eq!(
            video_md5(&ffmpeg, &stream, None),
            video_md5(&ffmpeg, &video, None),
            "{width}"
        );
        // The clip says MPEG-2, 1080i, 29.97, 16:9.
        let info = std::fs::read(out.join("BDMV/CLIPINF/00001.clpi")).unwrap();
        let text = format!("{:?}", disccraft_bdmv::clpi::read_clpi(&info).unwrap());
        assert!(
            text.contains("Mpeg2Video { video_format: 4, frame_rate: 4, aspect: 3 }"),
            "{text}"
        );
        // On a disc of another format it is encoded like any other video (it can be decoded).
        p.settings.bd_video = BdVideo::Hd720p59;
        let estimate = disccraft_dvd::estimate(&p).unwrap();
        assert!(estimate.transcode_seconds > 2.0, "{estimate:?}");
        if std::env::var_os("DISCCRAFT_KEEP_OUTPUT").is_none() {
            let _ = std::fs::remove_dir_all(&out);
        }
    }
}

/// M6.370: HD MPEG-2 as an elementary stream (`.m2v`, 1920 x 1080i) goes on a 1080i disc as it is, read from the
/// file itself: nothing is encoded, only its index is in the cache, and the disc's pictures are the file's.
#[test]
fn an_hd_mpeg2_elementary_stream_goes_on_the_disc_as_it_is() {
    let (Some(ffmpeg), Some(ffprobe)) = (ffmpeg(), ffprobe()) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let video = fixtures().join("mpeg2_1920x1080i.m2v");
    if !video.is_file() {
        std::fs::create_dir_all(fixtures()).unwrap();
        let made = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc2=size=1920x1080:rate=30000/1001")
            .args(["-t", "3", "-c:v", "mpeg2video", "-pix_fmt", "yuv420p"])
            .args(["-profile:v", "4", "-level:v", "4"])
            .args(["-b:v", "18M", "-maxrate", "25M", "-bufsize", "9M"])
            .args(["-vf", "setfield=tff", "-flags", "+ildct+ilme"])
            .args([
                "-bf",
                "2",
                "-g",
                "15",
                "-aspect",
                "16:9",
                "-f",
                "mpeg2video",
            ])
            .arg(&video)
            .status();
        if !made.is_ok_and(|status| status.success()) {
            let _ = std::fs::remove_file(&video);
            eprintln!("ffmpeg cannot make the fixture; skipping");
            return;
        }
    }
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    p.settings.bd_video = BdVideo::Hd1080i59;
    let v = p.add_asset(&video).unwrap();
    let t = p.add_timeline(v, Some("Main".to_string())).unwrap();
    p.settings.first_play = Link::Timeline { id: t, chapter: 0 };
    let estimate = disccraft_dvd::estimate(&p).unwrap();
    assert!(estimate.transcode_seconds.abs() < 1e-9, "{estimate:?}");

    let out = fixtures().join("bd_mpeg2_es_out");
    let _ = std::fs::remove_dir_all(&out);
    let report = build_bdmv(&p, &out).unwrap();
    assert_eq!(report.titles.len(), 1);
    let cache: Vec<(String, u64)> = std::fs::read_dir(out.join("disccraft-cache"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                entry.metadata().map_or(0, |meta| meta.len()),
            )
        })
        .collect();
    // No encode, and no copy of the video: the cache holds a small index.
    assert!(
        cache
            .iter()
            .all(|(name, size)| !name.starts_with("hd-") && *size < 100_000),
        "{cache:?}"
    );
    let stream = out.join("BDMV/STREAM/00001.m2ts");
    let probe = Command::new(&ffprobe)
        .args(["-v", "error", "-count_packets", "-show_entries"])
        .args([
            "stream=codec_name,width,height,nb_read_packets",
            "-of",
            "csv=p=0",
        ])
        .arg(&stream)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    assert!(text.contains("mpeg2video,1920,1080,90"), "{text}");
    assert_eq!(
        video_md5(&ffmpeg, &stream, None),
        video_md5(&ffmpeg, &video, None)
    );
    if std::env::var_os("DISCCRAFT_KEEP_OUTPUT").is_none() {
        let _ = std::fs::remove_dir_all(&out);
    }
}

/// The frames of `file`'s first video stream as MD5s, in the order they are shown.
fn frame_md5s(ffmpeg: &Path, file: &Path) -> Vec<String> {
    let output = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(file)
        .args(["-map", "0:v:0", "-f", "framemd5", "-"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{file:?}");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.rsplit(',').next().map(|hash| hash.trim().to_string()))
        .collect()
}

/// M6.385: HD MPEG-2 of the disc's size goes on a 1080i title as it is beside another clip, and trimmed at its
/// GOPs: two clips of the 1920 x 1080i transport stream, the first cut to its GOPs from frame 15 to 60, decode
/// without errors as the source's pictures, with nothing encoded.
#[test]
fn hd_mpeg2_is_trimmed_and_joined_as_it_is() {
    trimmed_and_joined(1920);
}

/// M6.388: the same with HDV's 1440 x 1080i: clips of one narrower width, back to back, are joined as they are too.
#[test]
fn hdv_clips_are_trimmed_and_joined_as_they_are() {
    trimmed_and_joined(1440);
}

fn trimmed_and_joined(width: u32) {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let video = fixtures().join(format!("mpeg2_{width}x1080i.m2ts"));
    if !video.is_file() {
        eprintln!("mpeg2_{width}x1080i.m2ts fixture not found (made by the M10.22 test); skipping");
        return;
    }
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    p.settings.bd_video = BdVideo::Hd1080i59;
    let v = p.add_asset(&video).unwrap();
    let t = p.add_timeline(v, Some("Joined".to_string())).unwrap();
    // The first clip from frame 15 to 60 (GOPs of 15), then the whole file again.
    let frame = 8_475_667_200_i64;
    let _ = p
        .append_timeline_video(t, v, disccraft_project::Tick(45 * frame))
        .unwrap();
    if let Some(timeline) = p.timeline_mut(t) {
        timeline.video_trims = vec![disccraft_project::ClipTrim {
            start: 15,
            end: Some(60),
        }];
    }
    p.settings.first_play = Link::Timeline { id: t, chapter: 0 };
    // The timeline trims it at its GOPs: a key picture every 15 frames, each one a clip can start on.
    let cut = disccraft_dvd::bd_cut_points(&p, &video, &std::env::temp_dir())
        .unwrap()
        .expect("no cut points");
    let keys = cut.frames();
    assert_eq!(keys.get(..4), Some(&[0, 15, 30, 45][..]), "{keys:?}");
    assert!(cut.can_start(15));
    // Nothing to encode: the estimate counts both clips as they are.
    let estimate = disccraft_dvd::estimate(&p).unwrap();
    assert!(estimate.passthrough_bytes > 0, "{width}: {estimate:?}");
    assert!(
        estimate.transcode_seconds.abs() < 1e-9,
        "{width}: {estimate:?}"
    );
    let out = fixtures().join(format!("bd_mpeg2_{width}_joined_out"));
    let _ = std::fs::remove_dir_all(&out);
    let _ = build_bdmv(&p, &out).unwrap();
    let encoded: Vec<String> = std::fs::read_dir(out.join("disccraft-cache"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("hd-"))
        .collect();
    assert!(encoded.is_empty(), "{encoded:?}");

    let stream = out.join("BDMV/STREAM/00001.m2ts");
    let decode = Command::new(&ffmpeg)
        .args(["-v", "error", "-xerror", "-i"])
        .arg(&stream)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decode.status.success() && decode.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&decode.stderr)
    );
    // The pictures keep the file's width.
    let probe = disccraft_transcode::probe_video(&stream).unwrap();
    assert_eq!((probe.width, probe.height), (width, 1080));
    let source = frame_md5s(&ffmpeg, &video);
    let disc = frame_md5s(&ffmpeg, &stream);
    // The first part: the source's pictures from frame 15 on, as many as the GOPs up to frame 60 show (an open
    // GOP's last B pictures belong to the next GOP); then the whole source.
    let first = disc.len() - source.len();
    assert!((40..=45).contains(&first), "{first} of {}", disc.len());
    assert_eq!(disc[..first], source[15..15 + first]);
    assert_eq!(disc[first..], source[..]);
    if std::env::var_os("DISCCRAFT_KEEP_OUTPUT").is_none() {
        let _ = std::fs::remove_dir_all(&out);
    }
}

/// M6.388 / M6.393: which titles are made at HDV's 1440: those whose clips that go on as they are are more 1440 x 1080
/// than 1920 x 1080 (a gap and a clip with a Blu-ray preset of its own are encoded at 1440 beside them). One 1440 clip
/// beside one 1920 clip: the disc's width (the 1440 clip encoded).
#[test]
fn a_title_of_mostly_hdv_clips_is_made_at_1440() {
    let narrow = fixtures().join("mpeg2_1440x1080i.m2ts");
    let full = fixtures().join("mpeg2_1920x1080i.m2ts");
    if !narrow.is_file() || !full.is_file() {
        eprintln!(
            "mpeg2_1440x1080i.m2ts / mpeg2_1920x1080i.m2ts fixtures not found (made by the M10.22 test); skipping"
        );
        return;
    }
    let format = disccraft_dvd::BdVideoFormat {
        width: 1920,
        height: 1080,
        rate: (30_000, 1001),
        interlaced: true,
    };
    let three_seconds = disccraft_project::Tick(90 * 8_475_667_200_i64);
    let title = |second: &Path| {
        let mut p = Project::default();
        p.settings.kind = DiscKind::BluRay;
        p.settings.bd_video = BdVideo::Hd1080i59;
        let a = p.add_asset(&narrow).unwrap();
        let b = p.add_asset(second).unwrap();
        let t = p.add_timeline(a, Some("Main".to_string())).unwrap();
        let _ = p.append_timeline_video(t, b, three_seconds).unwrap();
        p.settings.first_play = Link::Timeline { id: t, chapter: 0 };
        (p, t, b)
    };
    let width =
        |p: &Project, t| disccraft_dvd::narrow_mpeg2_width(p, p.timeline(t).unwrap(), format);
    // Two clips of 1440: at 1440, nothing encoded.
    let (p, t, _) = title(&narrow);
    assert_eq!(width(&p, t), Some(1440));
    let estimate = disccraft_dvd::estimate(&p).unwrap();
    assert!(estimate.transcode_seconds.abs() < 1e-9, "{estimate:?}");
    // 1440 beside 1920: the disc's width, the 1440 clip encoded.
    let (p, t, _) = title(&full);
    assert_eq!(width(&p, t), None);
    let estimate = disccraft_dvd::estimate(&p).unwrap();
    assert!(estimate.transcode_seconds > 2.0, "{estimate:?}");
    // A gap in front of the second clip: still 1440 (the black is encoded at 1440).
    let (mut p, t, _) = title(&narrow);
    p.timeline_mut(t).unwrap().video_gaps = vec![0, 15];
    assert_eq!(width(&p, t), Some(1440));
    // A clip with a Blu-ray preset of its own is encoded, at 1440 beside the other.
    let (mut p, t, b) = title(&narrow);
    p.assets.get_mut(&b).unwrap().bluray_transcode = Some(disccraft_project::BlurayTranscode {
        kbps: 20_000,
        max_kbps: 30_000,
        preset: String::new(),
        quality: 5,
        mode: disccraft_project::TranscodeMode::default(),
    });
    assert_eq!(width(&p, t), Some(1440));
    // On a 720p disc: never.
    let (p, t, _) = title(&narrow);
    let small = disccraft_dvd::BdVideoFormat {
        width: 1280,
        height: 720,
        ..format
    };
    assert_eq!(
        disccraft_dvd::narrow_mpeg2_width(&p, p.timeline(t).unwrap(), small),
        None
    );
}

/// M6.393: an HDV clip beside an SD clip on a 1080i disc: the title is made at 1440 x 1080, the HDV clip's pictures as
/// they are and the SD clip encoded at 1440 (MPEG-2 MP@H-14, shown 16:9), and the whole stream decodes without errors.
#[test]
fn an_hdv_clip_joins_a_clip_encoded_at_1440() {
    let (Some(ffmpeg), Some(ffprobe)) = (ffmpeg(), ffprobe()) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let narrow = fixtures().join("mpeg2_1440x1080i.m2ts");
    let sd = fixtures().join("ntsc.m2v");
    if !narrow.is_file() || !sd.is_file() {
        eprintln!("mpeg2_1440x1080i.m2ts / ntsc.m2v fixtures not found; skipping");
        return;
    }
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    p.settings.bd_video = BdVideo::Hd1080i59;
    let a = p.add_asset(&narrow).unwrap();
    let b = p.add_asset(&sd).unwrap();
    let t = p.add_timeline(a, Some("Main".to_string())).unwrap();
    let _ = p
        .append_timeline_video(t, b, disccraft_project::Tick(599 * 8_475_667_200_i64))
        .unwrap();
    // The SD clip's first 26 frames (two GOPs of 13).
    p.timeline_mut(t).unwrap().video_trims = vec![
        ClipTrim::default(),
        ClipTrim {
            start: 0,
            end: Some(26),
        },
    ];
    p.settings.first_play = Link::Timeline { id: t, chapter: 0 };
    let out = fixtures().join("bd_hdv_mixed_out");
    let _ = std::fs::remove_dir_all(&out);
    let _ = build_bdmv(&p, &out).unwrap();
    let stream = out.join("BDMV/STREAM/00001.m2ts");
    // Every picture 1440 x 1080, shown 16:9.
    let probe = Command::new(&ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries"])
        .args(["frame=width,height,sample_aspect_ratio", "-of", "csv=p=0"])
        .arg(&stream)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    let frames: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    assert!(frames.len() >= 90 + 20, "{}", frames.len());
    assert!(
        frames.iter().all(|line| line.starts_with("1440,1080,4:3")),
        "{text}"
    );
    let decode = Command::new(&ffmpeg)
        .args(["-v", "error", "-xerror", "-i"])
        .arg(&stream)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decode.status.success() && decode.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&decode.stderr)
    );
    // The HDV clip's pictures are the file's.
    let source = frame_md5s(&ffmpeg, &narrow);
    let disc = frame_md5s(&ffmpeg, &stream);
    assert_eq!(disc[..source.len()], source[..]);
    // The SD clip (4:3) is pillarboxed in the 16:9 frame, not stretched: its last picture is black at both sides.
    let last = out.join("last.pgm");
    let made = Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-i"])
        .arg(&stream)
        .args([
            "-map", "0:v:0", "-pix_fmt", "gray", "-f", "image2", "-update", "1",
        ])
        .arg(&last)
        .status()
        .unwrap();
    assert!(made.success());
    let pgm = std::fs::read(&last).unwrap();
    let pixels = &pgm[pgm.len() - 1440 * 1080..];
    let mean = |x0: usize, x1: usize| {
        let (mut sum, mut count) = (0_u64, 0_u64);
        for row in pixels.chunks(1440).step_by(8) {
            sum += row[x0..x1].iter().map(|v| u64::from(*v)).sum::<u64>();
            count += (x1 - x0) as u64;
        }
        sum / count
    };
    assert!(
        mean(0, 170) < 20 && mean(1270, 1440) < 20,
        "sides not black"
    );
    assert!(mean(400, 1040) > 60, "no picture in the middle");
    if std::env::var_os("DISCCRAFT_KEEP_OUTPUT").is_none() {
        let _ = std::fs::remove_dir_all(&out);
    }
}

/// M6.386: HD MPEG-2 in a program stream (an `.mpg` / `.vob` of 1920 x 1080i) goes on a 1080i disc as it is, as in
/// a transport stream: nothing encoded, the source's pictures.
#[test]
fn hd_mpeg2_in_a_program_stream_goes_on_the_disc_as_it_is() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let source = fixtures().join("mpeg2_1920x1080i.m2ts");
    if !source.is_file() {
        eprintln!("mpeg2_1920x1080i.m2ts fixture not found (made by the M10.22 test); skipping");
        return;
    }
    let video = fixtures().join("mpeg2_1920x1080i_ps.mpg");
    if !video.is_file() {
        let made = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-i"])
            .arg(&source)
            .args(["-c", "copy", "-f", "vob"])
            .arg(&video)
            .status();
        if !made.is_ok_and(|status| status.success()) {
            let _ = std::fs::remove_file(&video);
            eprintln!("ffmpeg cannot make the fixture; skipping");
            return;
        }
    }
    let format = disccraft_dvd::BdVideoFormat::of_video(BdVideo::Hd1080i59).unwrap();
    assert!(disccraft_dvd::mpeg2_fits_bd(&video, format));
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    p.settings.bd_video = BdVideo::Hd1080i59;
    let v = p.add_asset(&video).unwrap();
    let t = p.add_timeline(v, Some("Main".to_string())).unwrap();
    p.settings.first_play = Link::Timeline { id: t, chapter: 0 };
    let out = fixtures().join("bd_mpeg2_ps_out");
    let _ = std::fs::remove_dir_all(&out);
    let _ = build_bdmv(&p, &out).unwrap();
    let encoded: Vec<String> = std::fs::read_dir(out.join("disccraft-cache"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("hd-"))
        .collect();
    assert!(encoded.is_empty(), "{encoded:?}");
    let stream = out.join("BDMV/STREAM/00001.m2ts");
    assert_eq!(frame_md5s(&ffmpeg, &stream), frame_md5s(&ffmpeg, &video));
    if std::env::var_os("DISCCRAFT_KEEP_OUTPUT").is_none() {
        let _ = std::fs::remove_dir_all(&out);
    }
}
