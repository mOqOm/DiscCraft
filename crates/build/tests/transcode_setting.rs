//! M6.251 check: an asset's own transcode setting (a video bit rate) is what a build encodes it at, on a DVD and
//! on an HD Blu-ray, and a change of it makes a new transcode.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, TranscodeStatus, build, transcode_status};
use disccraft_project::{BdVideo, DiscKind, Link, Project};
use std::path::{Path, PathBuf};
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

/// Four seconds of a busy picture (noise over a test pattern), so an encoder spends the bits it is given.
fn busy_video(ffmpeg: &Path) -> Option<PathBuf> {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
    let video = fixtures.join("busy.mp4");
    if video.is_file() {
        return Some(video);
    }
    std::fs::create_dir_all(&fixtures).unwrap();
    let made = Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=720x480:rate=30000/1001,noise=alls=40:allf=t")
        .args(["-t", "4", "-c:v", "libx264", "-preset", "ultrafast"])
        .args(["-crf", "12", "-pix_fmt", "yuv420p"])
        .arg(&video)
        .status();
    if !made.is_ok_and(|status| status.success()) {
        let _ = std::fs::remove_file(&video);
        return None;
    }
    Some(video)
}

/// The bytes of the video stream of `file`, as ffprobe counts its packets.
fn video_bytes(ffprobe: &Path, file: &Path) -> u64 {
    let output = Command::new(ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries"])
        .args(["packet=size", "-of", "csv=p=0"])
        .arg(file)
        .output()
        .unwrap();
    assert!(output.status.success(), "{file:?}");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().trim_end_matches(',').parse::<u64>().ok())
        .sum()
}

#[test]
fn an_assets_bit_rate_is_what_it_is_encoded_at() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let Some(video) = busy_video(&ffmpeg) else {
        eprintln!("ffmpeg cannot make the fixture (no libx264?); skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "disccraft-transcode-setting-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut project = Project::default();
    let asset = project.add_asset(&video).unwrap();
    let timeline = project
        .add_timeline(asset, Some("Main".to_string()))
        .unwrap();
    project.settings.first_play = Link::Timeline {
        id: timeline,
        chapter: 0,
    };
    let build_to = |project: &Project, name: &str| {
        let out = dir.join(name);
        let report = build(
            project,
            &BuildOptions {
                output: OutputKind::Folder,
                path: out.clone(),
                cache: Some(dir.join("cache")),
                audio_language: None,
            },
        )
        .unwrap();
        assert_eq!(report.titles, 1);
        out
    };
    // Bits a second of four seconds of video.
    let rate = |bytes: u64| bytes * 8 / 4;

    // A DVD: automatic is the most a short title gets (8 Mbit/s); 2 Mbit/s of its own is about that.
    let automatic = video_bytes(
        &ffprobe,
        &build_to(&project, "dvd-auto").join("VIDEO_TS/VTS_01_1.VOB"),
    );
    project.set_asset_transcode(asset, Some(2_000)).unwrap();
    let low = video_bytes(
        &ffprobe,
        &build_to(&project, "dvd-low").join("VIDEO_TS/VTS_01_1.VOB"),
    );
    assert!(
        (1_400_000..2_700_000).contains(&rate(low)),
        "{} bit/s",
        rate(low)
    );
    assert!(rate(automatic) > 2 * rate(low), "{automatic} {low}");
    // The transcode in the cache is the asset's at that rate; another rate makes another transcode.
    let cache = dir.join("cache");
    assert_eq!(
        transcode_status(&project, Some(&cache))[&asset],
        TranscodeStatus::Transcoded
    );
    project.set_asset_transcode(asset, Some(4_000)).unwrap();
    assert_eq!(
        transcode_status(&project, Some(&cache))[&asset],
        TranscodeStatus::Untranscoded
    );
    let middle = video_bytes(
        &ffprobe,
        &build_to(&project, "dvd-mid").join("VIDEO_TS/VTS_01_1.VOB"),
    );
    assert!(
        (3_000_000..5_200_000).contains(&rate(middle)),
        "{} bit/s",
        rate(middle)
    );

    // M6.314: film (Encore's NTSC DV 23.976 presets) is another transcode; the disc's video is the four seconds in
    // 96 pictures shown by 3:2 pulldown (ffprobe reads repeat_pict 1, 0, 1, 0, …), and the VOB decodes without an
    // error.
    if let Some(found) = project.assets.get_mut(&asset) {
        found.transcode_film = true;
    }
    assert_eq!(
        transcode_status(&project, Some(&cache))[&asset],
        TranscodeStatus::Untranscoded
    );
    let vob = build_to(&project, "dvd-film").join("VIDEO_TS/VTS_01_1.VOB");
    let probed = Command::new(&ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries"])
        .args(["frame=repeat_pict", "-of", "csv=p=0"])
        .arg(&vob)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probed.stdout);
    let repeats: Vec<&str> = text
        .lines()
        .map(|line| line.trim().trim_end_matches(','))
        .collect();
    assert_eq!(repeats.len(), 96, "{text}");
    assert!(
        repeats.chunks(4).all(|four| four == ["1", "0", "1", "0"]),
        "{repeats:?}"
    );
    let decoded = Command::new(&ffmpeg)
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
    let film = video_bytes(&ffprobe, &vob);
    assert!(
        (3_000_000..5_200_000).contains(&rate(film)),
        "{} bit/s",
        rate(film)
    );

    // An HD Blu-ray (720p, where the file is encoded): its own Blu-ray setting (M6.307; the DVD one is not used).
    project.settings.kind = DiscKind::BluRay;
    project.settings.capacity = disccraft_project::Capacity::Bd25;
    project.settings.bd_video = BdVideo::Hd720p59;
    let bluray = |project: &mut Project, kbps: Option<u32>| {
        if let Some(found) = project.assets.get_mut(&asset) {
            found.bluray_transcode = kbps.map(|kbps| disccraft_project::BlurayTranscode {
                kbps,
                max_kbps: 30_000,
                preset: String::new(),
                quality: disccraft_project::BEST_QUALITY,
                mode: disccraft_project::TranscodeMode::Vbr,
            });
        }
    };
    bluray(&mut project, None);
    let stream = "BDMV/STREAM/00001.m2ts";
    let automatic = video_bytes(&ffprobe, &build_to(&project, "bd-auto").join(stream));
    bluray(&mut project, Some(3_000));
    let low = video_bytes(&ffprobe, &build_to(&project, "bd-low").join(stream));
    bluray(&mut project, Some(6_000));
    let middle = video_bytes(&ffprobe, &build_to(&project, "bd-mid").join(stream));
    eprintln!(
        "HD: automatic {} bit/s, 3 Mbit/s {} bit/s, 6 Mbit/s {} bit/s",
        rate(automatic),
        rate(low),
        rate(middle)
    );
    // The H.264 encoder stays under a short clip's target; the rates still follow the setting.
    assert!(
        (1_200_000..4_200_000).contains(&rate(low)),
        "{} bit/s",
        rate(low)
    );
    assert!(
        rate(middle) * 10 > rate(low) * 15 && rate(middle) < 8_400_000,
        "{} bit/s",
        rate(middle)
    );
    assert!(rate(automatic) > 2 * rate(low), "{automatic} {low}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Four seconds of DVD-compliant MPEG-2 (720x480, 29.97, about 8 Mbit/s): a DVD takes it as it is. `vob`: in a
/// program stream (as a DVD's VOB, with AC-3 sound), else an elementary stream.
fn compliant_video(ffmpeg: &Path, vob: bool) -> Option<PathBuf> {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
    let video = fixtures.join(if vob {
        "busy_compliant.vob"
    } else {
        "busy_compliant.m2v"
    });
    if video.is_file() {
        return Some(video);
    }
    std::fs::create_dir_all(&fixtures).unwrap();
    let mut command = Command::new(ffmpeg);
    let _ = command
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=720x480:rate=30000/1001,noise=alls=40:allf=t");
    if vob {
        let _ = command.args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"]);
    }
    let _ = command
        .args([
            "-t",
            "4",
            "-c:v",
            "mpeg2video",
            "-b:v",
            "8M",
            "-maxrate",
            "9M",
        ])
        .args([
            "-bufsize", "1835k", "-g", "15", "-pix_fmt", "yuv420p", "-aspect", "4:3",
        ]);
    if vob {
        let _ = command.args(["-c:a", "ac3", "-b:a", "192k", "-f", "vob"]);
    } else {
        let _ = command.args(["-f", "mpeg2video"]);
    }
    let made = command.arg(&video).status();
    if !made.is_ok_and(|status| status.success()) {
        let _ = std::fs::remove_file(&video);
        return None;
    }
    Some(video)
}

/// M6.315: a DVD-compliant video goes on a DVD as it is ("Don't Transcode"); a DVD quality preset of its own makes
/// the build transcode it all the same, at the preset's rate (Encore's preset picked for a compliant asset), and
/// taking the preset away puts the file back on as it is.
#[test]
fn a_preset_transcodes_a_compliant_video() {
    a_preset_transcodes(false);
}

/// M6.322: the same for a compliant program stream (a VOB), which a DVD takes demuxed.
#[test]
fn a_preset_transcodes_a_compliant_program_stream() {
    a_preset_transcodes(true);
}

fn a_preset_transcodes(vob: bool) {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let Some(video) = compliant_video(&ffmpeg, vob) else {
        eprintln!("ffmpeg cannot make the fixture; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "disccraft-forced-transcode-{}-{vob}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut project = Project::default();
    let asset = project.add_asset(&video).unwrap();
    let timeline = project
        .add_timeline(asset, Some("Main".to_string()))
        .unwrap();
    project.settings.first_play = Link::Timeline {
        id: timeline,
        chapter: 0,
    };
    let cache = dir.join("cache");
    let build_to = |project: &Project, name: &str| {
        let out = dir.join(name);
        build(
            project,
            &BuildOptions {
                output: OutputKind::Folder,
                path: out.clone(),
                cache: Some(cache.clone()),
                audio_language: None,
            },
        )
        .unwrap();
        out.join("VIDEO_TS/VTS_01_1.VOB")
    };
    let rate = |bytes: u64| bytes * 8 / 4;

    assert!(disccraft_build::video_fits_as_is(&project, asset));
    assert_eq!(
        transcode_status(&project, Some(&cache))[&asset],
        TranscodeStatus::Unneeded
    );
    let as_is = video_bytes(&ffprobe, &build_to(&project, "as-is"));
    assert!(rate(as_is) > 6_000_000, "{} bit/s", rate(as_is));

    project.set_asset_transcode(asset, Some(2_000)).unwrap();
    assert_eq!(
        transcode_status(&project, Some(&cache))[&asset],
        TranscodeStatus::Untranscoded
    );
    let vob = build_to(&project, "forced");
    let forced = video_bytes(&ffprobe, &vob);
    assert!(
        (1_400_000..2_700_000).contains(&rate(forced)),
        "{} bit/s",
        rate(forced)
    );
    let decoded = Command::new(&ffmpeg)
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
    assert_eq!(
        transcode_status(&project, Some(&cache))[&asset],
        TranscodeStatus::Transcoded
    );

    project.set_asset_transcode(asset, None).unwrap();
    assert_eq!(
        transcode_status(&project, Some(&cache))[&asset],
        TranscodeStatus::Unneeded
    );
    let _ = std::fs::remove_dir_all(&dir);
}
