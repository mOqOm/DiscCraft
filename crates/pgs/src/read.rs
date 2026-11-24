//! Reading interactive graphics back: a graphics stream's segments, and the interactive composition of each display
//! set's ICS (the inverse of [`crate::ics`], following the same layout of the Blu-ray graphics stream). The Blu-ray
//! preview reads the menus' buttons, their neighbours and their HDMV commands from the built clips with it.
//!
//! Every button of every button overlap group is listed (a BOG's buttons are its states' alternatives; DiscCraft
//! writes one button per group). Effect sequences are read past; their windows and objects are not kept.

use crate::PgsError;
use crate::ig::{IgButton, IgObject, IgPage, InteractiveComposition};
use crate::segment::PaletteEntry;

/// Segment type of an interactive composition segment.
const ICS_TYPE: u8 = 0x18;
/// Segment type of an object definition segment.
const ODS_TYPE: u8 = 0x15;
/// Segment type of a palette definition segment.
const PDS_TYPE: u8 = 0x14;
/// The most objects read from one stream (a menu's buttons' states and their animations).
const MAX_OBJECTS: usize = 4096;
/// The most bytes of run-length data one object may hold.
const MAX_OBJECT_DATA: usize = 4 * 1024 * 1024;
/// The first fragment of a composition's data.
const FIRST_IN_SEQUENCE: u8 = 0x80;
/// The last fragment of a composition's data.
const LAST_IN_SEQUENCE: u8 = 0x40;
/// User interface model: pop-up.
const USER_INTERFACE_POPUP: u8 = 0x40;
/// Stream model: always multiplexed is 0 (the composition carries its time-outs).
const STREAM_MODEL_OOM: u8 = 0x80;
/// Button auto-action flag.
const AUTO_ACTION: u8 = 0x80;
/// Object cropped flag of a composition object.
const OBJECT_CROPPED: u8 = 0x80;
/// Bytes of one HDMV navigation command.
const COMMAND_LEN: usize = 12;

/// The segments of a graphics stream, in order: (segment type, body). Stops at a segment the data cuts off.
pub fn segments(data: &[u8]) -> Vec<(u8, &[u8])> {
    let mut out = Vec::new();
    let mut rest = data;
    while let Some((&kind, after)) = rest.split_first() {
        let Some((length, body)) = after.split_first_chunk::<2>() else {
            break;
        };
        let length = usize::from(u16::from_be_bytes(*length));
        let Some((segment, next)) = body.split_at_checked(length) else {
            break;
        };
        out.push((kind, segment));
        rest = next;
    }
    out
}

/// The graphics objects of a graphics stream (M6.430: a Blu-ray menu's buttons' pictures), each id's first,
/// decoded to palette indices. An object is an ODS with its id (2), version (1), sequence flags (1) and, in its first
/// fragment, the data's length (3), width (2) and height (2) before the run-length data; later fragments carry
/// more of the data after the first four bytes. An object whose data does not decode is left out.
#[must_use]
pub fn objects(stream: &[u8]) -> Vec<IgObject> {
    let mut out: Vec<IgObject> = Vec::new();
    // The object being joined: its id, size and data so far.
    let mut open: Option<(u16, u16, u16, Vec<u8>)> = None;
    for (kind, body) in segments(stream) {
        if kind != ODS_TYPE || out.len() >= MAX_OBJECTS {
            continue;
        }
        let Some((&[id_high, id_low, _version, flags], rest)) = body.split_first_chunk::<4>()
        else {
            continue;
        };
        let id = u16::from_be_bytes([id_high, id_low]);
        if flags & FIRST_IN_SEQUENCE != 0 {
            let Some((&[_, _, _, w_high, w_low, h_high, h_low], data)) =
                rest.split_first_chunk::<7>()
            else {
                open = None;
                continue;
            };
            open = Some((
                id,
                u16::from_be_bytes([w_high, w_low]),
                u16::from_be_bytes([h_high, h_low]),
                data.to_vec(),
            ));
        } else if let Some((open_id, _, _, data)) = open.as_mut()
            && *open_id == id
            && data.len().saturating_add(rest.len()) <= MAX_OBJECT_DATA
        {
            data.extend_from_slice(rest);
        } else {
            open = None;
            continue;
        }
        if flags & LAST_IN_SEQUENCE != 0
            && let Some((id, width, height, data)) = open.take()
            && !out.iter().any(|object| object.id == id)
            && let Ok(pixels) = crate::rle::rle_decode(width, height, &data)
        {
            out.push(IgObject {
                id,
                width,
                height,
                pixels,
            });
        }
    }
    out
}

/// The palettes of a graphics stream (M6.430), each id's first: a PDS is its id (1), version (1) and entries of five
/// bytes (entry id, Y, Cr, Cb, alpha).
#[must_use]
pub fn palettes(stream: &[u8]) -> Vec<(u8, Vec<PaletteEntry>)> {
    let mut out: Vec<(u8, Vec<PaletteEntry>)> = Vec::new();
    for (kind, body) in segments(stream) {
        let Some((&[id, _version], entries)) = body.split_first_chunk::<2>() else {
            continue;
        };
        if kind != PDS_TYPE || out.iter().any(|(known, _)| *known == id) {
            continue;
        }
        let entries = entries
            .as_chunks::<5>()
            .0
            .iter()
            .map(|&[id, y, cr, cb, alpha]| PaletteEntry {
                id,
                y,
                cr,
                cb,
                alpha,
            })
            .collect();
        out.push((id, entries));
    }
    out
}

/// The interactive compositions of a graphics stream (the segments joined, as [`segments`] reads them), one per
/// display set's ICS, in order.
///
/// # Errors
///
/// A composition whose fragments or data do not fit together or end early.
pub fn interactive_compositions(stream: &[u8]) -> Result<Vec<InteractiveComposition>, PgsError> {
    let mut out = Vec::new();
    let mut data: Option<(InteractiveComposition, Vec<u8>, usize)> = None;
    for (kind, body) in segments(stream) {
        if kind != ICS_TYPE {
            continue;
        }
        let mut reader = Reader::new(body);
        let width = reader.u16()?;
        let height = reader.u16()?;
        let frame_rate_id = reader.u8()? >> 4;
        let number = reader.u16()?;
        let _state = reader.u8()?;
        let sequence = reader.u8()?;
        if sequence & FIRST_IN_SEQUENCE != 0 {
            let length = reader.u24()?;
            let length = usize::try_from(length).map_err(|_| err("composition too long"))?;
            let header = InteractiveComposition {
                width,
                height,
                frame_rate_id,
                number,
                popup: false,
                pages: Vec::new(),
            };
            // The data is at most a 24-bit length; reserve what the segment shows, not what it claims.
            data = Some((header, Vec::with_capacity(body.len()), length));
        }
        let Some((_, collected, _)) = data.as_mut() else {
            return Err(err("composition fragment without its first part"));
        };
        collected.extend_from_slice(reader.rest());
        if sequence & LAST_IN_SEQUENCE != 0 {
            let Some((header, collected, length)) = data.take() else {
                continue;
            };
            if collected.len() != length {
                return Err(err("composition data length does not match"));
            }
            out.push(composition(header, &collected)?);
        }
    }
    Ok(out)
}

/// The composition data after the ICS descriptors.
fn composition(
    mut header: InteractiveComposition,
    data: &[u8],
) -> Result<InteractiveComposition, PgsError> {
    let mut reader = Reader::new(data);
    let model = reader.u8()?;
    header.popup = model & USER_INTERFACE_POPUP != 0;
    if model & STREAM_MODEL_OOM == 0 {
        // composition_time_out_pts and selection_time_out_pts.
        reader.skip(10)?;
    }
    // user_time_out_duration.
    reader.skip(3)?;
    let page_count = reader.u8()?;
    for _ in 0..page_count {
        header.pages.push(page(&mut reader)?);
    }
    Ok(header)
}

fn page(reader: &mut Reader<'_>) -> Result<IgPage, PgsError> {
    let id = reader.u8()?;
    // page_version_number and the UO mask table.
    reader.skip(1 + 8)?;
    effect_sequence(reader)?;
    effect_sequence(reader)?;
    // animation_frame_rate_code.
    reader.skip(1)?;
    let default_selected = reader.u16()?;
    // default_activated_button_id_ref.
    reader.skip(2)?;
    let palette_id = reader.u8()?;
    let groups = reader.u8()?;
    let mut buttons = Vec::new();
    for _ in 0..groups {
        // default_valid_button_id_ref.
        reader.skip(2)?;
        let count = reader.u8()?;
        for _ in 0..count {
            buttons.push(button(reader)?);
        }
    }
    Ok(IgPage {
        id,
        default_selected,
        palette_id,
        buttons,
    })
}

/// An in or out effect sequence: its windows and effects, read past.
fn effect_sequence(reader: &mut Reader<'_>) -> Result<(), PgsError> {
    let windows = reader.u8()?;
    // window_id, x, y, width, height.
    reader.skip(usize::from(windows) * 9)?;
    let effects = reader.u8()?;
    for _ in 0..effects {
        // effect_duration and palette_id_ref.
        reader.skip(3 + 1)?;
        let objects = reader.u8()?;
        for _ in 0..objects {
            // object_id_ref and window_id_ref.
            reader.skip(2 + 1)?;
            let flags = reader.u8()?;
            // x, y, and the cropping rectangle when cropped.
            reader.skip(4)?;
            if flags & OBJECT_CROPPED != 0 {
                reader.skip(8)?;
            }
        }
    }
    Ok(())
}

fn button(reader: &mut Reader<'_>) -> Result<IgButton, PgsError> {
    let id = reader.u16()?;
    let numeric_value = reader.u16()?;
    let auto_action = reader.u8()? & AUTO_ACTION != 0;
    let x = reader.u16()?;
    let y = reader.u16()?;
    let upper = reader.u16()?;
    let lower = reader.u16()?;
    let left = reader.u16()?;
    let right = reader.u16()?;
    let normal_object = reader.u16()?;
    // normal end object, repeat / complete flags.
    reader.skip(2 + 1)?;
    // selected state sound.
    reader.skip(1)?;
    let selected_object = reader.u16()?;
    reader.skip(2 + 1)?;
    // activated state sound.
    reader.skip(1)?;
    let activated_object = reader.u16()?;
    reader.skip(2)?;
    let count = reader.u16()?;
    let mut commands = Vec::with_capacity(usize::from(count).min(reader.remaining() / COMMAND_LEN));
    for _ in 0..count {
        commands.push(reader.array::<COMMAND_LEN>()?);
    }
    Ok(IgButton {
        id,
        numeric_value,
        auto_action,
        x,
        y,
        upper,
        lower,
        left,
        right,
        normal_object,
        selected_object,
        activated_object,
        commands,
    })
}

/// A bounds-checked big-endian reader.
struct Reader<'a> {
    data: &'a [u8],
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    fn remaining(&self) -> usize {
        self.data.len()
    }

    fn rest(&self) -> &'a [u8] {
        self.data
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], PgsError> {
        let (head, rest) = self
            .data
            .split_first_chunk::<N>()
            .ok_or_else(|| err("graphics data ends early"))?;
        self.data = rest;
        Ok(*head)
    }

    fn u8(&mut self) -> Result<u8, PgsError> {
        Ok(u8::from_be_bytes(self.array::<1>()?))
    }

    fn u16(&mut self) -> Result<u16, PgsError> {
        Ok(u16::from_be_bytes(self.array::<2>()?))
    }

    fn u24(&mut self) -> Result<u32, PgsError> {
        let [a, b, c] = self.array::<3>()?;
        Ok(u32::from_be_bytes([0, a, b, c]))
    }

    fn skip(&mut self, count: usize) -> Result<(), PgsError> {
        let (_, rest) = self
            .data
            .split_at_checked(count)
            .ok_or_else(|| err("graphics data ends early"))?;
        self.data = rest;
        Ok(())
    }
}

fn err(message: &str) -> PgsError {
    PgsError(message.to_string())
}

#[cfg(test)]
mod tests {
    /// M6.430: objects (one small, one split over several segments) and palettes read back as written.
    #[test]
    fn objects_and_palettes_read_back() {
        use crate::rle::rle_encode;
        use crate::segment::{PaletteEntry, ods, pds};
        let small: Vec<u8> = (0..40 * 12).map(|n| (n % 3) as u8).collect();
        // Noisy enough not to shrink below one segment's size.
        let large: Vec<u8> = (0..600 * 200)
            .map(|n: u32| ((n * 7919) % 251) as u8)
            .collect();
        let mut stream = Vec::new();
        for (id, (width, height, pixels)) in [(40u16, 12u16, &small), (600, 200, &large)]
            .into_iter()
            .enumerate()
        {
            let rle = rle_encode(width, height, pixels).unwrap();
            for segment in ods(id as u16, 0, width, height, &rle).unwrap() {
                stream.extend(segment);
            }
        }
        let entries = [PaletteEntry {
            id: 1,
            y: 235,
            cr: 128,
            cb: 128,
            alpha: 255,
        }];
        stream.extend(pds(3, 0, &entries).unwrap());
        // The same id again later is not read twice.
        for segment in ods(0, 1, 40, 12, &rle_encode(40, 12, &small).unwrap()).unwrap() {
            stream.extend(segment);
        }
        let objects = super::objects(&stream);
        assert_eq!(objects.len(), 2);
        assert_eq!(
            (objects[0].id, objects[0].width, objects[0].height),
            (0, 40, 12)
        );
        assert_eq!(objects[0].pixels, small);
        assert_eq!(objects[1].pixels, large);
        assert_eq!(super::palettes(&stream), [(3, entries.to_vec())]);
        // Cut short anywhere: no panic.
        for cut in (0..stream.len()).step_by(97) {
            let _ = super::objects(&stream[..cut]);
            let _ = super::palettes(&stream[..cut]);
        }
    }

    use super::{interactive_compositions, segments};
    use crate::ig::{IgButton, IgPage, InteractiveComposition, ics};

    fn button(id: u16, commands: usize) -> IgButton {
        IgButton {
            id,
            numeric_value: id + 1,
            auto_action: id == 1,
            x: 100 + id,
            y: 200,
            upper: id,
            lower: if id == 0 { 1 } else { 0 },
            left: id,
            right: id,
            normal_object: 2 * id,
            selected_object: 2 * id + 1,
            activated_object: 2 * id + 1,
            commands: (0..commands)
                .map(|n| {
                    let mut command = [0_u8; 12];
                    command[11] = u8::try_from(n).unwrap_or(0);
                    command[0] = 0x50;
                    command
                })
                .collect(),
        }
    }

    /// What the writer encodes reads back the same, a pop-up and a big composition split over fragments included.
    #[test]
    fn compositions_read_back_as_written() -> Result<(), Box<dyn std::error::Error>> {
        let small = InteractiveComposition {
            width: 1920,
            height: 1080,
            frame_rate_id: 6,
            number: 3,
            popup: true,
            pages: vec![IgPage {
                id: 0,
                default_selected: 1,
                palette_id: 0,
                buttons: vec![button(0, 2), button(1, 1)],
            }],
        };
        // Enough buttons and commands to need more than one ICS fragment.
        let big = InteractiveComposition {
            width: 720,
            height: 480,
            frame_rate_id: 4,
            number: 4,
            popup: false,
            pages: vec![IgPage {
                id: 0,
                default_selected: 0,
                palette_id: 1,
                buttons: (0..2)
                    .map(|id| IgButton {
                        lower: 1 - id,
                        ..button(id, 3_000)
                    })
                    .collect(),
            }],
        };
        let mut stream = Vec::new();
        for composition in [&small, &big] {
            let fragments = ics(composition)?;
            if composition.number == 4 {
                assert!(fragments.len() > 1);
            }
            for fragment in fragments {
                stream.extend(fragment);
            }
            // A palette-like segment in between is passed over.
            stream.extend([0x14, 0, 2, 9, 9]);
        }
        assert_eq!(
            segments(&stream).len(),
            ics(&small)?.len() + ics(&big)?.len() + 2
        );
        let read = interactive_compositions(&stream)?;
        assert_eq!(read, vec![small, big]);
        Ok(())
    }

    #[test]
    fn broken_streams_do_not_panic() {
        assert!(segments(&[0x18, 0, 9, 1]).is_empty());
        assert!(interactive_compositions(&[0x18, 0, 2, 1, 2]).is_err());
        // A continuation fragment without its first part.
        let mut body = vec![0_u8; 8];
        body.push(0x40);
        let mut stream = vec![0x18, 0, 9];
        stream.extend(body);
        assert!(interactive_compositions(&stream).is_err());
    }
}
