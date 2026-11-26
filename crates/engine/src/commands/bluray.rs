//! Import from a Blu-ray (M6.429): a timeline made of a playlist of a Blu-ray disc, folder or mounted image, to
//! author it again (a DVD of a Blu-ray, a Blu-ray with new menus).
//!
//! The playlist's play items become the timeline's video clips, in order, each its clip file
//! (`BDMV/STREAM/*.m2ts`) trimmed to the play item's IN and OUT times (on the clip's time line, which starts at the
//! presentation start of its clip information file). Each primary audio stream DiscCraft decodes (LPCM, AC-3)
//! becomes an audio track of the clips' sound in that stream, in the language the playlist gives it; the others
//! (DTS, Dolby TrueHD) are listed as left out. In a Blu-ray project each presentation graphics subtitle stream
//! becomes a subtitle track (M6.491: a `.sup` taken out of the clips, at once or by the caller later). What the
//! playlist and the clip information say of each clip (its
//! length, picture and sound) is remembered as its probe, so the new timeline shows at once instead of after every
//! clip file has been read through. The entry marks become the chapter points, each
//! placed by its play item and its time in it, so the frames of the clips do not add up rounding. Encrypted discs
//! are not read: their clip files cannot be decoded, and nothing here works around that. The playlists are read by
//! [`disccraft_bdmv::outline`] (the public field tables, docs/formats/bdmv.md).

use std::path::{Path, PathBuf};

use crate::params::{opt_bool, opt_str, opt_u64, req_str};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_bdmv::outline::{
    Outline, OutlineItem, OutlineMark, OutlineStream, clip_presentation, read_outline, video_format,
};
use disccraft_project::{
    AssetId, AudioClip, AudioTrack, ClipTrim, FrameRate, MAX_TIMELINE_CLIPS, Tick, TimelineId,
};
use serde_json::Value;

use super::{always_enabled, object};

/// The most playlist files read from one disc (a disc has at most 2000).
const MAX_PLAYLISTS: usize = 2_000;
/// The largest playlist or clip information file read.
const MAX_SIZE: u64 = 4 * 1024 * 1024;
/// The largest disc metadata file read for the disc's title.
const MAX_META: u64 = 256 * 1024;
/// The audio tracks a timeline takes (a DVD's eight).
const MAX_AUDIO_TRACKS: usize = 8;
/// Ticks in one 45 kHz unit (254 016 000 000 / 45 000).
const TICKS_PER_45K: i64 = 5_644_800;

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "bluray.playlists",
        label: "Blu-ray Playlists",
        menu: &["File", "Import"],
        shortcut: None,
        params: r#"{"path": string (a drive, a disc folder, BDMV, .mpls)} → {"folder", "title", "main", "playlists": [{"playlist", "seconds", "chapters", "clips", "audio", "subtitles", "missing"?, "error"?}]}"#,
        enabled: always_enabled,
        run: run_playlists,
        journal: false,
    },
    CommandSpec {
        id: "timeline.importBluray",
        label: "Import from Blu-ray",
        menu: &["File", "Import"],
        shortcut: None,
        params: r#"{"path": string, "playlist"?: number (default: the longest), "name"?: string, "subtitlesLater"?: bool (true: the subtitles are not taken out but returned as subtitleJobs)} → {"timeline", "playlist", "clips", "chapters", "audioTracks", "audioLeftOut": [string], "subtitleTracks", "subtitlesLeftOut": [string], "subtitleJobs": [job], "assets": [id], "existing": bool (a timeline of the same clips was there already)}"#,
        enabled: always_enabled,
        run: run_import,
        journal: true,
    },
    CommandSpec {
        id: "timeline.addBluraySubtitle",
        label: "Blu-ray Subtitles to Subtitle Tracks",
        menu: &["Timeline"],
        shortcut: None,
        params: r#"{"job": job (one of subtitleJobs, after it is taken out)} → {"asset", "track"}"#,
        enabled: always_enabled,
        run: run_add_subtitle,
        journal: true,
    },
];

/// A disc's `BDMV` folder and its playlists, read or why not.
pub(crate) struct Disc {
    pub(crate) bdmv: PathBuf,
    pub(crate) playlists: Vec<(u32, Result<Outline, String>)>,
}

/// The disc at `path`: a drive (`F:`), a folder holding `BDMV`, the `BDMV` or `PLAYLIST` folder, or one `.mpls`
/// file (then only that playlist).
pub(crate) fn open_disc(path: &str) -> Result<Disc, String> {
    let trimmed = path.trim();
    let mut root = PathBuf::from(trimmed);
    // `F:` alone is the current folder on drive F; the disc is its root.
    if trimmed.len() == 2 && trimmed.ends_with(':') {
        root = PathBuf::from(format!("{trimmed}\\"));
    }
    if root
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("iso"))
    {
        return Err(format!(
            "{}: a disc image is read once it is mounted (on Windows, open it in Explorer): choose its drive",
            root.display()
        ));
    }
    let only = root.is_file().then(|| root.clone());
    let bdmv = match &only {
        // BDMV/PLAYLIST/x.mpls
        Some(file) => file
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .ok_or_else(|| format!("{}: not in a BDMV folder", file.display()))?,
        None => [root.join("BDMV"), root.clone(), root.join("..")]
            .into_iter()
            .find(|folder| folder.join("PLAYLIST").is_dir() && folder.join("STREAM").is_dir())
            .ok_or_else(|| {
                format!(
                    "no Blu-ray (BDMV with PLAYLIST and STREAM) in {}",
                    root.display()
                )
            })?,
    };
    let files: Vec<PathBuf> = match only {
        Some(file) => vec![file],
        None => {
            let mut files: Vec<PathBuf> = std::fs::read_dir(bdmv.join("PLAYLIST"))
                .map_err(|error| format!("{}: {error}", bdmv.display()))?
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.is_file()
                        && path
                            .extension()
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("mpls"))
                })
                .take(MAX_PLAYLISTS)
                .collect();
            files.sort();
            files
        }
    };
    let playlists = files
        .iter()
        .filter_map(|file| {
            let number = file
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.parse::<u32>().ok())?;
            Some((
                number,
                read_small(file)
                    .and_then(|data| read_outline(&data).map_err(|error| error.to_string())),
            ))
        })
        .collect();
    Ok(Disc { bdmv, playlists })
}

/// A small file whole (a playlist, a clip information file).
fn read_small(path: &Path) -> Result<Vec<u8>, String> {
    let size = std::fs::metadata(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .len();
    if size > MAX_SIZE {
        return Err(format!("{}: larger than {MAX_SIZE} bytes", path.display()));
    }
    std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// The clip file of `clip` (`BDMV/STREAM/<clip>.m2ts`).
pub(crate) fn clip_file(bdmv: &Path, clip: &str) -> PathBuf {
    bdmv.join("STREAM").join(format!("{clip}.m2ts"))
}

/// The disc's title from its metadata (`BDMV/META/DL/bdmt_*.xml`, the `di:name` of `di:title`), when it has one.
pub(crate) fn disc_title(bdmv: &Path) -> Option<String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(bdmv.join("META").join("DL"))
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    let name = name.to_ascii_lowercase();
                    name.starts_with("bdmt_") && name.ends_with(".xml")
                })
        })
        .collect();
    files.sort();
    files.into_iter().find_map(|file| {
        if std::fs::metadata(&file).ok()?.len() > MAX_META {
            return None;
        }
        let text = String::from_utf8_lossy(&std::fs::read(&file).ok()?).into_owned();
        let title = text.find("<di:title>")?;
        let rest = text.get(title..)?;
        let start = rest.find("<di:name>")? + "<di:name>".len();
        let end = rest.find("</di:name>")?;
        let name = rest.get(start..end)?;
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        (!name.is_empty()).then_some(name)
    })
}

/// The playlist a disc plays as its main title: the longest (the first of equally long ones).
pub(crate) fn main_playlist(disc: &Disc) -> Option<u32> {
    disc.playlists
        .iter()
        .filter_map(|(number, outline)| Some((*number, outline.as_ref().ok()?.length())))
        .fold(None::<(u32, u64)>, |best, (number, length)| match best {
            Some((_, longest)) if longest >= length => best,
            _ => Some((number, length)),
        })
        .map(|(number, _)| number)
}

/// Whether DiscCraft decodes an audio stream of this coding type (LPCM, AC-3).
fn readable(coding: u8) -> bool {
    matches!(coding, 0x80 | 0x81)
}

/// The name of an audio coding type.
fn audio_format(coding: u8) -> &'static str {
    match coding {
        0x80 => "LPCM",
        0x81 => "AC-3",
        0x82 => "DTS",
        0x83 => "Dolby TrueHD",
        0x84 | 0xA1 => "E-AC-3",
        0x85 => "DTS-HD HR",
        0x86 => "DTS-HD MA",
        0xA2 => "DTS-HD LBR",
        _ => "audio",
    }
}

/// A play item's audio streams for the list: language, format, and whether it is read.
fn audio_list(streams: &[OutlineStream]) -> Vec<Value> {
    streams
        .iter()
        .map(|stream| {
            serde_json::json!({
                "language": stream.language_code(),
                "format": audio_format(stream.coding),
                "readable": readable(stream.coding),
            })
        })
        .collect()
}

/// Tells the probe what the playlist and the clip information say of clip file `file` (its presentation from
/// `start` to `end`, 45 kHz): its length, picture size and rate, and whether it has sound DiscCraft reads. Only for
/// an HD picture (always 16:9); an SD clip's shape is left to the probe.
fn tell_probe(file: &Path, item: &OutlineItem, (start, end): (u32, u32)) {
    let Some(((width, height), fps)) = item.video.and_then(|(_, format)| video_format(format))
    else {
        return;
    };
    if height < 720 || end <= start {
        return;
    }
    let seconds = f64::from(end - start) / 45_000.0;
    let has_audio = item.audio.iter().any(|stream| readable(stream.coding));
    disccraft_transcode::tell_video(
        file,
        &disccraft_transcode::VideoProbe {
            seconds,
            width,
            height,
            fps,
            wide: true,
            has_audio,
        },
    );
    if has_audio {
        disccraft_transcode::tell_audio_seconds(file, seconds);
    }
}

/// `bluray.playlists`: the playlists of the disc at `path`, with their lengths, chapters, clips and languages.
fn run_playlists(_session: &mut Session, params: &Value) -> CommandResult {
    let path = req_str(params, "path")?;
    let disc = open_disc(&path)?;
    let playlists: Vec<Value> = disc
        .playlists
        .iter()
        .map(|(number, outline)| match outline {
            Ok(outline) => {
                let clips: Vec<String> = outline.items.iter().map(|item| item.clip.clone()).collect();
                let missing = clips
                    .iter()
                    .any(|clip| !clip_file(&disc.bdmv, clip).is_file());
                let first = outline.items.first();
                serde_json::json!({
                    "playlist": number,
                    "seconds": outline.length() as f64 / 45_000.0,
                    "chapters": outline.chapters().len(),
                    "clips": clips,
                    "audio": first.map(|item| audio_list(&item.audio)).unwrap_or_default(),
                    "subtitles": first
                        .map(|item| item.subtitles.iter().map(OutlineStream::language_code).collect::<Vec<_>>())
                        .unwrap_or_default(),
                    "missing": missing,
                })
            }
            Err(error) => serde_json::json!({"playlist": number, "error": error}),
        })
        .collect();
    Ok(object(&[
        (
            "folder",
            Value::from(disc.bdmv.to_string_lossy().into_owned()),
        ),
        (
            "title",
            disc_title(&disc.bdmv).map_or(Value::Null, Value::from),
        ),
        (
            "main",
            main_playlist(&disc).map_or(Value::Null, Value::from),
        ),
        ("playlists", Value::from(playlists)),
    ]))
}

/// 45 kHz units as frames of the disc's standard rate, to the nearest frame.
fn frames_of(rate: FrameRate, units: u64) -> u32 {
    let ticks = i64::try_from(units)
        .unwrap_or(i64::MAX / TICKS_PER_45K)
        .saturating_mul(TICKS_PER_45K);
    u32::try_from(rate.frame_at(rate.snap_nearest(Tick(ticks))).max(0)).unwrap_or(u32::MAX)
}

/// A playlist laid out on a timeline: each play item's clip file and how it is trimmed, where each starts and where
/// the last ends (frames of the disc's rate).
pub(crate) struct Layout {
    pub(crate) clips: Vec<(PathBuf, ClipTrim)>,
    pub(crate) starts: Vec<u32>,
    pub(crate) end: u32,
}

/// The playlist `number` of `disc`, read.
pub(crate) fn outline_of(disc: &Disc, number: u32) -> Result<Outline, String> {
    let outline = disc
        .playlists
        .iter()
        .find(|(found, _)| *found == number)
        .ok_or_else(|| format!("no playlist {number:05}"))?
        .1
        .clone()
        .map_err(|error| format!("playlist {number:05}: {error}"))?;
    if outline.items.is_empty() {
        return Err(format!("playlist {number:05} plays nothing"));
    }
    if outline.items.len() > MAX_TIMELINE_CLIPS {
        return Err(format!(
            "playlist {number:05} has {} clips; a timeline takes {MAX_TIMELINE_CLIPS}",
            outline.items.len()
        ));
    }
    Ok(outline)
}

/// How `outline` lies on a timeline at `rate`: each play item's clip file trimmed from its IN time (on the clip's
/// own time line, from the presentation start of its clip information file; a clip without one starts at the IN
/// time) for its length. When `tell`, what the playlist and the clip information say of each clip is told to the
/// probe.
pub(crate) fn layout(
    rate: FrameRate,
    disc: &Disc,
    outline: &Outline,
    tell: bool,
) -> Result<Layout, String> {
    let mut clips = Vec::with_capacity(outline.items.len());
    for item in &outline.items {
        let file = clip_file(&disc.bdmv, &item.clip);
        if !file.is_file() {
            return Err(format!("the clip file {} is missing", file.display()));
        }
        let presentation = read_small(
            &disc
                .bdmv
                .join("CLIPINF")
                .join(format!("{}.clpi", item.clip)),
        )
        .ok()
        .and_then(|data| clip_presentation(&data).ok())
        .filter(|(start, _)| *start <= item.in_time);
        if tell && let Some(presentation) = presentation {
            tell_probe(&file, item, presentation);
        }
        let start = presentation.map_or(item.in_time, |(start, _)| start);
        let trim_start = frames_of(rate, u64::from(item.in_time - start));
        let length = frames_of(rate, u64::from(item.length())).max(1);
        clips.push((
            file,
            ClipTrim {
                start: trim_start,
                end: Some(trim_start.saturating_add(length)),
            },
        ));
    }
    let mut starts = Vec::with_capacity(clips.len());
    let mut end = 0u32;
    for (_, trim) in &clips {
        starts.push(end);
        end = end.saturating_add(trim.end.unwrap_or(trim.start).saturating_sub(trim.start));
    }
    Ok(Layout { clips, starts, end })
}

/// The frame of the timeline `layout` lays out where `mark` is: its play item's place plus its time into it; `None`
/// for a mark outside its play item or the playlist.
pub(crate) fn mark_frame(
    rate: FrameRate,
    outline: &Outline,
    layout: &Layout,
    mark: &OutlineMark,
) -> Option<u32> {
    let index = usize::from(mark.play_item);
    let item = outline.items.get(index)?;
    let start = layout.starts.get(index)?;
    if !(item.in_time..item.out_time).contains(&mark.time) {
        return None;
    }
    let frame = start.saturating_add(frames_of(rate, u64::from(mark.time - item.in_time)));
    (frame < layout.end).then_some(frame)
}

/// Two paths of the same file, however they are written.
fn same_file(a: &Path, b: &Path) -> bool {
    let text = |path: &Path| {
        std::path::absolute(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_lowercase()
    };
    text(a) == text(b)
}

/// The project's asset of the file `file`, when it has one.
fn asset_of(session: &Session, file: &Path) -> Option<AssetId> {
    session
        .project
        .assets
        .values()
        .find(|asset| same_file(Path::new(&asset.path), file))
        .map(|asset| asset.id)
}

/// A timeline of the project made of `layout` (the same files, trimmed the same way), when it has one.
pub(crate) fn existing_timeline(session: &Session, layout: &Layout) -> Option<TimelineId> {
    session.project.timelines.values().find_map(|timeline| {
        let videos = timeline.videos();
        let same = videos.len() == layout.clips.len()
            && videos.iter().enumerate().zip(&layout.clips).all(
                |((index, asset), (file, trim))| {
                    session
                        .project
                        .assets
                        .get(asset)
                        .is_some_and(|asset| same_file(Path::new(&asset.path), file))
                        && timeline.video_trim(index) == *trim
                },
            );
        same.then_some(timeline.id)
    })
}

/// What importing a playlist made.
pub(crate) struct Imported {
    pub(crate) timeline: TimelineId,
    pub(crate) clips: usize,
    pub(crate) chapters: usize,
    pub(crate) tracks: usize,
    pub(crate) left_out: Vec<String>,
    /// M6.491: the subtitle streams to take out (each a subtitle track then), and those left out.
    pub(crate) subtitle_jobs: Vec<SubtitleJob>,
    pub(crate) subtitles_left_out: Vec<String>,
    pub(crate) assets: Vec<u64>,
    /// The project had a timeline of the playlist already: it is that one.
    pub(crate) existing: bool,
}

/// A timeline of playlist `number` of `disc` (named `name`, else after the disc): its clips, chapters and sound.
/// A timeline of the project made of the same clips is taken instead of making another.
pub(crate) fn import_playlist(
    session: &mut Session,
    disc: &Disc,
    number: u32,
    name: Option<String>,
) -> Result<Imported, String> {
    let outline = outline_of(disc, number)?;
    let rate = session.project.settings.frame_rate();
    let layout = layout(rate, disc, &outline, true)?;
    let mut assets: Vec<u64> = Vec::new();
    if let Some(timeline) = existing_timeline(session, &layout) {
        let found = session.project.timeline(timeline);
        return Ok(Imported {
            timeline,
            clips: layout.clips.len(),
            chapters: found.map_or(0, |timeline| timeline.chapters.len()),
            tracks: found.map_or(0, |timeline| timeline.audio.len()),
            left_out: Vec::new(),
            subtitle_jobs: Vec::new(),
            subtitles_left_out: Vec::new(),
            assets,
            existing: true,
        });
    }

    // Each clip file once, as an asset (one the project has is used again); each play item a trimmed clip of it.
    let mut clips: Vec<(AssetId, ClipTrim)> = Vec::with_capacity(layout.clips.len());
    for (file, trim) in &layout.clips {
        let asset = match asset_of(session, file) {
            Some(asset) => asset,
            None => session
                .project
                .add_asset(file)
                .map_err(|error| error.to_string())?,
        };
        if !assets.contains(&asset.0) {
            assets.push(asset.0);
        }
        clips.push((asset, *trim));
    }

    let title = disc_title(&disc.bdmv);
    let name = name.unwrap_or_else(|| match &title {
        Some(title) if Some(number) == main_playlist(disc) => title.clone(),
        Some(title) => format!("{title} {number:05}"),
        None => format!("Blu-ray {number:05}"),
    });
    let Some(&(first, first_trim)) = clips.first() else {
        return Err(format!("playlist {number:05} plays nothing"));
    };
    let timeline = session
        .project
        .add_timeline(first, Some(name))
        .map_err(|error| error.to_string())?;

    // The sound: each primary audio stream DiscCraft reads a track of the clips' sound in it (the stream's number
    // among the ones it reads in the file, as the playlist lists them in the order of their PIDs).
    let first_item = outline.items.first();
    let listed: Vec<OutlineStream> = first_item
        .map(|item| item.audio.clone())
        .unwrap_or_default();
    let mut tracks: Vec<(usize, String)> = Vec::new();
    let mut left_out: Vec<String> = Vec::new();
    for stream in &listed {
        if readable(stream.coding) && tracks.len() < MAX_AUDIO_TRACKS {
            let language = disccraft_bdmv::iso639_1(&stream.language_code())
                .unwrap_or_default()
                .to_owned();
            tracks.push((tracks.len(), language));
        } else {
            left_out.push(format!(
                "{} {}",
                stream.language_code(),
                audio_format(stream.coding)
            ));
        }
    }
    let first_file = layout
        .clips
        .first()
        .map(|(file, _)| file.clone())
        .unwrap_or_default();
    if listed.is_empty() {
        // A playlist that lists no streams: as many as the first clip file has.
        let count = disccraft_transcode::audio_stream_count(&first_file);
        tracks = (0..count.min(MAX_AUDIO_TRACKS))
            .map(|stream| (stream, String::new()))
            .collect();
    }
    let wide = disccraft_transcode::probe_video(&first_file)
        .ok()
        .map(|probe| probe.wide);
    if let Some(found) = session.project.timeline_mut(timeline) {
        found.more_video = clips.iter().skip(1).map(|(asset, _)| *asset).collect();
        found.video_trims = clips.iter().map(|(_, trim)| *trim).collect();
        if wide.is_some() {
            found.widescreen = wide;
        }
        for (stream, language) in &tracks {
            found.audio.push(AudioTrack {
                asset: first,
                language: language.clone(),
                stream: u32::try_from(*stream).unwrap_or(0),
                start: 0,
                trim_start: first_trim.start,
                trim_end: first_trim.end,
                more: clips
                    .iter()
                    .zip(&layout.starts)
                    .skip(1)
                    .map(|((asset, trim), start)| AudioClip {
                        asset: *asset,
                        start: *start,
                        trim_start: trim.start,
                        trim_end: trim.end,
                    })
                    .collect(),
            });
        }
    }

    // The chapters: each entry mark at its play item's place plus its time into it.
    let mut chapters = 0usize;
    for mark in outline.marks.iter().filter(|mark| mark.kind == 1) {
        let Some(frame) = mark_frame(rate, &outline, &layout, mark) else {
            continue;
        };
        if frame == 0 {
            // The timeline's first chapter is there already.
            chapters += 1;
            continue;
        }
        if session
            .project
            .add_chapter(timeline, rate.tick_of(i64::from(frame)), None)
            .is_ok()
        {
            chapters += 1;
        }
    }
    session.project.adopt_first_play(timeline);
    let (subtitle_jobs, subtitles_left_out) =
        subtitle_jobs(session, timeline, disc, &outline, (&layout, rate));
    Ok(Imported {
        timeline,
        clips: clips.len(),
        chapters,
        tracks: tracks.len(),
        left_out,
        subtitle_jobs,
        subtitles_left_out,
        assets,
        existing: false,
    })
}

/// Ticks (254 016 000 000 a second) in a 90 kHz tick.
const TICKS_PER_90K: i64 = 2_822_400;

/// M6.491: a presentation graphics subtitle stream of an imported playlist, to be taken out of the disc's clips into
/// a `.sup` and laid as a subtitle track of the playlist's timeline. Taking it out reads the clips' transport stream
/// indexes, which the first time means reading the clips through (minutes from a disc drive), so the window takes it
/// out on a thread of its own ([`take_bluray_subtitle`]) and adds it then (`timeline.addBluraySubtitle`); the
/// command line takes it out during the import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubtitleJob {
    timeline: TimelineId,
    pid: u16,
    /// The track's language (ISO 639-1; empty when the disc's code has none).
    language: String,
    /// The stream as a list of those left out names it (`jpn PG`).
    name: String,
    /// The `.sup` it goes into.
    file: PathBuf,
    parts: Vec<disccraft_transcode::BdSubtitlePart>,
}

impl SubtitleJob {
    fn to_json(&self) -> Value {
        let parts: Vec<Value> = self
            .parts
            .iter()
            .map(|part| {
                object(&[
                    (
                        "file",
                        Value::from(part.file.to_string_lossy().into_owned()),
                    ),
                    ("from", Value::from(part.from)),
                    ("to", Value::from(part.to)),
                    ("at", Value::from(part.at)),
                ])
            })
            .collect();
        object(&[
            ("timeline", Value::from(self.timeline.0)),
            ("pid", Value::from(self.pid)),
            ("language", Value::from(self.language.clone())),
            ("name", Value::from(self.name.clone())),
            (
                "file",
                Value::from(self.file.to_string_lossy().into_owned()),
            ),
            ("parts", Value::from(parts)),
        ])
    }

    fn from_json(job: &Value) -> Result<Self, String> {
        let text = |value: &Value, key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("the subtitle job has no {key}"))
        };
        let number = |value: &Value, key: &str| {
            value
                .get(key)
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("the subtitle job has no {key}"))
        };
        let parts = job
            .get("parts")
            .and_then(Value::as_array)
            .ok_or_else(|| "the subtitle job has no parts".to_string())?;
        if parts.len() > MAX_TIMELINE_CLIPS {
            return Err("the subtitle job has too many parts".to_string());
        }
        let parts = parts
            .iter()
            .map(|part| {
                Ok(disccraft_transcode::BdSubtitlePart {
                    file: PathBuf::from(text(part, "file")?),
                    from: number(part, "from")?,
                    to: number(part, "to")?,
                    at: number(part, "at")?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let timeline = job
            .get("timeline")
            .and_then(Value::as_u64)
            .ok_or_else(|| "the subtitle job has no timeline".to_string())?;
        let pid = u16::try_from(number(job, "pid")?)
            .map_err(|_| "the subtitle job's stream is not a PID".to_string())?;
        Ok(Self {
            timeline: TimelineId(timeline),
            pid,
            language: text(job, "language")?,
            name: text(job, "name")?,
            file: PathBuf::from(text(job, "file")?),
            parts,
        })
    }

    fn take(&self, go_on: &dyn Fn() -> bool) -> Result<(), String> {
        // The folder beside the project, the first time.
        if let Some(folder) = self.file.parent() {
            std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
        }
        disccraft_transcode::extract_bd_subtitle(&self.parts, self.pid, &self.file, go_on)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

/// Takes the subtitle stream of `job` (one of the `subtitleJobs` that `timeline.importBluray` and `menu.importBluray`
/// give with `"subtitlesLater": true`) out of the disc into its `.sup`, for `timeline.addBluraySubtitle` to add then
/// (M6.491). Slow (the clips' indexes are made the first time), so the window runs it on a thread of its own; `go_on`
/// is asked as it goes, and stops it when it says no.
///
/// # Errors
///
/// When `job` is not such a job, a clip cannot be read, the stream holds nothing, or `go_on` stopped it.
pub fn take_bluray_subtitle(job: &Value, go_on: &dyn Fn() -> bool) -> Result<(), String> {
    SubtitleJob::from_json(job)?.take(go_on)
}

/// M6.491: the playlist's presentation graphics subtitle streams (those of the main clips, as its first play item lists
/// them) as jobs to take each out of the play items' clips into a `.sup` beside the project, its display sets moved to
/// where each play item falls on the timeline, for a subtitle track in the disc's language; a Blu-ray project's only
/// (a DVD takes no PGS). Gives the jobs and the streams left out (language and kind).
fn subtitle_jobs(
    session: &Session,
    timeline: TimelineId,
    disc: &Disc,
    outline: &Outline,
    (layout, rate): (&Layout, FrameRate),
) -> (Vec<SubtitleJob>, Vec<String>) {
    let streams: Vec<OutlineStream> = outline
        .items
        .first()
        .map(|item| item.subtitles.clone())
        .unwrap_or_default();
    let kind = |stream: &OutlineStream| if stream.coding == 0x92 { "text" } else { "PG" };
    let mut left_out = Vec::new();
    if session.project.settings.kind != disccraft_project::DiscKind::BluRay {
        left_out.extend(
            streams
                .iter()
                .map(|stream| format!("{} {}", stream.language_code(), kind(stream))),
        );
        return (Vec::new(), left_out);
    }
    let parts: Vec<disccraft_transcode::BdSubtitlePart> = outline
        .items
        .iter()
        .zip(&layout.starts)
        .map(|(item, start)| disccraft_transcode::BdSubtitlePart {
            file: clip_file(&disc.bdmv, &item.clip),
            from: i64::from(item.in_time) * 2,
            to: i64::from(item.out_time) * 2,
            at: rate.tick_of(i64::from(*start)).0 / TICKS_PER_90K,
        })
        .collect();
    let folder = super::timeline::subtitle_folder(session);
    let stem: String = session
        .project
        .timeline(timeline)
        .map(|found| found.name.clone())
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(48)
        .collect();
    let mut room = session.project.timeline(timeline).map_or(0, |found| {
        super::timeline::MAX_SUBTITLE_TRACKS.saturating_sub(found.subtitles.len())
    });
    let mut jobs: Vec<SubtitleJob> = Vec::new();
    for stream in &streams {
        let language = stream.language_code();
        let name = format!("{language} {}", kind(stream));
        let (0x90, Some(pid)) = (stream.coding, stream.pid) else {
            left_out.push(name);
            continue;
        };
        if room == 0 {
            left_out.push(name);
            continue;
        }
        room -= 1;
        let safe: String = language
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        let base = format!("{stem}.{pid:04x}.{safe}");
        // A name no other file and no other job has (a project's earlier import, another unsaved project's).
        let file = (1..100u32)
            .map(|n| {
                folder.join(if n == 1 {
                    format!("{base}.sup")
                } else {
                    format!("{base}-{n}.sup")
                })
            })
            .find(|file| !file.exists() && jobs.iter().all(|job| job.file != *file))
            .unwrap_or_else(|| folder.join(format!("{base}.sup")));
        jobs.push(SubtitleJob {
            timeline,
            pid,
            language: disccraft_bdmv::iso639_1(&language)
                .unwrap_or_default()
                .to_owned(),
            name,
            file,
            parts: parts.clone(),
        });
    }
    (jobs, left_out)
}

/// Lays the `.sup` that `job` took out as a subtitle track of its timeline: the asset made of it and the track.
fn add_taken(session: &mut Session, job: &SubtitleJob) -> Result<(AssetId, usize), String> {
    let missing = || format!("timeline {} does not exist", job.timeline.0);
    let found = session.project.timeline(job.timeline).ok_or_else(missing)?;
    if found.subtitles.len() >= super::timeline::MAX_SUBTITLE_TRACKS {
        return Err("the timeline has as many subtitle tracks as a disc takes".to_string());
    }
    let asset = session
        .project
        .add_asset(&job.file)
        .map_err(|error| error.to_string())?;
    let found = session
        .project
        .timeline_mut(job.timeline)
        .ok_or_else(missing)?;
    found.subtitles.push(disccraft_project::SubtitleTrack {
        asset,
        language: job.language.clone(),
        group: 0,
    });
    Ok((asset, found.subtitles.len().saturating_sub(1)))
}

/// Takes `jobs` out now, each laid as a subtitle track: how many were, and the streams that could not be (to add to
/// those left out).
fn take_now(session: &mut Session, jobs: &[SubtitleJob]) -> (usize, Vec<String>) {
    let mut added = 0usize;
    let mut left_out = Vec::new();
    for job in jobs {
        match job.take(&|| true).and_then(|()| add_taken(session, job)) {
            Ok(_) => added += 1,
            Err(_) => left_out.push(job.name.clone()),
        }
    }
    (added, left_out)
}

/// The subtitles of an import, as a command answers them: taken out now (`subtitleTracks` added), or, with
/// `"subtitlesLater": true`, as `subtitleJobs` for the caller to take out. Gives (tracks, left out, jobs).
pub(crate) fn answer_subtitles(
    session: &mut Session,
    later: bool,
    jobs: &[SubtitleJob],
    mut left_out: Vec<String>,
) -> (Value, Value, Value) {
    if later {
        let jobs: Vec<Value> = jobs.iter().map(SubtitleJob::to_json).collect();
        return (Value::from(0), Value::from(left_out), Value::from(jobs));
    }
    let (added, failed) = take_now(session, jobs);
    left_out.extend(failed);
    (
        Value::from(added),
        Value::from(left_out),
        Value::from(Vec::<Value>::new()),
    )
}

/// `timeline.addBluraySubtitle`: lays the `.sup` of a subtitle job, once taken out, as a subtitle track.
fn run_add_subtitle(session: &mut Session, params: &Value) -> CommandResult {
    let job = SubtitleJob::from_json(
        params
            .get("job")
            .ok_or_else(|| "job is missing".to_string())?,
    )?;
    let (asset, track) = add_taken(session, &job)?;
    Ok(object(&[
        ("asset", Value::from(asset.0)),
        ("track", Value::from(track)),
    ]))
}

/// `timeline.importBluray`: a timeline of playlist `playlist` (the longest when not given) of the disc at `path`; a
/// timeline the project has of the same clips is given instead of a new one (`"existing": true`).
fn run_import(session: &mut Session, params: &Value) -> CommandResult {
    let path = req_str(params, "path")?;
    let wanted = opt_u64(params, "playlist")?;
    let name = opt_str(params, "name")?;
    let disc = open_disc(&path)?;
    let number = match wanted {
        Some(wanted) => u32::try_from(wanted).map_err(|_| format!("no playlist {wanted}"))?,
        None => {
            main_playlist(&disc).ok_or_else(|| "the disc has no readable playlist".to_string())?
        }
    };
    let later = opt_bool(params, "subtitlesLater")?.unwrap_or(false);
    let imported = import_playlist(session, &disc, number, name)?;
    let (subtitle_tracks, subtitles_left_out, subtitle_jobs) = answer_subtitles(
        session,
        later,
        &imported.subtitle_jobs,
        imported.subtitles_left_out,
    );
    Ok(object(&[
        ("timeline", Value::from(imported.timeline.0)),
        ("playlist", Value::from(number)),
        ("clips", Value::from(imported.clips)),
        ("chapters", Value::from(imported.chapters)),
        ("audioTracks", Value::from(imported.tracks)),
        ("audioLeftOut", Value::from(imported.left_out)),
        ("subtitleTracks", subtitle_tracks),
        ("subtitlesLeftOut", subtitles_left_out),
        ("subtitleJobs", subtitle_jobs),
        ("assets", Value::from(imported.assets)),
        ("existing", Value::from(imported.existing)),
    ]))
}
