use std::convert::TryInto;
use std::io::{BufWriter, Write};
use std::path::Path;

use disccraft_bdmv::clpi::{self, ClipInfo};
use disccraft_bdmv::mpls::{PlayItem, StnTable};
use disccraft_bdmv::{Stream, StreamCoding};
use disccraft_dvd::{SubtitleTrack, TitleAudio, TitleStreams};
use disccraft_m2tsmux::{
    AUDIO_PID, EsInput, EsUnit, M2tsLayout, MuxConfig, PCR_PID, PG_PID, PMT_PID, StreamKind,
    VIDEO_PID, bd_lpcm_units, mux_m2ts,
};
use disccraft_project::DiscSettings;
use disccraft_psmux::es::{Ac3Frame, Ac3Stream, LpcmStream, PictureType, VideoStream};

use crate::hd::HdTitle;
use crate::{BdError, frame_rate, invalid, io_error, video_format, write_bytes};

/// A clip written to `STREAM` and `CLIPINF`.
pub(crate) struct BuiltClip {
    /// Play item referencing this clip.
    pub(crate) play_item: PlayItem,
    /// Absolute chapter marks in 45 kHz units.
    pub(crate) marks: Vec<u32>,
    /// Size in bytes of the M2TS and CLPI files.
    pub(crate) bytes: u64,
    /// Clip duration in 90 kHz units.
    pub(crate) duration_90k: u64,
}

/// A title clip with its elementary streams ready to multiplex (nothing written yet). A pop-up menu's IG stream
/// can still join it before it is written.
pub(crate) struct PreparedClip {
    number: u16,
    inputs: Vec<EsInput>,
    /// Audio and PG streams (the video stream is separate).
    streams: Vec<Stream>,
    video_stream: Stream,
    presentation_start: u32,
    presentation_end: u32,
    marks: Vec<u32>,
}

impl PreparedClip {
    /// The clip number.
    pub(crate) fn number(&self) -> u16 {
        self.number
    }

    /// Clip start in 90 kHz units (where a pop-up menu's display set goes).
    pub(crate) fn start_90k(&self) -> u64 {
        u64::from(self.presentation_start).saturating_mul(2)
    }

    /// The clip as a playlist would see it before it is written (play item, marks, duration; no bytes yet).
    pub(crate) fn planned(&self) -> Result<BuiltClip, BdError> {
        Ok(BuiltClip {
            play_item: build_play_item(
                self.number,
                self.presentation_start,
                self.presentation_end,
                &self.video_stream,
                &self.streams,
            )?,
            marks: self.marks.clone(),
            bytes: 0,
            duration_90k: duration_90k(self.presentation_start, self.presentation_end)?,
        })
    }

    /// Adds an interactive graphics stream (a pop-up menu).
    pub(crate) fn add_ig(&mut self, units: Vec<EsUnit>, stream: Stream) {
        self.inputs.push(EsInput {
            pid: stream.pid,
            kind: StreamKind::Igs,
            units,
        });
        self.streams.push(stream);
    }
}

/// Write one M2TS clip and its CLPI file (with `hd`, the title's HD video in place of its SD video).
pub(crate) fn write_clip(
    title: &TitleStreams,
    hd: Option<&HdTitle>,
    number: u16,
    settings: &DiscSettings,
    (stream_dir, clipinf_dir): (&Path, &Path),
) -> Result<BuiltClip, BdError> {
    let prepared = prepare_clip(title, hd, number, settings)?;
    write_prepared(prepared, stream_dir, clipinf_dir)
}

/// Assembles a title's streams and timing without writing anything.
pub(crate) fn prepare_clip(
    title: &TitleStreams,
    hd: Option<&HdTitle>,
    number: u16,
    settings: &DiscSettings,
) -> Result<PreparedClip, BdError> {
    let (presentation_start, presentation_end) = match hd {
        Some(hd) => hd_presentation_times(title, hd)?,
        None => presentation_times(title)?,
    };
    let (inputs, streams) = build_inputs(title, hd)?;
    let video_stream = Stream {
        pid: VIDEO_PID,
        coding: match hd {
            // HD is 16:9; a 4:3 source is pillarboxed in it.
            Some(hd) => StreamCoding::H264Video {
                video_format: hd.video_format(),
                frame_rate: hd.frame_rate(),
                aspect: 3,
            },
            None => video_coding(settings, title.wide),
        },
    };
    let marks = chapter_marks(presentation_start, &title.chapter_starts)?;
    Ok(PreparedClip {
        number,
        inputs,
        streams,
        video_stream,
        presentation_start,
        presentation_end,
        marks,
    })
}

/// Multiplexes a prepared clip and writes its M2TS and CLPI files.
pub(crate) fn write_prepared(
    prepared: PreparedClip,
    stream_dir: &Path,
    clipinf_dir: &Path,
) -> Result<BuiltClip, BdError> {
    let number = prepared.number;
    let m2ts_path = stream_dir.join(format!("{number:05}.m2ts"));
    let (layout, m2ts_size) = write_m2ts(&m2ts_path, &prepared.inputs)?;

    let clip_info = build_clip_info(
        &layout,
        &prepared.video_stream,
        &prepared.streams,
        prepared.presentation_start,
        prepared.presentation_end,
    )?;

    let clpi_path = clipinf_dir.join(format!("{number:05}.clpi"));
    let clpi_size = write_clpi(&clpi_path, &clip_info)?;

    let mut built = prepared.planned()?;
    built.bytes = m2ts_size
        .checked_add(clpi_size)
        .ok_or_else(|| invalid("clip size overflow"))?;
    Ok(built)
}

fn build_inputs(
    title: &TitleStreams,
    hd: Option<&HdTitle>,
) -> Result<(Vec<EsInput>, Vec<Stream>), BdError> {
    let video = match hd {
        Some(hd) => EsInput {
            pid: VIDEO_PID,
            kind: StreamKind::H264Video,
            units: hd_units(title, hd)?,
        },
        None => EsInput {
            pid: VIDEO_PID,
            kind: StreamKind::Mpeg2Video,
            units: video_units(title)?,
        },
    };
    let (audio_inputs, mut streams) = audio_inputs(title)?;
    let (pg_inputs, pg_streams) = match hd {
        Some(hd) => pg_inputs(&hd.subtitles, (hd.width, hd.height))?,
        None => pg_inputs(
            &title.subtitles,
            (title.video.sequence.width, title.video.sequence.height),
        )?,
    };

    let mut inputs = vec![video];
    inputs.extend(audio_inputs);
    inputs.extend(pg_inputs);
    streams.extend(pg_streams);
    Ok((inputs, streams))
}

/// Encode every subtitle track as a presentation graphics stream (PID `PG_PID + i`) for a `width` x `height`
/// video.
fn pg_inputs(
    subtitles: &[SubtitleTrack],
    (width, height): (u16, u16),
) -> Result<(Vec<EsInput>, Vec<Stream>), BdError> {
    let mut inputs = Vec::new();
    let mut streams = Vec::new();
    for (index, track) in subtitles.iter().enumerate() {
        let offset = u16::try_from(index)
            .ok()
            .filter(|i| *i < 32)
            .ok_or_else(|| invalid("a Blu-ray title holds at most 32 subtitle tracks"))?;
        let pid = PG_PID
            .checked_add(offset)
            .ok_or_else(|| invalid("PG PID overflow"))?;
        inputs.push(EsInput {
            pid,
            kind: StreamKind::Pgs,
            units: pg_units(track, width, height)?,
        });
        streams.push(Stream {
            pid,
            coding: StreamCoding::Pgs {
                language: iso639_2(track.language),
            },
        });
    }
    Ok((inputs, streams))
}

/// Subtitle bitmaps → PGS display sets. Palette: 0 transparent, 1 white text, 2 black outline.
fn pg_units(track: &SubtitleTrack, width: u16, height: u16) -> Result<Vec<EsUnit>, BdError> {
    let palette = vec![
        disccraft_pgs::rgba_to_entry(0, 0, 0, 0, 0),
        disccraft_pgs::rgba_to_entry(1, 255, 255, 255, 255),
        disccraft_pgs::rgba_to_entry(2, 0, 0, 0, 255),
    ];
    let mut subtitles = Vec::with_capacity(track.cues.len());
    for cue in &track.cues {
        subtitles.push(disccraft_pgs::Subtitle {
            start: to_90k(cue.start)?,
            end: to_90k(cue.end)?,
            x: cue.bitmap.x,
            y: cue.bitmap.y,
            width: cue.bitmap.width,
            height: cue.bitmap.height,
            pixels: cue.bitmap.pixels.clone(),
            palette: palette.clone(),
        });
    }
    let segments = disccraft_pgs::encode_subtitles(&subtitles, width, height)
        .map_err(|e| BdError::Unsupported(format!("subtitles: {e}")))?;
    Ok(segments
        .into_iter()
        .map(|s| EsUnit {
            pts: s.pts,
            dts: s.dts,
            data: s.data,
            random_access: false,
        })
        .collect())
}

fn video_units(title: &TitleStreams) -> Result<Vec<EsUnit>, BdError> {
    picture_units(&title.video, &title.video_data)
}

/// The access units of a title's HD video, timed from the title's first picture (where its audio starts too). An
/// IDR picture is a random access point.
fn hd_units(title: &TitleStreams, hd: &HdTitle) -> Result<Vec<EsUnit>, BdError> {
    let start =
        i64::try_from(to_90k(title.first_pts)?).map_err(|_| invalid("timestamp overflow"))?;
    let at = |time: i64| {
        start
            .checked_add(time)
            .and_then(|value| u64::try_from(value).ok())
            .ok_or_else(|| invalid("HD time stamp out of range"))
    };
    let mut units = Vec::with_capacity(hd.units.len());
    for unit in &hd.units {
        let begin =
            usize::try_from(unit.offset).map_err(|_| invalid("HD unit offset too large"))?;
        let end = begin
            .checked_add(usize::try_from(unit.len).map_err(|_| invalid("HD unit too large"))?)
            .ok_or_else(|| invalid("HD unit range overflow"))?;
        let data = hd
            .data
            .get(begin..end)
            .ok_or_else(|| invalid("HD unit outside its stream"))?
            .to_vec();
        units.push(EsUnit {
            pts: at(unit.pts)?,
            dts: at(unit.dts)?,
            data,
            random_access: unit.keyframe,
        });
    }
    if units.is_empty() {
        return Err(invalid("HD video has no pictures"));
    }
    Ok(units)
}

/// A title's presentation start and end (45 kHz) with its HD video: from its first picture to the end of the last
/// HD frame.
fn hd_presentation_times(title: &TitleStreams, hd: &HdTitle) -> Result<(u32, u32), BdError> {
    let start = to_45k(title.first_pts)?;
    let last = hd
        .units
        .iter()
        .map(|unit| unit.pts)
        .max()
        .ok_or_else(|| invalid("HD video has no pictures"))?;
    let length = last
        .checked_add(hd.frame_90k())
        .and_then(|end| u32::try_from(end / 2).ok())
        .ok_or_else(|| invalid("HD video too long"))?;
    let end = start
        .checked_add(length)
        .ok_or_else(|| invalid("presentation end overflow"))?;
    Ok((start, end))
}

/// The access units of a parsed MPEG-2 video stream (`video_data` its bytes), for the multiplexer.
pub(crate) fn picture_units(
    video: &VideoStream,
    video_data: &[u8],
) -> Result<Vec<EsUnit>, BdError> {
    let mut units = Vec::new();

    for picture in video.pictures.iter() {
        let end = picture
            .offset
            .checked_add(picture.len)
            .ok_or_else(|| invalid("picture range overflow"))?;
        let data = video_data
            .get(picture.offset..end)
            .ok_or_else(|| invalid("picture data out of range"))?
            .to_vec();
        let pts = to_90k(picture.pts)?;
        let dts = to_90k(picture.dts)?;
        let random_access = matches!(picture.kind, PictureType::I) && picture.has_sequence_header;

        units.push(EsUnit {
            pts,
            dts,
            data,
            random_access,
        });
    }

    Ok(units)
}

fn audio_inputs(title: &TitleStreams) -> Result<(Vec<EsInput>, Vec<Stream>), BdError> {
    let start_pts = to_90k(title.first_pts)?;
    let mut inputs = Vec::new();
    let mut streams = Vec::new();

    for (index, audio) in title.audio.iter().enumerate() {
        let pid = audio_pid(index)?;

        match audio {
            TitleAudio::Lpcm { stream, language } => {
                let coding = lpcm_coding(stream, *language)?;
                let units = bd_lpcm_units(&stream.data, stream.channels, start_pts)
                    .map_err(|error| BdError::Mux(error.to_string()))?;
                inputs.push(EsInput {
                    pid,
                    kind: StreamKind::Lpcm,
                    units,
                });
                streams.push(Stream { pid, coding });
            }
            TitleAudio::Ac3 {
                stream,
                data,
                language,
            } => {
                let coding = ac3_coding(stream, *language);
                let units = ac3_units(&stream.frames, data)?;
                inputs.push(EsInput {
                    pid,
                    kind: StreamKind::Ac3,
                    units,
                });
                streams.push(Stream { pid, coding });
            }
        }
    }

    Ok((inputs, streams))
}

fn audio_pid(index: usize) -> Result<u16, BdError> {
    if index >= 32 {
        return Err(BdError::Unsupported("too many audio tracks".to_string()));
    }

    let offset = u16::try_from(index).map_err(|_| invalid("audio index overflow"))?;
    AUDIO_PID
        .checked_add(offset)
        .ok_or_else(|| invalid("audio PID overflow"))
}

fn ac3_units(frames: &[Ac3Frame], data: &[u8]) -> Result<Vec<EsUnit>, BdError> {
    let mut units = Vec::new();

    for frame in frames.iter() {
        let end = frame
            .offset
            .checked_add(frame.len)
            .ok_or_else(|| invalid("AC-3 frame range overflow"))?;
        let frame_data = data
            .get(frame.offset..end)
            .ok_or_else(|| invalid("AC-3 frame data out of range"))?
            .to_vec();
        let pts = to_90k(frame.pts)?;

        units.push(EsUnit {
            pts,
            dts: pts,
            data: frame_data,
            random_access: true,
        });
    }

    Ok(units)
}

pub(crate) fn lpcm_coding(
    stream: &LpcmStream,
    language: Option<[u8; 2]>,
) -> Result<StreamCoding, BdError> {
    if stream.sample_rate != 48_000 {
        return Err(BdError::Unsupported(
            "LPCM sample rate must be 48 kHz".to_string(),
        ));
    }
    if stream.bits != 16 {
        return Err(BdError::Unsupported(
            "LPCM bit depth must be 16".to_string(),
        ));
    }
    if stream.channels == 0 || stream.channels > 2 {
        return Err(BdError::Unsupported(
            "LPCM channel count must be 1 or 2".to_string(),
        ));
    }

    let audio_format = if stream.channels == 2 { 3 } else { 1 };
    Ok(StreamCoding::Lpcm {
        audio_format,
        sample_rate: 1,
        language: iso639_2(language),
    })
}

fn ac3_coding(stream: &Ac3Stream, language: Option<[u8; 2]>) -> StreamCoding {
    let audio_format = if stream.channels <= 1 {
        1
    } else if stream.channels == 2 {
        3
    } else {
        6
    };

    StreamCoding::Ac3 {
        audio_format,
        sample_rate: 1,
        language: iso639_2(language),
    }
}

pub(crate) fn video_coding(settings: &DiscSettings, wide: bool) -> StreamCoding {
    StreamCoding::Mpeg2Video {
        video_format: video_format(settings.standard),
        frame_rate: frame_rate(settings.standard),
        aspect: if wide { 3 } else { 2 },
    }
}

pub(crate) fn iso639_2(language: Option<[u8; 2]>) -> [u8; 3] {
    match language {
        Some([b'j', b'a']) => *b"jpn",
        Some([b'e', b'n']) => *b"eng",
        Some([b'f', b'r']) => *b"fra",
        Some([b'd', b'e']) => *b"deu",
        Some([b'e', b's']) => *b"spa",
        Some([b'i', b't']) => *b"ita",
        Some([b'z', b'h']) => *b"zho",
        Some([b'k', b'o']) => *b"kor",
        Some([b'p', b't']) => *b"por",
        Some([b'r', b'u']) => *b"rus",
        Some([b'n', b'l']) => *b"nld",
        Some([b's', b'v']) => *b"swe",
        Some(_) => *b"und",
        None => *b"und",
    }
}

pub(crate) fn to_90k(timestamp: i64) -> Result<u64, BdError> {
    if timestamp < 0 {
        return Err(invalid("negative timestamp"));
    }

    let value = timestamp
        .checked_div(300)
        .ok_or_else(|| invalid("timestamp division overflow"))?;
    u64::try_from(value).map_err(|_| invalid("timestamp overflow"))
}

pub(crate) fn to_45k(timestamp: i64) -> Result<u32, BdError> {
    if timestamp < 0 {
        return Err(invalid("negative timestamp"));
    }

    let value = timestamp
        .checked_div(600)
        .ok_or_else(|| invalid("timestamp division overflow"))?;
    u32::try_from(value).map_err(|_| invalid("timestamp overflow"))
}

fn presentation_times(title: &TitleStreams) -> Result<(u32, u32), BdError> {
    let start = to_45k(title.first_pts)?;
    let end = to_45k(video_end(&title.video)?)?;
    if end < start {
        return Err(invalid("presentation end before start"));
    }

    Ok((start, end))
}

/// When the last picture of `video` ends (27 MHz).
pub(crate) fn video_end(video: &VideoStream) -> Result<i64, BdError> {
    let mut max_end: Option<i64> = None;

    for picture in video.pictures.iter() {
        let end = picture
            .pts
            .checked_add(picture.duration)
            .ok_or_else(|| invalid("picture end overflow"))?;
        max_end = Some(match max_end {
            Some(current) if current >= end => current,
            Some(_) => end,
            None => end,
        });
    }

    max_end.ok_or_else(|| invalid("video has no pictures"))
}

fn chapter_marks(presentation_start: u32, chapter_starts: &[i64]) -> Result<Vec<u32>, BdError> {
    let mut marks = Vec::new();

    for chapter in chapter_starts.iter() {
        let offset = to_45k(*chapter)?;
        let mark = presentation_start
            .checked_add(offset)
            .ok_or_else(|| invalid("chapter mark overflow"))?;
        marks.push(mark);
    }

    Ok(marks)
}

pub(crate) fn write_m2ts(path: &Path, inputs: &[EsInput]) -> Result<(M2tsLayout, u64), BdError> {
    let file = std::fs::File::create(path).map_err(|error| io_error(path, error))?;
    let mut writer = BufWriter::new(file);
    let layout = mux_m2ts(inputs, &MuxConfig::default(), &mut writer)
        .map_err(|error| BdError::Mux(error.to_string()))?;
    writer.flush().map_err(|error| io_error(path, error))?;

    if layout.late_packets > 0 {
        return Err(BdError::Mux(format!(
            "{} late packets",
            layout.late_packets
        )));
    }

    drop(writer);
    let size = std::fs::metadata(path)
        .map_err(|error| io_error(path, error))?
        .len();
    Ok((layout, size))
}

pub(crate) fn write_clpi(path: &Path, info: &ClipInfo) -> Result<u64, BdError> {
    let bytes = clpi::write_clpi(info).map_err(|error| BdError::Bdmv(error.to_string()))?;
    write_bytes(path, &bytes)
}

pub(crate) fn build_clip_info(
    layout: &M2tsLayout,
    video: &Stream,
    audio: &[Stream],
    presentation_start: u32,
    presentation_end: u32,
) -> Result<ClipInfo, BdError> {
    let mut streams = Vec::new();
    streams.push(*video);
    streams.extend(audio.iter().copied());

    Ok(ClipInfo {
        ts_recording_rate: 6_000_000,
        source_packets: layout.source_packets,
        pcr_pid: PCR_PID,
        pmt_pid: PMT_PID,
        presentation_start,
        presentation_end,
        streams,
        ep_pid: layout.ep_pid,
        ep_map: ep_entries(&layout.ep_map),
    })
}

pub(crate) fn ep_entries(entries: &[disccraft_m2tsmux::EpEntry]) -> Vec<clpi::EpEntry> {
    entries
        .iter()
        .map(|entry| clpi::EpEntry {
            pts: entry.pts,
            spn: entry.spn,
        })
        .collect()
}

fn build_play_item(
    number: u16,
    presentation_start: u32,
    presentation_end: u32,
    video: &Stream,
    audio: &[Stream],
) -> Result<PlayItem, BdError> {
    let clip = clip_name(number)?;
    let streams = StnTable {
        video: vec![*video],
        audio: audio
            .iter()
            .copied()
            .filter(|s| {
                matches!(
                    s.coding,
                    StreamCoding::Lpcm { .. } | StreamCoding::Ac3 { .. }
                )
            })
            .collect(),
        pg: audio
            .iter()
            .copied()
            .filter(|s| matches!(s.coding, StreamCoding::Pgs { .. }))
            .collect(),
        ig: audio
            .iter()
            .copied()
            .filter(|s| matches!(s.coding, StreamCoding::Igs { .. }))
            .collect(),
    };

    Ok(PlayItem {
        clip,
        in_time: presentation_start,
        out_time: presentation_end,
        streams,
        still_mode: 0,
        still_time: 0,
    })
}

pub(crate) fn clip_name(number: u16) -> Result<[u8; 5], BdError> {
    let name = format!("{number:05}");
    name.as_bytes()
        .try_into()
        .map_err(|_| invalid("clip name length"))
}

fn duration_90k(start: u32, end: u32) -> Result<u64, BdError> {
    let delta = end
        .checked_sub(start)
        .ok_or_else(|| invalid("duration underflow"))?;
    u64::from(delta)
        .checked_mul(2)
        .ok_or_else(|| invalid("duration overflow"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso639_maps_known_languages() {
        assert_eq!(iso639_2(Some(*b"ja")), *b"jpn");
        assert_eq!(iso639_2(Some(*b"en")), *b"eng");
        assert_eq!(iso639_2(Some(*b"xy")), *b"und");
        assert_eq!(iso639_2(None), *b"und");
    }

    #[test]
    fn timestamp_conversion() -> Result<(), BdError> {
        assert_eq!(to_90k(300)?, 1);
        assert_eq!(to_45k(600)?, 1);
        Ok(())
    }

    #[test]
    fn negative_timestamp_is_invalid() {
        assert!(to_90k(-1).is_err());
        assert!(to_45k(-1).is_err());
    }

    #[test]
    fn clip_name_formats() -> Result<(), BdError> {
        assert_eq!(clip_name(7)?, *b"00007");
        Ok(())
    }

    #[test]
    fn chapter_marks_are_absolute() -> Result<(), BdError> {
        let marks = chapter_marks(10, &[0, 600])?;
        assert_eq!(marks, vec![10, 11]);
        Ok(())
    }
}
