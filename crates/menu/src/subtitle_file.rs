//! Subtitle files: SRT, WebVTT and timecoded text scripts (one cue per line: start, end, text), in UTF-8, UTF-16
//! (with a byte-order mark) or Shift_JIS (common for Japanese SRT files).
//!
//! The formats follow their public descriptions: SRT (numbered blocks, `HH:MM:SS,mmm --> HH:MM:SS,mmm`), WebVTT
//! (W3C WebVTT: a `WEBVTT` header, optional cue identifiers, `[HH:]MM:SS.mmm --> ...`, `NOTE` / `STYLE` /
//! `REGION` blocks), and a plain script with two timecodes and the text on each line.
//!
//! The other caption files (M6.566: ASS / SSA, Scenarist SCC, MacCaption MCC, EBU STL, TTML / DFXP and SAMI) are read
//! with craft-libs' `craft-captions`, through its public API; their text is decoded the way an SRT's is first (a
//! Japanese ASS file is often Shift_JIS), except EBU STL, a binary file with its own character table.
//!
//! Picture subtitles (Encore's image script and FAB image script) follow the Adobe Encore DVD 2.0 User Guide
//! (2005), "Structure of image scripts", "Structure of FAB image scripts" and "Importing image-based subtitle
//! scripts" (the colour mapping); no other reader of these scripts was looked at (AGENTS.md rule 3).

use std::path::Path;

use disccraft_project::{DiscSettings, Tick, format_time, parse_time};

use crate::MenuError;
use crate::subtitle::{
    Cue, MAX_CUES, MAX_SRT_BYTES, parse_srt, parse_srt_time_ticks, process_block, strip_tags_text,
};

/// The caption files read with craft-captions, by extension (M6.566).
pub const CAPTION_EXTENSIONS: [(&str, craft_captions::Format); 9] = [
    ("ass", craft_captions::Format::Ass),
    ("ssa", craft_captions::Format::Ass),
    ("scc", craft_captions::Format::Scc),
    ("mcc", craft_captions::Format::Mcc),
    ("stl", craft_captions::Format::Stl),
    ("ttml", craft_captions::Format::Ttml),
    ("dfxp", craft_captions::Format::Dfxp),
    ("smi", craft_captions::Format::Sami),
    ("sami", craft_captions::Format::Sami),
];

/// DiscCraft's ticks in a second (FilmCraft's time base).
const TICKS_PER_SECOND: i64 = 254_016_000_000;

/// The subtitle file formats DiscCraft reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubtitleFormat {
    /// SubRip (`.srt`).
    Srt,
    /// WebVTT (`.vtt`).
    WebVtt,
    /// One cue per line: start and end timecodes, then the text (`.txt`).
    TextScript,
    /// A caption file read with craft-captions ([`CAPTION_EXTENSIONS`]).
    Captions(craft_captions::Format),
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
        let captions = CAPTION_EXTENSIONS
            .iter()
            .find(|(known, _)| extension.as_deref() == Some(*known))
            .map(|(_, format)| *format);
        match extension.as_deref() {
            Some("vtt") => Self::WebVtt,
            Some("srt") => Self::Srt,
            _ if captions.is_some() => captions.map_or(Self::TextScript, Self::Captions),
            _ if text.contains("-->") => Self::Srt,
            _ => Self::TextScript,
        }
    }
}

/// A subtitle asset's cues: the ones edited in the project (Encore's subtitles added and trimmed in the Monitor) when
/// it has them, else its file's ([`read_subtitles`]).
///
/// # Errors
///
/// When the file is read and cannot be ([`read_subtitles`]).
pub fn asset_cues(
    asset: &disccraft_project::Asset,
    settings: &DiscSettings,
) -> Result<Vec<Cue>, MenuError> {
    match &asset.cues {
        Some(cues) => Ok(cues
            .iter()
            .map(|cue| Cue {
                start: cue.start,
                end: cue.end,
                text: cue.text.clone(),
                look: cue.look,
                image: cue.image.clone(),
            })
            .collect()),
        None => read_subtitles(Path::new(&asset.path), settings),
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
        // EBU STL is binary, with its own character table; the others are text, decoded as above.
        SubtitleFormat::Captions(craft_captions::Format::Stl) => {
            caption_cues(&bytes, craft_captions::Format::Stl)
        }
        SubtitleFormat::Captions(format) => caption_cues(text.as_bytes(), format),
    };
    cues.map_err(|e| MenuError::Invalid(format!("{display}: {e}")))
}

/// The cues of a caption file in `format` read with craft-captions, sorted by start time: their text with the
/// formatting tags taken out as an SRT's is, and cues with no text or no length left out.
fn caption_cues(bytes: &[u8], format: craft_captions::Format) -> Result<Vec<Cue>, MenuError> {
    let document = craft_captions::parse(bytes, format)
        .map_err(|error| MenuError::Invalid(error.to_string()))?;
    let mut cues = Vec::new();
    for cue in document.cues {
        let (start, end) = (ticks(cue.start), ticks(cue.end));
        let text = strip_tags_text(&cue.text);
        if end <= start || text.is_empty() {
            continue;
        }
        cues.push(Cue {
            start,
            end,
            text,
            look: disccraft_project::CueLook::default(),
            image: None,
        });
        if cues.len() > MAX_CUES {
            return Err(MenuError::Invalid("too many subtitle cues".to_string()));
        }
    }
    cues.sort_by_key(|cue| cue.start);
    Ok(cues)
}

/// A craft-captions time as DiscCraft's (the two count the same ticks a second today; the product keeps a time
/// exact if they ever differ).
fn ticks(time: craft_captions::Tick) -> Tick {
    let value = i128::from(time.0) * i128::from(TICKS_PER_SECOND)
        / i128::from(craft_captions::TICKS_PER_SECOND).max(1);
    Tick(i64::try_from(value).unwrap_or(if value < 0 { i64::MIN } else { i64::MAX }))
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
///
/// Encore's own export is read as it is written: a cue number before the two timecodes is skipped, and a lone
/// carriage return inside a cue is a line break (its lines end with CR LF).
pub fn parse_text_script(text: &str, settings: &DiscSettings) -> Result<Vec<Cue>, MenuError> {
    if text.len() > MAX_SRT_BYTES {
        return Err(MenuError::Invalid("script input is too large".to_string()));
    }
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut cues: Vec<Cue> = Vec::new();
    for (index, raw_line) in body.split(['\n', '\r']).enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (first, rest) = next_token(without_cue_number(line, settings));
        let (second, rest) = next_token(rest);
        let times = (script_time(first, settings), script_time(second, settings));
        match times {
            (Some(start), Some(end)) => {
                let text = rest.trim().to_string();
                if end > start && !text.is_empty() {
                    cues.push(Cue {
                        start,
                        end,
                        text,
                        look: disccraft_project::CueLook::default(),
                        image: None,
                    });
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

/// `line` without the cue number Encore writes before the two timecodes; other lines are returned as they are, so
/// that text which merely starts with a number stays text.
fn without_cue_number<'a>(line: &'a str, settings: &DiscSettings) -> &'a str {
    let (number, rest) = next_token(line);
    let (start, after) = next_token(rest);
    let (end, _) = next_token(after);
    let numbered = !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
        && script_time(start, settings).is_some()
        && script_time(end, settings).is_some();
    if numbered { rest } else { line }
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

/// One subtitle of an image script or a FAB image script: when it is shown, its picture, and where it stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageScriptCue {
    /// First tick at which it is shown.
    pub start: Tick,
    /// First tick at which it is no longer shown.
    pub end: Tick,
    /// The picture file.
    pub picture: std::path::PathBuf,
    /// Left, top, right and bottom in the frame's pixels.
    pub area: [u16; 4],
}

/// Where a picture stands when an image script has no `Display_Area`: from the frame's top left corner, as far as
/// the frame goes.
const WHOLE_FRAME: [u16; 4] = [0, 0, u16::MAX, u16::MAX];

/// Reads an image script or a FAB image script file (`fab`), as Encore's Timeline ▸ Import Subtitles ▸ Image
/// Script / FAB Image Script. A FAB script's pictures are in `folder` (the script's own folder when `None`).
///
/// # Errors
///
/// When the file cannot be read, is too large, or does not follow the format.
pub fn read_image_script(
    path: &Path,
    fab: bool,
    folder: Option<&Path>,
    settings: &DiscSettings,
) -> Result<Vec<ImageScriptCue>, MenuError> {
    let display = path.display();
    let metadata =
        std::fs::metadata(path).map_err(|e| MenuError::Invalid(format!("{display}: {e}")))?;
    if metadata.len() > u64::try_from(MAX_SRT_BYTES).unwrap_or(u64::MAX) {
        return Err(MenuError::Invalid(format!(
            "{display}: script is too large"
        )));
    }
    let bytes = std::fs::read(path).map_err(|e| MenuError::Invalid(format!("{display}: {e}")))?;
    let text = decode_text(&bytes);
    let base = path.parent().unwrap_or_else(|| Path::new(""));
    let cues = if fab {
        parse_fab_script(&text, settings, folder.unwrap_or(base))
    } else {
        parse_image_script(&text, settings, base)
    };
    cues.map_err(|e| MenuError::Invalid(format!("{display}: {e}")))
}

/// Parses an image script (Adobe Encore DVD 2.0 User Guide, "Structure of image scripts"): a `Display_Area (left
/// top right bottom)` line (where every picture stands, in the frame's pixels), a `Directory <folder>` line (where
/// the pictures are), then one subtitle a line: `number start end file`, separated by spaces or tabs. Timecodes
/// are read as in a text script (no leading zeros needed).
///
/// A folder that is not absolute is taken from `base` (the script's folder), and so are the pictures when there
/// is no Directory line. Other header lines before the first subtitle (such as `Contrast (...)`) and lines that
/// start with `#` are skipped.
///
/// # Errors
///
/// When the input is too large, the display area is not four numbers that make a box, a line after the first
/// subtitle is not a subtitle, or there are too many.
pub fn parse_image_script(
    text: &str,
    settings: &DiscSettings,
    base: &Path,
) -> Result<Vec<ImageScriptCue>, MenuError> {
    if text.len() > MAX_SRT_BYTES {
        return Err(MenuError::Invalid("script input is too large".to_string()));
    }
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut area = WHOLE_FRAME;
    let mut directory = base.to_path_buf();
    let mut cues = Vec::new();
    for (index, raw_line) in body.split(['\n', '\r']).enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let number = index.saturating_add(1);
        let (keyword, rest) = next_token(line);
        if keyword.eq_ignore_ascii_case("Display_Area") {
            area = parse_area(rest).ok_or_else(|| {
                MenuError::Invalid(format!(
                    "line {number}: Display_Area must be four numbers (left top right bottom) that make a box"
                ))
            })?;
            continue;
        }
        if keyword.eq_ignore_ascii_case("Directory") {
            directory = base.join(unquoted(rest.trim()));
            continue;
        }
        let (start, after_start) = next_token(rest);
        let (end, file) = next_token(after_start);
        let numbered = !keyword.is_empty() && keyword.chars().all(|c| c.is_ascii_digit());
        match (
            numbered,
            script_time(start, settings),
            script_time(end, settings),
        ) {
            (true, Some(start), Some(end)) if !file.trim().is_empty() => {
                if end > start {
                    cues.push(ImageScriptCue {
                        start,
                        end,
                        picture: directory.join(unquoted(file.trim())),
                        area,
                    });
                }
            }
            // A header line this reader does not use.
            _ if cues.is_empty() => {}
            _ => {
                return Err(MenuError::Invalid(format!(
                    "line {number}: expected a number, a start and an end timecode, and a picture file"
                )));
            }
        }
        if cues.len() > MAX_CUES {
            return Err(MenuError::Invalid("too many cues".to_string()));
        }
    }
    cues.sort_by_key(|cue| cue.start);
    Ok(cues)
}

/// Parses a FAB image script (Adobe Encore DVD 2.0 User Guide, "Structure of FAB image scripts"): one subtitle a
/// line, `file start end left top right bottom`, separated by spaces or tabs, the place in the frame's pixels.
/// The pictures are in `folder`. Lines that start with `#` and header lines with a parenthesis before the first
/// subtitle are skipped.
///
/// # Errors
///
/// When the input is too large, a line is not a subtitle, its place is not a box, or there are too many.
pub fn parse_fab_script(
    text: &str,
    settings: &DiscSettings,
    folder: &Path,
) -> Result<Vec<ImageScriptCue>, MenuError> {
    if text.len() > MAX_SRT_BYTES {
        return Err(MenuError::Invalid("script input is too large".to_string()));
    }
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut cues = Vec::new();
    for (index, raw_line) in body.split(['\n', '\r']).enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        // The file name may hold spaces: the last six fields are the times and the place.
        let parsed = tokens
            .len()
            .checked_sub(6)
            .filter(|at| *at > 0)
            .and_then(|at| {
                let fields = tokens.get(at..)?;
                let start = script_time(fields.first()?, settings)?;
                let end = script_time(fields.get(1)?, settings)?;
                let area = parse_area(&fields.get(2..)?.join(" "))?;
                Some((tokens.get(..at)?.join(" "), start, end, area))
            });
        match parsed {
            Some((file, start, end, area)) => {
                if end > start {
                    cues.push(ImageScriptCue {
                        start,
                        end,
                        picture: folder.join(unquoted(&file)),
                        area,
                    });
                }
            }
            None if cues.is_empty() && line.contains('(') => {}
            None => {
                return Err(MenuError::Invalid(format!(
                    "line {}: expected a picture file, a start and an end timecode, and left top right bottom",
                    index.saturating_add(1)
                )));
            }
        }
        if cues.len() > MAX_CUES {
            return Err(MenuError::Invalid("too many cues".to_string()));
        }
    }
    cues.sort_by_key(|cue| cue.start);
    Ok(cues)
}

/// Four numbers (left top right bottom), in parentheses or not, that make a box of some size.
fn parse_area(text: &str) -> Option<[u16; 4]> {
    let numbers: Vec<u16> = text
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',')
        .filter(|token| !token.is_empty())
        .map(|token| token.parse().ok())
        .collect::<Option<_>>()?;
    match numbers.as_slice() {
        [left, top, right, bottom] if left < right && top < bottom => {
            Some([*left, *top, *right, *bottom])
        }
        _ => None,
    }
}

/// A path without the quotes around it.
fn unquoted(text: &str) -> &str {
    text.strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(text)
}

/// Writes cues as SRT (subtitle export). Picture subtitles have no text and are left out.
pub fn write_srt(cues: &[Cue]) -> String {
    let mut out = String::new();
    for (index, cue) in cues.iter().filter(|cue| cue.image.is_none()).enumerate() {
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

/// Writes cues as the text script Encore CS6 exports (Timeline ▸ Export Subtitles ▸ Text Script), so that the file
/// goes back into Encore: UTF-16 LE with a byte-order mark, one cue a line as `number start end text` with
/// timecodes at the project's frame rate, CR LF between cues and a lone CR for a line break inside one. Picture
/// subtitles have no text and are left out.
pub fn write_text_script(cues: &[Cue], settings: &DiscSettings) -> Vec<u8> {
    let mut text = String::new();
    for (index, cue) in cues.iter().filter(|cue| cue.image.is_none()).enumerate() {
        text.push_str(&format!(
            "{} {} {} {}\r\n",
            index.saturating_add(1),
            format_time(cue.start, settings),
            format_time(cue.end, settings),
            cue.text.replace("\r\n", "\r").replace('\n', "\r")
        ));
    }
    let mut out = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
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
        SubtitleFormat, decode_text, parse_fab_script, parse_image_script, parse_text_script,
        parse_vtt, read_subtitles, write_srt, write_text_script,
    };
    use crate::subtitle::parse_srt;
    use disccraft_project::{DiscSettings, Tick, VideoStandard, parse_time};
    use std::error::Error;
    use std::path::Path;

    const MS: i64 = 254_016_000;

    /// The image script of the Encore DVD 2.0 User Guide's example: the display area and the folder from the
    /// header, one picture a line; timecodes without leading zeros; a relative folder from the script's.
    #[test]
    fn image_script_reads_the_guides_example() -> Result<(), Box<dyn Error>> {
        let settings = DiscSettings::default();
        let text = "Display_Area (000 000 720 480)\r\n\
                    Directory C:\\Subtitles\\Image Files\\Caption Inc Images\\\r\n\
                    1 00;00;02;02 00;00;03;15 CATTALESS001.TIF\r\n\
                    2 0;0;5;18\t00;00;09;20\tCATTALESS002.TIF\r\n";
        let cues = parse_image_script(text, &settings, Path::new("scripts"))?;
        assert_eq!(cues.len(), 2);
        let first = cues.first().ok_or("cue")?;
        assert_eq!(first.area, [0, 0, 720, 480]);
        assert_eq!(first.start, parse_time("00;00;02;02", &settings)?);
        assert_eq!(first.end, parse_time("00;00;03;15", &settings)?);
        if cfg!(windows) {
            assert_eq!(
                first.picture,
                Path::new("C:\\Subtitles\\Image Files\\Caption Inc Images\\CATTALESS001.TIF")
            );
        }
        let second = cues.get(1).ok_or("cue")?;
        assert_eq!(second.start, parse_time("00;00;05;18", &settings)?);
        // No Directory line: the pictures are beside the script; other header lines are skipped.
        let cues = parse_image_script(
            "Contrast (15 15 15 0)\n1 00;00;01;00 00;00;02;00 \"sub 1.png\"\n",
            &settings,
            Path::new("scripts"),
        )?;
        assert_eq!(
            cues.first().map(|cue| (cue.picture.clone(), cue.area)),
            Some((Path::new("scripts").join("sub 1.png"), super::WHOLE_FRAME))
        );
        // A display area that is not a box, and a broken line after the first subtitle, are refused.
        assert!(
            parse_image_script("Display_Area (10 0 10 480)\n", &settings, Path::new("")).is_err()
        );
        let broken = "1 00;00;01;00 00;00;02;00 a.png\nbroken\n";
        assert!(parse_image_script(broken, &settings, Path::new("")).is_err());
        Ok(())
    }

    /// The FAB image script of the Encore DVD 2.0 User Guide's example: each picture with its own place, in the
    /// folder asked for; a file name with spaces; PAL timecodes with colons.
    #[test]
    fn fab_script_reads_the_guides_example() -> Result<(), Box<dyn Error>> {
        let settings = DiscSettings::default();
        let text = "RACE001.TIF 00;00;02;02 00;00;03;15 000 000 720 480\n\
                    RACE002.TIF\t00;00;05;18\t00;00;09;20\t10 400 710 470\n";
        let cues = parse_fab_script(text, &settings, Path::new("pics"))?;
        assert_eq!(cues.len(), 2);
        assert_eq!(
            cues.iter().map(|cue| cue.area).collect::<Vec<_>>(),
            vec![[0, 0, 720, 480], [10, 400, 710, 470]]
        );
        assert_eq!(
            cues.get(1).map(|cue| cue.picture.clone()),
            Some(Path::new("pics").join("RACE002.TIF"))
        );
        assert_eq!(
            cues.get(1).map(|cue| cue.end),
            Some(parse_time("00;00;09;20", &settings)?)
        );
        let pal = DiscSettings {
            standard: VideoStandard::Pal,
            ..DiscSettings::default()
        };
        let line = "my sub 1.png 00:00:01:00 00:00:02:12 0 0 720 576\n";
        let cues = parse_fab_script(line, &pal, Path::new(""))?;
        assert_eq!(
            cues.first().map(|cue| (cue.picture.clone(), cue.end)),
            Some((Path::new("my sub 1.png").to_path_buf(), Tick(2480 * MS)))
        );
        // Too few fields, or a place that is not a box.
        for bad in [
            "a.png 00;00;01;00 00;00;02;00 0 0 720\n",
            "a.png 00;00;01;00 00;00;02;00 0 0 0 480\n",
        ] {
            assert!(
                parse_fab_script(bad, &settings, Path::new("")).is_err(),
                "{bad}"
            );
        }
        Ok(())
    }

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

    /// The export is what Encore CS6 writes, byte for byte (measured 2026-11-15), and reads back the same.
    #[test]
    fn text_script_is_written_as_encore_writes_it() -> Result<(), Box<dyn Error>> {
        let settings = DiscSettings::default();
        let cues = parse_text_script(
            "00;00;02;00 00;00;04;00 A\nCCCAA\n00;00;06;00 00;00;08;00 BBB\n",
            &settings,
        )?;
        let bytes = write_text_script(&cues, &settings);
        let expected =
            "\u{feff}1 00;00;02;00 00;00;04;00 A\rCCCAA\r\n2 00;00;06;00 00;00;08;00 BBB\r\n";
        let units: Vec<u8> = expected.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(bytes, units);
        let back = parse_text_script(&decode_text(&bytes), &settings)?;
        assert_eq!(back.len(), 2);
        assert_eq!(back.first().ok_or("cue")?.text, "A\nCCCAA");
        Ok(())
    }

    #[test]
    fn text_script_reads_what_encore_exports() -> Result<(), Box<dyn Error>> {
        // A number, two drop-frame timecodes and the text; CR LF between cues and a lone CR inside one.
        let settings = DiscSettings::default();
        let text =
            "\u{feff}1 00;00;02;00 00;00;04;00 A\rCCCAA\r\n2 00;00;06;00 00;00;08;00 BBB\r\n";
        let cues = parse_text_script(text, &settings)?;
        assert_eq!(cues.len(), 2);
        let first = cues.first().ok_or("cue")?;
        assert_eq!(first.text, "A\nCCCAA");
        assert_eq!(first.start, parse_time("00;00;02;00", &settings)?);
        assert_eq!(first.end, parse_time("00;00;04;00", &settings)?);
        let second = cues.get(1).ok_or("cue")?;
        assert_eq!(second.text, "BBB");
        // A line of text that starts with a number is still text.
        let cues = parse_text_script("00;00;01;00 00;00;02;00 One\n2 more lines\n", &settings)?;
        assert_eq!(cues.first().ok_or("cue")?.text, "One\n2 more lines");
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
