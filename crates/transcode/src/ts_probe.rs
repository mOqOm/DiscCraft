//! What a transport stream's index and its H.264 sequence parameter set say of its video, without decoding it
//! (M12.22).
//!
//! The media reader refuses H.264 coded as fields or MBAFF (camcorders' AVCHD, broadcast recordings), so such a
//! `.m2ts` / `.mts` / `.ts` could not even be asked how long it is. Its length is in its time stamps, and its
//! size, pixel shape and frame rate are in its sequence parameter set (ITU-T H.264 7.3.2.1.1 and E.1.1).

use std::path::Path;

use craft_media::mpegts::{Codec, Kind};

use crate::VideoProbe;

/// A transport packet's first byte.
const SYNC: u8 = 0x47;

/// Whether the file starts like a transport stream: sync bytes a packet apart, in packets of 188 bytes or of 192
/// (a Blu-ray's, each with four bytes in front).
pub(crate) fn looks_like_ts(path: &Path) -> bool {
    let mut head = [0u8; 4 + 192 * 3];
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut filled = 0usize;
    while let Some(rest) = head.get_mut(filled..).filter(|rest| !rest.is_empty()) {
        match std::io::Read::read(&mut file, rest) {
            Ok(0) | Err(_) => break,
            Ok(got) => filled += got,
        }
    }
    let synced = |first: usize, packet: usize| {
        (0..3).all(|n| head.get(first + n * packet).copied() == Some(SYNC))
            && filled > first + 2 * packet
    };
    synced(0, 188) || synced(4, 192)
}

/// The video of the transport stream `path` from its index and its sequence parameter set; `None` for another
/// kind of file, one without H.264 video, or one whose pictures have no times.
pub(crate) fn quick(path: &Path) -> Option<VideoProbe> {
    if !looks_like_ts(path) {
        return None;
    }
    let (source, file) = crate::ts_index::open(path)?;
    let index = file.find(Kind::Video, |codec| *codec == Codec::H264)?;
    let units = &file.streams.get(index)?.units;
    let times = || units.iter().filter_map(|unit| unit.pts);
    let (first, last) = (times().min()?, times().max()?);
    // The sequence parameter set is in the first picture's unit (a stream is not joined in the middle of a GOP).
    let sps = crate::h264_sps::first_in_annex_b(&file.read_unit(&source, index, 0).ok()?)?;
    let (width, height) = sps.shown;
    // Frames a second: as the stream says, else from how many pictures its time holds.
    let span = (last.checked_sub(first)?) as f64 / 90_000.0;
    let fps = sps
        .frame_rate()
        .or_else(|| (span > 0.0 && units.len() > 1).then(|| (units.len() - 1) as f64 / span))
        .filter(|fps| fps.is_finite() && *fps > 0.0)?;
    let seconds = span + 1.0 / fps;
    if width == 0 || height == 0 || !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    Some(VideoProbe {
        seconds,
        width,
        height,
        fps,
        wide: crate::is_wide(width, height, sps.pixel_aspect.unwrap_or((1, 1))),
        has_audio: file.find(Kind::Audio, |_| true).is_some(),
    })
}
