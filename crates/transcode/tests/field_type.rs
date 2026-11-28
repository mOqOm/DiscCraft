//! M6.318: a video's field type (Encore's フィールドタイプ) as DiscCraft reads it from the file's headers is what
//! ffprobe (a black box here) says of the same file: progressive, upper or lower field first, for MPEG-2 and H.264.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_test_support::{ffmpeg, ffprobe};
use disccraft_transcode::{FieldType, field_type};
use std::path::Path;
use std::process::Command;

/// ffprobe's field_order of the first video stream of `file`, as a field type.
fn probed(ffprobe: &Path, file: &Path) -> Option<FieldType> {
    let output = Command::new(ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries"])
        .args(["stream=field_order", "-of", "csv=p=0"])
        .arg(file)
        .output()
        .ok()?;
    match String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_end_matches(',')
    {
        "progressive" => Some(FieldType::Progressive),
        "tt" => Some(FieldType::UpperFirst),
        "bb" => Some(FieldType::LowerFirst),
        _ => None,
    }
}

#[test]
fn field_types_are_what_ffprobe_says() {
    let (Some(ffmpeg), Some(ffprobe)) = (ffmpeg(), ffprobe()) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-field-type-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cases: [(&str, &[&str], FieldType); 5] = [
        (
            "upper.m2v",
            &[
                "-c:v",
                "mpeg2video",
                "-flags",
                "+ildct+ilme",
                "-vf",
                "setfield=tff",
                "-f",
                "mpeg2video",
            ],
            FieldType::UpperFirst,
        ),
        (
            "lower.m2v",
            &[
                "-c:v",
                "mpeg2video",
                "-flags",
                "+ildct+ilme",
                "-vf",
                "setfield=bff",
                "-f",
                "mpeg2video",
            ],
            FieldType::LowerFirst,
        ),
        (
            "lower.mpg",
            &[
                "-c:v",
                "mpeg2video",
                "-flags",
                "+ildct+ilme",
                "-vf",
                "setfield=bff",
                "-f",
                "vob",
            ],
            FieldType::LowerFirst,
        ),
        (
            "frames.mp4",
            &[
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
            ],
            FieldType::Progressive,
        ),
        (
            "fields.mp4",
            &[
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
                "-flags",
                "+ildct+ilme",
                "-x264-params",
                "tff=1",
            ],
            FieldType::UpperFirst,
        ),
    ];
    let mut checked = 0;
    for (name, args, expected) in cases {
        let file = dir.join(name);
        let made = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc2=size=720x480:rate=30000/1001")
            .args(["-t", "1"])
            .args(args)
            .arg(&file)
            .status();
        if !made.is_ok_and(|status| status.success()) {
            // The MPEG-2 encoder is ffmpeg's own; only H.264 needs libx264, which a build may lack.
            assert!(name.ends_with(".mp4"), "ffmpeg cannot make {name}");
            eprintln!("ffmpeg cannot make {name} (no libx264?); skipped");
            continue;
        }
        let oracle = probed(&ffprobe, &file);
        assert_eq!(oracle, Some(expected), "ffprobe on {name}");
        assert_eq!(field_type(&file), Some(expected), "{name}");
        checked += 1;
    }
    // A file that is no video at all has none.
    let text = dir.join("notes.txt");
    std::fs::write(&text, b"no pictures here").unwrap();
    assert_eq!(field_type(&text), None);
    assert_eq!(field_type(&dir.join("missing.m2v")), None);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(checked >= 2, "only {checked} cases could be made");
}

/// M6.569: DV's field order is its first frame's (the VAUX source control pack's FS flag), in a raw DV file and in
/// MOV and AVI: ffmpeg writes FS = 0 for upper field first and FS = 1 for lower; ffprobe (a black box) reads it back
/// the same way frame by frame.
#[test]
fn dv_field_order_is_its_first_frames() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-dv-field-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (size, rate, pixels) in [
        ("720x480", "30000/1001", "yuv411p"),
        ("720x576", "25", "yuv420p"),
    ] {
        for (order, expected) in [
            ("tff", FieldType::UpperFirst),
            ("bff", FieldType::LowerFirst),
        ] {
            for (extension, format) in [("dv", "dv"), ("mov", "mov"), ("avi", "avi")] {
                let file = dir.join(format!("{order}-{pixels}.{extension}"));
                let made = Command::new(&ffmpeg)
                    .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
                    .arg(format!("testsrc2=size={size}:rate={rate}"))
                    .args(["-vf", &format!("setfield={order}"), "-t", "0.2"])
                    .args(["-c:v", "dvvideo", "-pix_fmt", pixels, "-f", format])
                    .arg(&file)
                    .status()
                    .unwrap()
                    .success();
                assert!(made, "ffmpeg could not make {}", file.display());
                assert_eq!(field_type(&file), Some(expected), "{}", file.display());
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
