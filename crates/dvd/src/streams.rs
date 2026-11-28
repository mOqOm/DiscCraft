//! Prepared elementary streams of every title, for multiplexers other than the DVD one (Blu-ray).

use std::path::Path;

use disccraft_menu::subtitle::CueBitmap;
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
    /// DTS frames and the elementary stream bytes.
    Dts {
        /// Parsed frames (27 MHz time stamps).
        stream: disccraft_psmux::es::DtsStream,
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
    /// MPEG-1 Layer II frames (a PAL DVD's; a Blu-ray title never has them).
    Mp2,
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
    /// The letters', the stroke's and the anti-alias colour, as red, green, blue and alpha.
    pub colors: [[u8; 4]; 3],
}

/// A colour group as the red, green, blue and alpha of the letters and of the stroke.
fn cue_colors(group: disccraft_project::SubtitleGroup) -> [[u8; 4]; 3] {
    let rgba = |color: disccraft_project::SubtitleColor| {
        let alpha = (u16::from(color.opacity.min(100)) * 255 + 50) / 100;
        let [r, g, b] = color.rgb;
        [r, g, b, u8::try_from(alpha).unwrap_or(255)]
    };
    [
        rgba(group.fill),
        rgba(group.stroke),
        rgba(group.antialias_color()),
    ]
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
    prepare::prepare_all_of(project, cache_dir, &budget, progress, timeline_video, None)?;
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
        let prepared = if timeline_video {
            prepare::prepare_slideshow(project, slideshow, cache_dir, &budget, progress)?
        } else {
            prepare::prepare_slideshow_without_video(project, slideshow, cache_dir)?
        };
        let loaded =
            load_slideshow_title(project, slideshow, project.settings.standard, &prepared)?;
        let mut title = convert(TitleSource::Slideshow(id), loaded)?;
        title.subtitles = slide_subtitles_sized(project, slideshow, &title, None)?;
        titles.push(title);
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

/// A subtitle whose text does not fit the subtitle area of the disc's frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtitleOverflow {
    /// `"timeline"` or `"slideshow"`.
    pub kind: &'static str,
    /// The timeline's or slideshow's id.
    pub id: u64,
    /// A timeline's subtitle track (from 1); 0 for a slideshow.
    pub track: usize,
    /// The subtitle in its track, or the slide in its show (from 1).
    pub number: usize,
    /// A timeline subtitle's in and out points.
    pub span: Option<(Tick, Tick)>,
}

/// The subtitles of timelines and slideshows whose text does not fit the subtitle area (see
/// [`disccraft_menu::subtitle::cue_overflows`]), for the disc's standard-definition frame; one entry a subtitle,
/// as Encore's check lists them. Subtitle files that cannot be read are left out (the build reports those).
#[must_use]
pub fn overflowing_subtitles(project: &Project) -> Vec<SubtitleOverflow> {
    let height = if project.settings.standard == VideoStandard::Ntsc {
        480
    } else {
        576
    };
    let overflows = |text: &str| disccraft_menu::subtitle::cue_overflows(text, 720, height);
    let mut found = Vec::new();
    for timeline in project.timelines.values() {
        for (track_index, track) in timeline.subtitles.iter().enumerate() {
            let Some(asset) = project.assets.get(&track.asset) else {
                continue;
            };
            let Ok(cues) = disccraft_menu::subtitle_file::asset_cues(asset, &project.settings)
            else {
                continue;
            };
            for (cue_index, cue) in cues.iter().enumerate() {
                if overflows(&cue.text) {
                    found.push(SubtitleOverflow {
                        kind: "timeline",
                        id: timeline.id.0,
                        track: track_index.saturating_add(1),
                        number: cue_index.saturating_add(1),
                        span: Some((cue.start, cue.end)),
                    });
                }
            }
        }
    }
    for (id, slideshow) in &project.slideshows {
        for (index, slide) in slideshow.slides.iter().enumerate() {
            if slide.subtitle && overflows(&project.slide_subtitle_text(slide)) {
                found.push(SubtitleOverflow {
                    kind: "slideshow",
                    id: *id,
                    track: 0,
                    number: index.saturating_add(1),
                    span: None,
                });
            }
        }
    }
    found
}

/// Two subtitles of a timeline that follow each other too closely for a Blu-ray disc.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtitleGap {
    /// The timeline's id.
    pub timeline: u64,
    /// The subtitle track (from 1).
    pub track: usize,
    /// Where the earlier subtitle ends and the later one starts.
    pub span: (disccraft_project::Tick, disccraft_project::Tick),
    /// The frames between them.
    pub frames: u32,
}

/// The fewest frames between two subtitles of a Blu-ray timeline (Encore's help, "Automatically fix subtitle
/// frame gaps for Blu-ray discs": an error when the gap is less than five frames).
pub const MIN_SUBTITLE_GAP_FRAMES: u32 = 5;

/// The subtitles of a Blu-ray project's timelines that follow the one before by fewer than
/// [`MIN_SUBTITLE_GAP_FRAMES`] frames. As in Encore this is asked of Blu-ray timelines whose frame is not
/// 720x480 (HD titles, and PAL ones); a DVD needs no gap. Subtitle files that cannot be read are left out.
#[must_use]
pub fn close_subtitles(project: &Project) -> Vec<SubtitleGap> {
    if project.settings.kind != disccraft_project::DiscKind::BluRay {
        return Vec::new();
    }
    let rate = match project.settings.bd_video.hd() {
        Some((_, _, num, den)) => f64::from(num) / f64::from(den.max(1)),
        None if project.settings.standard == VideoStandard::Pal => 25.0,
        // 720x480.
        None => return Vec::new(),
    };
    let mut found = Vec::new();
    for timeline in project.timelines.values() {
        for (track_index, track) in timeline.subtitles.iter().enumerate() {
            let Some(asset) = project.assets.get(&track.asset) else {
                continue;
            };
            let Ok(mut cues) = disccraft_menu::subtitle_file::asset_cues(asset, &project.settings)
            else {
                continue;
            };
            cues.sort_by_key(|cue| cue.start.0);
            for pair in cues.windows(2) {
                let (Some(earlier), Some(later)) = (pair.first(), pair.get(1)) else {
                    continue;
                };
                let seconds = later.start.seconds() - earlier.end.seconds();
                // Whole frames between the two (a tiny bit added against rounding of exact frame times);
                // `as` saturates, and subtitles that overlap have none.
                let frames = (seconds * rate + 1e-6).floor().max(0.0) as u32;
                if frames < MIN_SUBTITLE_GAP_FRAMES {
                    found.push(SubtitleGap {
                        timeline: timeline.id.0,
                        track: track_index.saturating_add(1),
                        span: (earlier.end, later.start),
                        frames,
                    });
                }
            }
        }
    }
    found
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
            // M6.546: a DVD's subtitles on a Blu-ray, each subpicture as a PGS subtitle, moved onto the title's clock.
            source::SubtitleFile::VobSub => {
                let start = first_pts / 300;
                let segments = crate::vobsub::vobsub_as_pgs(path, (width, height))?
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
        }
        let cues = disccraft_menu::subtitle_file::asset_cues(asset, &project.settings)
            .map_err(|e| DvdError::Invalid(e.to_string()))?;
        let mut rendered = Vec::with_capacity(cues.len());
        let set = timeline.subtitle_colors.unwrap_or_default();
        // A subtitle added in the Monitor and not typed yet has nothing to show; a picture subtitle (an image
        // script's) is its picture, at its place in the frame.
        for cue in cues.into_iter().filter(disccraft_menu::subtitle::cue_shows) {
            // The subtitle's own group, stroke and place where it has them, else the track's.
            let colors = cue_colors(set.group(cue.look.group.unwrap_or(track.group)));
            let bitmap = disccraft_menu::subtitle::render_any_cue(
                &cue,
                width,
                height,
                disccraft_menu::subtitle::CueStyle {
                    outline: cue.look.outline(),
                    vertical: cue.look.vertical(),
                    horizontal: cue.look.justify.unwrap_or_default(),
                },
            )
            .map_err(|e| DvdError::Invalid(format!("subtitle: {e}")))?;
            rendered.push(SubtitleCue {
                start: offset(first_pts, cue.start)?,
                end: offset(first_pts, cue.end)?,
                bitmap,
                colors,
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

/// The subtitle track of a slideshow with Encore's "Create Subtitle" slides: each such slide's name, shown while
/// the slide is up (from its cell's start to the next slide's). No track when no slide asks for one.
///
/// `size` draws them for a frame of that width and height (a Blu-ray disc's HD titles); `None` is the SD frame.
///
/// # Errors
///
/// When a subtitle cannot be drawn.
pub fn slide_subtitles_sized(
    project: &Project,
    slideshow: &disccraft_project::Slideshow,
    title: &TitleStreams,
    size: Option<(u16, u16)>,
) -> Result<Vec<SubtitleTrack>, DvdError> {
    if !slideshow.slides.iter().any(|slide| slide.subtitle) {
        return Ok(Vec::new());
    }
    let (width, height) = size.unwrap_or((
        720,
        if project.settings.standard == VideoStandard::Ntsc {
            480
        } else {
            576
        },
    ));
    let colors = slideshow.subtitle_colors.unwrap_or_default();
    // Where each slide starts: its cell, or — when the cells are not one a slide — the durations added up.
    let starts: Vec<i64> = if title.chapter_starts.len() == slideshow.slides.len() {
        title.chapter_starts.clone()
    } else {
        let mut at = 0_i64;
        slideshow
            .slides
            .iter()
            .map(|slide| {
                let start = at;
                at = at.saturating_add(ticks_to_27m(slide.duration));
                start
            })
            .collect()
    };
    let mut cues = Vec::new();
    for (index, slide) in slideshow.slides.iter().enumerate() {
        if !slide.subtitle {
            continue;
        }
        let name = project.slide_subtitle_text(slide);
        if name.is_empty() {
            continue;
        }
        let Some(start) = starts.get(index).copied() else {
            continue;
        };
        let end = starts
            .get(index.saturating_add(1))
            .copied()
            .unwrap_or_else(|| start.saturating_add(ticks_to_27m(slide.duration)));
        let bitmap = disccraft_menu::subtitle::render_cue_styled(
            &name,
            width,
            height,
            crate::source::slide_cue_style(slide),
        )
        .map_err(|e| DvdError::Invalid(format!("subtitle: {e}")))?;
        cues.push(SubtitleCue {
            start: title.first_pts.saturating_add(start),
            end: title.first_pts.saturating_add(end),
            bitmap,
            colors: cue_colors(colors.group(slide.subtitle_group)),
        });
    }
    Ok(vec![SubtitleTrack {
        language: project
            .settings
            .audio_language
            .as_deref()
            .and_then(source::parse_language),
        cues,
        segments: Vec::new(),
    }])
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
        LoadedAudio::Dts {
            stream,
            data,
            language,
        } => TitleAudio::Dts {
            stream,
            data,
            language,
        },
        LoadedAudio::Lpcm { stream, language } => TitleAudio::Lpcm { stream, language },
        LoadedAudio::Mp2 { .. } => TitleAudio::Mp2,
    }
}
