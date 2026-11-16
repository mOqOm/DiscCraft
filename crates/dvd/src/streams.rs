//! Prepared elementary streams of every title, for multiplexers other than the DVD one (Blu-ray).

use std::path::Path;

use disccraft_menu::subtitle::{CueBitmap, render_cue};
use disccraft_menu::subtitle_file::read_subtitles;
use disccraft_project::{Project, Tick, Timeline, VideoStandard};
use disccraft_psmux::es::{Ac3Stream, LpcmStream, VideoStream};

use crate::source::{self, LoadedAudio, LoadedTitle};
use crate::{DvdError, load_slideshow_title, prepare, slideshow_ids, ticks_to_27m};

/// Where a title comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleSource {
    /// A timeline (its id).
    Timeline(u64),
    /// A slideshow (its id).
    Slideshow(u64),
}

/// One audio track of a title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TitleAudio {
    /// AC-3 frames and the elementary stream bytes.
    Ac3 {
        /// Parsed frames (27 MHz time stamps).
        stream: Ac3Stream,
        /// Elementary stream bytes.
        data: Vec<u8>,
        /// ISO 639-1 language code, when set.
        language: Option<[u8; 2]>,
    },
    /// LPCM samples (big-endian, 16-bit, 48 kHz).
    Lpcm {
        /// Parsed samples.
        stream: LpcmStream,
        /// ISO 639-1 language code, when set.
        language: Option<[u8; 2]>,
    },
}

/// One subtitle cue rendered as a 2-bit bitmap (0 transparent, 1 fill, 2 outline).
#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleCue {
    /// First presentation time, 27 MHz.
    pub start: i64,
    /// End of the display, 27 MHz.
    pub end: i64,
    /// Rendered bitmap and its position in the frame.
    pub bitmap: CueBitmap,
}

/// One subtitle track of a title.
#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleTrack {
    /// ISO 639-1 language code, when set.
    pub language: Option<[u8; 2]>,
    /// Cues in time order.
    pub cues: Vec<SubtitleCue>,
    /// A PGS (`.sup`) track's own segments, passed through (times 90 kHz on the title's clock, as `first_pts`);
    /// empty for drawn cues.
    pub segments: Vec<crate::PgSegment>,
}

/// Elementary streams of one title, in the DVD-compatible encoding (MPEG-2 video, LPCM / AC-3 audio).
#[derive(Debug, Clone, PartialEq)]
pub struct TitleStreams {
    /// Where the title comes from.
    pub source: TitleSource,
    /// Parsed video (27 MHz time stamps).
    pub video: VideoStream,
    /// Video elementary stream bytes.
    pub video_data: Vec<u8>,
    /// Audio tracks in timeline order.
    pub audio: Vec<TitleAudio>,
    /// Chapter start offsets from the first picture, 27 MHz.
    pub chapter_starts: Vec<i64>,
    /// Presentation time of the first picture, 27 MHz.
    pub first_pts: i64,
    /// 16:9 anamorphic video.
    pub wide: bool,
    /// Subtitle tracks (rendered SRT cues) in timeline order.
    pub subtitles: Vec<SubtitleTrack>,
}

/// Prepare (transcode when needed, using the cache) and load the streams of every title: timelines first, then slideshows.
///
/// # Errors
///
/// Returns the same errors as a DVD build: invalid project, unsupported or unreadable media, I/O.
pub fn load_title_streams(
    project: &Project,
    cache_dir: &Path,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<Vec<TitleStreams>, DvdError> {
    load_titles(project, cache_dir, progress, true)
}

/// [`load_title_streams`] for a Blu-ray in HD, which encodes the timelines' video on its own: the timelines come
/// with their sound, chapters and timing but no video (and no SD subtitles), so their MPEG-2 is never made.
///
/// # Errors
///
/// As [`load_title_streams`].
pub fn load_title_streams_for_hd(
    project: &Project,
    cache_dir: &Path,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<Vec<TitleStreams>, DvdError> {
    load_titles(project, cache_dir, progress, false)
}

fn load_titles(
    project: &Project,
    cache_dir: &Path,
    progress: &mut dyn FnMut(&str, f32),
    timeline_video: bool,
) -> Result<Vec<TitleStreams>, DvdError> {
    let budget = prepare::budget(project)?;
    // Transcode the titles in parallel first; the titles below then come from the cache.
    prepare::prepare_all_of(project, cache_dir, &budget, progress, timeline_video)?;
    let mut titles = Vec::new();
    let standard = project.settings.standard;
    for timeline in project.timelines.values() {
        let source = TitleSource::Timeline(timeline.id.0);
        let title = if timeline_video {
            let prepared = prepare::prepare_title(project, timeline, cache_dir, &budget, progress)?;
            let loaded = source::load_title(project, timeline, standard, &prepared)?;
            let mut title = convert(source, loaded)?;
            title.subtitles = subtitle_tracks(project, timeline, title.first_pts)?;
            title
        } else {
            let prepared = prepare::prepare_title_sound(project, timeline, cache_dir)?;
            convert(
                source,
                source::load_title_without_video(project, timeline, standard, &prepared)?,
            )?
        };
        titles.push(title);
    }
    for id in slideshow_ids(project) {
        let slideshow = project
            .slideshows
            .get(&id)
            .ok_or_else(|| DvdError::Invalid("slideshow does not exist".to_string()))?;
        let prepared =
            prepare::prepare_slideshow(project, slideshow, cache_dir, &budget, progress)?;
        let loaded =
            load_slideshow_title(project, slideshow, project.settings.standard, &prepared)?;
        titles.push(convert(TitleSource::Slideshow(id), loaded)?);
    }
    Ok(titles)
}

/// A loaded title as the Blu-ray build takes it: its SD video (none for an HD title's) read into memory.
fn convert(source: TitleSource, loaded: LoadedTitle) -> Result<TitleStreams, DvdError> {
    let video_data = loaded
        .video_data
        .bytes(0..loaded.video_data.len())
        .map_err(|err| DvdError::Invalid(format!("video read error: {err}")))?;
    Ok(TitleStreams {
        source,
        video: loaded.video,
        video_data,
        audio: loaded.audio.into_iter().map(convert_audio).collect(),
        chapter_starts: loaded.cell_starts,
        first_pts: loaded.first_pts,
        wide: loaded.wide,
        subtitles: Vec::new(),
    })
}

/// Parse and render every subtitle track of a timeline; cue times become 27 MHz presentation times.
fn subtitle_tracks(
    project: &Project,
    timeline: &Timeline,
    first_pts: i64,
) -> Result<Vec<SubtitleTrack>, DvdError> {
    let height = if project.settings.standard == VideoStandard::Ntsc {
        480
    } else {
        576
    };
    subtitle_tracks_sized(project, timeline, first_pts, (720, height))
}

/// [`subtitle_tracks`] drawn for a frame of `width` x `height` (a Blu-ray disc's HD titles).
///
/// # Errors
///
/// When a subtitle file cannot be read or a cue cannot be drawn.
pub fn subtitle_tracks_sized(
    project: &Project,
    timeline: &Timeline,
    first_pts: i64,
    (width, height): (u16, u16),
) -> Result<Vec<SubtitleTrack>, DvdError> {
    let mut tracks = Vec::with_capacity(timeline.subtitles.len());
    for track in &timeline.subtitles {
        let asset = project
            .assets
            .get(&track.asset)
            .ok_or_else(|| DvdError::Invalid("subtitle asset does not exist".to_string()))?;
        let path = Path::new(&asset.path);
        match source::subtitle_file(path) {
            source::SubtitleFile::Text => {}
            source::SubtitleFile::Pgs => {
                // A Blu-ray's own subtitles: their segments as they are, moved onto the title's clock.
                let start = first_pts / 300;
                let segments = crate::sup::read_sup(path)?
                    .into_iter()
                    .map(|segment| crate::PgSegment {
                        pts: start + segment.pts,
                        dts: start + segment.dts,
                        data: segment.data,
                    })
                    .collect();
                tracks.push(SubtitleTrack {
                    language: source::parse_language(&track.language),
                    cues: Vec::new(),
                    segments,
                });
                continue;
            }
            source::SubtitleFile::VobSub => {
                return Err(DvdError::Unsupported(format!(
                    "{}: VobSub (.idx/.sub) subtitles are a DVD's; a Blu-ray takes PGS (.sup) or text subtitles",
                    path.display()
                )));
            }
        }
        let cues = read_subtitles(path, &project.settings)
            .map_err(|e| DvdError::Invalid(e.to_string()))?;
        let mut rendered = Vec::with_capacity(cues.len());
        for cue in cues {
            let bitmap = render_cue(&cue.text, width, height)
                .map_err(|e| DvdError::Invalid(format!("subtitle: {e}")))?;
            rendered.push(SubtitleCue {
                start: offset(first_pts, cue.start)?,
                end: offset(first_pts, cue.end)?,
                bitmap,
            });
        }
        tracks.push(SubtitleTrack {
            language: source::parse_language(&track.language),
            cues: rendered,
            segments: Vec::new(),
        });
    }
    Ok(tracks)
}

fn offset(first_pts: i64, at: Tick) -> Result<i64, DvdError> {
    first_pts
        .checked_add(ticks_to_27m(at))
        .ok_or_else(|| DvdError::Invalid("subtitle time overflow".to_string()))
}

fn convert_audio(audio: LoadedAudio) -> TitleAudio {
    match audio {
        LoadedAudio::Ac3 {
            stream,
            data,
            language,
        } => TitleAudio::Ac3 {
            stream,
            data,
            language,
        },
        LoadedAudio::Lpcm { stream, language } => TitleAudio::Lpcm { stream, language },
    }
}
