//! PGS segment builders.

use std::cmp::min;

use crate::PgsError;

const PDS_TYPE: u8 = 0x14;
const ODS_TYPE: u8 = 0x15;
const PCS_TYPE: u8 = 0x16;
const WDS_TYPE: u8 = 0x17;
const END_TYPE: u8 = 0x80;

const MAX_SEGMENT_BODY: usize = 65535;
const MAX_U24: u32 = 0xFF_FFFF;

fn err(message: &str) -> PgsError {
    PgsError(message.to_string())
}

fn build_segment(segment_type: u8, body: Vec<u8>) -> Result<Vec<u8>, PgsError> {
    let length = u16::try_from(body.len()).map_err(|_| err("segment body too large"))?;

    let mut out = Vec::with_capacity(3 + usize::from(length));
    out.push(segment_type);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

fn push_u24(out: &mut Vec<u8>, value: u32) {
    let bytes = value.to_be_bytes();
    out.extend(bytes.iter().skip(1).copied());
}

fn push_object_header(body: &mut Vec<u8>, object_id: u16, version: u8, flags: u8, data_len: u32) {
    body.extend_from_slice(&object_id.to_be_bytes());
    body.push(version);
    body.push(flags);
    push_u24(body, data_len);
}

fn clamp_to_range(value: i32, min_value: i32, max_value: i32) -> u8 {
    let clamped = value.clamp(min_value, max_value);
    u8::try_from(clamped).unwrap_or_default()
}

/// A palette entry in YCrCb with alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteEntry {
    /// Palette entry id.
    pub id: u8,
    /// Luma component.
    pub y: u8,
    /// Chroma red component.
    pub cr: u8,
    /// Chroma blue component.
    pub cb: u8,
    /// Alpha component.
    pub alpha: u8,
}

/// Convert RGBA to a BT.601 limited-range palette entry.
pub fn rgba_to_entry(id: u8, r: u8, g: u8, b: u8, a: u8) -> PaletteEntry {
    let rf = i32::from(r);
    let gf = i32::from(g);
    let bf = i32::from(b);

    let y = ((66 * rf + 129 * gf + 25 * bf + 128) >> 8) + 16;
    let cb = ((-38 * rf - 74 * gf + 112 * bf + 128) >> 8) + 128;
    let cr = ((112 * rf - 94 * gf - 18 * bf + 128) >> 8) + 128;

    PaletteEntry {
        id,
        y: clamp_to_range(y, 16, 235),
        cr: clamp_to_range(cr, 16, 240),
        cb: clamp_to_range(cb, 16, 240),
        alpha: a,
    }
}

/// Build a palette definition segment (PDS).
pub fn pds(palette_id: u8, version: u8, entries: &[PaletteEntry]) -> Result<Vec<u8>, PgsError> {
    let mut body = Vec::new();
    body.push(palette_id);
    body.push(version);

    for entry in entries {
        body.push(entry.id);
        body.push(entry.y);
        body.push(entry.cr);
        body.push(entry.cb);
        body.push(entry.alpha);
    }

    build_segment(PDS_TYPE, body)
}

fn ods_single(
    object_id: u16,
    version: u8,
    width: u16,
    height: u16,
    rle: &[u8],
    data_len: u32,
) -> Result<Vec<Vec<u8>>, PgsError> {
    let mut body = Vec::new();
    push_object_header(&mut body, object_id, version, 0xC0, data_len);
    body.extend_from_slice(&width.to_be_bytes());
    body.extend_from_slice(&height.to_be_bytes());
    body.extend_from_slice(rle);

    Ok(vec![build_segment(ODS_TYPE, body)?])
}

fn ods_split(
    object_id: u16,
    version: u8,
    width: u16,
    height: u16,
    rle: &[u8],
    data_len: u32,
) -> Result<Vec<Vec<u8>>, PgsError> {
    let first_header_len = 11usize;
    let max_first_rle = MAX_SEGMENT_BODY
        .checked_sub(first_header_len)
        .ok_or_else(|| err("invalid first ODS header"))?;

    let first_len = min(rle.len(), max_first_rle);
    let (first, rest) = rle.split_at(first_len);

    if rest.is_empty() {
        return Err(err("object data split produced no continuation"));
    }

    let mut segments = Vec::new();

    let mut body = Vec::new();
    push_object_header(&mut body, object_id, version, 0x40, data_len);
    body.extend_from_slice(&width.to_be_bytes());
    body.extend_from_slice(&height.to_be_bytes());
    body.extend_from_slice(first);
    segments.push(build_segment(ODS_TYPE, body)?);

    let mut remaining = rest;
    let continuation_header_len = 4usize;
    let max_part = MAX_SEGMENT_BODY
        .checked_sub(continuation_header_len)
        .ok_or_else(|| err("invalid continuation ODS header"))?;

    while !remaining.is_empty() {
        let part_len = min(remaining.len(), max_part);
        let (part, next) = remaining.split_at(part_len);
        let flags = if next.is_empty() { 0x80 } else { 0x00 };

        let mut body = Vec::new();
        body.extend_from_slice(&object_id.to_be_bytes());
        body.push(version);
        body.push(flags);
        body.extend_from_slice(part);

        segments.push(build_segment(ODS_TYPE, body)?);
        remaining = next;
    }

    Ok(segments)
}

/// Build one or more object definition segments (ODS).
pub fn ods(
    object_id: u16,
    version: u8,
    width: u16,
    height: u16,
    rle: &[u8],
) -> Result<Vec<Vec<u8>>, PgsError> {
    let data_len = 4usize
        .checked_add(rle.len())
        .ok_or_else(|| err("object data length overflow"))?;
    let data_len_u32 = u32::try_from(data_len).map_err(|_| err("object data length too large"))?;

    if data_len_u32 > MAX_U24 {
        return Err(err("object data length exceeds 24 bits"));
    }

    if data_len <= MAX_SEGMENT_BODY {
        return ods_single(object_id, version, width, height, rle, data_len_u32);
    }

    ods_split(object_id, version, width, height, rle, data_len_u32)
}

/// Build an end segment (END).
pub fn end() -> Vec<u8> {
    vec![END_TYPE, 0x00, 0x00]
}

/// A presentation window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// Window id.
    pub id: u8,
    /// X coordinate.
    pub x: u16,
    /// Y coordinate.
    pub y: u16,
    /// Window width.
    pub width: u16,
    /// Window height.
    pub height: u16,
}

/// Build a window definition segment (WDS).
pub fn wds(windows: &[Window]) -> Result<Vec<u8>, PgsError> {
    let count = u8::try_from(windows.len()).map_err(|_| err("too many windows"))?;

    let mut body = Vec::new();
    body.push(count);

    for window in windows {
        body.push(window.id);
        body.extend_from_slice(&window.x.to_be_bytes());
        body.extend_from_slice(&window.y.to_be_bytes());
        body.extend_from_slice(&window.width.to_be_bytes());
        body.extend_from_slice(&window.height.to_be_bytes());
    }

    build_segment(WDS_TYPE, body)
}

/// A composition object reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompositionObject {
    /// Object id.
    pub object_id: u16,
    /// Window id.
    pub window_id: u8,
    /// X coordinate.
    pub x: u16,
    /// Y coordinate.
    pub y: u16,
}

/// A presentation composition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composition {
    /// Presentation width.
    pub width: u16,
    /// Presentation height.
    pub height: u16,
    /// Composition number.
    pub number: u16,
    /// Composition state.
    pub state: u8,
    /// Palette id.
    pub palette_id: u8,
    /// Composition objects.
    pub objects: Vec<CompositionObject>,
}

/// Build a presentation composition segment (PCS).
pub fn pcs(c: &Composition) -> Result<Vec<u8>, PgsError> {
    let count = u8::try_from(c.objects.len()).map_err(|_| err("too many composition objects"))?;

    let mut body = Vec::new();
    body.extend_from_slice(&c.width.to_be_bytes());
    body.extend_from_slice(&c.height.to_be_bytes());
    body.push(0x10);
    body.extend_from_slice(&c.number.to_be_bytes());
    body.push(c.state);
    body.push(0x00);
    body.push(c.palette_id);
    body.push(count);

    for object in &c.objects {
        body.extend_from_slice(&object.object_id.to_be_bytes());
        body.push(object.window_id);
        body.push(0x00);
        body.extend_from_slice(&object.x.to_be_bytes());
        body.extend_from_slice(&object.y.to_be_bytes());
    }

    build_segment(PCS_TYPE, body)
}

#[cfg(test)]
mod tests {
    use super::{
        Composition, CompositionObject, PaletteEntry, Window, end, ods, pcs, pds, rgba_to_entry,
        wds,
    };

    #[test]
    fn end_segment() {
        assert_eq!(end(), vec![0x80u8, 0x00, 0x00]);
    }

    #[test]
    fn pds_bytes() {
        let entries = vec![PaletteEntry {
            id: 0,
            y: 16,
            cr: 128,
            cb: 128,
            alpha: 255,
        }];

        let segment = pds(0, 0, &entries).unwrap_or_default();

        let expected = vec![0x14u8, 0x00, 0x07, 0x00, 0x00, 0x00, 0x10, 0x80, 0x80, 0xFF];
        assert_eq!(segment, expected);
    }

    #[test]
    fn wds_bytes() {
        let windows = vec![Window {
            id: 0,
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        }];

        let segment = wds(&windows).unwrap_or_default();

        let expected = vec![
            0x17u8, 0x00, 0x0A, 0x01, 0x00, 0x00, 0x01, 0x00, 0x02, 0x00, 0x03, 0x00, 0x04,
        ];
        assert_eq!(segment, expected);
    }

    #[test]
    fn pcs_bytes() {
        let composition = Composition {
            width: 4,
            height: 5,
            number: 6,
            state: 0x80,
            palette_id: 0,
            objects: vec![CompositionObject {
                object_id: 7,
                window_id: 0,
                x: 1,
                y: 2,
            }],
        };

        let segment = pcs(&composition).unwrap_or_default();

        let expected = vec![
            0x16u8, 0x00, 0x13, 0x00, 0x04, 0x00, 0x05, 0x10, 0x00, 0x06, 0x80, 0x00, 0x00, 0x01,
            0x00, 0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x02,
        ];
        assert_eq!(segment, expected);
    }

    #[test]
    fn ods_single_bytes() {
        let segments = ods(1, 2, 3, 4, &[0x00u8, 0x00]).unwrap_or_default();

        let expected = vec![
            0x15u8, 0x00, 0x0D, 0x00, 0x01, 0x02, 0xC0, 0x00, 0x00, 0x06, 0x00, 0x03, 0x00, 0x04,
            0x00, 0x00,
        ];

        assert_eq!(segments, vec![expected]);
    }

    #[test]
    fn ods_splits_at_65535() {
        let rle = vec![0u8; 70000];
        let segments = ods(0, 0, 1, 1, &rle).unwrap_or_default();

        assert_eq!(segments.len(), 2);
        assert_eq!(segments.first().map(|segment| segment.len()), Some(65538));
        assert_eq!(segments.get(1).map(|segment| segment.len()), Some(4483));
        assert_eq!(
            segments.first().and_then(|segment| segment.get(6)).copied(),
            Some(0x40)
        );
        assert_eq!(
            segments.get(1).and_then(|segment| segment.get(6)).copied(),
            Some(0x80)
        );
    }

    #[test]
    fn rgba_to_entry_black_and_white() {
        let black = rgba_to_entry(0, 0, 0, 0, 255);
        assert_eq!(black.y, 16);
        assert_eq!(black.cr, 128);
        assert_eq!(black.cb, 128);
        assert_eq!(black.alpha, 255);

        let white = rgba_to_entry(0, 255, 255, 255, 0);
        assert_eq!(white.y, 235);
        assert_eq!(white.cr, 128);
        assert_eq!(white.cb, 128);
        assert_eq!(white.alpha, 0);
    }
}
