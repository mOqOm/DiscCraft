use std::path::Path;
use std::sync::Arc;

use filmcraft_codecs::{openers, reader_openers};
use filmcraft_frame::{AudioBuffer, VideoFrame};
use filmcraft_media::reader::{FileReader, open_reader};
use filmcraft_media::{FrameRequest, MediaInfo, MediaSource};
use filmcraft_time::Tick;

use crate::{TICKS_PER_SECOND, TranscodeError, io_error};

/// Opened FilmCraft media source with DVD-oriented validation.
pub(crate) struct Source {
    media: Arc<dyn MediaSource>,
}

impl Source {
    /// Open a media file and require a bounded-duration video stream.
    pub(crate) fn open(path: &Path) -> Result<Self, TranscodeError> {
        let source = Self::open_media(path)?;

        if source.media.info().video.is_none() {
            return Err(TranscodeError::Unsupported(
                "input has no video stream".to_string(),
            ));
        }

        Ok(source)
    }

    /// Open a media file and require a bounded-duration audio stream.
    pub(crate) fn open_audio(path: &Path) -> Result<Self, TranscodeError> {
        let source = Self::open_media(path)?;

        if source.media.info().audio_streams.is_empty() {
            return Err(TranscodeError::Unsupported(
                "input has no audio stream".to_string(),
            ));
        }

        Ok(source)
    }

    /// Borrow source metadata.
    pub(crate) fn info(&self) -> &MediaInfo {
        self.media.info()
    }

    /// Return whether the source advertises at least one audio stream.
    pub(crate) fn has_audio(&self) -> bool {
        !self.media.info().audio_streams.is_empty()
    }

    /// Request one decoded video frame at `time`.
    pub(crate) fn video_frame(&self, time: Tick) -> Result<Arc<VideoFrame>, TranscodeError> {
        self.media
            .video_frame(FrameRequest::full(time))
            .map_err(|error| TranscodeError::Decode(error.to_string()))
    }

    /// Request decoded audio starting at `start` sample frames.
    pub(crate) fn audio(
        &self,
        start: i64,
        frames: usize,
        sample_rate: u32,
    ) -> Result<AudioBuffer, TranscodeError> {
        self.media
            .audio(start, frames, sample_rate)
            .map_err(|error| TranscodeError::Decode(error.to_string()))
    }

    fn open_media(path: &Path) -> Result<Self, TranscodeError> {
        let reader = FileReader::open(path).map_err(|error| io_error(path, error))?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("input");

        let reader_openers = reader_openers();
        let extra = openers();

        let media: Arc<dyn MediaSource> =
            open_reader(name, Arc::new(reader), &reader_openers, &extra)
                .map_err(|error| TranscodeError::Decode(error.to_string()))?;

        validate_duration(media.info().duration)?;

        Ok(Self { media })
    }
}

fn validate_duration(duration: Tick) -> Result<(), TranscodeError> {
    if duration.0 < 0 {
        return Err(TranscodeError::Unsupported(
            "input duration is negative".to_string(),
        ));
    }

    let max_ticks = TICKS_PER_SECOND
        .checked_mul(36_000)
        .ok_or_else(|| TranscodeError::Unsupported("duration limit overflow".to_string()))?;

    if duration.0 > max_ticks {
        return Err(TranscodeError::Unsupported(
            "input duration exceeds 10 hours".to_string(),
        ));
    }

    Ok(())
}
