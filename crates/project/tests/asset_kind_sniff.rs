//! M6.568: an imported file's kind comes from its first bytes (craft-libs' craft-sniff) where they name a format
//! DiscCraft reads, and from its name otherwise: a movie, a sound or a picture under a wrong name or none is taken
//! for what it is; subtitles, DiscCraft's Craft documents, files with no signature and files that cannot be read go by
//! their names. ffmpeg makes the MP4 and Matroska files (that part is skipped without it).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use disccraft_test_support::ffmpeg;
use std::path::Path;
use std::process::Command;

use disccraft_project::{AssetKind, Project};

/// A 16-bit stereo 48 kHz WAV of a tenth of a second of silence.
fn wav() -> Vec<u8> {
    let data = 4800u32 * 4;
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&48_000u32.to_le_bytes());
    out.extend_from_slice(&192_000u32.to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    out.resize(out.len() + data as usize, 0);
    out
}

/// The start of a PNG file: its signature and an IHDR chunk.
fn png() -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    out.extend_from_slice(&13u32.to_be_bytes());
    out.extend_from_slice(b"IHDR");
    out.extend_from_slice(&[0, 0, 0, 2, 0, 0, 0, 1, 8, 2, 0, 0, 0]);
    out.extend_from_slice(&[0, 0, 0, 0]);
    out
}

/// The start of an ISO BMFF file with `brand` as its major brand.
fn ftyp(brand: &[u8; 4]) -> Vec<u8> {
    let mut out = 24u32.to_be_bytes().to_vec();
    out.extend_from_slice(b"ftyp");
    out.extend_from_slice(brand);
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(brand);
    out.extend_from_slice(b"mp41");
    out
}

/// The start of a Matroska file: its EBML header with the doctype `matroska`.
fn ebml() -> Vec<u8> {
    let mut out = vec![0x1A, 0x45, 0xDF, 0xA3, 0x93];
    out.extend_from_slice(&[0x42, 0x82, 0x88]);
    out.extend_from_slice(b"matroska");
    out.extend_from_slice(&[0x42, 0x87, 0x81, 0x04, 0x42, 0x85, 0x81, 0x02]);
    out
}

fn kind(head: &[u8], name: &str) -> AssetKind {
    let path = Path::new(name);
    AssetKind::from_contents(head, path, AssetKind::guess_from_path(path))
}

/// The decision from the first bytes and the name, file by file.
#[test]
fn the_first_bytes_decide_where_they_name_a_format_disccraft_reads() {
    // Wrong names and none.
    assert_eq!(kind(&wav(), "sound.bin"), AssetKind::Audio);
    assert_eq!(kind(&png(), "picture.dat"), AssetKind::Image);
    assert_eq!(kind(&ftyp(b"isom"), "clip.dat"), AssetKind::Video);
    assert_eq!(kind(&ftyp(b"isom"), "clip"), AssetKind::Video);
    assert_eq!(kind(&ebml(), "clip.bin"), AssetKind::Video);
    assert_eq!(kind(&wav(), "sound.mp4"), AssetKind::Audio);
    assert_eq!(kind(&png(), "picture.jpg"), AssetKind::Image);
    assert_eq!(kind(&ftyp(b"M4A "), "sound.dat"), AssetKind::Audio);
    // A container that may hold sound alone stays sound under a sound file's name.
    assert_eq!(kind(&ebml(), "sound.mka"), AssetKind::Audio);
    assert_eq!(kind(&ftyp(b"isom"), "sound.m4a"), AssetKind::Audio);
    // Subtitles and DiscCraft's Craft documents go by their names, whatever their bytes look like.
    let mut transport = vec![b' '; 600];
    for at in [0, 188, 376] {
        transport[at] = 0x47;
    }
    assert_eq!(kind(&transport, "script.txt"), AssetKind::Subtitle);
    assert_eq!(kind(&png(), "subtitle.srt"), AssetKind::Subtitle);
    assert_eq!(kind(&png(), "composition.ecproj"), AssetKind::Video);
    assert_eq!(kind(&wav(), "session.scraft"), AssetKind::Audio);
    assert_eq!(kind(b"PK\x03\x04", "picture.pcraft"), AssetKind::Image);
    // No signature DiscCraft reads: the name decides (raw AC-3, a GIF, a ZIP, nothing at all).
    assert_eq!(
        kind(&[0x0B, 0x77, 0x12, 0x34], "sound.ac3"),
        AssetKind::Audio
    );
    assert_eq!(
        kind(b"GIF89a\x01\x00\x01\x00", "picture.png"),
        AssetKind::Image
    );
    assert_eq!(kind(b"PK\x03\x04", "clip.mp4"), AssetKind::Video);
    assert_eq!(
        kind(b"GIF89a\x01\x00\x01\x00", "notes.dat"),
        AssetKind::Other
    );
    assert_eq!(kind(&[], "clip.mov"), AssetKind::Video);
    assert_eq!(kind(&[], "notes.docx"), AssetKind::Other);
    // Hostile bytes: some kind, never a panic.
    let mut state = 11u64;
    for length in [0usize, 1, 3, 7, 12, 64, 4096] {
        let bytes: Vec<u8> = (0..length)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 32) as u8
            })
            .collect();
        let _ = kind(&bytes, "noise.bin");
        let _ = kind(&bytes, "noise.mp4");
    }
}

/// Files on disk under wrong names import as what they are; a file that is not there, and a folder, go by their
/// names.
#[test]
fn files_under_wrong_names_import_as_what_they_are() {
    let dir = std::env::temp_dir().join(format!("disccraft-sniff-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut project = Project::default();
    let import = |project: &mut Project, name: &str, bytes: &[u8]| -> AssetKind {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(AssetKind::of_file(&path), kind(bytes, name), "{name}");
        let id = project.add_asset(&path).unwrap();
        let asset = project.assets.get(&id).unwrap();
        assert!(!asset.offline, "{name}");
        asset.kind
    };
    assert_eq!(import(&mut project, "sound.bin", &wav()), AssetKind::Audio);
    assert_eq!(
        import(&mut project, "picture.jpg", &png()),
        AssetKind::Image
    );
    assert_eq!(import(&mut project, "short.mp4", b"x"), AssetKind::Video);
    assert_eq!(import(&mut project, "empty.wav", b""), AssetKind::Audio);

    // Not there: by name, and offline.
    let gone = dir.join("gone.mp4");
    assert_eq!(AssetKind::of_file(&gone), AssetKind::Video);
    let id = project.add_asset(&gone).unwrap();
    let asset = project.assets.get(&id).unwrap();
    assert_eq!(asset.kind, AssetKind::Video);
    assert!(asset.offline);
    // A folder is no file.
    let folder = dir.join("folder.mp4");
    std::fs::create_dir_all(&folder).unwrap();
    assert_eq!(AssetKind::of_file(&folder), AssetKind::Video);

    // Real movies from ffmpeg, renamed.
    if let Some(ffmpeg) = ffmpeg() {
        for (made, renamed) in [("clip.mp4", "clip.dat"), ("clip.mkv", "clip.bin")] {
            let path = dir.join(made);
            let status = Command::new(&ffmpeg)
                .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
                .arg("testsrc2=size=64x48:rate=25")
                .args(["-t", "0.2", "-pix_fmt", "yuv420p"])
                .arg(&path)
                .status()
                .unwrap();
            assert!(status.success(), "{made}");
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(
                import(&mut project, renamed, &bytes),
                AssetKind::Video,
                "{renamed}"
            );
        }
    } else {
        eprintln!("ffmpeg not found; the ffmpeg-made movies are skipped");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
