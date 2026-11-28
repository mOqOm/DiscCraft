//! Frames, slices and lines (RFC 9043 §4.4 to §4.9) and the samples in them (§3).

use crate::coder::{BitReader, RangeDecoder, Transitions, VlcState};
use crate::params::{CONTEXT_SIZE, Parameters, TableSet, crc, read_parameters};
use crate::{Error, Layout, Picture};

/// The most pixels of a frame (8192 x 4320): a hostile size asks for no more memory.
const MOST_PIXELS: usize = 8192 * 4320;
/// The most slices of a frame.
const MOST_SLICES: usize = 4096;
/// The most memory the slices' adaptive states may take.
const MOST_STATE_BYTES: usize = 1 << 30;

/// `log2_run` (§3.8.2.2.1).
const LOG2_RUN: [u32; 41] = [
    0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 9, 10, 11, 12, 13,
    14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
];

/// The adaptive states of one context slot (a quantization table set index of a slice: luma, chroma, transparency).
#[derive(Debug, Clone)]
pub(crate) struct SlotState {
    set: usize,
    range: Vec<[u8; CONTEXT_SIZE]>,
    vlc: Vec<VlcState>,
}

impl SlotState {
    fn new(set_index: usize, set: &TableSet) -> Self {
        let range = match &set.initial {
            Some(initial) => initial.clone(),
            None => vec![[128u8; CONTEXT_SIZE]; set.contexts],
        };
        Self {
            set: set_index,
            range,
            vlc: vec![VlcState::default(); set.contexts],
        }
    }
}

/// What a slice keeps from one frame to the next (until a key frame): its slots' states.
#[derive(Debug, Clone, Default)]
pub(crate) struct SliceState {
    slots: Vec<SlotState>,
}

/// Where a slice's symbols come from.
enum Symbols<'a> {
    Range(RangeDecoder<'a>),
    Golomb(BitReader<'a>),
}

/// A slice's place in the frame and how its samples are coded.
struct SliceInfo {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    sets: Vec<usize>,
    picture_structure: u32,
    sar: (u32, u32),
}

/// The frame's planes being filled: one buffer of `u16` samples per plane, each `widths[p]` wide.
pub(crate) struct Planes {
    pub(crate) data: Vec<Vec<u16>>,
    pub(crate) widths: Vec<usize>,
    pub(crate) heights: Vec<usize>,
}

/// Decodes `frame` with `parameters` (from a configuration record, or `None` for versions 0 and 1, whose key frames
/// carry them: the last ones are kept in `kept`), carrying `slices`' states over from the frame before unless it is
/// a key frame.
pub(crate) fn decode_frame(
    frame: &[u8],
    (width, height): (usize, usize),
    record: Option<&Parameters>,
    kept: &mut Option<Parameters>,
    slices: &mut Vec<SliceState>,
) -> Result<Picture, Error> {
    if frame.is_empty() {
        return Err(Error::Corrupt("an empty frame"));
    }
    let mut first = RangeDecoder::new(frame);
    let mut key_state = 128u8;
    let tables_for_key = record.map_or_else(Transitions::default_tables, |record| {
        record.transitions.clone()
    });
    let keyframe = first.bit(&mut key_state, &tables_for_key);
    let parameters = match record {
        Some(record) => record.clone(),
        None => {
            if keyframe {
                let read = read_parameters(&mut first)?;
                if read.version >= 3 {
                    return Err(Error::Corrupt("version 3 parameters in a frame"));
                }
                *kept = Some(read);
            }
            kept.clone()
                .ok_or(Error::Corrupt("a frame before any key frame"))?
        }
    };
    if keyframe {
        slices.clear();
    }
    let mut planes = new_planes(&parameters, width, height)?;
    let mut structure = 0;
    let mut sar = (0, 0);
    if parameters.version >= 3 {
        let bounds = slice_bounds(frame, parameters.slice_crc)?;
        // Each slice keeps a range coder's 32 states and a Golomb-Rice state for every context of its sets.
        let contexts = parameters
            .sets
            .iter()
            .map(|set| set.contexts)
            .max()
            .unwrap_or(0);
        let per_slice = contexts
            .saturating_mul(parameters.index_count())
            .saturating_mul(CONTEXT_SIZE + 16);
        if bounds.len().saturating_mul(per_slice) > MOST_STATE_BYTES {
            return Err(Error::Unsupported(
                "so many slices and contexts".to_string(),
            ));
        }
        if !keyframe && slices.len() != bounds.len() {
            return Err(Error::Corrupt(
                "a frame whose slices differ from the key frame's",
            ));
        }
        if slices.len() < bounds.len() {
            slices.resize_with(bounds.len(), SliceState::default);
        }
        for (index, (start, size)) in bounds.into_iter().enumerate() {
            let bytes = frame
                .get(start..start + size)
                .ok_or(Error::Corrupt("a slice past the frame"))?;
            let mut coder = RangeDecoder::new(bytes);
            if index == 0 {
                let mut state = 128u8;
                let _keyframe = coder.bit(&mut state, &parameters.transitions);
            }
            let info = slice_header(&mut coder, &parameters, (width, height))?;
            if index == 0 {
                structure = info.picture_structure;
                sar = info.sar;
            }
            let state = slices
                .get_mut(index)
                .ok_or(Error::Corrupt("a slice state"))?;
            let symbols = symbols_after_header(coder, bytes, &parameters);
            decode_slice(symbols, &parameters, &info, state, &mut planes)?;
        }
    } else {
        let info = SliceInfo {
            x: 0,
            y: 0,
            width,
            height,
            sets: vec![0; parameters.index_count()],
            picture_structure: 0,
            sar: (0, 0),
        };
        if slices.is_empty() {
            slices.push(SliceState::default());
        }
        let state = slices.get_mut(0).ok_or(Error::Corrupt("a slice state"))?;
        let symbols = symbols_after_header(first, frame, &parameters);
        decode_slice(symbols, &parameters, &info, state, &mut planes)?;
    }
    Ok(finish(planes, &parameters, (width, height), structure, sar))
}

/// The coder of a slice's samples after its range-coded head: the same range decoder, or (Golomb-Rice) bits from
/// where the range-coded part ends, found by reading its closing symbol of state 129 (§3.8.1.1.1, sentinel mode).
fn symbols_after_header<'a>(
    mut coder: RangeDecoder<'a>,
    bytes: &'a [u8],
    parameters: &Parameters,
) -> Symbols<'a> {
    if parameters.coder != 0 {
        return Symbols::Range(coder);
    }
    let mut sentinel = 129u8;
    let _ = coder.bit(&mut sentinel, &parameters.transitions);
    let start = coder.bytes_read().saturating_sub(1);
    Symbols::Golomb(BitReader::new(bytes.get(start..).unwrap_or_default()))
}

/// Each slice's start and size, from the footers at the ends of the slices (§4.9), read from the frame's end.
fn slice_bounds(frame: &[u8], with_crc: bool) -> Result<Vec<(usize, usize)>, Error> {
    let footer = 3 + if with_crc { 5 } else { 0 };
    let mut bounds = Vec::new();
    let mut end = frame.len();
    while end > 0 {
        if bounds.len() >= MOST_SLICES || end < footer {
            return Err(Error::Corrupt("a slice footer past the frame's start"));
        }
        let at = end - footer;
        let size_bytes = frame
            .get(at..at + 3)
            .ok_or(Error::Corrupt("a slice footer"))?;
        let size = (usize::from(size_bytes[0]) << 16)
            | (usize::from(size_bytes[1]) << 8)
            | usize::from(size_bytes[2]);
        let start = at
            .checked_sub(size)
            .ok_or(Error::Corrupt("a slice larger than its frame"))?;
        if with_crc {
            let whole = frame.get(start..end).ok_or(Error::Corrupt("a slice"))?;
            if crc(whole) != 0 {
                return Err(Error::Corrupt("a slice's CRC does not match"));
            }
        }
        bounds.push((start, size));
        if size == 0 && start == end {
            return Err(Error::Corrupt("an empty slice"));
        }
        end = start;
    }
    bounds.reverse();
    Ok(bounds)
}

/// A slice's header (§4.6), with its own states.
fn slice_header(
    coder: &mut RangeDecoder<'_>,
    parameters: &Parameters,
    (width, height): (usize, usize),
) -> Result<SliceInfo, Error> {
    let mut state = [128u8; CONTEXT_SIZE];
    let tables = &parameters.transitions;
    let mut unsigned = |coder: &mut RangeDecoder<'_>| -> Result<usize, Error> {
        let value = coder.symbol(&mut state, false, tables)?;
        usize::try_from(value).map_err(|_| Error::Corrupt("a negative slice header field"))
    };
    let slice_x = unsigned(coder)?;
    let slice_y = unsigned(coder)?;
    let slice_width = unsigned(coder)?.saturating_add(1);
    let slice_height = unsigned(coder)?.saturating_add(1);
    let (h, v) = (parameters.h_slices as usize, parameters.v_slices as usize);
    if slice_x.saturating_add(slice_width) > h || slice_y.saturating_add(slice_height) > v {
        return Err(Error::Corrupt("a slice outside the slice raster"));
    }
    let mut sets = Vec::with_capacity(parameters.index_count());
    for _ in 0..parameters.index_count() {
        let set = unsigned(coder)?;
        if set >= parameters.sets.len() {
            return Err(Error::Corrupt("a quantization table set past the stream's"));
        }
        sets.push(set);
    }
    let picture_structure = u32::try_from(unsigned(coder)?).unwrap_or(0);
    let sar_num = u32::try_from(unsigned(coder)?).unwrap_or(0);
    let sar_den = u32::try_from(unsigned(coder)?).unwrap_or(0);
    let x = slice_x * width / h;
    let y = slice_y * height / v;
    Ok(SliceInfo {
        x,
        y,
        width: (slice_x + slice_width) * width / h - x,
        height: (slice_y + slice_height) * height / v - y,
        sets,
        picture_structure,
        sar: if sar_num == 0 || sar_den == 0 {
            (0, 0)
        } else {
            (sar_num, sar_den)
        },
    })
}

/// The frame's planes, empty: Y (or G, B, R through the transform), the chroma planes, the transparency plane.
fn new_planes(parameters: &Parameters, width: usize, height: usize) -> Result<Planes, Error> {
    if width == 0 || height == 0 || width.saturating_mul(height) > MOST_PIXELS {
        return Err(Error::Unsupported(format!("a {width} x {height} frame")));
    }
    let chroma = (
        width.div_ceil(1 << parameters.h_shift),
        height.div_ceil(1 << parameters.v_shift),
    );
    let mut sizes = vec![(width, height)];
    if parameters.chroma_planes {
        if parameters.colorspace == 1 {
            sizes.push((width, height));
            sizes.push((width, height));
        } else {
            sizes.push(chroma);
            sizes.push(chroma);
        }
    }
    if parameters.extra_plane {
        sizes.push((width, height));
    }
    Ok(Planes {
        data: sizes.iter().map(|&(w, h)| vec![0u16; w * h]).collect(),
        widths: sizes.iter().map(|&(w, _)| w).collect(),
        heights: sizes.iter().map(|&(_, h)| h).collect(),
    })
}

/// The context slot of plane `p` (§3.6): luma 0, chroma 1, transparency 2 (or 1 without chroma, after version 3).
fn slot_of(plane: usize, parameters: &Parameters) -> usize {
    match plane {
        0 => 0,
        1 | 2 if parameters.chroma_planes => 1,
        _ => usize::from(parameters.version <= 3 || parameters.chroma_planes) + 1,
    }
}

/// Decodes one slice's samples into `planes`.
fn decode_slice(
    mut symbols: Symbols<'_>,
    parameters: &Parameters,
    info: &SliceInfo,
    state: &mut SliceState,
    planes: &mut Planes,
) -> Result<(), Error> {
    // States made for the slice's table sets when it starts afresh (a key frame), or when they do not fit.
    let fits = state.slots.len() == info.sets.len()
        && state
            .slots
            .iter()
            .zip(&info.sets)
            .all(|(slot, &set)| slot.set == set);
    if !fits {
        state.slots = info
            .sets
            .iter()
            .map(|&set| {
                parameters
                    .sets
                    .get(set)
                    .map(|table| SlotState::new(set, table))
            })
            .collect::<Option<Vec<_>>>()
            .ok_or(Error::Corrupt("a quantization table set"))?;
    }
    let plane_count = planes.data.len();
    let rct = parameters.colorspace == 1;
    let bits = if rct {
        parameters.bits + 1
    } else {
        parameters.bits
    };
    let exception16 = !rct && parameters.bits == 16 && parameters.coder != 0;
    // Each plane's place and size within the slice.
    let geometry: Vec<(usize, usize, usize, usize)> = (0..plane_count)
        .map(|p| {
            let chroma = !rct && parameters.chroma_planes && (p == 1 || p == 2);
            if chroma {
                (
                    info.x >> parameters.h_shift,
                    info.y >> parameters.v_shift,
                    info.width.div_ceil(1 << parameters.h_shift),
                    info.height.div_ceil(1 << parameters.v_shift),
                )
            } else {
                (info.x, info.y, info.width, info.height)
            }
        })
        .collect();
    let mut rows: Vec<Rows> = geometry.iter().map(|&(_, _, w, _)| Rows::new(w)).collect();
    let mut run_index = vec![0usize; plane_count];
    let mut line = |p: usize,
                    y: usize,
                    symbols: &mut Symbols<'_>,
                    rows: &mut [Rows],
                    run_index: &mut [usize]| {
        let slot_index = slot_of(p, parameters);
        let slot = state
            .slots
            .get_mut(slot_index)
            .ok_or(Error::Corrupt("a context slot"))?;
        let set = parameters
            .sets
            .get(slot.set)
            .ok_or(Error::Corrupt("a quantization table set"))?;
        let plane_rows = rows.get_mut(p).ok_or(Error::Corrupt("a plane"))?;
        // The run index (§3.8.2.2.1) is each plane's, but one for all the planes of an RGB slice, whose lines take
        // turns (found against streams ffmpeg made: the RFC does not say).
        let run = run_index
            .get_mut(if rct { 0 } else { p })
            .ok_or(Error::Corrupt("a plane"))?;
        decode_line(
            symbols,
            slot,
            set,
            plane_rows,
            Line {
                y,
                bits,
                exception16,
            },
            run,
            &parameters.transitions,
        )
    };
    if rct {
        for y in 0..info.height {
            for p in 0..plane_count {
                line(p, y, &mut symbols, &mut rows, &mut run_index)?;
            }
            store_rct_row(&rows, parameters, info, y, planes);
        }
    } else {
        for p in 0..plane_count {
            let &(x0, y0, _, h) = geometry.get(p).ok_or(Error::Corrupt("a plane"))?;
            for y in 0..h {
                line(p, y, &mut symbols, &mut rows, &mut run_index)?;
                let current = rows
                    .get(p)
                    .map(|rows| rows.current.as_slice())
                    .unwrap_or_default();
                store_row(planes, p, (x0, y0 + y), current);
            }
        }
    }
    if let Symbols::Golomb(reader) = &symbols
        && reader.exhausted()
    {
        return Err(Error::Corrupt("a slice shorter than its samples"));
    }
    Ok(())
}

/// The two lines before the one being decoded, and that one, of a plane within a slice.
struct Rows {
    current: Vec<i32>,
    above: Vec<i32>,
    above2: Vec<i32>,
    /// How many lines have been decoded.
    done: usize,
}

impl Rows {
    fn new(width: usize) -> Self {
        Self {
            current: vec![0; width],
            above: vec![0; width],
            above2: vec![0; width],
            done: 0,
        }
    }
}

/// What a line's samples are.
#[derive(Clone, Copy)]
struct Line {
    y: usize,
    bits: u32,
    exception16: bool,
}

/// Decodes one line of a plane (§4.8) into `rows.current`, the line before it moving up.
fn decode_line(
    symbols: &mut Symbols<'_>,
    slot: &mut SlotState,
    set: &TableSet,
    rows: &mut Rows,
    line: Line,
    run_index: &mut usize,
    tables: &Transitions,
) -> Result<(), Error> {
    if line.y > 0 || rows.done > 0 {
        std::mem::swap(&mut rows.above2, &mut rows.above);
        std::mem::swap(&mut rows.above, &mut rows.current);
    }
    rows.done += 1;
    let y = line.y;
    let width = rows.current.len();
    let mask = if line.bits >= 32 {
        -1i32
    } else {
        (1i32 << line.bits) - 1
    };
    let (above, above2) = (&rows.above, &rows.above2);
    let at = |row: &[i32], x: usize| row.get(x).copied().unwrap_or(0);
    let mut run_mode = 0u8;
    let mut run_count: i64 = 0;
    for x in 0..width {
        // The neighbours with the slice's border (§3.1): rows above the slice are 0; the column left of it is the
        // first sample of the line above (0 for the slice's first line), the one before that 0; the column right of
        // it repeats the last sample.
        let t = if y >= 1 { at(above, x) } else { 0 };
        let tt = if y >= 2 { at(above2, x) } else { 0 };
        let left_of = |row_y: usize| -> i32 {
            // Column -1 of line `row_y`: the first sample of the line before it.
            if row_y == 0 {
                0
            } else if row_y == y {
                at(above, 0)
            } else if row_y + 1 == y {
                if y >= 2 { at(above2, 0) } else { 0 }
            } else {
                0
            }
        };
        let l = if x >= 1 {
            at(&rows.current, x - 1)
        } else {
            left_of(y)
        };
        let tl = if y == 0 {
            0
        } else if x >= 1 {
            at(above, x - 1)
        } else {
            left_of(y - 1)
        };
        let tr = if y == 0 {
            0
        } else if x + 1 < width {
            at(above, x + 1)
        } else {
            at(above, width.saturating_sub(1))
        };
        let big_l = if x >= 2 {
            at(&rows.current, x - 2)
        } else if x == 1 {
            left_of(y)
        } else {
            0
        };
        let q = &set.tables;
        let index = |value: i32| (value & 0xFF) as usize;
        let context = q[0][index(l - tl)]
            + q[1][index(tl - t)]
            + q[2][index(t - tr)]
            + q[3][index(big_l - l)]
            + q[4][index(tt - t)];
        let negative = context < 0;
        let context = context.unsigned_abs() as usize;
        if context >= set.contexts {
            return Err(Error::Corrupt("a context past the table set's"));
        }
        let mut diff = match symbols {
            Symbols::Range(coder) => {
                let states = slot
                    .range
                    .get_mut(context)
                    .ok_or(Error::Corrupt("a context"))?;
                coder.symbol(states, true, tables)?
            }
            Symbols::Golomb(reader) => {
                if context == 0 && run_mode == 0 {
                    run_mode = 1;
                }
                if run_mode != 0 {
                    let log2 = LOG2_RUN.get(*run_index).copied().unwrap_or(24);
                    if run_count == 0 && run_mode == 1 {
                        if reader.bits(1) == 1 {
                            run_count = 1i64 << log2;
                            if x as i64 + run_count <= width as i64 {
                                *run_index = (*run_index + 1).min(LOG2_RUN.len() - 1);
                            }
                        } else {
                            run_count = if log2 > 0 {
                                i64::from(reader.bits(log2))
                            } else {
                                0
                            };
                            *run_index = run_index.saturating_sub(1);
                            run_mode = 2;
                        }
                    }
                    run_count -= 1;
                    if run_count < 0 {
                        run_mode = 0;
                        run_count = 0;
                        let state = slot
                            .vlc
                            .get_mut(context)
                            .ok_or(Error::Corrupt("a context"))?;
                        let level = state.symbol(reader, line.bits);
                        if level >= 0 { level + 1 } else { level }
                    } else {
                        0
                    }
                } else {
                    let state = slot
                        .vlc
                        .get_mut(context)
                        .ok_or(Error::Corrupt("a context"))?;
                    state.symbol(reader, line.bits)
                }
            }
        };
        if negative {
            diff = -diff;
        }
        let predicted = if line.exception16 {
            let signed = |v: i32| if v >= 32768 { v - 65536 } else { v };
            median(signed(l), signed(t), signed(l) + signed(t) - signed(tl))
        } else {
            median(l, t, l + t - tl)
        };
        let sample = predicted.wrapping_add(diff) & mask;
        if let Some(slot) = rows.current.get_mut(x) {
            *slot = sample;
        }
    }
    Ok(())
}

fn median(a: i32, b: i32, c: i32) -> i32 {
    a.max(b).min(a.min(b).max(c))
}

/// Copies one decoded line of plane `p` to the frame at (`x`, `y`).
fn store_row(planes: &mut Planes, p: usize, (x, y): (usize, usize), row: &[i32]) {
    let (Some(width), Some(height)) = (
        planes.widths.get(p).copied(),
        planes.heights.get(p).copied(),
    ) else {
        return;
    };
    if y >= height {
        return;
    }
    if let Some(plane) = planes.data.get_mut(p) {
        for (offset, sample) in row.iter().enumerate() {
            if x + offset >= width {
                break;
            }
            if let Some(slot) = plane.get_mut(y * width + x + offset) {
                *slot = u16::try_from(*sample).unwrap_or(0);
            }
        }
    }
}

/// One line of RGB through the reversible transform (§3.7.2): the planes back to G, B, R (and transparency).
fn store_rct_row(
    rows: &[Rows],
    parameters: &Parameters,
    info: &SliceInfo,
    y: usize,
    planes: &mut Planes,
) {
    let (Some(luma), Some(cb), Some(cr)) = (rows.first(), rows.get(1), rows.get(2)) else {
        return;
    };
    let offset = 1i32 << parameters.bits;
    let swapped = (9..=15).contains(&parameters.bits) && !parameters.extra_plane;
    let width = planes.widths.first().copied().unwrap_or(0);
    let row = info.y + y;
    for x in 0..info.width {
        let yy = luma.current.get(x).copied().unwrap_or(0);
        let b_diff = cb.current.get(x).copied().unwrap_or(0) - offset;
        let r_diff = cr.current.get(x).copied().unwrap_or(0) - offset;
        // Planes 0, 1, 2 hold G, B, R (or, in the exception, B, G, R) once transformed back.
        let base = yy - ((b_diff + r_diff) >> 2);
        let (first, second, third) = (base, b_diff + base, r_diff + base);
        let (g, b, r) = if swapped {
            (second, first, third)
        } else {
            (first, second, third)
        };
        let at = row * width + info.x + x;
        for (p, value) in [(0usize, g), (1, b), (2, r)] {
            if let Some(slot) = planes.data.get_mut(p).and_then(|plane| plane.get_mut(at)) {
                *slot = u16::try_from(value.clamp(0, 65_535)).unwrap_or(0);
            }
        }
        if parameters.extra_plane
            && let (Some(alpha), Some(slot)) = (
                rows.get(3).and_then(|rows| rows.current.get(x)),
                planes.data.get_mut(3).and_then(|plane| plane.get_mut(at)),
            )
        {
            *slot = u16::try_from((*alpha).clamp(0, 65_535)).unwrap_or(0);
        }
    }
}

/// The decoded planes as a picture.
fn finish(
    planes: Planes,
    parameters: &Parameters,
    (width, height): (usize, usize),
    picture_structure: u32,
    sar: (u32, u32),
) -> Picture {
    let layout = if parameters.colorspace == 1 {
        Layout::Gbr
    } else if parameters.chroma_planes {
        Layout::Yuv {
            h_shift: parameters.h_shift,
            v_shift: parameters.v_shift,
        }
    } else {
        Layout::Gray
    };
    Picture {
        width: u32::try_from(width).unwrap_or(0),
        height: u32::try_from(height).unwrap_or(0),
        bits: parameters.bits,
        layout,
        alpha: parameters.extra_plane,
        planes: planes.data,
        picture_structure,
        sample_aspect: (sar.0 != 0).then_some(sar),
    }
}
