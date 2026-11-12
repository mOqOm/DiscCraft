//! BD の PG / IG セグメントの符号化。
//!
//! 層: L0（docs/architecture.md §5）。
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::error::Error;
use std::fmt;

pub mod ig;
mod rle;
mod segment;

pub use rle::{rle_decode, rle_encode};
pub use segment::{
    Composition, CompositionObject, PaletteEntry, Window, end, ods, pcs, pds, rgba_to_entry, wds,
};

/// PGS encoding error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgsError(
    /// Error message.
    pub String,
);

impl fmt::Display for PgsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for PgsError {}

/// One subtitle: a bitmap shown from `start` to `end` (90 kHz).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subtitle {
    /// Start presentation timestamp in 90 kHz units.
    pub start: u64,
    /// End presentation timestamp in 90 kHz units.
    pub end: u64,
    /// X coordinate of the bitmap.
    pub x: u16,
    /// Y coordinate of the bitmap.
    pub y: u16,
    /// Bitmap width.
    pub width: u16,
    /// Bitmap height.
    pub height: u16,
    /// Palette indices, row-major.
    pub pixels: Vec<u8>,
    /// Palette entries used by this subtitle.
    pub palette: Vec<PaletteEntry>,
}

/// A segment with its time stamps (90 kHz).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedSegment {
    /// Presentation timestamp.
    pub pts: u64,
    /// Decode timestamp.
    pub dts: u64,
    /// Encoded segment bytes.
    pub data: Vec<u8>,
}

fn err(message: &str) -> PgsError {
    PgsError(message.to_string())
}

fn timed_segments(pts: u64, segments: Vec<Vec<u8>>) -> Vec<TimedSegment> {
    segments
        .into_iter()
        .map(|data| TimedSegment {
            pts,
            dts: pts,
            data,
        })
        .collect()
}

fn check_subtitle_order(sub: &Subtitle, previous_end: Option<u64>) -> Result<(), PgsError> {
    if sub.end < sub.start {
        return Err(err("subtitle end is before start"));
    }

    if let Some(previous) = previous_end
        && sub.start < previous
    {
        return Err(err("subtitles overlap"));
    }

    Ok(())
}

fn composition_numbers(index: usize) -> Result<(u16, u16), PgsError> {
    let number = u16::try_from(index).map_err(|_| err("too many subtitles"))?;
    let next_number = number
        .checked_add(1)
        .ok_or_else(|| err("composition number overflow"))?;
    Ok((number, next_number))
}

fn validate_geometry(sub: &Subtitle, video_width: u16, video_height: u16) -> Result<(), PgsError> {
    let x_end = u32::from(sub.x)
        .checked_add(u32::from(sub.width))
        .ok_or_else(|| err("subtitle x overflow"))?;
    if x_end > u32::from(video_width) {
        return Err(err("subtitle lies outside video width"));
    }

    let y_end = u32::from(sub.y)
        .checked_add(u32::from(sub.height))
        .ok_or_else(|| err("subtitle y overflow"))?;
    if y_end > u32::from(video_height) {
        return Err(err("subtitle lies outside video height"));
    }

    Ok(())
}

fn start_set(
    sub: &Subtitle,
    number: u16,
    video_width: u16,
    video_height: u16,
) -> Result<Vec<TimedSegment>, PgsError> {
    let rle = rle_encode(sub.width, sub.height, &sub.pixels)?;

    let objects = vec![CompositionObject {
        object_id: 0,
        window_id: 0,
        x: sub.x,
        y: sub.y,
    }];

    let composition = Composition {
        width: video_width,
        height: video_height,
        number,
        state: 0x80,
        palette_id: 0,
        objects,
    };

    let window = Window {
        id: 0,
        x: sub.x,
        y: sub.y,
        width: sub.width,
        height: sub.height,
    };

    let mut segments = Vec::new();
    segments.push(pcs(&composition)?);
    segments.push(wds(&[window])?);
    segments.push(pds(0, 0, &sub.palette)?);
    segments.extend(ods(0, 0, sub.width, sub.height, &rle)?);
    segments.push(end());

    Ok(timed_segments(sub.start, segments))
}

fn end_set(
    sub: &Subtitle,
    number: u16,
    video_width: u16,
    video_height: u16,
) -> Result<Vec<TimedSegment>, PgsError> {
    let composition = Composition {
        width: video_width,
        height: video_height,
        number,
        state: 0x00,
        palette_id: 0,
        objects: Vec::new(),
    };

    let window = Window {
        id: 0,
        x: sub.x,
        y: sub.y,
        width: sub.width,
        height: sub.height,
    };

    let segments = vec![pcs(&composition)?, wds(&[window])?, end()];

    Ok(timed_segments(sub.end, segments))
}

/// Encode a subtitle track: for each subtitle, at `start` a display set PCS (epoch start, composition number n, one object at x/y
/// in window 0) + WDS (window 0 = the bitmap rectangle) + PDS (palette 0, version 0) + ODS (object 0, version 0) + END; at `end` a
/// display set PCS (normal state 0x00, next composition number, no objects) + WDS (same window) + END. pts = dts for every segment.
/// Subtitles must be sorted and not overlap (else Err); the bitmap must lie inside the video size.
pub fn encode_subtitles(
    subtitles: &[Subtitle],
    video_width: u16,
    video_height: u16,
) -> Result<Vec<TimedSegment>, PgsError> {
    let mut output = Vec::new();
    let mut previous_end: Option<u64> = None;

    for (index, sub) in subtitles.iter().enumerate() {
        check_subtitle_order(sub, previous_end)?;
        let (number, next_number) = composition_numbers(index)?;
        validate_geometry(sub, video_width, video_height)?;

        let mut start = start_set(sub, number, video_width, video_height)?;
        output.append(&mut start);

        let mut end = end_set(sub, next_number, video_width, video_height)?;
        output.append(&mut end);

        previous_end = Some(sub.end);
    }

    Ok(output)
}

/// Serialize segments as a `.sup` file ("PG" + pts u32 + dts u32 + segment).
pub fn to_sup(segments: &[TimedSegment]) -> Vec<u8> {
    let mut output = Vec::new();

    for segment in segments {
        output.push(b'P');
        output.push(b'G');

        let pts_bytes = segment.pts.to_be_bytes();
        output.extend(pts_bytes.iter().skip(4).copied());

        let dts_bytes = segment.dts.to_be_bytes();
        output.extend(dts_bytes.iter().skip(4).copied());

        output.extend_from_slice(&segment.data);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::{PaletteEntry, Subtitle, TimedSegment, encode_subtitles, to_sup};

    fn sample_subtitle(start: u64, end: u64) -> Subtitle {
        Subtitle {
            start,
            end,
            x: 0,
            y: 0,
            width: 2,
            height: 2,
            pixels: vec![0u8, 1, 2, 3],
            palette: vec![PaletteEntry {
                id: 0,
                y: 16,
                cr: 128,
                cb: 128,
                alpha: 255,
            }],
        }
    }

    #[test]
    fn two_subtitles_emit_start_and_end_sets() {
        let subtitles = vec![sample_subtitle(0, 100), sample_subtitle(100, 200)];
        let segments = encode_subtitles(&subtitles, 4, 4).unwrap_or_default();

        // Each subtitle emits five start segments and three end segments.
        assert_eq!(segments.len(), 16);

        let expected_types = [
            0x16u8, 0x17, 0x14, 0x15, 0x80, // first start
            0x16, 0x17, 0x80, // first end
            0x16, 0x17, 0x14, 0x15, 0x80, // second start
            0x16, 0x17, 0x80, // second end
        ];

        for (index, expected) in expected_types.iter().enumerate() {
            assert_eq!(
                segments
                    .get(index)
                    .and_then(|segment| segment.data.first().copied()),
                Some(*expected)
            );
        }

        assert_eq!(segments.first().map(|segment| segment.pts), Some(0));
        assert_eq!(segments.get(5).map(|segment| segment.pts), Some(100));
        assert_eq!(segments.get(8).map(|segment| segment.pts), Some(100));
        assert_eq!(segments.get(13).map(|segment| segment.pts), Some(200));
    }

    #[test]
    fn overlapping_subtitles_are_rejected() {
        let subtitles = vec![sample_subtitle(0, 10), sample_subtitle(5, 15)];
        assert!(encode_subtitles(&subtitles, 4, 4).is_err());
    }

    #[test]
    fn outside_video_is_rejected() {
        let subtitles = vec![sample_subtitle(0, 10)];
        assert!(encode_subtitles(&subtitles, 1, 1).is_err());
    }

    #[test]
    fn sup_header_uses_low_32_bits() {
        let segment = TimedSegment {
            pts: 0x1234_5678,
            dts: 0x9abc_def0,
            data: vec![0x14u8, 0x00, 0x00],
        };

        let sup = to_sup(&[segment]);
        let expected = vec![
            b'P', b'G', 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x14, 0x00, 0x00,
        ];

        assert_eq!(sup, expected);
    }
}
