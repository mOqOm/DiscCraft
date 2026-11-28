//! AVI and Matroska files whose video is a format DiscCraft decodes itself (M6.561, [`crate::own_video`]). The media
//! reader opens them (their length, their sound) but asks no decoder of DiscCraft's for their video, so their frames
//! are read here, an AVI's chunks with [`crate::avi`] and a Matroska file's blocks with craft-media's Matroska reader,
//! and decoded with [`crate::own_video::decoder`]; the rest comes from the media reader's own source.
//!
//! M6.567: an Ogg file's Theora video (`.ogv`) too, its packets from craft-media's Ogg reader. The media reader shows
//! such a file's sound but no video, and cannot open one with no sound at all ([`alone`]), so the video stream is
//! described from the Theora headers.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use craft_media::matroska::{MkvFile, OpenOptions, TrackKind, open_with};
use filmcraft_frame::{AudioBuffer, VideoFrame};
use filmcraft_media::reader::SharedReader;
use filmcraft_media::{
    FrameRequest, MediaError, MediaInfo, MediaKind, MediaSource, VideoStreamInfo,
};
use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick};

use crate::TranscodeError;
use crate::avi::{code_at, le32, read};
use crate::own_video::{Format, FrameDecoder, decoder};

/// How far back a key frame is looked for before decoding from the first frame.
const MOST_BACK: usize = 10_000;

/// The largest codec setup data read from a file (a BITMAPINFOHEADER's tail, a Matroska track's private data).
const MOST_SETUP: u32 = 1 << 20;

/// The largest coded frame read from an Ogg file (its packets may span any number of pages).
const MOST_PACKET: u64 = 64 << 20;

/// Where a file's coded frames are.
enum Frames {
    /// An AVI stream's chunks: where each one's data is and its length (0: the frame before it repeated).
    Avi {
        reader: SharedReader,
        chunks: Vec<(u64, u32)>,
    },
    /// A Matroska track's blocks, with each one's time in seconds (decoding order).
    Matroska {
        source: Shared,
        file: Box<MkvFile>,
        track: usize,
        times: Vec<f64>,
    },
    /// An Ogg stream's data packets, one frame each: where each one's pieces are (an empty packet: the frame before
    /// it repeated).
    Ogg {
        source: Shared,
        packets: Vec<Vec<(u64, u32)>>,
    },
}

impl Frames {
    fn count(&self) -> usize {
        match self {
            Self::Avi { chunks, .. } => chunks.len(),
            Self::Matroska { times, .. } => times.len(),
            Self::Ogg { packets, .. } => packets.len(),
        }
    }

    /// The coded bytes of frame `index` (empty for an AVI's empty chunk).
    fn read(&self, index: usize) -> Result<Vec<u8>, String> {
        match self {
            Self::Avi { reader, chunks } => {
                let &(at, size) = chunks.get(index).ok_or("no such frame")?;
                if size == 0 {
                    return Ok(Vec::new());
                }
                read(reader, at, size as usize)
                    .ok_or_else(|| "a frame past the end of the file".to_string())
            }
            Self::Matroska {
                source,
                file,
                track,
                ..
            } => file
                .read_sample(source, *track, index)
                .map_err(|error| error.to_string()),
            Self::Ogg { source, packets } => {
                let parts = packets.get(index).ok_or("no such frame")?;
                let size: u64 = parts.iter().map(|&(_, length)| u64::from(length)).sum();
                if size > MOST_PACKET {
                    return Err("a frame too large to read".to_string());
                }
                let mut coded = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
                for &(at, length) in parts {
                    let mut piece = vec![0u8; length as usize];
                    craft_media::ByteSource::read_at(source, at, &mut piece)
                        .map_err(|error| error.to_string())?;
                    coded.extend_from_slice(&piece);
                }
                Ok(coded)
            }
        }
    }

    /// The last key frame at or before `index` (a frame later ones are decoded from): a Matroska block marked so, an
    /// AVI chunk the format says is one; 0 when none is found within [`MOST_BACK`] frames.
    fn key_at_or_before(&self, index: usize, format: Format) -> Result<usize, String> {
        let first = index.saturating_sub(MOST_BACK);
        match self {
            Self::Matroska { file, track, .. } => {
                let samples = file
                    .tracks
                    .get(*track)
                    .map(|track| track.samples.as_slice())
                    .unwrap_or_default();
                Ok((first..=index)
                    .rev()
                    .find(|&at| samples.get(at).is_some_and(|sample| sample.keyframe))
                    .unwrap_or(0))
            }
            Self::Avi { .. } | Self::Ogg { .. } => {
                for at in (first..=index).rev() {
                    let coded = self.read(at)?;
                    if !coded.is_empty() && crate::own_video::is_key(format, &coded) {
                        return Ok(at);
                    }
                }
                Ok(0)
            }
        }
    }

    /// The frame shown at `time` (`rate` for the frames of an AVI, which come one a frame duration).
    fn index_at(&self, time: Tick, rate: FrameRate) -> usize {
        let last = self.count().saturating_sub(1);
        match self {
            Self::Avi { .. } | Self::Ogg { .. } => usize::try_from(rate.frame_at(time).max(0))
                .unwrap_or(0)
                .min(last),
            Self::Matroska { times, .. } => {
                // A little past the asked time, so a frame whose time is rounded a hair later is still found.
                let seconds = time.0 as f64 / TICKS_PER_SECOND as f64 + 1e-6;
                times
                    .partition_point(|&at| at <= seconds)
                    .saturating_sub(1)
                    .min(last)
            }
        }
    }
}

/// A file's media reader with DiscCraft decoding its video.
struct OwnVideo {
    info: MediaInfo,
    general: Arc<dyn MediaSource>,
    frames: Frames,
    format: Format,
    setup: Vec<u8>,
    /// The picture size the container gives.
    size: (u32, u32),
    rate: FrameRate,
    state: Mutex<State>,
}

/// The decoder of a source whose frames need the ones before them, and the last picture given.
struct State {
    decoder: Option<Box<dyn FrameDecoder>>,
    /// The frame the decoder takes next (frames are decoded in order from a key frame).
    next: usize,
    /// The decoder's last picture (shown again for an empty frame after it).
    shown: Option<VideoFrame>,
    last: Option<(usize, Arc<VideoFrame>)>,
}

/// `general` (the media reader's source of the file `reader`, whose first bytes are `head`), with its video decoded
/// by DiscCraft when it is an AVI or a Matroska file whose video is a format DiscCraft decodes itself.
///
/// # Errors
///
/// When it is such a file but its frames cannot be found.
pub(crate) fn with_own_video(
    reader: &SharedReader,
    head: &[u8],
    general: Arc<dyn MediaSource>,
) -> Result<Arc<dyn MediaSource>, TranscodeError> {
    let mut described = None;
    let found = if head.get(..4) == Some(b"RIFF") {
        avi_frames(reader)
    } else if head.get(..4) == Some(&[0x1A, 0x45, 0xDF, 0xA3]) {
        matroska_frames(reader)?
    } else if head.get(..4) == Some(b"OggS") {
        ogg_frames(reader).map(|(format, setup, frames, video)| {
            described = Some(video);
            (format, setup, frames)
        })
    } else {
        None
    };
    let Some((format, setup, frames)) = found else {
        return Ok(general);
    };
    // The media reader's view of the video, else (an Ogg file's Theora) the stream's own.
    let Some(video) = general.info().video.clone().or_else(|| described.clone()) else {
        return Ok(general);
    };
    if frames.count() == 0 {
        return Err(TranscodeError::Decode(format!(
            "{}: no {} frames",
            general.info().name,
            format.name()
        )));
    }
    let mut info = general.info().clone();
    if info.video.is_none() {
        info.video = Some(video.clone());
        info.kind = MediaKind::Movie;
        let frames_long = video
            .frame_rate
            .tick_of(i64::try_from(frames.count()).unwrap_or(i64::MAX));
        info.duration = info.duration.max(frames_long);
    }
    if let Some(shown) = info.video.as_mut() {
        format.name().clone_into(&mut shown.codec);
    }
    Ok(Arc::new(OwnVideo {
        info,
        general,
        frames,
        format,
        setup,
        size: (video.width, video.height),
        rate: video.frame_rate,
        state: Mutex::new(State {
            decoder: None,
            next: 0,
            shown: None,
            last: None,
        }),
    }))
}

/// A file the media reader cannot open whose video DiscCraft decodes itself (M6.567: an Ogg file of Theora video and
/// no sound): its video alone, as a source with no sound. `None` for any other file.
///
/// # Errors
///
/// When it is such a file but its frames cannot be found.
pub(crate) fn alone(
    reader: &SharedReader,
    head: &[u8],
    name: &str,
) -> Result<Option<Arc<dyn MediaSource>>, TranscodeError> {
    if head.get(..4) != Some(b"OggS") {
        return Ok(None);
    }
    let silent = Arc::new(Silent(MediaInfo {
        name: name.to_owned(),
        kind: MediaKind::Movie,
        duration: Tick(0),
        video: None,
        audio_streams: Vec::new(),
        container: "Ogg".to_owned(),
        start_timecode: None,
        file_size: Some(reader.len()),
    }));
    let source = with_own_video(reader, head, silent)?;
    Ok(source.info().video.is_some().then_some(source))
}

/// A file with nothing the media reader shows: no pictures, no sound.
struct Silent(MediaInfo);

impl MediaSource for Silent {
    fn info(&self) -> &MediaInfo {
        &self.0
    }

    fn video_frame(&self, _req: FrameRequest) -> Result<Arc<VideoFrame>, MediaError> {
        Err(MediaError::NoStream("video"))
    }

    fn audio(
        &self,
        _start: i64,
        _frames: usize,
        _sample_rate: u32,
    ) -> Result<AudioBuffer, MediaError> {
        Err(MediaError::NoStream("audio"))
    }

    fn audio_stream(
        &self,
        _stream: usize,
        _start: i64,
        _frames: usize,
        _sample_rate: u32,
    ) -> Result<AudioBuffer, MediaError> {
        Err(MediaError::NoStream("audio"))
    }
}

/// The Theora stream of an Ogg file: the format, its headers (Xiph-laced, as a Matroska track keeps them), its data
/// packets and the video stream its headers describe. `None` for a file with no Theora stream DiscCraft can read.
fn ogg_frames(reader: &SharedReader) -> Option<(Format, Vec<u8>, Frames, VideoStreamInfo)> {
    let source = Shared(reader.clone());
    let file = craft_media::ogg::open(&source).ok()?;
    let stream = file
        .streams
        .into_iter()
        .find(|stream| stream.codec == craft_media::ogg::Codec::Theora)?;
    let video = crate::own_video::theora_stream(&stream.headers)?;
    let setup = crate::own_video::xiph_lace(&stream.headers);
    let packets = stream
        .packets
        .into_iter()
        .map(|packet| packet.parts)
        .collect();
    Some((
        Format::Theora,
        setup,
        Frames::Ogg { source, packets },
        video,
    ))
}

/// The format, setup data and chunks of an AVI's first video stream of a format DiscCraft decodes itself.
fn avi_frames(reader: &SharedReader) -> Option<(Format, Vec<u8>, Frames)> {
    let avi = crate::avi::read_avi(reader)?;
    let (number, stream, format) =
        avi.streams
            .iter()
            .enumerate()
            .find_map(|(number, stream)| {
                let format = Format::of_code(&stream.compression)
                    .or_else(|| Format::of_code(&stream.handler));
                (&stream.kind == b"vids").then_some(())?;
                Some((number, *stream, format?))
            })?;
    let (at, size) = stream.format;
    let bytes = read(reader, at, size.min(MOST_SETUP) as usize).unwrap_or_default();
    let setup = after_bitmap_header(&bytes);
    let chunks = avi.chunks(reader, number);
    Some((
        format,
        setup,
        Frames::Avi {
            reader: reader.clone(),
            chunks,
        },
    ))
}

/// The format, setup data and blocks of a Matroska file's video track when its format is one DiscCraft decodes
/// itself (`None` for another file).
fn matroska_frames(
    reader: &SharedReader,
) -> Result<Option<(Format, Vec<u8>, Frames)>, TranscodeError> {
    let source = Shared(reader.clone());
    // The track's codec first, from the header alone: a file of another format is not indexed here.
    let quick = OpenOptions {
        index: false,
        verify_crc: false,
        index_limit: None,
    };
    let Ok(header) = open_with(source.clone(), &quick) else {
        return Ok(None);
    };
    let Some(track) = header.track_of_kind(TrackKind::Video) else {
        return Ok(None);
    };
    let Some(format) = header.tracks.get(track).and_then(|track| {
        let compression =
            (track.codec_id == "V_MS/VFW/FOURCC").then(|| code_at(&track.codec_private, 16));
        Format::of_matroska(&track.codec_id, compression)
    }) else {
        return Ok(None);
    };
    let full = OpenOptions {
        index: true,
        verify_crc: false,
        index_limit: None,
    };
    let file = open_with(source.clone(), &full)
        .map_err(|error| TranscodeError::Decode(error.to_string()))?;
    let Some(entry) = file.tracks.get(track) else {
        return Ok(None);
    };
    let private = &entry.codec_private;
    let setup = if entry.codec_id == "V_MS/VFW/FOURCC" {
        after_bitmap_header(private)
    } else {
        private.clone()
    };
    let (num, den) = entry.timebase;
    let unit = if den == 0 {
        0.0
    } else {
        num as f64 / den as f64
    };
    let times = entry
        .samples
        .iter()
        .map(|sample| sample.pts as f64 * unit)
        .collect();
    Ok(Some((
        format,
        setup,
        Frames::Matroska {
            source,
            file: Box::new(file),
            track,
            times,
        },
    )))
}

/// The codec's own data after a BITMAPINFOHEADER (`format`: an AVI stream's `strf`, a Matroska Video for Windows
/// track's private data). The header's size field counts the codec data too when a writer puts it there (ffmpeg's
/// AVI writer does; RFC 9043 §4.3.3.1 takes the data after the 40-byte header), and a chunk may end in a padding
/// byte: the data is from byte 40 to that size; after the larger BITMAPV4HEADER / BITMAPV5HEADER (108 / 124 bytes),
/// everything after them.
fn after_bitmap_header(format: &[u8]) -> Vec<u8> {
    let size = (le32(format, 0) as usize).min(format.len());
    let (start, end) = match size {
        108 | 124 => (size, format.len()),
        _ if size > 40 => (40, size),
        _ => (40, format.len()),
    };
    format
        .get(start.min(end)..end)
        .map(<[u8]>::to_vec)
        .unwrap_or_default()
}

/// The file's reader as craft-media's readers take one.
#[derive(Clone)]
struct Shared(SharedReader);

impl craft_media::ByteSource for Shared {
    fn len(&self) -> u64 {
        self.0.len()
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        self.0.read_at(offset, buf)
    }
}

impl OwnVideo {
    /// The picture of frame `index`. A frame that stands alone is decoded on its own (an empty one: the last frame
    /// before it with a picture); otherwise the frames are decoded in order, from where the decoder is when it is on
    /// the way, else from the last key frame at or before `index` (an empty frame keeps the picture before it).
    fn picture(&self, index: usize) -> Result<Arc<VideoFrame>, String> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((at, frame)) = &state.last
            && *at == index
        {
            return Ok(frame.clone());
        }
        let pts = self.rate.tick_of(i64::try_from(index).unwrap_or(i64::MAX));
        let mut picture = if self.format.intra_only() {
            let mut at = index;
            let coded = loop {
                let coded = self.frames.read(at)?;
                if !coded.is_empty() {
                    break coded;
                }
                at = at.checked_sub(1).ok_or("the first frame is empty")?;
            };
            decoder(self.format, &self.setup, self.size)?.decode(&coded, pts)?
        } else {
            let on_the_way = state.decoder.is_some() && state.next > 0 && state.next <= index;
            let from = if on_the_way {
                state.next
            } else {
                state.decoder = Some(decoder(self.format, &self.setup, self.size)?);
                state.shown = None;
                self.frames.key_at_or_before(index, self.format)?
            };
            for at in from..=index {
                let coded = self.frames.read(at)?;
                state.next = at + 1;
                if coded.is_empty() {
                    continue;
                }
                let decoder = state.decoder.as_mut().ok_or("no decoder")?;
                let decoded = decoder.decode(&coded, pts)?;
                state.shown = Some(decoded);
            }
            state.shown.clone().ok_or("the first frame is empty")?
        };
        picture.pts = pts;
        let frame = Arc::new(picture);
        state.last = Some((index, frame.clone()));
        Ok(frame)
    }
}

impl MediaSource for OwnVideo {
    fn info(&self) -> &MediaInfo {
        &self.info
    }

    fn video_frame(&self, req: FrameRequest) -> Result<Arc<VideoFrame>, MediaError> {
        let index = self.frames.index_at(req.time, self.rate);
        self.picture(index)
            .map_err(|error| MediaError::Decode(format!("{}: {error}", self.info.name)))
    }

    fn audio(
        &self,
        start: i64,
        frames: usize,
        sample_rate: u32,
    ) -> Result<AudioBuffer, MediaError> {
        self.general.audio(start, frames, sample_rate)
    }

    fn audio_stream(
        &self,
        stream: usize,
        start: i64,
        frames: usize,
        sample_rate: u32,
    ) -> Result<AudioBuffer, MediaError> {
        self.general
            .audio_stream(stream, start, frames, sample_rate)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;
    use filmcraft_color::ColorInfo;
    use filmcraft_media::reader::ByteReader;
    use filmcraft_media::{MediaKind, VideoStreamInfo};

    /// Bytes in memory as a reader.
    struct Bytes(Vec<u8>);

    impl ByteReader for Bytes {
        fn len(&self) -> u64 {
            self.0.len() as u64
        }

        fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
            let start = usize::try_from(offset).unwrap();
            let source = self
                .0
                .get(start..start + buf.len())
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "past the end"))?;
            buf.copy_from_slice(source);
            Ok(())
        }
    }

    /// The media reader's side: the file's length and frame rate, no pictures.
    struct General(MediaInfo);

    impl MediaSource for General {
        fn info(&self) -> &MediaInfo {
            &self.0
        }

        fn video_frame(&self, _req: FrameRequest) -> Result<Arc<VideoFrame>, MediaError> {
            Err(MediaError::Decode("no decoder".to_string()))
        }

        fn audio(
            &self,
            _start: i64,
            _frames: usize,
            _rate: u32,
        ) -> Result<AudioBuffer, MediaError> {
            Err(MediaError::NoStream("audio"))
        }

        fn audio_stream(
            &self,
            _stream: usize,
            _start: i64,
            _frames: usize,
            _rate: u32,
        ) -> Result<AudioBuffer, MediaError> {
            Err(MediaError::NoStream("audio"))
        }
    }

    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend_from_slice(&u32::try_from(body.len()).unwrap().to_le_bytes());
        out.extend_from_slice(body);
        if body.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    fn list(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        chunk(b"LIST", &[kind.as_slice(), body].concat())
    }

    /// An AVI of one `MPNG` video stream whose chunks are `frames`.
    fn avi(frames: &[Vec<u8>]) -> Vec<u8> {
        let mut strh = b"vidsMPNG".to_vec();
        strh.extend_from_slice(&[0; 12]);
        strh.extend_from_slice(&1u32.to_le_bytes());
        strh.extend_from_slice(&25u32.to_le_bytes());
        strh.resize(56, 0);
        let mut strf = 40u32.to_le_bytes().to_vec();
        strf.extend_from_slice(&2i32.to_le_bytes());
        strf.extend_from_slice(&1i32.to_le_bytes());
        strf.extend_from_slice(&[1, 0, 24, 0]);
        strf.extend_from_slice(b"MPNG");
        strf.resize(40, 0);
        let strl = list(
            b"strl",
            &[chunk(b"strh", &strh), chunk(b"strf", &strf)].concat(),
        );
        let hdrl = list(b"hdrl", &strl);
        let movi: Vec<u8> = frames
            .iter()
            .flat_map(|frame| chunk(b"00dc", frame))
            .collect();
        let movi = list(b"movi", &movi);
        chunk(b"RIFF", &[b"AVI ".as_slice(), &hdrl, &movi].concat())
    }

    fn png(rgb: &[u8; 6]) -> Vec<u8> {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 2, 1);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(rgb).unwrap();
        }
        encoded
    }

    fn rgba(frame: &VideoFrame) -> Vec<u8> {
        match &frame.data {
            filmcraft_frame::PixelData::Rgba8(pixels) => pixels.to_vec(),
            _ => panic!("not RGBA"),
        }
    }

    /// An AVI whose video DiscCraft decodes itself: each chunk's picture at its frame's time, and an empty chunk (a
    /// long run of them too) repeats the picture before it, without a decoder for the media reader's side.
    #[test]
    fn an_avi_of_png_frames_shows_each_chunk_and_repeats_over_empty_ones() {
        let mut frames = vec![
            png(&[255, 0, 0, 0, 255, 0]),
            png(&[0, 0, 255, 255, 255, 255]),
        ];
        frames.extend(std::iter::repeat_n(Vec::new(), 20_000));
        let bytes = avi(&frames);
        let reader: SharedReader = Arc::new(Bytes(bytes.clone()));
        let rate = FrameRate { num: 25, den: 1 };
        let general = Arc::new(General(MediaInfo {
            name: "test.avi".to_owned(),
            kind: MediaKind::Movie,
            duration: rate.tick_of(20_002),
            video: Some(VideoStreamInfo {
                width: 2,
                height: 1,
                frame_rate: rate,
                par: (1, 1),
                codec: "MPNG".to_owned(),
                pixel_format: "rgb24".to_owned(),
                color: ColorInfo::SRGB_FULL,
                has_alpha: false,
                bitrate: None,
                hdr: None,
                color_assumed: None,
            }),
            audio_streams: Vec::new(),
            container: "AVI".to_owned(),
            start_timecode: None,
            file_size: Some(bytes.len() as u64),
        }));
        let source = with_own_video(&reader, &bytes[..64], general).unwrap();
        assert_eq!(source.info().video.as_ref().unwrap().codec, "PNG");
        let first = source
            .video_frame(FrameRequest::full(rate.tick_of(0)))
            .unwrap();
        assert_eq!(rgba(&first), vec![255, 0, 0, 255, 0, 255, 0, 255]);
        let second = source
            .video_frame(FrameRequest::full(rate.tick_of(1)))
            .unwrap();
        let blue_white = vec![0, 0, 255, 255, 255, 255, 255, 255];
        assert_eq!(rgba(&second), blue_white);
        // The last of 20 000 empty chunks after it: still the second picture.
        let last = source
            .video_frame(FrameRequest::full(rate.tick_of(20_001)))
            .unwrap();
        assert_eq!(rgba(&last), blue_white);
        assert_eq!(last.pts, rate.tick_of(20_001));
    }
}
