//! M13.2 check: in a DVD ISO each set's backup IFO lies past a gap that keeps it out of its IFO's ECC block, at the
//! sector its IFO counts (the set's last sector), and each title set starts where the VMG says. Read through
//! ffmpeg's DVD navigation (libdvdread's own UDF reader) when ffmpeg is installed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_project::{Link, Project};
use disccraft_udf::read::list_udf;
use std::path::{Path, PathBuf};
use std::process::Command;

/// An ECC block of a DVD: 16 sectors.
const ECC_BLOCK: u32 = 16;

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

/// A short DVD-compliant MPEG-2 elementary stream (passed through without transcoding).
fn short_video() -> Option<PathBuf> {
    let video = fixtures().join("dvd_iso.m2v");
    if video.is_file() {
        return Some(video);
    }
    let ffmpeg = tool("DISCCRAFT_FFMPEG", "ffmpeg")?;
    std::fs::create_dir_all(fixtures()).unwrap();
    let ok = Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .args(["testsrc=size=720x480:rate=30000/1001", "-t", "4"])
        .args(["-vf", "setdar=4/3", "-c:v", "mpeg2video", "-b:v", "2M"])
        .args([
            "-maxrate", "6M", "-bufsize", "1835k", "-g", "15", "-bf", "0",
        ])
        .args(["-pix_fmt", "yuv420p", "-f", "mpeg2video"])
        .arg(&video)
        .status()
        .unwrap()
        .success();
    assert!(ok, "ffmpeg could not create the fixture");
    Some(video)
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// Checks one set (`ifo`, `bup` in the image at their sectors): the BUP is where the IFO's last-sector fields
/// put it and never shares an ECC block with the IFO.
fn check_set(video_ts: &Path, sectors: &dyn Fn(&str) -> u32, ifo: &str, bup: &str) {
    let bytes = std::fs::read(video_ts.join(ifo)).unwrap();
    let ifo_sectors = u32::try_from(bytes.len() / 2048).unwrap();
    let last_set = u32_at(&bytes, 0x00C);
    let ifo_at = sectors(&format!("VIDEO_TS/{ifo}"));
    let bup_at = sectors(&format!("VIDEO_TS/{bup}"));
    // The BUP is the set's last IFO-sized run of sectors.
    assert_eq!(bup_at, ifo_at + last_set + 1 - ifo_sectors, "{bup}");
    let ifo_last = ifo_at + ifo_sectors - 1;
    assert!(bup_at - ifo_last > ECC_BLOCK, "{bup} shares an ECC block");
    assert_ne!(ifo_last / ECC_BLOCK, bup_at / ECC_BLOCK, "{bup}");
}

#[test]
fn backups_lie_past_a_gap_that_the_ifos_count() {
    let Some(video) = short_video() else {
        eprintln!("ffmpeg not found and no fixture; skipping");
        return;
    };
    let mut project = Project::default();
    let v = project.add_asset(&video).unwrap();
    let first = project.add_timeline(v, Some("One".to_string())).unwrap();
    project.add_timeline(v, Some("Two".to_string())).unwrap();
    project.settings.first_play = Link::Timeline {
        id: first,
        chapter: 0,
    };

    let dir = std::env::temp_dir().join(format!("disccraft-dvd-iso-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let iso = dir.join("gap.iso");
    let report = build(
        &project,
        &BuildOptions {
            output: OutputKind::Iso,
            path: iso.clone(),
            cache: None,
            audio_language: Some("ja".to_owned()),
        },
    )
    .unwrap();
    assert_eq!(report.titles, 2);

    let listing = list_udf(&iso).unwrap();
    let sectors = |path: &str| {
        listing
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path} is not in the image"))
            .sector
    };
    let video_ts = dir.join("gap.files").join("VIDEO_TS");
    check_set(&video_ts, &sectors, "VIDEO_TS.IFO", "VIDEO_TS.BUP");
    check_set(&video_ts, &sectors, "VTS_01_0.IFO", "VTS_01_0.BUP");
    check_set(&video_ts, &sectors, "VTS_02_0.IFO", "VTS_02_0.BUP");

    // Each title set starts where the VMG says, counted from the VMG's own first sector.
    let vmgi =
        disccraft_ifo::read_vmgi(&std::fs::read(video_ts.join("VIDEO_TS.IFO")).unwrap()).unwrap();
    let vmg_at = sectors("VIDEO_TS/VIDEO_TS.IFO");
    for title in &vmgi.titles {
        let ifo = format!("VIDEO_TS/VTS_{:02}_0.IFO", title.vts);
        assert_eq!(sectors(&ifo) - vmg_at, title.vts_start_sector, "{ifo}");
    }

    // ffmpeg's DVD navigation reads both titles from the image.
    if let Some(ffmpeg) = tool("DISCCRAFT_FFMPEG", "ffmpeg") {
        for title in ["1", "2"] {
            let out = Command::new(&ffmpeg)
                .args(["-v", "error", "-f", "dvdvideo", "-title", title, "-i"])
                .arg(&iso)
                .args(["-map", "0", "-f", "null", "-"])
                .output()
                .unwrap();
            let stderr = String::from_utf8_lossy(&out.stderr);
            let errors: Vec<&str> = stderr
                .lines()
                .filter(|l| {
                    !l.contains("libdvdcss")
                        && !l.contains("for reading")
                        && !l.contains("CSS auth")
                        && !l.contains("device")
                })
                .collect();
            assert!(out.status.success() && errors.is_empty(), "{errors:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
