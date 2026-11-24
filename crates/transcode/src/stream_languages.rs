//! The languages a video file names for its sound streams (M6.421): a Matroska file's audio tracks (`LanguageBCP47`,
//! else `Language`), an MP4 / QuickTime file's sound tracks (`mdhd`), a transport stream's audio streams (the PMT's
//! ISO 639 language descriptor, M6.423: a `.m2ts` / `.mts` / `.ts`). A timeline made of the file gives each audio
//! track the language of its stream, so the disc names it (a DVD's audio stream attributes, a Blu-ray's stream
//! table) as the file did.
//!
//! Matroska's `Language` is `eng` when a file does not say, and an MP4's `mdhd` is often left at its default too
//! (`und`, or `eng` from a QuickTime writer), so a file whose sound streams all name English, or none, is taken to
//! say nothing: only languages that tell the streams apart, or name something else, are kept.

use std::path::Path;

use craft_media::ReadSeekSource;
use craft_media::isobmff;
use craft_media::matroska::{Demuxer, OpenOptions, TrackKind};

/// The language codes `path` gives its sound streams, in the order of its sound streams (as `audio_stream_count`
/// counts them): ISO 639-2 or BCP 47 as the file has them (`jpn`, `en-US`). `None` for a stream the file names no
/// language for; empty when it names none, is not a Matroska or MP4 / QuickTime file, or lists its sound tracks
/// otherwise than the media reader does. Only the file's headers are read.
#[must_use]
pub fn audio_stream_languages(path: &Path) -> Vec<Option<String>> {
    let mut head = [0u8; 8];
    let read = std::fs::File::open(path)
        .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut head))
        .is_ok();
    if !read {
        return Vec::new();
    }
    let named = if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        matroska(path).unwrap_or_default()
    } else if matches!(
        head.get(4..),
        Some(b"ftyp" | b"moov" | b"mdat" | b"wide" | b"free" | b"skip")
    ) {
        mp4(path).unwrap_or_default()
    } else if crate::ts_probe::looks_like_ts(path) {
        transport_stream(path).unwrap_or_default()
    } else {
        Vec::new()
    };
    let languages = telling(named);
    // The streams must be the ones the media reader lists, one for one.
    if languages.is_empty() || languages.len() != crate::audio_stream_count(path) {
        return Vec::new();
    }
    languages
}

/// `named` with the languages that say nothing taken out: `und` and empty ones always, and English when every
/// stream is English (a file's default).
fn telling(named: Vec<String>) -> Vec<Option<String>> {
    let undetermined = |code: &str| code.is_empty() || code.eq_ignore_ascii_case("und");
    let english = |code: &str| {
        code.eq_ignore_ascii_case("eng")
            || code.eq_ignore_ascii_case("en")
            || code.to_ascii_lowercase().starts_with("en-")
    };
    let all_english = named
        .iter()
        .filter(|code| !undetermined(code))
        .all(|code| english(code));
    let languages: Vec<Option<String>> = named
        .into_iter()
        .map(|code| (!undetermined(&code) && !(all_english && english(&code))).then_some(code))
        .collect();
    if languages.iter().all(Option::is_none) {
        return Vec::new();
    }
    languages
}

fn matroska(path: &Path) -> Option<Vec<String>> {
    let source = ReadSeekSource::new(std::fs::File::open(path).ok()?).ok()?;
    let options = OpenOptions {
        index: false,
        verify_crc: false,
        index_limit: None,
    };
    let demuxer = Demuxer::with_options(source, options).ok()?;
    Some(
        demuxer
            .tracks()
            .iter()
            .filter(|track| track.kind == TrackKind::Audio)
            .map(|track| {
                track
                    .language_bcp47
                    .clone()
                    .filter(|tag| !tag.trim().is_empty())
                    .unwrap_or_else(|| track.language.clone())
            })
            .collect(),
    )
}

/// A transport stream's audio streams' languages, in the order the stream's index lists them (its programme's),
/// from the index DiscCraft keeps of the file (`crate::ts_index`).
fn transport_stream(path: &Path) -> Option<Vec<String>> {
    let (_, file) = crate::ts_index::open(path)?;
    Some(
        file.streams
            .iter()
            .filter(|stream| stream.kind() == craft_media::mpegts::Kind::Audio)
            .map(|stream| stream.language.clone().unwrap_or_default())
            .collect(),
    )
}

fn mp4(path: &Path) -> Option<Vec<String>> {
    let source = ReadSeekSource::new(std::fs::File::open(path).ok()?).ok()?;
    let file = isobmff::open(&source).ok()?;
    Some(
        file.tracks
            .iter()
            .filter(|track| track.kind == isobmff::TrackKind::Audio)
            .map(|track| track.language.clone())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::telling;

    fn named(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|code| (*code).to_owned()).collect()
    }

    /// M6.421: languages that tell streams apart are kept; a file's defaults are not.
    #[test]
    fn only_languages_a_file_meant_are_kept() {
        assert_eq!(
            telling(named(&["jpn", "eng"])),
            [Some("jpn".to_owned()), Some("eng".to_owned())]
        );
        assert_eq!(
            telling(named(&["jpn", "und"])),
            [Some("jpn".to_owned()), None]
        );
        assert!(
            telling(named(&["eng", "eng"])).is_empty(),
            "Matroska's default"
        );
        assert!(telling(named(&["eng"])).is_empty());
        assert!(telling(named(&["und", ""])).is_empty());
        assert_eq!(telling(named(&["fra"])), [Some("fra".to_owned())]);
        assert_eq!(
            telling(named(&["en-US", "ja-JP"])),
            [Some("en-US".to_owned()), Some("ja-JP".to_owned())]
        );
    }
}
