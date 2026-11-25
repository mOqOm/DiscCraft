//! A Blu-ray playlist's presentation graphics subtitles taken out as a `.sup` (M6.491), so that importing a Blu-ray
//! brings its subtitles in as subtitle tracks.
//!
//! A PG stream is a series of display sets, each its segments in their own PES packets: the presentation composition
//! (PCS) first, then window, palette and object definitions (WDS, PDS, ODS) and an END. The PCS carries the time the
//! set shows at (the others carry earlier times, when they are decoded), the composition state (an epoch start or an
//! acquisition point holds every definition the epoch's later sets use; a normal case only changes what is shown)
//! and the objects shown, none when the set clears the screen. So the stream is cut by display set, at its PCS time,
//! not by packet; and where a play item starts in the middle of an epoch, the definitions of the epoch so far are
//! carried to its start (as an epoch start showing what was shown there), and where one ends while a subtitle shows,
//! a set that clears it is added. Public sources: the PGS segment layout as the BD-ROM format documents describe it
//! and as DiscCraft's own PGS writer (`disccraft-pgs`) builds it; the `.sup` file is each segment behind `PG` and
//! its PTS and DTS (90 kHz).

use std::path::{Path, PathBuf};

use craft_media::ReadSeekSource;
use craft_media::mpegts::{File, StreamId};

use crate::TranscodeError;
use crate::embedded_subtitles::push_sup;

/// Segment types.
const PDS: u8 = 0x14;
const ODS: u8 = 0x15;
const PCS: u8 = 0x16;
const WDS: u8 = 0x17;
const END: u8 = 0x80;
/// Composition states that hold all of an epoch's definitions: an epoch start and an acquisition point.
const EPOCH_START: u8 = 0x80;
const REFRESH: u8 = 0xC0;
/// The bytes of a PCS body before its composition objects: width, height, frame rate, composition number, state,
/// palette update flag, palette id, object count.
const PCS_HEAD: usize = 11;
/// The most bytes of a whole extracted track.
const MAX_TRACK: usize = 512 << 20;
/// The most units read back from a play item's start for the definitions shown there.
const MAX_BACK: usize = 4_096;
/// The most units of one clip's stream read.
const MAX_UNITS: usize = 1_000_000;

/// One play item of a Blu-ray playlist, as its presentation graphics subtitles are taken out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BdSubtitlePart {
    /// The play item's clip file (`BDMV/STREAM/*.m2ts`).
    pub file: PathBuf,
    /// Where it plays in the clip, 90 kHz on the clip's time line: its IN and OUT times (twice the playlist's 45 kHz).
    pub from: i64,
    pub to: i64,
    /// Where its IN time falls on the timeline, 90 kHz from the timeline's start.
    pub at: i64,
}

/// One segment (its type, length and body) with the times of its PES packet.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Segment {
    pts: i64,
    dts: i64,
    bytes: Vec<u8>,
}

impl Segment {
    fn kind(&self) -> Option<u8> {
        self.bytes.first().copied()
    }

    fn body(&self) -> &[u8] {
        self.bytes.get(3..).unwrap_or_default()
    }
}

/// A display set: its segments, the PCS first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DisplaySet {
    segments: Vec<Segment>,
}

impl DisplaySet {
    fn pcs(&self) -> &[u8] {
        self.segments.first().map(Segment::body).unwrap_or_default()
    }

    /// When it shows: its PCS's time.
    fn time(&self) -> i64 {
        self.segments.first().map_or(0, |segment| segment.pts)
    }

    fn state(&self) -> u8 {
        self.pcs().get(7).copied().unwrap_or(0)
    }

    fn refreshes(&self) -> bool {
        self.state() & REFRESH != 0
    }

    /// It shows something (else it clears the screen).
    fn shows(&self) -> bool {
        self.pcs().get(10).is_some_and(|count| *count > 0)
    }
}

/// The segments of a unit's data (one segment a PES packet as a rule; more are taken as they come).
fn split(data: &[u8], (pts, dts): (i64, i64)) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut at = 0usize;
    while let (Some(&high), Some(&low)) = (data.get(at + 1), data.get(at + 2)) {
        let length = usize::from(u16::from_be_bytes([high, low]));
        let Some(bytes) = data.get(at..at + 3 + length) else {
            break;
        };
        segments.push(Segment {
            pts,
            dts,
            bytes: bytes.to_vec(),
        });
        at += 3 + length;
    }
    segments
}

/// Groups segments into display sets, each from a PCS; segments before the first PCS (of a set begun earlier) are
/// left out.
fn group(segments: impl IntoIterator<Item = Segment>, sets: &mut Vec<DisplaySet>) {
    for segment in segments {
        if segment.kind() == Some(PCS) {
            sets.push(DisplaySet {
                segments: vec![segment],
            });
        } else if let Some(set) = sets.last_mut() {
            set.segments.push(segment);
        }
    }
}

/// A PCS segment: `head`'s screen, frame rate and palette id, with composition number `number`, state `state`, no
/// palette update, and the composition objects `objects` (their bytes as a PCS lists them, behind the count).
fn pcs_segment(head: &[u8], number: u16, state: u8, (count, objects): (u8, &[u8])) -> Vec<u8> {
    let mut body = Vec::with_capacity(PCS_HEAD + objects.len());
    body.extend_from_slice(head.get(..5).unwrap_or(&[0x07, 0x80, 0x04, 0x38, 0x10]));
    body.extend_from_slice(&number.to_be_bytes());
    body.push(state);
    body.push(0x00);
    body.push(head.get(9).copied().unwrap_or(0));
    body.push(count);
    body.extend_from_slice(objects);
    let mut bytes = vec![PCS];
    bytes.extend_from_slice(&u16::try_from(body.len()).unwrap_or(0).to_be_bytes());
    bytes.extend_from_slice(&body);
    bytes
}

fn number_of(pcs: &[u8]) -> u16 {
    u16::from_be_bytes([
        pcs.get(5).copied().unwrap_or(0),
        pcs.get(6).copied().unwrap_or(0),
    ])
}

/// The display set that carries what `before` (the sets before a play item's start, from the epoch's last epoch start
/// or acquisition point on) left on the screen to the play item's start, at `time`: an epoch start with all their
/// definitions (later ones after earlier, as they replaced them) showing what their last set showed. `None` without
/// such a set to start from.
fn carried(before: &[DisplaySet], time: i64) -> Option<DisplaySet> {
    let start = before.iter().rposition(DisplaySet::refreshes)?;
    let sets = before.get(start..)?;
    let last = sets.last()?.pcs();
    let count = last.get(10).copied().unwrap_or(0);
    let pcs = pcs_segment(
        last,
        number_of(last),
        EPOCH_START,
        (count, last.get(PCS_HEAD..).unwrap_or_default()),
    );
    let mut segments = vec![Segment {
        pts: time,
        dts: time,
        bytes: pcs,
    }];
    // The epoch's windows (its first set's), then every palette and object as defined last.
    let windows = sets
        .iter()
        .flat_map(|set| &set.segments)
        .find(|segment| segment.kind() == Some(WDS));
    let definitions = sets
        .iter()
        .flat_map(|set| &set.segments)
        .filter(|segment| matches!(segment.kind(), Some(PDS | ODS)));
    for segment in windows.into_iter().chain(definitions) {
        segments.push(Segment {
            pts: time,
            dts: time,
            bytes: segment.bytes.clone(),
        });
    }
    segments.push(Segment {
        pts: time,
        dts: time,
        bytes: vec![END, 0, 0],
    });
    Some(DisplaySet { segments })
}

/// A display set that clears what `last` shows, at `time`, with the epoch's windows as `sets` define them last.
fn cleared(last: &DisplaySet, sets: &[DisplaySet], time: i64) -> DisplaySet {
    let pcs = last.pcs();
    let mut segments = vec![Segment {
        pts: time,
        dts: time,
        bytes: pcs_segment(pcs, number_of(pcs).wrapping_add(1), 0x00, (0, &[])),
    }];
    if let Some(windows) = sets
        .iter()
        .rev()
        .flat_map(|set| set.segments.iter().rev())
        .find(|segment| segment.kind() == Some(WDS))
    {
        segments.push(Segment {
            pts: time,
            dts: time,
            bytes: windows.bytes.clone(),
        });
    }
    segments.push(Segment {
        pts: time,
        dts: time,
        bytes: vec![END, 0, 0],
    });
    DisplaySet { segments }
}

/// What one play item adds: the display sets that show within `[from, to)` (`sets`, those before `from` from the
/// epoch's last refresh on, and those within), with what shows at `from` carried there when the first set within does
/// not start afresh at `from`, and what still shows at `to` cleared there. Times stay the clip's.
fn part_sets(sets: Vec<DisplaySet>, from: i64, to: i64) -> Vec<DisplaySet> {
    let (before, within): (Vec<DisplaySet>, Vec<DisplaySet>) =
        sets.into_iter().partition(|set| set.time() < from);
    let within: Vec<DisplaySet> = within.into_iter().filter(|set| set.time() < to).collect();
    let fresh_at_start = within
        .first()
        .is_some_and(|set| set.time() == from && set.refreshes());
    let first_needs_epoch = within.first().is_some_and(|set| !set.refreshes());
    let shown_before = before.last().is_some_and(DisplaySet::shows);
    let mut out = Vec::new();
    if !fresh_at_start
        && (shown_before || first_needs_epoch)
        && let Some(set) = carried(&before, from)
    {
        out.push(set);
    }
    out.extend(within);
    if let Some(last) = out.last().filter(|last| last.shows()) {
        let clear = cleared(last, &out, to);
        out.push(clear);
    }
    out
}

/// The units of stream `stream` of `file` that bear on `[from, to)`, as display sets: from the epoch's last refresh
/// before `from` (read back at most [`MAX_BACK`] units) to the last set that shows before `to`.
fn read_sets(
    file: &File,
    source: &ReadSeekSource<std::fs::File>,
    stream: usize,
    (from, to): (i64, i64),
    go_on: &dyn Fn() -> bool,
) -> Result<Option<Vec<DisplaySet>>, String> {
    let Some(units) = file.streams.get(stream).map(|stream| &stream.units) else {
        return Ok(Some(Vec::new()));
    };
    let count = units.len().min(MAX_UNITS);
    let mut last_time = 0i64;
    let unit = |index: usize, last_time: &mut i64| -> Result<Vec<Segment>, String> {
        let Some(found) = units.get(index) else {
            return Ok(Vec::new());
        };
        let pts = found.pts.unwrap_or(*last_time);
        *last_time = pts;
        let data = file
            .read_unit(source, stream, index)
            .map_err(|error| error.to_string())?;
        Ok(split(&data, (pts, found.dts.unwrap_or(pts))))
    };
    let first = units
        .iter()
        .take(count)
        .position(|found| found.pts.is_some_and(|pts| pts >= from))
        .unwrap_or(count);
    // Back from the play item's start to the epoch's last refresh.
    let mut back: Vec<Vec<Segment>> = Vec::new();
    let mut index = first;
    while index > 0 && back.len() < MAX_BACK {
        index -= 1;
        let segments = unit(index, &mut last_time)?;
        let refresh = segments.first().is_some_and(|segment| {
            segment.kind() == Some(PCS)
                && segment
                    .body()
                    .get(7)
                    .is_some_and(|state| state & REFRESH != 0)
        });
        back.push(segments);
        if refresh {
            break;
        }
    }
    let mut sets = Vec::new();
    group(back.into_iter().rev().flatten(), &mut sets);
    for index in first..count {
        if !go_on() {
            return Ok(None);
        }
        let segments = unit(index, &mut last_time)?;
        if segments
            .first()
            .is_some_and(|segment| segment.kind() == Some(PCS) && segment.pts >= to)
        {
            break;
        }
        group(segments, &mut sets);
    }
    Ok(Some(sets))
}

/// Takes the presentation graphics stream `pid` of a Blu-ray playlist's play items `parts` out into the `.sup` `out`:
/// each play item's display sets that show within its IN and OUT times (what shows at its start carried there, what
/// still shows at its end cleared), moved to where it falls on the timeline, one after the other. The units come from
/// the clip's transport stream index (made once and kept: the first time, the clip is read through for it). `go_on`
/// is asked before each clip and each unit; when it says no, nothing is written. Gives how many segments were
/// written.
///
/// # Errors
///
/// When a clip cannot be read, the stream holds nothing in the parts played, it is too large, or `go_on` stopped it.
pub fn extract_bd_subtitle(
    parts: &[BdSubtitlePart],
    pid: u16,
    out: &Path,
    go_on: &dyn Fn() -> bool,
) -> Result<usize, TranscodeError> {
    let fail = |message: String| TranscodeError::Decode(format!("{}: {message}", out.display()));
    let stopped = || fail("stopped".to_owned());
    let mut bytes = Vec::new();
    let mut count = 0usize;
    for part in parts {
        if !go_on() {
            return Err(stopped());
        }
        let (source, file) = crate::ts_index::open(&part.file)
            .ok_or_else(|| fail(format!("{} is not a transport stream", part.file.display())))?;
        // A clip without the stream (a play item that lists none) adds nothing.
        let Some(stream) = file
            .streams
            .iter()
            .position(|stream| matches!(stream.id, StreamId::Pid(found) if found == pid))
        else {
            continue;
        };
        let Some(sets) =
            read_sets(&file, &source, stream, (part.from, part.to), go_on).map_err(fail)?
        else {
            return Err(stopped());
        };
        let moved = |time: i64| {
            time.saturating_sub(part.from)
                .saturating_add(part.at)
                .max(0)
        };
        for set in part_sets(sets, part.from, part.to) {
            for segment in &set.segments {
                count += push_sup(
                    &mut bytes,
                    &segment.bytes,
                    (moved(segment.pts), moved(segment.dts)),
                );
            }
            if bytes.len() > MAX_TRACK {
                return Err(fail("the subtitle stream is too large".to_owned()));
            }
        }
    }
    if count == 0 {
        return Err(fail(format!("the stream {pid:#06x} holds no subtitles")));
    }
    std::fs::write(out, &bytes).map_err(|error| fail(error.to_string()))?;
    Ok(count)
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::{DisplaySet, END, ODS, PCS, PDS, Segment, WDS, part_sets};

    fn segment(time: i64, kind: u8, body: &[u8]) -> Segment {
        let mut bytes = vec![kind];
        bytes.extend_from_slice(&u16::try_from(body.len()).unwrap_or(0).to_be_bytes());
        bytes.extend_from_slice(body);
        Segment {
            pts: time,
            dts: time,
            bytes,
        }
    }

    /// A display set at `time` in `state`, showing object 1 when `shows`, defining object `object` when given.
    fn set(time: i64, number: u16, state: u8, shows: bool, object: Option<u8>) -> DisplaySet {
        let mut pcs = vec![0x07, 0x80, 0x04, 0x38, 0x10];
        pcs.extend_from_slice(&number.to_be_bytes());
        pcs.extend_from_slice(&[state, 0x00, 0x00, u8::from(shows)]);
        if shows {
            pcs.extend_from_slice(&[0x00, 0x01, 0x00, 0x00, 0x01, 0x00, 0x03, 0x00]);
        }
        let mut segments = vec![segment(time, PCS, &pcs)];
        if state != 0 || shows {
            segments.push(segment(time - 10, WDS, &[1, 0, 0, 0, 0, 0, 0x10, 0, 0x10]));
        }
        if let Some(object) = object {
            segments.push(segment(time - 20, PDS, &[0, object]));
            segments.push(segment(time - 20, ODS, &[0, 1, object]));
        }
        segments.push(segment(time - 20, END, &[]));
        DisplaySet { segments }
    }

    fn summary(sets: &[DisplaySet]) -> Vec<(i64, u8, bool, usize)> {
        sets.iter()
            .map(|set| (set.time(), set.state(), set.shows(), set.segments.len()))
            .collect()
    }

    /// A play item that holds whole subtitles keeps their sets as they are.
    #[test]
    fn whole_subtitles_stay_as_they_are() {
        let sets = vec![
            set(1_000, 1, 0x80, true, Some(1)),
            set(2_000, 2, 0x00, false, None),
            set(3_000, 3, 0x80, true, Some(2)),
            set(4_000, 4, 0x00, false, None),
        ];
        assert_eq!(part_sets(sets.clone(), 500, 5_000), sets);
        // Sets from the play item's end on are not its.
        assert_eq!(part_sets(sets.clone(), 500, 3_000), sets[..2].to_vec());
    }

    /// M6.491: a play item that starts while a subtitle shows gets it at its start, as an epoch start with its
    /// definitions; one that ends while a subtitle shows gets a set that clears it at its end.
    #[test]
    fn a_cut_subtitle_is_carried_and_cleared() {
        let sets = vec![
            set(1_000, 1, 0x80, true, Some(1)),
            set(2_000, 2, 0x00, false, None),
            set(3_000, 3, 0x80, true, Some(2)),
            set(4_000, 4, 0x00, false, None),
        ];
        let made = part_sets(sets.clone(), 1_500, 3_500);
        assert_eq!(
            summary(&made),
            [
                // Carried: PCS, WDS, PDS, ODS, END.
                (1_500, 0x80, true, 5),
                (2_000, 0x00, false, 2),
                (3_000, 0x80, true, 5),
                // Cleared: PCS (number 4), WDS, END.
                (3_500, 0x00, false, 3)
            ]
        );
        assert_eq!(made[0].segments[3].body(), [0, 1, 1]);
        assert_eq!(made[3].pcs()[5..7], [0, 4]);
        assert_eq!(made[3].segments[1].bytes[0], WDS);
        // Nothing shows at the start and the first set starts afresh: nothing is carried.
        assert_eq!(
            summary(&part_sets(sets.clone(), 2_500, 5_000)),
            [(3_000, 0x80, true, 5), (4_000, 0x00, false, 2)]
        );
    }

    /// A play item whose first set only changes what the epoch defined before its start gets those definitions
    /// carried, showing nothing until that set shows them.
    #[test]
    fn a_normal_case_set_gets_its_epoch() {
        let sets = vec![
            set(1_000, 1, 0x80, true, Some(1)),
            set(2_000, 2, 0x00, false, None),
            set(3_000, 3, 0x00, true, None),
            set(4_000, 4, 0x00, false, None),
        ];
        let made = part_sets(sets, 2_500, 5_000);
        assert_eq!(
            summary(&made),
            [
                (2_500, 0x80, false, 5),
                (3_000, 0x00, true, 3),
                (4_000, 0x00, false, 2)
            ]
        );
    }
}
