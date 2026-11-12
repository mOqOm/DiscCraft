//! Subtitle files: SRT, WebVTT and timecoded text scripts (one cue per line: start, end, text), in UTF-8, UTF-16
//! (with a byte-order mark) or Shift_JIS (common for Japanese SRT files).
//!
//! The formats follow their public descriptions: SRT (numbered blocks, `HH:MM:SS,mmm --> HH:MM:SS,mmm`), WebVTT
//! (W3C WebVTT: a `WEBVTT` header, optional cue identifiers, `[HH:]MM:SS.mmm --> ...`, `NOTE` / `STYLE` /
//! `REGION` blocks), and a plain script with two timecodes and the text on each line.

use std::path::Path;

use disccraft_project::{DiscSettings, Tick, parse_time};

use crate::MenuError;
use crate::subtitle::{
    Cue, MAX_CUES, MAX_SRT_BYTES, parse_srt, parse_srt_time_ticks, process_block,
};

/// The subtitle file formats DiscCraft reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubtitleFormat {
    /// SubRip (`.srt`).
    Srt,
    /// WebVTT (`.vtt`).
    WebVtt,
    /// One cue per line: start and end timecodes, then the text (`.txt`).
    TextScript,
}

impl SubtitleFormat {
    /// The format of a file, from its contents first (a `WEBVTT` header, `-->` timing lines) and its extension
    /// second.
    pub fn detect(path: &Path, text: &str) -> Self {
        let body = text.trim_start_matches('\u{feff}').trim_start();
        if body.starts_with("WEBVTT") {
            return Self::WebVtt;
        }
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("vtt") => Self::WebVtt,
            Some("srt") => Self::Srt,
            _ if text.contains("-->") => Self::Srt,
            _ => Self::TextScript,
        }
    }
}

/// Reads a subtitle file into cues sorted by start time. Timecodes in a text script use the project's frame rate.
pub fn read_subtitles(path: &Path, settings: &DiscSettings) -> Result<Vec<Cue>, MenuError> {
    let display = path.display();
    let metadata =
        std::fs::metadata(path).map_err(|e| MenuError::Invalid(format!("{display}: {e}")))?;
    let limit = u64::try_from(MAX_SRT_BYTES).unwrap_or(u64::MAX);
    if metadata.len() > limit {
        return Err(MenuError::Invalid(format!(
            "{display}: subtitle file is too large"
        )));
    }
    let bytes = std::fs::read(path).map_err(|e| MenuError::Invalid(format!("{display}: {e}")))?;
    let text = decode_text(&bytes);
    let cues = match SubtitleFormat::detect(path, &text) {
        SubtitleFormat::Srt => parse_srt(&text),
        SubtitleFormat::WebVtt => parse_vtt(&text),
        SubtitleFormat::TextScript => parse_text_script(&text, settings),
    };
    cues.map_err(|e| MenuError::Invalid(format!("{display}: {e}")))
}

/// Decodes subtitle bytes: UTF-8 or UTF-16 with a byte-order mark, UTF-8, otherwise Shift_JIS.
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some((encoding, _)) = encoding_rs::Encoding::for_bom(bytes) {
        let (text, _) = encoding.decode_with_bom_removal(bytes);
        return text.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => {
            let (text, _) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(bytes);
            text.into_owned()
        }
    }
}

/// Parses WebVTT: the header, `NOTE`, `STYLE` and `REGION` blocks are skipped; cue settings after the end time are
/// ignored; hours may be left out.
pub fn parse_vtt(text: &str) -> Result<Vec<Cue>, MenuError> {
    if text.len() > MAX_SRT_BYTES {
        return Err(MenuError::Invalid("vtt input is too large".to_string()));
    }
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut cues = Vec::new();
    let mut block: Vec<&str> = Vec::new();
    let mut block_no = 1usize;
    let flush = |block: &mut Vec<&str>, cues: &mut Vec<Cue>, block_no: &mut usize| {
        let skip = block.first().is_none_or(|first| {
            ["WEBVTT", "NOTE", "STYLE", "REGION"]
                .iter()
                .any(|keyword| first.starts_with(keyword))
        });
        let result = if skip {
            Ok(())
        } else {
            process_block(block, *block_no, cues)
        };
        *block_no = block_no.saturating_add(1);
        block.clear();
        result
    };
    for raw_line in body.split('\n') {
        let line = raw_line.trim_end_matches('\r');
        if line.trim().is_empty() {
            if !block.is_empty() {
                flush(&mut block, &mut cues, &mut block_no)?;
            }
            continue;
        }
        block.push(line);
    }
    if !block.is_empty() {
        flush(&mut block, &mut cues, &mut block_no)?;
    }
    cues.sort_by_key(|cue| cue.start);
    Ok(cues)
}

/// Parses a text script: each cue line holds a start and an end timecode (`HH:MM:SS:FF`, `HH;MM;SS;FF` at the
/// project's frame rate, or `HH:MM:SS.mmm`) and then the text. A line that does not start with a timecode continues
/// the text of the cue above it on a new line. Empty lines and lines starting with `#` are skipped.
pub fn parse_text_script(text: &str, settings: &DiscSettings) -> Result<Vec<Cue>, MenuError> {
    if text.len() > MAX_SRT_BYTES {
        return Err(MenuError::Invalid("script input is too large".to_string()));
    }
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut cues: Vec<Cue> = Vec::new();
    for (index, raw_line) in body.split('\n').enumerate() {
        let line = raw_line.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (first, rest) = next_token(line);
        let (second, rest) = next_token(rest);
        let times = (script_time(first, settings), script_time(second, settings));
        match times {
            (Some(start), Some(end)) => {
                let text = rest.trim().to_string();
                if end > start && !text.is_empty() {
                    cues.push(Cue { start, end, text });
                }
            }
            _ => match cues.last_mut() {
                Some(cue) => {
                    cue.text.push('\n');
                    cue.text.push_str(line);
                }
                None => {
                    return Err(MenuError::Invalid(format!(
                        "line {}: expected a start and an end timecode",
                        index.saturating_add(1)
                    )));
                }
            },
        }
        if cues.len() > MAX_CUES {
            return Err(MenuError::Invalid("too many cues".to_string()));
        }
    }
    cues.sort_by_key(|cue| cue.start);
    Ok(cues)
}

/// The first whitespace-separated token of `text` and what follows it.
fn next_token(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    text.split_at(end)
}

/// One timecode of a text script: frames at the project's rate, or milliseconds.
fn script_time(token: &str, settings: &DiscSettings) -> Option<Tick> {
    if token.contains(['.', ',']) {
        return parse_srt_time_ticks(token);
    }
    // Only tokens that look like timecodes; text that happens to start with digits is not one.
    if token.chars().filter(|c| *c == ':' || *c == ';').count() != 3 {
        return None;
    }
    parse_time(token, settings).ok()
}

/// Writes cues as SRT (subtitle export).
pub fn write_srt(cues: &[Cue]) -> String {
    let mut out = String::new();
    for (index, cue) in cues.iter().enumerate() {
        out.push_str(&format!(
            "{}\r\n{} --> {}\r\n{}\r\n\r\n",
            index.saturating_add(1),
            srt_time(cue.start),
            srt_time(cue.end),
            cue.text.replace('\n', "\r\n")
        ));
    }
    out
}

fn srt_time(tick: Tick) -> String {
    let ms = u64::try_from(tick.0.max(0)).unwrap_or(0) / crate::subtitle::TICKS_PER_MS;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::{
        SubtitleFormat, decode_text, parse_text_script, parse_vtt, read_subtitles, write_srt,
    };
    use crate::subtitle::parse_srt;
    use disccraft_project::{DiscSettings, Tick, VideoStandard};
    use std::error::Error;
    use std::path::Path;

    const MS: i64 = 254_016_000;

    #[test]
    fn webvtt_skips_header_notes_and_settings() -> Result<(), Box<dyn Error>> {
        let text = "WEBVTT - demo\n\nNOTE written by hand\n\nSTYLE\n::cue { color: red }\n\nintro\n00:01.000 --> 00:03.500 align:start\nこんにちは\n\n00:00:04.000 --> 00:00:05.000\n<i>second</i>\n";
        let cues = parse_vtt(text)?;
        assert_eq!(cues.len(), 2);
        assert_eq!(
            cues.first().map(|cue| (cue.start, cue.end)),
            Some((Tick(1000 * MS), Tick(3500 * MS)))
        );
        assert_eq!(
            cues.first().map(|cue| cue.text.as_str()),
            Some("こんにちは")
        );
        assert_eq!(cues.get(1).map(|cue| cue.text.as_str()), Some("second"));
        Ok(())
    }

    #[test]
    fn text_script_uses_frames_and_continues_lines() -> Result<(), Box<dyn Error>> {
        let settings = DiscSettings {
            standard: VideoStandard::Pal,
            ..DiscSettings::default()
        };
        let text = "# comment\n00:00:01:00 00:00:02:12 First line\nsecond line\n00:00:03:00\t00:00:04:00\tNext\n";
        let cues = parse_text_script(text, &settings)?;
        assert_eq!(cues.len(), 2);
        let first = cues.first().ok_or("cue")?;
        assert_eq!(first.text, "First line\nsecond line");
        assert_eq!(first.start, Tick(1000 * MS));
        // 2 seconds and 12 frames at 25 fps.
        assert_eq!(first.end, Tick(2480 * MS));
        assert!(parse_text_script("not a cue\n", &settings).is_err());
        Ok(())
    }

    #[test]
    fn shift_jis_and_utf16_are_decoded() {
        // 「字幕」 in Shift_JIS.
        assert_eq!(decode_text(&[0x8E, 0x9A, 0x96, 0x8B]), "字幕");
        // UTF-16LE with a byte-order mark.
        assert_eq!(decode_text(&[0xFF, 0xFE, 0x57, 0x5B, 0x55, 0x5E]), "字幕");
        assert_eq!(decode_text("字幕".as_bytes()), "字幕");
    }

    #[test]
    fn formats_are_detected_by_content_then_extension() {
        assert_eq!(
            SubtitleFormat::detect(Path::new("a.txt"), "WEBVTT\n"),
            SubtitleFormat::WebVtt
        );
        assert_eq!(
            SubtitleFormat::detect(Path::new("a.srt"), "1\n"),
            SubtitleFormat::Srt
        );
        assert_eq!(
            SubtitleFormat::detect(Path::new("a.txt"), "1\n00:00:01,000 --> 00:00:02,000\nx\n"),
            SubtitleFormat::Srt
        );
        assert_eq!(
            SubtitleFormat::detect(Path::new("a.txt"), "00:00:01:00 00:00:02:00 x"),
            SubtitleFormat::TextScript
        );
    }

    #[test]
    fn a_shift_jis_srt_file_reads_and_round_trips() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-subs-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("ja.srt");
        let mut bytes = b"1\r\n00:00:01,000 --> 00:00:02,500\r\n".to_vec();
        bytes.extend_from_slice(&[0x8E, 0x9A, 0x96, 0x8B]);
        bytes.extend_from_slice(b"\r\n\r\n");
        std::fs::write(&path, &bytes)?;
        let cues = read_subtitles(&path, &DiscSettings::default())?;
        assert_eq!(cues.first().map(|cue| cue.text.as_str()), Some("字幕"));
        // Exported SRT reads back the same.
        assert_eq!(parse_srt(&write_srt(&cues))?, cues);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
