//! H.264 in a Matroska file (`.mkv`) that can go on a Blu-ray as it is: its length-prefixed NAL units are written
//! to the build cache as an Annex B stream with access unit delimiters and parameter sets, and its times (Matroska
//! keeps presentation times only, often rounded to milliseconds) are put on the frame grid with decode times.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use craft_media::ReadSeekSource;
use craft_media::matroska::{Codec, Demuxer, OpenOptions, TrackKind};
use disccraft_transcode::hd::AccessUnit;

use crate::DvdError;
use crate::demux::{BdVideoFormat, sps_fits};
use crate::h264_sps;
use crate::prepare::{audio_cache_key, part_number};

/// The most parameter-set bytes taken from a track's configuration record.
const MAX_PARAMETER_SETS: usize = 64 * 1024;
/// The most pictures of one file (a day at 60 a second).
const MAX_PICTURES: usize = 60 * 60 * 24 * 60;

/// An access unit delimiter that allows every slice type (7.3.2.4: primary_pic_type 7).
const DELIMITER: [u8; 6] = [0, 0, 0, 1, 9, 0xF0];

/// What an `avcC` record says: the bytes of each NAL's length, and its parameter sets as Annex B.
pub(crate) struct Config {
    pub length_size: usize,
    pub parameter_sets: Vec<u8>,
    pub sps: Option<h264_sps::Sps>,
}

/// Reads an AVCDecoderConfigurationRecord (ISO/IEC 14496-15 5.2.4.1).
fn config(avcc: &[u8]) -> Option<Config> {
    if *avcc.first()? != 1 {
        return None;
    }
    let length_size = usize::from(*avcc.get(4)? & 3) + 1;
    let mut at = 5usize;
    let mut parameter_sets = Vec::new();
    let mut sps = None;
    // The sequence parameter sets (a 5-bit count), then the picture parameter sets (an 8-bit count).
    for mask in [0x1Fu8, 0xFF] {
        let count = *avcc.get(at)? & mask;
        at = at.checked_add(1)?;
        for _ in 0..count {
            let len = usize::from(u16::from_be_bytes([*avcc.get(at)?, *avcc.get(at + 1)?]));
            at = at.checked_add(2)?;
            let nal = avcc.get(at..at.checked_add(len)?)?;
            at = at.checked_add(len)?;
            if parameter_sets.len().saturating_add(len) > MAX_PARAMETER_SETS {
                return None;
            }
            if sps.is_none() && nal.first().is_some_and(|header| header & 0x1F == 7) {
                sps = nal.get(1..).and_then(h264_sps::parse);
            }
            parameter_sets.extend_from_slice(&[0, 0, 0, 1]);
            parameter_sets.extend_from_slice(nal);
        }
    }
    Some(Config {
        length_size,
        parameter_sets,
        sps,
    })
}

/// The NAL units of a length-prefixed sample; `None` when a length runs past the end.
pub(crate) fn nals(sample: &[u8], length_size: usize) -> Option<Vec<&[u8]>> {
    let mut found = Vec::new();
    let mut at = 0usize;
    while at < sample.len() {
        let prefix = sample.get(at..at.checked_add(length_size)?)?;
        let len = prefix
            .iter()
            .fold(0usize, |len, byte| len << 8 | usize::from(*byte));
        at = at.checked_add(length_size)?;
        let nal = sample.get(at..at.checked_add(len)?)?;
        at = at.checked_add(len)?;
        if !nal.is_empty() {
            found.push(nal);
        }
    }
    Some(found)
}

pub(crate) fn nal_type(nal: &[u8]) -> u8 {
    nal.first().map_or(0, |header| header & 0x1F)
}

/// A coded slice (1, or 5 of an IDR picture) that starts a picture: first_mb_in_slice is 0, whose Exp-Golomb code
/// is a single 1 bit.
fn starts_picture(nal: &[u8]) -> bool {
    matches!(nal_type(nal), 1 | 5) && nal.get(1).is_some_and(|byte| byte & 0x80 != 0)
}

/// One Matroska sample as Annex B: every picture in it (a frame, or the two fields of one) led by an access unit
/// delimiter, and a key frame without parameter sets of its own given the track's.
fn annex_b(sample: &[u8], config: &Config, keyframe: bool) -> Option<Vec<u8>> {
    annex_b_with(sample, config, &config.parameter_sets, keyframe)
}

/// [`annex_b`] with the track's parameter sets given apart.
fn annex_b_with(
    sample: &[u8],
    config: &Config,
    parameter_sets: &[u8],
    keyframe: bool,
) -> Option<Vec<u8>> {
    let nals = nals(sample, config.length_size)?;
    // Where a delimiter is missing: in front of the sample, and in front of a second picture's first slice, ahead
    // of the supplemental and parameter NALs that lead it.
    let mut delimit = vec![false; nals.len()];
    let mut pictures = 0usize;
    for (index, nal) in nals.iter().enumerate() {
        if !starts_picture(nal) {
            continue;
        }
        pictures += 1;
        let mut lead = index;
        while lead > 0
            && nals
                .get(lead - 1)
                .is_some_and(|before| matches!(nal_type(before), 6..=8 | 14..=18))
        {
            lead -= 1;
        }
        let delimited = lead > 0 && nals.get(lead - 1).is_some_and(|nal| nal_type(nal) == 9);
        if !delimited && (pictures > 1 || lead == 0) {
            *delimit.get_mut(lead)? = true;
        }
    }
    let has_sps = nals.iter().any(|nal| nal_type(nal) == 7);
    let mut out = Vec::with_capacity(sample.len().saturating_add(64));
    let mut sets_pending = keyframe && !has_sps;
    for (index, nal) in nals.iter().enumerate() {
        if delimit.get(index).copied().unwrap_or(false) {
            out.extend_from_slice(&DELIMITER);
        }
        // The parameter sets go right after the first delimiter.
        if sets_pending && nal_type(nal) != 9 {
            out.extend_from_slice(parameter_sets);
            sets_pending = false;
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(nal);
    }
    Some(out)
}

/// The frame number at `rate` of a time in nanoseconds (the nearest: Matroska times are rounded).
fn frame_of_ns(time_ns: i64, rate: (u32, u32)) -> Option<i64> {
    let denominator = i128::from(rate.1).checked_mul(1_000_000_000)?;
    if denominator == 0 {
        return None;
    }
    let numerator = i128::from(time_ns).checked_mul(i128::from(rate.0))?;
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

/// The presentation and decode times (90 kHz, the first shown picture at 0) of pictures given in decode order by
/// their frame numbers. Each is decoded when the picture that many places into the display order is shown, less
/// the delay the stream's reordering needs, so decode times rise evenly and never pass a picture's own time.
/// `None` when two pictures share a frame, or the first is not the first shown.
pub(crate) fn times(frames: &[i64], rate: (u32, u32)) -> Option<Vec<(i64, i64)>> {
    let mut shown = frames.to_vec();
    shown.sort_unstable();
    if shown.array_windows().any(|[a, b]| a >= b) {
        return None;
    }
    let origin = *shown.first()?;
    if *frames.first()? != origin {
        return None;
    }
    let delay = shown
        .iter()
        .zip(frames)
        .map(|(shown, own)| shown.saturating_sub(*own))
        .max()?
        .max(0);
    frames
        .iter()
        .zip(&shown)
        .map(|(own, shown)| {
            let pts = frame_to_90k(own.checked_sub(origin)?, rate)?;
            let dts = frame_to_90k(shown.checked_sub(delay)?.checked_sub(origin)?, rate)?;
            Some((pts, dts))
        })
        .collect()
}

/// An opened Matroska file's H.264 track, when a Blu-ray title of `format` can carry it as it is.
struct Fit {
    demuxer: Demuxer<ReadSeekSource<fs::File>>,
    track: usize,
    config: Config,
    bytes: u64,
}

/// Opens `path` (its header only: no index is built) and checks its first video track against `format`.
fn fit(path: &Path, format: BdVideoFormat) -> Option<Fit> {
    let file = fs::File::open(path).ok()?;
    let bytes = file.metadata().ok()?.len();
    let source = ReadSeekSource::new(file).ok()?;
    let options = OpenOptions {
        index: false,
        verify_crc: false,
        index_limit: None,
    };
    let demuxer = Demuxer::with_options(source, options).ok()?;
    let track = demuxer.file().track_of_kind(TrackKind::Video)?;
    let info = demuxer.tracks().get(track)?;
    let Codec::Avc { avcc } = &info.codec else {
        return None;
    };
    // Compressed or encrypted frames are not read.
    if !info.encodings.is_empty() {
        return None;
    }
    let config = config(avcc)?;
    if !sps_fits(config.sps.as_ref()?, format) {
        return None;
    }
    // The frame rate, from the track's frame duration.
    let duration = info.default_duration_ns?;
    let (num, den) = format.rate;
    let expected = 1e9 * f64::from(den) / f64::from(num.max(1));
    if (duration as f64 - expected).abs() > expected * 0.005 {
        return None;
    }
    // No faster than a Blu-ray's video on average, the whole file counted as video.
    let seconds = demuxer.duration_ns()? as f64 / 1e9;
    if seconds <= 0.0 || bytes as f64 * 8.0 / seconds > crate::demux::BD_MAX_VIDEO_BITRATE {
        return None;
    }
    Some(Fit {
        demuxer,
        track,
        config,
        bytes,
    })
}

/// Whether `path` is a Matroska file whose H.264 a Blu-ray title of `format` carries as it is (from its header:
/// the pictures themselves are checked when they are taken out).
pub(crate) fn fits(path: &Path, format: BdVideoFormat) -> bool {
    is_matroska(path) && fit(path, format).is_some()
}

/// Whether the file starts with an EBML header.
pub(crate) fn is_matroska(path: &Path) -> bool {
    let mut head = [0u8; 4];
    fs::File::open(path)
        .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut head))
        .is_ok()
        && head == [0x1A, 0x45, 0xDF, 0xA3]
}

impl Pictures for Fit {
    fn config(&self) -> &Config {
        &self.config
    }

    fn bytes(&self) -> u64 {
        self.bytes
    }

    fn next(&mut self) -> Result<Option<Picture>, String> {
        loop {
            let Some(packet) = self.demuxer.next_packet().map_err(|err| err.to_string())? else {
                return Ok(None);
            };
            if packet.track == self.track {
                return Ok(Some(Picture {
                    data: packet.data,
                    keyframe: packet.keyframe,
                    time_ns: packet.pts_ns,
                    offset: packet.offset,
                }));
            }
        }
    }
}

/// [`crate::demux_h264_for_bd`] of a Matroska file.
pub(crate) fn demux_h264(
    path: &Path,
    cache_dir: &Path,
    format: BdVideoFormat,
    progress: &mut dyn FnMut(f32),
) -> Result<Option<H264Samples>, DvdError> {
    let mut open = || fit(path, format).map(|fit| Box::new(fit) as Box<dyn Pictures>);
    demux_pictures(path, cache_dir, format, &mut open, progress)
}

/// One coded picture (a frame, or both fields of one) as a container holds it: length-prefixed NAL units.
pub(crate) struct Picture {
    pub data: Vec<u8>,
    /// Where a player can start.
    pub keyframe: bool,
    /// When it is shown, nanoseconds.
    pub time_ns: i64,
    /// Where `data` is in the file.
    pub offset: u64,
}

/// The H.264 pictures of an opened file, in decode order.
pub(crate) trait Pictures {
    fn config(&self) -> &Config;
    /// The file's size.
    fn bytes(&self) -> u64;
    /// The next picture; an error when the file cannot be read.
    fn next(&mut self) -> Result<Option<Picture>, String>;
}

/// The H.264 pictures of a Matroska or MP4 file that go on a Blu-ray as they are. They stay in the file: each is
/// read from it and made an Annex B access unit ([`annex_b`]) as the disc's clip is written, so hours of video are
/// not written to the cache first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct H264Samples {
    path: PathBuf,
    length_size: usize,
    parameter_sets: Vec<u8>,
    /// Each picture's place in the file: offset and length.
    samples: Vec<(u64, u32)>,
    /// The access units they become: offsets in the Annex B stream, lengths, times (90 kHz from 0), key frames.
    units: Vec<AccessUnit>,
}

/// What the index of a file's pictures starts with (its version in the last byte).
const SAMPLES_MAGIC: &[u8; 8] = b"DCH264S\x01";
/// Bytes of one picture in the index: where it is (8 + 4), its unit's length (4), times (8 + 8), key frame (1).
const SAMPLE_ENTRY: usize = 33;

impl H264Samples {
    /// The file the pictures are in.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The access units the pictures become.
    #[must_use]
    pub fn units(&self) -> &[AccessUnit] {
        &self.units
    }

    /// Access unit `unit`, read from `file` (the opened [`Self::path`]).
    ///
    /// # Errors
    ///
    /// When it cannot be read, or the file is no longer what was indexed.
    pub fn read(&self, file: &mut fs::File, unit: usize) -> std::io::Result<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let changed = || std::io::Error::other(format!("{} has changed", self.path.display()));
        let (Some(&(offset, len)), Some(wanted)) = (self.samples.get(unit), self.units.get(unit))
        else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("no unit {unit}"),
            ));
        };
        file.seek(SeekFrom::Start(offset))?;
        let mut sample = vec![0u8; len as usize];
        file.read_exact(&mut sample)?;
        let config = Config {
            length_size: self.length_size,
            parameter_sets: Vec::new(),
            sps: None,
        };
        let bytes = annex_b_with(&sample, &config, &self.parameter_sets, wanted.keyframe)
            .ok_or_else(changed)?;
        if bytes.len() != wanted.len as usize {
            return Err(changed());
        }
        Ok(bytes)
    }

    /// Writes the index, as [`Self::load`] reads it.
    fn store(&self, out: &Path) -> std::io::Result<()> {
        let mut writer = std::io::BufWriter::new(fs::File::create(out)?);
        writer.write_all(SAMPLES_MAGIC)?;
        writer.write_all(&[self.length_size as u8])?;
        writer.write_all(&(self.parameter_sets.len() as u32).to_le_bytes())?;
        writer.write_all(&self.parameter_sets)?;
        writer.write_all(&(self.samples.len() as u64).to_le_bytes())?;
        for (&(offset, len), unit) in self.samples.iter().zip(&self.units) {
            writer.write_all(&offset.to_le_bytes())?;
            writer.write_all(&len.to_le_bytes())?;
            writer.write_all(&unit.len.to_le_bytes())?;
            writer.write_all(&unit.pts.to_le_bytes())?;
            writer.write_all(&unit.dts.to_le_bytes())?;
            writer.write_all(&[u8::from(unit.keyframe)])?;
        }
        writer.flush()
    }

    /// Reads an index of `path`'s pictures; `None` when it is not one (cut short, another version).
    fn load(index: &Path, path: &Path) -> Option<Self> {
        let bytes = fs::read(index).ok()?;
        let rest = bytes.strip_prefix(SAMPLES_MAGIC)?;
        let (&length_size, rest) = rest.split_first()?;
        let (sets_len, rest) = rest.split_first_chunk::<4>()?;
        let sets_len = u32::from_le_bytes(*sets_len) as usize;
        if !(1..=4).contains(&length_size) || sets_len > MAX_PARAMETER_SETS {
            return None;
        }
        let (parameter_sets, rest) = rest.split_at_checked(sets_len)?;
        let (count, rest) = rest.split_first_chunk::<8>()?;
        let count = usize::try_from(u64::from_le_bytes(*count)).ok()?;
        if count > MAX_PICTURES || rest.len() != count.checked_mul(SAMPLE_ENTRY)? {
            return None;
        }
        let mut samples = Vec::with_capacity(count);
        let mut units = Vec::with_capacity(count);
        let mut at = 0u64;
        for entry in rest.as_chunks::<SAMPLE_ENTRY>().0 {
            let field = |from: usize, len: usize| entry.get(from..from.checked_add(len)?);
            let offset = u64::from_le_bytes(field(0, 8)?.try_into().ok()?);
            let len = u32::from_le_bytes(field(8, 4)?.try_into().ok()?);
            let unit_len = u32::from_le_bytes(field(12, 4)?.try_into().ok()?);
            samples.push((offset, len));
            units.push(AccessUnit {
                offset: at,
                len: unit_len,
                pts: i64::from_le_bytes(field(16, 8)?.try_into().ok()?),
                dts: i64::from_le_bytes(field(24, 8)?.try_into().ok()?),
                keyframe: *entry.get(32)? != 0,
            });
            at = at.checked_add(u64::from(unit_len))?;
        }
        Some(Self {
            path: path.to_path_buf(),
            length_size: usize::from(length_size),
            parameter_sets: parameter_sets.to_vec(),
            samples,
            units,
        })
    }
}

/// Reads through the pictures of `path`, opened by `open` (`None` when a Blu-ray title of `format` cannot carry
/// them), and keeps where each is and the access unit it becomes in `cache_dir`. `None` when the file does not
/// fit; that the pictures themselves did not is remembered in the cache, so the file is not read through again.
pub(crate) fn demux_pictures(
    path: &Path,
    cache_dir: &Path,
    format: BdVideoFormat,
    open: &mut dyn FnMut() -> Option<Box<dyn Pictures>>,
    progress: &mut dyn FnMut(f32),
) -> Result<Option<H264Samples>, DvdError> {
    fs::create_dir_all(cache_dir).map_err(|err| crate::io_error(cache_dir, err))?;
    let key = format!("{}-{}", audio_cache_key(path)?, format.key());
    let index_path = cache_dir.join(format!("{key}.passthrough.src"));
    // What a file that was read and found unfit leaves, so it is not read through again.
    let refused = cache_dir.join(format!("{key}.passthrough.no"));
    if let Some(known) = H264Samples::load(&index_path, path) {
        return Ok(Some(known));
    }
    if refused.is_file() {
        return Ok(None);
    }
    let Some(mut source) = open() else {
        return Ok(None);
    };

    let unsupported =
        |what: String| DvdError::Unsupported(format!("demux {}: {what}", path.display()));
    let mut scan = || -> Result<Option<H264Samples>, DvdError> {
        // Each picture's place in the file, its unit's length, its frame number and whether it is a key frame.
        let mut pictures: Vec<(u64, u32, u32, i64, bool)> = Vec::new();
        let mut reported = 0u64;
        let total = source.bytes().max(1);
        while let Some(picture) = source.next().map_err(unsupported)? {
            let Some(bytes) = annex_b(&picture.data, source.config(), picture.keyframe) else {
                return Ok(None);
            };
            // The stream starts with an IDR picture. The key frames after it are where a player can start: IDR
            // pictures, or the I pictures with a recovery point that open the GOPs of a broadcast or disc stream.
            if pictures.is_empty() {
                let idr = bytes
                    .array_windows()
                    .any(|&[a, b, c, nal]| [a, b, c] == [0, 0, 1] && nal & 0x1F == 5);
                if !(picture.keyframe && idr) {
                    return Ok(None);
                }
            }
            let Some(frame) = frame_of_ns(picture.time_ns, format.rate) else {
                return Ok(None);
            };
            let too_large = |_| unsupported("a picture is too large".to_string());
            let len = u32::try_from(picture.data.len()).map_err(too_large)?;
            let unit_len = u32::try_from(bytes.len()).map_err(too_large)?;
            if pictures.len() >= MAX_PICTURES {
                return Ok(None);
            }
            pictures.push((picture.offset, len, unit_len, frame, picture.keyframe));
            if picture.offset.abs_diff(reported) >= 64 << 20 {
                reported = picture.offset;
                progress((picture.offset as f64 / total as f64).min(1.0) as f32);
            }
        }
        let frames: Vec<i64> = pictures.iter().map(|picture| picture.3).collect();
        let Some(times) = times(&frames, format.rate) else {
            return Ok(None);
        };
        let mut offset = 0u64;
        let mut units = Vec::with_capacity(pictures.len());
        for (&(_, _, len, _, keyframe), (pts, dts)) in pictures.iter().zip(times) {
            units.push(AccessUnit {
                offset,
                len,
                pts,
                dts,
                keyframe,
            });
            offset += u64::from(len);
        }
        let config = source.config();
        Ok(Some(H264Samples {
            path: path.to_path_buf(),
            length_size: config.length_size,
            parameter_sets: config.parameter_sets.clone(),
            samples: pictures
                .iter()
                .map(|&(offset, len, ..)| (offset, len))
                .collect(),
            units,
        }))
    };
    let Some(samples) = scan()? else {
        fs::write(&refused, b"no").map_err(|err| crate::io_error(&refused, err))?;
        return Ok(None);
    };
    let part = cache_dir.join(format!("{key}.{}.src.part", part_number()));
    if let Err(err) = samples.store(&part) {
        fs::remove_file(&part).unwrap_or(());
        return Err(crate::io_error(&part, err));
    }
    fs::rename(&part, &index_path).map_err(|err| crate::io_error(&part, err))?;
    // The copy of the video an earlier version kept here is not read any more.
    for old in ["h264", "idx", "ok"] {
        fs::remove_file(cache_dir.join(format!("{key}.passthrough.{old}"))).unwrap_or(());
    }
    progress(1.0);
    Ok(Some(samples))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_times_follow_the_display_order_less_the_reorder_delay() {
        // I P B B P B B in decode order.
        let frames = [0, 3, 1, 2, 6, 4, 5];
        let times = times(&frames, (30_000, 1001));
        let expected: Vec<(i64, i64)> = [(0, -1), (3, 0), (1, 1), (2, 2), (6, 3), (4, 4), (5, 5)]
            .iter()
            .map(|&(pts, dts)| (pts * 3003, dts * 3003))
            .collect();
        assert_eq!(times, Some(expected));
    }

    #[test]
    fn no_reordering_decodes_each_picture_when_it_is_shown() {
        assert_eq!(
            times(&[10, 11, 12], (25, 1)),
            Some(vec![(0, 0), (3600, 3600), (7200, 7200)])
        );
    }

    #[test]
    fn a_repeated_frame_or_a_late_first_picture_is_refused() {
        assert_eq!(times(&[0, 1, 1], (25, 1)), None);
        assert_eq!(times(&[2, 0, 1], (25, 1)), None);
        assert_eq!(times(&[], (25, 1)), None);
    }

    #[test]
    fn millisecond_times_land_on_their_frames() {
        let rate = (30_000, 1001);
        for (ns, frame) in [
            (0, 0),
            (33_000_000, 1),
            (67_000_000, 2),
            (1_001_000_000, 30),
        ] {
            assert_eq!(frame_of_ns(ns, rate), Some(frame));
        }
    }

    fn sample(nals: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for nal in nals {
            out.extend_from_slice(&u32::try_from(nal.len()).unwrap_or(0).to_be_bytes());
            out.extend_from_slice(nal);
        }
        out
    }

    fn types(annex_b: &[u8]) -> Vec<u8> {
        annex_b
            .array_windows()
            .filter(|&&[a, b, c, _]| [a, b, c] == [0, 0, 1])
            .map(|&[_, _, _, nal]| nal & 0x1F)
            .collect()
    }

    fn test_config() -> Config {
        Config {
            length_size: 4,
            parameter_sets: vec![0, 0, 0, 1, 0x67, 100, 0, 41, 0, 0, 0, 1, 0x68, 0xEE],
            sps: None,
        }
    }

    #[test]
    fn a_bare_key_frame_gets_a_delimiter_and_the_parameter_sets() {
        let out = annex_b(&sample(&[&[0x65, 0x88, 1]]), &test_config(), true);
        assert_eq!(out.as_deref().map(types), Some(vec![9, 7, 8, 5]));
    }

    #[test]
    fn a_delimited_sample_with_its_own_sets_is_kept() {
        let input = sample(&[&[9, 0xF0], &[0x67, 1], &[0x68, 2], &[6, 1], &[0x65, 0x88]]);
        let out = annex_b(&input, &test_config(), true);
        assert_eq!(out.as_deref().map(types), Some(vec![9, 7, 8, 6, 5]));
    }

    #[test]
    fn the_second_field_of_a_sample_gets_its_own_delimiter() {
        // Two field pictures (each a first slice and a second), the second led by an SEI.
        let input = sample(&[
            &[9, 0xF0],
            &[0x41, 0x9A],
            &[0x41, 0x1A],
            &[6, 1],
            &[0x41, 0x9A],
            &[0x41, 0x1A],
        ]);
        let out = annex_b(&input, &test_config(), false);
        assert_eq!(out.as_deref().map(types), Some(vec![9, 1, 1, 9, 6, 1, 1]));
    }

    /// Pictures stay in their file: the index says where, and each is read back as the unit it was measured as.
    #[test]
    fn pictures_are_indexed_and_read_from_their_file() {
        struct Two(Config, Vec<Picture>);
        impl Pictures for Two {
            fn config(&self) -> &Config {
                &self.0
            }
            fn bytes(&self) -> u64 {
                100
            }
            fn next(&mut self) -> Result<Option<Picture>, String> {
                Ok((!self.1.is_empty()).then(|| self.1.remove(0)))
            }
        }
        let dir = std::env::temp_dir().join(format!("disccraft-samples-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let cache = dir.join("cache");
        // A file with two samples in it, among other bytes: an IDR slice, then a P slice with its own delimiter.
        let (idr, p) = (
            sample(&[&[0x65, 0x88, 1]]),
            sample(&[&[9, 0xF0], &[0x41, 0x9A]]),
        );
        let mut file = vec![0xEE; 10];
        file.extend_from_slice(&idr);
        file.extend_from_slice(&[0xEE; 5]);
        file.extend_from_slice(&p);
        let video = dir.join("video.bin");
        fs::write(&video, &file).unwrap();
        let format = BdVideoFormat {
            width: 1920,
            height: 1080,
            rate: (24, 1),
            interlaced: false,
        };
        let picture = |data: &Vec<u8>, offset, frame: i64, keyframe| Picture {
            data: data.clone(),
            keyframe,
            time_ns: frame * 1_000_000_000 / 24,
            offset,
        };
        let pictures = || vec![picture(&idr, 10, 0, true), picture(&p, 22, 1, false)];
        let mut opens = 0;
        let mut open = || {
            opens += 1;
            Some(Box::new(Two(test_config(), pictures())) as Box<dyn Pictures>)
        };
        let made = demux_pictures(&video, &cache, format, &mut open, &mut |_| ())
            .unwrap()
            .unwrap();
        // Asked again, the index is read: the file is not gone through a second time.
        let again = demux_pictures(&video, &cache, format, &mut open, &mut |_| ())
            .unwrap()
            .unwrap();
        assert_eq!(opens, 1);
        assert_eq!(made, again);
        assert_eq!(made.units().len(), 2);
        let mut opened = fs::File::open(made.path()).unwrap();
        let first = made.read(&mut opened, 0).unwrap();
        let second = made.read(&mut opened, 1).unwrap();
        assert_eq!(types(&first), [9, 7, 8, 5]);
        assert_eq!(types(&second), [9, 1]);
        assert_eq!(made.units()[1].offset, first.len() as u64);
        assert_eq!(made.units()[1].pts, 3750);
        assert!(made.read(&mut opened, 2).is_err());
        // The file changed under the index: the unit is not what was measured.
        let mut other = file.clone();
        other[10 + 3] = 9; // the length of the first NAL
        fs::write(&video, &other).unwrap();
        let mut opened = fs::File::open(made.path()).unwrap();
        assert!(made.read(&mut opened, 0).is_err());
        // An index cut short is not one.
        let index = fs::read_dir(&cache)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().is_some_and(|ext| ext == "src"))
            .unwrap();
        let bytes = fs::read(&index).unwrap();
        fs::write(&index, &bytes[..bytes.len() - 1]).unwrap();
        assert!(H264Samples::load(&index, &video).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_length_past_the_end_is_refused() {
        assert!(annex_b(&[0, 0, 0, 9, 0x65], &test_config(), true).is_none());
        assert!(annex_b(&[0, 0], &test_config(), true).is_none());
    }

    #[test]
    fn reads_a_configuration_record() {
        let avcc = [
            1, 100, 0, 41, 0xFF, 0xE1, 0, 2, 0x67, 100, 1, 0, 2, 0x68, 0xEE,
        ];
        let config = config(&avcc);
        assert_eq!(config.as_ref().map(|config| config.length_size), Some(4));
        assert_eq!(
            config.map(|config| config.parameter_sets),
            Some(vec![0, 0, 0, 1, 0x67, 100, 0, 0, 0, 1, 0x68, 0xEE])
        );
        for len in 0..avcc.len() {
            let _ = super::config(&avcc[..len]);
        }
    }
}
