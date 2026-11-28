//! M10.45: HD MPEG-2 whose frames are coded as two field pictures (as some broadcast and recorder video is) goes on a
//! 1080i disc as it is, from a transport stream and from an elementary stream: nothing is encoded, the clip holds the
//! same field pictures, and it decodes without an error to the same pictures as the file. The stream is made by
//! DiscCraft's own encoder (M12.38: ffmpeg cannot make field pictures) and put in a transport stream by ffmpeg,
//! which is the oracle (skipped without it).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_bd::build_bdmv;
use disccraft_mpeg2venc::encoder::{Encoder, EncoderConfig, Format};
use disccraft_mpeg2venc::{Frame420, Standard};
use disccraft_project::{BdVideo, DiscKind, Link, Project};
use disccraft_test_support::{ffmpeg, fixtures};
use std::path::Path;
use std::process::Command;

/// Frame `t` of 1920 x 1088: bands and a square moving by (6, 3) px a field, each field from its own moment.
fn frame(t: usize) -> Frame420 {
    let (w, h) = (1920usize, 1088usize);
    let mut y = vec![0u8; w * h];
    for row in 0..h {
        let field_time = 2 * t + row % 2;
        let (sx, sy) = (200 + 6 * field_time, 300 + 3 * field_time);
        for col in 0..w {
            let band = ((col + 4 * field_time) / 40 % 2) as u8 * 60;
            let square = (sx..sx + 160).contains(&col) && (sy..sy + 160).contains(&row);
            y[row * w + col] = if square {
                220
            } else {
                60 + band + (row * 80 / h) as u8
            };
        }
    }
    let (cw, ch) = (w / 2, h / 2);
    Frame420 {
        width: w as u16,
        height: h as u16,
        y,
        cb: (0..cw * ch)
            .map(|i| (100 + (i % cw) * 50 / cw) as u8)
            .collect(),
        cr: (0..cw * ch)
            .map(|i| (150 - (i / cw) * 40 / ch) as u8)
            .collect(),
    }
}

/// 30 frames of 1920 x 1080i 29.97 as field pictures (two B pictures between references, a closed GOP of 15).
fn field_picture_stream() -> Vec<u8> {
    let mut encoder = Encoder::new(EncoderConfig {
        standard: Standard::Ntsc,
        gop_size: 15,
        bitrate: 15_000_000,
        max_bitrate: 25_000_000,
        wide: true,
        format: Format::Hd1080,
        pulldown: false,
    })
    .unwrap();
    encoder.set_b_frames(2);
    encoder.set_field_pictures(true);
    let mut es = Vec::new();
    for t in 0..30 {
        for picture in encoder.push_frame(&frame(t), t % 15 == 0).unwrap() {
            es.extend_from_slice(&picture.data);
        }
    }
    for picture in encoder.flush().unwrap() {
        es.extend_from_slice(&picture.data);
    }
    es.extend_from_slice(&encoder.finish());
    es
}

/// The picture_structure of every picture coding extension in `data`.
fn structures(data: &[u8]) -> Vec<u8> {
    data.windows(7)
        .filter(|window| window[..4] == [0, 0, 1, 0xB5] && window[4] >> 4 == 8)
        .map(|window| window[6] & 3)
        .collect()
}

fn video_md5(ffmpeg: &Path, file: &Path) -> String {
    let output = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(file)
        .args(["-map", "0:v:0", "-f", "md5", "-"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{file:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn hd_mpeg2_field_pictures_go_on_the_disc_as_they_are() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let es_path = fixtures().join("mpeg2_fields_1080i.m2v");
    let ts_path = fixtures().join("mpeg2_fields_1080i.m2ts");
    if !es_path.is_file() || !ts_path.is_file() {
        std::fs::create_dir_all(fixtures()).unwrap();
        std::fs::write(&es_path, field_picture_stream()).unwrap();
        let made = Command::new(&ffmpeg)
            // A raw stream has no times: ffmpeg makes them (a frame a packet, both field pictures in it).
            .args([
                "-v",
                "error",
                "-y",
                "-fflags",
                "+genpts",
                "-f",
                "mpegvideo",
                "-i",
            ])
            .arg(&es_path)
            .args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
            .args([
                "-map", "0:v", "-map", "1:a", "-c:v", "copy", "-c:a", "ac3", "-b:a", "192k",
            ])
            .args(["-shortest", "-f", "mpegts", "-mpegts_m2ts_mode", "1"])
            .arg(&ts_path)
            .status();
        if !made.is_ok_and(|status| status.success()) {
            let _ = std::fs::remove_file(&ts_path);
            panic!("ffmpeg could not put the field pictures in a transport stream");
        }
    }
    let source_structures = structures(&std::fs::read(&es_path).unwrap());
    assert_eq!(source_structures.len(), 60);
    assert!(
        source_structures
            .iter()
            .all(|&structure| structure == 1 || structure == 2)
    );

    for (name, video) in [("ts", &ts_path), ("es", &es_path)] {
        let mut p = Project::default();
        p.settings.kind = DiscKind::BluRay;
        p.settings.bd_video = BdVideo::Hd1080i59;
        let v = p.add_asset(video).unwrap();
        let t = p.add_timeline(v, Some("Fields".to_string())).unwrap();
        p.settings.first_play = Link::Timeline { id: t, chapter: 0 };
        // Nothing to encode: the estimate counts the file as it is.
        let estimate = disccraft_dvd::estimate(&p).unwrap();
        assert!(estimate.passthrough_bytes > 0, "{name}: {estimate:?}");
        assert!(
            estimate.transcode_seconds.abs() < 1e-9,
            "{name}: {estimate:?}"
        );

        let out = fixtures().join(format!("bd_fields_{name}_out"));
        let _ = std::fs::remove_dir_all(&out);
        let report = build_bdmv(&p, &out).unwrap();
        assert_eq!(report.titles.len(), 1);
        let encoded: Vec<String> = std::fs::read_dir(out.join("disccraft-cache"))
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("hd-") || name.ends_with(".m2v"))
            .collect();
        assert!(encoded.is_empty(), "{name}: {encoded:?}");

        let stream = out.join("BDMV/STREAM/00001.m2ts");
        // The clip's video as an elementary stream (its headers can straddle the transport packets).
        let video_es = out.join("video.m2v");
        let copied = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-i"])
            .arg(&stream)
            .args(["-map", "0:v:0", "-c", "copy", "-f", "mpeg2video"])
            .arg(&video_es)
            .status()
            .unwrap();
        assert!(copied.success(), "{name}");
        assert_eq!(
            structures(&std::fs::read(&video_es).unwrap()),
            source_structures,
            "{name}"
        );
        // Not `-err_detect explode`: ffmpeg 9.0.2 answers every packet of field pictures with "invalid data" under
        // it (all intra I + I fields too; never frame pictures), without saying why, while it and craft-video
        // decode them to the same pictures (M12.38).
        let decode = Command::new(&ffmpeg)
            .args(["-v", "error", "-xerror", "-i"])
            .arg(&stream)
            .args(["-map", "0:v:0", "-f", "null", "-"])
            .output()
            .unwrap();
        assert!(
            decode.status.success() && decode.stderr.is_empty(),
            "{name}: {}",
            String::from_utf8_lossy(&decode.stderr)
        );
        assert_eq!(
            video_md5(&ffmpeg, &stream),
            video_md5(&ffmpeg, video),
            "{name}"
        );
        let _ = std::fs::remove_dir_all(&out);
    }
}
