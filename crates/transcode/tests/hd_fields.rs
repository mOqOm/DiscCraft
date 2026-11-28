//! M12.37: an interlaced source's fields stay apart on an interlaced Blu-ray format, as on a DVD (M12.26). A narrow
//! white bar moves 48 pixels a field (59.94 fields a second, interlaced by x264 as 1080i, the top field first); coded
//! as MPEG-2 1080i its pictures are interlaced with the field tools (M12.36) and each field shows one sharp bar, and
//! as the standard-definition H.264 of a Blu-ray (720 x 480 interlaced, M10.41) each field is scaled on its own: the
//! even lines show one sharp bar and the odd lines one 18 pixels further right (48 × 720 / 1920), with no ghost of
//! the other field. M12.42: the same with the bottom field first (as DV), the streams then marked bottom field first.
//! Checked with ffmpeg as the oracle (skipped without it).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use disccraft_test_support::{ffmpeg, ffprobe, fixtures};
use std::path::{Path, PathBuf};
use std::process::Command;

use disccraft_transcode::RateMode;
use disccraft_transcode::hd::{HdCodec, HdOptions, transcode_h264};

/// The moving bar as 1080i H.264 (the DVD test's fixture; M12.42: `bottom_first`, the bottom field first, as DV),
/// made with ffmpeg when missing; `None` without libx264.
fn bar(ffmpeg: &Path, bottom_first: bool) -> Option<PathBuf> {
    let (name, interleave, order) = if bottom_first {
        (
            "h264_1080i_bar_bff.m2ts",
            "interleave_bottom",
            "bff=1:bframes=0",
        )
    } else {
        ("h264_1080i_bar.m2ts", "interleave_top", "tff=1:bframes=0")
    };
    let video = fixtures().join(name);
    if !video.is_file() {
        std::fs::create_dir_all(fixtures()).unwrap();
        let made = Command::new(ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!("color=black:size=1920x1080:rate=60000/1001[bg];color=white:size=16x1080:rate=60000/1001[bar];[bg][bar]overlay=x='mod(n*48,1600)+100':y=0:shortest=1,tinterlace=mode={interleave},format=yuv420p"))
            .args(["-t", "1", "-c:v", "libx264", "-preset", "ultrafast", "-qp", "10"])
            .args(["-flags", "+ildct+ilme", "-x264-params", order])
            .args(["-aspect", "16:9", "-f", "mpegts"])
            .arg(&video)
            .status();
        if !made.is_ok_and(|status| status.success()) {
            let _ = std::fs::remove_file(&video);
            return None;
        }
    }
    Some(video)
}

fn options(codec: HdCodec, (width, height): (u32, u32)) -> HdOptions {
    HdOptions {
        width,
        height,
        fps_num: 30_000,
        fps_den: 1001,
        target_kbps: 8000,
        max_kbps: 15_000,
        codec,
        quality: 5,
        rendering: disccraft_transcode::Rendering::default(),
        mode: RateMode::Vbr,
        interlaced: true,
        wide: true,
    }
}

/// The luma of picture 10 of the elementary stream `es` (ffmpeg `format`), `width` x `height`.
fn tenth_picture(
    ffmpeg: &Path,
    es: &Path,
    format: &str,
    (width, height): (usize, usize),
) -> Vec<u8> {
    let decoded = Command::new(ffmpeg)
        .args(["-v", "error", "-f", format, "-i"])
        .arg(es)
        .args(["-vf", r"select=eq(n\,10)", "-frames:v", "1"])
        .args(["-f", "rawvideo", "-pix_fmt", "gray", "-"])
        .output()
        .unwrap();
    assert_eq!(
        decoded.stdout.len(),
        width * height,
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    decoded.stdout
}

/// Each field of rows `(top, top + 1)` shows one sharp bar, the bottom field's `shift` pixels right of the top
/// field's, with no ghost of the other.
fn fields_apart(luma: &[u8], width: usize, rows: &[usize], (shift, sharp): (f64, usize)) {
    let bright = |row: usize| -> Vec<usize> {
        (0..width)
            .filter(|&x| luma[row * width + x] > 160)
            .collect()
    };
    let middle =
        |columns: &[usize]| columns.iter().sum::<usize>() as f64 / columns.len().max(1) as f64;
    for &top in rows {
        let (a, b) = (bright(top), bright(top + 1));
        assert!(!a.is_empty() && !b.is_empty(), "rows {top}: no bar");
        assert!(
            a.len() <= sharp && b.len() <= sharp,
            "rows {top}: {a:?} {b:?}"
        );
        let moved = middle(&b) - middle(&a);
        assert!((moved - shift).abs() < 4.0, "rows {top}: shift {moved}");
        let other = middle(&b).round() as usize;
        assert!(
            luma[top * width + other] < 90,
            "row {top}: ghost {}",
            luma[top * width + other]
        );
    }
}

#[test]
fn an_interlaced_source_keeps_its_fields_on_an_interlaced_blu_ray() {
    keeps_its_fields(false);
}

/// M12.42: a source whose bottom field comes first keeps its fields too, and the stream says the bottom field comes
/// first (each field still one sharp bar, the top field's now 48 px right of the bottom field's, which came first).
#[test]
fn a_bottom_field_first_source_keeps_its_fields_and_order() {
    keeps_its_fields(true);
}

fn keeps_its_fields(bottom_first: bool) {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let Some(video) = bar(&ffmpeg, bottom_first) else {
        eprintln!("ffmpeg cannot make interlaced H.264 (no libx264?); skipping");
        return;
    };
    let expected = if bottom_first {
        disccraft_transcode::FieldType::LowerFirst
    } else {
        disccraft_transcode::FieldType::UpperFirst
    };
    assert_eq!(disccraft_transcode::field_type(&video), Some(expected));
    // The bottom field's bar is this far right of the top field's.
    let sign = if bottom_first { -1.0 } else { 1.0 };
    let dir = std::env::temp_dir().join(format!(
        "disccraft-hd-fields-{}-{bottom_first}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // MPEG-2 1080i: interlaced pictures, the P pictures with the field tools; each field one bar, 48 px apart.
    let es = dir.join("mpeg2.m2v");
    transcode_h264(
        &video,
        &es,
        &dir.join("mpeg2.idx"),
        &options(HdCodec::Mpeg2, (1920, 1080)),
        &mut |_| {},
    )
    .unwrap();
    let bytes = std::fs::read(&es).unwrap();
    let mut picture_type = None;
    let mut flags = Vec::new();
    for at in 0..bytes.len().saturating_sub(9) {
        if bytes[at..at + 3] != [0, 0, 1] {
            continue;
        }
        match bytes[at + 3] {
            0x00 => picture_type = Some((bytes[at + 5] >> 3) & 7),
            0xB5 if bytes[at + 4] >> 4 == 8 => {
                if let Some(kind) = picture_type.take() {
                    // (type, top_field_first, frame_pred_frame_dct, progressive_frame)
                    flags.push((
                        kind,
                        bytes[at + 7] >> 7,
                        (bytes[at + 7] >> 6) & 1,
                        bytes[at + 8] >> 7,
                    ));
                }
            }
            _ => {}
        }
    }
    assert!(flags.len() >= 30, "{flags:?}");
    assert!(
        flags.iter().all(
            |&(_, top_first, _, progressive)| top_first == u8::from(!bottom_first)
                && progressive == 0
        ),
        "{flags:?}"
    );
    assert!(
        flags
            .iter()
            .filter(|flag| flag.0 == 2)
            .all(|flag| flag.2 == 0),
        "{flags:?}"
    );
    let luma = tenth_picture(&ffmpeg, &es, "mpegvideo", (1920, 1080));
    fields_apart(&luma, 1920, &[400, 540, 800], (48.0 * sign, 24));

    // Standard-definition H.264: each field scaled on its own, the bars 18 px apart.
    let es = dir.join("sd.h264");
    transcode_h264(
        &video,
        &es,
        &dir.join("sd.idx"),
        &options(HdCodec::H264, (720, 480)),
        &mut |_| {},
    )
    .unwrap();
    let luma = tenth_picture(&ffmpeg, &es, "h264", (720, 480));
    fields_apart(&luma, 720, &[200, 240, 300], (18.0 * sign, 10));
    // The H.264 says which field comes first.
    if let Some(ffprobe) = ffprobe() {
        let probed = Command::new(ffprobe)
            .args(["-v", "error", "-f", "h264", "-read_intervals", "%+#5"])
            .args([
                "-show_entries",
                "frame=interlaced_frame,top_field_first",
                "-of",
                "csv=p=0",
            ])
            .arg(&es)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&probed.stdout);
        let first = if bottom_first { "1,0" } else { "1,1" };
        // One line a frame (`interlaced_frame,top_field_first`, a trailing comma on some).
        let frames: Vec<&str> = text
            .lines()
            .map(|line| line.trim().trim_end_matches(','))
            .filter(|line| !line.is_empty())
            .collect();
        assert!(
            frames.len() >= 3 && frames.iter().all(|frame| *frame == first),
            "{text}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
