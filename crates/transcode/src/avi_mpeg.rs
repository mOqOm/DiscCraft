//! MPEG-1 / MPEG-2 video in an AVI (M6.417): what some capture cards and converters wrote (`mpg2`, `MMES`, …). The
//! media reader lists such a stream but has no decoder for it in an AVI. Its chunks are the elementary stream cut into
//! pictures, so they are read one after another as one elementary stream ([`Chunks`], nothing copied) by the media
//! reader's MPEG reader, and the sound comes from the media reader's AVI reader.

use std::io;
use std::sync::Arc;

use filmcraft_frame::{AudioBuffer, VideoFrame};
use filmcraft_media::reader::{ByteReader, SharedReader};
use filmcraft_media::{FrameRequest, MediaError, MediaInfo, MediaSource};

use crate::TranscodeError;
use crate::avi::{Stream, upper};

/// Whether an AVI stream is MPEG-1 or MPEG-2 video by its handler or compression code.
fn is_mpeg_stream(stream: &Stream) -> bool {
    let mpeg = |code: &[u8; 4]| {
        matches!(
            &upper(code),
            b"MPG2" | b"MPG1" | b"MPEG" | b"MMES" | b"MP2V" | b"M2V1"
        )
    };
    &stream.kind == b"vids" && (mpeg(&stream.handler) || mpeg(&stream.compression))
}

/// Whether `head` (the start of a file) is an AVI with an MPEG video stream among the stream headers it holds.
pub(crate) fn is_mpeg_avi(head: &[u8]) -> bool {
    crate::avi::head_has_stream(head, is_mpeg_stream)
}

/// The chunks of a stream read one after another as one stream of bytes.
struct Chunks {
    reader: SharedReader,
    /// Each chunk's place in the file and its length, and (in `starts`) where it begins in the joined stream.
    chunks: Vec<(u64, u32)>,
    starts: Vec<u64>,
    length: u64,
}

impl Chunks {
    fn new(reader: SharedReader, chunks: Vec<(u64, u32)>) -> Self {
        let chunks: Vec<(u64, u32)> = chunks.into_iter().filter(|&(_, size)| size > 0).collect();
        let mut starts = Vec::with_capacity(chunks.len());
        let mut length = 0u64;
        for &(_, size) in &chunks {
            starts.push(length);
            length = length.saturating_add(u64::from(size));
        }
        Chunks {
            reader,
            chunks,
            starts,
            length,
        }
    }
}

impl ByteReader for Chunks {
    fn len(&self) -> u64 {
        self.length
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let past_end = || {
            io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "past the end of the video chunks",
            )
        };
        let mut at = offset;
        let mut filled = 0usize;
        while filled < buf.len() {
            // The chunk `at` falls in.
            let index = self
                .starts
                .partition_point(|&start| start <= at)
                .checked_sub(1)
                .ok_or_else(past_end)?;
            let (&(file_at, size), &start) = (
                self.chunks.get(index).ok_or_else(past_end)?,
                self.starts.get(index).ok_or_else(past_end)?,
            );
            let into = at - start;
            let left = u64::from(size)
                .checked_sub(into)
                .filter(|&left| left > 0)
                .ok_or_else(past_end)?;
            let count = usize::try_from(left)
                .unwrap_or(usize::MAX)
                .min(buf.len() - filled);
            let target = buf.get_mut(filled..filled + count).ok_or_else(past_end)?;
            self.reader.read_at(file_at + into, target)?;
            filled += count;
            at += count as u64;
        }
        Ok(())
    }
}

/// The MPEG video of an AVI and its sound.
struct AviMpeg {
    info: MediaInfo,
    video: Arc<dyn MediaSource>,
    sound: Option<Arc<dyn MediaSource>>,
}

/// Opens the AVI `reader` (whose first bytes are `head`) when its video is MPEG: `None` otherwise.
///
/// # Errors
/// When it is, but its video cannot be read as MPEG.
pub(crate) fn open(
    name: &str,
    reader: &SharedReader,
    head: &[u8],
) -> Result<Option<Arc<dyn MediaSource>>, TranscodeError> {
    if !is_mpeg_avi(head) {
        return Ok(None);
    }
    let Some(avi) = crate::avi::read_avi(reader) else {
        return Ok(None);
    };
    let Some(number) = avi.streams.iter().position(is_mpeg_stream) else {
        return Ok(None);
    };
    let chunks: SharedReader = Arc::new(Chunks::new(reader.clone(), avi.chunks(reader, number)));
    let video: Arc<dyn MediaSource> = Arc::new(
        filmcraft_codecs::MpegSource::open_reader(name, chunks)
            .map_err(|error| TranscodeError::Decode(format!("{name}: {error}")))?,
    );
    let sound: Option<Arc<dyn MediaSource>> = if avi.has(b"auds") {
        filmcraft_codecs::AviSource::open_reader(name, reader.clone())
            .ok()
            .filter(|source| !source.info().audio_streams.is_empty())
            .map(|source| Arc::new(source) as Arc<dyn MediaSource>)
    } else {
        None
    };
    let mut info = video.info().clone();
    info.name = name.to_owned();
    info.container = "AVI".to_owned();
    info.audio_streams = sound
        .as_ref()
        .map(|sound| sound.info().audio_streams.clone())
        .unwrap_or_default();
    info.file_size = Some(reader.len());
    Ok(Some(Arc::new(AviMpeg { info, video, sound })))
}

impl MediaSource for AviMpeg {
    fn info(&self) -> &MediaInfo {
        &self.info
    }

    fn video_frame(&self, req: FrameRequest) -> Result<Arc<VideoFrame>, MediaError> {
        self.video.video_frame(req)
    }

    fn audio(
        &self,
        start: i64,
        frames: usize,
        sample_rate: u32,
    ) -> Result<AudioBuffer, MediaError> {
        match &self.sound {
            Some(sound) => sound.audio(start, frames, sample_rate),
            None => Err(MediaError::NoStream("audio")),
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
            Some(sound) => sound.audio_stream(stream, start, frames, sample_rate),
            None => Err(MediaError::NoStream("audio")),
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

    #[test]
    fn chunks_read_as_one_stream_across_their_ends() {
        let file: Vec<u8> = (0..100u8).collect();
        let reader: SharedReader = Arc::new(Bytes(file));
        // Bytes 10..15, then (an empty chunk) 40..43, then 70..80.
        let chunks = Chunks::new(reader, vec![(10, 5), (20, 0), (40, 3), (70, 10)]);
        assert_eq!(chunks.len(), 18);
        let mut all = vec![0u8; 18];
        chunks.read_at(0, &mut all).unwrap();
        let expected: Vec<u8> = (10..15).chain(40..43).chain(70..80).collect();
        assert_eq!(all, expected);
        let mut middle = [0u8; 4];
        chunks.read_at(3, &mut middle).unwrap();
        assert_eq!(middle, [13, 14, 40, 41]);
        assert!(chunks.read_at(15, &mut [0u8; 4]).is_err(), "past the end");
    }
}
