//! The stream types a transport stream's programme lists, from its first few megabytes (M6.429).
//!
//! Whether a `.m2ts` / `.ts` could go on a DVD as it is (MPEG-2 video) was asked of its index, and the index reads
//! the whole file: minutes for a Blu-ray's clips of ten gigabytes, before a timeline of them could even be shown.
//! The programme map table near the start says which streams the file has, so a file without MPEG-2 video is
//! answered at once. ISO/IEC 13818-1 2.4.3 (transport packets), 2.4.4.3 (the programme association table) and
//! 2.4.4.8 (the programme map table): the same tables DiscCraft's own multiplexers write.

use std::io::Read;
use std::path::Path;

/// How much of the file is looked through for the tables.
const HEAD_BYTES: u64 = 4 * 1024 * 1024;
/// A transport packet's sync byte.
const SYNC: u8 = 0x47;
/// The most bytes one table section holds (a 12-bit length).
const MAX_SECTION: usize = 4096;

/// The stream types of the first programme of the transport stream at `path` (0x01 / 0x02 MPEG video, 0x1B H.264,
/// 0x80 LPCM, 0x81 AC-3, ...), in the order the table lists them; `None` for a file that is not a transport stream
/// or whose tables are not in its first megabytes.
#[must_use]
pub fn ts_stream_types(path: &Path) -> Option<Vec<u8>> {
    let mut head = Vec::new();
    let _ = std::fs::File::open(path)
        .ok()?
        .take(HEAD_BYTES)
        .read_to_end(&mut head)
        .ok()?;
    stream_types(&head)
}

/// [`ts_stream_types`] of the bytes of a file's head.
fn stream_types(head: &[u8]) -> Option<Vec<u8>> {
    // 188-byte packets, or a Blu-ray's 192 (four bytes in front of each).
    let synced = |first: usize, size: usize| {
        (0..3).all(|n| head.get(first + n * size).copied() == Some(SYNC))
    };
    let (first, size) = if synced(0, 188) {
        (0, 188)
    } else if synced(4, 192) {
        (4, 192)
    } else {
        return None;
    };
    let packets = || {
        head.get(first..)
            .unwrap_or_default()
            .chunks(size)
            .filter_map(|chunk| chunk.get(..188))
    };
    let pat = section(packets(), 0)?;
    // The PAT's programmes: number (2), PID (2); number 0 is the network PID.
    let entries = pat.get(8..pat.len().checked_sub(4)?)?;
    let pmt_pid = entries
        .as_chunks::<4>()
        .0
        .iter()
        .find_map(|&[a, b, c, d]| {
            let number = u16::from_be_bytes([a, b]);
            let pid = u16::from_be_bytes([c, d]) & 0x1FFF;
            (number != 0).then_some(pid)
        })?;
    let pmt = section(packets(), pmt_pid)?;
    if pmt.first() != Some(&0x02) {
        return None;
    }
    // After the fixed 12 bytes and the programme descriptors: stream type (1), PID (2), ES info length (2) and
    // its descriptors, up to the CRC.
    let info = usize::from(u16::from_be_bytes([*pmt.get(10)?, *pmt.get(11)?]) & 0x0FFF);
    let end = pmt.len().checked_sub(4)?;
    let mut at = 12usize.checked_add(info)?;
    let mut types = Vec::new();
    while at.checked_add(5)? <= end {
        types.push(*pmt.get(at)?);
        let es_info =
            usize::from(u16::from_be_bytes([*pmt.get(at + 3)?, *pmt.get(at + 4)?]) & 0x0FFF);
        at = at.checked_add(5)?.checked_add(es_info)?;
    }
    Some(types)
}

/// The first whole table section carried on `pid`: from the packet that starts it (after its pointer field), as long
/// as its section length says, across packets.
fn section<'a>(packets: impl Iterator<Item = &'a [u8]>, pid: u16) -> Option<Vec<u8>> {
    let mut collected: Option<Vec<u8>> = None;
    for packet in packets {
        let (&[sync, b1, b2, b3], rest) = packet.split_first_chunk::<4>()?;
        if sync != SYNC || ((u16::from(b1 & 0x1F) << 8) | u16::from(b2)) != pid {
            continue;
        }
        let control = (b3 >> 4) & 0b11;
        if control & 0b01 == 0 {
            continue;
        }
        let payload = if control & 0b10 != 0 {
            let length = usize::from(*rest.first()?);
            rest.get(1 + length..)?
        } else {
            rest
        };
        let start = b1 & 0x40 != 0;
        match (&mut collected, start) {
            (None, true) => {
                let pointer = usize::from(*payload.first()?);
                collected = Some(payload.get(1 + pointer..)?.to_vec());
            }
            (Some(bytes), false) => bytes.extend_from_slice(payload),
            (Some(_), true) | (None, false) => {}
        }
        if let Some(bytes) = &collected
            && bytes.len() >= 3
        {
            let length = usize::from(u16::from_be_bytes([*bytes.get(1)?, *bytes.get(2)?]) & 0x0FFF);
            if length > MAX_SECTION {
                return None;
            }
            if bytes.len() >= 3 + length {
                return bytes.get(..3 + length).map(<[u8]>::to_vec);
            }
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::stream_types;

    /// One transport packet of `pid` carrying a table section that starts in it.
    fn packet(pid: u16, section: &[u8]) -> Vec<u8> {
        let mut packet = vec![
            0x47,
            0x40 | ((pid >> 8) as u8 & 0x1F),
            (pid & 0xFF) as u8,
            0x10,
            0x00,
        ];
        packet.extend_from_slice(section);
        packet.resize(188, 0xFF);
        packet
    }

    /// A section: table id, length, the body, and four bytes standing for the CRC.
    fn table(id: u8, body: &[u8]) -> Vec<u8> {
        let length = body.len() + 4;
        let mut section = vec![
            id,
            0xB0 | ((length >> 8) as u8 & 0x0F),
            (length & 0xFF) as u8,
        ];
        section.extend_from_slice(body);
        section.extend_from_slice(&[0, 0, 0, 0]);
        section
    }

    fn stream(types: &[(u8, u16)], extra: usize) -> Vec<u8> {
        let pat = table(0x00, &[0, 1, 0xC1, 0, 0, 0, 1, 0xE1, 0x00]);
        let mut body = vec![0, 1, 0xC1, 0, 0, 0xF0, 0x01, 0xF0, 0x00];
        for (kind, pid) in types {
            body.extend_from_slice(&[
                *kind,
                0xE0 | (pid >> 8) as u8,
                (pid & 0xFF) as u8,
                0xF0,
                0x00,
            ]);
        }
        let pmt = table(0x02, &body);
        let mut out = Vec::new();
        for raw in [packet(0, &pat), packet(0x100, &pmt), packet(0x1011, &[])] {
            // A Blu-ray's four bytes in front, when asked.
            out.extend(std::iter::repeat_n(0u8, extra));
            out.extend(raw);
        }
        out
    }

    #[test]
    fn the_programmes_stream_types_are_read_from_the_head() {
        let types = [
            (0x1B, 0x1011),
            (0x80, 0x1100),
            (0x86, 0x1101),
            (0x90, 0x1200),
        ];
        assert_eq!(
            stream_types(&stream(&types, 0)),
            Some(vec![0x1B, 0x80, 0x86, 0x90])
        );
        assert_eq!(
            stream_types(&stream(&types, 4)),
            Some(vec![0x1B, 0x80, 0x86, 0x90])
        );
        assert_eq!(
            stream_types(&stream(&[(0x02, 0x1011)], 0)),
            Some(vec![0x02])
        );
        // Not a transport stream, or cut off before its tables: no answer.
        assert_eq!(stream_types(b"\x00\x00\x01\xBA not a TS"), None);
        let whole = stream(&types, 0);
        for cut in 0..whole.len() {
            let _ = stream_types(&whole[..cut]);
        }
    }
}
