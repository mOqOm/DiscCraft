use std::fs;
use std::path::Path;

use disccraft_project::{Project, Timeline, VideoStandard};
use disccraft_psmux::es::{Ac3Stream, LpcmStream, VideoStream, parse_ac3, parse_video, parse_wav};

use crate::prepare::{PreparedAudio, PreparedTitle};
use crate::{DvdError, ticks_to_27m};
use disccraft_menu::subtitle::{Cue, render_cue};
use disccraft_menu::subtitle_file::read_subtitles;
use disccraft_project::Tick;
use disccraft_psmux::mux::SubpictureUnit;
use disccraft_spu::{Bitmap, SpuParams, encode_spu, spu_delay_from_90k};

const MAX_INPUT_BYTES: u64 = 8_589_934_592;
const START_DTS: i64 = 8_100_000;
const MAX_AUDIO: usize = 8;
const MAX_CELLS: usize = 255;

/// One title's parsed and validated inputs.
pub(crate) struct LoadedTitle {
    pub(crate) video: VideoStream,
    pub(crate) video_data: Vec<u8>,
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
    let video_data = read_path(&prepared.video_path)?;
    let video = parse_video(&video_data, START_DTS)
        .map_err(|err| DvdError::Invalid(format!("video parse error: {err}")))?;

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
    let (subpictures, subtitle_languages) = load_subtitles(project, timeline, first_pts, standard)?;

    // A transcoded asset brings its own soundtrack (AC-3) when the timeline has no explicit audio tracks.
    if audio.is_empty()
        && let Some(path) = &prepared.transcoded_audio
    {
        audio.push(load_soundtrack(path, first_pts, disc_language)?);
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
        subpictures,
        subtitle_languages,
        cell_still: 0,
    })
}

/// Load every SRT subtitle track of the timeline as DVD subpicture units (white text, black outline).
fn load_subtitles(
    project: &Project,
    timeline: &Timeline,
    first_pts: i64,
    standard: VideoStandard,
) -> Result<(Vec<Vec<SubpictureUnit>>, Vec<Option<[u8; 2]>>), DvdError> {
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
    let mut streams = Vec::new();
    let mut languages = Vec::new();
    for track in &timeline.subtitles {
        let asset = project
            .assets
            .get(&track.asset)
            .ok_or_else(|| DvdError::Invalid("subtitle asset does not exist".to_string()))?;
        // SRT, WebVTT or a timecoded text script, in UTF-8 / UTF-16 / Shift_JIS.
        let cues = read_subtitles(Path::new(&asset.path), &project.settings)
            .map_err(|e| DvdError::Invalid(e.to_string()))?;
        let mut units = Vec::with_capacity(cues.len());
        for cue in cues {
            units.push(subtitle_unit(&cue, first_pts, height)?);
        }
        streams.push(units);
        languages.push(parse_language(&track.language));
    }
    Ok((streams, languages))
}

fn subtitle_unit(cue: &Cue, first_pts: i64, height: u16) -> Result<SubpictureUnit, DvdError> {
    let bitmap = render_cue(&cue.text, 720, height)
        .map_err(|e| DvdError::Invalid(format!("subtitle: {e}")))?;
    let duration_90k = ticks_to_27m(Tick(cue.end.0.saturating_sub(cue.start.0))) / 300;
    let params = SpuParams {
        x: bitmap.x,
        y: bitmap.y,
        colors: [0, 1, 2, 0],
        alpha: [0, 15, 15, 0],
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
    let (want_width, want_height, want_rate) = match standard {
        VideoStandard::Ntsc => (720u16, 480u16, 4u8),
        VideoStandard::Pal => (720u16, 576u16, 3u8),
    };

    if sequence.width != want_width {
        return Err(DvdError::Unsupported(format!(
            "video width {} is not {want_width} for {standard:?}",
            sequence.width
        )));
    }
    if sequence.height != want_height {
        return Err(DvdError::Unsupported(format!(
            "video height {} is not {want_height} for {standard:?}",
            sequence.height
        )));
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
            PreparedAudio::Ac3(path) => {
                let data = read_path(path)?;
                let stream = parse_ac3(&data, first_pts)
                    .map_err(|err| DvdError::Invalid(format!("AC-3 parse error: {err}")))?;
                validate_ac3(&stream)?;
                LoadedAudio::Ac3 {
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
