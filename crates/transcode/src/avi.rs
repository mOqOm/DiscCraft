//! The parts of an AVI file DiscCraft reads itself (M6.410, M6.417): its streams' headers, and every chunk of one
//! stream in the `movi` lists of the `AVI ` part and of each OpenDML `AVIX` part (files over 1 GB). From Microsoft's
//! AVI RIFF file reference and the OpenDML AVI file format extensions. The media reader opens AVIs too, but gives no
//! way to its chunks; DV and MPEG video in an AVI are read through these.

use filmcraft_media::reader::{SharedReader, read_range};

/// The most RIFF parts and chunks read from an AVI (10 hours at 29.97 frames a second).
const MOST_PARTS: usize = 1_024;
pub(crate) const MOST_CHUNKS: usize = 1_100_000;
/// The longest `strl` list read whole (a stream's header, format and an OpenDML index's header).
const MOST_STREAM_LIST: u32 = 1 << 20;

/// A stream's header: its type (`vids`, `auds`, `iavs`, …), its handler, its format's compression (the
/// `biCompression` of a video stream's BITMAPINFOHEADER), and its time base: a chunk of a video stream lasts
/// `scale / rate` seconds (`dwScale` and `dwRate`; 0 when the header does not say).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Stream {
    pub(crate) kind: [u8; 4],
    pub(crate) handler: [u8; 4],
    pub(crate) compression: [u8; 4],
    pub(crate) scale: u32,
    pub(crate) rate: u32,
}

/// An AVI's streams (in order: the index is the stream's number) and where its `movi` lists are.
pub(crate) struct Avi {
    pub(crate) streams: Vec<Stream>,
    movies: Vec<(u64, u64)>,
}

/// `len` bytes at `at`, when the file has them all.
pub(crate) fn read(reader: &SharedReader, at: u64, len: usize) -> Option<Vec<u8>> {
    read_range(&**reader, at, len)
        .ok()
        .filter(|bytes| bytes.len() == len)
}

pub(crate) fn le32(bytes: &[u8], at: usize) -> u32 {
    bytes
        .get(at..at + 4)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map_or(0, u32::from_le_bytes)
}

/// The four bytes at `at` (zeros past the end).
pub(crate) fn code_at(bytes: &[u8], at: usize) -> [u8; 4] {
    bytes
        .get(at..at + 4)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .unwrap_or_default()
}

/// `code` in capitals.
pub(crate) fn upper(code: &[u8]) -> [u8; 4] {
    let mut upper = [0u8; 4];
    for (to, from) in upper.iter_mut().zip(code) {
        *to = from.to_ascii_uppercase();
    }
    upper
}

/// Whether `head` (the start of a file) is an AVI with a stream for which `wanted` holds among the stream headers
/// (`strh`, and the formats `strf` of video streams) it holds: what a file is, from its first bytes.
pub(crate) fn head_has_stream(head: &[u8], wanted: impl Fn(&Stream) -> bool) -> bool {
    if head.get(..4) != Some(b"RIFF") || head.get(8..12) != Some(b"AVI ") {
        return false;
    }
    let mut last = Stream::default();
    head.windows(4).enumerate().any(|(at, id)| {
        let body = at + 8;
        match id {
            b"strh" => {
                last = Stream {
                    kind: code_at(head, body),
                    handler: code_at(head, body + 4),
                    ..Stream::default()
                };
                wanted(&last)
            }
            b"strf" => {
                last.compression = code_at(head, body + 16);
                wanted(&last)
            }
            _ => false,
        }
    })
}

/// Reads the streams of the AVI `reader`: `None` when it is not an AVI.
pub(crate) fn read_avi(reader: &SharedReader) -> Option<Avi> {
    let length = reader.len();
    let mut avi = Avi {
        streams: Vec::new(),
        movies: Vec::new(),
    };
    let mut at = 0u64;
    for _ in 0..MOST_PARTS {
        let Some(head) = read(reader, at, 12) else {
            break;
        };
        if code_at(&head, 0) != *b"RIFF" || (at == 0 && code_at(&head, 8) != *b"AVI ") {
            break;
        }
        let size = u64::from(le32(&head, 4));
        let end = at.saturating_add(8).saturating_add(size).min(length);
        walk_lists(reader, at + 12, end, &mut avi, 0);
        at = end.saturating_add(size & 1);
    }
    (!avi.streams.is_empty()).then_some(avi)
}

/// Walks the chunks between `start` and `end`: the stream lists of `hdrl`, and where each `movi` list is.
fn walk_lists(reader: &SharedReader, start: u64, end: u64, avi: &mut Avi, depth: u32) {
    let mut at = start;
    for _ in 0..4_096 {
        let Some(head) = read(reader, at, 12) else {
            return;
        };
        if at.saturating_add(8) > end {
            return;
        }
        let size = u64::from(le32(&head, 4));
        let body_end = at.saturating_add(8).saturating_add(size).min(end);
        if code_at(&head, 0) == *b"LIST" {
            match &code_at(&head, 8) {
                b"hdrl" if depth == 0 => walk_lists(reader, at + 12, body_end, avi, depth + 1),
                b"strl" => avi.streams.push(stream(reader, at + 12, body_end)),
                b"movi" => avi.movies.push((at + 12, body_end)),
                _ => {}
            }
        }
        at = body_end.saturating_add(size & 1);
        if at >= end {
            return;
        }
    }
}

/// The header of the stream whose `strl` list is between `start` and `end`.
fn stream(reader: &SharedReader, start: u64, end: u64) -> Stream {
    let mut found = Stream::default();
    let length =
        usize::try_from(end.saturating_sub(start).min(u64::from(MOST_STREAM_LIST))).unwrap_or(0);
    let Some(list) = read(reader, start, length) else {
        return found;
    };
    let mut at = 0usize;
    while let Some(id) = list.get(at..at + 4) {
        let size = le32(&list, at + 4) as usize;
        let body = at + 8;
        match id {
            b"strh" => {
                found.kind = code_at(&list, body);
                found.handler = code_at(&list, body + 4);
                // fccType, fccHandler, dwFlags, wPriority, wLanguage, dwInitialFrames, then dwScale and dwRate.
                found.scale = le32(&list, body + 20);
                found.rate = le32(&list, body + 24);
            }
            b"strf" => found.compression = code_at(&list, body + 16),
            _ => {}
        }
        at = body.saturating_add(size).saturating_add(size & 1);
    }
    found
}

impl Avi {
    /// Whether a stream is of type `kind`.
    pub(crate) fn has(&self, kind: &[u8; 4]) -> bool {
        self.streams.iter().any(|stream| &stream.kind == kind)
    }

    /// The chunks of stream `number` (`00dc`, `00db`, …, going into `rec ` lists) in file order: where each one's
    /// data is and its length (0 for an empty chunk: the frame before it repeated).
    pub(crate) fn chunks(&self, reader: &SharedReader, number: usize) -> Vec<(u64, u32)> {
        let digits = [b'0' + (number / 10 % 10) as u8, b'0' + (number % 10) as u8];
        let mut chunks = Vec::new();
        for &(start, end) in &self.movies {
            collect_chunks(reader, (start, end), digits, &mut chunks, 0);
            if chunks.len() >= MOST_CHUNKS {
                break;
            }
        }
        chunks.truncate(MOST_CHUNKS);
        chunks
    }
}

/// Collects the chunks of stream `digits` between `start` and `end`, going into `rec ` lists.
fn collect_chunks(
    reader: &SharedReader,
    (start, end): (u64, u64),
    digits: [u8; 2],
    chunks: &mut Vec<(u64, u32)>,
    depth: u32,
) {
    let mut at = start;
    while at.saturating_add(8) <= end && chunks.len() < MOST_CHUNKS {
        let Some(head) = read(reader, at, 12) else {
            return;
        };
        let size = le32(&head, 4);
        let body_end = at
            .saturating_add(8)
            .saturating_add(u64::from(size))
            .min(end);
        let id = code_at(&head, 0);
        if id == *b"LIST" {
            if depth < 2 {
                collect_chunks(reader, (at + 12, body_end), digits, chunks, depth + 1);
            }
        } else if [id[0], id[1]] == digits {
            chunks.push((at + 8, size));
        }
        let next = body_end.saturating_add(u64::from(size & 1));
        if next <= at {
            return;
        }
        at = next;
    }
}
