//! M9.41 check: `disccraft-cli bd-chapters` lists a Blu-ray's playlists and prints the chapters of the longest,
//! of one asked for, as JSON and as an OGM chapter file.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_bdmv::mpls::{PlayItem, PlayList, PlayListMark, write_mpls};
use std::path::Path;
use std::process::Command;

/// 45 kHz.
const S: u32 = 45_000;

fn cli(args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_disccraft-cli"))
        .args(args)
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn playlist(items: &[(&[u8; 5], u32, u32)], marks: &[(u16, u32)]) -> Vec<u8> {
    write_mpls(&PlayList {
        items: items
            .iter()
            .map(|(clip, from, to)| PlayItem {
                clip: **clip,
                in_time: *from,
                out_time: *to,
                ..PlayItem::default()
            })
            .collect(),
        marks: marks
            .iter()
            .map(|(play_item, time)| PlayListMark {
                play_item: *play_item,
                time: *time,
            })
            .collect(),
        ..PlayList::default()
    })
    .unwrap()
}

#[test]
fn a_blu_rays_chapters_are_listed() {
    let dir = std::env::temp_dir().join(format!("disccraft-bd-chapters-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let folder = dir.join("BDMV").join("PLAYLIST");
    std::fs::create_dir_all(&folder).unwrap();
    // 00000: a short extra; 00001: the main title, two clips (1:00:00 + 0:30:00), chapters at 0, 0:20:00.5,
    // 1:00:00 and 1:10:00.
    std::fs::write(
        folder.join("00000.mpls"),
        playlist(&[(b"00009", 0, 90 * S)], &[(0, 0)]),
    )
    .unwrap();
    std::fs::write(
        folder.join("00001.mpls"),
        playlist(
            &[(b"00001", 600 * S, 4_200 * S), (b"00002", 0, 1_800 * S)],
            &[(0, 600 * S), (0, 1_800 * S + S / 2), (1, 0), (1, 600 * S)],
        ),
    )
    .unwrap();
    std::fs::write(folder.join("00002.mpls"), b"MPLS0200 broken").unwrap();
    let root = dir.to_string_lossy().into_owned();

    let (ok, out, err) = cli(&["bd-chapters", &root]);
    assert!(ok, "{err}");
    for line in [
        "  00000      0:01:30.000         1  00009",
        "* 00001      1:30:00.000         4  00001+00002",
        "  00002     cannot be read:",
        "00001.mpls: 1:30:00.000, 4 chapters",
        "   1   0:00:00.000   0:20:00.500",
        "   2   0:20:00.500   0:39:59.500",
        "   3   1:00:00.000   0:10:00.000",
        "   4   1:10:00.000   0:20:00.000",
    ] {
        assert!(out.contains(line), "missing {line:?} in\n{out}");
    }

    // Another playlist, named any way; the PLAYLIST folder or one file works too.
    for (path, wanted) in [
        (root.clone(), "0"),
        (folder.to_string_lossy().into_owned(), "00000.mpls"),
    ] {
        let (ok, out, err) = cli(&["bd-chapters", &path, "--playlist", wanted]);
        assert!(ok, "{err}");
        assert!(out.contains("00000.mpls: 0:01:30.000, 1 chapters"), "{out}");
    }
    let file = folder.join("00001.mpls").to_string_lossy().into_owned();
    let (ok, out, _) = cli(&["bd-chapters", &file]);
    assert!(
        ok && out.contains("00001.mpls: 1:30:00.000, 4 chapters"),
        "{out}"
    );

    let (ok, out, err) = cli(&["bd-chapters", &root, "--json"]);
    assert!(ok, "{err}");
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["chosen"], serde_json::json!(["00001"]));
    let starts: Vec<f64> = value["playlists"][1]["chapters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|chapter| chapter["start"].as_f64().unwrap())
        .collect();
    assert_eq!(starts, [0.0, 1_200.5, 3_600.0, 4_200.0]);
    assert!(value["playlists"][2]["error"].is_string());

    let (ok, out, err) = cli(&["bd-chapters", &root, "--ogm"]);
    assert!(ok, "{err}");
    assert!(
        out.starts_with(
            "CHAPTER01=00:00:00.000\nCHAPTER01NAME=Chapter 01\nCHAPTER02=00:20:00.500\n"
        ) || out.starts_with(
            "CHAPTER01=00:00:00.000\r\nCHAPTER01NAME=Chapter 01\r\nCHAPTER02=00:20:00.500\r\n"
        ),
        "{out}"
    );

    // Nothing there, or no such playlist: an error, not a panic.
    let (ok, _, err) = cli(&["bd-chapters", &dir.join("nothing").to_string_lossy()]);
    assert!(!ok && err.contains("no Blu-ray playlists"), "{err}");
    let (ok, _, err) = cli(&["bd-chapters", &root, "--playlist", "7"]);
    assert!(!ok && err.contains("no playlist 7"), "{err}");
    assert!(Path::new(&root).is_dir());
    let _ = std::fs::remove_dir_all(&dir);
}
