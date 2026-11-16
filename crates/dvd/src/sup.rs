//! PGS (`.sup`): a Blu-ray's own subtitles as a file (its presentation graphics segments with their times), as
//! ffmpeg, tsMuxeR, BDSup2Sub and other tools write them from a disc. A Blu-ray build passes the segments through.

use std::path::Path;

use crate::DvdError;

/// One segment of a `.sup`: its time stamps (90 kHz, as in the file) and its bytes (type, length and data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgSegment {
    /// Presentation time stamp, 90 kHz.
    pub pts: i64,
    /// Decoding time stamp, 90 kHz.
    pub dts: i64,
    /// The segment: type (1 byte), length (2 bytes) and data.
    pub data: Vec<u8>,
}

/// Reads every segment of a `.sup`, with its times as written: 90 kHz from the first picture of the video it
/// belongs to, as a subtitle file's times are.
///
/// # Errors
///
/// When the file cannot be read or is not PGS.
pub(crate) fn read_sup(path: &Path) -> Result<Vec<PgSegment>, DvdError> {
    let bytes = std::fs::read(path).map_err(|err| crate::io_error(path, err))?;
    parse_sup(&bytes)
        .ok_or_else(|| DvdError::Invalid(format!("{}: not a PGS (.sup) file", path.display())))
}

fn parse_sup(bytes: &[u8]) -> Option<Vec<PgSegment>> {
    let mut segments = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        let header = bytes.get(at..at + 13)?;
        if &header[..2] != b"PG" {
            return None;
        }
        let pts = i64::from(u32::from_be_bytes([
            header[2], header[3], header[4], header[5],
        ]));
        let dts = i64::from(u32::from_be_bytes([
            header[6], header[7], header[8], header[9],
        ]));
        let length = usize::from(u16::from_be_bytes([header[11], header[12]]));
        let data = bytes.get(at + 10..at + 13 + length)?.to_vec();
        segments.push(PgSegment { pts, dts, data });
        at += 13 + length;
    }
    (!segments.is_empty()).then_some(segments)
}

#[cfg(test)]
mod tests {
    use super::parse_sup;

    #[test]
    fn segments_and_times_are_read() {
        let mut sup = Vec::new();
        for (pts, kind, data) in [(90_000u32, 0x16u8, vec![1, 2, 3]), (90_000, 0x80, vec![])] {
            sup.extend_from_slice(b"PG");
            sup.extend_from_slice(&pts.to_be_bytes());
            sup.extend_from_slice(&0u32.to_be_bytes());
            sup.push(kind);
            sup.extend_from_slice(&(data.len() as u16).to_be_bytes());
            sup.extend_from_slice(&data);
        }
        let segments = parse_sup(&sup).unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].pts, 90_000);
        assert_eq!(segments[0].data, vec![0x16, 0, 3, 1, 2, 3]);
        assert_eq!(segments[1].data, vec![0x80, 0, 0]);
        assert_eq!(parse_sup(b"XX"), None);
    }
}
