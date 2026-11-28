//! M6.566: the caption files DiscCraft does not read itself (ASS / SSA, Scenarist SCC, MacCaption MCC, EBU STL, TTML /
//! DFXP, SAMI) are read with craft-libs' craft-captions into the same cues as the SRT they were made from. ffmpeg makes
//! the ASS, SSA and TTML files (those cases are skipped without it); craft-captions writes the SCC, MCC, EBU STL and
//! DFXP ones; the SAMI and the Shift_JIS ASS are written here.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use disccraft_test_support::ffmpeg;
use std::path::Path;
use std::process::Command;

use disccraft_menu::subtitle::Cue;
use disccraft_menu::subtitle_file::read_subtitles;
use disccraft_project::{AssetKind, DiscSettings};

const TICKS_PER_SECOND: f64 = 254_016_000_000.0;

const SRT: &str = "1\n00:00:01,000 --> 00:00:02,500\nHello\n\n2\n00:00:03,000 --> 00:00:04,200\n\
<i>Second line</i>\nand more\n\n3\n00:00:05,000 --> 00:00:06,000\nこんにちは、世界\n";

/// The cues as (start s, end s, text).
fn cues(cues: &[Cue]) -> Vec<(f64, f64, String)> {
    cues.iter()
        .map(|cue| {
            (
                cue.start.0 as f64 / TICKS_PER_SECOND,
                cue.end.0 as f64 / TICKS_PER_SECOND,
                cue.text.clone(),
            )
        })
        .collect()
}

/// Checks that `path` reads as `want`, the times within `slack` seconds.
fn check(path: &Path, want: &[(f64, f64, String)], slack: f64) {
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(
        AssetKind::guess_from_path(path),
        AssetKind::Subtitle,
        "{name}"
    );
    let got = cues(&read_subtitles(path, &DiscSettings::default()).unwrap());
    assert_eq!(got.len(), want.len(), "{name}: {got:?}");
    for (got, want) in got.iter().zip(want) {
        assert!(
            (got.0 - want.0).abs() <= slack && (got.1 - want.1).abs() <= slack,
            "{name}: {got:?} against {want:?}"
        );
        assert_eq!(got.2, want.2, "{name}");
    }
}

#[test]
fn caption_files_read_as_the_srt_they_were_made_from() {
    let dir = std::env::temp_dir().join(format!("disccraft-captions-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let srt = dir.join("source.srt");
    std::fs::write(&srt, SRT).unwrap();
    let all = cues(&read_subtitles(&srt, &DiscSettings::default()).unwrap());
    assert_eq!(all.len(), 3);
    assert_eq!(all[1].2, "Second line\nand more");
    // The formats of CEA-608 and EBU STL's Latin table carry the English cues only.
    let english = &all[..2];

    // Written by craft-captions from the English cues: SCC and MCC at 29.97 frames, EBU STL at 25; DFXP with all.
    let english_srt = craft_captions::parse(
        SRT.split("\n\n3\n").next().unwrap().as_bytes(),
        craft_captions::Format::Srt,
    )
    .unwrap();
    let full = craft_captions::parse(SRT.as_bytes(), craft_captions::Format::Srt).unwrap();
    for (name, document, format, slack) in [
        (
            "written.scc",
            &english_srt,
            craft_captions::Format::Scc,
            0.1,
        ),
        (
            "written.mcc",
            &english_srt,
            craft_captions::Format::Mcc,
            0.1,
        ),
        (
            "written.stl",
            &english_srt,
            craft_captions::Format::Stl,
            0.05,
        ),
        ("written.dfxp", &full, craft_captions::Format::Dfxp, 0.001),
    ] {
        let path = dir.join(name);
        let bytes =
            craft_captions::write(document, format, craft_captions::WriteOptions::default());
        std::fs::write(&path, bytes).unwrap();
        let want = if document.cues.len() == 2 {
            english
        } else {
            &all[..]
        };
        check(&path, want, slack);
    }

    // SAMI, as Windows Media Player's captions were written.
    let sami = dir.join("hand.smi");
    std::fs::write(
        &sami,
        "<SAMI><HEAD><STYLE TYPE=\"text/css\"><!-- .JACC { Name: Japanese; lang: ja-JP; } --></STYLE></HEAD><BODY>\n\
         <SYNC Start=1000><P Class=JACC>Hello\n\
         <SYNC Start=2500><P Class=JACC>&nbsp;\n\
         <SYNC Start=3000><P Class=JACC><i>Second line</i><br>and more\n\
         <SYNC Start=4200><P Class=JACC>&nbsp;\n\
         <SYNC Start=5000><P Class=JACC>こんにちは、世界\n\
         <SYNC Start=6000><P Class=JACC>&nbsp;\n\
         </BODY></SAMI>\n",
    )
    .unwrap();
    check(&sami, &all, 0.001);

    // An ASS file in Shift_JIS, as Japanese fansub files often are, with override tags.
    let ass = dir.join("shift_jis.ass");
    let script = "[Script Info]\r\nScriptType: v4.00+\r\n\r\n[V4+ Styles]\r\nFormat: Name, Fontname, Fontsize\r\n\
        Style: Default,Arial,20\r\n\r\n[Events]\r\n\
        Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\r\n\
        Dialogue: 0,0:00:01.00,0:00:02.50,Default,,0,0,0,,{\\an8}Hello\r\n\
        Dialogue: 0,0:00:03.00,0:00:04.20,Default,,0,0,0,,{\\i1}Second line{\\i0}\\Nand more\r\n\
        Dialogue: 0,0:00:05.00,0:00:06.00,Default,,0,0,0,,{\\c&H00FFFF&}こんにちは、世界\r\n";
    let (encoded, _, unmappable) = encoding_rs::SHIFT_JIS.encode(script);
    assert!(!unmappable);
    std::fs::write(&ass, &encoded).unwrap();
    check(&ass, &all, 0.001);

    // Made by ffmpeg from the SRT: ASS, SSA (centisecond times) and TTML.
    if let Some(ffmpeg) = ffmpeg() {
        for name in ["ffmpeg.ass", "ffmpeg.ssa", "ffmpeg.ttml"] {
            let path = dir.join(name);
            let status = Command::new(&ffmpeg)
                .args(["-v", "error", "-y", "-i"])
                .arg(&srt)
                .arg(&path)
                .status()
                .unwrap();
            assert!(status.success(), "{name}");
            check(&path, &all, 0.01);
        }
    } else {
        eprintln!("ffmpeg not found; the ffmpeg-made files are skipped");
    }

    // A file that is not what its name says is an error, not a panic.
    let wrong = dir.join("wrong.scc");
    std::fs::write(&wrong, "not a caption file").unwrap();
    assert!(read_subtitles(&wrong, &DiscSettings::default()).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
