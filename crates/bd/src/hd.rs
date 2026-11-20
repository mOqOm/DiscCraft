//! HD titles (ROADMAP M10): a timeline's video, or a slideshow's pictures (M10.6), encoded to H.264 at the disc's
//! HD format (`BdVideo`), or to MPEG-2 for the interlaced formats (1080i, M10.14), in place of its SD MPEG-2 video. The audio, chapters and timing stay those of the title's SD streams; subtitles are drawn
//! again at the HD size.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use disccraft_bdmv::StreamCoding;
use disccraft_dvd::{SubtitleTrack, TitleSource, TitleStreams};
use disccraft_m2tsmux::StreamKind;
use disccraft_project::Tick;
use disccraft_project::{BdCodec, BdVideo};
use disccraft_project::{Project, TimelineId};
use disccraft_transcode::hd::{
    AccessUnit, HdCodec, HdOptions, read_h264_index, transcode_h264_range,
};

/// Ticks in a second (the project's time base).
const TICKS_PER_SECOND: i64 = 254_016_000_000;

use crate::{BdError, invalid, io_error};

/// Target and peak bitrate of an H.264 HD motion menu, kilobits per second (menus are short and have text).
const MENU_H264_KBPS: (u32, u32) = (15_000, 30_000);

/// The bitrates of the disc's HD menus (a still ignores them: it is coded at a fixed quantiser).
pub(crate) fn menu_rates(choice: BdCodec, video: BdVideo) -> (u32, u32) {
    match codec(choice, video) {
        HdCodec::Mpeg2 => disccraft_dvd::hd_default_kbps(true),
        HdCodec::H264 => MENU_H264_KBPS,
    }
}

/// How the disc's HD video in format `video` is coded on a disc whose codec is `choice` (M12.28): MPEG-2 for the
/// interlaced formats unless the disc asks for H.264 (then coded as interlaced frames), else H.264.
pub(crate) fn codec(choice: BdCodec, video: BdVideo) -> HdCodec {
    if choice.mpeg2(video) {
        HdCodec::Mpeg2
    } else {
        HdCodec::H264
    }
}

/// The multiplexer's stream kind of the disc's HD video.
pub(crate) fn kind(choice: BdCodec, video: BdVideo) -> StreamKind {
    kind_of(codec(choice, video))
}

/// The multiplexer's stream kind of HD video coded as `codec`.
pub(crate) fn kind_of(codec: HdCodec) -> StreamKind {
    match codec {
        HdCodec::Mpeg2 => StreamKind::Mpeg2Video,
        HdCodec::H264 => StreamKind::H264Video,
    }
}

/// The clip and playlist attributes of video in the HD format `video` (`None` for SD): the format code (4 for
/// 1080i, 5 for 720p, 6 for 1080p), the frame rate code (1 23.976, 2 24, 3 25, 4 29.97, 6 50, 7 59.94; an
/// interlaced format has its frames' rate) and 16:9 (a 4:3 source is pillarboxed in the HD frame).
pub(crate) fn coding(choice: BdCodec, video: BdVideo) -> Option<StreamCoding> {
    coding_of(video, codec(choice, video))
}

/// [`coding`] of video in the format `video` that is coded as `codec` (a title whose own H.264 went on the disc as
/// it is, on a disc whose encoded video is MPEG-2).
pub(crate) fn coding_of(video: BdVideo, codec: HdCodec) -> Option<StreamCoding> {
    let (_, height, num, den) = video.hd()?;
    let video_format = if video.interlaced() {
        4
    } else if height >= 1080 {
        6
    } else {
        5
    };
    let frame_rate = match (num, den) {
        (24_000, 1001) => 1,
        (24, 1) => 2,
        (25, 1) => 3,
        (50, 1) => 6,
        (60_000, 1001) => 7,
        _ => 4,
    };
    Some(match codec {
        HdCodec::Mpeg2 => StreamCoding::Mpeg2Video {
            video_format,
            frame_rate,
            aspect: 3,
        },
        HdCodec::H264 => StreamCoding::H264Video {
            video_format,
            frame_rate,
            aspect: 3,
        },
    })
}

/// The encoder's settings for the disc's HD format at a target and peak bitrate in kilobits per second; `None`
/// for SD.
pub(crate) fn options(
    choice: BdCodec,
    video: BdVideo,
    (target_kbps, max_kbps): (u32, u32),
) -> Option<HdOptions> {
    let (width, height, fps_num, fps_den) = video.hd()?;
    let codec = codec(choice, video);
    Some(HdOptions {
        width,
        height,
        fps_num,
        fps_den,
        target_kbps,
        max_kbps,
        codec,
        quality: disccraft_transcode::BEST_QUALITY,
        rendering: disccraft_transcode::Rendering::default(),
        mode: disccraft_transcode::RateMode::Vbr,
        // H.264 of a 1080i format: coded as interlaced frames (M12.28).
        interlaced: codec == HdCodec::H264 && video.interlaced(),
    })
}

/// Reads an HD title's access units from its cached streams one at a time, as the multiplexer writes them.
pub(crate) struct HdReader {
    /// The title's files, each with the offset its first byte and the number its first unit have in the title's
    /// stream.
    files: Vec<(u64, usize, HdSource, std::sync::Mutex<std::fs::File>)>,
    /// Each unit's offset and length.
    units: Vec<(u64, u32)>,
}

impl disccraft_m2tsmux::UnitData for HdReader {
    fn read(&self, unit: usize) -> std::io::Result<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let (offset, len) = self.units.get(unit).copied().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("no unit {unit}"))
        })?;
        // The file the unit is in: the last that starts at or before it (a unit lies in one file).
        let at = self
            .files
            .partition_point(|(base, ..)| *base <= offset)
            .checked_sub(1);
        let (base, first, source, file) =
            at.and_then(|at| self.files.get(at)).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("no unit {unit}"))
            })?;
        let mut file = file
            .lock()
            .map_err(|_| std::io::Error::other("the HD stream reader was poisoned"))?;
        // Pictures left in their own file are read from it as access units.
        match source {
            HdSource::Samples(samples) => {
                return samples.read(&mut file, unit.saturating_sub(*first));
            }
            HdSource::Transport(ts) => return ts.read(&mut file, unit.saturating_sub(*first)),
            HdSource::File(_) => {}
        }
        file.seek(SeekFrom::Start(offset.saturating_sub(*base)))?;
        let mut bytes = vec![0u8; len as usize];
        file.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    fn len(&self, unit: usize) -> usize {
        self.units.get(unit).map_or(0, |(_, len)| *len as usize)
    }
}

/// Where a part of a title's HD video is read from.
#[derive(Debug, Clone)]
pub(crate) enum HdSource {
    /// A cached elementary stream (an encode, or H.264 taken out of a transport stream).
    File(PathBuf),
    /// The pictures of a Matroska or MP4 file whose H.264 goes on the disc as it is, left in that file.
    Samples(std::sync::Arc<disccraft_dvd::H264Samples>),
    /// The pictures of a transport stream whose H.264 goes on the disc as it is, left in that file (M6.367).
    Transport(std::sync::Arc<disccraft_dvd::TsUnits>),
}

impl HdSource {
    fn path(&self) -> &Path {
        match self {
            Self::File(path) => path,
            Self::Samples(samples) => samples.path(),
            Self::Transport(ts) => ts.path(),
        }
    }
}

/// A title's HD video.
pub(crate) struct HdTitle {
    /// The disc's HD format.
    pub(crate) video: BdVideo,
    /// How this title's video is coded: the disc's coding, or H.264 that went on the disc as it is.
    pub(crate) codec: HdCodec,
    /// The cached elementary streams (H.264 Annex B, or MPEG-2) that make the title's stream one after another
    /// (one for each clip and gap), each with the offset of its first byte in that stream. Read a unit at a time
    /// as the clip is written.
    pub(crate) streams: Vec<(u64, usize, HdSource)>,
    /// Its access units (offsets in the title's stream), time stamps in 90 kHz from 0.
    pub(crate) units: Vec<AccessUnit>,
    /// Frame size.
    pub(crate) width: u16,
    /// Frame height.
    pub(crate) height: u16,
    /// Frame rate (numerator, denominator).
    pub(crate) rate: (u32, u32),
    /// Subtitle tracks drawn at the HD size.
    pub(crate) subtitles: Vec<SubtitleTrack>,
}

impl HdTitle {
    /// A reader of its access units for the multiplexer.
    pub(crate) fn reader(&self) -> Result<HdReader, BdError> {
        let mut files = Vec::with_capacity(self.streams.len());
        for (base, first, source) in &self.streams {
            let path = source.path();
            let file = std::fs::File::open(path).map_err(|error| io_error(path, error))?;
            files.push((*base, *first, source.clone(), std::sync::Mutex::new(file)));
        }
        Ok(HdReader {
            files,
            units: self
                .units
                .iter()
                .map(|unit| (unit.offset, unit.len))
                .collect(),
        })
    }

    /// One frame in 90 kHz units.
    pub(crate) fn frame_90k(&self) -> i64 {
        let (num, den) = self.rate;
        i64::from(den)
            .saturating_mul(90_000)
            .checked_div(i64::from(num.max(1)))
            .unwrap_or(3003)
    }
}

/// The HD format `title` is made in: a timeline's its video's own (M6.325) or the project's, a slideshow's the
/// project's.
pub(crate) fn title_video(project: &Project, title: &TitleStreams) -> BdVideo {
    match title.source {
        TitleSource::Timeline(id) => project
            .timelines
            .get(&TimelineId(id))
            .map_or(project.settings.bd_video, |timeline| {
                project.title_bd_video(timeline)
            }),
        TitleSource::Slideshow(_) => project.settings.bd_video,
    }
}

/// The rates (target and peak, kilobits per second) `title` is encoded at from the project's `budget`: those of
/// its format's codec ([`disccraft_dvd::HdBudget::rates_for`]); nothing without a budget.
pub(crate) fn title_rates(
    project: &Project,
    title: &TitleStreams,
    budget: Option<&disccraft_dvd::HdBudget>,
) -> (u32, u32) {
    budget.map_or((0, 0), |budget| {
        budget.rates_for(title_video(project, title))
    })
}

/// The HD video of `title` when the disc's titles are HD: a timeline's video or a slideshow's pictures, encoded
/// once at `rates` (target and peak, kilobits per second: [`title_rates`]) in its format ([`title_video`]) and kept
/// in `cache_dir` (a timeline's by the video file, its size and time, the format and the rates).
/// The most titles made side by side ([`hd_titles`]).
const MOST_TITLES_AT_ONCE: usize = 4;

/// The HD video of every title (see [`hd_title`]), several made side by side (M6.368): the MPEG-2 encoder of a
/// 1080i title works on one core, so titles made one after another left most of the machine idle (H.264's
/// encoder uses every core already). One title for every eight cores, up to [`MOST_TITLES_AT_ONCE`]; the progress
/// of each comes back here. After a title fails, no further one is started, and the first failure (in the titles'
/// order) is returned.
pub(crate) fn hd_titles(
    project: &Project,
    titles: &[TitleStreams],
    cache_dir: &Path,
    budget: Option<&disccraft_dvd::HdBudget>,
    progress: &mut dyn FnMut(&str, f32),
) -> Result<Vec<Option<HdTitle>>, BdError> {
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .div_euclid(8)
        .clamp(1, MOST_TITLES_AT_ONCE)
        .min(titles.len());
    if workers <= 1 {
        return titles
            .iter()
            .map(|title| {
                let rates = title_rates(project, title, budget);
                hd_title(project, title, cache_dir, rates, progress)
            })
            .collect();
    }
    enum Message {
        Progress(String, f32),
        Done(usize, Result<Option<HdTitle>, BdError>),
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let failed = std::sync::atomic::AtomicBool::new(false);
    let mut results: Vec<Option<Result<Option<HdTitle>, BdError>>> =
        titles.iter().map(|_| None).collect();
    let (sender, receiver) = std::sync::mpsc::channel::<Message>();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let sender = sender.clone();
            let (next, failed) = (&next, &failed);
            let _ = scope.spawn(move || {
                while !failed.load(std::sync::atomic::Ordering::SeqCst) {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let Some(title) = titles.get(index) else {
                        break;
                    };
                    let rates = title_rates(project, title, budget);
                    let result =
                        hd_title(project, title, cache_dir, rates, &mut |label, fraction| {
                            let _ = sender.send(Message::Progress(label.to_owned(), fraction));
                        });
                    if result.is_err() {
                        failed.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    if sender.send(Message::Done(index, result)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        for message in receiver {
            match message {
                Message::Progress(label, fraction) => progress(&label, fraction),
                Message::Done(index, result) => {
                    if let Some(slot) = results.get_mut(index) {
                        *slot = Some(result);
                    }
                }
            }
        }
    });
    let mut made = Vec::with_capacity(titles.len());
    for result in results {
        match result {
            Some(result) => made.push(result?),
            // Not started after another title failed (whose error comes first, in order).
            None => return Err(invalid("a title was not made")),
        }
    }
    Ok(made)
}

/// The lock of the cache file `path` while it is made (M6.368): titles made side by side may play the same clip,
/// whose encode the second waits for and then finds made.
fn making(path: &Path) -> std::sync::Arc<std::sync::Mutex<()>> {
    type Locks = std::collections::HashMap<PathBuf, std::sync::Arc<std::sync::Mutex<()>>>;
    static MAKING: std::sync::Mutex<Option<Locks>> = std::sync::Mutex::new(None);
    let mut locks = MAKING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    std::sync::Arc::clone(
        locks
            .get_or_insert_with(Locks::new)
            .entry(path.to_path_buf())
            .or_default(),
    )
}

pub(crate) fn hd_title(
    project: &Project,
    title: &TitleStreams,
    cache_dir: &Path,
    rates: (u32, u32),
    progress: &mut dyn FnMut(&str, f32),
) -> Result<Option<HdTitle>, BdError> {
    let video = title_video(project, title);
    let video_format = video;
    let format_id = video.id();
    let Some(options) = options(project.settings.bd_codec, video, rates) else {
        return Ok(None);
    };
    let (width, height, fps_num, fps_den) = (
        options.width,
        options.height,
        options.fps_num,
        options.fps_den,
    );
    let size = |value: u32| u16::try_from(value).map_err(|_| invalid("HD size too large"));
    let id = match title.source {
        TitleSource::Timeline(id) => id,
        TitleSource::Slideshow(id) => {
            let slideshow = project
                .slideshows
                .get(&id)
                .ok_or_else(|| invalid("slideshow does not exist"))?;
            let (es, index) = disccraft_dvd::encode_hd_slideshow(
                project, slideshow, cache_dir, &options, progress,
            )
            .map_err(|error| BdError::Prepare(error.to_string()))?;
            let units =
                read_h264_index(&index).map_err(|error| BdError::Prepare(error.to_string()))?;
            // The slides' subtitles (Create Subtitle), drawn for the HD frame.
            let subtitles = disccraft_dvd::slide_subtitles_sized(
                project,
                slideshow,
                title,
                Some((size(width)?, size(height)?)),
            )
            .map_err(|error| BdError::Prepare(error.to_string()))?;
            return Ok(Some(HdTitle {
                video,
                codec: options.codec,
                streams: vec![(0, 0, HdSource::File(es))],
                units,
                width: size(width)?,
                height: size(height)?,
                rate: (fps_num, fps_den),
                subtitles,
            }));
        }
    };
    let timeline = project
        .timelines
        .get(&TimelineId(id))
        .ok_or_else(|| invalid("timeline does not exist"))?;
    // Each video clip is encoded by itself (its trimmed part only) and kept in the cache; the title is the clips
    // one after another. Every encode starts with an IDR picture and its parameter sets, so the join decodes.
    let standard_rate = if project.settings.standard == disccraft_project::VideoStandard::Ntsc {
        (30_000_u32, 1001_u32)
    } else {
        (25, 1)
    };
    let videos = timeline.videos();
    let format = disccraft_dvd::BdVideoFormat {
        width,
        height,
        rate: (fps_num, fps_den),
        interlaced: video_format.interlaced(),
    };
    // M6.388 / M6.393: a title whose clips that go on as they are are mostly HDV's 1440 x 1080 is made at 1440: those
    // join as they are, and its other clips and gaps are encoded at 1440 (shown 16:9). The disc's graphics stay
    // 1920 x 1080.
    let narrow = if options.codec == HdCodec::Mpeg2 {
        disccraft_dvd::narrow_mpeg2_width(project, timeline, format)
    } else {
        None
    };
    let title_width = narrow.unwrap_or(width);
    let options = HdOptions {
        width: title_width,
        ..options
    };
    let format_key = match narrow {
        Some(narrow) => format!("{format_id}-{narrow}"),
        // 1080i coded as H.264 (M12.28) is another stream than the format's MPEG-2: another cache file.
        None if options.interlaced => format!("{format_id}-h264"),
        None => format_id.to_string(),
    };
    let format_id = format_key.as_str();
    let (parts, title_codec) = {
        let mut parts = Vec::with_capacity(videos.len());
        let mut title_codec = options.codec;
        for (clip, video) in videos.iter().enumerate() {
            let asset = project
                .assets
                .get(video)
                .ok_or_else(|| invalid("timeline video does not exist"))?;
            let media = asset.media_file();
            let source: &Path = &media;
            let trim = timeline.video_trim(clip);
            // The empty stretch in front of the clip plays black.
            let gap = timeline.video_gap(clip);
            if gap > 0 {
                parts.push(black_part(
                    cache_dir,
                    gap_frames(gap, standard_rate, (fps_num, fps_den)),
                    format_id,
                    &options,
                )?);
            }
            // A Blu-ray's own H.264 (in an .m2ts, .mkv or .mp4) that fits the disc goes on it as it is: a clip used
            // whole, or trimmed at its key pictures. Anything else is encoded. On a disc whose encoded video is
            // MPEG-2 (1080i), only a title of that one clip: H.264 and MPEG-2 do not join into one stream.
            let whole = trim.start == 0 && trim.end.is_none();
            let alone = videos.len() == 1 && gap == 0;
            // A Blu-ray preset of its own transcodes it all the same (Encore's preset picked for a compliant asset).
            let forced = asset.bluray_transcode.is_some();
            let passthrough = if (options.codec == HdCodec::H264 || alone) && !forced {
                let label = format!("HD: {}", asset.name);
                disccraft_dvd::demux_h264_for_bd(source, cache_dir, format, &mut |fraction| {
                    progress(&label, fraction);
                })
                .map_err(|error| BdError::Prepare(error.to_string()))?
            } else {
                None
            };
            // HD MPEG-2 (HDV, a broadcast recording: M10.22; an .m2v: M6.370) goes on the disc as it is too: as the
            // only clip of its title, or (M6.385) beside the title's other clips when the title's video is MPEG-2 and
            // the file's pictures are the title's width (the disc's 1920, or HDV's 1440: M6.388 / M6.393). Trimmed,
            // it is cut at its GOPs.
            let joins = options.codec == HdCodec::Mpeg2
                && disccraft_transcode::probe_video(source)
                    .is_ok_and(|probe| probe.width == title_width);
            let passthrough = match passthrough {
                None if (alone || joins) && !forced => {
                    disccraft_dvd::demux_mpeg2_for_bd(source, cache_dir, format)
                        .map_err(|error| BdError::Prepare(error.to_string()))?
                }
                other => other,
            };
            // A trimmed clip: the pictures between its in and out points, when the stream can be cut there.
            let passthrough = match passthrough {
                Some(found) if !whole => {
                    match found
                        .trimmed(trim, standard_rate)
                        .map_err(|error| BdError::Prepare(error.to_string()))?
                    {
                        Some(disccraft_dvd::TrimmedPassthrough::Samples(samples)) => {
                            let units = samples.units().to_vec();
                            parts.push((HdSource::Samples(std::sync::Arc::new(samples)), units));
                            title_codec = HdCodec::H264;
                            continue;
                        }
                        Some(disccraft_dvd::TrimmedPassthrough::Transport(ts)) => {
                            let units = ts.units().to_vec();
                            parts.push((HdSource::Transport(std::sync::Arc::new(ts)), units));
                            title_codec = HdCodec::H264;
                            continue;
                        }
                        Some(disccraft_dvd::TrimmedPassthrough::Stream { es, units }) => {
                            parts.push((HdSource::File(es), units));
                            title_codec = HdCodec::H264;
                            continue;
                        }
                        Some(disccraft_dvd::TrimmedPassthrough::Mpeg2Stream { es, units }) => {
                            parts.push((HdSource::File(es), units));
                            title_codec = HdCodec::Mpeg2;
                            continue;
                        }
                        Some(disccraft_dvd::TrimmedPassthrough::Mpeg2Transport(ts)) => {
                            let units = ts.units().to_vec();
                            parts.push((HdSource::Transport(std::sync::Arc::new(ts)), units));
                            title_codec = HdCodec::Mpeg2;
                            continue;
                        }
                        None => None,
                    }
                }
                other => other,
            };
            match &passthrough {
                Some(
                    disccraft_dvd::Passthrough::Mpeg2 { .. }
                    | disccraft_dvd::Passthrough::TransportMpeg2(_),
                ) => title_codec = HdCodec::Mpeg2,
                Some(_) => title_codec = HdCodec::H264,
                None => {}
            }
            let (es, index) = match passthrough {
                Some(disccraft_dvd::Passthrough::Samples(samples)) => {
                    let units = samples.units().to_vec();
                    parts.push((HdSource::Samples(std::sync::Arc::new(samples)), units));
                    continue;
                }
                Some(
                    disccraft_dvd::Passthrough::Transport(ts)
                    | disccraft_dvd::Passthrough::TransportMpeg2(ts),
                ) => {
                    let units = ts.units().to_vec();
                    parts.push((HdSource::Transport(std::sync::Arc::new(ts)), units));
                    continue;
                }
                Some(
                    disccraft_dvd::Passthrough::Stream { es, index }
                    | disccraft_dvd::Passthrough::Mpeg2 { es, index },
                ) => (es, index),
                None => cached_paths(
                    cache_dir,
                    source,
                    (format_id, clip_rates(asset, rates), clip_coding(asset)),
                    trim,
                )?,
            };
            // The clip's own bit rate (M6.251), when its asset has one.
            let options = {
                let (target_kbps, max_kbps) = clip_rates(asset, rates);
                HdOptions {
                    target_kbps,
                    max_kbps,
                    quality: clip_quality(asset),
                    rendering: clip_rendering(asset),
                    mode: clip_mode(asset),
                    ..options.clone()
                }
            };
            let lock = making(&es);
            let _made_alone = lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !(es.is_file() && index.is_file()) {
                std::fs::create_dir_all(cache_dir).map_err(|error| io_error(cache_dir, error))?;
                let es_part = es.with_extension("h264.part");
                let index_part = es.with_extension("idx.part");
                let label = format!("HD: {}", asset.name);
                let range = (
                    trim_tick(trim.start, standard_rate)?,
                    trim.end
                        .map(|end| trim_tick(end, standard_rate))
                        .transpose()?,
                );
                let _ = transcode_h264_range(
                    source,
                    &es_part,
                    &index_part,
                    &options,
                    range,
                    &mut |fraction| {
                        progress(&label, fraction);
                    },
                )
                .map_err(|error| BdError::Prepare(error.to_string()))?;
                std::fs::rename(&es_part, &es).map_err(|error| io_error(&es, error))?;
                std::fs::rename(&index_part, &index).map_err(|error| io_error(&index, error))?;
            }
            let units =
                read_h264_index(&index).map_err(|error| BdError::Prepare(error.to_string()))?;
            parts.push((HdSource::File(es), units));
        }
        (parts, title_codec)
    };
    // The clips and gaps one after another. Each is read from its own cache file as the clip is written: hours
    // of video are never held in memory, nor written a second time as one file.
    let (streams, units) = join_hd_clips(parts, (fps_num, fps_den))?;
    let subtitles = disccraft_dvd::subtitle_tracks_sized(
        project,
        timeline,
        title.first_pts,
        (size(width)?, size(height)?),
    )
    .map_err(|error| BdError::Prepare(error.to_string()))?;
    Ok(Some(HdTitle {
        video: video_format,
        codec: title_codec,
        streams,
        units,
        width: size(width)?,
        height: size(height)?,
        rate: (fps_num, fps_den),
        subtitles,
    }))
}

/// How hard a clip of `asset` is searched for motion when it is encoded as MPEG-2 (M6.326): its own Blu-ray
/// preset's quality (Encore's "Long Form" MPEG-2's 3), else the best.
fn clip_quality(asset: &disccraft_project::Asset) -> u8 {
    asset
        .bluray_transcode
        .as_ref()
        .map_or(disccraft_transcode::BEST_QUALITY, |setting| {
            setting.quality.clamp(
                disccraft_project::MIN_QUALITY,
                disccraft_project::BEST_QUALITY,
            )
        })
}

/// How a clip of `asset` makes its pictures (M6.327): Encore's rendering checkboxes.
fn clip_rendering(asset: &disccraft_project::Asset) -> disccraft_transcode::Rendering {
    disccraft_transcode::Rendering {
        max_quality: asset.max_render_quality,
        frame_blend: asset.frame_blend,
    }
}

/// How a clip of `asset` keeps its rate (M6.328): its own Blu-ray setting's way (Encore's presets: one-pass VBR).
fn clip_mode(asset: &disccraft_project::Asset) -> disccraft_transcode::RateMode {
    use disccraft_project::TranscodeMode;
    use disccraft_transcode::RateMode;
    match asset
        .bluray_transcode
        .as_ref()
        .map(|setting| setting.mode)
        .unwrap_or_default()
    {
        TranscodeMode::Vbr => RateMode::Vbr,
        TranscodeMode::Vbr2Pass => RateMode::Vbr2Pass,
        TranscodeMode::Cbr => RateMode::Cbr,
    }
}

/// What besides the format and rates makes a clip's encode another one: its quality, rendering and way of keeping
/// the rate.
fn clip_coding(asset: &disccraft_project::Asset) -> ClipCoding {
    (clip_quality(asset), clip_rendering(asset), clip_mode(asset))
}

/// A clip's quality, rendering and way of keeping the rate ([`clip_coding`]).
type ClipCoding = (
    u8,
    disccraft_transcode::Rendering,
    disccraft_transcode::RateMode,
);

/// The target and peak bit rates (kbit/s) a clip of `asset` is encoded at: the asset's own Blu-ray transcode
/// setting (Encore's Blu-ray quality preset: its target and peak), else the disc's `rates`.
fn clip_rates(asset: &disccraft_project::Asset, rates: (u32, u32)) -> (u32, u32) {
    match &asset.bluray_transcode {
        Some(setting) => {
            let kbps = setting.kbps.clamp(
                disccraft_project::MIN_TRANSCODE_KBPS,
                disccraft_project::MAX_TRANSCODE_KBPS,
            );
            let peak = setting
                .max_kbps
                .clamp(kbps, disccraft_project::MAX_TRANSCODE_KBPS);
            (kbps, peak)
        }
        None => rates,
    }
}

/// A gap of `gap` frames of the disc's standard rate, as frames of the HD rate (the nearest count, at least one).
fn gap_frames(gap: u32, standard: (u32, u32), hd: (u32, u32)) -> u64 {
    let numerator = u128::from(gap) * u128::from(standard.1) * u128::from(hd.0);
    let denominator = (u128::from(standard.0) * u128::from(hd.1)).max(1);
    u64::try_from((numerator + denominator / 2) / denominator)
        .unwrap_or(u64::MAX)
        .max(1)
}

/// `frames` black pictures in the HD format, encoded once and kept in the cache (by their number and the format).
fn black_part(
    cache_dir: &Path,
    frames: u64,
    format: &str,
    options: &HdOptions,
) -> Result<(HdSource, Vec<AccessUnit>), BdError> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    "black".hash(&mut hasher);
    frames.hash(&mut hasher);
    format.hash(&mut hasher);
    options.target_kbps.hash(&mut hasher);
    options.max_kbps.hash(&mut hasher);
    let key = hasher.finish();
    let es = cache_dir.join(format!("hd-{key:016x}.h264"));
    let index = cache_dir.join(format!("hd-{key:016x}.idx"));
    if !(es.is_file() && index.is_file()) {
        std::fs::create_dir_all(cache_dir).map_err(|error| io_error(cache_dir, error))?;
        let es_part = es.with_extension("h264.part");
        let index_part = es.with_extension("idx.part");
        let _ = disccraft_transcode::hd::encode_h264_black(frames, &es_part, &index_part, options)
            .map_err(|error| BdError::Prepare(error.to_string()))?;
        std::fs::rename(&es_part, &es).map_err(|error| io_error(&es, error))?;
        std::fs::rename(&index_part, &index).map_err(|error| io_error(&index, error))?;
    }
    let units = read_h264_index(&index).map_err(|error| BdError::Prepare(error.to_string()))?;
    Ok((HdSource::File(es), units))
}

/// The time of a trim point: `frame` frames of the disc's standard rate (a clip's trims count those).
fn trim_tick(frame: u32, rate: (u32, u32)) -> Result<Tick, BdError> {
    if rate.0 == 0 {
        return Err(invalid("frame rate is zero"));
    }
    let ticks = i128::from(frame)
        .checked_mul(i128::from(TICKS_PER_SECOND))
        .and_then(|value| value.checked_mul(i128::from(rate.1)))
        .map(|value| value / i128::from(rate.0))
        .and_then(|value| i64::try_from(value).ok())
        .ok_or_else(|| invalid("trim point out of range"))?;
    Ok(Tick(ticks))
}

/// The frame number of a 90 kHz time at `rate`, rounded to the nearest frame (times are whole frames rounded down).
fn frame_of_90k(time: i64, rate: (u32, u32)) -> Option<i64> {
    let denominator = i128::from(rate.1).checked_mul(90_000)?;
    if denominator == 0 {
        return None;
    }
    let numerator = i128::from(time).checked_mul(i128::from(rate.0))?;
    let frame = numerator
        .checked_mul(2)?
        .checked_add(denominator)?
        .div_euclid(denominator.checked_mul(2)?);
    i64::try_from(frame).ok()
}

/// The 90 kHz time of a frame number at `rate`, rounded down.
fn frame_to_90k(frame: i64, rate: (u32, u32)) -> Option<i64> {
    if rate.0 == 0 {
        return None;
    }
    let value = i128::from(frame)
        .checked_mul(90_000)?
        .checked_mul(i128::from(rate.1))?
        .div_euclid(i128::from(rate.0));
    i64::try_from(value).ok()
}

/// Encoded clips one after another as one stream: each clip's file with the offset it starts at, and its access
/// units moved behind the bytes and the pictures before it (times stay on the frame grid of `rate`).
fn join_hd_clips(
    parts: Vec<(HdSource, Vec<AccessUnit>)>,
    rate: (u32, u32),
) -> Result<(Vec<(u64, usize, HdSource)>, Vec<AccessUnit>), BdError> {
    let mut streams = Vec::with_capacity(parts.len());
    let mut units = Vec::new();
    let mut base = 0_u64;
    let mut frames_before = 0_i64;
    for (path, clip_units) in parts {
        let moved = |time: i64| {
            frame_of_90k(time, rate)
                .and_then(|frame| frame.checked_add(frames_before))
                .and_then(|frame| frame_to_90k(frame, rate))
                .ok_or_else(|| invalid("HD time stamp out of range"))
        };
        // The clip takes the bytes its units reach to.
        let first = units.len();
        let mut end = base;
        for unit in &clip_units {
            let offset = unit
                .offset
                .checked_add(base)
                .ok_or_else(|| invalid("HD unit offset too large"))?;
            end = end.max(
                offset
                    .checked_add(u64::from(unit.len))
                    .ok_or_else(|| invalid("HD unit offset too large"))?,
            );
            units.push(AccessUnit {
                offset,
                len: unit.len,
                pts: moved(unit.pts)?,
                dts: moved(unit.dts)?,
                keyframe: unit.keyframe,
            });
        }
        frames_before = i64::try_from(clip_units.len())
            .ok()
            .and_then(|count| frames_before.checked_add(count))
            .ok_or_else(|| invalid("too many HD pictures"))?;
        // A clip without pictures has no bytes to find.
        if end > base {
            streams.push((base, first, path));
        }
        base = end;
    }
    Ok((streams, units))
}

/// Whether the HD video of every timeline clip that plays `asset` is in `cache_dir`, encoded for the project as
/// it is now (its format, its bitrates, the clips' trims): a build would encode none of it again. `false` for an
/// asset no timeline plays as video, and for a project that is not an HD Blu-ray.
pub(crate) fn hd_transcoded(
    project: &Project,
    cache_dir: &Path,
    asset: disccraft_project::AssetId,
) -> bool {
    let Ok(Some(budget)) = disccraft_dvd::hd_budget(project) else {
        return false;
    };
    // In its own format (M6.325), at that format's rates.
    let format = project.asset_bd_video(asset);
    let rates = budget.rates_for(format);
    // At the asset's own bit rate, when it has one.
    let rates = project
        .assets
        .get(&asset)
        .map_or(rates, |found| clip_rates(found, rates));
    let coding = project.assets.get(&asset).map_or(
        (
            disccraft_transcode::BEST_QUALITY,
            Default::default(),
            Default::default(),
        ),
        clip_coding,
    );
    let Some(media) = project.assets.get(&asset).map(|asset| asset.media_file()) else {
        return false;
    };
    let source: &Path = &media;
    let mut clips = 0usize;
    for timeline in project.timelines.values() {
        for (clip, video) in timeline.videos().into_iter().enumerate() {
            if video != asset {
                continue;
            }
            clips += 1;
            let cached = cached_paths(
                cache_dir,
                source,
                (format.id(), rates, coding),
                timeline.video_trim(clip),
            )
            .is_ok_and(|(es, index)| es.is_file() && index.is_file());
            if !cached {
                return false;
            }
        }
    }
    clips > 0
}

/// Removes from `cache_dir` the HD video of every timeline clip that plays `asset` (see [`hd_transcoded`]);
/// returns how many files went.
pub(crate) fn revert_hd(
    project: &Project,
    cache_dir: &Path,
    asset: disccraft_project::AssetId,
) -> usize {
    let Ok(Some(budget)) = disccraft_dvd::hd_budget(project) else {
        return 0;
    };
    let format = project.asset_bd_video(asset);
    let rates = budget.rates_for(format);
    // At the asset's own bit rate, when it has one.
    let rates = project
        .assets
        .get(&asset)
        .map_or(rates, |found| clip_rates(found, rates));
    let coding = project.assets.get(&asset).map_or(
        (
            disccraft_transcode::BEST_QUALITY,
            Default::default(),
            Default::default(),
        ),
        clip_coding,
    );
    let Some(media) = project.assets.get(&asset).map(|asset| asset.media_file()) else {
        return 0;
    };
    let source: &Path = &media;
    let mut removed = 0usize;
    for timeline in project.timelines.values() {
        for (clip, video) in timeline.videos().into_iter().enumerate() {
            if video != asset {
                continue;
            }
            let Ok((es, index)) = cached_paths(
                cache_dir,
                source,
                (format.id(), rates, coding),
                timeline.video_trim(clip),
            ) else {
                continue;
            };
            for file in [es, index] {
                if file.is_file() && std::fs::remove_file(&file).is_ok() {
                    removed += 1;
                }
            }
        }
    }
    removed
}

/// Where the HD encode of `source` in `format` is kept (of its trimmed part when `trim` is not the whole clip).
fn cached_paths(
    cache_dir: &Path,
    source: &Path,
    (format, rates, (quality, rendering, mode)): (&str, (u32, u32), ClipCoding),
    trim: disccraft_project::ClipTrim,
) -> Result<(PathBuf, PathBuf), BdError> {
    let metadata =
        disccraft_transcode::document_metadata(source).map_err(|error| io_error(source, error))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_secs());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    modified.hash(&mut hasher);
    format.hash(&mut hasher);
    if trim != disccraft_project::ClipTrim::default() {
        trim.start.hash(&mut hasher);
        trim.end.hash(&mut hasher);
    }
    // Interpret Footage: pixels of another shape make another picture.
    if let Some(aspect) = disccraft_transcode::chosen_pixel_aspect(source) {
        "par".hash(&mut hasher);
        aspect.hash(&mut hasher);
    }
    // The encode's version (2: frame counts round to the nearest frame).
    2u8.hash(&mut hasher);
    rates.0.hash(&mut hasher);
    rates.1.hash(&mut hasher);
    // A lower quality makes another MPEG-2 stream (the best leaves the keys as they were).
    if quality != disccraft_transcode::BEST_QUALITY {
        "quality".hash(&mut hasher);
        quality.hash(&mut hasher);
    }
    // So do Encore's rendering checkboxes (both off leaves the keys as they were).
    if rendering.max_quality {
        "max-render-quality".hash(&mut hasher);
    }
    if rendering.frame_blend {
        "frame-blend".hash(&mut hasher);
    }
    // So do two passes or a constant rate (one-pass VBR leaves the keys as they were).
    match mode {
        disccraft_transcode::RateMode::Vbr => {}
        disccraft_transcode::RateMode::Vbr2Pass => "vbr2pass".hash(&mut hasher),
        disccraft_transcode::RateMode::Cbr => "cbr".hash(&mut hasher),
    }
    let key = hasher.finish();
    Ok((
        cache_dir.join(format!("hd-{key:016x}.h264")),
        cache_dir.join(format!("hd-{key:016x}.idx")),
    ))
}

#[cfg(test)]
mod tests {
    use super::{HdSource, HdTitle, MENU_H264_KBPS, coding, coding_of, kind, kind_of, options};
    use disccraft_bdmv::StreamCoding;
    use disccraft_m2tsmux::StreamKind;
    use disccraft_project::{BdCodec, BdVideo};
    use disccraft_transcode::hd::HdCodec;

    #[test]
    fn h264_that_went_on_a_1080i_disc_as_it_is_is_declared_h264() {
        assert_eq!(
            coding_of(BdVideo::Hd1080i59, HdCodec::H264),
            Some(StreamCoding::H264Video {
                video_format: 4,
                frame_rate: 4,
                aspect: 3,
            })
        );
        assert_eq!(kind_of(HdCodec::H264), StreamKind::H264Video);
        assert_eq!(
            kind(BdCodec::Auto, BdVideo::Hd1080i59),
            StreamKind::Mpeg2Video
        );
    }

    fn title(rate: (u32, u32), height: u16) -> HdTitle {
        HdTitle {
            video: BdVideo::Hd1080p23,
            codec: HdCodec::H264,
            streams: Vec::new(),
            units: Vec::new(),
            width: 1920,
            height,
            rate,
            subtitles: Vec::new(),
        }
    }

    #[test]
    fn every_hd_format_has_its_codes() {
        assert_eq!(coding(BdCodec::Auto, BdVideo::Sd), None);
        for video in BdVideo::ALL {
            let Some((_, height, num, den)) = video.hd() else {
                continue;
            };
            let expected_rate = match (num, den) {
                (24_000, 1001) => 1,
                (24, 1) => 2,
                (25, 1) => 3,
                (30_000, 1001) => 4,
                (50, 1) => 6,
                _ => 7,
            };
            let expected_format = match (video.interlaced(), height) {
                (true, _) => 4,
                (false, 1080) => 6,
                _ => 5,
            };
            // An interlaced format is MPEG-2 everywhere (encoder, multiplexer, clip attributes); the others H.264.
            let (format, rate, mpeg2) = match coding(BdCodec::Auto, video) {
                Some(StreamCoding::Mpeg2Video {
                    video_format,
                    frame_rate,
                    aspect: 3,
                }) => (video_format, frame_rate, true),
                Some(StreamCoding::H264Video {
                    video_format,
                    frame_rate,
                    aspect: 3,
                }) => (video_format, frame_rate, false),
                other => panic!("{video:?}: {other:?}"),
            };
            assert_eq!(
                (format, rate),
                (expected_format, expected_rate),
                "{video:?}"
            );
            assert_eq!(mpeg2, video.interlaced(), "{video:?}");
            assert_eq!(
                kind(BdCodec::Auto, video) == StreamKind::Mpeg2Video,
                video.interlaced(),
                "{video:?}"
            );
            let rates = disccraft_dvd::hd_default_kbps(video.interlaced());
            let options = options(BdCodec::Auto, video, rates).unwrap();
            assert_eq!(options.codec == HdCodec::Mpeg2, video.interlaced());
            // Within a Blu-ray's 40 Mbps of video, the menus too.
            for (target, max) in [rates, super::menu_rates(BdCodec::Auto, video)] {
                assert!(target <= max && max <= 40_000, "{video:?}");
            }
        }
    }

    #[test]
    fn interlaced_formats_are_1080i_mpeg2() {
        assert_eq!(
            coding(BdCodec::Auto, BdVideo::Hd1080i59),
            Some(StreamCoding::Mpeg2Video {
                video_format: 4,
                frame_rate: 4,
                aspect: 3
            })
        );
        assert_eq!(
            coding(BdCodec::Auto, BdVideo::Hd1080i50),
            Some(StreamCoding::Mpeg2Video {
                video_format: 4,
                frame_rate: 3,
                aspect: 3
            })
        );
        assert_eq!(
            super::menu_rates(BdCodec::Auto, BdVideo::Hd1080i59),
            (25_000, 35_000)
        );
        assert_eq!(
            super::menu_rates(BdCodec::Auto, BdVideo::Hd1080p23),
            (15_000, 30_000)
        );
    }

    /// M12.28: with the disc's codec set to H.264, 1080i is H.264 coded as interlaced frames (the clip says H.264,
    /// 1080i); MPEG-2 and the automatic choice keep MPEG-2 for 1080i; the progressive formats are H.264 whatever
    /// the choice.
    #[test]
    fn the_disc_codec_chooses_1080i_h264_or_mpeg2() {
        let h264 = options(BdCodec::H264, BdVideo::Hd1080i59, (20_000, 30_000)).unwrap();
        assert_eq!(h264.codec, HdCodec::H264);
        assert!(h264.interlaced);
        assert_eq!(
            kind(BdCodec::H264, BdVideo::Hd1080i50),
            StreamKind::H264Video
        );
        assert_eq!(
            coding(BdCodec::H264, BdVideo::Hd1080i59),
            Some(StreamCoding::H264Video {
                video_format: 4,
                frame_rate: 4,
                aspect: 3
            })
        );
        assert_eq!(
            super::menu_rates(BdCodec::H264, BdVideo::Hd1080i59),
            MENU_H264_KBPS
        );
        for choice in [BdCodec::Auto, BdCodec::Mpeg2] {
            let mpeg2 = options(choice, BdVideo::Hd1080i59, (20_000, 30_000)).unwrap();
            assert_eq!(mpeg2.codec, HdCodec::Mpeg2);
            assert!(!mpeg2.interlaced);
        }
        for choice in BdCodec::ALL {
            let progressive = options(choice, BdVideo::Hd1080p23, (20_000, 30_000)).unwrap();
            assert_eq!(
                (progressive.codec, progressive.interlaced),
                (HdCodec::H264, false)
            );
        }
    }

    /// Clips are joined without being copied: each unit is read from its own clip's file, and the second clip's
    /// times follow the first's pictures.
    #[test]
    fn joined_clips_are_read_from_their_own_files() {
        use disccraft_m2tsmux::UnitData;
        use disccraft_transcode::hd::AccessUnit;
        let dir = std::env::temp_dir().join(format!("disccraft-hd-join-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (first, second) = (dir.join("a.h264"), dir.join("b.h264"));
        std::fs::write(&first, b"AAAABBB").unwrap();
        std::fs::write(&second, b"CCDDDDD").unwrap();
        let unit = |offset, len, frame: i64, keyframe| AccessUnit {
            offset,
            len,
            pts: frame * 3003,
            dts: frame * 3003,
            keyframe,
        };
        let parts = vec![
            (
                HdSource::File(first),
                vec![unit(0, 4, 0, true), unit(4, 3, 1, false)],
            ),
            (HdSource::File(dir.join("empty.h264")), Vec::new()),
            (
                HdSource::File(second),
                vec![unit(0, 2, 0, true), unit(2, 5, 1, false)],
            ),
        ];
        let (streams, units) = super::join_hd_clips(parts, (30_000, 1001)).unwrap();
        assert_eq!(
            streams
                .iter()
                .map(|(base, first, _)| (*base, *first))
                .collect::<Vec<_>>(),
            [(0, 0), (7, 2)]
        );
        assert_eq!(
            units.iter().map(|u| (u.offset, u.pts)).collect::<Vec<_>>(),
            [(0, 0), (4, 3003), (7, 6006), (9, 9009)]
        );
        let mut hd = title((30_000, 1001), 1080);
        hd.streams = streams;
        hd.units = units;
        let reader = hd.reader().unwrap();
        let read: Vec<Vec<u8>> = (0..4).map(|unit| reader.read(unit).unwrap()).collect();
        assert!(reader.read(4).is_err());
        drop(reader);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(read, [&b"AAAA"[..], b"BBB", b"CC", b"DDDDD"]);
    }

    #[test]
    fn frames_have_their_length() {
        assert_eq!(title((24_000, 1001), 1080).frame_90k(), 3753);
        assert_eq!(title((30_000, 1001), 1080).frame_90k(), 3003);
        assert_eq!(title((24, 1), 1080).frame_90k(), 3750);
    }
}
