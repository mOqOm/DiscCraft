//! DV decoded by this crate against ffmpeg as an outside oracle (AGENTS.md rule 9): ffmpeg makes raw DV files
//! (525/60 4:1:1, 625/50 4:2:0, 16:9, with 48 kHz sound), decodes them, and the pictures and the sound are
//! compared. Skipped without ffmpeg (DISCCRAFT_FFMPEG or `ffmpeg` on the path); the files are kept in
//! target/fixtures.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_test_support::{ffmpeg, fixtures, temp_path};
use std::path::{Path, PathBuf};
use std::process::Command;

use disccraft_dv::{Layout, Sampling, System};

/// A raw DV file of 2 seconds of moving test picture (and a 440 Hz / 880 Hz stereo tone), made by ffmpeg.
/// `interlaced`: fields of a picture at twice the rate (moving between them), coded with the 2-4-8 DCT where the
/// encoder finds it better.
fn dv_file(
    ffmpeg: &Path,
    name: &str,
    size: &str,
    rate: &str,
    pixels: &str,
    aspect: &str,
    interlaced: bool,
) -> PathBuf {
    let path = fixtures().join(name);
    if path.is_file() {
        return path;
    }
    std::fs::create_dir_all(fixtures()).unwrap();
    // Made beside it and moved into place: several tests ask for the same file at once, and one must not read
    // another's half-written one.
    let part = temp_path(&path);
    let (source, filter) = if interlaced {
        (
            format!("testsrc2=size={size}:rate={rate}*2"),
            "[0]tinterlace=mode=interleave_bottom,setfield=bff[v];[1][2]amerge=inputs=2[a]",
        )
    } else {
        (
            format!("testsrc2=size={size}:rate={rate}"),
            "[0]null[v];[1][2]amerge=inputs=2[a]",
        )
    };
    let ok = Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(source)
        .args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
        .args(["-f", "lavfi", "-i", "sine=frequency=880:sample_rate=48000"])
        .args(["-filter_complex", filter, "-map", "[v]", "-map", "[a]"])
        .args([
            "-t", "2", "-c:v", "dvvideo", "-pix_fmt", pixels, "-aspect", aspect,
        ])
        .args(if interlaced {
            &["-flags", "+ildct"][..]
        } else {
            &[][..]
        })
        .args(["-c:a", "pcm_s16le", "-f", "dv"])
        .arg(&part)
        .status()
        .unwrap()
        .success();
    assert!(ok, "ffmpeg could not make {name}");
    // Another test may have moved its copy into place first.
    if std::fs::rename(&part, &path).is_err() {
        let _ = std::fs::remove_file(&part);
        assert!(path.is_file(), "{name} could not be moved into place");
    }
    path
}

/// ffmpeg's decoding of `path`: raw planar Y'CbCr frames, and the sound as 16-bit stereo.
fn ffmpeg_decoded(ffmpeg: &Path, path: &Path, pixels: &str) -> (Vec<u8>, Vec<i16>) {
    let video = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-map", "0:v", "-f", "rawvideo", "-pix_fmt", pixels, "-"])
        .output()
        .unwrap();
    assert!(video.status.success());
    let audio = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-map", "0:a:0", "-f", "s16le", "-ac", "2", "-"])
        .output()
        .unwrap();
    assert!(audio.status.success());
    let samples = audio
        .stdout
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&b| i16::from_le_bytes(b))
        .collect();
    (video.stdout, samples)
}

/// How many DCT blocks of `frame` are coded 2-4-8: the mode bit after the 9-bit DC at the start of each of the six
/// areas of each video DIF block (Fig. 30).
fn blocks_248(frame: &[u8], system: System, sampling: Sampling) -> usize {
    let mut count = 0;
    // 50 Mb/s: two channels, and four blocks a macro block (Fig. 29).
    let (channels, areas): (usize, &[usize]) = if sampling == Sampling::C422 {
        (2, &[8, 232, 456, 536])
    } else {
        (1, &[8, 120, 232, 344, 456, 536])
    };
    for sequence in 0..system.sequences() * channels {
        for block in 0..150 {
            let start = sequence * 12_000 + block * 80;
            if frame[start] >> 5 != 4 {
                continue;
            }
            for &area in areas {
                let bit = area + 9;
                if frame[start + 3 + bit / 8] >> (7 - bit % 8) & 1 == 1 {
                    count += 1;
                }
            }
        }
    }
    count
}

/// The peak signal-to-noise ratio of `a` against `b`, in dB.
fn psnr(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let error: f64 = a
        .iter()
        .zip(b)
        .map(|(&x, &y)| (f64::from(x) - f64::from(y)).powi(2))
        .sum::<f64>()
        / a.len() as f64;
    if error == 0.0 {
        return 99.0;
    }
    10.0 * (255.0f64.powi(2) / error).log10()
}

/// Decodes every frame of `path` and compares it with ffmpeg's: the pictures, the header and the sound.
fn compare(ffmpeg: &Path, path: &Path, system: System, sampling: Sampling, wide: bool) -> usize {
    let bytes = std::fs::read(path).unwrap();
    // 50 Mb/s (4:2:2) frames are twice as long: two channels of DIF sequences.
    let frame_bytes = system.frame_bytes() * if sampling == Sampling::C422 { 2 } else { 1 };
    assert_eq!(bytes.len() % frame_bytes, 0);
    let frames = bytes.len() / frame_bytes;
    let (width, height) = (720, system.height());
    // 4:1:1 is compared at its own samples: ffmpeg gives yuv411p, this crate 4:2:2 with every other sample its own.
    let pixels = match sampling {
        Sampling::C411 => "yuv411p",
        Sampling::C420 => "yuv420p",
        Sampling::C422 => "yuv422p",
    };
    let (video, sound) = ffmpeg_decoded(ffmpeg, path, pixels);
    let (chroma_width, chroma_height) = match sampling {
        Sampling::C411 => (width / 4, height),
        Sampling::C420 => (width / 2, height / 2),
        Sampling::C422 => (width / 2, height),
    };
    let picture_bytes = width * height + 2 * chroma_width * chroma_height;
    assert_eq!(video.len(), frames * picture_bytes, "ffmpeg's frame count");
    let mut our_sound: Vec<i16> = Vec::new();
    let mut lowest = f64::MAX;
    let mut field_blocks = 0;
    for (index, frame) in bytes.chunks_exact(frame_bytes).enumerate() {
        assert!(disccraft_dv::sniff(frame));
        let header = disccraft_dv::header(frame).unwrap();
        field_blocks += blocks_248(frame, system, sampling);
        assert_eq!(
            (header.system, header.sampling, header.wide),
            (system, sampling, wide)
        );
        let picture = disccraft_dv::decode_video(frame).unwrap();
        let theirs = &video[index * picture_bytes..(index + 1) * picture_bytes];
        let (y, rest) = theirs.split_at(width * height);
        let (cb, cr) = rest.split_at(chroma_width * chroma_height);
        let ours_cb: Vec<u8>;
        let ours_cr: Vec<u8>;
        match picture.layout {
            Layout::Yuv422 if sampling == Sampling::C411 => {
                ours_cb = picture.cb.iter().step_by(2).copied().collect();
                ours_cr = picture.cr.iter().step_by(2).copied().collect();
            }
            Layout::Yuv422 => {
                ours_cb = picture.cb.clone();
                ours_cr = picture.cr.clone();
            }
            Layout::Yuv420 => {
                ours_cb = picture.cb.clone();
                ours_cr = picture.cr.clone();
            }
        }
        let (py, pb, pr) = (psnr(&picture.y, y), psnr(&ours_cb, cb), psnr(&ours_cr, cr));
        assert!(
            py > 45.0 && pb > 45.0 && pr > 45.0,
            "frame {index}: Y {py:.1} Cb {pb:.1} Cr {pr:.1} dB"
        );
        lowest = lowest.min(py).min(pb).min(pr);
        let audio = disccraft_dv::decode_audio(frame).unwrap().unwrap();
        assert_eq!(audio.sample_rate, 48_000);
        for i in 0..audio.channels[0].len() {
            our_sound.push(audio.channels[0][i]);
            our_sound.push(audio.channels[1][i]);
        }
    }
    eprintln!(
        "{}: {frames} frames, {field_blocks} blocks 2-4-8, lowest PSNR {lowest:.1} dB",
        path.display()
    );
    assert_eq!(our_sound.len(), sound.len(), "samples");
    assert!(our_sound == sound, "the sound differs from ffmpeg's");
    field_blocks
}

#[test]
fn ntsc_dv_decodes_as_ffmpeg_decodes_it() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found: skipped");
        return;
    };
    let path = dv_file(
        &ffmpeg,
        "dv_ntsc.dv",
        "720x480",
        "30000/1001",
        "yuv411p",
        "4:3",
        false,
    );
    let _ = compare(&ffmpeg, &path, System::Lines525, Sampling::C411, false);
    // Interlaced, so the 2-4-8 DCT is used.
    let path = dv_file(
        &ffmpeg,
        "dv_ntsc_fields.dv",
        "720x480",
        "30000/1001",
        "yuv411p",
        "4:3",
        true,
    );
    let field_blocks = compare(&ffmpeg, &path, System::Lines525, Sampling::C411, false);
    assert!(field_blocks > 1_000, "{field_blocks} blocks coded 2-4-8");
}

#[test]
fn pal_dv_decodes_as_ffmpeg_decodes_it() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found: skipped");
        return;
    };
    let path = dv_file(
        &ffmpeg,
        "dv_pal_wide.dv",
        "720x576",
        "25",
        "yuv420p",
        "16:9",
        true,
    );
    let field_blocks = compare(&ffmpeg, &path, System::Lines625, Sampling::C420, true);
    assert!(field_blocks > 1_000, "{field_blocks} blocks coded 2-4-8");
}

/// DVCPRO50 (4:2:2, 50 Mb/s): two channels of DIF sequences, the second coding the odd super block rows, and an
/// additional AC area after each Y block (M6.415).
#[test]
fn fifty_megabit_dv_decodes_as_ffmpeg_decodes_it() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found: skipped");
        return;
    };
    let path = dv_file(
        &ffmpeg,
        "dv50_ntsc.dv",
        "720x480",
        "30000/1001",
        "yuv422p",
        "4:3",
        true,
    );
    let field_blocks = compare(&ffmpeg, &path, System::Lines525, Sampling::C422, false);
    assert!(field_blocks > 1_000, "{field_blocks} blocks coded 2-4-8");
    let path = dv_file(
        &ffmpeg,
        "dv50_pal_wide.dv",
        "720x576",
        "25",
        "yuv422p",
        "16:9",
        false,
    );
    let _ = compare(&ffmpeg, &path, System::Lines625, Sampling::C422, true);
}

#[test]
fn a_broken_frame_does_not_panic() {
    // Every byte of a frame's video section set to each value: no panic, a picture or an error.
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found: skipped");
        return;
    };
    let path = dv_file(
        &ffmpeg,
        "dv_ntsc.dv",
        "720x480",
        "30000/1001",
        "yuv411p",
        "4:3",
        false,
    );
    let bytes = std::fs::read(path).unwrap();
    let frame = &bytes[..System::Lines525.frame_bytes()];
    for value in [0x00u8, 0xFF, 0x55, 0xF0] {
        let mut broken = frame.to_vec();
        for (i, byte) in broken.iter_mut().enumerate() {
            // Leave the header block so the frame is still taken for DV.
            if i >= 80 {
                *byte = value;
            }
        }
        let _ = disccraft_dv::decode_video(&broken);
        let _ = disccraft_dv::decode_audio(&broken);
    }
    for cut in [0, 7, 80, 1_000, 119_999] {
        assert!(disccraft_dv::decode_video(&frame[..cut]).is_err());
    }
}

/// Rewrites the sound of the DV frames in `bytes` (made by ffmpeg at 48 kHz) into another mode: the AAUX source packs
/// say `frequency` (0: 48, 1: 44.1, 2: 32 kHz), `quantisation` (0: 16 bits, 1: 12 bits) and AF SIZE `size`, and the
/// samples are numbered through the file (12 bits: the codes in turn, both pairs). The samples are placed as this
/// crate reads them, so a decoder that reads them otherwise (ffmpeg, the oracle) gets other values.
fn resound(bytes: &mut [u8], system: System, frequency: u8, quantisation: u8, size: u8) {
    let group = system.sequences() / 2;
    let unit = if quantisation == 1 { 3 } else { 2 };
    for (f, frame) in bytes.chunks_exact_mut(system.frame_bytes()).enumerate() {
        let block = |sequence: usize, block: usize| sequence * 12_000 + (6 + 16 * block) * 80;
        for sequence in 0..system.sequences() {
            for b in 0..9 {
                let at = block(sequence, b) + 3;
                if frame[at] == 0x50 {
                    frame[at + 1] = (frame[at + 1] & 0xC0) | size;
                    frame[at + 4] = (frame[at + 4] & 0xC0) | (frequency << 3) | quantisation;
                }
            }
        }
        for half in 0..2 {
            for n in 0..9 * group * (72 / unit) {
                let sequence = (n / 3 + 2 * (n % 3)) % group + half * group;
                let b = 3 * (n % 3) + (n % (9 * group)) / (3 * group);
                let at = block(sequence, b) + 8 + unit * (n / (9 * group));
                let number = f * 2_000 + n + 1_000 * half;
                if unit == 2 {
                    let value = ((number * 37) % 60_000) as i32 - 30_000;
                    frame[at..at + 2].copy_from_slice(&(value as i16).to_be_bytes());
                } else {
                    // Every 12-bit code but the error code 0x800.
                    let code = |k: usize| match (k % 4_096) as u16 {
                        0x800 => 0x801,
                        c => c,
                    };
                    let (left, right) = (code(number), code(number * 3 + 7));
                    frame[at] = (left >> 4) as u8;
                    frame[at + 1] = (right >> 4) as u8;
                    frame[at + 2] = (((left & 0x0F) << 4) | (right & 0x0F)) as u8;
                }
            }
        }
    }
}

/// The sound of the other modes (IEC 61834's 44.1 and 32 kHz, and a camcorder's 12-bit 32 kHz) read as ffmpeg reads
/// it: the same samples, as many in each frame.
#[test]
fn other_sound_modes_decode_as_ffmpeg_decodes_them() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found: skipped");
        return;
    };
    let ntsc = dv_file(
        &ffmpeg,
        "dv_ntsc.dv",
        "720x480",
        "30000/1001",
        "yuv411p",
        "4:3",
        false,
    );
    let pal = dv_file(
        &ffmpeg,
        "dv_pal_wide.dv",
        "720x576",
        "25",
        "yuv420p",
        "16:9",
        true,
    );
    for (source, system, frequency, quantisation, size, rate, bits) in [
        (&ntsc, System::Lines525, 2u8, 1u8, 15u8, 32_000u32, 12u32),
        (&ntsc, System::Lines525, 2, 0, 15, 32_000, 16),
        (&ntsc, System::Lines525, 1, 0, 15, 44_100, 16),
        (&pal, System::Lines625, 2, 1, 16, 32_000, 12),
        (&pal, System::Lines625, 1, 0, 22, 44_100, 16),
    ] {
        let mut bytes = std::fs::read(source).unwrap();
        resound(&mut bytes, system, frequency, quantisation, size);
        let path = std::env::temp_dir().join(format!(
            "disccraft-dv-sound-{}-{frequency}-{quantisation}-{}.dv",
            system.height(),
            std::process::id()
        ));
        std::fs::write(&path, &bytes).unwrap();
        // 12 bits: sound 2 too (M6.418), as ffmpeg's second sound stream.
        let pairs = if bits == 12 { 2 } else { 1 };
        for pair in 0..pairs {
            let theirs = ffmpeg_sound(&ffmpeg, &path, pair);
            let mut ours: Vec<i16> = Vec::new();
            for frame in bytes.chunks_exact(system.frame_bytes()) {
                let audio = disccraft_dv::decode_audio_pair(frame, pair)
                    .unwrap()
                    .unwrap();
                assert_eq!((audio.sample_rate, audio.bits), (rate, bits));
                for i in 0..audio.channels[0].len() {
                    ours.push(audio.channels[0][i]);
                    ours.push(audio.channels[1][i]);
                }
            }
            assert_eq!(
                ours.len(),
                theirs.len(),
                "{rate} Hz {bits} bits {:?} pair {pair}: samples",
                system
            );
            let first_difference = ours.iter().zip(&theirs).position(|(a, b)| a != b);
            assert_eq!(
                first_difference, None,
                "{rate} Hz {bits} bits {:?} pair {pair}",
                system
            );
        }
        if bits == 16 {
            assert!(
                disccraft_dv::decode_audio_pair(&bytes, 1)
                    .unwrap()
                    .is_none(),
                "16 bits: one pair"
            );
        }
        let _ = std::fs::remove_file(&path);
    }
}

/// ffmpeg's decoding of sound stream `stream` of `path`, 16-bit stereo.
fn ffmpeg_sound(ffmpeg: &Path, path: &Path, stream: usize) -> Vec<i16> {
    let decoded = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-map",
            &format!("0:a:{stream}"),
            "-f",
            "s16le",
            "-ac",
            "2",
            "-",
        ])
        .output()
        .unwrap();
    assert!(decoded.status.success());
    decoded
        .stdout
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&b| i16::from_le_bytes(b))
        .collect()
}

/// DVCPRO50's CH3 and CH4 (its second channel) as ffmpeg's second sound stream (M6.418).
#[test]
fn dvcpro50s_second_pair_decodes_as_ffmpeg_decodes_it() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found: skipped");
        return;
    };
    // Two stereo pairs: 440 / 880 Hz on CH1 / CH2, 660 / 1320 Hz on CH3 / CH4.
    let path = fixtures().join("dv50_four.dv");
    if !path.is_file() {
        std::fs::create_dir_all(fixtures()).unwrap();
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
                "-f",
                "lavfi",
                "-i",
                "aevalsrc=exprs=sin(2*PI*440*t)/2|sin(2*PI*880*t)/2:s=48000",
            ])
            .args([
                "-f",
                "lavfi",
                "-i",
                "aevalsrc=exprs=sin(2*PI*660*t)/2|sin(2*PI*1320*t)/2:s=48000",
            ])
            .args(["-map", "0:v", "-map", "1:a", "-map", "2:a", "-t", "1"])
            .args([
                "-c:v",
                "dvvideo",
                "-pix_fmt",
                "yuv422p",
                "-c:a",
                "pcm_s16le",
                "-f",
                "dv",
            ])
            .arg(&path)
            .status()
            .unwrap()
            .success();
        assert!(ok, "ffmpeg could not make the fixture");
    }
    let bytes = std::fs::read(&path).unwrap();
    let theirs = ffmpeg_sound(&ffmpeg, &path, 1);
    let mut ours: Vec<i16> = Vec::new();
    for frame in bytes.chunks_exact(System::Lines525.frame_bytes() * 2) {
        let Some(audio) = disccraft_dv::decode_audio_pair(frame, 1).unwrap() else {
            panic!("no second pair");
        };
        for i in 0..audio.channels[0].len() {
            ours.push(audio.channels[0][i]);
            ours.push(audio.channels[1][i]);
        }
    }
    assert_eq!(ours.len(), theirs.len());
    assert!(ours == theirs, "CH3 / CH4 differ from ffmpeg's");
}
