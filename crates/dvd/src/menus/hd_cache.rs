//! One play of an HD Blu-ray disc's motion menu kept in the build's cache (M6.387), so File ▸ Render ▸ Motion Menu
//! makes it before the build, the build takes it as it is, and the project preview plays it.
//!
//! Three files share a name made of everything the play is made of: the pictures as a transport stream of the video
//! alone (`.m2ts`, which the preview's media reader opens: it does not open a bare H.264 stream), the access units'
//! index (`.idx`, as an HD transcode's) and a marker written last (`.ok`). The build reads the pictures back from
//! the transport stream: one access unit per PES packet, as the multiplexer writes them.

use std::path::{Path, PathBuf};

use disccraft_m2tsmux::{EsInput, EsUnit, MuxConfig, StreamKind, mux_m2ts, pes_payloads};
use disccraft_transcode::hd::{AccessUnit, HdCodec, read_h264_index, write_h264_index};

/// The video's PID in the preview's transport stream.
const VIDEO_PID: u16 = 0x1011;
/// Where the stream's times start (90 kHz): the first decoding time stamp of a stream with B pictures is before its
/// first picture's.
const START_90K: i64 = 27_000;
/// The most access units a play holds (as the frame count of a motion menu).
const MAX_UNITS: usize = 200_000;
/// The largest transport stream read back (a motion menu of the longest length at the highest rate is far smaller).
const MAX_STREAM_BYTES: u64 = 4 << 30;

/// Where one play is kept.
pub(super) struct HdMotionCache {
    /// The pictures, as a transport stream of the video alone.
    pub(super) stream: PathBuf,
    /// The access units' index (offsets into the elementary stream the transport stream carries).
    index: PathBuf,
    /// Written last: the play is whole.
    marker: PathBuf,
}

impl HdMotionCache {
    /// The files of the play named `key` in `cache_dir`.
    pub(super) fn new(cache_dir: &Path, key: &str) -> Self {
        Self {
            stream: cache_dir.join(format!("{key}.m2ts")),
            index: cache_dir.join(format!("{key}.idx")),
            marker: cache_dir.join(format!("{key}.ok")),
        }
    }

    /// Whether the play is there, whole.
    pub(super) fn is_ready(&self) -> bool {
        self.marker.is_file() && self.stream.is_file() && self.index.is_file()
    }

    /// The play kept here: its elementary stream and access units (times from 0). `None` when it is not there or
    /// cannot be read back as it was written (the build then encodes it again).
    pub(super) fn read(&self) -> Option<(Vec<u8>, Vec<AccessUnit>)> {
        if !self.is_ready() {
            return None;
        }
        let units = read_h264_index(&self.index).ok()?;
        if units.is_empty() || units.len() > MAX_UNITS {
            return None;
        }
        if std::fs::metadata(&self.stream).ok()?.len() > MAX_STREAM_BYTES {
            return None;
        }
        let stream = std::fs::read(&self.stream).ok()?;
        let data: Vec<u8> = pes_payloads(&stream, VIDEO_PID).ok()?.concat();
        contiguous(&units, data.len()).then_some((data, units))
    }

    /// Keeps a play (each file written whole under another name first, the marker last). A cache that cannot be
    /// written only means the next build encodes it again.
    pub(super) fn store(&self, codec: HdCodec, data: &[u8], units: &[AccessUnit]) {
        let Some(parent) = self.stream.parent() else {
            return;
        };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        let _ = std::fs::remove_file(&self.marker);
        let stored = transport_stream(codec, data, units)
            .and_then(|stream| whole(&self.stream, |part| std::fs::write(part, &stream).ok()))
            .and_then(|()| whole(&self.index, |part| write_h264_index(part, units).ok()))
            .and_then(|()| std::fs::write(&self.marker, b"ok").ok());
        if stored.is_none() {
            let _ = std::fs::remove_file(&self.marker);
        }
    }
}

/// Whether `units` follow one another from the start of a stream of `len` bytes to its end.
fn contiguous(units: &[AccessUnit], len: usize) -> bool {
    let mut next = 0_u64;
    for unit in units {
        if unit.offset != next {
            return false;
        }
        let Some(end) = next.checked_add(u64::from(unit.len)) else {
            return false;
        };
        next = end;
    }
    u64::try_from(len).is_ok_and(|len| len == next)
}

/// `data` as a transport stream of the video alone.
fn transport_stream(codec: HdCodec, data: &[u8], units: &[AccessUnit]) -> Option<Vec<u8>> {
    if !contiguous(units, data.len()) {
        return None;
    }
    let at = |time: i64| u64::try_from(START_90K.checked_add(time)?).ok();
    let mut es = Vec::with_capacity(units.len());
    for unit in units {
        let begin = usize::try_from(unit.offset).ok()?;
        let end = begin.checked_add(usize::try_from(unit.len).ok()?)?;
        es.push(EsUnit {
            pts: at(unit.pts)?,
            dts: at(unit.dts)?,
            data: data.get(begin..end)?.to_vec(),
            random_access: unit.keyframe,
        });
    }
    let input = EsInput {
        pid: VIDEO_PID,
        kind: match codec {
            HdCodec::H264 => StreamKind::H264Video,
            HdCodec::Mpeg2 => StreamKind::Mpeg2Video,
            // A menu's video is H.264 or MPEG-2 (M12.43: HEVC is a title's, for Ultra HD).
            HdCodec::Hevc => return None,
        },
        units: es,
    };
    let mut out = Vec::new();
    let layout = mux_m2ts(&[input], &MuxConfig::default(), &mut out).ok()?;
    (layout.late_packets == 0).then_some(out)
}

/// Writes `path` whole with `write`: under another name first, then renamed.
fn whole(path: &Path, write: impl FnOnce(&Path) -> Option<()>) -> Option<()> {
    let part = path.with_extension(format!("{}.part", crate::prepare::part_number()));
    let written = write(&part).and_then(|()| std::fs::rename(&part, path).ok());
    if written.is_none() {
        let _ = std::fs::remove_file(&part);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(offset: u64, len: u32) -> AccessUnit {
        AccessUnit {
            offset,
            len,
            pts: 0,
            dts: 0,
            keyframe: false,
        }
    }

    #[test]
    fn units_must_cover_the_stream_in_order() {
        assert!(contiguous(&[unit(0, 4), unit(4, 6)], 10));
        assert!(!contiguous(&[unit(0, 4), unit(4, 6)], 11));
        assert!(!contiguous(&[unit(0, 4), unit(5, 5)], 10));
        assert!(!contiguous(&[unit(1, 9)], 10));
        assert!(!contiguous(&[unit(0, u32::MAX), unit(u64::MAX, 2)], 10));
    }

    #[test]
    fn a_stored_play_is_read_back_as_it_was() {
        let dir = std::env::temp_dir().join(format!("disccraft-hd-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = HdMotionCache::new(&dir, "motion-hd-test");
        // Three "pictures", the second larger than a transport packet, the third larger than a PES packet's length
        // field can say.
        let sizes = [10_usize, 1000, 70_000];
        let mut data = Vec::new();
        let mut units = Vec::new();
        for (index, size) in sizes.iter().enumerate() {
            let start = data.len() as u64;
            data.extend((0..*size).map(|byte| (byte % 251) as u8));
            units.push(AccessUnit {
                offset: start,
                len: *size as u32,
                pts: index as i64 * 3003 + 3003,
                dts: index as i64 * 3003,
                keyframe: index == 0,
            });
        }
        assert!(cache.read().is_none());
        cache.store(HdCodec::H264, &data, &units);
        assert!(cache.is_ready());
        let (read, read_units) = cache.read().unwrap();
        assert!(read == data);
        assert_eq!(read_units, units);
        // Without its marker it is not taken.
        std::fs::remove_file(&cache.marker).unwrap();
        assert!(cache.read().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
