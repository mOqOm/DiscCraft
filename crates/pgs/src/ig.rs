//! Interactive graphics composition segment (ICS) encoder.

use std::cmp::min;

use crate::rle_encode;
use crate::segment::{end, ods, pds};
use crate::{PaletteEntry, PgsError, TimedSegment};

const ICS_TYPE: u8 = 0x18;
const MAX_SEGMENT_BODY: usize = 65535;
const MAX_U24: u32 = 0xFF_FFFF;

const VIDEO_DESCRIPTOR_LEN: usize = 5;
const COMPOSITION_DESCRIPTOR_LEN: usize = 3;
const SEQUENCE_DESCRIPTOR_LEN: usize = 1;
const DATA_LENGTH_LEN: usize = 3;
const FIRST_HEADER_LEN: usize =
    VIDEO_DESCRIPTOR_LEN + COMPOSITION_DESCRIPTOR_LEN + SEQUENCE_DESCRIPTOR_LEN + DATA_LENGTH_LEN;
const CONTINUATION_HEADER_LEN: usize =
    VIDEO_DESCRIPTOR_LEN + COMPOSITION_DESCRIPTOR_LEN + SEQUENCE_DESCRIPTOR_LEN;

const COMPOSITION_STATE: u8 = 0x80;
const FIRST_IN_SEQUENCE: u8 = 0x80;
const LAST_IN_SEQUENCE: u8 = 0x40;
const USER_INTERFACE_POPUP: u8 = 0x40;
const AUTO_ACTION: u8 = 0x80;
const BUTTON_OBJECT_COMPLETE: u8 = 0x40;
const NONE_SOUND: u8 = 0xFF;
const NONE_BUTTON: u16 = 0xFFFF;

/// One interactive graphics button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgButton {
    /// Button id.
    pub id: u16,
    /// Numeric select value, or `0xFFFF` for none.
    pub numeric_value: u16,
    /// Whether the button has auto-action.
    pub auto_action: bool,
    /// X coordinate.
    pub x: u16,
    /// Y coordinate.
    pub y: u16,
    /// Upper neighbour button id.
    pub upper: u16,
    /// Lower neighbour button id.
    pub lower: u16,
    /// Left neighbour button id.
    pub left: u16,
    /// Right neighbour button id.
    pub right: u16,
    /// Normal object id, used as both start and end. `0xFFFF` means none.
    pub normal_object: u16,
    /// Selected object id, used as both start and end. `0xFFFF` means none.
    pub selected_object: u16,
    /// Activated object id, used as both start and end. `0xFFFF` means none.
    pub activated_object: u16,
    /// HDMV navigation commands, each 12 bytes.
    pub commands: Vec<[u8; 12]>,
}

/// One interactive graphics page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgPage {
    /// Page id.
    pub id: u8,
    /// Default selected button id.
    pub default_selected: u16,
    /// Palette id reference.
    pub palette_id: u8,
    /// Buttons. Each button is written as its own button overlap group.
    pub buttons: Vec<IgButton>,
}

/// An interactive graphics composition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractiveComposition {
    /// Presentation width.
    pub width: u16,
    /// Presentation height.
    pub height: u16,
    /// Frame rate code, for example 3 for 25 fps or 4 for 29.97 fps.
    pub frame_rate_id: u8,
    /// Composition number.
    pub number: u16,
    /// Whether the user interface model is pop-up.
    pub popup: bool,
    /// Pages.
    pub pages: Vec<IgPage>,
}

/// One graphics object for an interactive menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgObject {
    /// Object id.
    pub id: u16,
    /// Object width.
    pub width: u16,
    /// Object height.
    pub height: u16,
    /// Palette indices, row-major.
    pub pixels: Vec<u8>,
}

fn err(message: &str) -> PgsError {
    PgsError(message.to_string())
}

fn build_segment(segment_type: u8, body: Vec<u8>) -> Result<Vec<u8>, PgsError> {
    let length = u16::try_from(body.len()).map_err(|_| err("segment body too large"))?;
    let capacity = 3usize
        .checked_add(usize::from(length))
        .ok_or_else(|| err("segment too large"))?;

    let mut out = Vec::with_capacity(capacity);
    out.push(segment_type);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

fn push_u24(out: &mut Vec<u8>, value: u32) {
    let bytes = value.to_be_bytes();
    out.extend(bytes.iter().skip(1).copied());
}

fn frame_rate_byte(frame_rate_id: u8) -> u8 {
    let code = frame_rate_id & 0x0F;
    code << 4
}

fn model_byte(popup: bool) -> u8 {
    if popup { USER_INTERFACE_POPUP } else { 0 }
}

fn sequence_flags(first: bool, last: bool) -> u8 {
    let mut flags = 0;
    if first {
        flags |= FIRST_IN_SEQUENCE;
    }
    if last {
        flags |= LAST_IN_SEQUENCE;
    }
    flags
}

fn push_descriptors(out: &mut Vec<u8>, c: &InteractiveComposition, sequence: u8) {
    out.extend_from_slice(&c.width.to_be_bytes());
    out.extend_from_slice(&c.height.to_be_bytes());
    out.push(frame_rate_byte(c.frame_rate_id));
    out.extend_from_slice(&c.number.to_be_bytes());
    out.push(COMPOSITION_STATE);
    out.push(sequence);
}

fn push_button(out: &mut Vec<u8>, button: &IgButton) -> Result<(), PgsError> {
    out.extend_from_slice(&button.id.to_be_bytes());
    out.extend_from_slice(&button.numeric_value.to_be_bytes());
    out.push(if button.auto_action { AUTO_ACTION } else { 0 });
    out.extend_from_slice(&button.x.to_be_bytes());
    out.extend_from_slice(&button.y.to_be_bytes());
    out.extend_from_slice(&button.upper.to_be_bytes());
    out.extend_from_slice(&button.lower.to_be_bytes());
    out.extend_from_slice(&button.left.to_be_bytes());
    out.extend_from_slice(&button.right.to_be_bytes());

    out.extend_from_slice(&button.normal_object.to_be_bytes());
    out.extend_from_slice(&button.normal_object.to_be_bytes());
    out.push(BUTTON_OBJECT_COMPLETE);

    out.push(NONE_SOUND);
    out.extend_from_slice(&button.selected_object.to_be_bytes());
    out.extend_from_slice(&button.selected_object.to_be_bytes());
    out.push(BUTTON_OBJECT_COMPLETE);

    out.push(NONE_SOUND);
    out.extend_from_slice(&button.activated_object.to_be_bytes());
    out.extend_from_slice(&button.activated_object.to_be_bytes());

    let command_count =
        u16::try_from(button.commands.len()).map_err(|_| err("too many commands"))?;
    out.extend_from_slice(&command_count.to_be_bytes());
    for command in &button.commands {
        out.extend_from_slice(command);
    }

    Ok(())
}

fn push_bog(out: &mut Vec<u8>, button: &IgButton) -> Result<(), PgsError> {
    out.extend_from_slice(&button.id.to_be_bytes());
    out.push(1);
    push_button(out, button)
}

fn push_page(out: &mut Vec<u8>, page: &IgPage) -> Result<(), PgsError> {
    out.push(page.id);
    out.push(0);
    out.extend_from_slice(&[0u8; 8]);
    out.extend_from_slice(&[0u8, 0u8]);
    out.extend_from_slice(&[0u8, 0u8]);
    out.push(0);
    out.extend_from_slice(&page.default_selected.to_be_bytes());
    out.extend_from_slice(&NONE_BUTTON.to_be_bytes());
    out.push(page.palette_id);

    let bog_count = u8::try_from(page.buttons.len()).map_err(|_| err("too many buttons"))?;
    out.push(bog_count);

    for button in &page.buttons {
        push_bog(out, button)?;
    }

    Ok(())
}

fn composition_data(c: &InteractiveComposition) -> Result<Vec<u8>, PgsError> {
    let mut out = Vec::new();

    out.push(model_byte(c.popup));
    out.extend_from_slice(&[0u8; 5]);
    out.extend_from_slice(&[0u8; 5]);
    push_u24(&mut out, 0);

    let page_count = u8::try_from(c.pages.len()).map_err(|_| err("too many pages"))?;
    out.push(page_count);

    for page in &c.pages {
        push_page(&mut out, page)?;
    }

    Ok(out)
}

fn validate_unique_button_ids(buttons: &[IgButton]) -> Result<(), PgsError> {
    for (index, button) in buttons.iter().enumerate() {
        let next = index
            .checked_add(1)
            .ok_or_else(|| err("button index overflow"))?;
        for other in buttons.iter().skip(next) {
            if button.id == other.id {
                return Err(err("duplicate button id"));
            }
        }
    }

    Ok(())
}

fn validate_neighbour_ids(buttons: &[IgButton]) -> Result<(), PgsError> {
    for button in buttons {
        let neighbours = [button.upper, button.lower, button.left, button.right];
        for neighbour in neighbours {
            if !buttons.iter().any(|candidate| candidate.id == neighbour) {
                return Err(err("neighbour button id does not exist"));
            }
        }
    }

    Ok(())
}

fn validate_command_counts(buttons: &[IgButton]) -> Result<(), PgsError> {
    for button in buttons {
        let _ = u16::try_from(button.commands.len()).map_err(|_| err("too many commands"))?;
    }

    Ok(())
}

fn validate_page(page: &IgPage) -> Result<(), PgsError> {
    let _ = u8::try_from(page.buttons.len()).map_err(|_| err("too many buttons"))?;
    validate_unique_button_ids(&page.buttons)?;
    validate_neighbour_ids(&page.buttons)?;
    validate_command_counts(&page.buttons)?;
    Ok(())
}

fn validate_composition(c: &InteractiveComposition) -> Result<(), PgsError> {
    let _ = u8::try_from(c.pages.len()).map_err(|_| err("too many pages"))?;
    for page in &c.pages {
        validate_page(page)?;
    }
    Ok(())
}

fn fragment(data: &[u8], c: &InteractiveComposition) -> Result<Vec<Vec<u8>>, PgsError> {
    let data_len = u32::try_from(data.len()).map_err(|_| err("composition data too large"))?;
    if data_len > MAX_U24 {
        return Err(err("composition data length exceeds 24 bits"));
    }

    let max_first_data = MAX_SEGMENT_BODY
        .checked_sub(FIRST_HEADER_LEN)
        .ok_or_else(|| err("invalid ICS first header"))?;
    let max_continuation_data = MAX_SEGMENT_BODY
        .checked_sub(CONTINUATION_HEADER_LEN)
        .ok_or_else(|| err("invalid ICS continuation header"))?;

    let mut segments = Vec::new();
    let mut offset = 0usize;
    let mut first = true;

    loop {
        let remaining = data.len().saturating_sub(offset);
        let header_len = if first {
            FIRST_HEADER_LEN
        } else {
            CONTINUATION_HEADER_LEN
        };
        let max_data = if first {
            max_first_data
        } else {
            max_continuation_data
        };
        let chunk_len = min(remaining, max_data);

        if remaining > 0 && chunk_len == 0 {
            return Err(err("cannot fragment composition"));
        }

        let next_offset = offset
            .checked_add(chunk_len)
            .ok_or_else(|| err("composition offset overflow"))?;
        let is_last = next_offset >= data.len();
        let sequence = sequence_flags(first, is_last);

        let capacity = header_len
            .checked_add(chunk_len)
            .ok_or_else(|| err("ICS fragment too large"))?;
        let mut body = Vec::with_capacity(capacity);
        push_descriptors(&mut body, c, sequence);
        if first {
            push_u24(&mut body, data_len);
        }
        body.extend(data.iter().skip(offset).take(chunk_len).copied());

        segments.push(build_segment(ICS_TYPE, body)?);

        offset = next_offset;
        first = false;
        if is_last {
            break;
        }
    }

    Ok(segments)
}

/// Encode an interactive composition segment (ICS) as one or more segments.
pub fn ics(c: &InteractiveComposition) -> Result<Vec<Vec<u8>>, PgsError> {
    validate_composition(c)?;
    let data = composition_data(c)?;
    fragment(&data, c)
}

fn validate_object_ids(objects: &[IgObject]) -> Result<(), PgsError> {
    for (index, object) in objects.iter().enumerate() {
        let next = index
            .checked_add(1)
            .ok_or_else(|| err("object index overflow"))?;
        for other in objects.iter().skip(next) {
            if object.id == other.id {
                return Err(err("duplicate object id"));
            }
        }
    }

    Ok(())
}

/// Encode a complete IG display set at `pts`.
///
/// The order is ICS fragments, PDS, every ODS, then END. ICS and END use `dts = pts`.
/// PDS and ODS keep the display-set `pts` but use `dts = pts - 1`, saturating at zero,
/// so they are decoded before the composition is presented.
pub fn display_set(
    c: &InteractiveComposition,
    palette: &[PaletteEntry],
    objects: &[IgObject],
    pts: u64,
) -> Result<Vec<TimedSegment>, PgsError> {
    validate_object_ids(objects)?;

    let mut output = Vec::new();
    let pre_dts = pts.saturating_sub(1);

    for data in ics(c)? {
        output.push(TimedSegment {
            pts,
            dts: pts,
            data,
        });
    }

    let pds_segment = pds(0, 0, palette)?;
    output.push(TimedSegment {
        pts,
        dts: pre_dts,
        data: pds_segment,
    });

    for object in objects {
        let rle = rle_encode(object.width, object.height, &object.pixels)?;
        for data in ods(object.id, 0, object.width, object.height, &rle)? {
            output.push(TimedSegment {
                pts,
                dts: pre_dts,
                data,
            });
        }
    }

    output.push(TimedSegment {
        pts,
        dts: pts,
        data: end(),
    });

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{IgButton, IgObject, IgPage, InteractiveComposition, display_set, ics};
    use crate::PaletteEntry;

    fn make_button(id: u16) -> IgButton {
        IgButton {
            id,
            numeric_value: 0xFFFF,
            auto_action: false,
            x: 0,
            y: 0,
            upper: id,
            lower: id,
            left: id,
            right: id,
            normal_object: 0xFFFF,
            selected_object: 0xFFFF,
            activated_object: 0xFFFF,
            commands: Vec::new(),
        }
    }

    fn make_command() -> [u8; 12] {
        [
            0x21, 0x81, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
        ]
    }

    fn make_composition() -> InteractiveComposition {
        let mut button = make_button(0);
        button.commands.push(make_command());

        InteractiveComposition {
            width: 1920,
            height: 1080,
            frame_rate_id: 3,
            number: 0,
            popup: false,
            pages: vec![IgPage {
                id: 0,
                default_selected: 0,
                palette_id: 0,
                buttons: vec![button],
            }],
        }
    }

    fn make_object(id: u16) -> IgObject {
        IgObject {
            id,
            width: 2,
            height: 2,
            pixels: vec![0, 1, 2, 3],
        }
    }

    fn palette() -> Vec<PaletteEntry> {
        vec![PaletteEntry {
            id: 0,
            y: 16,
            cr: 128,
            cb: 128,
            alpha: 255,
        }]
    }

    #[test]
    fn minimal_ics_exact_bytes() {
        let segments = ics(&make_composition()).unwrap_or_default();

        // Built field by field from the ICS layout (see the module docs).
        let mut body = vec![0x07, 0x80, 0x04, 0x38, 0x30]; // video descriptor: 1920 x 1080, rate id 3
        body.extend([0x00, 0x00, 0x80]); // composition number 0, epoch start
        body.push(0xC0); // first and last in sequence
        body.extend([0x00, 0x00, 0x56]); // data length 86
        body.push(0x00); // stream model 0 (multiplexed), always-on
        body.extend([0x00; 10]); // composition and selection time-outs
        body.extend([0x00; 3]); // user time-out
        body.push(0x01); // one page
        body.extend([0x00, 0x00]); // page id, version
        body.extend([0x00; 8]); // UO mask
        body.extend([0x00, 0x00, 0x00, 0x00]); // empty in / out effects
        body.push(0x00); // animation frame rate code
        body.extend([0x00, 0x00, 0xFF, 0xFF]); // default selected 0, default activated none
        body.extend([0x00, 0x01]); // palette 0, one BOG
        body.extend([0x00, 0x00, 0x01]); // BOG: default valid button 0, one button
        body.extend([0x00, 0x00, 0xFF, 0xFF, 0x00]); // button 0, no numeric value, no auto action
        body.extend([0x00, 0x00, 0x00, 0x00]); // x, y
        body.extend([0x00; 8]); // neighbours (itself)
        body.extend([0xFF, 0xFF, 0xFF, 0xFF, 0x40]); // normal: no objects, complete flag
        body.extend([0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x40]); // selected: no sound, no objects, complete flag
        body.extend([0xFF, 0xFF, 0xFF, 0xFF, 0xFF]); // activated: no sound, no objects
        body.extend([0x00, 0x01]); // one command
        body.extend(make_command());
        assert_eq!(body.len(), 98);
        let mut expected = vec![0x18, 0x00, 0x62];
        expected.extend(body);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments.first().cloned(), Some(expected));
    }

    #[test]
    fn ics_fragments_large_composition() {
        let mut button = make_button(0);
        button.commands = vec![[0u8; 12]; 6000];

        let composition = InteractiveComposition {
            width: 1920,
            height: 1080,
            frame_rate_id: 3,
            number: 0,
            popup: false,
            pages: vec![IgPage {
                id: 0,
                default_selected: 0,
                palette_id: 0,
                buttons: vec![button],
            }],
        };

        let segments = ics(&composition).unwrap_or_default();

        assert_eq!(segments.len(), 2);
        assert_eq!(segments.first().map(|segment| segment.len()), Some(65538));
        assert_eq!(segments.get(1).map(|segment| segment.len()), Some(6563));
        assert_eq!(
            segments
                .first()
                .and_then(|segment| segment.get(11))
                .copied(),
            Some(0x80)
        );
        assert_eq!(
            segments.get(1).and_then(|segment| segment.get(11)).copied(),
            Some(0x40)
        );
        assert_eq!(
            segments
                .first()
                .and_then(|segment| segment.get(12..15))
                .map(|value| value.to_vec()),
            Some(vec![0x01, 0x19, 0x8A])
        );
    }

    #[test]
    fn display_set_order_and_timestamps() {
        let segments = display_set(&make_composition(), &palette(), &[make_object(0)], 1000)
            .unwrap_or_default();

        assert_eq!(segments.len(), 4);

        assert_eq!(
            segments
                .first()
                .and_then(|segment| segment.data.first())
                .copied(),
            Some(0x18)
        );
        assert_eq!(
            segments.first().map(|segment| (segment.pts, segment.dts)),
            Some((1000, 1000))
        );

        assert_eq!(
            segments
                .get(1)
                .and_then(|segment| segment.data.first())
                .copied(),
            Some(0x14)
        );
        assert_eq!(
            segments.get(1).map(|segment| (segment.pts, segment.dts)),
            Some((1000, 999))
        );

        assert_eq!(
            segments
                .get(2)
                .and_then(|segment| segment.data.first())
                .copied(),
            Some(0x15)
        );
        assert_eq!(
            segments.get(2).map(|segment| (segment.pts, segment.dts)),
            Some((1000, 999))
        );

        assert_eq!(
            segments
                .get(3)
                .and_then(|segment| segment.data.first())
                .copied(),
            Some(0x80)
        );
        assert_eq!(
            segments.get(3).map(|segment| (segment.pts, segment.dts)),
            Some((1000, 1000))
        );
    }

    #[test]
    fn duplicate_button_id_is_rejected() {
        let composition = InteractiveComposition {
            width: 4,
            height: 4,
            frame_rate_id: 3,
            number: 0,
            popup: false,
            pages: vec![IgPage {
                id: 0,
                default_selected: 0,
                palette_id: 0,
                buttons: vec![make_button(0), make_button(0)],
            }],
        };

        assert!(ics(&composition).is_err());
    }

    #[test]
    fn missing_neighbour_is_rejected() {
        let mut button = make_button(0);
        button.upper = 1;

        let composition = InteractiveComposition {
            width: 4,
            height: 4,
            frame_rate_id: 3,
            number: 0,
            popup: false,
            pages: vec![IgPage {
                id: 0,
                default_selected: 0,
                palette_id: 0,
                buttons: vec![button],
            }],
        };

        assert!(ics(&composition).is_err());
    }

    #[test]
    fn too_many_pages_is_rejected() {
        let mut pages = Vec::new();
        for _ in 0..256 {
            pages.push(IgPage {
                id: 0,
                default_selected: 0,
                palette_id: 0,
                buttons: Vec::new(),
            });
        }

        let composition = InteractiveComposition {
            width: 4,
            height: 4,
            frame_rate_id: 3,
            number: 0,
            popup: false,
            pages,
        };

        assert!(ics(&composition).is_err());
    }

    #[test]
    fn too_many_buttons_is_rejected() {
        let mut buttons = Vec::new();
        for _ in 0..256 {
            buttons.push(make_button(0));
        }

        let composition = InteractiveComposition {
            width: 4,
            height: 4,
            frame_rate_id: 3,
            number: 0,
            popup: false,
            pages: vec![IgPage {
                id: 0,
                default_selected: 0,
                palette_id: 0,
                buttons,
            }],
        };

        assert!(ics(&composition).is_err());
    }

    #[test]
    fn duplicate_object_id_is_rejected() {
        let objects = vec![make_object(0), make_object(0)];
        assert!(display_set(&make_composition(), &palette(), &objects, 1000).is_err());
    }
}
