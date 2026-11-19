//! How a video file's frames are scanned (Encore's フィールドタイプ of an asset: no fields, odd / upper field first,
//! even / lower field first), read from its headers without decoding it: an MPEG-2 stream's sequence extension and
//! first picture coding extension (ISO/IEC 13818-2 6.2.2.3, 6.2.3.1), or an H.264 sequence parameter set's
//! frame_mbs_only_flag (ITU-T H.264 7.3.2.1.1), found as Annex B or in an `avcC` record (an MP4's or a Matroska
//! file's decoder configuration, ISO/IEC 14496-15 5.2.4.1).

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// How a video's frames are scanned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    /// Whole frames (progressive scan).
    Progressive,
    /// Two fields a frame, the upper (odd) one first.
    UpperFirst,
    /// Two fields a frame, the lower (even) one first.
    LowerFirst,
}

impl FieldType {
    /// The name in command results.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            FieldType::Progressive => "progressive",
            FieldType::UpperFirst => "upperFirst",
            FieldType::LowerFirst => "lowerFirst",
        }
    }
}

/// How much of the start and of the end of a file is read: an MP4's `moov` (with its `avcC`) is often at the end.
const PART: u64 = 4 << 20;

/// How the frames of the video at `path` are scanned, when its headers say (MPEG-2 or H.264); `None` otherwise.
#[must_use]
pub fn field_type(path: &Path) -> Option<FieldType> {
    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let mut head = Vec::new();
    let _ = (&mut file).take(PART).read_to_end(&mut head).ok()?;
    // MPEG first: its slice start codes (00 00 01 07, …) would pass for H.264 parameter sets.
    if let Some(found) = mpeg2(&head)
        .or_else(|| h264_annex_b(&head))
        .or_else(|| h264_avcc(&head))
    {
        return Some(found);
    }
    if length <= PART {
        return None;
    }
    let _ = file.seek(SeekFrom::Start(length - PART)).ok()?;
    let mut tail = Vec::new();
    let _ = file.take(PART).read_to_end(&mut tail).ok()?;
    h264_avcc(&tail)
}

/// The positions of the start codes (00 00 01) in `data`.
fn start_codes(data: &[u8]) -> impl Iterator<Item = usize> + '_ {
    data.array_windows()
        .enumerate()
        .filter(|(_, window)| **window == [0, 0, 1])
        .map(|(at, _)| at)
}

/// An MPEG-1 or MPEG-2 video stream's scan, from its first sequence header (one followed by what follows a sequence
/// header, so other bytes are not taken for one): MPEG-1 (a GOP or a picture right after it) is progressive; MPEG-2
/// (its sequence extension right after it) by progressive_sequence, else by its first picture's progressive_frame
/// and top_field_first.
fn mpeg2(data: &[u8]) -> Option<FieldType> {
    let byte = |at: usize| data.get(at).copied();
    // The start code after a sequence header and its quantiser matrices (at most 140 bytes).
    let after = |header: usize| {
        start_codes(data.get(header + 4..(header + 160).min(data.len()))?)
            .map(|at| header + 4 + at)
            .next()
    };
    let (extension, code) = start_codes(data)
        .filter(|&at| byte(at + 3) == Some(0xB3))
        .find_map(|header| {
            let next = after(header)?;
            let code = byte(next + 3)?;
            let extension = code == 0xB5 && byte(next + 4).is_some_and(|b| b >> 4 == 1);
            (extension || code == 0xB8 || code == 0x00).then_some((next, code))
        })?;
    if code != 0xB5 {
        return Some(FieldType::Progressive);
    }
    if byte(extension + 5)? & 0x08 != 0 {
        return Some(FieldType::Progressive);
    }
    // The first picture's coding extension (identifier 8): top_field_first is the top bit of its fourth byte,
    // progressive_frame of its fifth.
    let coding = start_codes(data.get(extension..)?)
        .map(|at| extension + at)
        .find(|&at| byte(at + 3) == Some(0xB5) && byte(at + 4).is_some_and(|b| b >> 4 == 8))?;
    let (flags, progressive) = (byte(coding + 7)?, byte(coding + 8)?);
    Some(if progressive & 0x80 != 0 {
        FieldType::Progressive
    } else if flags & 0x80 != 0 {
        FieldType::UpperFirst
    } else {
        FieldType::LowerFirst
    })
}

/// The scan an H.264 sequence parameter set says: frames, or fields (taken as upper first: the order is in each
/// picture's timing, which is not read here, and upper first is what interlaced HD video has).
fn of_sps(sps: crate::h264_sps::Sps) -> FieldType {
    if sps.frame_mbs_only {
        FieldType::Progressive
    } else {
        FieldType::UpperFirst
    }
}

/// The first Annex B sequence parameter set's scan (an elementary stream, a transport stream).
fn h264_annex_b(data: &[u8]) -> Option<FieldType> {
    crate::h264_sps::first_in_annex_b(data).map(of_sps)
}

/// The scan of the first sequence parameter set of an `avcC` record: configurationVersion 1, the profile, its
/// compatibility and level, the NAL length size (top six bits set), one SPS (top three bits set), its length and its
/// NAL unit (type 7).
fn h264_avcc(data: &[u8]) -> Option<FieldType> {
    data.array_windows::<9>()
        .enumerate()
        .filter(|&(_, &[version, _, _, _, size, count, _, _, nal])| {
            version == 1
                && size & 0xFC == 0xFC
                && count & 0xE0 == 0xE0
                && count & 0x1F >= 1
                && nal & 0x1F == 7
        })
        .find_map(|(at, &[_, profile, _, level, _, _, high, low, _])| {
            let length = usize::from(u16::from_be_bytes([high, low]));
            let nal = data.get(at + 9..at + 8 + length)?;
            // The record repeats the SPS's profile and level.
            let sps = crate::h264_sps::parse(nal)?;
            (sps.profile == profile && sps.level == level).then_some(of_sps(sps))
        })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
    use super::{FieldType, h264_annex_b, h264_avcc, mpeg2};

    /// A sequence header (720x480, 29.97) followed by a sequence extension (progressive_sequence as given), a GOP,
    /// a picture and its coding extension (top_field_first and progressive_frame as given).
    fn mpeg2_head(progressive_sequence: bool, top_first: bool, progressive_frame: bool) -> Vec<u8> {
        let mut data = vec![
            0, 0, 1, 0xB3, 0x2D, 0x01, 0xE0, 0x24, 0xFF, 0xFF, 0xE0, 0x18,
        ];
        data.extend([
            0,
            0,
            1,
            0xB5,
            0x14,
            if progressive_sequence { 0x8A } else { 0x82 },
            0,
            0x01,
            0,
            0,
        ]);
        data.extend([0, 0, 1, 0xB8, 0, 0x08, 0, 0x40]);
        data.extend([0, 0, 1, 0x00, 0, 0x0F, 0xFF, 0xF8]);
        data.extend([
            0,
            0,
            1,
            0xB5,
            0x8F,
            0xFF,
            0xF3,
            if top_first { 0x80 } else { 0x00 } | 0x41,
            if progressive_frame { 0x80 } else { 0x00 },
        ]);
        data
    }

    /// M6.318: an MPEG-2 stream's scan from its headers: interlaced upper / lower field first, a progressive frame
    /// or sequence; MPEG-1 (no extension) is progressive; no sequence header, nothing.
    #[test]
    fn an_mpeg2_streams_fields_are_read_from_its_headers() {
        assert_eq!(
            mpeg2(&mpeg2_head(false, true, false)),
            Some(FieldType::UpperFirst)
        );
        assert_eq!(
            mpeg2(&mpeg2_head(false, false, false)),
            Some(FieldType::LowerFirst)
        );
        assert_eq!(
            mpeg2(&mpeg2_head(false, true, true)),
            Some(FieldType::Progressive)
        );
        assert_eq!(
            mpeg2(&mpeg2_head(true, true, false)),
            Some(FieldType::Progressive)
        );
        let mpeg1 = vec![
            0, 0, 1, 0xB3, 0x2D, 0x01, 0xE0, 0x24, 0xFF, 0xFF, 0xE0, 0x18, 0, 0, 1, 0xB8, 0, 0,
        ];
        assert_eq!(mpeg2(&mpeg1), Some(FieldType::Progressive));
        assert_eq!(mpeg2(&[0, 0, 1, 0xB5, 0x8F, 0xFF, 0xF3, 0x80, 0]), None);
        assert_eq!(mpeg2(&[]), None);
        // Cut short: nothing to read is no answer, not a fault.
        let head = mpeg2_head(false, true, false);
        assert_eq!(mpeg2(&head[..head.len() - 2]), None);
    }

    /// The sequence parameter set of a 1080i High@4.1 stream (frame_mbs_only_flag 0), as `h264_sps` tests it.
    const INTERLACED_SPS: [u8; 41] = [
        0x64, 0x00, 0x29, 0xac, 0xd1, 0x40, 0x78, 0x04, 0x4f, 0xde, 0x02, 0x84, 0x04, 0x04, 0x05,
        0x00, 0x00, 0x03, 0x03, 0xe9, 0x00, 0x00, 0xea, 0x60, 0xe6, 0xa0, 0x00, 0x17, 0xa6, 0xb0,
        0x00, 0x0b, 0xd3, 0x5a, 0x4c, 0x30, 0x0f, 0x8b, 0x16, 0x00, 0x00,
    ];

    /// M6.318: an H.264 stream's scan from its sequence parameter set, found as Annex B or in an `avcC` record.
    #[test]
    fn an_h264_streams_fields_are_read_from_its_sequence_parameter_set() {
        let expected = FieldType::UpperFirst;
        let mut annex_b = vec![0xAA, 0, 0, 0, 1, 0x67];
        annex_b.extend(INTERLACED_SPS);
        annex_b.extend([0, 0, 1, 0x68, 0xEE]);
        assert_eq!(h264_annex_b(&annex_b), Some(expected));
        let mut record = vec![
            0x11, b'a', b'v', b'c', b'C', 1, 0x64, 0x00, 0x29, 0xFF, 0xE1, 0x00,
        ];
        record.push(u8::try_from(INTERLACED_SPS.len() + 1).unwrap());
        record.push(0x67);
        record.extend(INTERLACED_SPS);
        record.extend([1, 0, 4, 0x68, 0xEE, 0x3C, 0x80]);
        assert_eq!(h264_avcc(&record), Some(expected));
        assert_eq!(h264_avcc(&record[..10]), None);
        assert_eq!(h264_annex_b(&[0, 0, 1, 0x65, 0x88]), None);
    }
}
