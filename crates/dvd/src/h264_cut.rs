//! Trimming H.264 that goes on a Blu-ray as it is ([`crate::demux_h264_for_bd`]): a clip can start only on a
//! picture that needs none before it (an IDR picture: ITU-T H.264 7.4.1.2.4, `nal_unit_type` 5) and end only in
//! front of a key picture, as an MPEG-2 clip is trimmed at its GOPs.
//!
//! A trim counts frames of the disc's standard rate (29.97 or 25 a second) from the file's first shown picture;
//! a key picture is "on" the frame its time rounds to.

use std::path::PathBuf;

use disccraft_project::ClipTrim;
use disccraft_transcode::hd::AccessUnit;

use crate::{DvdError, H264Samples, Passthrough};

/// The trimmed part of a file's H.264 that goes on a Blu-ray as it is.
#[derive(Debug)]
pub enum TrimmedPassthrough {
    /// Units of an Annex B stream in the build cache (their offsets are in that file), timed from 0.
    Stream {
        /// The Annex B stream.
        es: PathBuf,
        /// The units that play, in decoding order.
        units: Vec<AccessUnit>,
    },
    /// Pictures left in their Matroska or MP4 file.
    Samples(H264Samples),
    /// Pictures left in their transport stream (M6.367).
    Transport(crate::TsUnits),
    /// HD MPEG-2 (M6.385): units of an elementary stream (the file itself, or the build cache's), timed from 0.
    Mpeg2Stream {
        /// The MPEG-2 video elementary stream.
        es: PathBuf,
        /// The units that play, in decoding order.
        units: Vec<AccessUnit>,
    },
    /// HD MPEG-2 pictures left in their transport stream (M6.385).
    Mpeg2Transport(crate::TsUnits),
}

impl TrimmedPassthrough {
    /// Whether the part is MPEG-2 video (else H.264).
    #[must_use]
    pub fn is_mpeg2(&self) -> bool {
        matches!(self, Self::Mpeg2Stream { .. } | Self::Mpeg2Transport(_))
    }
}

/// The frame of the standard rate a 90 kHz time (from the first shown picture) rounds to.
fn frame_of(time: i64, rate: (u32, u32)) -> Option<i64> {
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

/// The first shown picture's time.
fn origin(units: &[AccessUnit]) -> Option<i64> {
    units.iter().map(|unit| unit.pts).min()
}

/// Where the key pictures of `units` are: the frame (of `rate`) each is on, with its index, in order of the
/// frames (one for a frame: the first in the stream).
#[must_use]
pub fn key_frames(units: &[AccessUnit], rate: (u32, u32)) -> Vec<(i64, usize)> {
    let Some(origin) = origin(units) else {
        return Vec::new();
    };
    let mut keys: Vec<(i64, usize)> = units
        .iter()
        .enumerate()
        .filter(|(_, unit)| unit.keyframe)
        .filter_map(|(index, unit)| Some((frame_of(unit.pts.checked_sub(origin)?, rate)?, index)))
        .collect();
    keys.sort_unstable();
    keys.dedup_by_key(|(frame, _)| *frame);
    keys
}

/// The units of `units` that play when the clip is trimmed to `trim`: their indices, and the units timed as a
/// whole stream is (the first shown picture where the whole stream's was). `None` when the trim's in point is
/// not on a key picture, or its out point is neither on one nor at (or past) the end.
#[must_use]
pub fn trimmed_units(
    units: &[AccessUnit],
    trim: ClipTrim,
    rate: (u32, u32),
) -> Option<(Vec<usize>, Vec<AccessUnit>)> {
    let origin = origin(units)?;
    let keys = key_frames(units, rate);
    let key_on = |frame: u32| {
        keys.iter()
            .find(|(on, _)| *on == i64::from(frame))
            .map(|(_, index)| *index)
    };
    let (first, from) = if trim.start == 0 {
        (0, None)
    } else {
        let first = key_on(trim.start)?;
        (first, Some(units.get(first)?.pts))
    };
    let (last, to) = match trim.end {
        None => (units.len(), None),
        Some(end) => match key_on(end) {
            Some(last) => (last, Some(units.get(last)?.pts)),
            None => {
                // At or past the last picture: the clip plays to its end.
                let length = units
                    .iter()
                    .filter_map(|unit| frame_of(unit.pts.checked_sub(origin)?, rate))
                    .max()?
                    .checked_add(1)?;
                if i64::from(end) < length {
                    return None;
                }
                (units.len(), None)
            }
        },
    };
    if last <= first {
        return None;
    }
    // In decoding order from the in point's picture up to the out point's; pictures shown before the first or
    // from the last on (an open GOP's leading pictures) do not belong to the part.
    let delta = from.map_or(0, |from| from.saturating_sub(origin));
    let mut indices = Vec::new();
    let mut kept = Vec::new();
    for (index, unit) in units.iter().enumerate().take(last).skip(first) {
        if from.is_some_and(|from| unit.pts < from) || to.is_some_and(|to| unit.pts >= to) {
            continue;
        }
        indices.push(index);
        kept.push(AccessUnit {
            offset: unit.offset,
            len: unit.len,
            pts: unit.pts.checked_sub(delta)?,
            dts: unit.dts.checked_sub(delta)?,
            keyframe: unit.keyframe,
        });
    }
    (!kept.is_empty()).then_some((indices, kept))
}

/// Whether an Annex B access unit is an IDR picture: its first slice is a `nal_unit_type` 5.
#[must_use]
pub fn is_idr(unit: &[u8]) -> bool {
    let mut at = 0usize;
    while let Some(found) = unit
        .get(at..)
        .and_then(|rest| rest.windows(3).position(|code| code == [0, 0, 1]))
    {
        let header = at.saturating_add(found).saturating_add(3);
        match unit.get(header).map(|byte| byte & 0x1F) {
            Some(5) => return true,
            Some(1) | None => return false,
            Some(_) => at = header,
        }
    }
    false
}

impl Passthrough {
    /// The access units of the stream, in decoding order.
    ///
    /// # Errors
    ///
    /// When the index in the cache cannot be read.
    pub fn units(&self) -> Result<Vec<AccessUnit>, DvdError> {
        match self {
            Self::Samples(samples) => Ok(samples.units().to_vec()),
            Self::Transport(ts) | Self::TransportMpeg2(ts) => Ok(ts.units().to_vec()),
            Self::Stream { index, .. } | Self::Mpeg2 { index, .. } => {
                disccraft_transcode::hd::read_h264_index(index)
                    .map_err(|error| DvdError::Unsupported(format!("{}: {error}", index.display())))
            }
        }
    }

    /// Access unit `index` of `units` ([`Self::units`]) as Annex B bytes.
    fn read_unit(&self, units: &[AccessUnit], index: usize) -> Result<Vec<u8>, DvdError> {
        use std::io::{Read, Seek, SeekFrom};
        match self {
            Self::Samples(samples) => {
                let path = samples.path();
                let mut file =
                    std::fs::File::open(path).map_err(|error| crate::io_error(path, error))?;
                samples
                    .read(&mut file, index)
                    .map_err(|error| crate::io_error(path, error))
            }
            Self::Transport(ts) | Self::TransportMpeg2(ts) => {
                let path = ts.path();
                let mut file =
                    std::fs::File::open(path).map_err(|error| crate::io_error(path, error))?;
                ts.read(&mut file, index)
                    .map_err(|error| crate::io_error(path, error))
            }
            Self::Stream { es, .. } | Self::Mpeg2 { es, .. } => {
                let unit = units
                    .get(index)
                    .ok_or_else(|| DvdError::Invalid(format!("no unit {index}")))?;
                let io = |error: std::io::Error| crate::io_error(es, error);
                let mut file = std::fs::File::open(es).map_err(io)?;
                let _ = file.seek(SeekFrom::Start(unit.offset)).map_err(io)?;
                let mut bytes = vec![0u8; unit.len as usize];
                file.read_exact(&mut bytes).map_err(io)?;
                Ok(bytes)
            }
        }
    }

    /// Whether a clip can start on unit `index` of `units` ([`Self::units`]): it is an IDR picture.
    ///
    /// # Errors
    ///
    /// When the unit cannot be read.
    pub fn starts_clean(&self, units: &[AccessUnit], index: usize) -> Result<bool, DvdError> {
        // MPEG-2 (M6.385): a GOP's I picture starts it (the open GOP's leading B pictures are left out).
        if matches!(self, Self::Mpeg2 { .. } | Self::TransportMpeg2(_)) {
            return Ok(units.get(index).is_some_and(|unit| unit.keyframe));
        }
        Ok(is_idr(&self.read_unit(units, index)?))
    }

    /// The part of the stream that plays when the clip is trimmed to `trim` (frames of `rate`, the disc's
    /// standard rate). `None` when the stream cannot be cut there ([`trimmed_units`]), or the in point's picture
    /// is not an IDR picture: the clip is then encoded.
    ///
    /// # Errors
    ///
    /// When the stream or its index cannot be read.
    pub fn trimmed(
        &self,
        trim: ClipTrim,
        rate: (u32, u32),
    ) -> Result<Option<TrimmedPassthrough>, DvdError> {
        let units = self.units()?;
        let Some((indices, kept)) = trimmed_units(&units, trim, rate) else {
            return Ok(None);
        };
        if trim.start > 0
            && let Some(first) = indices.first()
            && !self.starts_clean(&units, *first)?
        {
            return Ok(None);
        }
        Ok(Some(match self {
            Self::Samples(samples) => TrimmedPassthrough::Samples(samples.keep(&indices, kept)),
            Self::Transport(ts) => TrimmedPassthrough::Transport(ts.keep(&indices, kept)),
            Self::TransportMpeg2(ts) => TrimmedPassthrough::Mpeg2Transport(ts.keep(&indices, kept)),
            Self::Stream { es, .. } => TrimmedPassthrough::Stream {
                es: es.clone(),
                units: kept,
            },
            Self::Mpeg2 { es, .. } => TrimmedPassthrough::Mpeg2Stream {
                es: es.clone(),
                units: kept,
            },
        }))
    }
}

/// Tells whether the key picture in unit `index` is an IDR picture.
type Clean = Box<dyn Fn(usize) -> bool + Send>;

/// Where a clip of a file can be cut when its H.264 goes on a Blu-ray as it is.
pub struct CutPoints {
    /// The frame each key picture is on, with its unit.
    keys: Vec<(i64, usize)>,
    clean: Clean,
}

impl std::fmt::Debug for CutPoints {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CutPoints")
            .field("keys", &self.keys)
            .finish_non_exhaustive()
    }
}

impl CutPoints {
    /// The frames (of the disc's standard rate) key pictures are on, in order.
    #[must_use]
    pub fn frames(&self) -> Vec<i64> {
        self.keys.iter().map(|(frame, _)| *frame).collect()
    }

    /// Whether a clip can start on `frame`: the file's first frame, or an IDR picture's.
    #[must_use]
    pub fn can_start(&self, frame: i64) -> bool {
        frame == 0
            || self
                .keys
                .iter()
                .find(|(on, _)| *on == frame)
                .is_some_and(|(_, index)| (self.clean)(*index))
    }
}

/// Where the clips of `path` can be cut on `project`'s disc without encoding them: `None` when the file's video
/// does not go on the disc as it is (not an HD Blu-ray, or H.264 the disc cannot carry). A Matroska or MP4
/// file's pictures are indexed in `index_dir` (a small file); a transport stream is read where it is.
///
/// # Errors
///
/// As [`crate::demux_h264_for_bd`].
pub fn bd_cut_points(
    project: &disccraft_project::Project,
    path: &std::path::Path,
    index_dir: &std::path::Path,
) -> Result<Option<CutPoints>, DvdError> {
    // In the format of the file's titles (its asset's own, M6.325).
    let Some(format) = crate::BdVideoFormat::of_path(project, path) else {
        return Ok(None);
    };
    let rate = if project.settings.standard == disccraft_project::VideoStandard::Ntsc {
        (30_000, 1001)
    } else {
        (25, 1)
    };
    // HD MPEG-2 that goes on the disc as it is (M6.385) is cut at its GOPs: every I picture starts one.
    if !crate::h264_fits_bd(path, format) {
        if !crate::mpeg2_fits_bd(path, format) {
            return Ok(None);
        }
        let Some(passthrough) = crate::demux_mpeg2_for_bd(path, index_dir, format)? else {
            return Ok(None);
        };
        let units = passthrough.units()?;
        let keys = key_frames(&units, rate);
        return Ok(Some(CutPoints {
            keys,
            clean: Box::new(move |index| passthrough.starts_clean(&units, index).unwrap_or(false)),
        }));
    }
    // A transport stream's pictures are found by its index, without writing its video out first.
    if let Some((units, clean)) = crate::demux::ts_h264_keys(path) {
        return Ok(Some(CutPoints {
            keys: key_frames(&units, rate),
            clean,
        }));
    }
    let Some(passthrough) = crate::demux_h264_for_bd(path, index_dir, format, &mut |_| {})? else {
        return Ok(None);
    };
    let units = passthrough.units()?;
    let keys = key_frames(&units, rate);
    Ok(Some(CutPoints {
        keys,
        clean: Box::new(move |index| passthrough.starts_clean(&units, index).unwrap_or(false)),
    }))
}

#[cfg(test)]
mod tests {
    use super::{is_idr, key_frames, trimmed_units};
    use disccraft_project::ClipTrim;
    use disccraft_transcode::hd::AccessUnit;

    const NTSC: (u32, u32) = (30_000, 1001);

    /// Three GOPs of six pictures at 29.97 (I B B P B B in display order; the stream holds I P B B ... with the
    /// B pictures of an open GOP leading its I picture), `open` GOPs after the first when asked.
    fn stream(open: bool) -> Vec<AccessUnit> {
        let mut units = Vec::new();
        let mut push = |frame: i64, keyframe: bool| {
            let at = units.len() as i64;
            units.push(AccessUnit {
                offset: at as u64 * 100,
                len: 100,
                pts: 3003 + frame * 3003,
                dts: at * 3003,
                keyframe,
            });
        };
        for gop in 0..3_i64 {
            let base = gop * 6;
            if open && gop > 0 {
                // I at base + 2, its leading B pictures at base and base + 1, then P at base + 5 with two B.
                push(base + 2, true);
                push(base, false);
                push(base + 1, false);
                push(base + 5, false);
                push(base + 3, false);
                push(base + 4, false);
            } else {
                push(base, true);
                push(base + 3, false);
                push(base + 1, false);
                push(base + 2, false);
                push(base + 5, false);
                push(base + 4, false);
            }
        }
        units
    }

    fn frames(units: &[AccessUnit]) -> Vec<i64> {
        let mut shown: Vec<i64> = units.iter().map(|unit| unit.pts / 3003 - 1).collect();
        shown.sort_unstable();
        shown
    }

    #[test]
    fn a_closed_gop_stream_is_cut_at_its_key_pictures() {
        let units = stream(false);
        assert_eq!(key_frames(&units, NTSC), [(0, 0), (6, 6), (12, 12)]);
        // The whole clip.
        let (indices, kept) = trimmed_units(&units, ClipTrim::default(), NTSC).unwrap();
        assert_eq!(indices, (0..18).collect::<Vec<_>>());
        assert_eq!(kept, units);
        // The middle GOP: its six pictures, timed as the stream's first six were.
        let trim = ClipTrim {
            start: 6,
            end: Some(12),
        };
        let (indices, kept) = trimmed_units(&units, trim, NTSC).unwrap();
        assert_eq!(indices, (6..12).collect::<Vec<_>>());
        assert_eq!(frames(&kept), (0..6).collect::<Vec<_>>());
        assert_eq!(kept[0].offset, 600);
        assert_eq!(
            kept[0].pts - kept[0].dts,
            units[6].pts - units[6].dts,
            "the delay between decoding and showing stays"
        );
        // An out point at or past the end plays to the end; one inside a GOP cannot be cut.
        let to_end = ClipTrim {
            start: 12,
            end: Some(18),
        };
        assert_eq!(trimmed_units(&units, to_end, NTSC).unwrap().0.len(), 6);
        for (start, end) in [(5, None), (6, Some(11)), (6, Some(6)), (12, Some(6))] {
            assert!(
                trimmed_units(&units, ClipTrim { start, end }, NTSC).is_none(),
                "{start} {end:?}"
            );
        }
    }

    #[test]
    fn an_open_gops_leading_pictures_go_with_the_cut() {
        let units = stream(true);
        assert_eq!(key_frames(&units, NTSC), [(0, 0), (8, 6), (14, 12)]);
        // From the second key picture: its two leading B pictures (frames 6 and 7) are not shown.
        let trim = ClipTrim {
            start: 8,
            end: None,
        };
        let (indices, kept) = trimmed_units(&units, trim, NTSC).unwrap();
        assert_eq!(indices, [6, 9, 10, 11, 12, 13, 14, 15, 16, 17]);
        assert_eq!(frames(&kept), (0..10).collect::<Vec<_>>());
        // Up to the second key picture: the frames before its leading pictures.
        let trim = ClipTrim {
            start: 0,
            end: Some(8),
        };
        let (indices, kept) = trimmed_units(&units, trim, NTSC).unwrap();
        assert_eq!(indices, (0..6).collect::<Vec<_>>());
        assert_eq!(frames(&kept), (0..6).collect::<Vec<_>>());
    }

    #[test]
    fn an_idr_picture_is_told_by_its_first_slice() {
        // Access unit delimiter, SPS, PPS, then the slice.
        let head = [0, 0, 0, 1, 0x09, 0xF0, 0, 0, 1, 0x67, 1, 0, 0, 1, 0x68, 2];
        let unit = |slice: u8| [head.as_slice(), &[0, 0, 1, slice, 0x88]].concat();
        assert!(is_idr(&unit(0x65)));
        assert!(!is_idr(&unit(0x41)));
        // A non-IDR slice first, whatever follows.
        assert!(!is_idr(&[unit(0x41).as_slice(), &[0, 0, 1, 0x65]].concat()));
        assert!(!is_idr(&head));
        assert!(!is_idr(&[]));
    }
}
