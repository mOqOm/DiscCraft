//! M6.410 check: DV, the video of camcorders and of the AVI files a computer captured from them, makes a DVD. An NTSC
//! DV AVI (a DV stream and the sound as a PCM stream: type 2) and a PAL 16:9 raw DV file are built into DVDs whose
//! video keeps the fields in their order (the lower first, as DV's) and the picture's shape, with the sound as AC-3.
//! Made and decoded by ffmpeg (the outside oracle) when it is installed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_project::{Link, Project, VideoStandard};
use disccraft_test_support::{ffmpeg, fixtures};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Two seconds of interlaced moving picture (fields of a picture at twice the rate) as DV, with a stereo tone.
fn dv_fixture(
    ffmpeg: &Path,
    name: &str,
    size: &str,
    rate: &str,
    pixels: &str,
    aspect: &str,
    format: &str,
) -> PathBuf {
    let path = fixtures().join(name);
    if path.is_file() {
        return path;
    }
    std::fs::create_dir_all(fixtures()).unwrap();
    let ok = Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("testsrc2=size={size}:rate={rate}*2"))
        .args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
        .args(["-f", "lavfi", "-i", "sine=frequency=660:sample_rate=48000"])
        .args([
            "-filter_complex",
            "[0]tinterlace=mode=interleave_bottom,setfield=bff[v];[1][2]amerge=inputs=2[a]",
            "-map",
            "[v]",
            "-map",
            "[a]",
        ])
        .args([
            "-t", "2", "-c:v", "dvvideo", "-flags", "+ildct", "-pix_fmt", pixels, "-aspect", aspect,
        ])
        .args(["-c:a", "pcm_s16le", "-f", format])
        .arg(&path)
        .status()
        .unwrap()
        .success();
    assert!(ok, "ffmpeg could not make {name}");
    path
}

/// A DVD project of `standard` with one timeline of `video`, played first; 16:9 when `wide` (as a new timeline takes
/// its video's aspect in the app).
fn project_of(video: &Path, standard: VideoStandard, wide: bool) -> Project {
    let mut project = Project::default();
    project.settings.standard = standard;
    let asset = project.add_asset(video).unwrap();
    let timeline = project
        .add_timeline(asset, Some("Main".to_string()))
        .unwrap();
    project.timelines.get_mut(&timeline).unwrap().widescreen = Some(wide);
    project.settings.first_play = Link::Timeline {
        id: timeline,
        chapter: 0,
    };
    project
}

/// Builds `project` into a folder under the temporary directory named after `name`; the folder's VIDEO_TS.
fn build_folder(project: &Project, name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("disccraft-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = build(
        project,
        &BuildOptions {
            output: OutputKind::Folder,
            path: dir.join("out"),
            cache: Some(dir.join("cache")),
            audio_language: Some("en".to_owned()),
        },
    )
    .unwrap();
    dir.join("out").join("VIDEO_TS")
}

/// What ffmpeg says of the title's streams, after decoding it all with no error.
fn streams_after_decoding(ffmpeg: &Path, video_ts: &Path) -> String {
    let vob = video_ts.join("VTS_01_1.VOB");
    let decoded = Command::new(ffmpeg)
        .args(["-v", "error", "-xerror", "-i"])
        .arg(&vob)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decoded.status.success() && decoded.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    let streams = Command::new(ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(&vob)
        .output()
        .unwrap();
    String::from_utf8_lossy(&streams.stderr).into_owned()
}

#[test]
fn an_ntsc_dv_avi_makes_a_dvd_with_its_fields_and_sound() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let video = dv_fixture(
        &ffmpeg,
        "dv_ntsc_type2.avi",
        "720x480",
        "30000/1001",
        "yuv411p",
        "4:3",
        "avi",
    );
    let project = project_of(&video, VideoStandard::Ntsc, false);
    let video_ts = build_folder(&project, "dv-ntsc");
    let streams = streams_after_decoding(&ffmpeg, &video_ts);
    assert!(streams.contains("mpeg2video"), "{streams}");
    assert!(streams.contains("720x480"), "{streams}");
    assert!(streams.contains("DAR 4:3"), "{streams}");
    assert!(
        streams.contains("bottom first"),
        "the fields keep DV's order: {streams}"
    );
    assert!(streams.contains("Audio: ac3"), "{streams}");
    let _ = std::fs::remove_dir_all(video_ts.parent().unwrap().parent().unwrap());
}

#[test]
fn a_pal_wide_raw_dv_file_makes_a_wide_dvd() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let video = dv_fixture(
        &ffmpeg,
        "dv_pal_wide_build.dv",
        "720x576",
        "25",
        "yuv420p",
        "16:9",
        "dv",
    );
    // The app makes the timeline 16:9 from the probe, which says the file is wide.
    assert!(disccraft_transcode::probe_video(&video).unwrap().wide);
    let project = project_of(&video, VideoStandard::Pal, true);
    let video_ts = build_folder(&project, "dv-pal");
    let streams = streams_after_decoding(&ffmpeg, &video_ts);
    assert!(streams.contains("720x576"), "{streams}");
    assert!(
        streams.contains("DAR 16:9"),
        "the whole frame, not letterboxed: {streams}"
    );
    assert!(streams.contains("bottom first"), "{streams}");
    assert!(streams.contains("Audio: ac3"), "{streams}");
    let _ = std::fs::remove_dir_all(video_ts.parent().unwrap().parent().unwrap());
}

/// A raw DV file that ffmpeg copied into an AVI (`-c copy`; M6.419): ffmpeg counts the AVI's time in 1/600 s, so a
/// frame is in one chunk in about 20 and the others are empty. The file is as long as its frames (not 20 times longer,
/// as when each chunk was taken for a frame), and its DVD too.
#[test]
fn a_dv_avi_counting_600_chunks_a_second_is_as_long_as_its_frames() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let raw = dv_fixture(
        &ffmpeg,
        "dv_ntsc_build.dv",
        "720x480",
        "30000/1001",
        "yuv411p",
        "4:3",
        "dv",
    );
    let video = fixtures().join("dv_ntsc_600.avi");
    if !video.is_file() {
        let ok = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-i"])
            .arg(&raw)
            .args(["-c", "copy"])
            .arg(&video)
            .status()
            .unwrap()
            .success();
        assert!(ok, "ffmpeg could not copy the DV into an AVI");
    }
    let probe = disccraft_transcode::probe_video(&video).unwrap();
    let own = disccraft_transcode::probe_video(&raw).unwrap();
    assert!(
        (probe.seconds - own.seconds).abs() < 0.05,
        "{} s, the DV {} s",
        probe.seconds,
        own.seconds
    );
    let project = project_of(&video, VideoStandard::Ntsc, false);
    let video_ts = build_folder(&project, "dv-600");
    let streams = streams_after_decoding(&ffmpeg, &video_ts);
    // ffmpeg's estimate of the VOB's length (about 2 s; 40 s when each chunk was a frame).
    let seconds: f64 = streams
        .split("Duration: 00:00:")
        .nth(1)
        .and_then(|rest| rest.get(..5))
        .and_then(|text| text.parse().ok())
        .unwrap_or(f64::MAX);
    assert!((seconds - 2.0).abs() < 0.2, "{streams}");
    let _ = std::fs::remove_dir_all(video_ts.parent().unwrap().parent().unwrap());
}

/// DV in a QuickTime movie (what iMovie and Final Cut wrote, M6.413): decoded by the media reader with the DV decoder
/// DiscCraft registers, its fields kept in order and its 16:9 shape kept.
#[test]
fn a_pal_wide_dv_mov_makes_a_wide_dvd() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let video = dv_fixture(
        &ffmpeg,
        "dv_pal_wide.mov",
        "720x576",
        "25",
        "yuv420p",
        "16:9",
        "mov",
    );
    assert!(disccraft_transcode::probe_video(&video).unwrap().wide);
    assert_eq!(
        disccraft_transcode::field_type(&video),
        Some(disccraft_transcode::FieldType::LowerFirst)
    );
    let project = project_of(&video, VideoStandard::Pal, true);
    let video_ts = build_folder(&project, "dv-mov");
    let streams = streams_after_decoding(&ffmpeg, &video_ts);
    assert!(streams.contains("720x576"), "{streams}");
    assert!(streams.contains("DAR 16:9"), "{streams}");
    assert!(streams.contains("bottom first"), "{streams}");
    assert!(streams.contains("Audio: ac3"), "{streams}");
    let _ = std::fs::remove_dir_all(video_ts.parent().unwrap().parent().unwrap());
}

/// A timeline whose clip's sound is on an audio track of its own (as the app makes a new timeline) makes that sound
/// into AC-3 once (M6.414): the video's own sound would only be used without an audio track, and was made into AC-3
/// all the same, after the video (three seconds a minute).
#[test]
fn a_timeline_with_its_clips_sound_as_a_track_makes_the_sound_once() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let video = dv_fixture(
        &ffmpeg,
        "dv_ntsc_type2.avi",
        "720x480",
        "30000/1001",
        "yuv411p",
        "4:3",
        "avi",
    );
    let mut project = project_of(&video, VideoStandard::Ntsc, false);
    let asset = *project.assets.keys().next().unwrap();
    let timeline = *project.timelines.keys().next().unwrap();
    project
        .timeline_mut(timeline)
        .unwrap()
        .audio
        .push(disccraft_project::AudioTrack::of(asset));
    let video_ts = build_folder(&project, "dv-sound-once");
    let cache = video_ts.parent().unwrap().parent().unwrap().join("cache");
    let ac3: Vec<_> = std::fs::read_dir(&cache)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "ac3"))
        .collect();
    assert_eq!(ac3.len(), 1, "{ac3:?}");
    let streams = streams_after_decoding(&ffmpeg, &video_ts);
    assert!(streams.contains("Audio: ac3"), "{streams}");
    let _ = std::fs::remove_dir_all(video_ts.parent().unwrap().parent().unwrap());
}

/// DVCPRO50 (4:2:2, 50 Mb/s: frames twice as long, M6.415) as a raw DV file makes a DVD with its fields in order.
#[test]
fn a_dvcpro50_file_makes_a_dvd() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let video = dv_fixture(
        &ffmpeg,
        "dv50_build.dv",
        "720x480",
        "30000/1001",
        "yuv422p",
        "4:3",
        "dv",
    );
    let project = project_of(&video, VideoStandard::Ntsc, false);
    let video_ts = build_folder(&project, "dv50");
    let streams = streams_after_decoding(&ffmpeg, &video_ts);
    assert!(streams.contains("720x480"), "{streams}");
    assert!(streams.contains("bottom first"), "{streams}");
    assert!(streams.contains("Audio: ac3"), "{streams}");
    let _ = std::fs::remove_dir_all(video_ts.parent().unwrap().parent().unwrap());
}

/// DV in a Matroska file (M6.416): its frames found with craft-media's Matroska demuxer, its sound from the media
/// reader.
#[test]
fn an_ntsc_dv_mkv_makes_a_dvd() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let video = dv_fixture(
        &ffmpeg,
        "dv_ntsc.mkv",
        "720x480",
        "30000/1001",
        "yuv411p",
        "4:3",
        "matroska",
    );
    assert_eq!(
        disccraft_transcode::field_type(&video),
        Some(disccraft_transcode::FieldType::LowerFirst)
    );
    let project = project_of(&video, VideoStandard::Ntsc, false);
    let video_ts = build_folder(&project, "dv-mkv");
    let streams = streams_after_decoding(&ffmpeg, &video_ts);
    assert!(streams.contains("720x480"), "{streams}");
    assert!(streams.contains("bottom first"), "{streams}");
    assert!(streams.contains("Audio: ac3"), "{streams}");
    let _ = std::fs::remove_dir_all(video_ts.parent().unwrap().parent().unwrap());
}
