//! The chapters a video file carries: a Matroska file's chapter atoms (its default, or first, edition) and an
//! MP4 / QuickTime file's chapter track or `chpl` list. A timeline made of the file takes them as its chapter
//! points, as Encore takes the chapter markers embedded in a file.

use std::path::Path;

use craft_media::ReadSeekSource;
use craft_media::isobmff;
use craft_media::matroska::{Demuxer, OpenOptions};

/// The most chapters taken from one file.
const MAX_CHAPTERS: usize = 999;

/// The chapters of `path` as (seconds from the start, title), in time order; empty when the file has none or is
/// not a Matroska or MP4 / QuickTime file. Only the file's headers are read.
#[must_use]
pub fn file_chapters(path: &Path) -> Vec<(f64, String)> {
    let mut head = [0u8; 8];
    let read = std::fs::File::open(path)
        .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut head))
        .is_ok();
    if !read {
        return Vec::new();
    }
    let mut chapters = if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        matroska(path).unwrap_or_default()
    } else if matches!(
        head.get(4..),
        Some(b"ftyp" | b"moov" | b"mdat" | b"wide" | b"free" | b"skip")
    ) {
        mp4(path).unwrap_or_default()
    } else {
        Vec::new()
    };
    chapters.retain(|(seconds, _)| seconds.is_finite() && *seconds >= 0.0);
    chapters.sort_by(|a, b| a.0.total_cmp(&b.0));
    chapters.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-3);
    chapters.truncate(MAX_CHAPTERS);
    chapters
}

fn matroska(path: &Path) -> Option<Vec<(f64, String)>> {
    let source = ReadSeekSource::new(std::fs::File::open(path).ok()?).ok()?;
    let options = OpenOptions {
        index: false,
        verify_crc: false,
        index_limit: None,
    };
    let demuxer = Demuxer::with_options(source, options).ok()?;
    let editions = &demuxer.file().chapters;
    // The edition a player shows: the default one, else the first that is not hidden, else the first.
    let edition = editions
        .iter()
        .find(|edition| edition.default && !edition.hidden)
        .or_else(|| editions.iter().find(|edition| !edition.hidden))
        .or_else(|| editions.first())?;
    Some(
        edition
            .chapters
            .iter()
            .filter(|chapter| chapter.enabled && !chapter.hidden)
            .map(|chapter| {
                (
                    chapter.start_ns as f64 / 1e9,
                    chapter.title().unwrap_or_default().trim().to_owned(),
                )
            })
            .collect(),
    )
}

fn mp4(path: &Path) -> Option<Vec<(f64, String)>> {
    let source = ReadSeekSource::new(std::fs::File::open(path).ok()?).ok()?;
    let file = isobmff::open(&source).ok()?;
    if file.timescale == 0 {
        return None;
    }
    let chapters = file.read_chapters(&source).ok()?;
    Some(
        chapters
            .into_iter()
            .map(|chapter| {
                (
                    chapter.start as f64 / f64::from(file.timescale),
                    chapter.title.trim().to_owned(),
                )
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::file_chapters;
    use std::path::Path;

    #[test]
    fn a_file_without_chapters_has_none() {
        let dir = std::env::temp_dir().join(format!("disccraft-chapters-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let junk = dir.join("junk.mkv");
        std::fs::write(&junk, b"\x1A\x45\xDF\xA3 not really matroska").unwrap();
        let text = dir.join("notes.txt");
        std::fs::write(&text, b"chapter 1 at 00:10").unwrap();
        let (a, b) = (file_chapters(&junk), file_chapters(&text));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(a.is_empty() && b.is_empty());
        assert!(file_chapters(Path::new("no such file.mp4")).is_empty());
    }
}
