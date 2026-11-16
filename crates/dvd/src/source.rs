use std::fs;
use std::path::Path;

use disccraft_project::{Project, Timeline, VideoStandard};
use disccraft_psmux::es::{
    Ac3Stream, LpcmStream, VideoStream, parse_ac3, parse_video_reader, parse_wav,
};

use crate::prepare::{PreparedAudio, PreparedTitle};
use crate::{DvdError, ticks_to_27m};
use disccraft_menu::subtitle::{Cue, CueStyle, render_cue_styled};
use disccraft_project::Tick;
use disccraft_psmux::mux::{FileBytes, SubpictureUnit, VideoBytes};
use disccraft_spu::{Bitmap, SpuParams, encode_spu, spu_delay_from_90k};

const MAX_INPUT_BYTES: u64 = 8_589_934_592;
const START_DTS: i64 = 8_100_000;
const MAX_AUDIO: usize = 8;
const MAX_CELLS: usize = 255;

/// One title's parsed and validated inputs.
pub(crate) struct LoadedTitle {
    pub(crate) video: VideoStream,
    /// The video's bytes: read from its file as it is muxed (empty for a Blu-ray HD title's).
    pub(crate) video_data: Box<dyn VideoBytes + Send + Sync>,
    pub(crate) audio: Vec<LoadedAudio>,
    pub(crate) cell_starts: Vec<i64>,
    /// The cells (0-based) that start a program, a chapter: all of them but the layer break's own cell.
    pub(crate) program_cells: Vec<usize>,
    /// The cell (0-based) that starts at the manual layer break, when it is in this title.
    pub(crate) layer_break_cell: Option<usize>,
    pub(crate) first_pts: i64,
    pub(crate) ntsc: bool,
    pub(crate) wide: bool,
    /// One list of subpicture units per subtitle track, and the tracks' languages.
    pub(crate) subpictures: Vec<Vec<SubpictureUnit>>,
    pub(crate) subtitle_languages: Vec<Option<[u8; 2]>>,
    /// The title's palette (`00 Y Cr Cb`): the colours its subtitles draw with.
    pub(crate) palette: [u32; 16],
    /// Still time of every cell (`0xFF` holds each slide of a manual-advance slideshow until the viewer goes on).
    pub(crate) cell_still: u8,
}

/// One loaded audio stream.
pub(crate) enum LoadedAudio {
    /// AC-3 stream and its elementary bytes.
    Ac3 {
        /// Parsed AC-3 stream.
        stream: Ac3Stream,
        /// AC-3 elementary bytes.
        data: Vec<u8>,
        /// Optional ISO 639-1 language.
        language: Option<[u8; 2]>,
    },
    /// DTS stream and its elementary bytes.
    Dts {
        /// Parsed DTS stream.
        stream: disccraft_psmux::es::DtsStream,
        /// DTS elementary bytes.
        data: Vec<u8>,
        /// Optional ISO 639-1 language.
        language: Option<[u8; 2]>,
    },
    /// DVD LPCM stream parsed from WAV.
    Lpcm {
        /// Parsed LPCM stream.
        stream: LpcmStream,
        /// Optional ISO 639-1 language.
        language: Option<[u8; 2]>,
    },
}

/// Load and validate one timeline.
pub(crate) fn load_title(
    project: &Project,
    timeline: &Timeline,
    standard: VideoStandard,
    prepared: &PreparedTitle,
) -> Result<LoadedTitle, DvdError> {
    let (video, video_data) = load_video(&prepared.video_path)?;

    validate_video(&video, standard)?;
    let first_pts = first_pts(&video)?;
    let chapter_starts = cell_starts(timeline)?;
    let layer_break = match crate::prepare::layer_break_time(project, timeline) {
        Some(time) => {
            // The frame's own time, where the transcode started a GOP for it.
            let frame = crate::prepare::frame_at(time.0, standard);
            let frames = u64::try_from(video.pictures.len()).unwrap_or(u64::MAX);
            if frame >= frames {
                return Err(DvdError::Invalid(format!(
                    "the layer break (frame {frame}) is past the end of timeline \"{}\" ({frames} frames)",
                    timeline.name
                )));
            }
            Some(frame_time_27m(frame, standard))
        }
        None => None,
    };
    let (cell_starts, program_cells, layer_break_cell) =
        with_layer_break(&chapter_starts, layer_break, standard)?;
    let disc_language = audio_language(project);
    let mut audio = load_audio(timeline, first_pts, &prepared.audio_tracks, disc_language)?;
    let subtitles = load_subtitles(project, timeline, first_pts, standard)?;

    // A transcoded asset brings its own soundtrack (AC-3) when the timeline has no explicit audio tracks.
    if audio.is_empty()
        && let Some(path) = &prepared.transcoded_audio
    {
        audio.push(load_soundtrack(
            path,
            first_pts + prepared.soundtrack_delay,
            disc_language,
        )?);
    }

    Ok(LoadedTitle {
        video,
        video_data,
        audio,
        cell_starts,
        program_cells,
        layer_break_cell,
        first_pts,
        ntsc: standard == VideoStandard::Ntsc,
        wide: timeline.wide(&project.settings),
        subpictures: subtitles.streams,
        subtitle_languages: subtitles.languages,
        palette: subtitles.palette,
        cell_still: 0,
    })
}

/// A timeline's sound, chapters and timing without its video, for a Blu-ray title whose video is encoded in HD on
/// its own: the video is empty and starts at the time an I/P MPEG-2 title's would ([`START_DTS`]); the subtitles
/// are rendered at the HD size elsewhere.
pub(crate) fn load_title_without_video(
    project: &Project,
    timeline: &Timeline,
    standard: VideoStandard,
    prepared: &PreparedTitle,
) -> Result<LoadedTitle, DvdError> {
    let first_pts = START_DTS;
    let (cell_starts, program_cells, _) =
        with_layer_break(&cell_starts(timeline)?, None, standard)?;
    let disc_language = audio_language(project);
    let mut audio = load_audio(timeline, first_pts, &prepared.audio_tracks, disc_language)?;
    if audio.is_empty()
        && let Some(path) = &prepared.transcoded_audio
    {
        audio.push(load_soundtrack(
            path,
            first_pts + prepared.soundtrack_delay,
            disc_language,
        )?);
    }
    Ok(LoadedTitle {
        video: VideoStream {
            sequence: disccraft_psmux::es::SequenceInfo {
                width: 0,
                height: 0,
                aspect_code: 0,
                frame_rate_code: 0,
                bit_rate_400: 0,
                vbv_buffer_size: 0,
                progressive_sequence: false,
                profile_level: 0,
            },
            pictures: Vec::new(),
            has_sequence_end: false,
        },
        video_data: Box::new(Vec::new()),
        audio,
        cell_starts,
        program_cells,
        layer_break_cell: None,
        first_pts,
        ntsc: standard == VideoStandard::Ntsc,
        wide: timeline.wide(&project.settings),
        subpictures: Vec::new(),
        subtitle_languages: Vec::new(),
        palette: text_palette(),
        cell_still: 0,
    })
}

/// A title's subtitle streams (its subpicture units), their languages and the palette they draw with.
pub(crate) struct TitleSubtitles {
    pub(crate) streams: Vec<Vec<SubpictureUnit>>,
    pub(crate) languages: Vec<Option<[u8; 2]>>,
    pub(crate) palette: [u32; 16],
}

/// The kind of a subtitle file, by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubtitleFile {
    /// SRT, WebVTT or a timecoded text script: drawn.
    Text,
    /// VobSub (`.idx` + `.sub`): a DVD's subpictures, passed through.
    VobSub,
    /// PGS (`.sup`): a Blu-ray's subtitles, passed through.
    Pgs,
}

pub(crate) fn subtitle_file(path: &Path) -> SubtitleFile {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("idx") => SubtitleFile::VobSub,
        Some("sup") => SubtitleFile::Pgs,
        _ => SubtitleFile::Text,
    }
}

/// Load every subtitle track of the timeline as DVD subpicture units: text drawn (white text, black outline), a
/// VobSub's units as they are with their colours moved into the title's palette.
fn load_subtitles(
    project: &Project,
    timeline: &Timeline,
    first_pts: i64,
    standard: VideoStandard,
) -> Result<TitleSubtitles, DvdError> {
    if timeline.subtitles.len() > 32 {
        return Err(DvdError::Invalid(
            "a DVD title holds at most 32 subtitle tracks".to_string(),
        ));
    }
    let height = if standard == VideoStandard::Ntsc {
        480
    } else {
        576
    };
    let paths: Vec<&Path> = timeline
        .subtitles
        .iter()
        .map(|track| {
            project
                .assets
                .get(&track.asset)
                .map(|asset| Path::new(asset.path.as_str()))
                .ok_or_else(|| DvdError::Invalid("subtitle asset does not exist".to_string()))
        })
        .collect::<Result<_, _>>()?;
    let any_text = paths
        .iter()
        .any(|path| subtitle_file(path) == SubtitleFile::Text);
    let colors = timeline.subtitle_colors.unwrap_or_default();
    // Drawn text takes the colour set's entries (0 to 9); a VobSub's colours follow them.
    let mut palette =
        crate::vobsub::PaletteBuilder::new(any_text.then(|| subtitle_palette(&colors)));
    let mut streams = Vec::new();
    let mut languages = Vec::new();
    for (track, path) in timeline.subtitles.iter().zip(paths) {
        let (units, file_language) = match subtitle_file(path) {
            SubtitleFile::Text => {
                let asset = project.assets.get(&track.asset).ok_or_else(|| {
                    DvdError::Invalid("subtitle asset does not exist".to_string())
                })?;
                // SRT, WebVTT or a timecoded text script, in UTF-8 / UTF-16 / Shift_JIS (or the subtitles edited
                // in the project).
                let cues = disccraft_menu::subtitle_file::asset_cues(asset, &project.settings)
                    .map_err(|e| DvdError::Invalid(e.to_string()))?;
                let mut units = Vec::with_capacity(cues.len());
                // A subtitle added in the Monitor and not typed yet has nothing to show.
                for cue in cues.into_iter().filter(|cue| !cue.text.trim().is_empty()) {
                    // The subtitle's own group, stroke and place where it has them, else the track's.
                    units.push(subtitle_unit_styled(
                        &cue,
                        first_pts,
                        height,
                        CueStyle {
                            outline: cue.look.outline(),
                            vertical: cue.look.vertical(),
                            horizontal: cue.look.justify.unwrap_or_default(),
                        },
                        spu_look(&colors, cue.look.group.unwrap_or(track.group)),
                    )?);
                }
                (units, None)
            }
            SubtitleFile::VobSub => {
                let vobsub = crate::vobsub::read_vobsub(path)?;
                let mut units = Vec::with_capacity(vobsub.units.len());
                for (ms, mut spu) in vobsub.units {
                    palette.remap(&mut spu, &vobsub.palette)?;
                    let pts = first_pts.saturating_add(ms.saturating_mul(27_000));
                    units.push(SubpictureUnit { pts, data: spu });
                }
                (units, vobsub.language)
            }
            SubtitleFile::Pgs => {
                return Err(DvdError::Unsupported(format!(
                    "{}: PGS (.sup) subtitles are a Blu-ray's; a DVD takes VobSub (.idx/.sub) or text subtitles",
                    path.display()
                )));
            }
        };
        streams.push(units);
        languages.push(parse_language(&track.language).or(file_language));
    }
    Ok(TitleSubtitles {
        streams,
        languages,
        palette: palette.palette(),
    })
}

/// A title's colour table for a colour set: 0 black, then each group's letters, stroke and anti-alias colour (1 to 3
/// for group 1, 4 to 6 for group 2, 7 to 9 for group 3), the rest black. Entries are `00 Y Cr Cb`.
pub(crate) fn subtitle_palette(set: &disccraft_project::SubtitleColorSet) -> [u32; 16] {
    let black = disccraft_menu::ycrcb_entry([0, 0, 0]);
    let mut palette = [black; 16];
    for (index, group) in set.groups.iter().enumerate() {
        let base = index.saturating_mul(3);
        for (offset, color) in [
            (1, group.fill),
            (2, group.stroke),
            (3, group.antialias_color()),
        ] {
            if let Some(slot) = palette.get_mut(base.saturating_add(offset)) {
                *slot = disccraft_menu::ycrcb_entry(color.rgb);
            }
        }
    }
    palette
}

/// The colour numbers and contrasts of a subpicture of `group` (from 0): pixel 1 the letters, pixel 2 the stroke,
/// pixel 3 the anti-alias colour.
pub(crate) fn spu_look(set: &disccraft_project::SubtitleColorSet, group: u8) -> ([u8; 4], [u8; 4]) {
    let group = group.min(2);
    let colors = set.group(group);
    let base = group.saturating_mul(3);
    // 0 to 100 percent as a contrast of 0 to 15.
    let contrast = |opacity: u8| {
        let scaled = (u16::from(opacity.min(100)) * 15 + 50) / 100;
        u8::try_from(scaled).unwrap_or(15)
    };
    (
        [
            0,
            base.saturating_add(1),
            base.saturating_add(2),
            base.saturating_add(3),
        ],
        [
            0,
            contrast(colors.fill.opacity),
            contrast(colors.stroke.opacity),
            contrast(colors.antialias_color().opacity),
        ],
    )
}

/// The look of a slide's subtitle: its stroke and its place.
pub(crate) fn slide_cue_style(slide: &disccraft_project::Slide) -> CueStyle {
    CueStyle {
        outline: slide.subtitle_stroke.pixels(),
        vertical: slide.subtitle_align,
        horizontal: disccraft_project::SubtitleJustify::Center,
    }
}

/// One cue as a DVD subpicture unit, drawn in `style` with the colour numbers and contrasts of its group.
pub(crate) fn subtitle_unit_styled(
    cue: &Cue,
    first_pts: i64,
    height: u16,
    style: CueStyle,
    (colors, alpha): ([u8; 4], [u8; 4]),
) -> Result<SubpictureUnit, DvdError> {
    let bitmap = render_cue_styled(&cue.text, 720, height, style)
        .map_err(|e| DvdError::Invalid(format!("subtitle: {e}")))?;
    let duration_90k = ticks_to_27m(Tick(cue.end.0.saturating_sub(cue.start.0))) / 300;
    let params = SpuParams {
        x: bitmap.x,
        y: bitmap.y,
        colors,
        alpha,
        forced: false,
        stop_delay: Some(spu_delay_from_90k(
            u32::try_from(duration_90k).unwrap_or(u32::MAX),
        )),
    };
    let mut spu = Bitmap {
        width: bitmap.width,
        height: bitmap.height,
        pixels: bitmap.pixels,
    };
    crate::fit_subpicture(&mut spu, params.y, height);
    let data =
        encode_spu(&spu, &params).map_err(|e| DvdError::Invalid(format!("subtitle: {e}")))?;
    let pts = first_pts
        .checked_add(ticks_to_27m(cue.start))
        .ok_or_else(|| DvdError::Invalid("subtitle time overflow".to_string()))?;
    Ok(SubpictureUnit { pts, data })
}

/// Convert a channel count to the DVD byte range.
pub(crate) fn channels_u8<T: TryInto<u8>>(value: T) -> Result<u8, DvdError> {
    value
        .try_into()
        .map_err(|_| DvdError::Invalid("channel count out of range".to_string()))
}

/// The video elementary stream at `path`, parsed as it is read and then read again from the file as it is muxed:
/// a title's video is never in memory whole (bounded by MAX_INPUT_BYTES, as a DVD's titles are).
pub(crate) fn load_video(
    path: &Path,
) -> Result<(VideoStream, Box<dyn VideoBytes + Send + Sync>), DvdError> {
    let metadata = fs::metadata(path).map_err(|err| crate::io_error(path, err))?;
    if !metadata.is_file() {
        return Err(DvdError::Invalid(format!(
            "asset is not a file: {}",
            path.display()
        )));
    }
    if metadata.len() > MAX_INPUT_BYTES {
        return Err(DvdError::Unsupported(format!(
            "asset larger than 8 GiB: {}",
            path.display()
        )));
    }
    let file = fs::File::open(path).map_err(|err| crate::io_error(path, err))?;
    let video = parse_video_reader(std::io::BufReader::new(file), START_DTS)
        .map_err(|err| DvdError::Invalid(format!("video parse error: {err}")))?;
    let bytes = FileBytes::open(path).map_err(|err| crate::io_error(path, err))?;
    Ok((video, Box::new(bytes)))
}

/// Read a whole input file (bounded by MAX_INPUT_BYTES).
fn read_path(path: &Path) -> Result<Vec<u8>, DvdError> {
    let metadata = fs::metadata(path).map_err(|err| crate::io_error(path, err))?;

    if !metadata.is_file() {
        return Err(DvdError::Invalid(format!(
            "asset is not a file: {}",
            path.display()
        )));
    }
    if metadata.len() > MAX_INPUT_BYTES {
        return Err(DvdError::Unsupported(format!(
            "asset larger than 8 GiB: {}",
            path.display()
        )));
    }

    fs::read(path).map_err(|err| crate::io_error(path, err))
}

fn validate_video(video: &VideoStream, standard: VideoStandard) -> Result<(), DvdError> {
    let sequence = &video.sequence;
    let (ifo_standard, want_rate) = match standard {
        VideoStandard::Ntsc => (disccraft_ifo::Standard::Ntsc, 4u8),
        VideoStandard::Pal => (disccraft_ifo::Standard::Pal, 3u8),
    };

    // One of the DVD picture sizes: 720 or 704 wide at full height, 352 wide at full or half height.
    let Some(resolution) =
        disccraft_ifo::Resolution::of_picture(sequence.width, sequence.height, ifo_standard)
    else {
        return Err(DvdError::Unsupported(format!(
            "video size {}x{} is not a DVD size for {standard:?}",
            sequence.width, sequence.height
        )));
    };
    let small = matches!(
        resolution,
        disccraft_ifo::Resolution::Half | disccraft_ifo::Resolution::Quarter
    );
    if small && sequence.aspect_code == 3 {
        return Err(DvdError::Unsupported(
            "a 352-wide DVD video must be 4:3".to_string(),
        ));
    }
    if sequence.frame_rate_code != want_rate {
        return Err(DvdError::Unsupported(format!(
            "video frame_rate_code {} is not {want_rate} for {standard:?}",
            sequence.frame_rate_code
        )));
    }
    if sequence.aspect_code != 2 && sequence.aspect_code != 3 {
        return Err(DvdError::Unsupported(
            "DVD video must be 4:3 (aspect_code 2) or 16:9 (aspect_code 3)".to_string(),
        ));
    }

    let bit_rate = u64::from(sequence.bit_rate_400)
        .checked_mul(400)
        .ok_or_else(|| DvdError::Invalid("video bit rate overflow".to_string()))?;
    if bit_rate > 9_800_000 {
        return Err(DvdError::Unsupported(format!(
            "video bit rate {bit_rate} exceeds DVD maximum 9800000"
        )));
    }

    Ok(())
}

fn first_pts(video: &VideoStream) -> Result<i64, DvdError> {
    video
        .pictures
        .iter()
        .map(|picture| picture.pts)
        .min()
        .ok_or_else(|| DvdError::Invalid("video has no pictures".to_string()))
}

fn cell_starts(timeline: &Timeline) -> Result<Vec<i64>, DvdError> {
    if timeline.chapters.is_empty() {
        return Err(DvdError::Invalid("timeline has no chapters".to_string()));
    }
    if timeline.chapters.len() > MAX_CELLS {
        return Err(DvdError::Unsupported("too many chapters".to_string()));
    }

    let mut starts = Vec::with_capacity(timeline.chapters.len());
    let mut previous: Option<i64> = None;

    for chapter in &timeline.chapters {
        let time = ticks_to_27m(chapter.time);
        if time < 0 {
            return Err(DvdError::Invalid(
                "chapter time must not be negative".to_string(),
            ));
        }
        if let Some(prev) = previous
            && prev >= time
        {
            return Err(DvdError::Invalid(
                "chapter times must be strictly increasing".to_string(),
            ));
        }
        previous = Some(time);
        starts.push(time);
    }

    if starts.first().copied() != Some(0) {
        return Err(DvdError::Invalid(
            "first chapter must start at zero".to_string(),
        ));
    }

    Ok(starts)
}

/// The time (27 MHz) of `frame` at the disc's rate.
fn frame_time_27m(frame: u64, standard: VideoStandard) -> i64 {
    let per_frame: u64 = match standard {
        VideoStandard::Ntsc => 900_900,
        VideoStandard::Pal => 1_080_000,
    };
    i64::try_from(frame.saturating_mul(per_frame)).unwrap_or(i64::MAX)
}

/// The cells of a title: the chapters' starts and, when the manual layer break is in the title and not on a
/// chapter's frame, a cell of its own there that starts no program. Returns the cell starts (27 MHz), the cells that
/// start programs (0-based), and the layer break's cell.
pub(crate) fn with_layer_break(
    chapter_starts: &[i64],
    layer_break: Option<i64>,
    standard: VideoStandard,
) -> Result<(Vec<i64>, Vec<usize>, Option<usize>), DvdError> {
    let mut starts = chapter_starts.to_vec();
    let mut layer_cell = None;
    if let Some(at) = layer_break {
        // A chapter on the same frame is the break's cell already.
        let half_frame = frame_time_27m(1, standard) / 2;
        match starts
            .iter()
            .position(|start| (start - at).abs() <= half_frame)
        {
            Some(index) => layer_cell = Some(index),
            None => {
                let index = starts.partition_point(|start| *start < at);
                starts.insert(index, at);
                layer_cell = Some(index);
            }
        }
    }
    if starts.len() > MAX_CELLS {
        return Err(DvdError::Unsupported("too many chapters".to_string()));
    }
    let programs = (0..starts.len())
        .filter(|index| Some(*index) != layer_cell || chapter_starts.len() + 1 != starts.len())
        .collect();
    Ok((starts, programs, layer_cell))
}

/// The disc's language for audio without its own (`None` when it names none, or not as two letters).
pub(crate) fn audio_language(project: &Project) -> Option<[u8; 2]> {
    project
        .settings
        .audio_language
        .as_deref()
        .and_then(parse_language)
}

/// The timeline's audio tracks; one without a language of its own takes `disc_language`.
fn load_audio(
    timeline: &Timeline,
    first_pts: i64,
    prepared: &[PreparedAudio],
    disc_language: Option<[u8; 2]>,
) -> Result<Vec<LoadedAudio>, DvdError> {
    if timeline.audio.len() != prepared.len() {
        return Err(DvdError::Invalid(
            "prepared audio track count mismatch".to_string(),
        ));
    }
    if timeline.audio.len() > MAX_AUDIO {
        return Err(DvdError::Unsupported("too many audio tracks".to_string()));
    }

    let mut out = Vec::with_capacity(prepared.len());

    for (track, prepared_audio) in timeline.audio.iter().zip(prepared) {
        let language = parse_language(&track.language).or(disc_language);

        let loaded = match prepared_audio {
            PreparedAudio::Ac3(path) | PreparedAudio::DelayedAc3 { path, .. } => {
                let delay = match prepared_audio {
                    PreparedAudio::DelayedAc3 { delay, .. } => *delay,
                    _ => 0,
                };
                let data = read_path(path)?;
                let stream = parse_ac3(&data, first_pts + delay)
                    .map_err(|err| DvdError::Invalid(format!("AC-3 parse error: {err}")))?;
                validate_ac3(&stream)?;
                LoadedAudio::Ac3 {
                    stream,
                    data,
                    language,
                }
            }
            PreparedAudio::Dts(path) => {
                let data = read_path(path)?;
                let stream = disccraft_psmux::es::parse_dts(&data, first_pts)
                    .map_err(|err| DvdError::Invalid(format!("DTS parse error: {err}")))?;
                validate_dts(&stream)?;
                LoadedAudio::Dts {
                    stream,
                    data,
                    language,
                }
            }
            PreparedAudio::Wav(path) => {
                let data = read_path(path)?;
                let stream = parse_wav(&data)
                    .map_err(|err| DvdError::Invalid(format!("WAV parse error: {err}")))?;
                validate_lpcm(&stream)?;
                LoadedAudio::Lpcm { stream, language }
            }
        };

        out.push(loaded);
    }

    Ok(out)
}

/// A transcoded soundtrack: AC-3 (`.ac3`, as the build makes them) or LPCM from a WAV, in `language`.
pub(crate) fn load_soundtrack(
    path: &Path,
    first_pts: i64,
    language: Option<[u8; 2]>,
) -> Result<LoadedAudio, DvdError> {
    let data = read_path(path)?;
    let ac3 = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ac3"));
    if ac3 {
        let stream = parse_ac3(&data, first_pts)
            .map_err(|err| DvdError::Invalid(format!("transcoded audio: {err}")))?;
        validate_ac3(&stream)?;
        Ok(LoadedAudio::Ac3 {
            stream,
            data,
            language,
        })
    } else {
        let stream = parse_wav(&data)
            .map_err(|err| DvdError::Invalid(format!("transcoded audio: {err}")))?;
        validate_lpcm(&stream)?;
        Ok(LoadedAudio::Lpcm { stream, language })
    }
}

pub(crate) fn parse_language(language: &str) -> Option<[u8; 2]> {
    let bytes = language.as_bytes();
    if bytes.len() != 2 {
        return None;
    }

    let first = *bytes.first()?;
    let second = *bytes.get(1)?;
    if !first.is_ascii_alphabetic() || !second.is_ascii_alphabetic() {
        return None;
    }

    Some([first.to_ascii_lowercase(), second.to_ascii_lowercase()])
}

fn validate_ac3(stream: &Ac3Stream) -> Result<(), DvdError> {
    if stream.sample_rate != 48_000 {
        return Err(DvdError::Unsupported(format!(
            "AC-3 sample rate {} is not 48 kHz",
            stream.sample_rate
        )));
    }

    let channels = channels_u8(stream.channels)?;
    if channels == 0 || channels > 8 {
        return Err(DvdError::Unsupported(
            "AC-3 channel count must be 1..=8".to_string(),
        ));
    }

    Ok(())
}

/// A DTS stream a DVD carries as it is: 48 kHz, up to 8 channels, every frame small enough for one pack (a DVD
/// does not split DTS frames over packs; 2 013 bytes at 1 509.75 kbit/s is the largest in use).
fn validate_dts(stream: &disccraft_psmux::es::DtsStream) -> Result<(), DvdError> {
    /// The frame bytes one pack holds behind its headers.
    const PACK_ROOM: usize = 2016;
    if stream.sample_rate != 48_000 {
        return Err(DvdError::Unsupported(format!(
            "DTS sample rate {} is not 48 kHz",
            stream.sample_rate
        )));
    }
    if stream.channels == 0 || stream.channels > 8 {
        return Err(DvdError::Unsupported(
            "DTS channel count must be 1..=8".to_string(),
        ));
    }
    if stream.frames.iter().any(|frame| frame.len > PACK_ROOM) {
        return Err(DvdError::Unsupported(
            "DTS frames are too large for a DVD (the bit rate is over 1 509.75 kbit/s)".to_string(),
        ));
    }
    Ok(())
}

fn validate_lpcm(stream: &LpcmStream) -> Result<(), DvdError> {
    if stream.sample_rate != 48_000 {
        return Err(DvdError::Unsupported(format!(
            "LPCM sample rate {} is not 48 kHz",
            stream.sample_rate
        )));
    }
    if stream.bits != 16 {
        return Err(DvdError::Unsupported(
            "M2 supports only 16-bit LPCM".to_string(),
        ));
    }

    let channels = channels_u8(stream.channels)?;
    if channels == 0 || channels > 8 {
        return Err(DvdError::Unsupported(
            "LPCM channel count must be 1..=8".to_string(),
        ));
    }
    if stream.frame_bytes == 0 {
        return Err(DvdError::Invalid("LPCM frame size is zero".to_string()));
    }

    Ok(())
}

/// The palette of a title whose subtitles are all drawn text (and of one without subtitles).
pub(crate) fn text_palette() -> [u32; 16] {
    subtitle_palette(&disccraft_project::SubtitleColorSet::default())
}

#[cfg(test)]
mod subtitle_tests {
    use super::load_subtitles;
    use disccraft_project::{AssetKind, Project, SubtitleCue, SubtitleTrack, Tick, VideoStandard};
    use std::path::Path;

    /// Subtitles edited in the Monitor are built from the project, not the file (which need not exist), and one
    /// added but not typed yet is left off the disc.
    #[test]
    fn edited_subtitles_are_built_from_the_project() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let video = project.add_asset(Path::new("movie.mp4"))?;
        let subs = project.add_asset(Path::new("no-such-file.srt"))?;
        let timeline = project.add_timeline(video, None)?;
        let cue = |start: i64, text: &str| SubtitleCue {
            start: Tick(start * 254_016_000_000),
            end: Tick((start + 1) * 254_016_000_000),
            text: text.to_owned(),
            look: disccraft_project::CueLook::default(),
        };
        let asset = project.assets.get_mut(&subs).ok_or("asset")?;
        assert_eq!(asset.kind, AssetKind::Subtitle);
        asset.cues = Some(vec![cue(1, "Hello"), cue(3, " "), cue(5, "World")]);
        let timeline = project.timeline_mut(timeline).ok_or("timeline")?;
        timeline.subtitles.push(SubtitleTrack {
            asset: subs,
            language: "en".to_owned(),
            group: 0,
        });
        let timeline = timeline.clone();
        let loaded = load_subtitles(&project, &timeline, 0, VideoStandard::Ntsc)?;
        let (streams, languages) = (loaded.streams, loaded.languages);
        assert_eq!(streams.first().map(Vec::len), Some(2));
        assert_eq!(languages, vec![Some(*b"en")]);
        // Without edited cues the file is read, and a missing file is an error.
        if let Some(asset) = project.assets.get_mut(&subs) {
            asset.cues = None;
        }
        assert!(load_subtitles(&project, &timeline, 0, VideoStandard::Ntsc).is_err());
        Ok(())
    }
}

#[cfg(test)]
mod layer_break_tests {
    use super::with_layer_break;
    use disccraft_project::VideoStandard;

    /// The layer break between chapters gets a cell of its own that starts no program; on a chapter's frame it is
    /// that chapter's cell; without one every cell is a program.
    #[test]
    fn the_layer_break_cell_and_the_programs() -> Result<(), Box<dyn std::error::Error>> {
        let ntsc = VideoStandard::Ntsc;
        let chapters = [0, 37 * 900_900, 50 * 900_900];
        let (starts, programs, cell) = with_layer_break(&chapters, Some(40 * 900_900), ntsc)?;
        assert_eq!(starts, vec![0, 37 * 900_900, 40 * 900_900, 50 * 900_900]);
        assert_eq!(programs, vec![0, 1, 3]);
        assert_eq!(cell, Some(2));
        let (starts, programs, cell) = with_layer_break(&chapters, Some(37 * 900_900 + 10), ntsc)?;
        assert_eq!(starts, chapters.to_vec());
        assert_eq!(programs, vec![0, 1, 2]);
        assert_eq!(cell, Some(1));
        let (starts, programs, cell) = with_layer_break(&chapters, None, ntsc)?;
        assert_eq!((starts.len(), programs, cell), (3, vec![0, 1, 2], None));
        Ok(())
    }
}

#[cfg(test)]
mod color_tests {
    use super::{spu_look, subtitle_palette};
    use disccraft_project::{SubtitleColor, SubtitleColorSet};

    /// The default colour set is Encore's (group 1: pale yellow letters, near-black stroke, both opaque); the groups
    /// take the colour numbers 1 to 6 in turn, and an opacity becomes a contrast of 0 to 15.
    #[test]
    fn a_colour_set_becomes_the_titles_colour_table() {
        let mut set = SubtitleColorSet::default();
        let palette = subtitle_palette(&set);
        let entry = |rgb| Some(disccraft_menu::ycrcb_entry(rgb));
        assert_eq!(palette.first().copied(), Some(0x0010_8080));
        assert_eq!(palette.get(1).copied(), entry([234, 234, 176]));
        assert_eq!(palette.get(2).copied(), entry([16, 16, 16]));
        // The anti-alias colour is halfway between the two, as in Encore's default set.
        assert_eq!(palette.get(3).copied(), entry([125, 125, 96]));
        assert_eq!(palette.get(7).copied(), entry([196, 196, 233]));
        assert_eq!(spu_look(&set, 0), ([0, 1, 2, 3], [0, 15, 15, 15]));
        assert_eq!(spu_look(&set, 2), ([0, 7, 8, 9], [0, 15, 15, 15]));

        if let Some(group) = set.groups.get_mut(1) {
            group.fill = SubtitleColor {
                rgb: [255, 0, 0],
                opacity: 47,
            };
            group.stroke.opacity = 0;
            group.antialias = Some(SubtitleColor {
                rgb: [0, 0, 255],
                opacity: 100,
            });
        }
        let palette = subtitle_palette(&set);
        assert_eq!(palette.get(4).copied(), entry([255, 0, 0]));
        assert_eq!(palette.get(6).copied(), entry([0, 0, 255]));
        assert_eq!(spu_look(&set, 1), ([0, 4, 5, 6], [0, 7, 0, 15]));
    }
}
