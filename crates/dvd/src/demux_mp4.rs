//! H.264 in an MP4 or QuickTime file (`.mp4`, `.m4v`, `.mov`) that can go on a Blu-ray as it is: the samples of
//! its video track (ISO/IEC 14496-12, with the `avcC` record of 14496-15) are read in decode order and written as
//! the pictures of a Matroska file are ([`crate::demux_mkv`]).

use std::fs;
use std::path::Path;

use craft_media::ReadSeekSource;
use craft_media::isobmff::{self, CodecConfig, Mp4File, TrackKind};

use crate::DvdError;
use crate::demux::{BdVideoFormat, sps_fits};
use crate::demux_mkv::{Config, H264Samples, Picture, Pictures, demux_pictures, nal_type, nals};
use disccraft_transcode::h264_sps;

/// The most parameter-set bytes taken from a track's configuration record.
const MAX_PARAMETER_SETS: usize = 64 * 1024;

/// An opened file's H.264 track, when a Blu-ray title of `format` can carry it as it is.
struct Fit {
    source: ReadSeekSource<fs::File>,
    file: Mp4File,
    track: usize,
    config: Config,
    bytes: u64,
    /// The media time of the first shown picture.
    origin: i64,
    /// The sample read next.
    next: usize,
}

/// The parameter sets of an `avcC` record as [`Config`].
fn config(avc: &isobmff::AvcConfig) -> Option<Config> {
    let mut parameter_sets = Vec::new();
    for nal in avc.sps.iter().chain(&avc.pps) {
        if parameter_sets.len().saturating_add(nal.len()) > MAX_PARAMETER_SETS {
            return None;
        }
        parameter_sets.extend_from_slice(&[0, 0, 0, 1]);
        parameter_sets.extend_from_slice(nal);
    }
    let sps = avc
        .sps
        .first()
        .filter(|nal| nal_type(nal) == 7)
        .and_then(|nal| nal.get(1..))
        .and_then(h264_sps::parse);
    Some(Config {
        length_size: usize::from(avc.length_size).clamp(1, 4),
        parameter_sets,
        sps,
    })
}

/// Opens `path` (its `moov`: no media is read) and checks its first video track against `format`.
fn fit(path: &Path, format: BdVideoFormat) -> Option<Fit> {
    let opened = fs::File::open(path).ok()?;
    let bytes = opened.metadata().ok()?.len();
    let source = ReadSeekSource::new(opened).ok()?;
    let file = isobmff::open(&source).ok()?;
    let track = file.track_of_kind(TrackKind::Video)?;
    let info = file.tracks.get(track)?;
    // One description for every sample, and it is H.264.
    let [entry] = info.entries.as_slice() else {
        return None;
    };
    let CodecConfig::Avc(avc) = &entry.codec else {
        return None;
    };
    let config = config(avc)?;
    if !sps_fits(config.sps.as_ref()?, format) {
        return None;
    }
    let (num, den) = format.rate;
    let timescale = i128::from(info.timescale);
    if timescale == 0 || num == 0 || den == 0 {
        return None;
    }
    // The frame rate: the pictures fill the time between the first and the last shown, one a frame.
    let count = i128::try_from(info.samples.len()).ok()?;
    let origin = info.samples.iter().map(|sample| sample.pts).min()?;
    let end = info.samples.iter().map(|sample| sample.pts).max()?;
    let span = i128::from(end).checked_sub(i128::from(origin))?;
    // span / timescale seconds are count - 1 frames of den / num seconds.
    let shown = span.checked_mul(i128::from(num))?;
    let expected = (count - 1)
        .checked_mul(i128::from(den))?
        .checked_mul(timescale)?;
    if (shown - expected).abs() > expected / 200 {
        return None;
    }
    // The track plays from its first picture: no edit that cuts into it or holds it back by more than a frame.
    if info
        .edits
        .iter()
        .filter(|edit| edit.media_time >= 0)
        .count()
        > 1
    {
        return None;
    }
    let frame = timescale.checked_mul(i128::from(den))? / i128::from(num);
    let starts = i128::from(origin).checked_add(i128::from(info.edit_offset))?;
    if starts.abs() > frame {
        return None;
    }
    // No faster than a Blu-ray's video on average, the whole file counted as video.
    let seconds = count as f64 * f64::from(den) / f64::from(num);
    if seconds <= 0.0 || bytes as f64 * 8.0 / seconds > crate::demux::BD_MAX_VIDEO_BITRATE {
        return None;
    }
    Some(Fit {
        source,
        file,
        track,
        config,
        bytes,
        origin,
        next: 0,
    })
}

/// Whether `path` is an MP4 / QuickTime file whose H.264 a Blu-ray title of `format` carries as it is (from its
/// header: the pictures themselves are checked when they are taken out).
pub(crate) fn fits(path: &Path, format: BdVideoFormat) -> bool {
    is_mp4(path) && fit(path, format).is_some()
}

/// Whether the file starts with one of the boxes an MP4 or QuickTime file starts with.
pub(crate) fn is_mp4(path: &Path) -> bool {
    let mut head = [0u8; 8];
    fs::File::open(path)
        .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut head))
        .is_ok()
        && matches!(
            head.get(4..),
            Some(b"ftyp" | b"moov" | b"mdat" | b"wide" | b"free" | b"skip")
        )
}

/// The bytes of a NAL unit's payload without its emulation prevention bytes (7.4.1), at most `limit` of them.
fn rbsp(payload: &[u8], limit: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len().min(limit));
    let mut zeros = 0u8;
    for &byte in payload {
        if out.len() >= limit {
            break;
        }
        if zeros >= 2 && byte == 3 {
            zeros = 0;
            continue;
        }
        zeros = if byte == 0 {
            zeros.saturating_add(1)
        } else {
            0
        };
        out.push(byte);
    }
    out
}

/// Whether a supplemental enhancement NAL (type 6) has a recovery point message (D.1: payload type 6), which
/// marks a picture decoding can start at.
fn has_recovery_point(nal: &[u8]) -> bool {
    if nal_type(nal) != 6 {
        return false;
    }
    let bytes = rbsp(nal.get(1..).unwrap_or_default(), 4096);
    let mut at = 0usize;
    // Each message: its type and its size, each a run of 0xFF bytes and a last byte, then the payload.
    let number = |at: &mut usize| -> Option<usize> {
        let mut value = 0usize;
        loop {
            let byte = *bytes.get(*at)?;
            *at += 1;
            value = value.checked_add(usize::from(byte))?;
            if byte != 0xFF {
                return Some(value);
            }
        }
    };
    // The last byte is the trailing bits.
    while at.saturating_add(2) < bytes.len() {
        let (Some(kind), Some(size)) = (number(&mut at), number(&mut at)) else {
            return false;
        };
        if kind == 6 {
            return true;
        }
        at = at.saturating_add(size);
    }
    false
}

impl Pictures for Fit {
    fn config(&self) -> &Config {
        &self.config
    }

    fn bytes(&self) -> u64 {
        self.bytes
    }

    fn next(&mut self) -> Result<Option<Picture>, String> {
        let Some(track) = self.file.tracks.get(self.track) else {
            return Ok(None);
        };
        let Some(sample) = track.samples.get(self.next) else {
            return Ok(None);
        };
        let data = self
            .file
            .read_sample(&self.source, self.track, self.next)
            .map_err(|err| err.to_string())?;
        self.next += 1;
        // A sync sample is an IDR picture; the I pictures that open the other GOPs of a broadcast or camera
        // stream say so with a recovery point.
        let keyframe = sample.is_sync
            || nals(&data, self.config.length_size)
                .is_some_and(|nals| nals.iter().any(|nal| has_recovery_point(nal)));
        let time = i128::from(sample.pts) - i128::from(self.origin);
        let time_ns = time
            .checked_mul(1_000_000_000)
            .map(|ns| ns / i128::from(track.timescale.max(1)))
            .and_then(|ns| i64::try_from(ns).ok())
            .ok_or_else(|| "a picture's time is out of range".to_string())?;
        Ok(Some(Picture {
            data,
            keyframe,
            time_ns,
            offset: sample.offset,
        }))
    }
}

/// [`crate::demux_h264_for_bd`] of an MP4 / QuickTime file.
pub(crate) fn demux_h264(
    path: &Path,
    cache_dir: &Path,
    format: BdVideoFormat,
    progress: &mut dyn FnMut(f32),
) -> Result<Option<H264Samples>, DvdError> {
    let mut open = || fit(path, format).map(|fit| Box::new(fit) as Box<dyn Pictures>);
    demux_pictures(path, cache_dir, format, &mut open, progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_recovery_point_among_the_messages() {
        // Buffering period (type 0, 2 bytes), then a recovery point (type 6, 1 byte), then the trailing bits.
        assert!(has_recovery_point(&[6, 0, 2, 0xAA, 0xBB, 6, 1, 0x84, 0x80]));
        // Picture timing (type 1) only.
        assert!(!has_recovery_point(&[6, 1, 2, 0xAA, 0xBB, 0x80]));
        // A user data message whose type is past 255 and whose payload has a 6 in it.
        assert!(!has_recovery_point(&[6, 0xFF, 5, 2, 6, 6, 0x80]));
        // Emulation prevention inside a payload does not shift the next message.
        assert!(has_recovery_point(&[
            6, 5, 4, 0, 0, 3, 0, 1, 6, 1, 0x84, 0x80
        ]));
        // Not a supplemental NAL; cut short.
        assert!(!has_recovery_point(&[5, 6, 1, 0x84, 0x80]));
        assert!(!has_recovery_point(&[6]));
        assert!(!has_recovery_point(&[6, 0xFF, 0xFF]));
        assert!(!has_recovery_point(&[6, 5, 200, 1]));
    }

    #[test]
    fn a_record_becomes_annex_b_parameter_sets() {
        let avc = isobmff::AvcConfig::new(vec![vec![0x67, 100, 0, 41]], vec![vec![0x68, 0xEE]], 4);
        let config = config(&avc).unwrap();
        assert_eq!(config.length_size, 4);
        assert_eq!(
            config.parameter_sets,
            [0, 0, 0, 1, 0x67, 100, 0, 41, 0, 0, 0, 1, 0x68, 0xEE]
        );
        // Too short to be a sequence parameter set.
        assert!(config.sps.is_none());
    }

    #[test]
    fn tells_an_mp4_by_its_first_box() {
        let dir = std::env::temp_dir().join(format!("disccraft-mp4-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mp4 = dir.join("a.bin");
        fs::write(&mp4, b"\0\0\0\x18ftypisom").unwrap();
        let other = dir.join("b.bin");
        fs::write(&other, b"\x1A\x45\xDF\xA3\0\0\0\0").unwrap();
        let (yes, no) = (is_mp4(&mp4), is_mp4(&other));
        let _ = fs::remove_dir_all(&dir);
        assert!(yes && !no);
        assert!(!is_mp4(Path::new("no such file")));
    }
}
