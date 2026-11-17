use std::path::Path;
use std::sync::Arc;

use filmcraft_codecs::MpegSource;
use filmcraft_codecs::{openers, reader_openers};
use filmcraft_frame::{AudioBuffer, VideoFrame};
use filmcraft_media::reader::{FileReader, open_reader};
use filmcraft_media::{FrameRequest, MediaInfo, MediaSource, SharedReader};
use filmcraft_time::Tick;

use crate::pulldown::{self, Timeline};
use crate::{TICKS_PER_SECOND, TranscodeError, io_error};

/// Opened FilmCraft media source with DVD-oriented validation. Cheap to clone: one opened file is shared by
/// everything that asks for it (`crate::media_cache`).
#[derive(Clone)]
pub(crate) struct Source {
    media: Arc<dyn MediaSource>,
    /// The real display times of MPEG-2 video whose frames FilmCraft spaces evenly but the stream does not (soft
    /// pulldown), with the media info corrected to its real length; `None` for everything else.
    timing: Option<Arc<(Timeline, MediaInfo)>>,
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
        match &self.timing {
            Some(timing) => &timing.1,
            None => self.media.info(),
        }
    }

    /// Return whether the source advertises at least one audio stream.
    pub(crate) fn has_audio(&self) -> bool {
        !self.media.info().audio_streams.is_empty()
    }

    /// Request one decoded video frame at `time`.
    pub(crate) fn video_frame(&self, time: Tick) -> Result<Arc<VideoFrame>, TranscodeError> {
        // With a timeline: the frame showing at `time`, asked for where FilmCraft's even spacing puts it.
        let time = match (&self.timing, self.media.info().video.as_ref()) {
            (Some(timing), Some(video)) => {
                let frame = i64::try_from(timing.0.frame_at(time)).unwrap_or(i64::MAX);
                video.frame_rate.tick_of(frame)
            }
            _ => time,
        };
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

    /// Opens `path`, or gives the source it was opened as before (the same file: path, size and time). A long
    /// file's index is read once, however many times the file is asked about.
    fn open_media(path: &Path) -> Result<Self, TranscodeError> {
        // A SoundCraft session is opened as its mixdown.
        let resolved = crate::session::resolved(path)?;
        let path = resolved.as_ref();
        let Some(key) = crate::media_cache::FileKey::of(path) else {
            return Self::open_file(path);
        };
        // Found unreadable when it was opened ahead: not read through again each time it is asked for.
        if crate::media_cache::refused_file(&key) {
            return Err(TranscodeError::Decode(format!(
                "{}: cannot be read as media",
                path.display()
            )));
        }
        let slot = crate::media_cache::slot(&key);
        // Held while the file is opened: a second caller waits for this one's source.
        let mut held = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(source) = held.as_ref() {
            return Ok(source.clone());
        }
        let source = Self::open_file(path)?;
        *held = Some(source.clone());
        Ok(source)
    }

    fn open_file(path: &Path) -> Result<Self, TranscodeError> {
        let reader = FileReader::open(path).map_err(|error| io_error(path, error))?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("input");

        let reader: SharedReader = Arc::new(reader);
        let head = filmcraft_media::reader::read_range(&*reader, 0, 4096)
            .map_err(|error| io_error(path, error))?;

        // MPEG program, transport and video elementary streams: opened as FilmCraft's reader opener does, keeping
        // the source's own type for its timestamps.
        let (media, timing): (Arc<dyn MediaSource>, _) = if filmcraft_codecs::mpeg::sniff(&head) {
            let mpeg = MpegSource::open_reader(name, reader)
                .map_err(|error| TranscodeError::Decode(error.to_string()))?;
            let timing = mpeg_timing(&mpeg, path);
            (Arc::new(mpeg), timing)
        } else {
            let reader_openers = reader_openers();
            let extra = openers();
            let media = open_reader(name, reader, &reader_openers, &extra)
                .map_err(|error| TranscodeError::Decode(error.to_string()))?;
            (media, None)
        };

        let source = Self {
            media,
            timing: timing.map(Arc::new),
        };
        validate_duration(source.info().duration)?;
        Ok(source)
    }
}

/// The real timeline of an MPEG source's MPEG-2 video and its media info with the real length, when its frames are
/// spaced unevenly (soft pulldown): from the presentation timestamps of a program or transport stream, or the
/// repeat_first_field flags of an elementary stream.
fn mpeg_timing(mpeg: &MpegSource, path: &Path) -> Option<(Timeline, MediaInfo)> {
    let info = mpeg.info();
    let video = info.video.as_ref()?;
    if !video.codec.contains("MPEG") {
        return None;
    }
    let rate = video.frame_rate;
    let timeline = if mpeg.file().is_some() {
        Timeline::from_pts(&mpeg.video_pts())?
    } else {
        let pictures = pulldown::scan_elementary_stream(path).ok()??;
        Timeline::from_fields(&pulldown::presented_fields(&pictures), rate)?
    };
    if !timeline.is_uneven(rate) {
        return None;
    }
    let mut info = info.clone();
    info.duration = info.duration.max(timeline.duration());
    Some((timeline, info))
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
