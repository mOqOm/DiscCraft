//! M6.318: a video's field type (Encore's フィールドタイプ) as DiscCraft reads it from the file's headers is what
//! ffprobe (a black box here) says of the same file: progressive, upper or lower field first, for MPEG-2 and H.264.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_transcode::{FieldType, field_type};
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
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
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
