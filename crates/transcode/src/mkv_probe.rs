//! What a Matroska file's header says of its video, read without opening the file as media (M12.16).
//!
//! Opening a Matroska file as media reads through all of it to index its frames: half a minute for a file of
//! tens of gigabytes. Its length, frame size and frame rate are in the header at its front.

use std::path::Path;

use craft_media::ReadSeekSource;
use craft_media::matroska::{Codec, OpenOptions, TrackKind, open_with};

use crate::VideoProbe;

/// The frame rates a frame duration in nanoseconds is taken for when it is within a ten-thousandth of one.
const RATES: [(u32, u32); 8] = [
    (24_000, 1001),
    (24, 1),
    (25, 1),
    (30_000, 1001),
    (30, 1),
    (50, 1),
    (60_000, 1001),
    (60, 1),
];

/// The video of the Matroska file `path` from its header; `None` for another kind of file, one without H.264
/// video, or a header that leaves out the length, the size or the frame duration (it is then opened as media).
pub(crate) fn quick(path: &Path) -> Option<VideoProbe> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut magic = [0u8; 4];
    std::io::Read::read_exact(&mut file, &mut magic).ok()?;
    if magic != [0x1A, 0x45, 0xDF, 0xA3] {
        return None;
    }
    std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(0)).ok()?;
    let source = ReadSeekSource::new(file).ok()?;
    let options = OpenOptions {
        index: false,
        verify_crc: false,
        index_limit: None,
    };
    let mkv = open_with(source, &options).ok()?;
    let track = mkv.tracks.get(mkv.track_of_kind(TrackKind::Video)?)?;
    // The video the media reader is known to open; anything else is left to it to accept or refuse.
    if !matches!(track.codec, Codec::Avc { .. }) || !track.encodings.is_empty() {
        return None;
    }
    let video = track.video.as_ref()?;
    let (width, height) = (video.pixel_width, video.pixel_height);
    let seconds = mkv.duration_ns()? as f64 / 1e9;
    let fps = rate(track.default_duration_ns?)?;
    if width == 0 || height == 0 || !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    Some(VideoProbe {
        seconds,
        width,
        height,
        fps,
        wide: crate::is_wide(width, height, video.pixel_aspect()),
        has_audio: mkv.track_of_kind(TrackKind::Audio).is_some(),
    })
}

/// Frames a second of a frame duration in nanoseconds: a usual rate it is close to, else as it divides.
fn rate(duration_ns: u64) -> Option<f64> {
    if duration_ns == 0 {
        return None;
    }
    let fps = 1e9 / duration_ns as f64;
    Some(
        RATES
            .iter()
            .map(|&(num, den)| f64::from(num) / f64::from(den))
            .find(|usual| (usual - fps).abs() <= usual * 1e-4)
            .unwrap_or(fps),
    )
}

#[cfg(test)]
mod tests {
    use super::rate;

    #[test]
    fn frame_durations_become_usual_rates() {
        // Matroska's 29.97: 33,366,667 ns (also written 33,367,000).
        for duration in [33_366_667, 33_367_000, 33_366_666] {
            assert_eq!(rate(duration), Some(30_000.0 / 1001.0));
        }
        assert_eq!(rate(40_000_000), Some(25.0));
        assert_eq!(rate(41_708_333), Some(24_000.0 / 1001.0));
        assert_eq!(rate(16_683_333), Some(60_000.0 / 1001.0));
        // An unusual rate is kept.
        assert_eq!(rate(100_000_000), Some(10.0));
        assert_eq!(rate(0), None);
    }
}
