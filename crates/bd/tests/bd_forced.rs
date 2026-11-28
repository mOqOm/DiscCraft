//! M6.324: H.264 that goes on an HD Blu-ray as it is ("Don't Transcode") is encoded all the same when its asset has a
//! Blu-ray quality preset of its own (Encore's preset picked for a compliant asset), at the preset's rate; taking the
//! preset away puts the file back on as it is.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_bd::build_bdmv;
use disccraft_project::{BdVideo, BlurayTranscode, DiscKind, Link, Project};
use disccraft_test_support::{ffmpeg, ffprobe};
use std::path::Path;
use std::process::Command;

/// The MD5 of the decoded pictures of `file`'s first video stream.
fn video_md5(ffmpeg: &Path, file: &Path) -> String {
    let output = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(file)
        .args(["-map", "0:v:0", "-f", "md5", "-"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// The bytes of the video stream of `file`, as ffprobe counts its packets.
fn video_bytes(ffprobe: &Path, file: &Path) -> u64 {
    let output = Command::new(ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries"])
        .args(["packet=size", "-of", "csv=p=0"])
        .arg(file)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().trim_end_matches(',').parse::<u64>().ok())
        .sum()
}

#[test]
fn a_blu_ray_preset_encodes_video_that_fits_the_disc() {
    let (Some(ffmpeg), Some(ffprobe)) = (ffmpeg(), ffprobe()) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-bd-forced-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Four seconds of 720p59.94 High profile H.264 at about 12 Mbit/s: a 720p Blu-ray takes it as it is.
    let video = dir.join("hd.mp4");
    let made = Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=1280x720:rate=60000/1001,noise=alls=30:allf=t")
        .args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
        .args([
            "-t",
            "4",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-profile:v",
            "high",
        ])
        .args(["-level", "4.1", "-pix_fmt", "yuv420p", "-b:v", "12M"])
        .args([
            "-x264-params",
            "bframes=2:keyint=60:aud=0:vbv-maxrate=20000:vbv-bufsize=20000",
        ])
        .args(["-c:a", "aac", "-b:a", "128k"])
        .arg(&video)
        .status();
    if !made.is_ok_and(|status| status.success()) {
        eprintln!("ffmpeg cannot make H.264 (no libx264?); skipping");
        return;
    }

    let mut project = Project::default();
    project.settings.kind = DiscKind::BluRay;
    project.settings.capacity = disccraft_project::Capacity::Bd25;
    project.settings.bd_video = BdVideo::Hd720p59;
    let asset = project.add_asset(&video).unwrap();
    let timeline = project
        .add_timeline(asset, Some("Main".to_string()))
        .unwrap();
    project.settings.first_play = Link::Timeline {
        id: timeline,
        chapter: 0,
    };
    let built = |project: &Project, name: &str| {
        let out = dir.join(name);
        let report = build_bdmv(project, &out).unwrap();
        assert_eq!(report.titles.len(), 1);
        out.join("BDMV/STREAM/00001.m2ts")
    };
    let rate = |bytes: u64| bytes * 8 / 4;

    // As it is: the same pictures, nothing to encode.
    assert!(disccraft_dvd::video_fits_as_is(&project, asset));
    let kept = disccraft_dvd::estimate(&project).unwrap();
    assert!(kept.passthrough_bytes > 5_000_000, "{kept:?}");
    let as_is = built(&project, "as-is");
    assert_eq!(video_md5(&ffmpeg, &as_is), video_md5(&ffmpeg, &video));
    assert!(rate(video_bytes(&ffprobe, &as_is)) > 8_000_000);

    // A Blu-ray preset of its own (4 Mbit/s): encoded at about that rate, other pictures, no error.
    if let Some(found) = project.assets.get_mut(&asset) {
        found.bluray_transcode = Some(BlurayTranscode {
            kbps: 4_000,
            max_kbps: 8_000,
            preset: String::new(),
            quality: disccraft_project::BEST_QUALITY,
            mode: disccraft_project::TranscodeMode::Vbr,
        });
    }
    assert!(disccraft_dvd::video_fits_as_is(&project, asset));
    // Its size is counted at the preset's rate (4 s at 4 Mbit/s, about 2 MB), not as the file.
    let encoded_estimate = disccraft_dvd::estimate(&project).unwrap();
    assert!(
        (1_500_000..2_600_000).contains(&encoded_estimate.passthrough_bytes),
        "{encoded_estimate:?} {kept:?}"
    );
    let encoded = built(&project, "encoded");
    assert_ne!(video_md5(&ffmpeg, &encoded), video_md5(&ffmpeg, &video));
    let encoded_rate = rate(video_bytes(&ffprobe, &encoded));
    assert!(
        (1_200_000..5_600_000).contains(&encoded_rate),
        "{encoded_rate} bit/s"
    );
    let decoded = Command::new(&ffmpeg)
        .args(["-v", "error", "-xerror", "-i"])
        .arg(&encoded)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decoded.status.success() && decoded.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );

    // The preset taken away: as it is again.
    if let Some(found) = project.assets.get_mut(&asset) {
        found.bluray_transcode = None;
    }
    let again = built(&project, "again");
    assert_eq!(video_md5(&ffmpeg, &again), video_md5(&ffmpeg, &video));
    let _ = std::fs::remove_dir_all(&dir);
}
