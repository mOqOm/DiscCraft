//! M13.11 check: ROM content (Encore's DVD-ROM / BD-ROM content) is on the built disc beside the video: copied
//! next to `VIDEO_TS` / `BDMV` in a folder build, and in the image's file systems in an image build, where the
//! video still plays. Needs the `dvd_iso.m2v` fixture (made by `dvd_iso.rs` with ffmpeg); skipped without.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_project::{DiscKind, Link, Project};
use disccraft_test_support::{ffmpeg, fixtures};
use disccraft_udf::read::{list_iso9660, list_udf};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The files of the test content: path and bytes.
fn content() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("Read me.txt", b"Open the Extras folder.".to_vec()),
        ("Extras/Booklet.pdf", vec![0x25_u8; 70_000]),
        (
            "Extras/\u{58c1}\u{7d19}/\u{5199}\u{771f} 1.png",
            vec![0x89_u8; 3000],
        ),
    ]
}

/// A project of one title with the test content in `dir/content`.
fn project(dir: &Path, kind: DiscKind) -> Option<Project> {
    let video = fixtures().join("dvd_iso.m2v");
    if !video.is_file() {
        eprintln!("dvd_iso.m2v fixture not found; skipping");
        return None;
    }
    let folder = dir.join("content");
    for (path, bytes) in content() {
        let file = folder.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, bytes).unwrap();
    }
    let mut project = Project::default();
    project.settings.kind = kind;
    let v = project.add_asset(&video).unwrap();
    let t = project.add_timeline(v, Some("Main".to_string())).unwrap();
    project.settings.first_play = Link::Timeline { id: t, chapter: 0 };
    project.settings.rom_content = Some(folder.to_string_lossy().into_owned());
    Some(project)
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("disccraft-rom-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn options(output: OutputKind, path: &Path) -> BuildOptions {
    BuildOptions {
        output,
        path: path.to_path_buf(),
        cache: None,
        audio_language: Some("ja".to_owned()),
    }
}

fn read_at(image: &Path, sector: u32, len: usize) -> Vec<u8> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(image).unwrap();
    file.seek(SeekFrom::Start(u64::from(sector) * 2048))
        .unwrap();
    let mut bytes = vec![0_u8; len];
    file.read_exact(&mut bytes).unwrap();
    bytes
}

#[test]
fn a_dvd_image_holds_the_rom_content_beside_the_video() {
    let dir = temp("dvdiso");
    let Some(project) = project(&dir, DiscKind::Dvd) else {
        return;
    };
    // The size of the disc counts the content, and so does what is left for the video.
    let estimate = disccraft_dvd::estimate(&project).unwrap();
    assert_eq!(estimate.rom_bytes, (1 + 35 + 2 + 3) * 2048);
    let mut without = project.clone();
    without.settings.rom_content = None;
    let plain = disccraft_dvd::estimate(&without).unwrap();
    assert_eq!(plain.rom_bytes, 0);
    assert_eq!(
        estimate.estimated_bytes,
        plain.estimated_bytes + estimate.rom_bytes
    );

    let iso = dir.join("disc.iso");
    let report = build(&project, &options(OutputKind::Iso, &iso)).unwrap();
    assert_eq!(report.total_bytes, std::fs::metadata(&iso).unwrap().len());
    let plain_iso = dir.join("plain.iso");
    let _ = build(&without, &options(OutputKind::Iso, &plain_iso)).unwrap();

    let udf = list_udf(&iso).unwrap();
    let iso9660 = list_iso9660(&iso).unwrap();
    for (path, bytes) in content() {
        let file = udf
            .iter()
            .find(|file| file.path == path)
            .unwrap_or_else(|| panic!("{path} is not in the image: {udf:?}"));
        assert_eq!(file.size, bytes.len() as u64, "{path}");
        assert_eq!(read_at(&iso, file.sector, bytes.len()), bytes, "{path}");
        // ISO 9660 has the file too, under an 8.3 name, at the same place.
        assert!(
            iso9660
                .iter()
                .any(|other| other.sector == file.sector && other.size == file.size),
            "{path}: {iso9660:?}"
        );
    }
    // The video is where it is in an image without the content.
    let video = |listing: &[disccraft_udf::read::ListedFile]| -> Vec<(String, u32, u64)> {
        listing
            .iter()
            .filter(|file| file.path.starts_with("VIDEO_TS/"))
            .map(|file| (file.path.clone(), file.sector, file.size))
            .collect()
    };
    assert_eq!(video(&udf), video(&list_udf(&plain_iso).unwrap()));

    // The disc still plays: ffmpeg's DVD reader (libdvdread's own UDF reader) decodes the title.
    if let Some(ffmpeg) = ffmpeg() {
        let decode = Command::new(ffmpeg)
            .args(["-v", "error", "-f", "dvdvideo", "-i"])
            .arg(&iso)
            .args(["-map", "0:v:0", "-f", "null", "-"])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&decode.stderr);
        assert!(decode.status.success(), "{stderr}");
    }
    if std::env::var_os("DISCCRAFT_KEEP_OUTPUT").is_none() {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_folder_build_copies_the_rom_content_beside_the_video() {
    for kind in [DiscKind::Dvd, DiscKind::BluRay] {
        let dir = temp("folder");
        let Some(project) = project(&dir, kind) else {
            return;
        };
        let out = dir.join("out");
        let report = build(&project, &options(OutputKind::Folder, &out)).unwrap();
        let own = if kind == DiscKind::Dvd {
            "VIDEO_TS"
        } else {
            "BDMV"
        };
        assert!(out.join(own).is_dir(), "{kind:?}");
        let mut content_bytes = 0;
        for (path, bytes) in content() {
            assert_eq!(
                std::fs::read(out.join(path)).unwrap(),
                bytes,
                "{kind:?} {path}"
            );
            content_bytes += bytes.len() as u64;
        }
        assert!(report.total_bytes > content_bytes);

        // The disc's own folder in the content is refused before anything is built.
        let folder = PathBuf::from(project.settings.rom_content.clone().unwrap());
        std::fs::create_dir_all(folder.join(own.to_lowercase())).unwrap();
        std::fs::write(folder.join(own.to_lowercase()).join("x.txt"), b"x").unwrap();
        let error =
            build(&project, &options(OutputKind::Folder, &dir.join("refused"))).unwrap_err();
        assert!(error.contains("ROM content"), "{error}");
        assert!(!dir.join("refused").exists());

        // A content folder that is gone is an error, not a disc without it.
        let mut gone = project.clone();
        gone.settings.rom_content = Some(dir.join("gone").to_string_lossy().into_owned());
        let error = build(&gone, &options(OutputKind::Folder, &dir.join("gone-out"))).unwrap_err();
        assert!(error.contains("does not exist"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_blu_ray_image_holds_the_rom_content() {
    let dir = temp("bdiso");
    let Some(project) = project(&dir, DiscKind::BluRay) else {
        return;
    };
    let iso = dir.join("disc.iso");
    let report = build(&project, &options(OutputKind::Iso, &iso)).unwrap();
    let mut without = project.clone();
    without.settings.rom_content = None;
    let plain = build(&without, &options(OutputKind::Iso, &dir.join("plain.iso"))).unwrap();
    // The image grows by the content: each file's sectors (1 + 35 + 2) and their entries and folders, rounded
    // to the image's blocks.
    let grown = report.total_bytes - plain.total_bytes;
    assert!(
        (38 * 2048..=512 * 2048).contains(&grown),
        "grew by {grown} bytes"
    );
    // The files are in the image whole (the booklet is 70,000 bytes of one value).
    let image = std::fs::read(&iso).unwrap();
    let booklet = vec![0x25_u8; 70_000];
    assert!(
        image
            .windows(booklet.len())
            .any(|window| window == booklet.as_slice())
    );
    if std::env::var_os("DISCCRAFT_KEEP_OUTPUT").is_none() {
        let _ = std::fs::remove_dir_all(&dir);
    }
}
