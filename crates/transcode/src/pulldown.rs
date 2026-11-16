//! The real display times of MPEG-2 video whose frames are not evenly spaced: soft pulldown (film coded at 23.976
//! progressive frames, each shown for two or three fields by `repeat_first_field`, as nearly every film or anime NTSC
//! DVD is) and streams that mix it with true 29.97 video.
//!
//! FilmCraft's MPEG source spaces the presented frames evenly at the coded rate (29.97), so 90 s of soft-pulldown
//! film reads as 71.5 s and frame `k` as shown at `k / 29.97`. [`Timeline`] holds each presented frame's real start
//! instead, from the program stream's presentation timestamps or, for an elementary stream (which has none), from the
//! pictures' `repeat_first_field` flags.

use std::io::Read;
use std::path::Path;

use filmcraft_time::{FrameRate, Tick};

use crate::TICKS_PER_SECOND;

/// Ticks per 90 kHz clock tick (MPEG presentation timestamps).
const TICKS_PER_90K: i64 = TICKS_PER_SECOND / 90_000;

/// When each presented frame starts (in display order, from 0) and how long the video lasts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Timeline {
    starts: Vec<Tick>,
    duration: Tick,
}

impl Timeline {
    /// The length of the video.
    pub(crate) fn duration(&self) -> Tick {
        self.duration
    }

    /// The presented frame showing at `time` (the first before 0, the last after the end).
    pub(crate) fn frame_at(&self, time: Tick) -> usize {
        self.starts
            .partition_point(|start| *start <= time)
            .saturating_sub(1)
    }

    /// The timeline from the presented frames' durations in fields; `None` when there are no frames.
    pub(crate) fn from_fields(fields: &[u32], rate: FrameRate) -> Option<Self> {
        let mut starts = Vec::with_capacity(fields.len());
        let mut total = 0i64;
        for &count in fields {
            starts.push(field_tick(rate, total));
            total = total.checked_add(i64::from(count))?;
        }
        (!starts.is_empty()).then(|| Self {
            starts,
            duration: field_tick(rate, total),
        })
    }

    /// The timeline from the presented frames' timestamps (90 kHz, display order); `None` when one is missing, they
    /// go backwards, or there are fewer than two. The last frame lasts as long as the one before it.
    pub(crate) fn from_pts(pts: &[Option<i64>]) -> Option<Self> {
        let pts: Vec<i64> = pts.iter().copied().collect::<Option<_>>()?;
        let (&first, &last) = (pts.first()?, pts.last()?);
        if pts.len() < 2 || pts.array_windows().any(|[earlier, later]| later <= earlier) {
            return None;
        }
        let before_last = *pts.get(pts.len() - 2)?;
        let to_tick = |value: i64| Tick((value - first).saturating_mul(TICKS_PER_90K));
        Some(Self {
            starts: pts.iter().map(|&value| to_tick(value)).collect(),
            duration: to_tick(last + (last - before_last)),
        })
    }

    /// Whether the frames are spaced unevenly enough at `rate` to matter: a frame starts more than half a frame from
    /// where an even spacing puts it, or the length differs by more than one frame. Evenly spaced video is left to
    /// FilmCraft as it is.
    pub(crate) fn is_uneven(&self, rate: FrameRate) -> bool {
        let frame = rate.tick_of(1).0.max(1);
        let off = |a: Tick, b: Tick| (a.0 - b.0).abs();
        let count = i64::try_from(self.starts.len()).unwrap_or(i64::MAX);
        off(self.duration, rate.tick_of(count)) > frame
            || self
                .starts
                .iter()
                .enumerate()
                .any(|(index, &start)| off(start, rate.tick_of(index as i64)) * 2 > frame)
    }
}

/// The start of field `field` at frame rate `rate` (two fields a frame).
fn field_tick(rate: FrameRate, field: i64) -> Tick {
    // tick_of is exact on frame boundaries; a field in between is half a frame on.
    let whole = rate.tick_of(field / 2);
    if field % 2 == 0 {
        whole
    } else {
        Tick(whole.0 + (rate.tick_of(field / 2 + 1).0 - whole.0) / 2)
    }
}

/// One picture of an elementary stream, in stream (decode) order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Picture {
    /// A GOP header came before it (since the previous picture).
    pub(crate) gop: bool,
    /// That GOP header's closed_gop flag.
    pub(crate) closed: bool,
    pub(crate) temporal_reference: u16,
    pub(crate) intra: bool,
    /// Fields it is shown for: 2, or 3 with repeat_first_field.
    pub(crate) fields: u32,
}

/// The presented frames' durations in fields, in display order, as FilmCraft presents an elementary stream: from
/// the first I picture, each picture keyed by the start of its GOP (its index there) plus its temporal_reference,
/// those keyed before the first I picture left out (leading pictures whose references are missing).
pub(crate) fn presented_fields(pictures: &[Picture]) -> Vec<u32> {
    let Some(first) = pictures.iter().position(|picture| picture.intra) else {
        return Vec::new();
    };
    let mut base = 0i64;
    let keyed: Vec<(i64, usize, u32)> = pictures
        .get(first..)
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(index, picture)| {
            let index = index as i64;
            if picture.gop || index == 0 {
                base = index;
            }
            (
                base + i64::from(picture.temporal_reference),
                index as usize,
                picture.fields,
            )
        })
        .collect();
    let Some(&(first_key, _, _)) = keyed.first() else {
        return Vec::new();
    };
    let mut shown: Vec<&(i64, usize, u32)> = keyed
        .iter()
        .filter(|(key, _, _)| *key >= first_key)
        .collect();
    shown.sort_by_key(|(key, index, _)| (*key, *index));
    shown.into_iter().map(|(_, _, fields)| *fields).collect()
}

/// The start of each GOP of an elementary stream: when its first picture is shown, from the stream's first shown
/// picture (every picture counted, as a DVD multiplexer times them), and whether it is closed. `rate` is the coded
/// frame rate.
pub(crate) fn gop_starts(pictures: &[Picture], rate: FrameRate) -> Vec<(Tick, bool)> {
    // Each picture's GOP (from 0) and display key: the decode index of its GOP's first picture plus its
    // temporal_reference.
    let mut closed = Vec::new();
    let mut base = 0i64;
    let mut keyed: Vec<(i64, usize, usize, u32)> = Vec::with_capacity(pictures.len());
    for (index, picture) in pictures.iter().enumerate() {
        if picture.gop || index == 0 {
            closed.push(picture.closed);
            base = index as i64;
        }
        let key = base + i64::from(picture.temporal_reference);
        keyed.push((key, index, closed.len().saturating_sub(1), picture.fields));
    }
    keyed.sort_by_key(|&(key, index, _, _)| (key, index));

    let mut starts: Vec<Option<Tick>> = vec![None; closed.len()];
    let mut fields = 0i64;
    for &(_, _, gop, count) in &keyed {
        let time = field_tick(rate, fields);
        if let Some(slot) = starts.get_mut(gop)
            && slot.is_none_or(|start| time < start)
        {
            *slot = Some(time);
        }
        fields += i64::from(count);
    }
    starts
        .into_iter()
        .zip(closed)
        .filter_map(|(start, closed)| start.map(|start| (start, closed)))
        .collect()
}

/// The frame rate of a sequence header's frame_rate_code.
pub(crate) fn frame_rate_of_code(code: u8) -> Option<FrameRate> {
    let (num, den) = match code {
        1 => (24000, 1001),
        2 => (24, 1),
        3 => (25, 1),
        4 => (30000, 1001),
        5 => (30, 1),
        6 => (50, 1),
        7 => (60000, 1001),
        8 => (60, 1),
        _ => return None,
    };
    Some(FrameRate { num, den })
}

/// The pictures of an elementary stream and its sequence header's frame_rate_code.
pub(crate) struct Scanned {
    pub(crate) pictures: Vec<Picture>,
    pub(crate) frame_rate_code: u8,
}

/// Reads the pictures of an MPEG-2 video elementary stream (it starts with a sequence header) without decoding it.
/// `None` when it is not one, codes field pictures, or is a progressive sequence (whose repeat flags repeat frames,
/// not fields): those are left to FilmCraft.
pub(crate) fn scan_elementary_stream(path: &Path) -> std::io::Result<Option<Vec<Picture>>> {
    Ok(scan(path)?.map(|scanned| scanned.pictures))
}

/// [`scan_elementary_stream`], with the frame rate code.
pub(crate) fn scan(path: &Path) -> std::io::Result<Option<Scanned>> {
    let mut file = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(path)?);
    let mut scanner = Scanner::default();
    let mut window: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut chunk)?;
        window.extend_from_slice(chunk.get(..read).unwrap_or_default());
        let done = read == 0;
        // A start code is read once its first bytes after it are here (or the file has ended).
        let keep = if done { 0 } else { HEADER_BYTES + 3 };
        let usable = window.len().saturating_sub(keep);
        let consumed = scanner.scan(&window, usable);
        if scanner.unsupported {
            return Ok(None);
        }
        window.drain(..consumed.min(window.len()));
        if done {
            break;
        }
    }
    Ok(scanner.started.then_some(Scanned {
        pictures: scanner.pictures,
        frame_rate_code: scanner.frame_rate_code,
    }))
}

/// The bytes after a start code the scanner reads.
const HEADER_BYTES: usize = 4;

#[derive(Default)]
struct Scanner {
    started: bool,
    /// The closed_gop flag of a GOP header not yet followed by its first picture.
    gop: Option<bool>,
    unsupported: bool,
    frame_rate_code: u8,
    pictures: Vec<Picture>,
}

impl Scanner {
    /// Reads the start codes that begin before `usable` in `data`; returns how many bytes are done with.
    fn scan(&mut self, data: &[u8], usable: usize) -> usize {
        let mut position = 0;
        while position + 3 < usable {
            if data.get(position..position + 3) != Some(&[0, 0, 1]) {
                position += 1;
                continue;
            }
            let code = data.get(position + 3).copied().unwrap_or(0);
            let payload = data.get(position + 4..).unwrap_or_default();
            self.start_code(code, payload);
            position += 4;
        }
        position
    }

    fn start_code(&mut self, code: u8, payload: &[u8]) {
        let byte = |index: usize| payload.get(index).copied().unwrap_or(0);
        match code {
            0xB3 => {
                self.started = true;
                self.frame_rate_code = byte(3) & 0x0F;
            }
            // GOP header: time code (25 bits), then closed_gop.
            0xB8 => self.gop = Some(byte(3) & 0x40 != 0),
            0x00 if self.started => {
                let temporal_reference = (u16::from(byte(0)) << 2) | u16::from(byte(1) >> 6);
                let coding_type = (byte(1) >> 3) & 7;
                let gop = self.gop.take();
                self.pictures.push(Picture {
                    gop: gop.is_some(),
                    closed: gop.unwrap_or(false),
                    temporal_reference,
                    intra: coding_type == 1,
                    fields: 2,
                });
            }
            0xB5 => match byte(0) >> 4 {
                // Sequence extension: progressive_sequence.
                1 if byte(1) & 0x08 != 0 => self.unsupported = true,
                // Picture coding extension: picture_structure, repeat_first_field.
                8 => {
                    if byte(2) & 0x03 != 3 {
                        self.unsupported = true;
                    } else if byte(3) & 0x02 != 0
                        && let Some(picture) = self.pictures.last_mut()
                    {
                        picture.fields = 3;
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Picture, Timeline, presented_fields};
    use filmcraft_time::{FrameRate, Tick};

    const NTSC: FrameRate = FrameRate {
        num: 30000,
        den: 1001,
    };

    fn picture(gop: bool, temporal_reference: u16, intra: bool, fields: u32) -> Picture {
        Picture {
            gop,
            closed: false,
            temporal_reference,
            intra,
            fields,
        }
    }

    #[test]
    fn soft_pulldown_lasts_five_fourths_longer() {
        // 2:3 pulldown: 24 film frames are 60 fields, one second at 29.97.
        let fields: Vec<u32> = (0..24)
            .map(|index| if index % 2 == 0 { 2 } else { 3 })
            .collect();
        let timeline = Timeline::from_fields(&fields, NTSC).unwrap();
        assert_eq!(timeline.duration(), NTSC.tick_of(30));
        assert!(timeline.is_uneven(NTSC));
        // Frame 4 starts after 2 + 3 + 2 + 3 = 10 fields, five frame periods.
        assert_eq!(timeline.frame_at(NTSC.tick_of(5)), 4);
        assert_eq!(timeline.frame_at(Tick(NTSC.tick_of(5).0 - 1)), 3);
        assert_eq!(timeline.frame_at(Tick(-1)), 0);
        assert_eq!(timeline.frame_at(NTSC.tick_of(1000)), 23);
    }

    #[test]
    fn evenly_spaced_frames_are_left_alone() {
        let timeline = Timeline::from_fields(&[2; 30], NTSC).unwrap();
        assert_eq!(timeline.duration(), NTSC.tick_of(30));
        assert!(!timeline.is_uneven(NTSC));
    }

    #[test]
    fn timestamps_give_the_timeline() {
        // 23.976 frames: 3753.75 ticks of 90 kHz apart, as 3754 / 3753.
        let pts: Vec<Option<i64>> = (0..48)
            .map(|index| Some(1000 + index * 90_090 / 24))
            .collect();
        let timeline = Timeline::from_pts(&pts).unwrap();
        assert!(timeline.is_uneven(NTSC));
        let seconds = timeline.duration().0 as f64 / crate::TICKS_PER_SECOND as f64;
        assert!((seconds - 2.002).abs() < 0.001, "{seconds}");
        assert_eq!(Timeline::from_pts(&[Some(0), None, Some(6000)]), None);
        assert_eq!(Timeline::from_pts(&[Some(0), Some(0)]), None);
    }

    #[test]
    fn gops_start_at_their_first_shown_picture() {
        // Closed GOP I P (2 + 3 fields), then an open GOP I B B whose leading B pictures are shown first, 2 fields
        // each.
        let mut first = picture(true, 0, true, 2);
        first.closed = true;
        let pictures = [
            first,
            picture(false, 1, false, 3),
            picture(true, 2, true, 2),
            picture(false, 0, false, 2),
            picture(false, 1, false, 2),
        ];
        let starts = super::gop_starts(&pictures, NTSC);
        // The open GOP starts with its first B picture, after 5 fields.
        assert_eq!(
            starts,
            vec![(Tick(0), true), (super::field_tick(NTSC, 5), false)]
        );
    }

    #[test]
    fn display_order_follows_temporal_reference_and_drops_leading_pictures() {
        // Open GOP from mid-stream: B B (leading, dropped) after the I, then P B B.
        let pictures = [
            picture(true, 2, true, 3),
            picture(false, 0, false, 2),
            picture(false, 1, false, 3),
            picture(false, 5, false, 2),
            picture(false, 3, false, 3),
            picture(false, 4, false, 2),
        ];
        // Display order: I(2) B(3) B(4) P(5).
        assert_eq!(presented_fields(&pictures), vec![3, 3, 2, 2]);
        assert!(presented_fields(&[picture(true, 0, false, 2)]).is_empty());
    }
}
