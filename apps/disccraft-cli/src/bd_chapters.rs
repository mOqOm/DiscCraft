//! `disccraft-cli bd-chapters`: the chapters of a Blu-ray's playlists (M9.41).
//!
//! Reads `BDMV/PLAYLIST/*.mpls` of a disc in a drive, a disc's folder or an image's files copied out (nothing
//! encrypted is read: the playlists are not), lists the playlists with their lengths, and prints the chapters
//! (the entry marks) of the main one (the longest), of one asked for, or of all.

use std::path::{Path, PathBuf};

use disccraft_bdmv::outline::{Outline, read_outline};
use serde_json::json;

/// 45 kHz.
const TICKS: u64 = 45_000;
/// The most playlist files read from one folder (a disc has at most 2000).
const MAX_PLAYLISTS: usize = 2_000;
/// The largest playlist file read.
const MAX_SIZE: u64 = 4 * 1024 * 1024;
/// The most clips named in the list of playlists.
const SHOWN_CLIPS: usize = 4;

/// What to print.
pub(crate) struct Options {
    /// The playlist whose chapters are printed (`1`, `00001` or `00001.mpls`); `None`: the longest.
    pub(crate) playlist: Option<String>,
    /// The chapters of every playlist that has any.
    pub(crate) all: bool,
    /// JSON instead of text.
    pub(crate) json: bool,
    /// The chapters as an OGM chapter file (`CHAPTER01=00:00:00.000`, `CHAPTER01NAME=Chapter 01`).
    pub(crate) ogm: bool,
}

/// One playlist file read, or why not.
struct Read {
    name: String,
    outline: Result<Outline, String>,
}

/// `ticks` (45 kHz) as `H:MM:SS.mmm`, or with two-digit hours for an OGM file.
fn timecode(ticks: u64, wide_hours: bool) -> String {
    let millis = ticks.saturating_mul(1_000).saturating_add(TICKS / 2) / TICKS;
    let (hours, rest) = (millis / 3_600_000, millis % 3_600_000);
    let (minutes, rest) = (rest / 60_000, rest % 60_000);
    let (seconds, millis) = (rest / 1_000, rest % 1_000);
    if wide_hours {
        format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
    } else {
        format!("{hours}:{minutes:02}:{seconds:02}.{millis:03}")
    }
}

/// `ticks` (45 kHz) as seconds.
fn seconds(ticks: u64) -> f64 {
    ticks as f64 / TICKS as f64
}

/// The folder of the playlists, or the one playlist file, that `path` names: a `.mpls` file, a `PLAYLIST`
/// folder, a `BDMV` folder, a disc's root (a drive like `D:`).
fn playlist_files(path: &Path) -> Result<(PathBuf, Vec<PathBuf>), String> {
    let mut path = path.to_path_buf();
    // `D:` alone is the current folder on drive D; the disc is its root.
    let text = path.to_string_lossy();
    if text.len() == 2 && text.ends_with(':') {
        path = PathBuf::from(format!("{text}\\"));
    }
    if path.is_file() {
        let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
        return Ok((folder, vec![path]));
    }
    let folder = [
        path.join("BDMV").join("PLAYLIST"),
        path.join("PLAYLIST"),
        path.clone(),
    ]
    .into_iter()
    .find(|folder| {
        std::fs::read_dir(folder)
            .is_ok_and(|entries| entries.flatten().any(|entry| is_playlist(&entry.path())))
    })
    .ok_or_else(|| {
        format!(
            "no Blu-ray playlists (BDMV\\PLAYLIST\\*.mpls) in {}",
            path.display()
        )
    })?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(&folder)
        .map_err(|error| format!("{}: {error}", folder.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_playlist(path))
        .take(MAX_PLAYLISTS)
        .collect();
    files.sort();
    Ok((folder, files))
}

fn is_playlist(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("mpls"))
}

fn read(path: &Path) -> Read {
    let name = path
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    let outline = std::fs::metadata(path)
        .map_err(|error| error.to_string())
        .and_then(|metadata| {
            if metadata.len() > MAX_SIZE {
                Err(format!("larger than {MAX_SIZE} bytes"))
            } else {
                std::fs::read(path).map_err(|error| error.to_string())
            }
        })
        .and_then(|data| read_outline(&data).map_err(|error| error.to_string()));
    Read { name, outline }
}

/// The playlists whose chapters are printed.
fn chosen<'a>(read: &'a [Read], options: &Options) -> Result<Vec<&'a Read>, String> {
    let readable = || read.iter().filter(|playlist| playlist.outline.is_ok());
    if let Some(wanted) = &options.playlist {
        let wanted = wanted.trim_end_matches(".mpls").trim_end_matches(".MPLS");
        let number = wanted.parse::<u32>().ok();
        let found = read.iter().find(|playlist| {
            playlist.name.eq_ignore_ascii_case(wanted)
                || number.is_some() && playlist.name.parse::<u32>().ok() == number
        });
        return found
            .map(|playlist| vec![playlist])
            .ok_or_else(|| format!("no playlist {wanted}"));
    }
    if options.all {
        return Ok(readable()
            .filter(|playlist| {
                playlist
                    .outline
                    .as_ref()
                    .is_ok_and(|outline| !outline.chapters().is_empty())
            })
            .collect());
    }
    // The main one: the longest (the first of equally long ones).
    let longest = readable().fold(None::<&Read>, |best, playlist| {
        let length = |p: &Read| p.outline.as_ref().map_or(0, Outline::length);
        match best {
            Some(best) if length(best) >= length(playlist) => Some(best),
            _ => Some(playlist),
        }
    });
    Ok(longest.into_iter().collect())
}

/// The clips of a playlist, as `00001+00002`, shortened after a few.
fn clips(outline: &Outline) -> String {
    let mut names: Vec<&str> = outline
        .items
        .iter()
        .take(SHOWN_CLIPS)
        .map(|item| item.clip.as_str())
        .collect();
    if outline.items.len() > SHOWN_CLIPS {
        names.push("…");
    }
    names.join("+")
}

/// Prints the chapters of the Blu-ray at `path`.
pub(crate) fn run(path: &Path, options: &Options) -> Result<(), String> {
    let (folder, files) = playlist_files(path)?;
    let read: Vec<Read> = files.iter().map(|file| read(file)).collect();
    let chosen = chosen(&read, options)?;

    if options.json {
        let playlists: Vec<serde_json::Value> = read
            .iter()
            .map(|playlist| match &playlist.outline {
                Ok(outline) => json!({
                    "name": playlist.name,
                    "seconds": seconds(outline.length()),
                    "clips": outline.items.iter().map(|item| item.clip.clone()).collect::<Vec<_>>(),
                    "chapters": outline.chapters().iter().map(|chapter| json!({
                        "start": seconds(chapter.start),
                        "length": seconds(chapter.length),
                    })).collect::<Vec<_>>(),
                }),
                Err(error) => json!({"name": playlist.name, "error": error}),
            })
            .collect();
        let value = json!({
            "folder": folder.to_string_lossy(),
            "playlists": playlists,
            "chosen": chosen.iter().map(|playlist| playlist.name.clone()).collect::<Vec<_>>(),
        });
        return crate::print_json_pretty(&value);
    }

    if options.ogm {
        let [playlist] = chosen.as_slice() else {
            return Err(
                "--ogm prints one playlist's chapters: name it with --playlist".to_string(),
            );
        };
        let outline = playlist
            .outline
            .as_ref()
            .map_err(|error| format!("{}.mpls: {error}", playlist.name))?;
        for (index, chapter) in outline.chapters().iter().enumerate() {
            let number = index + 1;
            crate::say(&format!(
                "CHAPTER{number:02}={}\nCHAPTER{number:02}NAME=Chapter {number:02}",
                timecode(chapter.start, true)
            ))?;
        }
        return Ok(());
    }

    crate::say(&format!("{}", folder.display()))?;
    crate::say("  Playlist  Length        Chapters  Clips")?;
    for playlist in &read {
        let main = if chosen.iter().any(|chosen| std::ptr::eq(*chosen, playlist)) {
            "*"
        } else {
            " "
        };
        match &playlist.outline {
            Ok(outline) => crate::say(&format!(
                "{main} {:<8}  {:>12}  {:>8}  {}",
                playlist.name,
                timecode(outline.length(), false),
                outline.chapters().len(),
                clips(outline)
            ))?,
            Err(error) => crate::say(&format!(
                "{main} {:<8}  cannot be read: {error}",
                playlist.name
            ))?,
        }
    }
    for playlist in chosen {
        let Ok(outline) = &playlist.outline else {
            continue;
        };
        let chapters = outline.chapters();
        crate::say("")?;
        crate::say(&format!(
            "{}.mpls: {}, {} chapters",
            playlist.name,
            timecode(outline.length(), false),
            chapters.len()
        ))?;
        crate::say("   #  Start         Length")?;
        for (index, chapter) in chapters.iter().enumerate() {
            crate::say(&format!(
                "  {:>2}  {:>12}  {:>12}",
                index + 1,
                timecode(chapter.start, false),
                timecode(chapter.length, false)
            ))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::timecode;

    #[test]
    fn times_are_rounded_to_milliseconds() {
        assert_eq!(timecode(0, false), "0:00:00.000");
        assert_eq!(timecode(45_000 * 3_725 + 22, false), "1:02:05.000");
        assert_eq!(timecode(45_000 * 3_725 + 23, false), "1:02:05.001");
        assert_eq!(timecode(45_000 * 59, true), "00:00:59.000");
        assert!(!timecode(u64::MAX, false).is_empty());
    }
}
