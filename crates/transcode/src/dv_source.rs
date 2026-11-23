//! DV video (M6.410): a raw DV file (`.dv`, `.dif`: one DV frame after another) or an AVI whose video is DV (type 1:
//! the DV frames as one interleaved stream with their sound in them; type 2: a DV video stream, and the sound as a
//! stream of its own), decoded with disccraft-dv. The media reader lists DV in an AVI but has no decoder for it, so
//! this source finds the frames itself: the AVI's RIFF chunks (Microsoft's AVI RIFF file reference, and the OpenDML
//! AVI file format extensions for the `AVIX` parts of files over 1 GB). The sound of a type 2 AVI comes from the media
//! reader; the sound of the others from the DV frames (16-bit at 48, 44.1 or 32 kHz, or a camcorder's 12-bit 32 kHz:
//! its sound 1).

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use disccraft_dv::{Header, Layout, System};
use filmcraft_color::{ColorInfo, Matrix, Primaries, Range, Transfer};
use filmcraft_frame::{AudioBuffer, Chroma, PixelData, VideoFrame};
use filmcraft_media::reader::SharedReader;
use filmcraft_media::{
    AudioStreamInfo, FrameRequest, MediaError, MediaInfo, MediaKind, MediaSource, VideoStreamInfo,
};
use filmcraft_time::FrameRate;

use crate::TranscodeError;
use crate::avi::{Stream, read};

/// Where a DV file's frames are.
#[derive(Debug, Clone)]
enum Frames {
    /// One frame after another from the start of the file.
    Raw { count: usize },
    /// An AVI's chunks: offset and length of each frame (a frame repeated has the chunk of the one before).
    Chunks(Vec<(u64, u32)>),
}

/// Where a DV file's sound comes from.
enum Sound {
    /// The AVI's sound stream, through the media reader.
    Stream(Arc<dyn MediaSource>),
    /// The DV frames themselves.
    Frames,
    None,
}

/// A DV file as a media source.
pub(crate) struct DvSource {
    info: MediaInfo,
    reader: SharedReader,
    header: Header,
    frames: Frames,
    sound: Sound,
    /// The sample rate of the sound in the frames.
    sound_rate: u32,
    rate: FrameRate,
    /// The last picture decoded and its frame.
    last: Mutex<Option<(usize, Arc<VideoFrame>)>>,
}

/// Whether `head` (the start of a file) is DV: a raw DV file, an AVI with a DV video stream among the stream
/// headers it holds, or a Matroska file whose track header there names DV (M6.416).
pub(crate) fn is_dv(head: &[u8]) -> bool {
    if disccraft_dv::sniff(head) || is_matroska_dv(head) {
        return true;
    }
    crate::avi::head_has_stream(head, is_dv_stream)
}

/// Whether `bytes` (the start or the end of a file) describe DV video in another container (M6.413): a QuickTime
/// sample description (`stsd`) whose first entry is DV, or a Matroska track of DV by its VFW codec.
pub(crate) fn names_dv(bytes: &[u8]) -> bool {
    let found = |tag: &[u8], within: std::ops::Range<usize>| {
        bytes
            .windows(tag.len())
            .enumerate()
            .filter(|(_, window)| *window == tag)
            .any(|(at, _)| {
                within.clone().any(|offset| {
                    bytes
                        .get(at + offset..at + offset + 4)
                        .is_some_and(dv_handler)
                })
            })
    };
    // stsd: version and flags, entry count, the first entry's size, then its format.
    found(b"stsd", 16..17)
        // V_MS/VFW/FOURCC, then the codec private data: a BITMAPINFOHEADER whose compression is 16 bytes in.
        || found(b"V_MS/VFW/FOURCC", 15..120)
}

/// Whether a four-character code names 25 Mb/s DV (consumer DV, DVCAM, DVCPRO): an AVI's (`dvsd`, …), a Matroska
/// file's VFW codec (`dvsd`), or a QuickTime sample entry's (`dvc ` NTSC, `dvcp` PAL, `dvpp` DVCPRO PAL).
pub(crate) fn dv_handler(code: &[u8]) -> bool {
    let mut upper = [0u8; 4];
    for (to, from) in upper.iter_mut().zip(code) {
        *to = from.to_ascii_uppercase();
    }
    // DVCPRO50 (M6.415): `dv50` in an AVI, `dv5n` / `dv5p` in a QuickTime movie.
    matches!(
        &upper,
        b"DVSD" | b"DV25" | b"CDVC" | b"DVCP" | b"DVPP" | b"DVC " | b"DV50" | b"DV5N" | b"DV5P"
    )
}

/// The first frame of a raw DV file: its first DIF sequence says how long a frame is (twice as long at 50 Mb/s).
fn first_raw_frame(reader: &SharedReader) -> Option<Vec<u8>> {
    let start = read(reader, 0, disccraft_dv::SEQUENCE)?;
    let header = disccraft_dv::header(&start).ok()?;
    read(reader, 0, header.frame_bytes())
}

/// Whether `head` starts a Matroska file (its EBML header) whose track header names DV by its VFW codec.
fn is_matroska_dv(head: &[u8]) -> bool {
    head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) && names_dv(head)
}

/// The frames of the DV track of the Matroska file `path` (M6.416): the media reader's Matroska reader turns DV
/// down before it asks for a decoder, so the frames are found here with craft-media's Matroska demuxer (its index of
/// each track's frames: where each is and how long), and read as an AVI's are. `None` without such a track, or when
/// its frames are compressed or encrypted.
fn matroska_frames(path: &Path) -> Option<Vec<(u64, u32)>> {
    use craft_media::matroska::{Codec, Demuxer, OpenOptions};
    let source = craft_media::ReadSeekSource::new(std::fs::File::open(path).ok()?).ok()?;
    let options = OpenOptions {
        index: true,
        verify_crc: false,
        index_limit: None,
    };
    let demuxer = Demuxer::with_options(source, options).ok()?;
    let track = demuxer.tracks().iter().find(|track| {
        matches!(&track.codec, Codec::VfwFourcc { fourcc, .. } if dv_handler(fourcc))
            && track.encodings.is_empty()
    })?;
    let frames: Vec<(u64, u32)> = track
        .samples
        .iter()
        .take(crate::avi::MOST_CHUNKS)
        .map(|sample| (sample.offset, sample.size))
        .collect();
    (!frames.is_empty()).then_some(frames)
}

/// Opens the DV file at `path` (read through `reader`, its first bytes `head`): `None` when it is not DV.
///
/// # Errors
/// When it is DV but its frames cannot be read.
pub(crate) fn open(
    name: &str,
    path: &Path,
    reader: &SharedReader,
    head: &[u8],
) -> Result<Option<DvSource>, TranscodeError> {
    if !is_dv(head) {
        return Ok(None);
    }
    let decode = |error: disccraft_dv::DvError| TranscodeError::Decode(format!("{name}: {error}"));
    let (frames, sound, container) = if disccraft_dv::sniff(head) {
        let first = first_raw_frame(reader)
            .ok_or_else(|| TranscodeError::Decode(format!("{name}: shorter than a DV frame")))?;
        let header = disccraft_dv::header(&first).map_err(decode)?;
        let count = usize::try_from(reader.len() / header.frame_bytes() as u64).unwrap_or(0);
        (Frames::Raw { count }, Sound::Frames, "DV")
    } else if is_matroska_dv(head) {
        let Some(frames) = matroska_frames(path) else {
            return Ok(None);
        };
        // The sound: the file's own audio track, as the media reader reads it.
        let sound = match filmcraft_codecs::MkvSource::open_reader(name, reader.clone()) {
            Ok(source) if !source.info().audio_streams.is_empty() => {
                Sound::Stream(Arc::new(source))
            }
            _ => Sound::Frames,
        };
        (Frames::Chunks(frames), sound, "Matroska")
    } else {
        let Some(avi) = avi_frames(reader) else {
            return Ok(None);
        };
        let first = avi
            .chunks
            .iter()
            .find(|&&(_, size)| size > 0)
            .and_then(|&(at, size)| read(reader, at, size as usize))
            .ok_or_else(|| {
                TranscodeError::Decode(format!("{name}: its first DV frame cannot be read"))
            })?;
        let header = disccraft_dv::header(&first).map_err(decode)?;
        let frames = frames_of_chunks(&avi.chunks, avi.time_base, header.system.frame_rate());
        let sound = if avi.sound_stream {
            match filmcraft_codecs::AviSource::open_reader(name, reader.clone()) {
                Ok(source) if !source.info().audio_streams.is_empty() => {
                    Sound::Stream(Arc::new(source))
                }
                _ => Sound::Frames,
            }
        } else {
            Sound::Frames
        };
        (Frames::Chunks(frames), sound, "AVI")
    };
    let source = DvSource::new(name, reader.clone(), frames, sound, container)?;
    Ok(Some(source))
}

/// What an AVI holds of DV.
struct AviFrames {
    /// Every chunk of the DV stream in order: offset and length (0 for an empty chunk).
    chunks: Vec<(u64, u32)>,
    /// How long a chunk lasts: `scale / rate` seconds (the stream header's; 0 when it does not say).
    time_base: (u32, u32),
    /// It has a stream of sound (`auds`): a type 2 AVI.
    sound_stream: bool,
}

/// Whether an AVI stream is DV: an `iavs` stream (type 1), or a `vids` stream of a DV codec (type 2).
fn is_dv_stream(stream: &Stream) -> bool {
    &stream.kind == b"iavs"
        || (&stream.kind == b"vids"
            && (dv_handler(&stream.handler) || dv_handler(&stream.compression)))
}

/// The chunks of an AVI's first DV stream: `None` when it has none with a frame.
fn avi_frames(reader: &SharedReader) -> Option<AviFrames> {
    let avi = crate::avi::read_avi(reader)?;
    let number = avi.streams.iter().position(is_dv_stream)?;
    let stream = avi.streams.get(number)?;
    let chunks = avi.chunks(reader, number);
    chunks.iter().any(|&(_, size)| size > 0).then(|| AviFrames {
        chunks,
        time_base: (stream.scale, stream.rate),
        sound_stream: avi.has(b"auds"),
    })
}

/// The DV frames (offset and length) of an AVI's chunks `chunks`, which last `scale / rate` seconds each, at the DV
/// frame rate `num / den`. When a chunk lasts a frame (within 1 %), or the header does not say, a chunk is a frame
/// and an empty chunk repeats the frame before it. Otherwise each chunk with a frame is put at the frame of its time,
/// and the frames between repeat the one before: ffmpeg copying a raw DV stream into an AVI writes 600 chunks a
/// second, one in 20 with a frame (Encore turns such a file down; M6.419).
fn frames_of_chunks(
    chunks: &[(u64, u32)],
    (scale, rate): (u32, u32),
    (num, den): (u32, u32),
) -> Vec<(u64, u32)> {
    // Frames a chunk: scale * num / (rate * den).
    let (over, under) = (
        u128::from(scale) * u128::from(num),
        u128::from(rate) * u128::from(den),
    );
    let one_each = over == 0 || under == 0 || over.abs_diff(under) * 100 <= under;
    let frame_of = |index: usize| -> usize {
        let frame = if one_each {
            index
        } else {
            // The nearest frame (a chunk halfway between two goes to the earlier).
            let index = index as u128;
            usize::try_from((2 * index * over + under - 1) / (2 * under)).unwrap_or(usize::MAX)
        };
        frame.min(crate::avi::MOST_CHUNKS)
    };
    let mut frames: Vec<(u64, u32)> = Vec::new();
    for (index, &(at, size)) in chunks.iter().enumerate() {
        if size == 0 {
            continue;
        }
        let frame = frame_of(index);
        let fill = frames.last().copied().unwrap_or((at, size));
        while frames.len() < frame {
            frames.push(fill);
        }
        // Two chunks in one frame: the first is kept.
        if frames.len() == frame {
            frames.push((at, size));
        }
    }
    // Empty chunks at the end repeat the last frame.
    if let Some(&last) = frames.last() {
        let total = frame_of(chunks.len());
        while frames.len() < total {
            frames.push(last);
        }
    }
    frames
}

impl DvSource {
    fn new(
        name: &str,
        reader: SharedReader,
        frames: Frames,
        sound: Sound,
        container: &str,
    ) -> Result<Self, TranscodeError> {
        let decode =
            |error: disccraft_dv::DvError| TranscodeError::Decode(format!("{name}: {error}"));
        let first = match &frames {
            Frames::Raw { .. } => first_raw_frame(&reader),
            Frames::Chunks(chunks) => chunks
                .first()
                .and_then(|&(at, size)| read(&reader, at, size as usize)),
        }
        .ok_or_else(|| {
            TranscodeError::Decode(format!("{name}: its first DV frame cannot be read"))
        })?;
        let header = disccraft_dv::header(&first).map_err(decode)?;
        let (num, den) = header.system.frame_rate();
        let rate = FrameRate {
            num: i64::from(num),
            den: i64::from(den),
        };
        let count = match &frames {
            Frames::Raw { count } => *count,
            Frames::Chunks(chunks) => chunks.len(),
        };
        let in_frames = match sound {
            Sound::Frames => disccraft_dv::decode_audio(&first).ok().flatten(),
            _ => None,
        };
        let sound = match (sound, &in_frames) {
            (Sound::Frames, None) => Sound::None,
            (other, _) => other,
        };
        let sound_rate = in_frames.as_ref().map_or(48_000, |audio| audio.sample_rate);
        // A second stereo pair in the frames (M6.418: a camcorder's 12-bit sound 2, DVCPRO50's CH3 / CH4) is a
        // second sound stream.
        let second = match (&sound, &in_frames) {
            (Sound::Frames, Some(_)) => disccraft_dv::decode_audio_pair(&first, 1).ok().flatten(),
            _ => None,
        };
        let stream_info = |audio: &disccraft_dv::Audio| AudioStreamInfo {
            sample_rate: audio.sample_rate,
            channels: 2,
            codec: "PCM (DV)".to_owned(),
            bits_per_sample: Some(audio.bits),
        };
        let audio_streams = match (&sound, &in_frames) {
            (Sound::Stream(source), _) => source.info().audio_streams.clone(),
            (Sound::Frames, Some(audio)) => std::iter::once(stream_info(audio))
                .chain(second.as_ref().map(stream_info))
                .collect(),
            _ => Vec::new(),
        };
        let height = header.system.height();
        let info = MediaInfo {
            name: name.to_owned(),
            kind: MediaKind::Movie,
            duration: rate.tick_of(i64::try_from(count).unwrap_or(i64::MAX)),
            video: Some(VideoStreamInfo {
                width: 720,
                height: height as u32,
                frame_rate: rate,
                par: pixel_aspect(&header),
                codec: "DV".to_owned(),
                pixel_format: match header.sampling {
                    disccraft_dv::Sampling::C411 => "yuv411p".to_owned(),
                    disccraft_dv::Sampling::C420 => "yuv420p".to_owned(),
                    disccraft_dv::Sampling::C422 => "yuv422p".to_owned(),
                },
                color: color(&header),
                has_alpha: false,
                bitrate: Some(header.frame_bytes() as u64 * 8 * u64::from(num) / u64::from(den)),
                hdr: None,
                color_assumed: None,
            }),
            audio_streams,
            container: container.to_owned(),
            start_timecode: None,
            file_size: Some(reader.len()),
        };
        Ok(DvSource {
            info,
            reader,
            header,
            frames,
            sound,
            sound_rate,
            rate,
            last: Mutex::new(None),
        })
    }

    fn count(&self) -> usize {
        match &self.frames {
            Frames::Raw { count } => *count,
            Frames::Chunks(chunks) => chunks.len(),
        }
    }

    /// The bytes of frame `index`.
    fn frame(&self, index: usize) -> Option<Vec<u8>> {
        let length = self.header.frame_bytes();
        match &self.frames {
            Frames::Raw { count } => {
                (index < *count).then(|| read(&self.reader, (index * length) as u64, length))?
            }
            Frames::Chunks(chunks) => {
                let &(at, size) = chunks.get(index)?;
                read(&self.reader, at, (size as usize).min(length))
            }
        }
    }

    /// The picture of frame `index`: black when the frame cannot be read or decoded.
    fn picture(&self, index: usize) -> VideoFrame {
        let pts = self.rate.tick_of(i64::try_from(index).unwrap_or(i64::MAX));
        let picture = self
            .frame(index)
            .and_then(|bytes| disccraft_dv::decode_video(&bytes).ok());
        video_frame(picture, &self.header, pts)
    }

    /// The first sample of frame `index` (at 48 kHz a 525/60 frame holds 1 601.6 samples on average).
    fn first_sample(&self, index: i64) -> i64 {
        let (num, den) = (i128::from(self.rate.num), i128::from(self.rate.den));
        if num <= 0 {
            return 0;
        }
        i64::try_from(i128::from(index) * i128::from(self.sound_rate) * den / num)
            .unwrap_or(i64::MAX)
    }

    /// The frame that holds sample `sample`.
    fn frame_of_sample(&self, sample: i64) -> i64 {
        let (num, den) = (i128::from(self.rate.num), i128::from(self.rate.den));
        let per = i128::from(self.sound_rate) * den;
        if per <= 0 {
            return 0;
        }
        i64::try_from(i128::from(sample) * num / per).unwrap_or(i64::MAX)
    }

    /// The sound in the frames: samples `first` to `last` (inclusive) of both channels, as -1 to 1.
    fn frame_samples(&self, (first, last): (i64, i64), pair: usize) -> [Vec<f32>; 2] {
        let length = usize::try_from(last.saturating_sub(first).saturating_add(1)).unwrap_or(0);
        let mut channels = [vec![0.0f32; length], vec![0.0f32; length]];
        let count = i64::try_from(self.count()).unwrap_or(i64::MAX);
        let from = self.frame_of_sample(first.max(0)).max(0);
        let to = self
            .frame_of_sample(last.max(0))
            .min(count.saturating_sub(1));
        for index in from..=to {
            let Some(audio) = usize::try_from(index)
                .ok()
                .and_then(|i| self.frame(i))
                .and_then(|bytes| disccraft_dv::decode_audio_pair(&bytes, pair).ok().flatten())
            else {
                continue;
            };
            let (start, end) = (self.first_sample(index), self.first_sample(index + 1));
            for (channel, samples) in channels.iter_mut().zip(&audio.channels) {
                let mut held = 0i16;
                for sample in start..end {
                    // A frame with fewer samples than its share holds its last one; extra ones are dropped.
                    let offset = usize::try_from(sample - start).unwrap_or(usize::MAX);
                    if let Some(&value) = samples.get(offset) {
                        held = value;
                    }
                    if let Ok(at) = usize::try_from(sample - first)
                        && let Some(slot) = channel.get_mut(at)
                    {
                        *slot = f32::from(held) / 32_768.0;
                    }
                }
            }
        }
        channels
    }

    /// `frames` samples of the frames' sound at `sample_rate` from sample `start`, by linear interpolation from the
    /// frames' own rate.
    fn frames_audio(
        &self,
        start: i64,
        frames: usize,
        (sample_rate, pair): (u32, usize),
    ) -> AudioBuffer {
        if frames == 0 || sample_rate == 0 {
            return AudioBuffer::silence(sample_rate.max(1), 2, frames);
        }
        let own = self.sound_rate;
        let to_own = |sample: i64| i128::from(sample) * i128::from(own) / i128::from(sample_rate);
        let end = start.saturating_add(i64::try_from(frames).unwrap_or(i64::MAX));
        let first = i64::try_from(to_own(start)).unwrap_or(0);
        let last = i64::try_from(to_own(end)).unwrap_or(0).saturating_add(1);
        let source = self.frame_samples((first, last), pair);
        if sample_rate == own {
            return AudioBuffer {
                sample_rate,
                channels: source
                    .map(|mut samples| {
                        samples.truncate(frames);
                        samples
                    })
                    .to_vec(),
            };
        }
        let step = f64::from(own) / f64::from(sample_rate);
        let channels = source
            .iter()
            .map(|samples| {
                (0..frames)
                    .map(|i| {
                        let position = (start as f64 + i as f64) * step - first as f64;
                        let below = position.floor().max(0.0);
                        let weight = (position - below) as f32;
                        let at = below as usize;
                        let a = samples.get(at).copied().unwrap_or(0.0);
                        let b = samples.get(at + 1).copied().unwrap_or(a);
                        a + (b - a) * weight
                    })
                    .collect()
            })
            .collect();
        AudioBuffer {
            sample_rate,
            channels,
        }
    }
}

/// `picture` (of a frame whose header is `header`) as a media frame: black when there is none.
pub(crate) fn video_frame(
    picture: Option<disccraft_dv::Picture>,
    header: &Header,
    pts: filmcraft_time::Tick,
) -> VideoFrame {
    let height = header.system.height();
    let (data, chroma) = match picture {
        Some(picture) => {
            let chroma = match picture.layout {
                Layout::Yuv422 => Chroma::C422,
                Layout::Yuv420 => Chroma::C420,
            };
            ([picture.y, picture.cb, picture.cr], chroma)
        }
        None => {
            let (chroma, chroma_height) = match header.sampling {
                disccraft_dv::Sampling::C411 | disccraft_dv::Sampling::C422 => {
                    (Chroma::C422, height)
                }
                disccraft_dv::Sampling::C420 => (Chroma::C420, height / 2),
            };
            (
                [
                    vec![16; 720 * height],
                    vec![128; 360 * chroma_height],
                    vec![128; 360 * chroma_height],
                ],
                chroma,
            )
        }
    };
    let [y, cb, cr] = data;
    VideoFrame {
        width: 720,
        height: height as u32,
        data: PixelData::Yuv8 {
            planes: [Arc::new(y), Arc::new(cb), Arc::new(cr)],
            chroma,
            alpha: None,
        },
        color: color(header),
        par: pixel_aspect(header),
        pts,
    }
}

/// The pixel aspect ratio that makes the whole 720 pixels 4:3 or 16:9, as an MPEG-2 sequence header's display
/// aspect ratio does: what the DVD transcode maps onto its 720-pixel frame as it is.
fn pixel_aspect(header: &Header) -> (u32, u32) {
    match (header.system, header.wide) {
        (System::Lines525, false) => (8, 9),
        (System::Lines525, true) => (32, 27),
        (System::Lines625, false) => (16, 15),
        (System::Lines625, true) => (64, 45),
    }
}

/// ITU-R BT.601 colour, the primaries of the frame's system.
fn color(header: &Header) -> ColorInfo {
    ColorInfo {
        matrix: Matrix::Bt601,
        transfer: Transfer::Bt709,
        primaries: match header.system {
            System::Lines525 => Primaries::Bt601_525,
            System::Lines625 => Primaries::Bt601_625,
        },
        range: Range::Limited,
    }
}

impl MediaSource for DvSource {
    fn info(&self) -> &MediaInfo {
        &self.info
    }

    fn video_frame(&self, req: FrameRequest) -> Result<Arc<VideoFrame>, MediaError> {
        let count = self.count();
        if count == 0 {
            return Err(MediaError::NoStream("video"));
        }
        let index = usize::try_from(self.rate.frame_at(req.time).max(0))
            .unwrap_or(0)
            .min(count - 1);
        if let Some((at, frame)) = self
            .last
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            && *at == index
        {
            return Ok(frame.clone());
        }
        // Decoded without the lock held: the transcode asks for several frames at once, on threads of their own.
        let frame = Arc::new(self.picture(index));
        *self.last.lock().unwrap_or_else(PoisonError::into_inner) = Some((index, frame.clone()));
        Ok(frame)
    }

    fn audio(
        &self,
        start: i64,
        frames: usize,
        sample_rate: u32,
    ) -> Result<AudioBuffer, MediaError> {
        match &self.sound {
            Sound::Stream(source) => source.audio(start, frames, sample_rate),
            Sound::Frames => Ok(self.frames_audio(start, frames, (sample_rate, 0))),
            Sound::None => Err(MediaError::NoStream("audio")),
        }
    }

    fn audio_stream(
        &self,
        stream: usize,
        start: i64,
        frames: usize,
        sample_rate: u32,
    ) -> Result<AudioBuffer, MediaError> {
        match &self.sound {
            Sound::Stream(source) => source.audio_stream(stream, start, frames, sample_rate),
            Sound::Frames if stream < self.info.audio_streams.len() => {
                Ok(self.frames_audio(start, frames, (sample_rate, stream)))
            }
            _ => Err(MediaError::NoStream("audio")),
        }
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
    use std::path::PathBuf;

    /// A DV frame made by the layout of ITU-R BT.1618-1 (no encoder needed): every DCT block only its DC, `dc` for
    /// luma and 0 for colour (so Y is 128 + dc / 2, grey colour), and 48 kHz sound whose sample `n` of the frame is
    /// `sample(channel, n)`.
    fn frame(system: System, dc: i32, wide: bool, sample: impl Fn(usize, usize) -> i16) -> Vec<u8> {
        let sequences = system.sequences();
        let mut bytes = vec![0xFFu8; system.frame_bytes()];
        let (group, samples) = match system {
            System::Lines525 => (5, 1_600),
            System::Lines625 => (6, 1_920),
        };
        for sequence in 0..sequences {
            for block in 0..150 {
                let at = sequence * 12_000 + block * 80;
                let (section, number) = match block {
                    0 => (0, 0),
                    1 | 2 => (1, block - 1),
                    3..=5 => (2, block - 3),
                    _ if (block - 6) % 16 == 0 => (3, (block - 6) / 16),
                    _ => (4, (block - 6) / 16 * 15 + (block - 6) % 16 - 1),
                };
                bytes[at] = (section << 5) | 0x1F;
                bytes[at + 1] = ((sequence as u8) << 4) | 0x07;
                bytes[at + 2] = number as u8;
                match section {
                    0 => {
                        bytes[at + 3] = if system == System::Lines625 {
                            0xBF
                        } else {
                            0x3F
                        };
                        bytes[at + 4] = 0xF8; // APT 000: IEC 61834
                    }
                    2 if number == 0 => {
                        bytes[at + 3..at + 8].copy_from_slice(&[0x60, 0xFF, 0xFF, 0xC0, 0xFF]);
                        let disp = if wide { 0x02 } else { 0x00 };
                        bytes[at + 8..at + 13].copy_from_slice(&[
                            0x61,
                            0x3F,
                            0xC8 | disp,
                            0xFC,
                            0xFF,
                        ]);
                    }
                    3 => {
                        // The AAUX source pack: 1 600 / 1 920 samples, 48 kHz, 16 bits.
                        let size = samples
                            - if system == System::Lines625 {
                                1_896
                            } else {
                                1_580
                            };
                        bytes[at + 3..at + 8]
                            .copy_from_slice(&[0x50, size as u8, 0x00, 0xC0, 0xC0]);
                    }
                    4 => {
                        // STA 0, QNO 15; each area: DC, mode 0, class 0, EOB, then zeros.
                        bytes[at + 3] = 0x0F;
                        for byte in &mut bytes[at + 4..at + 80] {
                            *byte = 0;
                        }
                        for (l, start) in [8usize, 120, 232, 344, 456, 536].into_iter().enumerate()
                        {
                            let value = if l < 4 { dc } else { 0 };
                            let code = ((value as u32 & 0x1FF) << 7) | 0b0110;
                            for i in 0..16 {
                                let bit = start + i;
                                if code >> (15 - i) & 1 == 1 {
                                    bytes[at + 3 + bit / 8] |= 0x80 >> (bit % 8);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        for channel in 0..2 {
            for n in 0..samples {
                let sequence = (n / 3 + 2 * (n % 3)) % group + channel * group;
                let block = 3 * (n % 3) + (n % (9 * group)) / (3 * group);
                let byte = 8 + 2 * (n / (9 * group));
                let at = sequence * 12_000 + (6 + 16 * block) * 80 + byte;
                bytes[at..at + 2].copy_from_slice(&sample(channel, n).to_be_bytes());
            }
        }
        bytes
    }

    /// The test sound: a ramp on the left, its negative on the right, numbered through the frames.
    fn tone(frame: usize) -> impl Fn(usize, usize) -> i16 {
        move |channel, n| {
            let value = ((frame * 2_000 + n) % 30_000) as i16;
            if channel == 0 { value } else { -value }
        }
    }

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("disccraft-dv-{}-{name}", std::process::id()))
    }

    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        if body.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    fn list(kind: &[u8; 4], parts: &[Vec<u8>]) -> Vec<u8> {
        let mut body = kind.to_vec();
        for part in parts {
            body.extend_from_slice(part);
        }
        chunk(b"LIST", &body)
    }

    fn riff(form: &[u8; 4], parts: &[Vec<u8>]) -> Vec<u8> {
        let mut body = form.to_vec();
        for part in parts {
            body.extend_from_slice(part);
        }
        chunk(b"RIFF", &body)
    }

    /// A type 1 AVI (one `iavs` stream) of `frames`: the first two in the `AVI ` part (the second in a `rec ` list),
    /// an empty chunk (the frame repeated), and the rest in an OpenDML `AVIX` part.
    fn type1_avi(frames: &[Vec<u8>]) -> Vec<u8> {
        let mut strh = b"iavsdvsd".to_vec();
        strh.resize(56, 0);
        let strl = list(
            b"strl",
            &[chunk(b"strh", &strh), chunk(b"strf", &[0u8; 32])],
        );
        let hdrl = list(b"hdrl", &[chunk(b"avih", &[0u8; 56]), strl]);
        let first = list(
            b"movi",
            &[
                chunk(b"00dc", &frames[0]),
                list(b"rec ", &[chunk(b"00dc", &frames[1])]),
                chunk(b"00dc", &[]),
                chunk(b"ix00", &[0u8; 24]),
            ],
        );
        let mut file = riff(b"AVI ", &[hdrl, first, chunk(b"idx1", &[])]);
        let rest: Vec<Vec<u8>> = frames[2..].iter().map(|f| chunk(b"00dc", f)).collect();
        file.extend(riff(b"AVIX", &[list(b"movi", &rest)]));
        file
    }

    #[test]
    fn a_raw_dv_file_opens_with_its_pictures_fields_and_sound() {
        let frames: Vec<Vec<u8>> = (0..3)
            .map(|i| frame(System::Lines525, 40 + 20 * i as i32, false, tone(i)))
            .collect();
        let path = temp("raw.dv");
        std::fs::write(&path, frames.concat()).unwrap();
        let source = crate::source::Source::open(&path).unwrap();
        let info = source.info();
        let video = info.video.as_ref().unwrap();
        assert_eq!(
            (video.width, video.height, video.codec.as_str()),
            (720, 480, "DV")
        );
        assert_eq!(video.par, (8, 9));
        assert_eq!(info.duration, video.frame_rate.tick_of(3));
        assert_eq!(
            crate::field_type::field_type(&path),
            Some(crate::field_type::FieldType::LowerFirst)
        );
        for i in 0..3 {
            let picture = source.video_frame(video.frame_rate.tick_of(i)).unwrap();
            let PixelData::Yuv8 { planes, chroma, .. } = &picture.data else {
                panic!("not 8-bit Y'CbCr");
            };
            assert_eq!(*chroma, Chroma::C422);
            let luma = (128 + (40 + 20 * i) / 2) as u8;
            assert!(planes[0].iter().all(|&y| y == luma), "frame {i}");
            assert!(planes[1].iter().chain(planes[2].iter()).all(|&c| c == 128));
        }
        // The sound: frame 1 starts at sample 1 601 (1 601.6 a frame), its first sample is 2 000.
        let sound = source.audio(1_601, 4, 48_000).unwrap();
        let expected = [2_000.0f32, 2_001.0, 2_002.0, 2_003.0].map(|v| v / 32_768.0);
        assert_eq!(sound.channels[0], expected);
        assert_eq!(sound.channels[1], expected.map(|v| -v));
        // At 44.1 kHz: interpolated from the 48 kHz samples.
        let sound = source.audio(0, 441, 44_100).unwrap();
        assert_eq!(sound.channels[0].len(), 441);
        let last = sound.channels[0][440] * 32_768.0;
        assert!((last - 440.0 * 48_000.0 / 44_100.0).abs() < 0.01, "{last}");
        let _ = std::fs::remove_file(&path);
    }

    /// `frame` with its sound made a camcorder's 12-bit 32 kHz (525/60: 1 068 pairs a frame), every sample of sound 1
    /// 256 on the left and -256 on the right (codes 0x100 and 0xF00), of sound 2 (the second half) 512 and -512.
    fn twelve_bit(frame: &mut [u8]) {
        for sequence in 0..10 {
            for block in 0..9 {
                let at = sequence * 12_000 + (6 + 16 * block) * 80;
                if frame[at + 3] == 0x50 {
                    frame[at + 4] = (frame[at + 4] & 0xC0) | 15;
                    frame[at + 7] = (frame[at + 7] & 0xC0) | (2 << 3) | 1;
                }
                let unit_bytes = if sequence < 5 {
                    [0x10, 0xF0, 0x00]
                } else {
                    [0x20, 0xE0, 0x00]
                };
                for unit in frame[at + 8..at + 80].as_chunks_mut::<3>().0 {
                    unit.copy_from_slice(&unit_bytes);
                }
            }
        }
    }

    #[test]
    fn twelve_bit_sound_in_the_frames_is_read_at_its_own_rate() {
        let frames: Vec<Vec<u8>> = (0..3)
            .map(|i| {
                let mut f = frame(System::Lines525, 0, false, tone(i));
                twelve_bit(&mut f);
                f
            })
            .collect();
        let path = temp("twelve.dv");
        std::fs::write(&path, frames.concat()).unwrap();
        let source = crate::source::Source::open(&path).unwrap();
        let stream = &source.info().audio_streams[0];
        assert_eq!(
            (stream.sample_rate, stream.bits_per_sample),
            (32_000, Some(12))
        );
        let level = 256.0 / 32_768.0;
        let sound = source.audio(1_000, 200, 32_000).unwrap();
        assert!(sound.channels[0].iter().all(|&v| v == level));
        assert!(sound.channels[1].iter().all(|&v| v == -level));
        // Asked at 48 kHz (a DVD's rate): converted.
        let sound = source.audio(0, 4_000, 48_000).unwrap();
        assert_eq!(sound.channels[0].len(), 4_000);
        assert!(
            sound.channels[0][10..3_990]
                .iter()
                .all(|&v| (v - level).abs() < 1e-6)
        );
        // Sound 2 is the second stream (M6.418).
        assert_eq!(source.info().audio_streams.len(), 2);
        let second = source
            .clone()
            .with_stream(1)
            .audio(1_000, 200, 32_000)
            .unwrap();
        assert!(second.channels[0].iter().all(|&v| v == 2.0 * level));
        assert!(second.channels[1].iter().all(|&v| v == -2.0 * level));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_type_1_avi_over_two_riff_parts_has_every_frame() {
        let frames: Vec<Vec<u8>> = (0..4)
            .map(|i| frame(System::Lines625, 20 * i as i32, true, tone(i)))
            .collect();
        let path = temp("type1.avi");
        std::fs::write(&path, type1_avi(&frames)).unwrap();
        let source = crate::source::Source::open(&path).unwrap();
        let info = source.info();
        let video = info.video.as_ref().unwrap();
        assert_eq!((video.width, video.height), (720, 576));
        assert_eq!(video.par, (64, 45), "16:9");
        assert_eq!(info.audio_streams.len(), 1);
        // Frames 0, 1, 1 again (the empty chunk), 2, 3.
        assert_eq!(info.duration, video.frame_rate.tick_of(5));
        for (i, dc) in [0, 20, 20, 40, 60].into_iter().enumerate() {
            let picture = source
                .video_frame(video.frame_rate.tick_of(i as i64))
                .unwrap();
            let PixelData::Yuv8 { planes, chroma, .. } = &picture.data else {
                panic!("not 8-bit Y'CbCr");
            };
            assert_eq!(*chroma, Chroma::C420);
            assert_eq!(planes[0][1_000], (128 + dc / 2) as u8, "frame {i}");
        }
        let sound = source.audio(1_920 * 3, 1, 48_000).unwrap();
        assert_eq!(
            sound.channels[0][0] * 32_768.0,
            4_000.0,
            "frame 3 is frame 2 of the file"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// M6.419: an AVI whose DV stream counts time in 1/600 s (ffmpeg's copy of a raw DV stream) has a frame in one
    /// chunk in 20; the others are empty. Its frames are where their times are, not one a chunk.
    #[test]
    fn an_avi_of_600_chunks_a_second_has_its_frames_at_their_times() {
        let frames: Vec<Vec<u8>> = (0..4)
            .map(|i| frame(System::Lines525, 20 * i as i32, false, tone(i)))
            .collect();
        let mut strh = b"vidsdvsd".to_vec();
        strh.resize(56, 0);
        strh[20..24].copy_from_slice(&1u32.to_le_bytes());
        strh[24..28].copy_from_slice(&600u32.to_le_bytes());
        let mut strf = [0u8; 40];
        strf[16..20].copy_from_slice(b"dvsd");
        let strl = list(b"strl", &[chunk(b"strh", &strh), chunk(b"strf", &strf)]);
        let hdrl = list(b"hdrl", &[chunk(b"avih", &[0u8; 56]), strl]);
        // 61 chunks: frames at 0, 20, 40 and 60.
        let movi: Vec<Vec<u8>> = (0..61)
            .map(|i| {
                if i % 20 == 0 {
                    chunk(b"00dc", &frames[i / 20])
                } else {
                    chunk(b"00dc", &[])
                }
            })
            .collect();
        let path = temp("600.avi");
        std::fs::write(&path, riff(b"AVI ", &[hdrl, list(b"movi", &movi)])).unwrap();
        let source = crate::source::Source::open(&path).unwrap();
        let info = source.info();
        let video = info.video.as_ref().unwrap();
        assert_eq!(video.frame_rate.num, 30_000);
        assert_eq!(
            info.duration,
            video.frame_rate.tick_of(4),
            "4 frames, not 61"
        );
        for i in 0..4 {
            let picture = source
                .video_frame(video.frame_rate.tick_of(i as i64))
                .unwrap();
            let PixelData::Yuv8 { planes, .. } = &picture.data else {
                panic!("not 8-bit Y'CbCr");
            };
            assert_eq!(planes[0][1_000], (128 + 10 * i) as u8, "frame {i}");
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn chunks_are_frames_when_they_last_a_frame() {
        let chunks = [(0, 10), (10, 0), (20, 10), (30, 0)];
        let ntsc = (30_000, 1_001);
        // A chunk a frame (29.97, 30, or not said): an empty chunk repeats the frame before.
        let expected = vec![(0, 10), (0, 10), (20, 10), (20, 10)];
        for time_base in [(1_001, 30_000), (1, 30), (0, 0)] {
            assert_eq!(frames_of_chunks(&chunks, time_base, ntsc), expected);
        }
        // Two chunks a frame (59.94 a second): the first of each two.
        let chunks = [(0, 10), (10, 10), (20, 10), (30, 10)];
        assert_eq!(
            frames_of_chunks(&chunks, (1_001, 60_000), ntsc),
            vec![(0, 10), (20, 10)]
        );
        // A chunk two frames (14.985 a second): each twice.
        assert_eq!(
            frames_of_chunks(&chunks[..2], (2_002, 30_000), ntsc),
            vec![(0, 10), (0, 10), (10, 10), (10, 10)]
        );
        // A header saying a chunk lasts an hour does not make a million frames of one.
        assert!(frames_of_chunks(&chunks[..1], (3_600, 1), ntsc).len() <= crate::avi::MOST_CHUNKS);
    }

    #[test]
    fn dv_is_found_in_a_quicktime_sample_description_and_a_matroska_track() {
        // stsd, version and flags, one entry of 0x56 bytes, format `dvcp`.
        let mut mov = b"....stsd".to_vec();
        mov.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0x56]);
        mov.extend_from_slice(b"dvcp");
        assert!(names_dv(&mov));
        let avc = [&mov[..mov.len() - 4], b"avc1"].concat();
        assert!(!names_dv(&avc));
        // A Matroska track: the codec ID, then a BITMAPINFOHEADER of `dvsd` in the codec private data.
        let mut mkv = b"V_MS/VFW/FOURCC".to_vec();
        mkv.extend_from_slice(&[0x63, 0xA2, 0x28]);
        mkv.extend_from_slice(&[0u8; 16]);
        mkv.extend_from_slice(b"dvsd");
        assert!(names_dv(&mkv));
        assert!(!names_dv(
            b"V_MS/VFW/FOURCC and no DV after it at all, only other words here."
        ));
    }

    #[test]
    fn other_avis_are_not_taken_for_dv_and_broken_ones_do_not_panic() {
        let mut strh = b"vidsMJPG".to_vec();
        strh.resize(56, 0);
        let strl = list(
            b"strl",
            &[chunk(b"strh", &strh), chunk(b"strf", &[0u8; 40])],
        );
        let file = riff(b"AVI ", &[list(b"hdrl", &[strl])]);
        assert!(!is_dv(&file));
        let frames: Vec<Vec<u8>> = (0..3)
            .map(|i| frame(System::Lines525, 0, false, tone(i)))
            .collect();
        let avi = type1_avi(&frames);
        assert!(is_dv(&avi));
        // Cut anywhere: an error or a shorter file, never a panic.
        for cut in [12, 100, 300, 5_000, 130_000, 250_000, avi.len() - 1] {
            let path = temp(&format!("cut{cut}.avi"));
            std::fs::write(&path, &avi[..cut]).unwrap();
            if let Ok(source) = crate::source::Source::open(&path) {
                let rate = source.info().video.as_ref().unwrap().frame_rate;
                let _ = source.video_frame(rate.tick_of(0));
                let _ = source.audio(0, 100, 48_000);
            }
            let _ = std::fs::remove_file(&path);
        }
    }
}
